//! A drop shadow behind an undecorated window.
//!
//! On Windows `with_undecorated_shadow(true)` does it. On Linux a window without server-side
//! decorations gets no shadow, so it looks pasted onto what is behind it. [`Shadow::attach`] is the
//! one entry point and picks a route (the policy is [`decide`]: KWin's protocol wherever the
//! compositor has it, the client margin only where it is missing):
//!
//! 1. **client** (Wayland, any compositor; feature `client-margin`): the window is the inner
//!    rectangle of a larger transparent surface and the app paints the shadow into the margin
//!    ([`Shadow::paint`]). Needs the patched winit and softbuffer, see `docs/LINUX.md`.
//! 2. **kwin** (Wayland on KDE): KWin's `org_kde_kwin_shadow_manager`: eight small tiles handed to
//!    the compositor, which draws them outside the window. Nothing for the app to paint.
//! 3. **x11** (X11 and Xwayland; feature `client-margin`): the same margin, announced with
//!    `_GTK_FRAME_EXTENTS`; the compositor treats it as an invisible frame.
//! 4. nothing: `attach` returns `None` and the window is as it was.
//!
//! The client routes make the surface bigger than the window by [`Shadow::margin`] on every side
//! while the window is in a normal state; maximized, fullscreen or tiled the margin is 0 and the
//! surface is the window. The app's checklist (transparent window, buffer sizes, where to call
//! [`Shadow::paint`], pointer mapping, minimum size) is in `docs/LINUX.md`.
//!
//! Where the apps call it:
//!
//! 1. `Shadow::prepare(attributes, mode)` on the window attributes (turns on transparency when a
//!    client route may be used), then `Shadow::attach_mode(&window, mode)` (or `attach`) once after
//!    `create_window`; keep the `Option` for the window's life.
//! 2. `shadow.update(&window, scale, active)` on `Resized`, `ScaleFactorChanged` and `Focused`;
//!    it returns `true` when the margin or look changed (redraw then).
//! 3. In the redraw: allocate the buffer at `window.inner_size()` (the SURFACE), render the content
//!    at [`Shadow::content_size`], copy it to [`Shadow::content_origin`], and call
//!    [`Shadow::paint_frame`] with the buffer's age.
//!
//! On other systems `Shadow` is an empty stub whose `attach` returns `None`, so the call sites
//! need no `cfg`.

#[cfg(target_os = "linux")]
use winit::window::Window;
use winit::window::WindowAttributes;

/// Where the shadow reaches beyond the window, in logical px, and how it is shaped.
pub const EXTENT: f64 = 36.0;
/// Standard deviation of the blur, in logical px (a blur radius of about 24).
pub const SIGMA: f64 = 12.0;
/// How far the shadow is shifted down, in logical px.
pub const DROP: f64 = 6.0;
/// The darkest the shadow gets right at the window edge's own blur centre, active window.
pub const PEAK_ACTIVE: f64 = 0.35;
/// The same for a window that is not focused.
pub const PEAK_INACTIVE: f64 = 0.20;

/// The eight tiles as premultiplied ARGB8888 (little-endian: bytes B, G, R, A; the shadow is
/// black, so only A is non-zero). `m` is the tile depth in device px.
#[derive(Debug, Clone, PartialEq)]
pub struct Tiles {
    /// Device px the shadow reaches beyond the window.
    pub m: u32,
    /// `top`, `top_right`, `right`, `bottom_right`, `bottom`, `bottom_left`, `left`, `top_left`,
    /// the protocol's order; with their `(width, height)`.
    pub tiles: [(u32, u32, Vec<u8>); 8],
}

fn erf(x: f64) -> f64 {
    // Abramowitz and Stegun 7.1.26, error below 1.5e-7: far below one 8-bit alpha step.
    let t = 1.0 / (1.0 + 0.327_591_1 * x.abs());
    let y = 1.0
        - (((((1.061_405_429 * t - 1.453_152_027) * t) + 1.421_413_741) * t - 0.284_496_736) * t
            + 0.254_829_592)
            * t
            * (-x * x).exp();
    y.copysign(x)
}

/// Coverage of a half-plane whose edge is `d` px outward of the point, blurred by `sigma`:
/// 1 well inside, 0.5 on the edge, 0 well outside.
fn coverage(d: f64, sigma: f64) -> f64 {
    0.5 * (1.0 - erf(d / (sigma * std::f64::consts::SQRT_2)))
}

/// Renders the eight tiles for a window scale factor. The shadow is the window's rectangle shifted
/// down by [`DROP`] and blurred, so each tile is the product of one horizontal and one vertical
/// half-plane coverage; the part under the window is never seen, so the tiles stop at its edge.
pub fn render(scale: f64, active: bool) -> Tiles {
    let scale = if scale.is_finite() {
        scale.clamp(1.0, 8.0)
    } else {
        1.0
    };
    let m = (EXTENT * scale).ceil() as u32;
    let (sigma, drop) = (SIGMA * scale, DROP * scale);
    let peak = if active { PEAK_ACTIVE } else { PEAK_INACTIVE };
    // `h`/`v`: -1 = tile on the left/top of the window, +1 = right/bottom, 0 = spans the side.
    let tile = |h: i32, v: i32| {
        let (w, hgt) = (if h == 0 { 1 } else { m }, if v == 0 { 1 } else { m });
        let mut data = Vec::with_capacity((w * hgt * 4) as usize);
        for y in 0..hgt {
            for x in 0..w {
                // Distance outward of the window edge, at the pixel's centre.
                let out = |i: u32, side: i32| {
                    if side < 0 {
                        m as f64 - (i as f64 + 0.5)
                    } else {
                        i as f64 + 0.5
                    }
                };
                let ah = if h == 0 {
                    1.0
                } else {
                    coverage(out(x, h), sigma)
                };
                let av = match v {
                    0 => 1.0,
                    // The shadow's top edge is `drop` inside the window's, its bottom `drop` outside.
                    -1 => coverage(out(y, v) + drop, sigma),
                    _ => coverage(out(y, v) - drop, sigma),
                };
                let a = (ah * av * peak * 255.0).round().clamp(0.0, 255.0) as u8;
                data.extend_from_slice(&[0, 0, 0, a]);
            }
        }
        (w, hgt, data)
    };
    Tiles {
        m,
        tiles: [
            tile(0, -1),
            tile(1, -1),
            tile(1, 0),
            tile(1, 1),
            tile(0, 1),
            tile(-1, 1),
            tile(-1, 0),
            tile(-1, -1),
        ],
    }
}

mod client;
pub use client::takes_input;

#[cfg(target_os = "linux")]
mod wl;
#[cfg(all(target_os = "linux", feature = "client-margin"))]
mod x11;

/// The kind of display the window lives on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Session {
    Wayland,
    X11,
    Other,
}

/// The route chosen for a window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteKind {
    Kwin,
    ClientWayland,
    ClientX11,
    None,
}

/// The routing policy, pure so it can be tested as a table. `client_built`: the crate has the
/// `client-margin` feature; `kwin`: the compositor offers KWin's shadow protocol.
///
/// `Auto`: on Wayland KWin's protocol wherever it exists (KDE: the proven path, the window is
/// created as without a shadow: not transparent, no margin), the client margin only where it is
/// missing (GNOME, sway, other compositors) and the feature is built; on X11 nothing, because a
/// tiled X11 window would draw its shadow over its neighbour (winit cannot report tiling there).
/// The other modes force their route wherever it can work.
pub fn decide(mode: Mode, client_built: bool, session: Session, kwin: bool) -> RouteKind {
    match (mode, session) {
        (Mode::None, _) | (_, Session::Other) => RouteKind::None,
        (Mode::Kwin, Session::Wayland) if kwin => RouteKind::Kwin,
        (Mode::Kwin, _) => RouteKind::None,
        (Mode::Client, Session::Wayland) if client_built => RouteKind::ClientWayland,
        (Mode::Client, Session::X11) if client_built => RouteKind::ClientX11,
        (Mode::Client, _) => RouteKind::None,
        (Mode::Auto, Session::Wayland) if kwin => RouteKind::Kwin,
        (Mode::Auto, Session::Wayland) if client_built => RouteKind::ClientWayland,
        (Mode::Auto, _) => RouteKind::None,
    }
}

/// The session and the KWin answer for a display, probed once per process (the answer is cached
/// for `attach`; the probe is one registry round trip, measured at start-up in `docs/LINUX.md`).
#[cfg(target_os = "linux")]
fn probe(display: &impl winit::raw_window_handle::HasDisplayHandle) -> (Session, bool) {
    use std::sync::OnceLock;
    use winit::raw_window_handle::RawDisplayHandle;
    static CACHE: OnceLock<(Session, bool)> = OnceLock::new();
    *CACHE.get_or_init(|| match display.display_handle().map(|h| h.as_raw()) {
        Ok(RawDisplayHandle::Wayland(d)) => (
            Session::Wayland,
            wl::kwin_shadow_available(d.display.as_ptr().cast()),
        ),
        Ok(RawDisplayHandle::Xlib(_) | RawDisplayHandle::Xcb(_)) => (Session::X11, false),
        _ => (Session::Other, false),
    })
}

/// Logical px outside the content that still take pointer input on the client route (the winit
/// patch's input region), so an edge resize can start just outside the visible window.
pub const RESIZE_BAND: u32 = 6;

/// Which route [`Shadow::attach_mode`] may use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    /// KWin's protocol where the compositor offers it, else the client margin on Wayland (feature
    /// `client-margin`), else nothing; nothing on X11. See [`decide`].
    #[default]
    Auto,
    /// Only the client-drawn margin (Wayland or X11).
    Client,
    /// Only KWin's shadow protocol.
    Kwin,
    /// No shadow.
    None,
}

impl Mode {
    /// `MD_SHADOW=client|kwin|none|auto` (anything else: auto).
    pub fn from_env() -> Mode {
        match std::env::var("MD_SHADOW").as_deref() {
            Ok("client") => Mode::Client,
            Ok("kwin") => Mode::Kwin,
            Ok("none") => Mode::None,
            _ => Mode::Auto,
        }
    }
}

/// Whether this build can use the client-drawn margin (feature `client-margin`, which the apps turn
/// on together with the patched winit and softbuffer).
pub const CLIENT_ROUTE_BUILT: bool = cfg!(all(target_os = "linux", feature = "client-margin"));

/// The shadow of one window. Dropping it removes the KWin shadow; the client margin stays with the
/// window (keep the `Shadow` for the window's life).
#[cfg(target_os = "linux")]
pub struct Shadow {
    route: Route,
    scale: f64,
    active: bool,
    /// The margin in force, device px (client routes; 0 when maximized, tiled, fullscreen).
    margin: u32,
    painter: Option<client::Painter>,
    gate: client::Gate,
}

#[cfg(target_os = "linux")]
enum Route {
    Kwin(wl::Inner),
    #[cfg(feature = "client-margin")]
    ClientWayland,
    #[cfg(feature = "client-margin")]
    ClientX11(Box<x11::Inner>),
}

/// The resize band in device px.
#[cfg(target_os = "linux")]
fn band(scale: f64) -> u32 {
    (RESIZE_BAND as f64 * scale).ceil() as u32
}

#[cfg(target_os = "linux")]
impl Shadow {
    /// Window attributes for a window that will get a shadow under `mode`: when a client route may
    /// be used the window is transparent (the margin shows through) and a logical `with_inner_size`
    /// (the CONTENT size the app wants) is grown by the margin, because on the client route the
    /// initial size is a surface size (X11: not grown). Min/max sizes stay content sizes.
    /// Otherwise unchanged.
    pub fn prepare(
        mut attributes: WindowAttributes,
        mode: Mode,
        event_loop: &winit::event_loop::ActiveEventLoop,
    ) -> WindowAttributes {
        let mode = if std::env::var_os("MDSHARED_NO_SHADOW").is_some() {
            Mode::None
        } else {
            mode
        };
        let (session, kwin) = probe(event_loop);
        match decide(mode, CLIENT_ROUTE_BUILT, session, kwin) {
            RouteKind::ClientWayland => {
                if let Some(winit::dpi::Size::Logical(l)) = attributes.inner_size {
                    attributes.inner_size =
                        Some(winit::dpi::Size::Logical(winit::dpi::LogicalSize::new(
                            l.width + 2.0 * EXTENT,
                            l.height + 2.0 * EXTENT,
                        )));
                }
                attributes.with_transparent(true)
            }
            // X11: the window manager takes the initial size as the frame, not grown.
            RouteKind::ClientX11 => attributes.with_transparent(true),
            RouteKind::Kwin | RouteKind::None => attributes,
        }
    }

    /// Attaches a shadow to `window` by the best route, `None` if there is none (no compositor
    /// support, `MDSHARED_NO_SHADOW` set, or any failure on the way). `MD_SHADOW` picks a route.
    pub fn attach(window: &Window) -> Option<Shadow> {
        Shadow::attach_mode(window, Mode::from_env())
    }

    /// [`Shadow::attach`] restricted to one route.
    pub fn attach_mode(window: &Window, mode: Mode) -> Option<Shadow> {
        if std::env::var_os("MDSHARED_NO_SHADOW").is_some() {
            return None;
        }
        let (session, kwin) = probe(window);
        let scale = window.scale_factor();
        match decide(mode, CLIENT_ROUTE_BUILT, session, kwin) {
            RouteKind::Kwin => {
                let inner = wl::Inner::attach(window, scale, true)?;
                Some(Shadow {
                    route: Route::Kwin(inner),
                    scale,
                    active: true,
                    margin: 0,
                    painter: None,
                    gate: client::Gate::default(),
                })
            }
            #[cfg(feature = "client-margin")]
            RouteKind::ClientWayland | RouteKind::ClientX11 => Shadow::attach_client(window, scale),
            _ => None,
        }
    }

    #[cfg(feature = "client-margin")]
    fn attach_client(window: &Window, scale: f64) -> Option<Shadow> {
        use winit::platform::wayland::WindowExtWayland;
        use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
        let route = match window.window_handle().ok()?.as_raw() {
            RawWindowHandle::Wayland(_) => {
                window.set_content_margin(EXTENT as u32);
                Route::ClientWayland
            }
            RawWindowHandle::Xlib(_) | RawWindowHandle::Xcb(_) => {
                let inner = x11::Inner::attach(window)?;
                let size = window.inner_size();
                inner.set(
                    (EXTENT * scale).ceil() as u32,
                    band(scale),
                    size.width,
                    size.height,
                );
                Route::ClientX11(Box::new(inner))
            }
            _ => return None,
        };
        let mut s = Shadow {
            route,
            scale,
            active: true,
            margin: 0,
            painter: Some(client::Painter::new(scale, true)),
            gate: client::Gate::default(),
        };
        s.margin = s.margin_now(window);
        Some(s)
    }

    /// The margin in force, device px, read from the window.
    #[cfg_attr(not(feature = "client-margin"), allow(unused_variables))]
    fn margin_now(&self, window: &Window) -> u32 {
        match self.route {
            #[cfg(feature = "client-margin")]
            Route::ClientWayland => {
                use winit::platform::wayland::WindowExtWayland;
                match window.content_margin_now() {
                    0 => 0,
                    l => self
                        .painter
                        .as_ref()
                        .map_or(0, |p| p.m.min((l as f64 * self.scale).ceil() as u32)),
                }
            }
            // X11: the extents are announced once and the window manager keeps the margin even
            // maximized (the frame is the inner rectangle, the margin lies off screen).
            #[cfg(feature = "client-margin")]
            Route::ClientX11(_) => self.painter.as_ref().map_or(0, |p| p.m),
            Route::Kwin(_) => 0,
        }
    }

    /// Brings the shadow in line with the window; call on `Resized`, `ScaleFactorChanged` and
    /// `Focused` (cheap when nothing changed). Re-renders the tiles for a new scale or focus and
    /// re-reads the margin in force (it is 0 while maximized, fullscreen or tiled). Returns `true`
    /// when something the app draws changed: the margin, the scale, or the focus look. Redraw then.
    pub fn update(&mut self, window: &Window, scale: f64, active: bool) -> bool {
        let look = (scale - self.scale).abs() >= 1e-6 || active != self.active;
        if look {
            self.scale = scale;
            self.active = active;
            match &mut self.route {
                Route::Kwin(w) => w.set(scale, active),
                #[cfg(feature = "client-margin")]
                Route::ClientWayland => self.painter = Some(client::Painter::new(scale, active)),
                #[cfg(feature = "client-margin")]
                Route::ClientX11(_) => self.painter = Some(client::Painter::new(scale, active)),
            }
            self.gate.invalidate();
        }
        let margin = self.margin_now(window);
        #[cfg(feature = "client-margin")]
        if let Route::ClientX11(x) = &self.route {
            // The extents and the input shape follow the window's size and scale.
            let size = window.inner_size();
            x.set(margin, band(scale), size.width, size.height);
        }
        let moved = margin != self.margin;
        self.margin = margin;
        if moved {
            self.gate.invalidate();
        }
        // The KWin route draws nothing in the app: nothing for it to redraw.
        look && self.painter.is_some() || moved
    }

    /// Which route this is, for logging: `"client"`, `"client-x11"` or `"kwin"`.
    pub fn kind(&self) -> &'static str {
        match self.route {
            Route::Kwin(_) => "kwin",
            #[cfg(feature = "client-margin")]
            Route::ClientWayland => "client",
            #[cfg(feature = "client-margin")]
            Route::ClientX11(_) => "client-x11",
        }
    }

    /// Bytes of tile memory kept (the client route) or handed to the compositor (KWin).
    pub fn bytes(&self) -> usize {
        match &self.route {
            Route::Kwin(w) => w.bytes(),
            #[cfg(feature = "client-margin")]
            Route::ClientWayland | Route::ClientX11(_) => {
                self.painter.as_ref().map_or(0, |p| p.bytes())
            }
        }
    }

    /// Whether the app must give the window an alpha channel: the client routes. With the patched
    /// softbuffer: `surface.set_premultiplied_alpha(shadow.wants_alpha())`; the content must then
    /// be written with alpha `0xFF`.
    pub fn wants_alpha(&self) -> bool {
        !matches!(self.route, Route::Kwin(_))
    }

    /// Sets the window's minimum CONTENT size, in logical px, by what the route needs: on
    /// Wayland the patched winit already counts min/max sizes as content, on X11 the window
    /// manager sees the whole window so the margin is added; the other routes pass it through.
    pub fn set_min_content_size(&self, window: &Window, size: winit::dpi::LogicalSize<f64>) {
        #[cfg(feature = "client-margin")]
        if matches!(self.route, Route::ClientX11(_)) {
            window.set_min_inner_size(Some(winit::dpi::LogicalSize::new(
                size.width + 2.0 * EXTENT,
                size.height + 2.0 * EXTENT,
            )));
            return;
        }
        window.set_min_inner_size(Some(size));
    }

    /// The margin in force, in device px: the shadow's reach on every side while the window is in
    /// a normal state, 0 when maximized, fullscreen or tiled (the surface is the window then) and
    /// on the KWin route. As of the last [`Shadow::update`].
    pub fn margin(&self) -> u32 {
        self.margin
    }

    /// Where the content rectangle starts in the surface buffer, device px: `(margin, margin)`.
    pub fn content_origin(&self) -> (u32, u32) {
        (self.margin, self.margin)
    }

    /// The content size inside a surface of `surface_w` x `surface_h` device px (never 0).
    pub fn content_size(&self, surface_w: u32, surface_h: u32) -> (u32, u32) {
        let m2 = self.margin * 2;
        (
            surface_w.saturating_sub(m2).max(1),
            surface_h.saturating_sub(m2).max(1),
        )
    }

    /// The surface size for a content of `content_w` x `content_h` device px.
    pub fn surface_size(&self, content_w: u32, content_h: u32) -> (u32, u32) {
        (content_w + self.margin * 2, content_h + self.margin * 2)
    }

    /// A pointer position in surface coordinates (what `CursorMoved` reports) as content
    /// coordinates. It is negative or past the content size in the margin's resize band.
    pub fn to_content(&self, x: f64, y: f64) -> (f64, f64) {
        (x - self.margin as f64, y - self.margin as f64)
    }

    /// The inverse of [`Shadow::to_content`].
    pub fn to_surface(&self, x: f64, y: f64) -> (f64, f64) {
        (x + self.margin as f64, y + self.margin as f64)
    }

    /// Whether a pointer at surface position `(x, y)` lands on the window (the content grown by
    /// the resize band) or goes through to what is behind it. What the winit patch sets as the
    /// input region, here for the app's own hit-testing and for tests.
    pub fn takes_input(&self, x: f64, y: f64, surface_w: u32, surface_h: u32) -> bool {
        client::takes_input(x, y, surface_w, surface_h, self.margin, band(self.scale))
    }

    /// Fills the margin of a surface buffer (`surface_w` x `surface_h` device px, stride
    /// `surface_w`) with the shadow, premultiplied ARGB over transparent, and leaves the content
    /// rectangle untouched. Does nothing when no margin is in force or on the KWin route. About
    /// 1.4 MB of writes at 3440x1440 scale 1 (well under a millisecond); prefer
    /// [`Shadow::paint_frame`], which skips it when the margin is already there.
    pub fn paint(&self, pixels: &mut [u32], surface_w: u32, surface_h: u32) {
        if let Some(p) = &self.painter {
            p.paint(pixels, surface_w, surface_h, self.margin);
        }
    }

    /// [`Shadow::paint`] only when the buffer's margin may be stale: `age` is softbuffer's
    /// `Buffer::age()` (0: undefined contents). softbuffer's Wayland backend swaps two buffers
    /// and resizes each lazily, so after any change of size, margin, scale or focus both buffers
    /// are repainted once (two frames); otherwise a frame touches no margin pixel. Returns whether
    /// it painted. With damage-limited presents (`present_with_damage`) the margin needs no
    /// damage rectangle on a normal frame; on the repainted frames damage the whole surface.
    pub fn paint_frame(
        &mut self,
        pixels: &mut [u32],
        surface_w: u32,
        surface_h: u32,
        age: u8,
    ) -> bool {
        if self.painter.is_none() {
            return false;
        }
        if !self.gate.needed((surface_w, surface_h), age) {
            return false;
        }
        self.paint(pixels, surface_w, surface_h);
        true
    }
}

#[cfg(not(target_os = "linux"))]
pub struct Shadow;

#[cfg(not(target_os = "linux"))]
impl Shadow {
    /// No-op off Linux (Windows has `with_undecorated_shadow`).
    pub fn prepare(
        attributes: WindowAttributes,
        _mode: Mode,
        _event_loop: &winit::event_loop::ActiveEventLoop,
    ) -> WindowAttributes {
        attributes
    }
    pub fn attach(_window: &winit::window::Window) -> Option<Shadow> {
        None
    }
    pub fn attach_mode(_window: &winit::window::Window, _mode: Mode) -> Option<Shadow> {
        None
    }
    pub fn update(&mut self, _window: &winit::window::Window, _scale: f64, _active: bool) -> bool {
        false
    }
    pub fn kind(&self) -> &'static str {
        ""
    }
    pub fn bytes(&self) -> usize {
        0
    }
    pub fn wants_alpha(&self) -> bool {
        false
    }
    pub fn set_min_content_size(
        &self,
        window: &winit::window::Window,
        size: winit::dpi::LogicalSize<f64>,
    ) {
        window.set_min_inner_size(Some(size));
    }
    pub fn margin(&self) -> u32 {
        0
    }
    pub fn content_origin(&self) -> (u32, u32) {
        (0, 0)
    }
    pub fn content_size(&self, w: u32, h: u32) -> (u32, u32) {
        (w, h)
    }
    pub fn surface_size(&self, w: u32, h: u32) -> (u32, u32) {
        (w, h)
    }
    pub fn to_content(&self, x: f64, y: f64) -> (f64, f64) {
        (x, y)
    }
    pub fn to_surface(&self, x: f64, y: f64) -> (f64, f64) {
        (x, y)
    }
    pub fn takes_input(&self, _x: f64, _y: f64, _w: u32, _h: u32) -> bool {
        true
    }
    pub fn paint(&self, _pixels: &mut [u32], _w: u32, _h: u32) {}
    pub fn paint_frame(&mut self, _pixels: &mut [u32], _w: u32, _h: u32, _age: u8) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routing_table() {
        use Mode::*;
        use RouteKind as R;
        use Session::*;
        // (mode, client built, session, kwin) -> route
        let t = |m, b, s, k| decide(m, b, s, k);
        for built in [false, true] {
            // KDE Wayland: Auto and Kwin use KWin's protocol, windows untouched.
            assert_eq!(t(Auto, built, Wayland, true), R::Kwin);
            assert_eq!(t(Kwin, built, Wayland, true), R::Kwin);
            // Without the protocol, Kwin has nothing.
            assert_eq!(t(Kwin, built, Wayland, false), R::None);
            // Never on X11 or elsewhere through KWin; Auto gives nothing on X11.
            assert_eq!(t(Auto, built, X11, false), R::None);
            assert_eq!(t(Kwin, built, X11, false), R::None);
            assert_eq!(t(Auto, built, Other, false), R::None);
            assert_eq!(t(Client, built, Other, false), R::None);
            // None is none.
            for s in [Wayland, X11, Other] {
                for k in [false, true] {
                    assert_eq!(t(None, built, s, k), R::None);
                }
            }
        }
        // Feature off: no client route at all.
        assert_eq!(t(Auto, false, Wayland, false), R::None);
        assert_eq!(t(Client, false, Wayland, true), R::None);
        assert_eq!(t(Client, false, X11, false), R::None);
        // Feature on: the client margin where KWin's protocol is missing (GNOME, sway) ...
        assert_eq!(t(Auto, true, Wayland, false), R::ClientWayland);
        // ... forced on KDE only on request ...
        assert_eq!(t(Client, true, Wayland, true), R::ClientWayland);
        assert_eq!(t(Client, true, Wayland, false), R::ClientWayland);
        // ... and on X11 only on request.
        assert_eq!(t(Client, true, X11, false), R::ClientX11);
    }

    fn alpha(t: &(u32, u32, Vec<u8>), x: u32, y: u32) -> u8 {
        t.2[((y * t.0 + x) * 4 + 3) as usize]
    }

    #[test]
    fn sizes_and_premultiplied_black() {
        let t = render(1.0, true);
        assert_eq!(t.m, 36);
        let dims: Vec<_> = t.tiles.iter().map(|t| (t.0, t.1)).collect();
        assert_eq!(
            dims,
            [
                (1, 36),
                (36, 36),
                (36, 1),
                (36, 36),
                (1, 36),
                (36, 36),
                (36, 1),
                (36, 36)
            ]
        );
        for (w, h, d) in &t.tiles {
            assert_eq!(d.len() as u32, w * h * 4);
            assert!(d.chunks(4).all(|p| p[..3] == [0, 0, 0]));
        }
    }

    #[test]
    fn fades_outward_and_drops_down() {
        let t = render(1.0, true);
        let top = &t.tiles[0];
        let bottom = &t.tiles[4];
        // Top tile: row 35 touches the window, row 0 is the far edge.
        assert!(alpha(top, 0, 35) > alpha(top, 0, 18));
        assert!(alpha(top, 0, 18) > alpha(top, 0, 0));
        assert!(alpha(top, 0, 0) <= 1, "far edge is invisible");
        // Bottom tile: row 0 touches the window.
        assert!(alpha(bottom, 0, 0) > alpha(bottom, 0, 18));
        assert!(alpha(bottom, 0, 35) <= 1);
        // Shifted down: more shadow below than above at the same distance.
        assert!(alpha(bottom, 0, 0) > alpha(top, 0, 35));
        // The corner is never darker than either edge beside it.
        let c = &t.tiles[3];
        assert!(alpha(c, 0, 0) <= alpha(bottom, 0, 0));
        assert!(alpha(c, 0, 0) <= alpha(&t.tiles[2], 0, 0));
    }

    #[test]
    fn inactive_is_lighter_and_scale_grows() {
        let (a, i) = (render(1.0, true), render(1.0, false));
        assert!(alpha(&a.tiles[4], 0, 0) > alpha(&i.tiles[4], 0, 0));
        assert_eq!(render(2.0, true).m, 72);
        assert_eq!(render(1.25, true).m, 45);
        assert_eq!(render(f64::NAN, true).m, 36);
    }
}
