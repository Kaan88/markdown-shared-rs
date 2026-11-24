//! Generic widgets for a desktop-style shell drawn with [`crate::gfx::Painter`]: a menu bar
//! with drop-down menus, sortable column headers, a one-line text field and push buttons. Nothing
//! here knows about files; the application owns the state and hands it in each frame.

use crate::gfx::{Face, Painter, Rect, Rgb, rgb};

/// Colours the widgets draw with.
#[derive(Clone, Copy)]
pub struct Look {
    pub bar: Rgb,
    pub menu: Rgb,
    pub menu_border: Rgb,
    pub text: Rgb,
    pub dim: Rgb,
    pub hover: Rgb,
    pub accent: Rgb,
    pub accent_text: Rgb,
    pub header: Rgb,
    pub field: Rgb,
    pub field_border: Rgb,
    pub size: f32,
}

impl Look {
    pub fn dark() -> Look {
        Look {
            bar: rgb(0x2b2b2b),
            menu: rgb(0x2b2b2b),
            menu_border: rgb(0x5a5a5a),
            text: rgb(0xf0f0f0),
            dim: rgb(0xa0a0a0),
            hover: rgb(0x414141),
            accent: rgb(0x2f7fd8),
            accent_text: rgb(0xffffff),
            header: rgb(0x2f2f2f),
            field: rgb(0x1e1e1e),
            field_border: rgb(0x5a5a5a),
            size: 13.0,
        }
    }
}

// ---------------------------------------------------------------- menu bar

/// One item of a drop-down: a separator, or a label with an optional shortcut and check mark.
#[derive(Clone)]
pub struct MenuItem<C> {
    pub label: String,
    pub shortcut: String,
    pub checked: bool,
    pub command: Option<C>,
}

impl<C> MenuItem<C> {
    pub fn new(label: &str, shortcut: &str, command: C) -> Self {
        MenuItem { label: label.into(), shortcut: shortcut.into(), checked: false, command: Some(command) }
    }
    pub fn separator() -> Self {
        MenuItem { label: String::new(), shortcut: String::new(), checked: false, command: None }
    }
    pub fn checked(mut self, on: bool) -> Self {
        self.checked = on;
        self
    }
}

pub struct Menu<C> {
    pub title: String,
    pub items: Vec<MenuItem<C>>,
}

/// The open state of a menu bar: which menu is down and which item the pointer is on.
#[derive(Default)]
pub struct MenuBar {
    pub open: Option<usize>,
    pub hover: Option<usize>,
    titles: Vec<Rect>,
    drop: Rect0,
}

#[derive(Default, Clone, Copy)]
struct Rect0(f32, f32, f32, f32);

pub const ITEM_H: f32 = 22.0;
const SEP_H: f32 = 9.0;

impl MenuBar {
    /// Draws the titles along `bar`.
    pub fn draw_bar<C>(&mut self, p: &mut Painter, look: &Look, bar: Rect, menus: &[Menu<C>]) {
        p.fill(bar, look.bar);
        self.titles.clear();
        let mut x = bar.x + 2.0;
        for (i, m) in menus.iter().enumerate() {
            let w = p.measure(Face::Ui, look.size, &m.title) + 14.0;
            let r = Rect::new(x, bar.y, w, bar.h);
            if self.open == Some(i) {
                p.fill(r, look.hover);
            }
            p.text_center(Face::Ui, look.size, r, &m.title, look.text);
            self.titles.push(r);
            x += w;
        }
    }

    /// Draws the open drop-down, if any. Call after everything else so it is on top.
    pub fn draw_drop<C>(&mut self, p: &mut Painter, look: &Look, menus: &[Menu<C>]) {
        let Some(i) = self.open else { return };
        let (Some(m), Some(t)) = (menus.get(i), self.titles.get(i).copied()) else { return };
        let mut lw: f32 = 0.0;
        let mut sw: f32 = 0.0;
        let mut h = 6.0;
        for it in &m.items {
            if it.command.is_none() {
                h += SEP_H;
            } else {
                lw = lw.max(p.measure(Face::Ui, look.size, &it.label));
                sw = sw.max(p.measure(Face::Ui, look.size, &it.shortcut));
                h += ITEM_H;
            }
        }
        let w = 34.0 + lw + 40.0 + sw + 16.0;
        let r = Rect::new(t.x, t.bottom(), w.max(160.0), h);
        self.drop = Rect0(r.x, r.y, r.w, r.h);
        p.shadow(r, 2.0, true);
        p.frame(r, 0.0, look.menu, look.menu_border);
        let mut y = r.y + 3.0;
        for (k, it) in m.items.iter().enumerate() {
            if it.command.is_none() {
                p.fill(Rect::new(r.x + 8.0, y + SEP_H / 2.0, r.w - 16.0, 1.0), look.menu_border);
                y += SEP_H;
                continue;
            }
            let row = Rect::new(r.x + 2.0, y, r.w - 4.0, ITEM_H);
            if self.hover == Some(k) {
                p.fill(row, look.hover);
            }
            if it.checked {
                p.icon(crate::glyph::CHECK, 14.0, row.x + 14.0, row.cy(), look.text);
            }
            p.text(Face::Ui, look.size, row.x + 32.0, row.cy(), &it.label, look.text, lw + 8.0);
            p.text_right(Face::Ui, look.size, row.right() - 14.0, row.cy(), &it.shortcut, look.text);
            y += ITEM_H;
        }
    }

    /// The menu title under a point.
    pub fn title_at(&self, x: f32, y: f32) -> Option<usize> {
        self.titles.iter().position(|r| r.contains(x, y))
    }

    pub fn drop_contains(&self, x: f32, y: f32) -> bool {
        let d = self.drop;
        self.open.is_some() && Rect::new(d.0, d.1, d.2, d.3).contains(x, y)
    }

    /// The item index under a point in the open drop-down (separators excluded).
    pub fn item_at<C>(&self, menus: &[Menu<C>], x: f32, y: f32) -> Option<usize> {
        let m = menus.get(self.open?)?;
        if !self.drop_contains(x, y) {
            return None;
        }
        let mut top = self.drop.1 + 3.0;
        for (k, it) in m.items.iter().enumerate() {
            let h = if it.command.is_none() { SEP_H } else { ITEM_H };
            if y >= top && y < top + h {
                return it.command.is_some().then_some(k);
            }
            top += h;
        }
        None
    }

    /// Moves the highlighted item by `d`, skipping separators.
    pub fn step<C>(&mut self, menus: &[Menu<C>], d: i32) {
        let Some(m) = self.open.and_then(|i| menus.get(i)) else { return };
        let n = m.items.len() as i32;
        if n == 0 {
            return;
        }
        let mut k = self.hover.map(|h| h as i32).unwrap_or(if d > 0 { -1 } else { n });
        for _ in 0..n {
            k = (k + d).rem_euclid(n);
            if m.items[k as usize].command.is_some() {
                self.hover = Some(k as usize);
                return;
            }
        }
    }
}

// ---------------------------------------------------------------- column header

pub struct Column {
    pub title: String,
    /// Share of the free width; a column with `fixed` > 0 takes exactly that many px.
    pub fixed: f32,
    pub right_aligned: bool,
}

/// The x extents of each column across `width`, the first column taking what is left.
pub fn column_spans(cols: &[Column], x: f32, width: f32) -> Vec<(f32, f32)> {
    let fixed: f32 = cols.iter().skip(1).map(|c| c.fixed).sum();
    let first = (width - fixed).max(60.0);
    let mut out = Vec::with_capacity(cols.len());
    let mut at = x;
    for (i, c) in cols.iter().enumerate() {
        let w = if i == 0 { first } else { c.fixed };
        out.push((at, w));
        at += w;
    }
    out
}

/// Draws a header row; `sorted` is the sorted column and whether it is descending.
pub fn draw_header(p: &mut Painter, look: &Look, r: Rect, cols: &[Column], sorted: Option<(usize, bool)>) {
    p.fill(r, look.header);
    let spans = column_spans(cols, r.x, r.w);
    for (i, (c, (x, w))) in cols.iter().zip(spans).enumerate() {
        let mut tx = x + 6.0;
        if let Some((s, desc)) = sorted
            && s == i
        {
            let g = if desc { crate::glyph::DOWN } else { crate::glyph::UP };
            p.icon(g, 14.0, x + 10.0, r.cy(), look.text);
            tx += 14.0;
        }
        p.text(Face::Ui, look.size, tx, r.cy(), &c.title, look.text, w - (tx - x) - 4.0);
        if i > 0 {
            p.fill(Rect::new(x, r.y + 3.0, 1.0, r.h - 6.0), look.menu_border);
        }
    }
}

pub fn header_hit(cols: &[Column], r: Rect, x: f32) -> Option<usize> {
    column_spans(cols, r.x, r.w).iter().position(|(cx, w)| x >= *cx && x < cx + w)
}

// ---------------------------------------------------------------- text field

/// A one-line text field: the text and a caret (a char index).
#[derive(Clone, Default)]
pub struct Field {
    pub text: String,
    pub caret: usize,
    /// Everything selected (typing replaces it), as a field shows its preset text.
    pub all: bool,
}

impl Field {
    pub fn new(text: &str) -> Field {
        Field { text: text.into(), caret: text.chars().count(), all: true }
    }
    fn byte(&self, c: usize) -> usize {
        self.text.char_indices().nth(c).map(|(i, _)| i).unwrap_or(self.text.len())
    }
    pub fn insert(&mut self, s: &str) {
        if self.all {
            self.text.clear();
            self.caret = 0;
            self.all = false;
        }
        let b = self.byte(self.caret);
        self.text.insert_str(b, s);
        self.caret += s.chars().count();
    }
    pub fn backspace(&mut self) {
        if self.all {
            self.text.clear();
            self.caret = 0;
            self.all = false;
            return;
        }
        if self.caret > 0 {
            self.caret -= 1;
            let b = self.byte(self.caret);
            self.text.remove(b);
        }
    }
    pub fn delete(&mut self) {
        if self.all {
            return self.backspace();
        }
        if self.caret < self.text.chars().count() {
            let b = self.byte(self.caret);
            self.text.remove(b);
        }
    }
    pub fn left(&mut self) {
        self.all = false;
        self.caret = self.caret.saturating_sub(1);
    }
    pub fn right(&mut self) {
        self.all = false;
        self.caret = (self.caret + 1).min(self.text.chars().count());
    }
    pub fn home(&mut self) {
        self.all = false;
        self.caret = 0;
    }
    pub fn end(&mut self) {
        self.all = false;
        self.caret = self.text.chars().count();
    }

    pub fn draw(&self, p: &mut Painter, look: &Look, r: Rect, focused: bool, face: Face) {
        p.frame(r, 2.0, look.field, if focused { look.accent } else { look.field_border });
        let clip = p.clip(r.inset(2.0));
        let x = r.x + 5.0;
        let stops = p.caret_stops(face, look.size, &self.text);
        let cx = stops.get(self.caret).copied().unwrap_or(0.0);
        let shift = (cx - (r.w - 14.0)).max(0.0);
        if self.all && focused && !self.text.is_empty() {
            let w = stops.last().copied().unwrap_or(0.0);
            p.fill(Rect::new(x - shift, r.y + 3.0, w, r.h - 6.0), look.accent);
        }
        p.text(face, look.size, x - shift, r.cy(), &self.text, look.text, f32::INFINITY);
        if focused {
            p.fill(Rect::new(x - shift + cx, r.y + 4.0, 1.0, r.h - 8.0), look.text);
        }
        p.restore(clip);
    }
}

// ---------------------------------------------------------------- button

pub fn button(p: &mut Painter, look: &Look, r: Rect, label: &str, default: bool, hover: bool) {
    let fill = if hover { look.hover } else { look.header };
    p.frame(r, 3.0, fill, if default { look.accent } else { look.menu_border });
    p.text_center(Face::Ui, look.size, r, label, look.text);
}
