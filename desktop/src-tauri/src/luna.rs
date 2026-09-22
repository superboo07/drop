//! Tauri commands for the LunaTranslator integration's settings UI.
//!
//! The per-game toggles ride along with the rest of a game's user
//! configuration; these commands exist for the global setup: pointing Drop at
//! a LunaTranslator install and getting the in-prefix bridge out of it.

use serde::Serialize;

#[cfg(target_os = "linux")]
use database::borrow_db_checked;
use process::error::ProcessError;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LunaStatus {
    /// Whether this platform can run the integration at all. The bridge only
    /// exists to get text out of Wine, so it's Linux-only by nature.
    pub supported: bool,
    /// A LunaTranslator path has been set and still exists.
    pub configured: bool,
    /// The bridge has been extracted (or overridden) and is ready to use.
    pub bridge_ready: bool,
    pub bridge_path: Option<String>,
}

#[tauri::command]
pub fn fetch_luna_status() -> LunaStatus {
    #[cfg(not(target_os = "linux"))]
    return LunaStatus {
        supported: false,
        configured: false,
        bridge_ready: false,
        bridge_path: None,
    };

    #[cfg(target_os = "linux")]
    {
        let db = borrow_db_checked();

        let configured = db
            .settings
            .luna_translator_path
            .as_ref()
            .is_some_and(|path| !path.is_empty() && std::path::Path::new(path).exists());

        let bridge = db
            .settings
            .luna_bridge_path
            .as_ref()
            .filter(|path| !path.is_empty())
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| process::luna::bridge_cache_dir().join("LunaCompanion.exe"));

        LunaStatus {
            supported: true,
            configured,
            bridge_ready: bridge.is_file(),
            bridge_path: bridge.is_file().then(|| bridge.to_string_lossy().to_string()),
        }
    }
}

/// Pulls the bridge out of the configured LunaTranslator install. Happens
/// automatically on the first Luna-enabled launch too; this is here so the
/// settings page can do it up front and report a failure while the user is
/// still looking at the setting that caused it.
#[tauri::command]
pub fn extract_luna_bridge() -> Result<(), ProcessError> {
    #[cfg(not(target_os = "linux"))]
    return Err(ProcessError::InvalidPlatform);

    #[cfg(target_os = "linux")]
    {
        let db = borrow_db_checked();
        process::luna::extract_bridge(&db)
    }
}
