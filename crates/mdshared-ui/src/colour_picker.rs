//! A colour picker: a saturation and brightness square over a hue strip, a swatch of the colour
//! it opened with beside the new one, and the colour as six hex digits in a text field. The
//! settings form shows one in a popover over a colour row (`settings::Form::show_as_colour`);
//! anything else that wants one places it with [`flyout`] and draws it with [`draw_picker`].
//!
//! The model is [`Picker`]: the colour as hue, saturation and brightness ([`Hsv`]), the colour
//! it opened with, which part has the keyboard ([`Part`]) and which part the pointer holds. The
//! caller owns the events and the placement: it lays the parts out with [`Layout::new`] from
//! the card's content rectangle, passes a press, a drag, a release and the keys in, and acts on
//! the [`Outcome`] (a new colour, accepted, cancelled). [`draw_picker`] draws a picker and
//! changes nothing in it but its text field's scroll (as every text field's drawing does).
//! For a popup with Apply and Cancel buttons, use [`confirmation_flyout`],
//! [`ConfirmationLayout`] and the `*_with_confirmation` methods. They return a
//! [`Confirmation`] so the caller can keep edits local until the picker is applied.
//!
//! The colour is kept as HSV, and the hex is made from it, never the other way round while the
//! user works: so a drag through grey or black keeps the hue (and through black the saturation)
//! that the square was showing, where a colour read back from its hex would have lost them. A
//! hex typed or set from outside ([`Picker::set_rgb`]) is read into HSV, keeping the hue of the
//! moment when the colour is a grey and the hue and saturation when it is black. Every 6-digit
//! hex read in and written out again is itself ([`rgb_to_hsv`] then [`hsv_to_rgb`] is the
//! identity on all of them; the tests check a dense sample and every grey).
//!
//! Keys, when the picker has them: Tab and Shift+Tab move between the square, the hue strip and
//! the hex field. On the square Left and Right take saturation down and up, Up and Down
//! brightness up and down, by 1% (Shift: 10%); on the hue strip Left and Down take the hue
//! down, Right and Up up, by 1 degree (Shift: 10), round the circle. The hex field takes the
//! editing keys; a text that is a colour moves the markers at once, anything else waits. Enter
//! accepts ([`Outcome::Accept`]), Escape cancels ([`Outcome::Cancel`]), from any part.
//!
//! Why a strip and not a ring for the hue: the strip is the width of the square, under it, so
//! the card stays small and rectangular; one axis is one number, so a press, a drag and the
//! arrow keys mean the same thing on it, with nothing round the corners of a square set in a
//! ring; and it costs a row of pixels to compute, where a ring needs every pixel's angle and an
//! antialiased edge on both sides.
//!
//! Drawing is the painter's only, in software: the square and the strip are computed into
//! bitmaps kept in a [`Cache`] (the square per size in device pixels and hue, the strip per
//! size) and copied in, so a frame with the picker open costs two copies and the markers.
//!
//! Deliberately not here:
//! - alpha: the colours the applications keep are opaque six-digit hexes;
//! - other notations (`rgb()`, HSL numbers, three-digit hexes, colour names): the field takes
//!   exactly six hex digits, with or without `#`;
//! - an eyedropper, a palette of recent or saved colours, and colour management;
//! - placement, the shadow's motion and the focus scope: [`flyout`] places the card, and the
//!   caller decides when it opens and closes;
//! - a clock: nothing here moves by itself.

use crate::flyout::{self, Flyout};
use crate::gfx::{Face, Painter, Rect, Rgb};
use crate::keys::{KeyName, Mods};
use crate::menu::Item;
use crate::textfield::{ContextAction, FieldArea, FieldColors, FieldHit, TextField};
use crate::theme::Palette;
use mdshared_text::Bitmap;

mod confirmation;
pub use confirmation::{
    Button, Confirmation, ConfirmationLayout, ConfirmationLook, confirmation_flyout,
    draw_picker_with_confirmation,
};

// ---- colours ---------------------------------------------------------------------------------

/// A colour as hue (degrees, 0 to 360, both ends red), saturation and brightness ("value"), both
/// 0 to 1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hsv {
    pub h: f32,
    pub s: f32,
    pub v: f32,
}

impl Hsv {
    /// A colour of hue `h`, saturation `s` and brightness `v`, made sane: a hue outside 0 to
    /// 360 is taken round the circle (-10 is 350, 370 is 10; 360 stays 360, the same red as
    /// 0), saturation and brightness are held to 0 to 1, and a number that is not finite is 0.
    pub fn new(h: f32, s: f32, v: f32) -> Hsv {
        let h = if !h.is_finite() {
            0.0
        } else if (0.0..=360.0).contains(&h) {
            h
        } else {
            h.rem_euclid(360.0)
        };
        let unit = |x: f32| {
            if x.is_finite() {
                x.clamp(0.0, 1.0)
            } else {
                0.0
            }
        };
        Hsv {
            h,
            s: unit(s),
            v: unit(v),
        }
    }
}

/// How much of each of red, green and blue a hue lacks at full saturation: 0 where the hue has
/// all of the channel, 1 where it has none. [`hsv_to_rgb`] is `v * (1 - s * lack)` per channel.
fn lack(h: f32) -> [f32; 3] {
    let h6 = h / 60.0;
    [5.0f32, 3.0, 1.0].map(|n| {
        let k = (n + h6).rem_euclid(6.0);
        k.min(4.0 - k).clamp(0.0, 1.0)
    })
}

fn to_byte(x: f32) -> u8 {
    (x * 255.0).round().clamp(0.0, 255.0) as u8
}

/// The colour `c` in red, green and blue. Grey for no saturation (any hue), black for no
/// brightness. Takes `c` through [`Hsv::new`] first, so nonsense gives a colour, not a panic.
pub fn hsv_to_rgb(c: Hsv) -> Rgb {
    let c = Hsv::new(c.h, c.s, c.v);
    let m = lack(c.h);
    m.map(|m| to_byte(c.v * (1.0 - c.s * m)))
}

/// The colour `rgb` as hue, saturation and brightness. A grey (saturation 0) has hue 0, and
/// black saturation 0 too: a picker that keeps a hue through grey does so itself
/// ([`Picker::set_rgb`]).
pub fn rgb_to_hsv(rgb: Rgb) -> Hsv {
    let [r, g, b] = rgb.map(f32::from);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let v = max / 255.0;
    let s = if max == 0.0 { 0.0 } else { d / max };
    let h = if d == 0.0 {
        0.0
    } else if max == r {
        60.0 * ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    Hsv::new(h, s, v)
}

/// The colour in `text`: exactly six hex digits, either case, with or without one `#` before
/// them, and with space round them allowed. `None` for anything else (three digits, eight,
/// a name, an empty text).
pub fn parse_hex(text: &str) -> Option<Rgb> {
    let t = text.trim();
    let t = t.strip_prefix('#').unwrap_or(t);
    if t.len() != 6 || !t.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&t[i..i + 2], 16).ok();
    Some([byte(0)?, byte(2)?, byte(4)?])
}

/// How a hex is written: with a `#` or without, in capitals or small letters. Read from the
/// text a colour came in ([`HexForm::of`]) so the colour goes back in the same form.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HexForm {
    pub hash: bool,
    pub upper: bool,
}

impl HexForm {
    /// `#rrggbb` in small letters.
    pub const DEFAULT: HexForm = HexForm {
        hash: true,
        upper: false,
    };

    /// The form of `text`: a `#` when it starts with one (after space), capitals when it has a
    /// capital `A` to `F` and no small `a` to `f`. A text of digits only, or not a colour at
    /// all, is in small letters; an empty one is [`HexForm::DEFAULT`].
    pub fn of(text: &str) -> HexForm {
        let t = text.trim();
        if t.is_empty() {
            return HexForm::DEFAULT;
        }
        let upper = t.bytes().any(|b| (b'A'..=b'F').contains(&b))
            && !t.bytes().any(|b| (b'a'..=b'f').contains(&b));
        HexForm {
            hash: t.starts_with('#'),
            upper,
        }
    }

    /// `rgb` written in this form: `#1a2b3c`, `1A2B3C`, ...
    pub fn format(self, rgb: Rgb) -> String {
        let [r, g, b] = rgb;
        let hash = if self.hash { "#" } else { "" };
        if self.upper {
            format!("{hash}{r:02X}{g:02X}{b:02X}")
        } else {
            format!("{hash}{r:02x}{g:02x}{b:02x}")
        }
    }
}

impl Default for HexForm {
    fn default() -> HexForm {
        HexForm::DEFAULT
    }
}

// ---- geometry --------------------------------------------------------------------------------

/// The width of the picker's content, logical px: the square's, the strip's and the bottom
/// row's.
pub const CONTENT_W: f32 = 224.0;
/// The height of the saturation and brightness square.
pub const SQUARE_H: f32 = 150.0;
/// The height of the hue strip.
pub const HUE_H: f32 = 14.0;
/// The height of the bottom row: the two swatches and the hex field.
pub const ROW_H: f32 = 28.0;
/// The room between the square, the strip and the bottom row.
pub const GAP: f32 = 12.0;
/// The height of the picker's content.
pub const CONTENT_H: f32 = SQUARE_H + GAP + HUE_H + GAP + ROW_H;
/// The room round the content inside the card.
pub const PADDING: f32 = 12.0;
/// The width of the two swatches together (the colour it opened with, then the new one).
pub const SWATCHES_W: f32 = 64.0;
/// How far outside the square or the strip a press still takes it.
const GRAB: f32 = 4.0;

/// A flyout for a picker anchored to `anchor`: the content's size, [`PADDING`] round it. The
/// caller places it ([`Flyout::place_in`]) and lays the picker out in its
/// [`content_rect`](Flyout::content_rect).
pub fn flyout(anchor: Rect) -> Flyout {
    Flyout::new(anchor, (CONTENT_W, CONTENT_H)).padding(PADDING)
}

/// Where the parts of a picker are, logical px.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Layout {
    /// The saturation (left to right) and brightness (bottom to top) square.
    pub square: Rect,
    /// The hue strip, red to red left to right.
    pub hue: Rect,
    /// The swatch of the colour the picker opened with; a press on it goes back to it.
    pub old: Rect,
    /// The swatch of the colour now.
    pub new: Rect,
    /// The hex field.
    pub hex: Rect,
}

impl Layout {
    /// The parts laid out in `content` from its top left: the square across the top, the strip
    /// under it, the swatches and the hex field at the bottom. A content smaller than
    /// [`CONTENT_W`] by [`CONTENT_H`] (a window too small for the card) narrows the parts to
    /// its width and lets the bottom ones fall outside; a content that is not a number gives
    /// empty parts.
    pub fn new(content: Rect) -> Layout {
        if ![content.x, content.y, content.w, content.h]
            .iter()
            .all(|v| v.is_finite())
        {
            return Layout::default();
        }
        let (x, y) = (content.x, content.y);
        let w = content.w.clamp(0.0, CONTENT_W);
        let square = Rect::new(x, y, w, SQUARE_H);
        let hue = Rect::new(x, square.bottom() + GAP, w, HUE_H);
        let row_y = hue.bottom() + GAP;
        let sw = SWATCHES_W.min(w);
        let old = Rect::new(x, row_y, sw / 2.0, ROW_H);
        let new = Rect::new(x + sw / 2.0, row_y, sw / 2.0, ROW_H);
        let hx = x + sw + 8.0;
        let hex = Rect::new(hx, row_y, (x + w - hx).max(0.0), ROW_H);
        Layout {
            square,
            hue,
            old,
            new,
            hex,
        }
    }

    /// Where the square's marker is for `c`: saturation across, brightness up.
    pub fn square_marker(&self, c: Hsv) -> (f32, f32) {
        let r = self.square;
        (r.x + c.s * r.w, r.y + (1.0 - c.v) * r.h)
    }

    /// Where the strip's marker is for `c`, across.
    pub fn hue_marker(&self, c: Hsv) -> f32 {
        self.hue.x + c.h / 360.0 * self.hue.w
    }

    /// The saturation and brightness at a point of the square, held to its edges.
    fn sv_at(&self, x: f32, y: f32) -> (f32, f32) {
        let r = self.square;
        let s = fraction(x - r.x, r.w);
        let v = 1.0 - fraction(y - r.y, r.h);
        (s, v)
    }

    /// The hue at a point across the strip, held to its ends.
    fn hue_at(&self, x: f32) -> f32 {
        fraction(x - self.hue.x, self.hue.w) * 360.0
    }
}

/// `d` as a fraction of `len`, held to 0 to 1; 0 for a length of none or for NaN.
fn fraction(d: f32, len: f32) -> f32 {
    if len > 0.0 && d.is_finite() {
        (d / len).clamp(0.0, 1.0)
    } else {
        0.0
    }
}

fn grown(r: Rect, d: f32) -> Rect {
    r.inset(-d)
}

// ---- the model -------------------------------------------------------------------------------

/// A part of the picker that takes the keyboard or the pointer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Square,
    Hue,
    Hex,
}

/// What an event did to a [`Picker`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Nothing changed.
    None,
    /// The view changed (a marker moved within one colour, the focus, the field's caret or
    /// text), the colour did not.
    Redraw,
    /// The colour changed: [`Picker::rgb`] and [`Picker::hex`] are the new one.
    Colour,
    /// Enter: keep the colour and close.
    Accept,
    /// Escape: the caller closes and goes back to the colour it opened with
    /// ([`Picker::cancel`] puts the picker there).
    Cancel,
    /// A key the picker does not use.
    Unused,
}

/// A colour being picked. See the [module](self).
#[derive(Clone)]
pub struct Picker {
    hsv: Hsv,
    opened: Hsv,
    opened_rgb: Rgb,
    form: HexForm,
    focus: Part,
    held: Option<Part>,
    field: TextField,
    button_focus: Option<Button>,
    button_held: Option<Button>,
    button_hover: Option<Button>,
}

impl Picker {
    /// A picker opened on `rgb`, writing hexes in `form`, with the square taking the keys.
    pub fn new(rgb: Rgb, form: HexForm) -> Picker {
        let hsv = rgb_to_hsv(rgb);
        let mut field = TextField::new();
        field.set_text(&form.format(rgb));
        Picker {
            hsv,
            opened: hsv,
            opened_rgb: rgb,
            form,
            focus: Part::Square,
            held: None,
            field,
            button_focus: None,
            button_held: None,
            button_hover: None,
        }
    }

    /// A picker opened on the colour in `text`, writing in its form ([`HexForm::of`]). A text
    /// that is not a colour opens on `fallback` (and the hex written is the fallback's, in the
    /// form the text suggests).
    pub fn from_text(text: &str, fallback: Rgb) -> Picker {
        Picker::new(parse_hex(text).unwrap_or(fallback), HexForm::of(text))
    }

    /// The colour now.
    pub fn rgb(&self) -> Rgb {
        hsv_to_rgb(self.hsv)
    }

    /// The colour now as hue, saturation and brightness: where the markers are.
    pub fn hsv(&self) -> Hsv {
        self.hsv
    }

    /// The colour now as a hex in the picker's form.
    pub fn hex(&self) -> String {
        self.form.format(self.rgb())
    }

    /// The colour the picker opened with.
    pub fn opened(&self) -> Rgb {
        self.opened_rgb
    }

    /// The form hexes are written in.
    pub fn form(&self) -> HexForm {
        self.form
    }

    /// The part that has the keyboard.
    pub fn focus(&self) -> Part {
        self.focus
    }

    /// The part the pointer holds, between a press and its release.
    pub fn held(&self) -> Option<Part> {
        self.held
    }

    /// The hex field, for drawing.
    pub fn field(&self) -> &TextField {
        &self.field
    }

    /// The hex field, for a press or a drag in it: give it to a
    /// [`FieldInput`](crate::textfield::FieldInput) with [`hex_field_area`].
    pub fn field_mut(&mut self) -> &mut TextField {
        &mut self.field
    }

    /// The hex field's edit menu (Cut, Copy, Paste, Select all, from its real selection), for a
    /// right press on the field: the field takes the keyboard (from an Apply or Cancel button
    /// too), its caret and selection as they were. Choose an item with
    /// [`apply_hex_action`](Self::apply_hex_action).
    pub fn hex_context_menu(&mut self) -> Vec<Item<ContextAction>> {
        self.focus_hex();
        self.field.context_menu()
    }

    /// An item chosen from the hex field's edit menu, the field taking the keyboard. A text
    /// that is then a colour moves the markers, as typing it does: [`Outcome::Colour`]. Any
    /// other edit or a new selection is [`Outcome::Redraw`]; Copy, or an item the field
    /// refuses (Cut with nothing selected), is [`Outcome::None`].
    pub fn apply_hex_action(&mut self, action: ContextAction) -> Outcome {
        let focus = self.focus != Part::Hex || self.button_focus.is_some();
        self.focus_hex();
        let before = self.field.text().to_owned();
        if !self.field.apply_context_action(action) || action == ContextAction::Copy {
            return if focus {
                Outcome::Redraw
            } else {
                Outcome::None
            };
        }
        if self.field.text() == before {
            return Outcome::Redraw;
        }
        match parse_hex(self.field.text()) {
            Some(rgb) if self.take_rgb(rgb) => Outcome::Colour,
            _ => Outcome::Redraw,
        }
    }

    fn focus_hex(&mut self) {
        self.set_focus(Part::Hex);
        self.button_focus = None;
    }

    /// Moves the markers to `c` (made sane); true when the colour changed. The field shows the
    /// new hex.
    pub fn set_hsv(&mut self, c: Hsv) -> bool {
        let before = self.rgb();
        self.hsv = Hsv::new(c.h, c.s, c.v);
        self.show_hex();
        self.rgb() != before
    }

    /// Moves the markers to `rgb`, a colour typed or set from outside; true when the colour
    /// changed. The same colour leaves the markers where they are. A grey keeps the hue the
    /// strip shows, and black keeps the hue and the saturation, so the square does not jump to
    /// red under the pointer. The field shows the hex.
    pub fn set_rgb(&mut self, rgb: Rgb) -> bool {
        let changed = self.take_rgb(rgb);
        self.show_hex();
        changed
    }

    fn take_rgb(&mut self, rgb: Rgb) -> bool {
        if rgb == self.rgb() {
            return false;
        }
        let mut c = rgb_to_hsv(rgb);
        if c.s == 0.0 || c.v == 0.0 {
            c.h = self.hsv.h;
        }
        if c.v == 0.0 {
            c.s = self.hsv.s;
        }
        self.hsv = c;
        true
    }

    fn show_hex(&mut self) {
        let hex = self.hex();
        if self.field.text() != hex {
            self.field.set_text(&hex);
        }
    }

    /// Goes back to the colour the picker opened with, its markers where they were; true when
    /// the colour changed.
    pub fn cancel(&mut self) -> bool {
        self.held = None;
        let before = self.rgb();
        self.hsv = self.opened;
        self.show_hex();
        self.rgb() != before
    }

    /// Gives the keyboard to `part`. The hex field, left with a text that is not a colour,
    /// shows the colour's hex again.
    pub fn set_focus(&mut self, part: Part) {
        if self.focus == Part::Hex && part != Part::Hex {
            self.show_hex();
        }
        self.focus = part;
    }

    /// Tab (`back` false) or Shift+Tab: the next part, or the one before, round the three.
    pub fn tab(&mut self, back: bool) {
        let next = match (self.focus, back) {
            (Part::Square, false) | (Part::Hex, true) => Part::Hue,
            (Part::Hue, false) | (Part::Square, true) => Part::Hex,
            (Part::Hex, false) | (Part::Hue, true) => Part::Square,
        };
        self.set_focus(next);
    }

    /// A press at `x`, `y` in `layout`. On the square (or a few px round it) or on the strip:
    /// that part takes the keyboard and the pointer, and the marker jumps under the pointer. On
    /// the old swatch: back to the colour it opened with. On the hex field: the field takes the
    /// keyboard (the caller places the caret: a [`FieldInput`](crate::textfield::FieldInput)
    /// given [`Picker::field_mut`] and [`hex_field_area`] does all of the field's mouse
    /// editing). Anywhere else: [`Outcome::None`].
    pub fn press(&mut self, layout: &Layout, x: f32, y: f32) -> Outcome {
        self.held = None;
        if !x.is_finite() || !y.is_finite() {
            return Outcome::None;
        }
        if grown(layout.square, GRAB).contains(x, y) && layout.square.w > 0.0 {
            self.set_focus(Part::Square);
            self.held = Some(Part::Square);
            return self.point(layout, x, y);
        }
        if grown(layout.hue, GRAB).contains(x, y) && layout.hue.w > 0.0 {
            self.set_focus(Part::Hue);
            self.held = Some(Part::Hue);
            return self.point(layout, x, y);
        }
        if layout.old.contains(x, y) {
            return if self.cancel() {
                Outcome::Colour
            } else {
                Outcome::Redraw
            };
        }
        if layout.hex.contains(x, y) {
            self.set_focus(Part::Hex);
            return Outcome::Redraw;
        }
        Outcome::None
    }

    /// The pointer moved to `x`, `y` with a part held: its marker follows, held to the part's
    /// edges. [`Outcome::None`] with nothing held.
    pub fn drag(&mut self, layout: &Layout, x: f32, y: f32) -> Outcome {
        if self.held.is_none() || !x.is_finite() || !y.is_finite() {
            return Outcome::None;
        }
        self.point(layout, x, y)
    }

    /// The button went up: the part held is let go. True when one was held.
    pub fn release(&mut self) -> bool {
        self.held.take().is_some()
    }

    fn point(&mut self, layout: &Layout, x: f32, y: f32) -> Outcome {
        let before = (self.hsv, self.rgb());
        let c = match self.held {
            Some(Part::Square) => {
                let (s, v) = layout.sv_at(x, y);
                Hsv::new(self.hsv.h, s, v)
            }
            Some(Part::Hue) => Hsv::new(layout.hue_at(x), self.hsv.s, self.hsv.v),
            _ => return Outcome::None,
        };
        self.hsv = c;
        self.show_hex();
        if self.rgb() != before.1 {
            Outcome::Colour
        } else if self.hsv != before.0 {
            Outcome::Redraw
        } else {
            Outcome::None
        }
    }

    /// A key, with the text it typed. See the [module](self) for what each does.
    pub fn key(&mut self, key: &KeyName, mods: Mods, typed: Option<&str>) -> Outcome {
        match key {
            KeyName::Escape => return Outcome::Cancel,
            KeyName::Enter => return Outcome::Accept,
            KeyName::Tab => {
                self.tab(mods.shift);
                return Outcome::Redraw;
            }
            _ => {}
        }
        let big = mods.shift;
        let before = (self.hsv, self.rgb());
        match self.focus {
            Part::Square => {
                let d = if big { 0.1 } else { 0.01 };
                let (ds, dv) = match key {
                    KeyName::Left => (-d, 0.0),
                    KeyName::Right => (d, 0.0),
                    KeyName::Up => (0.0, d),
                    KeyName::Down => (0.0, -d),
                    _ => return Outcome::Unused,
                };
                self.hsv = Hsv::new(self.hsv.h, self.hsv.s + ds, self.hsv.v + dv);
            }
            Part::Hue => {
                let d = if big { 10.0 } else { 1.0 };
                let dh = match key {
                    KeyName::Left | KeyName::Down => -d,
                    KeyName::Right | KeyName::Up => d,
                    _ => return Outcome::Unused,
                };
                let h = if self.hsv.h >= 360.0 && dh > 0.0 {
                    dh
                } else {
                    self.hsv.h + dh
                };
                self.hsv = Hsv::new(h, self.hsv.s, self.hsv.v);
            }
            Part::Hex => {
                return match self.field.handle_key(key, mods, typed, true) {
                    Some(true) => match parse_hex(self.field.text()) {
                        Some(rgb) if self.take_rgb(rgb) => Outcome::Colour,
                        _ => Outcome::Redraw,
                    },
                    Some(false) => Outcome::Redraw,
                    None => Outcome::Unused,
                };
            }
        }
        self.show_hex();
        if self.rgb() != before.1 {
            Outcome::Colour
        } else if self.hsv != before.0 {
            Outcome::Redraw
        } else {
            Outcome::None
        }
    }
}

// ---- drawing ---------------------------------------------------------------------------------

/// The square and the strip computed into bitmaps, kept between frames: the square for one
/// size in device pixels and one hue, the strip for one size. And the hex field's caret stops,
/// measured as it was last drawn. A picker drawn at the same size and hue costs two copies.
#[derive(Default)]
pub struct Cache {
    square: Option<((i32, i32, u32), Bitmap)>,
    hue: Option<((i32, i32), Bitmap)>,
    stops: (String, Vec<f32>),
}

impl Cache {
    pub fn new() -> Cache {
        Cache::default()
    }

    /// The caret stops of the hex field's text as last drawn, for
    /// [`TextField::press`]. Empty before the first draw.
    pub fn hex_stops(&self) -> &[f32] {
        &self.stops.1
    }

    fn square(&mut self, w: i32, h: i32, hue: f32) -> &Bitmap {
        let key = (w, h, hue.to_bits());
        if self.square.as_ref().is_none_or(|(k, _)| *k != key) {
            self.square = Some((key, square_bitmap(w, h, hue)));
        }
        &self.square.as_ref().unwrap().1
    }

    fn hue(&mut self, w: i32, h: i32) -> &Bitmap {
        if self.hue.as_ref().is_none_or(|(k, _)| *k != (w, h)) {
            self.hue = Some(((w, h), hue_bitmap(w, h)));
        }
        &self.hue.as_ref().unwrap().1
    }
}

/// The share of the way along `n` pixels that pixel `i` is: 0 at the first, 1 at the last.
fn along(i: i32, n: i32) -> f32 {
    if n > 1 {
        i as f32 / (n - 1) as f32
    } else {
        0.0
    }
}

/// The square for `hue`, `w` by `h` device pixels: saturation 0 to 1 left to right, brightness
/// 1 to 0 top to bottom, its corners white, the pure hue, black and black.
fn square_bitmap(w: i32, h: i32, hue: f32) -> Bitmap {
    let mut bmp = Bitmap::new(w as u32, h as u32);
    let m = lack(Hsv::new(hue, 1.0, 1.0).h);
    let columns: Vec<[f32; 3]> = (0..w)
        .map(|x| {
            let s = along(x, w);
            m.map(|m| 1.0 - s * m)
        })
        .collect();
    let stride = w as usize * 4;
    if stride == 0 {
        return bmp;
    }
    for (y, row) in bmp.pixels.chunks_exact_mut(stride).enumerate() {
        let v = 1.0 - along(y as i32, h);
        for (px, c) in row.as_chunks_mut::<4>().0.iter_mut().zip(&columns) {
            px.copy_from_slice(&[to_byte(v * c[2]), to_byte(v * c[1]), to_byte(v * c[0]), 255]);
        }
    }
    bmp
}

/// The hue strip, `w` by `h` device pixels: every hue at full saturation and brightness, red
/// at both ends.
fn hue_bitmap(w: i32, h: i32) -> Bitmap {
    let mut bmp = Bitmap::new(w as u32, h as u32);
    let row: Vec<u8> = (0..w)
        .flat_map(|x| {
            let [r, g, b] = hsv_to_rgb(Hsv::new(along(x, w) * 360.0, 1.0, 1.0));
            [b, g, r, 255]
        })
        .collect();
    let stride = w as usize * 4;
    if stride > 0 {
        for dst in bmp.pixels.chunks_exact_mut(stride) {
            dst.copy_from_slice(&row);
        }
    }
    bmp
}

/// What the caller says about how the picker is shown.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Look {
    /// The keyboard has been used: the part with the focus has a ring.
    pub ring: bool,
    /// The blink phase of the hex field's caret, when the field has the focus.
    pub caret_on: bool,
}

/// The accent ring round a part with the keyboard, drawn before the part: an accent outline two
/// px outside `r`, the card's colour between.
fn focus_ring(p: &mut Painter, pal: &Palette, r: Rect, radius: f32) {
    p.rbox(r.inset(-3.0), radius + 3.0, pal.accent);
    p.rbox(r.inset(-1.5), radius + 1.5, pal.popup);
}

/// A hairline round `r`, just outside it, so the colours inside stay as computed.
fn outline(p: &mut Painter, r: Rect, c: Rgb, a: f32) {
    let px = p.px();
    p.fill_a(Rect::new(r.x - px, r.y - px, r.w + 2.0 * px, px), c, a);
    p.fill_a(Rect::new(r.x - px, r.bottom(), r.w + 2.0 * px, px), c, a);
    p.fill_a(Rect::new(r.x - px, r.y, px, r.h), c, a);
    p.fill_a(Rect::new(r.right(), r.y, px, r.h), c, a);
}

/// Where the hex field of a picker laid out at `layout` is, as [`draw_picker`] draws it at
/// `scale`: its frame (`layout.hex`; a press anywhere in it counts) and its text, inset 10 px
/// each side, in the monospace face at 13 px. For a caller that gives the hex field a whole
/// field's mouse editing (the click, the drag, the double and triple click, the edit menu):
/// keep a [`FieldInput`](crate::textfield::FieldInput) beside the picker and pass it
/// [`Picker::field_mut`] with this area, after [`Picker::press`] has given the field the
/// keyboard. A layout of no size gives an empty area, which the input ignores.
pub fn hex_field_area(layout: &Layout, scale: f32) -> FieldArea {
    let hex = layout.hex;
    FieldArea::new(
        hex,
        FieldHit {
            area: Rect::new(hex.x + 10.0, hex.y, (hex.w - 20.0).max(0.0), hex.h),
            face: Face::Mono,
            size: 13.0,
            scale,
        },
    )
}

/// Copies `bmp` (made for `r`'s size in device pixels) into `r`.
fn put(p: &mut Painter, r: Rect, bmp: &Bitmap) {
    let (x0, y0, _, _) = p.dev_rect(r);
    p.blit(bmp, 0, bmp.height as i32, x0, y0);
}

/// Draws `picker` on a card at `card` (the flyout's rectangle) with its parts at `layout`: the
/// card and its shadow as a flyout's ([`flyout::draw_card`]), the square for the picker's hue
/// and the hue strip from `cache`, a ring marker on the square and a handle on the strip
/// (white with a dark edge, readable on any colour), the swatches of the old and the new
/// colour, and the hex field, the part with the keyboard ringed when `look.ring`. Everything is
/// clipped to the card. A card of no size or not a number draws nothing.
pub fn draw_picker(
    p: &mut Painter,
    pal: &Palette,
    card: Rect,
    layout: &Layout,
    picker: &mut Picker,
    cache: &mut Cache,
    look: Look,
) {
    if ![card.x, card.y, card.w, card.h]
        .iter()
        .all(|v| v.is_finite())
        || card.w <= 0.0
        || card.h <= 0.0
    {
        return;
    }
    flyout::draw_card(p, pal, card);
    let old_clip = p.clip(card.inset(1.0));
    let edge = if pal.dark { [255, 255, 255] } else { [0, 0, 0] };
    let c = picker.hsv();

    // The square.
    let sq = layout.square;
    if look.ring && picker.focus() == Part::Square {
        focus_ring(p, pal, sq, 0.0);
    }
    let (x0, y0, x1, y1) = p.dev_rect(sq);
    if x1 > x0 && y1 > y0 {
        let bmp = cache.square(x1 - x0, y1 - y0, c.h);
        put(p, sq, bmp);
    }
    outline(p, sq, edge, 0.16);

    // The strip.
    let hue = layout.hue;
    if look.ring && picker.focus() == Part::Hue {
        focus_ring(p, pal, hue, 0.0);
    }
    let (x0, y0, x1, y1) = p.dev_rect(hue);
    if x1 > x0 && y1 > y0 {
        let bmp = cache.hue(x1 - x0, y1 - y0);
        put(p, hue, bmp);
    }
    outline(p, hue, edge, 0.16);

    // The markers: a white ring with a dark edge on the square, a white handle on the strip.
    let old = p.clip(grown(sq, 8.0));
    let (mx, my) = layout.square_marker(c);
    p.ring(mx, my, 6.0, 4.0, [0, 0, 0], 0.45);
    p.ring(mx, my, 6.0, 2.0, [255, 255, 255], 1.0);
    p.restore(old);
    let hx = layout.hue_marker(c);
    let handle = Rect::new((hx - 4.0).round(), hue.y - 3.0, 8.0, hue.h + 6.0);
    p.rbox_a(handle.inset(-1.0), 4.0, [0, 0, 0], 0.45);
    p.rbox(handle, 3.0, [255, 255, 255]);
    p.rbox(handle.inset(2.0), 1.5, hsv_to_rgb(Hsv::new(c.h, 1.0, 1.0)));

    // The swatches: the colour it opened with, then the new one, as one rounded box.
    let both = Rect::new(
        layout.old.x,
        layout.old.y,
        layout.new.right() - layout.old.x,
        layout.old.h,
    );
    if both.w > 0.0 {
        p.frame(both, 6.0, picker.opened(), pal.line);
        let o = p.clip(layout.new);
        p.rbox(both.inset(1.0), 5.0, picker.rgb());
        p.restore(o);
    }

    // The hex field.
    let hex = layout.hex;
    if hex.w > 0.0 {
        let focused = picker.focus() == Part::Hex;
        if look.ring && focused {
            focus_ring(p, pal, hex, 6.0);
        }
        p.frame(hex, 6.0, pal.field, pal.line);
        if focused {
            let o = p.clip(Rect::new(hex.x, hex.bottom() - 2.0, hex.w, 2.0));
            p.rbox(hex, 6.0, pal.accent);
            p.restore(o);
        }
        let shown = picker.field().display().into_owned();
        if cache.stops.0 != shown || cache.stops.1.is_empty() {
            cache.stops = (shown.clone(), p.caret_stops(Face::Mono, 13.0, &shown));
        }
        let colors = FieldColors {
            text: pal.text,
            placeholder: pal.faint,
            selection: pal.selection,
            caret: pal.text,
        };
        // The same text rectangle `hex_field_area` gives.
        let area = hex_field_area(layout, p.scale).text().area;
        picker.field_mut().draw(
            p,
            area,
            Face::Mono,
            13.0,
            &colors,
            "",
            focused,
            look.caret_on,
        );
    }
    p.restore(old_clip);
}

#[cfg(test)]
#[path = "colour_picker_tests.rs"]
mod tests;
