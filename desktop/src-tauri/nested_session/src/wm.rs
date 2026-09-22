//! A small floating, reparenting window manager for the nested X session.
//!
//! Deliberately floating rather than tiling: the point of the nested session
//! is that the game and LunaTranslator's windows are *ordinary windows the
//! user drags around*, on a Steam Deck as much as on a desktop. So every
//! window gets a chunky titlebar (easy to hit with a trackpad-as-mouse), a
//! grabbable bottom bar to resize from, and nothing that ever rearranges a
//! window behind the user's back.
//!
//! It is not a general-purpose WM - no workspaces, one desktop - but it is
//! not a toy either, because the two things it has to host are the two least
//! forgiving kinds of X client there are:
//!
//! - **Games under Wine** need ICCCM done properly. Synthetic
//!   `ConfigureNotify` on every move (`notify_geometry`), `WM_NORMAL_HINTS`
//!   respected so a fixed-size window isn't stretched, `_MOTIF_WM_HINTS` for
//!   the borderless windows they ask for, and real `_NET_WM_STATE_FULLSCREEN`
//!   support - deny that and they go fullscreen by brute force instead, which
//!   is worse.
//! - **Qt apps** (LunaTranslator) lean on EWMH: `_NET_WM_WINDOW_TYPE` to
//!   distinguish a tooltip from a window, `_NET_FRAME_EXTENTS` and
//!   `_NET_WORKAREA` to place their own dialogs, `_NET_WM_STATE_ABOVE` for
//!   an always-on-top translation window, `WM_TAKE_FOCUS` to accept focus,
//!   and `_NET_WM_MOVERESIZE` for client-side-decorated drags.
//!
//! There is a taskbar, and it is not decoration. A fullscreen Windows game
//! minimises *itself* every time it loses activation, and the Steam Deck in
//! game mode has no keyboard to press Super+Tab with - so without a strip of
//! clickable entries, minimising a window would lose it for good.
//!
//! Stacking is three fixed layers - below, normal, above - which is what lets
//! a fullscreen game and an always-on-top translator coexist: the game sinks,
//! the translator floats, and neither has to be denied what it asked for.

use std::{collections::HashMap, sync::mpsc::Sender, thread::sleep, time::Duration};

use log::{debug, info, warn};
use x11rb::{
    CURRENT_TIME, NONE,
    connection::Connection,
    properties::{WmHints, WmSizeHints},
    protocol::{
        Event,
        xproto::{
            AtomEnum, ButtonIndex, CONFIGURE_NOTIFY_EVENT, ChangeGCAux, ChangeWindowAttributesAux,
            ClientMessageEvent, ConfigureNotifyEvent, ConfigureWindowAux, ConnectionExt as _,
            CreateGCAux, CreateWindowAux, EventMask, Gcontext, GrabMode, InputFocus, MapState,
            ModMask, PropMode, Rectangle, Screen, Segment, SetMode, StackMode, Timestamp, Window,
            WindowClass,
        },
    },
    rust_connection::RustConnection,
    wrapper::ConnectionExt as _,
};

use crate::error::NestedSessionError;

// Matches Drop's own zinc/blue palette so the nested session doesn't look
// like a different application. Raw 24-bit RGB values: both Xwayland and
// Xephyr give us a TrueColor root visual, so no colormap allocation needed.
const COLOUR_ROOT: u32 = 0x09090b;
const COLOUR_FRAME: u32 = 0x27272a;
const COLOUR_FRAME_FOCUSED: u32 = 0x2563eb;
const COLOUR_TITLE: u32 = 0xfafafa;
const COLOUR_TITLE_UNFOCUSED: u32 = 0xa1a1aa;
const COLOUR_CLOSE: u32 = 0xdc2626;
const COLOUR_GRIP: u32 = 0x52525b;
const COLOUR_TASKBAR: u32 = 0x18181b;
const COLOUR_ENTRY: u32 = 0x3f3f46;
const COLOUR_ENTRY_ICONIFIED: u32 = 0x27272a;

// Generous, because on a Deck these are hit with a trackpad cursor rather
// than a mouse - a 20px desktop titlebar is miserable to grab there. The
// bottom bar is deliberately thicker than the sides: it's the resize handle
// that's actually visible, since the client window covers everything the
// frame owns except its own border.
const TITLEBAR_HEIGHT: i16 = 34;
const BORDER: i16 = 6;
const BOTTOM_BAR: i16 = 16;
/// How far from a frame edge still counts as grabbing that edge to resize.
const RESIZE_ZONE: i16 = 12;

/// The taskbar strip along the bottom of the root. Sized for the same reason
/// the titlebars are: it is hit with a trackpad cursor, not a mouse, and on a
/// Deck in game mode it is the *only* way to get a minimised window back.
const TASKBAR_HEIGHT: i16 = 40;
/// Padding around each entry, so entries read as separate buttons.
const TASKBAR_GAP: i16 = 3;

const MIN_WIDTH: u16 = 120;
const MIN_HEIGHT: u16 = 60;

/// Offset between cascaded windows, so a game and LunaTranslator opening at
/// the same moment don't land exactly on top of each other.
const CASCADE_STEP: i16 = 36;

const KEYSYM_TAB: u32 = 0xff09;

// _NET_WM_MOVERESIZE directions.
const MOVERESIZE_TOPLEFT: u32 = 0;
const MOVERESIZE_TOP: u32 = 1;
const MOVERESIZE_TOPRIGHT: u32 = 2;
const MOVERESIZE_RIGHT: u32 = 3;
const MOVERESIZE_BOTTOMRIGHT: u32 = 4;
const MOVERESIZE_BOTTOM: u32 = 5;
const MOVERESIZE_BOTTOMLEFT: u32 = 6;
const MOVERESIZE_LEFT: u32 = 7;
const MOVERESIZE_MOVE: u32 = 8;
const MOVERESIZE_CANCEL: u32 = 11;

// _NET_WM_STATE actions.
const STATE_REMOVE: u32 = 0;
const STATE_ADD: u32 = 1;

/// `_MOTIF_WM_HINTS`: flags word bit for "the decorations field is set", and
/// the index of that field. Predates EWMH but is still how Wine and a lot of
/// toolkits ask for a borderless window.
const MOTIF_HINTS_DECORATIONS: u32 = 1 << 1;
const MOTIF_DECORATIONS_FIELD: usize = 2;

/// Which stacking layer a window sits in. Fixed layers, rather than one flat
/// stack, are what let a fullscreen game and an always-on-top translation
/// window both get what they asked for.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Layer {
    Desktop,
    Below,
    Normal,
    Above,
}

const LAYERS: [Layer; 4] = [Layer::Desktop, Layer::Below, Layer::Normal, Layer::Above];

/// The buttons in a titlebar's right-hand end.
#[derive(Clone, Copy, PartialEq, Eq)]
enum TitlebarButton {
    Close,
    Maximize,
    Minimize,
}

/// Where each titlebar button starts, laid out right to left: close,
/// maximize, minimise.
///
/// One function rather than two so the hit test and the drawing cannot drift
/// apart - which they had, leaving a minimise button that was documented,
/// undrawn and unclickable. Maximize is skipped entirely for a fixed-size
/// window, and minimise slides right to take its place.
fn titlebar_buttons(frame_width: u16, resizable: bool) -> Vec<(TitlebarButton, i16)> {
    let mut buttons = vec![TitlebarButton::Close];
    if resizable {
        buttons.push(TitlebarButton::Maximize);
    }
    buttons.push(TitlebarButton::Minimize);
    buttons
        .into_iter()
        .enumerate()
        .map(|(index, button)| {
            (
                button,
                frame_width as i16 - TITLEBAR_HEIGHT * (index as i16 + 1),
            )
        })
        .collect()
}

/// Core X fonts are 8-bit, so anything outside Latin-1 - a Japanese visual
/// novel's own title, typically - would render as mojibake. Dropping those
/// characters is better than drawing nonsense, and the caller falls back to
/// `WM_CLASS` when nothing survives.
fn ascii_only(text: &str) -> String {
    text.chars()
        .filter(|character| character.is_ascii() && !character.is_control())
        .collect()
}

#[derive(Clone, Copy, Default)]
struct ResizeEdges {
    left: bool,
    right: bool,
    top: bool,
    bottom: bool,
}

impl ResizeEdges {
    fn any(&self) -> bool {
        self.left || self.right || self.top || self.bottom
    }
}

struct Client {
    window: Window,
    /// Frame position, in root coordinates.
    x: i16,
    y: i16,
    /// Client (not frame) size - the frame adds chrome around it.
    width: u16,
    height: u16,

    fullscreen: bool,
    maximized: bool,
    iconified: bool,
    above: bool,
    below: bool,
    /// False for windows that asked for no decorations via `_MOTIF_WM_HINTS`,
    /// and for window types that shouldn't have them.
    decorated: bool,
    /// Windowed geometry stashed on the way into fullscreen/maximize, so
    /// leaving either puts the window back exactly where the user had it.
    restore: Option<(i16, i16, u16, u16)>,

    hints: WmSizeHints,
    transient_for: Option<Window>,
    /// The client asked to be given focus via `WM_TAKE_FOCUS` rather than
    /// having it set directly.
    takes_focus: bool,
    /// `WM_HINTS.input`: false means the client manages its own focus and
    /// `SetInputFocus` on it is wrong.
    accepts_input: bool,
}

impl Client {
    fn layer(&self) -> Layer {
        // Fullscreen sinks rather than rises: that is what keeps a floating
        // translator readable over a fullscreen game without having to deny
        // the game its fullscreen.
        if self.below || self.fullscreen {
            Layer::Below
        } else if self.above {
            Layer::Above
        } else {
            Layer::Normal
        }
    }

    /// Frame chrome as (left, top, right, bottom). Zero on every side when
    /// the window is fullscreen or undecorated.
    fn chrome(&self) -> (i16, i16, i16, i16) {
        if self.fullscreen || !self.decorated {
            (0, 0, 0, 0)
        } else {
            (BORDER, TITLEBAR_HEIGHT, BORDER, BOTTOM_BAR)
        }
    }

    fn frame_width(&self) -> u16 {
        let (left, _, right, _) = self.chrome();
        self.width.saturating_add((left + right) as u16)
    }

    fn frame_height(&self) -> u16 {
        let (_, top, _, bottom) = self.chrome();
        self.height.saturating_add((top + bottom) as u16)
    }

    /// Applies `WM_NORMAL_HINTS` to a proposed size.
    ///
    /// Without this a window with a fixed size gets stretched to whatever the
    /// user dragged, and a terminal-style window ignores its resize
    /// increments. Games routinely publish a fixed min == max size, and
    /// honouring that is the difference between a correctly-sized game and a
    /// smeared one.
    fn constrain(&self, width: u16, height: u16) -> (u16, u16) {
        let (mut width, mut height) = (i32::from(width), i32::from(height));

        let (base_width, base_height) = self
            .hints
            .base_size
            .or(self.hints.min_size)
            .unwrap_or((0, 0));

        if let Some((increment_width, increment_height)) = self.hints.size_increment {
            if increment_width > 0 {
                width = base_width + (width - base_width) / increment_width * increment_width;
            }
            if increment_height > 0 {
                height = base_height + (height - base_height) / increment_height * increment_height;
            }
        }

        if let Some((min_aspect, max_aspect)) = self.hints.aspect {
            // Aspect ratios apply to the size above the base size.
            let aspect_width = (width - base_width).max(1);
            let aspect_height = (height - base_height).max(1);
            if min_aspect.denominator > 0
                && aspect_width * min_aspect.denominator < aspect_height * min_aspect.numerator
            {
                width = base_width + aspect_height * min_aspect.numerator / min_aspect.denominator;
            }
            if max_aspect.numerator > 0
                && aspect_width * max_aspect.denominator > aspect_height * max_aspect.numerator
            {
                height = base_height + aspect_width * max_aspect.denominator / max_aspect.numerator;
            }
        }

        if let Some((min_width, min_height)) = self.hints.min_size {
            width = width.max(min_width);
            height = height.max(min_height);
        }
        if let Some((max_width, max_height)) = self.hints.max_size {
            if max_width > 0 {
                width = width.min(max_width);
            }
            if max_height > 0 {
                height = height.min(max_height);
            }
        }

        (
            width.clamp(i32::from(MIN_WIDTH), i32::from(u16::MAX)) as u16,
            height.clamp(i32::from(MIN_HEIGHT), i32::from(u16::MAX)) as u16,
        )
    }

    /// Whether the user is allowed to resize this window at all - a window
    /// whose hints pin min == max is fixed-size by definition.
    fn resizable(&self) -> bool {
        match (self.hints.min_size, self.hints.max_size) {
            (Some(min), Some(max)) => min != max,
            _ => true,
        }
    }
}

enum Drag {
    None,
    Move {
        frame: Window,
        offset_x: i16,
        offset_y: i16,
    },
    Resize {
        frame: Window,
        edges: ResizeEdges,
        origin_x: i16,
        origin_y: i16,
        start_x: i16,
        start_y: i16,
        start_width: u16,
        start_height: u16,
    },
}

x11rb::atom_manager! {
    Atoms: AtomsCookie {
        WM_PROTOCOLS,
        WM_DELETE_WINDOW,
        WM_TAKE_FOCUS,
        WM_STATE,
        WM_CHANGE_STATE,
        _NET_WM_STATE_HIDDEN,
        UTF8_STRING,
        _MOTIF_WM_HINTS,
        _NET_WM_NAME,
        _NET_SUPPORTED,
        _NET_SUPPORTING_WM_CHECK,
        _NET_ACTIVE_WINDOW,
        _NET_CLIENT_LIST,
        _NET_CLOSE_WINDOW,
        _NET_FRAME_EXTENTS,
        _NET_WM_MOVERESIZE,
        _NET_WM_STATE,
        _NET_WM_STATE_FULLSCREEN,
        _NET_WM_STATE_MAXIMIZED_HORZ,
        _NET_WM_STATE_MAXIMIZED_VERT,
        _NET_WM_STATE_ABOVE,
        _NET_WM_STATE_BELOW,
        _NET_WM_STATE_MODAL,
        _NET_WM_STATE_SKIP_TASKBAR,
        _NET_WM_STATE_SKIP_PAGER,
        _NET_WM_ALLOWED_ACTIONS,
        _NET_WM_ACTION_MOVE,
        _NET_WM_ACTION_RESIZE,
        _NET_WM_ACTION_CLOSE,
        _NET_WM_ACTION_FULLSCREEN,
        _NET_WM_ACTION_MAXIMIZE_HORZ,
        _NET_WM_ACTION_MAXIMIZE_VERT,
        _NET_WM_ACTION_MINIMIZE,
        _NET_WM_WINDOW_TYPE,
        _NET_WM_WINDOW_TYPE_NORMAL,
        _NET_WM_WINDOW_TYPE_DIALOG,
        _NET_WM_WINDOW_TYPE_UTILITY,
        _NET_WM_WINDOW_TYPE_TOOLBAR,
        _NET_WM_WINDOW_TYPE_MENU,
        _NET_WM_WINDOW_TYPE_SPLASH,
        _NET_WM_WINDOW_TYPE_DOCK,
        _NET_WM_WINDOW_TYPE_DESKTOP,
        _NET_WM_WINDOW_TYPE_DROPDOWN_MENU,
        _NET_WM_WINDOW_TYPE_POPUP_MENU,
        _NET_WM_WINDOW_TYPE_TOOLTIP,
        _NET_WM_WINDOW_TYPE_NOTIFICATION,
        _NET_WM_WINDOW_TYPE_COMBO,
        _NET_WM_WINDOW_TYPE_DND,
        _NET_NUMBER_OF_DESKTOPS,
        _NET_CURRENT_DESKTOP,
        _NET_DESKTOP_GEOMETRY,
        _NET_DESKTOP_VIEWPORT,
        _NET_WORKAREA,
        _NET_WM_DESKTOP,
    }
}

pub struct WindowManager {
    conn: RustConnection,
    root: Window,
    root_width: u16,
    root_height: u16,
    depth: u8,
    visual: u32,
    atoms: Atoms,
    gc: Gcontext,
    has_font: bool,
    /// Keyed by frame window; `by_client` maps the other way.
    clients: HashMap<Window, Client>,
    by_client: HashMap<Window, Window>,
    /// Frame order, bottom-most first within each layer.
    order: Vec<Window>,
    /// Our own taskbar window. Never a client: it is override-redirect, it is
    /// not in `clients`/`order`/`_NET_CLIENT_LIST`, and Super+Tab skips it.
    taskbar: Window,
    /// Frames in the order they were first managed, which is what the taskbar
    /// is laid out from. Deliberately *not* `order` - that is stacking order,
    /// and entries that reshuffle themselves under the cursor every time one
    /// is clicked are unusable.
    taskbar_order: Vec<Window>,
    /// Windows we map but deliberately don't frame, paired with the layer
    /// they belong in. Their geometry is the client's business - a dropdown
    /// has to land exactly under its combo box - but their *stacking* is
    /// ours, or a tooltip ends up behind the window that opened it.
    unframed: Vec<(Window, Layer)>,
    /// Unmaps we caused ourselves and must not mistake for a client
    /// withdrawing - see the UnmapNotify handler.
    ignore_unmap: HashMap<Window, u32>,
    focused: Option<Window>,
    drag: Drag,
    cascade: i16,
    cycle_keycode: Option<u8>,
    /// Most recent server timestamp seen. `WM_TAKE_FOCUS` must carry a real
    /// one; CURRENT_TIME is explicitly forbidden there by ICCCM.
    last_time: Timestamp,
}

fn x11<E: std::fmt::Display>(error: E) -> NestedSessionError {
    NestedSessionError::X11(error.to_string())
}

impl WindowManager {
    /// Connects to `display`, takes over as its window manager, and runs
    /// until the connection drops - which is how the session is torn down:
    /// the caller kills the X server and this returns.
    ///
    /// Reports the outcome of *taking over* (as opposed to the outcome of the
    /// whole session) through `ready`, so the caller can fail the launch
    /// outright rather than dropping a game into an unmanaged root window
    /// where nothing would be movable.
    pub fn start(display: &str, ready: &Sender<Result<(), NestedSessionError>>) {
        let mut wm = match WindowManager::connect(display) {
            Ok(wm) => {
                let _ = ready.send(Ok(()));
                wm
            }
            Err(e) => {
                let _ = ready.send(Err(e));
                return;
            }
        };
        if let Err(e) = wm.adopt_existing_windows() {
            warn!("nested session: failed to adopt pre-existing windows: {e}");
        }
        if let Err(e) = wm.event_loop() {
            warn!("nested session window manager stopped: {e}");
        }
    }

    fn connect(display: &str) -> Result<Self, NestedSessionError> {
        // The socket appears a moment before the server accepts on it, so a
        // first connect can legitimately fail on a fast machine.
        let mut attempt = RustConnection::connect(Some(display));
        for _ in 0..20 {
            if attempt.is_ok() {
                break;
            }
            sleep(Duration::from_millis(100));
            attempt = RustConnection::connect(Some(display));
        }
        let (conn, screen_number) = attempt.map_err(x11)?;
        let screen: &Screen = &conn.setup().roots[screen_number];
        let root = screen.root;
        let root_width = screen.width_in_pixels;
        let root_height = screen.height_in_pixels;
        let depth = screen.root_depth;
        let visual = screen.root_visual;

        // Asking for SubstructureRedirect is what *makes* us the window
        // manager; exactly one client may hold it, so a failure here means
        // something else got there first.
        conn.change_window_attributes(
            root,
            &ChangeWindowAttributesAux::new()
                .event_mask(
                    EventMask::SUBSTRUCTURE_REDIRECT
                        | EventMask::SUBSTRUCTURE_NOTIFY
                        | EventMask::PROPERTY_CHANGE,
                )
                .background_pixel(COLOUR_ROOT),
        )
        .map_err(x11)?
        .check()
        .map_err(|_| NestedSessionError::WindowManagerConflict)?;
        conn.clear_area(false, root, 0, 0, 0, 0).map_err(x11)?;

        let atoms = Atoms::new(&conn).map_err(x11)?.reply().map_err(x11)?;

        let gc = conn.generate_id().map_err(x11)?;
        conn.create_gc(gc, root, &CreateGCAux::new().graphics_exposures(0))
            .map_err(x11)?;

        // Core X fonts are optional on a minimal host (SteamOS ships no
        // xorg-fonts-misc, for instance). Titles are a nicety, frames and
        // buttons are not - so if no font opens, draw everything except the
        // text and carry on.
        let font = conn.generate_id().map_err(x11)?;
        let has_font = [
            b"fixed".as_slice(),
            b"9x15".as_slice(),
            b"cursor".as_slice(),
        ]
        .iter()
        .any(|name| {
            conn.open_font(font, name)
                .ok()
                .and_then(|cookie| cookie.check().ok())
                .is_some()
        });
        if has_font {
            conn.change_gc(gc, &ChangeGCAux::new().font(font))
                .map_err(x11)?;
        } else {
            info!("nested session: no core X font available, drawing titlebars without titles");
        }

        let mut wm = WindowManager {
            conn,
            root,
            root_width,
            root_height,
            depth,
            visual,
            atoms,
            gc,
            has_font,
            clients: HashMap::new(),
            by_client: HashMap::new(),
            order: Vec::new(),
            taskbar: NONE,
            taskbar_order: Vec::new(),
            unframed: Vec::new(),
            ignore_unmap: HashMap::new(),
            focused: None,
            drag: Drag::None,
            cascade: 0,
            cycle_keycode: None,
            last_time: CURRENT_TIME,
        };

        wm.create_taskbar()?;
        wm.announce_ewmh()?;
        wm.grab_cycle_key()?;
        wm.conn.flush().map_err(x11)?;

        info!(
            "nested session window manager running on {display} ({}x{})",
            root_width, root_height
        );
        Ok(wm)
    }

    /// Publishes the EWMH a game or a Qt app will actually look for.
    ///
    /// `_NET_WM_STATE_FULLSCREEN` is in here deliberately. Leaving it out
    /// does not keep games windowed - they go fullscreen anyway by resizing
    /// themselves to the screen, and all that's achieved is that we don't
    /// *know* they did, so they end up with a frame larger than the root and
    /// their client area shoved down under a titlebar. LunaTranslator stays
    /// visible over a fullscreen game because of the stacking layers, not
    /// because the game was refused.
    ///
    /// The desktop/workarea properties matter more than they look: Qt reads
    /// them to decide where its own dialogs and popups go, and a toolkit that
    /// finds none of them falls back to guesses that land badly.
    fn announce_ewmh(&mut self) -> Result<(), NestedSessionError> {
        let check_window = self.conn.generate_id().map_err(x11)?;
        self.conn
            .create_window(
                self.depth,
                check_window,
                self.root,
                -1,
                -1,
                1,
                1,
                0,
                WindowClass::INPUT_OUTPUT,
                self.visual,
                &CreateWindowAux::new().override_redirect(1),
            )
            .map_err(x11)?;

        for window in [self.root, check_window] {
            self.conn
                .change_property32(
                    PropMode::REPLACE,
                    window,
                    self.atoms._NET_SUPPORTING_WM_CHECK,
                    AtomEnum::WINDOW,
                    &[check_window],
                )
                .map_err(x11)?;
        }
        self.conn
            .change_property8(
                PropMode::REPLACE,
                check_window,
                self.atoms._NET_WM_NAME,
                self.atoms.UTF8_STRING,
                b"Drop Nested Session",
            )
            .map_err(x11)?;

        self.conn
            .change_property32(
                PropMode::REPLACE,
                self.root,
                self.atoms._NET_SUPPORTED,
                AtomEnum::ATOM,
                &[
                    self.atoms._NET_SUPPORTING_WM_CHECK,
                    self.atoms._NET_ACTIVE_WINDOW,
                    self.atoms._NET_CLIENT_LIST,
                    self.atoms._NET_CLOSE_WINDOW,
                    self.atoms._NET_WM_NAME,
                    self.atoms._NET_FRAME_EXTENTS,
                    self.atoms._NET_WM_MOVERESIZE,
                    self.atoms._NET_WM_STATE,
                    self.atoms._NET_WM_STATE_FULLSCREEN,
                    self.atoms._NET_WM_STATE_MAXIMIZED_HORZ,
                    self.atoms._NET_WM_STATE_MAXIMIZED_VERT,
                    self.atoms._NET_WM_STATE_ABOVE,
                    self.atoms._NET_WM_STATE_BELOW,
                    self.atoms._NET_WM_STATE_HIDDEN,
                    self.atoms._NET_WM_STATE_MODAL,
                    self.atoms._NET_WM_STATE_SKIP_TASKBAR,
                    self.atoms._NET_WM_STATE_SKIP_PAGER,
                    self.atoms._NET_WM_ALLOWED_ACTIONS,
                    self.atoms._NET_WM_WINDOW_TYPE,
                    self.atoms._NET_WM_WINDOW_TYPE_NORMAL,
                    self.atoms._NET_WM_WINDOW_TYPE_DIALOG,
                    self.atoms._NET_WM_WINDOW_TYPE_UTILITY,
                    self.atoms._NET_WM_WINDOW_TYPE_TOOLBAR,
                    self.atoms._NET_WM_WINDOW_TYPE_MENU,
                    self.atoms._NET_WM_WINDOW_TYPE_SPLASH,
                    self.atoms._NET_WM_WINDOW_TYPE_DROPDOWN_MENU,
                    self.atoms._NET_WM_WINDOW_TYPE_POPUP_MENU,
                    self.atoms._NET_WM_WINDOW_TYPE_TOOLTIP,
                    self.atoms._NET_WM_WINDOW_TYPE_NOTIFICATION,
                    self.atoms._NET_WM_WINDOW_TYPE_COMBO,
                    self.atoms._NET_WM_WINDOW_TYPE_DND,
                    self.atoms._NET_NUMBER_OF_DESKTOPS,
                    self.atoms._NET_CURRENT_DESKTOP,
                    self.atoms._NET_DESKTOP_GEOMETRY,
                    self.atoms._NET_DESKTOP_VIEWPORT,
                    self.atoms._NET_WORKAREA,
                    self.atoms._NET_WM_DESKTOP,
                ],
            )
            .map_err(x11)?;

        // A single, fixed desktop covering the whole root - but a work area
        // that stops above the taskbar, so a maximized window doesn't put its
        // bottom edge (and whatever lives there) underneath it.
        let (width, height) = (u32::from(self.root_width), u32::from(self.root_height));
        let (area_x, area_y, area_width, area_height) = self.work_area();
        for (property, value) in [
            (self.atoms._NET_NUMBER_OF_DESKTOPS, vec![1u32]),
            (self.atoms._NET_CURRENT_DESKTOP, vec![0]),
            (self.atoms._NET_DESKTOP_GEOMETRY, vec![width, height]),
            (self.atoms._NET_DESKTOP_VIEWPORT, vec![0, 0]),
            (
                self.atoms._NET_WORKAREA,
                vec![
                    area_x as u32,
                    area_y as u32,
                    u32::from(area_width),
                    u32::from(area_height),
                ],
            ),
        ] {
            self.conn
                .change_property32(
                    PropMode::REPLACE,
                    self.root,
                    property,
                    AtomEnum::CARDINAL,
                    &value,
                )
                .map_err(x11)?;
        }
        Ok(())
    }

    /// Grabs Super+Tab to raise the next window. A game with an active
    /// pointer grab swallows titlebar clicks, and this is the way back out of
    /// that without reaching for a mouse.
    fn grab_cycle_key(&mut self) -> Result<(), NestedSessionError> {
        let setup = self.conn.setup();
        let min_keycode = setup.min_keycode;
        let count = setup.max_keycode - min_keycode + 1;
        let mapping = self
            .conn
            .get_keyboard_mapping(min_keycode, count)
            .map_err(x11)?
            .reply()
            .map_err(x11)?;

        let per_keycode = mapping.keysyms_per_keycode as usize;
        let Some(index) = mapping
            .keysyms
            .iter()
            .position(|keysym| *keysym == KEYSYM_TAB)
        else {
            debug!("nested session: no Tab key in the keymap, skipping the cycle shortcut");
            return Ok(());
        };
        let keycode = min_keycode + (index / per_keycode.max(1)) as u8;

        // NumLock (M2) and CapsLock latch into the modifier state, so each
        // combination has to be grabbed separately - a single grab on M4
        // alone silently stops working the moment NumLock is on.
        for extra in [
            ModMask::default(),
            ModMask::LOCK,
            ModMask::M2,
            ModMask::LOCK | ModMask::M2,
        ] {
            self.conn
                .grab_key(
                    false,
                    self.root,
                    ModMask::M4 | extra,
                    keycode,
                    GrabMode::ASYNC,
                    GrabMode::ASYNC,
                )
                .map_err(x11)?;
        }
        self.cycle_keycode = Some(keycode);
        Ok(())
    }

    /// Frames anything that was already mapped before we connected.
    fn adopt_existing_windows(&mut self) -> Result<(), NestedSessionError> {
        let tree = self
            .conn
            .query_tree(self.root)
            .map_err(x11)?
            .reply()
            .map_err(x11)?;
        for window in tree.children {
            let attributes = match self
                .conn
                .get_window_attributes(window)
                .map_err(x11)?
                .reply()
            {
                Ok(attributes) => attributes,
                Err(_) => continue,
            };
            if attributes.override_redirect || attributes.map_state == MapState::UNMAPPED {
                continue;
            }
            self.manage(window)?;
        }
        self.conn.flush().map_err(x11)?;
        Ok(())
    }

    fn event_loop(&mut self) -> Result<(), NestedSessionError> {
        loop {
            let event = match self.conn.wait_for_event() {
                Ok(event) => event,
                // The X server going away is the normal end of a session -
                // that's how the caller shuts us down.
                Err(e) => {
                    info!("nested session window manager exiting: {e}");
                    return Ok(());
                }
            };
            if let Err(e) = self.handle_event(event) {
                warn!("nested session: error handling event: {e}");
            }
            if let Err(e) = self.conn.flush() {
                info!("nested session window manager exiting: {e}");
                return Ok(());
            }
        }
    }

    fn handle_event(&mut self, event: Event) -> Result<(), NestedSessionError> {
        match event {
            Event::MapRequest(event) => {
                self.manage(event.window)?;
            }
            Event::ConfigureRequest(event) => {
                self.configure_request(event)?;
            }
            Event::UnmapNotify(event) => {
                // Only unmaps reported through the parent (frame or root)
                // mean the client actually withdrew; the copy delivered to
                // the window itself is our own reparenting.
                if event.event != event.window {
                    // Reparenting a window that was already mapped - which is
                    // every window we adopt at startup - unmaps it as a side
                    // effect. That's ours, not the client withdrawing.
                    if let Some(remaining) = self.ignore_unmap.get_mut(&event.window)
                        && *remaining > 0
                    {
                        *remaining -= 1;
                        return Ok(());
                    }
                    self.unmanage(event.window)?;
                }
            }
            Event::DestroyNotify(event) => {
                self.unmanage(event.window)?;
            }
            Event::ConfigureNotify(event) => {
                // Only the root's own size is news. A managed client cannot
                // resize itself behind our back - SubstructureRedirect on its
                // frame turns that into a ConfigureRequest - so every
                // ConfigureNotify for a client is just the echo of a resize
                // we performed. Acting on those means acting on stale sizes
                // that race our own requests, which desyncs the frame from
                // the client it wraps (clipped windows, leftover chrome, a
                // game re-sized to a stale geometry when it regains focus).
                if event.window == self.root {
                    self.root_resized(event.width, event.height)?;
                }
            }
            Event::PropertyNotify(event) => {
                self.last_time = event.time;
                self.property_changed(event.window, event.atom)?;
            }
            Event::Expose(event) => {
                if event.window == self.taskbar {
                    self.draw_taskbar()?;
                } else if self.clients.contains_key(&event.window) {
                    self.draw_frame(event.window)?;
                }
            }
            Event::ButtonPress(event) => {
                self.last_time = event.time;
                self.button_press(event)?;
            }
            Event::MotionNotify(event) => {
                self.last_time = event.time;
                self.motion(event.root_x, event.root_y)?;
            }
            Event::ButtonRelease(event) => {
                self.last_time = event.time;
                self.end_drag()?;
            }
            Event::KeyPress(event) => {
                self.last_time = event.time;
                if Some(event.detail) == self.cycle_keycode {
                    self.cycle_windows()?;
                }
            }
            Event::ClientMessage(event) => {
                self.client_message(event)?;
            }
            _ => {}
        }
        Ok(())
    }

    // ── properties ────────────────────────────────────────────────────────

    /// Clients change their hints and titles while running - a dialog that
    /// becomes fixed-size, a window that turns its decorations off - so these
    /// have to be re-read rather than captured once at map time.
    fn property_changed(&mut self, window: Window, atom: u32) -> Result<(), NestedSessionError> {
        let Some(frame) = self.by_client.get(&window).copied() else {
            return Ok(());
        };

        if atom == u32::from(AtomEnum::WM_NAME) || atom == self.atoms._NET_WM_NAME {
            self.draw_frame(frame)?;
            self.draw_taskbar()?;
        } else if atom == u32::from(AtomEnum::WM_NORMAL_HINTS) {
            let hints = self.read_size_hints(window);
            info!(
                "nested session: {window} changed WM_NORMAL_HINTS min={:?} max={:?}",
                hints.min_size, hints.max_size
            );
            if let Some(client) = self.clients.get_mut(&frame) {
                client.hints = hints;
                let (width, height) = client.constrain(client.width, client.height);
                client.width = width;
                client.height = height;
            }
            self.apply_geometry(frame)?;
            self.publish_allowed_actions(frame)?;
        } else if atom == u32::from(AtomEnum::WM_HINTS) {
            let (accepts_input, takes_focus) = self.read_focus_model(window);
            if let Some(client) = self.clients.get_mut(&frame) {
                client.accepts_input = accepts_input;
                client.takes_focus = takes_focus;
            }
        } else if atom == self.atoms._MOTIF_WM_HINTS {
            let decorated = self.read_motif_decorated(window);
            let changed = self.clients.get_mut(&frame).is_some_and(|client| {
                let changed = client.decorated != decorated;
                client.decorated = decorated;
                changed
            });
            if changed {
                self.apply_geometry(frame)?;
                self.publish_frame_extents(frame)?;
            }
        } else if atom == u32::from(AtomEnum::WM_TRANSIENT_FOR) {
            let transient_for = self.read_transient_for(window);
            if let Some(client) = self.clients.get_mut(&frame) {
                client.transient_for = transient_for;
            }
        }
        Ok(())
    }

    fn read_size_hints(&self, window: Window) -> WmSizeHints {
        WmSizeHints::get_normal_hints(&self.conn, window)
            .ok()
            .and_then(|cookie| cookie.reply().ok())
            .flatten()
            .unwrap_or_default()
    }

    /// `WM_HINTS.input` plus `WM_TAKE_FOCUS`, which together say how a client
    /// wants to be given focus. Qt apps use the "locally active" model, and
    /// calling `SetInputFocus` on a client that asked for `WM_TAKE_FOCUS`
    /// instead is how you get a window that looks focused but ignores the
    /// keyboard.
    fn read_focus_model(&self, window: Window) -> (bool, bool) {
        let accepts_input = WmHints::get(&self.conn, window)
            .ok()
            .and_then(|cookie| cookie.reply().ok())
            .flatten()
            .and_then(|hints| hints.input)
            .unwrap_or(true);

        let takes_focus = self
            .window_protocols(window)
            .contains(&self.atoms.WM_TAKE_FOCUS);

        (accepts_input, takes_focus)
    }

    fn window_protocols(&self, window: Window) -> Vec<u32> {
        self.conn
            .get_property(
                false,
                window,
                self.atoms.WM_PROTOCOLS,
                AtomEnum::ATOM,
                0,
                32,
            )
            .ok()
            .and_then(|cookie| cookie.reply().ok())
            .and_then(|reply| reply.value32().map(|values| values.collect()))
            .unwrap_or_default()
    }

    fn read_motif_decorated(&self, window: Window) -> bool {
        let hints: Vec<u32> = self
            .conn
            .get_property(
                false,
                window,
                self.atoms._MOTIF_WM_HINTS,
                AtomEnum::ANY,
                0,
                5,
            )
            .ok()
            .and_then(|cookie| cookie.reply().ok())
            .and_then(|reply| reply.value32().map(|values| values.collect()))
            .unwrap_or_default();

        match hints.first() {
            Some(flags) if flags & MOTIF_HINTS_DECORATIONS != 0 => hints
                .get(MOTIF_DECORATIONS_FIELD)
                .is_none_or(|decorations| *decorations != 0),
            _ => true,
        }
    }

    fn read_transient_for(&self, window: Window) -> Option<Window> {
        self.conn
            .get_property(
                false,
                window,
                AtomEnum::WM_TRANSIENT_FOR,
                AtomEnum::WINDOW,
                0,
                1,
            )
            .ok()
            .and_then(|cookie| cookie.reply().ok())
            .and_then(|reply| reply.value32().and_then(|mut values| values.next()))
            .filter(|parent| *parent != NONE)
    }

    fn window_types(&self, window: Window) -> Vec<u32> {
        self.conn
            .get_property(
                false,
                window,
                self.atoms._NET_WM_WINDOW_TYPE,
                AtomEnum::ATOM,
                0,
                16,
            )
            .ok()
            .and_then(|cookie| cookie.reply().ok())
            .and_then(|reply| reply.value32().map(|values| values.collect()))
            .unwrap_or_default()
    }

    /// Types whose *geometry* is the client's own business, so we map them
    /// as-is instead of framing them - and the layer each belongs in, since
    /// their stacking is still ours to get right.
    ///
    /// Nothing is hidden or dropped here; a frame is simply the wrong shape
    /// for these. Qt does *not* always set override-redirect on them - menus,
    /// combo popups, tooltips and drag pixmaps can all arrive as ordinary
    /// managed windows - so checking override-redirect alone is not enough,
    /// and a titlebar on a tooltip is what you get if you skip it.
    ///
    /// SPLASH and MENU are deliberately absent: a splash screen is a real
    /// toplevel that just shouldn't be *decorated*, and a torn-off menu is an
    /// ordinary window. Declining to frame those would leave them
    /// unpositioned and unable to go fullscreen - and for a game whose first
    /// window is a splash, that means it is never managed at all.
    fn unframed_layer(&self, types: &[u32]) -> Option<Layer> {
        for kind in types {
            if *kind == self.atoms._NET_WM_WINDOW_TYPE_DESKTOP {
                return Some(Layer::Desktop);
            }
            if *kind == self.atoms._NET_WM_WINDOW_TYPE_DOCK
                || *kind == self.atoms._NET_WM_WINDOW_TYPE_DROPDOWN_MENU
                || *kind == self.atoms._NET_WM_WINDOW_TYPE_POPUP_MENU
                || *kind == self.atoms._NET_WM_WINDOW_TYPE_TOOLTIP
                || *kind == self.atoms._NET_WM_WINDOW_TYPE_NOTIFICATION
                || *kind == self.atoms._NET_WM_WINDOW_TYPE_COMBO
                || *kind == self.atoms._NET_WM_WINDOW_TYPE_DND
            {
                return Some(Layer::Above);
            }
        }
        None
    }

    /// Managed and stacked like anything else, but with no chrome of its own.
    fn is_undecorated_type(&self, types: &[u32]) -> bool {
        types.contains(&self.atoms._NET_WM_WINDOW_TYPE_SPLASH)
    }

    /// A human-readable name for whatever `_NET_WM_WINDOW_TYPE` says, purely
    /// so the log can explain why a window was treated the way it was.
    fn describe_types(&self, types: &[u32]) -> String {
        let named = [
            (self.atoms._NET_WM_WINDOW_TYPE_NORMAL, "normal"),
            (self.atoms._NET_WM_WINDOW_TYPE_DIALOG, "dialog"),
            (self.atoms._NET_WM_WINDOW_TYPE_UTILITY, "utility"),
            (self.atoms._NET_WM_WINDOW_TYPE_TOOLBAR, "toolbar"),
            (self.atoms._NET_WM_WINDOW_TYPE_MENU, "menu"),
            (self.atoms._NET_WM_WINDOW_TYPE_SPLASH, "splash"),
            (self.atoms._NET_WM_WINDOW_TYPE_DOCK, "dock"),
            (self.atoms._NET_WM_WINDOW_TYPE_DESKTOP, "desktop"),
            (self.atoms._NET_WM_WINDOW_TYPE_DROPDOWN_MENU, "dropdown"),
            (self.atoms._NET_WM_WINDOW_TYPE_POPUP_MENU, "popup"),
            (self.atoms._NET_WM_WINDOW_TYPE_TOOLTIP, "tooltip"),
            (self.atoms._NET_WM_WINDOW_TYPE_NOTIFICATION, "notification"),
            (self.atoms._NET_WM_WINDOW_TYPE_COMBO, "combo"),
            (self.atoms._NET_WM_WINDOW_TYPE_DND, "dnd"),
        ];
        let described: Vec<&str> = types
            .iter()
            .filter_map(|kind| {
                named
                    .iter()
                    .find(|(atom, _)| atom == kind)
                    .map(|(_, name)| *name)
            })
            .collect();
        if described.is_empty() {
            "unset".to_owned()
        } else {
            described.join("+")
        }
    }

    fn initial_states(&self, window: Window) -> (bool, bool, bool, bool) {
        let states: Vec<u32> = self
            .conn
            .get_property(
                false,
                window,
                self.atoms._NET_WM_STATE,
                AtomEnum::ATOM,
                0,
                32,
            )
            .ok()
            .and_then(|cookie| cookie.reply().ok())
            .and_then(|reply| reply.value32().map(|values| values.collect()))
            .unwrap_or_default();

        (
            states.contains(&self.atoms._NET_WM_STATE_FULLSCREEN),
            states.contains(&self.atoms._NET_WM_STATE_MAXIMIZED_HORZ)
                && states.contains(&self.atoms._NET_WM_STATE_MAXIMIZED_VERT),
            states.contains(&self.atoms._NET_WM_STATE_ABOVE),
            states.contains(&self.atoms._NET_WM_STATE_BELOW),
        )
    }

    fn publish_state(&self, frame: Window) -> Result<(), NestedSessionError> {
        let Some(client) = self.clients.get(&frame) else {
            return Ok(());
        };
        let mut states = Vec::new();
        if client.fullscreen {
            states.push(self.atoms._NET_WM_STATE_FULLSCREEN);
        }
        if client.maximized {
            states.push(self.atoms._NET_WM_STATE_MAXIMIZED_HORZ);
            states.push(self.atoms._NET_WM_STATE_MAXIMIZED_VERT);
        }
        if client.above {
            states.push(self.atoms._NET_WM_STATE_ABOVE);
        }
        if client.below {
            states.push(self.atoms._NET_WM_STATE_BELOW);
        }
        if client.iconified {
            states.push(self.atoms._NET_WM_STATE_HIDDEN);
        }
        self.conn
            .change_property32(
                PropMode::REPLACE,
                client.window,
                self.atoms._NET_WM_STATE,
                AtomEnum::ATOM,
                &states,
            )
            .map_err(x11)?;
        Ok(())
    }

    /// `_NET_FRAME_EXTENTS` - how much chrome we put around the client.
    /// Toolkits subtract this when they position a window by its outer edge;
    /// without it, Qt's own "move this dialog to x,y" lands off by a
    /// titlebar every time.
    fn publish_frame_extents(&self, frame: Window) -> Result<(), NestedSessionError> {
        let Some(client) = self.clients.get(&frame) else {
            return Ok(());
        };
        let (left, top, right, bottom) = client.chrome();
        self.conn
            .change_property32(
                PropMode::REPLACE,
                client.window,
                self.atoms._NET_FRAME_EXTENTS,
                AtomEnum::CARDINAL,
                &[left as u32, right as u32, top as u32, bottom as u32],
            )
            .map_err(x11)?;
        Ok(())
    }

    fn publish_allowed_actions(&self, frame: Window) -> Result<(), NestedSessionError> {
        let Some(client) = self.clients.get(&frame) else {
            return Ok(());
        };
        let mut actions = vec![
            self.atoms._NET_WM_ACTION_MOVE,
            self.atoms._NET_WM_ACTION_CLOSE,
            self.atoms._NET_WM_ACTION_FULLSCREEN,
            self.atoms._NET_WM_ACTION_MINIMIZE,
        ];
        if client.resizable() {
            actions.push(self.atoms._NET_WM_ACTION_RESIZE);
            actions.push(self.atoms._NET_WM_ACTION_MAXIMIZE_HORZ);
            actions.push(self.atoms._NET_WM_ACTION_MAXIMIZE_VERT);
        }
        self.conn
            .change_property32(
                PropMode::REPLACE,
                client.window,
                self.atoms._NET_WM_ALLOWED_ACTIONS,
                AtomEnum::ATOM,
                &actions,
            )
            .map_err(x11)?;
        Ok(())
    }

    // ── geometry ──────────────────────────────────────────────────────────

    fn frame_rect(&self, client: &Client) -> (i16, i16, u16, u16) {
        (
            client.x,
            client.y,
            client.frame_width(),
            client.frame_height(),
        )
    }

    fn client_rect(&self, client: &Client) -> (i16, i16, u16, u16) {
        let (left, top, _, _) = client.chrome();
        (left, top, client.width, client.height)
    }

    /// Pushes a client's current geometry to the server: frame, client, the
    /// synthetic ConfigureNotify that tells the client where it ended up, and
    /// a redraw of the chrome.
    fn apply_geometry(&mut self, frame: Window) -> Result<(), NestedSessionError> {
        let (window, fx, fy, frame_width, frame_height, cx, cy, width, height) = {
            let Some(client) = self.clients.get(&frame) else {
                return Ok(());
            };
            let (fx, fy, frame_width, frame_height) = self.frame_rect(client);
            let (cx, cy, width, height) = self.client_rect(client);
            (
                client.window,
                fx,
                fy,
                frame_width,
                frame_height,
                cx,
                cy,
                width,
                height,
            )
        };

        self.conn
            .configure_window(
                frame,
                &ConfigureWindowAux::new()
                    .x(fx as i32)
                    .y(fy as i32)
                    .width(frame_width as u32)
                    .height(frame_height as u32),
            )
            .map_err(x11)?;
        self.conn
            .configure_window(
                window,
                &ConfigureWindowAux::new()
                    .x(cx as i32)
                    .y(cy as i32)
                    .width(width as u32)
                    .height(height as u32),
            )
            .map_err(x11)?;
        self.notify_geometry(window, fx + cx, fy + cy, width, height)?;
        self.draw_frame(frame)?;
        Ok(())
    }

    /// The synthetic ConfigureNotify ICCCM requires a reparenting window
    /// manager to send.
    ///
    /// Without it a client only ever sees the real ConfigureNotify, whose
    /// x/y are relative to the frame we reparented it into - so it believes
    /// it is at the chrome offset on screen and never learns otherwise. Wine
    /// is particularly unforgiving: it tracks the window rect from these
    /// events and hands it to the game, so a missing one shows up as the game
    /// rendering at the wrong size in the wrong place. Any *move* must send
    /// one too, not just a resize.
    fn notify_geometry(
        &self,
        window: Window,
        x: i16,
        y: i16,
        width: u16,
        height: u16,
    ) -> Result<(), NestedSessionError> {
        let event = ConfigureNotifyEvent {
            response_type: CONFIGURE_NOTIFY_EVENT,
            sequence: 0,
            event: window,
            window,
            above_sibling: NONE,
            x,
            y,
            width,
            height,
            border_width: 0,
            override_redirect: false,
        };
        self.conn
            .send_event(false, window, EventMask::STRUCTURE_NOTIFY, event)
            .map_err(x11)?;
        Ok(())
    }

    /// Restacks every frame into its layer. Within a layer, `order` is the
    /// stacking order, so raising a window is "move it to the end of order,
    /// then restack".
    fn restack(&self) -> Result<(), NestedSessionError> {
        let mut previous: Option<Window> = None;
        let mut stack = |conn: &RustConnection, window: Window| -> Result<(), NestedSessionError> {
            let aux = match previous {
                Some(sibling) => ConfigureWindowAux::new()
                    .sibling(sibling)
                    .stack_mode(StackMode::ABOVE),
                None => ConfigureWindowAux::new().stack_mode(StackMode::BELOW),
            };
            conn.configure_window(window, &aux).map_err(x11)?;
            previous = Some(window);
            Ok(())
        };

        for layer in LAYERS {
            for frame in &self.order {
                if self.clients.get(frame).map(Client::layer) == Some(layer) {
                    stack(&self.conn, *frame)?;
                }
            }
            // Unframed windows sit in the same layers as everything else -
            // that's the whole reason we track them.
            for (window, window_layer) in &self.unframed {
                if *window_layer == layer {
                    stack(&self.conn, *window)?;
                }
            }
        }
        // Above every layer, including Above: it is the only way back to a
        // minimised window, so nothing may cover it. A fullscreen game is in
        // the Below layer anyway, and still *occupies* the whole root - it
        // just doesn't get to hide the strip.
        if self.taskbar != NONE {
            stack(&self.conn, self.taskbar)?;
        }
        Ok(())
    }

    /// Moves a frame to the top of its layer, dragging any windows that are
    /// transient for it along - a dialog must not end up behind the window it
    /// belongs to.
    fn raise(&mut self, frame: Window) {
        let Some(window) = self.clients.get(&frame).map(|client| client.window) else {
            return;
        };
        self.order.retain(|candidate| *candidate != frame);
        self.order.push(frame);

        let children: Vec<Window> = self
            .order
            .iter()
            .filter(|candidate| {
                self.clients
                    .get(candidate)
                    .and_then(|client| client.transient_for)
                    == Some(window)
            })
            .copied()
            .collect();
        for child in children {
            self.order.retain(|candidate| *candidate != child);
            self.order.push(child);
        }
    }

    /// The root minus the taskbar. Fullscreen deliberately ignores this and
    /// uses the whole root: a game that asked for the screen gets the screen,
    /// and the taskbar simply sits on top of it in the stacking order.
    fn work_area(&self) -> (i16, i16, u16, u16) {
        (
            0,
            0,
            self.root_width,
            self.root_height.saturating_sub(TASKBAR_HEIGHT as u16),
        )
    }

    fn set_fullscreen(
        &mut self,
        frame: Window,
        fullscreen: bool,
    ) -> Result<(), NestedSessionError> {
        let (root_width, root_height) = (self.root_width, self.root_height);
        {
            let Some(client) = self.clients.get_mut(&frame) else {
                return Ok(());
            };
            if client.fullscreen == fullscreen {
                return Ok(());
            }
            if fullscreen {
                if client.restore.is_none() {
                    client.restore = Some((client.x, client.y, client.width, client.height));
                }
                // EWMH says a fullscreen window gets the whole screen; ICCCM
                // says never to size a window outside its WM_NORMAL_HINTS.
                // For a window that pins min == max those conflict, and the
                // hints win: the client has stated it cannot render at any
                // other size, and stretching it anyway doesn't make it do so.
                // It keeps drawing its own surface into a corner of an
                // oversized window, which is exactly the bug this caused.
                //
                // On a normal desktop this never comes up, because Wine pairs
                // a fullscreen request with a RandR mode switch and the
                // screen becomes the game's resolution. The nested server's
                // root is a fixed size, so instead we honour the size the
                // game says it is and centre it on the black root.
                let (width, height) = client.constrain(root_width, root_height);
                info!(
                    "nested session: fullscreen constrain {root_width}x{root_height} -> \
                     {width}x{height} (min={:?} max={:?})",
                    client.hints.min_size, client.hints.max_size
                );
                client.width = width;
                client.height = height;
                client.x = ((root_width as i32 - width as i32) / 2).max(0) as i16;
                client.y = ((root_height as i32 - height as i32) / 2).max(0) as i16;
            } else if let Some((x, y, width, height)) = client.restore.take() {
                client.x = x;
                client.y = y;
                client.width = width;
                client.height = height;
            }
            client.fullscreen = fullscreen;
            client.maximized = client.maximized && !fullscreen;
        }

        let geometry = self
            .clients
            .get(&frame)
            .map(|client| (client.x, client.y, client.width, client.height));
        info!("nested session: frame {frame} fullscreen={fullscreen} -> {geometry:?}");
        self.publish_state(frame)?;
        self.publish_frame_extents(frame)?;
        self.apply_geometry(frame)?;
        self.restack()
    }

    /// Minimise and restore.
    ///
    /// This is not optional politeness. A fullscreen Windows game minimises
    /// itself the moment it loses activation - that's what `WM_ACTIVATEAPP`
    /// does under Wine - so a window manager that drops the request on the
    /// floor leaves the game *believing it is minimised while still being
    /// displayed*. It then renders as though it were an icon: a small strip
    /// in the corner of an otherwise stale window. Honouring the request
    /// keeps the game's own idea of its state and ours in agreement, which is
    /// what makes it render correctly again when it comes back.
    ///
    /// Restoring is done from the taskbar entry, which stays put while the
    /// window is hidden; Super+Tab cycles through iconified windows too, for
    /// when a game's pointer grab is swallowing clicks.
    fn set_iconified(&mut self, frame: Window, iconified: bool) -> Result<(), NestedSessionError> {
        let window = {
            let Some(client) = self.clients.get_mut(&frame) else {
                return Ok(());
            };
            if client.iconified == iconified {
                return Ok(());
            }
            client.iconified = iconified;
            client.window
        };

        if iconified {
            // Unmapping the client is our doing, not the client withdrawing.
            *self.ignore_unmap.entry(window).or_default() += 1;
            self.conn.unmap_window(window).map_err(x11)?;
            self.conn.unmap_window(frame).map_err(x11)?;
            // WM_STATE = IconicState, per ICCCM.
            self.conn
                .change_property32(
                    PropMode::REPLACE,
                    window,
                    self.atoms.WM_STATE,
                    self.atoms.WM_STATE,
                    &[3, NONE],
                )
                .map_err(x11)?;
        } else {
            self.conn.map_window(frame).map_err(x11)?;
            self.conn.map_window(window).map_err(x11)?;
            self.conn
                .change_property32(
                    PropMode::REPLACE,
                    window,
                    self.atoms.WM_STATE,
                    self.atoms.WM_STATE,
                    &[1, NONE],
                )
                .map_err(x11)?;
        }

        info!("nested session: frame {frame} iconified={iconified}");
        self.publish_state(frame)?;

        if iconified {
            // Hand focus to whatever is still visible, rather than leaving it
            // on a window nobody can see.
            if self.focused == Some(frame) {
                self.focused = None;
                let next = self
                    .order
                    .iter()
                    .rev()
                    .find(|candidate| {
                        **candidate != frame
                            && self
                                .clients
                                .get(candidate)
                                .is_some_and(|client| !client.iconified)
                    })
                    .copied();
                if let Some(next) = next {
                    self.raise_and_focus(next)?;
                }
            }
            self.restack()?;
        } else {
            self.apply_geometry(frame)?;
            self.raise_and_focus(frame)?;
        }
        self.draw_taskbar()
    }

    fn set_maximized(&mut self, frame: Window, maximized: bool) -> Result<(), NestedSessionError> {
        let (area_x, area_y, area_width, area_height) = self.work_area();
        {
            let Some(client) = self.clients.get_mut(&frame) else {
                return Ok(());
            };
            if client.maximized == maximized || client.fullscreen {
                return Ok(());
            }
            if maximized {
                if client.restore.is_none() {
                    client.restore = Some((client.x, client.y, client.width, client.height));
                }
                let (left, top, right, bottom) = client.chrome();
                let (width, height) = client.constrain(
                    area_width.saturating_sub((left + right) as u16),
                    area_height.saturating_sub((top + bottom) as u16),
                );
                client.x = area_x;
                client.y = area_y;
                client.width = width;
                client.height = height;
            } else if let Some((x, y, width, height)) = client.restore.take() {
                client.x = x;
                client.y = y;
                client.width = width;
                client.height = height;
            }
            client.maximized = maximized;
        }

        let geometry = self
            .clients
            .get(&frame)
            .map(|client| (client.x, client.y, client.width, client.height));
        info!("nested session: frame {frame} maximized={maximized} -> {geometry:?}");
        self.publish_state(frame)?;
        self.apply_geometry(frame)
    }

    // ── managing windows ──────────────────────────────────────────────────

    fn manage(&mut self, window: Window) -> Result<(), NestedSessionError> {
        // Our own taskbar is not a client, and framing it would be a
        // spectacular way to lose the session.
        if window == self.taskbar || self.by_client.contains_key(&window) {
            return Ok(());
        }

        let attributes = self
            .conn
            .get_window_attributes(window)
            .map_err(x11)?
            .reply()
            .map_err(x11)?;

        let types = self.window_types(window);

        // Override-redirect windows are outside the window manager's reach by
        // definition - the client asked the server to bypass us entirely, and
        // the server already stacks them on top. Map and leave well alone.
        if attributes.override_redirect {
            self.conn.map_window(window).map_err(x11)?;
            return Ok(());
        }

        // Unframed, but not unmanaged: still mapped, still stacked, just not
        // given chrome or a position we chose.
        if let Some(layer) = self.unframed_layer(&types) {
            info!(
                "nested session: mapping {window} unframed (type {})",
                self.describe_types(&types)
            );
            self.conn
                .change_window_attributes(
                    window,
                    &ChangeWindowAttributesAux::new().event_mask(EventMask::STRUCTURE_NOTIFY),
                )
                .map_err(x11)?;
            self.conn.map_window(window).map_err(x11)?;
            self.unframed.retain(|(candidate, _)| *candidate != window);
            self.unframed.push((window, layer));
            return self.restack();
        }

        let geometry = self
            .conn
            .get_geometry(window)
            .map_err(x11)?
            .reply()
            .map_err(x11)?;

        let hints = self.read_size_hints(window);
        let (accepts_input, takes_focus) = self.read_focus_model(window);
        let transient_for = self.read_transient_for(window);
        let decorated = self.read_motif_decorated(window) && !self.is_undecorated_type(&types);
        let (wants_fullscreen, wants_maximized, above, below) = self.initial_states(window);

        // At info, not debug: in the Steam Deck's game mode there is no
        // terminal to read, and a window coming out the wrong size is the
        // failure mode this whole thing keeps hitting. These fire once per
        // window, not per frame.
        info!(
            "nested session: managing {window} '{title}' type={kind} requested={rw}x{rh} \
             decorated={decorated} min={min:?} max={max:?} inc={inc:?} base={base:?} \
             transient_for={transient_for:?} fullscreen={wants_fullscreen} \
             maximized={wants_maximized} input={accepts_input} take_focus={takes_focus}",
            title = self.window_title(window),
            kind = self.describe_types(&types),
            rw = geometry.width,
            rh = geometry.height,
            min = hints.min_size,
            max = hints.max_size,
            inc = hints.size_increment,
            base = hints.base_size,
        );

        let mut client = Client {
            window,
            x: 0,
            y: 0,
            width: geometry.width.max(MIN_WIDTH),
            height: geometry.height.max(MIN_HEIGHT),
            fullscreen: false,
            maximized: false,
            iconified: false,
            above,
            below,
            decorated,
            restore: None,
            hints,
            transient_for,
            takes_focus,
            accepts_input,
        };

        // Constrain to what the client says it can be, then to what fits in
        // the work area - the taskbar's height included, so a window doesn't
        // open with its bottom edge already underneath it.
        let (left, top, right, bottom) = client.chrome();
        let (_, _, area_width, area_height) = self.work_area();
        let max_width = area_width.saturating_sub((left + right) as u16);
        let max_height = area_height.saturating_sub((top + bottom) as u16);
        let (width, height) = client.constrain(client.width, client.height);
        client.width = width.min(max_width.max(MIN_WIDTH));
        client.height = height.min(max_height.max(MIN_HEIGHT));

        let (x, y) = match transient_for.and_then(|parent| self.by_client.get(&parent).copied()) {
            // A dialog belongs over the window it came from, not wherever the
            // cascade happens to be.
            Some(parent_frame) => self.centre_over(parent_frame, &client),
            None => self.next_position(client.width, client.height),
        };
        client.x = x;
        client.y = y;

        let frame = self.conn.generate_id().map_err(x11)?;
        self.conn
            .create_window(
                self.depth,
                frame,
                self.root,
                client.x,
                client.y,
                client.frame_width(),
                client.frame_height(),
                0,
                WindowClass::INPUT_OUTPUT,
                self.visual,
                &CreateWindowAux::new()
                    .background_pixel(COLOUR_FRAME)
                    .event_mask(
                        EventMask::SUBSTRUCTURE_REDIRECT
                            | EventMask::SUBSTRUCTURE_NOTIFY
                            | EventMask::BUTTON_PRESS
                            | EventMask::BUTTON_RELEASE
                            | EventMask::POINTER_MOTION
                            | EventMask::EXPOSURE,
                    ),
            )
            .map_err(x11)?;

        if attributes.map_state == MapState::VIEWABLE {
            *self.ignore_unmap.entry(window).or_default() += 1;
        }

        // So the client survives if we die mid-session rather than being
        // destroyed along with its frame.
        self.conn
            .change_save_set(SetMode::INSERT, window)
            .map_err(x11)?;
        let (chrome_left, chrome_top, _, _) = client.chrome();
        self.conn
            .reparent_window(window, frame, chrome_left, chrome_top)
            .map_err(x11)?;
        self.conn
            .configure_window(
                window,
                &ConfigureWindowAux::new()
                    .width(client.width as u32)
                    .height(client.height as u32)
                    .border_width(0),
            )
            .map_err(x11)?;
        self.conn
            .change_window_attributes(
                window,
                &ChangeWindowAttributesAux::new()
                    .event_mask(EventMask::PROPERTY_CHANGE | EventMask::STRUCTURE_NOTIFY),
            )
            .map_err(x11)?;

        // Click-to-focus without stealing the click: the grab is synchronous,
        // so we get the press first, raise the window, then replay the event
        // into the client as if we'd never been there.
        self.conn
            .grab_button(
                false,
                window,
                EventMask::BUTTON_PRESS,
                GrabMode::SYNC,
                GrabMode::ASYNC,
                NONE,
                NONE,
                ButtonIndex::M1,
                ModMask::ANY,
            )
            .map_err(x11)?;

        self.conn.map_window(frame).map_err(x11)?;
        self.conn.map_window(window).map_err(x11)?;

        // WM_STATE = NormalState. Some Windows toolkits running under Wine
        // wait for this before considering themselves mapped.
        self.conn
            .change_property32(
                PropMode::REPLACE,
                window,
                self.atoms.WM_STATE,
                self.atoms.WM_STATE,
                &[1, NONE],
            )
            .map_err(x11)?;
        self.conn
            .change_property32(
                PropMode::REPLACE,
                window,
                self.atoms._NET_WM_DESKTOP,
                AtomEnum::CARDINAL,
                &[0u32],
            )
            .map_err(x11)?;

        let (x, y, width, height) = (client.x, client.y, client.width, client.height);
        self.clients.insert(frame, client);
        self.by_client.insert(window, frame);
        self.order.push(frame);
        self.taskbar_order.push(frame);

        self.update_client_list()?;
        self.publish_state(frame)?;
        self.publish_frame_extents(frame)?;
        self.publish_allowed_actions(frame)?;

        // A client may have asked for either state before it was ever mapped,
        // in which case there's no ClientMessage coming.
        if wants_fullscreen {
            self.set_fullscreen(frame, true)?;
        } else if wants_maximized {
            self.set_maximized(frame, true)?;
        } else {
            // Tell the client where the frame actually put it (see
            // notify_geometry) - it has no other way to find out.
            self.notify_geometry(window, x + chrome_left, y + chrome_top, width, height)?;
        }

        self.raise_and_focus(frame)?;
        self.draw_taskbar()?;
        info!("nested session: framed {window} as {frame} at {x},{y} {width}x{height}");
        Ok(())
    }

    fn unmanage(&mut self, window: Window) -> Result<(), NestedSessionError> {
        let Some(frame) = self.by_client.remove(&window) else {
            // Might be one of the windows we map without framing.
            let tracked = self
                .unframed
                .iter()
                .any(|(candidate, _)| *candidate == window);
            if tracked {
                self.unframed.retain(|(candidate, _)| *candidate != window);
                return self.restack();
            }
            return Ok(());
        };
        self.clients.remove(&frame);
        self.order.retain(|candidate| *candidate != frame);
        self.taskbar_order.retain(|candidate| *candidate != frame);
        self.ignore_unmap.remove(&window);

        // The client may already be gone (DestroyNotify), in which case all
        // of these fail harmlessly - hence no `.check()`.
        let _ = self.conn.reparent_window(window, self.root, 0, 0);
        let _ = self.conn.change_save_set(SetMode::DELETE, window);
        let _ = self.conn.destroy_window(frame);
        // WM_STATE = WithdrawnState, per ICCCM, so a client that unmapped
        // itself can tell we let go of it.
        let _ = self.conn.change_property32(
            PropMode::REPLACE,
            window,
            self.atoms.WM_STATE,
            self.atoms.WM_STATE,
            &[0, NONE],
        );

        if self.focused == Some(frame) {
            self.focused = None;
            if let Some(next) = self.order.last().copied() {
                self.raise_and_focus(next)?;
            }
        }
        self.update_client_list()?;
        self.draw_taskbar()?;
        self.restack()
    }

    fn configure_request(
        &mut self,
        event: x11rb::protocol::xproto::ConfigureRequestEvent,
    ) -> Result<(), NestedSessionError> {
        let Some(frame) = self.by_client.get(&event.window).copied() else {
            // Not ours - let the client have exactly what it asked for.
            let aux = ConfigureWindowAux::from_configure_request(&event);
            self.conn
                .configure_window(event.window, &aux)
                .map_err(x11)?;
            return Ok(());
        };

        // A fullscreen or maximized window's geometry is ours, not its own.
        // Acknowledge with the size it actually has, per ICCCM, and ignore
        // what it asked for.
        if self
            .clients
            .get(&frame)
            .is_some_and(|client| client.fullscreen || client.maximized)
        {
            return self.apply_geometry(frame);
        }

        let (root_width, root_height) = (self.root_width, self.root_height);
        let Some(client) = self.clients.get_mut(&frame) else {
            return Ok(());
        };

        // Honour both position and size: a window asking to move itself is a
        // legitimate request, and this WM has no layout of its own to defend.
        let config = event.value_mask;
        if config.contains(x11rb::protocol::xproto::ConfigWindow::X) {
            client.x = event.x;
        }
        if config.contains(x11rb::protocol::xproto::ConfigWindow::Y) {
            client.y = event.y;
        }

        let (mut width, mut height) = (client.width, client.height);
        if config.contains(x11rb::protocol::xproto::ConfigWindow::WIDTH) {
            width = event.width;
        }
        if config.contains(x11rb::protocol::xproto::ConfigWindow::HEIGHT) {
            height = event.height;
        }
        let (width, height) = client.constrain(width, height);

        // A windowed client must still fit on screen with its chrome, or the
        // titlebar ends up off the edge and the window can't be moved.
        let (left, top, right, bottom) = client.chrome();
        client.width = width.min(
            root_width
                .saturating_sub((left + right) as u16)
                .max(MIN_WIDTH),
        );
        client.height = height.min(
            root_height
                .saturating_sub((top + bottom) as u16)
                .max(MIN_HEIGHT),
        );

        self.apply_geometry(frame)
    }

    /// The nested root is not a fixed size: Xwayland resizes it when the
    /// host window changes, and a game switching resolution through RandR
    /// does the same. Every placement decision here is in root coordinates,
    /// so a stale size means everything after it is wrong - fullscreen
    /// windows most obviously, since they're sized directly from it.
    fn root_resized(&mut self, width: u16, height: u16) -> Result<(), NestedSessionError> {
        if (width, height) == (self.root_width, self.root_height) {
            return Ok(());
        }
        info!(
            "nested session: root resized {}x{} -> {width}x{height}",
            self.root_width, self.root_height
        );
        self.root_width = width;
        self.root_height = height;

        self.layout_taskbar()?;

        let (area_x, area_y, area_width, area_height) = self.work_area();
        let (width, height) = (u32::from(width), u32::from(height));
        for (property, value) in [
            (self.atoms._NET_DESKTOP_GEOMETRY, vec![width, height]),
            (
                self.atoms._NET_WORKAREA,
                vec![
                    area_x as u32,
                    area_y as u32,
                    u32::from(area_width),
                    u32::from(area_height),
                ],
            ),
        ] {
            self.conn
                .change_property32(
                    PropMode::REPLACE,
                    self.root,
                    property,
                    AtomEnum::CARDINAL,
                    &value,
                )
                .map_err(x11)?;
        }

        // Anything sized from the root has to be resized to match it.
        let stretched: Vec<Window> = self
            .clients
            .iter()
            .filter(|(_, client)| client.fullscreen || client.maximized)
            .map(|(frame, _)| *frame)
            .collect();
        for frame in stretched {
            let (root_width, root_height) = (self.root_width, self.root_height);
            if let Some(client) = self.clients.get_mut(&frame)
                && client.fullscreen
            {
                client.x = 0;
                client.y = 0;
                client.width = root_width;
                client.height = root_height;
            }
            self.apply_geometry(frame)?;
        }
        self.restack()?;
        self.draw_taskbar()
    }

    // ── input ─────────────────────────────────────────────────────────────

    fn button_press(
        &mut self,
        event: x11rb::protocol::xproto::ButtonPressEvent,
    ) -> Result<(), NestedSessionError> {
        if event.event == self.taskbar {
            return self.taskbar_click(event.event_x);
        }

        // A press on the client itself: raise, then hand the click straight
        // back to the application.
        if let Some(frame) = self.by_client.get(&event.event).copied() {
            self.raise_and_focus(frame)?;
            self.conn
                .allow_events(x11rb::protocol::xproto::Allow::REPLAY_POINTER, CURRENT_TIME)
                .map_err(x11)?;
            return Ok(());
        }

        let frame = event.event;
        let Some(client) = self.clients.get(&frame) else {
            return Ok(());
        };
        let fullscreen = client.fullscreen;
        let decorated = client.decorated;
        let resizable = client.resizable() && !client.fullscreen;
        let maximized = client.maximized;
        let (frame_width, frame_height) = (client.frame_width(), client.frame_height());
        let (window, client_x, client_y, client_width, client_height) = (
            client.window,
            client.x,
            client.y,
            client.width,
            client.height,
        );

        self.raise_and_focus(frame)?;

        // No chrome to grab on a fullscreen or undecorated window.
        if fullscreen || !decorated {
            return Ok(());
        }

        // Titlebar buttons, right to left: close, maximize, minimise.
        if event.event_y < TITLEBAR_HEIGHT
            && let Some((button, _)) = titlebar_buttons(frame_width, resizable)
                .into_iter()
                .find(|(_, left)| event.event_x >= *left && event.event_x < *left + TITLEBAR_HEIGHT)
        {
            return match button {
                TitlebarButton::Close => self.close_client(window),
                TitlebarButton::Maximize => self.set_maximized(frame, !maximized),
                TitlebarButton::Minimize => self.set_iconified(frame, true),
            };
        }

        let edges = if resizable {
            self.edges_at(event.event_x, event.event_y, frame_width, frame_height)
        } else {
            ResizeEdges::default()
        };

        if edges.any() {
            self.drag = Drag::Resize {
                frame,
                edges,
                origin_x: event.root_x,
                origin_y: event.root_y,
                start_x: client_x,
                start_y: client_y,
                start_width: client_width,
                start_height: client_height,
            };
        } else if event.event_y < TITLEBAR_HEIGHT {
            self.drag = Drag::Move {
                frame,
                offset_x: event.root_x - client_x,
                offset_y: event.root_y - client_y,
            };
        } else {
            return Ok(());
        }

        self.grab_for_drag(frame)
    }

    /// Which frame edges the pointer is close enough to be grabbing. The
    /// bottom bar is thicker than the sides precisely so this is hittable:
    /// everything except the frame's own border is covered by the client
    /// window, which gets the pointer events instead of us.
    fn edges_at(&self, x: i16, y: i16, frame_width: u16, frame_height: u16) -> ResizeEdges {
        let (frame_width, frame_height) = (frame_width as i16, frame_height as i16);
        ResizeEdges {
            left: x < RESIZE_ZONE,
            right: x >= frame_width - RESIZE_ZONE,
            // The titlebar is for moving, so the top edge only counts in the
            // first few pixels above it.
            top: y < BORDER,
            bottom: y >= frame_height - BOTTOM_BAR,
        }
    }

    fn grab_for_drag(&self, frame: Window) -> Result<(), NestedSessionError> {
        // Grab the pointer for the duration of the drag so motion keeps
        // reaching us even when the cursor leaves the frame (which it does
        // the moment the drag outruns the window).
        self.conn
            .grab_pointer(
                false,
                frame,
                EventMask::BUTTON_RELEASE | EventMask::POINTER_MOTION,
                GrabMode::ASYNC,
                GrabMode::ASYNC,
                NONE,
                NONE,
                CURRENT_TIME,
            )
            .map_err(x11)?;
        Ok(())
    }

    fn end_drag(&mut self) -> Result<(), NestedSessionError> {
        if !matches!(self.drag, Drag::None) {
            self.drag = Drag::None;
            self.conn.ungrab_pointer(CURRENT_TIME).map_err(x11)?;
        }
        Ok(())
    }

    fn motion(&mut self, root_x: i16, root_y: i16) -> Result<(), NestedSessionError> {
        match self.drag {
            Drag::None => Ok(()),
            Drag::Move {
                frame,
                offset_x,
                offset_y,
            } => {
                let Some(client) = self.clients.get_mut(&frame) else {
                    return Ok(());
                };
                client.x = root_x - offset_x;
                client.y = root_y - offset_y;
                let (left, top, _, _) = client.chrome();
                let (window, x, y, width, height) = (
                    client.window,
                    client.x,
                    client.y,
                    client.width,
                    client.height,
                );
                self.conn
                    .configure_window(frame, &ConfigureWindowAux::new().x(x as i32).y(y as i32))
                    .map_err(x11)?;
                // A move changes the client's on-screen position without
                // generating a real ConfigureNotify for it, so this is the
                // only way it finds out. No redraw: the chrome is unchanged.
                self.notify_geometry(window, x + left, y + top, width, height)?;
                Ok(())
            }
            Drag::Resize {
                frame,
                edges,
                origin_x,
                origin_y,
                start_x,
                start_y,
                start_width,
                start_height,
            } => {
                let Some(client) = self.clients.get_mut(&frame) else {
                    return Ok(());
                };
                let dx = i32::from(root_x - origin_x);
                let dy = i32::from(root_y - origin_y);

                let mut width = i32::from(start_width);
                let mut height = i32::from(start_height);
                if edges.right {
                    width += dx;
                }
                if edges.left {
                    width -= dx;
                }
                if edges.bottom {
                    height += dy;
                }
                if edges.top {
                    height -= dy;
                }

                let (width, height) = client.constrain(
                    width.clamp(0, i32::from(u16::MAX)) as u16,
                    height.clamp(0, i32::from(u16::MAX)) as u16,
                );
                client.width = width;
                client.height = height;

                // Dragging a left or top edge moves the opposite corner's
                // anchor: the far edge has to stay put, so the origin shifts
                // by however much the size actually changed after hints were
                // applied - not by how far the pointer moved.
                if edges.left {
                    client.x = start_x + (start_width as i16 - width as i16);
                }
                if edges.top {
                    client.y = start_y + (start_height as i16 - height as i16);
                }

                self.apply_geometry(frame)
            }
        }
    }

    fn cycle_windows(&mut self) -> Result<(), NestedSessionError> {
        let Some(next) = self
            .order
            .iter()
            .position(|frame| Some(*frame) == self.focused)
            .map(|index| (index + 1) % self.order.len().max(1))
            .and_then(|index| self.order.get(index).copied())
            .or_else(|| self.order.first().copied())
        else {
            return Ok(());
        };
        self.raise_and_focus(next)
    }

    fn raise_and_focus(&mut self, frame: Window) -> Result<(), NestedSessionError> {
        // Focusing an iconified window means restoring it - that is the only
        // way back for a window with no taskbar entry.
        if self
            .clients
            .get(&frame)
            .is_some_and(|client| client.iconified)
        {
            return self.set_iconified(frame, false);
        }

        let Some(client) = self.clients.get(&frame) else {
            return Ok(());
        };
        let window = client.window;
        let accepts_input = client.accepts_input;
        let takes_focus = client.takes_focus;

        self.raise(frame);
        self.restack()?;

        // Three focus models, per ICCCM. Getting this wrong gives you a
        // window that looks focused and ignores the keyboard, which is
        // exactly what Qt does if you SetInputFocus at it when it asked for
        // WM_TAKE_FOCUS.
        if takes_focus {
            let event = ClientMessageEvent::new(
                32,
                window,
                self.atoms.WM_PROTOCOLS,
                [self.atoms.WM_TAKE_FOCUS, self.last_time, 0, 0, 0],
            );
            let _ = self
                .conn
                .send_event(false, window, EventMask::NO_EVENT, event);
        }
        if accepts_input {
            // A client that has gone away between the raise and here makes
            // this fail; harmless, the DestroyNotify will clean up after us.
            let _ = self
                .conn
                .set_input_focus(InputFocus::PARENT, window, CURRENT_TIME);
        }

        self.conn
            .change_property32(
                PropMode::REPLACE,
                self.root,
                self.atoms._NET_ACTIVE_WINDOW,
                AtomEnum::WINDOW,
                &[window],
            )
            .map_err(x11)?;

        let previous = self.focused.replace(frame);
        if let Some(previous) = previous
            && previous != frame
        {
            self.draw_frame(previous)?;
        }
        self.draw_frame(frame)?;
        self.draw_taskbar()?;
        Ok(())
    }

    fn close_client(&mut self, window: Window) -> Result<(), NestedSessionError> {
        if self
            .window_protocols(window)
            .contains(&self.atoms.WM_DELETE_WINDOW)
        {
            let event = ClientMessageEvent::new(
                32,
                window,
                self.atoms.WM_PROTOCOLS,
                [self.atoms.WM_DELETE_WINDOW, self.last_time, 0, 0, 0],
            );
            self.conn
                .send_event(false, window, EventMask::NO_EVENT, event)
                .map_err(x11)?;
        } else {
            self.conn.kill_client(window).map_err(x11)?;
        }
        Ok(())
    }

    fn client_message(&mut self, event: ClientMessageEvent) -> Result<(), NestedSessionError> {
        let data = event.data.as_data32();
        let frame = self.by_client.get(&event.window).copied();

        if event.type_ == self.atoms._NET_ACTIVE_WINDOW {
            if let Some(frame) = frame {
                self.raise_and_focus(frame)?;
            }
        } else if event.type_ == self.atoms._NET_CLOSE_WINDOW {
            self.close_client(event.window)?;
        } else if event.type_ == self.atoms._NET_WM_STATE {
            if let Some(frame) = frame {
                // data32: [action, first property, second property, ...],
                // where action is 0 remove, 1 add, 2 toggle.
                self.state_request(frame, data[0], data[1])?;
                self.state_request(frame, data[0], data[2])?;
            }
        } else if event.type_ == self.atoms._NET_WM_MOVERESIZE {
            if let Some(frame) = frame {
                self.moveresize_request(frame, data[0], data[1], data[2])?;
            }
        } else if event.type_ == self.atoms.WM_CHANGE_STATE {
            // data[0] == IconicState (3) is ICCCM's "please minimise me".
            if data[0] == 3
                && let Some(frame) = frame
            {
                self.set_iconified(frame, true)?;
            }
        }
        Ok(())
    }

    fn state_request(
        &mut self,
        frame: Window,
        action: u32,
        property: u32,
    ) -> Result<(), NestedSessionError> {
        if property == NONE {
            return Ok(());
        }
        let current =
            |client: Option<&Client>, pick: fn(&Client) -> bool| client.map(pick).unwrap_or(false);
        let client = self.clients.get(&frame);

        let target = |now: bool| match action {
            STATE_REMOVE => false,
            STATE_ADD => true,
            _ => !now,
        };

        if property == self.atoms._NET_WM_STATE_FULLSCREEN {
            let now = current(client, |client| client.fullscreen);
            self.set_fullscreen(frame, target(now))?;
        } else if property == self.atoms._NET_WM_STATE_MAXIMIZED_HORZ
            || property == self.atoms._NET_WM_STATE_MAXIMIZED_VERT
        {
            // We only do both axes at once; a request for either means
            // "maximize", which is what every toolkit asks for in practice.
            let now = current(client, |client| client.maximized);
            self.set_maximized(frame, target(now))?;
        } else if property == self.atoms._NET_WM_STATE_HIDDEN {
            let now = current(client, |client| client.iconified);
            self.set_iconified(frame, target(now))?;
        } else if property == self.atoms._NET_WM_STATE_ABOVE {
            let now = current(client, |client| client.above);
            let value = target(now);
            if let Some(client) = self.clients.get_mut(&frame) {
                client.above = value;
                if value {
                    client.below = false;
                }
            }
            self.publish_state(frame)?;
            self.restack()?;
        } else if property == self.atoms._NET_WM_STATE_BELOW {
            let now = current(client, |client| client.below);
            let value = target(now);
            if let Some(client) = self.clients.get_mut(&frame) {
                client.below = value;
                if value {
                    client.above = false;
                }
            }
            self.publish_state(frame)?;
            self.restack()?;
        }
        Ok(())
    }

    /// A client asking us to start a move or resize it initiated itself -
    /// how a window with client-side decorations (Qt's, among others) gets
    /// dragged, since its titlebar is drawn inside the client area where we
    /// never see the press.
    fn moveresize_request(
        &mut self,
        frame: Window,
        root_x: u32,
        root_y: u32,
        direction: u32,
    ) -> Result<(), NestedSessionError> {
        if direction == MOVERESIZE_CANCEL {
            return self.end_drag();
        }

        let Some(client) = self.clients.get(&frame) else {
            return Ok(());
        };
        let (start_x, start_y, start_width, start_height) =
            (client.x, client.y, client.width, client.height);
        let resizable = client.resizable() && !client.fullscreen;

        let (origin_x, origin_y) = (root_x as i16, root_y as i16);

        let edges = match direction {
            MOVERESIZE_TOPLEFT => ResizeEdges {
                left: true,
                top: true,
                ..Default::default()
            },
            MOVERESIZE_TOP => ResizeEdges {
                top: true,
                ..Default::default()
            },
            MOVERESIZE_TOPRIGHT => ResizeEdges {
                right: true,
                top: true,
                ..Default::default()
            },
            MOVERESIZE_RIGHT => ResizeEdges {
                right: true,
                ..Default::default()
            },
            MOVERESIZE_BOTTOMRIGHT => ResizeEdges {
                right: true,
                bottom: true,
                ..Default::default()
            },
            MOVERESIZE_BOTTOM => ResizeEdges {
                bottom: true,
                ..Default::default()
            },
            MOVERESIZE_BOTTOMLEFT => ResizeEdges {
                left: true,
                bottom: true,
                ..Default::default()
            },
            MOVERESIZE_LEFT => ResizeEdges {
                left: true,
                ..Default::default()
            },
            _ => ResizeEdges::default(),
        };

        self.drag = if direction == MOVERESIZE_MOVE || !edges.any() {
            Drag::Move {
                frame,
                offset_x: origin_x - start_x,
                offset_y: origin_y - start_y,
            }
        } else if resizable {
            Drag::Resize {
                frame,
                edges,
                origin_x,
                origin_y,
                start_x,
                start_y,
                start_width,
                start_height,
            }
        } else {
            return Ok(());
        };

        self.grab_for_drag(frame)
    }

    // ── drawing ───────────────────────────────────────────────────────────

    fn draw_frame(&mut self, frame: Window) -> Result<(), NestedSessionError> {
        let focused = self.focused == Some(frame);
        let Some(client) = self.clients.get(&frame) else {
            return Ok(());
        };
        // Fullscreen and undecorated windows have no chrome - the client
        // covers the frame completely, so there is nothing of ours to paint.
        if client.fullscreen || !client.decorated {
            return Ok(());
        }
        let window = client.window;
        let resizable = client.resizable();
        let (frame_width, frame_height) = (client.frame_width(), client.frame_height());

        let titlebar_colour = if focused {
            COLOUR_FRAME_FOCUSED
        } else {
            COLOUR_FRAME
        };

        self.fill(
            frame,
            titlebar_colour,
            0,
            0,
            frame_width,
            TITLEBAR_HEIGHT as u16,
        )?;

        // Titlebar buttons, each a solid block the full height of the bar so
        // it's a big target. Laid out by the same function the hit test uses.
        let inset = 11;
        let buttons = titlebar_buttons(frame_width, resizable);
        for (button, left) in &buttons {
            let (left, button) = (*left, *button);
            let background = match button {
                TitlebarButton::Close => COLOUR_CLOSE,
                _ => COLOUR_GRIP,
            };
            self.fill(
                frame,
                background,
                left,
                0,
                TITLEBAR_HEIGHT as u16,
                TITLEBAR_HEIGHT as u16,
            )?;
            match button {
                // An X.
                TitlebarButton::Close => {
                    self.conn
                        .change_gc(self.gc, &ChangeGCAux::new().foreground(COLOUR_TITLE))
                        .map_err(x11)?;
                    self.conn
                        .poly_segment(
                            frame,
                            self.gc,
                            &[
                                Segment {
                                    x1: left + inset,
                                    y1: inset,
                                    x2: left + TITLEBAR_HEIGHT - inset,
                                    y2: TITLEBAR_HEIGHT - inset,
                                },
                                Segment {
                                    x1: left + TITLEBAR_HEIGHT - inset,
                                    y1: inset,
                                    x2: left + inset,
                                    y2: TITLEBAR_HEIGHT - inset,
                                },
                            ],
                        )
                        .map_err(x11)?;
                }
                // An outlined square.
                TitlebarButton::Maximize => {
                    self.conn
                        .change_gc(self.gc, &ChangeGCAux::new().foreground(COLOUR_TITLE))
                        .map_err(x11)?;
                    self.conn
                        .poly_rectangle(
                            frame,
                            self.gc,
                            &[Rectangle {
                                x: left + inset,
                                y: inset,
                                width: (TITLEBAR_HEIGHT - inset * 2) as u16,
                                height: (TITLEBAR_HEIGHT - inset * 2) as u16,
                            }],
                        )
                        .map_err(x11)?;
                }
                // A bar along the bottom of the button's box - the window
                // collapsing downwards, to where its taskbar entry is.
                TitlebarButton::Minimize => {
                    self.fill(
                        frame,
                        COLOUR_TITLE,
                        left + inset,
                        TITLEBAR_HEIGHT - inset - 2,
                        (TITLEBAR_HEIGHT - inset * 2) as u16,
                        2,
                    )?;
                }
            }
        }

        // Bottom bar - the visible, grabbable resize handle - with grip ticks
        // at its right-hand end.
        let bottom_top = frame_height as i16 - BOTTOM_BAR;
        self.fill(
            frame,
            titlebar_colour,
            0,
            bottom_top,
            frame_width,
            BOTTOM_BAR as u16,
        )?;
        if resizable {
            self.conn
                .change_gc(self.gc, &ChangeGCAux::new().foreground(COLOUR_TITLE))
                .map_err(x11)?;
            let ticks: Vec<Segment> = (1..4)
                .map(|index| {
                    let offset = index * 5;
                    Segment {
                        x1: frame_width as i16 - 4 - offset,
                        y1: frame_height as i16 - 4,
                        x2: frame_width as i16 - 4,
                        y2: frame_height as i16 - 4 - offset,
                    }
                })
                .collect();
            self.conn
                .poly_segment(frame, self.gc, &ticks)
                .map_err(x11)?;
        }

        if self.has_font {
            let colour = if focused {
                COLOUR_TITLE
            } else {
                COLOUR_TITLE_UNFOCUSED
            };
            self.conn
                .change_gc(
                    self.gc,
                    &ChangeGCAux::new()
                        .foreground(colour)
                        .background(titlebar_colour),
                )
                .map_err(x11)?;
            // Stop short of the left-most button, whichever one that is.
            let buttons_left = buttons
                .last()
                .map(|(_, left)| *left)
                .unwrap_or(frame_width as i16);
            let limit = ((buttons_left - 20) / 6).max(0) as usize;
            let bytes: Vec<u8> = self.window_label(window).bytes().take(limit).collect();
            if !bytes.is_empty() {
                self.conn
                    .image_text8(frame, self.gc, 10, TITLEBAR_HEIGHT / 2 + 5, &bytes)
                    .map_err(x11)?;
            }
        }

        Ok(())
    }

    fn fill(
        &self,
        drawable: Window,
        colour: u32,
        x: i16,
        y: i16,
        width: u16,
        height: u16,
    ) -> Result<(), NestedSessionError> {
        self.conn
            .change_gc(self.gc, &ChangeGCAux::new().foreground(colour))
            .map_err(x11)?;
        self.conn
            .poly_fill_rectangle(
                drawable,
                self.gc,
                &[Rectangle {
                    x,
                    y,
                    width,
                    height,
                }],
            )
            .map_err(x11)?;
        Ok(())
    }

    fn window_title(&self, window: Window) -> String {
        for (property, property_type) in [
            (self.atoms._NET_WM_NAME, self.atoms.UTF8_STRING),
            (u32::from(AtomEnum::WM_NAME), u32::from(AtomEnum::STRING)),
        ] {
            if let Ok(cookie) =
                self.conn
                    .get_property(false, window, property, property_type, 0, 256)
                && let Ok(reply) = cookie.reply()
                && !reply.value.is_empty()
            {
                return String::from_utf8_lossy(&reply.value).to_string();
            }
        }
        String::new()
    }

    // ── taskbar ───────────────────────────────────────────────────────────

    /// Creates the strip along the bottom of the root.
    ///
    /// It is our window, not a client's: override-redirect so the server
    /// never routes it back through us as a MapRequest, and absent from
    /// `clients`, `order`, `taskbar_order`, `_NET_CLIENT_LIST` and Super+Tab.
    /// As far as every client is concerned it does not exist - it only shows
    /// up as the height missing from `_NET_WORKAREA`.
    fn create_taskbar(&mut self) -> Result<(), NestedSessionError> {
        let taskbar = self.conn.generate_id().map_err(x11)?;
        self.conn
            .create_window(
                self.depth,
                taskbar,
                self.root,
                0,
                self.root_height as i16 - TASKBAR_HEIGHT,
                self.root_width,
                TASKBAR_HEIGHT as u16,
                0,
                WindowClass::INPUT_OUTPUT,
                self.visual,
                &CreateWindowAux::new()
                    .background_pixel(COLOUR_TASKBAR)
                    .override_redirect(1)
                    .event_mask(EventMask::BUTTON_PRESS | EventMask::EXPOSURE),
            )
            .map_err(x11)?;
        self.conn.map_window(taskbar).map_err(x11)?;
        self.taskbar = taskbar;
        Ok(())
    }

    /// Re-pins the taskbar to the bottom edge after the root changes size.
    fn layout_taskbar(&self) -> Result<(), NestedSessionError> {
        if self.taskbar == NONE {
            return Ok(());
        }
        self.conn
            .configure_window(
                self.taskbar,
                &ConfigureWindowAux::new()
                    .x(0)
                    .y(i32::from(self.root_height as i16 - TASKBAR_HEIGHT))
                    .width(u32::from(self.root_width))
                    .height(TASKBAR_HEIGHT as u32),
            )
            .map_err(x11)?;
        Ok(())
    }

    /// Entries share the strip evenly and use all of it. Deliberately
    /// uncapped: the session this exists for holds two windows - a game and
    /// LunaTranslator - and half a screen each is exactly the touch target
    /// wanted on a Deck, where a cap would leave most of the bar dead space.
    /// With many windows they shrink, which is what every taskbar does.
    fn taskbar_entry_width(&self) -> i16 {
        let count = self.taskbar_order.len() as i16;
        if count <= 0 {
            return 0;
        }
        (self.root_width as i16 / count).max(1)
    }

    /// Repaints the whole strip: one entry per managed window, *including*
    /// iconified ones, which is the entire reason it exists.
    fn draw_taskbar(&self) -> Result<(), NestedSessionError> {
        if self.taskbar == NONE {
            return Ok(());
        }
        let taskbar = self.taskbar;
        self.fill(
            taskbar,
            COLOUR_TASKBAR,
            0,
            0,
            self.root_width,
            TASKBAR_HEIGHT as u16,
        )?;

        let width = self.taskbar_entry_width();
        let entry_width = (width - TASKBAR_GAP * 2).max(1);
        for (index, frame) in self.taskbar_order.iter().enumerate() {
            let Some((window, iconified)) = self
                .clients
                .get(frame)
                .map(|client| (client.window, client.iconified))
            else {
                continue;
            };
            let focused = self.focused == Some(*frame);
            let left = index as i16 * width + TASKBAR_GAP;

            // Iconified entries are dimmer than mapped ones and carry a bar
            // along their bottom edge, so "still here, just hidden" reads at
            // a glance rather than only from the colour.
            let background = if focused {
                COLOUR_FRAME_FOCUSED
            } else if iconified {
                COLOUR_ENTRY_ICONIFIED
            } else {
                COLOUR_ENTRY
            };
            self.fill(
                taskbar,
                background,
                left,
                TASKBAR_GAP,
                entry_width as u16,
                (TASKBAR_HEIGHT - TASKBAR_GAP * 2) as u16,
            )?;
            if iconified {
                // Bright and thick rather than tasteful: at arm's length on a
                // Deck this is the only thing saying "that window still
                // exists, it is just hidden", and a dim 3px line said it too
                // quietly to see.
                self.fill(
                    taskbar,
                    COLOUR_TITLE_UNFOCUSED,
                    left,
                    TASKBAR_HEIGHT - TASKBAR_GAP - 5,
                    entry_width as u16,
                    5,
                )?;
            }

            if self.has_font {
                let colour = if focused || !iconified {
                    COLOUR_TITLE
                } else {
                    COLOUR_TITLE_UNFOCUSED
                };
                self.conn
                    .change_gc(
                        self.gc,
                        &ChangeGCAux::new().foreground(colour).background(background),
                    )
                    .map_err(x11)?;
                let limit = ((entry_width - 12) / 6).max(0) as usize;
                let bytes: Vec<u8> = self.window_label(window).bytes().take(limit).collect();
                if !bytes.is_empty() {
                    self.conn
                        .image_text8(taskbar, self.gc, left + 6, TASKBAR_HEIGHT / 2 + 5, &bytes)
                        .map_err(x11)?;
                }
            }
        }
        Ok(())
    }

    /// A click on the strip focuses that window - or restores it first, if it
    /// was iconified, which `raise_and_focus` already does.
    fn taskbar_click(&mut self, x: i16) -> Result<(), NestedSessionError> {
        let width = self.taskbar_entry_width();
        if width <= 0 || x < 0 {
            return Ok(());
        }
        let Some(frame) = self.taskbar_order.get((x / width) as usize).copied() else {
            return Ok(());
        };
        self.raise_and_focus(frame)
    }

    /// What a taskbar entry (and a titlebar) calls a window.
    ///
    /// A title we cannot render at all - a Japanese visual novel's, say -
    /// leaves an entry that is a blank rectangle among other blank
    /// rectangles, so fall back to `WM_CLASS`, which is ASCII in practice
    /// (`shinydays.exe`), and to *something* rather than nothing after that.
    fn window_label(&self, window: Window) -> String {
        let title = ascii_only(&self.window_title(window));
        if !title.trim().is_empty() {
            return title.trim().to_owned();
        }
        let class = ascii_only(&self.window_class(window));
        if !class.trim().is_empty() {
            return class.trim().to_owned();
        }
        "window".to_owned()
    }

    /// `WM_CLASS` is two NUL-terminated strings, instance then class; the
    /// class is the more presentable of the two.
    fn window_class(&self, window: Window) -> String {
        let value = self
            .conn
            .get_property(false, window, AtomEnum::WM_CLASS, AtomEnum::STRING, 0, 128)
            .ok()
            .and_then(|cookie| cookie.reply().ok())
            .map(|reply| reply.value)
            .unwrap_or_default();
        let mut parts = value
            .split(|byte| *byte == 0)
            .filter(|part| !part.is_empty());
        let instance = parts.next();
        let class = parts.next().or(instance).unwrap_or_default();
        String::from_utf8_lossy(class).to_string()
    }

    // ── placement ─────────────────────────────────────────────────────────

    fn centre_over(&self, parent_frame: Window, client: &Client) -> (i16, i16) {
        let Some(parent) = self.clients.get(&parent_frame) else {
            return (0, 0);
        };
        let (_, _, area_width, area_height) = self.work_area();
        let x = parent.x + (parent.frame_width() as i16 - client.frame_width() as i16) / 2;
        let y = parent.y + (parent.frame_height() as i16 - client.frame_height() as i16) / 2;
        let max_x = (area_width as i16 - client.frame_width() as i16).max(0);
        let max_y = (area_height as i16 - client.frame_height() as i16).max(0);
        (x.clamp(0, max_x), y.clamp(0, max_y))
    }

    /// Cascades new windows instead of stacking them all at the same origin,
    /// wrapping back to the top-left before they'd run off the screen.
    fn next_position(&mut self, width: u16, height: u16) -> (i16, i16) {
        let (left, top, right, bottom) = (BORDER, TITLEBAR_HEIGHT, BORDER, BOTTOM_BAR);
        let frame_width = width as i16 + left + right;
        let frame_height = height as i16 + top + bottom;
        let (_, _, area_width, area_height) = self.work_area();
        let max_x = (area_width as i16 - frame_width).max(0);
        let max_y = (area_height as i16 - frame_height).max(0);

        // First window lands centred; later ones cascade down from there.
        if self.cascade == 0 {
            self.cascade = CASCADE_STEP;
            return (max_x / 2, max_y / 2);
        }

        let x = self.cascade.min(max_x);
        let y = self.cascade.min(max_y);
        self.cascade += CASCADE_STEP;
        if self.cascade > max_y.min(max_x) {
            self.cascade = CASCADE_STEP;
        }
        (x, y)
    }

    fn update_client_list(&self) -> Result<(), NestedSessionError> {
        let windows: Vec<Window> = self
            .order
            .iter()
            .filter_map(|frame| self.clients.get(frame).map(|client| client.window))
            .collect();
        self.conn
            .change_property32(
                PropMode::REPLACE,
                self.root,
                self.atoms._NET_CLIENT_LIST,
                AtomEnum::WINDOW,
                &windows,
            )
            .map_err(x11)?;
        Ok(())
    }
}
