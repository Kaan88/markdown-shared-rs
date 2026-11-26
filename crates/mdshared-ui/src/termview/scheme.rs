//! What the colours a program names look like: the sixteen, the 256 and true colour, the
//! default foreground and background, the cursor and the selection.

use crate::gfx::{Rgb, mix, rgb};
use crate::theme::Palette;
use mdshared_term::{Attrs, Cell, Color};

/// The sixteen colours on a dark page: Windows Terminal's "Campbell", the console's own since
/// 2019 (black, red, green, yellow, blue, magenta, cyan, white, then their bright forms).
pub const DARK_ANSI: [Rgb; 16] = [
    rgb(0x0c0c0c),
    rgb(0xc50f1f),
    rgb(0x13a10e),
    rgb(0xc19c00),
    rgb(0x0037da),
    rgb(0x881798),
    rgb(0x3a96dd),
    rgb(0xcccccc),
    rgb(0x767676),
    rgb(0xe74856),
    rgb(0x16c60c),
    rgb(0xf9f1a5),
    rgb(0x3b78ff),
    rgb(0xb4009e),
    rgb(0x61d6d6),
    rgb(0xf2f2f2),
];

/// The sixteen colours on a light page: Windows Terminal's "One Half Light", whose yellow and
/// cyan can still be read on white (Campbell's bright yellow cannot).
pub const LIGHT_ANSI: [Rgb; 16] = [
    rgb(0x383a42),
    rgb(0xe45649),
    rgb(0x50a14f),
    rgb(0xc18301),
    rgb(0x0184bc),
    rgb(0xa626a4),
    rgb(0x0997b3),
    rgb(0xfafafa),
    rgb(0x4f525d),
    rgb(0xdf6c75),
    rgb(0x98c379),
    rgb(0xe4c07a),
    rgb(0x61afef),
    rgb(0xc577dd),
    rgb(0x56b5c1),
    rgb(0xffffff),
];

/// The steps of each channel of xterm's 6 x 6 x 6 colour cube (colours 16 to 231).
const CUBE: [u8; 6] = [0, 95, 135, 175, 215, 255];

/// Colour `n` of the 256 past the sixteen, as xterm defines them: 16 to 231 the cube (red
/// counts by 36, green by 6, blue by 1), 232 to 255 a ramp of greys from 8 to 238 in steps of
/// 10. `None` for 0 to 15, which belong to a scheme.
pub fn xterm_color(n: u8) -> Option<Rgb> {
    match n {
        0..=15 => None,
        16..=231 => {
            let i = (n - 16) as usize;
            Some([CUBE[i / 36], CUBE[i / 6 % 6], CUBE[i % 6]])
        }
        232..=255 => {
            let g = 8 + 10 * (n - 232);
            Some([g, g, g])
        }
    }
}

/// The colours of a terminal's grid.
///
/// Built from the toolkit's palette with [`Scheme::from_palette`] and changed field by field
/// (the struct is `#[non_exhaustive]`, so a field added later, for named schemes, breaks no
/// application). Every colour drawn comes from here or from the true colour a program names;
/// none is written into the drawing.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Scheme {
    /// The sixteen: 0 to 7 the standard colours, 8 to 15 their bright forms.
    pub ansi: [Rgb; 16],
    /// The colour of text the program gave no colour.
    pub foreground: Rgb,
    /// The background of cells the program gave no colour, and the padding around the grid.
    pub background: Rgb,
    /// The cursor's fill (block), bar, underline or outline.
    pub cursor: Rgb,
    /// The character under a block cursor.
    pub cursor_text: Rgb,
    /// The background of selected cells.
    pub selection: Rgb,
    /// The colour of selected text, or `None` to keep each cell's own.
    pub selection_text: Option<Rgb>,
    /// Bold text in one of the eight standard colours is drawn in its bright form (xterm's
    /// `boldColors`), besides being bold. Off unless set.
    pub bold_is_bright: bool,
}

/// What a cell looks like once its colours and attributes are read: the colour its character
/// and lines are drawn in (`None` when the cell is hidden), and its background.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CellColors {
    /// The colour of the character, its underline and its strike-through; `None` for a hidden
    /// cell, of which only the background is drawn.
    pub glyph: Option<Rgb>,
    /// The colour of the cell's background.
    pub back: Rgb,
}

impl Scheme {
    /// The scheme that goes with `pal`: the page's background and text colour as the defaults,
    /// [`DARK_ANSI`] or [`LIGHT_ANSI`] by `pal.dark`, the text colour for the cursor with the
    /// page's colour under it, and the selection as the toolkit's text selection shows it
    /// (`pal.selection` at 0.35 over the page, as `seltext` and the text field draw it), with
    /// the text keeping its colours.
    pub fn from_palette(pal: &Palette) -> Scheme {
        Scheme {
            ansi: if pal.dark { DARK_ANSI } else { LIGHT_ANSI },
            foreground: pal.text,
            background: pal.page,
            cursor: pal.text,
            cursor_text: pal.page,
            selection: mix(pal.page, pal.selection, 0.35),
            selection_text: None,
            bold_is_bright: false,
        }
    }

    /// Colour `n` of the 256: the scheme's sixteen for 0 to 15, [`xterm_color`] for the rest.
    pub fn indexed(&self, n: u8) -> Rgb {
        xterm_color(n).unwrap_or(self.ansi[n as usize & 15])
    }

    /// The colour `c` names, with `default` for [`Color::Default`].
    pub fn color(&self, c: Color, default: Rgb) -> Rgb {
        match c {
            Color::Default => default,
            Color::Indexed(n) => self.indexed(n),
            Color::Rgb(r, g, b) => [r, g, b],
        }
    }

    /// The colours `cell` is drawn in: its foreground (the bright form for bold in one of the
    /// eight when [`bold_is_bright`](Self::bold_is_bright)), its background, the two changed
    /// places for inverse, the glyph colour half way to the background for dim, and no glyph
    /// colour for hidden. Selection and cursor are not in it; the drawing lays those over.
    pub fn cell_colors(&self, cell: &Cell) -> CellColors {
        let fg = match cell.fg {
            Color::Indexed(n)
                if n < 8 && self.bold_is_bright && cell.attrs.contains(Attrs::BOLD) =>
            {
                Color::Indexed(n + 8)
            }
            c => c,
        };
        let (mut fg, mut bg) = (
            self.color(fg, self.foreground),
            self.color(cell.bg, self.background),
        );
        if cell.attrs.contains(Attrs::INVERSE) {
            std::mem::swap(&mut fg, &mut bg);
        }
        if cell.attrs.contains(Attrs::DIM) {
            fg = mix(fg, bg, 0.5);
        }
        CellColors {
            glyph: (!cell.attrs.contains(Attrs::HIDDEN)).then_some(fg),
            back: bg,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{DARK, LIGHT};

    #[test]
    fn the_cube_and_the_greys_are_xterms() {
        // The plain way: xterm's 256colres.pl, a step of 40 from 95 for each level above 0.
        let level = |v: u32| if v == 0 { 0 } else { (55 + v * 40) as u8 };
        for n in 16u32..232 {
            let i = n - 16;
            let want = [level(i / 36), level(i / 6 % 6), level(i % 6)];
            assert_eq!(xterm_color(n as u8), Some(want), "{n}");
        }
        for n in 232u32..256 {
            let g = (8 + (n - 232) * 10) as u8;
            assert_eq!(xterm_color(n as u8), Some([g, g, g]));
        }
        for n in 0..16 {
            assert_eq!(xterm_color(n), None);
        }
        assert_eq!(xterm_color(196), Some([255, 0, 0]));
        assert_eq!(xterm_color(255), Some([238, 238, 238]));
    }

    #[test]
    fn the_scheme_follows_the_palette_in_both_themes() {
        for (pal, ansi) in [(DARK, DARK_ANSI), (LIGHT, LIGHT_ANSI)] {
            let s = Scheme::from_palette(&pal);
            assert_eq!(s.ansi, ansi);
            assert_eq!((s.foreground, s.background), (pal.text, pal.page));
            assert_eq!((s.cursor, s.cursor_text), (pal.text, pal.page));
            assert_eq!(s.selection, mix(pal.page, pal.selection, 0.35));
            assert_eq!(s.indexed(1), ansi[1]);
            assert_eq!(s.indexed(15), ansi[15]);
            assert_eq!(s.indexed(21), [0, 0, 255]);
            assert_eq!(s.color(Color::Default, [1, 2, 3]), [1, 2, 3]);
            assert_eq!(s.color(Color::Rgb(9, 8, 7), [1, 2, 3]), [9, 8, 7]);
        }
    }

    #[test]
    fn attributes_change_the_colours_a_cell_is_drawn_in() {
        let s = Scheme::from_palette(&DARK);
        let cell = |fg, bg, attrs| Cell {
            ch: 'x',
            fg,
            bg,
            attrs,
            width: 1,
        };
        let plain = s.cell_colors(&cell(Color::Default, Color::Default, Attrs::NONE));
        assert_eq!(
            plain,
            CellColors {
                glyph: Some(s.foreground),
                back: s.background
            }
        );

        let red = s.cell_colors(&cell(Color::Indexed(1), Color::Indexed(4), Attrs::NONE));
        assert_eq!(
            red,
            CellColors {
                glyph: Some(s.ansi[1]),
                back: s.ansi[4]
            }
        );

        let inverse = s.cell_colors(&cell(Color::Indexed(1), Color::Default, Attrs::INVERSE));
        assert_eq!(
            inverse,
            CellColors {
                glyph: Some(s.background),
                back: s.ansi[1]
            }
        );

        let dim = s.cell_colors(&cell(
            Color::Rgb(200, 100, 0),
            Color::Rgb(0, 0, 0),
            Attrs::DIM,
        ));
        assert_eq!(dim.glyph, Some([100, 50, 0]));

        let hidden = s.cell_colors(&cell(Color::Indexed(2), Color::Indexed(3), Attrs::HIDDEN));
        assert_eq!(
            hidden,
            CellColors {
                glyph: None,
                back: s.ansi[3]
            }
        );

        let bold = cell(Color::Indexed(1), Color::Default, Attrs::BOLD);
        assert_eq!(s.cell_colors(&bold).glyph, Some(s.ansi[1]));
        let bright = Scheme {
            bold_is_bright: true,
            ..s
        };
        assert_eq!(bright.cell_colors(&bold).glyph, Some(s.ansi[9]));
        // Only the eight standard colours brighten.
        let cube = cell(Color::Indexed(100), Color::Default, Attrs::BOLD);
        assert_eq!(bright.cell_colors(&cube).glyph, Some(s.indexed(100)));
    }
}
