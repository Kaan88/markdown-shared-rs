//! The Linux backend: the desktop's notification server (`org.freedesktop.Notifications`) on
//! the session bus, through zbus.
//!
//! A notification is sent with `Notify`, which answers the server's number for it; a later one
//! with the same tag passes that number to replace it. The server's `ActionInvoked` and
//! `NotificationClosed` signals are heard on two threads of their own, which turn the server's
//! numbers back into [`Id`]s and queue an [`Event`]; they end at the first signal after the
//! [`Server`] is dropped, or when the bus goes.

use std::collections::HashMap;
use std::io;
use std::sync::{Arc, Mutex, PoisonError, Weak};

use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::Value;

use super::{Event, Id, Identity, Inbox, Notification, Urgency, file_url};

const NAME: &str = "org.freedesktop.Notifications";
const PATH: &str = "/org/freedesktop/Notifications";

/// The action key the specification gives a click on the notification itself.
const DEFAULT: &str = "default";

/// `NotificationClosed`'s reason when the person closed it.
const DISMISSED: u32 = 2;

fn err(e: zbus::Error) -> io::Error {
    io::Error::other(e)
}

/// The server's numbers for the notifications shown, both ways.
#[derive(Default)]
struct Numbers {
    ids: HashMap<u32, Id>,
    numbers: HashMap<Id, u32>,
}

impl Numbers {
    fn insert(&mut self, id: Id, number: u32) {
        if let Some(old) = self.ids.insert(number, id) {
            self.numbers.remove(&old);
        }
        self.numbers.insert(id, number);
    }

    fn remove(&mut self, id: Id) -> Option<u32> {
        let number = self.numbers.remove(&id)?;
        self.ids.remove(&number);
        Some(number)
    }
}

pub(super) struct Server {
    proxy: Proxy<'static>,
    app_name: String,
    app_id: String,
    numbers: Arc<Mutex<Numbers>>,
    /// Kept so that the listening threads end once this is dropped.
    _alive: Arc<()>,
}

/// The `actions` list: key and label in turn, the click on the body first.
pub(super) fn actions(n: &Notification) -> Vec<String> {
    let mut out = vec![DEFAULT.to_string(), String::new()];
    for a in n.shown_actions() {
        out.push(a.id.clone());
        out.push(a.label.clone());
    }
    out
}

/// The `urgency` hint's byte.
pub(super) fn urgency(u: Urgency) -> u8 {
    match u {
        Urgency::Low => 0,
        Urgency::Normal => 1,
        Urgency::Critical => 2,
    }
}

/// The event an `ActionInvoked` key stands for.
pub(super) fn invoked(id: Id, key: &str) -> Event {
    if key == DEFAULT {
        Event::Clicked(id)
    } else {
        Event::Action(id, key.to_string())
    }
}

impl Server {
    pub(super) fn open(identity: &Identity, inbox: Arc<Inbox>) -> io::Result<Server> {
        let bus = Connection::session().map_err(err)?;
        let proxy = Proxy::new(&bus, NAME, PATH, NAME).map_err(err)?;
        // Is there a server at all? Asking starts one that is activated on demand.
        proxy
            .call::<_, _, (String, String, String, String)>("GetServerInformation", &())
            .map_err(err)?;
        let numbers = Arc::new(Mutex::new(Numbers::default()));
        let alive = Arc::new(());
        listen(&proxy, "ActionInvoked", &inbox, &numbers, &alive, |msg| {
            let (number, key) = msg.body().deserialize::<(u32, String)>().ok()?;
            Some((number, Some(key)))
        })?;
        listen(
            &proxy,
            "NotificationClosed",
            &inbox,
            &numbers,
            &alive,
            |msg| {
                let (number, reason) = msg.body().deserialize::<(u32, u32)>().ok()?;
                Some((number, (reason == DISMISSED).then_some(String::new())))
            },
        )?;
        Ok(Server {
            proxy,
            app_name: identity.name.to_string(),
            app_id: identity.app_id.to_string(),
            numbers,
            _alive: alive,
        })
    }

    pub(super) fn show(
        &mut self,
        id: Id,
        replaces: Option<Id>,
        n: &Notification,
    ) -> io::Result<()> {
        let replaces_number = replaces
            .and_then(|r| {
                self.numbers
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .numbers
                    .get(&r)
                    .copied()
            })
            .unwrap_or(0);
        let mut hints: HashMap<&str, Value<'_>> = HashMap::new();
        hints.insert("urgency", Value::from(urgency(n.urgency)));
        if !self.app_id.is_empty() {
            hints.insert("desktop-entry", Value::from(self.app_id.as_str()));
        }
        if n.silent || n.urgency == Urgency::Low {
            hints.insert("suppress-sound", Value::from(true));
        }
        let image = n.image.as_deref().map(file_url);
        if let Some(url) = &image {
            hints.insert("image-path", Value::from(url.as_str()));
        }
        let actions = actions(n);
        let actions: Vec<&str> = actions.iter().map(String::as_str).collect();
        // -1: the server's own timeout; a critical one stays by the specification.
        let number: u32 = self
            .proxy
            .call(
                "Notify",
                &(
                    self.app_name.as_str(),
                    replaces_number,
                    "",
                    n.title.as_str(),
                    n.body.as_str(),
                    actions,
                    hints,
                    -1i32,
                ),
            )
            .map_err(err)?;
        let mut numbers = self.numbers.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(r) = replaces {
            numbers.remove(r);
        }
        numbers.insert(id, number);
        Ok(())
    }

    pub(super) fn close(&mut self, id: Id) -> io::Result<()> {
        let number = self
            .numbers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(id);
        match number {
            Some(n) => self
                .proxy
                .call::<_, _, ()>("CloseNotification", &(n,))
                .map_err(err),
            None => Ok(()),
        }
    }
}

/// Reads a signal: the server's number and, for an event, the key.
type Reader = fn(&zbus::Message) -> Option<(u32, Option<String>)>;

/// Hears `signal` on a thread of its own. `read` gives the server's number and, for an event,
/// `Some(key)`: an action's key for `ActionInvoked`, an empty string for a dismissal.
fn listen(
    proxy: &Proxy<'static>,
    signal: &'static str,
    inbox: &Arc<Inbox>,
    numbers: &Arc<Mutex<Numbers>>,
    alive: &Arc<()>,
    read: Reader,
) -> io::Result<()> {
    // Subscribed here, before any notification is shown, so no answer is missed.
    let signals = proxy.receive_signal(signal).map_err(err)?;
    let inbox = Arc::downgrade(inbox);
    let numbers = numbers.clone();
    let alive: Weak<()> = Arc::downgrade(alive);
    std::thread::Builder::new()
        .name(format!("notify {signal}"))
        .spawn(move || {
            for msg in signals {
                let (Some(inbox), Some(_)) = (inbox.upgrade(), alive.upgrade()) else {
                    break;
                };
                let Some((number, Some(key))) = read(&msg) else {
                    continue;
                };
                let id = {
                    let mut n = numbers.lock().unwrap_or_else(PoisonError::into_inner);
                    let id = n.ids.get(&number).copied();
                    if signal == "NotificationClosed"
                        && let Some(id) = id
                    {
                        n.remove(id);
                    }
                    id
                };
                let Some(id) = id else { continue };
                inbox.push(if signal == "ActionInvoked" {
                    invoked(id, &key)
                } else {
                    Event::Dismissed(id)
                });
            }
        })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_body_click_comes_first_and_buttons_follow_in_pairs() {
        let n = Notification::new("t", "b")
            .action("next", "Next")
            .action("", "no id")
            .action("pause", "Pause");
        assert_eq!(
            actions(&n),
            ["default", "", "next", "Next", "pause", "Pause"]
        );
        assert_eq!(invoked(Id(3), "default"), Event::Clicked(Id(3)));
        assert_eq!(invoked(Id(3), "next"), Event::Action(Id(3), "next".into()));
        assert_eq!(urgency(Urgency::Critical), 2);
    }

    #[test]
    fn numbers_go_both_ways_and_a_reused_number_forgets_the_old_id() {
        let mut n = Numbers::default();
        n.insert(Id(1), 7);
        n.insert(Id(2), 7);
        assert_eq!(n.ids.get(&7), Some(&Id(2)));
        assert_eq!(n.remove(Id(1)), None);
        assert_eq!(n.remove(Id(2)), Some(7));
        assert!(n.ids.is_empty() && n.numbers.is_empty());
    }
}
