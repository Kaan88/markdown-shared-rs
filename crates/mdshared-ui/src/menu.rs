//! Popup menus drawn inside the window: the main menu, Recent and the right-click menus.

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
const PAD: f32 = 5.0;
const MAX_WIDTH: f32 = 420.0;

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
        }
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
        for item in &self.items {
            let hint = if item.hint.is_empty() {
                0.0
            } else {
                p.measure(Face::Ui, 12.0, &item.hint) + 28.0
            };
            let (label, _) = self.shown(&item.label);
            w = w.max(lead + p.measure(Face::Ui, 13.5, &label) + hint + 16.0);
        }
        let w = w.min(MAX_WIDTH).min(area.w - 8.0);
        let h: f32 = 2.0 * PAD
            + self
                .items
                .iter()
                .map(|i| ROW + if i.separator_before { SEP } else { 0.0 })
                .sum::<f32>();
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
            // Where it was opened, even when that is in the title row (a tab's own menu); only
            // being pushed up to fit stops at the area's top.
            self.anchor.1
        };
        x = x.min(area.right() - w - 4.0).max(area.x + 4.0);
        if y + h > bottom {
            y = (bottom - h).max(top);
        }
        let shown = h.min(bottom - top);
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
            if self.hover == Some(i) && item.enabled {
                p.rbox(r, 5.0, pal.hover);
            }
            let color = if item.enabled { pal.text } else { pal.faint };
            let mut x = r.x + 9.0;
            if marks || icons {
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
            let hint_w = if item.hint.is_empty() {
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
            .filter(|&i| self.items[i].enabled);
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

    /// The highlighted item's id, for a click or Enter.
    pub fn chosen(&self) -> Option<T> {
        self.hover
            .filter(|&i| self.items[i].enabled)
            .map(|i| self.items[i].id.clone())
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

    fn many(n: u8) -> Vec<Item<u8>> {
        (0..n).map(|i| Item::new(i, format!("Item {i}"))).collect()
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
}
