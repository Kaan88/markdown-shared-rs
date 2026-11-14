//! Putting a window where the other application asked: the viewer and the editor side by side.
//! Rectangles are in physical screen pixels, as winit reports them.
//!
//! On Windows and X11 the window is moved with winit. A Wayland client cannot position itself, so
//! on KDE the window is moved by a small KWin script, loaded through KWin's scripting interface
//! on the session bus; elsewhere on Wayland [`set`] returns false and the caller can say so.

use mdshared_app::sister::Rect;
use winit::window::Window;

/// `area` cut into two halves side by side: left, then right. An odd width gives the extra pixel
/// to the right.
pub fn halves(area: Rect) -> (Rect, Rect) {
    let left = area.w / 2;
    (
        Rect { w: left, ..area },
        Rect {
            x: area.x + left as i32,
            w: area.w - left,
            ..area
        },
    )
}

/// The area of the monitor the window is on that it may fill: on Windows without the taskbar. Under
/// X11 and Wayland the whole monitor: a panel is not subtracted here, and [`set`] keeps a window
/// out from under KDE's panels itself.
pub fn work_area(window: &Window) -> Option<Rect> {
    let monitor = window.current_monitor()?;
    let (at, size) = (monitor.position(), monitor.size());
    let whole = Rect {
        x: at.x,
        y: at.y,
        w: size.width,
        h: size.height,
    };
    Some(work_of_monitor(window).unwrap_or(whole))
}

#[cfg(windows)]
fn work_of_monitor(window: &Window) -> Option<Rect> {
    use windows_sys::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
    };
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let RawWindowHandle::Win32(handle) = window.window_handle().ok()?.as_raw() else {
        return None;
    };
    let mut info: MONITORINFO = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
    // SAFETY: `info` is a live MONITORINFO with its size set; the handle is this window's.
    let ok = unsafe {
        let monitor = MonitorFromWindow(handle.hwnd.get() as _, MONITOR_DEFAULTTONEAREST);
        GetMonitorInfoW(monitor, &mut info)
    };
    let work = info.rcWork;
    (ok != 0 && work.right > work.left && work.bottom > work.top).then(|| Rect {
        x: work.left,
        y: work.top,
        w: (work.right - work.left) as u32,
        h: (work.bottom - work.top) as u32,
    })
}

#[cfg(not(windows))]
fn work_of_monitor(_window: &Window) -> Option<Rect> {
    None
}

/// Moves and resizes `window` to `rect`, un-maximising it first. True when the window was moved or
/// a move was asked for: under KDE on Wayland the move is made by KWin on a helper thread a moment
/// later, and a failure there is silent. False where nothing can be done (Wayland on another
/// desktop), so the caller can show a hint.
pub fn set(window: &Window, rect: Rect) -> bool {
    #[cfg(not(windows))]
    if is_wayland(window) {
        return kde_session() && kwin::place(window, rect);
    }
    use winit::dpi::{PhysicalPosition, PhysicalSize};
    window.set_maximized(false);
    // The rectangle is the outer one; the size asked for is the inner one.
    let (outer, inner) = (window.outer_size(), window.inner_size());
    let frame_w = outer.width.saturating_sub(inner.width);
    let frame_h = outer.height.saturating_sub(inner.height);
    window.set_outer_position(PhysicalPosition::new(rect.x, rect.y));
    // The size is not asked for again if the window is already that size, which would flash a
    // resize on some systems; the result is ignored, as the window may take another size.
    let _ = window.request_inner_size(PhysicalSize::new(
        rect.w.saturating_sub(frame_w).max(1),
        rect.h.saturating_sub(frame_h).max(1),
    ));
    true
}

#[cfg(not(windows))]
fn is_wayland(window: &Window) -> bool {
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    window
        .window_handle()
        .is_ok_and(|h| matches!(h.as_raw(), RawWindowHandle::Wayland(_)))
}

/// `$XDG_CURRENT_DESKTOP` lists KDE.
#[cfg(not(windows))]
fn kde_session() -> bool {
    std::env::var("XDG_CURRENT_DESKTOP").is_ok_and(|d| lists_kde(&d))
}

/// `desktops` is a colon-separated `$XDG_CURRENT_DESKTOP`.
#[cfg(not(windows))]
fn lists_kde(desktops: &str) -> bool {
    desktops.split(':').any(|d| d.eq_ignore_ascii_case("KDE"))
}

/// The KWin script that places the window of process `pid` at `rect`, in KWin's logical pixels,
/// kept inside the area panels leave free.
#[cfg(not(windows))]
fn kwin_script(pid: u32, rect: Rect) -> String {
    let Rect { x, y, w, h } = rect;
    format!(
        "var list = workspace.windowList ? workspace.windowList() : workspace.clientList();\n\
         for (var i = 0; i < list.length; i++) {{\n\
         \tvar w = list[i];\n\
         \tif (w.pid !== {pid}) continue;\n\
         \tvar a = workspace.clientArea(KWin.PlacementArea, w);\n\
         \tvar left = Math.max({x}, a.x), top = Math.max({y}, a.y);\n\
         \tvar right = Math.min({x} + {w}, a.x + a.width), bottom = Math.min({y} + {h}, a.y + a.height);\n\
         \tif (right <= left || bottom <= top) {{ left = {x}; top = {y}; right = {x} + {w}; bottom = {y} + {h}; }}\n\
         \tw.setMaximize(false, false);\n\
         \tw.frameGeometry = {{x: left, y: top, width: right - left, height: bottom - top}};\n\
         }}\n"
    )
}

#[cfg(not(windows))]
mod kwin {
    use super::{Rect, Window, kwin_script};

    /// Asks KWin, on a helper thread, to place the window. KWin works in logical pixels.
    pub fn place(window: &Window, rect: Rect) -> bool {
        let scale = window.scale_factor().max(0.1);
        let logical = |v: f64| (v / scale).round() as i32;
        let rect = Rect {
            x: logical(rect.x as f64),
            y: logical(rect.y as f64),
            w: logical(rect.w as f64).max(1) as u32,
            h: logical(rect.h as f64).max(1) as u32,
        };
        let pid = std::process::id();
        std::thread::Builder::new()
            .name("mdshared-place".into())
            .spawn(move || _ = run(pid, &kwin_script(pid, rect)))
            .is_ok()
    }

    /// Writes `script` to the runtime folder, has KWin load and run it, and removes it again.
    fn run(pid: u32, script: &str) -> Option<()> {
        let runtime = std::env::var_os("XDG_RUNTIME_DIR").map(std::path::PathBuf::from)?;
        let file = runtime.join(format!("mdshared-place-{pid}.js"));
        std::fs::write(&file, script).ok()?;
        let done = call(pid, &file);
        _ = std::fs::remove_file(&file);
        done
    }

    fn call(pid: u32, file: &std::path::Path) -> Option<()> {
        let bus = zbus::blocking::Connection::session().ok()?;
        let name = format!("mdshared-place-{pid}");
        let scripting = |method: &str, body: &(&str, &str)| {
            bus.call_method(
                Some("org.kde.KWin"),
                "/Scripting",
                Some("org.kde.kwin.Scripting"),
                method,
                body,
            )
        };
        let id: i32 = scripting("loadScript", &(file.to_str()?, &name))
            .ok()?
            .body()
            .deserialize()
            .ok()?;
        // The object of a loaded script is `/Scripting/Script<id>` in Plasma 6 and `/<id>` in 5.
        let run = |path: String| {
            bus.call_method(
                Some("org.kde.KWin"),
                path,
                Some("org.kde.kwin.Script"),
                "run",
                &(),
            )
        };
        let ran = run(format!("/Scripting/Script{id}")).or_else(|_| run(format!("/{id}")));
        _ = bus.call_method(
            Some("org.kde.KWin"),
            "/Scripting",
            Some("org.kde.kwin.Scripting"),
            "unloadScript",
            &(&name,),
        );
        ran.ok().map(drop)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn halves_tile_the_area() {
        let area = Rect {
            x: -1920,
            y: 24,
            w: 1921,
            h: 1000,
        };
        let (left, right) = halves(area);
        assert_eq!(
            left,
            Rect {
                x: -1920,
                y: 24,
                w: 960,
                h: 1000
            }
        );
        assert_eq!(
            right,
            Rect {
                x: -960,
                y: 24,
                w: 961,
                h: 1000
            }
        );
        assert_eq!(left.x + left.w as i32, right.x);
        assert_eq!(left.w + right.w, area.w);
    }

    #[cfg(not(windows))]
    #[test]
    fn the_kwin_script_names_the_process_and_the_rectangle() {
        let script = kwin_script(
            4242,
            Rect {
                x: 10,
                y: -20,
                w: 800,
                h: 600,
            },
        );
        assert!(script.contains("w.pid !== 4242"), "{script}");
        assert!(script.contains("Math.max(10, a.x)") && script.contains("Math.max(-20, a.y)"));
        assert!(script.contains("Math.min(10 + 800, a.x + a.width)"));
        assert!(script.contains("w.setMaximize(false, false)"));
        assert!(script.contains("w.frameGeometry ="));
        assert!(script.contains("KWin.PlacementArea"));
        assert_eq!(script.matches('{').count(), script.matches('}').count());
    }

    #[cfg(not(windows))]
    #[test]
    fn only_a_kde_desktop_is_taken_for_kde() {
        assert!(lists_kde("KDE") && lists_kde("ubuntu:kde"));
        assert!(!lists_kde("GNOME") && !lists_kde("X-KDE-Something") && !lists_kde(""));
    }
}
