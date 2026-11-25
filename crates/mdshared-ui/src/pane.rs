//! Side panes: the title row of a pane, a selectable row, and the splitter on its inner edge.
//! Pure drawing; the caller records the hits and keeps the scroll state.

use crate::gfx::{Face, Painter, Rect, Rgb};
use crate::theme::Palette;

/// Row pitch and height in a pane's list.
pub const ROW: f32 = 30.0;
pub const ROW_H: f32 = 28.0;
/// The pane title row.
pub const HEAD: f32 = 40.0;

/// Fills the pane's title row (the top `HEAD` px of `r` are the title; the fill covers all of
/// `r`, as the pane background) and writes `title` in it.
pub fn draw_head(p: &mut Painter, pal: &Palette, r: Rect, title: &str) {
    p.fill(r, pal.pane);
    p.text(
        Face::Semibold,
        11.0,
        r.x + 16.0,
        r.y + HEAD / 2.0,
        title,
        pal.muted,
        r.w - 60.0,
    );
}

/// One switch of a pane's head: a face the pane can show instead of the one it shows.
#[derive(Clone, Copy)]
pub struct Switch<'a> {
    pub icon: &'a str,
    pub label: &'a str,
    /// The face shown now.
    pub lit: bool,
    /// The fill behind the switch (hovered, pressed), from the caller's own pointer state.
    pub fill: Option<Rgb>,
}

/// Height of a switch's hit target, and the width of one that is an icon alone.
pub const SWITCH_H: f32 = 28.0;
pub const SWITCH_ICON_W: f32 = 32.0;
const SWITCH_PAD: f32 = 8.0;
const SWITCH_LEFT: f32 = 8.0;
const SWITCH_TEXT: f32 = 12.0;
/// Room kept at a head's right edge for the button that belongs to the face shown (28 wide, 10
/// from the edge). It is kept whatever the face, so the switches do not move between faces.
pub const HEAD_ACTION: f32 = 38.0;

/// The hit rectangles of a head's switches for the widths of their labels: text with a gap round
/// each when `labels`, icons alone otherwise. The first starts at the left of `r`.
pub fn switch_rects(label_w: &[f32], r: Rect, labels: bool) -> Vec<Rect> {
    let mut x = r.x + SWITCH_LEFT;
    label_w
        .iter()
        .map(|w| {
            let w = if labels {
                w + 2.0 * SWITCH_PAD
            } else {
                SWITCH_ICON_W
            };
            let b = Rect::new(x, r.y + (HEAD - SWITCH_H) / 2.0, w, SWITCH_H);
            x += w;
            b
        })
        .collect()
}

/// Whether labelled switches fit a head `width` wide beside the button of the face shown;
/// icons alone do below that.
pub fn switch_labels_fit(label_w: &[f32], width: f32) -> bool {
    let need: f32 = label_w.iter().map(|w| w + 2.0 * SWITCH_PAD).sum();
    SWITCH_LEFT + need + HEAD_ACTION <= width
}

/// Fills the pane as [`draw_head`] does and puts `switches` in the title row in place of its
/// word: their names while they fit beside the button of the face shown, their icons alone
/// when they do not. The lit one is in the text colour with an accent line under it. Returns
/// each switch's rectangle, in order, for the caller to record as hits; the button on the right
/// is the caller's.
pub fn draw_head_switches(
    p: &mut Painter,
    pal: &Palette,
    r: Rect,
    switches: &[Switch],
) -> Vec<Rect> {
    p.fill(r, pal.pane);
    let widths: Vec<f32> = switches
        .iter()
        .map(|s| p.measure(Face::Semibold, SWITCH_TEXT, s.label))
        .collect();
    let labels = switch_labels_fit(&widths, r.w);
    let rects = switch_rects(&widths, r, labels);
    for (s, b) in switches.iter().zip(&rects) {
        if let Some(fill) = s.fill {
            p.rbox(*b, 5.0, fill);
        }
        let color = if s.lit { pal.text } else { pal.muted };
        if labels {
            p.text(
                Face::Semibold,
                SWITCH_TEXT,
                b.x + SWITCH_PAD,
                b.cy(),
                s.label,
                color,
                b.w - 2.0 * SWITCH_PAD,
            );
        } else {
            p.icon(s.icon, 15.0, b.cx(), b.cy(), color);
        }
        if s.lit {
            let (x, w) = if labels {
                (b.x + SWITCH_PAD, b.w - 2.0 * SWITCH_PAD)
            } else {
                (b.x + 6.0, b.w - 12.0)
            };
            p.fill(Rect::new(x, r.y + HEAD - 4.0, w, 2.0), pal.accent);
        }
    }
    rects
}

/// The background of a list row: the selected look (with the accent bar), the hover or pressed
/// fill, or nothing.
pub fn draw_row_fill(
    p: &mut Painter,
    pal: &Palette,
    row: Rect,
    hovered: bool,
    pressed: bool,
    selected: bool,
) {
    if selected {
        p.rbox(row, 5.0, pal.on);
        p.rbox(
            Rect::new(row.x, row.y + 7.0, 3.0, row.h - 14.0),
            1.5,
            pal.accent,
        );
    } else if hovered {
        p.rbox(row, 5.0, if pressed { pal.press } else { pal.hover });
    }
}

/// The splitter on a pane's inner edge, and the hairline that marks it. `right_edge` is true for
/// a pane on the left (whose inner edge is its right side). Returns the rectangle to hit-test
/// for the drag.
pub fn draw_splitter(p: &mut Painter, pal: &Palette, r: Rect, right_edge: bool, hot: bool) -> Rect {
    let px = p.px();
    let x = if right_edge { r.right() - px } else { r.x };
    if hot {
        p.fill(Rect::new(x - px, r.y, 3.0 * px, r.h), pal.accent);
    } else {
        p.fill(Rect::new(x, r.y, px, r.h), pal.line);
    }
    Rect::new(x - 3.0, r.y, 6.0, r.h)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gfx::Gfx;
    use crate::theme::LIGHT;
    use mdshared_text::Bitmap;

    const NAMES: [&str; 3] = ["Files", "Functions", "Notes"];

    fn switches(lit: usize) -> Vec<Switch<'static>> {
        NAMES
            .iter()
            .enumerate()
            .map(|(i, label)| Switch {
                icon: crate::glyph::FOLDER,
                label,
                lit: i == lit,
                fill: None,
            })
            .collect()
    }

    fn widths() -> Vec<f32> {
        let mut bmp = Bitmap::new(8, 8);
        let mut gfx = Gfx::new();
        let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
        NAMES
            .iter()
            .map(|n| p.measure(Face::Semibold, SWITCH_TEXT, n))
            .collect()
    }

    fn head(width: f32, lit: usize) -> (Bitmap, Vec<Rect>) {
        let mut bmp = Bitmap::new(width as u32, HEAD as u32);
        let mut gfx = Gfx::new();
        let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
        let r = Rect::new(0.0, 0.0, width, HEAD);
        let rects = draw_head_switches(&mut p, &LIGHT, r, &switches(lit));
        (bmp, rects)
    }

    fn pixel(bmp: &Bitmap, x: usize, y: usize) -> [u8; 4] {
        let at = (y * bmp.width as usize + x) * 4;
        bmp.pixels[at..at + 4].try_into().unwrap()
    }

    #[test]
    fn names_show_while_they_fit_beside_the_button_and_icons_alone_below() {
        let w = widths();
        let full: f32 = w.iter().map(|w| w + 16.0).sum::<f32>() + SWITCH_LEFT + HEAD_ACTION;
        // The plan's "about 210" was measured with Segoe UI, Windows's interface font. Other
        // systems draw these names in their own fonts (DejaVu Sans on a bare Linux needs 231
        // px), so there the number only has to fall between the two widths drawn below, which
        // keeps both sides of the rule seen on every system.
        #[cfg(windows)]
        assert!(
            (200.0..=215.0).contains(&full),
            "the names need {full} px: the plan's 'about 210'"
        );
        assert!(
            160.0 < full && full < 300.0,
            "the names need {full} px: between the narrow head and the wide one"
        );
        assert!(switch_labels_fit(&w, 300.0));
        assert!(switch_labels_fit(&w, full));
        assert!(!switch_labels_fit(&w, full - 1.0));
        assert!(!switch_labels_fit(&w, 160.0));
        let (_, wide) = head(300.0, 0);
        let (_, narrow) = head(160.0, 0);
        assert!(wide.iter().all(|b| b.w > SWITCH_ICON_W), "{wide:?}");
        assert!(narrow.iter().all(|b| b.w == SWITCH_ICON_W), "{narrow:?}");
    }

    #[test]
    fn the_switches_stay_clear_of_the_button_and_of_each_other_at_every_width() {
        for width in 160..=520 {
            let width = width as f32;
            let (_, rects) = head(width, 1);
            assert_eq!(rects.len(), 3);
            for pair in rects.windows(2) {
                assert!(pair[0].right() <= pair[1].x, "{width}: {pair:?}");
            }
            let last = rects.last().unwrap();
            assert!(last.right() <= width - HEAD_ACTION, "{width}: {last:?}");
            assert!(rects[0].x >= 0.0);
            assert!(
                rects
                    .iter()
                    .all(|b| b.h == SWITCH_H && b.y >= 0.0 && b.bottom() <= HEAD)
            );
        }
    }

    #[test]
    fn only_the_lit_switch_has_the_accent_line_under_it() {
        for width in [160.0, 300.0] {
            for lit in 0..3 {
                let (bmp, rects) = head(width, lit);
                let mut reference = Bitmap::new(4, 4);
                let mut gfx = Gfx::new();
                Painter::new(&mut reference, &mut gfx, 1.0)
                    .fill(Rect::new(0.0, 0.0, 4.0, 4.0), LIGHT.accent);
                let accent = pixel(&reference, 1, 1);
                let y = (HEAD - 3.5) as usize;
                for (i, b) in rects.iter().enumerate() {
                    let marked = pixel(&bmp, b.cx() as usize, y) == accent;
                    assert_eq!(marked, i == lit, "width {width}, lit {lit}, switch {i}");
                }
            }
        }
    }

    #[test]
    fn the_plain_head_draws_a_title_and_no_switch_line() {
        let mut bmp = Bitmap::new(300, HEAD as u32);
        let mut gfx = Gfx::new();
        let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
        draw_head(&mut p, &LIGHT, Rect::new(0.0, 0.0, 300.0, HEAD), "FILES");
        let mut reference = Bitmap::new(4, 4);
        Painter::new(&mut reference, &mut gfx, 1.0)
            .fill(Rect::new(0.0, 0.0, 4.0, 4.0), LIGHT.accent);
        let accent = pixel(&reference, 1, 1);
        let y = (HEAD - 3.5) as usize;
        assert!((0..300).all(|x| pixel(&bmp, x, y) != accent));
        // The word is where it was: muted ink at the left, not a switch's box.
        let ink = (16..60).any(|x| (8..32).any(|y| pixel(&bmp, x, y) != pixel(&bmp, 299, 0)));
        assert!(ink);
    }
}
