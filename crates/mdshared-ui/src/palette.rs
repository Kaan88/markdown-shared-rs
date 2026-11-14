//! The command palette: one card near the top centre of the window with a text field and a list of
//! ranked entries. The application owns what the entries are (commands, recent files, files in
//! the folder, lines, symbols) and swaps them when [`Outcome::QueryChanged`] says the text moved,
//! for instance on a leading ">" or ":"; the palette owns the query box, the ranking, the
//! selection and the scroll.
//!
//! Hosting: keep a `Palette` (or an `Option` of one) in the UI state. While it is open, draw it
//! last with [`Palette::draw`] and forward keys ([`key`](Palette::key)), the pointer
//! ([`pointer_move`](Palette::pointer_move), [`press`](Palette::press),
//! [`release`](Palette::release)) and the wheel ([`wheel`](Palette::wheel)) to it instead of the
//! page. There is no scrim, so the page stays visible, but nothing reaches it.
//!
//! All coordinates are logical px. No callbacks, no threads: every event returns an [`Outcome`].

use crate::gfx::{Face, Painter, Rect, Rgb};
use crate::glyph;
use crate::keys::{KeyName, Mods};
use crate::scroll;
use crate::textfield::{FieldColors, TextField};
use crate::theme::Palette as Colors;
use mdshared_app::fuzzy;

/// The widest the card gets, and the margin it keeps from the window's sides.
const CARD_W: f32 = 640.0;
const MARGIN: f32 = 24.0;
const FIELD_H: f32 = 50.0;
const ROW: f32 = 34.0;
const HEADER: f32 = 30.0;
const PAD: f32 = 6.0;
const FOOTER_H: f32 = 28.0;
/// Rows the list shows before it scrolls.
pub const MAX_ROWS: usize = 12;
/// The most results kept for a query.
const MAX_RESULTS: usize = 300;

/// One thing the palette can choose.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    /// The application's own number, given back by [`Outcome::Chosen`].
    pub id: u32,
    pub label: String,
    /// The shortcut or the path, right-aligned and muted.
    pub hint: String,
    /// An icon from [`glyph`], or empty.
    pub icon: &'static str,
    /// The application's section number: entries of one group are listed together under a header
    /// (see [`Palette::set_groups`]) while the query is empty.
    pub group: u8,
    /// A disabled entry is listed but cannot be chosen.
    pub enabled: bool,
}

impl Entry {
    pub fn new(id: u32, label: impl Into<String>) -> Entry {
        Entry {
            id,
            label: label.into(),
            hint: String::new(),
            icon: "",
            group: 0,
            enabled: true,
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
    pub fn group(mut self, group: u8) -> Self {
        self.group = group;
        self
    }
    pub fn enabled(mut self, on: bool) -> Self {
        self.enabled = on;
        self
    }
}

/// What an event did. Redraw on everything but `None` and `Unused`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Nothing changed.
    None,
    /// The view changed (caret, selection, scroll, hover); nothing for the application to do.
    Redraw,
    /// The query text changed: swap the entry source if the prefix says so, and call
    /// [`Palette::set_entries`] / [`Palette::set_message`]. The palette has already ranked the
    /// entries it holds.
    QueryChanged,
    /// The entry with this id was chosen (Enter or a click). The palette stays as it is; the
    /// application closes it, or keeps it open for a mode change.
    Chosen(u32),
    /// Enter (or a click on the message row) with no entry to choose: act on
    /// [`Palette::query`], such as "go to line 120".
    Submitted,
    /// Escape, or a click outside the card.
    Closed,
    /// A key the palette does not use: the application may treat it as a shortcut.
    Unused,
}

#[derive(Clone, Debug)]
struct Shown {
    entry: usize,
    positions: Vec<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Line {
    /// The group's header, with the group number.
    Header(u8),
    /// An entry, by index into the shown list.
    Item(usize),
    /// The message row.
    Message,
}

#[derive(Clone, Copy, Debug)]
struct Placed {
    line: Line,
    /// From the top of the list's content.
    y: f32,
    h: f32,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Drag {
    None,
    /// The scrollbar thumb, grabbed this far below its top.
    Bar(f32),
    /// A selection in the query box.
    Field,
}

pub struct Palette {
    entries: Vec<Entry>,
    groups: Vec<(u8, String)>,
    field: TextField,
    placeholder: String,
    message: Option<String>,
    /// How many chars at the start of the query the ranking ignores (a mode prefix).
    skip: usize,

    shown: Vec<Shown>,
    lines: Vec<Placed>,
    /// Where each shown entry sits in `lines`.
    item_line: Vec<usize>,
    content: f32,
    has_icons: bool,
    selected: Option<usize>,
    /// The query changed since the entries were last set, so the selection starts over.
    fresh_query: bool,
    scroll: f32,
    reveal: bool,
    /// The needle of the last ranking and the entries that matched it, for the next, longer one.
    last_needle: String,
    last_matches: Vec<usize>,
    /// Entries replaced since `last_matches` was made.
    entries_changed: bool,

    window: Rect,
    title_h: f32,
    card: Rect,
    field_box: Rect,
    field_area: Rect,
    list: Rect,
    footer: Rect,
    stops: Vec<f32>,
    stops_text: String,
    drag: Drag,
    pressed_row: Option<usize>,
    pressed_message: bool,
    bar_hover: bool,
}

impl Default for Palette {
    fn default() -> Self {
        Palette::new()
    }
}

impl Palette {
    pub fn new() -> Palette {
        Palette {
            entries: Vec::new(),
            groups: Vec::new(),
            field: TextField::new(),
            placeholder: "Search".to_string(),
            message: None,
            skip: 0,
            shown: Vec::new(),
            lines: Vec::new(),
            item_line: Vec::new(),
            content: 0.0,
            has_icons: false,
            selected: None,
            fresh_query: true,
            scroll: 0.0,
            reveal: false,
            last_needle: String::new(),
            last_matches: Vec::new(),
            entries_changed: true,
            window: Rect::default(),
            title_h: 0.0,
            card: Rect::default(),
            field_box: Rect::default(),
            field_area: Rect::default(),
            list: Rect::default(),
            footer: Rect::default(),
            stops: Vec::new(),
            stops_text: String::new(),
            drag: Drag::None,
            pressed_row: None,
            pressed_message: false,
            bar_hover: false,
        }
    }

    // ---- what the application gives ----------------------------------------------------------

    /// Starts over with `query` in the box (with the caret after it) and the list at the top.
    /// The entries stay; set them again if the source depends on the query.
    pub fn open(&mut self, query: &str) {
        self.field.set_text(query);
        self.message = None;
        self.scroll = 0.0;
        self.fresh_query = true;
        self.drag = Drag::None;
        self.pressed_row = None;
        self.rebuild();
    }

    /// Replaces the entries (on open, or when the query changed the source) and ranks them against
    /// the query. The selection stays on the same entry when it is still there and the query did
    /// not change since the last call; otherwise it moves to the first entry that can be chosen.
    pub fn set_entries(&mut self, entries: Vec<Entry>) {
        let prev = self.selected_id();
        self.entries = entries;
        self.entries_changed = true;
        self.rebuild_keeping(prev);
    }

    /// The names of the groups, shown as headers while the query is empty. A group without a name
    /// has no header.
    pub fn set_groups(&mut self, groups: Vec<(u8, String)>) {
        self.groups = groups;
        self.rebuild();
    }

    /// A single row at the top of the list that is not an entry: "Go to line 120", "No matches".
    /// Enter with nothing to choose gives [`Outcome::Submitted`]. While the query matches nothing
    /// and no message is set, the palette shows "No matches" itself.
    pub fn set_message(&mut self, message: Option<String>) {
        self.message = message;
        self.rebuild();
    }

    /// The text shown in the empty box.
    pub fn set_placeholder(&mut self, text: &str) {
        self.placeholder = text.to_string();
    }

    /// Ignores the first `chars` chars of the query when ranking, for a mode prefix the
    /// application recognises (">" for commands): the box shows it, the ranking does not use it.
    pub fn set_ignore_prefix(&mut self, chars: usize) {
        if self.skip != chars {
            self.skip = chars;
            self.rebuild();
        }
    }

    // ---- what the application reads ----------------------------------------------------------

    /// The box's text, mode prefix included.
    pub fn query(&self) -> &str {
        self.field.text()
    }

    /// The query as the ranking sees it: without the ignored prefix and leading spaces.
    pub fn needle(&self) -> String {
        self.field
            .text()
            .chars()
            .skip(self.skip)
            .collect::<String>()
            .trim_start()
            .to_string()
    }

    /// Replaces the text (caret at the end). Returns nothing to do: call [`set_entries`](Self::set_entries)
    /// as for a change typed by the user.
    pub fn set_query(&mut self, query: &str) {
        self.field.set_text(query);
        self.fresh_query = true;
        self.scroll = 0.0;
        self.rebuild();
    }

    /// The id of the entry the keyboard (or the pointer) is on.
    pub fn selected_id(&self) -> Option<u32> {
        self.selected.map(|s| self.entries[self.shown[s].entry].id)
    }

    /// How many entries the list holds now.
    pub fn len(&self) -> usize {
        self.shown.len()
    }

    pub fn is_empty(&self) -> bool {
        self.shown.is_empty()
    }

    /// The ids of the entries listed, in order.
    pub fn listed_ids(&self) -> Vec<u32> {
        self.shown
            .iter()
            .map(|s| self.entries[s.entry].id)
            .collect()
    }

    /// The card as last laid out.
    pub fn rect(&self) -> Rect {
        self.card
    }

    /// Whether `(x, y)` is on the card.
    pub fn contains(&self, x: f32, y: f32) -> bool {
        self.card.contains(x, y)
    }

    /// How far the list is scrolled, in logical px.
    pub fn scroll(&self) -> f32 {
        self.scroll
    }

    /// Where the entry at list position `index` was last placed, scrolling included (it may lie
    /// outside the list's rectangle).
    pub fn row_rect(&self, index: usize) -> Rect {
        match self.item_line.get(index) {
            Some(&l) => self.line_rect(self.lines[l]),
            None => Rect::default(),
        }
    }

    /// Where the header above the entry at `index` was placed, if it has one.
    pub fn header_rect(&self, index: usize) -> Option<Rect> {
        let l = *self.item_line.get(index)?;
        let prev = *self.lines.get(l.checked_sub(1)?)?;
        matches!(prev.line, Line::Header(_)).then(|| self.line_rect(prev))
    }

    /// The list's own rectangle: the rows are clipped to it.
    pub fn list_rect(&self) -> Rect {
        self.list
    }

    /// The query box's rectangle.
    pub fn field_rect(&self) -> Rect {
        self.field_box
    }

    /// The position in the list of the selected entry.
    pub fn selected_index(&self) -> Option<usize> {
        self.selected
    }

    // ---- ranking and the list ----------------------------------------------------------------

    fn group_name(&self, g: u8) -> Option<&str> {
        self.groups
            .iter()
            .find(|(n, _)| *n == g)
            .map(|(_, s)| s.as_str())
            .filter(|s| !s.is_empty())
    }

    fn rebuild(&mut self) {
        self.rebuild_keeping(self.selected_id());
    }

    /// Ranks and lays out the list again; the selection stays on `prev_id` when the query stands.
    fn rebuild_keeping(&mut self, prev_id: Option<u32>) {
        let needle = self.needle();
        let old_len = self.shown.len();
        self.shown.clear();
        if needle.is_empty() {
            self.last_needle.clear();
            self.last_matches.clear();
            self.shown.extend((0..self.entries.len()).map(|i| Shown {
                entry: i,
                positions: Vec::new(),
            }));
        } else {
            // A longer query can only match what the shorter one matched.
            let reuse = !self.entries_changed
                && !self.last_needle.is_empty()
                && needle.starts_with(&self.last_needle);
            let pool: Vec<usize> = if reuse {
                std::mem::take(&mut self.last_matches)
            } else {
                (0..self.entries.len()).collect()
            };
            let labels: Vec<&str> = pool
                .iter()
                .map(|&i| self.entries[i].label.as_str())
                .collect();
            let ranked = fuzzy::rank(&needle, &labels, usize::MAX);
            self.last_matches = ranked.iter().map(|(k, _)| pool[*k]).collect();
            self.last_needle = needle.clone();
            self.shown
                .extend(ranked.into_iter().take(MAX_RESULTS).map(|(k, m)| Shown {
                    entry: pool[k],
                    positions: m.positions,
                }));
        }
        self.entries_changed = false;
        self.has_icons = self.entries.iter().any(|e| !e.icon.is_empty());

        // The lines: headers only while the query is empty.
        self.lines.clear();
        self.item_line.clear();
        let mut y = PAD;
        let message = self.message.is_some() || (!needle.is_empty() && self.shown.is_empty());
        if message {
            self.lines.push(Placed {
                line: Line::Message,
                y,
                h: ROW,
            });
            y += ROW;
        }
        let mut group = None;
        for (i, s) in self.shown.iter().enumerate() {
            let g = self.entries[s.entry].group;
            if needle.is_empty() && group != Some(g) && self.group_name(g).is_some() {
                self.lines.push(Placed {
                    line: Line::Header(g),
                    y,
                    h: HEADER,
                });
                y += HEADER;
            }
            group = Some(g);
            self.item_line.push(self.lines.len());
            self.lines.push(Placed {
                line: Line::Item(i),
                y,
                h: ROW,
            });
            y += ROW;
        }
        self.content = if self.lines.is_empty() { 0.0 } else { y + PAD };

        // The selection: the same entry while the query is the same, else the first.
        let keep = if self.fresh_query || old_len == 0 {
            None
        } else {
            prev_id.and_then(|id| {
                self.shown
                    .iter()
                    .position(|s| self.entries[s.entry].id == id && self.entries[s.entry].enabled)
            })
        };
        self.selected = keep.or_else(|| self.first_enabled());
        if self.fresh_query {
            self.scroll = 0.0;
        }
        self.fresh_query = false;
        self.reveal = true;
        self.relayout();
    }

    fn first_enabled(&self) -> Option<usize> {
        self.shown
            .iter()
            .position(|s| self.entries[s.entry].enabled)
    }

    fn last_enabled(&self) -> Option<usize> {
        self.shown
            .iter()
            .rposition(|s| self.entries[s.entry].enabled)
    }

    fn line_rect(&self, p: Placed) -> Rect {
        Rect::new(
            self.list.x + PAD,
            self.list.y + p.y - self.scroll,
            self.list.w - 2.0 * PAD,
            p.h,
        )
    }

    // ---- layout ------------------------------------------------------------------------------

    /// Places the card inside `window` (the whole window, in logical px; `title_h` is the height
    /// of the title row, which the card never covers) and returns it. Called by
    /// [`draw`](Self::draw); events use the last placement.
    pub fn layout(&mut self, window: Rect, title_h: f32) -> Rect {
        self.window = window;
        self.title_h = title_h;
        self.relayout()
    }

    fn relayout(&mut self) -> Rect {
        let window = self.window;
        if window.w <= 0.0 || window.h <= 0.0 {
            return self.card;
        }
        let w = (window.w - 2.0 * MARGIN)
            .min(CARD_W)
            .max(window.w.min(240.0));
        let x = (window.x + (window.w - w) / 2.0).round();
        let top = (window.y + window.h * 0.12)
            .max(window.y + self.title_h + 6.0)
            .round();
        let room = (window.bottom() - 16.0 - top - FIELD_H - FOOTER_H - 1.0).max(0.0);
        let max_list = (MAX_ROWS as f32 * ROW + 2.0 * PAD).min(room);
        let list_h = if self.content > 0.0 {
            self.content.min(max_list).max(0.0)
        } else {
            0.0
        };
        let card_h = FIELD_H
            + if list_h > 0.0 {
                1.0 + list_h + FOOTER_H
            } else {
                0.0
            };
        self.card = Rect::new(x, top, w, card_h.round());
        self.field_box = Rect::new(x, top, w, FIELD_H);
        self.field_area = Rect::new(x + 46.0, top, w - 46.0 - 18.0, FIELD_H);
        self.list = Rect::new(x, top + FIELD_H + 1.0, w, list_h);
        self.footer = Rect::new(
            x,
            self.list.bottom(),
            w,
            if list_h > 0.0 { FOOTER_H } else { 0.0 },
        );

        let max = (self.content - list_h).max(0.0);
        if self.reveal {
            self.reveal = false;
            if let Some(&l) = self.selected.and_then(|s| self.item_line.get(s)) {
                let p = self.lines[l];
                let top_off = match l.checked_sub(1).map(|k| self.lines[k]) {
                    Some(h) if matches!(h.line, Line::Header(_)) => h.y,
                    _ => p.y,
                };
                if top_off - PAD < self.scroll {
                    self.scroll = top_off - PAD;
                } else if p.y + p.h + PAD > self.scroll + list_h {
                    self.scroll = p.y + p.h + PAD - list_h;
                }
            }
        }
        self.scroll = self.scroll.clamp(0.0, max).round().min(max);
        self.card
    }

    fn bar(&self) -> Option<(Rect, Rect)> {
        scroll::scrollbar(self.list, self.content, self.list.h, self.scroll)
    }

    // ---- keyboard ----------------------------------------------------------------------------

    /// A key press. `typed` is the text the event produced, if any (what winit calls
    /// `KeyEvent::text`). Up and Down move the selection and wrap; PageUp and PageDown move it by
    /// a page; Home and End go to the first and last entry (with Shift they are the box's, to
    /// select to its ends); Enter chooses; Escape closes; everything else is the query box's, as in
    /// the find box (selection, word movement, Ctrl+A/C/X/V).
    pub fn key(&mut self, key: &KeyName, mods: Mods, typed: Option<&str>) -> Outcome {
        match key {
            KeyName::Escape => Outcome::Closed,
            KeyName::Enter => match self.selected_id() {
                Some(id) => Outcome::Chosen(id),
                None => Outcome::Submitted,
            },
            KeyName::Up if !mods.ctrl && !mods.alt => self.step(false, 1, true),
            KeyName::Down if !mods.ctrl && !mods.alt => self.step(true, 1, true),
            KeyName::PageUp => self.step(false, self.page(), false),
            KeyName::PageDown => self.step(true, self.page(), false),
            KeyName::Home if !mods.shift => self.go(self.first_enabled()),
            KeyName::End if !mods.shift => self.go(self.last_enabled()),
            KeyName::Tab => Outcome::None,
            _ => match self.field.handle_key(key, mods, typed, true) {
                Some(true) => {
                    self.fresh_query = true;
                    self.rebuild();
                    Outcome::QueryChanged
                }
                Some(false) => Outcome::Redraw,
                None => Outcome::Unused,
            },
        }
    }

    fn page(&self) -> usize {
        (((self.list.h - 2.0 * PAD) / ROW).floor() as usize)
            .saturating_sub(1)
            .max(1)
    }

    fn go(&mut self, to: Option<usize>) -> Outcome {
        if to.is_none() || to == self.selected {
            return Outcome::None;
        }
        self.selected = to;
        self.reveal = true;
        self.relayout();
        Outcome::Redraw
    }

    /// Moves the selection by `n` enabled entries; wraps at the ends when `wrap`, otherwise stops
    /// at them.
    fn step(&mut self, forward: bool, n: usize, wrap: bool) -> Outcome {
        let len = self.shown.len();
        if len == 0 {
            return Outcome::None;
        }
        let enabled = |s: &Palette, i: usize| s.entries[s.shown[i].entry].enabled;
        let mut i = self.selected;
        for _ in 0..n {
            let mut cur = i;
            let mut moved = None;
            for _ in 0..len {
                let next = match (cur, forward) {
                    (None, true) => Some(0),
                    (None, false) => Some(len - 1),
                    (Some(c), true) if c + 1 < len => Some(c + 1),
                    (Some(c), false) if c > 0 => Some(c - 1),
                    (Some(_), true) if wrap => Some(0),
                    (Some(_), false) if wrap => Some(len - 1),
                    _ => None,
                };
                match next {
                    Some(k) if enabled(self, k) => {
                        moved = Some(k);
                        break;
                    }
                    Some(k) => cur = Some(k),
                    None => break,
                }
            }
            match moved {
                Some(k) => i = Some(k),
                None => break,
            }
        }
        if i == self.selected {
            return Outcome::None;
        }
        self.selected = i;
        self.reveal = true;
        self.relayout();
        Outcome::Redraw
    }

    // ---- pointer -----------------------------------------------------------------------------

    fn entry_at(&self, x: f32, y: f32) -> Option<usize> {
        if !self.list.contains(x, y) {
            return None;
        }
        (0..self.shown.len()).find(|&i| self.row_rect(i).contains(x, y))
    }

    /// The pointer moved. Over a row it moves the selection there, so the highlight follows the
    /// pointer but not a scroll under it. Returns true when something needs redrawing.
    pub fn pointer_move(&mut self, x: f32, y: f32) -> bool {
        match self.drag {
            Drag::Bar(grab) => {
                if let Some((track, thumb)) = self.bar() {
                    let off = scroll::offset_for_thumb(
                        track,
                        thumb.h,
                        y - grab,
                        self.content,
                        self.list.h,
                    );
                    return self.set_scroll(off);
                }
                false
            }
            Drag::Field => {
                let before = self.field.caret();
                self.field.drag(x - self.field_area.x, &self.stops);
                self.field.caret() != before
            }
            Drag::None => {
                let on_bar = self.bar().is_some_and(|(t, _)| t.contains(x, y));
                let mut changed = on_bar != self.bar_hover;
                self.bar_hover = on_bar;
                if let Some(i) = self.entry_at(x, y)
                    && self.entries[self.shown[i].entry].enabled
                    && self.selected != Some(i)
                {
                    self.selected = Some(i);
                    changed = true;
                }
                changed
            }
        }
    }

    fn set_scroll(&mut self, to: f32) -> bool {
        let max = (self.content - self.list.h).max(0.0);
        let to = to.clamp(0.0, max).round().min(max);
        let moved = to != self.scroll;
        self.scroll = to;
        moved
    }

    /// A mouse button went down at `(x, y)`; `clicks` is 1, or 2 for a double click. A press
    /// outside the card closes the palette; in the box it places the caret (a double click selects
    /// a word); on the scrollbar it grabs or jumps; on a row it selects it, and the release over
    /// the same row chooses it.
    pub fn press(&mut self, x: f32, y: f32, mods: Mods, clicks: u8) -> Outcome {
        self.drag = Drag::None;
        self.pressed_row = None;
        self.pressed_message = false;
        if !self.card.contains(x, y) {
            return Outcome::Closed;
        }
        if self.field_box.contains(x, y) {
            let rel = x - self.field_area.x;
            if clicks >= 2 {
                self.field.select_word_at(rel, &self.stops);
            } else {
                self.field.press(rel, &self.stops, mods.shift);
                self.drag = Drag::Field;
            }
            return Outcome::Redraw;
        }
        if let Some((track, thumb)) = self.bar()
            && track.contains(x, y)
        {
            if thumb.contains(x, y) {
                self.drag = Drag::Bar(y - thumb.y);
            } else {
                let off = scroll::offset_for_thumb(
                    track,
                    thumb.h,
                    y - thumb.h / 2.0,
                    self.content,
                    self.list.h,
                );
                self.set_scroll(off);
                self.drag = Drag::Bar(thumb.h / 2.0);
            }
            return Outcome::Redraw;
        }
        if let Some(i) = self.entry_at(x, y) {
            if self.entries[self.shown[i].entry].enabled {
                self.selected = Some(i);
                self.pressed_row = Some(i);
            }
            return Outcome::Redraw;
        }
        if let Some(p) = self.lines.iter().find(|p| p.line == Line::Message)
            && self.list.contains(x, y)
            && self.line_rect(*p).contains(x, y)
        {
            self.pressed_message = true;
        }
        Outcome::None
    }

    /// The button went up: chooses the row the press began on when the pointer is still on it.
    pub fn release(&mut self, x: f32, y: f32) -> Outcome {
        self.drag = Drag::None;
        if let Some(i) = self.pressed_row.take()
            && self.entry_at(x, y) == Some(i)
        {
            return Outcome::Chosen(self.entries[self.shown[i].entry].id);
        }
        if std::mem::take(&mut self.pressed_message)
            && self.selected.is_none()
            && let Some(p) = self.lines.iter().find(|p| p.line == Line::Message)
            && self.line_rect(*p).contains(x, y)
        {
            return Outcome::Submitted;
        }
        Outcome::None
    }

    /// The wheel: `dy` logical px, positive towards the end of the list. Returns true when the
    /// list moved.
    pub fn wheel(&mut self, dy: f32) -> bool {
        self.set_scroll(self.scroll + dy)
    }

    /// The count at the left of the footer. Empty while a message row stands in for results
    /// ("Go to line 25") and there are no entries: "0 results" would contradict it.
    fn footer_count(&self) -> String {
        if self.shown.is_empty() && self.message.is_some() {
            return String::new();
        }
        let n = self.shown.len();
        match (self.needle().is_empty(), n) {
            (true, 1) => "1 item".to_string(),
            (true, n) => format!("{n} items"),
            (false, 1) => "1 result".to_string(),
            (false, n) => format!("{n} results"),
        }
    }

    // ---- drawing -----------------------------------------------------------------------------

    /// Lays the card out in `window` and draws it. `caret_on` is the blink phase.
    pub fn draw(
        &mut self,
        p: &mut Painter,
        pal: &Colors,
        window: Rect,
        title_h: f32,
        caret_on: bool,
    ) {
        self.layout(window, title_h);
        let card = self.card;
        p.shadow(card, 10.0, pal.dark);
        p.frame(card, 10.0, pal.popup, pal.popup_line);

        // The query box.
        let fb = self.field_box;
        p.icon(glyph::SEARCH, 16.0, fb.x + 24.0, fb.cy(), pal.muted);
        if self.stops_text != self.field.text() {
            self.stops = p.caret_stops(Face::Ui, 15.0, self.field.text());
            self.stops_text = self.field.text().to_string();
        }
        let colors = FieldColors {
            text: pal.text,
            placeholder: pal.faint,
            selection: pal.selection,
            caret: pal.text,
        };
        let area = self.field_area;
        let placeholder = self.placeholder.clone();
        self.field.draw(
            p,
            area,
            Face::Ui,
            15.0,
            &colors,
            &placeholder,
            true,
            caret_on,
        );
        if self.list.h <= 0.0 {
            return;
        }
        let px = p.px();
        p.fill(
            Rect::new(card.x + 1.0, self.list.y - 1.0, card.w - 2.0, px.max(1.0)),
            pal.popup_line,
        );

        // The rows.
        let old = p.clip(self.list);
        for i in 0..self.lines.len() {
            let pl = self.lines[i];
            let r = self.line_rect(pl);
            if r.bottom() < self.list.y || r.y > self.list.bottom() {
                continue;
            }
            match pl.line {
                Line::Header(g) => {
                    let name = self.group_name(g).unwrap_or("").to_string();
                    p.text(
                        Face::Semibold,
                        12.0,
                        r.x + 10.0,
                        r.cy() + 3.0,
                        &name,
                        pal.muted,
                        r.w - 20.0,
                    );
                }
                Line::Message => {
                    let text = self.message.clone().unwrap_or_else(|| "No matches".into());
                    let accent = self.message.is_some() && self.selected.is_none();
                    if accent {
                        p.rbox(r, 6.0, pal.on);
                    }
                    p.text(
                        Face::Ui,
                        13.5,
                        r.x + 16.0,
                        r.cy(),
                        &text,
                        if accent { pal.text } else { pal.faint },
                        r.w - 32.0,
                    );
                }
                Line::Item(k) => self.draw_row(p, pal, r, k),
            }
        }
        p.restore(old);
        if let Some((track, thumb)) = self.bar() {
            scroll::draw_scrollbar(
                p,
                pal,
                track,
                thumb,
                self.bar_hover || self.drag != Drag::None,
            );
        }

        // The footer.
        let f = self.footer;
        p.fill(
            Rect::new(f.x + 1.0, f.y, f.w - 2.0, px.max(1.0)),
            pal.popup_line,
        );
        let count = self.footer_count();
        p.text(
            Face::Ui,
            11.5,
            f.x + 16.0,
            f.cy() + 1.0,
            &count,
            pal.faint,
            f.w / 3.0,
        );
        p.text_right(
            Face::Ui,
            11.5,
            f.right() - 16.0,
            f.cy() + 1.0,
            "Enter to choose    Esc to close",
            pal.faint,
        );
    }

    fn draw_row(&self, p: &mut Painter, pal: &Colors, r: Rect, k: usize) {
        let s = &self.shown[k];
        let e = &self.entries[s.entry];
        let selected = self.selected == Some(k);
        if selected {
            p.rbox(r, 6.0, pal.on);
            p.rbox(
                Rect::new(r.x + 2.0, r.cy() - 8.0, 3.0, 16.0),
                1.5,
                pal.accent,
            );
        }
        let (text, muted, hi) = if e.enabled {
            (pal.text, pal.muted, pal.accent)
        } else {
            (pal.faint, pal.faint, pal.faint)
        };
        let mut x = r.x + 14.0;
        if self.has_icons {
            if !e.icon.is_empty() {
                p.icon(
                    e.icon,
                    15.0,
                    r.x + 24.0,
                    r.cy(),
                    if selected && e.enabled {
                        pal.accent
                    } else {
                        muted
                    },
                );
            }
            x = r.x + 42.0;
        }
        let mut hint_w = 0.0;
        if !e.hint.is_empty() {
            let max = (r.w * 0.42).max(60.0);
            let shown = p.elide(Face::Ui, 12.0, &e.hint, max);
            hint_w =
                p.text_right(Face::Ui, 12.0, r.right() - 12.0, r.cy(), &shown, pal.faint) + 20.0;
        }
        let max = r.right() - 12.0 - hint_w - x;
        draw_label(p, x, r.cy(), &e.label, &s.positions, max, text, hi);
    }
}

/// `label` with the chars at `positions` (char indices, ascending) in the accent colour and
/// semibold, drawn as runs.
#[allow(clippy::too_many_arguments)]
fn draw_label(
    p: &mut Painter,
    x: f32,
    cy: f32,
    label: &str,
    positions: &[u32],
    max: f32,
    color: Rgb,
    accent: Rgb,
) {
    if positions.is_empty() {
        p.text(Face::Ui, 13.5, x, cy, label, color, max);
        return;
    }
    let mut x = x;
    let end = x + max;
    let mut run = String::new();
    let mut run_hi = false;
    let mut at = 0usize; // next position index
    let flush = |p: &mut Painter, run: &mut String, hi: bool, x: &mut f32| {
        if run.is_empty() {
            return;
        }
        let (face, c) = if hi {
            (Face::Semibold, accent)
        } else {
            (Face::Ui, color)
        };
        let room = end - *x;
        if room > 0.0 {
            *x += p.text(face, 13.5, *x, cy, run, c, room);
        }
        run.clear();
    };
    for (i, ch) in label.chars().enumerate() {
        let hi = positions.get(at) == Some(&(i as u32));
        if hi {
            at += 1;
        }
        if hi != run_hi {
            flush(p, &mut run, run_hi, &mut x);
            run_hi = hi;
        }
        run.push(ch);
    }
    flush(p, &mut run, run_hi, &mut x);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gfx::Gfx;
    use mdshared_text::Bitmap;

    const WIN: Rect = Rect::new(0.0, 0.0, 900.0, 640.0);
    const NONE: Mods = Mods {
        ctrl: false,
        shift: false,
        alt: false,
    };

    fn entries(n: u32) -> Vec<Entry> {
        (0..n)
            .map(|i| Entry::new(i, format!("Command {i}")).group((i % 2) as u8))
            .collect()
    }

    fn palette(entries: Vec<Entry>) -> Palette {
        let mut p = Palette::new();
        p.set_groups(vec![(0, "Commands".into()), (1, "Recent files".into())]);
        p.set_entries(entries);
        p.layout(WIN, 40.0);
        p
    }

    /// No group has a name, so no headers.
    fn plain(entries: Vec<Entry>) -> Palette {
        let mut p = Palette::new();
        p.set_entries(entries);
        p.layout(WIN, 40.0);
        p
    }

    fn typed(p: &mut Palette, text: &str) -> Outcome {
        let mut last = Outcome::None;
        for ch in text.chars() {
            let s = ch.to_string();
            last = p.key(&KeyName::Char(ch), NONE, Some(&s));
        }
        last
    }

    #[test]
    fn the_card_sits_near_the_top_and_never_above_the_title_row() {
        let mut p = palette(entries(3));
        let card = p.rect();
        assert_eq!(card.w, 640.0);
        assert_eq!(card.x, 130.0);
        assert_eq!(card.y, (640.0f32 * 0.12).round().max(46.0));
        // A short window pushes it below the title row.
        let c = p.layout(Rect::new(0.0, 0.0, 900.0, 200.0), 40.0);
        assert!(c.y >= 46.0, "{c:?}");
        // A narrow window: 24 px each side.
        let c = p.layout(Rect::new(0.0, 0.0, 400.0, 640.0), 40.0);
        assert_eq!((c.x, c.w), (24.0, 352.0));
    }

    #[test]
    fn at_most_twelve_rows_then_it_scrolls() {
        let p = palette((0..40).map(|i| Entry::new(i, format!("E{i}"))).collect());
        assert_eq!(p.list_rect().h, 12.0 * ROW + 2.0 * PAD);
        assert!(p.bar().is_some());
        // A few entries: the list is as tall as they are, with no scrollbar.
        let p = plain((0..3).map(|i| Entry::new(i, format!("E{i}"))).collect());
        assert_eq!(p.list_rect().h, 3.0 * ROW + 2.0 * PAD);
        assert!(p.bar().is_none());
        // Nothing to list: the card is the box alone.
        let p = palette(Vec::new());
        assert_eq!(p.rect().h, FIELD_H);
    }

    #[test]
    fn empty_query_lists_in_order_with_group_headers() {
        let items = vec![
            Entry::new(1, "Open").group(0),
            Entry::new(2, "Save").group(0),
            Entry::new(3, "notes.md").group(1),
            Entry::new(4, "todo.md").group(1),
            Entry::new(5, "x").group(9),
        ];
        let p = palette(items);
        assert_eq!(p.listed_ids(), [1, 2, 3, 4, 5]);
        let h0 = p.header_rect(0).unwrap();
        assert!(p.header_rect(1).is_none());
        let h2 = p.header_rect(2).unwrap();
        // A header sits right above its first entry; group 9 has no name and so no header.
        assert_eq!(h0.bottom(), p.row_rect(0).y);
        assert_eq!(h2.bottom(), p.row_rect(2).y);
        assert!(p.header_rect(4).is_none());
        assert_eq!(p.row_rect(2).y - p.row_rect(1).y, ROW + HEADER);
    }

    #[test]
    fn a_query_gives_a_flat_ranked_list_without_headers() {
        let items = vec![
            Entry::new(1, "Soft wrap").group(0),
            Entry::new(2, "Open File").group(1),
            Entry::new(3, "Find").group(0),
            Entry::new(4, "Zoom").group(1),
        ];
        let mut p = palette(items);
        assert_eq!(typed(&mut p, "of"), Outcome::QueryChanged);
        assert_eq!(p.listed_ids(), [2, 1]);
        assert!(p.header_rect(0).is_none() && p.header_rect(1).is_none());
        assert_eq!(p.row_rect(1).y - p.row_rect(0).y, ROW);
        assert_eq!(p.selected_id(), Some(2));
        // Matched chars are kept for the highlight.
        assert_eq!(p.shown[0].positions, [0, 5]);
        // Clearing the query brings the original order and the headers back.
        for _ in 0..2 {
            p.key(&KeyName::Backspace, NONE, None);
        }
        assert_eq!(p.listed_ids(), [1, 2, 3, 4]);
        assert!(p.header_rect(0).is_some());
    }

    #[test]
    fn longer_queries_rank_the_same_as_a_fresh_ranking() {
        let items: Vec<Entry> = (0..300)
            .map(|i| Entry::new(i, format!("item {} of {}", i * 7 % 100, i % 13)))
            .collect();
        let mut p = palette(items.clone());
        typed(&mut p, "it 1");
        let incremental = p.listed_ids();
        let mut q = palette(items);
        q.set_query("it 1");
        assert_eq!(incremental, q.listed_ids());
        // After the app swaps entries, the pool is the new entries.
        p.set_entries(vec![Entry::new(1000, "it 1 new")]);
        assert_eq!(p.listed_ids(), [1000]);
    }

    #[test]
    fn the_mode_prefix_is_not_ranked() {
        let mut p = palette(vec![Entry::new(1, "Save"), Entry::new(2, "Open")]);
        p.set_ignore_prefix(1);
        typed(&mut p, ">sa");
        assert_eq!(p.query(), ">sa");
        assert_eq!(p.needle(), "sa");
        assert_eq!(p.listed_ids(), [1]);
        p.set_query(">");
        assert_eq!(p.listed_ids(), [1, 2]);
    }

    #[test]
    fn selection_moves_and_wraps() {
        let mut p = palette(entries(5));
        assert_eq!(p.selected_index(), Some(0));
        assert_eq!(p.key(&KeyName::Down, NONE, None), Outcome::Redraw);
        assert_eq!(p.selected_index(), Some(1));
        assert_eq!(p.key(&KeyName::End, NONE, None), Outcome::Redraw);
        assert_eq!(p.selected_index(), Some(4));
        p.key(&KeyName::Down, NONE, None);
        assert_eq!(p.selected_index(), Some(0), "wraps to the first");
        p.key(&KeyName::Up, NONE, None);
        assert_eq!(p.selected_index(), Some(4), "wraps to the last");
        p.key(&KeyName::Home, NONE, None);
        assert_eq!(p.selected_index(), Some(0));
        assert_eq!(p.key(&KeyName::Home, NONE, None), Outcome::None);
        // Page keys stop at the ends instead of wrapping.
        let mut p = palette(entries(40));
        p.key(&KeyName::PageDown, NONE, None);
        assert_eq!(p.selected_index(), Some(11));
        for _ in 0..10 {
            p.key(&KeyName::PageDown, NONE, None);
        }
        assert_eq!(p.selected_index(), Some(39));
        assert_eq!(p.key(&KeyName::PageDown, NONE, None), Outcome::None);
        p.key(&KeyName::PageUp, NONE, None);
        assert_eq!(p.selected_index(), Some(28));
    }

    #[test]
    fn disabled_entries_are_skipped() {
        let items = vec![
            Entry::new(1, "a").enabled(false),
            Entry::new(2, "b"),
            Entry::new(3, "c").enabled(false),
            Entry::new(4, "d"),
        ];
        let mut p = palette(items);
        assert_eq!(p.selected_id(), Some(2));
        p.key(&KeyName::Down, NONE, None);
        assert_eq!(p.selected_id(), Some(4));
        p.key(&KeyName::Down, NONE, None);
        assert_eq!(
            p.selected_id(),
            Some(2),
            "wraps past the disabled first row"
        );
        p.key(&KeyName::End, NONE, None);
        assert_eq!(p.selected_id(), Some(4));
        // A click on a disabled row neither selects nor chooses.
        let r = p.row_rect(0);
        p.press(r.x + 40.0, r.cy(), NONE, 1);
        assert_eq!(p.release(r.x + 40.0, r.cy()), Outcome::None);
        assert_eq!(p.selected_id(), Some(4));
    }

    #[test]
    fn the_selection_follows_the_keys_into_view() {
        let mut p = palette(entries(40));
        for _ in 0..30 {
            p.key(&KeyName::Down, NONE, None);
            let r = p.row_rect(p.selected_index().unwrap());
            let l = p.list_rect();
            assert!(r.y >= l.y && r.bottom() <= l.bottom(), "{r:?} in {l:?}");
        }
        // Wrapping to the top scrolls back, header included.
        p.key(&KeyName::Home, NONE, None);
        assert_eq!(p.scroll(), 0.0);
        assert!(p.header_rect(0).unwrap().y >= p.list_rect().y);
        p.key(&KeyName::End, NONE, None);
        let r = p.row_rect(39);
        assert_eq!(r.bottom() + PAD, p.list_rect().bottom());
    }

    #[test]
    fn enter_escape_and_unused_keys() {
        let mut p = palette(entries(3));
        p.key(&KeyName::Down, NONE, None);
        assert_eq!(p.key(&KeyName::Enter, NONE, Some("\r")), Outcome::Chosen(1));
        assert_eq!(p.key(&KeyName::Escape, NONE, None), Outcome::Closed);
        assert_eq!(p.key(&KeyName::F(5), NONE, None), Outcome::Unused);
        let ctrl = Mods { ctrl: true, ..NONE };
        assert_eq!(p.key(&KeyName::Char('s'), ctrl, Some("s")), Outcome::Unused);
        // Nothing to choose: the app decides from the query.
        let mut p = palette(entries(3));
        typed(&mut p, "zzz");
        assert!(p.is_empty());
        assert_eq!(p.key(&KeyName::Enter, NONE, None), Outcome::Submitted);
    }

    #[test]
    fn the_box_edits_like_the_find_box() {
        let mut p = palette(entries(3));
        typed(&mut p, "foo bar");
        let ctrl = Mods { ctrl: true, ..NONE };
        assert_eq!(
            p.key(&KeyName::Backspace, ctrl, None),
            Outcome::QueryChanged
        );
        assert_eq!(p.query(), "foo ");
        // Moving the caret needs a redraw, not a new query.
        assert_eq!(p.key(&KeyName::Left, NONE, None), Outcome::Redraw);
        let shift = Mods {
            shift: true,
            ..NONE
        };
        assert_eq!(p.key(&KeyName::Home, shift, None), Outcome::Redraw);
        assert!(p.field.has_selection());
        assert_eq!(typed(&mut p, "x"), Outcome::QueryChanged);
        assert_eq!(p.query(), "x ");
        assert_eq!(p.key(&KeyName::Char('a'), ctrl, Some("a")), Outcome::Redraw);
        assert_eq!(p.field.selected_text(), "x ");
    }

    #[test]
    fn clicking_a_row_chooses_it_on_release() {
        let mut p = palette(entries(6));
        let r = p.row_rect(3);
        assert_eq!(p.press(r.x + 50.0, r.cy(), NONE, 1), Outcome::Redraw);
        assert_eq!(p.selected_index(), Some(3));
        assert_eq!(p.release(r.x + 50.0, r.cy()), Outcome::Chosen(3));
        // Pressed on one row and released on another: nothing chosen.
        let (a, b) = (p.row_rect(1), p.row_rect(4));
        p.press(a.x + 50.0, a.cy(), NONE, 1);
        assert_eq!(p.release(b.x + 50.0, b.cy()), Outcome::None);
        // A second release does not choose again.
        assert_eq!(p.release(a.x + 50.0, a.cy()), Outcome::None);
    }

    #[test]
    fn clicks_outside_close_and_clicks_on_the_card_do_not() {
        let mut p = palette(entries(3));
        assert_eq!(p.press(5.0, 5.0, NONE, 1), Outcome::Closed);
        let c = p.rect();
        assert_eq!(p.press(c.x - 1.0, c.cy(), NONE, 1), Outcome::Closed);
        assert_eq!(p.press(c.cx(), c.bottom() + 2.0, NONE, 1), Outcome::Closed);
        // In the footer: nothing.
        let f = p.footer;
        assert_eq!(p.press(f.cx(), f.cy(), NONE, 1), Outcome::None);
        // In the box: the caret.
        let b = p.field_rect();
        assert_eq!(p.press(b.cx(), b.cy(), NONE, 1), Outcome::Redraw);
    }

    #[test]
    fn hover_moves_the_selection_only_when_the_pointer_moves() {
        let mut p = palette(entries(8));
        let r = p.row_rect(5);
        assert!(p.pointer_move(r.x + 30.0, r.cy()));
        assert_eq!(p.selected_index(), Some(5));
        // The same spot again changes nothing.
        assert!(!p.pointer_move(r.x + 31.0, r.cy()));
        // The keyboard moves it away, and the unmoved pointer does not take it back.
        p.key(&KeyName::Home, NONE, None);
        assert_eq!(p.selected_index(), Some(0));
        // A move within the same row, though, is a move: the row under the pointer is taken.
        assert!(p.pointer_move(r.x + 32.0, r.cy()));
        assert_eq!(p.selected_index(), Some(5));
        // Over a header or outside the list the selection stays.
        let h = p.header_rect(0).unwrap();
        assert!(!p.pointer_move(h.x + 30.0, h.cy()));
        assert_eq!(p.selected_index(), Some(5));
    }

    #[test]
    fn hits_account_for_the_scroll() {
        let mut p = palette((0..40).map(|i| Entry::new(i, format!("E{i}"))).collect());
        assert!(p.wheel(ROW * 10.0));
        assert_eq!(p.scroll(), ROW * 10.0);
        let r = p.row_rect(12);
        assert!(r.y >= p.list_rect().y && r.bottom() <= p.list_rect().bottom());
        p.press(r.x + 30.0, r.cy(), NONE, 1);
        assert_eq!(p.release(r.x + 30.0, r.cy()), Outcome::Chosen(12));
        // A row scrolled out of the list is not under a point outside it.
        let gone = p.row_rect(0);
        assert!(gone.bottom() < p.list_rect().y);
        assert_eq!(p.entry_at(gone.x + 30.0, gone.cy()), None);
        // The wheel clamps at both ends.
        assert!(p.wheel(1e6));
        assert!(!p.wheel(1.0));
        assert_eq!(p.row_rect(39).bottom() + PAD, p.list_rect().bottom());
        assert!(p.wheel(-1e6));
        assert_eq!(p.scroll(), 0.0);
        assert!(!p.wheel(-1.0));
    }

    #[test]
    fn the_scrollbar_drags_and_jumps() {
        let mut p = palette((0..60).map(|i| Entry::new(i, format!("E{i}"))).collect());
        let (track, thumb) = p.bar().unwrap();
        assert_eq!(p.press(thumb.cx(), thumb.cy(), NONE, 1), Outcome::Redraw);
        assert!(p.pointer_move(thumb.cx(), thumb.cy() + 40.0));
        assert!(p.scroll() > 0.0);
        let mid = p.scroll();
        assert_eq!(p.release(0.0, 0.0), Outcome::None);
        p.pointer_move(thumb.cx(), thumb.cy() + 80.0);
        assert_eq!(p.scroll(), mid, "released");
        // A press on the track below the thumb jumps there.
        p.press(track.cx(), track.bottom() - 4.0, NONE, 1);
        assert!(p.scroll() > mid);
        p.release(0.0, 0.0);
    }

    #[test]
    fn the_message_row() {
        let mut p = plain(Vec::new());
        p.set_message(Some("Go to line 120".into()));
        assert_eq!(p.lines.len(), 1);
        assert_eq!(p.list_rect().h, ROW + 2.0 * PAD);
        assert_eq!(p.key(&KeyName::Enter, NONE, None), Outcome::Submitted);
        let r = p.line_rect(p.lines[0]);
        p.press(r.x + 20.0, r.cy(), NONE, 1);
        assert_eq!(p.release(r.x + 20.0, r.cy()), Outcome::Submitted);
        // No entries and a message: the footer says no count (not "0 results").
        assert_eq!(p.footer_count(), "");
        // With entries, the message is a row above them and Enter still chooses.
        let mut p = plain(entries(2));
        p.set_message(Some("Searching".into()));
        assert_eq!(p.footer_count(), "2 items");
        assert_eq!(p.lines.len(), 3);
        assert_eq!(p.key(&KeyName::Enter, NONE, None), Outcome::Chosen(0));
        // The palette says "No matches" for an empty result on its own.
        let mut p = plain(entries(2));
        typed(&mut p, "qqq");
        assert_eq!(p.lines.len(), 1);
        assert_eq!(p.lines[0].line, Line::Message);
        assert_eq!(p.footer_count(), "0 results");
        p.set_message(None);
        assert_eq!(p.lines.len(), 1);
    }

    #[test]
    fn set_entries_keeps_the_selection_while_the_query_stands() {
        let mut p = palette(entries(6));
        p.key(&KeyName::Down, NONE, None);
        p.key(&KeyName::Down, NONE, None);
        assert_eq!(p.selected_id(), Some(2));
        p.set_entries(entries(6));
        assert_eq!(p.selected_id(), Some(2));
        // A typed change starts over, even for the same entries.
        typed(&mut p, "c");
        p.key(&KeyName::Down, NONE, None);
        typed(&mut p, "o");
        assert_ne!(p.selected_index(), None);
        assert_eq!(p.selected_index(), Some(0));
    }

    #[test]
    fn open_starts_over() {
        let mut p = palette(entries(40));
        p.wheel(300.0);
        typed(&mut p, "co");
        p.open(">");
        assert_eq!(p.query(), ">");
        assert_eq!(p.scroll(), 0.0);
        p.open("");
        assert_eq!(p.query(), "");
        assert_eq!(p.len(), 40);
    }

    /// Draws into a bitmap and checks nothing panics at odd sizes, 1.0 and 1.5 scale.
    #[test]
    fn draws_at_any_size() {
        let mut gfx = Gfx::new();
        for (w, h, scale) in [(900u32, 640u32, 1.0f32), (300, 200, 1.5), (60, 40, 1.0)] {
            let mut bmp = Bitmap::new(w, h);
            let mut p = Painter::new(&mut bmp, &mut gfx, scale);
            let win = Rect::new(0.0, 0.0, w as f32 / scale, h as f32 / scale);
            let mut pal = palette(entries(40));
            typed(&mut pal, "c");
            pal.draw(&mut p, &crate::theme::LIGHT, win, 40.0, true);
            pal.draw(&mut p, &crate::theme::DARK, win, 40.0, false);
        }
    }

    /// 5,000 entries and a typed query: the time per keystroke. Run in release for the number:
    /// `cargo test -p mdshared-ui --release palette_keystroke -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn palette_keystroke_time() {
        let words = [
            "Open", "File", "Save", "All", "Find", "Replace", "Toggle", "Word", "Wrap", "Line",
            "Numbers", "Fold", "Unfold", "Close", "Other", "Tabs", "Go", "To", "Next", "Previous",
        ];
        let items: Vec<Entry> = (0..5000u32)
            .map(|i| {
                let a = words[(i as usize * 7) % words.len()];
                let b = words[(i as usize * 13 + 3) % words.len()];
                let c = words[(i as usize * 31 + 5) % words.len()];
                Entry::new(i, format!("{a} {b} {c} {i}"))
            })
            .collect();
        let mut p = palette(items);
        let query = "opfi";
        let mut times = Vec::new();
        for ch in query.chars() {
            let s = ch.to_string();
            let t = std::time::Instant::now();
            p.key(&KeyName::Char(ch), NONE, Some(&s));
            times.push(t.elapsed());
        }
        println!("keystrokes {query:?}: {times:?}, {} results", p.len());
        // Backspace re-ranks from scratch.
        let t = std::time::Instant::now();
        p.key(&KeyName::Backspace, NONE, None);
        println!("backspace: {:?}, {} results", t.elapsed(), p.len());
        // A fresh first character is the worst case: everything matches a letter.
        p.set_query("");
        let t = std::time::Instant::now();
        p.key(&KeyName::Char('o'), NONE, Some("o"));
        println!("first char: {:?}, {} results", t.elapsed(), p.len());
        let t = std::time::Instant::now();
        p.set_query("");
        println!("clear: {:?}", t.elapsed());
    }
}
