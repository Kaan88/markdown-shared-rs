//! OS glue: clipboard, URLs, file manager, system theme/metrics, dialogs.

use mdshared_app::backdrop::Backdrop;
pub use mdshared_text::Subpixel;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[cfg(feature = "recycle")]
mod recycle;
#[cfg(feature = "recycle")]
pub use recycle::{BinRefusal, open_recycle_bin, recycle, recycle_refusal};

fn has_safe_scheme(url: &str) -> bool {
    let b = url.as_bytes();
    let has = |p: &str| b.len() >= p.len() && b[..p.len()].eq_ignore_ascii_case(p.as_bytes());
    has("http://") || has("https://") || has("mailto:")
}

/// Native "open files" dialog with one named filter (the application's file kind, for example
/// `"Markdown"` and its extensions) plus "All files".
pub fn pick_files(
    start_dir: Option<&Path>,
    filter_name: &str,
    extensions: &[&str],
) -> Vec<PathBuf> {
    let mut d = rfd::FileDialog::new()
        .add_filter(filter_name, extensions)
        .add_filter("All files", &["*"]);
    if let Some(dir) = start_dir {
        d = d.set_directory(dir);
    }
    d.pick_files().unwrap_or_default()
}

/// Native folder picker.
pub fn pick_folder(start_dir: Option<&Path>) -> Option<PathBuf> {
    let mut d = rfd::FileDialog::new();
    if let Some(dir) = start_dir {
        d = d.set_directory(dir);
    }
    d.pick_folder()
}

/// Set while a dialog started by one of the `*_async` functions is open.
static DIALOG_OPEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Clears [`DIALOG_OPEN`] when the dialog's thread ends, even by a panic.
struct DialogGuard;

impl Drop for DialogGuard {
    fn drop(&mut self) {
        DIALOG_OPEN.store(false, std::sync::atomic::Ordering::SeqCst);
    }
}

/// Whether a dialog opened by `pick_files_async`, `pick_folder_async` or `pick_save_async` is open.
pub fn dialog_open() -> bool {
    DIALOG_OPEN.load(std::sync::atomic::Ordering::SeqCst)
}

/// Runs `show` (a blocking dialog) on a helper thread and then `done` with its result, on that
/// thread: the caller must only post a message to the UI thread from `done`. One dialog at a time:
/// while one is open, a new request is refused, `done` is called at once on the calling thread with
/// `empty`, and the result is false. True means the dialog was started. Used off the UI thread
/// because on Linux a blocking dialog on it makes the window manager flag the window as not
/// responding; it works the same on every OS.
fn run_dialog<T: Send + 'static>(
    empty: T,
    show: impl FnOnce() -> T + Send + 'static,
    done: impl FnOnce(T) + Send + 'static,
) -> bool {
    use std::sync::atomic::Ordering;
    if DIALOG_OPEN
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        done(empty);
        return false;
    }
    // `done` and `empty` are shared with the thread so they can still be used if it cannot start.
    let job = std::sync::Arc::new(std::sync::Mutex::new(Some((show, done, empty))));
    let thread_job = job.clone();
    let spawned = std::thread::Builder::new()
        .name("mdshared-dialog".into())
        .spawn(move || {
            let _guard = DialogGuard;
            let taken = thread_job.lock().ok().and_then(|mut j| j.take());
            if let Some((show, done, _)) = taken {
                done(show());
            }
        });
    if spawned.is_err() {
        DIALOG_OPEN.store(false, Ordering::SeqCst);
        if let Some((_, done, empty)) = job.lock().ok().and_then(|mut j| j.take()) {
            done(empty);
        }
        return false;
    }
    true
}

/// [`pick_files`] without blocking the caller: the dialog runs on a helper thread and `done`
/// receives the chosen files (empty when cancelled) on that thread. Returns false, with `done`
/// called at once with an empty list, when another dialog of this family is already open.
pub fn pick_files_async(
    start_dir: Option<&Path>,
    filter_name: &str,
    extensions: &[&str],
    done: impl FnOnce(Vec<PathBuf>) + Send + 'static,
) -> bool {
    let start = start_dir.map(Path::to_path_buf);
    let name = filter_name.to_string();
    let exts: Vec<String> = extensions.iter().map(|e| e.to_string()).collect();
    run_dialog(
        Vec::new(),
        move || {
            let exts: Vec<&str> = exts.iter().map(String::as_str).collect();
            pick_files(start.as_deref(), &name, &exts)
        },
        done,
    )
}

/// [`pick_folder`] without blocking the caller; see [`pick_files_async`].
pub fn pick_folder_async(
    start_dir: Option<&Path>,
    done: impl FnOnce(Option<PathBuf>) + Send + 'static,
) -> bool {
    let start = start_dir.map(Path::to_path_buf);
    run_dialog(None, move || pick_folder(start.as_deref()), done)
}

/// A Save as dialog opened on `current`'s folder (when it exists) with its file name filled in,
/// without blocking the caller; `done` gets the chosen path or `None`. See [`pick_files_async`].
pub fn pick_save_async(
    current: &Path,
    done: impl FnOnce(Option<PathBuf>) + Send + 'static,
) -> bool {
    let current = current.to_path_buf();
    run_dialog(
        None,
        move || {
            let mut dialog = rfd::FileDialog::new();
            if let Some(dir) = current.parent().filter(|d| d.is_dir()) {
                dialog = dialog.set_directory(dir);
            }
            if let Some(name) = current.file_name() {
                dialog = dialog.set_file_name(name.to_string_lossy());
            }
            dialog.save_file()
        },
        done,
    )
}

#[cfg(windows)]
mod imp {
    use super::*;
    use windows_sys::Win32::Foundation::{GlobalFree, HANDLE, HWND};
    use windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute;
    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, GetClipboardData, OpenClipboard, RegisterClipboardFormatW,
        SetClipboardData,
    };
    use windows_sys::Win32::System::Memory::{
        GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock,
    };
    use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetDoubleClickTime;
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetCaretBlinkTime, SW_SHOWNORMAL};

    const CF_UNICODETEXT: u32 = 13;

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// Opens the clipboard, retrying briefly; returns whether it is now open.
    fn open_clipboard() -> bool {
        for _ in 0..10 {
            // SAFETY: a null owner window is allowed; associates with the current task.
            if unsafe { OpenClipboard(std::ptr::null_mut()) } != 0 {
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        false
    }

    pub fn clipboard_text() -> Option<String> {
        if !open_clipboard() {
            return None;
        }
        let result = read_clipboard_open();
        // SAFETY: the clipboard was opened by this thread above.
        unsafe { CloseClipboard() };
        result
    }

    fn read_clipboard_open() -> Option<String> {
        // SAFETY: the clipboard is open; the returned handle is owned by the system.
        let h: HANDLE = unsafe { GetClipboardData(CF_UNICODETEXT) };
        if h.is_null() {
            return None;
        }
        // SAFETY: h is a valid global memory handle from the clipboard.
        let ptr = unsafe { GlobalLock(h) } as *const u16;
        if ptr.is_null() {
            return None;
        }
        // SAFETY: h is locked, so querying its size is valid.
        let max = unsafe { GlobalSize(h) } / 2;
        // SAFETY: ptr points to `max` readable u16 units while the lock is held.
        let units = unsafe { std::slice::from_raw_parts(ptr, max) };
        let len = units.iter().position(|&u| u == 0).unwrap_or(units.len());
        let text = String::from_utf16_lossy(&units[..len]);
        // SAFETY: matches the GlobalLock above.
        unsafe { GlobalUnlock(h) };
        Some(text.replace("\r\n", "\n"))
    }

    /// Text and HTML together: CF_UNICODETEXT and "HTML Format". The text is what counts; the
    /// HTML is best effort, so a failure of it still leaves the copy usable.
    pub fn set_clipboard_html(html: &str, text: &str) -> bool {
        let text = text.replace("\r\n", "\n").replace('\n', "\r\n");
        let data: Vec<u8> = wide(&text).iter().flat_map(|u| u.to_le_bytes()).collect();
        let mut cf = crate::cf_html::cf_html(html).into_bytes();
        cf.push(0);
        // SAFETY: a NUL-terminated UTF-16 name.
        let format = unsafe { RegisterClipboardFormatW(wide("HTML Format").as_ptr()) };
        if !open_clipboard() {
            return false;
        }
        // SAFETY: the clipboard was opened by this thread above.
        let ok = unsafe { EmptyClipboard() } != 0 && put_format(CF_UNICODETEXT, &data);
        if ok && format != 0 {
            put_format(format, &cf);
        }
        // SAFETY: the clipboard is open and owned by this thread.
        unsafe { CloseClipboard() };
        ok
    }

    /// The HTML the clipboard holds: the fragment of its "HTML Format".
    pub fn clipboard_html() -> Option<String> {
        crate::cf_html::fragment(&clipboard_html_format()?)
    }

    /// The bytes of the "HTML Format" on the clipboard, up to its first NUL; `None` when there
    /// is none, or more than [`crate::cf_html::MAX_BYTES`].
    pub fn clipboard_html_format() -> Option<Vec<u8>> {
        // SAFETY: a NUL-terminated UTF-16 name.
        let format = unsafe { RegisterClipboardFormatW(wide("HTML Format").as_ptr()) };
        if format == 0 || !open_clipboard() {
            return None;
        }
        // SAFETY: the clipboard is open; the returned handle is owned by the system.
        let h: HANDLE = unsafe { GetClipboardData(format) };
        let mut out = None;
        if !h.is_null() {
            // SAFETY: h is a valid global memory handle from the clipboard.
            let ptr = unsafe { GlobalLock(h) } as *const u8;
            if !ptr.is_null() {
                // SAFETY: h is locked, so its size is valid and ptr points to that many bytes.
                let bytes = unsafe { std::slice::from_raw_parts(ptr, GlobalSize(h)) };
                let len = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
                if len <= crate::cf_html::MAX_BYTES {
                    out = bytes.get(..len).map(<[u8]>::to_vec);
                }
                // SAFETY: matches the GlobalLock above.
                unsafe { GlobalUnlock(h) };
            }
        }
        // SAFETY: the clipboard was opened by this thread above.
        unsafe { CloseClipboard() };
        out
    }

    /// Puts one format on the open clipboard; the system owns the memory once that succeeds.
    fn put_format(format: u32, bytes: &[u8]) -> bool {
        // SAFETY: plain allocation request.
        let h = unsafe { GlobalAlloc(GMEM_MOVEABLE, bytes.len()) };
        if h.is_null() {
            return false;
        }
        // SAFETY: h is a valid moveable global handle.
        let dst = unsafe { GlobalLock(h) } as *mut u8;
        if dst.is_null() {
            // SAFETY: h is still owned by us.
            unsafe { GlobalFree(h) };
            return false;
        }
        // SAFETY: dst has room for `bytes.len()` bytes; regions do not overlap.
        unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), dst, bytes.len()) };
        // SAFETY: matches the GlobalLock above.
        unsafe { GlobalUnlock(h) };
        // SAFETY: h holds the bytes of the format; on success the system owns it.
        if unsafe { SetClipboardData(format, h) }.is_null() {
            // SAFETY: ownership was not transferred.
            unsafe { GlobalFree(h) };
            return false;
        }
        true
    }

    pub fn set_clipboard_text(text: &str) -> bool {
        let converted;
        let text = if text.contains("\r\n") || !text.contains('\n') {
            text
        } else {
            converted = text.replace('\n', "\r\n");
            &converted
        };
        let data = wide(text);
        let bytes = data.len() * 2;
        if !open_clipboard() {
            return false;
        }
        let ok = write_clipboard_open(&data, bytes);
        // SAFETY: the clipboard was opened by this thread above.
        unsafe { CloseClipboard() };
        ok
    }

    fn write_clipboard_open(data: &[u16], bytes: usize) -> bool {
        // SAFETY: the clipboard is open and owned by this thread.
        if unsafe { EmptyClipboard() } == 0 {
            return false;
        }
        // SAFETY: plain allocation request.
        let h = unsafe { GlobalAlloc(GMEM_MOVEABLE, bytes) };
        if h.is_null() {
            return false;
        }
        // SAFETY: h is a valid moveable global handle.
        let dst = unsafe { GlobalLock(h) } as *mut u16;
        if dst.is_null() {
            // SAFETY: h is still owned by us.
            unsafe { GlobalFree(h) };
            return false;
        }
        // SAFETY: dst has room for `data.len()` u16 units; regions do not overlap.
        unsafe { std::ptr::copy_nonoverlapping(data.as_ptr(), dst, data.len()) };
        // SAFETY: matches the GlobalLock above.
        unsafe { GlobalUnlock(h) };
        // SAFETY: h holds NUL-terminated UTF-16; on success the system owns it.
        if unsafe { SetClipboardData(CF_UNICODETEXT, h) }.is_null() {
            // SAFETY: ownership was not transferred.
            unsafe { GlobalFree(h) };
            return false;
        }
        true
    }

    pub fn open_url(url: &str) -> bool {
        if !has_safe_scheme(url) {
            return false;
        }
        let verb = wide("open");
        let target = wide(url);
        // SAFETY: both strings are NUL-terminated and outlive the call.
        let r = unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                verb.as_ptr(),
                target.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                SW_SHOWNORMAL,
            )
        };
        (r as isize) > 32
    }

    pub fn open_file(path: &Path) -> bool {
        use std::os::windows::ffi::OsStrExt;
        let verb = wide("open");
        let target: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        // SAFETY: both strings are NUL-terminated and outlive the call.
        let r = unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                verb.as_ptr(),
                target.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                SW_SHOWNORMAL,
            )
        };
        (r as isize) > 32
    }

    pub fn reveal_in_file_manager(path: &Path) -> bool {
        use std::os::windows::process::CommandExt;
        // raw_arg avoids Rust's own quoting, which Explorer does not understand.
        std::process::Command::new("explorer.exe")
            .raw_arg(select_argument(path))
            .spawn()
            .is_ok()
    }

    /// Explorer's `/select,"<path>"` argument. Explorer wants backslashes in the path, and the
    /// switch itself keeps its slash: with `\select` Explorer ignores the argument and opens
    /// its default folder.
    pub(super) fn select_argument(path: &Path) -> String {
        let path = path.to_string_lossy().replace('/', "\\");
        format!("/select,\"{path}\"")
    }

    fn reg_dword(subkey: &str, name: &str) -> Option<u32> {
        let sub = wide(subkey);
        let nm = wide(name);
        let mut val: u32 = 0;
        let mut size: u32 = 4;
        // SAFETY: strings are NUL-terminated; val/size are valid for 4 bytes.
        let rc = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                sub.as_ptr(),
                nm.as_ptr(),
                RRF_RT_REG_DWORD,
                std::ptr::null_mut(),
                &mut val as *mut u32 as *mut _,
                &mut size,
            )
        };
        (rc == 0).then_some(val)
    }

    pub fn system_is_dark() -> bool {
        reg_dword(
            "Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize",
            "AppsUseLightTheme",
        ) == Some(0)
    }

    /// Windows is asked directly and has nothing to watch here: the application reads again on
    /// winit's `ThemeChanged` or when its window gains the focus.
    pub fn watch_motion_reduced(_wake: impl Fn() + Send + Sync + 'static) {}

    /// "Animation effects" in Settings, Accessibility, Visual effects (before Windows 11:
    /// "Show animations in Windows"), which is `SPI_GETCLIENTAREAANIMATION`: whether a window
    /// animates what is inside it. Off means less motion is asked for. A call that fails says
    /// nothing was asked.
    pub fn motion_reduced() -> bool {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            SPI_GETCLIENTAREAANIMATION, SystemParametersInfoW,
        };
        let mut animates: i32 = 1;
        // SAFETY: this action writes one BOOL through the pointer, which is to a live i32.
        let ok = unsafe {
            SystemParametersInfoW(
                SPI_GETCLIENTAREAANIMATION,
                0,
                (&mut animates as *mut i32).cast(),
                0,
            )
        };
        ok != 0 && animates == 0
    }

    pub fn double_click_time() -> Duration {
        // SAFETY: no preconditions.
        let ms = unsafe { GetDoubleClickTime() };
        Duration::from_millis(if ms == 0 { 500 } else { ms as u64 })
    }

    pub fn caret_blink_time() -> Option<Duration> {
        // SAFETY: no preconditions.
        let ms = unsafe { GetCaretBlinkTime() };
        match ms {
            0 => Some(Duration::from_millis(530)),
            u32::MAX => None,
            ms => Some(Duration::from_millis(ms as u64)),
        }
    }

    /// The system DPI as a scale factor, for rendering without a window (`--shot`).
    pub fn system_scale() -> f32 {
        use windows_sys::Win32::UI::HiDpi::{
            DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForSystem,
            SetProcessDpiAwarenessContext,
        };
        // SAFETY: no pointers; failure (already set) is harmless.
        unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
        // SAFETY: no preconditions.
        let dpi = unsafe { GetDpiForSystem() };
        if dpi == 0 { 1.0 } else { dpi as f32 / 96.0 }
    }

    /// The system's text antialiasing: greyscale unless font smoothing is on and set to
    /// ClearType, then the ClearType stripe order. A failed query keeps the RGB default.
    pub fn subpixel_order() -> Subpixel {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            FE_FONTSMOOTHINGCLEARTYPE, FE_FONTSMOOTHINGORIENTATIONBGR, SPI_GETFONTSMOOTHING,
            SPI_GETFONTSMOOTHINGORIENTATION, SPI_GETFONTSMOOTHINGTYPE, SystemParametersInfoW,
        };
        let get = |action| {
            let mut value: u32 = 0;
            // SAFETY: each of these actions writes one BOOL or UINT (4 bytes) to `pvparam`.
            let ok =
                unsafe { SystemParametersInfoW(action, 0, (&mut value as *mut u32).cast(), 0) };
            (ok != 0).then_some(value)
        };
        match get(SPI_GETFONTSMOOTHING) {
            Some(0) => return Subpixel::None,
            None => return Subpixel::Rgb,
            Some(_) => {}
        }
        match get(SPI_GETFONTSMOOTHINGTYPE) {
            Some(FE_FONTSMOOTHINGCLEARTYPE) => {}
            Some(_) => return Subpixel::None,
            None => return Subpixel::Rgb,
        }
        match get(SPI_GETFONTSMOOTHINGORIENTATION) {
            Some(FE_FONTSMOOTHINGORIENTATIONBGR) => Subpixel::Bgr,
            _ => Subpixel::Rgb,
        }
    }

    /// The window rectangle, the client rectangle (screen coordinates) and the monitor's work area,
    /// for checking that a maximized frameless window covers exactly the work area.
    pub fn frame_report(hwnd: isize) -> String {
        use windows_sys::Win32::Foundation::{POINT, RECT};
        use windows_sys::Win32::Graphics::Gdi::{
            ClientToScreen, GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO,
            MonitorFromWindow,
        };
        use windows_sys::Win32::UI::WindowsAndMessaging::{GetClientRect, GetWindowRect};
        let h = hwnd as HWND;
        let mut win = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        let mut client = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        let mut origin = POINT { x: 0, y: 0 };
        let mut info: MONITORINFO = unsafe { std::mem::zeroed() };
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        // SAFETY: all out-pointers reference live locals of the right type.
        unsafe {
            GetWindowRect(h, &mut win);
            GetClientRect(h, &mut client);
            ClientToScreen(h, &mut origin);
            let monitor = MonitorFromWindow(h, MONITOR_DEFAULTTONEAREST);
            GetMonitorInfoW(monitor, &mut info);
        }
        let r = |r: RECT| format!("({},{})-({},{})", r.left, r.top, r.right, r.bottom);
        let client = RECT {
            left: origin.x,
            top: origin.y,
            right: origin.x + client.right,
            bottom: origin.y + client.bottom,
        };
        format!(
            "window {} client {} work {} monitor {}",
            r(win),
            r(client),
            r(info.rcWork),
            r(info.rcMonitor)
        )
    }

    /// The dark title-bar mode. The corners are the apps' setting: `corners::set_window_corners`.
    pub fn style_window(hwnd: isize, dark: bool) {
        let h = hwnd as HWND;
        let dark: u32 = dark as u32;
        // SAFETY: the pointer references a 4-byte local valid for the call; failure is ignored.
        unsafe {
            DwmSetWindowAttribute(h, 20, &dark as *const u32 as *const _, 4);
        }
    }

    /// The Windows build number ("22621"), from the registry; 0 when it cannot be read.
    fn windows_build() -> u32 {
        use windows_sys::Win32::System::Registry::{HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ};
        let sub = wide("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion");
        let nm = wide("CurrentBuildNumber");
        let mut buf = [0u16; 16];
        let mut size: u32 = std::mem::size_of_val(&buf) as u32;
        // SAFETY: strings are NUL-terminated; buf/size describe a live buffer of that many bytes.
        let rc = unsafe {
            RegGetValueW(
                HKEY_LOCAL_MACHINE,
                sub.as_ptr(),
                nm.as_ptr(),
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                buf.as_mut_ptr() as *mut _,
                &mut size,
            )
        };
        if rc != 0 {
            return 0;
        }
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        String::from_utf16_lossy(&buf[..len])
            .trim()
            .parse()
            .unwrap_or(0)
    }

    fn high_contrast() -> bool {
        use windows_sys::Win32::UI::WindowsAndMessaging::SystemParametersInfoW;
        // HIGHCONTRASTW, declared here so the crate needs no further windows-sys feature.
        #[repr(C)]
        struct HighContrast {
            size: u32,
            flags: u32,
            scheme: *mut u16,
        }
        const SPI_GETHIGHCONTRAST: u32 = 0x0042;
        const HCF_HIGHCONTRASTON: u32 = 1;
        let mut hc = HighContrast {
            size: std::mem::size_of::<HighContrast>() as u32,
            flags: 0,
            scheme: std::ptr::null_mut(),
        };
        // SAFETY: hc is a live HIGHCONTRASTW of the size it states.
        let ok = unsafe {
            SystemParametersInfoW(SPI_GETHIGHCONTRAST, hc.size, &mut hc as *mut _ as *mut _, 0)
        };
        ok != 0 && hc.flags & HCF_HIGHCONTRASTON != 0
    }

    fn battery_saver() -> bool {
        use windows_sys::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
        // SAFETY: a zeroed SYSTEM_POWER_STATUS is a valid out-parameter.
        let mut st: SYSTEM_POWER_STATUS = unsafe { std::mem::zeroed() };
        // SAFETY: st is a live out-parameter.
        let ok = unsafe { GetSystemPowerStatus(&mut st) };
        ok != 0 && st.SystemStatusFlag == 1
    }

    pub fn backdrop_available() -> bool {
        super::backdrop_allowed(
            windows_build(),
            reg_dword(
                "Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize",
                "EnableTransparency",
            ),
            high_contrast(),
            battery_saver(),
        )
    }

    pub fn set_backdrop(hwnd: isize, backdrop: Backdrop) -> bool {
        use windows_sys::Win32::Graphics::Dwm::{
            DWM_BB_BLURREGION, DWM_BB_ENABLE, DWM_BLURBEHIND, DwmEnableBlurBehindWindow,
        };
        use windows_sys::Win32::Graphics::Gdi::{CreateRectRgn, DeleteObject};
        let h = hwnd as HWND;
        let on = backdrop != Backdrop::Off;
        // An empty blur region: DWM then honours the alpha of every pixel the window presents
        // (what winit's `with_transparent` does at creation).
        let region = if on {
            // SAFETY: plain GDI call; the region is deleted below.
            unsafe { CreateRectRgn(0, 0, -1, -1) }
        } else {
            std::ptr::null_mut()
        };
        let bb = DWM_BLURBEHIND {
            dwFlags: if on {
                DWM_BB_ENABLE | DWM_BB_BLURREGION
            } else {
                DWM_BB_ENABLE
            },
            fEnable: on as i32,
            hRgnBlur: region,
            fTransitionOnMaximized: 0,
        };
        // DWM_SYSTEMBACKDROP_TYPE: 0 is the system's choice (the state of a new window).
        let kind: u32 = match backdrop {
            Backdrop::Off => 0,
            Backdrop::Mica => 2,
            Backdrop::Acrylic => 3,
        };
        // SAFETY: bb and kind are live for the calls; the region handle is ours.
        let (blur, attr) = unsafe {
            let blur = DwmEnableBlurBehindWindow(h, &bb);
            if !region.is_null() {
                DeleteObject(region);
            }
            let attr = DwmSetWindowAttribute(h, 38, &kind as *const u32 as *const _, 4);
            (blur, attr)
        };
        blur >= 0 && attr >= 0
    }
}

#[cfg(not(windows))]
mod imp {
    use super::*;

    use std::sync::Mutex;

    /// One clipboard for the life of the process. On X11 the text is served by the process that
    /// set it, so dropping the handle after a copy would empty the clipboard.
    static CLIPBOARD: Mutex<Option<arboard::Clipboard>> = Mutex::new(None);

    fn with_clipboard<T>(f: impl FnOnce(&mut arboard::Clipboard) -> Option<T>) -> Option<T> {
        let mut guard = CLIPBOARD.lock().ok()?;
        if guard.is_none() {
            *guard = arboard::Clipboard::new().ok();
        }
        f(guard.as_mut()?)
    }

    pub fn clipboard_text() -> Option<String> {
        with_clipboard(|c| c.get_text().ok())
    }
    pub fn set_clipboard_text(text: &str) -> bool {
        with_clipboard(|c| c.set_text(text).ok()).is_some()
    }
    /// `text/html` on Linux (X11 or Wayland), `public.html` on a Mac, as the clipboard crate
    /// reads them: the whole document a browser put there.
    pub fn clipboard_html() -> Option<String> {
        with_clipboard(|c| c.get().html().ok()).filter(|h| h.len() <= crate::cf_html::MAX_BYTES)
    }
    pub fn set_clipboard_html(html: &str, text: &str) -> bool {
        let document = crate::cf_html::html_document(html);
        with_clipboard(|c| c.set().html(document.as_str(), Some(text)).ok()).is_some()
    }
    /// The program that opens a URL or a file with its default handler.
    const OPENER: &str = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };

    pub fn open_url(url: &str) -> bool {
        has_safe_scheme(url) && std::process::Command::new(OPENER).arg(url).spawn().is_ok()
    }
    pub fn open_file(path: &Path) -> bool {
        std::process::Command::new(OPENER).arg(path).spawn().is_ok()
    }
    /// Finder selects the file itself (`open -R`); the XDG route can only open its folder.
    #[cfg(target_os = "macos")]
    pub fn reveal_in_file_manager(path: &Path) -> bool {
        std::process::Command::new("open")
            .arg("-R")
            .arg(path)
            .spawn()
            .is_ok()
    }
    #[cfg(not(target_os = "macos"))]
    pub fn reveal_in_file_manager(path: &Path) -> bool {
        let dir = path.parent().unwrap_or(path);
        std::process::Command::new("xdg-open")
            .arg(dir)
            .spawn()
            .is_ok()
    }
    /// The system's appearance: `AppleInterfaceStyle` in the global defaults is "Dark" while dark
    /// mode is on (also while "Auto" is in its dark half) and absent otherwise. A read of the
    /// defaults database, cheap enough for every call and safe off the main thread.
    #[cfg(target_os = "macos")]
    pub fn system_is_dark() -> bool {
        use objc2_foundation::{NSString, NSUserDefaults};
        let defaults = NSUserDefaults::standardUserDefaults();
        defaults
            .stringForKey(&NSString::from_str("AppleInterfaceStyle"))
            .is_some_and(|style| style.to_string().eq_ignore_ascii_case("dark"))
    }
    /// The desktop's colour scheme from the settings portal, cached for a second.
    #[cfg(not(target_os = "macos"))]
    pub fn system_is_dark() -> bool {
        use std::sync::Mutex;
        use std::time::Instant;
        static CACHE: Mutex<Option<(Instant, bool)>> = Mutex::new(None);
        let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((at, dark)) = *cache
            && at.elapsed() < Duration::from_secs(1)
        {
            return dark;
        }
        let dark = portal_prefers_dark(Duration::from_millis(500));
        *cache = Some((Instant::now(), dark));
        dark
    }

    /// Reads `org.freedesktop.appearance` `color-scheme` (1 is dark; 0 none, 2 light) from the
    /// settings portal on a helper thread, giving up after `timeout`: a portal that is not running
    /// may be started by the bus, and that can take long. False on any failure.
    #[cfg(not(target_os = "macos"))]
    pub(super) fn portal_prefers_dark(timeout: Duration) -> bool {
        let (tx, rx) = std::sync::mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("mdshared-portal".into())
            .spawn(move || _ = tx.send(read_color_scheme()));
        spawned.is_ok() && rx.recv_timeout(timeout).ok().flatten() == Some(1)
    }

    #[cfg(not(target_os = "macos"))]
    fn read_color_scheme() -> Option<u32> {
        let bus = zbus::blocking::Connection::session().ok()?;
        read_setting(&bus, "org.freedesktop.appearance", "color-scheme", |v| {
            u32::try_from(v).ok()
        })
    }

    /// One value of the settings portal, read by `read` once the variants it is wrapped in are
    /// taken off. `None` when the portal does not answer, does not know the key, or `read`
    /// refuses the value's type.
    #[cfg(not(target_os = "macos"))]
    fn read_setting<T>(
        bus: &zbus::blocking::Connection,
        namespace: &str,
        key: &str,
        read: impl Fn(&zbus::zvariant::Value) -> Option<T>,
    ) -> Option<T> {
        use zbus::zvariant::{OwnedValue, Value};
        let call = |method: &str| {
            bus.call_method(
                Some("org.freedesktop.portal.Desktop"),
                "/org/freedesktop/portal/desktop",
                Some("org.freedesktop.portal.Settings"),
                method,
                &(namespace, key),
            )
        };
        // `ReadOne` (portal version 2) answers with the value; the older `Read` wraps it once more.
        let reply = call("ReadOne").or_else(|_| call("Read")).ok()?;
        let value: OwnedValue = reply.body().deserialize().ok()?;
        let mut value: &Value = &value;
        while let Value::Value(inner) = value {
            value = inner;
        }
        read(value)
    }

    /// macOS: Accessibility, Display, Reduce motion.
    #[cfg(target_os = "macos")]
    pub fn motion_reduced() -> bool {
        objc2_app_kit::NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion()
    }
    /// macOS is asked directly and has nothing to watch here: the application reads again on
    /// winit's `ThemeChanged` or when its window gains the focus.
    #[cfg(target_os = "macos")]
    pub fn watch_motion_reduced(_wake: impl Fn() + Send + Sync + 'static) {}

    /// What the settings portal last said about motion, kept between calls.
    #[cfg(not(target_os = "macos"))]
    static MOTION: LastGood = LastGood::new();

    /// The least time between two readings of the portal that [`motion_reduced`] starts.
    #[cfg(not(target_os = "macos"))]
    const MOTION_EVERY: Duration = Duration::from_secs(1);

    /// The desktop's wish for less motion, as the settings portal last said it. It never waits
    /// for the portal: it gives the last answer at once (false until there has been one) and,
    /// at most once a second, starts a helper thread that asks again. A portal that is slow,
    /// hung or absent costs the caller nothing and leaves the last answer standing.
    #[cfg(not(target_os = "macos"))]
    pub fn motion_reduced() -> bool {
        use std::time::Instant;
        if MOTION.claim_reading(Instant::now(), MOTION_EVERY) {
            let spawned = std::thread::Builder::new()
                .name("mdshared-motion".into())
                .spawn(read_and_watch_motion);
            if spawned.is_err() {
                MOTION.reading_done(Instant::now());
            }
        }
        MOTION.get()
    }

    #[cfg(not(target_os = "macos"))]
    pub fn watch_motion_reduced(wake: impl Fn() + Send + Sync + 'static) {
        MOTION.set_wake(std::sync::Arc::new(wake));
    }

    /// The body of the helper thread: one reading of the portal, and then, if no other thread
    /// is doing so already, listening for the portal's `SettingChanged` signal for as long as
    /// the bus is there, reading again at each change. Every answer goes to [`MOTION`]; no
    /// answer (no bus, no portal, an error) leaves the last one standing.
    #[cfg(not(target_os = "macos"))]
    fn read_and_watch_motion() {
        use std::time::Instant;
        use zbus::zvariant::OwnedValue;
        let bus = zbus::blocking::Connection::session().ok();
        // The one thread that listens subscribes before it reads, so that a change between the
        // reading and the subscription is not lost.
        let watching = bus.is_some() && MOTION.claim_watching();
        let signals = bus.as_ref().filter(|_| watching).and_then(|bus| {
            zbus::blocking::Proxy::new(
                bus,
                "org.freedesktop.portal.Desktop",
                "/org/freedesktop/portal/desktop",
                "org.freedesktop.portal.Settings",
            )
            .and_then(|portal| portal.receive_signal("SettingChanged"))
            .ok()
        });
        MOTION.answer(bus.as_ref().and_then(read_reduced_motion));
        MOTION.reading_done(Instant::now());
        if let (Some(bus), Some(signals)) = (&bus, signals) {
            for change in signals {
                // The signal carries the namespace, the key and the new value. Only the three
                // namespaces read below matter; a signal that cannot be read is taken to.
                let ours = change
                    .body()
                    .deserialize::<(String, String, OwnedValue)>()
                    .map_or(true, |(namespace, _, _)| {
                        MOTION_NAMESPACES.contains(&namespace.as_str())
                    });
                if ours {
                    MOTION.answer(read_reduced_motion(bus));
                }
            }
        }
        if watching {
            // The bus went away, or the portal has no signal: a later reading listens again.
            MOTION.watching_done();
        }
    }

    /// The namespaces of the settings portal that [`read_reduced_motion`] reads.
    #[cfg(not(target_os = "macos"))]
    const MOTION_NAMESPACES: [&str; 3] = [
        "org.freedesktop.appearance",
        "org.gnome.desktop.interface",
        "org.kde.kdeglobals.KDE",
    ];

    /// The first of these the portal answers:
    ///
    /// 1. `org.freedesktop.appearance` `reduced-motion`, the portal's own key (1 asks for less
    ///    motion, 0 has no wish), where the portal is new enough to have it;
    /// 2. `org.gnome.desktop.interface` `enable-animations` (GNOME's Settings, Accessibility,
    ///    Seeing, "Animation effects"; false asks for less), which the GNOME and GTK portals
    ///    pass on;
    /// 3. `org.kde.kdeglobals.KDE` `AnimationDurationFactor` (Plasma's System Settings,
    ///    "Animation speed"; 0 is its "instant" end), which the KDE portal passes on.
    ///
    /// `None` when the portal answers none of them: there is then no new answer, and the last
    /// one is kept.
    #[cfg(not(target_os = "macos"))]
    fn read_reduced_motion(bus: &zbus::blocking::Connection) -> Option<bool> {
        use zbus::zvariant::Value;
        let wish = read_setting(bus, "org.freedesktop.appearance", "reduced-motion", |v| {
            u32::try_from(v).ok()
        });
        if let Some(wish) = wish {
            return Some(wish == 1);
        }
        let animations = read_setting(
            bus,
            "org.gnome.desktop.interface",
            "enable-animations",
            |v| bool::try_from(v).ok(),
        );
        if let Some(on) = animations {
            return Some(!on);
        }
        // KDE stores its settings as text; a portal may also hand the number over as one.
        let factor = read_setting(
            bus,
            "org.kde.kdeglobals.KDE",
            "AnimationDurationFactor",
            |v| match v {
                Value::Str(s) => s.as_str().trim().parse::<f64>().ok(),
                Value::F64(f) => Some(*f),
                _ => None,
            },
        )?;
        Some(factor == 0.0)
    }
    /// macOS: the interval set in Settings (`NSEvent.doubleClickInterval`). Elsewhere 500 ms.
    pub fn double_click_time() -> Duration {
        #[cfg(target_os = "macos")]
        {
            let s = objc2_app_kit::NSEvent::doubleClickInterval();
            if s.is_finite() && s > 0.05 && s < 5.0 {
                return Duration::from_secs_f64(s);
            }
        }
        Duration::from_millis(500)
    }
    /// macOS: `NSTextInsertionPointBlinkPeriodOn` when someone set it (in ms; 0 or less is no
    /// blinking), else the 530 ms used everywhere off Windows.
    pub fn caret_blink_time() -> Option<Duration> {
        #[cfg(target_os = "macos")]
        {
            use objc2_foundation::{NSString, NSUserDefaults};
            let key = NSString::from_str("NSTextInsertionPointBlinkPeriodOn");
            let defaults = NSUserDefaults::standardUserDefaults();
            if defaults.objectForKey(&key).is_some() {
                let ms = defaults.integerForKey(&key);
                return (ms > 0).then(|| Duration::from_millis(ms.min(5000) as u64));
            }
        }
        Some(Duration::from_millis(530))
    }
    pub fn style_window(_hwnd: isize, _dark: bool) {}
    pub fn backdrop_available() -> bool {
        false
    }
    pub fn set_backdrop(_hwnd: isize, _backdrop: Backdrop) -> bool {
        false
    }
    pub fn system_scale() -> f32 {
        1.0
    }
    /// macOS draws text with greyscale smoothing only (subpixel smoothing went in 10.14), and a
    /// Retina screen has no use for it; elsewhere the common RGB stripe order.
    pub fn subpixel_order() -> Subpixel {
        if cfg!(target_os = "macos") {
            Subpixel::None
        } else {
            Subpixel::Rgb
        }
    }
    pub fn frame_report(_hwnd: isize) -> String {
        String::new()
    }
}

/// UTF-8 text from the clipboard, if any.
pub fn clipboard_text() -> Option<String> {
    imp::clipboard_text()
}
/// Puts text on the clipboard. Returns false on failure.
pub fn set_clipboard_text(text: &str) -> bool {
    imp::set_clipboard_text(text)
}
/// The HTML the clipboard holds, for a paste that keeps formatting: on Windows the fragment of
/// its "HTML Format" (what was copied, without the page around it); elsewhere the `text/html` a
/// browser or word processor offered, often a whole document. `None` when it holds no HTML.
pub fn clipboard_html() -> Option<String> {
    imp::clipboard_html()
}
/// Puts HTML and its plain-text equivalent on the clipboard together, so a paste into a word
/// processor or mail keeps the formatting and one into a text field gets the text. `html` is a
/// fragment (no `<html>` or `<body>`). Windows writes CF_HTML; elsewhere `text/html` goes out with
/// the text as its alternative. Returns false when the text could not be placed.
pub fn set_clipboard_html(html: &str, text: &str) -> bool {
    imp::set_clipboard_html(html, text)
}
/// Opens an http(s)/mailto URL with the default handler.
pub fn open_url(url: &str) -> bool {
    imp::open_url(url)
}
/// Opens an existing local file with the system's default app for its type (an HTML file opens
/// in the default browser). Only a file whose extension is one of `extensions` (case does not
/// matter) is opened, so a caller that means to open a document it wrote cannot be turned into
/// launching a program. False when the file is missing, the extension is not listed, or the
/// system could not start the handler.
pub fn open_file(path: &Path, extensions: &[&str]) -> bool {
    let listed = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| extensions.iter().any(|x| x.eq_ignore_ascii_case(e)));
    listed && path.is_file() && imp::open_file(path)
}
/// Opens the file manager with the file selected.
pub fn reveal_in_file_manager(path: &Path) -> bool {
    imp::reveal_in_file_manager(path)
}
/// Whether the system asks for less motion, on every system this crate runs on: what an
/// [`Animator`](crate::anim::Animator) is told through
/// [`set_system_reduced`](crate::anim::Animator::set_system_reduced). The application reads it
/// at start and again when the system says its settings changed (winit's `ThemeChanged` is a
/// good moment, and so is the window gaining the focus); it is cheap enough for that, not for
/// every frame.
///
/// - Windows: Settings, Accessibility, Visual effects, "Animation effects" switched off
///   (`SPI_GETCLIENTAREAANIMATION`).
/// - macOS: System Settings, Accessibility, Display, "Reduce motion".
/// - Linux: the settings portal, asked for its own `reduced-motion` key, then GNOME's
///   `enable-animations`, then Plasma's animation speed at "instant". Without a portal the
///   answer is false.
///
/// It never blocks. On Windows and macOS the system answers at once. On Linux the portal is
/// another process that may be slow, hung or not there, so it is asked on a helper thread and
/// this call gives the last answer it gave (false before the first one), taking only a lock
/// for a moment. Each call starts a new reading, at most once a second, and the first reading
/// stays to listen for the portal's `SettingChanged` signal, so a change of the setting is
/// read when it is made. A reading that fails or never ends leaves the last answer standing:
/// a setting that was on is not taken for off because the portal did not answer in time.
///
/// Because the first call on Linux comes before the first answer, an application that wants
/// the right value from its first frame gives [`watch_motion_reduced`] a wake and reads again
/// when woken.
pub fn motion_reduced() -> bool {
    imp::motion_reduced()
}

/// Has `wake` called whenever the answer of [`motion_reduced`] changes behind the
/// application's back, which is on Linux only: when the helper thread's first reading of the
/// settings portal differs from false, and when the user changes the setting. `wake` runs on
/// that helper thread, so it must only wake the application's loop (send a user event through
/// winit's `EventLoopProxy`); the loop then calls [`motion_reduced`] and passes the answer to
/// its animators. Like the wake of a [`Job`](mdshared_app::job), nothing here looks at what it
/// does. A second call replaces the wake of the first.
///
/// On Windows and macOS nothing changes behind the application's back (the system tells the
/// window, and the application reads again then), so the wake is dropped and never called.
pub fn watch_motion_reduced(wake: impl Fn() + Send + Sync + 'static) {
    imp::watch_motion_reduced(wake)
}

/// An answer from a service that may be slow or absent, kept between askings: the last good
/// answer, who is asking now, and whom to wake when it changes. The part of the Linux
/// [`motion_reduced`] that is plain logic, apart from the bus so that it is tested on every
/// system. Time comes in as an argument.
#[cfg(any(test, all(not(windows), not(target_os = "macos"))))]
struct LastGood {
    held: std::sync::Mutex<Held>,
}

#[cfg(any(test, all(not(windows), not(target_os = "macos"))))]
struct Held {
    /// The last answer; false until there is one.
    value: bool,
    /// A reading is under way.
    reading: bool,
    /// When the last reading ended.
    read_at: Option<std::time::Instant>,
    /// A thread is listening for changes.
    watching: bool,
    wake: Option<std::sync::Arc<dyn Fn() + Send + Sync>>,
}

#[cfg(any(test, all(not(windows), not(target_os = "macos"))))]
impl LastGood {
    const fn new() -> LastGood {
        LastGood {
            held: std::sync::Mutex::new(Held {
                value: false,
                reading: false,
                read_at: None,
                watching: false,
                wake: None,
            }),
        }
    }

    /// The lock, also after a thread panicked while holding it: the state is a few plain
    /// values, all of them valid whatever was half done.
    fn held(&self) -> std::sync::MutexGuard<'_, Held> {
        self.held.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The last answer.
    fn get(&self) -> bool {
        self.held().value
    }

    /// Takes an answer. `None` (the service did not answer) changes nothing: the last answer
    /// stands. An answer that differs from the last one calls the wake, after the lock is let
    /// go, so a wake that reads the value at once does not wait on itself.
    fn answer(&self, answer: Option<bool>) {
        let wake = {
            let mut held = self.held();
            match answer {
                Some(value) if value != held.value => {
                    held.value = value;
                    held.wake.clone()
                }
                _ => None,
            }
        };
        if let Some(wake) = wake {
            wake();
        }
    }

    /// Whether the caller should start a reading now: none is under way, and the last one
    /// ended at least `every` ago (or there was none). When true, the reading is the
    /// caller's, and it says [`reading_done`](Self::reading_done) when it ends. A reading that
    /// never ends is never followed by another: one stuck thread, not one a call.
    fn claim_reading(&self, now: std::time::Instant, every: Duration) -> bool {
        let mut held = self.held();
        let due = held
            .read_at
            .is_none_or(|at| now.saturating_duration_since(at) >= every);
        let claim = !held.reading && due;
        held.reading |= claim;
        claim
    }

    fn reading_done(&self, now: std::time::Instant) {
        let mut held = self.held();
        held.reading = false;
        held.read_at = Some(now);
    }

    /// Whether the caller should listen for changes: no thread does. When true the listening
    /// is the caller's, and it says [`watching_done`](Self::watching_done) if it ever stops.
    fn claim_watching(&self) -> bool {
        !std::mem::replace(&mut self.held().watching, true)
    }

    fn watching_done(&self) {
        self.held().watching = false;
    }

    fn set_wake(&self, wake: std::sync::Arc<dyn Fn() + Send + Sync>) {
        self.held().wake = Some(wake);
    }
}

/// Whether the system asks for less motion (macOS: Accessibility, Display, Reduce motion), so
/// animations stay still. False elsewhere, where the applications ask the system themselves;
/// [`motion_reduced`] asks on every system, and is what new code calls. This one is kept as it
/// was for the applications that already use it.
pub fn reduces_motion() -> bool {
    #[cfg(target_os = "macos")]
    {
        objc2_app_kit::NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion()
    }
    #[cfg(not(target_os = "macos"))]
    false
}

/// Tells the system a document was opened, for its lists of recent items: on macOS the Dock
/// icon's menu and Apple menu, Recent Items. Call it on the main thread. Elsewhere nothing.
pub fn note_recent_document(path: &Path) {
    #[cfg(target_os = "macos")]
    {
        use objc2_app_kit::NSDocumentController;
        use objc2_foundation::{NSString, NSURL};
        let Some(mtm) = objc2::MainThreadMarker::new() else {
            return;
        };
        let Some(path) = path.to_str() else {
            return;
        };
        let url = NSURL::fileURLWithPath(&NSString::from_str(path));
        NSDocumentController::sharedDocumentController(mtm).noteNewRecentDocumentURL(&url);
    }
    #[cfg(not(target_os = "macos"))]
    let _ = path;
}

/// Whether apps should use the dark theme.
pub fn system_is_dark() -> bool {
    imp::system_is_dark()
}
/// The system double-click interval, default 500 ms.
pub fn double_click_time() -> Duration {
    imp::double_click_time()
}
/// The caret blink half-period; None when blinking is disabled.
pub fn caret_blink_time() -> Option<Duration> {
    imp::caret_blink_time()
}
/// The desktop's display scale, without a window.
pub fn system_scale() -> f32 {
    imp::system_scale()
}

/// The subpixel order to antialias UI text with: the system's ClearType setting on Windows, RGB
/// elsewhere.
pub fn subpixel_order() -> Subpixel {
    imp::subpixel_order()
}
/// Window, client and work-area rectangles, for checking the frameless-window geometry.
pub fn frame_report(hwnd: isize) -> String {
    imp::frame_report(hwnd)
}
/// Windows 11 polish for a borderless window (dark title-bar mode; the corners are `corners::set_window_corners`).
pub fn style_window(hwnd: isize, dark: bool) {
    imp::style_window(hwnd, dark)
}

/// The first Windows build with `DWMWA_SYSTEMBACKDROP_TYPE` (Windows 11 22H2).
pub const BACKDROP_MIN_BUILD: u32 = 22621;

/// Whether a system backdrop can be counted on to show behind transparent pixels: the Windows
/// build has one, "Transparency effects" is not switched off (`transparency` is the registry's
/// `EnableTransparency`, `None` when the value is missing, which is the system's default: on),
/// and neither high contrast nor battery saver is on (the system then shows a flat colour of its
/// own choosing). The rule behind [`backdrop_available`], apart so it can be tested.
pub fn backdrop_allowed(
    build: u32,
    transparency: Option<u32>,
    high_contrast: bool,
    battery_saver: bool,
) -> bool {
    build >= BACKDROP_MIN_BUILD && transparency != Some(0) && !high_contrast && !battery_saver
}

/// Whether this system, as it is set up right now, shows a backdrop behind a window that asks for
/// one ([`backdrop_allowed`] of what the system says). Always false off Windows. A few registry
/// and system-parameter reads: call it when the setting is on, not at every start.
pub fn backdrop_available() -> bool {
    imp::backdrop_available()
}

/// Asks the compositor for `backdrop` behind the window, or with `Backdrop::Off` takes it away
/// again. With a backdrop the window's pixels are composed by their alpha: every pixel the
/// application wants opaque must carry alpha 255, and one with alpha 0 shows the backdrop
/// (see [`crate::frost`]). Returns whether the system took the request; false off Windows. Call
/// it only after [`backdrop_available`] said yes: on a system without backdrops a window made
/// transparent shows the desktop itself.
pub fn set_backdrop(hwnd: isize, backdrop: Backdrop) -> bool {
    imp::set_backdrop(hwnd, backdrop)
}

#[cfg(test)]
mod motion_tests {
    use super::*;

    /// The system is asked and answers. What it answers depends on the machine the test runs
    /// on, so only that it answers is held; on Windows and macOS, where the system itself is
    /// asked, also that it answers the same when asked again at once. (On Linux the helper
    /// thread's first answer may come between the two calls.)
    #[test]
    fn the_wish_for_less_motion_is_read_without_failing() {
        let reduced = motion_reduced();
        println!("the system asks for less motion: {reduced}");
        if cfg!(any(windows, target_os = "macos")) {
            assert_eq!(motion_reduced(), reduced);
        }
        // The older call is what it was: macOS's answer, and false elsewhere.
        if !cfg!(target_os = "macos") {
            assert!(!reduces_motion());
        }
    }

    /// The call does not wait for the portal: asked on a thread of its own, many times over, it
    /// has answered long before a portal's time limit could run out even once. Whether a
    /// portal is there or not, and whatever it says.
    #[test]
    fn asking_for_the_wish_never_waits_for_the_system() {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for _ in 0..200 {
                motion_reduced();
            }
            _ = tx.send(());
        });
        rx.recv_timeout(Duration::from_secs(20))
            .expect("two hundred calls took longer than twenty seconds: the call waits");
    }

    /// A wake can be set on every system, and setting it calls nothing by itself on the two
    /// where nothing changes behind the application's back.
    #[test]
    fn a_wake_for_the_wish_can_be_set_and_replaced() {
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        for _ in 0..2 {
            let calls = calls.clone();
            watch_motion_reduced(move || {
                calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            });
        }
        if cfg!(any(windows, target_os = "macos")) {
            motion_reduced();
            assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        }
    }

    #[test]
    fn the_last_good_answer_stands_when_the_service_does_not_answer() {
        let last = LastGood::new();
        assert!(!last.get(), "false before any answer");
        last.answer(None);
        assert!(!last.get());
        last.answer(Some(true));
        assert!(last.get());
        // A reading that failed or timed out is no answer: the setting that was on stays on.
        for _ in 0..3 {
            last.answer(None);
            assert!(last.get(), "a failed reading must not switch the wish off");
        }
        last.answer(Some(false));
        assert!(!last.get());
        last.answer(None);
        assert!(!last.get());
    }

    #[test]
    fn a_wake_is_called_when_the_answer_changes_and_only_then() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};
        let last = Arc::new(LastGood::new());
        let seen = Arc::new(AtomicUsize::new(0));
        // The wake reads the value at once, as an application woken by it will: it must not
        // wait on the lock the answer was stored under.
        let (reader, count) = (last.clone(), seen.clone());
        last.set_wake(Arc::new(move || {
            count.fetch_add(if reader.get() { 100 } else { 1 }, Ordering::SeqCst);
        }));
        let (tx, rx) = std::sync::mpsc::channel();
        let worker = last.clone();
        std::thread::spawn(move || {
            worker.answer(Some(false)); // as it was: no wake
            worker.answer(None); // no answer: no wake
            worker.answer(Some(true)); // changed: woken, and the wake reads true
            worker.answer(Some(true)); // the same again: no wake
            worker.answer(None);
            worker.answer(Some(false)); // changed back: woken, and the wake reads false
            _ = tx.send(());
        });
        rx.recv_timeout(Duration::from_secs(10))
            .expect("the wake waited on the lock of the answer it was called for");
        assert_eq!(seen.load(Ordering::SeqCst), 101);
        // A second wake replaces the first.
        let count = seen.clone();
        last.set_wake(Arc::new(move || {
            count.fetch_add(10_000, Ordering::SeqCst);
        }));
        last.answer(Some(true));
        assert_eq!(seen.load(Ordering::SeqCst), 10_101);
        // Without a wake an answer is just kept.
        let quiet = LastGood::new();
        quiet.answer(Some(true));
        assert!(quiet.get());
    }

    #[test]
    fn one_reading_at_a_time_no_more_than_one_an_interval_and_one_listener() {
        let every = Duration::from_secs(1);
        let start = std::time::Instant::now();
        let at = |ms: u64| start + Duration::from_millis(ms);
        let last = LastGood::new();
        // The first call reads; while that reading is under way no other is started, however
        // long it takes (a hung portal is one stuck thread, not one a call).
        assert!(last.claim_reading(at(0), every));
        for ms in [0, 10, 5_000, 3_600_000] {
            assert!(
                !last.claim_reading(at(ms), every),
                "a second reading at {ms} ms"
            );
        }
        // It ended: the next is due an interval after its end, not before.
        last.reading_done(at(400));
        assert!(!last.claim_reading(at(400), every));
        assert!(!last.claim_reading(at(1_399), every));
        assert!(last.claim_reading(at(1_400), every));
        assert!(!last.claim_reading(at(9_000), every));
        last.reading_done(at(1_450));
        // A clock that reads earlier than the last reading's end starts nothing.
        assert!(!last.claim_reading(at(100), every));
        assert!(last.claim_reading(at(2_450), every));
        // One listener: the first to ask is it, until it says it stopped.
        assert!(last.claim_watching());
        assert!(!last.claim_watching());
        assert!(!last.claim_watching());
        last.watching_done();
        assert!(last.claim_watching());
        // None of this touched the answer.
        assert!(!last.get());
    }
}

#[cfg(test)]
mod backdrop_tests {
    use super::*;

    #[test]
    fn a_backdrop_needs_the_build_and_nothing_switched_off() {
        assert!(backdrop_allowed(22621, Some(1), false, false));
        assert!(backdrop_allowed(26300, None, false, false));
        // Windows 10 and Windows 11 21H2.
        assert!(!backdrop_allowed(19045, Some(1), false, false));
        assert!(!backdrop_allowed(22000, Some(1), false, false));
        assert!(!backdrop_allowed(0, None, false, false));
        // Transparency effects off, high contrast, battery saver.
        assert!(!backdrop_allowed(26300, Some(0), false, false));
        assert!(!backdrop_allowed(26300, Some(1), true, false));
        assert!(!backdrop_allowed(26300, Some(1), false, true));
    }

    #[cfg(not(windows))]
    #[test]
    fn there_is_no_backdrop_off_windows() {
        assert!(!backdrop_available());
        assert!(!set_backdrop(0, Backdrop::Mica));
    }
}

#[cfg(all(test, windows))]
mod explorer_tests {
    use super::*;

    /// The clipboard is one for the whole system: tests that write it and read it back take
    /// this lock, or two of them running at once read each other's text.
    static CLIPBOARD: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn clipboard_lock() -> std::sync::MutexGuard<'static, ()> {
        CLIPBOARD.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn clipboard_round_trip() {
        let _lock = clipboard_lock();
        let s = "İstanbul ı ğ 😀 ok\nsecond line";
        assert!(set_clipboard_text(s));
        assert_eq!(clipboard_text().as_deref(), Some(s));
    }

    #[test]
    fn html_copy_leaves_the_text_readable() {
        let _lock = clipboard_lock();
        assert!(set_clipboard_html(
            "<p><b>bold</b> \u{e9}</p>",
            "bold \u{e9}\nnext"
        ));
        assert_eq!(clipboard_text().as_deref(), Some("bold \u{e9}\nnext"));
        // The HTML went out beside it, in the header's own offsets.
        let cf = imp::clipboard_html_format().expect("the HTML Format is on the clipboard");
        let s = String::from_utf8(cf).unwrap();
        let field = |name: &str| -> usize {
            let at = s.find(name).unwrap() + name.len();
            s[at..at + 10].parse().unwrap()
        };
        assert_eq!(
            &s[field("StartFragment:")..field("EndFragment:")],
            "<p><b>bold</b> \u{e9}</p>"
        );
        assert_eq!(field("EndHTML:"), s.len());
        // And a paste reads the fragment back.
        assert_eq!(
            clipboard_html().as_deref(),
            Some("<p><b>bold</b> \u{e9}</p>")
        );
    }

    #[test]
    fn open_url_refuses_unsafe() {
        assert!(!open_url("file:///c:/x"));
        assert!(!open_url("javascript:alert(1)"));
        assert!(!open_url(""));
    }

    /// The query itself cannot be checked against a known answer (it is the user's setting), but
    /// it must agree with itself and not fall over.
    #[test]
    fn subpixel_order_reads_the_system_setting() {
        let order = subpixel_order();
        eprintln!("system subpixel order: {order:?}");
        assert_eq!(order, subpixel_order());
    }
}

#[cfg(all(test, windows))]
mod tests {
    use std::path::Path;

    #[test]
    fn explorer_select_keeps_its_switch_and_gets_backslashes() {
        assert_eq!(
            super::imp::select_argument(Path::new("C:/docs/a b/read me.md")),
            "/select,\"C:\\docs\\a b\\read me.md\""
        );
    }
}

/// The portal read with a given time limit, for the test: false on any failure, never a hang.
#[cfg(all(test, not(windows), not(target_os = "macos")))]
mod theme_tests {
    use super::*;

    #[test]
    fn the_portal_answer_is_a_bool_within_the_time_limit() {
        let start = std::time::Instant::now();
        let dark = imp::portal_prefers_dark(Duration::from_millis(500));
        let took = start.elapsed();
        println!("portal says dark: {dark}, in {took:?}");
        assert!(took < Duration::from_millis(900));
        // Asking again through the cached entry point agrees.
        assert_eq!(
            system_is_dark(),
            imp::portal_prefers_dark(Duration::from_millis(500))
        );
    }
}

#[cfg(test)]
mod dialog_tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    /// One dialog at a time: a second request is refused with the empty result at once, and a new
    /// one is accepted after the first closes. The dialog itself is replaced by a channel wait.
    #[test]
    fn one_dialog_at_a_time() {
        let (release, wait) = mpsc::channel::<()>();
        let (result, got) = mpsc::channel();
        assert!(run_dialog(
            0,
            move || {
                wait.recv().unwrap();
                7
            },
            move |v| result.send(v).unwrap(),
        ));
        assert!(dialog_open());
        let (refused_tx, refused) = mpsc::channel();
        assert!(!run_dialog(0, || 9, move |v| refused_tx.send(v).unwrap()));
        assert_eq!(refused.try_recv(), Ok(0), "done runs at once, empty");
        release.send(()).unwrap();
        assert_eq!(got.recv_timeout(Duration::from_secs(5)), Ok(7));
        for _ in 0..200 {
            if !dialog_open() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(!dialog_open());
        let (again_tx, again) = mpsc::channel();
        assert!(run_dialog(0, || 5, move |v| again_tx.send(v).unwrap()));
        assert_eq!(again.recv_timeout(Duration::from_secs(5)), Ok(5));
    }
}

#[cfg(test)]
mod open_file_tests {
    use super::*;

    /// Every refusal happens before anything is launched: nothing starts in this test.
    #[test]
    fn open_file_refuses_unlisted_types_and_missing_files() {
        let dir = std::env::temp_dir().join(format!("mdshared-open-file-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("run.bat");
        std::fs::write(&script, "echo hi").unwrap();
        assert!(
            !open_file(&script, &["html", "htm"]),
            "extension not listed"
        );
        assert!(
            !open_file(&dir.join("gone.html"), &["html"]),
            "no such file"
        );
        assert!(!open_file(&dir.join("noext"), &["html"]));
        assert!(!open_file(&script, &[]));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
