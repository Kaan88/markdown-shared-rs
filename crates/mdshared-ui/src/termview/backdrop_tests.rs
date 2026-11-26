//! Terminal background pixels before and after the host's chrome key pass.

use super::*;
use crate::frost::{Key, key_out};
use crate::gfx::{Gfx, Painter, Rect, Rgb};
use crate::theme::{DARK, LIGHT};
use mdshared_term::{Point, Screen, Selection, SelectionKind};
use mdshared_text::Bitmap;

const W: i32 = 16;
const H: i32 = 24;
const AREA: Rect = Rect::new(0.0, 0.0, 117.0, 51.0);

fn screen(seq: &str) -> Screen {
    let mut s = Screen::new(7, 2, 0);
    s.feed(seq.as_bytes());
    s
}

fn key() -> Key {
    Key::new(DARK.page, 52)
}

fn pixel(bmp: &Bitmap, x: i32, y: i32) -> [u8; 4] {
    let i = (y as usize * bmp.width as usize + x as usize) * 4;
    bmp.pixels[i..i + 4].try_into().unwrap()
}

fn metrics() -> Metrics {
    Metrics::fixed(18.0, 1.0, W as f32, H as f32)
}

fn paint(
    s: &Screen,
    scheme: &Scheme,
    opacity: Option<BackgroundOpacity>,
    frosted: bool,
    sel: Option<&Selection>,
    view: View,
) -> Bitmap {
    let mut bmp = Bitmap::new(120, 54);
    bmp.clear([17, 18, 19]);
    let mut gfx = Gfx::new();
    let mut font = CellFont::new();
    let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
    p.set_frost_key(frosted.then(key));
    if let Some(opacity) = opacity {
        draw_screen_with_background(
            &mut p,
            &mut font,
            scheme,
            &metrics(),
            AREA,
            s,
            sel,
            view,
            opacity,
        );
    } else {
        draw_screen(&mut p, &mut font, scheme, &metrics(), AREA, s, sel, view);
    }
    bmp
}

#[test]
fn only_default_background_is_translucent_even_when_highlights_and_explicit_colours_match_it() {
    let s = screen(" \x1b[48;2;10;20;30m \x1b[0mab\x1b[7m \x1b[0m\x1b[1;6H");
    let line = s.screen_start();
    let mut sel = Selection::start(&s, Point::new(line, 2), SelectionKind::Stream, 1);
    sel.extend(&s, Point::new(line, 3));
    let mut scheme = Scheme::from_palette(&DARK);
    scheme.background = [10, 20, 30];
    scheme.selection = scheme.background;
    scheme.selection_text = Some(scheme.background);
    scheme.cursor = scheme.background;
    let bmp = paint(
        &s,
        &scheme,
        Some(BackgroundOpacity::new(0.8)),
        true,
        Some(&sel),
        View::new().focused(true),
    );
    let opaque = [30, 20, 10, 255];
    assert_eq!(pixel(&bmp, W / 2, 4), [24, 16, 8, 204]);
    for col in [1, 2, 5] {
        assert_eq!(pixel(&bmp, col * W + W / 2, 4), opaque, "col {col}");
    }
    assert_eq!(pixel(&bmp, 4 * W + W / 2, 4)[3], 255, "inverse");
    let draw = |a| {
        paint(
            &s,
            &scheme,
            Some(BackgroundOpacity::new(a)),
            true,
            None,
            View::new(),
        )
    };
    let bmp = draw(253.0 / 255.0);
    let mut present: Vec<u32> = bmp
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .copied()
        .map(u32::from_le_bytes)
        .collect();
    let before = present.clone();
    key_out(
        &key(),
        &mut present,
        bmp.width as usize,
        (0, 0, bmp.width as usize, bmp.height as usize),
    );
    assert_eq!(present[W as usize / 2] >> 24, 253);
    assert_eq!(present, before);
    assert_eq!(pixel(&draw(254.0 / 255.0), W / 2, 4), opaque);
}

#[test]
fn box_shades_and_edges_composite_over_black_and_white_without_darkening_the_ground() {
    let s = screen("░▒▓╳╭\x1b[4;9m \x1b[0m\x1b[1;7H\x1b[6 q");
    let scheme = Scheme::from_palette(&DARK);
    let view = View::new().focused(true);
    let old = paint(&s, &scheme, None, true, None, view);
    let got = paint(
        &s,
        &scheme,
        Some(BackgroundOpacity::new(0.8)),
        true,
        None,
        view,
    );
    let back = pixel(&got, W + 1, H + 1);
    let [r, g, b] = scheme.foreground;
    for (col, ch) in ['░', '▒', '▓', '╳', '╭'].into_iter().enumerate() {
        let pieces = super::boxes::pieces(ch, W, H, metrics().stroke()).unwrap();
        let mut partial = 0;
        for y in 0..H {
            for x in 0..W {
                let now = pixel(&got, col as i32 * W + x, y);
                assert_ne!(now[3], crate::frost::MARK);
                let covers: Vec<_> = pieces
                    .iter()
                    .map(|p| match *p {
                        super::boxes::Piece::Fill {
                            x0,
                            y0,
                            x1,
                            y1,
                            alpha,
                        } if (x0..x1).contains(&x) && (y0..y1).contains(&y) => alpha,
                        _ => super::boxes::coverage(p, x, y, W, H),
                    })
                    .filter(|&a| a > 0.0)
                    .collect();
                // Check single-piece samples independently; intersections can accumulate ink.
                if let [a] = covers.as_slice()
                    && *a < 1.0
                {
                    let a = (*a * 255.0).round() / 255.0;
                    let alpha = (204.0 + 51.0 * a).round() as u8;
                    let alpha = if alpha == 254 { 255 } else { alpha };
                    assert_eq!(now[3], alpha, "{ch} at ({x},{y})");
                    for (i, ink) in [b, g, r].into_iter().enumerate() {
                        let expected = f32::from(ink) * a + f32::from(back[i]) * (1.0 - a);
                        assert!((f32::from(now[i]) - expected).abs() <= 1.0);
                        for desktop in [0.0, 255.0] {
                            let got = f32::from(now[i]) + desktop * f32::from(255 - alpha) / 255.0;
                            let want = expected + desktop * 51.0 * (1.0 - a) / 255.0;
                            assert!((got - want).abs() <= 1.6, "{ch}: {got} vs {want}");
                        }
                    }
                    partial += 1;
                }
            }
        }
        assert!(partial > 0, "{ch} did not exercise partial coverage");
    }
    for x in 5 * W..7 * W {
        for y in 0..H {
            let was = pixel(&old, x, y);
            if was != pixel(&old, W + 1, H + 1) {
                assert_eq!(pixel(&got, x, y), was, "decoration or cursor");
            }
        }
    }
}

#[test]
fn repainting_a_finished_translucent_row_replaces_coverage_instead_of_stacking_it() {
    let old = screen("╳▓\r\n▒");
    let new = screen("\x1b[38;2;30;30;30m╳\x1b[0m░\r\n▒");
    let scheme = Scheme::from_palette(&DARK);
    let view = View::new().cursor_shown(false);
    let opacity = BackgroundOpacity::new(0.8).with_padding(true);
    let mut bmp = paint(&old, &scheme, Some(opacity), true, None, view);
    let want = paint(&new, &scheme, Some(opacity), true, None, view);
    assert_ne!(bmp.pixels, want.pixels);
    let mut gfx = Gfx::new();
    let mut font = CellFont::new();
    for _ in 0..3 {
        let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
        p.set_frost_key(Some(key()));
        draw_rows_with_background(
            &mut p,
            &mut font,
            &scheme,
            &metrics(),
            AREA,
            &new,
            None,
            view,
            std::slice::from_ref(&(0..1)),
            opacity,
        );
        for y in 0..bmp.height as i32 {
            for x in 0..bmp.width as i32 {
                assert_eq!(
                    pixel(&bmp, x, y),
                    pixel(&want, x, y),
                    "repaint at ({x}, {y})"
                );
            }
        }
    }
}

#[test]
fn the_contrast_floor_bounds_the_composite_and_leaves_already_low_contrast_colours_opaque() {
    let s = screen("╳\x1b[38;2;123;123;123m╳\x1b[0m ");
    for (ground, ink) in [([120; 3], [0; 3]), ([112; 3], [255; 3])] {
        let mut scheme = Scheme::from_palette(&LIGHT);
        scheme.background = ground;
        scheme.foreground = ink;
        let bmp = paint(
            &s,
            &scheme,
            Some(BackgroundOpacity::new(0.05)),
            true,
            None,
            View::new().cursor_shown(false),
        );
        let [b, g, r, a] = pixel(&bmp, 2 * W + W / 2, 4);
        assert!(a > 200 && a < 254, "floor alpha: {a}");
        let contrast = |pre: Rgb, alpha: u8, d: u8| {
            let over =
                pre.map(|ch| ch + ((u32::from(255 - alpha) * u32::from(d) + 127) / 255) as u8);
            crate::cover::contrast(ink, over)
        };
        for desktop in 0..=255 {
            assert!(contrast([r, g, b], a, desktop) >= 4.5, "{a} over {desktop}");
        }
        let weaker = ground.map(|c| ((u32::from(c) * u32::from(a - 1) + 127) / 255) as u8);
        assert!(contrast(weaker, a - 1, 0).min(contrast(weaker, a - 1, 255)) < 4.5);
        assert!((0..H).all(|y| (W..2 * W).all(|x| pixel(&bmp, x, y)[3] == 255)));
    }
}

#[test]
fn the_padding_option_controls_both_edges_and_clipped_rows_match_the_full_frame() {
    let s = screen("abc\r\n╳");
    let scheme = Scheme::from_palette(&DARK);
    let view = View::new().cursor_shown(false);
    for pad in [false, true] {
        let opacity = BackgroundOpacity::new(0.8).with_padding(pad);
        let full = paint(&s, &scheme, Some(opacity), true, None, view);
        let expected = if pad { 204 } else { 255 };
        assert_eq!(pixel(&full, 7 * W + 1, H + 4)[3], expected);
        assert_eq!(pixel(&full, 1, 2 * H + 1)[3], expected);
        assert_eq!(pixel(&full, 2 * W, H + 4)[3], 204);
        let mut bmp = Bitmap::new(120, 54);
        bmp.clear([17, 18, 19]);
        let mut gfx = Gfx::new();
        let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
        p.set_frost_key(Some(key()));
        let clip = p.clip(Rect::new(3.0, 2.0, AREA.w, AREA.h));
        draw_rows_with_background(
            &mut p,
            &mut CellFont::new(),
            &scheme,
            &metrics(),
            AREA,
            &s,
            None,
            view,
            std::slice::from_ref(&(1..2)),
            opacity,
        );
        p.restore(clip);
        p.clip(Rect::new(500.0, 0.0, 20.0, AREA.h));
        draw_screen_with_background(
            &mut p,
            &mut CellFont::new(),
            &scheme,
            &metrics(),
            AREA,
            &s,
            None,
            view,
            opacity,
        );
        for y in 0..54 {
            for x in 0..120 {
                let expected = if (H..2 * H).contains(&y) && (3..AREA.w as i32).contains(&x) {
                    pixel(&full, x, y)
                } else {
                    [19, 18, 17, 255]
                };
                assert_eq!(pixel(&bmp, x, y), expected, "({x}, {y})");
            }
        }
    }
}

#[test]
fn translucent_glyph_overhang_does_not_enter_a_neighbour_or_disappear_on_repaint() {
    let s = Screen::new(2, 1, 0);
    let mut ink = Screen::new(2, 1, 0);
    ink.feed(b"\x1b[38;2;120;120;120mW");
    let mut scheme = Scheme::from_palette(&DARK);
    scheme.background = [0; 3];
    scheme.foreground = [255; 3];
    let view = View::new().cursor_shown(false);
    let m = Metrics::fixed(24.0, 1.0, 6.0, 32.0);
    let area = Rect::new(0.0, 0.0, 12.0, 32.0);
    let mut gfx = Gfx::new();
    let mut font = CellFont::new();
    let opacity = BackgroundOpacity::new(0.0);
    let mut draw = |bmp: &mut Bitmap, screen: &Screen, opacity| {
        let mut p = Painter::new(bmp, &mut gfx, 1.0);
        p.set_frost_key(Some(key()));
        draw_screen_with_background(
            &mut p, &mut font, &scheme, &m, area, screen, None, view, opacity,
        );
    };
    let mut blank = Bitmap::new(12, 32);
    draw(&mut blank, &s, opacity);
    let mut full = Bitmap::new(12, 32);
    draw(&mut full, &ink, opacity);
    let mut opaque = Bitmap::new(12, 32);
    draw(&mut opaque, &ink, BackgroundOpacity::new(1.0));
    let mut legacy = Bitmap::new(12, 32);
    legacy.clear([0; 3]);
    font.draw(
        &mut legacy,
        (0, 0, 12, 32),
        'W',
        (false, false),
        &m,
        (0, 6, m.baseline()),
        [120; 3],
        false,
    );
    if !(0..32).any(|y| (6..12).any(|x| pixel(&legacy, x, y)[0] > 0)) {
        eprintln!("skipping overhang test: system font did not produce an overhang");
        return;
    }
    assert_eq!(
        opaque.pixels, legacy.pixels,
        "opaque retains row-wide glyph drawing"
    );
    assert_eq!(pixel(&blank, 6, 0), [0, 0, 0, 137]);
    assert!((0..32).any(|y| (0..6).any(|x| pixel(&full, x, y)[0] > 0)));
    let mut repainted = Bitmap::new(12, 32);
    repainted.pixels.copy_from_slice(&full.pixels);
    let mut p = Painter::new(&mut repainted, &mut gfx, 1.0);
    p.set_frost_key(Some(key()));
    p.clip(Rect::new(6.0, 0.0, 6.0, 32.0));
    draw_rows_with_background(
        &mut p,
        &mut font,
        &scheme,
        &m,
        area,
        &ink,
        None,
        view,
        std::slice::from_ref(&(0..1)),
        opacity,
    );
    assert!(
        repainted.pixels == full.pixels,
        "neighbour-only repaint changed the frame"
    );
    for y in 0..32 {
        for x in 6..12 {
            assert_eq!(pixel(&full, x, y), pixel(&blank, x, y), "({x}, {y})");
        }
    }
}

#[test]
fn full_opacity_and_no_active_backdrop_match_draw_screen() {
    for pal in [LIGHT, DARK] {
        let s = screen(" ╳W\x1b[41m \x1b[0m");
        let scheme = Scheme::from_palette(&pal);
        let view = View::new().focused(true);
        let old = paint(&s, &scheme, None, true, None, view);
        for (alpha, frosted) in [(1.0, true), (2.0, true), (f32::NAN, true), (0.0, false)] {
            let got = paint(
                &s,
                &scheme,
                Some(BackgroundOpacity::new(alpha).with_padding(true)),
                frosted,
                None,
                view,
            );
            assert_eq!(got.pixels, old.pixels, "{alpha}, key present: {frosted}");
        }
        let translucent = paint(
            &s,
            &scheme,
            Some(BackgroundOpacity::new(0.8).with_padding(true)),
            true,
            None,
            view,
        );
        assert_eq!(pixel(&translucent, 1, 1)[3], 204, "active control");
    }
}

#[test]
#[ignore = "writes a sheet for visual inspection"]
fn terminal_backdrop_sheet() {
    let s = screen("Mono ╳\r\n\x1b[44m RGB \x1b[0m");
    let sel = Selection::start(&s, Point::new(s.screen_start(), 1), SelectionKind::Block, 1);
    let out = std::path::Path::new("target/specimen");
    std::fs::create_dir_all(out).unwrap();
    for scale in [1.0, 1.5] {
        let mut bmp = Bitmap::new(
            (AREA.w * scale).round() as u32,
            (AREA.h * 2.0 * scale).round() as u32,
        );
        let mut gfx = Gfx::new();
        let mut font = CellFont::new();
        let mut p = Painter::new(&mut bmp, &mut gfx, scale);
        p.set_frost_key(Some(key()));
        for (i, pal) in [DARK, LIGHT].iter().enumerate() {
            let y = i as f32 * AREA.h;
            draw_screen_with_background(
                &mut p,
                &mut font,
                &Scheme::from_palette(pal),
                &Metrics::fixed(18.0, scale, W as f32, H as f32),
                Rect::new(0.0, y, AREA.w, AREA.h),
                &s,
                Some(&sel),
                View::new(),
                BackgroundOpacity::new(0.6).with_padding(true),
            );
        }
        assert!(bmp.pixels.as_chunks::<4>().0.iter().any(|px| px[3] < 254));
        for (i, px) in bmp.pixels.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let x = i % bmp.width as usize;
            let d = if (x / (16.0 * scale) as usize).is_multiple_of(2) {
                [255u32, 200, 240]
            } else {
                [10, 150, 70]
            };
            let keep = 255 - u32::from(px[3]);
            for ch in 0..3 {
                px[ch] += ((keep * d[ch] + 127) / 255) as u8;
            }
            px[3] = 255;
        }
        let suffix = (scale * 100.0) as u32;
        bmp.write_png(&out.join(format!("term-backdrop-{suffix}.png")))
            .unwrap();
    }
}
