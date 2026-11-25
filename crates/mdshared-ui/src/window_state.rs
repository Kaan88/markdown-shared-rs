//! Remembering where a window was: its restored rectangle, whether it was maximized, the monitor
//! it was on and the scale it was saved at, as one value of the settings file, and putting it back
//! there at the next start.
//!
//! The rectangle is in logical pixels measured from the top-left corner of the monitor the window
//! was on, so a window of 1200 logical px is the same apparent size on a 1x screen and on a 2x one.
//! A monitor is named as winit names it, together with the physical position and size it had, so a
//! monitor that was unplugged and another one with the same name put somewhere else are told
//! apart. [`restore`] is pure: it takes the saved state and the monitors that exist now, and says
//! which monitor and which physical rectangle on the screen to use, if any. The application owns
//! the state; it writes [`WindowState::to_setting`] under a key of its own and passes the value
//! back to [`WindowState::from_setting`], and it owns applying the answer (with
//! [`place::set`](crate::place::set), then maximizing when [`Placement::maximized`] says to).
//!
//! [`capture`] and [`monitors`] are the only functions that talk to winit. They cannot be tested
//! without a window and are kept thin. A minimized window is not captured, since the desktop does
//! not say where it will return to; a fullscreen window is captured as the rectangle it covered.
//!
//! Deliberately not here: the settings file itself (the key belongs to the caller), a minimized
//! state to go back to, a fullscreen flag, a default rectangle for a first start, and any
//! arithmetic over the whole desktop (the caller provides the monitors, primary first).

use mdshared_app::settings;
use mdshared_app::sister::Rect as ScreenRect;
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::monitor::MonitorHandle;
use winit::window::Window;

use crate::gfx::Rect;
use crate::place;

/// The largest logical window edge a saved value may have, in logical px.
const MAX_EDGE: f32 = 100_000.0;
/// The furthest a saved logical coordinate may be from its monitor's corner.
const MAX_POS: f32 = 1_000_000.0;
/// The largest physical monitor edge that is believed, in physical px.
const MAX_MONITOR: u32 = 1_000_000;
/// The scale factors that are believed: outside this a value is taken for nonsense.
const MIN_SCALE: f32 = 0.05;
const MAX_SCALE: f32 = 16.0;
/// The longest settings value and monitor name that are read.
const MAX_LINE: usize = 8192;
const MAX_NAME: usize = 512;
/// The smallest window edge that is placed, in logical px.
const MIN_EDGE: f32 = 1.0;

/// Which monitor a saved window belonged to: the name winit gave it and the physical position and
/// size it had. The position and size are needed because a monitor's name may be reused by
/// another monitor, or the same one may be moved in the desktop's arrangement.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MonitorId {
    /// The monitor's name as winit reports it, where the system gives one.
    pub name: Option<String>,
    /// Its top-left corner in physical screen px.
    pub position: (i32, i32),
    /// Its size in physical px.
    pub size: (u32, u32),
}

impl MonitorId {
    /// `name` at physical `position`, of physical `size`.
    pub fn new(name: Option<String>, position: (i32, i32), size: (u32, u32)) -> MonitorId {
        MonitorId {
            name,
            position,
            size,
        }
    }
}

/// A window's saved place: its restored (not maximized) outer rectangle in logical px measured
/// from its monitor's top-left corner, whether it was maximized, the scale of that monitor when
/// the state was written, and the monitor itself.
///
/// The rectangle is logical, so [`restore`] places it at whatever scale the monitor has now. The
/// saved scale is kept for callers that want to describe or migrate the state; placing the window
/// does not need it.
#[derive(Clone, Debug, PartialEq)]
pub struct WindowState {
    /// The outer rectangle when the window was not maximized, in logical px from the monitor's
    /// top-left corner. A window the user dragged partly off its monitor may be negative or
    /// larger than the monitor; [`restore`] brings it back on screen.
    pub rect: Rect,
    /// Whether the window was maximized.
    pub maximized: bool,
    /// The scale factor of the monitor when the state was written.
    pub scale: f32,
    /// The monitor the window was on.
    pub monitor: MonitorId,
}

impl WindowState {
    /// The state as one settings value, to be written with the caller's key. The monitor's name is
    /// escaped, so the whole value stays on one line and keeps any character a name may hold.
    ///
    /// The shape is a list of `key=value` fields separated by `;`:
    /// `x`, `y`, `w` and `h` the rectangle; `max` 0 or 1; `scale` the scale factor; `mon` the
    /// monitor's name (empty when it has none); `monx` and `mony` its position; `monw` and `monh`
    /// its size. Positions and sizes are written to two decimals and the scale to four, so a
    /// rectangle is kept to a hundredth of a logical pixel. A caller that reads the value back
    /// should use [`from_setting`](Self::from_setting) rather than parsing it.
    pub fn to_setting(&self) -> String {
        format!(
            "x={:.2};y={:.2};w={:.2};h={:.2};max={};scale={:.4};mon={};\
             monx={};mony={};monw={};monh={}",
            self.rect.x,
            self.rect.y,
            self.rect.w,
            self.rect.h,
            u8::from(self.maximized),
            self.scale,
            escape_name(self.monitor.name.as_deref().unwrap_or("")),
            self.monitor.position.0,
            self.monitor.position.1,
            self.monitor.size.0,
            self.monitor.size.1,
        )
    }

    /// The state a value of [`to_setting`](Self::to_setting) wrote, or `None` when the value is
    /// partial, damaged or not one this module wrote.
    ///
    /// Every field must be there and must be a finite number; the rectangle must have a positive
    /// width and height; the scale factor must be one a monitor could have; the monitor must have
    /// a size. Fields that are not known are ignored, so a value from a later version that adds
    /// one still reads, and a field that appears twice keeps its last value. An empty or
    /// whitespace-only piece between separators is skipped, so a trailing `;` is harmless.
    pub fn from_setting(value: &str) -> Option<WindowState> {
        if value.len() > MAX_LINE {
            return None;
        }
        let (mut x, mut y, mut w, mut h) = (None, None, None, None);
        let (mut maximized, mut scale) = (None, None);
        let (mut name, mut monx, mut mony, mut monw, mut monh) = (None, None, None, None, None);
        for part in value.split(';') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            let (key, field) = part.split_once('=')?;
            let field = field.trim();
            match key.trim() {
                "x" => x = Some(number(field)?),
                "y" => y = Some(number(field)?),
                "w" => w = Some(number(field)?),
                "h" => h = Some(number(field)?),
                "max" => {
                    maximized = Some(match field {
                        "0" => false,
                        "1" => true,
                        _ => return None,
                    })
                }
                "scale" => scale = Some(number(field)?),
                "mon" => name = Some(settings::unescape(field)),
                "monx" => monx = Some(field.parse::<i32>().ok()?),
                "mony" => mony = Some(field.parse::<i32>().ok()?),
                "monw" => monw = Some(field.parse::<u32>().ok()?),
                "monh" => monh = Some(field.parse::<u32>().ok()?),
                _ => {}
            }
        }
        let rect = Rect::new(x?, y?, w?, h?);
        let maximized = maximized?;
        let scale = scale?;
        let name = name?;
        let position = (monx?, mony?);
        let size = (monw?, monh?);
        let edge_ok = rect.w > 0.0
            && rect.w <= MAX_EDGE
            && rect.h > 0.0
            && rect.h <= MAX_EDGE
            && rect.x.abs() <= MAX_POS
            && rect.y.abs() <= MAX_POS
            && rect.x.is_finite()
            && rect.y.is_finite()
            && rect.w.is_finite()
            && rect.h.is_finite();
        let scale_ok = (MIN_SCALE..=MAX_SCALE).contains(&scale);
        let monitor_ok = size.0 > 0 && size.1 > 0 && size.0 <= MAX_MONITOR && size.1 <= MAX_MONITOR;
        let name_ok = name.chars().count() <= MAX_NAME;
        (edge_ok && scale_ok && monitor_ok && name_ok).then(|| WindowState {
            rect,
            maximized,
            scale,
            monitor: MonitorId::new((!name.is_empty()).then_some(name), position, size),
        })
    }
}

/// A monitor that exists now, as the pure part of this module needs it: its name and physical
/// geometry to recognise it, its scale factor, and the area of it a window may fill in logical px
/// from its top-left corner.
#[derive(Clone, Debug, PartialEq)]
pub struct Monitor {
    /// The name as winit reports it, where the system gives one.
    pub name: Option<String>,
    /// Its top-left corner in physical screen px.
    pub position: (i32, i32),
    /// Its size in physical px.
    pub size: (u32, u32),
    /// Its scale factor: physical pixels per logical pixel.
    pub scale: f32,
    /// The area a window may fill, in logical px from this monitor's top-left corner: the whole
    /// monitor less a taskbar or a Dock where the system says so. A nonsense area is ignored by
    /// [`restore`], which falls back to the monitor's own rectangle.
    pub work: Rect,
}

impl Monitor {
    /// The identity of this monitor: its name, position and size. A saved state that has the same
    /// identity is on this monitor; one whose name matches but whose position or size does not is
    /// not.
    pub fn id(&self) -> MonitorId {
        MonitorId::new(self.name.clone(), self.position, self.size)
    }
}

/// Where a saved window is to be put: the index of the monitor in the slice [`restore`] was given,
/// the outer rectangle in physical screen pixels (ready for [`place::set`](crate::place::set)),
/// and whether it is to be maximized afterwards.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Placement {
    /// The index of the monitor in the slice given to [`restore`], 0 being the primary when the
    /// caller lists it first.
    pub monitor: usize,
    /// The outer rectangle in physical screen px.
    pub rect: ScreenRect,
    /// Whether the window was saved maximized: place it at `rect`, then maximize it.
    pub maximized: bool,
}

/// The monitor whose centre is nearest the saved monitor's centre (the first of equals), for a
/// window whose monitor is gone: it returns to the side of the desk it was on.
fn nearest(monitors: &[Monitor], saved: &MonitorId) -> usize {
    let centre = |pos: (i32, i32), size: (u32, u32)| {
        (
            f64::from(pos.0) + f64::from(size.0) / 2.0,
            f64::from(pos.1) + f64::from(size.1) / 2.0,
        )
    };
    let (sx, sy) = centre(saved.position, saved.size);
    let mut best = (0, f64::INFINITY);
    for (i, m) in monitors.iter().enumerate() {
        let (mx, my) = centre(m.position, m.size);
        let d = (mx - sx).powi(2) + (my - sy).powi(2);
        if d < best.1 {
            best = (i, d);
        }
    }
    best.0
}

/// Where to put a window saved as `state`, given the monitors that exist now, or `None` when there
/// are none.
///
/// The window goes to the monitor whose name, position and size match the saved one. When that
/// monitor is gone (unplugged, moved in the desktop's arrangement, or a different size), the
/// window goes to the monitor whose centre is nearest the saved one's, so it stays on the side
/// of the desk it was on (the first of equals: the caller lists the primary first). The saved logical rectangle is kept: it is turned into physical pixels with that
/// monitor's own scale factor, so a window is the same apparent size on a 1x and a 2x screen, and
/// a scale that changed since it was saved needs nothing else. The width and height are then
/// reduced to the monitor's work area when they are larger, and the rectangle is moved so that it
/// lies entirely inside the work area.
///
/// Nonsense is handled rather than refused: a non-finite or zero-size rectangle, a scale factor
/// that is not one and a work area that is not one are all replaced with usable values, so the
/// answer, when there is a monitor at all, is always a real rectangle of at least one pixel.
pub fn restore(state: &WindowState, monitors: &[Monitor]) -> Option<Placement> {
    let saved = &state.monitor;
    let chosen = monitors
        .iter()
        .position(|monitor| same_monitor(monitor, saved))
        .unwrap_or_else(|| nearest(monitors, saved));
    let monitor = monitors.get(chosen)?;
    let scale = sane_scale(monitor.scale);
    let work = sane_work(monitor, scale);
    let rect = sane_rect(&state.rect);

    let work_px = screen_work(monitor, work, scale);
    let w = physical(rect.w, scale).clamp(1, i64::from(work_px.w)) as u32;
    let h = physical(rect.h, scale).clamp(1, i64::from(work_px.h)) as u32;
    let x = i64::from(monitor.position.0) + physical(rect.x, scale);
    let y = i64::from(monitor.position.1) + physical(rect.y, scale);
    let max_x = i64::from(work_px.x) + i64::from(work_px.w) - i64::from(w);
    let max_y = i64::from(work_px.y) + i64::from(work_px.h) - i64::from(h);
    let x = x.clamp(i64::from(work_px.x), max_x);
    let y = y.clamp(i64::from(work_px.y), max_y);
    Some(Placement {
        monitor: chosen,
        rect: ScreenRect {
            x: to_i32(x),
            y: to_i32(y),
            w,
            h,
        },
        maximized: state.maximized,
    })
}

/// The window's state as it is now, or `None` when there is no window to measure: no monitor is
/// known, or the window is minimized (the desktop does not say where it will return to).
///
/// A maximized window saves the rectangle the system will restore it to where the system keeps one
/// (Windows); elsewhere, and for a fullscreen window, it saves the rectangle it covers now.
pub fn capture(window: &Window) -> Option<WindowState> {
    if window.is_minimized() == Some(true) {
        return None;
    }
    let monitor = window.current_monitor()?;
    let position = monitor.position();
    let size = monitor.size();
    let scale = sane_scale(monitor.scale_factor() as f32);
    let outer = if window.is_maximized() {
        place::normal_rect(window).or_else(|| place::outer(window))?
    } else {
        place::outer(window)?
    };
    let rect = local_of(outer, (position.x, position.y), scale);
    Some(WindowState {
        rect: Rect::new(rect.x, rect.y, rect.w.max(MIN_EDGE), rect.h.max(MIN_EDGE)),
        maximized: window.is_maximized(),
        scale,
        monitor: MonitorId::new(
            monitor.name(),
            (position.x, position.y),
            (size.width, size.height),
        ),
    })
}

/// The monitors that exist now, the primary one first where winit names one, as [`restore`] wants
/// them. Each monitor's work area is the whole monitor less a taskbar or a Dock where the system
/// gives that for every monitor (Windows), and less those on the current monitor elsewhere; a
/// monitor that is not the current one has its whole rectangle as the work area.
pub fn monitors(window: &Window) -> Vec<Monitor> {
    let primary = window.primary_monitor();
    let mut list = Vec::new();
    if let Some(handle) = &primary {
        list.push(monitor_of(window, handle));
    }
    for handle in window.available_monitors() {
        if primary.as_ref().is_some_and(|p| same_handle(p, &handle)) {
            continue;
        }
        list.push(monitor_of(window, &handle));
    }
    list
}

/// Whether `monitor` is the one `saved` names: the same name (an empty one and none being the
/// same), at the same physical position, of the same physical size.
fn same_monitor(monitor: &Monitor, saved: &MonitorId) -> bool {
    monitor.name.as_deref().unwrap_or("") == saved.name.as_deref().unwrap_or("")
        && monitor.position == saved.position
        && monitor.size == saved.size
}

/// The same monitor as winit hands it out twice.
fn same_handle(a: &MonitorHandle, b: &MonitorHandle) -> bool {
    a.position() == b.position() && a.size() == b.size()
}

/// `handle` as the pure part of this module wants it.
fn monitor_of(window: &Window, handle: &MonitorHandle) -> Monitor {
    let position = handle.position();
    let size = handle.size();
    let scale = sane_scale(handle.scale_factor() as f32);
    let work = work_of(window, handle, position, size, scale);
    Monitor {
        name: handle.name(),
        position: (position.x, position.y),
        size: (size.width, size.height),
        scale,
        work,
    }
}

/// The work area of `handle` in logical px from its top-left corner. Windows gives it for every
/// monitor; the other systems give it for the current monitor only, and the whole monitor is used
/// for the rest.
#[cfg(windows)]
fn work_of(
    _window: &Window,
    handle: &MonitorHandle,
    position: PhysicalPosition<i32>,
    size: PhysicalSize<u32>,
    scale: f32,
) -> Rect {
    use windows_sys::Win32::Graphics::Gdi::{GetMonitorInfoW, MONITORINFO};
    use winit::platform::windows::MonitorHandleExtWindows;
    let mut info: MONITORINFO = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
    // SAFETY: `info` is a live MONITORINFO with its size set; the handle is this monitor's.
    let ok = unsafe { GetMonitorInfoW(handle.hmonitor() as _, &mut info) };
    let work = info.rcWork;
    if ok != 0 && work.right > work.left && work.bottom > work.top {
        let rect = ScreenRect {
            x: work.left,
            y: work.top,
            w: (work.right - work.left) as u32,
            h: (work.bottom - work.top) as u32,
        };
        local_of(rect, (position.x, position.y), scale)
    } else {
        whole(size, scale)
    }
}

#[cfg(not(windows))]
fn work_of(
    window: &Window,
    handle: &MonitorHandle,
    position: PhysicalPosition<i32>,
    size: PhysicalSize<u32>,
    scale: f32,
) -> Rect {
    let current = window
        .current_monitor()
        .is_some_and(|c| same_handle(&c, handle));
    if current && let Some(work) = place::work_area(window) {
        return local_of(work, (position.x, position.y), scale);
    }
    whole(size, scale)
}

/// Every monitor's rectangle, in logical px from its own corner.
fn whole(size: PhysicalSize<u32>, scale: f32) -> Rect {
    Rect::new(
        0.0,
        0.0,
        (size.width as f32 / scale).max(MIN_EDGE),
        (size.height as f32 / scale).max(MIN_EDGE),
    )
}

/// `rect`, which is in physical screen px, in logical px from `at`.
fn local_of(rect: ScreenRect, at: (i32, i32), scale: f32) -> Rect {
    let dx = i64::from(rect.x) - i64::from(at.0);
    let dy = i64::from(rect.y) - i64::from(at.1);
    Rect::new(
        dx as f32 / scale,
        dy as f32 / scale,
        rect.w as f32 / scale,
        rect.h as f32 / scale,
    )
}

/// A scale factor the arithmetic can use: a system's 0 or NaN is taken for 1.
fn sane_scale(scale: f32) -> f32 {
    if scale.is_finite() && scale >= MIN_SCALE {
        scale.min(MAX_SCALE)
    } else {
        1.0
    }
}

/// The rectangle `v` logical px wide at `scale`, as whole physical px.
fn physical(v: f32, scale: f32) -> i64 {
    (f64::from(v) * f64::from(scale)).round() as i64
}

/// A saved rectangle with every field a finite number of a usable size.
fn sane_rect(rect: &Rect) -> Rect {
    let field = |v: f32, fallback: f32| if v.is_finite() { v } else { fallback };
    let span = |v: f32| field(v, MIN_EDGE).clamp(MIN_EDGE, MAX_EDGE);
    Rect::new(
        field(rect.x, 0.0).clamp(-MAX_POS, MAX_POS),
        field(rect.y, 0.0).clamp(-MAX_POS, MAX_POS),
        span(rect.w),
        span(rect.h),
    )
}

/// The monitor's work area, or its whole rectangle when the work area is not one.
fn sane_work(monitor: &Monitor, scale: f32) -> Rect {
    let work = &monitor.work;
    let usable = work.x.is_finite()
        && work.y.is_finite()
        && work.w.is_finite()
        && work.h.is_finite()
        && work.w >= MIN_EDGE
        && work.h >= MIN_EDGE;
    if usable {
        Rect::new(
            work.x.clamp(-MAX_POS, MAX_POS),
            work.y.clamp(-MAX_POS, MAX_POS),
            work.w.clamp(MIN_EDGE, MAX_EDGE),
            work.h.clamp(MIN_EDGE, MAX_EDGE),
        )
    } else {
        whole(PhysicalSize::new(monitor.size.0, monitor.size.1), scale)
    }
}

/// The work area in physical screen px, at least one pixel wide and tall.
fn screen_work(monitor: &Monitor, work: Rect, scale: f32) -> ScreenRect {
    ScreenRect {
        x: to_i32(i64::from(monitor.position.0) + physical(work.x, scale)),
        y: to_i32(i64::from(monitor.position.1) + physical(work.y, scale)),
        w: to_u32(physical(work.w, scale)),
        h: to_u32(physical(work.h, scale)),
    }
}

fn to_i32(v: i64) -> i32 {
    v.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

/// A physical edge: at least one pixel and no more than a monitor can have.
fn to_u32(v: i64) -> u32 {
    v.clamp(1, i64::from(MAX_MONITOR)) as u32
}

/// `text` parsed as a finite number.
fn number(text: &str) -> Option<f32> {
    let v = text.parse::<f32>().ok()?;
    v.is_finite().then_some(v)
}

/// A monitor name with the `;` and `=` that separate fields turned into escapes, so the settings
/// value stays one value. [`settings::escape`] has already dealt with `%` and line breaks.
fn escape_name(name: &str) -> String {
    settings::escape(name)
        .replace(';', "%3B")
        .replace('=', "%3D")
}

#[cfg(test)]
#[path = "window_state_tests.rs"]
mod tests;
