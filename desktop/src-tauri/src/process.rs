use std::sync::Arc;

use database::{GameDownloadStatus, borrow_db_checked, db::DATA_ROOT_DIR};
use process::{
    PROCESS_MANAGER,
    error::ProcessError,
    process_manager::{LaunchOption, ProcessHandlerOption, ProcessManager},
};
use log::info;
use serde::Serialize;
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

#[cfg(target_os = "linux")]
#[tauri::command]
pub fn run_winecfg(game_id: String) -> Result<(), ProcessError> {
    run_wine_tool(game_id, "winecfg", &[], &[])
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct WinetricksVerb {
    pub category: String,
    pub name: String,
    pub description: String,
}

// Verb availability doesn't depend on a specific game/prefix, so this just
// runs the system winetricks directly rather than going through umu-run.
#[cfg(target_os = "linux")]
#[tauri::command]
pub fn list_winetricks_verbs() -> Result<Vec<WinetricksVerb>, ProcessError> {
    let mut command = std::process::Command::new("winetricks");
    command.arg("list-all");
    sanitize_external_command(&mut command);

    let output = command.output()?;
    let stdout = String::from_utf8_lossy(&output.stdout);

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

    Ok(verbs)
}
