//! Popup menus drawn inside the window: the main menu, Recent and the right-click menus.

use crate::gfx::{Face, Painter, Rect};
use crate::scroll;
use crate::theme::Palette;

#[derive(Clone, Debug)]
pub struct Item<T> {
    pub id: T,
    pub label: String,
    pub hint: String,
    /// An icon from `glyph`, or empty.
    pub icon: &'static str,
    pub enabled: bool,
    pub checked: bool,
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
            separator_before: false,
        }
    }
    pub fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = hint.into();
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
        self
    }
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
}

impl<T: Clone> Menu<T> {
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
        }
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
        let icons = self.items.iter().any(|i| !i.icon.is_empty() || i.checked);
        let lead = if icons { 40.0 } else { 14.0 };
        let mut w: f32 = 180.0;
        for item in &self.items {
            let hint = if item.hint.is_empty() {
                0.0
            } else {
                p.measure(Face::Ui, 12.0, &item.hint) + 28.0
            };
            w = w.max(lead + p.measure(Face::Ui, 13.5, &item.label) + hint + 16.0);
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
        let icons = self.items.iter().any(|i| !i.icon.is_empty() || i.checked);
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
            if icons {
                let glyph = if item.checked {
                    crate::glyph::CHECK
                } else {
                    item.icon
                };
                if !glyph.is_empty() {
                    let c = if item.checked {
                        pal.accent
                    } else if item.enabled {
                        pal.muted
                    } else {
                        pal.faint
                    };
                    p.icon(glyph, 14.0, r.x + 18.0, r.cy(), c);
                }
                x = r.x + 36.0;
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
            p.text(
                Face::Ui,
                13.5,
                x,
                r.cy(),
                &item.label,
                color,
                r.right() - x - hint_w - 8.0,
            );
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
