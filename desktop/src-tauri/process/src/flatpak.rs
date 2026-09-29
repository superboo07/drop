//! Running a user's own Flatpak-installed emulator (bring-your-own-emulator
//! mode). Everything goes through the host's `flatpak` CLI.

use std::{path::PathBuf, process::Command};

use serde::Serialize;
use utils::external_open::sanitize_external_command;

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct FlatpakApp {
    pub id: String,
    pub name: String,
}

pub fn find_flatpak() -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths)
        .map(|dir| dir.join("flatpak"))
        .find(|candidate| candidate.is_file())
}

fn flatpak_command(flatpak: &PathBuf) -> Command {
    let mut command = Command::new(flatpak);
    sanitize_external_command(&mut command);
    command
}

/// Installed Flatpak applications (user and system), sorted by name. Empty
/// if Flatpak isn't installed.
pub fn list_apps() -> Vec<FlatpakApp> {
    let Some(flatpak) = find_flatpak() else {
        return Vec::new();
    };
    let Ok(output) = flatpak_command(&flatpak)
        .args(["list", "--app", "--columns=application,name"])
        .output()
    else {
        return Vec::new();
    };

    let mut apps: Vec<FlatpakApp> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let (id, name) = line.split_once('\t').unwrap_or((line, line));
            let id = id.trim();
            (!id.is_empty()).then(|| FlatpakApp {
                id: id.to_owned(),
                name: name.trim().to_owned(),
            })
        })
        .collect();
    // The same app can be installed both per-user and system-wide.
    apps.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then(a.id.cmp(&b.id)));
    apps.dedup_by(|a, b| a.id == b.id);
    apps
}

pub fn is_installed(flatpak: &PathBuf, app_id: &str) -> bool {
    flatpak_command(flatpak)
        .args(["info", "--show-ref", app_id])
        .output()
        .is_ok_and(|output| output.status.success())
}
