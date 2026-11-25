use super::*;
use crate::gfx::{Gfx, Rect};
use crate::glyph;
use mdshared_text::Bitmap;

const W: u32 = 48;
const INK: Rgb = [0, 0, 0];

/// A white bitmap `W` square with `f` drawn on it at `scale`.
fn drawn(scale: f32, f: impl FnOnce(&mut Painter)) -> Bitmap {
    let side = (W as f32 * scale).ceil() as u32;
    let mut bmp = Bitmap::new(side, side);
    for px in bmp.pixels.chunks_mut(4) {
        px.copy_from_slice(&[255, 255, 255, 255]);
    }
    let mut gfx = Gfx::new();
    f(&mut Painter::new(&mut bmp, &mut gfx, scale));
    bmp
}

/// How much ink each pixel holds, 0 to 255, from a black drawing on white.
fn ink(bmp: &Bitmap) -> Vec<f32> {
    bmp.pixels
        .as_chunks::<4>()
        .0
        .iter()
        .map(|px| 255.0 - px[1] as f32)
        .collect()
}

/// The total ink, its centroid, and the box of the pixels with more than a quarter of ink.
fn shape(bmp: &Bitmap) -> (f32, (f32, f32), (usize, usize, usize, usize)) {
    let w = bmp.width as usize;
    let (mut sum, mut sx, mut sy) = (0.0, 0.0, 0.0);
    let mut bbox = (usize::MAX, usize::MAX, 0, 0);
    for (i, a) in ink(bmp).into_iter().enumerate() {
        let (x, y) = (i % w, i / w);
        sum += a;
        sx += a * x as f32;
        sy += a * y as f32;
        if a > 64.0 {
            bbox = (bbox.0.min(x), bbox.1.min(y), bbox.2.max(x), bbox.3.max(y));
        }
    }
    (sum, (sx / sum, sy / sum), bbox)
}

fn close(a: usize, b: usize, by: usize) -> bool {
    a.abs_diff(b) <= by
}

#[test]
fn no_turn_is_the_upright_icon_pixel_for_pixel() {
    for scale in [1.0, 1.5] {
        for (name, path) in glyph::ALL {
            let upright = drawn(scale, |p| p.icon(path, 16.0, 20.0, 21.5, INK));
            for degrees in [0.0, -0.0, 360.0, -360.0, 720.0] {
                let turned = drawn(scale, |p| {
                    p.icon_rotated(path, 16.0, 20.0, 21.5, degrees, INK)
                });
                assert!(
                    upright.pixels == turned.pixels,
                    "{name} at {degrees} degrees, scale {scale}, is not the icon"
                );
            }
        }
    }
}

#[test]
fn a_right_chevron_turned_a_quarter_is_the_down_chevron_and_three_quarters_the_up() {
    // RIGHT's points turned a quarter about (8, 8) are DOWN's points (in the other order), so
    // the drawings must agree up to rounding: the same ink, the same place, the same box.
    for scale in [1.0, 1.5] {
        for (degrees, other, name) in [
            (90.0, glyph::DOWN, "DOWN"),
            (-270.0, glyph::DOWN, "DOWN"),
            (270.0, glyph::UP, "UP"),
            (-90.0, glyph::UP, "UP"),
        ] {
            let turned = drawn(scale, |p| {
                p.icon_rotated(glyph::RIGHT, 16.0, 24.0, 24.0, degrees, INK)
            });
            let plain = drawn(scale, |p| p.icon(other, 16.0, 24.0, 24.0, INK));
            let (ink_t, (xt, yt), box_t) = shape(&turned);
            let (ink_p, (xp, yp), box_p) = shape(&plain);
            let at = format!("RIGHT at {degrees} against {name}, scale {scale}");
            assert!(
                (ink_t - ink_p).abs() / ink_p < 0.03,
                "{at}: ink {ink_t} and {ink_p}"
            );
            assert!(
                (xt - xp).abs() < 0.25 && (yt - yp).abs() < 0.25,
                "{at}: centroid ({xt}, {yt}) and ({xp}, {yp})"
            );
            assert!(
                close(box_t.0, box_p.0, 1)
                    && close(box_t.1, box_p.1, 1)
                    && close(box_t.2, box_p.2, 1)
                    && close(box_t.3, box_p.3, 1),
                "{at}: box {box_t:?} and {box_p:?}"
            );
            let worst = ink(&turned)
                .iter()
                .zip(ink(&plain))
                .map(|(a, b)| (a - b).abs())
                .fold(0.0, f32::max);
            assert!(worst <= 32.0, "{at}: a pixel differs by {worst}");
        }
    }
}

/// The right chevron's three points turned by hand `degrees` clockwise about the centre of the
/// grid, written out as a path of its own: a second way of drawing the turned chevron, which
/// `Painter::icon` draws upright.
fn right_turned_by_hand(degrees: f32) -> String {
    let (sin, cos) = degrees.to_radians().sin_cos();
    let turn = |x: f32, y: f32| {
        let (dx, dy) = (x - 8.0, y - 8.0);
        (8.0 + dx * cos - dy * sin, 8.0 + dx * sin + dy * cos)
    };
    let (a, b, c) = (turn(5.8, 3.5), turn(10.3, 8.0), turn(5.8, 12.5));
    format!(
        "M{:.5} {:.5}L{:.5} {:.5}L{:.5} {:.5}",
        a.0, a.1, b.0, b.1, c.0, c.1
    )
}

#[test]
fn a_turn_on_the_way_is_the_chevron_turned_by_hand_and_neither_end() {
    assert_eq!(
        glyph::RIGHT,
        "M5.8 3.5L10.3 8l-4.5 4.5",
        "the points above are RIGHT's"
    );
    for scale in [1.0, 1.5] {
        let right = drawn(scale, |p| p.icon(glyph::RIGHT, 16.0, 24.0, 24.0, INK));
        let down = drawn(scale, |p| p.icon(glyph::DOWN, 16.0, 24.0, 24.0, INK));
        for degrees in [9.0, 30.0, 45.0, 60.0, 81.0, 135.0, 200.0, -45.0] {
            let turned = drawn(scale, |p| {
                p.icon_rotated(glyph::RIGHT, 16.0, 24.0, 24.0, degrees, INK)
            });
            let path = right_turned_by_hand(degrees);
            let by_hand = drawn(scale, |p| p.icon(&path, 16.0, 24.0, 24.0, INK));
            let at = format!("{degrees} degrees at scale {scale}");
            assert!(
                turned.pixels != right.pixels && turned.pixels != down.pixels,
                "{at} is a resting chevron"
            );
            let worst = ink(&turned)
                .iter()
                .zip(ink(&by_hand))
                .map(|(a, b)| (a - b).abs())
                .fold(0.0, f32::max);
            assert!(
                worst <= 4.0,
                "{at}: a pixel differs from the hand-turned path by {worst}"
            );
            let (ink_t, _, _) = shape(&turned);
            let (ink_r, _, _) = shape(&right);
            assert!(
                (ink_t - ink_r).abs() / ink_r < 0.05,
                "{at}: ink {ink_t} and {ink_r}"
            );
        }
    }
}

#[test]
fn turned_masks_are_not_kept_in_the_cache() {
    let mut bmp = Bitmap::new(W, W);
    let mut gfx = Gfx::new();
    let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
    p.icon(glyph::RIGHT, 16.0, 24.0, 24.0, INK);
    let kept = p.gfx.icons.len();
    for step in 1..200 {
        p.icon_rotated(glyph::RIGHT, 16.0, 24.0, 24.0, step as f32 * 0.45, INK);
    }
    assert_eq!(p.gfx.icons.len(), kept, "an animation grew the cache");
}

#[test]
fn a_turned_icon_keeps_to_the_clip_and_follows_the_offset() {
    let whole = drawn(1.0, |p| {
        p.icon_rotated(glyph::SEARCH, 16.0, 24.0, 24.0, 30.0, INK)
    });
    let clipped = drawn(1.0, |p| {
        let old = p.clip(Rect::new(0.0, 0.0, 24.0, W as f32));
        p.icon_rotated(glyph::SEARCH, 16.0, 24.0, 24.0, 30.0, INK);
        p.restore(old);
    });
    let w = W as usize;
    for (i, (a, b)) in whole
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .zip(clipped.pixels.as_chunks::<4>().0.iter())
        .enumerate()
    {
        if i % w < 24 {
            assert_eq!(a, b, "inside the clip at {i}");
        } else {
            assert_eq!(*b, [255, 255, 255, 255], "outside the clip at {i}");
        }
    }
    let moved = drawn(1.0, |p| {
        let old = p.offset(4.0, 3.0);
        p.icon_rotated(glyph::SEARCH, 16.0, 20.0, 21.0, 30.0, INK);
        p.restore_offset(old);
    });
    assert!(moved.pixels == whole.pixels, "the offset was not followed");
}

#[test]
fn nonsense_draws_nothing() {
    let blank = drawn(1.0, |_| {});
    for (size, degrees) in [
        (16.0, f32::NAN),
        (16.0, f32::INFINITY),
        (16.0, f32::NEG_INFINITY),
        (f32::NAN, 30.0),
        (f32::INFINITY, 30.0),
        (0.0, 30.0),
        (-16.0, 30.0),
    ] {
        let bmp = drawn(1.0, |p| {
            p.icon_rotated(glyph::RIGHT, size, 24.0, 24.0, degrees, INK)
        });
        assert!(
            bmp.pixels == blank.pixels,
            "size {size} at {degrees} degrees drew"
        );
    }
    // Far outside the bitmap, and with an empty glyph, nothing is drawn and nothing panics.
    let bmp = drawn(1.0, |p| {
        p.icon_rotated(glyph::RIGHT, 16.0, -500.0, 5000.0, 30.0, INK);
        p.icon_rotated("", 16.0, 24.0, 24.0, 30.0, INK);
    });
    assert!(bmp.pixels == blank.pixels);
}
