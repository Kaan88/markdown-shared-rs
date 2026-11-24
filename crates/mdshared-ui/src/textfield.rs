//! A single-line text field for the hand-drawn UI. It owns the text, the caret, the selection and
//! the horizontal scroll; the app feeds it keys, pasted text and mouse positions, and draws it with
//! a [`Painter`]. No clipboard or windowing code lives here.
//!
//! # The caret's blink
//!
//! [`TextField::draw`] is told whether the caret is on (`caret_on`) and keeps no time. For a
//! caret that blinks at the system's rate, [`TextField::blink_caret`] gives that `caret_on`, and
//! [`TextField::draw_blinking`] is `draw` with it. Both read the blink from an
//! [`Animator`] the caller owns, through [`Animator::blink`], and are given the half-period the
//! caller read from [`platform::caret_blink_time`](crate::platform::caret_blink_time).
//!
//! Why the animator, and not an `Instant` passed in: the animator already has a blink that asks
//! for a frame only at the moments it changes, stands still with reduced motion, does not jump
//! after a stall, and folds its deadline into the one deadline of the window
//! ([`Animator::end_frame`]) with everything else in flight. A clock in the field would be a
//! second copy of all that, and the loop would have two deadlines to merge. So the field keeps
//! no time at all; it keeps one private flag, set by every call that types, deletes, moves the
//! caret or changes the selection, and `blink_caret` turns that flag into
//! [`Animator::forget`], so the next asking starts the blink again, on.
//!
//! The rules, as Windows has them:
//!
//! * Typing, moving the caret and focusing start the blink again with the caret shown.
//! * While the field is not focused there is no caret, nothing is asked of the animator, and so
//!   no deadline: the window sleeps.
//! * While a selection is shown the caret is hidden, steadily, and there is no deadline either.
//! * When the system's blink time is `None` (blinking switched off, `GetCaretBlinkTime` giving
//!   `INFINITE`) the caret is steadily shown and there is no deadline. With reduced motion the
//!   animator gives the same.
//!
//! The deadline is the animator's: the loop that called [`Animator::begin_frame`] takes
//! [`Animator::end_frame`] and waits until it (`ControlFlow::WaitUntil`), and it is the moment
//! the caret next turns on or off.
//!
//! Deliberately not here: reading the system's blink time (the caller passes it in, so a test
//! can), a blink that stops after a while with no typing (the caller can stop asking), a caret
//! wider than one pixel, and a fade between on and off.

use crate::anim::{Animator, AsId};
use crate::gfx::{Face, Painter, Rect, Rgb};
use crate::keys::{KeyName, Mods};
use std::time::Duration;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EditKey {
    Left,
    Right,
    Home,
    End,
    Backspace,
    Delete,
}

/// Colours the field is drawn with.
pub struct FieldColors {
    pub text: Rgb,
    pub placeholder: Rgb,
    /// Drawn at about 0.35 alpha behind the text.
    pub selection: Rgb,
    pub caret: Rgb,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Class {
    Space,
    Word,
    Other,
}

fn class(c: char) -> Class {
    if c.is_whitespace() {
        Class::Space
    } else if c.is_alphanumeric() || c == '_' {
        Class::Word
    } else {
        Class::Other
    }
}

#[derive(Default, Clone)]
pub struct TextField {
    text: String,
    /// Byte offset of the caret (always on a char boundary).
    caret: usize,
    /// Byte offset of the other end of the selection; equal to `caret` when nothing is selected.
    anchor: usize,
    /// Horizontal scroll in logical px.
    scroll: f32,
    /// What an input method is composing, drawn at the caret until it is committed or dropped.
    preedit: Option<Preedit>,
    /// A secret (an API key): drawn as dots, never copied or cut.
    secret: bool,
    /// Set by every call that edits, moves the caret or changes the selection; taken by
    /// [`TextField::blink_caret`], which starts the blink again. Nothing draws by it.
    restart: bool,
}

/// What a secret field draws for each char.
const MASK: char = '•';

/// The text an input method is composing (winit's `Ime::Preedit`): drawn underlined where it will
/// go, not yet part of the text. `cursor` is the input method's caret or selected clause in it,
/// byte offsets.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Preedit {
    pub text: String,
    pub cursor: Option<(usize, usize)>,
}

impl Preedit {
    /// The composition winit reports, or `None` for the empty one that ends it.
    pub fn new(text: String, cursor: Option<(usize, usize)>) -> Option<Preedit> {
        let ok = |i: usize| i <= text.len() && text.is_char_boundary(i);
        let cursor = cursor.filter(|&(a, b)| a <= b && ok(a) && ok(b));
        (!text.is_empty()).then_some(Preedit { text, cursor })
    }

    /// Where the input method's caret is in the text, a byte offset (its end when it has none).
    pub fn caret(&self) -> usize {
        self.cursor.map_or(self.text.len(), |c| c.1)
    }
}

impl TextField {
    pub fn new() -> TextField {
        TextField::default()
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    /// Caret and selection anchor, byte offsets, for knowing when a redraw is due.
    pub fn caret(&self) -> (usize, usize) {
        (self.caret, self.anchor)
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// Makes the field a secret one: its text is drawn as dots and cannot be copied or cut.
    pub fn set_secret(&mut self, on: bool) {
        self.secret = on;
    }

    pub fn is_secret(&self) -> bool {
        self.secret
    }

    /// The text as it is drawn: the text itself, or one dot per char for a secret. Caret stops
    /// are measured on this.
    pub fn display(&self) -> std::borrow::Cow<'_, str> {
        if self.secret {
            std::iter::repeat_n(MASK, self.text.chars().count())
                .collect::<String>()
                .into()
        } else {
            self.text.as_str().into()
        }
    }

    /// Sets or clears what an input method is composing at the caret. Returns true if it changed.
    pub fn set_preedit(&mut self, preedit: Option<Preedit>) -> bool {
        let changed = self.preedit != preedit;
        self.restart |= changed;
        self.preedit = preedit;
        changed
    }

    pub fn preedit(&self) -> Option<&Preedit> {
        self.preedit.as_ref()
    }

    /// Replaces the text, caret at the end, no selection.
    pub fn set_text(&mut self, text: &str) {
        self.restart = true;
        self.text = clean(text);
        self.caret = self.text.len();
        self.anchor = self.caret;
        self.scroll = 0.0;
        self.preedit = None;
    }

    pub fn clear(&mut self) {
        self.restart = true;
        self.preedit = None;
        self.text.clear();
        self.caret = 0;
        self.anchor = 0;
        self.scroll = 0.0;
    }

    pub fn select_all(&mut self) {
        self.restart = true;
        self.anchor = 0;
        self.caret = self.text.len();
    }

    pub fn has_selection(&self) -> bool {
        self.caret != self.anchor
    }

    fn sel_range(&self) -> (usize, usize) {
        (self.caret.min(self.anchor), self.caret.max(self.anchor))
    }

    pub fn selected_text(&self) -> &str {
        let (a, b) = self.sel_range();
        &self.text[a..b]
    }

    /// Inserts typed or pasted text at the caret, replacing the selection. Control characters are
    /// dropped; newlines and tabs become spaces. Returns true if the text changed.
    pub fn insert(&mut self, text: &str) -> bool {
        let clean = clean(text);
        self.restart = true;
        self.preedit = None;
        if clean.is_empty() && !self.has_selection() {
            return false;
        }
        let (a, b) = self.sel_range();
        self.text.replace_range(a..b, &clean);
        self.caret = a + clean.len();
        self.anchor = self.caret;
        true
    }

    /// Deletes the selection (for Cut). Returns true if the text changed.
    pub fn delete_selection(&mut self) -> bool {
        if !self.has_selection() {
            return false;
        }
        self.restart = true;
        let (a, b) = self.sel_range();
        self.text.replace_range(a..b, "");
        self.caret = a;
        self.anchor = a;
        true
    }

    fn prev_char(&self, i: usize) -> usize {
        self.text[..i]
            .char_indices()
            .next_back()
            .map_or(0, |(p, _)| p)
    }

    fn next_char(&self, i: usize) -> usize {
        self.text[i..]
            .chars()
            .next()
            .map_or(i, |c| i + c.len_utf8())
    }

    /// The start of the word before `i`: skip whitespace, then one run of the same class.
    fn prev_word(&self, i: usize) -> usize {
        let mut pos = i;
        let mut chars = self.text[..i].char_indices().rev().peekable();
        while let Some(&(p, c)) = chars.peek() {
            if class(c) != Class::Space {
                break;
            }
            pos = p;
            chars.next();
        }
        let Some(&(_, first)) = chars.peek() else {
            return pos;
        };
        let k = class(first);
        for (p, c) in chars {
            if class(c) != k {
                break;
            }
            pos = p;
        }
        pos
    }

    /// The end of the word after `i`: skip whitespace, then one run of the same class.
    fn next_word(&self, i: usize) -> usize {
        let mut pos = i;
        let mut chars = self.text[i..].chars().peekable();
        while let Some(&c) = chars.peek() {
            if class(c) != Class::Space {
                break;
            }
            pos += c.len_utf8();
            chars.next();
        }
        let Some(&first) = chars.peek() else {
            return pos;
        };
        let k = class(first);
        for c in chars {
            if class(c) != k {
                break;
            }
            pos += c.len_utf8();
        }
        pos
    }

    /// Caret movement and deletion. `word` is Ctrl held (move/delete by word), `extend` is Shift
    /// held (extend the selection; ignored for Backspace/Delete). Left/Right without Shift collapse
    /// a selection to its start/end. Returns true if the TEXT changed.
    pub fn key(&mut self, key: EditKey, word: bool, extend: bool) -> bool {
        self.restart = true;
        match key {
            EditKey::Left | EditKey::Right | EditKey::Home | EditKey::End => {
                let (a, b) = self.sel_range();
                let to = match key {
                    EditKey::Left if !extend && a != b => a,
                    EditKey::Right if !extend && a != b => b,
                    EditKey::Left if word => self.prev_word(self.caret),
                    EditKey::Left => self.prev_char(self.caret),
                    EditKey::Right if word => self.next_word(self.caret),
                    EditKey::Right => self.next_char(self.caret),
                    EditKey::Home => 0,
                    _ => self.text.len(),
                };
                self.caret = to;
                if !extend {
                    self.anchor = to;
                }
                false
            }
            EditKey::Backspace | EditKey::Delete => {
                if self.has_selection() {
                    return self.delete_selection();
                }
                let to = match (key, word) {
                    (EditKey::Backspace, true) => self.prev_word(self.caret),
                    (EditKey::Backspace, false) => self.prev_char(self.caret),
                    (_, true) => self.next_word(self.caret),
                    _ => self.next_char(self.caret),
                };
                if to == self.caret {
                    return false;
                }
                let (a, b) = (to.min(self.caret), to.max(self.caret));
                self.text.replace_range(a..b, "");
                self.caret = a;
                self.anchor = a;
                true
            }
        }
    }

    /// The editing keys every single-line box shares: caret movement, word movement (Ctrl),
    /// selection (Shift), deletion, Ctrl+A/C/X/V when `clipboard` is on, and typed text. `typed`
    /// is the text the key event produced. `None` when the key is not an editing key (the caller
    /// treats it as its own: Enter, Escape, Up, Down, Tab, a shortcut); otherwise whether the
    /// TEXT changed. Paste takes the clipboard's first line.
    pub fn handle_key(
        &mut self,
        key: &KeyName,
        mods: Mods,
        typed: Option<&str>,
        clipboard: bool,
    ) -> Option<bool> {
        let (key, mods) = crate::keys::text_key(key.clone(), mods);
        let key = &key;
        let (word, extend) = (mods.ctrl, mods.shift);
        Some(match key {
            KeyName::Left => self.key(EditKey::Left, word, extend),
            KeyName::Right => self.key(EditKey::Right, word, extend),
            KeyName::Home => self.key(EditKey::Home, false, extend),
            KeyName::End => self.key(EditKey::End, false, extend),
            KeyName::Backspace => self.key(EditKey::Backspace, word, false),
            KeyName::Delete => self.key(EditKey::Delete, word, false),
            KeyName::Char('a') if mods.ctrl && !mods.alt => {
                self.select_all();
                false
            }
            KeyName::Char('c') if mods.ctrl && !mods.alt => {
                if clipboard && self.has_selection() && !self.secret {
                    crate::platform::set_clipboard_text(self.selected_text());
                }
                false
            }
            KeyName::Char('x') if mods.ctrl && !mods.alt => {
                if clipboard && self.has_selection() && !self.secret {
                    crate::platform::set_clipboard_text(self.selected_text());
                    self.delete_selection()
                } else {
                    false
                }
            }
            KeyName::Char('v') if mods.ctrl && !mods.alt => {
                match crate::platform::clipboard_text().filter(|_| clipboard) {
                    Some(t) => self.insert(t.lines().next().unwrap_or("")),
                    None => false,
                }
            }
            KeyName::Enter
            | KeyName::Escape
            | KeyName::Tab
            | KeyName::Up
            | KeyName::Down
            | KeyName::PageUp
            | KeyName::PageDown
            | KeyName::F(_) => return None,
            _ => {
                // A shortcut, or text (see `keys::types_text`: AltGr on Windows, Option on a Mac).
                let t = typed.filter(|t| !t.chars().any(char::is_control))?;
                if !crate::keys::types_text(mods) {
                    return None;
                }
                self.insert(t)
            }
        })
    }

    /// Byte offset of the char boundary nearest to `x` (text-relative, scroll already applied).
    fn offset_at(&self, x: f32, stops: &[f32]) -> usize {
        let n = self.text.chars().count() + 1;
        let stops = &stops[..stops.len().min(n)];
        let mut best = 0;
        let mut best_d = f32::INFINITY;
        for (i, &s) in stops.iter().enumerate() {
            let d = (s - x).abs();
            if d < best_d {
                best_d = d;
                best = i;
            }
        }
        self.text
            .char_indices()
            .nth(best)
            .map_or(self.text.len(), |(p, _)| p)
    }

    /// Mouse press at `x` logical px from the left edge of the text area (before scroll; the field
    /// adds its own scroll). `stops` is `Painter::caret_stops` for the current text. `extend` is
    /// Shift.
    pub fn press(&mut self, x: f32, stops: &[f32], extend: bool) {
        self.restart = true;
        let at = self.offset_at(x + self.scroll, stops);
        self.caret = at;
        if !extend {
            self.anchor = at;
        }
    }

    /// Mouse drag to `x`: extends the selection from the press anchor.
    pub fn drag(&mut self, x: f32, stops: &[f32]) {
        self.restart = true;
        self.caret = self.offset_at(x + self.scroll, stops);
    }

    /// Double click: selects the word (or the run of spaces) at `x`.
    pub fn select_word_at(&mut self, x: f32, stops: &[f32]) {
        if self.text.is_empty() {
            return;
        }
        // A secret's words are not shown, so a double click takes all of it.
        if self.secret {
            self.select_all();
            return;
        }
        let x = x + self.scroll;
        let count = self.text.chars().count();
        let stops = &stops[..stops.len().min(count + 1)];
        // The char whose span holds x.
        let mut idx = 0;
        for (i, &s) in stops.iter().enumerate().take(count) {
            if s <= x {
                idx = i;
            }
        }
        let (start, c) = self.text.char_indices().nth(idx).unwrap();
        let k = class(c);
        let mut a = start;
        for (p, ch) in self.text[..start].char_indices().rev() {
            if class(ch) != k {
                break;
            }
            a = p;
        }
        let mut b = start;
        for ch in self.text[start..].chars() {
            if class(ch) != k {
                break;
            }
            b += ch.len_utf8();
        }
        self.restart = true;
        self.anchor = a;
        self.caret = b;
    }

    /// Draws text, selection and caret inside `area` (the text area, already inset from the box
    /// the caller drew), clipped to it and vertically centred, scrolling horizontally so the caret
    /// stays visible. A selection and the caret (when `caret_on`) show only when `focused`. When
    /// the text is empty, `placeholder` is drawn in the placeholder colour.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        p: &mut Painter,
        area: Rect,
        face: Face,
        size: f32,
        colors: &FieldColors,
        placeholder: &str,
        focused: bool,
        caret_on: bool,
    ) {
        // While an input method composes, its text is drawn in place at the caret, underlined,
        // with the input method's caret; the selection is not shown.
        // A secret shows neither its text nor what is being composed for it.
        let composing = self.preedit.as_ref().filter(|_| focused && !self.secret);
        let shown: std::borrow::Cow<str> = match composing {
            Some(pre) => {
                let mut s = self.text.clone();
                s.insert_str(self.caret, &pre.text);
                s.into()
            }
            // Owned, so the scroll below can still change.
            None if self.secret => self.display().into_owned().into(),
            None => self.text.as_str().into(),
        };
        let (caret_b, anchor_b) = match composing {
            Some(pre) => (self.caret + pre.caret(), self.caret + pre.caret()),
            None if self.secret => (self.masked(self.caret), self.masked(self.anchor)),
            None => (self.caret, self.anchor),
        };
        let stops = p.caret_stops(face, size, &shown);
        let total = stops.last().copied().unwrap_or(0.0);
        let caret_idx = shown[..caret_b].chars().count().min(stops.len() - 1);
        let anchor_idx = shown[..anchor_b].chars().count().min(stops.len() - 1);
        let cx = stops[caret_idx];

        // Keep the caret in view, then keep the scroll within the text.
        let max_scroll = (total + 1.0 - area.w).max(0.0);
        if cx - self.scroll > area.w - 1.0 {
            self.scroll = cx - (area.w - 1.0);
        }
        if cx < self.scroll {
            self.scroll = cx;
        }
        self.scroll = self.scroll.clamp(0.0, max_scroll);

        let cy = area.cy();
        let h = size * 1.25;
        let top = cy - h / 2.0;
        let old = p.clip(area);
        let ox = area.x - self.scroll;
        if shown.is_empty() {
            p.text(
                face,
                size,
                area.x,
                cy,
                placeholder,
                colors.placeholder,
                area.w,
            );
        } else {
            if focused && caret_idx != anchor_idx {
                let (a, b) = (
                    stops[caret_idx.min(anchor_idx)],
                    stops[caret_idx.max(anchor_idx)],
                );
                p.fill_a(Rect::new(ox + a, top, b - a, h), colors.selection, 0.35);
            }
            p.text(face, size, ox, cy, &shown, colors.text, f32::INFINITY);
            if let Some(pre) = composing {
                let at = self.caret;
                p.underline_range(
                    face,
                    size,
                    ox,
                    cy,
                    &shown,
                    at..at + pre.text.len(),
                    colors.text,
                );
                // The clause the input method has selected, a second line under it.
                if let Some((a, b)) = pre.cursor.filter(|c| c.0 < c.1) {
                    let a_x = p.measure(face, size, &shown[..at + a]);
                    let b_x = p.measure(face, size, &shown[..at + b]);
                    p.fill_a(
                        Rect::new(ox + a_x, top, b_x - a_x, h),
                        colors.selection,
                        0.25,
                    );
                }
            }
        }
        if focused && caret_on {
            let x = if shown.is_empty() { area.x } else { ox + cx };
            p.fill(Rect::new(x, top, 1.0f32.max(p.px()), h), colors.caret);
        }
        p.restore(old);
    }

    /// Whether the caret is drawn now, for a caret that blinks at the system's rate: the
    /// `caret_on` that [`draw`](Self::draw) (or a caller that draws its own field, as the
    /// command palette does) is given. It is read from the blink named `id` of `anim`, which the
    /// caller owns and has begun a frame on; `blink` is the half-period, on and off alike, from
    /// [`platform::caret_blink_time`](crate::platform::caret_blink_time).
    ///
    /// It is false while the field is not `focused`, and false while a selection is shown (the
    /// caret is hidden over a selection, as on Windows); in both cases the animator is not
    /// asked, so it forgets the blink at the end of the frame and gives no deadline for it. It
    /// is true, steadily and with no deadline, when `blink` is `None` (blinking switched off) or
    /// zero, and with reduced motion. Otherwise it asks [`Animator::blink`], whose deadline at
    /// [`end_frame`](Animator::end_frame) is the moment the caret next turns on or off.
    ///
    /// The blink starts again, on, the first time it is asked for after the field was typed
    /// in, its caret moved or its selection changed (any of the calls that edit or move), and
    /// after a frame in which the field was not focused: so the caret is solid while the user
    /// types and the moment focus arrives.
    pub fn blink_caret(
        &mut self,
        anim: &mut Animator,
        id: impl AsId,
        blink: Option<Duration>,
        focused: bool,
    ) -> bool {
        let id = id.as_id();
        if std::mem::take(&mut self.restart) {
            anim.forget(id);
        }
        if !focused {
            // Not asked, so forgotten at the end of this frame; focus starts it on.
            return false;
        }
        if self.selection_shown() {
            // Asked for again once the selection goes; that move set the restart flag.
            return false;
        }
        match blink {
            Some(half) => anim.blink(id, half, half),
            None => true,
        }
    }

    /// [`draw`](Self::draw) with a caret that blinks at the system's rate: `caret_on` is
    /// [`blink_caret`](Self::blink_caret) of the same `anim`, `id`, `blink` and `focused`. Draws
    /// exactly what `draw` draws with that `caret_on`, and the loop waits for
    /// [`Animator::end_frame`], which wakes it only while the field is focused and its caret
    /// blinks.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_blinking(
        &mut self,
        p: &mut Painter,
        anim: &mut Animator,
        id: impl AsId,
        blink: Option<Duration>,
        area: Rect,
        face: Face,
        size: f32,
        colors: &FieldColors,
        placeholder: &str,
        focused: bool,
    ) {
        let caret_on = self.blink_caret(anim, id, blink, focused);
        self.draw(p, area, face, size, colors, placeholder, focused, caret_on);
    }

    /// Whether `draw` would show a selection of a focused field: there is one, and no input
    /// method is composing over it.
    fn selection_shown(&self) -> bool {
        let composing = self.preedit.is_some() && !self.secret;
        self.has_selection() && !composing
    }
}

impl TextField {
    /// Where byte offset `at` of the text falls in the dots a secret draws.
    fn masked(&self, at: usize) -> usize {
        self.text[..at].chars().count() * MASK.len_utf8()
    }
}

/// Drops control characters; newlines and tabs become spaces.
fn clean(text: &str) -> String {
    text.replace("\r\n", "\n")
        .chars()
        .filter_map(|c| match c {
            '\n' | '\r' | '\t' => Some(' '),
            c if c.is_control() => None,
            c => Some(c),
        })
        .collect()
}

#[cfg(test)]
#[path = "textfield_tests.rs"]
mod blink_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_composition_shows_until_it_is_committed() {
        assert_eq!(Preedit::new(String::new(), None), None);
        // A cursor off a character boundary is dropped, not trusted.
        let p = Preedit::new("çay".into(), Some((0, 1))).unwrap();
        assert_eq!(p.cursor, None);
        assert_eq!(p.caret(), 4);
        let mut f = field("ab");
        assert!(f.set_preedit(Preedit::new("か".into(), Some((3, 3)))));
        assert!(!f.set_preedit(Preedit::new("か".into(), Some((3, 3)))));
        assert_eq!(f.text(), "ab", "a composition is not text yet");
        f.insert("家");
        assert_eq!(f.text(), "ab家");
        assert_eq!(f.preedit(), None);
        f.set_preedit(Preedit::new("x".into(), None));
        f.clear();
        assert_eq!(f.preedit(), None);
    }

    fn field(s: &str) -> TextField {
        let mut f = TextField::new();
        f.set_text(s);
        f
    }

    /// 10 px per char.
    fn stops(s: &str) -> Vec<f32> {
        (0..=s.chars().count()).map(|i| i as f32 * 10.0).collect()
    }

    fn at(f: &mut TextField, caret: usize) {
        f.caret = caret;
        f.anchor = caret;
    }

    #[test]
    fn a_secret_draws_one_dot_per_char_and_selects_whole() {
        let mut f = field("sk-ğü");
        assert_eq!(f.display(), "sk-ğü");
        f.set_secret(true);
        assert_eq!(f.display(), "•••••");
        assert_eq!(f.text(), "sk-ğü");
        // The caret after "sk-ğ" (5 bytes) is after the fourth dot.
        at(&mut f, 5);
        assert_eq!(f.masked(f.caret), 4 * MASK.len_utf8());
        f.select_word_at(1.0, &stops("•••••"));
        assert_eq!(f.selected_text(), "sk-ğü");
    }

    #[test]
    fn insert_and_replace_selection() {
        let mut f = TextField::new();
        assert!(f.insert("hello world"));
        assert_eq!(f.text(), "hello world");
        f.select_all();
        assert_eq!(f.selected_text(), "hello world");
        assert!(f.insert("x"));
        assert_eq!(f.text(), "x");
        assert!(!f.has_selection());
        at(&mut f, 0);
        f.insert("ab");
        assert_eq!(f.text(), "abx");
        assert_eq!(f.caret, 2);
        assert!(!f.insert(""));
    }

    #[test]
    fn control_characters_filtered() {
        let mut f = TextField::new();
        f.insert("a\tb\nc\r\nd\u{0}e\u{7f}\u{1b}f");
        assert_eq!(f.text(), "a b c def");
        assert!(!f.insert("\u{0}\u{1}"));
        f.set_text("x\ny");
        assert_eq!(f.text(), "x y");
    }

    #[test]
    fn backspace_delete_at_ends() {
        let mut f = field("ab");
        assert!(!f.key(EditKey::Delete, false, false));
        assert!(f.key(EditKey::Backspace, false, false));
        assert_eq!(f.text(), "a");
        at(&mut f, 0);
        assert!(!f.key(EditKey::Backspace, false, false));
        assert!(f.key(EditKey::Delete, false, false));
        assert_eq!(f.text(), "");
        assert!(!f.key(EditKey::Delete, true, false));
        assert!(!f.key(EditKey::Backspace, true, false));
        assert!(!f.delete_selection());
    }

    #[test]
    fn backspace_deletes_selection() {
        let mut f = field("hello");
        f.key(EditKey::Left, false, true);
        f.key(EditKey::Left, false, true);
        assert!(f.key(EditKey::Backspace, false, false));
        assert_eq!(f.text(), "hel");
        f.select_all();
        assert!(f.key(EditKey::Delete, true, true));
        assert!(f.is_empty());
    }

    #[test]
    fn word_movement() {
        let mut f = field("foo  bar_1 .. baz");
        f.key(EditKey::Left, true, false);
        assert_eq!(f.caret, 14);
        f.key(EditKey::Left, true, false);
        assert_eq!(f.caret, 11);
        f.key(EditKey::Left, true, false);
        assert_eq!(f.caret, 5);
        f.key(EditKey::Left, true, false);
        assert_eq!(f.caret, 0);
        f.key(EditKey::Left, true, false);
        assert_eq!(f.caret, 0);
        f.key(EditKey::Right, true, false);
        assert_eq!(f.caret, 3);
        f.key(EditKey::Right, true, false);
        assert_eq!(f.caret, 10);
        f.key(EditKey::Right, true, false);
        assert_eq!(f.caret, 13);
        f.key(EditKey::Right, true, false);
        assert_eq!(f.caret, 17);
        f.key(EditKey::Right, true, false);
        assert_eq!(f.caret, 17);
    }

    #[test]
    fn word_delete() {
        let mut f = field("one two  three");
        assert!(f.key(EditKey::Backspace, true, false));
        assert_eq!(f.text(), "one two  ");
        assert!(f.key(EditKey::Backspace, true, false));
        assert_eq!(f.text(), "one ");
        at(&mut f, 0);
        assert!(f.key(EditKey::Delete, true, false));
        assert_eq!(f.text(), " ");
        let mut g = field("a  b");
        at(&mut g, 1);
        g.key(EditKey::Delete, true, false);
        assert_eq!(g.text(), "a");
    }

    #[test]
    fn shift_extend_then_collapse() {
        let mut f = field("abcdef");
        at(&mut f, 3);
        f.key(EditKey::Right, false, true);
        f.key(EditKey::Right, false, true);
        assert_eq!(f.selected_text(), "de");
        f.key(EditKey::Right, false, false);
        assert!(!f.has_selection());
        assert_eq!(f.caret, 5);
        f.key(EditKey::Left, false, true);
        f.key(EditKey::Left, false, true);
        assert_eq!(f.selected_text(), "de");
        f.key(EditKey::Left, false, false);
        assert_eq!(f.caret, 3);
        assert!(!f.has_selection());
        f.key(EditKey::End, false, true);
        assert_eq!(f.selected_text(), "def");
        f.key(EditKey::Home, false, true);
        assert_eq!(f.selected_text(), "abc");
        f.key(EditKey::Home, false, false);
        assert!(!f.has_selection());
        assert_eq!(f.caret, 0);
    }

    #[test]
    fn select_word() {
        let s = "foo bar  baz";
        let mut f = field(s);
        let st = stops(s);
        f.select_word_at(45.0, &st);
        assert_eq!(f.selected_text(), "bar");
        f.select_word_at(2.0, &st);
        assert_eq!(f.selected_text(), "foo");
        f.select_word_at(75.0, &st);
        assert_eq!(f.selected_text(), "  ");
        f.select_word_at(500.0, &st);
        assert_eq!(f.selected_text(), "baz");
        f.select_word_at(-50.0, &st);
        assert_eq!(f.selected_text(), "foo");
        TextField::new().select_word_at(5.0, &[0.0]);
    }

    #[test]
    fn multibyte_safety() {
        let s = "İı çay 日本語 😀x";
        let mut f = field(s);
        let st = stops(s);
        for _ in 0..30 {
            f.key(EditKey::Left, false, false);
            assert!(s.is_char_boundary(f.caret));
        }
        assert_eq!(f.caret, 0);
        for _ in 0..30 {
            f.key(EditKey::Right, true, true);
            assert!(s.is_char_boundary(f.caret));
            let _ = f.selected_text();
        }
        for i in 0..=s.chars().count() * 10 + 20 {
            f.press(i as f32, &st, false);
            assert!(s.is_char_boundary(f.caret));
            f.select_word_at(i as f32, &st);
            let _ = f.selected_text();
        }
        f.select_all();
        f.insert("😀");
        f.key(EditKey::Backspace, false, false);
        assert!(f.is_empty());
        let mut g = field("a😀b");
        at(&mut g, 1);
        g.key(EditKey::Delete, false, false);
        assert_eq!(g.text(), "ab");
        g.set_text("日本語");
        g.key(EditKey::Backspace, true, false);
        assert!(g.is_empty());
    }

    #[test]
    fn press_and_drag() {
        let s = "abcdef";
        let mut f = field(s);
        let st = stops(s);
        f.press(24.0, &st, false);
        assert_eq!(f.caret, 2);
        assert!(!f.has_selection());
        f.drag(41.0, &st);
        assert_eq!(f.selected_text(), "cd");
        f.drag(-100.0, &st);
        assert_eq!(f.selected_text(), "ab");
        f.drag(1000.0, &st);
        assert_eq!(f.selected_text(), "cdef");
        f.press(0.0, &st, false);
        f.press(36.0, &st, true);
        assert_eq!(f.selected_text(), "abcd");
        // Mismatched or empty stops do not panic.
        f.press(5.0, &[], false);
        f.drag(5.0, &[0.0]);
        let mut e = TextField::new();
        e.press(10.0, &[0.0], false);
        e.drag(10.0, &[0.0]);
        assert_eq!(e.caret, 0);
    }

    #[test]
    fn scroll_is_added_to_x() {
        let s = "abcdef";
        let mut f = field(s);
        f.scroll = 20.0;
        f.press(0.0, &stops(s), false);
        assert_eq!(f.caret, 2);
    }

    #[test]
    fn handle_key_edits_and_declines() {
        let none = Mods::default();
        let ctrl = Mods {
            ctrl: true,
            ..Mods::default()
        };
        let shift = Mods {
            shift: true,
            ..Mods::default()
        };
        let mut f = TextField::new();
        assert_eq!(
            f.handle_key(&KeyName::Char('a'), none, Some("a"), false),
            Some(true)
        );
        assert_eq!(
            f.handle_key(&KeyName::Char('b'), none, Some("b"), false),
            Some(true)
        );
        assert_eq!(f.text(), "ab");
        assert_eq!(
            f.handle_key(&KeyName::Left, shift, None, false),
            Some(false)
        );
        assert_eq!(f.selected_text(), "b");
        assert_eq!(
            f.handle_key(&KeyName::Backspace, none, None, false),
            Some(true)
        );
        assert_eq!(f.text(), "a");
        assert_eq!(
            f.handle_key(&KeyName::Char('a'), ctrl, Some("a"), false),
            Some(false)
        );
        assert!(f.has_selection());
        // Ctrl+letters are shortcuts, not text; Enter and Escape are the caller's.
        assert_eq!(
            f.handle_key(&KeyName::Char('k'), ctrl, Some("k"), false),
            None
        );
        assert_eq!(f.handle_key(&KeyName::Enter, none, Some("\r"), false), None);
        assert_eq!(f.handle_key(&KeyName::Escape, none, None, false), None);
        assert_eq!(f.handle_key(&KeyName::Down, none, None, false), None);
        // A key with no text and no meaning is not an editing key.
        assert_eq!(f.handle_key(&KeyName::Char('z'), none, None, false), None);
    }
}
