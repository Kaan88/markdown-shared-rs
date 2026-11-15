//! Blur behind the window on KDE Plasma (KWin) under Wayland: the Linux half of the "Window
//! backdrop" setting (Windows has DWM, see [`crate::platform::set_backdrop`]).
//!
//! The compositor is asked with KWin's `org_kde_kwin_blur` protocol, on the window's own
//! `wl_display` and `wl_surface` (the same way as the shadow, `shadow/wl.rs`): the whole surface,
//! with no region. Where the global is missing (X11, GNOME, wlroots compositors, Windows) every
//! call here does nothing and says so with `false`.
//!
//! The blur only shows through pixels that have alpha. What the window must do for that is not
//! all done here (the pixels are the application's):
//!
//! * the window must be transparent (`with_transparent(true)` at creation; [`set`] also calls
//!   `Window::set_transparent(true)` when it turns the blur on, so the surface has no opaque
//!   region), and
//! * on Wayland the softbuffer surface must be told to present premultiplied alpha
//!   (`set_premultiplied_alpha(true)`, which needs
//!   `docs/patches/softbuffer-0.4.8-wayland-alpha.patch`; stock softbuffer 0.4.8 makes XRGB8888
//!   buffers and the alpha byte is ignored). See [`crate::frost::Frost::wants_alpha`]; the frame
//!   must be written with alpha too.
//!
//! Without both, the blur is requested and the compositor honours it, but the window stays an
//! opaque rectangle and nothing is seen.

use winit::raw_window_handle::{
    HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle,
};
use winit::window::Window;

#[cfg(target_os = "linux")]
mod wl;

/// The name of KWin's blur global.
pub const MANAGER_INTERFACE: &str = "org_kde_kwin_blur_manager";

/// Whether the environment variable `MDSHARED_NO_BLUR` (set, to anything) turns the blur off.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn disabled(var: Option<&std::ffi::OsStr>) -> bool {
    var.is_some()
}

/// Whether `interfaces`, the globals a compositor offers, include KWin's blur manager.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn offers_blur<'a>(mut interfaces: impl Iterator<Item = &'a str>) -> bool {
    interfaces.any(|i| i == MANAGER_INTERFACE)
}

/// The `wl_display` and `wl_surface` of a Wayland window, from winit's raw handles. `None` on
/// every other window system.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn wayland_pointers(
    display: RawDisplayHandle,
    window: RawWindowHandle,
) -> Option<(*mut std::ffi::c_void, *mut std::ffi::c_void)> {
    match (display, window) {
        (RawDisplayHandle::Wayland(d), RawWindowHandle::Wayland(w)) => {
            Some((d.display.as_ptr().cast(), w.surface.as_ptr().cast()))
        }
        _ => None,
    }
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn pointers(window: &Window) -> Option<(*mut std::ffi::c_void, *mut std::ffi::c_void)> {
    let d = window.display_handle().ok()?.as_raw();
    let w = window.window_handle().ok()?.as_raw();
    wayland_pointers(d, w)
}

/// Whether the compositor behind `window` can blur it: a Wayland window on a compositor that
/// offers `org_kde_kwin_blur_manager`. One registry round trip the first time (a private event
/// queue, winit's is never touched); false on Windows, X11 and everywhere else, and with
/// `MDSHARED_NO_BLUR` set.
pub fn available(window: &Window) -> bool {
    #[cfg(target_os = "linux")]
    {
        if disabled(std::env::var_os("MDSHARED_NO_BLUR").as_deref()) {
            return false;
        }
        pointers(window).is_some_and(|(display, surface)| wl::available(display, surface))
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = window;
        false
    }
}

/// Asks the compositor to blur behind the whole window (`on`), or to stop. Returns whether the
/// request was made: false when there is no blur protocol (nothing was done), true otherwise.
/// Turning it on also makes the window transparent (see the module notes). Takes effect with
/// the window's next frame. Asking for what is already the case does nothing.
pub fn set(window: &Window, on: bool) -> bool {
    #[cfg(target_os = "linux")]
    {
        let Some((display, surface)) = pointers(window) else {
            return false;
        };
        if disabled(std::env::var_os("MDSHARED_NO_BLUR").as_deref()) {
            return false;
        }
        if !wl::set(display, surface, on) {
            return false;
        }
        if on {
            window.set_transparent(true);
        }
        // The compositor applies the blur with the surface's next commit.
        window.request_redraw();
        true
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (window, on);
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;
    use std::ptr::NonNull;
    use winit::raw_window_handle::{
        WaylandDisplayHandle, WaylandWindowHandle, Win32WindowHandle, WindowsDisplayHandle,
        XlibDisplayHandle, XlibWindowHandle,
    };

    #[test]
    fn the_blur_global_is_found_by_name_only() {
        assert!(offers_blur(
            ["wl_shm", "org_kde_kwin_blur_manager", "wl_seat"].into_iter()
        ));
        assert!(!offers_blur(
            ["wl_shm", "org_kde_kwin_shadow_manager"].into_iter()
        ));
        assert!(!offers_blur(["org_kde_kwin_blur"].into_iter()));
        assert!(!offers_blur(std::iter::empty()));
    }

    #[test]
    fn the_environment_switch() {
        assert!(!disabled(None));
        assert!(disabled(Some(OsStr::new("1"))));
        assert!(disabled(Some(OsStr::new(""))));
    }

    #[test]
    fn only_a_wayland_window_has_pointers() {
        let d = NonNull::<u8>::dangling().cast();
        let s = NonNull::<u8>::dangling().cast();
        let wl_d = RawDisplayHandle::Wayland(WaylandDisplayHandle::new(d));
        let wl_w = RawWindowHandle::Wayland(WaylandWindowHandle::new(s));
        let got = wayland_pointers(wl_d, wl_w).unwrap();
        assert_eq!(got.0, d.as_ptr().cast());
        assert_eq!(got.1, s.as_ptr().cast());

        let win_d = RawDisplayHandle::Windows(WindowsDisplayHandle::new());
        let win_w = RawWindowHandle::Win32(Win32WindowHandle::new(
            std::num::NonZeroIsize::new(1).unwrap(),
        ));
        let x_d = RawDisplayHandle::Xlib(XlibDisplayHandle::new(None, 0));
        let x_w = RawWindowHandle::Xlib(XlibWindowHandle::new(1));
        assert!(wayland_pointers(win_d, win_w).is_none());
        assert!(wayland_pointers(x_d, x_w).is_none());
        // Mixed handles never happen, but must not be taken for Wayland.
        assert!(wayland_pointers(wl_d, x_w).is_none());
        assert!(wayland_pointers(x_d, wl_w).is_none());
    }
}
