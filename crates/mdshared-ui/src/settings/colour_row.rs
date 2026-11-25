//! Text rows shown as colours ([`Form::show_as_colour`]): a [`Row::Text`] whose id the form was
//! given, holding a six-digit hex (`#1a2b3c` or `1A2B3C`), is drawn as a swatch of its colour
//! before a narrower text field, and a click on the swatch opens a colour picker
//! ([`colour_picker`]) in a popover anchored to the row. Nothing else about the row changes: it
//! is built with [`Row::text`], its value comes and goes as [`Value::Text`], the field takes
//! typing as every text row's does (the swatch follows it), and every change is an
//! [`Outcome::Changed`] with the text. A form never told of a colour draws and answers as it
//! always has.
//!
//! The marks are ids, kept by the form: they outlast [`Form::set_rows`], so a row rebuilt with
//! the same id is still a colour, and an id with no text row marks nothing until one comes. An
//! open picker closes at `set_rows` (its row may now be another), with no report.
//!
//! The popover: placed as a [`flyout`](crate::flyout) is, under the row's control (above it
//! when there is no room below, inside the window always), drawn over everything else in the
//! form with the flyout's card and shadow. While it is open it takes the keys and the pointer:
//! the square, the hue strip and the hex field are the picker's (see [`colour_picker`] for
//! which key does what). Every colour picked is put into the row's text at once, in the form
//! the text had when the picker opened (`#` or not, capitals or not), and reported as
//! [`Outcome::Changed`]: by a press, by [`Form::pointer`] during a drag (with
//! [`Form::pointer_move`], whose answer is a `bool`, the drag's change is reported by
//! [`Form::release`]), by a key at once. Enter, or F4 and Alt+Down on the row, open it from the
//! keyboard; Enter accepts and closes; Escape, a press anywhere outside the card (the swatch
//! too) cancels: the row's text goes back to what it was when the picker opened, exactly, and
//! when that differs from the last text reported, it is reported. The wheel does nothing while
//! it is open, so the row it hangs from stays where it is.
//!
//! No timer and no clock: the picker moves only on events, and an idle form with the picker
//! open does nothing. The square and the strip are kept as bitmaps between frames (see
//! [`colour_picker::Cache`]).
//!
//! Deliberately not here:
//! - other kinds of row as colours (a choice of named colours is a choice row);
//! - a colour row in the paged form ([`pages`](super::pages)), which draws such a row as its
//!   text field: the marks are this form's;
//! - motion: the popover appears and goes at once in [`Form::draw_animated`] too, and its
//!   markers jump; the rest of the form moves as it does there;
//! - checking the text: a row whose text is not a colour shows an empty swatch with a slash, and
//!   its picker opens on a mid grey.

use super::{Drag, Form, Outcome, Row, Value};
use crate::colour_picker::{self, Cache, Layout, Look, Part, Picker, parse_hex};
use crate::flyout::Flyout;
use crate::gfx::{Painter, Rect, Rgb, mix};
use crate::keys::{KeyName, Mods};
use crate::theme::Palette as Colors;

/// The swatch's width, logical px; it is as high as the control.
pub(super) const SWATCH_W: f32 = 28.0;
/// The room between the swatch and the text field.
pub(super) const SWATCH_GAP: f32 = 6.0;
/// The colour a picker opens on when its row's text is not a colour.
const FALLBACK: Rgb = [128, 128, 128];

/// The colour rows of a form: the marks, the open picker, the swatch pressed and the one under
/// the pointer.
#[derive(Default)]
pub(super) struct Colours {
    ids: Vec<u32>,
    open: Option<Open>,
    pressed: Option<usize>,
    hover: Option<usize>,
    cache: Cache,
}

/// An open picker.
struct Open {
    row: usize,
    picker: Picker,
    flyout: Flyout,
    layout: Layout,
    /// The row's text when the picker opened: what a cancel puts back.
    original: String,
    /// The text last reported in an [`Outcome::Changed`] (or set by the application).
    reported: String,
    /// The keyboard was used since it opened: the part with the focus is ringed.
    ring: bool,
    /// A press in the hex field is selecting text.
    text_drag: bool,
}

impl Form {
    /// Draws the text row `id` as a colour (see the [module](self)) from the next layout on:
    /// a swatch, then the hex in a narrower field, and a picker behind the swatch. Its value
    /// stays a [`Value::Text`]. The mark is kept by id through [`set_rows`](Self::set_rows),
    /// and marks a text row of that id added later. Returns whether the form has such a text
    /// row now (a secret one is never shown as a colour); another kind of row with that id is
    /// drawn as it was.
    pub fn show_as_colour(&mut self, id: u32) -> bool {
        if !self.colours.ids.contains(&id) {
            self.colours.ids.push(id);
        }
        self.rows
            .iter()
            .any(|r| matches!(r, Row::Text { id: k, secret: false, .. } if *k == id))
    }

    /// The colour in the text row `id`: its text read as six hex digits with or without `#`
    /// ([`colour_picker::parse_hex`]). `None` when there is no text row of that id or its text
    /// is not such a colour. Any text row is read, shown as a colour or not.
    pub fn colour(&self, id: u32) -> Option<Rgb> {
        self.rows.iter().find_map(|r| match r {
            Row::Text { id: k, value, .. } if *k == id => parse_hex(value),
            _ => None,
        })
    }

    /// The id of the row whose picker is open, if one is.
    pub fn colour_open(&self) -> Option<u32> {
        let o = self.colours.open.as_ref()?;
        self.rows.get(o.row).and_then(Row::id)
    }

    /// The open picker's card as last placed, or an empty rectangle.
    pub fn picker_rect(&self) -> Rect {
        self.colours
            .open
            .as_ref()
            .map_or(Rect::default(), |o| o.flyout.rect())
    }

    /// The parts of the open picker as last placed (the square, the strip, the swatches and
    /// the hex field), or `None`.
    pub fn picker_layout(&self) -> Option<Layout> {
        self.colours.open.as_ref().map(|o| o.layout)
    }

    /// The swatch of colour row `i`: the left end of its [`control_rect`](Self::control_rect).
    /// Empty for a row that is not shown as a colour.
    pub fn swatch_rect(&self, i: usize) -> Rect {
        if !self.is_colour(i) {
            return Rect::default();
        }
        let c = self.control_rect(i);
        Rect::new(c.x, c.y, SWATCH_W.min(c.w), c.h)
    }

    /// Whether row `i` is a text row shown as a colour.
    pub(super) fn is_colour(&self, i: usize) -> bool {
        !self.colours.ids.is_empty()
            && matches!(self.rows.get(i), Some(Row::Text { id, secret: false, .. }) if self.colours.ids.contains(id))
    }

    /// The text field of colour row `i`, after its swatch; `None` for any other row.
    pub(super) fn colour_field(&self, i: usize) -> Option<Rect> {
        if !self.is_colour(i) {
            return None;
        }
        let c = self.control_rect(i);
        let d = SWATCH_W + SWATCH_GAP;
        Some(Rect::new(c.x + d, c.y, (c.w - d).max(0.0), c.h))
    }

    /// Closes the picker with no report and forgets the swatch pressed: the rows were replaced.
    pub(super) fn close_colour(&mut self) {
        self.colours.open = None;
        self.colours.pressed = None;
        self.colours.hover = None;
    }

    /// Opens the picker of colour row `i`; `ring` when the keyboard opened it.
    fn open_colour(&mut self, i: usize, ring: bool) {
        let Some(Row::Text { value, .. }) = self.rows.get(i) else {
            return;
        };
        let text = value.clone();
        self.menu = None;
        self.drag = Drag::None;
        self.focus = Some(i);
        self.colours.open = Some(Open {
            row: i,
            picker: Picker::from_text(&text, FALLBACK),
            flyout: colour_picker::flyout(self.control_rect(i)),
            layout: Layout::default(),
            original: text.clone(),
            reported: text,
            ring,
            text_drag: false,
        });
        self.place_colour();
    }

    /// Places the open picker by its row, inside the window of the last layout.
    fn place_colour(&mut self) {
        let Some(row) = self.colours.open.as_ref().map(|o| o.row) else {
            return;
        };
        let anchor = self.control_rect(row);
        let window = self.window;
        if let Some(o) = &mut self.colours.open {
            o.flyout.set_anchor(anchor);
            o.flyout.place_in(window);
            o.layout = Layout::new(o.flyout.content_rect());
        }
    }

    /// Puts `text` into row `i` and its field; nothing when it is already there.
    fn put_colour_text(&mut self, i: usize, text: &str) {
        if let Some(Row::Text { value, .. }) = self.rows.get_mut(i)
            && value != text
        {
            text.clone_into(value);
            self.fields[i].set_text(text);
        }
    }

    /// The picker's colour put into its row.
    fn picked(&mut self) {
        if let Some(o) = &self.colours.open {
            let (row, hex) = (o.row, o.picker.hex());
            self.put_colour_text(row, &hex);
        }
    }

    /// [`Outcome::Changed`] when the open row's text differs from the text last reported,
    /// which it then is; [`Outcome::Redraw`] otherwise.
    fn colour_report(&mut self) -> Outcome {
        let Some(o) = &mut self.colours.open else {
            return Outcome::Redraw;
        };
        match self.rows.get(o.row) {
            Some(Row::Text { id, value, .. }) if *value != o.reported => {
                o.reported.clone_from(value);
                Outcome::Changed {
                    id: *id,
                    value: Value::Text(value.clone()),
                }
            }
            _ => Outcome::Redraw,
        }
    }

    /// Closes the open picker: `accept` keeps the colour (reporting a change not yet
    /// reported), otherwise the row's text goes back to what it was when it opened.
    fn close_colour_picker(&mut self, accept: bool) -> Outcome {
        let Some(o) = self.colours.open.take() else {
            return Outcome::None;
        };
        if !accept {
            self.put_colour_text(o.row, &o.original);
        }
        match self.rows.get(o.row) {
            Some(Row::Text { id, value, .. }) if *value != o.reported => Outcome::Changed {
                id: *id,
                value: Value::Text(value.clone()),
            },
            _ => Outcome::Redraw,
        }
    }

    /// What the picker's answer means to the form.
    fn colour_outcome(&mut self, o: colour_picker::Outcome) -> Outcome {
        match o {
            colour_picker::Outcome::Colour => {
                self.picked();
                self.colour_report()
            }
            colour_picker::Outcome::Accept => self.close_colour_picker(true),
            colour_picker::Outcome::Cancel => self.close_colour_picker(false),
            colour_picker::Outcome::Redraw => Outcome::Redraw,
            colour_picker::Outcome::None => Outcome::None,
            colour_picker::Outcome::Unused => Outcome::Unused,
        }
    }

    /// A key, before the form's own: all of them while a picker is open; Enter, F4 and
    /// Alt+Down on a colour row open its picker. `None` leaves the key to the form.
    pub(super) fn colour_key(
        &mut self,
        key: &KeyName,
        mods: Mods,
        typed: Option<&str>,
    ) -> Option<Outcome> {
        if let Some(o) = &mut self.colours.open {
            o.ring = true;
            let answer = o.picker.key(key, mods, typed);
            return Some(self.colour_outcome(answer));
        }
        let f = self.focus?;
        let opens = match key {
            KeyName::Enter | KeyName::F(4) => !mods.ctrl && !mods.alt,
            KeyName::Down => mods.alt && !mods.ctrl,
            _ => false,
        };
        if !opens || !self.is_colour(f) {
            return None;
        }
        self.focus_visible = true;
        self.open_colour(f, true);
        Some(Outcome::Redraw)
    }

    /// A press, before the form's own: inside an open picker it is the picker's, outside it
    /// cancels; on a colour row's swatch it waits for the release. `None` leaves it to the form.
    pub(super) fn colour_press(
        &mut self,
        x: f32,
        y: f32,
        mods: Mods,
        clicks: u8,
    ) -> Option<Outcome> {
        self.colours.pressed = None;
        if let Some(o) = &mut self.colours.open {
            o.ring = false;
            // A selection whose release was lost does not outlive the press.
            o.text_drag = false;
            if !o.flyout.contains(x, y) {
                return Some(self.close_colour_picker(false));
            }
            let answer = o.picker.press(&o.layout, x, y);
            let hex = o.layout.hex;
            if o.picker.focus() == Part::Hex && hex.contains(x, y) {
                let rel = x - (hex.x + 10.0);
                let stops = self.colours.cache.hex_stops().to_vec();
                let field = o.picker.field_mut();
                if clicks >= 2 {
                    field.select_word_at(rel, &stops);
                } else {
                    field.press(rel, &stops, mods.shift);
                    o.text_drag = true;
                }
            }
            return Some(match self.colour_outcome(answer) {
                Outcome::None => Outcome::Redraw,
                done => done,
            });
        }
        // An open dropdown takes the press first, as it does over any row: its menu may cover a
        // swatch, and a press anywhere else closes it.
        if self.colours.ids.is_empty()
            || self.menu.is_some()
            || !self.card.contains(x, y)
            || !self.body.contains(x, y)
        {
            return None;
        }
        let i = (0..self.rows.len())
            .find(|&i| self.is_colour(i) && self.swatch_rect(i).contains(x, y))?;
        self.colours.pressed = Some(i);
        self.focus = Some(i);
        Some(Outcome::Redraw)
    }

    /// The button went up, before the form's own: a part of the picker is let go (with the
    /// change not yet reported); a swatch pressed opens its picker if the pointer is still on
    /// it. `None` leaves it to the form.
    pub(super) fn colour_release(&mut self, x: f32, y: f32) -> Option<Outcome> {
        if let Some(o) = &mut self.colours.open {
            o.text_drag = false;
            if o.picker.release() {
                return Some(self.colour_report());
            }
            return Some(Outcome::None);
        }
        let i = self.colours.pressed.take()?;
        if self.is_colour(i) && self.swatch_rect(i).contains(x, y) {
            self.open_colour(i, false);
            return Some(Outcome::Redraw);
        }
        Some(Outcome::None)
    }

    /// The pointer moved with a picker open: a part held follows it (its colour put into the
    /// row, not reported), a selection in the hex field grows. Some(whether to redraw), or
    /// `None` with no picker open.
    pub(super) fn colour_move(&mut self, x: f32, y: f32) -> Option<bool> {
        let o = self.colours.open.as_mut()?;
        if o.text_drag {
            let rel = x - (o.layout.hex.x + 10.0);
            let field = o.picker.field_mut();
            let before = field.caret();
            field.drag(rel, self.colours.cache.hex_stops());
            return Some(field.caret() != before);
        }
        let answer = o.picker.drag(&o.layout, x, y);
        if answer == colour_picker::Outcome::Colour {
            self.picked();
        }
        Some(answer != colour_picker::Outcome::None)
    }

    /// [`Form::pointer`] with a picker open: a drag's change reported at once. `None` with no
    /// picker open.
    pub(super) fn colour_pointer(&mut self, x: f32, y: f32) -> Option<Outcome> {
        let moved = self.colour_move(x, y)?;
        Some(match self.colour_report() {
            Outcome::Redraw if !moved => Outcome::None,
            o => o,
        })
    }

    /// The swatch under the pointer, for its hover; true when that changed.
    pub(super) fn colour_hover(&mut self, x: f32, y: f32) -> bool {
        if self.colours.ids.is_empty() {
            return false;
        }
        let over = (self.card.contains(x, y) && self.body.contains(x, y))
            .then(|| {
                (0..self.rows.len())
                    .find(|&i| self.is_colour(i) && self.swatch_rect(i).contains(x, y))
            })
            .flatten();
        std::mem::replace(&mut self.colours.hover, over) != over
    }

    /// The application set the value of row `i`: an open picker on it follows a colour, and
    /// takes the text as reported.
    pub(super) fn colour_value_set(&mut self, i: usize) {
        let Some(Row::Text { value, .. }) = self.rows.get(i) else {
            return;
        };
        let text = value.clone();
        if let Some(o) = self.colours.open.as_mut().filter(|o| o.row == i) {
            if let Some(rgb) = parse_hex(&text) {
                o.picker.set_rgb(rgb);
            }
            o.reported = text;
        }
    }

    /// Draws colour row `i`'s swatch and returns its text field, or `None` for any other row.
    pub(super) fn draw_swatch(&self, p: &mut Painter, pal: &Colors, i: usize) -> Option<Rect> {
        let field = self.colour_field(i)?;
        let sw = self.swatch_rect(i);
        let open = self.colours.open.as_ref().is_some_and(|o| o.row == i);
        let hot = self.colours.hover == Some(i) || self.colours.pressed == Some(i);
        let edge = if open {
            pal.accent
        } else if hot {
            mix(pal.line, pal.text, 0.35)
        } else {
            pal.line
        };
        let colour = match self.rows.get(i) {
            Some(Row::Text { value, .. }) => parse_hex(value),
            _ => None,
        };
        match colour {
            Some(c) => {
                p.frame(sw, 6.0, edge, edge);
                p.rbox(sw.inset(1.0), 5.0, c);
            }
            None => {
                // Not a colour: an empty swatch with a slash through it.
                p.frame(sw, 6.0, pal.field, edge);
                let path = crate::gfx::Path::new()
                    .move_to(sw.x + 7.0, sw.bottom() - 7.0)
                    .line_to(sw.right() - 7.0, sw.y + 7.0);
                p.stroke_path(&path, 1.5, pal.muted, 1.0);
            }
        }
        Some(field)
    }

    /// Draws the open picker over everything, placed by its row.
    pub(super) fn draw_colour_picker(&mut self, p: &mut Painter, pal: &Colors, caret_on: bool) {
        if self.colours.open.is_none() {
            return;
        }
        self.place_colour();
        let Colours { open, cache, .. } = &mut self.colours;
        let Some(o) = open else {
            return;
        };
        let look = Look {
            ring: o.ring,
            caret_on,
        };
        colour_picker::draw_picker(
            p,
            pal,
            o.flyout.rect(),
            &o.layout,
            &mut o.picker,
            cache,
            look,
        );
    }
}
