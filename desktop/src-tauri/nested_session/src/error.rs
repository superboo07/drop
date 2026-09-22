use std::{
    fmt::Display,
    io::{self, Error},
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
    /// Another window manager already owns the nested display's root window.
    WindowManagerConflict,
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
            NestedSessionError::WindowManagerConflict => {
                "Another window manager is already running on the nested display".to_owned()
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
