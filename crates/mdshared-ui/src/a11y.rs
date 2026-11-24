//! Accessibility: an AccessKit tree built from the same models that draw (roadmap W8; feature
//! `a11y`). On Windows the window's adapter (`accesskit_winit`) serves it to UI Automation, so
//! Narrator reads the widgets and a UIA client (a test rig) finds them by AutomationId and drives
//! them with Invoke, Toggle, SelectionItem, ScrollItem, ExpandCollapse and RangeValue.
//!
//! The application builds the tree each time something it shows changes, the same way it draws:
//! [`Tree::begin`], one [`Tree::add`] per widget (the helpers below make the nodes), then
//! [`Tree::end`]. `end` compares with the last tree and returns only the nodes that changed, which
//! is what the adapter wants; [`Tree::full`] is the whole tree for the adapter's first request.
//! Requests from the reader come back as [`Request`]s through [`request`], and the application
//! acts on them as on a click or a key.
//!
//! Coordinates are the toolkit's logical px. The window's root node carries the scale as its
//! transform ([`window`]), so no other node needs to know it.
//!
//! **Ids.** A node's id is made from its author id, the name a test finds it by ([`id`]): the
//! same string gives the same id in every frame and every run. UIA's AutomationId is the author
//! id. Rows of a list get ids from the list's id and their index ([`item_id`]).
//!
//! **Lists.** [`list`] adds a list and *all* its rows: a row costs one small node, a reader says
//! "5 of 2,000" from the row's position and the list's count, and a UIA client finds any row by
//! its AutomationId and scrolls it into view (ScrollItem). Only the visible rows are drawn, as
//! before. AccessKit has no UIA Scroll or ItemContainer pattern, so realizing only the visible rows
//! would leave a far row unreachable except by stepping; at the sizes these applications have
//! (a few thousand rows) the whole list is cheap. The rows are placed in the list's content
//! coordinates and the list node carries the scroll offset as its transform, so scrolling changes
//! one node, not every row.
//!
//! Deliberately not here:
//! - **The adapter and the window.** `accesskit_winit::Adapter` is made by whoever owns the window
//!   (the application, or the host W1), before the window is first shown.
//! - **AT-SPI and macOS.** The feature turns on the Windows adapter only (Purge's scope). The tree
//!   is the same everywhere; the other adapters are a later row.
//! - **Text ranges.** No UIA Text pattern: text fields expose their value, not their characters.
//! - **Live regions and announcements.** Not needed yet.

use std::collections::HashMap;

pub use accesskit;
use accesskit::{
    Action, ActionData, ActionRequest, Affine, Node, NodeId, Rect as AkRect, Role, Toggled, TreeId,
    TreeInfo, TreeUpdate,
};

use crate::gfx::Rect;

/// The toolkit's name in the tree's information.
pub const TOOLKIT: &str = "mdshared-ui";

/// The id of the node whose author id is `author`: FNV-1a over the bytes, never 0. The same string
/// gives the same id every time.
pub fn id(author: &str) -> NodeId {
    NodeId(fnv(author.as_bytes(), 0xcbf2_9ce4_8422_2325).max(1))
}

/// The id of item `index` of the container `container` (a list's row): mixed from both, so two
/// lists' rows never share ids in practice.
pub fn item_id(container: NodeId, index: usize) -> NodeId {
    NodeId(
        fnv(
            &(index as u64).to_le_bytes(),
            container.0 ^ 0x9e37_79b9_7f4a_7c15,
        )
        .max(1),
    )
}

fn fnv(bytes: &[u8], seed: u64) -> u64 {
    let mut h = seed;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// A toolkit rectangle as AccessKit's.
pub fn bounds(r: Rect) -> AkRect {
    AkRect {
        x0: f64::from(r.x),
        y0: f64::from(r.y),
        x1: f64::from(r.x + r.w),
        y1: f64::from(r.y + r.h),
    }
}

/// The tree of one window, kept between frames so that each update carries only what changed.
pub struct Tree {
    root: NodeId,
    toolkit_version: Option<String>,
    last: HashMap<NodeId, Node>,
    building: Vec<(NodeId, Node)>,
    sent_info: bool,
}

impl Tree {
    /// A tree whose root is the node `root` (the window: [`window`]).
    pub fn new(root: NodeId) -> Tree {
        Tree {
            root,
            toolkit_version: None,
            last: HashMap::new(),
            building: Vec::new(),
            sent_info: false,
        }
    }

    /// The root's id.
    pub fn root(&self) -> NodeId {
        self.root
    }

    /// Starts the nodes of a new frame.
    pub fn begin(&mut self) {
        self.building.clear();
    }

    /// One node of the frame. A node's children are named in its own [`Node::set_children`]; a
    /// node that no parent names is dropped by AccessKit, so add the parent too.
    pub fn add(&mut self, id: NodeId, node: Node) {
        self.building.push((id, node));
    }

    /// Ends the frame: the update with every node that is new or changed since the last frame,
    /// with `focus` (the root when nothing in the window has the keyboard). Nodes left out of this
    /// frame are forgotten; AccessKit removes them when their parent no longer names them.
    pub fn end(&mut self, focus: NodeId) -> TreeUpdate {
        let mut next = HashMap::with_capacity(self.building.len());
        let mut nodes = Vec::new();
        for (id, node) in self.building.drain(..) {
            if self.last.get(&id) != Some(&node) {
                nodes.push((id, node.clone()));
            }
            next.insert(id, node);
        }
        self.last = next;
        let tree = if self.sent_info {
            None
        } else {
            self.sent_info = true;
            Some(self.info())
        };
        TreeUpdate {
            nodes,
            tree,
            tree_id: TreeId::ROOT,
            focus,
        }
    }

    /// The whole tree as last ended, for the adapter's first request (an activation handler).
    pub fn full(&self, focus: NodeId) -> TreeUpdate {
        TreeUpdate {
            nodes: self
                .last
                .iter()
                .map(|(id, node)| (*id, node.clone()))
                .collect(),
            tree: Some(self.info()),
            tree_id: TreeId::ROOT,
            focus,
        }
    }

    /// How many nodes the last frame had.
    pub fn len(&self) -> usize {
        self.last.len()
    }

    /// Whether the last frame had no nodes.
    pub fn is_empty(&self) -> bool {
        self.last.is_empty()
    }

    fn info(&self) -> TreeInfo {
        let mut info = TreeInfo::new(self.root);
        info.toolkit_name = Some(TOOLKIT.into());
        info.toolkit_version = self.toolkit_version.clone();
        info
    }
}

/// What a reader or a UIA client asked for, by node. Every request names the node it is for.
#[derive(Clone, Debug, PartialEq)]
pub enum Request {
    /// Invoke a button or link, toggle a check box or switch, select a row, pick a tab.
    Click(NodeId),
    /// Move the keyboard to the node.
    Focus(NodeId),
    /// Scroll the node into view (UIA ScrollItem).
    ScrollIntoView(NodeId),
    /// Open an expander or a drop-down (UIA ExpandCollapse).
    Expand(NodeId),
    /// Close an expander or a drop-down.
    Collapse(NodeId),
    /// Put this text in a field (UIA Value).
    SetValue(NodeId, String),
    /// Set a range's value (UIA RangeValue).
    SetNumber(NodeId, f64),
    /// Anything else, as AccessKit gave it.
    Other(ActionRequest),
}

/// The [`Request`] for an AccessKit action request.
pub fn request(r: ActionRequest) -> Request {
    let id = r.target_node;
    match (r.action, &r.data) {
        (Action::Click, _) => Request::Click(id),
        (Action::Focus, _) => Request::Focus(id),
        (Action::ScrollIntoView, _) => Request::ScrollIntoView(id),
        (Action::Expand, _) => Request::Expand(id),
        (Action::Collapse, _) => Request::Collapse(id),
        (Action::SetValue, Some(ActionData::Value(v))) => Request::SetValue(id, v.to_string()),
        (Action::SetValue, Some(ActionData::NumericValue(v))) => Request::SetNumber(id, *v),
        _ => Request::Other(r),
    }
}

fn named(role: Role, label: &str, author: &str, r: Rect) -> Node {
    let mut n = Node::new(role);
    if !label.is_empty() {
        n.set_label(label);
    }
    if !author.is_empty() {
        n.set_author_id(author);
    }
    n.set_bounds(bounds(r));
    n
}

/// The window, the root: its children are `children`, and its transform is the scale, so that
/// every other node is in logical px.
pub fn window(title: &str, scale: f32, children: Vec<NodeId>) -> Node {
    let mut n = Node::new(Role::Window);
    n.set_label(title);
    n.set_transform(Affine::scale(f64::from(scale)));
    n.set_children(children);
    n
}

/// A group of other nodes (a pane, a card, an InfoBar), with a name a reader announces.
pub fn group(label: &str, author: &str, r: Rect, children: Vec<NodeId>) -> Node {
    let mut n = named(Role::Group, label, author, r);
    n.set_children(children);
    n
}

/// A push button: UIA Invoke.
pub fn button(label: &str, author: &str, r: Rect, disabled: bool) -> Node {
    let mut n = named(Role::Button, label, author, r);
    if disabled {
        n.set_disabled();
    } else {
        n.add_action(Action::Click);
        n.add_action(Action::Focus);
    }
    n
}

/// A link (`link`): UIA Invoke.
pub fn link(label: &str, author: &str, r: Rect, disabled: bool) -> Node {
    let mut n = button(label, author, r, disabled);
    n.set_role(Role::Link);
    n
}

/// A check box (`check`): UIA Toggle. `None` is the mixed state.
pub fn check_box(label: &str, author: &str, r: Rect, on: Option<bool>, disabled: bool) -> Node {
    let mut n = button(label, author, r, disabled);
    n.set_role(Role::CheckBox);
    n.set_toggled(match on {
        Some(true) => Toggled::True,
        Some(false) => Toggled::False,
        None => Toggled::Mixed,
    });
    n
}

/// A toggle switch (`toggle`): UIA Toggle.
pub fn switch(label: &str, author: &str, r: Rect, on: bool, disabled: bool) -> Node {
    let mut n = check_box(label, author, r, Some(on), disabled);
    n.set_role(Role::Switch);
    n
}

/// A drop-down (`dropdown`): UIA ExpandCollapse and Value (the chosen option's label).
pub fn combo_box(label: &str, author: &str, r: Rect, value: &str, open: bool) -> Node {
    let mut n = named(Role::ComboBox, label, author, r);
    n.set_value(value);
    n.set_expanded(open);
    n.add_action(Action::Expand);
    n.add_action(Action::Collapse);
    n.add_action(Action::Focus);
    n
}

/// An expander's header (`expander`): a button with UIA ExpandCollapse.
pub fn expander(label: &str, author: &str, header: Rect, expanded: bool) -> Node {
    let mut n = named(Role::Button, label, author, header);
    n.set_expanded(expanded);
    n.add_action(Action::Expand);
    n.add_action(Action::Collapse);
    n.add_action(Action::Click);
    n.add_action(Action::Focus);
    n
}

/// A progress bar (`slider::draw_progress`): UIA RangeValue from 0 to 100. `None` is an
/// indeterminate bar, which has no value.
pub fn progress(label: &str, author: &str, r: Rect, fraction: Option<f32>) -> Node {
    let mut n = named(Role::ProgressIndicator, label, author, r);
    if let Some(f) = fraction {
        n.set_min_numeric_value(0.0);
        n.set_max_numeric_value(100.0);
        n.set_numeric_value(f64::from(f.clamp(0.0, 1.0)) * 100.0);
    }
    n
}

/// Text a reader reads and a test reads back (UIA Value): a count, a status, a title.
pub fn text(value: &str, author: &str, r: Rect) -> Node {
    let mut n = named(Role::Label, "", author, r);
    n.set_value(value);
    n
}

/// A single-line text field (`textfield`): UIA Value, settable.
pub fn text_input(label: &str, author: &str, r: Rect, value: &str, secret: bool) -> Node {
    let mut n = named(
        if secret {
            Role::PasswordInput
        } else {
            Role::TextInput
        },
        label,
        author,
        r,
    );
    if !secret {
        n.set_value(value);
    }
    n.add_action(Action::Focus);
    n.add_action(Action::SetValue);
    n
}

/// A tab strip or selector bar (`segments::draw_selector`): its tabs are `children`.
pub fn tab_list(label: &str, author: &str, r: Rect, children: Vec<NodeId>) -> Node {
    let mut n = named(Role::TabList, label, author, r);
    n.set_children(children);
    n
}

/// One tab: UIA SelectionItem.
pub fn tab(label: &str, author: &str, r: Rect, selected: bool) -> Node {
    let mut n = named(Role::Tab, label, author, r);
    n.set_selected(selected);
    n.add_action(Action::Click);
    n.add_action(Action::Focus);
    n
}

/// A dialog card (`overlay::dialog_card`): its buttons and content are `children`. `modal` says
/// the rest of the window cannot be used while it is open.
pub fn dialog(title: &str, author: &str, r: Rect, modal: bool, children: Vec<NodeId>) -> Node {
    let mut n = named(Role::Dialog, title, author, r);
    if modal {
        n.set_modal();
    }
    n.set_children(children);
    n
}

/// What [`list`] needs of a list.
pub struct List<'a> {
    /// The list's name ("Programs").
    pub label: &'a str,
    /// Its author id ("ProgramList"); its rows' ids come from it ([`item_id`]).
    pub author: &'a str,
    /// The list's viewport on screen.
    pub area: Rect,
    /// The pitch of a row.
    pub row_h: f32,
    /// How many rows there are.
    pub count: usize,
    /// The scroll offset from the top of the first row.
    pub scroll: f32,
    /// Whether several rows can be selected.
    pub multiselect: bool,
}

/// Adds the list and every one of its rows to `tree`, and returns the list's id (for its parent's
/// children). `row` gives each row's name, author id (or "" for none) and whether it is selected.
/// The rows are in the list's content coordinates and the list carries the scroll as its
/// transform, so a scroll changes only the list's node.
pub fn list(
    tree: &mut Tree,
    l: &List,
    mut row: impl FnMut(usize) -> (String, String, bool),
) -> NodeId {
    let list_id = id(l.author);
    let scroll = f64::from(l.scroll);
    let mut n = Node::new(Role::ListBox);
    n.set_label(l.label);
    n.set_author_id(l.author);
    // In content coordinates; the transform puts it back where it is on screen.
    n.set_bounds(bounds(Rect::new(
        l.area.x,
        l.area.y + l.scroll,
        l.area.w,
        l.area.h,
    )));
    n.set_transform(Affine::translate((0.0, -scroll)));
    // Not `set_clips_children`: AccessKit then hides every row outside the viewport but the
    // nearest one each side, which is the stepping this module is here to avoid.
    if l.multiselect {
        n.set_multiselectable();
    }
    n.set_size_of_set(l.count);
    n.add_action(Action::Focus);
    let ids: Vec<NodeId> = (0..l.count).map(|i| item_id(list_id, i)).collect();
    n.set_children(ids.clone());
    tree.add(list_id, n);
    for (i, rid) in ids.into_iter().enumerate() {
        let (label, author, selected) = row(i);
        let r = Rect::new(l.area.x, l.area.y + i as f32 * l.row_h, l.area.w, l.row_h);
        let mut item = named(Role::ListBoxOption, &label, &author, r);
        // AccessKit's position is 0-based; UIA's PositionInSet is one more. The set's size comes
        // from the list.
        item.set_position_in_set(i);
        item.set_selected(selected);
        item.add_action(Action::Click);
        item.add_action(Action::ScrollIntoView);
        item.add_action(Action::Focus);
        tree.add(rid, item);
    }
    list_id
}

/// The row a list's item id names, if it is one of `list_author`'s `count` rows.
pub fn row_of(list_author: &str, count: usize, node: NodeId) -> Option<usize> {
    let list_id = id(list_author);
    // Ids are hashes, so the row is found by recomputing them; a list's rows are few enough.
    (0..count).find(|i| item_id(list_id, *i) == node)
}

/// A map from item ids back to rows, for a list long enough that [`row_of`]'s search would show.
pub struct RowIndex {
    rows: HashMap<NodeId, usize>,
}

impl RowIndex {
    /// The index of the `count` rows of the list `list_author`.
    pub fn new(list_author: &str, count: usize) -> RowIndex {
        let list_id = id(list_author);
        RowIndex {
            rows: (0..count).map(|i| (item_id(list_id, i), i)).collect(),
        }
    }

    /// The row `node` is, if it is one of them.
    pub fn row(&self, node: NodeId) -> Option<usize> {
        self.rows.get(&node).copied()
    }
}

#[cfg(test)]
#[path = "a11y_tests.rs"]
mod tests;
