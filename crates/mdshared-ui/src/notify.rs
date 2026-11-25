//! System notifications: a toast that says something happened while the window was not looked
//! at (the track changed, the output switched, a copy finished), with buttons where the system
//! has them, and what the person did with it sent back.
//!
//! The model is plain: the application fills a [`Notification`] and gives it to
//! [`Notifier::show`], which answers an [`Id`]. A notification with a [`tag`](Notification::tag)
//! replaces the last one shown with the same tag, so "now playing" is one toast that changes,
//! not a pile. What the person does (a click on the toast, a click on one of its buttons, closing
//! it) arrives on the system's threads, is queued, and the `wake` the application gave is called
//! (send the window an event); the next frame takes the queue with [`Notifier::events`]. There
//! are no callbacks into the application.
//!
//! The backends sit behind the `notify` feature: on Windows a toast through
//! `ToastNotificationManager` (the `windows` crate's WinRT), on Linux the desktop's
//! notification server (`org.freedesktop.Notifications` over zbus, already there for the
//! settings portal). Without the feature, and on macOS for now, [`Notifier::new`] answers
//! `Unsupported` and the application goes on without toasts. The model types are there
//! everywhere, so the application's code does not change with the feature.
//!
//! On Windows a program that is not installed from a package has no identity of its own for
//! toasts. [`Notifier::new`] gives it one: it writes the [`Identity`]'s `app_id` with its name
//! and icon under `HKEY_CURRENT_USER\Software\Classes\AppUserModelId`, as the Windows App SDK
//! does for unpackaged programs, and shows the toasts under it. The key stays after the program
//! ends (it is the program's name in the notification settings); an uninstaller removes it.
//!
//! Status: on Windows the example (`cargo run -p mdshared-ui --features notify --example
//! notify`) was run and its toasts checked in the system's notification history; clicks and
//! buttons were not pressed by a person. The Linux backend is only type-checked (from Windows,
//! and in CI); it is **unverified** against a real notification server.
//!
//! Deliberately not here: macOS (`UNUserNotificationCenter`, which needs a signed bundle; the
//! roadmap keeps it open); activation after the program has ended (Windows's COM activator,
//! the server starting the program again): an event comes only while the [`Notifier`] lives;
//! text fields and progress bars in a toast; and sounds of the application's choosing.

use std::collections::HashMap;
use std::io;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};

#[cfg(all(target_os = "linux", feature = "notify"))]
mod fdo;
#[cfg(all(windows, feature = "notify"))]
mod toast;

/// How much a notification asks for attention.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Urgency {
    /// Shown quietly, if at all: a background job that finished.
    Low,
    #[default]
    Normal,
    /// Stays until the person closes it: a failure they must act on.
    Critical,
}

/// A button on a notification: `id` comes back in [`Event::Action`], `label` is what it says.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Action {
    pub id: String,
    pub label: String,
}

impl Action {
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Action {
        Action {
            id: id.into(),
            label: label.into(),
        }
    }
}

/// What a notification says.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Notification {
    /// The first line, in bold: "Now playing".
    pub title: String,
    /// The text under it; may be empty.
    pub body: String,
    /// A picture shown with it (a cover), as a PNG or JPEG file: both systems read pictures
    /// from files. `None` shows the program's icon.
    pub image: Option<PathBuf>,
    /// Up to three buttons ([`MAX_ACTIONS`]); more are left out.
    pub actions: Vec<Action>,
    /// A notification with the same tag as one shown before replaces it.
    pub tag: Option<String>,
    /// No sound.
    pub silent: bool,
    pub urgency: Urgency,
}

/// The buttons both systems show at most (Windows's limit is five, many Linux servers show
/// three).
pub const MAX_ACTIONS: usize = 3;

impl Notification {
    /// A notification with a title and a body and nothing else.
    pub fn new(title: impl Into<String>, body: impl Into<String>) -> Notification {
        Notification {
            title: title.into(),
            body: body.into(),
            ..Notification::default()
        }
    }

    /// The same with a button added.
    pub fn action(mut self, id: impl Into<String>, label: impl Into<String>) -> Notification {
        self.actions.push(Action::new(id, label));
        self
    }

    /// The same with a tag ([`tag`](Self::tag)).
    pub fn tagged(mut self, tag: impl Into<String>) -> Notification {
        self.tag = Some(tag.into());
        self
    }

    /// The buttons that are shown: the first [`MAX_ACTIONS`] with an id and a label.
    pub fn shown_actions(&self) -> impl Iterator<Item = &Action> {
        self.actions
            .iter()
            .filter(|a| !a.id.is_empty() && !a.label.is_empty())
            .take(MAX_ACTIONS)
    }
}

/// A notification as [`Notifier::show`] named it; the same [`Id`] comes back in its events.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Id(pub u64);

/// What the person did with a notification.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Event {
    /// A click on the notification itself: bring the window forward.
    Clicked(Id),
    /// A click on one of its buttons, by the button's [`Action::id`].
    Action(Id, String),
    /// The person closed it. A notification that times out or is put away by the system is
    /// not closed by the person and says nothing.
    Dismissed(Id),
}

/// How the program is named to the system.
#[derive(Clone, Copy, Debug)]
pub struct Identity<'a> {
    /// The name people read: "mdplay".
    pub name: &'a str,
    /// The program's id. On Windows its AppUserModelID, a dotted name such as
    /// `"Kweb.mdplay"`; on Linux the desktop file's name without `.desktop`, so the server
    /// finds the icon and the settings.
    pub app_id: &'a str,
    /// The program's icon as a file, shown with every notification on Windows (`.png` or
    /// `.ico`); `None` shows a blank square there. Linux takes the icon from the desktop file.
    pub icon: Option<&'a std::path::Path>,
}

/// The queue the system's threads add to. Without a backend nothing adds to it.
pub(crate) struct Inbox {
    events: Mutex<Vec<Event>>,
    #[cfg_attr(
        not(all(any(windows, target_os = "linux"), feature = "notify")),
        allow(dead_code)
    )]
    wake: Box<dyn Fn() + Send + Sync>,
}

impl Inbox {
    #[cfg_attr(
        not(all(any(windows, target_os = "linux"), feature = "notify")),
        allow(dead_code)
    )]
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

#[cfg(all(windows, feature = "notify"))]
type Backend = toast::Toasts;
#[cfg(all(target_os = "linux", feature = "notify"))]
type Backend = fdo::Server;
#[cfg(not(all(any(windows, target_os = "linux"), feature = "notify")))]
type Backend = ();

/// The program's way to the system's notifications.
pub struct Notifier {
    inbox: Arc<Inbox>,
    next: u64,
    /// The last notification shown under each tag, which a new one with the tag replaces.
    tags: HashMap<String, Id>,
    backend: Backend,
}

impl Notifier {
    /// Opens the way. `wake` is called from the system's threads when an event waits; it must
    /// be cheap and must not block. `Unsupported` without the `notify` feature and on macOS;
    /// otherwise the system's error (no session bus, no notification server, the registry
    /// refusing the identity).
    pub fn new(
        identity: &Identity,
        wake: impl Fn() + Send + Sync + 'static,
    ) -> io::Result<Notifier> {
        let inbox = Arc::new(Inbox {
            events: Mutex::new(Vec::new()),
            wake: Box::new(wake),
        });
        Ok(Notifier {
            backend: open(identity, inbox.clone())?,
            inbox,
            next: 1,
            tags: HashMap::new(),
        })
    }

    /// Shows `n` and answers its [`Id`]. With a tag it replaces the last one shown with that
    /// tag (which says nothing more of itself).
    pub fn show(&mut self, n: &Notification) -> io::Result<Id> {
        let id = Id(self.next);
        self.next += 1;
        let replaces = n.tag.as_ref().and_then(|t| self.tags.get(t).copied());
        show(&mut self.backend, id, replaces, n)?;
        if let Some(tag) = &n.tag {
            self.tags.insert(tag.clone(), id);
        }
        Ok(id)
    }

    /// Takes a notification away, if it is still there.
    pub fn close(&mut self, id: Id) -> io::Result<()> {
        self.tags.retain(|_, shown| *shown != id);
        close(&mut self.backend, id)
    }

    /// What the person did since the last call, oldest first.
    pub fn events(&self) -> Vec<Event> {
        self.inbox.take()
    }
}

#[cfg(all(windows, feature = "notify"))]
fn open(identity: &Identity, inbox: Arc<Inbox>) -> io::Result<Backend> {
    toast::Toasts::open(identity, inbox)
}
#[cfg(all(target_os = "linux", feature = "notify"))]
fn open(identity: &Identity, inbox: Arc<Inbox>) -> io::Result<Backend> {
    fdo::Server::open(identity, inbox)
}
#[cfg(not(all(any(windows, target_os = "linux"), feature = "notify")))]
fn open(identity: &Identity, inbox: Arc<Inbox>) -> io::Result<Backend> {
    _ = (identity, inbox);
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "no notifications on this system, or the notify feature is off",
    ))
}

#[cfg(all(any(windows, target_os = "linux"), feature = "notify"))]
fn show(backend: &mut Backend, id: Id, replaces: Option<Id>, n: &Notification) -> io::Result<()> {
    backend.show(id, replaces, n)
}
#[cfg(not(all(any(windows, target_os = "linux"), feature = "notify")))]
fn show(backend: &mut Backend, id: Id, replaces: Option<Id>, n: &Notification) -> io::Result<()> {
    _ = (backend, id, replaces, n);
    Ok(())
}

#[cfg(all(any(windows, target_os = "linux"), feature = "notify"))]
fn close(backend: &mut Backend, id: Id) -> io::Result<()> {
    backend.close(id)
}
#[cfg(not(all(any(windows, target_os = "linux"), feature = "notify")))]
fn close(backend: &mut Backend, id: Id) -> io::Result<()> {
    _ = (backend, id);
    Ok(())
}

/// `s` safe inside XML text and attribute values.
#[cfg_attr(not(all(windows, feature = "notify")), allow(dead_code))]
pub(crate) fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            // XML 1.0 refuses the other control characters.
            '\t' | '\n' | '\r' => out.push(c),
            c if (c as u32) < 0x20 => {}
            c => out.push(c),
        }
    }
    out
}

/// What a click on the toast itself sends back in Windows's activation arguments.
#[cfg_attr(not(all(windows, feature = "notify")), allow(dead_code))]
pub(crate) const CLICK: &str = "click";
/// What a button sends back: this, then the button's id.
#[cfg_attr(not(all(windows, feature = "notify")), allow(dead_code))]
pub(crate) const ACTION: &str = "action:";

/// The event an activation's arguments stand for.
#[cfg_attr(not(all(windows, feature = "notify")), allow(dead_code))]
pub(crate) fn activation(id: Id, arguments: &str) -> Event {
    match arguments.strip_prefix(ACTION) {
        Some(action) => Event::Action(id, action.to_string()),
        None => Event::Clicked(id),
    }
}

/// The toast's XML for Windows: the title and body as two lines of text, the picture as the
/// logo, the buttons, and the sound and how long it stays.
#[cfg_attr(not(all(windows, feature = "notify")), allow(dead_code))]
pub(crate) fn toast_xml(n: &Notification) -> String {
    let scenario = match n.urgency {
        Urgency::Critical => " scenario=\"reminder\"",
        _ => "",
    };
    let mut x =
        format!("<toast launch=\"{CLICK}\"{scenario}><visual><binding template=\"ToastGeneric\">");
    x += &format!("<text>{}</text>", xml_escape(&n.title));
    if !n.body.is_empty() {
        x += &format!("<text>{}</text>", xml_escape(&n.body));
    }
    if let Some(image) = &n.image {
        x += &format!(
            "<image placement=\"appLogoOverride\" src=\"{}\"/>",
            xml_escape(&file_url(image))
        );
    }
    x += "</binding></visual>";
    let mut actions = n.shown_actions().peekable();
    if actions.peek().is_some() || n.urgency == Urgency::Critical {
        x += "<actions>";
        let mut any = false;
        for a in actions {
            any = true;
            x += &format!(
                "<action content=\"{}\" arguments=\"{ACTION}{}\" activationType=\"foreground\"/>",
                xml_escape(&a.label),
                xml_escape(&a.id)
            );
        }
        if !any {
            // A toast that stays needs a button to go: the system's own Dismiss.
            x += "<action content=\"\" arguments=\"dismiss\" activationType=\"system\"/>";
        }
        x += "</actions>";
    }
    if n.silent || n.urgency == Urgency::Low {
        x += "<audio silent=\"true\"/>";
    }
    x += "</toast>";
    x
}

/// A path as a `file:` URL, which both systems take for pictures: `/` separators, and the
/// characters a URL cannot hold escaped.
#[cfg_attr(
    not(all(any(windows, target_os = "linux"), feature = "notify")),
    allow(dead_code)
)]
pub(crate) fn file_url(path: &std::path::Path) -> String {
    let s = path.to_string_lossy().replace('\\', "/");
    let mut out = String::from("file://");
    if !s.starts_with('/') {
        out.push('/');
    }
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'-' | b'_' | b'.' | b'~' | b':' => {
                out.push(b as char)
            }
            b => out += &format!("%{b:02X}"),
        }
    }
    out
}

#[cfg(test)]
#[path = "notify_tests.rs"]
mod tests;
