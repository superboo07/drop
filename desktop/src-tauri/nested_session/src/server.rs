use std::{
    collections::BTreeSet,
    ffi::OsString,
    fs::File,
    io::Read as _,
    os::{
        fd::{AsRawFd as _, FromRawFd as _, OwnedFd, RawFd},
        unix::process::CommandExt as _,
    },
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
/// Killed on drop.
pub struct XServer {
    pub display: String,
    display_number: u32,
    child: Child,
    /// Read end of the server's `-displayfd`: it writes the display number
    /// here once it's accepting connections. Only needed until then.
    ready: Option<File>,
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
        let (ready_read, ready_write) = ready_pipe()?;
        let ready_fd = ready_write.as_raw_fd().to_string();

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
                "-displayfd",
                &ready_fd,
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
                "-displayfd",
                &ready_fd,
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

        die_with_parent(&mut command);
        inherit_fd(&mut command, ready_write.as_raw_fd());

        info!("starting nested X server on {display} ({width}x{height})");
        let child = command.spawn()?;
        // The server holds its own copy now; ours has to go, or a server that
        // dies during startup would never close the pipe.
        drop(ready_write);
        let mut server = XServer {
            display,
            display_number,
            child,
            ready: Some(ready_read),
        };
        server.wait_until_ready()?;
        Ok(server)
    }

    /// Waits for the server to report, over `-displayfd`, that it is
    /// accepting connections - giving up if it exits first (e.g. Xwayland
    /// refusing to start because the compositor went away) or doesn't get
    /// there in `SERVER_START_TIMEOUT`.
    ///
    /// Not "the socket file exists": the server creates that early in its
    /// startup, and under a nested compositor it can then spend a long time
    /// (seconds, on a loaded Steam Deck) getting its output set up before it
    /// serves anyone. A client that connects in that gap doesn't fail - it
    /// blocks, with no timeout, and with it whatever thread started the
    /// session.
    fn wait_until_ready(&mut self) -> Result<(), NestedSessionError> {
        let mut ready = self.ready.take().expect("waited for readiness twice");
        let mut reply = Vec::new();
        let deadline = Instant::now() + SERVER_START_TIMEOUT;
        while Instant::now() < deadline {
            if let Some(status) = self.child.try_wait()? {
                warn!("nested X server exited during startup with {status}");
                return Err(NestedSessionError::ServerTimeout(self.display.clone()));
            }
            if !poll_readable(&ready, Duration::from_millis(50))? {
                continue;
            }
            let mut chunk = [0u8; 16];
            match ready.read(&mut chunk)? {
                // Closed without a word: the server is on its way out, which
                // the next try_wait will report.
                0 => sleep(Duration::from_millis(50)),
                read => reply.extend_from_slice(&chunk[..read]),
            }
            if reply.ends_with(b"\n") {
                let reported = String::from_utf8_lossy(&reply);
                if reported.trim() != self.display_number.to_string() {
                    warn!(
                        "nested X server was started on {} but reports display :{}",
                        self.display,
                        reported.trim()
                    );
                }
                return Ok(());
            }
        }
        Err(NestedSessionError::ServerTimeout(self.display.clone()))
    }
}

impl XServer {
    /// Whether the server has gone away underneath us - the compositor it
    /// was nested in exiting, typically.
    pub fn has_exited(&mut self) -> bool {
        !matches!(self.child.try_wait(), Ok(None))
    }
}

impl Drop for XServer {
    fn drop(&mut self) {
        terminate(
            &mut self.child,
            &format!("nested X server on {}", self.display),
        );
        release_display_number(self.display_number);
        info!("nested X server on {} stopped", self.display);
    }
}

/// The pipe behind `-displayfd`. Both ends are close-on-exec; the write end
/// is made inheritable only in the server's own child (see `inherit_fd`), so
/// no other process Drop spawns meanwhile holds it open.
fn ready_pipe() -> Result<(File, OwnedFd), NestedSessionError> {
    let mut fds: [RawFd; 2] = [-1; 2];
    if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) } != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let (read, write) = unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) };
    Ok((File::from(read), write))
}

/// Lets `fd` survive the exec into `command`.
fn inherit_fd(command: &mut Command, fd: RawFd) {
    unsafe {
        command.pre_exec(move || {
            if libc::fcntl(fd, libc::F_SETFD, 0) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

/// Whether `file` has something to read (or has been closed) within
/// `timeout`.
fn poll_readable(file: &File, timeout: Duration) -> std::io::Result<bool> {
    let mut poll = libc::pollfd {
        fd: file.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    let timeout = libc::c_int::try_from(timeout.as_millis()).unwrap_or(libc::c_int::MAX);
    match unsafe { libc::poll(&mut poll, 1, timeout) } {
        -1 => {
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::Interrupted {
                Ok(false)
            } else {
                Err(error)
            }
        }
        0 => Ok(false),
        _ => Ok(true),
    }
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

pub(crate) fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    std::fs::metadata(path)
        .map(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

/// How long a child gets to exit on SIGTERM before it's SIGKILLed.
const TERMINATE_GRACE: Duration = Duration::from_secs(2);

/// Stops a child politely: SIGTERM, then SIGKILL if it hasn't gone after
/// `TERMINATE_GRACE`. Not `Child::kill` straight away - that's SIGKILL, and
/// an X server killed that way leaves its `/tmp/.X<n>-lock` and socket
/// behind, which permanently takes that display number out of the pool
/// `reserve_display_number` picks from.
pub(crate) fn terminate(child: &mut Child, what: &str) {
    if matches!(child.try_wait(), Ok(Some(_))) {
        return;
    }
    if let Ok(pid) = libc::pid_t::try_from(child.id()) {
        unsafe {
            libc::kill(pid, libc::SIGTERM);
        }
        let deadline = Instant::now() + TERMINATE_GRACE;
        while Instant::now() < deadline {
            if matches!(child.try_wait(), Ok(Some(_))) {
                return;
            }
            sleep(Duration::from_millis(20));
        }
        warn!("{what} ignored SIGTERM; killing it");
    }
    if let Err(e) = child.kill() {
        warn!("failed to stop {what}: {e}");
    }
    let _ = child.wait();
}

/// Ties a child's lifetime to ours at the kernel level. `Drop` only runs on a
/// graceful exit: if Drop is SIGKILLed, panics, or is killed from a terminal,
/// the X server and the tools in it would otherwise be orphaned, the server
/// holding a display number forever.
///
/// PR_SET_PDEATHSIG fires when the *thread* that forked the child exits, not
/// the process. Anything spawned with this must be spawned from a thread that
/// lives as long as the child should - which is why the whole session is run
/// from its own thread (see `NestedSession::start`) rather than from whatever
/// thread asked for it.
pub(crate) fn die_with_parent(command: &mut Command) {
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
}
