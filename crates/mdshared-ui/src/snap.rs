//! The Windows 11 snap-layouts flyout for a hand-drawn maximize button.
//!
//! Windows shows the flyout when the pointer rests on what the window answers to `WM_NCHITTEST`
//! as `HTMAXBUTTON`. The windows here are undecorated and draw their own caption buttons, so by
//! default the whole client area answers `HTCLIENT` and there is no flyout. An application that
//! wants it calls [`set_maximize_button_rect`] with where its maximize button is drawn.
//!
//! What it does, on Windows only, and only for a window that was given a rectangle:
//!
//! - A `WM_NCHITTEST` that the window answered `HTCLIENT` inside the rectangle is answered
//!   `HTMAXBUTTON`. Nothing else about hit-testing changes: the edges and corners the
//!   application resizes by (`drag_resize_window`), the title row it drags by (`drag_window`)
//!   and every point outside the rectangle are answered exactly as before.
//! - Windows then delivers the mouse for that rectangle as non-client messages, which winit
//!   ignores. They are translated back, so the application's own button keeps working: the move
//!   (`WM_NCMOUSEMOVE`) is also sent on as a `WM_MOUSEMOVE` at the client position, the press and
//!   release (`WM_NCLBUTTONDOWN`, `WM_NCLBUTTONDBLCLK`, `WM_NCLBUTTONUP`) as `WM_LBUTTONDOWN` and
//!   `WM_LBUTTONUP`, and the pointer leaving (`WM_NCMOUSELEAVE`) as `WM_MOUSELEAVE`. The
//!   application sees ordinary `CursorMoved`, `CursorEntered`, `CursorLeft` and `MouseInput`
//!   events and maximizes with `set_maximized` as it does today. The default handling of the
//!   non-client press is not run: it would maximize a second time.
//! - Windows sends a `WM_MOUSELEAVE` as the pointer passes from the client area into the
//!   rectangle, because the rectangle is no longer client. That one is held back while the pointer
//!   is on the button (winit would report `CursorLeft` and clear the button's hover on every
//!   move), and the pointer coming back into the client area is announced as a `CursorLeft`
//!   followed by the move that winit turns into `CursorEntered`, which makes winit ask Windows for
//!   the next leave again.
//!
//! The rectangle is in logical pixels from the top-left of the client area, as the application
//! lays out its title row. Give `None` when the button is not drawn or something covers it (a
//! menu, a dialog). Call it again after a resize, a maximize or a change of scale: it stores the
//! rectangle and costs a lookup. Leave the resize band out of it: over the rectangle the pointer
//! is non-client, so the resize cursor the application sets there would not show.
//!
//! Off Windows the function does nothing.
//!
//! A restored window is also given back the 1 px strip at the top that winit keeps out of the
//! client area of an undecorated window for its shadow (`WM_NCCALCSIZE`): with it Windows never
//! shows the flyout for a restored window, only for a maximized one.
//!
//! Not verified here: the flyout itself, which needs a real pointer on Windows 11. The message
//! handling is exercised by the tests with a window of their own and `SendMessage`.

use crate::gfx::Rect;

/// Where the maximize button is, or `None` to stop answering for it. See the module's text.
///
/// Call it on the thread that owns the window (the event loop's); from another thread it does
/// nothing. A window that is destroyed takes its subclass with it.
#[cfg(windows)]
pub fn set_maximize_button_rect(window: &winit::window::Window, rect: Option<Rect>) {
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let Ok(handle) = window.window_handle() else {
        return;
    };
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return;
    };
    let device = rect.and_then(|r| device_rect(r, window.scale_factor()));
    imp::set(handle.hwnd.get(), device);
}

/// Does nothing off Windows.
#[cfg(not(windows))]
pub fn set_maximize_button_rect(_window: &winit::window::Window, _rect: Option<Rect>) {}

#[cfg(any(windows, test))]
/// `rect` in device pixels, as `[left, top, right, bottom]`, rounded outwards; `None` for a
/// rectangle with no area or a scale that is not a positive number.
fn device_rect(rect: Rect, scale: f64) -> Option<[i32; 4]> {
    if !(scale.is_finite() && scale > 0.0) {
        return None;
    }
    let edge = |v: f32, up: bool| {
        let v = f64::from(v) * scale;
        if !v.is_finite() {
            return None;
        }
        let v = if up { v.ceil() } else { v.floor() };
        (v.abs() < 1e7).then_some(v as i32)
    };
    let out = [
        edge(rect.x, false)?,
        edge(rect.y, false)?,
        edge(rect.x + rect.w, true)?,
        edge(rect.y + rect.h, true)?,
    ];
    (out[2] > out[0] && out[3] > out[1]).then_some(out)
}

#[cfg(any(windows, test))]
/// Whether the device point is in the rectangle (left and top edges inside, right and bottom
/// outside, as `PtInRect`).
fn contains(rect: [i32; 4], x: i32, y: i32) -> bool {
    x >= rect[0] && x < rect[2] && y >= rect[1] && y < rect[3]
}

#[cfg(any(windows, test))]
/// The signed x and y in the low and high words of a mouse message's `lparam` (screen coordinates
/// for the non-client messages, which can be negative on a monitor left of or above the primary).
fn point_of(lparam: isize) -> (i32, i32) {
    let l = lparam as u32;
    (
        i32::from(l as u16 as i16),
        i32::from((l >> 16) as u16 as i16),
    )
}

#[cfg(any(windows, test))]
/// An `lparam` for a client mouse message at `x`, `y`.
fn lparam_of(x: i32, y: i32) -> isize {
    ((u32::from(y as i16 as u16) << 16) | u32::from(x as i16 as u16)) as isize
}

#[cfg(windows)]
#[cfg(windows)]
mod imp {
    use super::{contains, lparam_of, point_of};
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
    use windows_sys::Win32::Graphics::Gdi::ScreenToClient;
    use windows_sys::Win32::System::Threading::GetCurrentThreadId;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        GetCapture, TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CallWindowProcW, DefWindowProcW, GWLP_WNDPROC, GetWindowLongPtrW, GetWindowThreadProcessId,
        HTCLIENT, HTMAXBUTTON, IsZoomed, NCCALCSIZE_PARAMS, SWP_FRAMECHANGED, SWP_NOACTIVATE,
        SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SetWindowLongPtrW, SetWindowPos, WM_LBUTTONDOWN,
        WM_LBUTTONUP, WM_MOUSEMOVE, WM_NCCALCSIZE, WM_NCDESTROY, WM_NCHITTEST, WM_NCLBUTTONDBLCLK,
        WM_NCLBUTTONDOWN, WM_NCLBUTTONUP, WM_NCMOUSELEAVE, WM_NCMOUSEMOVE, WNDPROC,
    };

    /// `WM_MOUSELEAVE`, which windows-sys keeps behind another feature.
    const WM_MOUSELEAVE: u32 = 0x02A3;
    /// `MK_LBUTTON`.
    const MK_LBUTTON: WPARAM = 1;
    /// `TME_NONCLIENT`.
    const TME_NONCLIENT: u32 = 0x10;

    /// What is kept for a window whose procedure was replaced by [`subclass_proc`].
    struct Entry {
        hwnd: isize,
        /// The procedure that was the window procedure (winit's), as `GetWindowLongPtrW` gave it.
        prev: isize,
        /// The maximize button in device pixels from the client origin, if one is set.
        rect: Cell<Option<[i32; 4]>>,
        /// The pointer is on the button: `WM_NCMOUSEMOVE` has said so and nothing has said it
        /// left. While it is, winit has been told the pointer is in the window.
        over: Cell<bool>,
    }

    thread_local! {
        /// The windows of this thread that have been given a rectangle. A window handles its
        /// messages on its own thread, so a thread-local is all the sharing there is.
        static ENTRIES: RefCell<Vec<Rc<Entry>>> = const { RefCell::new(Vec::new()) };
    }

    fn entry(hwnd: isize) -> Option<Rc<Entry>> {
        ENTRIES.with(|e| e.borrow().iter().find(|x| x.hwnd == hwnd).cloned())
    }

    /// Stores `rect` for the window `hwnd`, replacing the window procedure the first time a
    /// rectangle is given.
    ///
    /// The procedure is replaced with `SetWindowLongPtrW` and chained with `CallWindowProcW`,
    /// not with `SetWindowSubclass`: that one is in comctl32 version 6, and a program without
    /// that version's manifest would not even start with it imported.
    pub(super) fn set(hwnd: isize, rect: Option<[i32; 4]>) {
        let window = hwnd as HWND;
        // SAFETY: `hwnd` is the handle winit reports for a live window of this process; the two
        // calls take no pointers (a null process id is allowed), and a stale handle only makes
        // the first return 0.
        let same_thread = unsafe {
            GetWindowThreadProcessId(window, std::ptr::null_mut()) == GetCurrentThreadId()
        };
        if !same_thread {
            return;
        }
        let entry = match entry(hwnd) {
            Some(entry) => entry,
            None if rect.is_some() => {
                // SAFETY: reads the window procedure; the handle was checked above.
                let prev = unsafe { GetWindowLongPtrW(window, GWLP_WNDPROC) };
                if prev == 0 {
                    return;
                }
                let entry = Rc::new(Entry {
                    hwnd,
                    prev,
                    rect: Cell::new(None),
                    over: Cell::new(false),
                });
                // The entry is in place before the procedure that looks for it.
                ENTRIES.with(|e| e.borrow_mut().push(entry.clone()));
                // SAFETY: `subclass_proc` is an `extern "system"` function with a window
                // procedure signature that calls `prev` for everything it does not handle; the
                // window belongs to this thread.
                unsafe {
                    SetWindowLongPtrW(
                        window,
                        GWLP_WNDPROC,
                        subclass_proc as *const () as usize as isize,
                    )
                };
                // The client area is worked out again with the procedure in place.
                // SAFETY: the window is live and on this thread.
                unsafe {
                    SetWindowPos(
                        window,
                        std::ptr::null_mut(),
                        0,
                        0,
                        0,
                        0,
                        SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                    )
                };
                entry
            }
            None => return,
        };
        entry.rect.set(rect);
        if rect.is_none() {
            // SAFETY: the window is live.
            unsafe { leave(window, &entry) };
        }
    }

    /// Calls the window procedure that `subclass_proc` replaced.
    ///
    /// # Safety
    /// `entry` must be the entry of `hwnd`, a live window of this thread.
    unsafe fn call(entry: &Entry, hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        // SAFETY: `prev` came from `GetWindowLongPtrW(GWLP_WNDPROC)` of this window, which is a
        // window procedure or a handle `CallWindowProcW` knows how to call; `WNDPROC` is an
        // optional function pointer, the same size as that value.
        unsafe {
            CallWindowProcW(
                std::mem::transmute::<isize, WNDPROC>(entry.prev),
                hwnd,
                msg,
                wparam,
                lparam,
            )
        }
    }

    /// The pointer is no longer on the button: tells the window procedure (winit) that it left,
    /// if the pointer was reported on the button.
    ///
    /// # Safety
    /// As [`call`].
    unsafe fn leave(hwnd: HWND, entry: &Entry) {
        if entry.over.replace(false) {
            // SAFETY: the caller's promise; the message carries no pointer.
            unsafe { call(entry, hwnd, WM_MOUSELEAVE, 0, 0) };
        }
    }

    /// `lparam` (screen coordinates, from a non-client message) as the client coordinates of the
    /// same point, for a client message.
    ///
    /// # Safety
    /// `hwnd` must be a live window.
    unsafe fn client_lparam(hwnd: HWND, lparam: LPARAM) -> Option<LPARAM> {
        let (x, y) = point_of(lparam);
        let mut pt = POINT { x, y };
        // SAFETY: `pt` is a local valid for the call.
        (unsafe { ScreenToClient(hwnd, &mut pt) } != 0).then(|| lparam_of(pt.x, pt.y))
    }

    /// The window procedure that stands in front of the window's own.
    ///
    /// # Safety
    /// Called by Windows as the window procedure of a window of this thread.
    unsafe extern "system" fn subclass_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        let Some(state) = entry(hwnd as isize) else {
            // Cannot happen: the entry is made before the procedure is set, and removed only
            // after the last message of the window.
            // SAFETY: a window procedure default for the message received.
            return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
        };
        let state = &*state;
        let armed = state.rect.get();
        let on_button = wparam as u32 == HTMAXBUTTON;
        // SAFETY (for the calls below): `hwnd` is the window this procedure is handling a message
        // for, so it is live and on this thread; `state` is its entry; the messages passed on are
        // the ones received or ones without pointers.
        unsafe {
            match msg {
                WM_NCHITTEST => {
                    let result = call(state, hwnd, msg, wparam, lparam);
                    if let Some(rect) = armed.filter(|_| result == HTCLIENT as LRESULT) {
                        let (x, y) = point_of(lparam);
                        let mut pt = POINT { x, y };
                        if ScreenToClient(hwnd, &mut pt) != 0 && contains(rect, pt.x, pt.y) {
                            return HTMAXBUTTON as LRESULT;
                        }
                    }
                    result
                }
                WM_NCMOUSEMOVE if armed.is_some() && on_button => {
                    // The default handling is what Windows watches for the flyout.
                    let result = call(state, hwnd, msg, wparam, lparam);
                    if !state.over.replace(true) {
                        // Ask for the leave of the non-client area too: it is how the pointer
                        // leaving the button towards the outside of the window is seen.
                        let mut track = TRACKMOUSEEVENT {
                            cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                            dwFlags: TME_LEAVE | TME_NONCLIENT,
                            hwndTrack: hwnd,
                            dwHoverTime: 0,
                        };
                        TrackMouseEvent(&mut track);
                    }
                    if let Some(client) = client_lparam(hwnd, lparam) {
                        call(state, hwnd, WM_MOUSEMOVE, 0, client);
                    }
                    result
                }
                WM_NCLBUTTONDOWN | WM_NCLBUTTONDBLCLK if armed.is_some() && on_button => {
                    // The default handling would track the press and maximize on its own; the
                    // application does that.
                    if let Some(client) = client_lparam(hwnd, lparam) {
                        call(state, hwnd, WM_LBUTTONDOWN, MK_LBUTTON, client);
                    }
                    0
                }
                WM_NCLBUTTONUP if armed.is_some() && on_button => {
                    if let Some(client) = client_lparam(hwnd, lparam) {
                        call(state, hwnd, WM_LBUTTONUP, 0, client);
                    }
                    0
                }
                WM_NCMOUSELEAVE => {
                    leave(hwnd, state);
                    call(state, hwnd, msg, wparam, lparam)
                }
                // The leave Windows sends because the button is not client area.
                WM_MOUSELEAVE if state.over.get() => 0,
                WM_MOUSEMOVE if state.over.get() => {
                    // A press that began on the button holds the capture, and its moves stay
                    // client messages; only a move that is not part of one ends the visit.
                    if GetCapture() != hwnd {
                        leave(hwnd, state);
                    }
                    call(state, hwnd, msg, wparam, lparam)
                }
                WM_NCCALCSIZE if wparam != 0 => {
                    // winit keeps a 1 px non-client strip at the top of an undecorated window
                    // (the client area is moved down a pixel) so that Windows draws its shadow.
                    // With that strip a restored window never gets the snap-layouts flyout; the
                    // shadow does not need it. Give the pixel back to the client area.
                    let params = lparam as *mut NCCALCSIZE_PARAMS;
                    let before = (*params).rgrc[0];
                    let result = call(state, hwnd, msg, wparam, lparam);
                    let after = &mut (*params).rgrc[0];
                    if IsZoomed(hwnd) == 0
                        && after.left == before.left
                        && after.right == before.right
                        && after.top == before.top + 1
                        && after.bottom == before.bottom + 1
                    {
                        *after = before;
                    }
                    result
                }
                WM_NCDESTROY => {
                    let result = call(state, hwnd, msg, wparam, lparam);
                    ENTRIES.with(|e| e.borrow_mut().retain(|x| x.hwnd != hwnd as isize));
                    result
                }
                _ => call(state, hwnd, msg, wparam, lparam),
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::cell::RefCell;
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, DestroyWindow, HTCAPTION, RegisterClassW,
            SendMessageW, WNDCLASSW, WS_POPUP,
        };

        thread_local! {
            /// What the window's own procedure (the stand-in for winit's) was sent.
            static LOG: RefCell<Vec<(u32, usize, isize)>> = const { RefCell::new(Vec::new()) };
        }

        unsafe extern "system" fn inner(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
            if matches!(
                msg,
                WM_NCHITTEST
                    | WM_NCMOUSEMOVE
                    | WM_NCLBUTTONDOWN
                    | WM_NCLBUTTONDBLCLK
                    | WM_NCLBUTTONUP
                    | WM_NCMOUSELEAVE
                    | WM_MOUSEMOVE
                    | WM_LBUTTONDOWN
                    | WM_LBUTTONUP
                    | WM_MOUSELEAVE
            ) {
                LOG.with(|log| log.borrow_mut().push((msg, w, l)));
            }
            // The window procedure answers what the tests ask and never runs the default handling:
            // that of a press on a caption starts a modal move loop.
            if msg == WM_NCHITTEST {
                return HTCLIENT as LRESULT;
            }
            if msg == WM_NCCALCSIZE && w != 0 {
                // What winit does for an undecorated window with a shadow.
                // SAFETY: the test sends a valid NCCALCSIZE_PARAMS.
                let p = unsafe { &mut *(l as *mut NCCALCSIZE_PARAMS) };
                p.rgrc[0].top += 1;
                p.rgrc[0].bottom += 1;
                return 0;
            }
            if (WM_NCMOUSEMOVE..=WM_NCLBUTTONDBLCLK).contains(&msg) {
                return 0;
            }
            // SAFETY: the message is passed on as it came.
            unsafe { DefWindowProcW(hwnd, msg, w, l) }
        }

        fn log() -> Vec<(u32, usize, isize)> {
            LOG.with(|log| std::mem::take(&mut *log.borrow_mut()))
        }

        fn send(hwnd: HWND, msg: u32, w: usize, l: isize) -> isize {
            // SAFETY: a message without pointers to a window of this thread.
            unsafe { SendMessageW(hwnd, msg, w, l) }
        }

        /// A hidden window at (100, 100), 200 x 100, with a class of its own.
        fn window() -> HWND {
            let name: Vec<u16> = "mdshared_snap_test\0".encode_utf16().collect();
            let class = WNDCLASSW {
                style: 0,
                lpfnWndProc: Some(inner),
                cbClsExtra: 0,
                cbWndExtra: 0,
                hInstance: std::ptr::null_mut(),
                hIcon: std::ptr::null_mut(),
                hCursor: std::ptr::null_mut(),
                hbrBackground: std::ptr::null_mut(),
                lpszMenuName: std::ptr::null(),
                lpszClassName: name.as_ptr(),
            };
            // SAFETY: `class` and the name outlive the calls; registering twice (another test in
            // the same process) fails harmlessly and the class is already there.
            unsafe {
                RegisterClassW(&class);
                CreateWindowExW(
                    0,
                    name.as_ptr(),
                    name.as_ptr(),
                    WS_POPUP,
                    100,
                    100,
                    200,
                    100,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null(),
                )
            }
        }

        /// A screen position for client (x, y) of the test window.
        fn screen(x: i32, y: i32) -> isize {
            lparam_of(100 + x, 100 + y)
        }

        #[test]
        fn the_subclass_answers_the_button_and_translates_its_messages() {
            let hwnd = window();
            assert!(!hwnd.is_null(), "the test window was not created");
            let hit = |x, y| send(hwnd, WM_NCHITTEST, 0, screen(x, y));
            let button = [150, 0, 190, 30];
            let inside = (170, 10);
            let outside = (20, 10);

            // Without a rectangle the subclass is not there and nothing changes.
            set(hwnd as isize, None);
            assert_eq!(hit(inside.0, inside.1), HTCLIENT as isize);
            log();

            set(hwnd as isize, Some(button));
            assert_eq!(hit(inside.0, inside.1), HTMAXBUTTON as isize);
            assert_eq!(hit(149, 10), HTCLIENT as isize, "left of the rectangle");
            assert_eq!(hit(190, 10), HTCLIENT as isize, "the right edge is outside");
            assert_eq!(
                hit(170, 30),
                HTCLIENT as isize,
                "the bottom edge is outside"
            );
            assert_eq!(hit(outside.0, outside.1), HTCLIENT as isize);
            log();

            // Moving over the button: the original message goes on, and a client move follows.
            send(hwnd, WM_NCMOUSEMOVE, HTMAXBUTTON as usize, screen(170, 10));
            assert_eq!(
                log(),
                [
                    (WM_NCMOUSEMOVE, HTMAXBUTTON as usize, screen(170, 10)),
                    (WM_MOUSEMOVE, 0, lparam_of(170, 10)),
                ]
            );
            // Windows' own leave for the client area is held back while the pointer is on it.
            send(hwnd, WM_MOUSELEAVE, 0, 0);
            assert_eq!(log(), []);
            // The press and release come back as client messages, and the non-client ones are
            // not passed on.
            send(
                hwnd,
                WM_NCLBUTTONDOWN,
                HTMAXBUTTON as usize,
                screen(170, 10),
            );
            send(hwnd, WM_NCLBUTTONUP, HTMAXBUTTON as usize, screen(170, 10));
            send(
                hwnd,
                WM_NCLBUTTONDBLCLK,
                HTMAXBUTTON as usize,
                screen(171, 10),
            );
            assert_eq!(
                log(),
                [
                    (WM_LBUTTONDOWN, 1, lparam_of(170, 10)),
                    (WM_LBUTTONUP, 0, lparam_of(170, 10)),
                    (WM_LBUTTONDOWN, 1, lparam_of(171, 10)),
                ]
            );
            // The pointer leaving towards the outside: one client leave, and the original.
            send(hwnd, WM_NCMOUSELEAVE, 0, 0);
            assert_eq!(log(), [(WM_MOUSELEAVE, 0, 0), (WM_NCMOUSELEAVE, 0, 0)]);
            // Afterwards a leave is an ordinary one, and a second leave of the button is not
            // reported twice.
            send(hwnd, WM_MOUSELEAVE, 0, 0);
            send(hwnd, WM_NCMOUSELEAVE, 0, 0);
            assert_eq!(log(), [(WM_MOUSELEAVE, 0, 0), (WM_NCMOUSELEAVE, 0, 0)]);

            // The pointer coming from the button into the client area: the leave is reported
            // before the move, so winit sees it enter again.
            send(hwnd, WM_NCMOUSEMOVE, HTMAXBUTTON as usize, screen(170, 10));
            log();
            send(hwnd, WM_MOUSEMOVE, 0, lparam_of(20, 10));
            assert_eq!(
                log(),
                [(WM_MOUSELEAVE, 0, 0), (WM_MOUSEMOVE, 0, lparam_of(20, 10))]
            );
            send(hwnd, WM_MOUSEMOVE, 0, lparam_of(21, 10));
            assert_eq!(log(), [(WM_MOUSEMOVE, 0, lparam_of(21, 10))]);

            // Everything else goes through untouched: a press on the title row (drag), one on a
            // resize edge, a non-client move that is not on the button.
            send(hwnd, WM_NCLBUTTONDOWN, HTCAPTION as usize, screen(20, 10));
            send(hwnd, WM_NCMOUSEMOVE, HTCAPTION as usize, screen(20, 10));
            assert_eq!(
                log(),
                [
                    (WM_NCLBUTTONDOWN, HTCAPTION as usize, screen(20, 10)),
                    (WM_NCMOUSEMOVE, HTCAPTION as usize, screen(20, 10)),
                ]
            );

            // Taking the rectangle away stops the answers, and a visit in progress ends.
            send(hwnd, WM_NCMOUSEMOVE, HTMAXBUTTON as usize, screen(170, 10));
            log();
            set(hwnd as isize, None);
            assert_eq!(log(), [(WM_MOUSELEAVE, 0, 0)]);
            assert_eq!(hit(inside.0, inside.1), HTCLIENT as isize);
            send(
                hwnd,
                WM_NCLBUTTONDOWN,
                HTMAXBUTTON as usize,
                screen(170, 10),
            );
            assert_eq!(
                log()[1..],
                [(WM_NCLBUTTONDOWN, HTMAXBUTTON as usize, screen(170, 10))]
            );

            // The 1 px strip winit leaves at the top is given back to the client area.
            let mut params: NCCALCSIZE_PARAMS = unsafe { std::mem::zeroed() };
            params.rgrc[0] = windows_sys::Win32::Foundation::RECT {
                left: 10,
                top: 20,
                right: 210,
                bottom: 120,
            };
            let before = params.rgrc[0];
            send(
                hwnd,
                WM_NCCALCSIZE,
                1,
                &mut params as *mut NCCALCSIZE_PARAMS as isize,
            );
            assert_eq!(
                (
                    params.rgrc[0].left,
                    params.rgrc[0].top,
                    params.rgrc[0].right,
                    params.rgrc[0].bottom
                ),
                (before.left, before.top, before.right, before.bottom)
            );

            // A new rectangle is taken, and destroying the window removes the subclass (the state
            // is freed in `WM_NCDESTROY`; a run under a leak checker would show it).
            set(hwnd as isize, Some([0, 0, 10, 10]));
            assert_eq!(hit(5, 5), HTMAXBUTTON as isize);
            // SAFETY: the window belongs to this thread and is not used again.
            unsafe { DestroyWindow(hwnd) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_logical_rectangle_becomes_device_pixels_rounded_outwards() {
        let r = Rect::new(100.0, 0.0, 46.0, 32.0);
        assert_eq!(device_rect(r, 1.0), Some([100, 0, 146, 32]));
        assert_eq!(device_rect(r, 1.5), Some([150, 0, 219, 48]));
        assert_eq!(device_rect(r, 2.0), Some([200, 0, 292, 64]));
        // Fractions grow the rectangle to the pixels they touch.
        let r = Rect::new(10.3, 0.2, 10.0, 10.0);
        assert_eq!(device_rect(r, 1.0), Some([10, 0, 21, 11]));
    }

    #[test]
    fn nothing_is_answered_for_an_empty_or_unusable_rectangle() {
        let r = Rect::new(0.0, 0.0, 10.0, 10.0);
        assert_eq!(device_rect(Rect::new(5.0, 5.0, 0.0, 10.0), 1.0), None);
        assert_eq!(device_rect(Rect::new(5.0, 5.0, 10.0, -3.0), 1.0), None);
        assert_eq!(device_rect(Rect::new(f32::NAN, 0.0, 1.0, 1.0), 1.0), None);
        assert_eq!(
            device_rect(Rect::new(0.0, 0.0, f32::INFINITY, 1.0), 1.0),
            None
        );
        assert_eq!(device_rect(r, 0.0), None);
        assert_eq!(device_rect(r, -1.0), None);
        assert_eq!(device_rect(r, f64::NAN), None);
        assert_eq!(device_rect(r, f64::INFINITY), None);
        assert_eq!(device_rect(Rect::new(1e30, 0.0, 1.0, 1.0), 1.0), None);
    }

    #[test]
    fn the_rectangle_includes_its_top_left_and_excludes_its_bottom_right() {
        let r = [10, 5, 20, 15];
        assert!(contains(r, 10, 5));
        assert!(contains(r, 19, 14));
        assert!(!contains(r, 20, 14));
        assert!(!contains(r, 19, 15));
        assert!(!contains(r, 9, 10));
        assert!(!contains(r, 15, 4));
    }

    #[test]
    fn points_pack_and_unpack_with_their_signs() {
        for (x, y) in [
            (0, 0),
            (1, 2),
            (-1, -1),
            (-1920, 1080),
            (3839, -5),
            (32767, -32768),
        ] {
            assert_eq!(point_of(lparam_of(x, y)), (x, y));
        }
        // A negative coordinate does not leak into the other word.
        assert_eq!(lparam_of(-1, 0) as u32, 0x0000_ffff);
        assert_eq!(lparam_of(0, -1) as u32, 0xffff_0000);
    }
}
