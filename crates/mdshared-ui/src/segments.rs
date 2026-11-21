//! A segmented switch: two or three labelled options in one rounded frame, one of them chosen
//! (or none, for a question not answered yet). Geometry and drawing only; the caller records the
//! hits and handles the keyboard and the value. It refuses more than a few options: a switch is
//! read left to right and every option must stay readable, so a longer list belongs in a menu or
//! a dropdown.

use crate::gfx::{Face, Painter, Rect};
use crate::theme::Palette;

/// The height a switch wants, and what each side of a label keeps inside its own segment.
pub const SEGMENT_H: f32 = 28.0;
pub const SEGMENT_PAD: f32 = 14.0;
/// The size of a segment's label.
const SEGMENT_TEXT: f32 = 13.0;

/// The width a switch of these labels wants: every label plus [`SEGMENT_PAD`] on each side. A
/// caller that makes its rectangle this wide gets segments that tile it exactly; a narrower one
/// squeezes the last segments, which is the caller's choice and not this module's.
pub fn switch_width(p: &mut Painter, labels: &[&str]) -> f32 {
    labels
        .iter()
        .map(|l| p.measure(Face::Semibold, SEGMENT_TEXT, l) + 2.0 * SEGMENT_PAD)
        .sum()
}

/// The rectangle of each segment of a switch that fills `r`, from `widths`, left to right. Each
/// segment is as wide as its width and the last one takes whatever is left, so the rectangles
/// tile `r` with no gap and no overlap; a width past the right edge leaves the segments after it
/// empty. It lays out and measures nothing, so it can be asked again without a painter.
pub fn segment_rects(r: Rect, widths: &[f32]) -> Vec<Rect> {
    let mut out = Vec::with_capacity(widths.len());
    let mut x = r.x;
    for (k, w) in widths.iter().enumerate() {
        let right = if k + 1 == widths.len() {
            r.right()
        } else {
            (x + w).min(r.right())
        };
        out.push(Rect::new(x, r.y, (right - x).max(0.0), r.h));
        x = right;
    }
    out
}

/// Which segment a point falls in, or `None` outside the switch. The rectangles tile the switch,
/// so a point inside is in exactly one.
pub fn segment_at(rects: &[Rect], x: f32, y: f32) -> Option<usize> {
    rects.iter().position(|s| s.contains(x, y))
}

/// Draws a switch of `labels` in `r`, of which `selected` is the chosen one and `hovered` the one
/// under the pointer (from the caller's own state). The chosen segment is filled with the accent
/// and its label is the on-accent colour in the semibold face; the others carry the text colour,
/// with the hovered one filled quiet. Returns the segment rectangles, in order, for the caller to
/// record as hits; it records none itself. With no labels it draws nothing at all and returns no
/// rectangles: a switch without options is a caller's mistake, not a frame to show.
pub fn draw_segments(
    p: &mut Painter,
    pal: &Palette,
    r: Rect,
    labels: &[&str],
    selected: Option<usize>,
    hovered: Option<usize>,
) -> Vec<Rect> {
    if labels.is_empty() {
        return Vec::new();
    }
    let widths: Vec<f32> = labels
        .iter()
        .map(|l| p.measure(Face::Semibold, SEGMENT_TEXT, l) + 2.0 * SEGMENT_PAD)
        .collect();
    let rects = segment_rects(r, &widths);
    p.frame(r, 6.0, pal.popup, pal.popup_line);
    for (k, (label, seg)) in labels.iter().zip(&rects).enumerate() {
        if selected == Some(k) {
            p.rbox(seg.inset(1.0), 5.0, pal.accent);
            p.text_center(Face::Semibold, SEGMENT_TEXT, *seg, label, pal.on_accent);
        } else {
            if hovered == Some(k) {
                p.rbox(seg.inset(1.0), 5.0, pal.hover);
            }
            p.text_center(Face::Ui, SEGMENT_TEXT, *seg, label, pal.text);
        }
    }
    rects
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gfx::Gfx;
    use crate::theme::{DARK, LIGHT};
    use mdshared_text::Bitmap;

    const TWO: [&str; 2] = ["Changes", "Preview"];
    const THREE: [&str; 3] = ["Off", "Some", "All"];
    const PAGE: Rect = Rect::new(0.0, 0.0, 300.0, 60.0);

    fn painter<'a>(bmp: &'a mut Bitmap, gfx: &'a mut Gfx) -> Painter<'a> {
        Painter::new(bmp, gfx, 1.0)
    }

    /// The measured width of each label's segment.
    fn widths(p: &mut Painter, labels: &[&str]) -> Vec<f32> {
        labels
            .iter()
            .map(|l| p.measure(Face::Semibold, SEGMENT_TEXT, l) + 2.0 * SEGMENT_PAD)
            .collect()
    }

    fn at(bmp: &Bitmap, x: f32, y: f32) -> [u8; 3] {
        let o = (y as usize * bmp.width as usize + x as usize) * 4;
        [bmp.pixels[o + 2], bmp.pixels[o + 1], bmp.pixels[o]]
    }

    #[test]
    fn the_segments_tile_the_switch_and_a_point_lands_in_one() {
        let mut bmp = Bitmap::new(8, 8);
        let mut gfx = Gfx::new();
        let mut p = painter(&mut bmp, &mut gfx);
        for labels in [&TWO[..], &THREE[..]] {
            let w = switch_width(&mut p, labels);
            let r = Rect::new(10.0, 20.0, w, SEGMENT_H);
            let rects = segment_rects(r, &widths(&mut p, labels));
            assert_eq!(rects.len(), labels.len());
            assert_eq!(rects[0].x, r.x);
            for pair in rects.windows(2) {
                assert_eq!(
                    pair[0].right(),
                    pair[1].x,
                    "{pair:?} overlap or leave a gap"
                );
            }
            assert_eq!(rects.last().unwrap().right(), r.right());
            for s in &rects {
                assert_eq!((s.y, s.h), (r.y, r.h));
                assert!(s.w > 0.0);
            }
            // Every segment, at both edges and in the middle, and nothing outside.
            for (k, s) in rects.iter().enumerate() {
                assert_eq!(segment_at(&rects, s.x, s.cy()), Some(k));
                assert_eq!(segment_at(&rects, s.cx(), s.cy()), Some(k));
                assert_eq!(segment_at(&rects, s.right() - 0.01, s.cy()), Some(k));
            }
            assert_eq!(segment_at(&rects, r.x - 0.01, r.cy()), None);
            assert_eq!(segment_at(&rects, r.right(), r.cy()), None);
            assert_eq!(segment_at(&rects, r.cx(), r.y - 0.01), None);
            assert_eq!(segment_at(&rects, r.cx(), r.bottom()), None);
        }
        // A rectangle narrower than the labels squeezes from the right and never overflows; a
        // wider one gives the rest to the last segment.
        let rects = segment_rects(Rect::new(0.0, 0.0, 40.0, SEGMENT_H), &[30.0, 30.0, 30.0]);
        assert_eq!((rects[0].w, rects[1].w, rects[2].w), (30.0, 10.0, 0.0));
        let rects = segment_rects(Rect::new(0.0, 0.0, 80.0, SEGMENT_H), &[30.0, 30.0]);
        assert_eq!((rects[0].w, rects[1].w), (30.0, 50.0));
        // A switch with no options draws nothing, not an empty frame.
        let r = Rect::new(10.0, 20.0, 100.0, SEGMENT_H);
        assert!(draw_segments(&mut p, &LIGHT, r, &[], None, None).is_empty());
    }

    #[test]
    fn only_the_chosen_segment_is_filled_with_the_accent() {
        for pal in [&LIGHT, &DARK] {
            for selected in [None, Some(0), Some(1)] {
                let mut bmp = Bitmap::new(300, 60);
                let mut gfx = Gfx::new();
                let r = {
                    let mut p = painter(&mut bmp, &mut gfx);
                    p.fill(PAGE, pal.pane);
                    let w = switch_width(&mut p, &TWO);
                    Rect::new(10.0, 16.0, w, SEGMENT_H)
                };
                let rects = {
                    let mut p = painter(&mut bmp, &mut gfx);
                    draw_segments(&mut p, pal, r, &TWO, selected, None)
                };
                for (k, s) in rects.iter().enumerate() {
                    // Just inside the left edge, clear of the centred label.
                    let look = at(&bmp, s.x + 3.0, s.cy());
                    if selected == Some(k) {
                        assert_eq!(look, pal.accent, "segment {k} of {selected:?}");
                    } else {
                        assert_eq!(look, pal.popup, "segment {k} of {selected:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn a_hovered_segment_is_filled_quiet_and_the_chosen_one_keeps_the_accent() {
        let mut bmp = Bitmap::new(300, 60);
        let mut gfx = Gfx::new();
        let r = {
            let mut p = painter(&mut bmp, &mut gfx);
            p.fill(PAGE, LIGHT.pane);
            let w = switch_width(&mut p, &TWO);
            Rect::new(10.0, 16.0, w, SEGMENT_H)
        };
        let rects = {
            let mut p = painter(&mut bmp, &mut gfx);
            draw_segments(&mut p, &LIGHT, r, &TWO, None, Some(1))
        };
        assert_eq!(at(&bmp, rects[1].x + 3.0, rects[1].cy()), LIGHT.hover);
        assert_eq!(at(&bmp, rects[0].x + 3.0, rects[0].cy()), LIGHT.popup);
        // Chosen and hovered at once: the accent wins.
        let mut bmp = Bitmap::new(300, 60);
        let rects = {
            let mut p = painter(&mut bmp, &mut gfx);
            draw_segments(&mut p, &LIGHT, r, &TWO, Some(1), Some(1))
        };
        assert_eq!(at(&bmp, rects[1].x + 3.0, rects[1].cy()), LIGHT.accent);
    }

    /// The "Changes / Preview" switch as the editor's review draws it today (`ai_ui::review`): the
    /// shape this one must keep, so the review can adopt it without changing a pixel.
    fn review_switch(p: &mut Painter, pal: &Palette, right: f32, y: f32, preview: bool) {
        let seg_w = |p: &mut Painter, s: &str| p.measure(Face::Semibold, 13.0, s) + 28.0;
        let (cw, pw) = (seg_w(p, "Changes"), seg_w(p, "Preview"));
        let sw = Rect::new(right - cw - pw, y, cw + pw, 28.0);
        p.frame(sw, 6.0, pal.popup, pal.popup_line);
        for (k, (label, on)) in [("Changes", !preview), ("Preview", preview)]
            .into_iter()
            .enumerate()
        {
            let seg = if k == 0 {
                Rect::new(sw.x, sw.y, cw, sw.h)
            } else {
                Rect::new(sw.x + cw, sw.y, pw, sw.h)
            };
            if on {
                p.rbox(seg.inset(1.0), 5.0, pal.accent);
                p.text_center(Face::Semibold, 13.0, seg, label, pal.on_accent);
            } else {
                p.text_center(Face::Ui, 13.0, seg, label, pal.text);
            }
        }
    }

    #[test]
    fn the_switch_draws_as_the_review_switch_draws_it_today() {
        let mut probe = Bitmap::new(300, 60);
        let mut gfx = Gfx::new();
        let w = {
            let mut p = painter(&mut probe, &mut gfx);
            switch_width(&mut p, &TWO)
        };
        let sw = Rect::new(10.0, 16.0, w, SEGMENT_H);
        for pal in [&LIGHT, &DARK] {
            for preview in [false, true] {
                let mut before = Bitmap::new(300, 60);
                let mut gfx = Gfx::new();
                {
                    let mut p = painter(&mut before, &mut gfx);
                    p.fill(PAGE, pal.pane);
                    review_switch(&mut p, pal, sw.right(), sw.y, preview);
                }
                let mut after = Bitmap::new(300, 60);
                let mut gfx = Gfx::new();
                {
                    let mut p = painter(&mut after, &mut gfx);
                    p.fill(PAGE, pal.pane);
                    draw_segments(&mut p, pal, sw, &TWO, Some(usize::from(preview)), None);
                }
                assert_eq!(
                    before.pixels, after.pixels,
                    "preview {preview}: the shared switch draws elsewhere"
                );
            }
        }
    }
}
