use super::*;
use crate::gfx::Gfx;
use mdshared_text::Bitmap;

/// A bitmap of one grey, so a shadow's alpha can be read back from any pixel.
fn ground(w: u32, h: u32, v: u8) -> Bitmap {
    let mut bmp = Bitmap::new(w, h);
    for px in bmp.pixels.chunks_mut(4) {
        px.copy_from_slice(&[v, v, v, 255]);
    }
    bmp
}

fn drawn(w: u32, h: u32, v: u8, scale: f32, f: impl FnOnce(&mut Painter)) -> Bitmap {
    let mut bmp = ground(w, h, v);
    let mut gfx = Gfx::new();
    f(&mut Painter::new(&mut bmp, &mut gfx, scale));
    bmp
}

fn px(bmp: &Bitmap, x: i32, y: i32) -> [u8; 4] {
    let o = (y as usize * bmp.width as usize + x as usize) * 4;
    bmp.pixels[o..o + 4].try_into().unwrap()
}

/// The shadow's alpha at a pixel of a white bitmap.
fn alpha(bmp: &Bitmap, x: i32, y: i32) -> f32 {
    1.0 - px(bmp, x, y)[1] as f32 / 255.0
}

/// The plain way: the rounded box sampled 6 x 6 times in each pixel of a window of four
/// deviations round the point, each sample weighed by the Gaussian, summed. Device px.
fn brute(b: (f32, f32, f32, f32), rad: f32, sigma: f32, x: f32, y: f32) -> f32 {
    const N: i32 = 6;
    let inside = |u: f32, v: f32| {
        if u < b.0 || u > b.2 || v < b.1 || v > b.3 {
            return false;
        }
        let qx = (b.0 + rad - u).max(u - (b.2 - rad)).max(0.0);
        let qy = (b.1 + rad - v).max(v - (b.3 - rad)).max(0.0);
        qx.hypot(qy) <= rad
    };
    let reach = (4.0 * sigma).ceil() as i32;
    let norm = 1.0 / (2.0 * std::f32::consts::PI * sigma * sigma) / (N * N) as f32;
    let mut sum = 0.0;
    for i in -reach * N..reach * N {
        for j in -reach * N..reach * N {
            let (du, dv) = ((i as f32 + 0.5) / N as f32, (j as f32 + 0.5) / N as f32);
            if inside(x + du, y + dv) {
                sum += (-(du * du + dv * dv) / (2.0 * sigma * sigma)).exp();
            }
        }
    }
    sum * norm
}

#[test]
fn a_soft_shadow_is_the_gaussian_blur_of_its_rounded_box() {
    // One layer of alpha 1, moved down 4 px and blurred 12 px (a deviation of 6), round a box
    // with 8 px corners, at 100% and 150%; probes along every side and round both kinds of
    // corner, against the box's mask blurred by brute force.
    for scale in [1.0f32, 1.5] {
        let r = Rect::new(40.0, 30.0, 90.0, 60.0);
        let layer = [Shade::new(0.0, 4.0, 12.0, 1.0)];
        let bmp = drawn(260, 200, 255, scale, |p| p.soft_shadow(r, 8.0, &layer, 1.0));
        let s = scale;
        let b = (40.0 * s, (30.0 + 4.0) * s, 130.0 * s, (90.0 + 4.0) * s);
        let probes = [
            (85.0, 92.0),
            (85.0, 100.0),
            (85.0, 110.0),
            (85.0, 26.0),
            (85.0, 20.0),
            (34.0, 60.0),
            (26.0, 60.0),
            (136.0, 60.0),
            (36.0, 26.0),
            (134.0, 24.0),
            (37.0, 94.0),
            (133.0, 97.0),
            (128.0, 93.0),
            (42.0, 92.0),
            (30.0, 104.0),
        ];
        for (lx, ly) in probes {
            let (x, y) = ((lx * s) as i32, (ly * s) as i32);
            let want = brute(b, 8.0 * s, 6.0 * s, x as f32 + 0.5, y as f32 + 0.5);
            let got = alpha(&bmp, x, y);
            assert!(
                (got - want).abs() <= 2.0 / 255.0,
                "at {scale}, ({x}, {y}): {got} drawn, {want} the blur"
            );
        }
    }
}

/// CSS's shadow below the middle of a wide box, `d` device px under its foot: each layer's
/// Gaussian across the foot alone, the layers over one another.
fn recipe_below(layers: &[Shade], k: f32, scale: f32, d: f32) -> f32 {
    let mut keep = 1.0;
    for l in layers {
        let sigma = l.blur * scale / 2.0;
        // A layer is never darker than black at its whole: its alpha times k is held to 1.
        keep *= 1.0 - (l.alpha * k).min(1.0) * phi((l.dy * scale - d) / sigma);
    }
    1.0 - keep
}

#[test]
fn below_a_wide_box_the_popup_shadow_is_the_css_recipe_at_100_and_150() {
    // `0 8px 24px 55%, 0 2px 6px 35%` at the look's 60%: at the foot about a third dark, and
    // fading to nothing some 40 px down.
    for scale in [1.0f32, 1.5] {
        let r = Rect::new(20.0, 20.0, 600.0, 100.0);
        let bmp = drawn(
            (680.0 * scale) as u32,
            (200.0 * scale) as u32,
            255,
            scale,
            |p| p.soft_shadow(r, 6.0, &Shade::POPUP, 0.6),
        );
        let foot = (120.0 * scale) as i32;
        let x = (320.0 * scale) as i32;
        for d in [0, 1, 2, 4, 8, 12, 16, 24, 32, 40, 50] {
            let y = foot + (d as f32 * scale) as i32;
            let want = recipe_below(&Shade::POPUP, 0.6, scale, (y - foot) as f32 + 0.5);
            let got = alpha(&bmp, x, y);
            assert!(
                (got - want).abs() <= 1.5 / 255.0,
                "{d} px below at {scale}: {got} drawn, {want} the recipe"
            );
        }
        // And the numbers themselves, at 100%: a third dark at the foot, a sixth 8 px down.
        if scale == 1.0 {
            assert!((alpha(&bmp, x, foot) - 0.35).abs() < 0.02);
            assert!((alpha(&bmp, x, foot + 8) - 0.18).abs() < 0.02);
            assert!(alpha(&bmp, x, foot + 44) < 1.0 / 255.0);
        }
    }
}

#[test]
fn the_shadow_falls_mostly_below_the_box() {
    // Above the box the popup shadow is under half what it is below, and the sides are between.
    let r = Rect::new(60.0, 60.0, 200.0, 120.0);
    let bmp = drawn(320, 260, 255, 1.0, |p| {
        p.soft_shadow(r, 6.0, &Shade::POPUP, 0.6)
    });
    let (above, side, below) = (
        alpha(&bmp, 160, 58),
        alpha(&bmp, 58, 120),
        alpha(&bmp, 160, 181),
    );
    assert!(above < below / 2.0, "above {above}, below {below}");
    assert!(above < side && side < below, "{above} {side} {below}");
}

#[test]
fn the_box_itself_is_left_as_it_was() {
    // Every pixel the box covers wholly keeps its colour; a corner pixel it covers in part is
    // shadowed less than the pixel just outside it.
    for scale in [1.0f32, 1.25, 1.5, 2.0] {
        let r = Rect::new(30.3, 20.6, 120.45, 70.15);
        let mut bmp = drawn(400, 300, 255, scale, |p| {
            p.soft_shadow(r, 9.0, &Shade::POPUP, 1.0);
        });
        let mut gfx = Gfx::new();
        let p = Painter::new(&mut bmp, &mut gfx, scale);
        let (x0, y0, x1, y1) = p.dev_rect(r);
        let band = (9.0 * scale).ceil() as i32;
        for y in y0..y1 {
            for x in x0..x1 {
                let corner = (x < x0 + band || x >= x1 - band) && (y < y0 + band || y >= y1 - band);
                if !corner {
                    assert_eq!(px(&bmp, x, y), [255; 4], "({x}, {y}) at {scale}");
                }
            }
        }
        // In the bottom-left corner square each pixel is the whole shadow there times what
        // the box's curve leaves of the pixel (as `rbox` covers it); some are cut in part.
        let rad = 9.0 * scale;
        let mut cut = 0;
        for y in y1 - band..y1 {
            for x in x0..x0 + band {
                let whole = {
                    let mut keep = 1.0;
                    for s in Shade::POPUP {
                        let (dx, dy) = (s.dx * scale, s.dy * scale);
                        let b = (
                            x0 as f32 + dx,
                            y0 as f32 + dy,
                            x1 as f32 + dx,
                            y1 as f32 + dy,
                        );
                        let l = Layer::new(s.alpha, b, rad, s.blur * scale / 2.0, x, 1, y, 1);
                        keep *= 1.0 - s.alpha * l.cover(0, 0);
                    }
                    1.0 - keep
                };
                let qx = (x0 as f32 + rad - (x as f32 + 0.5)).max(0.0);
                let qy = ((y as f32 + 0.5) - (y1 as f32 - rad)).max(0.0);
                let covered = (rad - qx.hypot(qy) + 0.5).clamp(0.0, 1.0);
                if covered > 0.0 && covered < 1.0 {
                    cut += 1;
                }
                let want = whole * (1.0 - covered);
                let got = alpha(&bmp, x, y);
                assert!(
                    (got - want).abs() <= 1.0 / 255.0,
                    "({x}, {y}) at {scale}: {got}, {want}"
                );
            }
        }
        assert!(cut > 0, "at {scale}");
    }
}

#[test]
fn a_dark_page_is_darkened_by_the_shadow_not_sunk_to_black() {
    // On mdplay's dark page, value 20, the pixel under a card is 20 times what the recipe
    // keeps, rounded: never 0, as many faint boxes each rounded down made it.
    let r = Rect::new(20.0, 20.0, 300.0, 100.0);
    let bmp = drawn(360, 200, 20, 1.0, |p| {
        p.soft_shadow(r, 8.0, &Shade::POPUP, 0.6)
    });
    for d in [0, 2, 4, 8, 16] {
        let y = 120 + d;
        let want = 20.0 * (1.0 - recipe_below(&Shade::POPUP, 0.6, 1.0, d as f32 + 0.5));
        let got = px(&bmp, 170, y)[0] as f32;
        assert!(
            (got - want).abs() <= 1.0,
            "{d} px below: {got}, want {want}"
        );
        assert!(got >= 12.0);
    }
}

#[test]
fn a_tail_too_faint_to_take_half_a_level_off_a_dark_page_leaves_it_as_it_was() {
    // One blend a pixel, rounded: where the recipe takes less than half a level off a page of
    // 20 the page stays 20, though the same rows of a white page are shadowed. A blend that
    // rounded down would take a level off every pixel in reach, a dark rim round every card.
    let r = Rect::new(20.0, 20.0, 600.0, 100.0);
    let shadow = |p: &mut Painter| p.soft_shadow(r, 6.0, &Shade::POPUP, 0.6);
    let dark = drawn(680, 200, 20, 1.0, shadow);
    let white = drawn(680, 200, 255, 1.0, shadow);
    let mut faint = 0;
    for y in 120..200 {
        let a = recipe_below(&Shade::POPUP, 0.6, 1.0, (y - 120) as f32 + 0.5);
        if a * 256.0 >= 2.0 && a * 20.0 < 0.4 {
            faint += 1;
            assert_eq!(px(&dark, 320, y)[0], 20, "row {y}: {a}");
            assert!(px(&white, 320, y)[0] < 255, "row {y} is shadowed: {a}");
        }
    }
    assert!(faint >= 3, "{faint} faint rows");
}

#[test]
fn a_corner_is_cut_into_strips_by_its_blur_not_by_its_pixels() {
    // A popup's layers at 150% round the Look dialog's largest corners (18 px): four strips
    // under the wide one, thirteen under the sharp one, where a strip a pixel high made
    // eighteen each; square corners none; at most MAX_STRIPS.
    assert_eq!(strips_for(18.0, 18.0), 4);
    assert_eq!(strips_for(18.0, 4.5), 13);
    assert_eq!(strips_for(2.0, 36.0), 1);
    assert_eq!(strips_for(0.0, 6.0), 0);
    assert_eq!(strips_for(500.0, 0.5), MAX_STRIPS);
}

/// The most a layer of alpha 1 round a box with corners of `rad` under a deviation of `sigma`,
/// its corners cut into `strips`, differs from the same cut into strips a twentieth of a pixel
/// apart, in levels of 255.
fn strip_error(rad: f32, sigma: f32, strips: usize) -> f32 {
    let b = (
        100.0,
        100.0,
        100.0 + 4.0 * rad + 40.0,
        100.0 + 3.0 * rad + 40.0,
    );
    let reach = (SIGMAS * sigma + 2.0).ceil() as i32;
    let (cx0, cy0) = (100 - reach, 100 - reach);
    let cols = (b.2 - b.0) as usize + 2 * reach as usize;
    let rows = (b.3 - b.1) as usize + 2 * reach as usize;
    let area = (cx0, cols, cy0, rows);
    let coarse = Layer::with_strips(1.0, b, rad, sigma, area, strips);
    let fine = Layer::with_strips(1.0, b, rad, sigma, area, (rad * 20.0).ceil() as usize);
    let mut worst = 0.0f32;
    for r in 0..rows {
        for c in 0..cols {
            worst = worst.max((coarse.cover(c, r) - fine.cover(c, r)).abs());
        }
    }
    worst * 255.0
}

#[test]
fn coarse_strips_cost_under_half_a_level() {
    // For corners small to large and blurs from a pixel to very wide, the strips the blur asks
    // for against strips a twentieth of a pixel apart: under half a level of 255, wherever the
    // count is not held to MAX_STRIPS.
    let mut tried = 0;
    for rad in [2.0f32, 4.0, 6.0, 9.0, 12.0, 18.0, 24.0, 36.0] {
        for sigma in [1.0f32, 1.5, 2.0, 3.0, 4.5, 6.0, 9.0, 12.0, 18.0, 36.0] {
            let n = strips_for(rad, sigma);
            if n == MAX_STRIPS {
                continue;
            }
            tried += 1;
            let e = strip_error(rad, sigma, n);
            assert!(
                e < 0.5,
                "corners {rad}, deviation {sigma}, {n} strips: {e} levels"
            );
        }
    }
    assert!(tried > 60, "{tried} cases");
}

#[test]
fn the_strength_scales_every_layer_and_zero_draws_nothing() {
    let r = Rect::new(20.0, 20.0, 300.0, 100.0);
    let at = |k: f32| {
        let bmp = drawn(360, 200, 255, 1.0, |p| {
            p.soft_shadow(r, 6.0, &Shade::POPUP, k)
        });
        alpha(&bmp, 170, 124)
    };
    assert_eq!(at(0.0), 0.0);
    for k in [0.3f32, 0.6, 1.0, 2.0] {
        let want = recipe_below(&Shade::POPUP, k, 1.0, 4.5);
        assert!((at(k) - want).abs() <= 1.5 / 255.0, "k {k}");
    }
}

#[test]
fn the_shadow_at_150_is_the_shadow_at_100_made_larger() {
    let r = Rect::new(20.0, 20.0, 200.0, 80.0);
    let one = drawn(300, 200, 255, 1.0, |p| {
        p.soft_shadow(r, 6.0, &Shade::POPUP, 0.6)
    });
    let more = drawn(450, 300, 255, 1.5, |p| {
        p.soft_shadow(r, 6.0, &Shade::POPUP, 0.6)
    });
    // Logical points below, beside and above the box, and past its corners.
    for (x, y) in [
        (120, 104),
        (120, 112),
        (120, 130),
        (12, 60),
        (228, 60),
        (120, 16),
        (14, 108),
        (226, 108),
    ] {
        let a = alpha(&one, x, y);
        let b = alpha(&more, (x as f32 * 1.5) as i32, (y as f32 * 1.5) as i32);
        assert!(
            (a - b).abs() <= 3.0 / 255.0,
            "({x}, {y}): {a} at 100%, {b} at 150%"
        );
    }
}

#[test]
fn nothing_reaches_past_its_bounds_or_the_clip() {
    let r = Rect::new(80.0, 70.0, 120.0, 60.0);
    for layers in [
        &Shade::POPUP[..],
        &Shade::TAB[..],
        &[Shade::new(-9.0, 14.0, 30.0, 1.0)][..],
    ] {
        let bmp = drawn(360, 280, 255, 1.0, |p| p.soft_shadow(r, 6.0, layers, 4.0));
        let b = soft_bounds(r, layers);
        for y in 0..280 {
            for x in 0..360 {
                if !b.contains(x as f32 + 0.5, y as f32 + 0.5) {
                    assert_eq!(px(&bmp, x, y), [255; 4], "({x}, {y}) outside {b:?}");
                }
            }
        }
    }
    let clip = Rect::new(100.0, 120.0, 60.0, 30.0);
    let bmp = drawn(360, 280, 255, 1.0, |p| {
        p.clip(clip);
        p.soft_shadow(r, 6.0, &Shade::POPUP, 1.0);
    });
    let mut inside = 0;
    for y in 0..280 {
        for x in 0..360 {
            if clip.contains(x as f32 + 0.5, y as f32 + 0.5) {
                inside += (px(&bmp, x, y) != [255; 4]) as u32;
            } else {
                assert_eq!(px(&bmp, x, y), [255; 4], "({x}, {y}) outside the clip");
            }
        }
    }
    assert!(inside > 0, "the clip's part of the shadow is drawn");
}

#[test]
fn an_offset_moves_the_shadow_as_it_moves_the_box() {
    let r = Rect::new(40.0, 30.0, 100.0, 50.0);
    for scale in [1.0f32, 1.5] {
        let moved = drawn(300, 220, 255, scale, |p| {
            p.offset(12.0, 8.0);
            p.soft_shadow(r, 6.0, &Shade::POPUP, 1.0);
        });
        let placed = drawn(300, 220, 255, scale, |p| {
            p.soft_shadow(Rect::new(52.0, 38.0, 100.0, 50.0), 6.0, &Shade::POPUP, 1.0);
        });
        assert!(moved.pixels == placed.pixels, "at {scale}");
    }
}

#[test]
fn nonsense_draws_nothing_and_does_not_panic() {
    let good = Rect::new(20.0, 20.0, 60.0, 40.0);
    let nan = f32::NAN;
    let inf = f32::INFINITY;
    let untouched = |f: &dyn Fn(&mut Painter)| {
        let bmp = drawn(120, 100, 255, 1.0, f);
        bmp.pixels.iter().all(|&v| v == 255)
    };
    for k in [0.0, -1.0, nan, -inf] {
        assert!(
            untouched(&|p| p.soft_shadow(good, 6.0, &Shade::POPUP, k)),
            "k {k}"
        );
    }
    for r in [
        Rect::new(nan, 20.0, 60.0, 40.0),
        Rect::new(20.0, inf, 60.0, 40.0),
        Rect::new(20.0, 20.0, -inf, 40.0),
        Rect::new(20.0, 20.0, 0.0, 40.0),
        Rect::new(20.0, 20.0, 60.0, -5.0),
        Rect::new(-3e38, -3e38, 3e38, 3e38),
    ] {
        assert!(
            untouched(&|p| p.soft_shadow(r, 6.0, &Shade::POPUP, 1.0)),
            "{r:?}"
        );
    }
    assert!(untouched(&|p| p.soft_shadow(good, 6.0, &[], 1.0)));
    for l in [
        Shade::new(nan, 2.0, 6.0, 0.5),
        Shade::new(0.0, inf, 6.0, 0.5),
        Shade::new(0.0, 2.0, nan, 0.5),
        Shade::new(0.0, 2.0, 6.0, 0.0),
        Shade::new(0.0, 2.0, 6.0, -1.0),
        Shade::new(0.0, 2.0, 6.0, nan),
    ] {
        assert!(untouched(&|p| p.soft_shadow(good, 6.0, &[l], 1.0)), "{l:?}");
    }
    // Out of reason but drawable: held, not refused, and nothing panics.
    for (radius, l) in [
        (nan, Shade::POPUP[0]),
        (-4.0, Shade::POPUP[0]),
        (1e9, Shade::POPUP[0]),
        (6.0, Shade::new(1e9, -1e9, 1e9, 9.0)),
        (6.0, Shade::new(0.0, 0.0, -5.0, 0.5)),
        (6.0, Shade::new(0.0, 0.0, 0.0, 1.0)),
    ] {
        drawn(120, 100, 255, 1.0, |p| {
            p.soft_shadow(good, radius, &[l], 4.0)
        });
        drawn(120, 100, 255, 1.0, |p| {
            p.soft_shadow(good, radius, &[l], inf)
        });
    }
    let b = soft_bounds(good, &[Shade::new(nan, 0.0, 6.0, 1.0)]);
    assert_eq!(b, good);
}

#[test]
fn the_recipes_are_the_prototypes() {
    // mdplay's look prototype (mockup/html): menus `0 8px 24px 55%, 0 2px 6px 35%`, a joined
    // menu's control `0 2px 10px 45%`, and light themes at .45 of the dark's.
    assert_eq!(
        Shade::POPUP,
        [
            Shade::new(0.0, 8.0, 24.0, 0.55),
            Shade::new(0.0, 2.0, 6.0, 0.35)
        ]
    );
    assert_eq!(Shade::TAB, [Shade::new(0.0, 2.0, 10.0, 0.45)]);
    assert_eq!(LIGHT_SHADOW, 0.45);
}

#[test]
fn phi_is_the_normal_distribution() {
    // Against a few known values of the standard normal cumulative distribution.
    for (t, want) in [
        (0.0, 0.5),
        (1.0, 0.841_344_7),
        (-1.0, 0.158_655_3),
        (2.0, 0.977_249_9),
        (-2.5, 0.006_209_7),
        (3.0, 0.998_650_1),
        (-7.0, 0.0),
        (7.0, 1.0),
    ] {
        assert!((phi(t) - want).abs() < 1e-6, "phi({t}) = {}", phi(t));
    }
}
