//! A segmented switch: two or three labelled options in one rounded frame, one of them chosen
//! (or none, for a question not answered yet), and a selector bar of any number of options, like
//! a tab strip. Geometry and drawing only; the caller records the hits and handles the pointer,
//! the keyboard and the value.
//!
//! The switch keeps to a few options: it is read left to right and every option must stay
//! readable, so a longer list belongs in a menu or a dropdown. The selector bar is the same look
//! without that limit, for a place a Windows tab strip or a radio group would be: [`bar_width`],
//! [`selector_rects`], [`selector_at`], [`selector_key`], [`draw_selector`] and
//! [`draw_selector_animated`]. Both kinds lay their options out with [`segment_rects`], so the
//! options tile the rectangle in the same way.
//!
//! Deliberately not here: the chosen value itself, the pointer and key events, the focus ring and
//! the accessibility nodes. The caller keeps the selection, passes the keys through
//! [`selector_key`], and asks [`selector_rects`] for one rectangle per option, which is what a
//! tab item will need.

use crate::anim::{Animator, AsId, Id, fluent};
use crate::gfx::{Face, Painter, Rect, mix};
use crate::keys::KeyName;
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
    bar_width(p, labels)
}

/// The width a selector bar of these labels wants: every label plus [`SEGMENT_PAD`] on each side,
/// the same measurement [`switch_width`] makes. A bar laid out at this width gives each option its
/// label's room exactly; a wider one gives the last option the rest.
pub fn bar_width(p: &mut Painter, labels: &[&str]) -> f32 {
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

/// The rectangle of each option of a selector bar that fills `r`, left to right, measured from
/// `labels`. It is [`segment_rects`] over the labels' own widths, so the options tile `r` with no
/// gap and no overlap and the last one takes whatever is left. A rectangle narrower than
/// [`bar_width`] squeezes the options from the right: each keeps its label's width until the
/// space runs out, and the options after that get nothing, with their labels elided in the
/// smaller ones that remain. That is how a switch behaves too, and it is the caller's choice of
/// rectangle, not this module's.
///
/// No labels give no rectangles. This is the list an accessibility layer reports a tab item for.
pub fn selector_rects(p: &mut Painter, r: Rect, labels: &[&str]) -> Vec<Rect> {
    let widths: Vec<f32> = labels
        .iter()
        .map(|l| p.measure(Face::Semibold, SEGMENT_TEXT, l) + 2.0 * SEGMENT_PAD)
        .collect();
    segment_rects(r, &widths)
}

/// Which option of a selector bar a point falls in, or `None` outside it. The options tile the
/// bar, so a point on the edge between two is in the later one, as [`segment_at`] has it. A point
/// that is not a number is in none.
pub fn selector_at(rects: &[Rect], x: f32, y: f32) -> Option<usize> {
    segment_at(rects, x, y)
}

/// What a key does to the selection of a selector bar of `count` options: the new selected option,
/// or `None` when the key is not one of the four or would move past an end. Left and Right move by
/// one, Home and End go to the first and the last. It does not wrap, as a Windows tab strip does
/// not: Left on the first and Right on the last change nothing.
///
/// With nothing selected (`selected` is `None`) any of the four picks an option: Left, Right and
/// Home the first, End the last. A selection past the end, from a caller's damaged state, is
/// repaired by any of the four: Right and End give the last, Left the one before it (the last when
/// there is only one), Home the first. No options, or any other key, give `None`. It reads no
/// clock, no painter and no state of its own.
pub fn selector_key(selected: Option<usize>, count: usize, key: &KeyName) -> Option<usize> {
    if count == 0 {
        return None;
    }
    let last = count - 1;
    match key {
        KeyName::Left => match selected {
            Some(0) => None,
            Some(k) if k > last => Some(last.saturating_sub(1)),
            Some(k) => Some(k - 1),
            None => Some(0),
        },
        KeyName::Right => match selected {
            Some(k) if k < last => Some(k + 1),
            Some(k) if k > last => Some(last),
            Some(_) => None,
            None => Some(0),
        },
        KeyName::Home => match selected {
            Some(0) => None,
            _ => Some(0),
        },
        KeyName::End => match selected {
            Some(k) if k == last => None,
            _ => Some(last),
        },
        _ => None,
    }
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
    draw_bar(p, pal, r, labels, selected, hovered)
}

/// [`draw_segments`] for any number of options: the same frame, the same chosen and hovered
/// fills and the same labels, with each option as wide as its label plus [`SEGMENT_PAD`] on each
/// side. The options tile `r` left to right exactly as [`segment_rects`] lays them out, so a
/// rectangle narrower than [`bar_width`] squeezes the options from the right and elides their
/// labels, and a wider one gives the last option the rest. With two or three labels the pixels
/// are exactly those of [`draw_segments`].
///
/// Returns one rectangle per option, in order, for the caller to record as hits and for an
/// accessibility layer to report. With no labels it draws nothing and returns no rectangles.
pub fn draw_selector(
    p: &mut Painter,
    pal: &Palette,
    r: Rect,
    labels: &[&str],
    selected: Option<usize>,
    hovered: Option<usize>,
) -> Vec<Rect> {
    draw_bar(p, pal, r, labels, selected, hovered)
}

/// [`draw_segments_animated`] for any number of options: the accent fill slides from the option
/// that was chosen to the one that is, the hover fill fades in and out, and a choice made or
/// taken away fades. `id` names this bar in `anim`; two bars need two names.
///
/// At rest it draws exactly the pixels of [`draw_selector`], and so it does on every frame when
/// the animator does not animate (reduced motion). The fill moves relative to the bar, so a bar
/// that moves with its window (a resize, a scroll) does not set it sliding. The rectangles
/// returned are the options' own: the hits do not move with the fill. The frame that first shows
/// a bar shows it at rest.
///
/// As with every animated drawing, the caller ends its frame with [`Animator::end_frame`] and
/// draws again when that asks for it.
#[allow(clippy::too_many_arguments)] // the arguments of `draw_selector`, the animator and a name
pub fn draw_selector_animated(
    p: &mut Painter,
    pal: &Palette,
    anim: &mut Animator,
    id: impl AsId,
    r: Rect,
    labels: &[&str],
    selected: Option<usize>,
    hovered: Option<usize>,
) -> Vec<Rect> {
    draw_bar_animated(p, pal, anim, Id::new(id), r, labels, selected, hovered)
}

/// The pixels both plain drawings make: a switch and a selector bar look the same.
fn draw_bar(
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

/// [`draw_segments`] with motion: the accent fill slides from the segment that was
/// chosen to the one that is, the hover fill fades in and out, and a choice made or taken away
/// fades. `id` names this switch in `anim`; two switches need two names.
///
/// At rest it draws exactly the pixels of [`draw_segments`], and so it does on every frame when
/// the animator does not animate (reduced motion). On the way, a label takes the on-accent
/// colour by how much of its segment the fill covers, and the semibold face once that is half.
/// The fill moves relative to the switch, so a switch that moves with its window (a resize, a
/// scroll) does not set it sliding. The rectangles returned are the segments' own, as always:
/// the hits do not move with the fill.
///
/// The frame that first shows a switch shows it at rest. As with every animated drawing, the
/// caller ends its frame with [`Animator::end_frame`] and draws again when that asks for it.
#[allow(clippy::too_many_arguments)] // the arguments of `draw_segments`, the animator and a name
pub fn draw_segments_animated(
    p: &mut Painter,
    pal: &Palette,
    anim: &mut Animator,
    id: impl AsId,
    r: Rect,
    labels: &[&str],
    selected: Option<usize>,
    hovered: Option<usize>,
) -> Vec<Rect> {
    draw_bar_animated(p, pal, anim, Id::new(id), r, labels, selected, hovered)
}

/// The moving pixels both animated drawings make.
#[allow(clippy::too_many_arguments)] // the arguments of the bar, the animator and its name
fn draw_bar_animated(
    p: &mut Painter,
    pal: &Palette,
    anim: &mut Animator,
    id: Id,
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
    for (k, seg) in rects.iter().enumerate() {
        let on = hovered == Some(k) && selected != Some(k);
        let hover = anim.toggle(id.with(("hover", k)), on, fluent::FADE);
        if hover > 0.0 {
            p.rbox_a(seg.inset(1.0), 5.0, pal.hover, hover);
        }
    }
    // The fill: how much of it is there, and where, as an offset from the switch's corner.
    let chosen = selected.filter(|&k| k < rects.len());
    let there = anim.toggle(id.with("chosen"), chosen.is_some(), fluent::FADE);
    let place = id.with("fill");
    let fill = match chosen {
        Some(k) => {
            let at = rects[k].inset(1.0);
            let rest = Rect::new(at.x - r.x, at.y - r.y, at.w, at.h);
            Some(anim.value(place, rest, fluent::MOVE))
        }
        // A choice taken away fades out where it was. If its fill was on its way from one
        // segment to another, it goes on there while it fades: `keep` moves it to this frame.
        None if there > 0.0 => {
            anim.keep(place);
            anim.get::<Rect>(place)
        }
        None => None,
    }
    .map(|rel| Rect::new(r.x + rel.x, r.y + rel.y, rel.w, rel.h));
    if let Some(fill) = fill {
        p.rbox_a(fill, 5.0, pal.accent, there);
    }
    for (label, seg) in labels.iter().zip(&rects) {
        // How much of this segment the fill covers, and how solid the fill is.
        let covered = fill.map_or(0.0, |f| {
            let inner = seg.inset(1.0);
            let overlap = f.right().min(inner.right()) - f.x.max(inner.x);
            if inner.w > 0.0 {
                (overlap / inner.w).clamp(0.0, 1.0) * there
            } else {
                0.0
            }
        });
        let face = if covered >= 0.5 {
            Face::Semibold
        } else {
            Face::Ui
        };
        let colour = mix(pal.text, pal.on_accent, covered);
        p.text_center(face, SEGMENT_TEXT, *seg, label, colour);
    }
    rects
}

#[cfg(test)]
#[path = "segments_tests.rs"]
mod selector_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anim::{Clock, ManualClock, Policy};
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

    /// A switch of `THREE` drawn through the animator, frame after frame, on one clock.
    struct Moving {
        clock: ManualClock,
        anim: Animator,
        gfx: Gfx,
        r: Rect,
    }

    impl Moving {
        fn new() -> Moving {
            let mut probe = Bitmap::new(8, 8);
            let mut gfx = Gfx::new();
            let w = switch_width(&mut painter(&mut probe, &mut gfx), &THREE);
            Moving {
                clock: ManualClock::new(),
                anim: Animator::new(),
                gfx,
                r: Rect::new(10.0, 16.0, w, SEGMENT_H),
            }
        }

        /// One frame `ms` later: the bitmap, the segments, and whether another frame is asked.
        fn frame(
            &mut self,
            ms: u64,
            selected: Option<usize>,
            hovered: Option<usize>,
        ) -> (Bitmap, Vec<Rect>, bool) {
            self.clock.advance_ms(ms);
            self.anim.begin_frame(self.clock.now());
            let mut bmp = Bitmap::new(300, 60);
            let rects = {
                let mut p = painter(&mut bmp, &mut self.gfx);
                p.fill(PAGE, LIGHT.pane);
                draw_segments_animated(
                    &mut p,
                    &LIGHT,
                    &mut self.anim,
                    "switch",
                    self.r,
                    &THREE,
                    selected,
                    hovered,
                )
            };
            (bmp, rects, self.anim.end_frame().is_some())
        }

        /// The same state as the plain switch draws it.
        fn plain(&mut self, selected: Option<usize>, hovered: Option<usize>) -> Bitmap {
            let mut bmp = Bitmap::new(300, 60);
            let mut p = painter(&mut bmp, &mut self.gfx);
            p.fill(PAGE, LIGHT.pane);
            draw_segments(&mut p, &LIGHT, self.r, &THREE, selected, hovered);
            bmp
        }

        /// Frames of 10 ms until no further one is asked for; the last bitmap.
        fn settle(&mut self, selected: Option<usize>, hovered: Option<usize>) -> Bitmap {
            for _ in 0..200 {
                let (bmp, _, more) = self.frame(10, selected, hovered);
                if !more {
                    return bmp;
                }
            }
            panic!("the switch never comes to rest");
        }
    }

    /// The first and last column of row `y` that hold exactly `c`.
    fn run_of(bmp: &Bitmap, y: f32, c: [u8; 3]) -> Option<(usize, usize)> {
        let xs: Vec<usize> = (0..bmp.width as usize)
            .filter(|&x| at(bmp, x as f32, y) == c)
            .collect();
        Some((*xs.first()?, *xs.last()?))
    }

    #[test]
    fn the_animated_switch_at_rest_is_the_plain_switch_pixel_for_pixel() {
        let mut m = Moving::new();
        for (selected, hovered) in [
            (Some(0), None),
            (Some(2), Some(1)),
            (None, Some(0)),
            (Some(1), Some(1)),
            (None, None),
        ] {
            // First seen, or come to rest after a change: no difference.
            let rest = m.settle(selected, hovered);
            assert!(
                rest.pixels == m.plain(selected, hovered).pixels,
                "{selected:?} {hovered:?} at rest"
            );
        }
        // The very first frame of a switch is at rest, and asks for no other.
        let mut fresh = Moving::new();
        let (bmp, rects, more) = fresh.frame(0, Some(1), None);
        assert!(!more);
        assert!(bmp.pixels == fresh.plain(Some(1), None).pixels);
        assert_eq!(rects.len(), 3);
        // No labels: nothing drawn, nothing remembered.
        let mut bmp = Bitmap::new(300, 60);
        let mut p = painter(&mut bmp, &mut fresh.gfx);
        fresh.anim.begin_frame(fresh.clock.now());
        let none = draw_segments_animated(
            &mut p,
            &LIGHT,
            &mut fresh.anim,
            "empty",
            fresh.r,
            &[],
            None,
            None,
        );
        assert!(none.is_empty());
        fresh.anim.end_frame();
        assert!(fresh.anim.is_empty());
    }

    #[test]
    fn the_fill_slides_from_the_old_choice_to_the_new_one() {
        let mut m = Moving::new();
        let (_, rects, _) = m.frame(0, Some(0), None);
        let y = rects[0].y + 3.0;
        let start = run_of(&m.plain(Some(0), None), y, LIGHT.accent).unwrap();
        let end = run_of(&m.plain(Some(2), None), y, LIGHT.accent).unwrap();
        // The frame that brings the new choice still draws the old place, and asks for more.
        let (bmp, hits, more) = m.frame(0, Some(2), None);
        assert!(more);
        assert_eq!(run_of(&bmp, y, LIGHT.accent), Some(start));
        assert_eq!(hits, rects, "the hits are the segments, not the fill");
        // On the way the left edge only moves right, and never past its end.
        let mut last = start.0;
        let mut between = 0;
        for _ in 0..40 {
            let (bmp, _, more) = m.frame(10, Some(2), None);
            let (left, right) = run_of(&bmp, y, LIGHT.accent).unwrap();
            assert!(
                left >= last && left <= end.0 && right <= end.1,
                "{left}..{right}"
            );
            between += usize::from(left > start.0 && left < end.0);
            last = left;
            if !more {
                break;
            }
        }
        assert!(between >= 5, "only {between} frames between the two places");
        assert_eq!(last, end.0);
        assert!(m.settle(Some(2), None).pixels == m.plain(Some(2), None).pixels);
        // 167 ms: by a quarter of a second it is over.
        m.frame(0, Some(0), None);
        let mut frames = 0;
        while m.frame(10, Some(0), None).2 {
            frames += 1;
        }
        assert!((14..=18).contains(&frames), "{frames} frames of 10 ms");
    }

    #[test]
    fn a_hover_and_a_choice_fade_and_the_switch_moving_does_not_slide_the_fill() {
        let mut m = Moving::new();
        let (_, rects, _) = m.frame(0, None, None);
        let (x, y) = (rects[1].x + 3.0, rects[1].cy());
        // Hover: on the way the colour is between the popup and the hover fill.
        m.frame(0, None, Some(1));
        let (bmp, _, more) = m.frame(40, None, Some(1));
        let mid = at(&bmp, x, y);
        assert!(more && mid != LIGHT.popup && mid != LIGHT.hover, "{mid:?}");
        assert_eq!(at(&m.settle(None, Some(1)), x, y), LIGHT.hover);
        // A choice made where there was none fades in at its place; taken away, it fades out
        // there.
        m.settle(None, None);
        m.frame(0, Some(1), None);
        let (bmp, _, _) = m.frame(40, Some(1), None);
        let mid = at(&bmp, x, y);
        assert!(mid != LIGHT.popup && mid != LIGHT.accent, "{mid:?}");
        assert_eq!(at(&m.settle(Some(1), None), x, y), LIGHT.accent);
        m.frame(0, None, None);
        let (bmp, _, more) = m.frame(40, None, None);
        let mid = at(&bmp, x, y);
        assert!(more && mid != LIGHT.popup && mid != LIGHT.accent, "{mid:?}");
        assert!(m.settle(None, None).pixels == m.plain(None, None).pixels);

        // The whole switch moved (its window was resized): the fill is with it at once.
        m.settle(Some(0), None);
        m.r.x += 40.0;
        let (bmp, _, more) = m.frame(10, Some(0), None);
        assert!(!more);
        assert!(bmp.pixels == m.plain(Some(0), None).pixels);
    }

    #[test]
    fn a_choice_taken_away_while_its_fill_slides_goes_on_sliding_as_it_fades() {
        let mut m = Moving::new();
        m.frame(0, Some(0), None);
        m.frame(0, Some(2), None);
        m.frame(40, Some(2), None);
        // The fill's place, read as the widget keeps it: under the switch's own id.
        let fill = Id::new("switch").with("fill");
        let x = |m: &Moving| m.anim.get::<Rect>(fill).map(|r| r.x);
        let a = x(&m).unwrap();
        let end = {
            let (_, rects, _) = Moving::new().frame(0, Some(2), None);
            rects[2].inset(1.0).x - m.r.x
        };
        assert!(a > 1.0 && a < end, "{a} on the way to {end}");
        // The choice is taken away half way: the fill is no longer asked for, only kept, and
        // it must not stop where it was while it fades.
        let (_, _, more) = m.frame(10, None, None);
        let b = x(&m).unwrap();
        assert!(more && b > a && b < end, "{b} after {a}");
        let (_, _, more) = m.frame(10, None, None);
        let c = x(&m).unwrap();
        assert!(more && c > b && c <= end, "{c} after {b}");
        // It comes to rest as the plain switch with no choice, and the fill is forgotten.
        assert!(m.settle(None, None).pixels == m.plain(None, None).pixels);
        assert_eq!(x(&m), None);
        assert!(!m.anim.animating());
    }

    #[test]
    fn with_reduced_motion_the_animated_switch_is_the_plain_one_on_every_frame() {
        let mut m = Moving::new();
        m.anim.set_policy(Policy::Never);
        for (selected, hovered) in [(Some(0), None), (Some(2), Some(0)), (None, Some(1))] {
            let (bmp, _, more) = m.frame(10, selected, hovered);
            assert!(!more);
            assert!(bmp.pixels == m.plain(selected, hovered).pixels);
        }
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
