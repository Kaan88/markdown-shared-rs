use super::*;
use crate::anim::{Clock, ManualClock, Policy};
use crate::gfx::Gfx;
use crate::keys::KeyName;
use crate::theme::{DARK, LIGHT};
use mdshared_text::Bitmap;

const ONE: [&str; 1] = ["Overview"];
const TWO: [&str; 2] = ["Changes", "Preview"];
const THREE: [&str; 3] = ["Off", "Some", "All"];
const FOUR: [&str; 4] = ["Details", "History", "Permissions", "Advanced"];
const SEVEN: [&str; 7] = [
    "General", "View", "Search", "Tools", "Export", "Share", "About",
];
const PAGE: Rect = Rect::new(0.0, 0.0, 760.0, 60.0);

fn painter<'a>(bmp: &'a mut Bitmap, gfx: &'a mut Gfx) -> Painter<'a> {
    Painter::new(bmp, gfx, 1.0)
}

fn at(bmp: &Bitmap, x: f32, y: f32) -> [u8; 3] {
    let o = (y as usize * bmp.width as usize + x as usize) * 4;
    [bmp.pixels[o + 2], bmp.pixels[o + 1], bmp.pixels[o]]
}

/// A difference that is only the last bit of a float sum.
fn same(a: f32, b: f32) -> bool {
    (a - b).abs() <= 0.001
}

/// A selector bar laid out at the width its labels want, on the page.
fn bar_rect(p: &mut Painter, labels: &[&str]) -> Rect {
    Rect::new(10.0, 16.0, bar_width(p, labels), SEGMENT_H)
}

/// A page drawn with `f`, and what `f` answered.
fn shot<T>(pal: &Palette, f: impl FnOnce(&mut Painter) -> T) -> (Bitmap, T) {
    let mut bmp = Bitmap::new(PAGE.w as u32, PAGE.h as u32);
    let mut gfx = Gfx::new();
    let out = {
        let mut p = painter(&mut bmp, &mut gfx);
        p.fill(PAGE, pal.pane);
        f(&mut p)
    };
    (bmp, out)
}

#[test]
fn options_of_any_count_tile_the_bar_and_a_point_lands_in_one() {
    let mut bmp = Bitmap::new(8, 8);
    let mut gfx = Gfx::new();
    let mut p = painter(&mut bmp, &mut gfx);
    for labels in [&ONE[..], &TWO[..], &FOUR[..], &SEVEN[..]] {
        let r = bar_rect(&mut p, labels);
        let rects = selector_rects(&mut p, r, labels);
        assert_eq!(rects.len(), labels.len());
        assert_eq!(rects[0].x, r.x, "{labels:?}");
        for (k, pair) in rects.windows(2).enumerate() {
            assert_eq!(
                pair[0].right(),
                pair[1].x,
                "option {} and {} overlap or leave a gap in {labels:?}",
                k,
                k + 1
            );
            // The edge between two options belongs to the later one: no gap, no doubt.
            assert_eq!(selector_at(&rects, pair[1].x, r.cy()), Some(k + 1));
        }
        assert_eq!(rects.last().unwrap().right(), r.right());
        for (k, s) in rects.iter().enumerate() {
            assert_eq!((s.y, s.h), (r.y, r.h));
            assert!(s.w > 0.0, "option {k} of {labels:?} has no width");
            assert_eq!(selector_at(&rects, s.x, s.cy()), Some(k));
            assert_eq!(selector_at(&rects, s.cx(), s.cy()), Some(k));
            assert_eq!(selector_at(&rects, s.right() - 0.01, s.cy()), Some(k));
        }
        assert_eq!(selector_at(&rects, r.x - 0.01, r.cy()), None);
        assert_eq!(selector_at(&rects, r.right(), r.cy()), None);
        assert_eq!(selector_at(&rects, r.cx(), r.y - 0.01), None);
        assert_eq!(selector_at(&rects, r.cx(), r.bottom()), None);
        assert_eq!(selector_at(&rects, f32::NAN, r.cy()), None);
    }
}

#[test]
fn a_narrow_bar_squeezes_from_the_right_and_a_wide_one_gives_the_rest_to_the_last() {
    let mut bmp = Bitmap::new(8, 8);
    let mut gfx = Gfx::new();
    let mut p = painter(&mut bmp, &mut gfx);
    // `selector_rects` is `segment_rects` over the labels' own widths, with no other rule.
    let want: Vec<f32> = THREE
        .iter()
        .map(|l| p.measure(Face::Semibold, SEGMENT_TEXT, l) + 2.0 * SEGMENT_PAD)
        .collect();
    for r in [
        Rect::new(4.0, 9.0, 30.0, SEGMENT_H),
        Rect::new(4.0, 9.0, 100.0, SEGMENT_H),
        Rect::new(4.0, 9.0, 400.0, SEGMENT_H),
    ] {
        assert_eq!(selector_rects(&mut p, r, &THREE), segment_rects(r, &want));
    }
    // Too narrow for two options: the ones after the room left get nothing, and asking for a
    // point there finds none.
    let narrow = Rect::new(0.0, 0.0, want[0] + 5.0, SEGMENT_H);
    let rects = selector_rects(&mut p, narrow, &THREE);
    assert!(same(rects[0].w, want[0]), "{}", rects[0].w);
    assert!(same(rects[1].w, 5.0), "{}", rects[1].w);
    assert!(same(rects[2].w, 0.0), "{}", rects[2].w);
    assert_eq!(selector_at(&rects, rects[2].x, rects[2].cy()), None);
    // Wider than the labels want: the last option takes the rest.
    let wide = Rect::new(0.0, 0.0, want.iter().sum::<f32>() + 50.0, SEGMENT_H);
    let rects = selector_rects(&mut p, wide, &THREE);
    assert!(same(rects[2].w, want[2] + 50.0), "{}", rects[2].w);
    // No labels: no width, no rectangles, and nonsense sizes do not panic.
    assert_eq!(bar_width(&mut p, &[]), 0.0);
    assert!(selector_rects(&mut p, narrow, &[]).is_empty());
    for r in [
        Rect::new(0.0, 0.0, 0.0, 0.0),
        Rect::new(0.0, 0.0, f32::NAN, SEGMENT_H),
        Rect::new(f32::NAN, 0.0, 100.0, f32::INFINITY),
    ] {
        assert_eq!(selector_rects(&mut p, r, &THREE).len(), 3);
        let rects = selector_rects(&mut p, r, &THREE);
        assert_eq!(selector_at(&rects, f32::NAN, f32::NAN), None);
    }
}

#[test]
fn the_keys_move_the_selection_and_do_not_wrap() {
    // Right and Left, one option at a time, and nothing past either end.
    assert_eq!(selector_key(Some(0), 4, &KeyName::Right), Some(1));
    assert_eq!(selector_key(Some(2), 4, &KeyName::Right), Some(3));
    assert_eq!(selector_key(Some(3), 4, &KeyName::Right), None);
    assert_eq!(selector_key(Some(3), 4, &KeyName::Left), Some(2));
    assert_eq!(selector_key(Some(1), 4, &KeyName::Left), Some(0));
    assert_eq!(selector_key(Some(0), 4, &KeyName::Left), None);
    // Home and End go to the ends; at the end they have nothing to do. Both ends at once in a
    // bar of one option.
    assert_eq!(selector_key(Some(2), 4, &KeyName::Home), Some(0));
    assert_eq!(selector_key(Some(2), 4, &KeyName::End), Some(3));
    assert_eq!(selector_key(Some(0), 4, &KeyName::Home), None);
    assert_eq!(selector_key(Some(3), 4, &KeyName::End), None);
    for key in [KeyName::Left, KeyName::Right, KeyName::Home, KeyName::End] {
        assert_eq!(selector_key(Some(0), 1, &key), None, "{key:?}");
        assert_eq!(selector_key(None, 1, &key), Some(0), "{key:?}");
    }
    // Nothing chosen yet: any of the four picks an end.
    assert_eq!(selector_key(None, 4, &KeyName::Left), Some(0));
    assert_eq!(selector_key(None, 4, &KeyName::Home), Some(0));
    assert_eq!(selector_key(None, 4, &KeyName::Right), Some(0));
    assert_eq!(selector_key(None, 4, &KeyName::End), Some(3));
    // A selection past the end, from a damaged caller, is read as the last.
    assert_eq!(selector_key(Some(9), 4, &KeyName::Up), None);
    assert_eq!(selector_key(Some(9), 4, &KeyName::Right), None);
    assert_eq!(selector_key(Some(9), 4, &KeyName::Left), Some(2));
    assert_eq!(selector_key(Some(9), 4, &KeyName::Home), Some(0));
    assert_eq!(selector_key(Some(9), 4, &KeyName::End), Some(3));
    // Any other key, and no options at all, change nothing.
    for key in [KeyName::Up, KeyName::Down, KeyName::Tab, KeyName::Char('x')] {
        assert_eq!(selector_key(Some(1), 4, &key), None, "{key:?}");
    }
    for key in [KeyName::Left, KeyName::Right, KeyName::Home, KeyName::End] {
        assert_eq!(selector_key(Some(0), 0, &key), None, "{key:?}");
        assert_eq!(selector_key(None, 0, &key), None, "{key:?}");
    }
}

#[test]
fn the_selected_pill_is_under_the_selected_option() {
    for pal in [&LIGHT, &DARK] {
        for selected in [None, Some(0), Some(2), Some(3)] {
            let (bmp, rects) = shot(pal, |p| {
                let r = bar_rect(p, &FOUR);
                draw_selector(p, pal, r, &FOUR, selected, Some(1))
            });
            for (k, s) in rects.iter().enumerate() {
                // Just inside the left edge, clear of the centred label.
                let look = at(&bmp, s.x + 3.0, s.cy());
                let want = if selected == Some(k) {
                    pal.accent
                } else if k == 1 {
                    pal.hover
                } else {
                    pal.popup
                };
                assert_eq!(look, want, "option {k} of {selected:?} in {pal:?}");
            }
        }
        // Once drawn, the option the pill is under is the one that reads on the accent.
        let (bmp, rects) = shot(pal, |p| {
            let r = bar_rect(p, &SEVEN);
            draw_selector(p, pal, r, &SEVEN, Some(5), None)
        });
        for (k, s) in rects.iter().enumerate() {
            let inside = at(&bmp, s.x + 2.0, s.cy());
            if k == 5 {
                assert_eq!(inside, pal.accent, "the pill sits under option 5");
            } else {
                assert_ne!(inside, pal.accent, "option {k} is not the pill");
            }
        }
    }
}

#[test]
fn no_options_draw_nothing_and_return_no_rectangles() {
    for pal in [&LIGHT, &DARK] {
        let (blank, _) = shot(pal, |_| ());
        let (bmp, rects) = shot(pal, |p| {
            let r = Rect::new(10.0, 16.0, 100.0, SEGMENT_H);
            draw_selector(p, pal, r, &[], None, Some(0))
        });
        assert!(rects.is_empty());
        assert!(bmp.pixels == blank.pixels);
    }
}

/// The body of `draw_segments` as it was before the selector bar was added, copied whole, so the
/// pixels can be compared with what the module draws now.
fn before_draw_segments(
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

#[test]
fn the_two_and_three_option_switch_draws_exactly_as_it_did_before() {
    for pal in [&LIGHT, &DARK] {
        for labels in [&TWO[..], &THREE[..]] {
            let r = {
                let (_, r) = shot(&LIGHT, |p| bar_rect(p, labels));
                r
            };
            for selected in [None, Some(0), Some(labels.len() - 1)] {
                for hovered in [None, Some(0), Some(1)] {
                    let (before, before_hits) = shot(pal, |p| {
                        before_draw_segments(p, pal, r, labels, selected, hovered)
                    });
                    let (after, after_hits) =
                        shot(pal, |p| draw_segments(p, pal, r, labels, selected, hovered));
                    let (selector, selector_hits) =
                        shot(pal, |p| draw_selector(p, pal, r, labels, selected, hovered));
                    assert!(
                        before.pixels == after.pixels,
                        "draw_segments {labels:?} {selected:?} {hovered:?} in {pal:?}"
                    );
                    assert!(
                        before.pixels == selector.pixels,
                        "draw_selector {labels:?} {selected:?} {hovered:?} in {pal:?}"
                    );
                    assert_eq!(before_hits, after_hits);
                    assert_eq!(before_hits, selector_hits);
                }
            }
        }
    }
}

/// A selector bar drawn through the animator, frame after frame, on one clock.
struct Moving {
    clock: ManualClock,
    anim: Animator,
    gfx: Gfx,
    r: Rect,
    labels: &'static [&'static str],
}

impl Moving {
    fn new(labels: &'static [&'static str]) -> Moving {
        let mut probe = Bitmap::new(8, 8);
        let mut gfx = Gfx::new();
        let r = {
            let mut p = painter(&mut probe, &mut gfx);
            bar_rect(&mut p, labels)
        };
        Moving {
            clock: ManualClock::new(),
            anim: Animator::new(),
            gfx,
            r,
            labels,
        }
    }

    /// One frame `ms` later: the bitmap, the options' rectangles, and whether another frame is
    /// asked for.
    fn frame(
        &mut self,
        ms: u64,
        selected: Option<usize>,
        hovered: Option<usize>,
    ) -> (Bitmap, Vec<Rect>, bool) {
        self.clock.advance_ms(ms);
        self.anim.begin_frame(self.clock.now());
        let mut bmp = Bitmap::new(PAGE.w as u32, PAGE.h as u32);
        let rects = {
            let mut p = painter(&mut bmp, &mut self.gfx);
            p.fill(PAGE, LIGHT.pane);
            draw_selector_animated(
                &mut p,
                &LIGHT,
                &mut self.anim,
                "selector",
                self.r,
                self.labels,
                selected,
                hovered,
            )
        };
        (bmp, rects, self.anim.end_frame().is_some())
    }

    /// The same state as the plain selector draws it.
    fn plain(&mut self, selected: Option<usize>, hovered: Option<usize>) -> Bitmap {
        let mut bmp = Bitmap::new(PAGE.w as u32, PAGE.h as u32);
        let mut p = painter(&mut bmp, &mut self.gfx);
        p.fill(PAGE, LIGHT.pane);
        draw_selector(&mut p, &LIGHT, self.r, self.labels, selected, hovered);
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
        panic!("the selector never comes to rest");
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
fn the_animated_selector_at_rest_is_the_plain_selector_pixel_for_pixel() {
    let mut m = Moving::new(&SEVEN);
    for (selected, hovered) in [
        (Some(0), None),
        (Some(5), Some(1)),
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
    // The very first frame of a bar is at rest, and asks for no other.
    let mut fresh = Moving::new(&FOUR);
    let (bmp, rects, more) = fresh.frame(0, Some(2), None);
    assert!(!more);
    assert!(bmp.pixels == fresh.plain(Some(2), None).pixels);
    assert_eq!(rects.len(), 4);
}

#[test]
fn the_fill_slides_from_the_old_choice_to_the_new_one() {
    let mut m = Moving::new(&FOUR);
    let (_, rects, _) = m.frame(0, Some(0), None);
    let y = rects[0].y + 3.0;
    let start = run_of(&m.plain(Some(0), None), y, LIGHT.accent).unwrap();
    let end = run_of(&m.plain(Some(3), None), y, LIGHT.accent).unwrap();
    // The frame that brings the new choice still draws the old place, and asks for more.
    let (bmp, hits, more) = m.frame(0, Some(3), None);
    assert!(more);
    assert_eq!(run_of(&bmp, y, LIGHT.accent), Some(start));
    assert_eq!(hits, rects, "the hits are the options, not the fill");
    // On the way the left edge only moves right, and never past its end.
    let mut last = start.0;
    let mut between = 0;
    for _ in 0..40 {
        let (bmp, _, more) = m.frame(10, Some(3), None);
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
    assert!(m.settle(Some(3), None).pixels == m.plain(Some(3), None).pixels);
    // The whole bar moved (its window was resized): the fill is with it at once.
    m.r.x += 40.0;
    let (bmp, _, more) = m.frame(10, Some(3), None);
    assert!(!more);
    assert!(bmp.pixels == m.plain(Some(3), None).pixels);
}

#[test]
fn with_reduced_motion_the_animated_selector_is_the_plain_one_on_every_frame() {
    let mut m = Moving::new(&FOUR);
    m.anim.set_policy(Policy::Never);
    for (selected, hovered) in [(Some(0), None), (Some(3), Some(0)), (None, Some(1))] {
        let (bmp, _, more) = m.frame(10, selected, hovered);
        assert!(!more);
        assert!(bmp.pixels == m.plain(selected, hovered).pixels);
    }
}

/// A sheet a person looks at: selector bars of one, two, four and seven options, chosen and
/// hovered in turn, and the seven squeezed into too narrow a rectangle so the elided labels
/// show; light and dark, at two scales.
/// `cargo test -p mdshared-ui selector_sheet -- --ignored`, then open
/// target/specimen/selector.png and selector-150.png.
#[test]
#[ignore]
fn selector_sheet() {
    const W: f32 = 760.0;
    const ROW: f32 = 74.0;
    const ROWS: f32 = 5.0;
    /// The labels, the chosen option, the hovered one, a forced width (0 to measure), and a name.
    type Case = (
        &'static [&'static str],
        Option<usize>,
        Option<usize>,
        f32,
        &'static str,
    );
    for (scale, file) in [(1.0, "selector.png"), (1.5, "selector-150.png")] {
        let mut bmp = Bitmap::new((W * scale) as u32, (2.0 * ROWS * ROW * scale) as u32);
        let mut gfx = Gfx::new();
        let mut p = Painter::new(&mut bmp, &mut gfx, scale);
        for (half, pal) in [&LIGHT, &DARK].into_iter().enumerate() {
            let oy = half as f32 * ROWS * ROW;
            p.fill(Rect::new(0.0, oy, W, ROWS * ROW), pal.pane);
            let rows: [Case; 5] = [
                (&ONE, Some(0), None, 0.0, "one option"),
                (&TWO, Some(1), Some(0), 0.0, "two, the left one hovered"),
                (&FOUR, Some(2), Some(3), 0.0, "four, the last hovered"),
                (&SEVEN, Some(4), None, 0.0, "seven"),
                (&SEVEN, Some(4), Some(5), 330.0, "seven, squeezed to 330 px"),
            ];
            for (row, (labels, selected, hovered, narrow, name)) in rows.into_iter().enumerate() {
                let w = if narrow > 0.0 {
                    narrow
                } else {
                    bar_width(&mut p, labels)
                };
                let r = Rect::new(24.0, oy + row as f32 * ROW + 14.0, w, SEGMENT_H);
                draw_selector(&mut p, pal, r, labels, selected, hovered);
                p.text(
                    Face::Ui,
                    10.0,
                    r.x,
                    r.bottom() + 9.0,
                    name,
                    pal.muted,
                    300.0,
                );
            }
        }
        let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/specimen");
        std::fs::create_dir_all(&out).unwrap();
        bmp.write_png(&out.join(file)).unwrap();
    }
}
