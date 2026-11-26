//! A Win32 clipboard writer needs a window before it calls EmptyClipboard. A private
//! message-only control per writer thread provides one even when the app has no window yet.

use std::{cell::RefCell, ptr::null_mut};
use windows_sys::Win32::{
    Foundation::HWND,
    UI::WindowsAndMessaging::{CreateWindowExW, DestroyWindow, HWND_MESSAGE},
};

struct Owner(HWND);

impl Drop for Owner {
    fn drop(&mut self) {
        // SAFETY: this window was created by this thread and is destroyed on the same thread.
        unsafe { DestroyWindow(self.0) };
    }
}

thread_local! {
    static OWNER: RefCell<Option<Owner>> = const { RefCell::new(None) };
}

pub(super) fn window() -> Option<HWND> {
    OWNER
        .try_with(|owner| {
            if let Some(owner) = owner.borrow().as_ref() {
                return Some(owner.0);
            }
            const CLASS: [u16; 7] = [83, 84, 65, 84, 73, 67, 0]; // Win32's predefined STATIC class
            // SAFETY: CLASS is NUL-terminated and static. The predefined control needs no custom
            // window procedure or creation data. HWND_MESSAGE creates no visible window.
            let hwnd = unsafe {
                CreateWindowExW(
                    0,
                    CLASS.as_ptr(),
                    null_mut(),
                    0,
                    0,
                    0,
                    0,
                    0,
                    HWND_MESSAGE,
                    null_mut(),
                    null_mut(),
                    null_mut(),
                )
            };
            if hwnd.is_null() {
                return None;
            }
            *owner.borrow_mut() = Some(Owner(hwnd));
            Some(hwnd)
        })
        .ok()
        .flatten()
}

/// For a bounded read the caller provides no more than `limit + 1` UTF-16 units. Reject a
/// truncated or oversized value before conversion. UTF-8 can still be longer than UTF-16.
pub(super) fn decode_text(units: &[u16], limit: Option<usize>) -> Option<String> {
    let end = units.iter().position(|&u| u == 0).unwrap_or(units.len());
    if limit.is_some_and(|n| end > n) {
        return None;
    }
    let mut text = String::from_utf16_lossy(&units[..end]);
    if limit.is_none() && text.contains("\r\n") {
        text = text.replace("\r\n", "\n");
    }
    limit.is_none_or(|n| text.len() <= n).then_some(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_utf16_rejects_oversize_in_units_and_utf8_bytes() {
        assert!(decode_text(&[65; 5], Some(4)).is_none());
        assert!(decode_text(&[0x2028, 0], Some(2)).is_none());
        assert_eq!(
            decode_text(&[0x2028, 0], Some(3)).as_deref(),
            Some("\u{2028}")
        );
        assert_eq!(
            decode_text(&[65, 0, 66, 66, 66], Some(1)).as_deref(),
            Some("A")
        );
    }
}
