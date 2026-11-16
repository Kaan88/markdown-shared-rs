//! Frameless-window helpers that are pure functions of a pointer position and the window size.

use winit::window::ResizeDirection;

/// How close to the window edge a press resizes, in logical px. The window has no frame outside
/// its pixels to grab, so the band has to be wide enough to hit without aiming.
pub const EDGE: f32 = 8.0;
/// How far along an edge from a corner a press still resizes diagonally.
pub const CORNER: f32 = 20.0;

/// The resize direction for a pointer at logical `(x, y)` in a `w` x `h` window, or `None` when it
/// is not on the resize band. The caller returns `None` itself for maximized windows.
pub fn edge_at(x: f32, y: f32, w: f32, h: f32) -> Option<ResizeDirection> {
    if !(x < EDGE || x >= w - EDGE || y < EDGE || y >= h - EDGE) {
        return None;
    }
    // On an edge: the corner band along it turns the resize diagonal.
    let (l, r, t, b) = (x < CORNER, x >= w - CORNER, y < CORNER, y >= h - CORNER);
    let (l, r) = (l && (x < EDGE || t || b), r && (x >= w - EDGE || t || b));
    let (t, b) = (t && (y < EDGE || l || r), b && (y >= h - EDGE || l || r));
    Some(match (l, r, t, b) {
        (true, _, true, _) => ResizeDirection::NorthWest,
        (_, true, true, _) => ResizeDirection::NorthEast,
        (true, _, _, true) => ResizeDirection::SouthWest,
        (_, true, _, true) => ResizeDirection::SouthEast,
        (true, ..) => ResizeDirection::West,
        (_, true, ..) => ResizeDirection::East,
        (_, _, true, _) => ResizeDirection::North,
        (_, _, _, true) => ResizeDirection::South,
        _ => return None,
    })
}

/// Gives the window the application's identity on Linux: the Wayland app id and the X11 `WM_CLASS`
/// (both `name`, which the desktop entry's `StartupWMClass` repeats, so the launcher's icon and
/// grouping match), and from `icon_png` the X11 window icon (`_NET_WM_ICON`; Wayland takes its icon
/// from the desktop entry). Call it on the attributes before `create_window`. On Windows and on
/// other systems the attributes come back unchanged. A PNG that does not decode is ignored.
#[cfg(target_os = "linux")]
pub fn identify(
    attrs: winit::window::WindowAttributes,
    name: &str,
    icon_png: Option<&[u8]>,
) -> winit::window::WindowAttributes {
    use winit::platform::wayland::WindowAttributesExtWayland;
    use winit::platform::x11::WindowAttributesExtX11;
    let mut attrs = WindowAttributesExtWayland::with_name(attrs, name, name);
    attrs = WindowAttributesExtX11::with_name(attrs, name, name);
    let icon = icon_png
        .and_then(mdshared_text::decode_png_rgba)
        .and_then(|(rgba, w, h)| winit::window::Icon::from_rgba(rgba, w, h).ok());
    attrs.with_window_icon(icon)
}

/// macOS: the window keeps the system's frame with the title bar made transparent and drawn over
/// the content (the title row is ours, under the system's close, minimise and zoom buttons at its
/// left; see `chrome::NATIVE_FRAME`), and no title text. The name and icon come from the bundle.
#[cfg(target_os = "macos")]
pub fn identify(
    attrs: winit::window::WindowAttributes,
    _name: &str,
    _icon_png: Option<&[u8]>,
) -> winit::window::WindowAttributes {
    use winit::platform::macos::WindowAttributesExtMacOS;
    attrs
        .with_decorations(true)
        .with_titlebar_transparent(true)
        .with_title_hidden(true)
        .with_fullsize_content_view(true)
}

/// See the Linux version: here the attributes are returned as they are.
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub fn identify(
    attrs: winit::window::WindowAttributes,
    _name: &str,
    _icon_png: Option<&[u8]>,
) -> winit::window::WindowAttributes {
    attrs
}

/// What a window needs once it exists. macOS: input methods are on, so a dead key (Option+E,
/// then E) and the accent menu of a held key reach the text as `Ime::Commit`. Elsewhere nothing
/// yet: there they have not been tried.
pub fn created(window: &winit::window::Window) {
    #[cfg(target_os = "macos")]
    window.set_ime_allowed(true);
    #[cfg(not(target_os = "macos"))]
    let _ = window;
}

/// What a double click on the title row does: maximize (zoom on a Mac) or restore, minimise, or
/// nothing. macOS has the setting (Desktop & Dock, "Double-click a window's title bar to");
/// elsewhere it is always maximize.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TitleDoubleClick {
    Maximize,
    Minimize,
    Nothing,
}

impl TitleDoubleClick {
    /// The system's choice, from the global `AppleActionOnDoubleClick` on macOS ("Maximize",
    /// "Minimize", "None"; unset is Maximize, the default).
    pub fn system() -> TitleDoubleClick {
        #[cfg(target_os = "macos")]
        {
            use objc2_foundation::{NSString, NSUserDefaults};
            let value = NSUserDefaults::standardUserDefaults()
                .stringForKey(&NSString::from_str("AppleActionOnDoubleClick"))
                .map(|v| v.to_string());
            Self::from_setting(value.as_deref())
        }
        #[cfg(not(target_os = "macos"))]
        TitleDoubleClick::Maximize
    }

    /// The action a value of `AppleActionOnDoubleClick` names.
    pub fn from_setting(value: Option<&str>) -> TitleDoubleClick {
        match value {
            Some(v) if v.eq_ignore_ascii_case("minimize") => TitleDoubleClick::Minimize,
            Some(v) if v.eq_ignore_ascii_case("none") => TitleDoubleClick::Nothing,
            _ => TitleDoubleClick::Maximize,
        }
    }
}

/// Does what a double click on the title row does ([`TitleDoubleClick::system`]).
pub fn title_double_click(window: &winit::window::Window) {
    match TitleDoubleClick::system() {
        TitleDoubleClick::Maximize => window.set_maximized(!window.is_maximized()),
        TitleDoubleClick::Minimize => window.set_minimized(true),
        TitleDoubleClick::Nothing => {}
    }
}

/// macOS: the dot in the window's close button that says a document has unsaved changes (and
/// makes the system ask before the window closes with a gesture of its own). Nothing elsewhere.
pub fn set_edited(window: &winit::window::Window, edited: bool) {
    #[cfg(target_os = "macos")]
    {
        use winit::platform::macos::WindowExtMacOS;
        window.set_document_edited(edited);
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (window, edited);
}

/// A trackpad pinch turned into zoom steps: winit gives the pinch in small deltas (+0.01 is 1 %
/// larger), which add up until they make a step of [`PinchZoom::STEP`] either way. A pinch that
/// ends drops what was left over.
#[derive(Clone, Copy, Debug, Default)]
pub struct PinchZoom {
    sum: f64,
}

impl PinchZoom {
    /// The pinch that makes one zoom step.
    pub const STEP: f64 = 0.08;

    /// Adds a pinch delta; the whole zoom steps it completes, positive to zoom in.
    pub fn add(&mut self, delta: f64) -> i32 {
        if !delta.is_finite() {
            return 0;
        }
        self.sum += delta;
        let steps = (self.sum / Self::STEP).trunc();
        self.sum -= steps * Self::STEP;
        steps as i32
    }

    /// The pinch ended or was cancelled.
    pub fn end(&mut self) {
        self.sum = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pinch_makes_whole_steps_and_keeps_the_rest() {
        let mut p = PinchZoom::default();
        assert_eq!(p.add(0.05), 0);
        assert_eq!(p.add(0.05), 1, "0.10 is one step and 0.02 left");
        assert_eq!(p.add(0.15), 2);
        assert_eq!(p.add(-0.2), -2);
        p.end();
        assert_eq!(p.add(-0.07), 0);
        assert_eq!(p.add(f64::NAN), 0);
    }

    #[test]
    fn the_title_double_click_setting() {
        use TitleDoubleClick::*;
        assert_eq!(TitleDoubleClick::from_setting(None), Maximize);
        assert_eq!(TitleDoubleClick::from_setting(Some("Maximize")), Maximize);
        assert_eq!(TitleDoubleClick::from_setting(Some("Minimize")), Minimize);
        assert_eq!(TitleDoubleClick::from_setting(Some("None")), Nothing);
        assert_eq!(TitleDoubleClick::from_setting(Some("bogus")), Maximize);
    }

    #[test]
    fn bands() {
        let (w, h) = (800.0, 600.0);
        assert_eq!(edge_at(400.0, 300.0, w, h), None);
        assert_eq!(edge_at(2.0, 300.0, w, h), Some(ResizeDirection::West));
        assert_eq!(edge_at(798.0, 300.0, w, h), Some(ResizeDirection::East));
        assert_eq!(edge_at(400.0, 1.0, w, h), Some(ResizeDirection::North));
        assert_eq!(edge_at(400.0, 599.0, w, h), Some(ResizeDirection::South));
        assert_eq!(edge_at(2.0, 2.0, w, h), Some(ResizeDirection::NorthWest));
        assert_eq!(edge_at(15.0, 2.0, w, h), Some(ResizeDirection::NorthWest));
        assert_eq!(
            edge_at(795.0, 595.0, w, h),
            Some(ResizeDirection::SouthEast)
        );
        assert_eq!(edge_at(25.0, 2.0, w, h), Some(ResizeDirection::North));
        assert_eq!(edge_at(12.0, 300.0, w, h), None);
    }
}
