//! Accessibility: an AccessKit tree built from the same models that draw (roadmap W8; feature
//! `a11y`). On Windows the window's adapter (`accesskit_winit`) serves it to UI Automation, so
//! Narrator reads the widgets and a UIA client (a test rig) finds them by AutomationId and drives
//! them with Invoke, Toggle, SelectionItem, ScrollItem, ExpandCollapse and RangeValue. The tree
//! is plain Rust and builds on every system; only the adapter is Windows-only so far.
//!
//! The application builds the tree each time something it shows changes, the same way it draws:
//! [`Tree::begin`], one [`Tree::add`] per widget (the helpers below make the nodes), then
//! [`Tree::end`]. `end` keeps the nodes the root reaches, compares them with the last tree and
//! returns only the nodes that changed, which is what the adapter wants; [`Tree::full`] is the
//! whole tree for the adapter's first request. Requests from the reader come back as
//! [`Request`]s through [`request`], and the application acts on them as on a click or a key.
//!
//! Coordinates are the toolkit's logical px. The window's root node carries the scale as its
//! transform ([`window`]), so no other node needs to know it. A coordinate that is NaN or
//! infinite counts as 0 ([`bounds`]).
//!
//! **Ids.** A node's id is made from its author id, the name a test finds it by ([`id`]): the
//! same string gives the same id in every frame and every run. UIA's AutomationId is the author
//! id. Rows of a list get ids from the list's id and a key the application gives each row
//! ([`item_id`], [`Row::key`]): the key follows the item, not its position, so after a filter or
//! a sort a client that kept an element still has the same item.
//!
//! **Lists.** [`list`] adds a list and *all* its rows: a row costs one small node, a reader says
//! "5 of 2,000" from the row's position and the list's count, and a UIA client finds any row by
//! its AutomationId and scrolls it into view (ScrollItem). Only the visible rows are drawn, as
//! before. AccessKit has no UIA Scroll or ItemContainer pattern, so realizing only the visible rows
//! would leave a far row unreachable except by stepping; at the sizes these applications have
//! (a few thousand rows) the whole list is cheap. The rows are placed in the list's content
//! coordinates and the list node carries the scroll offset as its transform.
//!
//! A row's bounds are the part of it inside the viewport, and a row wholly outside has none.
//! AccessKit hit-tests a node's children without looking at the node's own bounds, so a row
//! scrolled out of the list would otherwise take the clicks and hovers meant for what is drawn
//! where it would be: the toolbar above the list, or the footer below. Even the row just outside
//! reaches as far as a row's height, more than a column header is tall. A row without bounds
//! still takes ScrollIntoView, and UIA reports an empty rectangle for it (IsOffscreen). So a
//! scroll changes the list node, the rows that come into the viewport or leave it, and the two
//! rows cut by its edges; a filter changes the list node and the rows that appear, or whose
//! position or bounds moved; a change of the list's width changes only the rows in view.
//! [`plain_list`] is a short list whose items' bounds the caller gives, for the items of a group;
//! the caller cuts them the same way.
//!
//! **Selection.** AccessKit turns UIA's SelectionItem `Select`, `AddToSelection` and
//! `RemoveFromSelection` all into the same [`Request::Click`], sent only when the row's selected
//! state would change. They cannot be told apart, so `AddToSelection` and `RemoveFromSelection`
//! are not supported. An application treats a click on an unselected row as `Select` (that row
//! only) and a click on a selected row as nothing, so that `Select` on a selected row keeps it
//! selected.
//!
//! **Modal dialogs.** While a modal dialog is open, mark the window's other top-level nodes
//! hidden ([`Node::set_hidden`]) and keep them in the tree: AccessKit then leaves them, and
//! everything under them, out of what UIA sees, and they come back unchanged when the dialog
//! closes.
//!
//! Deliberately not here:
//! - **The adapter and the window.** `accesskit_winit::Adapter` is made by whoever owns the window
//!   (the application, or the host W1), before the window is first shown.
//! - **AT-SPI and macOS.** The feature turns on the Windows adapter only (Purge's scope). The tree
//!   is the same everywhere; the other adapters are a later row.
//! - **Text ranges.** No UIA Text pattern: text fields expose their value, not their characters.
//! - **Adding to or removing from a selection** through UIA (see above).
//! - **Live regions and announcements.** Not needed yet.

use std::collections::{HashMap, HashSet};
use std::ops::Range;

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

/// The id of the item with key `key` in the container `container` (a list's row): mixed from
/// both, so two lists' rows never share ids in practice.
pub fn item_id(container: NodeId, key: usize) -> NodeId {
    NodeId(
        fnv(
            &(key as u64).to_le_bytes(),
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

/// A toolkit rectangle as AccessKit's. A coordinate or size that is NaN or infinite counts as 0,
/// so nonsense gives an empty or shifted rectangle, never a NaN.
pub fn bounds(r: Rect) -> AkRect {
    let (x, y, w, h) = (finite(r.x), finite(r.y), finite(r.w), finite(r.h));
    AkRect {
        x0: x,
        y0: y,
        x1: x + w,
        y1: y + h,
    }
}

/// `v` as an `f64`, or 0 when it is NaN or infinite.
fn finite(v: f32) -> f64 {
    if v.is_finite() { f64::from(v) } else { 0.0 }
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
    /// node that no parent names is dropped by [`end`](Self::end), so add the parent too. Adding
    /// the same id twice keeps the second node.
    pub fn add(&mut self, id: NodeId, node: Node) {
        self.building.push((id, node));
    }

    /// The node `id` as last added in this frame, to change it after a helper added it (to hide
    /// a [`list`] behind a modal dialog). `None` when it was not added since [`begin`](Self::begin).
    /// The search starts at the last node added.
    pub fn get_mut(&mut self, id: NodeId) -> Option<&mut Node> {
        self.building
            .iter_mut()
            .rev()
            .find(|(i, _)| *i == id)
            .map(|(_, n)| n)
    }

    /// Ends the frame: the update with every node that is new or changed since the last frame,
    /// with `focus` (the root when nothing in the window has the keyboard).
    ///
    /// Only the nodes the root reaches through children are kept, because AccessKit panics on a
    /// new node that no parent names and removes an old one whose parent stops naming it. So a
    /// node left out of a frame, or no longer named, is forgotten here too, and is sent whole
    /// when it comes back. A node that no parent names, a child that was not added this frame, a
    /// child named twice and a missing root are mistakes of the caller: a debug build stops on
    /// them (`debug_assert`); a release build drops the node (or the child from its parent's
    /// list, or makes an empty window the root). A `focus` that is not in the frame is the same
    /// kind of mistake, and a release build sends the root as the focus instead.
    pub fn end(&mut self, focus: NodeId) -> TreeUpdate {
        let mut built: HashMap<NodeId, Node> = self.building.drain(..).collect();
        if let std::collections::hash_map::Entry::Vacant(e) = built.entry(self.root) {
            debug_assert!(false, "a11y: the root {:?} was not added", self.root);
            e.insert(Node::new(Role::Window));
        }
        let mut next = HashMap::with_capacity(built.len());
        let mut claimed = HashSet::from([self.root]);
        let mut stack = vec![self.root];
        while let Some(at) = stack.pop() {
            let Some(mut node) = built.remove(&at) else {
                continue;
            };
            let kids = node.children();
            let mut keep = Vec::with_capacity(kids.len());
            for c in kids {
                // A child still in `built` was added and not yet taken; one already claimed is
                // named twice, and one taken already is an ancestor.
                if built.contains_key(c) && claimed.insert(*c) {
                    keep.push(*c);
                }
            }
            if keep.len() != kids.len() {
                debug_assert!(
                    false,
                    "a11y: {at:?} names a child that was not added, is named twice or is an ancestor"
                );
                node.set_children(keep.clone());
            }
            stack.extend(keep);
            next.insert(at, node);
        }
        debug_assert!(
            built.is_empty(),
            "a11y: {} nodes that no parent names, such as {:?}",
            built.len(),
            built.keys().next()
        );
        let focus = if next.contains_key(&focus) {
            focus
        } else {
            debug_assert!(false, "a11y: the focus {focus:?} is not in the tree");
            self.root
        };
        let nodes = next
            .iter()
            .filter(|(id, node)| self.last.get(id) != Some(node))
            .map(|(id, node)| (*id, node.clone()))
            .collect();
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
    /// A `focus` that is not in it gives the root.
    pub fn full(&self, focus: NodeId) -> TreeUpdate {
        let focus = if self.last.contains_key(&focus) {
            focus
        } else {
            self.root
        };
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
    /// Invoke a button or link, toggle a check box or switch, select a row, pick a tab. For a row
    /// it is any of SelectionItem's `Select`, `AddToSelection` and `RemoveFromSelection` (see the
    /// [module](self)).
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

/// A node with a label and an author id (each left out when ""), and no bounds yet.
fn labelled(role: Role, label: &str, author: &str) -> Node {
    let mut n = Node::new(role);
    if !label.is_empty() {
        n.set_label(label);
    }
    if !author.is_empty() {
        n.set_author_id(author);
    }
    n
}

fn named(role: Role, label: &str, author: &str, r: Rect) -> Node {
    let mut n = labelled(role, label, author);
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

/// A group of other nodes (a pane, a card, an InfoBar), with a name a reader announces ("" for
/// none, when a child already names it).
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
/// indeterminate bar, which has no value. The fraction is clamped to 0..=1, and NaN counts as 0.
pub fn progress(label: &str, author: &str, r: Rect, fraction: Option<f32>) -> Node {
    let mut n = named(Role::ProgressIndicator, label, author, r);
    if let Some(f) = fraction {
        let f = if f.is_nan() { 0.0 } else { f.clamp(0.0, 1.0) };
        n.set_min_numeric_value(0.0);
        n.set_max_numeric_value(100.0);
        n.set_numeric_value(f64::from(f) * 100.0);
    }
    n
}

/// Text a reader reads and a test reads back: a count, a status, a title. It is a `Role::Label`
/// whose value is the text; UIA shows it as a Text element whose Name is that text, with no Value
/// pattern, so a client reads it back as the element's Name.
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
/// the rest of the window cannot be used while it is open; the caller also hides the rest (see
/// the [module](self)). Give it the body text with [`Node::set_description`], which UIA shows as
/// the FullDescription, read after the title.
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
    /// The pitch of a row. NaN, infinite, 0 or less: no row has bounds.
    pub row_h: f32,
    /// How many rows there are.
    pub count: usize,
    /// The scroll offset from the top of the first row. NaN or infinite counts as 0.
    pub scroll: f32,
    /// Whether several rows can be selected.
    pub multiselect: bool,
}

/// One row of a [`list`], as the row closure gives it.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    /// What the row stands for, the same in every frame however the list is filtered or sorted
    /// (an index into the application's items, a database id). The row's node id is
    /// [`item_id`]`(list, key)`. The keys of one list must differ.
    pub key: usize,
    /// The row's name, what a reader says.
    pub label: String,
    /// The row's author id (UIA AutomationId), or "" for none.
    pub author: String,
    /// Whether the row is selected.
    pub selected: bool,
}

/// Adds the list and every one of its rows to `tree`, and returns the list's id (for its parent's
/// children). `row(i)` gives the row at position `i` (0 is the top). The rows are in the list's
/// content coordinates and the list carries the scroll as its transform. A row's bounds are the
/// part of it inside the viewport; a row wholly outside has none (see the [module](self)).
pub fn list(tree: &mut Tree, l: &List, mut row: impl FnMut(usize) -> Row) -> NodeId {
    let list_id = id(l.author);
    let scroll = if l.scroll.is_finite() { l.scroll } else { 0.0 };
    let mut n = Node::new(Role::ListBox);
    n.set_label(l.label);
    n.set_author_id(l.author);
    // In content coordinates; the transform puts it back where it is on screen.
    n.set_bounds(bounds(Rect::new(
        l.area.x,
        l.area.y + scroll,
        l.area.w,
        l.area.h,
    )));
    n.set_transform(Affine::translate((0.0, -f64::from(scroll))));
    // Not `set_clips_children`: AccessKit then hides every row outside the viewport but the
    // nearest one each side, which is the stepping this module is here to avoid.
    if l.multiselect {
        n.set_multiselectable();
    }
    n.set_size_of_set(l.count);
    n.add_action(Action::Focus);
    let seen = seen_rows(l, scroll);
    // The viewport in content coordinates.
    let (top, bottom) = (l.area.y + scroll, l.area.y + scroll + l.area.h);
    let mut ids = Vec::with_capacity(l.count);
    for i in 0..l.count {
        let r = row(i);
        let rid = item_id(list_id, r.key);
        let mut item = labelled(Role::ListBoxOption, &r.label, &r.author);
        if seen.contains(&i) {
            let y0 = (l.area.y + i as f32 * l.row_h).max(top);
            let y1 = (l.area.y + (i + 1) as f32 * l.row_h).min(bottom);
            if y1 > y0 {
                item.set_bounds(bounds(Rect::new(l.area.x, y0, l.area.w, y1 - y0)));
            }
        }
        // AccessKit's position is 0-based; UIA's PositionInSet is one more. The set's size comes
        // from the list.
        item.set_position_in_set(i);
        item.set_selected(r.selected);
        item.add_action(Action::Click);
        item.add_action(Action::ScrollIntoView);
        item.add_action(Action::Focus);
        tree.add(rid, item);
        ids.push(rid);
    }
    n.set_children(ids);
    tree.add(list_id, n);
    list_id
}

/// The positions of the rows the viewport touches, which get bounds.
fn seen_rows(l: &List, scroll: f32) -> Range<usize> {
    let h = l.row_h;
    if !(h.is_finite() && h > 0.0) {
        return 0..0;
    }
    let view = if l.area.h.is_finite() {
        l.area.h.max(0.0)
    } else {
        0.0
    };
    // Row i is [i h, (i + 1) h) from the top of the first row, and the viewport is
    // [scroll, scroll + view): it touches the rows from floor(scroll / h) to before
    // ceil((scroll + view) / h). A scroll above the top (negative) can leave none; the casts
    // saturate.
    let first = (scroll / h).floor().max(0.0) as usize;
    let end = ((scroll + view) / h).ceil().max(0.0) as usize;
    first..end.min(l.count)
}

/// The key of the row a list's item id names, if it is one of the keys 0..`count` of
/// `list_author`'s rows.
pub fn key_of(list_author: &str, count: usize, node: NodeId) -> Option<usize> {
    let list_id = id(list_author);
    // Ids are hashes, so the key is found by recomputing them; a list's rows are few enough.
    (0..count).find(|k| item_id(list_id, *k) == node)
}

/// A map from item ids back to row keys, for a list long enough that [`key_of`]'s search would
/// show. The application maps a key to the row's position itself (it knows its filter and sort).
pub struct RowIndex {
    keys: HashMap<NodeId, usize>,
}

impl RowIndex {
    /// The index of the rows with these keys in the list `list_author`.
    pub fn new(list_author: &str, keys: impl IntoIterator<Item = usize>) -> RowIndex {
        let list_id = id(list_author);
        RowIndex {
            keys: keys.into_iter().map(|k| (item_id(list_id, k), k)).collect(),
        }
    }

    /// The key of the row `node` is, if it is one of them.
    pub fn key(&self, node: NodeId) -> Option<usize> {
        self.keys.get(&node).copied()
    }
}

/// One item of a [`plain_list`].
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    /// What the item stands for, the same in every frame; its node id is
    /// [`item_id`]`(list, key)`. The keys of one list must differ.
    pub key: usize,
    /// The item's name.
    pub label: String,
    /// Its author id, or "" for none.
    pub author: String,
    /// Where it is on screen, cut to what shows it, or `None` when it is out of view (see the
    /// [module](self) on bounds).
    pub bounds: Option<Rect>,
}

/// Adds a plain list (UIA List, items ListItem) and its `items` to `tree`, and returns the list's
/// id: the items of a group, a short read-only list. Each item says where it is ("2 of 6"):
/// AccessKit reads the set's size only from an ancestor, so the list carries it. `label` may be ""
/// for none; `author` is the list's author id and makes its id ([`id`]), so it must differ
/// between lists; `area` is the list's bounds, or `None` for none. The items take no requests.
pub fn plain_list(
    tree: &mut Tree,
    label: &str,
    author: &str,
    area: Option<Rect>,
    items: impl IntoIterator<Item = Item>,
) -> NodeId {
    let list_id = id(author);
    let mut ids = Vec::new();
    for (i, it) in items.into_iter().enumerate() {
        let rid = item_id(list_id, it.key);
        let mut n = labelled(Role::ListItem, &it.label, &it.author);
        if let Some(b) = it.bounds {
            n.set_bounds(bounds(b));
        }
        n.set_position_in_set(i);
        tree.add(rid, n);
        ids.push(rid);
    }
    let mut n = labelled(Role::List, label, author);
    if let Some(a) = area {
        n.set_bounds(bounds(a));
    }
    n.set_size_of_set(ids.len());
    n.set_children(ids);
    tree.add(list_id, n);
    list_id
}

#[cfg(test)]
#[path = "a11y_tests.rs"]
mod tests;
