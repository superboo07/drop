//! Self-update for the AppImage build, against releases the server admin
//! uploads under Admin > Settings > Updater.
//!
//! A release's identity is its UUID on the server; its tag is only a label and
//! can repeat. We remember which release we last installed (in
//! `client-release.json` in the data dir) along with the file's size and
//! mtime, so we can tell whether the AppImage is still that file. When we
//! can't tell - first run, or the user swapped the file by hand - we send the
//! file's SHA-256 instead and let the server match it.
//!
//! Installing replaces `$APPIMAGE` in place: download to a temp file in the
//! same folder (same filesystem, so the rename is atomic), verify the hash,
//! copy the old file's permissions (plus u+x), then rename over the original.
//! The running copy keeps working, since its mounted image holds the old inode.

use std::{
    fs::{self, File},
    io::{Read, Write},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::{
        LazyLock,
        atomic::{AtomicBool, Ordering},
        nonpoison::Mutex,
    },
    time::UNIX_EPOCH,
};

use chrono::{DateTime, Utc};
use database::{borrow_db_checked, db::DATA_ROOT_DIR};
use futures_lite::StreamExt;
use log::{debug, info, warn};
use remote::{
    auth::generate_authorization_header,
    requests::{generate_url, make_authenticated_get},
    utils::get_client_download,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::AppHandle;
use utils::app_emit;

const TARGET: &str = "linux-appimage";
const RECORD_FILE: &str = "client-release.json";
const STATUS_EVENT: &str = "client_update/status";
const PROGRESS_EVENT: &str = "client_update/progress";

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseNotes {
    id: String,
    tag: String,
    published_at: DateTime<Utc>,
    notes: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct OfferedUpdate {
    id: String,
    tag: String,
    branch_slug: String,
    size: u64,
    sha256: String,
    published_at: DateTime<Utc>,
    /// The offered build is older than the one we run (the newer one was
    /// withdrawn, or we switched from the test branch to release)
    rollback: bool,
    required: bool,
    notes: Vec<ReleaseNotes>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct KnownRelease {
    id: String,
    tag: String,
    branch_slug: String,
}

#[derive(Deserialize)]
struct CheckResponse {
    update: Option<OfferedUpdate>,
    current: Option<KnownRelease>,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct ClientUpdateStatus {
    /// The version this build reports (tauri.conf.json's, as stamped by
    /// build_appimage.sh: "<version>-g<commit>[.dirty]")
    app_version: String,
    arch: &'static str,
    /// Why this copy can't update itself, if it can't
    unsupported_reason: Option<String>,
    appimage_path: Option<String>,
    branch: String,
    /// The server's record of the build we run, when it has one
    current: Option<KnownRelease>,
    update: Option<OfferedUpdate>,
    checking: bool,
    last_checked: Option<DateTime<Utc>>,
    error: Option<String>,
    /// An update was installed; restart to use it
    installed: Option<String>,
}

// Phase names stay PascalCase ("Downloading"), matching ClientUpdateProgress
// in main/types.ts; the fields are single words, so need no renaming
#[derive(Serialize, Clone)]
#[serde(tag = "phase")]
enum Progress {
    Downloading { downloaded: u64, total: u64 },
    Verifying,
    Replacing,
    Done,
    Cancelled,
    Failed { message: String },
}

/// What we last installed, and how to recognise that the file is still it
#[derive(Serialize, Deserialize)]
struct InstalledRecord {
    release_id: String,
    path: PathBuf,
    size: u64,
    mtime: u64,
}

static STATUS: LazyLock<Mutex<ClientUpdateStatus>> =
    LazyLock::new(|| Mutex::new(ClientUpdateStatus::default()));
static INSTALLING: AtomicBool = AtomicBool::new(false);
static CANCEL: AtomicBool = AtomicBool::new(false);

fn arch() -> &'static str {
    std::env::consts::ARCH
}

fn appimage_path() -> Result<PathBuf, String> {
    match std::env::var_os("APPIMAGE") {
        Some(path) if !path.is_empty() => Ok(PathBuf::from(path)),
        _ => Err("This copy of Drop wasn't started from an AppImage. Only the AppImage build can install updates from your server, so update this copy the same way you installed it.".to_string()),
    }
}

fn record_path() -> PathBuf {
    DATA_ROOT_DIR.join(RECORD_FILE)
}

fn file_identity(path: &Path) -> std::io::Result<(u64, u64)> {
    let meta = fs::metadata(path)?;
    let mtime = meta
        .modified()?
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    Ok((meta.len(), mtime))
}

/// The release id we installed, if the AppImage is still that exact file
fn recorded_release(appimage: &Path) -> Option<String> {
    let record: InstalledRecord =
        serde_json::from_str(&fs::read_to_string(record_path()).ok()?).ok()?;
    let (size, mtime) = file_identity(appimage).ok()?;
    (record.path == appimage && record.size == size && record.mtime == mtime)
        .then_some(record.release_id)
}

fn write_record(appimage: &Path, release_id: &str) {
    let result = file_identity(appimage).and_then(|(size, mtime)| {
        let record = InstalledRecord {
            release_id: release_id.to_string(),
            path: appimage.to_path_buf(),
            size,
            mtime,
        };
        fs::write(
            record_path(),
            serde_json::to_string(&record).expect("record serializes"),
        )
    });
    if let Err(e) = result {
        warn!("failed to record installed client release: {e}");
    }
}

fn hash_file(path: &Path) -> std::io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1024 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex::encode(hasher.finalize()))
}

fn emit_status(app: &AppHandle) {
    let status = STATUS.lock().clone();
    app_emit!(app, STATUS_EVENT, status);
}

fn emit_progress(app: &AppHandle, progress: Progress) {
    app_emit!(app, PROGRESS_EVENT, progress);
}

fn update_status(app: &AppHandle, change: impl FnOnce(&mut ClientUpdateStatus)) {
    change(&mut STATUS.lock());
    emit_status(app);
}

/// Fills in what we know without asking the server
fn init_status(app: &AppHandle) {
    let branch = borrow_db_checked().settings.update_branch;
    let mut status = STATUS.lock();
    status.app_version = app.package_info().version.to_string();
    status.arch = arch();
    status.branch = branch.as_str().to_string();
    match appimage_path() {
        Ok(path) => status.appimage_path = Some(path.to_string_lossy().to_string()),
        Err(reason) => status.unsupported_reason = Some(reason),
    }
    if !matches!(arch(), "x86_64" | "aarch64") {
        status.unsupported_reason = Some(format!(
            "Updates aren't available for {} computers.",
            arch()
        ));
    }
}

#[tauri::command]
pub fn fetch_client_update_status(app: AppHandle) -> ClientUpdateStatus {
    if STATUS.lock().app_version.is_empty() {
        init_status(&app);
    }
    STATUS.lock().clone()
}

#[tauri::command]
pub async fn check_client_update(app: AppHandle) -> Result<ClientUpdateStatus, String> {
    if STATUS.lock().app_version.is_empty() {
        init_status(&app);
    }
    if STATUS.lock().unsupported_reason.is_some() || INSTALLING.load(Ordering::Relaxed) {
        return Ok(STATUS.lock().clone());
    }

    update_status(&app, |s| {
        s.checking = true;
        s.error = None;
    });
    let result = check().await;
    update_status(&app, |s| {
        s.checking = false;
        s.last_checked = Some(Utc::now());
        match result {
            Ok(response) => {
                s.update = response.update;
                s.current = response.current;
            }
            Err(e) => {
                warn!("client update check failed: {e}");
                s.error = Some(e);
            }
        }
    });
    Ok(STATUS.lock().clone())
}

async fn check() -> Result<CheckResponse, String> {
    let appimage = appimage_path()?;
    let branch = borrow_db_checked().settings.update_branch;
    {
        let mut status = STATUS.lock();
        status.branch = branch.as_str().to_string();
    }

    let recorded = recorded_release(&appimage);
    let sha256 = match &recorded {
        Some(_) => String::new(),
        None => {
            let path = appimage.clone();
            tauri::async_runtime::spawn_blocking(move || hash_file(&path))
                .await
                .map_err(|e| e.to_string())?
                .map_err(|e| format!("Couldn't read {}: {e}", appimage.display()))?
        }
    };

    let url = generate_url(
        &["/api/v1/client/update"],
        &[
            ("target", TARGET),
            ("arch", arch()),
            ("branch", branch.as_str()),
            ("current", recorded.as_deref().unwrap_or("")),
            ("sha256", &sha256),
        ],
    )
    .map_err(|e| e.to_string())?;
    let response = make_authenticated_get(url)
        .await
        .map_err(|e| format!("Couldn't reach the server: {e}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "The server couldn't check for updates (HTTP {}).",
            response.status()
        ));
    }
    let response: CheckResponse = response
        .json()
        .await
        .map_err(|e| format!("The server sent an unexpected reply: {e}"))?;

    // The server recognised our file by hash - remember which release it is
    if recorded.is_none()
        && let Some(current) = &response.current
    {
        write_record(&appimage, &current.id);
    }

    debug!(
        "client update check: current {:?}, offered {:?}",
        response.current.as_ref().map(|c| &c.id),
        response.update.as_ref().map(|u| &u.id)
    );
    Ok(response)
}

#[tauri::command]
pub async fn install_client_update(app: AppHandle) -> Result<(), String> {
    let update = STATUS
        .lock()
        .update
        .clone()
        .ok_or("There's no update to install.")?;
    if INSTALLING.swap(true, Ordering::SeqCst) {
        return Err("An update is already being installed.".to_string());
    }
    CANCEL.store(false, Ordering::SeqCst);

    let result = install(&app, &update).await;
    INSTALLING.store(false, Ordering::SeqCst);

    match result {
        Ok(()) => {
            info!("installed client release {} ({})", update.tag, update.id);
            update_status(&app, |s| {
                s.installed = Some(update.tag.clone());
                s.current = Some(KnownRelease {
                    id: update.id.clone(),
                    tag: update.tag.clone(),
                    branch_slug: update.branch_slug.clone(),
                });
                s.update = None;
            });
            emit_progress(&app, Progress::Done);
            Ok(())
        }
        Err(InstallError::Cancelled) => {
            emit_progress(&app, Progress::Cancelled);
            Ok(())
        }
        Err(InstallError::Failed(message)) => {
            warn!("client update failed: {message}");
            emit_progress(
                &app,
                Progress::Failed {
                    message: message.clone(),
                },
            );
            Err(message)
        }
    }
}

#[tauri::command]
pub fn cancel_client_update() {
    CANCEL.store(true, Ordering::SeqCst);
}

enum InstallError {
    Cancelled,
    Failed(String),
}

impl From<String> for InstallError {
    fn from(value: String) -> Self {
        InstallError::Failed(value)
    }
}

/// Removes the temp file unless the install got as far as renaming it
struct TempFile(PathBuf);
impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

async fn install(app: &AppHandle, update: &OfferedUpdate) -> Result<(), InstallError> {
    let appimage = appimage_path()?;
    let dir = appimage
        .parent()
        .ok_or_else(|| format!("Couldn't find the folder {} is in.", appimage.display()))?;
    let file_name = appimage
        .file_name()
        .ok_or_else(|| format!("{} isn't a file.", appimage.display()))?
        .to_string_lossy()
        .to_string();
    let temp = TempFile(dir.join(format!(".{file_name}.update")));

    let not_writable = |e: std::io::Error| {
        format!(
            "Drop can't write to {}: {e}. Move Drop to a folder you own (like ~/Applications) and try again.",
            dir.display()
        )
    };
    let mut file = File::create(&temp.0).map_err(not_writable)?;

    // ── download, hashing as we go ──
    let url = generate_url(&["/api/v1/client/update", &update.id, "download"], &[])
        .map_err(|e| e.to_string())?;
    let response = get_client_download()
        .get(url)
        .header("Authorization", generate_authorization_header())
        .send()
        .await
        .map_err(|e| format!("Couldn't start the download: {e}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "The server refused the download (HTTP {}).",
            response.status()
        )
        .into());
    }

    let total = response.content_length().unwrap_or(update.size);
    let mut downloaded = 0u64;
    let mut last_emitted = 0u64;
    let mut hasher = Sha256::new();
    let mut stream = response.bytes_stream();
    emit_progress(app, Progress::Downloading { downloaded, total });
    while let Some(chunk) = stream.next().await {
        if CANCEL.load(Ordering::SeqCst) {
            return Err(InstallError::Cancelled);
        }
        let chunk = chunk.map_err(|e| format!("The download was interrupted: {e}"))?;
        hasher.update(&chunk);
        file.write_all(&chunk).map_err(not_writable)?;
        downloaded += chunk.len() as u64;
        // ~every 1MB, so the UI isn't flooded
        if downloaded - last_emitted >= 1024 * 1024 || downloaded == total {
            last_emitted = downloaded;
            emit_progress(app, Progress::Downloading { downloaded, total });
        }
    }

    // ── verify ──
    emit_progress(app, Progress::Verifying);
    let hash = hex::encode(hasher.finalize());
    if !hash.eq_ignore_ascii_case(&update.sha256) {
        return Err(
            "The downloaded file doesn't match the server's checksum, so it wasn't installed. Try again."
                .to_string()
                .into(),
        );
    }

    // ── permissions: same as the file we're replacing, and runnable ──
    let old_mode = fs::metadata(&appimage)
        .map(|m| m.permissions().mode() & 0o7777)
        .unwrap_or(0o755);
    let mode = if old_mode & 0o100 == 0 {
        old_mode | 0o755
    } else {
        old_mode
    };
    file.set_permissions(fs::Permissions::from_mode(mode))
        .map_err(not_writable)?;
    file.sync_all().map_err(not_writable)?;
    drop(file);

    if CANCEL.load(Ordering::SeqCst) {
        return Err(InstallError::Cancelled);
    }

    // ── swap ──
    emit_progress(app, Progress::Replacing);
    fs::rename(&temp.0, &appimage).map_err(not_writable)?;
    if let Ok(dir_handle) = File::open(dir) {
        let _ = dir_handle.sync_all();
    }
    write_record(&appimage, &update.id);
    Ok(())
}

/// Starts the new AppImage once this process has exited (the single-instance
/// plugin would otherwise hand it straight back to us), then quits.
#[tauri::command]
pub async fn restart_after_client_update(app: AppHandle) -> Result<(), String> {
    let appimage = appimage_path()?;
    let mut command = std::process::Command::new("sh");
    command
        .arg("-c")
        .arg(r#"while kill -0 "$1" 2>/dev/null; do sleep 0.2; done; exec "$2""#)
        .arg("drop-restart")
        .arg(std::process::id().to_string())
        .arg(&appimage)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    // Don't hand our AppImage's runtime environment to the new one
    utils::external_open::sanitize_external_command(&mut command);
    std::os::unix::process::CommandExt::process_group(&mut command, 0);
    command
        .spawn()
        .map_err(|e| format!("Couldn't start the new version: {e}"))?;

    crate::client::cleanup_and_exit(&app).await;
    Ok(())
}
