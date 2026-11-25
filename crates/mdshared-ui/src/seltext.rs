//! Selectable text that cannot be edited: log output, a path, a message in a details pane. It
//! holds the text, a selection as byte offsets, and the pointer drag in progress; the caller
//! gives it the wrapped lines of the paragraph it draws, passes in the pointer and key events,
//! and acts on what they return. Nothing here reads the clipboard, a clock or a window.
//!
//! # The lines
//!
//! The text wraps as [`Painter::paragraph`] draws it, and only that layout knows where a byte
//! sits on screen. [`Painter::paragraph_lines`](crate::gfx::Painter::paragraph_lines), given the
//! face, size, width, text and colour `paragraph` is given, reports the lines of the very layout
//! `paragraph` draws, each with the x of every character boundary, so hit testing and the
//! selection highlight agree with the drawn glyphs. The caller asks for those lines and hands
//! them to [`SelectableText::press`] and [`SelectableText::drag`]. The model keeps the last lines
//! it was given for the keys (Left and Right step over clusters, Home and End find the wrapped
//! line, Up and Down move between lines), and the next press, drag or
//! [`SelectableText::set_lines`] replaces them. **After a resize or a font change the caller
//! must call `set_lines` with the new lines** before it passes keys again; until then the keys
//! use the old wrapping. [`draw`] lays the paragraph out itself on every call, so the drawing is
//! never stale.
//!
//! Lines that do not fit the text (a range past its end or off a char boundary, a stop count
//! that is not the range's char count plus one) are dropped where they come in, so lines of
//! another text are ignored rather than trusted.
//!
//! # Where the caret is
//!
//! The end of a wrapped line that breaks without a newline is the same byte offset as the start
//! of the next line. The model remembers which of the two the caret means: after a click past
//! the end of a line, End, or Up and Down landing at a line's end, the caret is at the end of
//! that line, so End again stays and Home goes to that line's start. Every other move puts it
//! at the start of the next line. [`SelectableText::caret_line`] tells a caller that draws a
//! caret which line it is on.
//!
//! # What selection means
//!
//! One click puts the caret at the nearest cluster edge and starts a drag; Shift keeps the
//! anchor so the press extends the selection. Two clicks select the word under the pointer,
//! using the word classes of [`crate::textfield`] (a run of letters, digits and `_`, a run of
//! spaces, or a run of other symbols) with a line break as a class of its own. Three clicks
//! select the line the click is on, the whole line between its line breaks rather than one
//! wrapped piece of it. Shift+click after two or three clicks extends from the far edge of the
//! word or line, so it stays selected. A line break is `\n`, `\r`, `\r\n`, U+2028 or U+2029,
//! the ones parley breaks a line at.
//!
//! Left and Right step one cluster, so the caret never stands inside "e" and its accent, a
//! joined emoji or a flag; with a selection and no Shift they drop it at its left or right end,
//! as Windows does. Home and End go to the ends of the wrapped line, before its line break, and
//! Ctrl+Home and Ctrl+End to the ends of the text. Up and Down keep the x they started from;
//! with Shift, Up on the first line goes to the start of the text and Down on the last line to
//! its end. Shift with any of them extends the selection.
//!
//! Deliberately not here: editing or typing, the caret and its blink (a read-only pane shows
//! none), the clipboard itself, scrolling a paragraph taller than its rectangle, word steps
//! (Ctrl+Left, Ctrl+Right), and drawing anything but the paragraph and its selection. The UI
//! Automation Text pattern is not done; a caller that needs it reports the text and the
//! selection from [`SelectableText::text`] and [`SelectableText::selected_text`] to its own
//! accessibility tree.
//!
//! Deliberately not tested: right-to-left paragraphs. The stops follow parley's runs and carry
//! its direction, but no test exercises an RTL paragraph.

use crate::gfx::{Face, Line, Painter, Rect};
use crate::keys::{KeyName, Mods};
use crate::menu::Item;
use crate::textfield::{Class, class};
use crate::theme::Palette;
use std::ops::Range;

/// What a key or a context menu asks the caller to do. The clipboard is the caller's: Copy names
/// the text and the caller puts it there, and Select all selects everything in the model.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// Put this text on the clipboard.
    Copy(String),
    /// Select all of the text.
    SelectAll,
}

/// A paragraph that cannot be edited but can be selected.
///
/// The selection is two byte offsets on character boundaries, an anchor and a caret; the text
/// between the smaller and the larger is selected. A click moves both unless Shift is held, in
/// which case the anchor stays and the caret moves: the selection runs from where the drag
/// started to where it is now.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SelectableText {
    text: String,
    /// Byte offset of the moving end of the selection, on a char boundary.
    caret: usize,
    /// Byte offset of the fixed end, on a char boundary; equal to `caret` when nothing is
    /// selected.
    anchor: usize,
    /// The caret is at the end of the wrapped line that ends at `caret`, not at the start of the
    /// next one. Only set where a line breaks without a line break char.
    upstream: bool,
    /// The word or line two or three clicks selected, while that selection stands, so Shift+click
    /// extends from its far edge.
    unit: Option<Range<usize>>,
    /// The last wrapped lines given that fit the text, for the keys.
    lines: Vec<Line>,
    /// The x logical Up and Down keep while they move, in paragraph px.
    goal_x: Option<f32>,
    /// A press started a drag that has not been released.
    dragging: bool,
}

impl SelectableText {
    /// An empty paragraph; [`set_text`](Self::set_text) gives it words.
    pub fn new() -> SelectableText {
        SelectableText::default()
    }

    /// The text as it is, newlines included.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Replaces the text and clears the selection, with the caret at the start. The lines of the
    /// old text are dropped; give it the new ones with the next press, drag or
    /// [`set_lines`](Self::set_lines).
    pub fn set_text(&mut self, text: &str) {
        self.text = text.to_string();
        self.caret = 0;
        self.anchor = 0;
        self.upstream = false;
        self.unit = None;
        self.lines.clear();
        self.goal_x = None;
        self.dragging = false;
    }

    /// Gives the model the lines of the paragraph as it is drawn now, from the top of the
    /// paragraph: [`Painter::paragraph_lines`](crate::gfx::Painter::paragraph_lines) for the same
    /// text, face, size, width and colour.
    ///
    /// The keys need them: Left and Right step over clusters by them, Home and End find the
    /// wrapped line, and Up and Down move between lines. [`press`](Self::press) and
    /// [`drag`](Self::drag) call this with the lines they are given, but nothing else refreshes
    /// them, so the caller must call it after a resize or a font change. A line that does not
    /// fit the text is dropped: its range is past the end or off a char boundary, or its `stops`
    /// or `edges` do not have the range's char count plus one entries.
    pub fn set_lines(&mut self, lines: &[Line]) {
        self.lines = lines
            .iter()
            .filter(|l| fits(l, &self.text))
            .cloned()
            .collect();
    }

    /// The caret and the anchor, byte offsets, for knowing when a redraw is due: the same pair
    /// [`crate::textfield::TextField::caret`] gives.
    pub fn caret(&self) -> (usize, usize) {
        (self.caret, self.anchor)
    }

    /// The index, among the lines last given, of the wrapped line the caret is on, for a caller
    /// that draws a caret of its own. Where a line wraps without a line break the caret's offset
    /// is both the end of one line and the start of the next, and this says which the caret
    /// means. `None` when the model has no lines.
    pub fn caret_line(&self) -> Option<usize> {
        (!self.lines.is_empty()).then(|| self.line_of(self.caret, self.upstream))
    }

    /// Whether anything is selected.
    pub fn has_selection(&self) -> bool {
        self.caret != self.anchor
    }

    /// The selected byte range, from the smaller end to the larger; empty when nothing is
    /// selected. Both ends are character boundaries.
    pub fn selection(&self) -> Range<usize> {
        self.caret.min(self.anchor)..self.caret.max(self.anchor)
    }

    /// The selected text, or the empty string when nothing is selected.
    pub fn selected_text(&self) -> &str {
        let r = self.selection();
        &self.text[r]
    }

    /// Selects every byte of the text, with the caret at its end.
    pub fn select_all(&mut self) {
        self.anchor = 0;
        self.caret = self.text.len();
        self.upstream = false;
        self.unit = None;
        self.goal_x = None;
    }

    /// Drops the selection and leaves the caret where it is.
    pub fn clear_selection(&mut self) {
        self.anchor = self.caret;
        self.unit = None;
        self.goal_x = None;
    }

    /// A pointer press at `(x, y)`, in logical px from the top left of the paragraph: the
    /// `lines` must be [`Painter::paragraph_lines`](crate::gfx::Painter::paragraph_lines) for the
    /// same text, face, size, width and colour, and replace the ones the model has (see
    /// [`set_lines`](Self::set_lines); lines that do not fit the text are ignored). `clicks`
    /// counts the presses of a multiple click: one puts the caret at the nearest cluster edge
    /// and starts a drag, two select the word under the pointer, and three or more select its
    /// line. With `mods.shift` a single click moves only the caret, extending the selection from
    /// its anchor, or from the far edge of the word or line two or three clicks selected.
    ///
    /// A point above or below the paragraph lands on the nearest line, and one left or right of
    /// a line lands on its nearest edge, so a click in the margin is not lost. Past the end of a
    /// line the caret goes before its line break, and at the end of a line that wraps it stays
    /// on that line. For two and three clicks the char under the pointer counts, the one whose
    /// cluster spans its x; past the end of a line it is the line's last char before its break.
    pub fn press(&mut self, lines: &[Line], x: f32, y: f32, mods: Mods, clicks: u8) {
        self.set_lines(lines);
        self.goal_x = None;
        self.dragging = false;
        if clicks >= 2 {
            let at = self.char_at(x, y);
            let unit = if clicks >= 3 {
                logical_line(&self.text, at)
            } else {
                word_at(&self.text, at, &edge_table(&self.lines, &self.text))
            };
            self.anchor = unit.start;
            self.caret = unit.end;
            self.upstream = false;
            self.unit = Some(unit);
            return;
        }
        let (at, upstream) = self.hit(x, y);
        if mods.shift {
            self.extend_to(at, upstream);
        } else {
            self.anchor = at;
            self.caret = at;
            self.upstream = upstream;
            self.unit = None;
        }
        self.goal_x = x.is_finite().then_some(x);
        self.dragging = true;
    }

    /// A pointer drag to `(x, y)`, in the same coordinates and with the same `lines` as
    /// [`press`](Self::press): the selection runs from the press's anchor to the caret here.
    /// A drag without a press in progress does nothing.
    pub fn drag(&mut self, lines: &[Line], x: f32, y: f32) {
        if !self.dragging {
            return;
        }
        self.set_lines(lines);
        let (at, upstream) = self.hit(x, y);
        self.extend_to(at, upstream);
        self.goal_x = x.is_finite().then_some(x);
    }

    /// Ends the drag a press started; a later [`drag`](Self::drag) does nothing until the next
    /// press.
    pub fn release(&mut self) {
        self.dragging = false;
    }

    /// A key, for a pane that has the keyboard. Ctrl+A selects everything and Ctrl+C gives
    /// [`Action::Copy`] of the selected text (or `None` when nothing is selected). Left and Right
    /// step one cluster, or without Shift drop a selection at its left or right end; Home and End
    /// go to the ends of the wrapped line, Ctrl+Home and Ctrl+End to the ends of the text; Up and
    /// Down move by the wrapped lines and keep the x they started at, and with Shift go to the
    /// start or the end of the text from the first or the last line. Shift with any of them
    /// extends the selection. The keys use the lines last given (see
    /// [`set_lines`](Self::set_lines)); with none, Left and Right step one char, Home and End go
    /// to the ends of the line between line breaks, and Up and Down do nothing. Alt is refused
    /// whole, so Alt+C is not a copy and changes nothing. Every other key is `None` and changes
    /// nothing.
    pub fn key(&mut self, key: &KeyName, mods: Mods) -> Option<Action> {
        if mods.alt {
            return None;
        }
        let shift = mods.shift;
        match key {
            KeyName::Char('a') if mods.ctrl => {
                self.select_all();
                None
            }
            KeyName::Char('c') if mods.ctrl => self
                .has_selection()
                .then(|| Action::Copy(self.selected_text().to_string())),
            KeyName::Left if !mods.ctrl => {
                let to = if self.has_selection() && !shift {
                    self.selection().start
                } else {
                    self.prev_edge(self.caret)
                };
                self.move_to(to, false, shift);
                None
            }
            KeyName::Right if !mods.ctrl => {
                let to = if self.has_selection() && !shift {
                    self.selection().end
                } else {
                    self.next_edge(self.caret)
                };
                self.move_to(to, false, shift);
                None
            }
            KeyName::Home if mods.ctrl => {
                self.move_to(0, false, shift);
                None
            }
            KeyName::End if mods.ctrl => {
                self.move_to(self.text.len(), false, shift);
                None
            }
            KeyName::Home => {
                let (start, _, _) = self.line_ends();
                self.move_to(start, false, shift);
                None
            }
            KeyName::End => {
                let (_, end, upstream) = self.line_ends();
                self.move_to(end, upstream, shift);
                None
            }
            KeyName::Up if !mods.ctrl => {
                self.move_vertical(false, shift);
                None
            }
            KeyName::Down if !mods.ctrl => {
                self.move_vertical(true, shift);
                None
            }
            _ => None,
        }
    }

    /// The items of a context menu for the paragraph: Copy, disabled while nothing is selected,
    /// and Select all, disabled while there is no text. Their ids are the [`Action`]s to carry
    /// out, so a caller handles a chosen item exactly as it handles [`key`](Self::key)'s answer.
    pub fn context_menu_items(&self) -> Vec<Item<Action>> {
        vec![
            Item::new(Action::Copy(self.selected_text().to_string()), "Copy")
                .hint("Ctrl+C")
                .enabled(self.has_selection()),
            Item::new(Action::SelectAll, "Select all")
                .hint("Ctrl+A")
                .enabled(!self.text.is_empty()),
        ]
    }

    /// A key's caret move: to `at`, at the end of its line when `upstream`. With `extend` the
    /// anchor stays, without it the selection collapses to the caret. A key move forgets the x
    /// Up and Down would keep and the word two clicks selected.
    fn move_to(&mut self, at: usize, upstream: bool, extend: bool) {
        self.caret = at;
        self.upstream = upstream;
        if !extend {
            self.anchor = at;
        }
        self.goal_x = None;
        self.unit = None;
    }

    /// A Shift+click or a drag to `at`: the anchor stays, or with a word or line from two or
    /// three clicks standing, the anchor goes to its far edge so it stays selected.
    fn extend_to(&mut self, at: usize, upstream: bool) {
        self.caret = at;
        self.upstream = upstream;
        if let Some(unit) = &self.unit {
            if at < unit.start {
                self.anchor = unit.end;
            } else if at > unit.end {
                self.anchor = unit.start;
            } else {
                self.anchor = unit.start;
                self.caret = unit.end;
                self.upstream = false;
            }
        }
    }

    /// The edge nearest to `(x, y)` and whether it is the end of a wrapped line rather than the
    /// start of the next. A point off the text lands on the nearest line and edge.
    fn hit(&self, x: f32, y: f32) -> (usize, bool) {
        let Some(i) = self.line_at_y(y) else {
            return (0, false);
        };
        let at = offset_in_line(&self.lines[i], &self.text, x);
        (at, self.soft_end(i, at))
    }

    /// The start of the cluster under `(x, y)`: the one whose stops span `x` on the line at `y`,
    /// or the nearest one when none does, never a line break. On a line that is only a line
    /// break, its start.
    fn char_at(&self, x: f32, y: f32) -> usize {
        let Some(i) = self.line_at_y(y) else {
            return 0;
        };
        let line = &self.lines[i];
        let limit = content_end(&self.text, line.range.clone());
        let edges: Vec<(usize, f32)> = bounds(line, &self.text)
            .filter(|&(i, b)| line.edges[i] && b <= limit)
            .map(|(i, b)| (b, line.stops[i]))
            .collect();
        if edges.len() < 2 {
            return line.range.start;
        }
        let x = if x.is_nan() { 0.0 } else { x };
        let mut best = (edges[0].0, f32::INFINITY);
        for w in edges.windows(2) {
            let ((a, xa), (_, xb)) = (w[0], w[1]);
            let (lo, hi) = (xa.min(xb), xa.max(xb));
            if lo <= x && x < hi {
                return a;
            }
            let d = (x - (lo + hi) / 2.0).abs();
            if d < best.1 {
                best = (a, d);
            }
        }
        best.0
    }

    /// The index of the line whose box holds `y`: the first line whose box ends below it, or the
    /// last line. `None` with no lines.
    fn line_at_y(&self, y: f32) -> Option<usize> {
        if self.lines.is_empty() {
            return None;
        }
        Some(
            self.lines
                .iter()
                .position(|l| y < l.y + l.height)
                .unwrap_or(self.lines.len() - 1),
        )
    }

    /// Whether `at` on line `i` is the end of a line that wraps without a line break, where the
    /// same offset is also the start of the next line.
    fn soft_end(&self, i: usize, at: usize) -> bool {
        let line = &self.lines[i];
        i + 1 < self.lines.len() && at == line.range.end && at > line.range.start
    }

    /// The start and end of the wrapped line the caret is on, for Home and End, and whether the
    /// end is a wrapped line's end that the caret should keep to. The end is before the line's
    /// line break, so End does not land on the next line. With no lines, the line between line
    /// breaks around the caret.
    fn line_ends(&self) -> (usize, usize, bool) {
        let logical = logical_line(&self.text, self.caret);
        if self.lines.is_empty() {
            return (logical.start, logical.end, false);
        }
        let i = self.line_of(self.caret, self.upstream);
        let line = &self.lines[i];
        if self.caret < line.range.start || self.caret > line.range.end {
            return (logical.start, logical.end, false);
        }
        let start = line.range.start.max(logical.start);
        let end = content_end(&self.text, line.range.clone()).min(logical.end);
        (start, end, self.soft_end(i, end))
    }

    /// The index of the line that holds byte `at`: the line that ends there when `upstream` is
    /// set and one does, else the first line that ends after it, or the last line when `at` is
    /// at the very end.
    fn line_of(&self, at: usize, upstream: bool) -> usize {
        if upstream
            && let Some(i) = self
                .lines
                .iter()
                .position(|l| l.range.start < at && l.range.end == at)
        {
            return i;
        }
        self.lines
            .iter()
            .position(|l| at < l.range.end)
            .unwrap_or(self.lines.len().saturating_sub(1))
    }

    /// The next cluster edge after `at`, by the lines; one char on with no line holding `at`.
    fn next_edge(&self, at: usize) -> usize {
        let Some(line) = self
            .lines
            .iter()
            .find(|l| l.range.start <= at && at < l.range.end)
        else {
            return next_char(&self.text, at);
        };
        bounds(line, &self.text)
            .find(|&(i, b)| b > at && line.edges[i])
            .map_or(line.range.end, |(_, b)| b)
    }

    /// The cluster edge before `at`, by the lines; one char back with no line holding it.
    fn prev_edge(&self, at: usize) -> usize {
        let Some(line) = self
            .lines
            .iter()
            .find(|l| l.range.start < at && at <= l.range.end)
        else {
            return prev_char(&self.text, at);
        };
        bounds(line, &self.text)
            .filter(|&(i, b)| b < at && line.edges[i])
            .last()
            .map_or(line.range.start, |(_, b)| b)
    }

    /// Up or Down: to the nearest edge of the line above or below at the x this move keeps,
    /// extending the selection when `extend` is set. At the first or the last line it stays
    /// where it is, or with `extend` goes to the start or the end of the text.
    fn move_vertical(&mut self, down: bool, extend: bool) {
        if self.lines.is_empty() {
            return;
        }
        let here = self.line_of(self.caret, self.upstream);
        let at_edge = if down {
            here + 1 >= self.lines.len()
        } else {
            here == 0
        };
        if at_edge {
            if extend {
                let to = if down { self.text.len() } else { 0 };
                self.move_to(to, false, true);
            }
            return;
        }
        let there = if down { here + 1 } else { here - 1 };
        let x = self
            .goal_x
            .unwrap_or_else(|| x_of(&self.lines[here], &self.text, self.caret));
        let at = offset_in_line(&self.lines[there], &self.text, x);
        self.caret = at;
        self.upstream = self.soft_end(there, at);
        if !extend {
            self.anchor = at;
        }
        self.unit = None;
        self.goal_x = Some(x);
    }
}

/// Draws the paragraph in `area` and, when `model` has a selection and `focused` is set, the
/// selection behind the text: `pal.selection` at the same 0.35 alpha the text field uses, over
/// each wrapped line the selection touches. A selected empty line gets a highlight as wide as a
/// space, so it shows. The text is drawn by
/// [`Painter::paragraph`](crate::gfx::Painter::paragraph) with `pal.text`, in `face` and `size`,
/// clipped to `area`.
///
/// The lines are laid out for `area.w` on every call, with `pal.text`, so they come from the
/// same cached layout `paragraph` draws and the highlight follows a resize without the model
/// being told. The highlight is placed in device px as the glyphs are, from the rounded corner
/// of `area`, so the two agree at a fractional `area.x`. The model's own lines are not touched;
/// the caller gives them to it with [`SelectableText::set_lines`].
pub fn draw(
    p: &mut Painter,
    pal: &Palette,
    area: Rect,
    face: Face,
    size: f32,
    model: &SelectableText,
    focused: bool,
) {
    let old = p.clip(area);
    if focused && model.has_selection() {
        let lines = p.paragraph_lines(face, size, area.w, &model.text, pal.text);
        let selection = model.selection();
        let s = p.scale;
        // The device px corner the paragraph's glyphs are placed from.
        let (ox, oy) = ((area.x * s).round(), (area.y * s).round());
        let mut space = None;
        for line in &lines {
            let from = selection.start.max(line.range.start);
            let to = selection.end.min(line.range.end);
            if from >= to {
                continue;
            }
            let (mut x0, mut x1) = (x_of(line, &model.text, from), x_of(line, &model.text, to));
            if x1 < x0 {
                std::mem::swap(&mut x0, &mut x1);
            }
            if content_end(&model.text, line.range.clone()) == line.range.start {
                let w = *space.get_or_insert_with(|| {
                    p.caret_stops(face, size, " ")
                        .last()
                        .copied()
                        .unwrap_or(0.0)
                });
                x1 = x1.max(x0 + w);
            }
            p.fill_a(
                Rect::new(
                    (ox + x0 * s) / s,
                    (oy + line.y * s) / s,
                    x1 - x0,
                    line.height,
                ),
                pal.selection,
                0.35,
            );
        }
    }
    p.paragraph(face, size, area.x, area.y, area.w, &model.text, pal.text);
    p.restore(old);
}

/// Whether `line` can be read against `text`: its range lies in the text on char boundaries,
/// and it has one stop and one edge flag per char boundary of the range.
fn fits(line: &Line, text: &str) -> bool {
    let Some(piece) = text.get(line.range.clone()) else {
        return false;
    };
    let count = piece.chars().count() + 1;
    line.stops.len() == count && line.edges.len() == count
}

/// Each stop of `line` with the byte offset in `text` it stands for, the line's start first and
/// its end last. `line` must fit `text`.
fn bounds<'a>(line: &'a Line, text: &'a str) -> impl Iterator<Item = (usize, usize)> + 'a {
    let start = line.range.start;
    let piece = &text[line.range.clone()];
    piece
        .char_indices()
        .map(move |(b, _)| start + b)
        .chain(std::iter::once(line.range.end))
        .enumerate()
}

/// The edge of `line` nearest to `x`, in the line's own x coordinates, never after its line
/// break. Of two edges at the same distance the earlier wins, so a cluster's inner chars, which
/// share its start's x, are never chosen.
fn offset_in_line(line: &Line, text: &str, x: f32) -> usize {
    let limit = content_end(text, line.range.clone());
    let lo = line.stops.iter().copied().fold(f32::INFINITY, f32::min);
    let hi = line.stops.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let x = if x.is_nan() { 0.0 } else { x };
    let x = if lo <= hi { x.clamp(lo, hi) } else { x };
    let mut best: Option<(usize, f32)> = None;
    for (i, b) in bounds(line, text) {
        if !line.edges[i] || b > limit {
            continue;
        }
        let d = (line.stops[i] - x).abs();
        if best.is_none_or(|(_, closest)| d < closest) {
            best = Some((b, d));
        }
    }
    best.map_or(line.range.start, |(b, _)| b)
}

/// The x of byte `at`, which must lie in `line`'s range, in the line's own coordinates. `line`
/// must fit `text`.
fn x_of(line: &Line, text: &str, at: usize) -> f32 {
    let mut at = at.min(line.range.end).max(line.range.start);
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    let index = text[line.range.start..at].chars().count();
    line.stops
        .get(index)
        .or_else(|| line.stops.last())
        .copied()
        .unwrap_or(0.0)
}

/// Whether `c` ends a line: the chars parley breaks a line at.
fn is_break(c: char) -> bool {
    matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}')
}

/// The end of `range` in `text` before its line break, if it ends with one (`\r\n` counts as
/// one).
fn content_end(text: &str, range: Range<usize>) -> usize {
    let piece = &text[range.clone()];
    let kept = match piece.strip_suffix("\r\n") {
        Some(kept) => kept,
        None => match piece.chars().next_back() {
            Some(c) if is_break(c) => &piece[..piece.len() - c.len_utf8()],
            _ => piece,
        },
    };
    range.start + kept.len()
}

/// The word class of `c` for two clicks, `None` for a line break, a class of its own.
fn kind(c: char) -> Option<Class> {
    (!is_break(c)).then(|| class(c))
}

/// For every byte offset of `text` and its end, whether a caret may stand there by `lines`:
/// `false` for the chars inside a cluster, `true` everywhere else.
fn edge_table(lines: &[Line], text: &str) -> Vec<bool> {
    let mut table = vec![true; text.len() + 1];
    for line in lines {
        for (i, b) in bounds(line, text) {
            if !line.edges[i] {
                table[b] = false;
            }
        }
    }
    table
}

/// The word that starts with the cluster at `at`: the run of clusters of the same class as its
/// first char, a cluster taking the class of its first char. A line break is a class of its own
/// and selects nothing, and so does the end of the text.
fn word_at(text: &str, at: usize, edge: &[bool]) -> Range<usize> {
    let at = at.min(text.len());
    let is_edge = |b: usize| edge.get(b).copied().unwrap_or(true);
    let Some(c) = text[at..].chars().next() else {
        return at..at;
    };
    let Some(k) = kind(c) else {
        return at..at;
    };
    let mut from = at;
    for (p, ch) in text[..at].char_indices().rev() {
        if !is_edge(p) {
            continue;
        }
        if kind(ch) != Some(k) {
            break;
        }
        from = p;
    }
    let to = text[at..]
        .char_indices()
        .skip(1)
        .map(|(p, ch)| (at + p, ch))
        .find(|&(p, ch)| is_edge(p) && kind(ch) != Some(k))
        .map_or(text.len(), |(p, _)| p);
    from..to
}

/// The line that holds `at`, between the line breaks around it: the whole logical line, not one
/// wrapped piece. The line breaks are left out.
fn logical_line(text: &str, at: usize) -> Range<usize> {
    let mut at = at.min(text.len());
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    let start = text[..at]
        .char_indices()
        .rev()
        .find(|&(_, c)| is_break(c))
        .map_or(0, |(i, c)| i + c.len_utf8());
    let end = text[at..]
        .char_indices()
        .find(|&(_, c)| is_break(c))
        .map_or(text.len(), |(i, _)| at + i);
    start..end
}

fn prev_char(text: &str, i: usize) -> usize {
    let mut i = i.min(text.len());
    while !text.is_char_boundary(i) {
        i -= 1;
    }
    text[..i].char_indices().next_back().map_or(0, |(p, _)| p)
}

fn next_char(text: &str, i: usize) -> usize {
    let mut i = i.min(text.len());
    while !text.is_char_boundary(i) {
        i -= 1;
    }
    text[i..].chars().next().map_or(i, |c| i + c.len_utf8())
}

#[cfg(test)]
#[path = "seltext_tests.rs"]
mod tests;
