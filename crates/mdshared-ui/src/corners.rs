//! Rounded window corners: the setting both apps offer, the corner mask, and the Windows call.
//!
//! The window's outer corners get a radius of 8 logical px by default, scaled with the display.
//! How it is done depends on who draws the window's edge:
//!
//! - **Windows 11**: the system rounds the window; [`set_window_corners`] only tells it which
//!   preference to use (`DWMWA_WINDOW_CORNER_PREFERENCE`). Nothing is drawn by the app. Windows 10
//!   does not know the attribute: the call fails silently and the window stays square.
//! - **Wayland with the client-drawn margin** (`shadow` module): the corner squares of the content
//!   are cut with [`mask`] and the shadow beneath them is drawn for the rounded shape, in the
//!   transparent ARGB buffer ([`crate::shadow::Frame`]).
//! - **KWin's shadow protocol, X11 without the margin**: the window is opaque there, so the corners
//!   stay square.
//!
//! A window that is maximised, tiled or fullscreen is square on every route.

/// The "Window corners" choice of the Settings dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Corners {
    /// No rounding.
    Square,
    /// A radius of 4 logical px (Windows: the system's small rounding).
    Small,
    /// A radius of 8 logical px: the default, the radius of the apps' dialogs.
    #[default]
    Round,
}

impl Corners {
    /// Every choice, in the order the Settings dialog lists them.
    pub const ALL: [Corners; 3] = [Corners::Square, Corners::Small, Corners::Round];
    /// The labels of [`Corners::ALL`].
    pub const LABELS: [&'static str; 3] = ["Square", "Small", "Round"];
    /// The row's label in the Settings dialog.
    pub const SETTING_LABEL: &'static str = "Window corners";

    /// The radius in logical px.
    pub fn radius(self) -> f64 {
        match self {
            Corners::Square => 0.0,
            Corners::Small => 4.0,
            Corners::Round => 8.0,
        }
    }

    /// The radius in device px at `scale`, at least 1 for any rounding.
    pub fn px(self, scale: f64) -> u32 {
        let r = self.radius();
        if r == 0.0 || !scale.is_finite() {
            return 0;
        }
        (r * scale.clamp(0.5, 8.0)).round().max(1.0) as u32
    }

    /// The position in [`Corners::ALL`].
    pub fn index(self) -> usize {
        match self {
            Corners::Square => 0,
            Corners::Small => 1,
            Corners::Round => 2,
        }
    }

    /// The choice at a position of [`Corners::ALL`]; the default for a position past the end.
    pub fn from_index(i: usize) -> Corners {
        Corners::ALL.get(i).copied().unwrap_or_default()
    }

    /// The value the settings file keeps: the logical radius, `0`, `4` or `8`.
    pub fn to_setting(self) -> u8 {
        self.radius() as u8
    }

    /// The choice for a settings-file value; the default for anything unknown.
    pub fn from_setting(v: u8) -> Corners {
        match v {
            0 => Corners::Square,
            4 => Corners::Small,
            _ => Corners::Round,
        }
    }

    /// A name typed on a command line: `square`, `small`, `round` (or the radius `0`, `4`, `8`).
    pub fn from_name(s: &str) -> Option<Corners> {
        match s.trim().to_ascii_lowercase().as_str() {
            "square" | "0" => Some(Corners::Square),
            "small" | "4" => Some(Corners::Small),
            "round" | "8" => Some(Corners::Round),
            _ => None,
        }
    }
}

/// The help line under the setting: what the platform does with it.
pub fn setting_help() -> &'static str {
    if cfg!(windows) {
        "Windows 11 only; Windows 10 is square."
    } else {
        "Some desktops need a restart to apply."
    }
}

/// How much of each pixel of the top-left corner square (`r` x `r`, row-major) lies inside the
/// window's rounded corner: 255 inside the arc, 0 outside, a ramp one pixel wide across it. The
/// arc is centred `r` px in from both edges. The other three corners are mirror images. Empty for
/// a radius of 0.
pub fn mask(r: u32) -> Vec<u8> {
    let r = r as usize;
    let rf = r as f64;
    let mut out = Vec::with_capacity(r * r);
    for y in 0..r {
        for x in 0..r {
            let d = (rf - x as f64 - 0.5).hypot(rf - y as f64 - 0.5);
            out.push(((rf - d + 0.5).clamp(0.0, 1.0) * 255.0).round() as u8);
        }
    }
    out
}

/// Tells Windows how to round `hwnd`: the system's rounding (8 px), its small rounding (4 px) or
/// none. Windows 10 ignores it. Other systems: nothing.
#[cfg(windows)]
pub fn set_window_corners(hwnd: isize, corners: Corners) {
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute;
    // DWMWCP_DONOTROUND, DWMWCP_ROUNDSMALL, DWMWCP_ROUND.
    let preference: u32 = match corners {
        Corners::Square => 1,
        Corners::Small => 3,
        Corners::Round => 2,
    };
    // SAFETY: the pointer references a 4-byte local valid for the call; a failure (Windows 10)
    // is ignored.
    unsafe {
        // 33 is DWMWA_WINDOW_CORNER_PREFERENCE.
        DwmSetWindowAttribute(hwnd as HWND, 33, &preference as *const u32 as *const _, 4);
    }
}

/// Tells Windows how to round `hwnd`; nothing off Windows.
#[cfg(not(windows))]
pub fn set_window_corners(_hwnd: isize, _corners: Corners) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choices_round_trip_and_default_is_eight() {
        for c in Corners::ALL {
            assert_eq!(Corners::from_index(c.index()), c);
            assert_eq!(Corners::from_setting(c.to_setting()), c);
            assert_eq!(Corners::ALL[c.index()], c);
        }
        assert_eq!(Corners::default().radius(), 8.0);
        assert_eq!(Corners::from_index(9), Corners::Round);
        assert_eq!(Corners::from_setting(200), Corners::Round);
        assert_eq!(Corners::LABELS.len(), Corners::ALL.len());
        assert_eq!(Corners::Square.to_setting(), 0);
    }

    #[test]
    fn names_on_a_command_line() {
        assert_eq!(Corners::from_name("Square"), Some(Corners::Square));
        assert_eq!(Corners::from_name(" small "), Some(Corners::Small));
        assert_eq!(Corners::from_name("8"), Some(Corners::Round));
        assert_eq!(Corners::from_name("pointy"), None);
    }

    #[test]
    fn radius_scales_with_the_display() {
        assert_eq!(Corners::Round.px(1.0), 8);
        assert_eq!(Corners::Round.px(2.0), 16);
        assert_eq!(Corners::Round.px(1.5), 12);
        assert_eq!(Corners::Small.px(1.0), 4);
        assert_eq!(Corners::Square.px(2.0), 0);
        assert_eq!(Corners::Round.px(f64::NAN), 0);
        assert!(Corners::Small.px(0.1) >= 1);
    }

    #[test]
    fn mask_is_empty_for_no_radius_and_cuts_the_corner() {
        assert!(mask(0).is_empty());
        let r = 8usize;
        let m = mask(r as u32);
        assert_eq!(m.len(), r * r);
        // The outermost pixel is outside the arc, the innermost inside it.
        assert_eq!(m[0], 0);
        assert_eq!(m[r * r - 1], 255);
        // The pixels beside the straight edges (far from the arc's start) are inside.
        assert!(m[r - 1] >= 250, "top row, last pixel");
        assert!(m[(r - 1) * r] >= 250, "left column, last pixel");
        // Symmetric across the diagonal.
        for y in 0..r {
            for x in 0..r {
                assert_eq!(m[y * r + x], m[x * r + y]);
            }
        }
        // Monotonic along the diagonal, with a partly covered pixel on the arc (anti-aliased).
        let diag: Vec<u8> = (0..r).map(|i| m[i * r + i]).collect();
        assert!(diag.windows(2).all(|w| w[0] <= w[1]), "{diag:?}");
        assert!(
            m.iter().any(|&v| v > 0 && v < 255),
            "the edge is anti-aliased"
        );
    }

    #[test]
    fn mask_area_is_close_to_the_arc() {
        // The covered area of a corner square is r*r minus the corner cut away: r*r*(1 - pi/4)
        // is outside. The linear ramp keeps the sum within a few percent of that.
        for r in [4u32, 8, 16, 32] {
            let m = mask(r);
            let covered: f64 = m.iter().map(|&v| v as f64 / 255.0).sum();
            let exact = (r * r) as f64 * std::f64::consts::FRAC_PI_4;
            let outside_exact = (r * r) as f64 - exact;
            let outside = (r * r) as f64 - covered;
            assert!(
                (outside - outside_exact).abs() < 0.08 * (r * r) as f64,
                "r {r}: outside {outside}, exact {outside_exact}"
            );
        }
    }
}
