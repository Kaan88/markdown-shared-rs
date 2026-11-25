//! Popup menus drawn inside the window: the main menu, Recent and the right-click menus.
//!
//! A menu hangs from a point ([`Menu::new`], [`Menu::above`]) or is anchored to a rectangle
//! and kept on screen at any corner ([`Menu::around`]). Submenus, their hover delay and their
//! keys are [`submenu::Cascade`](crate::submenu::Cascade), which is made of these menus placed
//! [`beside`](Menu::beside) one another. A menu can be drawn joined to the control that opens
//! it, the control a tab of the menu: [`join_under`], [`join_over`] and [`joins`].

use crate::gfx::{Face, LIGHT_SHADOW, Painter, Rect, Shade, soft_bounds};
use crate::scroll;
use crate::theme::Palette;
use std::borrow::Cow;
use std::ops::Range;

#[path = "menu_join.rs"]
mod join;
pub use join::{FILLET, join_over, join_under, joins};

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

/// How a menu comes in ([`Menu::draw_in_entering`]); every one is a fade as well.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Entrance {
    /// [`MENU_DROP`] px down into place (up, for a menu above its anchor).
    Drop,
    /// From its top edge down ([`crate::anim::unfold`]), as a menu hanging from its control;
    /// a menu above its anchor rises instead.
    Unfold,
    /// [`MENU_DROP`] px sideways into place, as a submenu beside its row: from the left, or from
    /// the right for one `leftward` of its parent.
    Side { leftward: bool },
    /// A fade in place, and nothing else: what a closing menu does.
    Fade,
}

/// The room the arrow of a row that opens a submenu takes at the row's right, in place of a
/// hint.
const ARROW: f32 = 28.0;

/// How a menu is drawn: its corners, its shadow, its rows and the gaps between them, in logical
/// px. It changes the frame and the geometry only; the text, the marks, the icons, the arrow and
/// the colours (the [`Palette`]'s `popup`, `popup_line` and `hover`) are the same in every look.
///
/// [`Look::ROUNDED`] is the look every menu has had, and what a menu that is given none draws,
/// pixel for pixel. [`Look::FLAT`] is square, with no shadow, shorter rows and a highlight across
/// the whole row, for an application whose own chrome is flat. One that wants something between
/// starts from either and changes fields (`let mut look = Look::FLAT; look.row = 24.0;`); the
/// struct is `non_exhaustive`, so a field added later breaks no application.
///
/// A size that is not finite, is negative, or is out of all reason (a row under 1 px or over
/// 256, any other size over 64) is taken as the rounded look's own, so a look read from a
/// settings line cannot break a menu. The shadow's strength is held to 0..=2, and one that is
/// not finite is 1.
///
/// Deliberately not here: colours (they are the palette's, or [`crate::tokens`]'), the text's
/// size, and the motion of [`Menu::draw_in_presence`], which every look shares.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct Look {
    /// The radius of the menu's corners; 0 is square.
    pub radius: f32,
    /// Whether a soft shadow is drawn round the menu.
    pub shadow: bool,
    /// How dark that shadow is ([`Painter::shadow_with`]): 1 is the shadow menus have always
    /// had, 0.5 half as dark, 0 none; [`sane`](Look::sane) holds it to 0..=2.
    pub shadow_strength: f32,
    /// Whether that shadow is the soft one a web page draws, in place of
    /// [`Painter::shadow_with`]'s: [`Painter::soft_shadow`] with [`Shade::POPUP`] (a wide layer
    /// well below the menu and a tight one just under it) at `shadow_strength`, times
    /// [`LIGHT_SHADOW`] on a light palette; a joined menu's control casts [`Shade::TAB`]
    /// ([`join_under`]). Off in [`Look::ROUNDED`] and [`Look::FLAT`], so a menu draws as it
    /// always has until its application turns it on.
    pub soft_shadow: bool,
    /// The height of a row.
    pub row: f32,
    /// The room above the first row and below the last.
    pub pad: f32,
    /// The room between the menu's side and a row's, and so its highlight's. 1 keeps a full-row
    /// highlight inside the menu's 1 px line.
    pub inset: f32,
    /// The radius of the highlight's corners; 0 is square.
    pub hover_radius: f32,
    /// The room a separator takes before its row; the line is drawn across its middle.
    pub separator: f32,
    /// The gap between a menu and a submenu beside it ([`crate::submenu::GAP`] in the rounded
    /// look).
    pub gap: f32,
    /// The size of a label, in logical px ([`TEXT`], 13.5, in both looks here). The menu is
    /// measured at the same size, so its width follows.
    pub text: f32,
    /// Whether the check mark and the icon share one column ([`ONE_LEAD`] px wide, the label
    /// after it; a checked item shows its mark there, else its icon), as a web page's menus
    /// lay them out, in place of a column each. A menu with neither has a [`NO_LEAD`] px
    /// margin before its labels. Off in both looks here.
    pub one_lead: bool,
}

/// The size of a menu label in [`Look::ROUNDED`] and [`Look::FLAT`].
pub const TEXT: f32 = 13.5;
/// The shared column of [`Look::one_lead`], from the row's left side to the label.
pub const ONE_LEAD: f32 = 30.0;
/// The margin before a label in a [`Look::one_lead`] menu with no marks and no icons.
pub const NO_LEAD: f32 = 14.0;

impl Look {
    /// The look every menu has had: 8 px corners with a shadow, 32 px rows, 5 px of padding at
    /// every side, a highlight with 5 px corners, 9 px before a separator, submenus 2 px off.
    pub const ROUNDED: Look = Look {
        radius: 8.0,
        shadow: true,
        shadow_strength: 1.0,
        soft_shadow: false,
        row: ROW,
        pad: PAD,
        inset: PAD,
        hover_radius: 5.0,
        separator: SEP,
        gap: crate::submenu::GAP,
        text: TEXT,
        one_lead: false,
    };

    /// Square and flat: no corners and no shadow, 26 px rows with 4 px above and below, a
    /// square highlight across the whole row inside the menu's line, 9 px before a separator,
    /// and submenus against their menu with no gap.
    pub const FLAT: Look = Look {
        radius: 0.0,
        shadow: false,
        shadow_strength: 1.0,
        soft_shadow: false,
        row: 26.0,
        pad: 4.0,
        inset: 1.0,
        hover_radius: 0.0,
        separator: SEP,
        gap: 0.0,
        text: TEXT,
        one_lead: false,
    };

    /// This look with every size that is out of reason taken from [`Look::ROUNDED`], and the
    /// shadow's strength held to 0..=2 (1 when it is not finite).
    pub fn sane(self) -> Look {
        let d = Look::ROUNDED;
        let ok = |v: f32, lo: f32, hi: f32, d: f32| {
            if v.is_finite() && (lo..=hi).contains(&v) {
                v
            } else {
                d
            }
        };
        Look {
            radius: ok(self.radius, 0.0, 64.0, d.radius),
            shadow: self.shadow,
            shadow_strength: if self.shadow_strength.is_finite() {
                self.shadow_strength.clamp(0.0, 2.0)
            } else {
                d.shadow_strength
            },
            soft_shadow: self.soft_shadow,
            row: ok(self.row, 1.0, 256.0, d.row),
            pad: ok(self.pad, 0.0, 64.0, d.pad),
            inset: ok(self.inset, 0.0, 64.0, d.inset),
            hover_radius: ok(self.hover_radius, 0.0, 64.0, d.hover_radius),
            separator: ok(self.separator, 0.0, 64.0, d.separator),
            gap: ok(self.gap, 0.0, 64.0, d.gap),
            text: ok(self.text, 6.0, 48.0, d.text),
            one_lead: self.one_lead,
        }
    }
}

impl Look {
    /// The strength of the soft shadow on `pal`: [`shadow_strength`](Look::shadow_strength),
    /// times [`LIGHT_SHADOW`] on a light palette.
    pub(crate) fn soft_strength(&self, pal: &Palette) -> f32 {
        self.shadow_strength * if pal.dark { 1.0 } else { LIGHT_SHADOW }
    }
}

impl Default for Look {
    /// [`Look::ROUNDED`].
    fn default() -> Look {
        Look::ROUNDED
    }
}

/// How a menu placed by [`Menu::around`] or [`Menu::beside`] stands to its rectangle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Around {
    /// Below the rectangle, flipped above or to the left when it does not fit.
    Below(Rect),
    /// Beside the rectangle (an item as wide as its menu), on the left first when `true`.
    Beside(Rect, bool),
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
    around: Option<Around>,
    /// The last placement by a rectangle put the menu to the left of it, or above it.
    went_left: bool,
    went_up: bool,
    /// Which rows open a submenu and draw an arrow; empty in a menu without submenus.
    branches: Vec<bool>,
    /// How the menu is drawn, made sane when it was given.
    look: Look,
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
            look: Look::ROUNDED,
        }
    }

    /// The same menu drawn in `look` (default [`Look::ROUNDED`]), its sizes made
    /// [`sane`](Look::sane). It changes where the rows are, so a menu given a look while open
    /// is placed again by the next `place` or draw.
    pub fn with_look(mut self, look: Look) -> Self {
        self.look = look.sane();
        self
    }

    /// Changes the look of a menu that was made already ([`with_look`](Self::with_look)), for an
    /// application whose own code made it. It takes effect at the next `place` or draw.
    pub fn set_look(&mut self, look: Look) {
        self.look = look.sane();
    }

    /// The look the menu is drawn in.
    pub fn look(&self) -> Look {
        self.look
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
        let look = self.look;
        let lead = if look.one_lead {
            look.inset + if marks || icons { ONE_LEAD } else { NO_LEAD }
        } else {
            (if marks || icons { 40.0 } else { 14.0 }) + if marks && icons { MARK } else { 0.0 }
        };
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
            w = w.max(lead + p.measure(Face::Ui, look.text, &label) + hint + 16.0);
        }
        let w = w.min(MAX_WIDTH).min(area.w - 8.0);
        let h: f32 = 2.0 * look.pad
            + self
                .items
                .iter()
                .map(|i| {
                    look.row
                        + if i.separator_before {
                            look.separator
                        } else {
                            0.0
                        }
                })
                .sum::<f32>();
        // The top of a menu that stands on its anchor's top, kept as it is (not rounded again).
        let mut stands = None;
        let (x, y, shown) = match self.around {
            Some(around) => {
                let placed = match around {
                    Around::Below(anchor) => crate::submenu::around(w, h, anchor, area),
                    Around::Beside(item, leftward) => {
                        crate::submenu::beside_with(w, h, item, area, leftward, &look)
                    }
                };
                self.went_left = placed.left;
                self.went_up = placed.above;
                let (mut x, mut y) = (placed.rect.x, placed.rect.y);
                if let Around::Below(anchor) = around {
                    // A menu hung from its anchor's right edge keeps that edge on the anchor's
                    // after rounding: x and w rounded apart could leave it most of a px short,
                    // and a joined menu (`joins`) apart from its control. So the right edge is
                    // rounded and the left taken from it; likewise the bottom of a menu standing
                    // on its anchor's top, whose top is then not rounded again (its height may be
                    // a fraction in a look of fractional sizes).
                    if placed.left && x == anchor.right() - placed.rect.w {
                        x = anchor.right().round() - w.round();
                    }
                    if placed.above && y == anchor.y - h {
                        y = anchor.y.round() - placed.rect.h;
                        stands = Some(y);
                    }
                }
                (x, y, placed.rect.h)
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
        self.rect = Rect::new(x.round(), stands.unwrap_or(y.round()), w.round(), shown);
        self.content = h;
        let max = (h - shown).max(0.0);
        if let Some(i) = self.reveal.take() {
            let (off, row) = row_span_with(&self.items, i, &look);
            if off - look.pad < self.scroll {
                self.scroll = off - look.pad;
            } else if off + row + look.pad > self.scroll + shown {
                self.scroll = off + row + look.pad - shown;
            }
        }
        self.scroll = self.scroll.clamp(0.0, max).round().min(max);
        self.rows.clear();
        let mut ry = self.rect.y + look.pad - self.scroll;
        for item in &self.items {
            if item.separator_before {
                ry += look.separator;
            }
            self.rows.push(Rect::new(
                self.rect.x + look.inset,
                ry,
                (self.rect.w - 2.0 * look.inset).max(0.0),
                look.row,
            ));
            ry += look.row;
        }
    }

    pub fn draw(&mut self, p: &mut Painter, pal: &Palette, width: f32, height: f32) {
        self.draw_in(p, pal, Rect::new(0.0, 0.0, width, height));
    }

    /// Places the menu inside `area` ([`place_in`](Self::place_in)) and draws it.
    pub fn draw_in(&mut self, p: &mut Painter, pal: &Palette, area: Rect) {
        self.place_in(p, area);
        let (marks, icons) = self.columns();
        let look = self.look;
        if look.shadow {
            if look.soft_shadow {
                let k = look.soft_strength(pal);
                p.soft_shadow(self.rect, look.radius, &Shade::POPUP, k);
            } else {
                p.shadow_with(self.rect, look.radius, pal.dark, look.shadow_strength);
            }
        }
        p.frame(self.rect, look.radius, pal.popup, pal.popup_line);
        let old = p.clip(self.rect);
        for (i, item) in self.items.iter().enumerate() {
            let r = self.rows[i];
            if item.separator_before {
                let line = Rect::new(
                    self.rect.x + 1.0,
                    r.y - look.separator / 2.0 - 0.5,
                    self.rect.w - 2.0,
                    1.0,
                );
                p.fill(line, pal.popup_line);
            }
            if self.hover == Some(i) && item.enabled {
                if look.hover_radius > 0.0 {
                    p.rbox(r, look.hover_radius, pal.hover);
                } else {
                    p.fill(r, pal.hover);
                }
            }
            let color = if item.enabled { pal.text } else { pal.faint };
            let mut x = r.x + 9.0;
            if look.one_lead {
                // One column for both, the label after it: the mark when the item is checked,
                // else its icon.
                let col = r.x + ONE_LEAD / 2.0;
                if item.checked {
                    p.icon(crate::glyph::CHECK, 14.0, col, r.cy(), pal.accent);
                } else if !item.icon.is_empty() {
                    let c = if item.enabled { pal.muted } else { pal.faint };
                    p.icon(item.icon, 14.0, col, r.cy(), c);
                }
                x = r.x + if marks || icons { ONE_LEAD } else { NO_LEAD };
            } else if marks || icons {
                // The check mark has a column of its own before the icons, so an item shows both
                // what it is and whether it is on. A menu without icons has the mark where the
                // icons would be.
                let mut col = r.x + 18.0;
                if item.checked {
                    p.icon(crate::glyph::CHECK, 14.0, col, r.cy(), pal.accent);
                }
                if marks && icons {
                    col += MARK;
                }
                if icons && !item.icon.is_empty() {
                    let c = if item.enabled { pal.muted } else { pal.faint };
                    p.icon(item.icon, 14.0, col, r.cy(), c);
                }
                x = col + 18.0;
            }
            let hint_w = if self.opens_submenu(i) {
                let c = if item.enabled { pal.muted } else { pal.faint };
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
                    pal.faint,
                ) + 18.0
            };
            let (label, key) = self.shown(&item.label);
            let room = r.right() - x - hint_w - 8.0;
            p.text(Face::Ui, look.text, x, r.cy(), &label, color, room);
            if let Some(key) = key.filter(|_| self.access_visible) {
                // Only when the label was drawn whole: an elided one may have cut the letter.
                if p.measure(Face::Ui, look.text, &label) <= room {
                    p.underline_range(Face::Ui, look.text, x, r.cy(), &label, key, color);
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
        let bounds = self.presence_bounds();
        crate::anim::pop(p, bounds, t, (0.0, drop), |p| self.draw_in(p, pal, area));
    }

    /// What the menu draws where it was last placed, its shadow included: the rectangle a
    /// fade or a slide of it must take whole. 18 px round it for the toolkit's shadow; the
    /// soft shadow's reach ([`soft_bounds`]) in a look with [`Look::soft_shadow`].
    pub(crate) fn presence_bounds(&self) -> Rect {
        if self.look.shadow && self.look.soft_shadow {
            soft_bounds(self.rect, &Shade::POPUP)
        } else {
            self.rect.inset(-18.0)
        }
    }

    /// Places the menu inside `area` and draws it at presence `t` with the entrance `how`: the
    /// same as [`draw_in_presence`](Self::draw_in_presence) for [`Entrance::Drop`], and at 1
    /// [`draw_in`](Self::draw_in) pixel for pixel whatever `how` is.
    pub fn draw_in_entering(
        &mut self,
        p: &mut Painter,
        pal: &Palette,
        area: Rect,
        t: f32,
        how: Entrance,
    ) {
        self.draw_entering_with(p, area, t, how, |p, m| m.draw_in(p, pal, area));
    }

    /// As [`draw_in_entering`](Self::draw_in_entering), with the drawing given: `draw` draws
    /// the menu and anything that belongs to it (a submenu's arrow, a badge), all of it moving
    /// and fading as one thing. It is called with the menu placed, and not at all at 0.
    pub fn draw_entering_with(
        &mut self,
        p: &mut Painter,
        area: Rect,
        t: f32,
        how: Entrance,
        draw: impl FnOnce(&mut Painter, &mut Self),
    ) {
        self.place_in(p, area);
        let drop = if self.opened_above() {
            MENU_DROP
        } else {
            -MENU_DROP
        };
        let shift = match how {
            Entrance::Drop => (0.0, drop),
            // A menu above its control cannot unfold downward from it: it rises, as a dropped
            // one above does.
            Entrance::Unfold if self.opened_above() => (0.0, drop),
            Entrance::Unfold => {
                let (rect, bounds) = (self.rect, self.presence_bounds());
                crate::anim::unfold(p, rect, bounds, t, |p| draw(p, self));
                return;
            }
            Entrance::Side { leftward: false } => (-MENU_DROP, 0.0),
            Entrance::Side { leftward: true } => (MENU_DROP, 0.0),
            Entrance::Fade => (0.0, 0.0),
        };
        let bounds = self.presence_bounds();
        crate::anim::pop(p, bounds, t, shift, |p| draw(p, self));
    }

    /// Everything the menu draws where it was last placed, its shadow included: what a fade of
    /// it must take whole.
    pub fn drawn_bounds(&self) -> Rect {
        self.presence_bounds()
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
/// An index past the last item gives the offset a row would have there. This is the rounded
/// look's layout; a menu drawn in another is [`row_span_with`].
pub fn row_span<T>(items: &[Item<T>], index: usize) -> (f32, f32) {
    row_span_with(items, index, &Look::ROUNDED)
}

/// [`row_span`] for a menu drawn in `look` ([`Menu::with_look`]), its sizes made
/// [`sane`](Look::sane) as the menu makes them.
pub fn row_span_with<T>(items: &[Item<T>], index: usize, look: &Look) -> (f32, f32) {
    let look = look.sane();
    let mut y = look.pad;
    for (i, item) in items.iter().enumerate() {
        if item.separator_before {
            y += look.separator;
        }
        if i == index {
            return (y, look.row);
        }
        y += look.row;
    }
    (y, look.row)
}

#[cfg(test)]
#[path = "menu_tests.rs"]
mod tests;
