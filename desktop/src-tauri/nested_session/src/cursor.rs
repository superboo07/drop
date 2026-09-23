//! Draws the session's cursor into the session itself, for gamescope.
//!
//! gamescope draws no cursor over a Wayland window - which is what our rootful
//! Xwayland is to it (`NO CURSOR IMPL XDG` in its steamcompmgr.cpp) - so the
//! pointer over the session was invisible. It can't be handed one either: the
//! cursor it draws is read from under the pointer of its own first X server,
//! which doesn't move while the session has focus and in game mode sits over
//! Steam's own (cursorless) window.
//!
//! So the cursor becomes part of the picture instead: a small window at the
//! top of the session's stack, showing whatever cursor the session currently
//! has (XFixes `CursorNotify` + `GetCursorImage`), kept at the pointer
//! (XInput 2 raw motion, which arrives even while a game has the pointer
//! grabbed). It's composited by picom like any other window, and never takes
//! input: override-redirect, so openbox and tint2 leave it alone, with an
//! empty input shape, so clicks go through it to whatever is underneath.
//!
//! A game hiding its pointer does it with an invisible cursor; the window is
//! unmapped then.

use std::{
    os::fd::AsRawFd as _,
    thread::JoinHandle,
    time::{Duration, Instant},
};

use log::{debug, warn};
use x11rb::{
    connection::{Connection, RequestConnection as _},
    protocol::{
        Event,
        shape::SK,
        xfixes::{self, ConnectionExt as _},
        xinput::{self, ConnectionExt as _},
        xproto::{
            AtomEnum, ChangeWindowAttributesAux, ColormapAlloc, ConfigureWindowAux,
            ConnectionExt as _, CreateGCAux, CreateWindowAux, EventMask, ImageFormat, ImageOrder,
            PropMode, StackMode, VisualClass, Window, WindowClass,
        },
    },
    rust_connection::RustConnection,
    wrapper::ConnectionExt as _,
};

use crate::error::NestedSessionError;

/// How often the pointer is checked without being told it moved. Raw motion
/// covers everything a person does; this covers the pointer being moved for
/// them (a game warping it), which produces no raw events.
const POLL_INTERVAL: Duration = Duration::from_millis(50);

fn x11<E: std::fmt::Display>(error: E) -> NestedSessionError {
    NestedSessionError::X11(error.to_string())
}

/// Starts drawing `display`'s cursor into it. Runs until the server goes
/// away.
pub(crate) fn spawn(display: String) -> std::io::Result<JoinHandle<()>> {
    std::thread::Builder::new()
        .name("nested-cursor".to_owned())
        .spawn(move || match Cursor::new(&display) {
            Ok(mut cursor) => {
                if let Err(e) = cursor.run() {
                    // The usual way out: the server was stopped, and the
                    // connection with it.
                    debug!("nested session cursor ended: {e}");
                }
            }
            Err(e) => warn!("could not draw the nested session's cursor: {e}"),
        })
}

struct Cursor {
    conn: RustConnection,
    root: Window,
    window: Window,
    image_order: ImageOrder,
    /// The current image's hotspot, and whether it's anything but invisible.
    hotspot: (i16, i16),
    visible: bool,
    mapped: bool,
    position: Option<(i16, i16)>,
}

impl Cursor {
    fn new(display: &str) -> Result<Self, NestedSessionError> {
        let (conn, screen) = RustConnection::connect(Some(display)).map_err(x11)?;
        let screen = conn.setup().roots[screen].clone();
        let root = screen.root;
        let image_order = conn.setup().image_byte_order;

        conn.xfixes_query_version(5, 0)
            .map_err(x11)?
            .reply()
            .map_err(x11)?;
        conn.xinput_xi_query_version(2, 0)
            .map_err(x11)?
            .reply()
            .map_err(x11)?;

        // A 32-bit visual, so the cursor's own transparency survives picom.
        let visual = screen
            .allowed_depths
            .iter()
            .filter(|depth| depth.depth == 32)
            .flat_map(|depth| depth.visuals.iter())
            .find(|visual| visual.class == VisualClass::TRUE_COLOR)
            .ok_or_else(|| x11("the nested server has no 32-bit visual"))?
            .visual_id;
        let colormap = conn.generate_id().map_err(x11)?;
        conn.create_colormap(ColormapAlloc::NONE, colormap, root, visual)
            .map_err(x11)?;

        let window = conn.generate_id().map_err(x11)?;
        conn.create_window(
            32,
            window,
            root,
            0,
            0,
            1,
            1,
            0,
            WindowClass::INPUT_OUTPUT,
            visual,
            // A depth-32 window under a depth-24 root needs its own border
            // pixel and colormap, or this is a BadMatch.
            &CreateWindowAux::new()
                .override_redirect(1)
                .border_pixel(0)
                .background_pixel(0)
                .colormap(colormap),
        )
        .map_err(x11)?
        .check()
        .map_err(x11)?;
        // Nothing reaches it: clicks land on whatever is underneath.
        let empty = conn.generate_id().map_err(x11)?;
        conn.xfixes_create_region(empty, &[]).map_err(x11)?;
        conn.xfixes_set_window_shape_region(window, SK::INPUT, 0, 0, empty)
            .map_err(x11)?;
        conn.xfixes_destroy_region(empty).map_err(x11)?;
        conn.change_property8(
            PropMode::REPLACE,
            window,
            AtomEnum::WM_NAME,
            AtomEnum::STRING,
            b"drop-nested-session-cursor",
        )
        .map_err(x11)?;

        // Told about: the cursor changing, the pointer moving (raw, so grabs
        // don't hide it), and windows being mapped or restacked over ours.
        conn.xfixes_select_cursor_input(root, xfixes::CursorNotifyMask::DISPLAY_CURSOR)
            .map_err(x11)?;
        conn.xinput_xi_select_events(
            root,
            &[xinput::EventMask {
                deviceid: xinput::Device::ALL_MASTER.into(),
                mask: vec![xinput::XIEventMask::RAW_MOTION],
            }],
        )
        .map_err(x11)?;
        conn.change_window_attributes(
            root,
            &ChangeWindowAttributesAux::new().event_mask(EventMask::SUBSTRUCTURE_NOTIFY),
        )
        .map_err(x11)?
        .check()
        .map_err(x11)?;

        let mut cursor = Cursor {
            conn,
            root,
            window,
            image_order,
            hotspot: (0, 0),
            visible: false,
            mapped: false,
            position: None,
        };
        cursor.update_image()?;
        cursor.update_position(true)?;
        cursor.conn.flush().map_err(x11)?;
        Ok(cursor)
    }

    fn run(&mut self) -> Result<(), NestedSessionError> {
        let mut last_poll = Instant::now();
        loop {
            let (mut image_changed, mut moved, mut restacked) = (false, false, false);
            while let Some(event) = self.conn.poll_for_event().map_err(x11)? {
                match event {
                    Event::XfixesCursorNotify(_) => image_changed = true,
                    Event::XinputRawMotion(_) => moved = true,
                    // Anything mapped or raised may now cover us.
                    Event::MapNotify(e) if e.window != self.window => restacked = true,
                    Event::ConfigureNotify(e) if e.window != self.window => restacked = true,
                    _ => {}
                }
            }
            if image_changed {
                self.update_image()?;
            }
            if moved || image_changed || last_poll.elapsed() >= POLL_INTERVAL {
                self.update_position(restacked)?;
                last_poll = Instant::now();
            } else if restacked {
                self.raise()?;
            }
            self.conn.flush().map_err(x11)?;
            wait_readable(&self.conn, POLL_INTERVAL)?;
        }
    }

    /// Puts the session's current cursor image into the window.
    fn update_image(&mut self) -> Result<(), NestedSessionError> {
        let conn = &self.conn;
        let image = conn
            .xfixes_get_cursor_image()
            .map_err(x11)?
            .reply()
            .map_err(x11)?;
        self.hotspot = (image.xhot as i16, image.yhot as i16);
        self.visible = image.width > 0
            && image.height > 0
            && image.cursor_image.iter().any(|pixel| pixel >> 24 != 0);
        if !self.visible {
            return Ok(());
        }

        let (width, height) = (image.width, image.height);
        // XFixes hands out premultiplied ARGB - what an ARGB visual holds.
        let bytes: Vec<u8> = image
            .cursor_image
            .iter()
            .flat_map(|pixel| match self.image_order {
                ImageOrder::MSB_FIRST => pixel.to_be_bytes(),
                _ => pixel.to_le_bytes(),
            })
            .collect();

        let pixmap = conn.generate_id().map_err(x11)?;
        conn.create_pixmap(32, pixmap, self.window, width, height)
            .map_err(x11)?;
        let gc = conn.generate_id().map_err(x11)?;
        conn.create_gc(gc, pixmap, &CreateGCAux::new())
            .map_err(x11)?;
        // A scaled cursor theme can go past the maximum request size in one
        // PutImage, so send rows in chunks.
        let row_bytes = usize::from(width) * 4;
        let rows_per_request = ((conn.maximum_request_bytes() - 64) / row_bytes).max(1);
        for (chunk, rows) in bytes.chunks(row_bytes * rows_per_request).enumerate() {
            conn.put_image(
                ImageFormat::Z_PIXMAP,
                pixmap,
                gc,
                width,
                (rows.len() / row_bytes) as u16,
                0,
                (chunk * rows_per_request) as i16,
                0,
                32,
                rows,
            )
            .map_err(x11)?;
        }
        conn.free_gc(gc).map_err(x11)?;

        // As the background, so the server repaints it by itself whenever
        // it's needed; the pixmap can go once it's set.
        conn.change_window_attributes(
            self.window,
            &ChangeWindowAttributesAux::new().background_pixmap(pixmap),
        )
        .map_err(x11)?;
        conn.free_pixmap(pixmap).map_err(x11)?;
        conn.configure_window(
            self.window,
            &ConfigureWindowAux::new()
                .width(u32::from(width))
                .height(u32::from(height)),
        )
        .map_err(x11)?;
        conn.clear_area(false, self.window, 0, 0, 0, 0)
            .map_err(x11)?;
        Ok(())
    }

    /// Follows the pointer, and shows or hides the window with the image.
    fn update_position(&mut self, force_raise: bool) -> Result<(), NestedSessionError> {
        let pointer = self
            .conn
            .query_pointer(self.root)
            .map_err(x11)?
            .reply()
            .map_err(x11)?;
        let position = (pointer.root_x, pointer.root_y);

        if !self.visible {
            if self.mapped {
                self.conn.unmap_window(self.window).map_err(x11)?;
                self.mapped = false;
            }
            self.position = Some(position);
            return Ok(());
        }

        if self.position != Some(position) || !self.mapped || force_raise {
            self.conn
                .configure_window(
                    self.window,
                    &ConfigureWindowAux::new()
                        .x(i32::from(position.0 - self.hotspot.0))
                        .y(i32::from(position.1 - self.hotspot.1))
                        .stack_mode(StackMode::ABOVE),
                )
                .map_err(x11)?;
            self.position = Some(position);
        }
        if !self.mapped {
            self.conn.map_window(self.window).map_err(x11)?;
            self.mapped = true;
        }
        Ok(())
    }

    fn raise(&self) -> Result<(), NestedSessionError> {
        if self.mapped {
            self.conn
                .configure_window(
                    self.window,
                    &ConfigureWindowAux::new().stack_mode(StackMode::ABOVE),
                )
                .map_err(x11)?;
        }
        Ok(())
    }
}

/// Waits until the connection has something to read, or `timeout` passes.
fn wait_readable(conn: &RustConnection, timeout: Duration) -> Result<(), NestedSessionError> {
    let mut poll = libc::pollfd {
        fd: conn.stream().as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    let timeout = libc::c_int::try_from(timeout.as_millis()).unwrap_or(libc::c_int::MAX);
    if unsafe { libc::poll(&mut poll, 1, timeout) } == -1 {
        let error = std::io::Error::last_os_error();
        if error.kind() != std::io::ErrorKind::Interrupted {
            return Err(error.into());
        }
    }
    // A closed connection reads as "readable"; the next poll_for_event
    // reports it.
    Ok(())
}
