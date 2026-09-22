//! A nested X11 session for running a game and LunaTranslator side by side.
//!
//! The problem this solves is specific to the Steam Deck's game mode:
//! gamescope composites exactly one window, so a game and a translator
//! running next to it are two things the user can only ever look at one at a
//! time. Putting both inside an X server of our own turns them into two
//! ordinary windows on one surface - and that surface is the single window
//! gamescope is happy to show.
//!
//! It is just as usable on a desktop, where the nested server is a window
//! like any other.
//!
//! Everything here is Linux-only; on other platforms the crate is an empty
//! shell so the workspace still builds.

pub mod error;

#[cfg(target_os = "linux")]
mod server;
#[cfg(target_os = "linux")]
mod wm;

#[cfg(target_os = "linux")]
pub use linux::NestedSession;

#[cfg(target_os = "linux")]
mod linux {
    use std::{sync::mpsc::channel, thread::JoinHandle};

    use log::warn;

    use crate::{error::NestedSessionError, server::XServer, wm::WindowManager};

    /// A running nested session: an X server plus the window manager driving
    /// it. Dropping this tears both down.
    pub struct NestedSession {
        /// `Option` only so `Drop` can take it and kill the server *before*
        /// joining the window manager thread - see below.
        server: Option<XServer>,
        display: String,
        window_manager: Option<JoinHandle<()>>,
    }

    impl NestedSession {
        /// Starts a nested X server sized `width`x`height` and takes over as
        /// its window manager. Returns once windows opened on it will
        /// actually be managed.
        pub fn start(width: u16, height: u16) -> Result<Self, NestedSessionError> {
            let server = XServer::start(width, height)?;
            let display = server.display.clone();

            let (ready_sender, ready_receiver) = channel();
            let window_manager = std::thread::Builder::new()
                .name("nested-session-wm".to_owned())
                .spawn(move || WindowManager::start(&display, &ready_sender))?;

            // A send error means the thread died before reporting - treat the
            // same as an explicit failure, and let the server's Drop clean up.
            match ready_receiver.recv() {
                Ok(Ok(())) => {}
                Ok(Err(e)) => return Err(e),
                Err(_) => return Err(NestedSessionError::WindowManagerConflict),
            }

            Ok(NestedSession {
                display: server.display.clone(),
                server: Some(server),
                window_manager: Some(window_manager),
            })
        }

        /// The `DISPLAY` value to hand to anything that should appear inside
        /// this session.
        pub fn display(&self) -> &str {
            &self.display
        }
    }

    impl Drop for NestedSession {
        fn drop(&mut self) {
            // Order matters: killing the server drops the window manager's
            // connection, which is what ends its event loop. Joining first
            // would deadlock.
            let window_manager = self.window_manager.take();
            drop(self.server.take());
            if let Some(window_manager) = window_manager
                && window_manager.join().is_err()
            {
                warn!("nested session window manager thread panicked");
            }
        }
    }
}
