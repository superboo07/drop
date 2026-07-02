use std::sync::Arc;

use process::{
    PROCESS_MANAGER,
    error::ProcessError,
    process_manager::{LaunchOption, ProcessHandlerOption, ProcessManager},
};
use log::info;
use serde::Serialize;
use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;

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

    // On Linux, the AppImage bundles its own (older) xdg-utils, and the
    // AppRun-set PATH puts that bundled xdg-open ahead of the system one. The
    // bundled xdg-open doesn't know about Plasma 6's naming and calls
    // "kde-open6", which doesn't exist there (only "kde-open"/"kde-open5")
    // — it fails silently and xdg-open still exits 0. The AppRun-set
    // LD_LIBRARY_PATH (for our bundled webkit2gtk/GTK libs) would also break
    // any native opener helper it did manage to launch. So we bypass the
    // bundled xdg-open by invoking the system's copy directly with a clean
    // LD_LIBRARY_PATH, falling back to the opener plugin if that binary isn't
    // there (e.g. a distro without xdg-utils at that path).
    #[cfg(target_os = "linux")]
    let result = match std::process::Command::new("/usr/bin/xdg-open")
        .arg(&dir)
        .env_remove("LD_LIBRARY_PATH")
        .spawn()
    {
        Ok(_) => Ok(()),
        Err(_) => app_handle
            .opener()
            .open_path(dir.display().to_string(), None::<&str>)
            .map_err(|v| ProcessError::OpenerError(Arc::new(v))),
    };

    #[cfg(not(target_os = "linux"))]
    let result = app_handle
        .opener()
        .open_path(dir.display().to_string(), None::<&str>)
        .map_err(|v| ProcessError::OpenerError(Arc::new(v)));

    match &result {
        Ok(()) => info!("open_process_logs succeeded"),
        Err(e) => info!("open_process_logs failed: {e}"),
    }
    result
}
