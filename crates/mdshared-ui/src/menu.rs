//! Popup menus drawn inside the window: the main menu, Recent and the right-click menus.
//!
//! A menu hangs from a point ([`Menu::new`], [`Menu::above`]) or is anchored to a rectangle
//! and kept on screen at any corner ([`Menu::around`]). Submenus, their hover delay and their
//! keys are [`submenu::Cascade`](crate::submenu::Cascade), which is made of these menus placed
//! [`beside`](Menu::beside) one another.

use crate::gfx::{Face, Painter, Rect};
use crate::scroll;
use crate::theme::Palette;
use std::borrow::Cow;
use std::ops::Range;

#[derive(Clone, Debug)]
pub struct Item<T> {
    pub id: T,
    pub label: String,
    pub hint: String,
    /// An icon from `glyph`, or empty.
    pub icon: &'static str,
    pub enabled: bool,
    pub checked: bool,
    /// The item is a toggle or one of a set of choices: its menu keeps room for a check mark
    /// whether it is on or off, so the rows do not shift between openings.
    pub checkable: bool,
    pub separator_before: bool,
}

impl<T> Item<T> {
    pub fn new(id: T, label: impl Into<String>) -> Item<T> {
        Item {
            id,
            label: label.into(),
            hint: String::new(),
            icon: "",
            enabled: true,
            checked: false,
            checkable: false,
            separator_before: false,
        }
    }
    /// The shortcut shown at the item's right, as the table writes it ("Ctrl+S"); printed the
    /// platform's way (`keys::label`: "⌘S" on a Mac).
    pub fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = crate::keys::label(&hint.into()).to_string();
        self
    }
    pub fn icon(mut self, icon: &'static str) -> Self {
        self.icon = icon;
        self
    }
    pub fn sep(mut self) -> Self {
        self.separator_before = true;
        self
    }
    pub fn enabled(mut self, on: bool) -> Self {
        self.enabled = on;
        self
    }
    pub fn checked(mut self, on: bool) -> Self {
        self.checked = on;
        self.checkable = true;
        self
    }
}

/// A label read for access keys: what is drawn, and which character, if any, is the access key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Access {
    /// The label as drawn: every `&&` is one `&`, and the marking `&` is gone.
    pub text: String,
    /// The byte range in `text` of the marked character.
    pub key: Option<Range<usize>>,
}

/// Reads `label` for access keys. `&x` marks `x` (the first mark in the label is the access key;
/// a later `&y` drops its `&` and marks nothing), `&&` is a literal `&`, and a `&` before white
/// space or at the end of the label is a literal `&` as well.
pub fn parse_access(label: &str) -> Access {
    let mut text = String::with_capacity(label.len());
    let mut key = None;
    let mut chars = label.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '&' {
            text.push(c);
            continue;
        }
        match chars.peek().copied() {
            Some('&') => {
                chars.next();
                text.push('&');
            }
            Some(next) if !next.is_whitespace() => {
                chars.next();
                if key.is_none() {
                    key = Some(text.len()..text.len() + next.len_utf8());
                }
                text.push(next);
            }
            _ => text.push('&'),
        }
    }
    Access { text, key }
}

/// `text` with every `&` doubled, for a label that comes from data (a file name, a completion)
/// and goes into a menu that reads access keys.
pub fn escape_access(text: &str) -> String {
    text.replace('&', "&&")
}

/// The access key of `label`, if it marks one.
pub fn access_key_of(label: &str) -> Option<char> {
    if !label.contains('&') {
        return None;
    }
    let parsed = parse_access(label);
    parsed.key.and_then(|k| parsed.text[k].chars().next())
}

/// Whether the typed `typed` is the access key `key`: equal, or equal in Unicode lower case.
pub fn same_letter(key: char, typed: char) -> bool {
    key == typed || key.to_lowercase().eq(typed.to_lowercase())
}

const ROW: f32 = 32.0;
const SEP: f32 = 9.0;
/// The padding above the first row and below the last, and at each side of the rows.
pub(crate) const PAD: f32 = 5.0;
const MAX_WIDTH: f32 = 420.0;

/// How far from its place a menu starts when it is drawn by
/// [`Menu::draw_in_presence`], in logical px.
pub const MENU_DROP: f32 = 6.0;

/// The room the arrow of a row that opens a submenu takes at the row's right, in place of a
/// hint.
const ARROW: f32 = 28.0;

/// How a menu placed by [`Menu::around`] or [`Menu::beside`] stands to its rectangle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Around {
    /// Below the rectangle, flipped above or to the left when it does not fit.
    Below(Rect),
    /// Beside the rectangle (an item as wide as its menu), on the left first when `true`.
    Beside(Rect, bool),
    /// Below the rectangle, flipped above or to the left when it does not fit, and not over it:
    /// where it fits on neither side it is cut to the room on the roomier side and scrolls (a
    /// drop-down's list, which must not hide its field), unless not even one row would show on
    /// either side, where it is placed as [`Around::Below`] places it.
    Clear(Rect),
}

pub struct Menu<T> {
    pub items: Vec<Item<T>>,
    /// Where the menu hangs from: its top-left corner, or its top-right when `align_right`.
    anchor: (f32, f32),
    align_right: bool,
    /// The anchor is the menu's bottom edge, not its top.
    above: bool,
    pub hover: Option<usize>,
    rect: Rect,
    rows: Vec<Rect>,
    /// How far the rows are scrolled up, in logical px; zero unless the menu is taller than the
    /// room it was placed in.
    scroll: f32,
    /// The height all the rows need, padding included.
    content: f32,
    /// A row the keyboard moved to, brought into view by the next `place`.
    reveal: Option<usize>,
    /// Labels are read for access keys ([`parse_access`]).
    access: bool,
    /// Whether the marked letters are drawn underlined.
    access_visible: bool,
    /// Placed by a rectangle ([`around`](Self::around), [`beside`](Self::beside)) rather than
    /// from `anchor`.
    pub(crate) around: Option<Around>,
    /// The last placement by a rectangle put the menu to the left of it, or above it.
    went_left: bool,
    went_up: bool,
    /// Which rows open a submenu and draw an arrow; empty in a menu without submenus.
    branches: Vec<bool>,
    /// The least width the menu is placed at ([`min_width`](Self::min_width)); zero for none.
    pub(crate) min_w: f32,
    /// The highlighted row is drawn in the palette's accent with `on_accent`
    /// ([`set_high_contrast`](Self::set_high_contrast)).
    high_contrast: bool,
    /// Each row's hover fill, while [`draw_in_animated`](Self::draw_in_animated) draws.
    fade: Option<Vec<f32>>,
}

/// The width of the check-mark column in a menu that also has icons.
const MARK: f32 = 20.0;

impl<T: Clone> Menu<T> {
    /// Whether the menu has a check-mark column and an icon column.
    fn columns(&self) -> (bool, bool) {
        (
            self.items.iter().any(|i| i.checkable),
            self.items.iter().any(|i| !i.icon.is_empty()),
        )
    }

    pub fn new(items: Vec<Item<T>>, x: f32, y: f32, align_right: bool) -> Menu<T> {
        Menu {
            items,
            anchor: (x, y),
            align_right,
            above: false,
            hover: None,
            rect: Rect::default(),
            rows: Vec::new(),
            scroll: 0.0,
            content: 0.0,
            reveal: None,
            access: false,
            access_visible: !crate::keys::MAC,
            around: None,
            went_left: false,
            went_up: false,
            branches: Vec::new(),
            min_w: 0.0,
            high_contrast: false,
            fade: None,
        }
    }

    /// Anchors the menu to `anchor` and keeps it on screen at any corner: a context menu on a
    /// selected word or a cell, a drop-down under a button, or one at a right-click, which is a
    /// rectangle of no size at the pointer. The menu opens below the anchor with its left edge
    /// at the anchor's left; where it would cross the bottom of the area it opens above the
    /// anchor instead, and where it would cross the right it opens to the left, its right edge at
    /// the anchor's right. When it fits on neither side it goes to the side with more room and
    /// is pushed inside the area, over the anchor; one taller than the area scrolls. This takes
    /// the place of the point given to [`new`](Self::new), of its `align_right` and of
    /// [`above`](Self::above). The geometry is [`submenu::around`](crate::submenu::around).
    pub fn around(mut self, anchor: Rect) -> Self {
        self.around = Some(Around::Below(anchor));
        self
    }

    /// Places the menu beside `item` as a submenu: `item` is the row that opens it, as wide as
    /// its menu ([`Menu::rect`]'s `x` and `w`, the row's `y` and `h`). The menu goes to the
    /// right of it with its first row level with the item, or to the left first when `leftward`
    /// (the menu it comes from opened to the left, so its submenus carry on that way); it flips
    /// to the other side when it would leave the area, and slides up to stay inside it. The
    /// geometry is [`submenu::beside`](crate::submenu::beside).
    pub fn beside(mut self, item: Rect, leftward: bool) -> Self {
        self.around = Some(Around::Beside(item, leftward));
        self
    }

    /// Makes the menu at least `w` logical px wide, as a drop-down's list is at least as wide as
    /// its field. The widest row still widens it, the menu's own limit of 420 px and the area it
    /// is placed in still narrow it, and a width that is not finite or not positive asks for
    /// nothing. A menu not given one is as wide as before.
    pub fn min_width(mut self, w: f32) -> Self {
        self.set_min_width(w);
        self
    }

    /// [`min_width`](Self::min_width) on a menu already made: the next placement uses it.
    pub(crate) fn set_min_width(&mut self, w: f32) {
        self.min_w = if w.is_finite() { w.max(0.0) } else { 0.0 };
    }

    /// Draws the highlighted row as Windows does under high contrast: filled with the palette's
    /// `accent` and its label, mark, icon and hint in `on_accent`, which for the system's
    /// contrast theme ([`crate::tokens::Tokens::from_system_colors`]) are its Highlight and
    /// HighlightText ([`crate::tokens::selected`]). Off by default: the row is filled with
    /// `hover` and keeps its colours, which under a contrast theme stays close to the popup.
    /// Pass the host's `Cx::high_contrast` or
    /// [`crate::platform::system_high_contrast`].
    pub fn set_high_contrast(&mut self, on: bool) {
        self.high_contrast = on;
    }

    pub(crate) fn set_around(&mut self, around: Around) {
        self.around = Some(around);
    }

    /// Whether the last placement put the menu to the left of the rectangle it was placed by:
    /// flipped by [`around`](Self::around), or on the left by [`beside`](Self::beside). False
    /// for a menu placed from a point.
    pub fn opened_left(&self) -> bool {
        self.went_left
    }

    /// Whether the menu stands above its anchor: flipped above by the last placement of
    /// [`around`](Self::around), or, for a menu hung from a point, made [`above`](Self::above).
    pub fn opened_above(&self) -> bool {
        if self.around.is_some() {
            self.went_up
        } else {
            self.above
        }
    }

    /// Marks the rows at `rows` (indices into `items`) as rows that open a submenu: each draws a
    /// right-pointing arrow at its right in place of its hint, and the menu keeps room for it.
    /// An index past the last item is ignored. [`submenu::Cascade`](crate::submenu::Cascade)
    /// marks its menus this way; a menu marked by nobody draws as it always did.
    pub fn submenu_rows(mut self, rows: impl IntoIterator<Item = usize>) -> Self {
        for row in rows {
            if row < self.items.len() {
                if self.branches.len() <= row {
                    self.branches.resize(row + 1, false);
                }
                self.branches[row] = true;
            }
        }
        self
    }

    /// Whether row `index` opens a submenu ([`submenu_rows`](Self::submenu_rows)).
    pub fn opens_submenu(&self, index: usize) -> bool {
        self.branches.get(index).copied().unwrap_or(false)
    }

    /// Reads the labels of this menu for access keys: `Open &files…` marks `f`, `&&` is a
    /// literal `&`. Off by default, so a label with a `&` in it (a file name in a Recent menu, say)
    /// is drawn as it is. In a menu that has it on, label text that comes from data goes through
    /// [`escape_access`].
    pub fn access_keys(mut self, on: bool) -> Self {
        self.access = on;
        self
    }

    /// Whether the marked letters are drawn underlined (default: yes, always). An application
    /// that wants the Windows convention sets it to true only while the menu was opened with the
    /// keyboard or Alt is held, and calls this again when that changes.
    /// Never on a Mac, which has no access keys: the marks are still read, so a label's `&` is not
    /// drawn, and typing the letter still picks the item.
    pub fn set_access_visible(&mut self, on: bool) {
        self.access_visible = on && !crate::keys::MAC;
    }

    /// The label as drawn and its marked character: the label itself, borrowed, unless this menu
    /// reads access keys and the label has a `&`.
    fn shown<'a>(&self, label: &'a str) -> (Cow<'a, str>, Option<Range<usize>>) {
        if !self.access || !label.contains('&') {
            return (Cow::Borrowed(label), None);
        }
        let parsed = parse_access(label);
        (Cow::Owned(parsed.text), parsed.key)
    }

    /// For the key `typed` while the menu is open: the first enabled item whose access key it is
    /// (case-insensitively, by Unicode lower case) gets the highlight, and its id is returned for
    /// the application to act on as it does for Enter. `None` when no item has the key or the menu
    /// does not read access keys; the application then handles the key as before.
    pub fn access_key(&mut self, typed: char) -> Option<T> {
        if !self.access {
            return None;
        }
        let index = self.items.iter().position(|item| {
            item.enabled && access_key_of(&item.label).is_some_and(|k| same_letter(k, typed))
        })?;
        self.hover = Some(index);
        self.reveal = Some(index);
        Some(self.items[index].id.clone())
    }

    /// Makes the menu stand on its anchor: `y` is its bottom edge, for a menu opened from the
    /// bottom of a window.
    pub fn above(mut self) -> Self {
        self.above = true;
        self
    }

    /// Places the menu inside a `width` x `height` window. Called by `draw`; call it before to
    /// know [`rect`](Self::rect).
    pub fn place(&mut self, p: &mut Painter, width: f32, height: f32) {
        self.place_in(p, Rect::new(0.0, 0.0, width, height));
    }

    /// Places the menu inside `area`, the part of the window it may use (the window less its
    /// title row, say), with a margin of a few px. The menu never rises above the area; one
    /// taller than the area is as tall as fits and scrolls ([`scroll_by`](Self::scroll_by)).
    pub fn place_in(&mut self, p: &mut Painter, area: Rect) {
        let (marks, icons) = self.columns();
        let lead =
            if marks || icons { 40.0 } else { 14.0 } + if marks && icons { MARK } else { 0.0 };
        let mut w: f32 = 180.0;
        for (i, item) in self.items.iter().enumerate() {
            let hint = if self.opens_submenu(i) {
                ARROW
            } else if item.hint.is_empty() {
                0.0
            } else {
                p.measure(Face::Ui, 12.0, &item.hint) + 28.0
            };
            let (label, _) = self.shown(&item.label);
            w = w.max(lead + p.measure(Face::Ui, 13.5, &label) + hint + 16.0);
        }
        let w = w.max(self.min_w).min(MAX_WIDTH).min(area.w - 8.0);
        let h: f32 = 2.0 * PAD
            + self
                .items
                .iter()
                .map(|i| ROW + if i.separator_before { SEP } else { 0.0 })
                .sum::<f32>();
        let (x, y, shown) = match self.around {
            Some(around) => {
                let placed = match around {
                    Around::Below(anchor) => crate::submenu::around(w, h, anchor, area),
                    Around::Clear(anchor) => crate::submenu::around_clear(w, h, anchor, area),
                    Around::Beside(item, leftward) => {
                        crate::submenu::beside(w, h, item, area, leftward)
                    }
                };
                self.went_left = placed.left;
                self.went_up = placed.above;
                (placed.rect.x, placed.rect.y, placed.rect.h)
            }
            None => {
                let top = area.y.max(4.0);
                let bottom = area.bottom() - 4.0;
                let mut x = if self.align_right {
                    self.anchor.0 - w
                } else {
                    self.anchor.0
                };
                let mut y = if self.above {
                    (self.anchor.1 - h).max(top)
                } else {
                    // Where it was opened, even when that is in the title row (a tab's own
                    // menu); only being pushed up to fit stops at the area's top.
                    self.anchor.1
                };
                x = x.min(area.right() - w - 4.0).max(area.x + 4.0);
                if y + h > bottom {
                    y = (bottom - h).max(top);
                }
                (x, y, h.min(bottom - top))
            }
        };
        self.rect = Rect::new(x.round(), y.round(), w.round(), shown);
        self.content = h;
        let max = (h - shown).max(0.0);
        if let Some(i) = self.reveal.take() {
            let (off, row) = row_span(&self.items, i);
            if off - PAD < self.scroll {
                self.scroll = off - PAD;
            } else if off + row + PAD > self.scroll + shown {
                self.scroll = off + row + PAD - shown;
            }
        }
        self.scroll = self.scroll.clamp(0.0, max).round().min(max);
        self.rows.clear();
        let mut ry = self.rect.y + PAD - self.scroll;
        for item in &self.items {
            if item.separator_before {
                ry += SEP;
            }
            self.rows.push(Rect::new(
                self.rect.x + PAD,
                ry,
                self.rect.w - 2.0 * PAD,
                ROW,
            ));
            ry += ROW;
        }
    }

    pub fn draw(&mut self, p: &mut Painter, pal: &Palette, width: f32, height: f32) {
        self.draw_in(p, pal, Rect::new(0.0, 0.0, width, height));
    }

    /// Places the menu inside `area` ([`place_in`](Self::place_in)) and draws it.
    pub fn draw_in(&mut self, p: &mut Painter, pal: &Palette, area: Rect) {
        self.place_in(p, area);
        let (marks, icons) = self.columns();
        p.shadow(self.rect, 8.0, pal.dark);
        p.frame(self.rect, 8.0, pal.popup, pal.popup_line);
        let old = p.clip(self.rect);
        for (i, item) in self.items.iter().enumerate() {
            let r = self.rows[i];
            if item.separator_before {
                let line = Rect::new(
                    self.rect.x + 1.0,
                    r.y - SEP / 2.0 - 0.5,
                    self.rect.w - 2.0,
                    1.0,
                );
                p.fill(line, pal.popup_line);
            }
            let lit = self.hover == Some(i) && item.enabled;
            let strong = lit && self.high_contrast;
            match self.fade.as_ref().and_then(|f| f.get(i).copied()) {
                // Under high contrast the highlight is at once, as the system's.
                Some(a) if !self.high_contrast => {
                    if a > 0.0 {
                        p.rbox_a(r, 5.0, pal.hover, a.min(1.0));
                    }
                }
                _ => {
                    if lit {
                        p.rbox(r, 5.0, if strong { pal.accent } else { pal.hover });
                    }
                }
            }
            // Under high contrast everything on the highlighted row reads on Highlight.
            let ink = |c| if strong { pal.on_accent } else { c };
            let color = ink(if item.enabled { pal.text } else { pal.faint });
            let mut x = r.x + 9.0;
            if marks || icons {
                // The check mark has a column of its own before the icons, so an item shows both
                // what it is and whether it is on. A menu without icons has the mark where the
                // icons would be.
                let mut col = r.x + 18.0;
                if item.checked {
                    p.icon(crate::glyph::CHECK, 14.0, col, r.cy(), ink(pal.accent));
                }
                if marks && icons {
                    col += MARK;
                }
                if icons && !item.icon.is_empty() {
                    let c = ink(if item.enabled { pal.muted } else { pal.faint });
                    p.icon(item.icon, 14.0, col, r.cy(), c);
                }
                x = col + 18.0;
            }
            let hint_w = if self.opens_submenu(i) {
                let c = ink(if item.enabled { pal.muted } else { pal.faint });
                p.icon(crate::glyph::RIGHT, 12.0, r.right() - 12.0, r.cy(), c);
                ARROW - 2.0
            } else if item.hint.is_empty() {
                0.0
            } else {
                p.text_right(
                    Face::Ui,
                    12.0,
                    r.right() - 10.0,
                    r.cy(),
                    &item.hint,
                    ink(pal.faint),
                ) + 18.0
            };
            let (label, key) = self.shown(&item.label);
            let room = r.right() - x - hint_w - 8.0;
            p.text(Face::Ui, 13.5, x, r.cy(), &label, color, room);
            if let Some(key) = key.filter(|_| self.access_visible) {
                // Only when the label was drawn whole: an elided one may have cut the letter.
                if p.measure(Face::Ui, 13.5, &label) <= room {
                    p.underline_range(Face::Ui, 13.5, x, r.cy(), &label, key, color);
                }
            }
        }
        p.restore(old);
        if let Some((track, thumb)) =
            scroll::scrollbar(self.rect, self.content, self.rect.h, self.scroll)
        {
            scroll::draw_scrollbar(p, pal, track, thumb, false);
        }
    }

    /// Places the menu inside `area` and draws it as it opens or closes: `t` is how present it
    /// is, 0 gone and 1 there, from [`Animator::presence`](crate::anim::Animator::presence) or
    /// a [`Transient`](crate::anim::Transient) that holds the menu while it closes. The menu
    /// fades as a whole and moves [`MENU_DROP`] px into place, downwards (upwards for a menu
    /// that opens [`above`](Self::above) its anchor). At 1 this is
    /// [`draw_in`](Self::draw_in), pixel for pixel; at 0 the menu is placed and nothing is
    /// drawn. A menu that is closing takes no pointer and no keys: the application has let go
    /// of it and only draws it.
    pub fn draw_in_presence(&mut self, p: &mut Painter, pal: &Palette, area: Rect, t: f32) {
        self.place_in(p, area);
        let drop = if self.opened_above() {
            MENU_DROP
        } else {
            -MENU_DROP
        };
        // The frame and the shadow round it.
        let bounds = self.rect.inset(-18.0);
        crate::anim::pop(p, bounds, t, (0.0, drop), |p| self.draw_in(p, pal, area));
    }

    /// Everything [`draw_in_presence`](Self::draw_in_presence) touches at any `t`, as last
    /// placed: the frame, its shadow, the drop and a device pixel round them
    /// ([`crate::anim::pop_bounds`]). The damage to mark while the menu opens or closes, and on
    /// the frame after it closed.
    pub fn presence_bounds(&self) -> Rect {
        let drop = if self.opened_above() {
            MENU_DROP
        } else {
            -MENU_DROP
        };
        crate::anim::pop_bounds(self.rect.inset(-18.0), (0.0, drop))
    }

    /// [`draw_in`](Self::draw_in) with the hover row's fill fading in on the row the pointer or
    /// the keys reach and out on the one they left (roadmap X3), by
    /// [`crate::anim::document::HOVER`]. `id` names the menu in `anim`. At rest, and when the
    /// animator does not animate, this is `draw_in`, pixel for pixel; under high contrast the
    /// highlight is at once.
    pub fn draw_in_animated(
        &mut self,
        p: &mut Painter,
        pal: &Palette,
        area: Rect,
        anim: &mut crate::anim::Animator,
        id: impl crate::anim::AsId,
    ) {
        let id = crate::anim::Id::new(id);
        let fade = (0..self.items.len())
            .map(|i| {
                let on = self.hover == Some(i) && self.items[i].enabled;
                anim.toggle(id.with(("row", i)), on, crate::anim::document::HOVER)
            })
            .collect();
        self.fade = Some(fade);
        self.draw_in(p, pal, area);
        self.fade = None;
    }

    /// Where the menu was last placed.
    pub fn rect(&self) -> Rect {
        self.rect
    }

    pub fn contains(&self, x: f32, y: f32) -> bool {
        self.rect.contains(x, y)
    }

    /// Updates the hovered row. Returns true if it changed.
    pub fn pointer(&mut self, x: f32, y: f32) -> bool {
        let hit = self
            .rows
            .iter()
            .position(|r| r.contains(x, y) && self.rect.contains(x, y))
            .filter(|&i| self.items.get(i).is_some_and(|item| item.enabled));
        let changed = hit != self.hover;
        self.hover = hit;
        changed
    }

    /// Moves the keyboard highlight by one enabled row.
    pub fn step(&mut self, forward: bool) {
        let n = self.items.len();
        if n == 0 {
            return;
        }
        let mut i = self.hover.unwrap_or(if forward { n - 1 } else { 0 });
        for _ in 0..n {
            i = if forward {
                (i + 1) % n
            } else {
                (i + n - 1) % n
            };
            if self.items[i].enabled {
                self.hover = Some(i);
                self.reveal = Some(i);
                return;
            }
        }
    }

    /// Moves the keyboard highlight to row `index` and brings it into view at the next
    /// placement, as [`step`](Self::step) does for the next row. Returns false, and changes
    /// nothing, for an index past the last item or a disabled item.
    pub fn highlight(&mut self, index: usize) -> bool {
        if !self.items.get(index).is_some_and(|i| i.enabled) {
            return false;
        }
        self.hover = Some(index);
        self.reveal = Some(index);
        true
    }

    /// Scrolls the rows down the list by `dy` logical px (negative goes back up) and moves the
    /// placed rows with it. Returns true if the menu moved. A menu that fits does not move.
    pub fn scroll_by(&mut self, dy: f32) -> bool {
        let max = (self.content - self.rect.h).max(0.0);
        let next = (self.scroll + dy).clamp(0.0, max).round().min(max);
        let moved = next != self.scroll;
        let d = next - self.scroll;
        self.scroll = next;
        for r in &mut self.rows {
            r.y -= d;
        }
        moved
    }

    /// Scrolls to the end of the rows at the next placement (for a menu opened already scrolled,
    /// and for screenshots).
    pub fn scroll_to_end(&mut self) {
        self.scroll = f32::MAX;
    }

    /// Whether the menu is taller than the room it was placed in, so that it scrolls.
    pub fn scrolls(&self) -> bool {
        self.content > self.rect.h + 0.5
    }

    /// How far the rows are scrolled, in logical px.
    pub fn scroll(&self) -> f32 {
        self.scroll
    }

    /// Where row `index` was last placed, scrolling included (it may lie partly or wholly
    /// outside [`rect`](Self::rect)). An index past the last item gives an empty rectangle.
    pub fn row_rect(&self, index: usize) -> Rect {
        self.rows.get(index).copied().unwrap_or_default()
    }

    /// The highlighted item's id, for a click or Enter. None when the highlight names no item:
    /// the items were replaced by fewer while the menu was open.
    pub fn chosen(&self) -> Option<T> {
        let item = self.items.get(self.hover?)?;
        item.enabled.then(|| item.id.clone())
    }
}

/// Where row `index` of a menu with these `items` sits: its distance below the top of the placed
/// menu ([`Menu::rect`]) and its height, in logical pixels. A submenu opened beside an item
/// anchors on this, so no app needs to know the menu's padding, row height or separator gap.
/// An index past the last item gives the offset a row would have there.
pub fn row_span<T>(items: &[Item<T>], index: usize) -> (f32, f32) {
    let mut y = PAD;
    for (i, item) in items.iter().enumerate() {
        if item.separator_before {
            y += SEP;
        }
        if i == index {
            return (y, ROW);
        }
        y += ROW;
    }
    (y, ROW)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gfx::Gfx;
    use mdshared_text::Bitmap;

    /// `row_span` is what `place` lays out: every row's top and height, with separators.
    #[test]
    fn row_span_is_the_layout_the_menu_places() {
        let items = vec![
            Item::new(0u8, "One"),
            Item::new(1, "Two").sep(),
            Item::new(2, "Three"),
            Item::new(3, "Four").sep(),
            Item::new(4, "Five").sep(),
        ];
        let mut menu = Menu::new(items, 30.0, 40.0, false);
        let mut bmp = Bitmap::new(400, 400);
        let mut gfx = Gfx::new();
        let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
        menu.place(&mut p, 400.0, 400.0);
        let top = menu.rect().y;
        for i in 0..menu.items.len() {
            let (offset, height) = row_span(&menu.items, i);
            assert_eq!(top + offset, menu.rows[i].y, "row {i}");
            assert_eq!(height, menu.rows[i].h, "row {i}");
        }
        // The pinned numbers a viewer once copied: 5 px padding, 32 px rows, 9 px before a rule.
        assert_eq!(row_span(&menu.items, 0), (5.0, 32.0));
        assert_eq!(row_span(&menu.items, 2), (5.0 + 9.0 + 64.0, 32.0));
    }

    fn placed(items: Vec<Item<u8>>, anchor: (f32, f32), area: Rect) -> Menu<u8> {
        let mut menu = Menu::new(items, anchor.0, anchor.1, true);
        let mut bmp = Bitmap::new(100, 100);
        let mut gfx = Gfx::new();
        let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
        menu.place_in(&mut p, area);
        menu
    }

    #[test]
    fn under_high_contrast_the_highlighted_row_is_highlight_with_highlight_text() {
        let colors = crate::tokens::SystemColors {
            window: crate::gfx::rgb(0xfffaef),
            window_text: crate::gfx::rgb(0x3d3d3d),
            highlight: crate::gfx::rgb(0x903909),
            highlight_text: crate::gfx::rgb(0xfff5e3),
            hot_light: crate::gfx::rgb(0x1c5e75),
            gray_text: crate::gfx::rgb(0x676767),
            button_face: crate::gfx::rgb(0xfffaef),
            button_text: crate::gfx::rgb(0x3d3d3d),
        };
        let pal = crate::tokens::Tokens::from_system_colors(&colors).palette;
        let area = Rect::new(0.0, 0.0, 300.0, 300.0);
        let draw = |hc: Option<bool>| {
            let mut menu = Menu::new(many(3), 40.0, 40.0, false);
            menu.highlight(1);
            if let Some(on) = hc {
                menu.set_high_contrast(on);
            }
            let mut bmp = Bitmap::new(300, 300);
            let mut gfx = Gfx::new();
            let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
            p.fill(area, pal.page);
            menu.draw_in(&mut p, &pal, area);
            (bmp, menu.row_rect(0), menu.row_rect(1))
        };
        let px = |b: &Bitmap, x: f32, y: f32| {
            let o = (y as usize * b.width as usize + x as usize) * 4;
            [b.pixels[o + 2], b.pixels[o + 1], b.pixels[o]]
        };
        let count = |b: &Bitmap, r: Rect, c: crate::gfx::Rgb| {
            let mut n = 0;
            for y in r.y as usize..r.bottom() as usize {
                for x in r.x as usize..r.right() as usize {
                    if px(b, x as f32, y as f32) == c {
                        n += 1;
                    }
                }
            }
            n
        };
        let (plain, row0, row1) = draw(None);
        let (off, _, _) = draw(Some(false));
        let (on, _, _) = draw(Some(true));
        assert!(
            off.pixels == plain.pixels,
            "off is the default, pixel for pixel"
        );
        assert_eq!(px(&plain, row1.x + 4.0, row1.cy()), pal.hover);
        assert_eq!(px(&on, row1.x + 4.0, row1.cy()), colors.highlight);
        // The label is drawn in HighlightText (light on Desert's dark Highlight); its
        // antialiased edges blend, so count the pixels near it.
        let mut light = 0;
        for y in row1.y as usize..row1.bottom() as usize {
            for x in row1.x as usize..row1.right() as usize {
                let c = px(&on, x as f32, y as f32);
                if c[0] >= 0xd0 && c[1] >= 0xc0 && c[2] >= 0xb0 {
                    light += 1;
                }
            }
        }
        assert!(
            light > 10,
            "the label is in HighlightText ({light} light pixels)"
        );
        assert_eq!(
            count(&on, row1, pal.text),
            0,
            "no window text left on Highlight"
        );
        assert_eq!(
            count(&on, row0, pal.text),
            count(&plain, row0, pal.text),
            "other rows unchanged"
        );
    }

    /// What `draw_in_presence` changes at any `t` lies in `presence_bounds`, for a menu below its
    /// anchor and one flipped above it, a toast and a dialog card, at 1x and 1.5x.
    #[test]
    fn presence_draws_stay_inside_presence_bounds() {
        use crate::theme::LIGHT;
        let area = Rect::new(0.0, 0.0, 400.0, 320.0);
        let changed_outside = |scale: f32, out: Rect, draw: &dyn Fn(&mut Painter)| {
            let (w, h) = ((400.0 * scale) as u32, (320.0 * scale) as u32);
            let mut bmp = Bitmap::new(w, h);
            let mut gfx = Gfx::new();
            let mut p = Painter::new(&mut bmp, &mut gfx, scale);
            p.fill(area, LIGHT.page);
            let clean = bmp.pixels.clone();
            let mut p = Painter::new(&mut bmp, &mut gfx, scale);
            draw(&mut p);
            let mut n = 0;
            for y in 0..h {
                for x in 0..w {
                    let o = ((y * w + x) * 4) as usize;
                    if bmp.pixels[o..o + 4] != clean[o..o + 4]
                        && !out.contains(x as f32 / scale, y as f32 / scale)
                    {
                        n += 1;
                    }
                }
            }
            n
        };
        for scale in [1.0, 1.5] {
            for (ax, ay) in [(40.0, 40.0), (40.0, 300.0)] {
                for t in [0.01, 0.1, 0.5, 0.95] {
                    let menu = std::cell::RefCell::new(Menu::new(many(5), ax, ay, false));
                    let draw =
                        |p: &mut Painter| menu.borrow_mut().draw_in_presence(p, &LIGHT, area, t);
                    // Placed by the first draw; the bounds read after it.
                    let mut bmp = Bitmap::new(400, 320);
                    let mut gfx = Gfx::new();
                    menu.borrow_mut().draw_in(
                        &mut Painter::new(&mut bmp, &mut gfx, scale),
                        &LIGHT,
                        area,
                    );
                    let out = menu.borrow().presence_bounds();
                    assert_eq!(
                        changed_outside(scale, out, &draw),
                        0,
                        "menu at {ax},{ay}, t {t}"
                    );
                }
            }
            for t in [0.01, 0.1, 0.5, 0.95] {
                let mut bmp = Bitmap::new(400, 320);
                let mut gfx = Gfx::new();
                let toast = crate::overlay::Bubble::toast(
                    &mut Painter::new(&mut bmp, &mut gfx, scale),
                    "Saved",
                    area,
                );
                let out = toast.presence_bounds();
                let draw = |p: &mut Painter| toast.draw_presence(p, &LIGHT, t);
                assert_eq!(changed_outside(scale, out, &draw), 0, "toast, t {t}");
                let card = crate::overlay::dialog_card(240.0, 160.0, 400.0, 320.0);
                let out = crate::overlay::dialog_presence_bounds(card);
                let draw = |p: &mut Painter| {
                    crate::anim::pop(
                        p,
                        crate::overlay::dialog_bounds(card),
                        t,
                        (0.0, crate::overlay::DIALOG_RISE),
                        |p| {
                            crate::overlay::draw_dialog_card(p, &LIGHT, card);
                        },
                    )
                };
                assert_eq!(changed_outside(scale, out, &draw), 0, "dialog, t {t}");
            }
        }
    }

    /// The hover row's fade: at rest the plain draw, on its way something else, and under high
    /// contrast or with an animator that does not animate the plain draw at once.
    #[test]
    fn the_hover_row_fades_and_is_the_plain_draw_at_rest() {
        use crate::anim::{Animator, Clock, ManualClock, Policy};
        use crate::theme::LIGHT;
        let area = Rect::new(0.0, 0.0, 300.0, 300.0);
        let shot = |menu: &mut Menu<u8>, anim: Option<&mut Animator>| {
            let mut bmp = Bitmap::new(300, 300);
            let mut gfx = Gfx::new();
            let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
            p.fill(area, LIGHT.page);
            match anim {
                Some(a) => menu.draw_in_animated(&mut p, &LIGHT, area, a, "menu"),
                None => menu.draw_in(&mut p, &LIGHT, area),
            }
            bmp.pixels
        };
        let mut menu = Menu::new(many(4), 40.0, 40.0, false);
        let clock = ManualClock::new();
        let mut anim = Animator::new();
        let mut frame = |menu: &mut Menu<u8>, ms: u64| {
            clock.advance_ms(ms);
            anim.begin_frame(clock.now());
            let px = shot(menu, Some(&mut anim));
            anim.end_frame();
            px
        };
        menu.highlight(1);
        assert!(
            frame(&mut menu, 0) == shot(&mut menu, None),
            "first seen: at rest"
        );
        menu.highlight(2);
        frame(&mut menu, 16);
        let moving = frame(&mut menu, 40);
        assert!(
            moving != shot(&mut menu, None),
            "row 2 fading in, row 1 out"
        );
        for _ in 0..20 {
            frame(&mut menu, 16);
        }
        assert!(frame(&mut menu, 16) == shot(&mut menu, None), "arrived");
        let mut still = Animator::new();
        still.set_policy(Policy::Never);
        still.begin_frame(clock.now());
        menu.highlight(3);
        assert!(shot(&mut menu, Some(&mut still)) == shot(&mut menu, None));
        still.end_frame();
        menu.set_high_contrast(true);
        menu.highlight(0);
        let mut anim = Animator::new();
        anim.begin_frame(clock.now());
        shot(&mut menu, Some(&mut anim));
        anim.end_frame();
        menu.highlight(1);
        clock.advance_ms(40);
        anim.begin_frame(clock.now());
        assert!(
            shot(&mut menu, Some(&mut anim)) == shot(&mut menu, None),
            "high contrast: at once"
        );
        anim.end_frame();
    }

    fn many(n: u8) -> Vec<Item<u8>> {
        (0..n).map(|i| Item::new(i, format!("Item {i}"))).collect()
    }

    #[test]
    fn a_menu_that_is_there_draws_as_before_and_one_opening_is_fainter_and_off_its_place() {
        use crate::theme::LIGHT;
        let area = Rect::new(0.0, 0.0, 300.0, 400.0);
        let draw = |t: Option<f32>, above: bool| {
            let mut menu = Menu::new(many(4), 60.0, 200.0, false);
            if above {
                menu = menu.above();
            }
            let mut bmp = Bitmap::new(300, 400);
            let mut gfx = Gfx::new();
            let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
            p.fill(area, LIGHT.page);
            match t {
                Some(t) => menu.draw_in_presence(&mut p, &LIGHT, area, t),
                None => menu.draw_in(&mut p, &LIGHT, area),
            }
            (bmp, menu.rect())
        };
        let mut blank = Bitmap::new(300, 400);
        blank.clear(LIGHT.page);
        for above in [false, true] {
            let (plain, rect) = draw(None, above);
            let (there, _) = draw(Some(1.0), above);
            assert!(there.pixels == plain.pixels, "above {above}: at 1");
            // Gone: placed all the same, so the caller's geometry holds, and nothing drawn.
            let (gone, placed) = draw(Some(0.0), above);
            assert!(gone.pixels == blank.pixels && placed == rect);
            // Half way: the whole menu, shadow and text and all, at half strength, and half of
            // the drop away from its place: 3 px up when it opens downwards, 3 px down when it
            // opens upwards.
            let (half, _) = draw(Some(0.5), above);
            let shift: i32 = if above { -3 } else { 3 };
            for y in 0..400i32 {
                for x in 0..300usize {
                    let from = (y + shift).clamp(0, 399) as usize;
                    for c in 0..3 {
                        let moved = plain.pixels[(from * 300 + x) * 4 + c] as u32;
                        let under = blank.pixels[(y as usize * 300 + x) * 4 + c] as u32;
                        assert_eq!(
                            half.pixels[(y as usize * 300 + x) * 4 + c] as u32,
                            (moved + under).div_ceil(2),
                            "above {above} at {x},{y}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn a_menu_that_fits_is_placed_as_before() {
        let area = Rect::new(0.0, 40.0, 800.0, 560.0);
        let menu = placed(many(5), (700.0, 50.0), area);
        assert_eq!(menu.rect().y, 50.0);
        assert!(!menu.scrolls());
        // Opened inside the title row, a menu that fits stays where it was opened.
        assert_eq!(placed(many(3), (700.0, 25.0), area).rect().y, 25.0);
        // Too low for its anchor: moved up, but not above the area.
        let menu = placed(many(10), (700.0, 400.0), area);
        assert_eq!(menu.rect().bottom(), 596.0);
        assert!(!menu.scrolls());
    }

    #[test]
    fn a_tall_menu_stays_below_the_top_of_its_area_and_scrolls() {
        let area = Rect::new(0.0, 40.0, 800.0, 400.0);
        let mut menu = placed(many(20), (700.0, 4.0), area);
        // 20 rows of 32 px and 10 px of padding do not fit in 392 px.
        assert!(menu.scrolls());
        assert_eq!(menu.rect().y, 40.0);
        assert_eq!(menu.rect().bottom(), 436.0);
        // The same for a menu that stands on its anchor.
        let above = Menu::new(many(20), 700.0, 300.0, true).above();
        let mut above = above;
        let mut bmp = Bitmap::new(100, 100);
        let mut gfx = Gfx::new();
        let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
        above.place_in(&mut p, area);
        assert_eq!(above.rect().y, 40.0);
        // Scrolling moves the rows, clamps at both ends, and the last row ends the menu.
        assert!(menu.scroll_by(64.0));
        assert_eq!(menu.scroll(), 64.0);
        assert_eq!(menu.row_rect(0).y, 40.0 + PAD - 64.0);
        assert!(menu.scroll_by(1e6));
        let last = menu.row_rect(19);
        assert_eq!(last.bottom() + PAD, menu.rect().bottom());
        assert!(!menu.scroll_by(1.0));
        let mut ended = placed(many(20), (700.0, 4.0), area);
        ended.scroll_to_end();
        let mut bmp = Bitmap::new(100, 100);
        let mut gfx = Gfx::new();
        let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
        ended.place_in(&mut p, area);
        assert_eq!(ended.scroll(), menu.scroll());
        assert!(menu.scroll_by(-1e6));
        assert_eq!(menu.scroll(), 0.0);
        assert!(!menu.scroll_by(-1.0));
    }

    #[test]
    fn hits_account_for_the_scroll_and_the_clip() {
        let area = Rect::new(0.0, 40.0, 800.0, 400.0);
        let mut menu = placed(many(20), (700.0, 4.0), area);
        menu.scroll_by(320.0);
        let r = menu.row_rect(10);
        assert!(menu.pointer(r.x + 20.0, r.cy()));
        assert_eq!(menu.hover, Some(10));
        // A row scrolled out of the menu is not under a point outside it.
        let gone = menu.row_rect(0);
        assert!(gone.bottom() < menu.rect().y);
        let r0 = menu.row_rect(19);
        assert!(!menu.pointer(r0.x + 20.0, menu.rect().bottom() + 3.0) || menu.hover.is_none());
        assert_eq!(menu.hover, None);
    }

    #[test]
    fn a_menu_whose_items_became_fewer_chooses_nothing_stale() {
        let area = Rect::new(0.0, 0.0, 400.0, 600.0);
        let mut menu = placed(many(5), (10.0, 10.0), area);
        menu.hover = Some(4);
        assert_eq!(menu.chosen(), Some(4));
        // The items are listed again while the menu is open, fewer, its rows not placed again.
        let r4 = menu.row_rect(4);
        menu.items.truncate(2);
        assert_eq!(menu.chosen(), None);
        assert!(menu.pointer(r4.x + 20.0, r4.cy()) && menu.hover.is_none());
        menu.hover = Some(3);
        menu.step(true);
        assert_eq!(menu.hover, Some(0));
        assert_eq!(menu.chosen(), Some(0));
    }

    #[test]
    fn labels_are_read_for_access_keys() {
        let read = |s: &str| {
            let a = parse_access(s);
            let key = a.key.clone().map(|k| a.text[k].to_string());
            (a.text, key)
        };
        assert_eq!(
            read("Open &files…"),
            ("Open files…".into(), Some("f".into()))
        );
        assert_eq!(read("&&"), ("&".into(), None));
        assert_eq!(
            read("Fish && &Chips"),
            ("Fish & Chips".into(), Some("C".into()))
        );
        // A `&` before a space or at the end is the character itself.
        assert_eq!(read("A & B"), ("A & B".into(), None));
        assert_eq!(read("Save &"), ("Save &".into(), None));
        assert_eq!(read("&"), ("&".into(), None));
        // The first mark wins; a later one loses its `&` and marks nothing.
        assert_eq!(
            read("&Alpha &beta"),
            ("Alpha beta".into(), Some("A".into()))
        );
        // Non-ASCII letters: the range is in bytes of the text drawn.
        let a = parse_access("Çı&kış &Ç");
        assert_eq!(a.text, "Çıkış Ç");
        assert_eq!(a.key, Some(4..5));
        assert_eq!(access_key_of("&Ünite"), Some('Ü'));
        assert_eq!(access_key_of("No mark"), None);
        assert_eq!(access_key_of("A && B"), None);
        assert_eq!(escape_access("Q&A & more"), "Q&&A && more");
        assert_eq!(parse_access(&escape_access("Q&A && &x")).text, "Q&A && &x");
        assert!(parse_access(&escape_access("Q&A & &x")).key.is_none());
        // Case-insensitive, by Unicode lower case.
        assert!(same_letter('f', 'F') && same_letter('F', 'f'));
        assert!(same_letter('Ç', 'ç') && same_letter('ç', 'Ç'));
        assert!(same_letter('Ö', 'ö'));
        assert!(!same_letter('f', 'g'));
        assert!(!same_letter('ç', 'c'));
    }

    fn menu_of(labels: &[&str], access: bool) -> Menu<u8> {
        let items = labels
            .iter()
            .enumerate()
            .map(|(i, l)| Item::new(i as u8, *l))
            .collect();
        Menu::new(items, 10.0, 10.0, false).access_keys(access)
    }

    fn placed_width(menu: &mut Menu<u8>) -> f32 {
        let mut bmp = Bitmap::new(600, 400);
        let mut gfx = Gfx::new();
        let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
        menu.place(&mut p, 600.0, 400.0);
        menu.rect().w
    }

    #[test]
    fn the_ampersand_is_not_measured() {
        let long = "x".repeat(24);
        let plain = format!("Open {long} files…");
        let marked = format!("Open {long} &files…");
        let both = format!("Open {long} &&files…");
        let mut a = menu_of(&[&plain], true);
        let mut b = menu_of(&[&marked], true);
        assert_eq!(placed_width(&mut a), placed_width(&mut b));
        // `&&` draws one `&`, which has width.
        let mut c = menu_of(&[&both], true);
        assert!(placed_width(&mut c) > placed_width(&mut a));
        // A menu that does not read access keys draws the label as it is, `&` included.
        let mut d = menu_of(&[&marked], false);
        assert!(placed_width(&mut d) > placed_width(&mut a));
    }

    #[test]
    fn a_letter_activates_the_first_enabled_item_with_that_key() {
        let mut menu = menu_of(
            &["&New", "&Open files…", "Sa&ve", "Çık&ış", "O&pen again"],
            true,
        );
        menu.items[1].enabled = false;
        // Case does not matter, and a disabled item is skipped for the next one with the key.
        assert_eq!(menu.access_key('n'), Some(0));
        assert_eq!(menu.hover, Some(0));
        assert_eq!(menu.access_key('N'), Some(0));
        assert_eq!(menu.access_key('o'), None);
        assert_eq!(menu.access_key('p'), Some(4));
        assert_eq!(menu.hover, Some(4));
        assert_eq!(menu.chosen(), Some(4));
        assert_eq!(menu.access_key('V'), Some(2));
        assert_eq!(menu.access_key('Ş'), None);
        assert_eq!(menu.access_key('x'), None);
        // The same key on two enabled items: the first.
        let mut two = menu_of(&["&Copy", "Co&py path", "C&ut"], true);
        assert_eq!(two.access_key('c'), Some(0));
        assert_eq!(two.access_key('y'), None);
        assert_eq!(two.access_key('u'), Some(2));
        // Non-ASCII, by Unicode lower case.
        let mut turkish = menu_of(&["&Çıkış", "&Ünite", "&İlk"], true);
        assert_eq!(turkish.access_key('ç'), Some(0));
        assert_eq!(turkish.access_key('Ç'), Some(0));
        assert_eq!(turkish.access_key('ü'), Some(1));
        // A menu that does not read access keys never answers.
        let mut off = menu_of(&["&New", "Sa&ve"], false);
        assert_eq!(off.access_key('n'), None);
        assert_eq!(off.hover, None);
        // A label without a mark has no key, in a menu that reads them.
        let mut none = menu_of(&["New", "Open"], true);
        assert_eq!(none.access_key('n'), None);
        assert_eq!(none.hover, None);
    }

    fn drawn(menu: &mut Menu<u8>) -> Vec<u8> {
        let mut bmp = Bitmap::new(400, 300);
        let mut gfx = Gfx::new();
        let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
        menu.draw(&mut p, &crate::theme::LIGHT, 400.0, 300.0);
        bmp.pixels
    }

    #[test]
    fn marks_are_underlined_only_where_asked_and_unmarked_menus_draw_as_before() {
        let labels = ["&New", "Open", "Sa&ve"];
        // Labels without a `&` draw identically whether the menu reads access keys or not, and
        // whether the underlines are shown.
        let plain = ["New", "Open", "Save"];
        let reference = drawn(&mut menu_of(&plain, false));
        assert_eq!(reference, drawn(&mut menu_of(&plain, true)));
        let mut hidden = menu_of(&plain, true);
        hidden.set_access_visible(false);
        assert_eq!(reference, drawn(&mut hidden));
        // With marks: the text is that of the unmarked labels, plus an underline when shown.
        let mut shown = menu_of(&labels, true);
        let with_line = drawn(&mut shown);
        let mut quiet = menu_of(&labels, true);
        quiet.set_access_visible(false);
        let without_line = drawn(&mut quiet);
        assert_eq!(without_line, reference, "no mark is visible when hidden");
        // A Mac has no access keys: the marks are read but never drawn.
        if crate::keys::MAC {
            assert_eq!(with_line, reference, "a Mac draws no underline");
            return;
        }
        assert_ne!(with_line, reference, "the marks are underlined");
        // Menu width and row positions are the same, so the underline is all that differs, and
        // it is on the marked letters' rows only (rows 0 and 2).
        let rows = shown.rect();
        let (r0, r1, r2) = (shown.row_rect(0), shown.row_rect(1), shown.row_rect(2));
        let differs = |r: Rect| {
            let (y0, y1) = (r.y as usize, r.bottom() as usize);
            (y0..y1).any(|y| {
                let row = |b: &[u8]| {
                    let start = (y * 400 + rows.x as usize) * 4;
                    b[start..start + rows.w as usize * 4].to_vec()
                };
                row(&with_line) != row(&reference)
            })
        };
        assert!(differs(r0) && differs(r2));
        assert!(!differs(r1));
    }
    #[test]
    fn stepping_keeps_the_row_in_view_and_wraps() {
        let area = Rect::new(0.0, 40.0, 800.0, 400.0);
        let mut menu = placed(many(20), (700.0, 4.0), area);
        let mut bmp = Bitmap::new(100, 100);
        let mut gfx = Gfx::new();
        let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
        for k in 0..20 {
            menu.step(true);
            menu.place_in(&mut p, area);
            let r = menu.row_rect(menu.hover.unwrap());
            assert_eq!(menu.hover, Some(k));
            assert!(
                r.y >= menu.rect().y && r.bottom() <= menu.rect().bottom(),
                "row {k}"
            );
        }
        // The last row is in view with the menu scrolled to its end.
        assert_eq!(menu.row_rect(19).bottom() + PAD, menu.rect().bottom());
        // Down from the last row wraps to the first, and the menu scrolls back.
        menu.step(true);
        menu.place_in(&mut p, area);
        assert_eq!(menu.hover, Some(0));
        assert_eq!(menu.scroll(), 0.0);
        // Up from the first wraps to the last.
        menu.step(false);
        menu.place_in(&mut p, area);
        assert_eq!(menu.hover, Some(19));
        assert!(menu.scroll() > 0.0);
    }

    #[test]
    fn a_least_width_widens_a_narrow_menu_and_nothing_else() {
        let plain = placed_width(&mut menu_of(&["One", "Two"], false));
        assert_eq!(plain, 180.0, "the menu's own least width");
        for (asked, got) in [
            (260.0, 260.0),
            (100.0, plain),
            (0.0, plain),
            (-5.0, plain),
            (f32::NAN, plain),
            (f32::INFINITY, plain),
            // The menu's own limit and the area still narrow it.
            (9000.0, MAX_WIDTH),
        ] {
            let mut menu = menu_of(&["One", "Two"], false).min_width(asked);
            assert_eq!(placed_width(&mut menu), got, "asked {asked}");
        }
        let mut menu = menu_of(&["One", "Two"], false).min_width(500.0);
        let mut bmp = Bitmap::new(300, 300);
        let mut gfx = Gfx::new();
        let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
        menu.place(&mut p, 300.0, 300.0);
        assert_eq!(menu.rect().w, 292.0);
    }

    #[test]
    fn a_highlight_set_by_index_is_brought_into_view_and_refuses_what_cannot_be_chosen() {
        let mut items = many(30);
        items[3].enabled = false;
        let mut menu = Menu::new(items, 10.0, 10.0, false);
        assert!(!menu.highlight(3), "a disabled item");
        assert!(!menu.highlight(30), "past the last");
        assert_eq!(menu.hover, None);
        assert!(menu.highlight(25));
        assert_eq!(menu.hover, Some(25));
        let mut bmp = Bitmap::new(300, 300);
        let mut gfx = Gfx::new();
        let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
        let area = Rect::new(0.0, 0.0, 300.0, 300.0);
        menu.place_in(&mut p, area);
        let row = menu.row_rect(25);
        assert!(row.y >= menu.rect().y && row.bottom() <= menu.rect().bottom());
        assert!(menu.highlight(0));
        menu.place_in(&mut p, area);
        assert_eq!(menu.scroll(), 0.0);
    }
}
