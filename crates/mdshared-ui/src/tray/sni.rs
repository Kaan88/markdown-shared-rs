//! The Linux backend: a StatusNotifierItem with its menu as `com.canonical.dbusmenu`, served by
//! zbus on a connection of its own and registered with the desktop's StatusNotifierWatcher.
//!
//! The tray reads the properties when it likes and is told of a change by the item's
//! `NewIcon` and `NewToolTip` signals and the menu's `LayoutUpdated`, which this sends itself.
//! The icon is sent as pixels (`IconPixmap`, ARGB in network order), so no icon theme is
//! needed. The menu's items are numbered from 1 in the order shown (0 is the root), anew with
//! each [`Item::set_menu`]; its revision counts the changes.

use std::collections::HashMap;
use std::io;
use std::sync::{Arc, Mutex, PoisonError};

use mdshared_text::image::Image;
use zbus::blocking::{Connection, Proxy, connection};
use zbus::zvariant::{ObjectPath, OwnedValue, Value};
use zbus::{fdo, interface};

use super::{Entry, Event, Identity, Inbox, marked};

const ITEM_PATH: &str = "/StatusNotifierItem";
const MENU_PATH: &str = "/MenuBar";
const ITEM: &str = "org.kde.StatusNotifierItem";
const MENU: &str = "com.canonical.dbusmenu";

fn err(e: zbus::Error) -> io::Error {
    io::Error::other(e)
}

/// An icon as StatusNotifierItem sends it: width, height and ARGB32 in network byte order.
pub(super) type Pixmap = (i32, i32, Vec<u8>);

/// The picture as pixmaps: at 22 and 32 px for a tray that picks the nearest, and as it is.
pub(super) fn pixmaps(image: &Image) -> Vec<Pixmap> {
    let mut out = Vec::new();
    let mut put = |im: &Image| {
        let mut argb = Vec::with_capacity(im.rgba.len());
        for p in im.rgba.as_chunks::<4>().0 {
            argb.extend_from_slice(&[p[3], p[0], p[1], p[2]]);
        }
        out.push((im.width as i32, im.height as i32, argb));
    };
    for size in [22, 32] {
        if image.width != size
            && let Some(s) = image.scaled(size, size)
        {
            put(&s);
        }
    }
    put(image);
    out
}

/// What the item and its menu answer from, shared with zbus's thread.
struct Shown {
    title: String,
    tooltip: String,
    icon: Vec<Pixmap>,
    menu: Vec<Entry>,
    revision: u32,
}

fn lock(shown: &Mutex<Shown>) -> std::sync::MutexGuard<'_, Shown> {
    shown.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The properties of a menu's entry as dbusmenu names them.
pub(super) fn properties(entry: &Entry) -> HashMap<String, OwnedValue> {
    let mut m = HashMap::new();
    let mut put = |k: &str, v: Value<'_>| {
        if let Ok(v) = OwnedValue::try_from(v) {
            m.insert(k.to_string(), v);
        }
    };
    match entry {
        Entry::Separator => put("type", Value::from("separator")),
        Entry::Item {
            text,
            access,
            enabled,
            checked,
            hint,
            ..
        } => {
            put("label", Value::from(marked(text, *access, '_')));
            if !enabled {
                put("enabled", Value::from(false));
            }
            if let Some(on) = checked {
                put("toggle-type", Value::from("checkmark"));
                put("toggle-state", Value::from(i32::from(*on)));
            }
            // dbusmenu's shortcut is a list of key names that trays draw differently or not
            // at all; the hint goes to the accessible description instead.
            if !hint.is_empty() {
                put("accessible-desc", Value::from(hint.as_str()));
            }
        }
    }
    m
}

/// A menu's layout as `GetLayout` answers it: the root (0) with every entry (1, 2, ...) as its
/// child.
pub(super) type Layout = (i32, HashMap<String, OwnedValue>, Vec<OwnedValue>);

/// The root's properties.
fn root_properties() -> HashMap<String, OwnedValue> {
    let mut root = HashMap::new();
    if let Ok(v) = OwnedValue::try_from(Value::from("submenu")) {
        root.insert("children-display".to_string(), v);
    }
    root
}

pub(super) fn layout(menu: &[Entry]) -> Layout {
    let children = menu
        .iter()
        .enumerate()
        .filter_map(|(i, e)| {
            let props: HashMap<String, Value<'_>> = properties(e)
                .into_iter()
                .map(|(k, v)| (k, Value::from(v)))
                .collect();
            let child = (i as i32 + 1, props, Vec::<Value<'_>>::new());
            OwnedValue::try_from(Value::from(child)).ok()
        })
        .collect();
    (0, root_properties(), children)
}

/// The application's id of the menu's entry `n` (counted from 1), if it is an enabled item.
pub(super) fn chosen(menu: &[Entry], n: i32) -> Option<u32> {
    let i = usize::try_from(n).ok()?.checked_sub(1)?;
    match menu.get(i)? {
        Entry::Item { id, enabled, .. } if *enabled => Some(*id),
        _ => None,
    }
}

/// The properties of entry `id` (0 the root) of `menu`.
fn properties_of(menu: &[Entry], id: i32) -> HashMap<String, OwnedValue> {
    if id == 0 {
        return root_properties();
    }
    usize::try_from(id)
        .ok()
        .and_then(|i| i.checked_sub(1))
        .and_then(|i| menu.get(i))
        .map(properties)
        .unwrap_or_default()
}

struct StatusItem {
    id: String,
    shown: Arc<Mutex<Shown>>,
    inbox: Arc<Inbox>,
}

#[interface(name = "org.kde.StatusNotifierItem")]
impl StatusItem {
    fn activate(&self, _x: i32, _y: i32) {
        self.inbox.push(Event::Clicked);
    }

    fn secondary_activate(&self, _x: i32, _y: i32) {
        self.inbox.push(Event::MiddleClicked);
    }

    /// The tray shows the menu itself from `Menu`; nothing to do here.
    fn context_menu(&self, _x: i32, _y: i32) {}

    fn scroll(&self, _delta: i32, _orientation: &str) {}

    #[zbus(property)]
    fn category(&self) -> &str {
        "ApplicationStatus"
    }

    #[zbus(property)]
    fn id(&self) -> String {
        self.id.clone()
    }

    #[zbus(property)]
    fn title(&self) -> String {
        lock(&self.shown).title.clone()
    }

    #[zbus(property)]
    fn status(&self) -> &str {
        "Active"
    }

    #[zbus(property)]
    fn window_id(&self) -> i32 {
        0
    }

    #[zbus(property)]
    fn icon_name(&self) -> &str {
        ""
    }

    #[zbus(property)]
    fn icon_pixmap(&self) -> Vec<Pixmap> {
        lock(&self.shown).icon.clone()
    }

    #[zbus(property)]
    fn tool_tip(&self) -> (String, Vec<Pixmap>, String, String) {
        let s = lock(&self.shown);
        (
            String::new(),
            Vec::new(),
            s.title.clone(),
            s.tooltip.clone(),
        )
    }

    #[zbus(property)]
    fn item_is_menu(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn menu(&self) -> ObjectPath<'static> {
        ObjectPath::from_static_str_unchecked(MENU_PATH)
    }
}

struct DbusMenu {
    shown: Arc<Mutex<Shown>>,
    inbox: Arc<Inbox>,
}

impl DbusMenu {
    fn clicked(&self, id: i32, event_id: &str) {
        if event_id == "clicked"
            && let Some(app_id) = chosen(&lock(&self.shown).menu, id)
        {
            self.inbox.push(Event::Chosen(app_id));
        }
    }
}

#[interface(name = "com.canonical.dbusmenu")]
impl DbusMenu {
    fn get_layout(
        &self,
        parent_id: i32,
        _recursion_depth: i32,
        _property_names: Vec<String>,
    ) -> (u32, Layout) {
        let s = lock(&self.shown);
        if parent_id == 0 {
            return (s.revision, layout(&s.menu));
        }
        // The entries have no children of their own.
        (
            s.revision,
            (parent_id, properties_of(&s.menu, parent_id), Vec::new()),
        )
    }

    fn get_group_properties(
        &self,
        ids: Vec<i32>,
        _property_names: Vec<String>,
    ) -> Vec<(i32, HashMap<String, OwnedValue>)> {
        let s = lock(&self.shown);
        let ids = if ids.is_empty() {
            (0..=s.menu.len() as i32).collect()
        } else {
            ids
        };
        ids.into_iter()
            .map(|id| (id, properties_of(&s.menu, id)))
            .collect()
    }

    fn get_property(&self, id: i32, name: &str) -> fdo::Result<OwnedValue> {
        properties_of(&lock(&self.shown).menu, id)
            .remove(name)
            .ok_or_else(|| fdo::Error::InvalidArgs(format!("no property {name} on {id}")))
    }

    fn event(&self, id: i32, event_id: &str, _data: OwnedValue, _timestamp: u32) {
        self.clicked(id, event_id);
    }

    fn event_group(&self, events: Vec<(i32, String, OwnedValue, u32)>) -> Vec<i32> {
        for (id, event_id, _, _) in events {
            self.clicked(id, &event_id);
        }
        Vec::new()
    }

    fn about_to_show(&self, _id: i32) -> bool {
        false
    }

    fn about_to_show_group(&self, _ids: Vec<i32>) -> (Vec<i32>, Vec<i32>) {
        (Vec::new(), Vec::new())
    }

    #[zbus(property)]
    fn version(&self) -> u32 {
        3
    }

    #[zbus(property)]
    fn text_direction(&self) -> &str {
        "ltr"
    }

    #[zbus(property)]
    fn status(&self) -> &str {
        "normal"
    }

    #[zbus(property)]
    fn icon_theme_path(&self) -> Vec<String> {
        Vec::new()
    }
}

pub(super) struct Item {
    connection: Connection,
    shown: Arc<Mutex<Shown>>,
}

impl Item {
    pub(super) fn open(identity: &Identity, icon: &Image, inbox: Arc<Inbox>) -> io::Result<Item> {
        let shown = Arc::new(Mutex::new(Shown {
            title: identity.name.to_string(),
            tooltip: String::new(),
            icon: pixmaps(icon),
            menu: Vec::new(),
            revision: 1,
        }));
        let name = format!("org.kde.StatusNotifierItem-{}-1", std::process::id());
        let connection = connection::Builder::session()
            .and_then(|b| b.name(name.clone()))
            .and_then(|b| {
                b.serve_at(
                    ITEM_PATH,
                    StatusItem {
                        id: identity.id.to_string(),
                        shown: shown.clone(),
                        inbox: inbox.clone(),
                    },
                )
            })
            .and_then(|b| {
                b.serve_at(
                    MENU_PATH,
                    DbusMenu {
                        shown: shown.clone(),
                        inbox,
                    },
                )
            })
            .and_then(|b| b.build())
            .map_err(err)?;
        let watcher = Proxy::new(
            &connection,
            "org.kde.StatusNotifierWatcher",
            "/StatusNotifierWatcher",
            "org.kde.StatusNotifierWatcher",
        )
        .map_err(err)?;
        watcher
            .call::<_, _, ()>("RegisterStatusNotifierItem", &(name.as_str(),))
            .map_err(|e| {
                io::Error::new(
                    io::ErrorKind::Unsupported,
                    format!("no tray on this desktop (no StatusNotifierWatcher): {e}"),
                )
            })?;
        Ok(Item { connection, shown })
    }

    fn signal(&self, name: &str) -> io::Result<()> {
        self.connection
            .emit_signal(None::<&str>, ITEM_PATH, ITEM, name, &())
            .map_err(err)
    }

    pub(super) fn set_icon(&mut self, icon: &Image) -> io::Result<()> {
        lock(&self.shown).icon = pixmaps(icon);
        self.signal("NewIcon")
    }

    pub(super) fn set_tooltip(&mut self, text: &str) -> io::Result<()> {
        lock(&self.shown).tooltip = text.to_string();
        self.signal("NewToolTip")
    }

    pub(super) fn set_menu(&mut self, menu: &[Entry]) -> io::Result<()> {
        let revision = {
            let mut s = lock(&self.shown);
            if s.menu == menu {
                return Ok(());
            }
            s.menu = menu.to_vec();
            s.revision = s.revision.wrapping_add(1);
            s.revision
        };
        self.connection
            .emit_signal(
                None::<&str>,
                MENU_PATH,
                MENU,
                "LayoutUpdated",
                &(revision, 0i32),
            )
            .map_err(err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn menu() -> Vec<Entry> {
        super::super::entries(&[
            crate::menu::Item::new(7, "&Play"),
            crate::menu::Item::new(8, "Shuffle").checked(true),
            crate::menu::Item {
                separator_before: true,
                enabled: false,
                ..crate::menu::Item::new(9, "Q_uit")
            },
        ])
    }

    #[test]
    fn pixels_go_to_argb_in_network_order_at_the_trays_sizes() {
        let image = Image::new(1, 1, vec![10, 20, 30, 40]).unwrap();
        let p = pixmaps(&image);
        assert_eq!(p.len(), 3);
        assert_eq!(p[2], (1, 1, vec![40, 10, 20, 30]));
        assert_eq!((p[0].0, p[1].0), (22, 32));
    }

    #[test]
    fn the_layout_numbers_the_entries_from_one_under_the_root() {
        let m = menu();
        let (root, props, children) = layout(&m);
        assert_eq!(root, 0);
        assert!(props.contains_key("children-display"));
        assert_eq!(children.len(), 4);
        let label = |e: &Entry| {
            properties(e)
                .get("label")
                .and_then(|v| v.downcast_ref::<String>().ok())
        };
        assert_eq!(label(&m[0]).as_deref(), Some("_Play"));
        assert!(properties(&m[2]).contains_key("type"));
        assert_eq!(label(&m[3]).as_deref(), Some("Q__uit"));
        assert!(properties(&m[3]).contains_key("enabled"));
        assert!(properties(&m[1]).contains_key("toggle-state"));
    }

    #[test]
    fn a_click_names_the_applications_id_and_only_an_enabled_items() {
        let m = menu();
        assert_eq!(chosen(&m, 1), Some(7));
        assert_eq!(chosen(&m, 2), Some(8));
        assert_eq!(chosen(&m, 3), None, "the separator");
        assert_eq!(chosen(&m, 4), None, "disabled");
        assert_eq!(chosen(&m, 0), None);
        assert_eq!(chosen(&m, 99), None);
    }
}
