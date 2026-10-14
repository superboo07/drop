//! Sends the client's existing logs to the Drop server, where admins read
//! them under Admin > Clients > Logs.
//!
//! Nothing here writes logs of its own: a bundle is `drop.log` (and the
//! previous run's, kept as `drop.previous.log` by `setup()`), a game's
//! launch logs from `logs/<game id>/`, and any `crash-*.log` the panic
//! handler wrote, plus a few environment variables that explain most
//! "works on the desktop, not under Steam" problems.
//!
//! Bundles are sent when a game fails to launch, on the start after a
//! crash (both unless "Send logs automatically" is off), and when someone
//! presses "Send logs" in Settings > Debug. Nothing is sent in always-offline
//! mode.

use std::{
    fs::{self, File},
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::OnceLock,
    time::Duration,
};

use database::{borrow_db_checked, db::DATA_ROOT_DIR};
use log::{info, warn};
use process::process_manager::{LAUNCH_FAILURE_HOOK, LaunchFailure};
use remote::{
    auth::generate_authorization_header, requests::generate_url, utils::DROP_CLIENT_ASYNC,
};
use serde::{Deserialize, Serialize};
use tauri::AppHandle;

pub const DROP_LOG: &str = "drop.log";
pub const PREVIOUS_DROP_LOG: &str = "drop.previous.log";

/// Each file is cut to its last part beyond this, so a bundle stays well
/// under the server's upload limit (20 MB by default)
const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;
const UPLOAD_TIMEOUT: Duration = Duration::from_secs(120);

/// Variables worth seeing next to the log. Only these are sent, never the
/// whole environment.
const ENV_VARS: &[&str] = &[
    "PATH",
    "LD_LIBRARY_PATH",
    "LD_PRELOAD",
    "PYTHONHOME",
    "PYTHONPATH",
    "APPIMAGE",
    "XDG_SESSION_TYPE",
    "XDG_CURRENT_DESKTOP",
    "XDG_RUNTIME_DIR",
    "DISPLAY",
    "WAYLAND_DISPLAY",
    "GAMESCOPE_WAYLAND_DISPLAY",
    "STEAM_RUNTIME",
    "SteamAppId",
    "SteamGameId",
    "WEBKIT_DISABLE_DMABUF_RENDERER",
    "RUST_LOG",
    "LANG",
];

#[derive(Serialize)]
#[serde(rename_all = "lowercase")]
enum Trigger {
    Manual,
    Crash,
    Launch,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Summary {
    title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_error: Option<String>,
}

#[derive(Serialize)]
struct BundleFile {
    name: String,
    label: String,
    path: String,
    content: String,
    truncated: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Bundle {
    trigger: Trigger,
    version: String,
    arch: &'static str,
    session: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    game_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    summary: Option<Summary>,
    environment: Vec<(String, String, bool)>,
    files: Vec<BundleFile>,
}

#[derive(Deserialize)]
struct UploadResponse {
    accepted: bool,
}

static APP: OnceLock<AppHandle> = OnceLock::new();

/// Sends a bundle whenever the process manager sees a game fail to launch
pub fn init(app: &AppHandle) {
    let _ = APP.set(app.clone());
    let _ = LAUNCH_FAILURE_HOOK.set(report_launch_failure);
}

/// Rename the last run's `drop.log` before log4rs truncates it, so a crash
/// report (sent on this start) and "Send logs" still have it
pub fn keep_previous_log() {
    let current = DATA_ROOT_DIR.join(DROP_LOG);
    if current.exists()
        && let Err(e) = fs::rename(&current, DATA_ROOT_DIR.join(PREVIOUS_DROP_LOG))
    {
        eprintln!("failed to keep the previous drop.log: {e}");
    }
}

fn version(app: &AppHandle) -> String {
    app.package_info().version.to_string()
}

fn session() -> String {
    if std::env::var_os("GAMESCOPE_WAYLAND_DISPLAY").is_some() {
        return "gamescope".to_string();
    }
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").ok();
    let kind = std::env::var("XDG_SESSION_TYPE").ok();
    match (desktop, kind) {
        (Some(desktop), Some(kind)) => format!("{desktop} ({kind})"),
        (Some(desktop), None) => desktop,
        (None, Some(kind)) => kind,
        (None, None) => std::env::consts::OS.to_string(),
    }
}

/// [name, value, flagged]: flagged values differ from a normal desktop start
fn environment() -> Vec<(String, String, bool)> {
    let mut entries = Vec::new();

    #[cfg(target_os = "linux")]
    {
        let render_nodes: Vec<String> = fs::read_dir("/dev/dri")
            .map(|dir| {
                let mut names: Vec<String> = dir
                    .flatten()
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect();
                names.sort();
                names
            })
            .unwrap_or_default();
        let has_render_node = render_nodes.iter().any(|n| n.starts_with("renderD"));
        entries.push((
            "/dev/dri".to_string(),
            if render_nodes.is_empty() {
                "(missing)".to_string()
            } else {
                render_nodes.join(" ")
            },
            !has_render_node,
        ));
    }

    for &name in ENV_VARS {
        let value = std::env::var(name).ok();
        let flagged = match name {
            "PATH" => value.is_none(),
            "GAMESCOPE_WAYLAND_DISPLAY" | "PYTHONHOME" | "PYTHONPATH" => value.is_some(),
            _ => false,
        };
        // Unset values only matter where their absence is the point
        if value.is_none() && !matches!(name, "PATH" | "DISPLAY" | "WAYLAND_DISPLAY") {
            continue;
        }
        let mut value = value.unwrap_or_else(|| "(unset)".to_string());
        value.truncate(4096);
        entries.push((name.to_string(), value, flagged));
    }

    if std::env::var_os("DISPLAY").is_none() && std::env::var_os("WAYLAND_DISPLAY").is_none() {
        for entry in entries
            .iter_mut()
            .filter(|(name, ..)| name == "DISPLAY" || name == "WAYLAND_DISPLAY")
        {
            entry.2 = true;
        }
    }

    entries
}

/// The file's last `MAX_FILE_BYTES`, starting at a whole line
fn read_tail(path: &Path) -> Option<(String, bool)> {
    let mut file = File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let truncated = len > MAX_FILE_BYTES;
    if truncated {
        file.seek(SeekFrom::Start(len - MAX_FILE_BYTES)).ok()?;
    }
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).ok()?;
    let mut content = String::from_utf8_lossy(&bytes).into_owned();
    if truncated && let Some(newline) = content.find('\n') {
        content.drain(..=newline);
    }
    Some((content, truncated))
}

fn bundle_file(path: &Path, label: &str) -> Option<BundleFile> {
    let (content, truncated) = read_tail(path)?;
    Some(BundleFile {
        name: path.file_name()?.to_string_lossy().into_owned(),
        label: label.to_string(),
        path: path.to_string_lossy().into_owned(),
        content,
        truncated,
    })
}

/// The last line of a game's stderr that looks like an error, or else its
/// last line - what the admin page shows at the top
fn last_error(path: &Path) -> Option<String> {
    let (content, _) = read_tail(path)?;
    let lines: Vec<&str> = content
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    let is_error = |line: &str| {
        let lower = line.to_ascii_lowercase();
        lower.starts_with("error") || lower.starts_with("err:") || lower.contains("fatal")
    };
    let line = lines
        .iter()
        .rev()
        .find(|line| is_error(line))
        .or(lines.last())?;
    Some(line.chars().take(2000).collect())
}

/// `crash-*.log` files the panic handler wrote that haven't been sent yet
fn unsent_crash_files() -> Vec<PathBuf> {
    let Ok(dir) = fs::read_dir(DATA_ROOT_DIR.as_path()) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = dir
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("crash-") && n.ends_with(".log"))
        })
        .collect();
    files.sort();
    files
}

/// The most recently written stdout/stderr logs of any game
fn newest_game_logs() -> Vec<PathBuf> {
    let newest = fs::read_dir(DATA_ROOT_DIR.join("logs"))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|game| fs::read_dir(game.path()).ok())
        .flatten()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().ends_with("-error.log"))
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .max_by_key(|(modified, _)| *modified);
    let Some((_, error_log)) = newest else {
        return Vec::new();
    };
    let name = error_log.file_name().unwrap_or_default().to_string_lossy();
    let output_log = error_log.with_file_name(name.replace("-error.log", ".log"));
    vec![error_log, output_log]
}

async fn upload(bundle: Bundle) -> Result<bool, String> {
    {
        let db = borrow_db_checked();
        // Always-offline mode means nothing leaves this device, logs included.
        // Crash reports stay on disk and go out once it's turned off.
        if db.settings.force_offline {
            return Err("Drop is in always-offline mode.".to_string());
        }
        if db.auth.is_none() {
            return Err("This client isn't signed in to a server.".to_string());
        }
    }
    let url = generate_url(&["/api/v1/client/logs"], &[]).map_err(|e| e.to_string())?;
    let response = DROP_CLIENT_ASYNC
        .post(url)
        .header("Authorization", generate_authorization_header())
        .timeout(UPLOAD_TIMEOUT)
        .json(&bundle)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("the server answered {status}: {body}"));
    }
    let response: UploadResponse = response.json().await.map_err(|e| e.to_string())?;
    Ok(response.accepted)
}

/// Sends `crash-*.log` files left by an earlier run, with that run's log.
/// Called once the client is signed in; files stay for the next start if
/// sending fails.
pub async fn send_crash_reports(app: AppHandle) {
    if !borrow_db_checked().settings.send_logs_automatically {
        return;
    }
    let crashes = unsent_crash_files();
    if crashes.is_empty() {
        return;
    }

    let mut files: Vec<BundleFile> = crashes
        .iter()
        .filter_map(|path| bundle_file(path, "crash"))
        .collect();
    let panic = files
        .first()
        .and_then(|f| f.content.lines().nth(1).map(str::to_string));
    files.extend(bundle_file(
        &DATA_ROOT_DIR.join(PREVIOUS_DROP_LOG),
        "run that crashed",
    ));

    let bundle = Bundle {
        trigger: Trigger::Crash,
        version: version(&app),
        arch: std::env::consts::ARCH,
        session: session(),
        game_id: None,
        summary: Some(Summary {
            title: "Drop crashed".to_string(),
            detail: (crashes.len() > 1).then(|| format!("{} crash reports", crashes.len())),
            last_error: panic,
        }),
        environment: environment(),
        files,
    };

    match upload(bundle).await {
        Ok(accepted) => {
            info!(
                "sent {} crash report(s) (accepted: {accepted})",
                crashes.len()
            );
            for path in crashes {
                let mut sent = path.clone().into_os_string();
                sent.push(".sent");
                if let Err(e) = fs::rename(&path, sent) {
                    warn!("failed to mark {} as sent: {e}", path.display());
                }
            }
        }
        Err(e) => warn!("failed to send crash reports: {e}"),
    }
}

/// Hooked into the process manager (LAUNCH_FAILURE_HOOK), and called for
/// launches that fail before the game runs
pub fn report_launch_failure(failure: LaunchFailure) {
    // Called with the database already borrowed (on_process_finish), so the
    // setting is read in the task below rather than here
    let Some(app) = APP.get().cloned() else {
        warn!("not sending launch failure logs: client_logs::init hasn't run");
        return;
    };
    tauri::async_runtime::spawn(async move {
        if !borrow_db_checked().settings.send_logs_automatically {
            return;
        }
        let mut files = Vec::new();
        if let Some(path) = &failure.error_log_path {
            files.extend(bundle_file(path, "game stderr"));
        }
        if let Some(path) = &failure.log_path {
            files.extend(bundle_file(path, "game stdout"));
        }
        files.extend(bundle_file(&DATA_ROOT_DIR.join(DROP_LOG), "this run"));

        let title = match (&failure.error, &failure.exit) {
            (Some(_), _) => "The game couldn't be started".to_string(),
            (None, Some(exit)) => format!(
                "The game exited ({exit}) after {:.1} s",
                failure.elapsed.as_secs_f32()
            ),
            (None, None) => "The game failed to launch".to_string(),
        };
        let last_error = failure
            .error
            .clone()
            .or_else(|| failure.error_log_path.as_deref().and_then(last_error));

        let bundle = Bundle {
            trigger: Trigger::Launch,
            version: version(&app),
            arch: std::env::consts::ARCH,
            session: session(),
            game_id: Some(failure.game_id.clone()),
            summary: Some(Summary {
                title,
                detail: None,
                last_error,
            }),
            environment: environment(),
            files,
        };

        match upload(bundle).await {
            Ok(accepted) => info!(
                "sent launch failure logs for {} (accepted: {accepted})",
                failure.game_id
            ),
            Err(e) => warn!("failed to send launch failure logs: {e}"),
        }
    });
}

/// For launch errors returned before a game process exists
pub fn report_launch_error(game_id: &str, error: &process::error::ProcessError) {
    use process::error::ProcessError;
    if matches!(
        error,
        ProcessError::AlreadyRunning | ProcessError::RequiredDependency(..)
    ) {
        return;
    }
    report_launch_failure(LaunchFailure {
        game_id: game_id.to_string(),
        exit: None,
        elapsed: Duration::ZERO,
        log_path: None,
        error_log_path: None,
        error: Some(error.to_string()),
    });
}

/// "Send logs" in Settings > Debug
#[tauri::command]
pub async fn send_client_logs(app: AppHandle) -> Result<(), String> {
    let mut files = Vec::new();
    files.extend(bundle_file(&DATA_ROOT_DIR.join(DROP_LOG), "this run"));
    files.extend(bundle_file(
        &DATA_ROOT_DIR.join(PREVIOUS_DROP_LOG),
        "previous run",
    ));
    for path in newest_game_logs() {
        let label = if path.to_string_lossy().ends_with("-error.log") {
            "last game stderr"
        } else {
            "last game stdout"
        };
        files.extend(bundle_file(&path, label));
    }

    let bundle = Bundle {
        trigger: Trigger::Manual,
        version: version(&app),
        arch: std::env::consts::ARCH,
        session: session(),
        game_id: None,
        summary: None,
        environment: environment(),
        files,
    };

    match upload(bundle).await? {
        true => Ok(()),
        false => Err("The server didn't accept the logs.".to_string()),
    }
}
