//! Running a user's own Flatpak-installed emulator (bring-your-own-emulator
//! mode). Everything goes through the host's `flatpak` CLI.

use std::{path::PathBuf, process::Command};

use serde::Serialize;
use utils::external_open::sanitize_external_command;

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FlatpakApp {
    pub id: String,
    pub name: String,
    // Every installed branch (e.g. `stable`, `beta`), sorted. Usually one.
    pub branches: Vec<String>,
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
        .args(["list", "--app", "--columns=application,name,branch"])
        .output()
    else {
        return Vec::new();
    };
    parse_list(&String::from_utf8_lossy(&output.stdout))
}

fn parse_list(output: &str) -> Vec<FlatpakApp> {
    let mut apps: Vec<FlatpakApp> = Vec::new();
    for line in output.lines() {
        let mut columns = line.split('\t').map(str::trim);
        let id = columns.next().unwrap_or_default();
        if id.is_empty() {
            continue;
        }
        let name = columns.next().filter(|name| !name.is_empty()).unwrap_or(id);
        let branch = columns.next().filter(|branch| !branch.is_empty());

        // One line per installed branch, and the same branch can be
        // installed both per-user and system-wide.
        let app = match apps.iter_mut().find(|app| app.id == id) {
            Some(app) => app,
            None => {
                apps.push(FlatpakApp {
                    id: id.to_owned(),
                    name: name.to_owned(),
                    branches: Vec::new(),
                });
                apps.last_mut().unwrap()
            }
        };
        if let Some(branch) = branch
            && !app.branches.iter().any(|b| b == branch)
        {
            app.branches.push(branch.to_owned());
        }
    }
    for app in &mut apps {
        app.branches.sort();
    }
    apps.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then(a.id.cmp(&b.id))
    });
    apps
}

/// Whether `app_id` is installed, on `branch` if one is given. Not
/// `flatpak info`: that errors out when more than one branch of the app is
/// installed (e.g. Dolphin's `stable` and `test`) and none is named.
pub fn is_installed(app_id: &str, branch: Option<&str>) -> bool {
    list_apps().iter().any(|app| {
        app.id == app_id && branch.is_none_or(|branch| app.branches.iter().any(|b| b == branch))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_branches_per_app() {
        let apps = parse_list(
            "org.DolphinEmu.dolphin-emu\tDolphin Emulator\tstable\n\
             org.libretro.RetroArch\tRetroArch\tstable\n\
             org.DolphinEmu.dolphin-emu\tDolphin Emulator\ttest\n\
             org.DolphinEmu.dolphin-emu\tDolphin Emulator\tstable\n",
        );
        assert_eq!(
            apps,
            vec![
                FlatpakApp {
                    id: "org.DolphinEmu.dolphin-emu".into(),
                    name: "Dolphin Emulator".into(),
                    branches: vec!["stable".into(), "test".into()],
                },
                FlatpakApp {
                    id: "org.libretro.RetroArch".into(),
                    name: "RetroArch".into(),
                    branches: vec!["stable".into()],
                },
            ]
        );
    }
}
