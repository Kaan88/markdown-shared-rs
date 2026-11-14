//! The X11 side of the client route: the window is the inner rectangle of a larger ARGB window.
//! `_GTK_FRAME_EXTENTS` tells the window manager the outer `margin` px of the window are an
//! invisible frame (Mutter, KWin and others place, tile and maximize by the inner rectangle), and
//! an input shape makes the pointer pass through the transparent margin except a resize band.
//! Done from a second connection to the X server by the window id from the raw handle, so no
//! winit change is needed. The window must have been created transparent (an ARGB visual).

use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::Window;
use x11rb::connection::Connection;
use x11rb::protocol::shape::{ConnectionExt as _, SK, SO};
use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _, PropMode, Rectangle};
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;

pub struct Inner {
    conn: RustConnection,
    window: u32,
    atom: u32,
}

impl Inner {
    pub fn attach(window: &Window) -> Option<Inner> {
        let id = match window.window_handle().ok()?.as_raw() {
            RawWindowHandle::Xlib(h) => h.window as u32,
            RawWindowHandle::Xcb(h) => h.window.get(),
            _ => return None,
        };
        let (conn, _) = x11rb::connect(None).ok()?;
        let atom = conn
            .intern_atom(false, b"_GTK_FRAME_EXTENTS")
            .ok()?
            .reply()
            .ok()?
            .atom;
        // The shape extension is needed for the input shape; without it the margin takes clicks.
        let _ = conn.shape_query_version().ok()?.reply().ok()?;
        Some(Inner {
            conn,
            window: id,
            atom,
        })
    }

    /// Announces `margin` device px of invisible frame on every side and shapes the input to the
    /// content grown by `band`, for a window of `w` x `h` device px.
    pub fn set(&self, margin: u32, band: u32, w: u32, h: u32) {
        let m = margin;
        let _ = self.conn.change_property32(
            PropMode::REPLACE,
            self.window,
            self.atom,
            AtomEnum::CARDINAL,
            &[m, m, m, m],
        );
        self.shape(margin, band, w, h);
        let _ = self.conn.flush();
    }

    /// Re-applies the input shape for a new window size.
    pub fn shape(&self, margin: u32, band: u32, w: u32, h: u32) {
        let b = band.min(margin) as i32;
        let m = margin as i32;
        let rect = Rectangle {
            x: (m - b) as i16,
            y: (m - b) as i16,
            width: (w as i32 - 2 * (m - b)).max(1) as u16,
            height: (h as i32 - 2 * (m - b)).max(1) as u16,
        };
        let _ = self.conn.shape_rectangles(
            SO::SET,
            SK::INPUT,
            x11rb::protocol::xproto::ClipOrdering::UNSORTED,
            self.window,
            0,
            0,
            &[rect],
        );
        let _ = self.conn.flush();
    }
}
