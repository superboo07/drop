//! Self-update is only implemented for the Linux AppImage build so far; on
//! other platforms the commands exist but report that updates aren't
//! available. See client_update.rs.

use serde::Serialize;
use tauri::AppHandle;

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ClientUpdateStatus {
    app_version: String,
    arch: &'static str,
    unsupported_reason: Option<String>,
    checking: bool,
}

#[tauri::command]
pub fn fetch_client_update_status(app: AppHandle) -> ClientUpdateStatus {
    ClientUpdateStatus {
        app_version: app.package_info().version.to_string(),
        arch: std::env::consts::ARCH,
        unsupported_reason: Some(
            "Drop can only update itself on Linux (AppImage) for now. Update this copy the same way you installed it."
                .to_string(),
        ),
        checking: false,
    }
}

#[tauri::command]
pub async fn check_client_update(app: AppHandle) -> Result<ClientUpdateStatus, String> {
    Ok(fetch_client_update_status(app))
}

#[tauri::command]
pub async fn install_client_update() -> Result<(), String> {
    Err("Updates aren't available on this platform.".to_string())
}

#[tauri::command]
pub fn cancel_client_update() {}

#[tauri::command]
pub async fn restart_after_client_update() -> Result<(), String> {
    Err("Updates aren't available on this platform.".to_string())
}
