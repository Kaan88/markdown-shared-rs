//! The look as named tokens: the [`Palette`] every `draw_*` already takes, with an accent of
//! the application's or the system's choosing and high-contrast variants, and the sizes the
//! widgets are drawn at, named in one place.
//!
//! It is opt-in. An application that never names it draws exactly as before: [`Tokens::new`]
//! is [`LIGHT`] or [`DARK`] and [`Sizes::DEFAULT`] holds the widgets' own constants. One that
//! opts in builds its [`Tokens`] once (and again when the system's setting changes), gives
//! `tokens.palette` to the widgets as it gave a palette before, and reads `tokens.sizes` for its
//! own layout: rows, headers, text, corners.
//!
//! An accent is made legible before it is used ([`accented`]): moved darker on a light page or
//! lighter on a dark one until text in it reads at 4.5:1, and the colours that follow from it
//! (the text on it, the selected row, the text selection) made from it. The high-contrast
//! palettes ([`HIGH_CONTRAST_LIGHT`], [`HIGH_CONTRAST_DARK`]) put text, quiet text and the accent
//! at 7:1 or better and the lines where they can be seen.
//!
//! Under a Windows contrast theme (Aquatic, Desert, Dusk, Night sky, or one the person made)
//! the palette is made from that theme's own colours instead ([`SystemColors`],
//! [`Tokens::from_system_colors`]), as WinUI does, so the application looks like the rest of
//! the desktop. Where the system gives no colours (Linux's portal says only whether contrast
//! is asked for) the toolkit's palettes stand in. A backdrop (Mica, Acrylic, the frosted
//! chrome) is not shown under high contrast: [`allowed_backdrop`] is the rule.
//!
//! Deliberately not here: widgets reading sizes from the tokens (they keep their constants,
//! which [`Sizes::DEFAULT`] names, so no application's look moves); fonts; a link colour and a
//! button text colour in the palette (it has neither field; [`SystemColors::hot_light`] and
//! [`SystemColors::button_text`] are there for an application that draws its own); and
//! watching the system. The application asks again on winit's `ThemeChanged` or when the
//! window gains the focus, as for [`crate::platform::system_is_dark`]; a switch of the
//! contrast theme reaches no winit event, so on Windows [`crate::platform::watch_contrast`]
//! wakes the application for it.

use crate::cover::{contrast, luminance};
use crate::gfx::{Rgb, mix, rgb};
use crate::theme::{DARK, LIGHT, Palette};
use mdshared_app::backdrop::Backdrop;

/// The sizes the widgets are drawn at, in logical px, by name.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Sizes {
    /// A button's, a field's or a card's corners.
    pub radius: f32,
    /// A popup's, a menu's or a dialog's corners.
    pub radius_popup: f32,
    /// A row of a pane's list ([`crate::pane::ROW_H`]).
    pub row: f32,
    /// The pitch of a pane's rows, the row and its gap ([`crate::pane::ROW`]).
    pub row_pitch: f32,
    /// A table's header ([`crate::table::HEADER_H`]).
    pub header: f32,
    /// A table cell's padding at each side ([`crate::table::CELL_PAD`]).
    pub cell_pad: f32,
    /// Body text: list rows, fields, buttons.
    pub text: f32,
    /// Hints, captions, secondary columns.
    pub text_small: f32,
    /// Menu items.
    pub text_menu: f32,
    /// Headings within a pane.
    pub text_heading: f32,
    /// A slider's thumb ([`crate::slider::THUMB`]).
    pub thumb: f32,
    /// A slider's track ([`crate::slider::TRACK`]).
    pub track: f32,
    /// A check box ([`crate::check::TICK`]).
    pub tick: f32,
    /// An icon's grid ([`crate::glyph::GRID`]).
    pub icon: f32,
    /// A splitter's grip ([`crate::splitter::GRIP`]).
    pub grip: f32,
}

impl Sizes {
    /// The sizes the widgets use now.
    pub const DEFAULT: Sizes = Sizes {
        radius: 5.0,
        radius_popup: 8.0,
        row: crate::pane::ROW_H,
        row_pitch: crate::pane::ROW,
        header: crate::table::HEADER_H,
        cell_pad: crate::table::CELL_PAD,
        text: 13.0,
        text_small: 12.0,
        text_menu: 13.5,
        text_heading: 14.0,
        thumb: crate::slider::THUMB,
        track: crate::slider::TRACK,
        tick: crate::check::TICK,
        icon: crate::glyph::GRID,
        grip: crate::splitter::GRIP,
    };

    /// Every size times `factor`, for an application that offers larger text and controls.
    /// A factor that is not a positive number changes nothing.
    pub fn scaled(self, factor: f32) -> Sizes {
        if !(factor.is_finite() && factor > 0.0) {
            return self;
        }
        let s = |v: f32| v * factor;
        Sizes {
            radius: s(self.radius),
            radius_popup: s(self.radius_popup),
            row: s(self.row),
            row_pitch: s(self.row_pitch),
            header: s(self.header),
            cell_pad: s(self.cell_pad),
            text: s(self.text),
            text_small: s(self.text_small),
            text_menu: s(self.text_menu),
            text_heading: s(self.text_heading),
            thumb: s(self.thumb),
            track: s(self.track),
            tick: s(self.tick),
            icon: s(self.icon),
            grip: s(self.grip),
        }
    }
}

impl Default for Sizes {
    fn default() -> Sizes {
        Sizes::DEFAULT
    }
}

/// A light palette for high contrast: black on white, quiet text and the accent at 7:1 or
/// better, lines dark enough to see, and the selections light enough for the text on them.
pub const HIGH_CONTRAST_LIGHT: Palette = Palette {
    dark: false,
    page: rgb(0xffffff),
    title: rgb(0xffffff),
    bar: rgb(0xffffff),
    pane: rgb(0xffffff),
    text: rgb(0x000000),
    muted: rgb(0x262626),
    faint: rgb(0x4d4d4d),
    hover: rgb(0xd9d9d9),
    press: rgb(0xbfbfbf),
    on: rgb(0xc8dcff),
    line: rgb(0x595959),
    accent: rgb(0x0037a6),
    on_accent: rgb(0xffffff),
    field: rgb(0xffffff),
    popup: rgb(0xffffff),
    popup_line: rgb(0x000000),
    folder: rgb(0x7a5200),
    thumb: rgb(0x4d4d4d),
    find_mark: rgb(0xffe600),
    find_current: rgb(0xff8a1f),
    selection: rgb(0xb3d1ff),
    danger: rgb(0xa80000),
    warn: rgb(0x6b3d00),
    warn_bg: rgb(0xfff0b3),
};

/// A dark palette for high contrast: white on black, quiet text and the accent at 7:1 or
/// better, lines light enough to see, and the selections dark enough for the text on them.
pub const HIGH_CONTRAST_DARK: Palette = Palette {
    dark: true,
    page: rgb(0x000000),
    title: rgb(0x000000),
    bar: rgb(0x000000),
    pane: rgb(0x000000),
    text: rgb(0xffffff),
    muted: rgb(0xe0e0e0),
    faint: rgb(0xb3b3b3),
    hover: rgb(0x2e2e2e),
    press: rgb(0x474747),
    on: rgb(0x1f3a66),
    line: rgb(0x9e9e9e),
    accent: rgb(0x75e9ff),
    on_accent: rgb(0x000000),
    field: rgb(0x000000),
    popup: rgb(0x0a0a0a),
    popup_line: rgb(0xffffff),
    folder: rgb(0xffd166),
    thumb: rgb(0xb3b3b3),
    find_mark: rgb(0xffe600),
    find_current: rgb(0xff8a1f),
    selection: rgb(0x1f4f8f),
    danger: rgb(0xff6b5e),
    warn: rgb(0xffd166),
    warn_bg: rgb(0x3d2e00),
};

/// The contrast text in the accent must reach against the page: WCAG's AA for body text.
pub const ACCENT_CONTRAST: f32 = 4.5;

/// `colour` moved towards black (on a light `ground`) or white (on a dark one) by the least
/// that brings it to `ratio` against `ground`; as it is when it reaches it already. Black or
/// white when even they do not.
pub fn legible(colour: Rgb, ground: Rgb, ratio: f32) -> Rgb {
    if contrast(colour, ground) >= ratio {
        return colour;
    }
    let toward = if contrast([0, 0, 0], ground) >= contrast([255, 255, 255], ground) {
        [0, 0, 0]
    } else {
        [255, 255, 255]
    };
    // The contrast grows with the move: halve the interval to the least move that reaches.
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    for _ in 0..16 {
        let mid = (lo + hi) / 2.0;
        if contrast(mix(colour, toward, mid), ground) >= ratio {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    mix(colour, toward, hi)
}

/// `colour` moved towards black or white, whichever is the further from the first of `read`,
/// by the least that brings every colour of `read` to `ratio` against it; when no move brings
/// them all there, the one that gives the worst-read of them the most contrast. The answer is
/// `colour` when it already reads, and `colour` when `read` is empty.
///
/// A made fill is read by more than one colour (text, a hint, a marked letter), where
/// [`legible`] follows only one of them.
fn legible_fill(colour: Rgb, read: &[Rgb], ratio: f32) -> Rgb {
    if read.is_empty() {
        return colour;
    }
    let weakest = |c: Rgb| {
        read.iter()
            .map(|&r| contrast(r, c))
            .fold(f32::INFINITY, f32::min)
    };
    if weakest(colour) >= ratio {
        return colour;
    }
    let toward = if contrast([0, 0, 0], read[0]) >= contrast([255, 255, 255], read[0]) {
        [0, 0, 0]
    } else {
        [255, 255, 255]
    };
    // The contrast of a reading colour grows towards one end and falls towards the other, so
    // walk the way in steps, then halve the step that reached for the least move.
    const STEPS: usize = 32;
    let mut best = (weakest(colour), 0.0f32);
    for step in 1..=STEPS {
        let mut t = step as f32 / STEPS as f32;
        let w = weakest(mix(colour, toward, t));
        if w >= ratio {
            let mut lo = (step - 1) as f32 / STEPS as f32;
            for _ in 0..16 {
                let mid = (lo + t) / 2.0;
                if weakest(mix(colour, toward, mid)) >= ratio {
                    t = mid;
                } else {
                    lo = mid;
                }
            }
            return mix(colour, toward, t);
        }
        if w > best.0 {
            best = (w, t);
        }
    }
    mix(colour, toward, best.1)
}

/// `base` with `accent` in place of its own, made legible against the page
/// ([`ACCENT_CONTRAST`]), and the colours that follow from it made anew: the text on an
/// accent fill (black or white, whichever reads better), the selected row's tint and the text
/// selection, which the text over it still reads on at 3:1. Everything else is `base`'s. The
/// palette's own accent gives `base` unchanged.
pub fn accented(base: &Palette, accent: Rgb) -> Palette {
    if accent == base.accent {
        return *base;
    }
    let accent = legible(accent, base.page, ACCENT_CONTRAST);
    let on_accent = if contrast(accent, [0, 0, 0]) >= contrast(accent, [255, 255, 255]) {
        [0, 0, 0]
    } else {
        [255, 255, 255]
    };
    let (tint, select) = if base.dark {
        (0.28, 0.55)
    } else {
        (0.16, 0.75)
    };
    Palette {
        accent,
        on_accent,
        on: mix(base.pane, accent, tint),
        selection: legible(mix(base.page, accent, select), base.text, 3.0),
        ..*base
    }
}

/// The colours of the system's contrast theme, one for each of the system colours a
/// contrast theme sets (Windows's `GetSysColor`, named after its `COLOR_` indices). Each pair
/// is chosen by the theme to be read together: window text on the window, highlight text on
/// the highlight, button text on the button face. [`crate::platform::system_contrast_colors`]
/// reads them; [`Tokens::from_system_colors`] makes a palette of them.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct SystemColors {
    /// `COLOR_WINDOW`: behind content.
    pub window: Rgb,
    /// `COLOR_WINDOWTEXT`: text on the window.
    pub window_text: Rgb,
    /// `COLOR_HIGHLIGHT`: a selected item's fill.
    pub highlight: Rgb,
    /// `COLOR_HIGHLIGHTTEXT`: text on the highlight.
    pub highlight_text: Rgb,
    /// `COLOR_HOTLIGHT`: hyperlinks.
    pub hot_light: Rgb,
    /// `COLOR_GRAYTEXT`: text that cannot be used (disabled).
    pub gray_text: Rgb,
    /// `COLOR_BTNFACE`: a button's fill.
    pub button_face: Rgb,
    /// `COLOR_BTNTEXT`: text on a button.
    pub button_text: Rgb,
}

/// The contrast a fill the toolkit makes up (hover, press, the selected row) keeps against
/// the text drawn on it under a system contrast theme: WCAG's AA for body text.
pub const SYSTEM_FILL_CONTRAST: f32 = 4.5;

/// The contrast a line needs against the window to be drawn in the theme's grey text rather
/// than its window text: WCAG's 3:1 for the parts of a control.
pub const SYSTEM_LINE_CONTRAST: f32 = 3.0;

/// The backdrop to ask for when the setting is `want`: none under high contrast, where the
/// system shows flat colours and the theme's colours must not be mixed with a blurred desktop;
/// `want` otherwise. Give its answer to [`crate::frost::Frost::sync`] (or
/// [`crate::platform::set_backdrop`]) in place of the setting, with `high_contrast` from
/// [`crate::platform::system_high_contrast`] read when the tokens are made. Then a switch to
/// high contrast while the window has the keyboard turns the backdrop off at the next frame,
/// and on Linux, where the blur does not ask about contrast, it is off too.
pub fn allowed_backdrop(want: Backdrop, high_contrast: bool) -> Backdrop {
    if high_contrast { Backdrop::Off } else { want }
}

/// A selected row's or item's fill and the colour of its text, as `(fill, text)`.
///
/// Normally `on` with `text`. Under high contrast (`high_contrast` from
/// [`crate::platform::system_high_contrast`] or the host's `Cx::high_contrast`) it is the
/// palette's `accent` with `on_accent`, which for the system's theme
/// ([`Tokens::from_system_colors`]) are its Highlight and HighlightText, the pair Windows draws a
/// selected item in. The palette's `on` cannot do that there: it has to keep the window text
/// readable, so it stays close to the window (about 1.2:1 in Desert, 1.9:1 in Night sky) and a
/// selected row hardly shows. A row drawn with this pair draws its secondary text (a hint, a
/// size) in the same `text` colour, since `faint` and `accent` do not read on Highlight.
pub fn selected(pal: &Palette, high_contrast: bool) -> (Rgb, Rgb) {
    if high_contrast || *pal == HIGH_CONTRAST_LIGHT || *pal == HIGH_CONTRAST_DARK {
        (pal.accent, pal.on_accent)
    } else {
        (pal.on, pal.text)
    }
}

/// The look an application draws with: a palette and the sizes.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Tokens {
    pub palette: Palette,
    pub sizes: Sizes,
}

impl Tokens {
    /// The toolkit's own look: [`LIGHT`] or [`DARK`] with [`Sizes::DEFAULT`], exactly what the
    /// widgets draw without tokens.
    pub fn new(dark: bool) -> Tokens {
        Tokens {
            palette: if dark { DARK } else { LIGHT },
            sizes: Sizes::DEFAULT,
        }
    }

    /// The high-contrast look: [`HIGH_CONTRAST_LIGHT`] or [`HIGH_CONTRAST_DARK`].
    pub fn high_contrast(dark: bool) -> Tokens {
        Tokens {
            palette: if dark {
                HIGH_CONTRAST_DARK
            } else {
                HIGH_CONTRAST_LIGHT
            },
            sizes: Sizes::DEFAULT,
        }
    }

    /// The look of the system's contrast theme `colors`, mapped onto the palette as WinUI maps
    /// them, with [`Sizes::DEFAULT`]:
    ///
    /// * `page`, `pane`, `field` and `popup` are the window;
    /// * `title` and `bar`, the rows that hold the window's buttons, are the button face,
    ///   unless the window text does not read on it at [`SYSTEM_FILL_CONTRAST`]; then the
    ///   window (every label of the toolkit is drawn in `text`, and the button text has no
    ///   field of its own);
    /// * `text`, `muted` and `folder` are the window text. `muted` is text that can still be
    ///   used (a shortcut beside a menu item, a hint), and WinUI draws such secondary text in
    ///   the window text under high contrast;
    /// * `faint`, the toolkit's text for what cannot be used, is the grey text;
    /// * `accent` is the highlight and `on_accent` the highlight text, the theme's own pair;
    /// * `line` and `thumb` are the grey text where it reads on the window at
    ///   [`SYSTEM_LINE_CONTRAST`], else the window text; `popup_line` is the window text;
    /// * `hover`, `press`, `on` (the selected row) and `selection` (selected text) are made:
    ///   the window moved towards the window text (hover, press) or the highlight (on,
    ///   selection), then kept at [`SYSTEM_FILL_CONTRAST`] against every colour drawn on them
    ///   ([`legible`], [`legible_fill`]): `text` on all four, and `faint` and `accent` too on
    ///   `hover` and `on`, where a hint and a matched letter are drawn. The toolkit draws a
    ///   selected row's text and selected text in `text`, not in a text of their own, so the
    ///   highlight itself, which only the highlight text reads on, cannot be their fill;
    /// * the find marks, `danger`, `warn` and `warn_bg`, which no system colour names, are
    ///   those of [`HIGH_CONTRAST_DARK`] or [`HIGH_CONTRAST_LIGHT`].
    ///
    /// `dark` is whether the window is darker than its text, whatever the application's own
    /// choice of dark or light: Desert is a light theme even under a dark desktop.
    ///
    /// The hyperlink colour and the button text have no field ([`SystemColors::hot_light`],
    /// [`SystemColors::button_text`]). The theme's own colours are used as they are: a theme
    /// the person made with colours that do not read is theirs to choose.
    pub fn from_system_colors(colors: &SystemColors) -> Tokens {
        let c = colors;
        let text = c.window_text;
        let dark = luminance(c.window) < luminance(text);
        let base = if dark {
            HIGH_CONTRAST_DARK
        } else {
            HIGH_CONTRAST_LIGHT
        };
        let fill =
            |towards: Rgb, t: f32| legible(mix(c.window, towards, t), text, SYSTEM_FILL_CONTRAST);
        // `hover` and `on` carry more than `text`: a hovered row's hint, and a selected row's
        // hint and matched letters, in `faint` and `accent`.
        let read = [text, c.gray_text, c.highlight];
        let fill_read = |towards: Rgb, t: f32| {
            legible_fill(mix(c.window, towards, t), &read, SYSTEM_FILL_CONTRAST)
        };
        let chrome = if contrast(text, c.button_face) >= SYSTEM_FILL_CONTRAST {
            c.button_face
        } else {
            c.window
        };
        let line = if contrast(c.gray_text, c.window) >= SYSTEM_LINE_CONTRAST {
            c.gray_text
        } else {
            text
        };
        Tokens {
            palette: Palette {
                dark,
                page: c.window,
                title: chrome,
                bar: chrome,
                pane: c.window,
                text,
                muted: text,
                faint: c.gray_text,
                hover: fill_read(text, 0.15),
                press: fill(text, 0.25),
                on: fill_read(c.highlight, 0.35),
                selection: fill(c.highlight, 0.6),
                line,
                accent: c.highlight,
                on_accent: c.highlight_text,
                field: c.window,
                popup: c.window,
                popup_line: text,
                folder: text,
                thumb: line,
                ..base
            },
            sizes: Sizes::DEFAULT,
        }
    }

    /// The high-contrast look: the system's contrast theme when `colors` gives one
    /// ([`Tokens::from_system_colors`]), else the toolkit's own ([`Tokens::high_contrast`],
    /// dark or light by `dark`), as on Linux, whose portal gives no colours.
    pub fn high_contrast_with(colors: Option<&SystemColors>, dark: bool) -> Tokens {
        match colors {
            Some(colors) => Tokens::from_system_colors(colors),
            None => Tokens::high_contrast(dark),
        }
    }

    /// The same with `accent` as the accent ([`accented`]).
    pub fn with_accent(self, accent: Rgb) -> Tokens {
        Tokens {
            palette: accented(&self.palette, accent),
            ..self
        }
    }

    /// The same with the sizes times `factor` ([`Sizes::scaled`]).
    pub fn scaled(self, factor: f32) -> Tokens {
        Tokens {
            sizes: self.sizes.scaled(factor),
            ..self
        }
    }

    /// The look the system asks for: dark or light ([`crate::platform::system_is_dark`]), high
    /// contrast when it is on, and the desktop's accent when `follow_accent` and the system
    /// gives one (a high-contrast look keeps its own accent). High contrast is the system's
    /// contrast theme where the system gives its colours
    /// ([`crate::platform::system_contrast_colors`], Windows) and the toolkit's high-contrast
    /// palettes elsewhere ([`Tokens::high_contrast_with`]). Each call asks the system; on
    /// Linux that is the settings portal, up to half a second for each question when it does
    /// not answer.
    pub fn from_system(follow_accent: bool) -> Tokens {
        let dark = crate::platform::system_is_dark();
        if crate::platform::system_high_contrast() {
            let colors = crate::platform::system_contrast_colors();
            return Tokens::high_contrast_with(colors.as_ref(), dark);
        }
        let tokens = Tokens::new(dark);
        match crate::platform::system_accent() {
            Some(accent) if follow_accent => tokens.with_accent(accent),
            _ => tokens,
        }
    }
}

#[cfg(test)]
#[path = "tokens_tests.rs"]
mod tests;
