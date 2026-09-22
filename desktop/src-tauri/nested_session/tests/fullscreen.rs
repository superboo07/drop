//! Drives a real client through the transitions that broke the first time
//! this shipped, against a real nested session.
//!
//! A game going fullscreen used to end up with a frame larger than the screen
//! and its client area pushed down under a titlebar, because the window
//! manager had no concept of fullscreen at all. These tests assert the
//! geometry a client actually ends up with, which is the only thing that
//! matters to the game rendering into it.
//!
//! Skipped (not failed) when there's no X server to nest inside - CI runners
//! generally have neither Xwayland nor Xephyr.

#![cfg(target_os = "linux")]

use std::{
    thread::sleep,
    time::{Duration, Instant},
};

use nested_session::{NestedSession, error::NestedSessionError};
use x11rb::{
    connection::Connection,
    protocol::xproto::{
        AtomEnum, ClientMessageEvent, ConnectionExt as _, CreateWindowAux, EventMask, PropMode,
        Window, WindowClass,
    },
    rust_connection::RustConnection,
    wrapper::ConnectionExt as _,
};

const ROOT_WIDTH: u16 = 1280;
const ROOT_HEIGHT: u16 = 800;
const CLIENT_WIDTH: u16 = 400;
const CLIENT_HEIGHT: u16 = 300;

const STATE_REMOVE: u32 = 0;
const STATE_ADD: u32 = 1;

/// Starts a session, or returns `None` when this machine can't host one.
fn session() -> Option<NestedSession> {
    match NestedSession::start(ROOT_WIDTH, ROOT_HEIGHT) {
        Ok(session) => Some(session),
        Err(NestedSessionError::NoXServer) => {
            eprintln!("skipping: no Xwayland or Xephyr available to nest inside");
            None
        }
        Err(e) => panic!("could not start a nested session: {e}"),
    }
}

fn atom(conn: &RustConnection, name: &str) -> u32 {
    conn.intern_atom(false, name.as_bytes())
        .expect("intern_atom")
        .reply()
        .expect("intern_atom reply")
        .atom
}

/// The client's size and its position in root coordinates - i.e. what the
/// window actually occupies on screen, chrome accounted for.
fn absolute_geometry(conn: &RustConnection, window: Window, root: Window) -> (i16, i16, u16, u16) {
    let geometry = conn
        .get_geometry(window)
        .expect("get_geometry")
        .reply()
        .expect("get_geometry reply");
    let translated = conn
        .translate_coordinates(window, root, 0, 0)
        .expect("translate_coordinates")
        .reply()
        .expect("translate_coordinates reply");
    (
        translated.dst_x,
        translated.dst_y,
        geometry.width,
        geometry.height,
    )
}

/// Polls until `predicate` holds, because every transition here is a round
/// trip through the window manager's event loop.
fn wait_for(
    conn: &RustConnection,
    window: Window,
    root: Window,
    what: &str,
    predicate: impl Fn((i16, i16, u16, u16)) -> bool,
) -> (i16, i16, u16, u16) {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut last = absolute_geometry(conn, window, root);
    while Instant::now() < deadline {
        last = absolute_geometry(conn, window, root);
        if predicate(last) {
            return last;
        }
        sleep(Duration::from_millis(50));
    }
    panic!("timed out waiting for {what}; geometry was {last:?}");
}

fn send_state(conn: &RustConnection, root: Window, window: Window, action: u32, state: u32) {
    let net_wm_state = atom(conn, "_NET_WM_STATE");
    let event = ClientMessageEvent::new(32, window, net_wm_state, [action, state, 0, 1, 0]);
    conn.send_event(
        false,
        root,
        EventMask::SUBSTRUCTURE_NOTIFY | EventMask::SUBSTRUCTURE_REDIRECT,
        event,
    )
    .expect("send_event");
    conn.flush().expect("flush");
}

fn map_test_window(conn: &RustConnection, root: Window, depth: u8, visual: u32) -> Window {
    let window = conn.generate_id().expect("generate_id");
    conn.create_window(
        depth,
        window,
        root,
        0,
        0,
        CLIENT_WIDTH,
        CLIENT_HEIGHT,
        0,
        WindowClass::INPUT_OUTPUT,
        visual,
        &CreateWindowAux::new().background_pixel(0x00ff00),
    )
    .expect("create_window");
    conn.change_property8(
        PropMode::REPLACE,
        window,
        AtomEnum::WM_NAME,
        AtomEnum::STRING,
        b"nested session test",
    )
    .expect("set WM_NAME");
    conn.map_window(window).expect("map_window");
    conn.flush().expect("flush");
    window
}

#[test]
fn fullscreen_gives_the_client_the_whole_root_and_gives_it_back() {
    let Some(session) = session() else {
        return;
    };
    let (conn, screen_number) =
        RustConnection::connect(Some(session.display())).expect("connect to nested display");
    let screen = &conn.setup().roots[screen_number].clone();
    let root = screen.root;

    let window = map_test_window(&conn, root, screen.root_depth, screen.root_visual);

    // Framed: the client keeps its requested size and is inset by the
    // titlebar, rather than being left at the root's origin.
    let windowed = wait_for(
        &conn,
        window,
        root,
        "the window to be framed",
        |(x, y, w, h)| w == CLIENT_WIDTH && h == CLIENT_HEIGHT && y > 0 && x > 0,
    );

    send_state(
        &conn,
        root,
        window,
        STATE_ADD,
        atom(&conn, "_NET_WM_STATE_FULLSCREEN"),
    );

    // The whole root, at its origin, with no chrome pushing it anywhere.
    // This is exactly what was broken before: the client used to be left at
    // its windowed size, offset under a titlebar, inside an oversized frame.
    let fullscreen = wait_for(&conn, window, root, "fullscreen", |(x, y, w, h)| {
        x == 0 && y == 0 && w == ROOT_WIDTH && h == ROOT_HEIGHT
    });
    assert_eq!(fullscreen, (0, 0, ROOT_WIDTH, ROOT_HEIGHT));

    send_state(
        &conn,
        root,
        window,
        STATE_REMOVE,
        atom(&conn, "_NET_WM_STATE_FULLSCREEN"),
    );

    // ...and leaving fullscreen puts it back exactly where it was.
    let restored = wait_for(&conn, window, root, "the windowed geometry", |geometry| {
        geometry == windowed
    });
    assert_eq!(restored, windowed);
}

#[test]
fn fullscreen_is_advertised_so_clients_take_the_ewmh_path() {
    let Some(session) = session() else {
        return;
    };
    let (conn, screen_number) =
        RustConnection::connect(Some(session.display())).expect("connect to nested display");
    let root = conn.setup().roots[screen_number].root;

    let supported: Vec<u32> = conn
        .get_property(
            false,
            root,
            atom(&conn, "_NET_SUPPORTED"),
            AtomEnum::ATOM,
            0,
            256,
        )
        .expect("get _NET_SUPPORTED")
        .reply()
        .expect("_NET_SUPPORTED reply")
        .value32()
        .expect("_NET_SUPPORTED is a list of atoms")
        .collect();

    // A client that can't see these falls back to brute force - resizing
    // itself to the screen, which is the failure mode this whole thing
    // exists to avoid.
    for required in [
        "_NET_WM_STATE",
        "_NET_WM_STATE_FULLSCREEN",
        "_NET_FRAME_EXTENTS",
        "_NET_WM_MOVERESIZE",
        "_NET_WORKAREA",
        "_NET_WM_WINDOW_TYPE",
    ] {
        assert!(
            supported.contains(&atom(&conn, required)),
            "{required} missing from _NET_SUPPORTED"
        );
    }
}

#[test]
fn a_fixed_size_window_is_not_stretched_to_fit() {
    let Some(session) = session() else {
        return;
    };
    let (conn, screen_number) =
        RustConnection::connect(Some(session.display())).expect("connect to nested display");
    let screen = conn.setup().roots[screen_number].clone();
    let root = screen.root;

    let window = conn.generate_id().expect("generate_id");
    conn.create_window(
        screen.root_depth,
        window,
        root,
        0,
        0,
        CLIENT_WIDTH,
        CLIENT_HEIGHT,
        0,
        WindowClass::INPUT_OUTPUT,
        screen.root_visual,
        &CreateWindowAux::new().background_pixel(0x0000ff),
    )
    .expect("create_window");

    // WM_NORMAL_HINTS pinning min == max: the classic "this game runs at
    // exactly this resolution" declaration.
    let mut hints = x11rb::properties::WmSizeHints::new();
    hints.min_size = Some((i32::from(CLIENT_WIDTH), i32::from(CLIENT_HEIGHT)));
    hints.max_size = Some((i32::from(CLIENT_WIDTH), i32::from(CLIENT_HEIGHT)));
    hints
        .set_normal_hints(&conn, window)
        .expect("set WM_NORMAL_HINTS")
        .check()
        .expect("WM_NORMAL_HINTS applied");

    conn.map_window(window).expect("map_window");
    conn.flush().expect("flush");

    let framed = wait_for(
        &conn,
        window,
        root,
        "the window to be framed",
        |(x, y, _, _)| x > 0 && y > 0,
    );
    assert_eq!(
        (framed.2, framed.3),
        (CLIENT_WIDTH, CLIENT_HEIGHT),
        "a window that declared a fixed size was resized anyway"
    );
}

/// The taskbar window: ours, override-redirect, pinned to the bottom edge.
/// Found by shape rather than by id, since the WM never publishes it - which
/// is the point, no client is supposed to know it exists.
fn find_taskbar(conn: &RustConnection, root: Window) -> Window {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let children = conn
            .query_tree(root)
            .expect("query_tree")
            .reply()
            .expect("query_tree reply")
            .children;
        for window in children {
            let Ok(attributes) = conn
                .get_window_attributes(window)
                .expect("get_window_attributes")
                .reply()
            else {
                continue;
            };
            if !attributes.override_redirect {
                continue;
            }
            let Ok(geometry) = conn.get_geometry(window).expect("get_geometry").reply() else {
                continue;
            };
            if geometry.width == ROOT_WIDTH
                && geometry.height > 0
                && geometry.y as i32 + i32::from(geometry.height) == i32::from(ROOT_HEIGHT)
            {
                return window;
            }
        }
        assert!(Instant::now() < deadline, "no taskbar found on the root");
        sleep(Duration::from_millis(50));
    }
}

fn map_state(conn: &RustConnection, window: Window) -> x11rb::protocol::xproto::MapState {
    conn.get_window_attributes(window)
        .expect("get_window_attributes")
        .reply()
        .expect("get_window_attributes reply")
        .map_state
}

fn wait_for_map_state(
    conn: &RustConnection,
    window: Window,
    wanted: x11rb::protocol::xproto::MapState,
    what: &str,
) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if map_state(conn, window) == wanted {
            return;
        }
        sleep(Duration::from_millis(50));
    }
    panic!("timed out waiting for {what}");
}

/// Minimising must not be a one-way door.
///
/// A fullscreen Windows game minimises *itself* every time it loses
/// activation, and the Steam Deck in game mode has no keyboard to press
/// Super+Tab with - so a minimised window that can only be restored from the
/// keyboard is a window that is gone. This drives the whole round trip the
/// way a user would: iconify, then click the taskbar entry.
#[test]
fn a_minimised_window_can_be_restored_from_the_taskbar() {
    let Some(session) = session() else {
        return;
    };
    let (conn, screen_number) =
        RustConnection::connect(Some(session.display())).expect("connect to nested display");
    let screen = conn.setup().roots[screen_number].clone();
    let root = screen.root;

    let window = map_test_window(&conn, root, screen.root_depth, screen.root_visual);
    wait_for(
        &conn,
        window,
        root,
        "the window to be framed",
        |(x, y, _, _)| x > 0 && y > 0,
    );

    // ICCCM's "please minimise me" - the same message Wine sends on
    // WM_ACTIVATEAPP when a fullscreen game loses focus.
    let wm_change_state = atom(&conn, "WM_CHANGE_STATE");
    let iconify = ClientMessageEvent::new(32, window, wm_change_state, [3u32, 0, 0, 0, 0]);
    conn.send_event(
        false,
        root,
        EventMask::SUBSTRUCTURE_NOTIFY | EventMask::SUBSTRUCTURE_REDIRECT,
        iconify,
    )
    .expect("send_event");
    conn.flush().expect("flush");

    wait_for_map_state(
        &conn,
        window,
        x11rb::protocol::xproto::MapState::UNMAPPED,
        "the window to be iconified",
    );

    let taskbar = find_taskbar(&conn, root);

    // Click the first entry, which is the only window there is.
    let click = x11rb::protocol::xproto::ButtonPressEvent {
        response_type: x11rb::protocol::xproto::BUTTON_PRESS_EVENT,
        detail: 1,
        sequence: 0,
        time: x11rb::CURRENT_TIME,
        root,
        event: taskbar,
        child: x11rb::NONE,
        root_x: 10,
        root_y: 10,
        event_x: 10,
        event_y: 10,
        state: 0u16.into(),
        same_screen: true,
    };
    conn.send_event(false, taskbar, EventMask::BUTTON_PRESS, click)
        .expect("send_event");
    conn.flush().expect("flush");

    wait_for_map_state(
        &conn,
        window,
        x11rb::protocol::xproto::MapState::VIEWABLE,
        "the window to be restored by a taskbar click",
    );
}

/// The taskbar is excluded from the work area, so a maximized window stops
/// above it instead of putting its own controls underneath it.
#[test]
fn the_work_area_leaves_room_for_the_taskbar() {
    let Some(session) = session() else {
        return;
    };
    let (conn, screen_number) =
        RustConnection::connect(Some(session.display())).expect("connect to nested display");
    let root = conn.setup().roots[screen_number].root;

    let work_area: Vec<u32> = conn
        .get_property(
            false,
            root,
            atom(&conn, "_NET_WORKAREA"),
            AtomEnum::CARDINAL,
            0,
            4,
        )
        .expect("get _NET_WORKAREA")
        .reply()
        .expect("_NET_WORKAREA reply")
        .value32()
        .expect("_NET_WORKAREA is a list of cardinals")
        .collect();
    assert_eq!(work_area.len(), 4, "_NET_WORKAREA should be x, y, w, h");

    let taskbar = find_taskbar(&conn, root);
    let geometry = conn
        .get_geometry(taskbar)
        .expect("get_geometry")
        .reply()
        .expect("get_geometry reply");

    assert_eq!(work_area[2], u32::from(ROOT_WIDTH));
    assert_eq!(
        work_area[3],
        u32::from(ROOT_HEIGHT - geometry.height),
        "the work area should be the root minus exactly the taskbar's height"
    );
}
