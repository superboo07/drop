//! Drives real clients through a real nested session - the bundled openbox,
//! picom and tint2 on a nested Xwayland/Xephyr - and asserts what they end
//! up with, which is the only thing that matters to a game rendering into a
//! window or a translator drawing over it.
//!
//! Skipped (not failed) when this machine can't host a session: no Xwayland
//! or Xephyr to nest inside (CI runners generally have neither), or the tools
//! haven't been built (`bash desktop/vendor/build.sh`).

#![cfg(target_os = "linux")]

use std::{
    thread::sleep,
    time::{Duration, Instant},
};

use nested_session::{NestedSession, error::NestedSessionError};
use x11rb::{
    connection::Connection,
    protocol::shape::ConnectionExt as _,
    protocol::xfixes::{ConnectionExt as _, GetCursorImageReply},
    protocol::xtest::ConnectionExt as _,
    protocol::xproto::{
        AtomEnum, BUTTON_PRESS_EVENT, BUTTON_RELEASE_EVENT, ButtonPressEvent,
        ChangeWindowAttributesAux, ClientMessageEvent, ColormapAlloc, ConfigureWindowAux, ConnectionExt as _, CreateWindowAux, EventMask,
        MapState, PropMode, Screen, Window, WindowClass,
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

const ICCCM_NORMAL: u32 = 1;
const ICCCM_ICONIC: u32 = 3;

/// Starts a session, or returns `None` when this machine can't host one.
fn session() -> Option<NestedSession> {
    match NestedSession::start(ROOT_WIDTH, ROOT_HEIGHT) {
        Ok(session) => Some(session),
        Err(NestedSessionError::NoXServer) => {
            eprintln!("skipping: no Xwayland or Xephyr available to nest inside");
            None
        }
        Err(e @ NestedSessionError::MissingTools(_)) => {
            eprintln!("skipping: {e} - run `bash desktop/vendor/build.sh`");
            None
        }
        Err(e) => panic!("could not start a nested session: {e}"),
    }
}

fn connect(session: &NestedSession) -> (RustConnection, Screen) {
    let (conn, screen_number) =
        RustConnection::connect(Some(session.display())).expect("connect to nested display");
    let screen = conn.setup().roots[screen_number].clone();
    (conn, screen)
}

fn atom(conn: &RustConnection, name: &str) -> u32 {
    conn.intern_atom(false, name.as_bytes())
        .expect("intern_atom")
        .reply()
        .expect("intern_atom reply")
        .atom
}

fn property32(conn: &RustConnection, window: Window, property: u32, kind: AtomEnum) -> Vec<u32> {
    conn.get_property(false, window, property, kind, 0, 256)
        .expect("get_property")
        .reply()
        .expect("get_property reply")
        .value32()
        .map(Iterator::collect)
        .unwrap_or_default()
}

/// Polls until `check` returns something, because every transition here is
/// a round trip through another process.
fn wait_until<T>(what: &str, mut check: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(value) = check() {
            return value;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        sleep(Duration::from_millis(50));
    }
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

fn wait_for_geometry(
    conn: &RustConnection,
    window: Window,
    root: Window,
    what: &str,
    predicate: impl Fn((i16, i16, u16, u16)) -> bool,
) -> (i16, i16, u16, u16) {
    wait_until(what, || {
        let geometry = absolute_geometry(conn, window, root);
        predicate(geometry).then_some(geometry)
    })
}

/// Framed means reparented away from the root and inset by the chrome.
fn wait_until_framed(conn: &RustConnection, window: Window, root: Window) -> (i16, i16, u16, u16) {
    wait_for_geometry(
        conn,
        window,
        root,
        "the window to be framed",
        |(x, y, _, _)| x > 0 && y > 0,
    )
}

fn send_to_root(conn: &RustConnection, root: Window, event: ClientMessageEvent) {
    conn.send_event(
        false,
        root,
        EventMask::SUBSTRUCTURE_NOTIFY | EventMask::SUBSTRUCTURE_REDIRECT,
        event,
    )
    .expect("send_event");
    conn.flush().expect("flush");
}

fn send_state(conn: &RustConnection, root: Window, window: Window, action: u32, state: &str) {
    let event = ClientMessageEvent::new(
        32,
        window,
        atom(conn, "_NET_WM_STATE"),
        [action, atom(conn, state), 0, 1, 0],
    );
    send_to_root(conn, root, event);
}

/// Asks the window manager to activate `window`, as a pager or a click would.
fn activate(conn: &RustConnection, root: Window, window: Window) {
    let event = ClientMessageEvent::new(
        32,
        window,
        atom(conn, "_NET_ACTIVE_WINDOW"),
        [2, x11rb::CURRENT_TIME, 0, 0, 0],
    );
    send_to_root(conn, root, event);
}

fn create_window(
    conn: &RustConnection,
    root: Window,
    depth: u8,
    visual: u32,
    aux: &CreateWindowAux,
) -> Window {
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
        aux,
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
    window
}

fn map_test_window(conn: &RustConnection, screen: &Screen) -> Window {
    let window = create_window(
        conn,
        screen.root,
        screen.root_depth,
        screen.root_visual,
        &CreateWindowAux::new().background_pixel(0x00ff00),
    );
    conn.map_window(window).expect("map_window");
    conn.flush().expect("flush");
    window
}

/// The ancestor of `window` that's a direct child of the root - its frame,
/// for a managed window.
fn top_level(conn: &RustConnection, mut window: Window, root: Window) -> Window {
    loop {
        let parent = conn
            .query_tree(window)
            .expect("query_tree")
            .reply()
            .expect("query_tree reply")
            .parent;
        if parent == root {
            return window;
        }
        window = parent;
    }
}

/// Position of `window`'s frame in the root's stacking order, bottom first.
fn stacking_index(conn: &RustConnection, window: Window, root: Window) -> usize {
    let frame = top_level(conn, window, root);
    conn.query_tree(root)
        .expect("query_tree")
        .reply()
        .expect("query_tree reply")
        .children
        .iter()
        .position(|&child| child == frame)
        .expect("frame is a child of the root")
}

fn wm_state(conn: &RustConnection, window: Window) -> Option<u32> {
    let wm_state = atom(conn, "WM_STATE");
    conn.get_property(false, window, wm_state, wm_state, 0, 2)
        .expect("get WM_STATE")
        .reply()
        .expect("WM_STATE reply")
        .value32()
        .and_then(|mut values| values.next())
}

fn wm_class(conn: &RustConnection, window: Window) -> Vec<u8> {
    conn.get_property(false, window, AtomEnum::WM_CLASS, AtomEnum::STRING, 0, 64)
        .expect("get WM_CLASS")
        .reply()
        .map(|reply| reply.value)
        .unwrap_or_default()
}

/// tint2's panel window, found by class anywhere under the root (openbox
/// reparents it, like any other client).
fn find_taskbar(conn: &RustConnection, root: Window) -> Window {
    fn search(conn: &RustConnection, window: Window) -> Option<Window> {
        if wm_class(conn, window)
            .split(|&byte| byte == 0)
            .any(|part| part == b"tint2")
        {
            return Some(window);
        }
        let children = conn.query_tree(window).ok()?.reply().ok()?.children;
        children.into_iter().find_map(|child| search(conn, child))
    }
    wait_until("the taskbar", || search(conn, root))
}

#[test]
fn fullscreen_gives_the_client_the_whole_root_and_gives_it_back() {
    let Some(session) = session() else {
        return;
    };
    let (conn, screen) = connect(&session);
    let root = screen.root;

    let window = map_test_window(&conn, &screen);
    let windowed = wait_until_framed(&conn, window, root);
    assert_eq!((windowed.2, windowed.3), (CLIENT_WIDTH, CLIENT_HEIGHT));

    send_state(&conn, root, window, STATE_ADD, "_NET_WM_STATE_FULLSCREEN");

    // The whole root, at its origin, with no chrome pushing it anywhere.
    let fullscreen = wait_for_geometry(&conn, window, root, "fullscreen", |(x, y, w, h)| {
        x == 0 && y == 0 && w == ROOT_WIDTH && h == ROOT_HEIGHT
    });
    assert_eq!(fullscreen, (0, 0, ROOT_WIDTH, ROOT_HEIGHT));

    send_state(
        &conn,
        root,
        window,
        STATE_REMOVE,
        "_NET_WM_STATE_FULLSCREEN",
    );

    // ...and leaving fullscreen puts it back exactly where it was.
    let restored = wait_for_geometry(&conn, window, root, "the windowed geometry", |geometry| {
        geometry == windowed
    });
    assert_eq!(restored, windowed);
}

#[test]
fn the_ewmh_features_games_and_qt_rely_on_are_advertised() {
    let Some(session) = session() else {
        return;
    };
    let (conn, screen) = connect(&session);

    let supported = property32(
        &conn,
        screen.root,
        atom(&conn, "_NET_SUPPORTED"),
        AtomEnum::ATOM,
    );

    // A client that can't see these falls back to brute force - resizing
    // itself to the screen, guessing where to put its dialogs.
    for required in [
        "_NET_WM_STATE",
        "_NET_WM_STATE_FULLSCREEN",
        "_NET_WM_STATE_ABOVE",
        "_NET_WM_STATE_HIDDEN",
        "_NET_FRAME_EXTENTS",
        "_NET_WM_MOVERESIZE",
        "_NET_WORKAREA",
        "_NET_WM_WINDOW_TYPE",
        "_NET_ACTIVE_WINDOW",
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
    let (conn, screen) = connect(&session);
    let root = screen.root;

    let window = create_window(
        &conn,
        root,
        screen.root_depth,
        screen.root_visual,
        &CreateWindowAux::new().background_pixel(0x0000ff),
    );

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

    let framed = wait_until_framed(&conn, window, root);
    assert_eq!(
        (framed.2, framed.3),
        (CLIENT_WIDTH, CLIENT_HEIGHT),
        "a window that declared a fixed size was resized anyway"
    );
}

/// Minimising must not be a one-way door.
///
/// A fullscreen Windows game minimises *itself* every time it loses
/// activation, and the Steam Deck in game mode has no keyboard - so a
/// minimised window that can only be restored from the keyboard is a window
/// that is gone. This drives the whole round trip the way a user would:
/// iconify, then click the taskbar entry.
#[test]
fn a_minimised_window_can_be_restored_from_the_taskbar() {
    let Some(session) = session() else {
        return;
    };
    let (conn, screen) = connect(&session);
    let root = screen.root;

    let window = map_test_window(&conn, &screen);
    wait_until_framed(&conn, window, root);

    // ICCCM's "please minimise me" - the same message Wine sends on
    // WM_ACTIVATEAPP when a fullscreen game loses focus.
    let iconify = ClientMessageEvent::new(
        32,
        window,
        atom(&conn, "WM_CHANGE_STATE"),
        [ICCCM_ICONIC, 0, 0, 0, 0],
    );
    send_to_root(&conn, root, iconify);
    wait_until("the window to be iconified", || {
        (wm_state(&conn, window) == Some(ICCCM_ICONIC)).then_some(())
    });

    // Click the first entry, which is the only window there is. tint2 acts
    // on the release; the click is repeated because tint2 may not have laid
    // the entry out yet, and a repeat is harmless - `toggle` only ever
    // activates.
    let taskbar = find_taskbar(&conn, root);
    let click = |response_type| ButtonPressEvent {
        response_type,
        detail: 1,
        sequence: 0,
        time: x11rb::CURRENT_TIME,
        root,
        event: taskbar,
        child: x11rb::NONE,
        root_x: 20,
        root_y: 20,
        event_x: 20,
        event_y: 20,
        state: 0u16.into(),
        same_screen: true,
    };
    wait_until("the window to be restored by a taskbar click", || {
        for response_type in [BUTTON_PRESS_EVENT, BUTTON_RELEASE_EVENT] {
            conn.send_event(
                false,
                taskbar,
                EventMask::BUTTON_PRESS | EventMask::BUTTON_RELEASE,
                click(response_type),
            )
            .expect("send_event");
        }
        conn.flush().expect("flush");
        sleep(Duration::from_millis(200));

        let viewable = conn
            .get_window_attributes(window)
            .expect("get_window_attributes")
            .reply()
            .expect("get_window_attributes reply")
            .map_state
            == MapState::VIEWABLE;
        (viewable && wm_state(&conn, window) == Some(ICCCM_NORMAL)).then_some(())
    });
}

/// The taskbar is excluded from the work area, so a maximized window stops
/// above it instead of putting its own controls underneath it.
#[test]
fn the_work_area_leaves_room_for_the_taskbar() {
    let Some(session) = session() else {
        return;
    };
    let (conn, screen) = connect(&session);
    let root = screen.root;

    let work_area = property32(
        &conn,
        root,
        atom(&conn, "_NET_WORKAREA"),
        AtomEnum::CARDINAL,
    );
    assert_eq!(work_area.len(), 4, "_NET_WORKAREA should be x, y, w, h");

    let taskbar = find_taskbar(&conn, root);
    let (_, taskbar_y, taskbar_width, taskbar_height) = absolute_geometry(&conn, taskbar, root);
    assert_eq!(
        taskbar_width, ROOT_WIDTH,
        "the taskbar should span the root"
    );
    assert_eq!(
        i32::from(taskbar_y) + i32::from(taskbar_height),
        i32::from(ROOT_HEIGHT),
        "the taskbar should sit on the bottom edge"
    );

    assert_eq!(work_area[2], u32::from(ROOT_WIDTH));
    assert_eq!(
        work_area[3],
        u32::from(ROOT_HEIGHT - taskbar_height),
        "the work area should be the root minus exactly the taskbar's height"
    );
}

/// Qt (so LunaTranslator) only draws a translucent window when a compositing
/// manager owns this selection - without it, "see-through" is just black.
#[test]
fn a_compositing_manager_owns_the_screen() {
    let Some(session) = session() else {
        return;
    };
    let (conn, screen) = connect(&session);
    let screen_number = conn
        .setup()
        .roots
        .iter()
        .position(|candidate| candidate.root == screen.root)
        .expect("screen");

    let owner = conn
        .get_selection_owner(atom(&conn, &format!("_NET_WM_CM_S{screen_number}")))
        .expect("get_selection_owner")
        .reply()
        .expect("get_selection_owner reply")
        .owner;
    assert_ne!(
        owner,
        x11rb::NONE,
        "nothing owns _NET_WM_CM_S{screen_number}"
    );
}

/// A compositor can only blend what it's given: a 32-bit client reparented
/// into a 24-bit frame has its alpha channel thrown away before the
/// compositor ever sees it. Every window between the client and the root has
/// to keep the depth.
#[test]
fn a_translucent_window_keeps_its_alpha_channel_when_framed() {
    let Some(session) = session() else {
        return;
    };
    let (conn, screen) = connect(&session);
    let root = screen.root;

    let visual = screen
        .allowed_depths
        .iter()
        .filter(|depth| depth.depth == 32)
        .flat_map(|depth| depth.visuals.iter())
        .find(|visual| visual.class == x11rb::protocol::xproto::VisualClass::TRUE_COLOR)
        .expect("the nested server offers a 32-bit TrueColor visual")
        .visual_id;

    let colormap = conn.generate_id().expect("generate_id");
    conn.create_colormap(ColormapAlloc::NONE, colormap, root, visual)
        .expect("create_colormap");
    // A depth-32 window in a depth-24 parent needs its own border pixel and
    // colormap, or the request fails with BadMatch.
    let window = create_window(
        &conn,
        root,
        32,
        visual,
        &CreateWindowAux::new()
            .background_pixel(0x8000_ff00)
            .border_pixel(0)
            .colormap(colormap),
    );
    conn.map_window(window).expect("map_window");
    conn.flush().expect("flush");
    wait_until_framed(&conn, window, root);

    let mut current = window;
    while current != root {
        let depth = conn
            .get_geometry(current)
            .expect("get_geometry")
            .reply()
            .expect("get_geometry reply")
            .depth;
        assert_eq!(
            depth, 32,
            "window {current:#x} between the client and the root is {depth}-bit"
        );
        current = conn
            .query_tree(current)
            .expect("query_tree")
            .reply()
            .expect("query_tree reply")
            .parent;
    }
}

/// The reason the nested session exists: on a Deck, the game is fullscreen
/// and LunaTranslator's window has to stay readable on top of it - as does
/// the taskbar. Upstream openbox lifts a focused fullscreen window above
/// both; Drop's build doesn't (desktop/vendor/patches).
#[test]
fn an_always_on_top_window_and_the_taskbar_stay_over_a_focused_fullscreen_game() {
    let Some(session) = session() else {
        return;
    };
    let (conn, screen) = connect(&session);
    let root = screen.root;

    let game = map_test_window(&conn, &screen);
    wait_until_framed(&conn, game, root);

    let translator = create_window(
        &conn,
        root,
        screen.root_depth,
        screen.root_visual,
        &CreateWindowAux::new().background_pixel(0xff00ff),
    );
    // Set before mapping, the way Qt's WindowStaysOnTopHint does it.
    conn.change_property32(
        PropMode::REPLACE,
        translator,
        atom(&conn, "_NET_WM_STATE"),
        AtomEnum::ATOM,
        &[atom(&conn, "_NET_WM_STATE_ABOVE")],
    )
    .expect("set _NET_WM_STATE");
    conn.map_window(translator).expect("map_window");
    conn.flush().expect("flush");
    wait_until_framed(&conn, translator, root);

    send_state(&conn, root, game, STATE_ADD, "_NET_WM_STATE_FULLSCREEN");
    wait_for_geometry(&conn, game, root, "fullscreen", |(x, y, w, h)| {
        x == 0 && y == 0 && w == ROOT_WIDTH && h == ROOT_HEIGHT
    });

    activate(&conn, root, game);
    let active_window = atom(&conn, "_NET_ACTIVE_WINDOW");
    wait_until("the game to be focused", || {
        property32(&conn, root, active_window, AtomEnum::WINDOW)
            .first()
            .is_some_and(|&active| active == game)
            .then_some(())
    });
    // Give a restack that shouldn't happen the chance to.
    sleep(Duration::from_millis(300));

    let game_index = stacking_index(&conn, game, root);
    assert!(
        stacking_index(&conn, translator, root) > game_index,
        "the always-on-top window went under the focused fullscreen game"
    );
    let taskbar = find_taskbar(&conn, root);
    assert!(
        stacking_index(&conn, taskbar, root) > game_index,
        "the taskbar went under the focused fullscreen game"
    );
}

/// Leaving fullscreen the way Wine does it: decorations back on, the new
/// windowed geometry requested, *then* `_NET_WM_STATE_FULLSCREEN` dropped.
///
/// Upstream openbox ignores a resize while the window is still fullscreen
/// and then restores the pre-fullscreen size - for a game that started
/// fullscreen, the whole screen. The game believes it's 800x600, renders
/// scaled into a window that isn't, and every click lands offset. Fixed by
/// desktop/vendor/patches/openbox-fullscreen-configure.patch; both orders
/// are checked.
#[test]
fn a_game_leaving_fullscreen_gets_the_windowed_size_it_asked_for() {
    let Some(session) = session() else {
        return;
    };
    let (conn, screen) = connect(&session);
    let root = screen.root;

    for resize_first in [true, false] {
        let window = create_window(
            &conn,
            root,
            screen.root_depth,
            screen.root_visual,
            &CreateWindowAux::new().background_pixel(0x00ff00),
        );
        conn.configure_window(
            window,
            &ConfigureWindowAux::new()
                .width(u32::from(ROOT_WIDTH))
                .height(u32::from(ROOT_HEIGHT)),
        )
        .expect("configure_window");
        // Wine's fullscreen window: undecorated, fullscreen before it's
        // ever mapped.
        let motif = atom(&conn, "_MOTIF_WM_HINTS");
        conn.change_property32(PropMode::REPLACE, window, motif, motif, &[2, 0, 0, 0, 0])
            .expect("set _MOTIF_WM_HINTS");
        conn.change_property32(
            PropMode::REPLACE,
            window,
            atom(&conn, "_NET_WM_STATE"),
            AtomEnum::ATOM,
            &[atom(&conn, "_NET_WM_STATE_FULLSCREEN")],
        )
        .expect("set _NET_WM_STATE");
        conn.map_window(window).expect("map_window");
        conn.flush().expect("flush");
        wait_for_geometry(&conn, window, root, "fullscreen", |(x, y, w, h)| {
            x == 0 && y == 0 && w == ROOT_WIDTH && h == ROOT_HEIGHT
        });

        conn.change_property32(PropMode::REPLACE, window, motif, motif, &[2, 0, 1, 0, 0])
            .expect("set _MOTIF_WM_HINTS");
        let windowed = ConfigureWindowAux::new()
            .x(100)
            .y(100)
            .width(u32::from(CLIENT_WIDTH))
            .height(u32::from(CLIENT_HEIGHT));
        if resize_first {
            conn.configure_window(window, &windowed)
                .expect("configure_window");
            send_state(
                &conn,
                root,
                window,
                STATE_REMOVE,
                "_NET_WM_STATE_FULLSCREEN",
            );
        } else {
            send_state(
                &conn,
                root,
                window,
                STATE_REMOVE,
                "_NET_WM_STATE_FULLSCREEN",
            );
            conn.configure_window(window, &windowed)
                .expect("configure_window");
        }
        conn.flush().expect("flush");

        let order = if resize_first {
            "resize, then leave fullscreen"
        } else {
            "leave fullscreen, then resize"
        };
        wait_for_geometry(
            &conn,
            window,
            root,
            &format!("the requested windowed size ({order})"),
            |(_, _, w, h)| w == CLIENT_WIDTH && h == CLIENT_HEIGHT,
        );
        conn.destroy_window(window).expect("destroy_window");
        conn.flush().expect("flush");
    }
}

/// The window `draw_cursor` draws the pointer with.
fn find_cursor_window(conn: &RustConnection, root: Window) -> Option<Window> {
    conn.query_tree(root)
        .expect("query_tree")
        .reply()
        .expect("query_tree reply")
        .children
        .into_iter()
        .find(|&window| {
            conn.get_property(false, window, AtomEnum::WM_NAME, AtomEnum::STRING, 0, 64)
                .expect("get_property")
                .reply()
                .is_ok_and(|reply| reply.value == b"drop-nested-session-cursor")
        })
}

/// Moves the pointer the way a person does: as device motion (XTEST), which
/// is what raw motion events - and so the drawn cursor - follow.
fn move_pointer(conn: &RustConnection, root: Window, x: i16, y: i16) {
    conn.xtest_fake_input(x11rb::protocol::xproto::MOTION_NOTIFY_EVENT, 0, 0, root, x, y, 0)
        .expect("xtest_fake_input");
    conn.flush().expect("flush");
}

fn cursor_image(conn: &RustConnection) -> GetCursorImageReply {
    conn.xfixes_query_version(2, 0)
        .expect("xfixes_query_version")
        .reply()
        .expect("xfixes_query_version reply");
    conn.xfixes_get_cursor_image()
        .expect("xfixes_get_cursor_image")
        .reply()
        .expect("xfixes_get_cursor_image reply")
}

#[test]
fn the_cursor_is_drawn_into_the_session_and_follows_the_pointer() {
    let Some(mut session) = session() else {
        return;
    };
    session
        .draw_cursor()
        .unwrap_or_else(|e| panic!("draw_cursor: {e}"));
    let (conn, screen) = connect(&session);
    let root = screen.root;

    // A game window, wearing the core font's watch, pointer over it.
    let window = map_test_window(&conn, &screen);
    wait_until_framed(&conn, window, root);
    let font = conn.generate_id().expect("generate_id");
    conn.open_font(font, b"cursor").expect("open_font");
    let watch = conn.generate_id().expect("generate_id");
    conn.create_glyph_cursor(watch, font, font, 150, 151, 0, 0, 0, 0xffff, 0xffff, 0xffff)
        .expect("create_glyph_cursor");
    conn.change_window_attributes(window, &ChangeWindowAttributesAux::new().cursor(watch))
        .expect("change_window_attributes");
    let (x, y, _, _) = absolute_geometry(&conn, window, root);
    move_pointer(&conn, root, x + 100, y + 100);

    let cursor_window = wait_until("the cursor window", || find_cursor_window(&conn, root));
    let expected = cursor_image(&conn);
    assert!(expected.width > 1, "the session's cursor is the watch");

    // It wears exactly the session's cursor, hotspot on the pointer, on top
    // of everything.
    let at = |px: i16, py: i16| {
        let (hx, hy) = (expected.xhot as i16, expected.yhot as i16);
        wait_until("the cursor window at the pointer", || {
            let attributes = conn
                .get_window_attributes(cursor_window)
                .expect("get_window_attributes")
                .reply()
                .expect("get_window_attributes reply");
            let geometry = conn
                .get_geometry(cursor_window)
                .expect("get_geometry")
                .reply()
                .expect("get_geometry reply");
            (attributes.map_state == MapState::VIEWABLE
                && (geometry.x, geometry.y) == (px - hx, py - hy)
                && (geometry.width, geometry.height) == (expected.width, expected.height))
                .then_some(())
        });
    };
    at(x + 100, y + 100);
    let children = conn
        .query_tree(root)
        .expect("query_tree")
        .reply()
        .expect("query_tree reply")
        .children;
    assert_eq!(children.last(), Some(&cursor_window), "the cursor is on top");
    let pixels = conn
        .get_image(
            x11rb::protocol::xproto::ImageFormat::Z_PIXMAP,
            cursor_window,
            0,
            0,
            expected.width,
            expected.height,
            !0,
        )
        .expect("get_image")
        .reply()
        .expect("get_image reply")
        .data;
    let drawn: Vec<u32> = pixels
        .as_chunks::<4>()
        .0
        .iter()
        .map(|&p| u32::from_le_bytes(p))
        .collect();
    assert_eq!(drawn, expected.cursor_image, "the drawn image is the session's cursor");

    // It never takes a click: no input shape at all.
    let input = conn
        .shape_get_rectangles(cursor_window, x11rb::protocol::shape::SK::INPUT)
        .expect("shape_get_rectangles")
        .reply()
        .expect("shape_get_rectangles reply");
    assert!(input.rectangles.is_empty(), "the cursor window has no input area");

    // It follows the pointer.
    move_pointer(&conn, root, x + 250, y + 40);
    at(x + 250, y + 40);

    // A game hiding its pointer does it with an invisible cursor.
    let empty = conn.generate_id().expect("generate_id");
    conn.create_pixmap(1, empty, root, 1, 1).expect("create_pixmap");
    let blank = conn.generate_id().expect("generate_id");
    conn.create_cursor(blank, empty, empty, 0, 0, 0, 0, 0, 0, 0, 0)
        .expect("create_cursor");
    conn.change_window_attributes(window, &ChangeWindowAttributesAux::new().cursor(blank))
        .expect("change_window_attributes");
    conn.flush().expect("flush");
    wait_until("the cursor window to be hidden", || {
        let attributes = conn
            .get_window_attributes(cursor_window)
            .expect("get_window_attributes")
            .reply()
            .expect("get_window_attributes reply");
        (attributes.map_state == MapState::UNMAPPED).then_some(())
    });
}
