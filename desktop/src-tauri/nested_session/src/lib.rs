//! A nested X11 session for running a game and LunaTranslator side by side.
//!
//! The problem this solves is specific to the Steam Deck's game mode:
//! gamescope composites exactly one window, so a game and a translator
//! running next to it are two things the user can only ever look at one at a
//! time. Putting both inside an X server of our own turns them into two
//! ordinary windows on one surface - and that surface is the single window
//! gamescope is happy to show.
//!
//! Inside it run openbox (floating windows), picom (compositing, which is
//! what lets LunaTranslator's window be see-through) and tint2 (a taskbar,
//! so a window that minimised itself can be brought back without a
//! keyboard) - see `tools`.
//!
//! It is just as usable on a desktop, where the nested server is a window
//! like any other.
//!
//! Everything here is Linux-only; on other platforms the crate is an empty
//! shell so the workspace still builds.

pub mod error;

#[cfg(target_os = "linux")]
mod cursor;
#[cfg(target_os = "linux")]
mod server;
#[cfg(target_os = "linux")]
mod tools;

#[cfg(target_os = "linux")]
pub use linux::NestedSession;
#[cfg(target_os = "linux")]
pub use tools::TOOLS_ENV;

#[cfg(target_os = "linux")]
mod linux {
    use std::{
        sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel},
        thread::JoinHandle,
        time::Duration,
    };

    use log::{error, warn};

    use crate::{error::NestedSessionError, server::XServer, tools::Tools};

    /// How often the session thread checks on the X server and the tools.
    const SUPERVISE_INTERVAL: Duration = Duration::from_millis(500);

    /// A running nested session: an X server plus the window manager,
    /// compositor and taskbar inside it. Dropping this tears it all down.
    pub struct NestedSession {
        display: String,
        /// Dropped (disconnected) to tell the session thread to stop.
        stop: Option<Sender<()>>,
        thread: Option<JoinHandle<()>>,
        /// See `draw_cursor`. Ends by itself once the server is gone.
        cursor: Option<JoinHandle<()>>,
    }

    impl NestedSession {
        /// Starts a nested X server sized `width`x`height` with everything
        /// running inside it. Returns once windows opened on it will actually
        /// be managed, composited and listed in the taskbar.
        pub fn start(width: u16, height: u16) -> Result<Self, NestedSessionError> {
            let (ready_sender, ready_receiver) = channel();
            let (stop, stop_receiver) = channel();

            // Everything is spawned from this one thread, which lives exactly
            // as long as the session: the children are tied to the thread
            // that forked them (see `server::die_with_parent`), so spawning
            // them from the caller's thread - a pooled worker, possibly -
            // would kill the session whenever that thread happened to exit.
            let thread = std::thread::Builder::new()
                .name("nested-session".to_owned())
                .spawn(move || run(width, height, &ready_sender, &stop_receiver))?;

            match ready_receiver.recv() {
                Ok(Ok(display)) => Ok(NestedSession {
                    display,
                    stop: Some(stop),
                    thread: Some(thread),
                    cursor: None,
                }),
                Ok(Err(e)) => {
                    let _ = thread.join();
                    Err(e)
                }
                Err(_) => {
                    let _ = thread.join();
                    Err(NestedSessionError::ToolFailed {
                        tool: "session",
                        reason: "the session thread died while starting".to_owned(),
                    })
                }
            }
        }

        /// The `DISPLAY` value to hand to anything that should appear inside
        /// this session.
        pub fn display(&self) -> &str {
            &self.display
        }

        /// Draws the pointer into the session itself, for as long as it
        /// runs.
        ///
        /// For gamescope, which draws no cursor over the session otherwise;
        /// see `cursor` for why. Not for a desktop session: the host
        /// compositor already shows the session's cursor there, and this
        /// would be a second one.
        pub fn draw_cursor(&mut self) -> Result<(), NestedSessionError> {
            if self.cursor.is_none() {
                self.cursor = Some(crate::cursor::spawn(self.display.clone())?);
            }
            Ok(())
        }
    }

    impl Drop for NestedSession {
        fn drop(&mut self) {
            drop(self.stop.take());
            if let Some(thread) = self.thread.take()
                && thread.join().is_err()
            {
                warn!("nested session thread panicked");
            }
            // After the server: its connection closing is what ends it.
            if let Some(cursor) = self.cursor.take()
                && cursor.join().is_err()
            {
                warn!("nested session cursor mirror panicked");
            }
        }
    }

    fn run(
        width: u16,
        height: u16,
        ready: &Sender<Result<String, NestedSessionError>>,
        stop: &Receiver<()>,
    ) {
        let mut server = match XServer::start(width, height) {
            Ok(server) => server,
            Err(e) => {
                let _ = ready.send(Err(e));
                return;
            }
        };
        // Declared after the server, so on every path out of here the tools
        // are stopped first and the server last - the other way round, they
        // lose their connection and log a pile of errors on the way out.
        let mut tools = match Tools::start(&server.display) {
            Ok(tools) => tools,
            Err(e) => {
                let _ = ready.send(Err(e));
                return;
            }
        };
        let _ = ready.send(Ok(server.display.clone()));

        // Anything but a timeout - a message, or the sender being dropped -
        // means stop.
        while let Err(RecvTimeoutError::Timeout) = stop.recv_timeout(SUPERVISE_INTERVAL) {
            if server.has_exited() {
                // Nothing to restart the tools onto; the windows that were in
                // it are gone too.
                error!("nested X server on {} exited unexpectedly", server.display);
                break;
            }
            tools.supervise();
        }

        drop(tools);
        drop(server);
    }
}
