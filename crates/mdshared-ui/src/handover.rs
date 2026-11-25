//! The hand-over of a second launch reaching a winit window (roadmap P8b).
//!
//! [`mdshared_app::single_mutex::send`] hands the first instance's window a second launch's
//! command line in a `WM_COPYDATA`. That message is sent, not posted, so it reaches only the
//! window procedure, and winit 0.30's `with_msg_hook` sees posted messages only: a winit window
//! answers it with the default handling (0), and `send` gives false. [`listen`] replaces the
//! window procedure once with one that gives each `WM_COPYDATA` to
//! [`single_mutex::received`](mdshared_app::single_mutex::received): a hand-over for the
//! application's id is queued, answered with 1 (so `send` gives true), and `wake` is called; any
//! other message goes on to winit's procedure. The loop takes the queued hand-overs with [`take`]
//! when it is woken.
//!
//! ```ignore
//! // At start: `instance` from `single_mutex::acquire(APP_ID)`, held.
//! instance.publish_window(hwnd);
//! let proxy = event_loop.create_proxy();
//! handover::listen(&window, APP_ID, move || { let _ = proxy.send_event(UserEvent::Handover); });
//! // In `user_event`: for args in handover::take(&window) { open(args) }
//! ```
//!
//! The procedure is replaced with `SetWindowLongPtrW` and chained with `CallWindowProcW`, as
//! [`crate::snap`] does, so either may be set first: each passes on what it does not handle.
//!
//! Deliberately not here: bringing the window forward (the sender does that, see
//! `single_mutex`), and anything off Windows: [`listen`] gives false there and [`take`] nothing.

use std::ffi::OsString;

/// Replaces the window procedure of `window` once, so hand-overs for `app_id` are queued for
/// [`take`] and `wake` is called for each. True when it is in place (also when it already was:
/// the first `app_id` and `wake` stay). Call it on the window's own thread (the event loop's);
/// from another thread, and off Windows, it does nothing and gives false. `wake` runs inside the
/// window procedure: it must be cheap, not block and not panic (an event loop proxy's
/// `send_event` is right).
#[cfg(windows)]
pub fn listen(window: &winit::window::Window, app_id: &str, wake: impl Fn() + 'static) -> bool {
    hwnd_of(window).is_some_and(|hwnd| imp::listen(hwnd, app_id, Box::new(wake)))
}

/// Does nothing off Windows.
#[cfg(not(windows))]
pub fn listen(_window: &winit::window::Window, _app_id: &str, _wake: impl Fn() + 'static) -> bool {
    false
}

/// The hand-overs `window` received since the last call, oldest first: each the arguments of
/// one launch, as [`single_mutex::send`](mdshared_app::single_mutex::send) was given them.
/// Empty when none came, before [`listen`], and off Windows.
#[cfg(windows)]
pub fn take(window: &winit::window::Window) -> Vec<Vec<OsString>> {
    hwnd_of(window).map(imp::take).unwrap_or_default()
}

/// Nothing off Windows.
#[cfg(not(windows))]
pub fn take(_window: &winit::window::Window) -> Vec<Vec<OsString>> {
    Vec::new()
}

#[cfg(windows)]
fn hwnd_of(window: &winit::window::Window) -> Option<isize> {
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    match window.window_handle().ok()?.as_raw() {
        RawWindowHandle::Win32(h) => Some(h.hwnd.get()),
        _ => None,
    }
}

#[cfg(windows)]
mod imp {
    use std::cell::RefCell;
    use std::ffi::OsString;
    use std::rc::Rc;
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::System::Threading::GetCurrentThreadId;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CallWindowProcW, DefWindowProcW, GWLP_WNDPROC, GetWindowLongPtrW, GetWindowThreadProcessId,
        SetWindowLongPtrW, WM_NCDESTROY, WNDPROC,
    };

    /// A window whose procedure [`listen`] replaced.
    struct Entry {
        hwnd: isize,
        /// The procedure that was the window procedure, as `GetWindowLongPtrW` gave it.
        prev: isize,
        app_id: String,
        wake: Box<dyn Fn()>,
        queued: RefCell<Vec<Vec<OsString>>>,
    }

    thread_local! {
        /// The windows of this thread that listen. A window handles its messages on its own
        /// thread, so a thread-local is all the sharing there is.
        static ENTRIES: RefCell<Vec<Rc<Entry>>> = const { RefCell::new(Vec::new()) };
    }

    fn entry(hwnd: isize) -> Option<Rc<Entry>> {
        ENTRIES.with(|e| e.borrow().iter().find(|x| x.hwnd == hwnd).cloned())
    }

    pub(super) fn listen(hwnd: isize, app_id: &str, wake: Box<dyn Fn()>) -> bool {
        let window = hwnd as HWND;
        // SAFETY: no pointers are passed (a null process id is allowed); a stale handle only
        // makes it return 0.
        let same_thread = unsafe {
            GetWindowThreadProcessId(window, std::ptr::null_mut()) == GetCurrentThreadId()
        };
        if !same_thread {
            return false;
        }
        if entry(hwnd).is_some() {
            return true;
        }
        // SAFETY: reads the window procedure of a window of this thread.
        let prev = unsafe { GetWindowLongPtrW(window, GWLP_WNDPROC) };
        if prev == 0 {
            return false;
        }
        let entry = Rc::new(Entry {
            hwnd,
            prev,
            app_id: app_id.to_owned(),
            wake,
            queued: RefCell::new(Vec::new()),
        });
        // The entry is in place before the procedure that looks for it.
        ENTRIES.with(|e| e.borrow_mut().push(entry));
        // SAFETY: `proc` is an `extern "system"` function with a window procedure's signature
        // that calls `prev` for everything it does not handle; the window belongs to this thread.
        unsafe {
            SetWindowLongPtrW(
                window,
                GWLP_WNDPROC,
                handover_proc as *const () as usize as isize,
            )
        };
        true
    }

    pub(super) fn take(hwnd: isize) -> Vec<Vec<OsString>> {
        entry(hwnd)
            .map(|e| std::mem::take(&mut *e.queued.borrow_mut()))
            .unwrap_or_default()
    }

    /// The window procedure that stands in front of the window's own.
    ///
    /// # Safety
    /// Called by Windows as the window procedure of a window of this thread.
    unsafe extern "system" fn handover_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        let Some(state) = entry(hwnd as isize) else {
            // Cannot happen: the entry is made before the procedure is set and removed only
            // after the window's last message.
            // SAFETY: the default handling of the message received.
            return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
        };
        // SAFETY: `msg` and `lparam` are the ones this procedure was called with.
        if let Some(args) =
            unsafe { mdshared_app::single_mutex::received(&state.app_id, msg, lparam) }
        {
            state.queued.borrow_mut().push(args);
            (state.wake)();
            return 1;
        }
        // SAFETY: `prev` came from `GetWindowLongPtrW(GWLP_WNDPROC)` of this window, a window
        // procedure or a handle `CallWindowProcW` knows how to call; `WNDPROC` is an optional
        // function pointer, the same size as that value. The message is passed on as it came.
        let result = unsafe {
            CallWindowProcW(
                std::mem::transmute::<isize, WNDPROC>(state.prev),
                hwnd,
                msg,
                wparam,
                lparam,
            )
        };
        if msg == WM_NCDESTROY {
            // The window's last message: its entry goes with it.
            ENTRIES.with(|e| e.borrow_mut().retain(|x| x.hwnd != state.hwnd));
        }
        result
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::cell::Cell;
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DestroyWindow, RegisterClassW, WNDCLASSW, WS_POPUP,
        };

        thread_local! {
            /// How many `WM_COPYDATA` the window's own procedure (winit's stand-in) saw.
            static INNER: Cell<usize> = const { Cell::new(0) };
        }

        unsafe extern "system" fn inner(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
            if msg == windows_sys::Win32::UI::WindowsAndMessaging::WM_COPYDATA {
                INNER.with(|n| n.set(n.get() + 1));
            }
            // SAFETY: the message is passed on as it came.
            unsafe { DefWindowProcW(hwnd, msg, w, l) }
        }

        fn window() -> HWND {
            let name: Vec<u16> = "mdshared_handover_test\0".encode_utf16().collect();
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
            // SAFETY: `class` and the name outlive the calls; registering twice fails harmlessly.
            unsafe {
                RegisterClassW(&class);
                CreateWindowExW(
                    0,
                    name.as_ptr(),
                    name.as_ptr(),
                    WS_POPUP,
                    0,
                    0,
                    10,
                    10,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null(),
                )
            }
        }

        /// A hand-over for the window's id is queued, answered (so `send` gives true) and wakes
        /// once each; another id's goes on to the window's own procedure and is refused; the
        /// window going takes its entry with it.
        #[test]
        fn a_handover_is_queued_and_answered_and_others_pass_on() {
            use mdshared_app::single_mutex::send;
            let hwnd = window();
            assert!(!hwnd.is_null(), "the test window was not created");
            let h = hwnd as isize;
            let id = "mdshared.handover.test";
            // Before listening: winit's default handling, so the sender is told no.
            assert!(!send(id, h, &["early".into()]));
            assert!(take(h).is_empty());
            let wakes = Rc::new(Cell::new(0));
            let woken = wakes.clone();
            assert!(listen(h, id, Box::new(move || woken.set(woken.get() + 1))));
            assert!(
                listen(h, "another", Box::new(|| {})),
                "once; the first id stays"
            );
            INNER.with(|n| n.set(0));
            assert!(send(id, h, &["a.md".into(), "--new".into()]));
            assert!(send(id, h, &[]));
            assert_eq!(wakes.get(), 2);
            assert_eq!(INNER.with(|n| n.get()), 0, "answered here, not passed on");
            let got = take(h);
            assert_eq!(
                got,
                vec![
                    vec![OsString::from("a.md"), OsString::from("--new")],
                    vec![]
                ]
            );
            assert!(take(h).is_empty(), "taken once");
            // Another application's hand-over is not ours: on to the window, refused.
            assert!(!send("another", h, &["x".into()]));
            assert_eq!(INNER.with(|n| n.get()), 1);
            assert!(take(h).is_empty());
            assert_eq!(wakes.get(), 2);
            // SAFETY: the test's own window, on this thread.
            unsafe { DestroyWindow(hwnd) };
            assert!(entry(h).is_none(), "the entry went with the window");
        }
    }
}
