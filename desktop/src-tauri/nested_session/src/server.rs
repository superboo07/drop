use std::{
    collections::BTreeSet,
    ffi::OsString,
    os::unix::process::CommandExt as _,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::Mutex,
    thread::sleep,
    time::{Duration, Instant},
};

use log::{info, warn};
use utils::external_open::sanitize_external_command;

use crate::error::NestedSessionError;

/// Display numbers below this are in normal use (`:0` for the session,
/// `:1`.. for Xwayland instances Steam/gamescope/Proton start themselves),
/// so we pick from a range nothing else is likely to want.
const DISPLAY_SEARCH_START: u32 = 64;
const DISPLAY_SEARCH_END: u32 = 128;

const SERVER_START_TIMEOUT: Duration = Duration::from_secs(15);

/// How many display numbers to try before giving up. More than one because
/// picking a number and the server claiming it are not atomic, so losing the
/// race to someone else is an ordinary outcome, not a failure.
const DISPLAY_ATTEMPTS: usize = 8;

/// Display numbers this process has handed out but whose servers may not have
/// created their lock files yet.
///
/// Checking the filesystem alone is racy in exactly the case that matters:
/// two sessions starting at once (or two tests running in parallel) both see
/// `:64` free, both spawn a server on it, and the loser exits with "server
/// already running". Reserving here closes the in-process half of that, and
/// the retry loop in `start` covers the rest.
static RESERVED_DISPLAYS: Mutex<BTreeSet<u32>> = Mutex::new(BTreeSet::new());

/// A nested X server hosting the game and LunaTranslator.
///
/// Killed on drop, which also brings down the window manager thread attached
/// to it (its connection dies with the server).
pub struct XServer {
    pub display: String,
    display_number: u32,
    child: Child,
}

impl XServer {
    /// Starts an X server nested inside the current session, sized to
    /// `width`x`height`.
    ///
    /// On a Wayland session - which includes a Steam Deck's gamescope game
    /// mode - that's a rootful, fullscreen Xwayland: the compositor sees a
    /// single ordinary window, which is exactly the one surface gamescope is
    /// willing to show, and everything inside it is ours to arrange. On a
    /// plain X11 desktop it's Xephyr instead.
    pub fn start(width: u16, height: u16) -> Result<Self, NestedSessionError> {
        let mut last = NestedSessionError::NoXServer;
        for _ in 0..DISPLAY_ATTEMPTS {
            let display_number = reserve_display_number().ok_or(NestedSessionError::NoXServer)?;
            match Self::start_on(display_number, width, height) {
                Ok(server) => return Ok(server),
                Err(e) => {
                    // Whoever won the race owns that number now; ours is free
                    // to hand out again only once nothing of ours holds it.
                    release_display_number(display_number);
                    // A missing X server binary will not get better on the
                    // next number.
                    if matches!(e, NestedSessionError::NoXServer) {
                        return Err(e);
                    }
                    warn!("nested X server failed to start on :{display_number}: {e}");
                    last = e;
                }
            }
        }
        Err(last)
    }

    fn start_on(display_number: u32, width: u16, height: u16) -> Result<Self, NestedSessionError> {
        let display = format!(":{display_number}");

        // gamescope advertises its own compositor separately from whatever it
        // is itself nested inside, and in its session WAYLAND_DISPLAY is not
        // always the one we want to connect to (or set at all) - so take
        // gamescope's socket when it names one.
        let gamescope_socket = std::env::var_os("GAMESCOPE_WAYLAND_DISPLAY");
        let wayland_socket = gamescope_socket.or_else(|| std::env::var_os("WAYLAND_DISPLAY"));

        let mut command = if let Some(wayland_socket) = wayland_socket {
            let binary = find_binary("Xwayland").ok_or(NestedSessionError::NoXServer)?;
            let mut command = Command::new(binary);
            command.env("WAYLAND_DISPLAY", wayland_socket);
            command.args([
                display.as_str(),
                // Rootful (one window on the parent compositor holding the
                // whole X root, rather than a surface per X toplevel - we
                // want to do the arranging, not the host compositor) is
                // Xwayland's default; `-rootless` is the opt-in, and there is
                // no `-rootful` to pass. Passing one is a usage error that
                // makes Xwayland exit immediately.
                "-fullscreen",
                "-geometry",
                &format!("{width}x{height}"),
                // Keep the server alive across the gap between the game
                // exiting and LunaTranslator being torn down (and vice versa).
                "-noreset",
            ]);
            command
        } else {
            let binary = find_binary("Xephyr").ok_or(NestedSessionError::NoXServer)?;
            let mut command = Command::new(binary);
            command.args([
                display.as_str(),
                "-screen",
                &format!("{width}x{height}"),
                "-resizeable",
                "-noreset",
            ]);
            command
        };

        command
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            // The X server must NOT inherit the nested DISPLAY - it has to
            // connect to the *host* session to present its own window.
            .env_remove("DISPLAY");
        sanitize_external_command(&mut command);

        // Tie the server's lifetime to ours at the kernel level. `Drop` only
        // runs on a graceful exit: if Drop is SIGKILLed, panics, or is killed
        // from a terminal, the X server would otherwise be orphaned and sit
        // there holding a display number forever.
        unsafe {
            command.pre_exec(|| {
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
                // Guard the race where the parent died between fork and here.
                if libc::getppid() == 1 {
                    libc::raise(libc::SIGTERM);
                }
                Ok(())
            });
        }

        info!("starting nested X server on {display} ({width}x{height})");
        let child = command.spawn()?;
        let mut server = XServer {
            display,
            display_number,
            child,
        };
        server.wait_until_ready()?;
        Ok(server)
    }

    /// Waits for the server's socket to appear, giving up if the server exits
    /// first (e.g. Xwayland refusing to start because the compositor went
    /// away) rather than sitting out the whole timeout.
    fn wait_until_ready(&mut self) -> Result<(), NestedSessionError> {
        let socket = socket_path(&self.display);
        let deadline = Instant::now() + SERVER_START_TIMEOUT;
        while Instant::now() < deadline {
            if let Some(status) = self.child.try_wait()? {
                warn!("nested X server exited during startup with {status}");
                return Err(NestedSessionError::ServerTimeout(self.display.clone()));
            }
            if socket.exists() {
                // The socket exists a moment before the server is listening
                // on it; a short settle beats a connect-retry loop here,
                // since the window manager's own connect retries anyway.
                sleep(Duration::from_millis(150));
                return Ok(());
            }
            sleep(Duration::from_millis(50));
        }
        Err(NestedSessionError::ServerTimeout(self.display.clone()))
    }
}

impl Drop for XServer {
    fn drop(&mut self) {
        if let Err(e) = self.child.kill() {
            warn!("failed to stop nested X server on {}: {e}", self.display);
        }
        let _ = self.child.wait();
        release_display_number(self.display_number);
        info!("nested X server on {} stopped", self.display);
    }
}

fn socket_path(display: &str) -> PathBuf {
    PathBuf::from(format!(
        "/tmp/.X11-unix/X{}",
        display.trim_start_matches(':')
    ))
}

/// Claims a display number with neither a socket nor a lock file, and not
/// already handed out by this process. Both files are checked: a crashed
/// server can leave a stale lock behind that a new one will refuse to start
/// over.
fn reserve_display_number() -> Option<u32> {
    let mut reserved = RESERVED_DISPLAYS.lock().ok()?;
    let number = (DISPLAY_SEARCH_START..DISPLAY_SEARCH_END).find(|number| {
        !reserved.contains(number)
            && !PathBuf::from(format!("/tmp/.X11-unix/X{number}")).exists()
            && !PathBuf::from(format!("/tmp/.X{number}-lock")).exists()
    })?;
    reserved.insert(number);
    Some(number)
}

fn release_display_number(number: u32) {
    if let Ok(mut reserved) = RESERVED_DISPLAYS.lock() {
        reserved.remove(&number);
    }
}

/// Resolves a binary against PATH ourselves rather than relying on the
/// spawn to do it, so a missing X server surfaces as `NoXServer` (with a
/// useful message) instead of a bare ENOENT from `spawn`.
fn find_binary(name: &str) -> Option<PathBuf> {
    // Running inside the AppImage, PATH starts with the bundle's own
    // directories - harmless here (we bundle no X server for them to shadow),
    // but the host's own bin dirs are appended as a fallback in case PATH is
    // unset or unusual.
    let path = std::env::var_os("PATH").unwrap_or_else(|| OsString::from("/usr/bin:/bin"));
    std::env::split_paths(&path)
        .chain([PathBuf::from("/usr/bin"), PathBuf::from("/bin")])
        .map(|dir| dir.join(name))
        .find(|candidate| is_executable(candidate))
}

fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    std::fs::metadata(path)
        .map(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}
