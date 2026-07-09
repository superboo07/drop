use bitcode::{Decode, Encode};
use database::{
    ApplicationTransientStatus, Database, DownloadableMetadata, GameDownloadStatus, GameVersion,
    borrow_db_checked, borrow_db_mut_checked,
    models::data::{InstalledGameType, UserConfiguration},
};
use log::{debug, error, warn};
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use remote::{
    auth::generate_authorization_header, error::RemoteAccessError, requests::generate_url,
    utils::DROP_CLIENT_ASYNC,
};
use serde::{Deserialize, Serialize};
use std::fs;
use std::fs::{read_dir, remove_dir, remove_dir_all, remove_file};
use std::path::Path;
use std::thread::spawn;
use tauri::AppHandle;
use utils::app_emit;

use crate::downloads::drop_data::{DROPDATA_PATH, DropData, InstalledFileRecord, hash_file};
use crate::state::{GameStatusManager, GameStatusWithTransient};

#[derive(Serialize, Deserialize, Debug)]
pub struct FetchGameStruct {
    pub game: Game,
    pub status: GameStatusWithTransient,
    pub version: Option<GameVersion>,
}

impl FetchGameStruct {
    pub fn new(game: Game, status: GameStatusWithTransient, version: Option<GameVersion>) -> Self {
        Self {
            game,
            status,
            version,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, Encode, Decode)]
#[serde(rename_all = "camelCase")]
pub struct Game {
    pub id: String,
    #[serde(rename = "type")]
    pub game_type: String,
    pub m_name: String,
    pub m_short_description: String,
    pub m_description: String,
    // mDevelopers
    // mPublishers
    pub m_icon_object_id: String,
    pub m_banner_object_id: String,
    pub m_cover_object_id: String,
    pub m_image_library_object_ids: Vec<String>,
    pub m_image_carousel_object_ids: Vec<String>,
    pub library_path: String,
}
impl Game {
    pub fn id(&self) -> &String {
        &self.id
    }
}
#[derive(serde::Serialize, Clone)]
pub struct GameUpdateEvent {
    pub game_id: String,
    pub status: (
        Option<GameDownloadStatus>,
        Option<ApplicationTransientStatus>,
    ),
    pub version: Option<GameVersion>,
}

/**
 * Called by:
 *  - on_cancel, when cancelled, for obvious reasons
 *  - when downloading, so if drop unexpectedly quits, we can resume the download. hidden by the "Downloading..." transient state, though
 *  - when scanning, to import the game
 */
pub fn set_partially_installed(
    meta: &DownloadableMetadata,
    install_dir: String,
    app_handle: Option<&AppHandle>,
    configuration: UserConfiguration,
) {
    set_partially_installed_db(&mut borrow_db_mut_checked(), meta, install_dir, app_handle, configuration);
}

pub fn set_partially_installed_db(
    db_lock: &mut Database,
    meta: &DownloadableMetadata,
    install_dir: String,
    app_handle: Option<&AppHandle>,
    configuration: UserConfiguration,
) {
    db_lock.applications.transient_statuses.remove(meta);
    db_lock.applications.game_statuses.insert(
        meta.id.clone(),
        GameDownloadStatus::Installed {
            install_type: InstalledGameType::PartiallyInstalled { configuration },
            version_id: meta.version.clone(),
            install_dir,
            update_available: false,
        },
    );
    db_lock
        .applications
        .installed_game_version
        .insert(meta.id.clone(), meta.clone());

    if let Some(app_handle) = app_handle {
        push_game_update(
            app_handle,
            &meta.id,
            None,
            GameStatusManager::fetch_state(&meta.id, db_lock),
        );
    }
}

pub fn uninstall_game_logic(meta: DownloadableMetadata, app_handle: &AppHandle) {
    debug!("triggered uninstall for agent");
    let mut db_handle = borrow_db_mut_checked();
    db_handle
        .applications
        .transient_statuses
        .insert(meta.clone(), ApplicationTransientStatus::Uninstalling {});

    push_game_update(
        app_handle,
        &meta.id,
        None,
        GameStatusManager::fetch_state(&meta.id, &db_handle),
    );

    let previous_state = db_handle.applications.game_statuses.get(&meta.id).cloned();

    let previous_state = if let Some(state) = previous_state {
        state
    } else {
        warn!("uninstall job doesn't have previous state, failing silently");
        return;
    };

    if let Some((_, install_dir, verify)) = match previous_state {
        GameDownloadStatus::Installed {
            install_type,
            version_id: version_name,
            install_dir,
            update_available: _,
        } => {
            // Only a completed install has trustworthy hashes to verify
            // against - a cancelled/partial install's server_hash was
            // recorded for the *target* file list before any bytes were
            // necessarily written, so an incomplete file would look
            // "modified" and get wrongly preserved. For those, delete
            // unconditionally, same as before this verification existed.
            let verify = matches!(
                install_type,
                InstalledGameType::Installed | InstalledGameType::SetupRequired
            );
            Some((version_name, install_dir, verify))
        }
        _ => None,
    } {
        db_handle
            .applications
            .transient_statuses
            .insert(meta.clone(), ApplicationTransientStatus::Uninstalling {});

        drop(db_handle);

        let app_handle = app_handle.clone();
        spawn(move || {
            uninstall_files(&install_dir, verify);
            let mut db_handle = borrow_db_mut_checked();
            db_handle.applications.transient_statuses.remove(&meta);
            db_handle
                .applications
                .installed_game_version
                .remove(&meta.id);
            db_handle
                .applications
                .game_statuses
                .insert(meta.id.clone(), GameDownloadStatus::Remote {});
            let _ = db_handle.applications.transient_statuses.remove(&meta);

            push_game_update(
                &app_handle,
                &meta.id,
                None,
                GameStatusManager::fetch_state(&meta.id, &db_handle),
            );

            debug!("uninstalled game id {}", meta.id);
            app_emit!(&app_handle, "update_library", ());
        });
    } else {
        warn!("invalid previous state for uninstall, failing silently.");
    }
}

// Removes only what Drop actually put in `install_dir`, leaving anything the
// user placed there themselves (mods, extra saves, unrelated files) alone -
// and, when `verify` is true, also leaving behind any tracked file whose
// content no longer matches what was installed (e.g. a game that wrote save
// data or self-patched a file into its own install folder).
//
// We know exactly which files were installed from `.dropdata`'s
// `installed_files` (populated during download/update, see `drop_data.rs`).
// If that manifest can't be read or is empty - e.g. a game installed before
// this tracking existed, or one imported by pointing Drop at an existing
// folder - we have no way to tell "ours" from "not ours" apart, so we fall
// back to the old behavior of removing the whole directory rather than
// silently leaving orphaned game files behind.
fn uninstall_files(install_dir: &str, verify: bool) {
    let install_dir = Path::new(install_dir);

    let installed_files = match DropData::read(install_dir) {
        Ok(drop_data) => drop_data.get_installed_files(),
        Err(e) => {
            debug!("no readable .dropdata in {}: {e}", install_dir.display());
            Default::default()
        }
    };

    if installed_files.is_empty() {
        warn!(
            "no install manifest for {}, removing entire directory",
            install_dir.display()
        );
        if let Err(e) = remove_dir_all(install_dir) {
            error!("{e}");
        }
        return;
    }

    let decisions: Vec<(String, bool)> = installed_files
        .into_par_iter()
        .map(|(relative_path, record)| {
            let keep = verify && should_keep(install_dir, &relative_path, &record);
            (relative_path, keep)
        })
        .collect();

    for (relative_path, keep) in decisions {
        let file_path = install_dir.join(&relative_path);
        if keep {
            warn!(
                "{} was modified after install, keeping it",
                file_path.display()
            );
            continue;
        }
        if let Err(e) = remove_file(&file_path)
            && e.kind() != std::io::ErrorKind::NotFound
        {
            warn!(
                "failed to remove installed file {}: {e}",
                file_path.display()
            );
        }
    }
    let _ = remove_file(install_dir.join(DROPDATA_PATH));

    remove_empty_dirs(install_dir);
    // Only actually disappears if it's now empty, i.e. nothing foreign (or
    // preserved-as-modified) was left behind. Fails silently otherwise.
    let _ = remove_dir(install_dir);
}

// Whether a tracked file has changed since install and should be preserved
// rather than deleted. Never dereferences symlinks to hash them - a
// game-controlled (or malicious) symlink could point at an arbitrary or
// unbounded target outside the install dir - so symlinks are always safe to
// delete, matching pre-verification behavior for them.
fn should_keep(install_dir: &Path, relative_path: &str, record: &InstalledFileRecord) -> bool {
    let file_path = install_dir.join(relative_path);

    let Ok(metadata) = fs::symlink_metadata(&file_path) else {
        return false; // already gone, nothing to keep
    };
    if !metadata.is_file() {
        return false;
    }

    let Ok(current_hash) = hash_file(&file_path) else {
        return false;
    };

    match record.server_hash.as_deref().or(record.client_hash.as_deref()) {
        Some(known_good) => current_hash != known_good,
        // No baseline recorded at all (e.g. installed before hash tracking
        // existed) - preserve the old, unverified delete behavior.
        None => false,
    }
}

// Recursively removes directories left empty after `uninstall_files` deletes
// tracked files, without touching directories that still hold other content.
fn remove_empty_dirs(dir: &Path) {
    let Ok(entries) = read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            remove_empty_dirs(&path);
            let _ = remove_dir(&path);
        }
    }
}

pub fn get_current_meta(game_id: &String) -> Option<DownloadableMetadata> {
    borrow_db_checked()
        .applications
        .installed_game_version
        .get(game_id)
        .cloned()
}

pub async fn on_game_complete(
    meta: &DownloadableMetadata,
    configuration: UserConfiguration,
    install_dir: String,
    app_handle: &AppHandle,
) -> Result<(), RemoteAccessError> {
    // Fetch game version information from remote
    let response = generate_url(
        &["/api/v1/client/game", &meta.id, "version", &meta.version],
        &[],
    )?;
    let response = DROP_CLIENT_ASYNC
        .get(response)
        .header("Authorization", generate_authorization_header())
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(RemoteAccessError::InvalidResponse(response.json().await?));
    }

    let mut game_version: GameVersion = response.json().await?;
    game_version.user_configuration = configuration;

    let mut handle = borrow_db_mut_checked();
    handle
        .applications
        .game_versions
        .insert(meta.version.clone(), game_version.clone());
    handle
        .applications
        .installed_game_version
        .insert(meta.id.clone(), meta.clone());

    drop(handle);

    let setup_configuration = game_version
        .setups
        .iter()
        .find(|v| v.platform == meta.target_platform);

    let status = GameDownloadStatus::Installed {
        version_id: meta.version.clone(),
        install_dir,
        install_type: if setup_configuration.is_none() {
            InstalledGameType::Installed
        } else {
            InstalledGameType::SetupRequired
        },
        update_available: false,
    };

    let mut db_handle = borrow_db_mut_checked();
    db_handle
        .applications
        .game_statuses
        .insert(meta.id.clone(), status.clone());
    db_handle.applications.transient_statuses.remove(meta);
    drop(db_handle);
    app_emit!(
        app_handle,
        &format!("update_game/{}", meta.id),
        GameUpdateEvent {
            game_id: meta.id.clone(),
            status: (Some(status), None),
            version: Some(game_version),
        }
    );

    app_emit!(app_handle, "update_library", ());

    Ok(())
}

pub fn push_game_update(
    app_handle: &AppHandle,
    game_id: &String,
    version: Option<GameVersion>,
    status: GameStatusWithTransient,
) {
    if let Some(GameDownloadStatus::Installed {
        install_type: InstalledGameType::Installed | InstalledGameType::SetupRequired,
        ..
    }) = &status.0
        && version.is_none()
    {
        panic!("pushed game for installed game that doesn't have version information");
    }

    app_emit!(
        app_handle,
        &format!("update_game/{game_id}"),
        GameUpdateEvent {
            game_id: game_id.clone(),
            status,
            version,
        }
    );
}