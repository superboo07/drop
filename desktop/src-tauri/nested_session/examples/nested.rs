//! Runs a command inside a nested session, with nothing else attached.
//!
//! Rebuilding the whole Drop AppImage to look at a titlebar takes ten
//! minutes. This doesn't: it's a plain host binary that starts the same
//! nested session the real launch path does - the same Xwayland, the same
//! bundled openbox/picom/tint2 and the same generated config - and runs
//! whatever you give it inside.
//!
//!     bash desktop/vendor/build.sh      # once, and after touching vendor/
//!     cargo run -p nested_session --example nested -- xterm
//!     cargo run -p nested_session --example nested -- wine game.exe
//!     NESTED_SIZE=1280x800 cargo run -p nested_session --example nested
//!
//! With no command it just sits there, so you can point other clients at the
//! DISPLAY it prints. The tools' own output lands in this log, prefixed with
//! their names.

use std::{process::Command, thread::sleep, time::Duration};

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("nested sessions are Linux-only");
}

#[cfg(target_os = "linux")]
fn main() {
    // Deliberately verbose by default - the whole point of running this is to
    // watch what the session does.
    unsafe {
        if std::env::var_os("RUST_LOG").is_none() {
            std::env::set_var("RUST_LOG", "debug");
        }
    }
    env_logger::init();

    let (width, height) = std::env::var("NESTED_SIZE")
        .ok()
        .and_then(|size| {
            let (width, height) = size.split_once('x')?;
            Some((width.parse().ok()?, height.parse().ok()?))
        })
        .unwrap_or((1280, 800));

    let session = match nested_session::NestedSession::start(width, height) {
        Ok(session) => session,
        Err(e) => {
            eprintln!("could not start the nested session: {e}");
            std::process::exit(1);
        }
    };

    println!("nested session up on DISPLAY={}", session.display());

    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some((program, arguments)) = args.split_first() else {
        println!("no command given - attach your own clients, Ctrl-C to stop");
        loop {
            sleep(Duration::from_secs(1));
        }
    };

    let mut command = Command::new(program);
    command
        .args(arguments)
        .env("DISPLAY", session.display())
        .env_remove("WAYLAND_DISPLAY")
        .env("SDL_VIDEODRIVER", "x11");

    match command.spawn() {
        Ok(mut child) => {
            let _ = child.wait();
        }
        Err(e) => eprintln!("could not run {program}: {e}"),
    }
}
