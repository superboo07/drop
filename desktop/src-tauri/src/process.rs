use std::sync::Arc;

use database::{GameDownloadStatus, borrow_db_checked, db::DATA_ROOT_DIR};
use process::{
    PROCESS_MANAGER,
    error::ProcessError,
    process_manager::{LaunchOption, ProcessHandlerOption, ProcessManager},
};
use log::info;
use remote::{
    error::RemoteAccessError,
    requests::{generate_url, make_authenticated_get},
};
use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;
use utils::external_open::open_externally;

#[cfg(target_os = "linux")]
use utils::external_open::sanitize_external_command;

#[tauri::command]
pub fn get_launch_options(id: String) -> Result<Vec<LaunchOption>, ProcessError> {
    let launch_options = ProcessManager::get_launch_options(id)?;

    Ok(launch_options)
}

#[tauri::command]
pub fn get_process_handlers(id: String) -> Result<Vec<ProcessHandlerOption>, ProcessError> {
    PROCESS_MANAGER.lock().get_process_handlers(id)
}

#[derive(Deserialize)]
struct RemotePlaytimeResponse {
    seconds: u64,
}

// Merges the server's confirmed total with any locally-recorded sessions
// that haven't synced yet, so the displayed number is accurate even before
// a sync completes. Falls back to local-only on any network failure -
// still an accurate figure for this device, just not aware of other
// devices' playtime until back online.
#[tauri::command]
pub async fn fetch_game_playtime(game_id: String) -> Result<u64, RemoteAccessError> {
    let local: u64 = {
        let db = borrow_db_checked();
        db.applications
            .pending_playtime_sessions
            .iter()
            .filter(|s| s.game_id == game_id)
            .map(|s| s.seconds)
            .sum()
    };

    let url = generate_url(&["/api/v1/client/playtime", &game_id], &[])?;
    let server = match make_authenticated_get(url).await {
        Ok(response) if response.status().is_success() => response
            .json::<RemotePlaytimeResponse>()
            .await
            .map(|p| p.seconds)
            .unwrap_or(0),
        _ => 0,
    };

    Ok(server + local)
}

#[derive(Serialize)]
#[serde(tag = "result", content = "data")]
pub enum LaunchResult {
    Success,
    InstallRequired(String, String),
}

#[tauri::command]
pub fn launch_game(id: String, index: usize) -> Result<LaunchResult, ProcessError> {
    let result = {
        let mut process_manager_lock = PROCESS_MANAGER.lock();

        process_manager_lock.launch_process(id, index)
    };

    if let Err(err) = &result
        && let ProcessError::RequiredDependency(game_id, version_id) = err
    {
        return Ok(LaunchResult::InstallRequired(
            game_id.to_string(),
            version_id.to_string(),
        ));
    }

    result?;

    Ok(LaunchResult::Success)
}

#[tauri::command]
pub fn kill_game(game_id: String) -> Result<(), ProcessError> {
    Ok(PROCESS_MANAGER.lock().kill_game(game_id)?)
}

#[tauri::command]
pub fn open_process_logs(game_id: String, app_handle: AppHandle) -> Result<(), ProcessError> {
    let process_manager_lock = PROCESS_MANAGER.lock();

    let dir = process_manager_lock.get_log_dir(game_id);
    drop(process_manager_lock);
    info!("opening log directory: {}", dir.display());

    let result = open_externally(&dir.display().to_string(), || {
        app_handle
            .opener()
            .open_path(dir.display().to_string(), None::<&str>)
            .map_err(|v| ProcessError::OpenerError(Arc::new(v)))
    });

    match &result {
        Ok(()) => info!("open_process_logs succeeded"),
        Err(e) => info!("open_process_logs failed: {e}"),
    }
    result
}

#[tauri::command]
pub fn open_game_install_dir(game_id: String, app_handle: AppHandle) -> Result<(), ProcessError> {
    let install_dir = {
        let db_lock = borrow_db_checked();
        match db_lock.applications.game_statuses.get(&game_id) {
            Some(GameDownloadStatus::Installed { install_dir, .. }) => install_dir.clone(),
            _ => return Err(ProcessError::NotInstalled),
        }
    };

    open_externally(&install_dir, || {
        app_handle
            .opener()
            .open_path(install_dir.clone(), None::<&str>)
            .map_err(|v| ProcessError::OpenerError(Arc::new(v)))
    })
}

#[cfg(target_os = "linux")]
#[tauri::command]
pub fn open_game_wine_prefix(game_id: String, app_handle: AppHandle) -> Result<(), ProcessError> {
    let pfx_dir = DATA_ROOT_DIR.join("pfx").join(&game_id);
    if !pfx_dir.exists() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "This game hasn't been launched through Proton yet, so it has no prefix",
        )
        .into());
    }

    open_externally(&pfx_dir.display().to_string(), || {
        app_handle
            .opener()
            .open_path(pfx_dir.display().to_string(), None::<&str>)
            .map_err(|v| ProcessError::OpenerError(Arc::new(v)))
    })
}

// Runs a Wine helper tool (winetricks, winecfg, ...) inside the same
// Proton/Wine environment (PROTONPATH, WINEPREFIX, GAMEID) a game itself
// launches with, via umu-run, mirroring UMUCompatLauncher's env
// construction. Not tied into ProcessManager/process tracking since these
// are one-off diagnostic tools, not the game itself.
#[cfg(target_os = "linux")]
fn run_wine_tool(
    game_id: String,
    tool: &str,
    extra_args: &[&str],
    extra_env: &[(&str, &str)],
) -> Result<(), ProcessError> {
    let db_lock = borrow_db_checked();

    let meta = db_lock
        .applications
        .installed_game_version
        .get(&game_id)
        .cloned()
        .ok_or(ProcessError::NotInstalled)?;

    let version_id = match db_lock.applications.game_statuses.get(&game_id) {
        Some(GameDownloadStatus::Installed { version_id, .. }) => version_id.clone(),
        _ => return Err(ProcessError::NotInstalled),
    };

    let game_version = db_lock
        .applications
        .game_versions
        .get(&version_id)
        .ok_or(ProcessError::InvalidVersion)?;

    let proton_path = game_version
        .user_configuration
        .override_proton_path
        .clone()
        .or_else(|| db_lock.applications.default_proton_path.clone())
        .ok_or(ProcessError::NoCompat)?;

    let umu_id_override = game_version
        .launches
        .iter()
        .find(|v| v.platform == meta.target_platform)
        .and_then(|v| v.umu_id_override.as_ref())
        .map_or("", |v| v);
    let umu_game_id = if umu_id_override.is_empty() {
        game_version.version_id.clone()
    } else {
        umu_id_override.to_string()
    };

    let compat_env = process::process_handlers::compat_env_vars(&game_version.user_configuration);

    drop(db_lock);

    let pfx_dir = DATA_ROOT_DIR.join("pfx").join(&game_id);
    if !pfx_dir.exists() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "This game hasn't been launched through Proton yet, so it has no prefix",
        )
        .into());
    }

    let umu_launcher = ::client::compat::UMU_LAUNCHER_EXECUTABLE
        .as_ref()
        .ok_or(ProcessError::NoCompat)?;

    // Proton's own winetricks handler runs this with check=False and
    // exits quietly on failure, so without capturing stdout/stderr
    // ourselves a failure here is completely invisible.
    let log_dir = DATA_ROOT_DIR.join("logs").join(&game_id);
    std::fs::create_dir_all(&log_dir)?;
    let log_path = log_dir.join(format!(
        "{tool}-{}.log",
        chrono::offset::Local::now().timestamp()
    ));
    let log_file = std::fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .create(true)
        .open(&log_path)?;
    info!("running {tool}, logging to {}", log_path.display());

    let mut command = std::process::Command::new(umu_launcher);
    command
        .arg(tool)
        .args(extra_args)
        .envs(extra_env.iter().copied())
        .envs(compat_env)
        .env("GAMEID", umu_game_id)
        .env("PROTONPATH", proton_path)
        .env("WINEPREFIX", pfx_dir)
        .stdout(log_file.try_clone()?)
        .stderr(log_file);
    sanitize_external_command(&mut command);

    command.spawn()?;
    Ok(())
}

#[cfg(target_os = "linux")]
#[tauri::command]
pub fn install_winetricks_verb(game_id: String, verb: String) -> Result<(), ProcessError> {
    // -q (unattended) skips winetricks' own confirmation prompts entirely,
    // so this doesn't depend on zenity/kdialog being detected/working at
    // all -- we have our own verb picker in the frontend instead.
    run_wine_tool(game_id, "winetricks", &["-q", &verb], &[])
}

// Installs several verbs in one winetricks run, for the server's recommended
// components.
#[cfg(target_os = "linux")]
#[tauri::command]
pub fn install_winetricks_verbs(game_id: String, verbs: Vec<String>) -> Result<(), ProcessError> {
    let mut args = vec!["-q"];
    args.extend(verbs.iter().map(String::as_str));
    run_wine_tool(game_id, "winetricks", &args, &[])
}

// Verbs winetricks has recorded as installed in this game's prefix. It logs
// each one to winetricks.log in the Wine prefix, which Proton keeps in a pfx
// folder inside WINEPREFIX, so both places are checked. Empty when the game
// hasn't been launched yet.
#[cfg(target_os = "linux")]
#[tauri::command]
pub fn fetch_installed_winetricks(game_id: String) -> Vec<String> {
    let pfx_dir = DATA_ROOT_DIR.join("pfx").join(&game_id);
    [
        pfx_dir.join("winetricks.log"),
        pfx_dir.join("pfx").join("winetricks.log"),
    ]
    .iter()
    .filter_map(|path| std::fs::read_to_string(path).ok())
    .flat_map(|log| {
        log.lines()
            .map(|v| v.trim().to_owned())
            .filter(|v| !v.is_empty())
            .collect::<Vec<_>>()
    })
    .collect()
}

#[cfg(target_os = "linux")]
#[tauri::command]
pub fn run_winecfg(game_id: String) -> Result<(), ProcessError> {
    run_wine_tool(game_id, "winecfg", &[], &[])
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct WinetricksVerb {
    pub category: String,
    pub name: String,
    pub description: String,
}

// `winetricks list-all` can take minutes on some systems and the verb list
// doesn't depend on a specific game/prefix, so it's fetched at most once
// per session (prefetched at startup, see prefetch_winetricks_verbs) and
// also kept on disk across sessions, keyed on the winetricks script itself.
#[cfg(target_os = "linux")]
static WINETRICKS_VERBS: std::sync::nonpoison::Mutex<Option<Arc<Vec<WinetricksVerb>>>> =
    std::sync::nonpoison::Mutex::new(None);

#[cfg(target_os = "linux")]
#[derive(Serialize, Deserialize, PartialEq)]
struct WinetricksFingerprint {
    size: u64,
    modified_nanos: u128,
}

#[cfg(target_os = "linux")]
#[derive(Serialize, Deserialize)]
struct WinetricksVerbsDiskCache {
    fingerprint: WinetricksFingerprint,
    verbs: Vec<WinetricksVerb>,
}

#[cfg(target_os = "linux")]
fn winetricks_verbs_cache_path() -> std::path::PathBuf {
    DATA_ROOT_DIR.join("winetricks-verbs.json")
}

// Size + mtime rather than path: inside the AppImage the vendored copy's
// path changes with every mount point, but its metadata doesn't.
#[cfg(target_os = "linux")]
fn winetricks_fingerprint(command: &std::process::Command) -> Option<WinetricksFingerprint> {
    let path_var = command
        .get_envs()
        .find(|(key, _)| *key == "PATH")
        .and_then(|(_, value)| value.map(|v| v.to_os_string()))
        .or_else(|| std::env::var_os("PATH"))?;
    let metadata = std::env::split_paths(&path_var)
        .map(|dir| dir.join("winetricks"))
        .find_map(|path| std::fs::metadata(path).ok().filter(|m| m.is_file()))?;
    Some(WinetricksFingerprint {
        size: metadata.len(),
        modified_nanos: metadata
            .modified()
            .ok()?
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .as_nanos(),
    })
}

#[cfg(target_os = "linux")]
fn parse_winetricks_list_all(stdout: &str) -> Vec<WinetricksVerb> {
    let mut verbs = Vec::new();
    let mut current_category = String::new();
    for line in stdout.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(category) = trimmed
            .strip_prefix("=====")
            .and_then(|v| v.strip_suffix("====="))
        {
            current_category = category.trim().to_string();
            continue;
        }
        let Some(split_at) = line.find(char::is_whitespace) else {
            continue;
        };
        let name = line[..split_at].trim();
        if name.is_empty() {
            continue;
        }
        verbs.push(WinetricksVerb {
            category: current_category.clone(),
            name: name.to_string(),
            description: line[split_at..].trim().to_string(),
        });
    }
    verbs
}

// Blocking. Holding the lock for the whole run means concurrent callers
// (the startup prefetch and an opened Proton tab) share one winetricks run
// instead of starting their own. Failures aren't cached, so the next call
// retries.
#[cfg(target_os = "linux")]
fn winetricks_verbs() -> Result<Arc<Vec<WinetricksVerb>>, ProcessError> {
    let mut cached = WINETRICKS_VERBS.lock();
    if let Some(verbs) = cached.as_ref() {
        return Ok(verbs.clone());
    }

    // Verb availability doesn't depend on a specific game/prefix, so this
    // just runs the system winetricks directly rather than going through
    // umu-run.
    let mut command = std::process::Command::new("winetricks");
    command.arg("list-all");
    sanitize_external_command(&mut command);

    let fingerprint = winetricks_fingerprint(&command);
    let cache_path = winetricks_verbs_cache_path();
    if let Some(fingerprint) = &fingerprint
        && let Ok(data) = std::fs::read(&cache_path)
        && let Ok(disk) = serde_json::from_slice::<WinetricksVerbsDiskCache>(&data)
        && disk.fingerprint == *fingerprint
    {
        info!(
            "loaded {} winetricks verbs from {}",
            disk.verbs.len(),
            cache_path.display()
        );
        let verbs = Arc::new(disk.verbs);
        *cached = Some(verbs.clone());
        return Ok(verbs);
    }

    let output = command.output()?;
    let verbs = parse_winetricks_list_all(&String::from_utf8_lossy(&output.stdout));
    if verbs.is_empty() {
        return Err(std::io::Error::other(format!(
            "winetricks list-all returned no verbs ({}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ))
        .into());
    }

    if let Some(fingerprint) = fingerprint {
        let disk = serde_json::json!({ "fingerprint": fingerprint, "verbs": &verbs });
        if let Err(e) = std::fs::write(&cache_path, disk.to_string()) {
            log::warn!("failed to write {}: {e}", cache_path.display());
        }
    }

    let verbs = Arc::new(verbs);
    *cached = Some(verbs.clone());
    Ok(verbs)
}

// Warms the cache in the background so the Proton tab usually has the list
// ready by the time it's opened.
#[cfg(target_os = "linux")]
pub fn prefetch_winetricks_verbs() {
    tauri::async_runtime::spawn_blocking(|| {
        if let Err(e) = winetricks_verbs() {
            log::warn!("failed to prefetch winetricks verbs: {e}");
        }
    });
}

// Async so it runs off the main thread: a sync command would freeze the
// whole window for as long as winetricks takes.
#[cfg(target_os = "linux")]
#[tauri::command]
pub async fn list_winetricks_verbs() -> Result<Vec<WinetricksVerb>, ProcessError> {
    let verbs = tauri::async_runtime::spawn_blocking(winetricks_verbs)
        .await
        .map_err(|e| std::io::Error::other(e.to_string()))??;
    Ok(verbs.as_ref().clone())
}

// Locales the host actually has generated (`locale -a`), so the per-game
// locale picker can warn when a native game's locale won't take effect.
// Proton games don't need this - umu-run's container generates its own.
// Normalised to e.g. "ja_JP.UTF-8" since glibc lists "ja_JP.utf8".
#[tauri::command]
pub fn list_system_locales() -> Vec<String> {
    #[cfg(target_os = "windows")]
    {
        Vec::new()
    }
    #[cfg(not(target_os = "windows"))]
    {
        let mut command = std::process::Command::new("locale");
        command.arg("-a");
        #[cfg(target_os = "linux")]
        sanitize_external_command(&mut command);

        let Ok(output) = command.output() else {
            return Vec::new();
        };
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(|v| match v.split_once('.') {
                Some((name, codeset))
                    if codeset.eq_ignore_ascii_case("utf8")
                        || codeset.eq_ignore_ascii_case("utf-8") =>
                {
                    format!("{name}.UTF-8")
                }
                _ => v.to_owned(),
            })
            .collect()
    }
}
