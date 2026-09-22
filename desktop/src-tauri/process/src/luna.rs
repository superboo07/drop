//! LunaTranslator integration (Linux only).
//!
//! LunaTranslator is a separate GPL application that the user installs
//! themselves; Drop never bundles or links any part of it. All we do is run
//! the user's own copy as an ordinary child process and arrange for the game
//! to be launched through LunaTranslator's bridge.
//!
//! How the bridge works, upstream's design: `LunaCompanion.exe` is a small
//! Windows executable that runs *inside* the game's Wine/Proton prefix. It
//! loads LunaHost, injects LunaHook into the game, and connects back out to
//! LunaTranslator on the host over plain TCP on loopback (which is reachable
//! from inside pressure-vessel, since the container shares host networking).
//! LunaTranslator is the server and the companion is the client, so the
//! ordering is forgiving: the companion retries the connection for ~30s while
//! the game and LunaTranslator are both still starting up.
//!
//! So a Luna-enabled launch is just the normal Proton launch with the game
//! command wrapped:
//!
//! ```text
//! umu-run LunaCompanion.exe 127.0.0.1:52300 --hook-dir Z:\...\LunaHook\ Z:\...\game.exe [args]
//! ```

use std::{
    fs::{File, create_dir_all, read_dir},
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
};

use database::{Database, db::DATA_ROOT_DIR};
use log::{info, warn};
use utils::external_open::sanitize_external_command;

use crate::{error::ProcessError, parser::ParsedCommand};

/// Where LunaTranslator's AppImage keeps the bridge and the hook DLLs it
/// needs beside it.
const BRIDGE_DIR_IN_APPIMAGE: &str = "opt/lunatranslator/files/LunaHook";
const BRIDGE_EXECUTABLE: &str = "LunaCompanion.exe";

/// Our extracted copy of that directory, so we don't re-extract a ~360MB
/// AppImage on every launch.
pub fn bridge_cache_dir() -> PathBuf {
    DATA_ROOT_DIR.join("luna").join("LunaHook")
}

/// Everything needed to wrap a game command in the bridge.
pub struct LunaBridge {
    /// The companion executable, as a host path (Proton takes a unix path
    /// for the executable it runs).
    pub executable: PathBuf,
    /// The directory holding LunaHost/LunaHook DLLs, as a Wine path - this is
    /// passed to the companion, which is running inside the prefix.
    pub hook_dir_windows: String,
    pub port: u16,
}

/// Resolves the bridge, extracting it from the configured LunaTranslator
/// AppImage the first time.
pub fn resolve_bridge(database: &Database) -> Result<LunaBridge, ProcessError> {
    let port = database.settings.luna_port;

    // An explicit override wins and is never second-guessed.
    if let Some(configured) = database
        .settings
        .luna_bridge_path
        .as_ref()
        .filter(|path| !path.is_empty())
    {
        let executable = PathBuf::from(configured);
        if !executable.is_file() {
            return Err(ProcessError::LunaBridgeMissing(configured.clone()));
        }
        let hook_dir = executable
            .parent()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/"));
        return Ok(LunaBridge {
            hook_dir_windows: windows_path(&hook_dir),
            executable,
            port,
        });
    }

    let cached = bridge_cache_dir();
    if !cached.join(BRIDGE_EXECUTABLE).is_file() {
        extract_bridge(database)?;
    }

    let executable = cached.join(BRIDGE_EXECUTABLE);
    if !executable.is_file() {
        return Err(ProcessError::LunaBridgeMissing(
            executable.to_string_lossy().to_string(),
        ));
    }

    Ok(LunaBridge {
        hook_dir_windows: windows_path(&cached),
        executable,
        port,
    })
}

/// Pulls `LunaHook/` out of the configured LunaTranslator install into our
/// own data directory.
///
/// Also accepts a path to an unpacked install (a checkout, or an extracted
/// AppDir), since "point Drop at LunaTranslator" is a reasonable thing to do
/// with something that isn't an AppImage.
pub fn extract_bridge(database: &Database) -> Result<(), ProcessError> {
    let source = database
        .settings
        .luna_translator_path
        .as_ref()
        .filter(|path| !path.is_empty())
        .ok_or(ProcessError::LunaNotConfigured)?;
    let source = PathBuf::from(source);

    let destination = bridge_cache_dir();

    if source.is_dir() {
        // A LunaTranslator source tree or an extracted AppDir - the hook
        // directory sits at one of a few known places under it.
        let candidates = [
            source.join(BRIDGE_DIR_IN_APPIMAGE),
            source.join("src/files/LunaHook"),
            source.join("files/LunaHook"),
            source.clone(),
        ];
        let found = candidates
            .iter()
            .find(|candidate| candidate.join(BRIDGE_EXECUTABLE).is_file())
            .ok_or_else(|| {
                ProcessError::LunaBridgeMissing(source.to_string_lossy().to_string())
            })?;
        copy_dir(found, &destination)?;
        info!("extracted LunaTranslator bridge from {}", found.display());
        return Ok(());
    }

    if !source.is_file() {
        return Err(ProcessError::LunaBridgeMissing(
            source.to_string_lossy().to_string(),
        ));
    }

    // AppImages extract into `squashfs-root` relative to the working
    // directory, and take a glob for what to pull out - so this costs a
    // couple of megabytes rather than unpacking the whole ~360MB image.
    let staging = DATA_ROOT_DIR.join("luna").join("extract");
    if staging.exists() {
        std::fs::remove_dir_all(&staging)?;
    }
    create_dir_all(&staging)?;

    let mut command = Command::new(&source);
    command
        .arg("--appimage-extract")
        .arg(format!("{BRIDGE_DIR_IN_APPIMAGE}/*"))
        .current_dir(&staging)
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    sanitize_external_command(&mut command);

    let status = command.status()?;
    if !status.success() {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(ProcessError::LunaExtractFailed(format!(
            "{} --appimage-extract exited with {status}",
            source.display()
        )));
    }

    let extracted = staging.join("squashfs-root").join(BRIDGE_DIR_IN_APPIMAGE);
    if !extracted.join(BRIDGE_EXECUTABLE).is_file() {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(ProcessError::LunaExtractFailed(format!(
            "{BRIDGE_EXECUTABLE} was not found inside {}. Is this a LunaTranslator AppImage?",
            source.display()
        )));
    }

    copy_dir(&extracted, &destination)?;
    let _ = std::fs::remove_dir_all(&staging);
    info!(
        "extracted LunaTranslator bridge from {} to {}",
        source.display(),
        destination.display()
    );
    Ok(())
}

/// Wraps a Proton launch command so the game starts under the bridge.
///
/// `launch_command` is the game as it would otherwise be launched (an
/// absolute host path plus its arguments). Inside the prefix the game has to
/// be named the way Windows sees it, hence the `Z:` translation - but the
/// companion itself is what Proton executes, so that stays a host path.
pub fn wrap_launch_command(
    bridge: &LunaBridge,
    launch_command: String,
) -> Result<String, ProcessError> {
    let game = ParsedCommand::parse(launch_command)?;

    // The companion loads a 32- or 64-bit LunaHost to match the game; it
    // can't tell on its own, so it defaults to its own bitness (64). Visual
    // novels - the whole reason this integration exists - are overwhelmingly
    // 32-bit, so getting this wrong by default would break the common case.
    let bitness_flag = match pe_is_64_bit(Path::new(&game.command)) {
        Some(true) => "--64",
        Some(false) => "--32",
        None => {
            warn!(
                "could not read the PE header of {}, letting LunaCompanion use its default bitness",
                game.command
            );
            "--64"
        }
    };

    let mut parts = game.env;
    parts.push(bridge.executable.to_string_lossy().to_string());
    parts.push(format!("127.0.0.1:{}", bridge.port));
    parts.push(bitness_flag.to_owned());
    parts.push("--hook-dir".to_owned());
    parts.push(bridge.hook_dir_windows.clone());
    parts.push(windows_path(Path::new(&game.command)));
    parts.extend(game.args);

    Ok(shell_words::join(parts))
}

/// Starts the user's LunaTranslator.
///
/// `display`, when set, is the nested session's X display: LunaTranslator
/// then has to be told to use X11 explicitly and to forget the host's Wayland
/// socket, or Qt will happily connect back out to the outer compositor and
/// the window will appear outside the session it's supposed to be inside.
pub fn spawn(
    database: &Database,
    display: Option<&str>,
    stdout: File,
    stderr: File,
) -> Result<Child, ProcessError> {
    let path = database
        .settings
        .luna_translator_path
        .as_ref()
        .filter(|path| !path.is_empty())
        .ok_or(ProcessError::LunaNotConfigured)?;
    // The settings page accepts a directory as well as an AppImage, since an
    // extracted AppDir is a perfectly ordinary way to have LunaTranslator
    // installed. `AppRun` is that directory's entry point.
    let path = PathBuf::from(path);
    let path = if path.is_dir() {
        let app_run = path.join("AppRun");
        if !app_run.is_file() {
            return Err(ProcessError::LunaBridgeMissing(
                app_run.to_string_lossy().to_string(),
            ));
        }
        app_run
    } else {
        path
    };
    if !path.is_file() {
        return Err(ProcessError::LunaBridgeMissing(
            path.to_string_lossy().to_string(),
        ));
    }

    let mut command = Command::new(&path);
    command.stdout(stdout).stderr(stderr);
    // LunaTranslator's AppImage bundles its own Python and GStreamer; it must
    // not inherit ours (see utils::external_open for the full story).
    sanitize_external_command(&mut command);

    if let Some(display) = display {
        command
            .env("DISPLAY", display)
            .env("QT_QPA_PLATFORM", "xcb")
            .env_remove("WAYLAND_DISPLAY");
    }

    // An AppImage's runtime forks the payload rather than exec'ing it, so
    // killing the pid we spawn would leave LunaTranslator itself running.
    // Its own process group makes the whole tree signalable at once.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt as _;
        command.process_group(0);
    }

    info!("starting LunaTranslator from {}", path.display());
    Ok(command.spawn()?)
}

/// Translates a host path to the way Wine sees it. Wine maps `Z:` to `/` in
/// every prefix, including the ones umu-run creates.
fn windows_path(path: &Path) -> String {
    format!("Z:{}", path.to_string_lossy().replace('/', "\\"))
}

/// Reads a PE executable's COFF machine field. `None` when the file isn't
/// readable or isn't a PE at all.
fn pe_is_64_bit(path: &Path) -> Option<bool> {
    let mut file = File::open(path).ok()?;

    let mut magic = [0u8; 2];
    file.read_exact(&mut magic).ok()?;
    if &magic != b"MZ" {
        return None;
    }

    // e_lfanew, at a fixed offset in the DOS header, points at the PE header.
    file.seek(SeekFrom::Start(0x3c)).ok()?;
    let mut offset = [0u8; 4];
    file.read_exact(&mut offset).ok()?;

    file.seek(SeekFrom::Start(u32::from_le_bytes(offset) as u64))
        .ok()?;
    let mut header = [0u8; 6];
    file.read_exact(&mut header).ok()?;
    if &header[0..4] != b"PE\0\0" {
        return None;
    }

    match u16::from_le_bytes([header[4], header[5]]) {
        0x8664 | 0xaa64 => Some(true), // x86-64, arm64
        0x014c | 0x01c4 => Some(false), // i386, armnt
        _ => None,
    }
}

/// Recursive directory copy. The bridge directory is a dozen flat files, so
/// this doesn't need to be clever - just not to pull in a dependency for it.
fn copy_dir(source: &Path, destination: &Path) -> Result<(), ProcessError> {
    create_dir_all(destination)?;
    for entry in read_dir(source)? {
        let entry = entry?;
        let target = destination.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// The pieces of a Luna-enabled launch that live for as long as the game
/// does: LunaTranslator itself, and (optionally) the nested X session both it
/// and the game are running inside.
///
/// Dropping this shuts both down, in that order - the field order below is
/// the drop order, and killing the X server out from under LunaTranslator
/// first would just make it crash instead of exit.
pub struct LunaSession {
    luna: Child,
    nested: Option<nested_session::NestedSession>,
}

impl LunaSession {
    pub fn new(luna: Child, nested: Option<nested_session::NestedSession>) -> Self {
        LunaSession { luna, nested }
    }

    /// The display the game must be launched on, when running nested.
    pub fn display(&self) -> Option<&str> {
        self.nested.as_ref().map(|nested| nested.display())
    }
}

impl Drop for LunaSession {
    fn drop(&mut self) {
        // Signal the group set at spawn time first, so the AppImage's payload
        // goes down with its runtime and gets to exit cleanly; `kill` is the
        // backstop for anything that ignores it.
        #[cfg(unix)]
        {
            let pid = self.luna.id() as i32;
            unsafe {
                libc::kill(-pid, libc::SIGTERM);
            }
            for _ in 0..30 {
                match self.luna.try_wait() {
                    Ok(Some(_)) => {
                        info!("LunaTranslator stopped");
                        return;
                    }
                    Ok(None) => std::thread::sleep(std::time::Duration::from_millis(100)),
                    Err(_) => break,
                }
            }
        }

        if let Err(e) = self.luna.kill() {
            warn!("failed to stop LunaTranslator: {e}");
        }
        let _ = self.luna.wait();
        info!("LunaTranslator stopped");
    }
}
