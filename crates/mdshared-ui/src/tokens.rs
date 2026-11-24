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
//! Deliberately not here: the system's own high-contrast colours (Windows's chosen scheme:
//! the palettes here are the toolkit's); widgets reading sizes from the tokens (they keep
//! their constants, which [`Sizes::DEFAULT`] names, so no application's look moves);
//! fonts; and watching the system (the application asks again on winit's `ThemeChanged` or
//! when the window gains the focus, as for [`crate::platform::system_is_dark`]).

use crate::cover::contrast;
use crate::gfx::{Rgb, mix, rgb};
use crate::theme::{DARK, LIGHT, Palette};

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
    /// gives one (a high-contrast look keeps its own accent). Each call asks the system; on
    /// Linux that is the settings portal, up to half a second for each question when it does
    /// not answer.
    pub fn from_system(follow_accent: bool) -> Tokens {
        let dark = crate::platform::system_is_dark();
        if crate::platform::system_high_contrast() {
            return Tokens::high_contrast(dark);
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
