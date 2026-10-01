// Linux-only file

use std::path::PathBuf;

// Discovery lives in the client crate so installs (the games crate) can
// match Proton builds too.
pub use client::proton::{
    ProtonPath, discover_proton_paths, find_proton_by_name, read_proton_path,
};
use database::{borrow_db_checked, borrow_db_mut_checked};
use log::warn;
use serde::Serialize;

#[derive(Serialize)]
pub struct ProtonPaths {
    pub autodiscovered: Vec<ProtonPath>,
    pub custom: Vec<ProtonPath>,
    pub default: Option<String>,
}

#[tauri::command]
pub async fn fetch_proton_paths() -> Result<ProtonPaths, String> {
    let autodiscovered = discover_proton_paths();

    let db_lock = borrow_db_checked();

    let custom = db_lock
        .applications
        .additional_proton_paths
        .iter()
        .filter_map(|v| {
            read_proton_path(PathBuf::from(v))
                .inspect_err(|e| warn!("skipping unreadable custom proton path {v}: {e}"))
                .ok()
        })
        .flatten()
        .collect::<Vec<ProtonPath>>();

    let default = db_lock.applications.default_proton_path.clone();

    Ok(ProtonPaths {
        autodiscovered,
        custom,
        default,
    })
}

#[tauri::command]
pub fn add_proton_layer(path: String) -> Result<(), String> {
    let path = PathBuf::from(path);

    let proton_layer = read_proton_path(path)
        .map_err(|err| err.to_string())?
        .ok_or("Unable to detect Proton at selected path.".to_owned())?;

    let mut db = borrow_db_mut_checked();
    db.applications
        .additional_proton_paths
        .push(proton_layer.path);

    Ok(())
}

#[tauri::command]
pub async fn remove_proton_layer(index: usize) {
    let mut db = borrow_db_mut_checked();
    let deleted = db.applications.additional_proton_paths.try_remove(index);
    if let Some(deleted) = deleted
        && let Some(default_path) = &db.applications.default_proton_path
        && *default_path == deleted
    {
        db.applications.default_proton_path = None;
    }
}

#[tauri::command]
pub async fn set_default(path: String) -> Result<(), String> {
    let proton_paths = fetch_proton_paths().await?;

    let valid = proton_paths
        .autodiscovered
        .iter()
        .find(|v| v.path == path)
        .or(proton_paths.custom.iter().find(|v| v.path == path))
        .is_some();

    if !valid {
        return Err("Invalid default Proton path.".to_string());
    }

    let mut db_lock = borrow_db_mut_checked();
    db_lock.applications.default_proton_path = Some(path);

    Ok(())
}

// The installed build a server's recommended Proton name would be filled in
// with, for showing it before install and for resetting to it.
#[tauri::command]
pub fn match_proton_name(name: String) -> Option<ProtonPath> {
    find_proton_by_name(&borrow_db_checked(), &name)
}
