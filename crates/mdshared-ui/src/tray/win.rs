//! The Windows backend: `Shell_NotifyIconW`, heard through a hidden window of its own.
//!
//! The icon belongs to a window, and its clicks come as that window's messages, so a thread of
//! the backend's own makes a hidden window and runs its message loop: the application's thread
//! never needs to pump anything. The application's changes are put in [`Shared`] and the window
//! is told with a posted message; the window adds, changes and deletes the icon, shows the menu
//! with `TrackPopupMenuEx` (which needs the window in front first, or the menu does not close
//! when the person clicks elsewhere), and adds the icon again when Explorer restarts
//! (`TaskbarCreated`). A hidden top-level window is used, not a message-only one, because only
//! top-level windows hear that broadcast.

use std::io;
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;

use mdshared_text::image::Image;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateBitmap, CreateDIBSection, DIB_RGB_COLORS,
    DeleteObject, GetDC, ReleaseDC,
};
use windows_sys::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY, NIM_SETVERSION,
    NIN_SELECT, NOTIFYICON_VERSION_4, NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CREATESTRUCTW, CreateIconIndirect, CreatePopupMenu, CreateWindowExW,
    DefWindowProcW, DestroyIcon, DestroyMenu, DestroyWindow, DispatchMessageW, GWLP_USERDATA,
    GetCursorPos, GetMessageW, GetSystemMetrics, GetWindowLongPtrW, HICON, ICONINFO, MF_CHECKED,
    MF_GRAYED, MF_SEPARATOR, MF_STRING, MSG, PostMessageW, PostQuitMessage, RegisterClassW,
    RegisterWindowMessageW, SM_CXSMICON, SetForegroundWindow, SetWindowLongPtrW, TPM_NONOTIFY,
    TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenuEx, TranslateMessage, WM_APP, WM_CONTEXTMENU,
    WM_DESTROY, WM_MBUTTONUP, WM_NCCREATE, WM_NULL, WNDCLASSW, WS_OVERLAPPED,
};

use super::{Entry, Event, Identity, Inbox, marked, tooltip_units};

/// The icon's messages to its window.
const CALLBACK: u32 = WM_APP + 1;
/// The application changed something in [`Shared`].
const CHANGED: u32 = WM_APP + 2;
/// The application dropped the tray.
const QUIT: u32 = WM_APP + 3;
/// Enter or Space on the icon: `NIN_SELECT | NINF_KEY`, which windows-sys does not name.
const NIN_KEYSELECT: u32 = NIN_SELECT | 0x1;

/// What the application set, read by the window's thread.
struct Shared {
    icon: Option<Image>,
    tooltip: String,
    menu: Vec<Entry>,
}

/// The window's own state, behind its `GWLP_USERDATA`.
struct State {
    shared: Arc<Mutex<Shared>>,
    inbox: Arc<Inbox>,
    taskbar_created: u32,
    hicon: HICON,
    added: bool,
}

// SAFETY: the icon handle is owned by the state alone; the state moves once, to the window's
// thread, before any of its handles is made, and is used only there afterwards.
unsafe impl Send for State {}

pub(super) struct Icon {
    shared: Arc<Mutex<Shared>>,
    /// The hidden window, as an integer so the handle can cross threads.
    hwnd: isize,
    thread: Option<JoinHandle<()>>,
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

impl Icon {
    pub(super) fn open(identity: &Identity, icon: &Image, inbox: Arc<Inbox>) -> io::Result<Icon> {
        let shared = Arc::new(Mutex::new(Shared {
            icon: Some(icon.clone()),
            tooltip: identity.name.to_string(),
            menu: Vec::new(),
        }));
        let (tx, rx) = std::sync::mpsc::channel::<io::Result<isize>>();
        let state = State {
            shared: shared.clone(),
            inbox,
            taskbar_created: 0,
            hicon: std::ptr::null_mut(),
            added: false,
        };
        let thread = std::thread::Builder::new()
            .name("tray".into())
            .spawn(move || run(state, tx))?;
        let hwnd = rx
            .recv()
            .map_err(|_| io::Error::other("the tray's thread ended before its window"))??;
        Ok(Icon {
            shared,
            hwnd,
            thread: Some(thread),
        })
    }

    fn changed(&self) -> io::Result<()> {
        // SAFETY: the window lives until QUIT, which only Drop posts.
        let ok = unsafe { PostMessageW(self.hwnd as HWND, CHANGED, 0, 0) };
        if ok == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    fn shared(&self) -> std::sync::MutexGuard<'_, Shared> {
        self.shared.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(super) fn set_icon(&mut self, icon: &Image) -> io::Result<()> {
        self.shared().icon = Some(icon.clone());
        self.changed()
    }

    pub(super) fn set_tooltip(&mut self, text: &str) -> io::Result<()> {
        self.shared().tooltip = text.to_string();
        self.changed()
    }

    pub(super) fn set_menu(&mut self, menu: &[Entry]) -> io::Result<()> {
        // The menu is read when it opens; nothing on screen changes now.
        self.shared().menu = menu.to_vec();
        Ok(())
    }
}

impl Drop for Icon {
    fn drop(&mut self) {
        // SAFETY: as in `changed`; the window deletes the icon and ends its loop.
        unsafe { PostMessageW(self.hwnd as HWND, QUIT, 0, 0) };
        if let Some(t) = self.thread.take() {
            _ = t.join();
        }
    }
}

/// The window's thread: makes the window, adds the icon, answers `tx`, and runs the loop.
fn run(state: State, tx: std::sync::mpsc::Sender<io::Result<isize>>) {
    let class = wide("mdshared-tray");
    let wc = WNDCLASSW {
        lpfnWndProc: Some(proc_),
        lpszClassName: class.as_ptr(),
        // SAFETY: an all-zero WNDCLASSW is valid for the fields not set.
        ..unsafe { std::mem::zeroed() }
    };
    // SAFETY: `wc` and the class name outlive the call. A second tray finds the class there
    // already; that failure is harmless and the window below is made all the same.
    unsafe { RegisterClassW(&wc) };
    let title = wide("");
    let state = Box::into_raw(Box::new(state));
    // SAFETY: the class is registered; `state` is handed to WM_NCCREATE, which keeps it in the
    // window, and is freed at WM_DESTROY.
    let hwnd = unsafe {
        CreateWindowExW(
            0,
            class.as_ptr(),
            title.as_ptr(),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            state.cast(),
        )
    };
    if hwnd.is_null() {
        let e = io::Error::last_os_error();
        // SAFETY: the window was not made, so nothing else holds `state`.
        drop(unsafe { Box::from_raw(state) });
        _ = tx.send(Err(e));
        return;
    }
    // SAFETY: the window's state is set (WM_NCCREATE ran) and only this thread touches it.
    let added = unsafe { add(hwnd, &mut *state) };
    if let Err(e) = added {
        // SAFETY: the window is ours; WM_DESTROY frees `state`.
        unsafe { DestroyWindow(hwnd) };
        _ = tx.send(Err(e));
        return;
    }
    _ = tx.send(Ok(hwnd as isize));
    // SAFETY: an ordinary message loop on the window's own thread.
    unsafe {
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

/// The icon's data for `Shell_NotifyIconW`.
fn data(hwnd: HWND, state: &State) -> NOTIFYICONDATAW {
    // SAFETY: an all-zero NOTIFYICONDATAW is valid; the fields used are set below.
    let mut d: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
    d.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    d.hWnd = hwnd;
    d.uID = 1;
    d.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP | NIF_SHOWTIP;
    d.uCallbackMessage = CALLBACK;
    d.hIcon = state.hicon;
    let tip = tooltip_units(
        &state
            .shared
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .tooltip,
        d.szTip.len() - 1,
    );
    d.szTip[..tip.len()].copy_from_slice(&tip);
    d
}

/// Makes the icon's picture anew from what the application set, if it set one.
fn refresh_icon(state: &mut State) {
    let image = state
        .shared
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .icon
        .take();
    if let Some(image) = image {
        let new = hicon(&image);
        if !new.is_null() {
            if !state.hicon.is_null() {
                // SAFETY: made by `hicon` and no longer given to the shell after this.
                unsafe { DestroyIcon(state.hicon) };
            }
            state.hicon = new;
        }
    }
}

/// Adds the icon (again, after Explorer restarted) and asks for the version 4 messages.
///
/// # Safety
/// `hwnd` is the tray's window, on its own thread.
unsafe fn add(hwnd: HWND, state: &mut State) -> io::Result<()> {
    refresh_icon(state);
    let mut d = data(hwnd, state);
    // SAFETY: `d` is filled for this window.
    if unsafe { Shell_NotifyIconW(NIM_ADD, &d) } == 0 {
        return Err(io::Error::other("the shell refused the tray icon"));
    }
    d.Anonymous.uVersion = NOTIFYICON_VERSION_4;
    // SAFETY: as above.
    unsafe { Shell_NotifyIconW(NIM_SETVERSION, &d) };
    state.added = true;
    Ok(())
}

/// An icon from RGBA pixels, at the size of a small icon: a 32-bit colour bitmap with its
/// alpha and an empty mask. Null when the system refuses.
fn hicon(image: &Image) -> HICON {
    // SAFETY: a plain metric.
    let size = unsafe { GetSystemMetrics(SM_CXSMICON) }.clamp(16, 256) as u32;
    let scaled;
    let image = if image.width == size && image.height == size {
        image
    } else {
        match image.scaled(size, size) {
            Some(s) => {
                scaled = s;
                &scaled
            }
            None => return std::ptr::null_mut(),
        }
    };
    let (w, h) = (image.width as i32, image.height as i32);
    // SAFETY: an all-zero BITMAPINFO is valid; the header is filled below.
    let mut info: BITMAPINFO = unsafe { std::mem::zeroed() };
    info.bmiHeader = BITMAPINFOHEADER {
        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: w,
        // Negative: the rows top down, as the image's.
        biHeight: -h,
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB,
        ..info.bmiHeader
    };
    let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
    // SAFETY: the screen's DC is taken and given back; the section's bits are written within
    // its w * h * 4 bytes; the bitmaps are deleted once the icon holds its own copies.
    unsafe {
        let dc = GetDC(std::ptr::null_mut());
        let color = CreateDIBSection(
            dc,
            &info,
            DIB_RGB_COLORS,
            &mut bits,
            std::ptr::null_mut(),
            0,
        );
        ReleaseDC(std::ptr::null_mut(), dc);
        if color.is_null() || bits.is_null() {
            return std::ptr::null_mut();
        }
        let out = std::slice::from_raw_parts_mut(bits.cast::<u8>(), (w * h * 4) as usize);
        for (o, i) in out
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(image.rgba.as_chunks::<4>().0)
        {
            o.copy_from_slice(&[i[2], i[1], i[0], i[3]]);
        }
        let mask = CreateBitmap(w, h, 1, 1, std::ptr::null());
        let info = ICONINFO {
            fIcon: 1,
            xHotspot: 0,
            yHotspot: 0,
            hbmMask: mask,
            hbmColor: color,
        };
        let icon = CreateIconIndirect(&info);
        DeleteObject(color);
        DeleteObject(mask);
        icon
    }
}

/// Shows the menu at `(x, y)` and answers the chosen item's id.
///
/// # Safety
/// `hwnd` is the tray's window, on its own thread.
unsafe fn show_menu(hwnd: HWND, menu: &[Entry], x: i32, y: i32) -> Option<u32> {
    if menu.is_empty() {
        return None;
    }
    // SAFETY: the menu is made, filled, shown and destroyed here; the strings outlive the
    // calls that read them.
    unsafe {
        let hmenu = CreatePopupMenu();
        if hmenu.is_null() {
            return None;
        }
        let mut ids = Vec::new();
        for entry in menu {
            match entry {
                Entry::Separator => {
                    AppendMenuW(hmenu, MF_SEPARATOR, 0, std::ptr::null());
                }
                Entry::Item {
                    id,
                    text,
                    access,
                    hint,
                    enabled,
                    checked,
                } => {
                    let mut label = marked(text, *access, '&');
                    if !hint.is_empty() {
                        label.push('\t');
                        label.push_str(hint);
                    }
                    let label = wide(&label);
                    let mut flags = MF_STRING;
                    if !enabled {
                        flags |= MF_GRAYED;
                    }
                    if *checked == Some(true) {
                        flags |= MF_CHECKED;
                    }
                    // Command 0 means "nothing chosen", so the commands count from 1.
                    ids.push(*id);
                    AppendMenuW(hmenu, flags, ids.len(), label.as_ptr());
                }
            }
        }
        SetForegroundWindow(hwnd);
        let chosen = TrackPopupMenuEx(
            hmenu,
            TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON,
            x,
            y,
            hwnd,
            std::ptr::null(),
        );
        // The documented nudge so the next opening works.
        PostMessageW(hwnd, WM_NULL, 0, 0);
        DestroyMenu(hmenu);
        usize::try_from(chosen)
            .ok()
            .and_then(|c| c.checked_sub(1))
            .and_then(|i| ids.get(i).copied())
    }
}

/// The low and high words of a version 4 callback's `lParam`, and its anchor in `wParam`.
fn split(wparam: WPARAM, lparam: LPARAM) -> (u32, i32, i32) {
    let event = (lparam as u32) & 0xffff;
    let x = (wparam & 0xffff) as u16 as i16 as i32;
    let y = ((wparam >> 16) & 0xffff) as u16 as i16 as i32;
    (event, x, y)
}

unsafe extern "system" fn proc_(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // SAFETY (whole function): the state pointer is set at WM_NCCREATE from the Box made in
    // `run`, read only on this thread, and freed once at WM_DESTROY.
    unsafe {
        if msg == WM_NCCREATE {
            let create = &*(lparam as *const CREATESTRUCTW);
            let state = create.lpCreateParams.cast::<State>();
            (*state).taskbar_created = RegisterWindowMessageW(wide("TaskbarCreated").as_ptr());
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, state as isize);
            return DefWindowProcW(hwnd, msg, wparam, lparam);
        }
        let state = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut State;
        if state.is_null() {
            return DefWindowProcW(hwnd, msg, wparam, lparam);
        }
        let s = &mut *state;
        match msg {
            CALLBACK => {
                let (event, x, y) = split(wparam, lparam);
                match event {
                    NIN_SELECT | NIN_KEYSELECT => s.inbox.push(Event::Clicked),
                    WM_MBUTTONUP => s.inbox.push(Event::MiddleClicked),
                    WM_CONTEXTMENU => {
                        let menu = s
                            .shared
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner)
                            .menu
                            .clone();
                        let (mut x, mut y) = (x, y);
                        if x == 0 && y == 0 {
                            let mut p = POINT { x: 0, y: 0 };
                            GetCursorPos(&mut p);
                            (x, y) = (p.x, p.y);
                        }
                        if let Some(id) = show_menu(hwnd, &menu, x, y) {
                            s.inbox.push(Event::Chosen(id));
                        }
                    }
                    _ => {}
                }
                0
            }
            CHANGED => {
                refresh_icon(s);
                if s.added {
                    let d = data(hwnd, s);
                    Shell_NotifyIconW(NIM_MODIFY, &d);
                }
                0
            }
            QUIT => {
                if s.added {
                    let d = data(hwnd, s);
                    Shell_NotifyIconW(NIM_DELETE, &d);
                    s.added = false;
                }
                DestroyWindow(hwnd);
                0
            }
            WM_DESTROY => {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                let s = Box::from_raw(state);
                if !s.hicon.is_null() {
                    DestroyIcon(s.hicon);
                }
                PostQuitMessage(0);
                0
            }
            m if m == s.taskbar_created && m != 0 => {
                // Explorer restarted: the icon is gone from the new taskbar; add it again.
                s.added = false;
                _ = add(hwnd, s);
                0
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_callbacks_words_are_read_as_version_4_gives_them() {
        // The event in the low word of lParam, the icon's id in the high; the anchor in wParam.
        let lparam = ((1u32 << 16) | WM_CONTEXTMENU) as LPARAM;
        let wparam = ((300usize) << 16) | ((-20i16 as u16) as usize);
        assert_eq!(split(wparam, lparam), (WM_CONTEXTMENU, -20, 300));
    }

    #[test]
    fn an_icon_is_made_from_pixels_of_any_size() {
        let image = Image::new(40, 40, vec![255; 40 * 40 * 4]).unwrap();
        let icon = hicon(&image);
        assert!(!icon.is_null());
        // SAFETY: made just above and not given to anyone.
        unsafe { DestroyIcon(icon) };
    }
}
