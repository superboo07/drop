use std::{
    fmt::Display,
    io::{self, Error},
    path::PathBuf,
    sync::Arc,
};

use serde_with::SerializeDisplay;

#[derive(SerializeDisplay, Clone)]
pub enum NestedSessionError {
    /// Neither Xwayland (Wayland sessions, including gamescope) nor Xephyr
    /// (plain X11 desktops) could be found on the host. Neither is something
    /// we can bundle in the AppImage - an X server has to match the host's
    /// own graphics stack.
    NoXServer,
    /// The X server started but never accepted a connection.
    ServerTimeout(String),
    /// The bundled openbox/picom/tint2 aren't where they should be. Only
    /// expected outside the AppImage, when `desktop/vendor/build.sh` hasn't
    /// been run (or `DROP_NESTED_SESSION_TOOLS` points somewhere wrong).
    MissingTools(Vec<PathBuf>),
    /// One of the session's tools exited, or never became ready, while the
    /// session was starting.
    ToolFailed {
        tool: &'static str,
        reason: String,
    },
    X11(String),
    IOError(Arc<Error>),
}

impl Display for NestedSessionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            NestedSessionError::NoXServer => {
                "Could not find Xwayland or Xephyr on this system. Install one of them (Xwayland on \
                 Wayland/gamescope sessions, Xephyr on X11) to use the nested window session."
                    .to_owned()
            }
            NestedSessionError::ServerTimeout(display) => {
                format!("The nested X server never came up on display {display}")
            }
            NestedSessionError::MissingTools(searched) => {
                let searched = searched
                    .iter()
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                format!(
                    "The nested session's window manager, compositor and taskbar aren't bundled \
                     with this build of Drop (looked in: {searched})"
                )
            }
            NestedSessionError::ToolFailed { tool, reason } => {
                format!("The nested session's {tool} failed to start: {reason}")
            }
            NestedSessionError::X11(error) => format!("X11 error: {error}"),
            NestedSessionError::IOError(error) => error.to_string(),
        };
        write!(f, "{s}")
    }
}

impl From<io::Error> for NestedSessionError {
    fn from(value: io::Error) -> Self {
        NestedSessionError::IOError(Arc::new(value))
    }
}
