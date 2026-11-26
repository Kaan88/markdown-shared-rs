//! Push buttons: primary, secondary, subtle and destructive, with the states a person sees
//! (rest, hovered, pressed, open, focused, disabled) and the mark of the default button. The
//! model is the label and the kind; the drawing is a free function that changes nothing.
//!
//! The caller owns the pointer, the hit, the keyboard and the focus. It asks [`hit`] whether a
//! press belongs to the button, draws with the flags it knows ([`Look`]), and calls [`key`] with
//! the key it received: Enter or Space answers `true` and the caller does the button's work. A
//! disabled button is drawn quiet, and the caller leaves it out of its own hits and keys, as it
//! does with every other disabled control.
//!
//! Deliberately not here: the click itself and what it does; motion (each state is the pixels of
//! the frame it is drawn on, and no frame is asked for); the pointer's tracking and the focus
//! model, which the caller runs and passes in as [`Look`] flags; key repeat, which [`key`] knows
//! nothing of (the caller decides whether a repeat activates again); and the accessible node,
//! which the caller builds with [`crate::a11y::button`] (the label as the name, [`Button::rect`]
//! as the bounds).

use crate::cover::contrast;
use crate::cursor::Cursor;
use crate::focus::draw_ring;
use crate::gfx::{Face, Painter, Path, Rect, Rgb, mix, rgb};
use crate::keys::{KeyName, Mods};
use crate::theme::Palette;
use crate::tokens::{HIGH_CONTRAST_DARK, HIGH_CONTRAST_LIGHT};

/// The height every push button wants, in logical px. A shorter rectangle still draws; a taller
/// one is filled and centred the same way.
pub const HEIGHT: f32 = 32.0;

/// The corner radius of a button, its focus ring and the outline of a default button, in
/// logical px. The same as [`crate::overlay::draw_dialog_button`] uses.
pub const RADIUS: f32 = 5.0;

/// The room left and right of a button's label, in logical px.
pub const PAD: f32 = 12.0;

/// The size of a button's label, in logical px. The same as the ordinary interface text and a
/// dialog button's label.
pub const TEXT: f32 = 13.0;

/// A light label ink, considered for the destructive fill when the palette's own light inks do
/// not reach 4.5:1 on it. The dark palette's `text` is 4.48:1 on its danger red, just under
/// WCAG's AA, and white is 5.6:1; there is no "on danger" colour in the palette, so this is the
/// one constant, and [`destructive_ink`] still picks whichever ink reads best.
pub const ON_DANGER: Rgb = rgb(0xffffff);

/// The four kinds of push button.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
pub enum Kind {
    /// The accent fill: the one action a view leads with.
    Primary,
    /// A quiet fill with an edge: the ordinary button.
    #[default]
    Secondary,
    /// No fill until the pointer is over it: a toolbar's or a row's button.
    Subtle,
    /// The danger fill: an action that removes or discards.
    Destructive,
}

impl Kind {
    /// All four kinds, in the order primary, secondary, subtle, destructive.
    pub const ALL: [Kind; 4] = [
        Kind::Primary,
        Kind::Secondary,
        Kind::Subtle,
        Kind::Destructive,
    ];

    /// The kind's name in lower case ("primary", "secondary", "subtle", "destructive"), for a
    /// log line or a test's message.
    pub fn name(self) -> &'static str {
        match self {
            Kind::Primary => "primary",
            Kind::Secondary => "secondary",
            Kind::Subtle => "subtle",
            Kind::Destructive => "destructive",
        }
    }
}

/// A button: its label and its kind, and nothing else. The caller keeps one beside the command
/// it runs, asks it for the rectangle it needs, and passes the label and kind to [`draw`].
///
/// The label is kept as it was given, spaces and all; measuring and drawing do not trim it. An
/// empty label is a button as wide as its padding that draws no text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Button {
    text: String,
    kind: Kind,
}

impl Button {
    /// A button that shows `text`, drawn in `kind`.
    pub fn new(text: impl Into<String>, kind: Kind) -> Button {
        Button {
            text: text.into(),
            kind,
        }
    }

    /// The label the button shows.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The kind the button is drawn in.
    pub fn kind(&self) -> Kind {
        self.kind
    }

    /// The width this button wants, in logical px: its label in the face and size [`draw`] uses,
    /// with [`PAD`] on each side, rounded to a whole pixel. An empty label is `2 * PAD` wide. A
    /// painter is needed because the width depends on the fonts the window has.
    pub fn width(&self, p: &mut Painter) -> f32 {
        width(p, &self.text, self.kind)
    }

    /// The rectangle this button wants with its top left at `x`, `y`: [`Button::width`] wide and
    /// [`HEIGHT`] high. The label is centred in it.
    pub fn rect(&self, p: &mut Painter, x: f32, y: f32) -> Rect {
        button_rect(p, x, y, &self.text, self.kind)
    }

    /// Whether a point is on this button, by [`hit`]: the rectangle answers, not the label's own
    /// pixels.
    pub fn hit(&self, r: Rect, x: f32, y: f32) -> bool {
        hit(r, x, y)
    }

    /// The cursor to show over this button: [`Cursor::Hand`], as over a link. A disabled button
    /// is left out of the caller's hits, so this is not asked for it.
    pub fn cursor(&self) -> Cursor {
        cursor()
    }

    /// What this button does with a key, by [`key`]: `true` for Enter and Space with neither
    /// Ctrl, Alt nor the Mac's Control held, `false` for everything else.
    pub fn key(&self, k: &KeyName, mods: Mods) -> bool {
        key(k, mods)
    }
}

impl Default for Button {
    /// An empty secondary button.
    fn default() -> Button {
        Button::new("", Kind::Secondary)
    }
}

/// The face a label is drawn in for `kind`: [`Face::Semibold`] for the two filled kinds, primary
/// and destructive, which lead the eye, and [`Face::Ui`] for the rest, as
/// [`crate::overlay::draw_dialog_button`] draws a primary label semibold and a secondary one
/// regular.
pub(crate) fn face(kind: Kind) -> Face {
    match kind {
        Kind::Primary | Kind::Destructive => Face::Semibold,
        Kind::Secondary | Kind::Subtle => Face::Ui,
    }
}

/// The width a button with this label wants, in logical px: the measured label in the face
/// `kind` draws it, plus [`PAD`] on each side, rounded. An empty label is `2 * PAD` wide. The
/// numbers are passed through, including a NaN or an infinite measurement, which a broken font
/// stack could give.
pub fn width(p: &mut Painter, label: &str, kind: Kind) -> f32 {
    (p.measure(face(kind), TEXT, label) + 2.0 * PAD).round()
}

/// The rectangle a button with `label` wants with its top left at `x`, `y`: [`width`] wide and
/// [`HEIGHT`] high. The numbers are passed through, including a NaN or an infinite `x` or `y`;
/// drawing such a rectangle draws nothing, and a hit on it is false.
pub fn button_rect(p: &mut Painter, x: f32, y: f32, label: &str, kind: Kind) -> Rect {
    Rect::new(x, y, width(p, label, kind), HEIGHT)
}

/// Whether the point `x`, `y` is on the button in `r`. The left and top edges belong to the
/// button and the right and bottom edges do not, as with [`Rect::contains`]. NaN and infinite
/// points are outside it.
pub fn hit(r: Rect, x: f32, y: f32) -> bool {
    r.contains(x, y)
}

/// The cursor a button asks for: the hand, which the toolkit shows over everything a click
/// activates ([`Cursor::Hand`]).
pub fn cursor() -> Cursor {
    Cursor::Hand
}

/// Whether the key activates a button: Enter and Space do, with or without Shift, when neither
/// Ctrl, Alt nor the Mac's Control ([`Mods::control`]) is held, as [`crate::toggle`] reads them;
/// every other key does not. The caller decides whether the keyboard is on the button at all,
/// and leaves a disabled button out of the keys as it leaves it out of the hits.
///
/// Repeat is not here: the key event carries no such flag in this API. A caller that gets one
/// (the window host's [`KeyPress::repeat`](crate::host::KeyPress::repeat)) should ignore a
/// repeat for Enter or Space, so a held key does not press the button again. Space acts on the
/// key-down event: the toolkit's window host sends no key-release event (`Event::Key` is a press
/// or a repeat), so a caller with a loop of its own that wants Windows' "act when Space goes up"
/// tracks the release itself.
pub fn key(key: &KeyName, mods: Mods) -> bool {
    !mods.ctrl && !mods.alt && !mods.control && matches!(key, KeyName::Enter | KeyName::Space)
}

/// How a button is drawn just now, from the caller's own state.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Look {
    /// The pointer is over it.
    pub hovered: bool,
    /// A press that started on the button is held. A press counts only while the pointer is
    /// still over it ([`Look::hovered`]), as a dialog button's does: dragging off the button
    /// puts it back to its hover look, and letting go outside does nothing.
    pub pressed: bool,
    /// What it opened (a menu, a popover) is open: drawn as pressed whatever the pointer is
    /// doing, so a menu opened from the keyboard, or one whose menu the pointer is inside, keeps
    /// its button lit. [`Look::pressed`] alone is not enough for that: it needs
    /// [`Look::hovered`].
    pub open: bool,
    /// The keyboard is on it: a focus ring lies just outside the rectangle, through
    /// [`draw_ring`].
    pub focused: bool,
    /// It cannot be used: drawn as the one quiet frame, with a faint label, whatever the other
    /// flags and whatever the kind.
    pub disabled: bool,
    /// It is the default button, the one Enter presses: a line runs just inside its edge, in the
    /// label's own ink on the two filled kinds and in the accent on the other two. Disabled, the
    /// mark is left out: no mark promises something the button cannot do.
    pub default: bool,
    /// The palette is a high-contrast one (the host's `Cx::high_contrast`, or
    /// [`crate::platform::system_high_contrast`]), so the destructive button takes the system's
    /// own colours: the page as its fill, the window text as its label and `danger` as its
    /// frame. The system's own contrast theme ([`crate::tokens::Tokens::from_system_colors`])
    /// needs this flag; [`HIGH_CONTRAST_LIGHT`] and [`HIGH_CONTRAST_DARK`] are recognised by
    /// identity without it, as [`crate::infobar`] recognises them.
    pub high_contrast: bool,
}

/// Whether `pal` is a high-contrast palette: the caller's `flag`, or one of the toolkit's two
/// high-contrast palettes, recognised by identity as [`crate::infobar::severity_colors_in`] does.
pub(crate) fn high_contrast(pal: &Palette, flag: bool) -> bool {
    flag || *pal == HIGH_CONTRAST_LIGHT || *pal == HIGH_CONTRAST_DARK
}

/// The colours a kind is drawn with, before the state is applied. Shared with the menu and split
/// buttons so the four kinds look the same wherever they appear.
pub(crate) struct Style {
    /// The fill at rest; `None` for a subtle button, which paints nothing until it is hovered.
    pub fill: Option<Rgb>,
    /// The fill under the pointer.
    pub hover: Rgb,
    /// The fill while pressed or open.
    pub press: Rgb,
    /// The label in every state.
    pub ink: Rgb,
    /// The edge at rest; `None` for a kind without one.
    pub border: Option<Rgb>,
}

/// The colours `kind` is drawn with in `pal`. Under `high_contrast` the destructive kind takes
/// the palette's own page, text and danger — no mixes — as an outlined danger button: the
/// system's contrast palettes are built so text reads on the page, the hover and the press.
pub(crate) fn style(pal: &Palette, kind: Kind, high_contrast: bool) -> Style {
    match kind {
        Kind::Primary => Style {
            fill: Some(pal.accent),
            hover: mix(pal.accent, pal.popup, 0.1),
            press: mix(pal.accent, pal.popup, 0.2),
            ink: pal.on_accent,
            border: None,
        },
        Kind::Secondary => Style {
            fill: Some(mix(pal.popup, pal.text, 0.04)),
            hover: pal.hover,
            press: pal.press,
            ink: pal.text,
            border: Some(pal.popup_line),
        },
        Kind::Subtle => Style {
            fill: None,
            hover: pal.hover,
            press: pal.press,
            ink: pal.text,
            border: None,
        },
        Kind::Destructive if high_contrast => Style {
            fill: Some(pal.page),
            hover: pal.hover,
            press: pal.press,
            ink: pal.text,
            border: Some(pal.danger),
        },
        Kind::Destructive => {
            let ink = destructive_ink(pal, pal.danger);
            // The hover and the press move the fill away from the label's ink, so the label
            // reads at least as well as it does at rest: darker where the ink is light, lighter
            // where it is dark.
            let away = shade(pal, ink);
            Style {
                fill: Some(pal.danger),
                hover: mix(pal.danger, away, 0.1),
                press: mix(pal.danger, away, 0.2),
                ink,
                border: None,
            }
        }
    }
}

/// The ink that reads best on `fill`, from the palette's own light and dark inks and
/// [`ON_DANGER`]: the dark palette's danger red is a light red, so its label must be dark or
/// white at more than its own `text` gives (4.48:1); the toolkit's dark high-contrast red is
/// lighter still, and takes the palette's black `on_accent`.
pub(crate) fn destructive_ink(pal: &Palette, fill: Rgb) -> Rgb {
    let mut best = ON_DANGER;
    let mut best_by = contrast(best, fill);
    for c in [pal.on_accent, pal.text, pal.page] {
        let by = contrast(c, fill);
        if by > best_by {
            best = c;
            best_by = by;
        }
    }
    best
}

/// The palette colour farthest from `ink`, out of `text` and `page`: the direction a hover or a
/// press moves a fill so that `ink` reads better on it.
pub(crate) fn shade(pal: &Palette, ink: Rgb) -> Rgb {
    if contrast(pal.text, ink) >= contrast(pal.page, ink) {
        pal.text
    } else {
        pal.page
    }
}

/// The colour of a default button's line: the label's `ink` on the two filled kinds, where the
/// ink is already known to read on the fill, and the accent on the other two, whose fill is
/// quiet.
pub(crate) fn default_line(pal: &Palette, kind: Kind, ink: Rgb) -> Rgb {
    if matches!(kind, Kind::Primary | Kind::Destructive) {
        ink
    } else {
        pal.accent
    }
}

/// The fill and label colour `look` asks for, from a kind's `style`.
pub(crate) fn state(style: &Style, look: Look) -> (Option<Rgb>, Rgb) {
    if look.open || (look.pressed && look.hovered) {
        (Some(style.press), style.ink)
    } else if look.hovered {
        (Some(style.hover), style.ink)
    } else {
        (style.fill, style.ink)
    }
}

/// Draws `kind`'s box in `r` for `look`, and answers the fill and label colour it used, so a
/// caller with something beside the label (a chevron) can draw it in the same ink. It draws no
/// label. A disabled button is the quiet frame; a rectangle with a side that is not positive or
/// not finite draws nothing.
pub(crate) fn draw_box(
    p: &mut Painter,
    pal: &Palette,
    r: Rect,
    kind: Kind,
    look: Look,
) -> (Option<Rgb>, Rgb) {
    if !sane(r) {
        return (None, pal.faint);
    }
    if look.disabled {
        p.frame(r, RADIUS, pal.field, pal.line);
        return (Some(pal.field), pal.faint);
    }
    if look.focused {
        draw_ring(p, pal, r, RADIUS);
    }
    let style = style(pal, kind, high_contrast(pal, look.high_contrast));
    let (fill, ink) = state(&style, look);
    if let Some(fill) = fill {
        match style.border {
            Some(border) => p.frame(r, RADIUS, fill, border),
            None => p.rbox(r, RADIUS, fill),
        }
    }
    if look.default {
        // Crisp, one device pixel, as the frame is: the fill is drawn again under the line, so
        // nothing else is touched. A subtle button with no fill takes the field's, which is the
        // colour a filled box would use.
        let line = default_line(pal, kind, ink);
        p.frame(r, RADIUS, fill.unwrap_or(pal.field), line);
    }
    (fill, ink)
}

/// Draws a button with `label` in `r` in `kind`, with the states `look` asks for, and changes
/// nothing.
///
/// At rest a primary button is the accent fill with `on_accent` text, a secondary one a quiet
/// fill with an edge and the text colour, a subtle one its label and no fill, and a destructive
/// one the danger fill with the ink [`destructive_ink`] picked (white on the dark palette's red,
/// the palette's black on a light high-contrast red). Hovered it moves the fill one way and
/// pressed the other, so the label keeps reading. Pressed counts only while the pointer is
/// still on the button; open draws the pressed fill whatever the pointer does. Focused,
/// [`draw_ring`] draws the accent ring one pixel outside the rectangle. Disabled, every kind is
/// the quiet field frame with a faint label, and neither the ring, the hover, the press, the
/// open nor the default mark is drawn.
///
/// A label wider than `r` is clipped at `r`'s width (and centred, so both its ends are cut); an
/// empty label draws the box alone. A rectangle with a side that is not positive or not finite,
/// or one whose numbers are not finite, draws nothing at all. Nothing is drawn outside `r`
/// except the focus ring of a focused button.
pub fn draw(p: &mut Painter, pal: &Palette, r: Rect, label: &str, kind: Kind, look: Look) {
    let (_, ink) = draw_box(p, pal, r, kind, look);
    if !sane(r) {
        return;
    }
    // A disabled button is the one quiet drawing, in the dialog's semibold whatever its kind;
    // the kind no longer shows.
    let face = if look.disabled {
        Face::Semibold
    } else {
        face(kind)
    };
    p.text_center(face, TEXT, r, label, ink);
}

/// Whether a rectangle can be drawn at all: every number finite and both sides positive.
fn sane(r: Rect) -> bool {
    [r.x, r.y, r.w, r.h].iter().all(|v| v.is_finite()) && r.w > 0.0 && r.h > 0.0
}

/// The left or the right half of a rounded rectangle, its outer side rounded and its inner side
/// straight: the fill of one half of a split button. `right` picks the side that is rounded.
pub(crate) fn side(r: Rect, radius: f32, right: bool) -> Path {
    let radius = if radius.is_finite() {
        radius.min(r.w / 2.0).min(r.h / 2.0).max(0.0)
    } else {
        0.0
    };
    let k = radius * 0.552_284_8;
    let (x0, y0, x1, y1) = (r.x, r.y, r.right(), r.bottom());
    if right {
        Path::new()
            .move_to(x0, y0)
            .line_to(x1 - radius, y0)
            .curve_to(
                (x1 - radius + k, y0),
                (x1, y0 + radius - k),
                x1,
                y0 + radius,
            )
            .line_to(x1, y1 - radius)
            .curve_to(
                (x1, y1 - radius + k),
                (x1 - radius + k, y1),
                x1 - radius,
                y1,
            )
            .line_to(x0, y1)
            .close()
    } else {
        Path::new()
            .move_to(x1, y0)
            .line_to(x0 + radius, y0)
            .curve_to(
                (x0 + radius - k, y0),
                (x0, y0 + radius - k),
                x0,
                y0 + radius,
            )
            .line_to(x0, y1 - radius)
            .curve_to(
                (x0, y1 - radius + k),
                (x0 + radius - k, y1),
                x0 + radius,
                y1,
            )
            .line_to(x1, y1)
            .close()
    }
}

#[cfg(test)]
#[path = "button_tests.rs"]
mod tests;
