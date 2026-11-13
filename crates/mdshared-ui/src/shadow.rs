//! A drop shadow behind an undecorated window, drawn by the compositor.
//!
//! On Windows `with_undecorated_shadow(true)` does it. On Linux a window without server-side
//! decorations gets no shadow, so it looks pasted onto what is behind it. KDE's KWin has a
//! protocol for exactly this: the client hands over eight small shadow images (four edges, four
//! corners) and four offsets, and KWin draws them outside the window, for the window's whole life,
//! without the client painting anything. [`Shadow::attach`] does that on Wayland
//! (`org_kde_kwin_shadow_manager`). Where the compositor lacks that global (GNOME's Mutter,
//! wlroots compositors) and on X11 sessions it returns `None` and nothing changes.
//!
//! Where the apps call it:
//!
//! 1. Once after `create_window`: `let shadow = Shadow::attach(&window);` and keep the `Option`
//!    in the app for the window's life (dropping it removes the shadow).
//! 2. `if let Some(s) = &mut shadow { s.update(scale, active, maximized) }` on
//!    `WindowEvent::ScaleFactorChanged`, on `WindowEvent::Focused` (the shadow is lighter while the
//!    window is inactive), and after a maximise or un-maximise (`window.is_maximized()`).
//!    `update` does nothing when nothing it depends on changed, so it is cheap to call often.
//! 3. A shadow set here takes effect at the surface's next commit; the apps' next `present` is
//!    that commit, `update` itself requests no redraw.
//!
//! Nothing runs per frame: the shadow lives in the compositor. On other systems `Shadow` is an
//! empty stub whose `attach` returns `None`, so the call sites need no `cfg`.

#[cfg(target_os = "linux")]
use winit::window::Window;

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

#[cfg(target_os = "linux")]
mod wl;

/// The shadow of one window. Dropping it removes the shadow.
#[cfg(target_os = "linux")]
pub struct Shadow {
    inner: wl::Inner,
    scale: f64,
    active: bool,
}

#[cfg(target_os = "linux")]
impl Shadow {
    /// Attaches a shadow to `window` if the compositor can draw one, `None` otherwise (no
    /// compositor support, `MDSHARED_NO_SHADOW` set, or any failure on the way).
    pub fn attach(window: &Window) -> Option<Shadow> {
        if std::env::var_os("MDSHARED_NO_SHADOW").is_some() {
            return None;
        }
        let scale = window.scale_factor();
        let inner = wl::Inner::attach(window, scale, true)?;
        Some(Shadow {
            inner,
            scale,
            active: true,
        })
    }

    /// Brings the shadow in line with the window: re-renders when the scale factor or the focus
    /// changed. `maximized` is accepted so the call sites pass everything they know;
    /// see [`Shadow::attach`] and the docs for what the compositor does with it.
    pub fn update(&mut self, scale: f64, active: bool, maximized: bool) {
        let _ = maximized;
        if (scale - self.scale).abs() < 1e-6 && active == self.active {
            return;
        }
        self.scale = scale;
        self.active = active;
        self.inner.set(scale, active);
    }

    /// Which kind of shadow this is, for logging: `"wayland"` currently always `"wayland"`.
    pub fn kind(&self) -> &'static str {
        self.inner.kind()
    }

    /// Bytes of tile memory handed to the compositor.
    pub fn bytes(&self) -> usize {
        self.inner.bytes()
    }
}

#[cfg(not(target_os = "linux"))]
pub struct Shadow;

#[cfg(not(target_os = "linux"))]
impl Shadow {
    /// No-op off Linux (Windows has `with_undecorated_shadow`).
    pub fn attach(_window: &winit::window::Window) -> Option<Shadow> {
        None
    }
    pub fn update(&mut self, _scale: f64, _active: bool, _maximized: bool) {}
    pub fn kind(&self) -> &'static str {
        ""
    }
    pub fn bytes(&self) -> usize {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
