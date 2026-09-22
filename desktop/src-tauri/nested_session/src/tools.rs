//! The window manager, compositor and taskbar that run inside the nested
//! X server: openbox, picom and tint2.
//!
//! None of them is ours. They're built from the submodules in
//! `desktop/vendor` (see `build.sh` there) into a relocatable prefix that the
//! AppImage carries at `usr/libexec/drop-tools/nested-session`, and each runs
//! as its own process - which is also what keeps their GPL/MPL licences
//! from reaching Drop's own binary.
//!
//! Each one is only considered started once it's visibly doing its job on
//! the display, not merely once it's been spawned:
//! - openbox: `_NET_SUPPORTING_WM_CHECK` is set on the root.
//! - picom: something owns `_NET_WM_CM_S<screen>`. This is also the thing Qt
//!   checks before it will draw a translucent window at all.
//! - tint2: `_NET_WORKAREA` has shrunk to leave room for it, i.e. openbox has
//!   seen its strut and maximized windows will stop above the taskbar.

use std::{
    fs,
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread::sleep,
    time::{Duration, Instant},
};

use log::{info, warn};
use utils::external_open::sanitize_external_command;
use x11rb::{
    connection::Connection,
    protocol::xproto::{AtomEnum, ConnectionExt as _, Window},
    rust_connection::RustConnection,
};

use crate::{
    error::NestedSessionError,
    server::{die_with_parent, terminate},
};

/// Overrides where the tools are looked for - mainly for running a host
/// build against `desktop/vendor/out/nested-session`.
pub const TOOLS_ENV: &str = "DROP_NESTED_SESSION_TOOLS";

/// Where `build_appimage.sh` puts the prefix, relative to `$APPDIR`.
const APPIMAGE_TOOLS_DIR: &str = "usr/libexec/drop-tools/nested-session";

const READY_TIMEOUT: Duration = Duration::from_secs(10);

/// A tool that keeps dying is left dead after this many restarts inside
/// `RESTART_WINDOW`, rather than being respawned in a tight loop forever.
const MAX_RESTARTS: usize = 5;
const RESTART_WINDOW: Duration = Duration::from_secs(60);

const RC_XML: &str = include_str!("../config/rc.xml");
const MENU_XML: &str = include_str!("../config/menu.xml");
const PICOM_CONF: &str = include_str!("../config/picom.conf");
const TINT2RC: &str = include_str!("../config/tint2rc");
const THEME: &[(&str, &str)] = &[
    ("themerc", include_str!("../config/theme/themerc")),
    ("close.xbm", include_str!("../config/theme/close.xbm")),
    ("max.xbm", include_str!("../config/theme/max.xbm")),
    (
        "max_toggled.xbm",
        include_str!("../config/theme/max_toggled.xbm"),
    ),
    ("iconify.xbm", include_str!("../config/theme/iconify.xbm")),
];

#[derive(Clone, Copy, Debug)]
enum Tool {
    Openbox,
    Picom,
    Tint2,
}

impl Tool {
    /// Start order: the compositor wants a window manager to already be
    /// framing windows, and the taskbar's strut is applied by the window
    /// manager.
    const ALL: [Tool; 3] = [Tool::Openbox, Tool::Picom, Tool::Tint2];

    fn name(self) -> &'static str {
        match self {
            Tool::Openbox => "openbox",
            Tool::Picom => "picom",
            Tool::Tint2 => "tint2",
        }
    }

    fn spawn(self, bin_dir: &Path, config: &Path, display: &str) -> std::io::Result<Child> {
        let mut command = Command::new(bin_dir.join(self.name()));
        match self {
            Tool::Openbox => {
                command
                    .arg("--config-file")
                    .arg(config.join("openbox/rc.xml"))
                    // menu.xml is found relative to the config home, the
                    // theme relative to the data home.
                    .env("XDG_CONFIG_HOME", config)
                    .env("XDG_DATA_HOME", config);
            }
            Tool::Picom => {
                command.arg("--config").arg(config.join("picom.conf"));
            }
            Tool::Tint2 => {
                command.arg("-c").arg(config.join("tint2rc"));
            }
        }

        // The tools come with their own libraries (RUNPATH $ORIGIN/../lib),
        // and sanitize_external_command strips the AppImage's
        // LD_LIBRARY_PATH, which would otherwise take precedence over it.
        sanitize_external_command(&mut command);
        command
            .env("DISPLAY", display)
            .env_remove("WAYLAND_DISPLAY")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        die_with_parent(&mut command);

        let mut child = command.spawn()?;
        if let Some(stdout) = child.stdout.take() {
            forward_output(self.name(), stdout);
        }
        if let Some(stderr) = child.stderr.take() {
            forward_output(self.name(), stderr);
        }
        info!(
            "started nested session {} (pid {})",
            self.name(),
            child.id()
        );
        Ok(child)
    }

    fn is_ready(self, display: &Display) -> Result<bool, NestedSessionError> {
        match self {
            Tool::Openbox => Ok(display
                .root_property32(display.atom("_NET_SUPPORTING_WM_CHECK")?, AtomEnum::WINDOW)?
                .first()
                .is_some_and(|&window| window != x11rb::NONE)),
            Tool::Picom => {
                let selection = display.atom(&format!("_NET_WM_CM_S{}", display.screen))?;
                let owner = display
                    .conn
                    .get_selection_owner(selection)
                    .map_err(x11)?
                    .reply()
                    .map_err(x11)?
                    .owner;
                Ok(owner != x11rb::NONE)
            }
            Tool::Tint2 => {
                let work_area =
                    display.root_property32(display.atom("_NET_WORKAREA")?, AtomEnum::CARDINAL)?;
                Ok(work_area.len() >= 4 && work_area[3] < u32::from(display.height))
            }
        }
    }
}

/// Their output goes to Drop's own log, since on a Deck in game mode there's
/// no terminal to have seen it in.
fn forward_output(name: &'static str, stream: impl Read + Send + 'static) {
    let spawned = std::thread::Builder::new()
        .name(format!("nested-{name}-output"))
        .spawn(move || {
            for line in BufReader::new(stream).lines().map_while(Result::ok) {
                info!("[{name}] {line}");
            }
        });
    if let Err(e) = spawned {
        warn!("could not forward {name}'s output: {e}");
    }
}

fn x11<E: std::fmt::Display>(error: E) -> NestedSessionError {
    NestedSessionError::X11(error.to_string())
}

/// Just enough of a connection to the nested display to see whether the
/// tools are up.
struct Display {
    conn: RustConnection,
    screen: usize,
    root: Window,
    height: u16,
}

impl Display {
    fn connect(display: &str) -> Result<Self, NestedSessionError> {
        let (conn, screen) = RustConnection::connect(Some(display)).map_err(x11)?;
        let root = &conn.setup().roots[screen];
        let (root, height) = (root.root, root.height_in_pixels);
        Ok(Display {
            conn,
            screen,
            root,
            height,
        })
    }

    fn atom(&self, name: &str) -> Result<u32, NestedSessionError> {
        Ok(self
            .conn
            .intern_atom(false, name.as_bytes())
            .map_err(x11)?
            .reply()
            .map_err(x11)?
            .atom)
    }

    fn root_property32(
        &self,
        property: u32,
        kind: AtomEnum,
    ) -> Result<Vec<u32>, NestedSessionError> {
        let reply = self
            .conn
            .get_property(false, self.root, property, kind, 0, 64)
            .map_err(x11)?
            .reply()
            .map_err(x11)?;
        Ok(reply.value32().map(Iterator::collect).unwrap_or_default())
    }
}

/// Finds the bundled tools: `$DROP_NESTED_SESSION_TOOLS`, then the
/// AppImage's copy, then (debug builds only) the output of
/// `desktop/vendor/build.sh` in this checkout.
fn locate() -> Result<PathBuf, NestedSessionError> {
    let mut candidates = Vec::new();
    if let Some(dir) = std::env::var_os(TOOLS_ENV) {
        candidates.push(PathBuf::from(dir));
    }
    if let Some(appdir) = std::env::var_os("APPDIR") {
        candidates.push(PathBuf::from(appdir).join(APPIMAGE_TOOLS_DIR));
    }
    #[cfg(debug_assertions)]
    candidates.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vendor/out/nested-session"));

    candidates
        .iter()
        .find(|dir| {
            Tool::ALL
                .iter()
                .all(|tool| crate::server::is_executable(&dir.join("bin").join(tool.name())))
        })
        .cloned()
        .ok_or(NestedSessionError::MissingTools(candidates))
}

/// The generated configuration, removed again when the session ends.
struct ConfigDir(PathBuf);

impl ConfigDir {
    fn create(display: &str) -> Result<Self, NestedSessionError> {
        let base = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .filter(|dir| dir.is_dir())
            .unwrap_or_else(std::env::temp_dir);
        let dir = base.join(format!(
            "drop-nested-session-{}-{}",
            std::process::id(),
            display.trim_start_matches(':')
        ));
        // Left over from a crashed run with a recycled pid, at worst.
        let _ = fs::remove_dir_all(&dir);

        let theme = dir.join("themes/Drop/openbox-3");
        fs::create_dir_all(dir.join("openbox"))?;
        fs::create_dir_all(&theme)?;
        fs::write(dir.join("openbox/rc.xml"), RC_XML)?;
        fs::write(dir.join("openbox/menu.xml"), MENU_XML)?;
        fs::write(dir.join("picom.conf"), PICOM_CONF)?;
        fs::write(dir.join("tint2rc"), TINT2RC)?;
        for (name, contents) in THEME {
            fs::write(theme.join(name), contents)?;
        }
        Ok(ConfigDir(dir))
    }
}

impl Drop for ConfigDir {
    fn drop(&mut self) {
        if let Err(e) = fs::remove_dir_all(&self.0) {
            warn!(
                "could not remove nested session config {}: {e}",
                self.0.display()
            );
        }
    }
}

struct Running {
    tool: Tool,
    child: Child,
    restarts: Vec<Instant>,
    given_up: bool,
}

impl Drop for Running {
    fn drop(&mut self) {
        terminate(
            &mut self.child,
            &format!("nested session {}", self.tool.name()),
        );
    }
}

/// The three tools, running on one display. Dropping this stops them.
pub(crate) struct Tools {
    /// In start order; stopped in reverse.
    running: Vec<Running>,
    bin_dir: PathBuf,
    display: String,
    // Declared last so it outlives the processes reading from it.
    config: ConfigDir,
}

impl Tools {
    /// Starts all three on `display` and returns once each is actually
    /// working. Must be called from the thread that will own the session:
    /// they die with it (see `die_with_parent`).
    pub(crate) fn start(display: &str) -> Result<Self, NestedSessionError> {
        let bin_dir = locate()?.join("bin");
        info!("nested session tools: {}", bin_dir.display());
        let config = ConfigDir::create(display)?;
        let connection = Display::connect(display)?;

        let mut tools = Tools {
            running: Vec::new(),
            bin_dir,
            display: display.to_owned(),
            config,
        };
        for tool in Tool::ALL {
            let child = tool.spawn(&tools.bin_dir, &tools.config.0, display)?;
            // Pushed before waiting, so a failure below still stops it.
            tools.running.push(Running {
                tool,
                child,
                restarts: Vec::new(),
                given_up: false,
            });
            let running = tools.running.last_mut().expect("just pushed");
            wait_until_ready(running, &connection)?;
        }
        Ok(tools)
    }

    /// Restarts whichever tools have died since the last call. Losing
    /// openbox mid-game would leave every window frameless and immovable,
    /// losing tint2 would make minimised windows unrecoverable on a Deck.
    pub(crate) fn supervise(&mut self) {
        for running in &mut self.running {
            let status = match running.child.try_wait() {
                Ok(Some(status)) => status,
                Ok(None) => continue,
                Err(e) => {
                    warn!("could not poll nested session {}: {e}", running.tool.name());
                    continue;
                }
            };
            if running.given_up {
                continue;
            }

            let now = Instant::now();
            running
                .restarts
                .retain(|at| now.duration_since(*at) < RESTART_WINDOW);
            if running.restarts.len() >= MAX_RESTARTS {
                warn!(
                    "nested session {} keeps dying (last: {status}); not restarting it again",
                    running.tool.name()
                );
                running.given_up = true;
                continue;
            }

            warn!(
                "nested session {} exited with {status}; restarting it",
                running.tool.name()
            );
            match running
                .tool
                .spawn(&self.bin_dir, &self.config.0, &self.display)
            {
                Ok(child) => {
                    running.child = child;
                    running.restarts.push(now);
                }
                Err(e) => warn!(
                    "could not restart nested session {}: {e}",
                    running.tool.name()
                ),
            }
        }
    }
}

impl Drop for Tools {
    fn drop(&mut self) {
        while let Some(running) = self.running.pop() {
            drop(running);
        }
    }
}

fn wait_until_ready(running: &mut Running, display: &Display) -> Result<(), NestedSessionError> {
    let tool = running.tool.name();
    let deadline = Instant::now() + READY_TIMEOUT;
    while Instant::now() < deadline {
        if let Some(status) = running.child.try_wait()? {
            return Err(NestedSessionError::ToolFailed {
                tool,
                reason: format!("exited with {status}"),
            });
        }
        if running.tool.is_ready(display)? {
            info!("nested session {tool} is ready");
            return Ok(());
        }
        sleep(Duration::from_millis(50));
    }
    Err(NestedSessionError::ToolFailed {
        tool,
        reason: format!("not ready after {}s", READY_TIMEOUT.as_secs()),
    })
}
