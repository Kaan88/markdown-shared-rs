//! An icon in the system's tray, with a tooltip and a menu: for a player or a control panel that
//! keeps running with its window closed.
//!
//! The model is plain: the application makes a [`Tray`] with its icon, gives it a tooltip and a
//! menu of [`menu::Item`]s (the same data its drawn menus take, with `u32` ids) and changes
//! them when they change (the track's name in the tooltip, Play turned into Pause). What the
//! person does arrives on the system's threads, is queued, and the `wake` the application gave
//! is called (send the window an event); the next frame takes the queue with [`Tray::events`]:
//! a click on the icon ([`Event::Clicked`], show or hide the window), a middle click, or an item
//! of the menu chosen ([`Event::Chosen`] with its id). There are no callbacks into the
//! application. The menu is the system's own, drawn by it, so it opens without a window.
//!
//! The backends are hand-written, with no crate of their own: on Windows `Shell_NotifyIconW`
//! with a hidden window of its own on a thread of its own (its message loop hears the icon and
//! shows the menu with `TrackPopupMenuEx`), which puts the icon back when Explorer restarts; on
//! Linux a StatusNotifierItem with its menu as `com.canonical.dbusmenu`, served by zbus on the
//! session bus, registered with the desktop's StatusNotifierWatcher (KDE Plasma, and GNOME with
//! the AppIndicator extension; a desktop without a watcher answers an error). On macOS
//! [`Tray::new`] answers `Unsupported` for now.
//!
//! Status: on Windows the example (`cargo run -p mdshared-ui --example tray`) was run and the
//! icon looked for through the shell; clicks and the menu were not used by a person. The Linux
//! backend is only type-checked (from Windows, and in CI); it is **unverified** against a real
//! tray.
//!
//! Deliberately not here: submenus in the tray's menu (roadmap L1 is the drawn menus'); icons
//! beside the menu's items; balloon messages (notifications are [`crate::notify`]'s); an
//! animated icon; the order among other programs' icons; and macOS (`NSStatusItem`, the
//! roadmap keeps it open).

use std::io;
use std::sync::{Arc, Mutex, PoisonError};

use mdshared_text::image::Image;

use crate::menu::{self, Item};

#[cfg(target_os = "linux")]
mod sni;
#[cfg(windows)]
mod win;

/// What the person did with the icon.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Event {
    /// A click on the icon (or Enter on it from the keyboard): show the window, or hide it.
    Clicked,
    /// A middle click: a player plays or pauses.
    MiddleClicked,
    /// An item of the menu was chosen: its [`Item::id`].
    Chosen(u32),
}

/// How the program is named to the system.
#[derive(Clone, Copy, Debug)]
pub struct Identity<'a> {
    /// The name people read: "mdplay". The tooltip's first line on Linux, and the tooltip on
    /// Windows until one is set.
    pub name: &'a str,
    /// A name for the bus and the tray's settings, letters, digits, `-` and `_`: "mdplay".
    pub id: &'a str,
}

/// The queue the system's threads add to.
pub(crate) struct Inbox {
    events: Mutex<Vec<Event>>,
    #[cfg_attr(not(any(windows, target_os = "linux")), allow(dead_code))]
    wake: Box<dyn Fn() + Send + Sync>,
}

impl Inbox {
    #[cfg_attr(not(any(windows, target_os = "linux")), allow(dead_code))]
    pub(crate) fn push(&self, event: Event) {
        self.events
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(event);
        (self.wake)();
    }

    fn take(&self) -> Vec<Event> {
        std::mem::take(&mut *self.events.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

#[cfg(windows)]
type Backend = win::Icon;
#[cfg(target_os = "linux")]
type Backend = sni::Item;
#[cfg(not(any(windows, target_os = "linux")))]
type Backend = ();

/// The program's icon in the tray. Dropping it takes the icon away.
pub struct Tray {
    inbox: Arc<Inbox>,
    backend: Backend,
}

impl Tray {
    /// Puts `icon` in the tray (any size: it is scaled to the tray's). `wake` is called from the
    /// system's threads when an event waits; it must be cheap and must not block.
    /// `Unsupported` on macOS; otherwise the system's error (no session bus, no tray on the
    /// desktop, the shell refusing the icon).
    pub fn new(
        identity: &Identity,
        icon: &Image,
        wake: impl Fn() + Send + Sync + 'static,
    ) -> io::Result<Tray> {
        let inbox = Arc::new(Inbox {
            events: Mutex::new(Vec::new()),
            wake: Box::new(wake),
        });
        Ok(Tray {
            backend: open(identity, icon, inbox.clone())?,
            inbox,
        })
    }

    /// Changes the icon.
    pub fn set_icon(&mut self, icon: &Image) -> io::Result<()> {
        set_icon(&mut self.backend, icon)
    }

    /// Changes the tooltip: what the icon says when the pointer rests on it ("mdplay — Song").
    pub fn set_tooltip(&mut self, text: &str) -> io::Result<()> {
        set_tooltip(&mut self.backend, text)
    }

    /// Changes the menu the icon opens (a right click). An empty menu opens nothing. The items'
    /// `label` (with `&` for the access key), `hint`, `enabled`, `checked`, `checkable` and
    /// `separator_before` are shown; their `icon` is not.
    pub fn set_menu(&mut self, items: &[Item<u32>]) -> io::Result<()> {
        set_menu(&mut self.backend, items)
    }

    /// What the person did since the last call, oldest first.
    pub fn events(&self) -> Vec<Event> {
        self.inbox.take()
    }
}

#[cfg(windows)]
fn open(identity: &Identity, icon: &Image, inbox: Arc<Inbox>) -> io::Result<Backend> {
    win::Icon::open(identity, icon, inbox)
}
#[cfg(target_os = "linux")]
fn open(identity: &Identity, icon: &Image, inbox: Arc<Inbox>) -> io::Result<Backend> {
    sni::Item::open(identity, icon, inbox)
}
#[cfg(not(any(windows, target_os = "linux")))]
fn open(identity: &Identity, icon: &Image, inbox: Arc<Inbox>) -> io::Result<Backend> {
    _ = (identity, icon, inbox);
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "no tray icon on this system yet",
    ))
}

#[cfg(any(windows, target_os = "linux"))]
fn set_icon(backend: &mut Backend, icon: &Image) -> io::Result<()> {
    backend.set_icon(icon)
}
#[cfg(not(any(windows, target_os = "linux")))]
fn set_icon(backend: &mut Backend, icon: &Image) -> io::Result<()> {
    _ = (backend, icon);
    Ok(())
}

#[cfg(any(windows, target_os = "linux"))]
fn set_tooltip(backend: &mut Backend, text: &str) -> io::Result<()> {
    backend.set_tooltip(text)
}
#[cfg(not(any(windows, target_os = "linux")))]
fn set_tooltip(backend: &mut Backend, text: &str) -> io::Result<()> {
    _ = (backend, text);
    Ok(())
}

#[cfg(any(windows, target_os = "linux"))]
fn set_menu(backend: &mut Backend, items: &[Item<u32>]) -> io::Result<()> {
    backend.set_menu(&entries(items))
}
#[cfg(not(any(windows, target_os = "linux")))]
fn set_menu(backend: &mut Backend, items: &[Item<u32>]) -> io::Result<()> {
    _ = (backend, items);
    Ok(())
}

/// A row of the system's menu: an item, or the line between two.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) enum Entry {
    Separator,
    Item {
        /// The application's id, which comes back in [`Event::Chosen`].
        id: u32,
        /// The label as the menu shows it, without the access mark.
        text: String,
        /// The access key's position in `text`, in chars.
        access: Option<usize>,
        hint: String,
        enabled: bool,
        /// `None` for an item that is not a toggle.
        checked: Option<bool>,
    },
}

/// The menu as the systems take it: the items in order with a separator wherever one asks for
/// one before it (never first, never two in a row).
pub(crate) fn entries(items: &[Item<u32>]) -> Vec<Entry> {
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        if item.separator_before && !matches!(out.last(), None | Some(Entry::Separator)) {
            out.push(Entry::Separator);
        }
        let parsed = menu::parse_access(&item.label);
        let access = parsed.key.map(|k| parsed.text[..k.start].chars().count());
        out.push(Entry::Item {
            id: item.id,
            text: parsed.text,
            access,
            hint: item.hint.clone(),
            enabled: item.enabled,
            checked: (item.checkable || item.checked).then_some(item.checked),
        });
    }
    out
}

/// `text` with `mark` before the access key and the mark itself doubled elsewhere: Windows's
/// `&` and dbusmenu's `_`.
#[cfg_attr(not(any(windows, target_os = "linux")), allow(dead_code))]
pub(crate) fn marked(text: &str, access: Option<usize>, mark: char) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    for (i, c) in text.chars().enumerate() {
        if access == Some(i) {
            out.push(mark);
        }
        if c == mark {
            out.push(mark);
        }
        out.push(c);
    }
    out
}

/// The tooltip cut to `max` UTF-16 units (Windows keeps 127), at a char's edge.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn tooltip_units(text: &str, max: usize) -> Vec<u16> {
    let mut out = Vec::with_capacity(max.min(text.len()));
    for c in text.chars() {
        let mut buf = [0u16; 2];
        let units = c.encode_utf16(&mut buf);
        if out.len() + units.len() > max {
            break;
        }
        out.extend_from_slice(units);
    }
    out
}

#[cfg(test)]
#[path = "tray_tests.rs"]
mod tests;
