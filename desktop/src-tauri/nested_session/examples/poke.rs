//! Drives focus changes against a nested session, so window-manager
//! behaviour that normally needs a human clicking can be reproduced and
//! logged.
//!
//!     cargo run -p nested_session --example poke -- :70 list
//!     cargo run -p nested_session --example poke -- :70 activate 1
//!     cargo run -p nested_session --example poke -- :70 minimise 0
//!     cargo run -p nested_session --example poke -- :70 restore 0
//!
//! `activate` sends the same `_NET_ACTIVE_WINDOW` message a pager or a click
//! would, which is the path a "click the other window" bug travels.
//! `minimise` sends the ICCCM `WM_CHANGE_STATE` a Wine game sends itself when
//! it loses activation, and `restore` clicks that window's taskbar entry -
//! together they reproduce the "tab away and it's gone" bug without a human.

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("nested sessions are Linux-only");
}

#[cfg(target_os = "linux")]
fn main() {
    use x11rb::{
        connection::Connection,
        protocol::xproto::{
            AtomEnum, BUTTON_PRESS_EVENT, ButtonPressEvent, ClientMessageEvent, ConnectionExt as _,
            EventMask,
        },
        rust_connection::RustConnection,
    };

    let args: Vec<String> = std::env::args().skip(1).collect();
    let display = args.first().cloned().unwrap_or_else(|| ":0".to_owned());
    let action = args.get(1).cloned().unwrap_or_else(|| "list".to_owned());

    let (conn, screen) = RustConnection::connect(Some(&display)).expect("connect");
    let root = conn.setup().roots[screen].root;

    // Force the server to process everything queued before this process
    // exits: `flush` only pushes bytes at the socket, and a connection that
    // closes on their heels can have them dropped on the floor - which shows
    // up as an action that silently does nothing, intermittently.
    let sync = || {
        conn.get_input_focus()
            .expect("sync")
            .reply()
            .expect("sync reply");
    };

    let atom = |name: &str| {
        conn.intern_atom(false, name.as_bytes())
            .expect("intern")
            .reply()
            .expect("intern reply")
            .atom
    };

    let clients: Vec<u32> = conn
        .get_property(
            false,
            root,
            atom("_NET_CLIENT_LIST"),
            AtomEnum::WINDOW,
            0,
            64,
        )
        .expect("client list")
        .reply()
        .expect("client list reply")
        .value32()
        .expect("client list is windows")
        .collect();

    for (index, window) in clients.iter().enumerate() {
        let title = conn
            .get_property(false, *window, AtomEnum::WM_NAME, AtomEnum::STRING, 0, 64)
            .ok()
            .and_then(|cookie| cookie.reply().ok())
            .map(|reply| String::from_utf8_lossy(&reply.value).to_string())
            .unwrap_or_default();
        println!("{index}: {window} '{title}'");
    }

    if action == "tree" {
        let tree = conn
            .query_tree(root)
            .expect("tree")
            .reply()
            .expect("tree reply");
        for child in tree.children {
            let attributes = conn
                .get_window_attributes(child)
                .ok()
                .and_then(|cookie| cookie.reply().ok());
            let geometry = conn
                .get_geometry(child)
                .ok()
                .and_then(|cookie| cookie.reply().ok());
            let kids = conn
                .query_tree(child)
                .ok()
                .and_then(|cookie| cookie.reply().ok())
                .map(|reply| reply.children)
                .unwrap_or_default();
            println!(
                "win {child} geom={:?} mapped={:?} or={:?} children={:?}",
                geometry.map(|g| (g.x, g.y, g.width, g.height)),
                attributes.as_ref().map(|a| a.map_state),
                attributes.as_ref().map(|a| a.override_redirect),
                kids
            );
            for kid in kids {
                if let Some(g) = conn.get_geometry(kid).ok().and_then(|c| c.reply().ok()) {
                    println!("    child {kid} geom={:?}", (g.x, g.y, g.width, g.height));
                }
            }
        }
    }

    // The taskbar is override-redirect and never published, so find it by
    // its shape: full width, flush with the bottom edge of the root.
    let find_taskbar = || {
        let root_geometry = conn
            .get_geometry(root)
            .expect("root geometry")
            .reply()
            .expect("root geometry reply");
        conn.query_tree(root)
            .expect("tree")
            .reply()
            .expect("tree reply")
            .children
            .into_iter()
            .find(|child| {
                let override_redirect = conn
                    .get_window_attributes(*child)
                    .ok()
                    .and_then(|cookie| cookie.reply().ok())
                    .is_some_and(|attributes| attributes.override_redirect);
                let geometry = conn.get_geometry(*child).ok().and_then(|c| c.reply().ok());
                override_redirect
                    && geometry.is_some_and(|g| {
                        g.width == root_geometry.width
                            && g.height > 0
                            && i32::from(g.y) + i32::from(g.height)
                                == i32::from(root_geometry.height)
                    })
            })
    };

    if action == "minimise" || action == "minimize" {
        let index: usize = args.get(2).and_then(|i| i.parse().ok()).unwrap_or(0);
        let Some(window) = clients.get(index) else {
            eprintln!("no window at index {index}");
            return;
        };
        println!("minimising {index}: {window}");
        // data[0] == IconicState.
        let event =
            ClientMessageEvent::new(32, *window, atom("WM_CHANGE_STATE"), [3u32, 0, 0, 0, 0]);
        conn.send_event(
            false,
            root,
            EventMask::SUBSTRUCTURE_NOTIFY | EventMask::SUBSTRUCTURE_REDIRECT,
            event,
        )
        .expect("send");
        conn.flush().expect("flush");
        sync();
    }

    if action == "restore" {
        let index: usize = args.get(2).and_then(|i| i.parse().ok()).unwrap_or(0);
        let Some(taskbar) = find_taskbar() else {
            eprintln!("no taskbar found on {display}");
            return;
        };
        let taskbar_geometry = conn
            .get_geometry(taskbar)
            .expect("taskbar geometry")
            .reply()
            .expect("taskbar geometry reply");
        // Entries divide the strip evenly, so the middle of entry n is where
        // a finger would land.
        let entry = taskbar_geometry.width as i32 / clients.len().max(1) as i32;
        let x = (entry * index as i32 + entry / 2) as i16;
        println!("clicking taskbar entry {index} at x={x} on {taskbar}");
        let click = ButtonPressEvent {
            response_type: BUTTON_PRESS_EVENT,
            detail: 1,
            sequence: 0,
            time: x11rb::CURRENT_TIME,
            root,
            event: taskbar,
            child: x11rb::NONE,
            root_x: x,
            root_y: taskbar_geometry.y + 10,
            event_x: x,
            event_y: 10,
            state: 0u16.into(),
            same_screen: true,
        };
        conn.send_event(false, taskbar, EventMask::BUTTON_PRESS, click)
            .expect("send");
        conn.flush().expect("flush");
        sync();
    }

    if action == "activate" {
        let index: usize = args.get(2).and_then(|i| i.parse().ok()).unwrap_or(0);
        let Some(window) = clients.get(index) else {
            eprintln!("no window at index {index}");
            return;
        };
        println!("activating {index}: {window}");
        let event =
            ClientMessageEvent::new(32, *window, atom("_NET_ACTIVE_WINDOW"), [2u32, 0, 0, 0, 0]);
        conn.send_event(
            false,
            root,
            EventMask::SUBSTRUCTURE_NOTIFY | EventMask::SUBSTRUCTURE_REDIRECT,
            event,
        )
        .expect("send");
        conn.flush().expect("flush");
        sync();
    }
}
