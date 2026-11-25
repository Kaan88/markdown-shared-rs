//! The Windows backend: toasts through `ToastNotificationManager`, under an AppUserModelID
//! this registers for the program in the current user's registry.
//!
//! The activation and dismissal events come on a thread of the system's pool and only queue an
//! [`Event`]. They come while the toast object they were registered on lives, so the last
//! [`KEEP`] are kept; a click on an older one in the notification centre says nothing.

use std::collections::VecDeque;
use std::io;
use std::sync::Arc;

use windows::Data::Xml::Dom::XmlDocument;
use windows::Foundation::TypedEventHandler;
use windows::UI::Notifications::{
    ToastActivatedEventArgs, ToastDismissalReason, ToastDismissedEventArgs, ToastNotification,
    ToastNotificationManager, ToastNotifier,
};
use windows::core::{HSTRING, IInspectable, Interface, Ref};

use super::{Event, Id, Identity, Inbox, Notification, activation, toast_xml};

/// How many toasts are kept to hear from.
const KEEP: usize = 32;

/// The group every toast of the program is in; the tag names one within it.
const GROUP: &str = "mdshared";

fn err(e: windows::core::Error) -> io::Error {
    io::Error::other(e)
}

/// A tag as Windows takes it: at most 16 characters on older systems, so a hash of it.
fn tag_of(tag: &str) -> String {
    // FNV-1a: stable across runs, unlike the standard hasher.
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in tag.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}")
}

struct Shown {
    id: Id,
    toast: ToastNotification,
    activated: i64,
    dismissed: i64,
}

impl Shown {
    fn forget(self) {
        _ = self.toast.RemoveActivated(self.activated);
        _ = self.toast.RemoveDismissed(self.dismissed);
    }
}

pub(super) struct Toasts {
    notifier: ToastNotifier,
    inbox: Arc<Inbox>,
    shown: VecDeque<Shown>,
}

impl Toasts {
    pub(super) fn open(identity: &Identity, inbox: Arc<Inbox>) -> io::Result<Toasts> {
        register(identity)?;
        let notifier =
            ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(identity.app_id))
                .map_err(err)?;
        Ok(Toasts {
            notifier,
            inbox,
            shown: VecDeque::new(),
        })
    }

    pub(super) fn show(
        &mut self,
        id: Id,
        replaces: Option<Id>,
        n: &Notification,
    ) -> io::Result<()> {
        let doc = XmlDocument::new().map_err(err)?;
        doc.LoadXml(&HSTRING::from(toast_xml(n))).map_err(err)?;
        let toast = ToastNotification::CreateToastNotification(&doc).map_err(err)?;
        if let Some(tag) = &n.tag {
            // The same tag and group replace the earlier toast in the notification centre.
            toast.SetTag(&HSTRING::from(tag_of(tag))).map_err(err)?;
            toast.SetGroup(&HSTRING::from(GROUP)).map_err(err)?;
        }
        let to = self.inbox.clone();
        let activated = toast
            .Activated(&TypedEventHandler::new(
                move |_: Ref<ToastNotification>, args: Ref<IInspectable>| {
                    let args: ToastActivatedEventArgs = args.ok()?.cast()?;
                    to.push(activation(id, &args.Arguments()?.to_string_lossy()));
                    Ok(())
                },
            ))
            .map_err(err)?;
        let to = self.inbox.clone();
        let dismissed = toast
            .Dismissed(&TypedEventHandler::new(
                move |_: Ref<ToastNotification>, args: Ref<ToastDismissedEventArgs>| {
                    if args.ok()?.Reason()? == ToastDismissalReason::UserCanceled {
                        to.push(Event::Dismissed(id));
                    }
                    Ok(())
                },
            ))
            .map_err(err)?;
        let shown = Shown {
            id,
            toast,
            activated,
            dismissed,
        };
        if let Err(e) = self.notifier.Show(&shown.toast) {
            shown.forget();
            return Err(err(e));
        }
        if let Some(old) = replaces
            && let Some(at) = self.shown.iter().position(|s| s.id == old)
            && let Some(s) = self.shown.remove(at)
        {
            s.forget();
        }
        self.shown.push_back(shown);
        while self.shown.len() > KEEP {
            if let Some(s) = self.shown.pop_front() {
                s.forget();
            }
        }
        Ok(())
    }

    pub(super) fn close(&mut self, id: Id) -> io::Result<()> {
        let Some(at) = self.shown.iter().position(|s| s.id == id) else {
            return Ok(());
        };
        let Some(s) = self.shown.remove(at) else {
            return Ok(());
        };
        let hidden = self.notifier.Hide(&s.toast);
        s.forget();
        hidden.map_err(err)
    }
}

impl Drop for Toasts {
    fn drop(&mut self) {
        for s in self.shown.drain(..) {
            s.forget();
        }
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Writes the program's name and icon under its AppUserModelID, which is what an unpackaged
/// program needs for its toasts to show.
fn register(identity: &Identity) -> io::Result<()> {
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, REG_SZ, RegCloseKey, RegCreateKeyW, RegSetValueExW,
    };
    let id = identity.app_id;
    if id.is_empty()
        || id.len() > 128
        || id.contains(['\\', '/'])
        || id.contains(char::is_whitespace)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "an AppUserModelID is a dotted name of at most 128 characters",
        ));
    }
    let path = wide(&format!("Software\\Classes\\AppUserModelId\\{id}"));
    let mut key: HKEY = std::ptr::null_mut();
    // SAFETY: the path is NUL-terminated and `key` is written by the call; it is closed below.
    let made = unsafe { RegCreateKeyW(HKEY_CURRENT_USER, path.as_ptr(), &mut key) };
    if made != ERROR_SUCCESS {
        return Err(io::Error::from_raw_os_error(made as i32));
    }
    let set = |name: &str, value: &str| {
        let name = wide(name);
        let value = wide(value);
        let bytes = u32::try_from(value.len() * 2).unwrap_or(u32::MAX);
        // SAFETY: `key` is open; both strings are NUL-terminated and `bytes` is the value's
        // length in bytes with its NUL.
        let r =
            unsafe { RegSetValueExW(key, name.as_ptr(), 0, REG_SZ, value.as_ptr().cast(), bytes) };
        if r == ERROR_SUCCESS {
            Ok(())
        } else {
            Err(io::Error::from_raw_os_error(r as i32))
        }
    };
    let mut result = set("DisplayName", identity.name);
    if result.is_ok()
        && let Some(icon) = identity.icon
    {
        result = set("IconUri", &icon.to_string_lossy());
    }
    // SAFETY: opened above and not used after.
    unsafe { RegCloseKey(key) };
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_become_short_and_stable() {
        let a = tag_of("now playing");
        assert_eq!(a.len(), 16);
        assert_eq!(a, tag_of("now playing"));
        assert_ne!(a, tag_of("copy finished"));
    }

    #[test]
    fn the_toasts_xml_is_read_by_the_system() {
        let n = Notification::new("Now playing", "Tom & Jerry <live>")
            .action("next", "Next")
            .tagged("now");
        let doc = XmlDocument::new().unwrap();
        doc.LoadXml(&HSTRING::from(toast_xml(&n))).unwrap();
        assert!(ToastNotification::CreateToastNotification(&doc).is_ok());
    }

    #[test]
    fn a_bad_identity_is_refused_before_the_registry() {
        let bad = Identity {
            name: "x",
            app_id: "a\\b",
            icon: None,
        };
        assert_eq!(
            register(&bad).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
    }
}
