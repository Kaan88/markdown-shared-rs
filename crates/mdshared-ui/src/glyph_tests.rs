use super::*;
use crate::anim::{Clock, ManualClock, Policy};
use crate::gfx::Gfx;
use crate::theme::{DARK, LIGHT};
use mdshared_text::Bitmap;
use std::time::Duration;

const SIZE: f32 = 12.0;

/// A bitmap filled with `ground`, with `f` drawn on it at `scale`.
fn drawn(scale: f32, ground: Rgb, f: impl FnOnce(&mut Painter)) -> Bitmap {
    let side = (32.0 * scale).ceil() as u32;
    let mut bmp = Bitmap::new(side, side);
    for px in bmp.pixels.chunks_mut(4) {
        px.copy_from_slice(&[ground[2], ground[1], ground[0], 255]);
    }
    let mut gfx = Gfx::new();
    f(&mut Painter::new(&mut bmp, &mut gfx, scale));
    bmp
}

/// The plain chevrons as the applications draw them today: `glyph::RIGHT` for a closed row,
/// `glyph::DOWN` for an open one, with `Painter::icon`.
fn plain(scale: f32, ground: Rgb, ink: Rgb, open: bool) -> Bitmap {
    let chevron = if open { DOWN } else { RIGHT };
    drawn(scale, ground, |p| p.icon(chevron, SIZE, 16.0, 15.5, ink))
}

fn disclosure(scale: f32, ground: Rgb, ink: Rgb, open: f32) -> Bitmap {
    drawn(scale, ground, |p| {
        draw_disclosure(p, 16.0, 15.5, SIZE, open, ink)
    })
}

#[test]
fn at_rest_the_disclosure_is_the_plain_right_or_down_chevron() {
    for scale in [1.0, 1.5] {
        for pal in [&LIGHT, &DARK] {
            let (ground, ink) = (pal.page, pal.muted);
            let right = plain(scale, ground, ink, false);
            let down = plain(scale, ground, ink, true);
            for open in [0.0, -0.0, -1.0, f32::NEG_INFINITY, f32::NAN] {
                assert!(
                    disclosure(scale, ground, ink, open).pixels == right.pixels,
                    "open {open} at scale {scale} is not the right chevron"
                );
            }
            for open in [1.0, 1.5, f32::INFINITY] {
                assert!(
                    disclosure(scale, ground, ink, open).pixels == down.pixels,
                    "open {open} at scale {scale} is not the down chevron"
                );
            }
        }
    }
}

#[test]
fn on_the_way_the_disclosure_is_the_right_chevron_turned_by_a_part_of_a_quarter() {
    for scale in [1.0, 1.5] {
        let (ground, ink) = (LIGHT.page, LIGHT.muted);
        let right = plain(scale, ground, ink, false);
        let down = plain(scale, ground, ink, true);
        for open in [0.01, 0.1, 0.3, 0.5, 0.7, 0.9, 0.99] {
            let bmp = disclosure(scale, ground, ink, open);
            assert!(
                bmp.pixels != right.pixels && bmp.pixels != down.pixels,
                "open {open} at scale {scale} is a resting chevron"
            );
            let turned = drawn(scale, ground, |p| {
                p.icon_rotated(RIGHT, SIZE, 16.0, 15.5, open * 90.0, ink)
            });
            assert!(
                bmp.pixels == turned.pixels,
                "open {open} at scale {scale} is not RIGHT turned {} degrees",
                open * 90.0
            );
        }
    }
}

/// Runs frames 16 ms apart, the disclosure asked for as `expanded` in each, and answers how
/// far it had turned and whether the animator asked for another frame.
fn frames(anim: &mut Animator, clock: &ManualClock, wants: &[bool]) -> Vec<(f32, bool)> {
    wants
        .iter()
        .map(|&expanded| {
            clock.advance(Duration::from_millis(16));
            anim.begin_frame(clock.now());
            let t = disclosure_turn(anim, "row", expanded);
            (t, anim.end_frame().is_some())
        })
        .collect()
}

#[test]
fn the_disclosure_turns_over_its_motion_and_then_rests() {
    let clock = ManualClock::new();
    let mut anim = Animator::new();
    anim.set_policy(Policy::Always);
    let rest = frames(&mut anim, &clock, &[false, false]);
    assert_eq!(rest, vec![(0.0, false), (0.0, false)], "a closed row asked");
    // The frame that asks for the new state still shows the old one; it moves from the next.
    let opening = frames(&mut anim, &clock, &[true; 16]);
    assert_eq!(opening[0], (0.0, true), "the turn did not wait a frame");
    let mut last = 0.0;
    let mut arrived = None;
    for (i, &(t, more)) in opening.iter().enumerate().skip(1) {
        assert!(t >= last, "frame {i}: the turn went back, {t} after {last}");
        last = t;
        if t == 1.0 && !more && arrived.is_none() {
            arrived = Some(i);
        }
        if arrived.is_none() {
            assert!(t < 1.0 && more, "frame {i}: {t}, more {more}");
        }
    }
    // 167 ms at 16 ms a frame: the eleventh frame after the one that asked is past it.
    let arrived = arrived.expect("the turn never ended");
    assert!((10..=12).contains(&arrived), "ended on frame {arrived}");
    assert!(opening[5].0 > 0.2 && opening[5].0 < 1.0, "{:?}", opening[5]);
    // And back, as long.
    let closing = frames(&mut anim, &clock, &[false; 16]);
    assert_eq!(closing[0], (1.0, true));
    assert!(closing[5].0 > 0.0 && closing[5].0 < 0.8, "{:?}", closing[5]);
    assert_eq!(closing[15], (0.0, false));
}

#[test]
fn a_row_first_seen_open_is_open_with_no_turn() {
    let clock = ManualClock::new();
    let mut anim = Animator::new();
    anim.set_policy(Policy::Always);
    assert_eq!(frames(&mut anim, &clock, &[true]), vec![(1.0, false)]);
}

#[test]
fn reduced_motion_and_never_turn_at_once() {
    let clock = ManualClock::new();
    let mut reduced = Animator::new();
    reduced.set_system_reduced(true);
    let mut never = Animator::new();
    never.set_policy(Policy::Never);
    for anim in [&mut reduced, &mut never] {
        let got = frames(anim, &clock, &[false, true, true, false]);
        assert_eq!(
            got,
            vec![(0.0, false), (1.0, false), (1.0, false), (0.0, false)]
        );
    }
}

#[test]
fn the_turn_lives_under_its_own_part_of_the_name() {
    let clock = ManualClock::new();
    let mut anim = Animator::new();
    anim.set_policy(Policy::Always);
    anim.begin_frame(clock.now());
    // The row's own name carries another value; the disclosure must not read or move it.
    let other: f32 = anim.value("row", 7.0_f32, crate::anim::fluent::FADE);
    assert_eq!(disclosure_turn(&mut anim, "row", true), 1.0);
    assert_eq!(other, 7.0);
    anim.end_frame();
    clock.advance(Duration::from_millis(16));
    anim.begin_frame(clock.now());
    assert_eq!(anim.value("row", 7.0_f32, crate::anim::fluent::FADE), 7.0);
    assert_eq!(disclosure_turn(&mut anim, "row", true), 1.0);
    assert!(anim.end_frame().is_none());
}

#[test]
fn the_animated_draw_is_the_plain_chevron_at_rest_and_turns_between() {
    for scale in [1.0, 1.5] {
        let (ground, ink) = (LIGHT.page, LIGHT.muted);
        let right = plain(scale, ground, ink, false);
        let down = plain(scale, ground, ink, true);
        let clock = ManualClock::new();
        let mut anim = Animator::new();
        anim.set_policy(Policy::Always);
        let mut frame = |expanded: bool| {
            clock.advance(Duration::from_millis(16));
            anim.begin_frame(clock.now());
            let bmp = drawn(scale, ground, |p| {
                draw_disclosure_animated(p, &mut anim, "row", 16.0, 15.5, SIZE, expanded, ink)
            });
            (bmp, anim.end_frame().is_some())
        };
        let (bmp, more) = frame(false);
        assert!(bmp.pixels == right.pixels && !more, "closed at rest");
        let (bmp, more) = frame(true);
        assert!(bmp.pixels == right.pixels && more, "the frame that asks");
        let (bmp, more) = frame(true);
        assert!(bmp.pixels != right.pixels && bmp.pixels != down.pixels && more);
        for _ in 0..12 {
            frame(true);
        }
        let (bmp, more) = frame(true);
        assert!(bmp.pixels == down.pixels && !more, "open at rest");
    }
}

/// The sheet a person looks at: the disclosure at eleven points of its turn, at three sizes, in
/// both themes. `cargo test -p mdshared-ui disclosure_sheet -- --ignored`, then open
/// target/specimen/disclosure.png and disclosure-150.png.
#[test]
#[ignore]
fn disclosure_sheet() {
    for (scale, file) in [(1.0, "disclosure.png"), (1.5, "disclosure-150.png")] {
        let (w, h) = (300.0, 160.0);
        let mut bmp = Bitmap::new((w * scale) as u32, (h * scale) as u32);
        let mut gfx = Gfx::new();
        {
            let mut p = Painter::new(&mut bmp, &mut gfx, scale);
            for (half, pal) in [&LIGHT, &DARK].into_iter().enumerate() {
                let oy = half as f32 * h / 2.0;
                p.fill(crate::gfx::Rect::new(0.0, oy, w, h / 2.0), pal.page);
                for (row, size) in [10.0, 12.0, 16.0].into_iter().enumerate() {
                    for k in 0..=10 {
                        let open = k as f32 / 10.0;
                        let (cx, cy) = (16.0 + 26.0 * k as f32, oy + 16.0 + 24.0 * row as f32);
                        draw_disclosure(&mut p, cx, cy, size, open, pal.muted);
                    }
                }
            }
        }
        let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/specimen");
        bmp.write_png(&out.join(file)).unwrap();
    }
}
