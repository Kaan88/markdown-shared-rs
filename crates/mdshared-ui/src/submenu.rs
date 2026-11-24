//! Submenus: an item of a menu that opens another menu at its side, and the cascade of menus
//! that makes. Also the geometry of a menu anchored to a rectangle, which keeps a context menu
//! on screen at any corner.
//!
//! A [`Cascade`] is a tree of [`Node`]s (an item and, for an item that opens a submenu, its
//! children) and the menus that are open along one path through it, the root first, each a
//! [`menu::Menu`](crate::menu::Menu) placed [`beside`](crate::menu::Menu::beside) the row that
//! opened it. The application owns the events and the time. It passes the pointer with the
//! time of the event ([`Cascade::pointer`]), a click ([`Cascade::click`]), a key
//! ([`Cascade::key`]) or a wheel turn ([`Cascade::wheel`]), and acts on what comes back: a
//! `bool` that says whether to draw again, or an [`Outcome`] (an item chosen, the cascade to
//! close, a key it did not use). Nothing here reads a clock.
//!
//! The hover delay: resting on a row that opens a submenu opens it [`DELAY`] after the
//! pointer came onto the row, and resting on another row of a menu whose submenu is open closes
//! or replaces that submenu after the same delay, so a pointer that only crosses a row changes
//! nothing. The application asks [`Cascade::deadline`] when that is due, wakes then
//! (`ControlFlow::WaitUntil`, or [`Animator::wake_at`]) and calls [`Cascade::tick`].
//!
//! The aim test: a pointer that leaves the row of an open submenu and crosses other rows on
//! its way to the submenu must not close it. While the pointer moves inside the triangle whose
//! point is where it last was not on such a way ([`aims_at`]) and whose base is the near edge
//! of the submenu, the open row stays lit and nothing else is lit; only once the pointer rests
//! there for the delay, or leaves the triangle, does the row under it count.
//!
//! Keys: Up and Down move in the menu that has the keys (the one the pointer was last in, or
//! the one Right went into), closing any submenu of it; Right, Enter and Space on a row that
//! opens a submenu open it at once and move the keys into it; Left and Escape close the
//! innermost submenu and give the keys back to its menu; Escape in the root is
//! [`Outcome::Close`]. Left in the root and Right on a row without a submenu are
//! [`Outcome::Ignored`], so an application with a menu bar moves to the menu beside.
//!
//! The geometry, as plain functions of rectangles: [`around`] places a menu by a rectangle or
//! a point (below and to the right, flipped above or to the left where it would not fit) and
//! [`beside`] places a submenu by its row (to the right, flipped to the left, slid up to stay
//! inside the area).
//!
//! Deliberately not here: right-to-left (submenus open to the right first and the arrow
//! points right); changing the tree of an open cascade (make a new one: the tree is what it
//! was made with); a submenu leaving with an animation (it goes at once, as on Windows; only
//! its entrance plays, in [`Cascade::draw_animated`]); scrolling a tall menu by resting on
//! arrows at its ends (the wheel scrolls it); a menu bar (the application keeps one cascade per
//! open menu and moves on [`Outcome::Ignored`]); a submenu that opens with no delay at all,
//! other than by [`Cascade::with_delay`] of zero.

use std::time::{Duration, Instant};

use crate::anim::{Animator, AsId, fluent, pop};
use crate::gfx::{Painter, Rect};
use crate::keys::KeyName;
use crate::menu::{Around, Item, MENU_DROP, Menu, PAD};
use crate::theme::Palette;

/// How long the pointer rests on a row before its submenu opens, or on another row before the
/// open submenu closes or gives way: Windows' own default (`SPI_GETMENUSHOWDELAY`).
pub const DELAY: Duration = Duration::from_millis(400);

/// The gap between a menu and a submenu beside it, in logical px.
pub const GAP: f32 = 2.0;

/// How far from the edges of its area a placed menu stays, in logical px.
pub const MARGIN: f32 = 4.0;

/// How much the base of the aim triangle reaches past the submenu's top and bottom, so a
/// pointer aimed at its first or last row is not cut off by a pixel.
const SLACK: f32 = 6.0;

/// The longest delay [`Cascade::with_delay`] takes.
const LONGEST: Duration = Duration::from_secs(10);

/// Where [`around`] or [`beside`] put a menu.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    /// The menu's rectangle: as tall as the menu, or as tall as the area allows for a menu that
    /// is taller (it then scrolls).
    pub rect: Rect,
    /// The menu went to the left: flipped by `around`, or on the left side by `beside`.
    pub left: bool,
    /// The menu went above its anchor (`around` only).
    pub above: bool,
}

/// The part of `area` a menu may use: the area less [`MARGIN`] on every side, never of a
/// negative size.
fn room(area: Rect) -> Rect {
    Rect::new(
        area.x + MARGIN,
        area.y + MARGIN,
        (area.w - 2.0 * MARGIN).max(0.0),
        (area.h - 2.0 * MARGIN).max(0.0),
    )
}

/// A size as given, with NaN and negative sizes taken as none.
fn size(v: f32) -> f32 {
    if v.is_nan() { 0.0 } else { v.max(0.0) }
}

/// Whether `len` from `start` lies within `lo..hi`.
fn fits(start: f32, len: f32, lo: f32, hi: f32) -> bool {
    start >= lo && start + len <= hi
}

/// `start` moved as little as it takes to put `len` from it within `lo..hi`; the low end wins
/// when it cannot.
fn push_in(start: f32, len: f32, lo: f32, hi: f32) -> f32 {
    start.min(hi - len).max(lo)
}

/// Places a menu `w` by `h` by `anchor`, inside `area` less [`MARGIN`]: below the anchor with
/// its left edge at the anchor's left. Where it would cross the bottom of the area it goes
/// above the anchor (its bottom at the anchor's top), and where it would cross the right it
/// goes to the left (its right edge at the anchor's right). A point is an anchor of no size.
///
/// On an axis where it fits on neither side, the menu goes to the side with more room and is
/// pushed inside the area, over the anchor; a menu taller than the area is as tall as the area
/// (and scrolls), one wider is as wide. NaN and negative sizes are none; an area too small for
/// its margins has no room, and the menu is put at its inner corner with no size.
pub fn around(w: f32, h: f32, anchor: Rect, area: Rect) -> Placement {
    let room = room(area);
    let w = size(w).min(room.w);
    let h = size(h);
    let shown = h.min(room.h);
    let (lo, hi) = (room.x, room.right());
    let (right, left) = (anchor.x, anchor.right() - w);
    let (x, went_left) = if fits(right, w, lo, hi) {
        (right, false)
    } else if fits(left, w, lo, hi) {
        (left, true)
    } else {
        let more_left = anchor.right() - lo > hi - anchor.x;
        let x = if more_left { left } else { right };
        (push_in(x, w, lo, hi), more_left)
    };
    let (lo, hi) = (room.y, room.bottom());
    let (below, above) = (anchor.bottom(), anchor.y - h);
    let (y, went_up) = if fits(below, h, lo, hi) {
        (below, false)
    } else if fits(above, h, lo, hi) {
        (above, true)
    } else {
        let more_above = anchor.y - lo > hi - anchor.bottom();
        let y = if more_above { above } else { below };
        (push_in(y, shown, lo, hi), more_above)
    };
    Placement {
        rect: Rect::new(x, y, w, shown),
        left: went_left,
        above: went_up,
    }
}

/// Places a submenu `w` by `h` beside `item`, the row that opens it as wide as its menu,
/// inside `area` less [`MARGIN`]: [`GAP`] px to the right of it, or to the left first when
/// `leftward`, with its first row level with the item (the menu's padding above the item's
/// top). It goes to the other side when it would leave the area there, and to the side with
/// more room, pushed inside the area over its menu, when it fits on neither. It slides up as far
/// as it takes to end inside the area, and never above the area's top; a submenu taller than
/// the area is as tall as the area and scrolls. NaN and negative sizes are none.
pub fn beside(w: f32, h: f32, item: Rect, area: Rect, leftward: bool) -> Placement {
    let room = room(area);
    let w = size(w).min(room.w);
    let shown = size(h).min(room.h);
    let (lo, hi) = (room.x, room.right());
    let right = (item.right() + GAP, false);
    let left = (item.x - GAP - w, true);
    let order = if leftward {
        [left, right]
    } else {
        [right, left]
    };
    let (x, went_left) = order
        .into_iter()
        .find(|&(x, _)| fits(x, w, lo, hi))
        .unwrap_or_else(|| {
            let more_left = item.x - lo > hi - item.right();
            let x = if more_left { left.0 } else { right.0 };
            (push_in(x, w, lo, hi), more_left)
        });
    let y = push_in(item.y - PAD, shown, room.y, room.bottom());
    Placement {
        rect: Rect::new(x, y, w, shown),
        left: went_left,
        above: false,
    }
}

/// The aim test: whether a pointer that was at `from` and is now at `to` is on its way to
/// `target`, an open submenu. True when `to` lies in the triangle (edges included) whose point
/// is `from` and whose base is the edge of `target` nearer `from`, reaching a few px past its
/// top and bottom. False for an empty target and for any coordinate that is not finite.
pub fn aims_at(from: (f32, f32), to: (f32, f32), target: Rect) -> bool {
    let finite = [
        from.0, from.1, to.0, to.1, target.x, target.y, target.w, target.h,
    ]
    .iter()
    .all(|v| v.is_finite());
    if !finite || target.w <= 0.0 || target.h <= 0.0 {
        return false;
    }
    let near = if target.cx() >= from.0 {
        target.x
    } else {
        target.right()
    };
    let top = (near, target.y - SLACK);
    let bottom = (near, target.bottom() + SLACK);
    let cross = |o: (f32, f32), a: (f32, f32), b: (f32, f32)| {
        (a.0 - o.0) * (b.1 - o.1) - (a.1 - o.1) * (b.0 - o.0)
    };
    let d = [
        cross(from, top, to),
        cross(top, bottom, to),
        cross(bottom, from, to),
    ];
    let negative = d.iter().any(|&v| v < 0.0);
    let positive = d.iter().any(|&v| v > 0.0);
    !(negative && positive)
}

/// One item of a cascade: the menu item, and the items of the submenu it opens. An item with no
/// children is an ordinary item; one with children opens them and is never chosen itself.
#[derive(Clone, Debug)]
pub struct Node<T> {
    pub item: Item<T>,
    pub children: Vec<Node<T>>,
}

impl<T> Node<T> {
    /// An item that opens nothing.
    pub fn new(item: Item<T>) -> Node<T> {
        Node {
            item,
            children: Vec::new(),
        }
    }

    /// An item that opens a submenu of `children`.
    pub fn submenu(item: Item<T>, children: Vec<Node<T>>) -> Node<T> {
        Node { item, children }
    }

    /// Whether the item opens a submenu: it has children.
    pub fn opens(&self) -> bool {
        !self.children.is_empty()
    }
}

impl<T> From<Item<T>> for Node<T> {
    fn from(item: Item<T>) -> Node<T> {
        Node::new(item)
    }
}

/// What a click or a key did to a [`Cascade`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome<T> {
    /// Taken, and nothing changed that is drawn.
    Nothing,
    /// Taken; draw again.
    Changed,
    /// An item was chosen, by a click, Enter, Space or its access key. The application acts on
    /// it and closes the cascade.
    Chosen(T),
    /// Escape with no submenu open: the application closes the cascade.
    Close,
    /// A click outside every open menu: the application closes the cascade (and may pass the
    /// click on to what is under it).
    Outside,
    /// A key the cascade does not use: Left in the root, Right on a row without a submenu, a
    /// letter that is no access key, Tab. The application may move to the next menu of a menu
    /// bar, or handle the key as it would without a menu.
    Ignored,
}

/// A change the pointer asked for, due once it has rested: at `due`, the menu at `level` is
/// lit at `row` and the submenu of that row is the one open (none for `None` or a row that
/// opens nothing).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Pending {
    level: usize,
    row: Option<usize>,
    due: Instant,
}

/// A menu with submenus, and the menus of it that are open. See the module's documentation
/// for the delay, the aim test and the keys.
///
/// Levels count from the root, 0. The menu at level `k + 1` is the submenu of the row
/// `path()[k]` of the menu at level `k`, and that row stays lit while it is open.
pub struct Cascade<T> {
    nodes: Vec<Node<T>>,
    anchor: Rect,
    /// The open menus, the root first; never empty.
    menus: Vec<Menu<T>>,
    /// For each open submenu, the row of the menu before it that opened it.
    path: Vec<usize>,
    /// The level that has the keys.
    focus: usize,
    pending: Option<Pending>,
    /// The point of the aim triangle: where the pointer last was not on its way to a submenu.
    apex: Option<(f32, f32)>,
    /// Where the pointer last was.
    last: Option<(f32, f32)>,
    delay: Duration,
    access: bool,
    access_visible: bool,
}

impl<T: Clone> Cascade<T> {
    /// A cascade of `nodes` whose root menu is anchored to `anchor` and kept on screen at any
    /// corner ([`around`]): a context menu on a selection, a drop-down under a button. Only the
    /// root is open.
    pub fn new(nodes: Vec<Node<T>>, anchor: Rect) -> Cascade<T> {
        let mut cascade = Cascade {
            nodes,
            anchor,
            menus: Vec::new(),
            path: Vec::new(),
            focus: 0,
            pending: None,
            apex: None,
            last: None,
            delay: DELAY,
            access: false,
            access_visible: true,
        };
        cascade.menus.push(cascade.root());
        cascade
    }

    /// A cascade of `nodes` opened at the point `x`, `y` (where a right-click was).
    pub fn at(nodes: Vec<Node<T>>, x: f32, y: f32) -> Cascade<T> {
        Cascade::new(nodes, Rect::new(x, y, 0.0, 0.0))
    }

    /// Reads the labels of every menu of the cascade for access keys
    /// ([`Menu::access_keys`]); off by default. Called on a cascade that is open, it closes
    /// its submenus.
    pub fn access_keys(mut self, on: bool) -> Self {
        self.access = on;
        self.reset();
        self
    }

    /// Whether the marked letters are drawn underlined, in every open menu and in those opened
    /// later ([`Menu::set_access_visible`]).
    pub fn set_access_visible(&mut self, on: bool) {
        self.access_visible = on;
        for menu in &mut self.menus {
            menu.set_access_visible(on);
        }
    }

    /// The same cascade with another hover delay (default [`DELAY`]); a delay longer than ten
    /// seconds is ten seconds.
    pub fn with_delay(mut self, delay: Duration) -> Self {
        self.delay = delay.min(LONGEST);
        self
    }

    fn reset(&mut self) {
        self.menus = vec![self.root()];
        self.path.clear();
        self.focus = 0;
        self.pending = None;
    }

    fn root(&self) -> Menu<T> {
        self.menu_of(&self.nodes).around(self.anchor)
    }

    fn menu_of(&self, nodes: &[Node<T>]) -> Menu<T> {
        let items = nodes.iter().map(|n| n.item.clone()).collect();
        let rows = nodes.iter().enumerate().filter(|(_, n)| n.opens());
        let mut menu = Menu::new(items, 0.0, 0.0, false)
            .access_keys(self.access)
            .submenu_rows(rows.map(|(i, _)| i));
        menu.set_access_visible(self.access_visible);
        menu
    }

    /// The nodes of the menu at the end of `path`.
    fn nodes_at(&self, path: &[usize]) -> &[Node<T>] {
        let mut nodes = &self.nodes[..];
        for &i in path {
            nodes = nodes.get(i).map_or(&[][..], |n| &n.children[..]);
        }
        nodes
    }

    /// How many menus are open: 1 for the root alone.
    pub fn depth(&self) -> usize {
        self.menus.len()
    }

    /// The open menu at `level`, for its [`rect`](Menu::rect), its rows and its highlight.
    pub fn menu(&self, level: usize) -> Option<&Menu<T>> {
        self.menus.get(level)
    }

    /// The open menu at `level`, to change: an item's `enabled` or `checked` while it is open.
    /// Its items stay the ones of the tree; a different number of them is the application's
    /// mistake, which picks nothing stale ([`Menu::chosen`]) but may light the wrong row.
    pub fn menu_mut(&mut self, level: usize) -> Option<&mut Menu<T>> {
        self.menus.get_mut(level)
    }

    /// The rows that opened the open submenus, from the root: empty when only the root is
    /// open.
    pub fn path(&self) -> &[usize] {
        &self.path
    }

    /// The level of the menu that has the keys.
    pub fn focus(&self) -> usize {
        self.focus
    }

    /// When a change the pointer asked for is due ([`tick`](Self::tick)): the application
    /// wakes then. `None` when nothing waits.
    pub fn deadline(&self) -> Option<Instant> {
        self.pending.map(|p| p.due)
    }

    /// Whether `x`, `y` is in any open menu (as last placed).
    pub fn contains(&self, x: f32, y: f32) -> bool {
        self.menus.iter().any(|m| m.contains(x, y))
    }

    /// The deepest open menu under the point: submenus are drawn over the menus before them.
    fn level_at(&self, x: f32, y: f32) -> Option<usize> {
        (0..self.menus.len())
            .rev()
            .find(|&k| self.menus[k].contains(x, y))
    }

    /// The enabled row of the menu at `level` under the point.
    fn row_at(&self, level: usize, x: f32, y: f32) -> Option<usize> {
        let menu = &self.menus[level];
        if !menu.contains(x, y) {
            return None;
        }
        (0..menu.items.len())
            .find(|&i| menu.row_rect(i).contains(x, y))
            .filter(|&i| menu.items[i].enabled)
    }

    /// Whether row `row` of the menu at `level` opens a submenu now: it has children and is
    /// enabled.
    fn opens(&self, level: usize, row: usize) -> bool {
        let enabled = self.menus[level].items.get(row).is_some_and(|i| i.enabled);
        enabled
            && self
                .nodes_at(&self.path[..level])
                .get(row)
                .is_some_and(Node::opens)
    }

    /// Closes every submenu after `level`.
    fn close_after(&mut self, level: usize) {
        self.menus.truncate(level + 1);
        self.path.truncate(level);
        self.focus = self.focus.min(level);
    }

    /// Opens the submenu of row `row` of the menu at `level`, closing whatever was open after
    /// that menu. False, and nothing closed, when the row opens nothing.
    fn open(&mut self, level: usize, row: usize) -> bool {
        if !self.opens(level, row) {
            return false;
        }
        self.close_after(level);
        self.path.push(row);
        let mut menu = self.menu_of(self.nodes_at(&self.path));
        menu.set_around(Around::Beside(Rect::default(), false));
        self.menus.push(menu);
        self.menus[level].hover = Some(row);
        true
    }

    /// Lights the rows that opened the submenus, in the menus before `level`.
    fn hold_path(&mut self, level: usize) {
        for j in 0..level.min(self.path.len()) {
            self.menus[j].hover = Some(self.path[j]);
        }
    }

    /// What is drawn of the state: which submenus are open and which rows are lit.
    fn lit(&self) -> (Vec<usize>, Vec<Option<usize>>) {
        (
            self.path.clone(),
            self.menus.iter().map(|m| m.hover).collect(),
        )
    }

    /// Asks for the menu at `level` to be lit at `row`, with the submenu of that row open,
    /// once the delay has passed. Asked again for the same, it keeps the time first asked, so
    /// a pointer moving within a row does not put its submenu off.
    fn schedule(&mut self, level: usize, row: Option<usize>, now: Instant) {
        if self
            .pending
            .is_some_and(|p| p.level == level && p.row == row)
        {
            return;
        }
        self.pending = Some(Pending {
            level,
            row,
            due: now.checked_add(self.delay).unwrap_or(now),
        });
    }

    /// The pointer moved to `x`, `y` at `now`. Lights the row under it at once, and asks for
    /// the submenu of that row (or for the open submenu to close) after the delay; on its way
    /// to an open submenu it lights nothing new (the aim test). Outside every menu it lights
    /// nothing in the innermost menu and changes nothing else. Returns true if what is drawn
    /// changed. The menus must have been placed ([`place_in`](Self::place_in), or a draw).
    pub fn pointer(&mut self, x: f32, y: f32, now: Instant) -> bool {
        let before = self.lit();
        let point = (x, y);
        match self.level_at(x, y) {
            None => {
                self.pending = None;
                let last = self.menus.len() - 1;
                self.menus[last].hover = None;
                self.hold_path(last);
                self.apex = Some(point);
            }
            Some(k) => {
                if self.pending.is_some_and(|p| p.level != k) {
                    self.pending = None;
                }
                self.focus = k;
                self.hold_path(k);
                let row = self.row_at(k, x, y);
                let open = self.path.get(k).copied();
                let aimed = match (open, self.apex) {
                    (Some(_), Some(apex)) => aims_at(apex, point, self.menus[k + 1].rect()),
                    _ => false,
                };
                if aimed && row != open {
                    // On its way to the submenu: the row that opened it stays lit, and the row
                    // under the pointer counts only once the pointer rests on it.
                    self.menus[k].hover = open;
                    self.pending = Some(Pending {
                        level: k,
                        row,
                        due: now.checked_add(self.delay).unwrap_or(now),
                    });
                } else {
                    self.apex = Some(point);
                    self.menus[k].hover = row;
                    let branch = row.is_some_and(|r| self.opens(k, r));
                    if (row.is_some() && row == open) || (open.is_none() && !branch) {
                        self.pending = None;
                    } else {
                        self.schedule(k, row, now);
                    }
                }
            }
        }
        self.last = Some(point);
        before != self.lit()
    }

    /// Makes the change the pointer asked for if it is due at `now`: opens, closes or replaces
    /// a submenu. Returns true if what is drawn changed. Call it when the
    /// [`deadline`](Self::deadline) has come; earlier it does nothing.
    pub fn tick(&mut self, now: Instant) -> bool {
        let Some(p) = self.pending else {
            return false;
        };
        if now < p.due {
            return false;
        }
        self.pending = None;
        self.apply(p)
    }

    /// Makes the change `p` asked for, now. Returns true if what is drawn changed.
    fn apply(&mut self, p: Pending) -> bool {
        if p.level >= self.menus.len() {
            return false;
        }
        if p.row.is_some() && p.row == self.path.get(p.level).copied() {
            self.menus[p.level].hover = p.row;
            return false;
        }
        let before = self.lit();
        self.close_after(p.level);
        self.menus[p.level].hover = p.row;
        if p.row.is_some_and(|r| self.open(p.level, r)) {
            // The submenu opened under a resting pointer: the aim starts from there.
            self.apex = self.last;
        }
        before != self.lit()
    }

    /// A click at `x`, `y` (on the button's release, or its press, as the application's other
    /// menus do). On a row that opens a submenu it opens it at once and is [`Outcome::Changed`]
    /// (or [`Outcome::Nothing`] when it is open already); on another enabled row it is
    /// [`Outcome::Chosen`]; on a disabled row, a separator or the padding it is
    /// [`Outcome::Nothing`]; outside every menu it is [`Outcome::Outside`]. A change the
    /// pointer asked for and that still waits is made first. A chosen item closes the submenus
    /// after its menu, for an application that keeps the cascade open (a check mark turned on).
    pub fn click(&mut self, x: f32, y: f32) -> Outcome<T> {
        let applied = self.pending.take().is_some_and(|p| self.apply(p));
        let nothing = if applied {
            Outcome::Changed
        } else {
            Outcome::Nothing
        };
        let Some(k) = self.level_at(x, y) else {
            return Outcome::Outside;
        };
        self.focus = k;
        self.hold_path(k);
        let Some(row) = self.row_at(k, x, y) else {
            return nothing;
        };
        if self.opens(k, row) {
            if self.path.get(k) == Some(&row) {
                return nothing;
            }
            self.open(k, row);
            self.apex = Some((x, y));
            return Outcome::Changed;
        }
        self.close_after(k);
        self.menus[k].hover = Some(row);
        self.menus[k]
            .chosen()
            .map_or(Outcome::Nothing, Outcome::Chosen)
    }

    /// Opens the submenu of the lit row of the menu at `level`, if it has one, and gives it the
    /// keys with its first enabled row lit.
    fn enter(&mut self, level: usize) -> Option<Outcome<T>> {
        let row = self.menus[level].hover?;
        if !self.opens(level, row) {
            return None;
        }
        if self.path.get(level) != Some(&row) {
            self.open(level, row);
        }
        self.focus = level + 1;
        let sub = &mut self.menus[level + 1];
        if sub.hover.is_none() {
            sub.step(true);
        }
        Some(Outcome::Changed)
    }

    /// A key, for the menu that has the keys. Up, Down, Home and End move its highlight (and
    /// close its submenu); Right, Enter and Space open the lit row's submenu and move into it;
    /// Enter and Space on another row choose it; Left and Escape close the innermost submenu;
    /// Escape with none open is [`Outcome::Close`]; a letter is an access key
    /// ([`access_keys`](Self::access_keys)), which chooses its item or opens its submenu.
    /// Anything else is [`Outcome::Ignored`]. A key waits for nothing: a change the pointer
    /// asked for and that still waits is made first, so the key acts on what is lit.
    pub fn key(&mut self, key: &KeyName) -> Outcome<T> {
        if let Some(p) = self.pending.take() {
            self.apply(p);
        }
        let k = self.focus.min(self.menus.len() - 1);
        self.focus = k;
        match key {
            KeyName::Up | KeyName::Down => {
                self.close_after(k);
                self.menus[k].step(*key == KeyName::Down);
                Outcome::Changed
            }
            KeyName::Home | KeyName::End => {
                self.close_after(k);
                // From no highlight, a step forward lands on the first enabled row and a step
                // back on the last.
                self.menus[k].hover = None;
                self.menus[k].step(*key == KeyName::Home);
                Outcome::Changed
            }
            KeyName::Right => self.enter(k).unwrap_or(Outcome::Ignored),
            KeyName::Enter | KeyName::Space => match self.menus[k].hover {
                None => Outcome::Nothing,
                Some(_) => self.enter(k).unwrap_or_else(|| {
                    self.menus[k]
                        .chosen()
                        .map_or(Outcome::Nothing, Outcome::Chosen)
                }),
            },
            KeyName::Left if k > 0 => {
                self.close_after(k - 1);
                Outcome::Changed
            }
            KeyName::Escape if self.menus.len() > 1 => {
                self.close_after(self.menus.len() - 2);
                Outcome::Changed
            }
            KeyName::Escape => Outcome::Close,
            KeyName::Char(c) => match self.menus[k].access_key(*c) {
                Some(id) => {
                    if self.path.get(k) != self.menus[k].hover.as_ref() {
                        self.close_after(k);
                    }
                    self.enter(k).unwrap_or(Outcome::Chosen(id))
                }
                None => Outcome::Ignored,
            },
            _ => Outcome::Ignored,
        }
    }

    /// The wheel turned by `dy` logical px over `x`, `y`: scrolls the menu under the point
    /// ([`Menu::scroll_by`]); its submenu follows its row at the next placement. Returns true
    /// if a menu moved.
    pub fn wheel(&mut self, x: f32, y: f32, dy: f32) -> bool {
        if !dy.is_finite() {
            return false;
        }
        match self.level_at(x, y) {
            Some(k) => self.menus[k].scroll_by(dy),
            None => false,
        }
    }

    /// Places every open menu inside `area`: the root by its anchor, each submenu beside the
    /// row that opened it (kept within its menu when that row is scrolled out of view), and to
    /// the left when the menu before it went to the left. Called by the draws; call it before
    /// to know the menus' rectangles.
    pub fn place_in(&mut self, p: &mut Painter, area: Rect) {
        self.menus[0].place_in(p, area);
        for k in 1..self.menus.len() {
            let parent = &self.menus[k - 1];
            let outer = parent.rect();
            let row = parent.row_rect(self.path[k - 1]);
            let y = row.y.min(outer.bottom() - row.h).max(outer.y);
            let item = Rect::new(outer.x, y, outer.w, row.h);
            let leftward = parent.opened_left();
            self.menus[k].set_around(Around::Beside(item, leftward));
            self.menus[k].place_in(p, area);
        }
    }

    /// Everything the open menus draw, their shadows included.
    fn bounds(&self) -> Rect {
        let mut r = self.menus[0].rect();
        for m in &self.menus[1..] {
            let s = m.rect();
            let (x, y) = (r.x.min(s.x), r.y.min(s.y));
            r = Rect::new(
                x,
                y,
                r.right().max(s.right()) - x,
                r.bottom().max(s.bottom()) - y,
            );
        }
        r.inset(-18.0)
    }

    fn drop(&self) -> f32 {
        if self.menus[0].opened_above() {
            MENU_DROP
        } else {
            -MENU_DROP
        }
    }

    /// Places the menus inside `area` and draws them, each over the one before.
    pub fn draw_in(&mut self, p: &mut Painter, pal: &Palette, area: Rect) {
        self.place_in(p, area);
        for menu in &mut self.menus {
            menu.draw_in(p, pal, area);
        }
    }

    /// Draws the whole cascade as it opens or closes, `t` from 0 (gone) to 1 (there): faded and
    /// moved as one, the way [`Menu::draw_in_presence`] draws a menu. At 1 this is
    /// [`draw_in`](Self::draw_in), pixel for pixel.
    pub fn draw_in_presence(&mut self, p: &mut Painter, pal: &Palette, area: Rect, t: f32) {
        self.place_in(p, area);
        let (bounds, drop) = (self.bounds(), self.drop());
        let menus = &mut self.menus;
        pop(p, bounds, t, (0.0, drop), |p| {
            for menu in menus.iter_mut() {
                menu.draw_in(p, pal, area);
            }
        });
    }

    /// [`draw_in_presence`](Self::draw_in_presence), with each submenu entering on its own as
    /// it opens: it fades in and slides [`MENU_DROP`] px out from its menu, by
    /// [`fluent::ENTER`], asked of `anim` under `id` and the submenu's path. A submenu that
    /// closes goes at once. Once every entrance has played this is `draw_in_presence`, pixel for
    /// pixel; with reduced motion it always is.
    pub fn draw_animated(
        &mut self,
        p: &mut Painter,
        pal: &Palette,
        area: Rect,
        t: f32,
        anim: &mut Animator,
        id: impl AsId,
    ) {
        self.place_in(p, area);
        let id = id.as_id();
        let shown: Vec<f32> = (1..self.menus.len())
            .map(|k| anim.presence(id.with(&self.path[..k]), true, fluent::ENTER, fluent::EXIT))
            .collect();
        let (bounds, drop) = (self.bounds(), self.drop());
        let menus = &mut self.menus;
        pop(p, bounds, t, (0.0, drop), |p| {
            for (k, menu) in menus.iter_mut().enumerate() {
                if k == 0 {
                    menu.draw_in(p, pal, area);
                    continue;
                }
                let dx = if menu.opened_left() {
                    MENU_DROP
                } else {
                    -MENU_DROP
                };
                let b = menu.rect().inset(-18.0);
                pop(p, b, shown[k - 1], (dx, 0.0), |p| {
                    menu.draw_in(p, pal, area)
                });
            }
        });
    }
}

#[cfg(test)]
#[path = "submenu_tests.rs"]
mod tests;
