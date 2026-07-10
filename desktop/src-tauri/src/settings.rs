use std::{
    fs::create_dir_all,
    io::{Error, ErrorKind},
    path::{Path, PathBuf},
};

use database::{
    Settings, borrow_db_checked, borrow_db_mut_checked, db::DATA_ROOT_DIR, debug::SystemData,
};
use download_manager::error::DownloadManagerError;
use games::scan::scan_install_dirs;
use log::error;
use serde_json::Value;

// Will, in future, return disk/remaining size
// Just returns the directories that have been set up
#[tauri::command]
pub fn fetch_download_dir_stats() -> Vec<PathBuf> {
    let lock = borrow_db_checked();
    lock.applications.install_dirs.clone()
}

#[tauri::command]
pub fn delete_download_dir(index: usize) {
    let mut lock = borrow_db_mut_checked();
    lock.applications.install_dirs.remove(index);
}

#[tauri::command]
pub fn add_download_dir(new_dir: PathBuf) -> Result<(), DownloadManagerError<()>> {
    // Check the new directory is all good
    let new_dir_path = Path::new(&new_dir);
    if new_dir_path.exists() {
        let dir_contents = new_dir_path.read_dir()?;
        if dir_contents.count() != 0 {
            return Err(Error::new(
                ErrorKind::DirectoryNotEmpty,
                "Selected directory cannot contain any existing files",
            )
            .into());
        }
    } else {
        create_dir_all(new_dir_path)?;
    }

    // Add it to the dictionary
    let mut lock = borrow_db_mut_checked();
    if lock.applications.install_dirs.contains(&new_dir) {
        return Err(Error::new(
            ErrorKind::AlreadyExists,
            "Selected directory already exists in database",
        )
        .into());
    }
    lock.applications.install_dirs.push(new_dir);
    drop(lock);

    scan_install_dirs();

    Ok(())
}

#[tauri::command]
pub fn update_settings(new_settings: Value) {
    let mut db_lock = borrow_db_mut_checked();
    let mut current_settings =
        serde_json::to_value(db_lock.settings.clone()).expect("Failed to parse existing settings");
    let values = match new_settings.as_object() {
        Some(values) => values,
        None => {
            error!("Could not parse settings values into object");
            return;
        }
    };
    for (key, value) in values {
        current_settings[key] = value.clone();
    }
    let new_settings: Settings = match serde_json::from_value(current_settings) {
        Ok(settings) => settings,
        Err(e) => {
            error!("Could not parse settings with error {}", e);
            return;
        }
    };
    db_lock.settings = new_settings;
}
#[tauri::command]
pub fn fetch_settings() -> Settings {
    borrow_db_checked().settings.clone()
}
#[tauri::command]
pub fn fetch_system_data() -> SystemData {
    let db_handle = borrow_db_checked();
    SystemData::new(
        db_handle.auth.as_ref().unwrap().client_id.clone(),
        db_handle.base_url.clone(),
        DATA_ROOT_DIR.to_string_lossy().to_string(),
        std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string()),
    )
}

// gamescope sets this to point at its own compositor's Wayland socket
// (distinct from any outer WAYLAND_DISPLAY it's nested inside), so its
// presence is a reliable signal that we're running under it - used by the
// frontend's UI scaling (use-ui-scale.ts) to decide whether a fullscreen
// window's viewport size should be allowed to scale the UI down, not just
// up: gamescope renders at an internal resolution and then uniformly
// stretches that to the real display, so a lower internal resolution needs
// to be compensated for or it ends up too big once stretched back out. A
// plain (non-gamescope) fullscreen window at a genuinely low resolution has
// no such stretch to compensate for, so it shouldn't get smaller.
#[tauri::command]
pub fn is_gamescope() -> bool {
    std::env::var_os("GAMESCOPE_WAYLAND_DISPLAY").is_some()
}
