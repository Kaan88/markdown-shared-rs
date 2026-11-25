//! Cover art: the colours a picture suggests, and the picture drawn into a rectangle.
//!
//! [`colors`] reads a picture and answers what a music player wants to know about it: the
//! average colour of its largest area ([`CoverColors::background`]), a text colour that stands
//! out against that background, and an accent taken from the most common colourful area. The
//! answer is the same for the same pixels on every run, so a player can compare it, store it or
//! draw its parts in any order.
//!
//! [`draw_cover`] draws a decoded picture, or a music note when there is none, into a rectangle
//! the caller gives.
//!
//! The caller owns the picture: decoding it (see [`mdshared_text::decode_png_rgba`]), keeping
//! the buffer alive, and caching the [`CoverColors`] of each picture. The colours are meant to
//! be computed once when a picture arrives, not once per frame.
//!
//! Deliberately not here: decoding, caching, blurred backdrops behind the picture, and an
//! outline around it (the painter has no call that strokes a rounded rectangle without filling
//! it, and filling would paint over the picture).

use crate::gfx::{Painter, Rect, Rgb, mix};
use crate::glyph;
use crate::theme::Palette;

/// The side of the sampling grid: at most this many pixels across and down are read.
const SAMPLE: u32 = 64;

/// How many colour buckets there are: the top four bits of red, green and blue.
const BUCKETS: usize = 4096;

/// The two colours text may take over a picture: white, or near-black rather than pure black,
/// which is softer on a dark cover.
const WHITE: Rgb = [255, 255, 255];
const NEAR_BLACK: Rgb = [20, 20, 20];
/// Where neither of the two reaches 4.5 (a narrow band of luminance around 0.195), text goes
/// to pure black: (20, 20, 20) is too light there to be read at that contrast.
const BLACK: Rgb = [0, 0, 0];

/// The relative luminance of a colour, from 0 for black to 1 for white, as WCAG 2 defines it:
/// each channel is made linear (the sRGB curve), then the channels are weighed by how much the
/// eye sees them.
///
/// The edge cases are the ends: black is 0, white is 1, and every colour between is between
/// them. The value is never NaN for bytes.
pub fn luminance(c: Rgb) -> f32 {
    fn linear(v: u8) -> f32 {
        let v = v as f32 / 255.0;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    }
    0.2126 * linear(c[0]) + 0.7152 * linear(c[1]) + 0.0722 * linear(c[2])
}

/// How far apart two colours are for reading, from 1 for the same colour to 21 for white on
/// black, as WCAG 2 defines it: the lighter luminance plus a little over the darker plus the
/// same little. The order of the arguments does not matter.
///
/// The value is never NaN and never below 1 for bytes; 4.5 is the usual least for body text and
/// 3.0 for large text.
pub fn contrast(a: Rgb, b: Rgb) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    let (lighter, darker) = if la >= lb { (la, lb) } else { (lb, la) };
    (lighter + 0.05) / (darker + 0.05)
}

/// The colours a picture suggests: what to paint behind it, what to write on that, and what to
/// mark with.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CoverColors {
    /// The average colour of the picture's largest area.
    pub background: Rgb,
    /// White or near-black, whichever reads better on [`CoverColors::background`]; pure black in
    /// the narrow band where neither reaches 4.5. Always at least 4.5 from the background.
    pub text: Rgb,
    /// A colourful, contrasting average taken from the picture; when the picture has none, a
    /// mix of the background and the text.
    pub accent: Rgb,
}

/// The colours a straight RGBA picture suggests, or `None` when there is nothing to read.
///
/// The answer is `None` when `width` or `height` is zero, when `rgba` holds fewer than
/// `width * height * 4` bytes, or when every sampled pixel is more than half transparent. The
/// bytes are not examined beyond that count, so extra bytes are ignored.
///
/// At most 64 by 64 pixels are read, evenly spread over the picture whatever its size, and
/// pixels with an alpha below 128 are skipped. Each remaining pixel falls in a bucket by the
/// top four bits of its red, green and blue, and every bucket keeps its count and the sum of
/// its channels. The background is the average of the fullest bucket, the lowest bucket on a
/// tie, so the same pixels always give the same answer.
///
/// The text is white or near-black, whichever contrasts more with the background. In the
/// narrow band of backgrounds where neither reaches the 4.5 that body text wants, it is pure
/// black instead, so the result always reaches 4.5. The accent is the average of the fullest
/// bucket whose colour is saturated (the HSV saturation, at least 0.3) and stands at least 3.0
/// from the background; when no bucket qualifies it is [`mix`]`(background, text, 0.6)`.
pub fn colors(rgba: &[u8], width: u32, height: u32) -> Option<CoverColors> {
    if width == 0 || height == 0 {
        return None;
    }
    let needed = (width as usize)
        .checked_mul(height as usize)?
        .checked_mul(4)?;
    if rgba.len() < needed {
        return None;
    }

    let cols = width.min(SAMPLE);
    let rows = height.min(SAMPLE);
    let mut counts = vec![0u64; BUCKETS];
    let mut sums = vec![[0u64; 3]; BUCKETS];
    let mut seen = 0u64;
    for row in 0..rows {
        let y = (row as u64 * height as u64 / rows as u64) as usize;
        for col in 0..cols {
            let x = (col as u64 * width as u64 / cols as u64) as usize;
            let at = (y * width as usize + x) * 4;
            let px = &rgba[at..at + 4];
            if px[3] < 128 {
                continue;
            }
            let bucket =
                ((px[0] as usize >> 4) << 8) | ((px[1] as usize >> 4) << 4) | (px[2] as usize >> 4);
            counts[bucket] += 1;
            for (sum, &v) in sums[bucket].iter_mut().zip(px) {
                *sum += v as u64;
            }
            seen += 1;
        }
    }
    if seen == 0 {
        return None;
    }

    let average = |bucket: usize| -> Rgb {
        let n = counts[bucket];
        let ch = |c: usize| (sums[bucket][c] / n) as u8;
        [ch(0), ch(1), ch(2)]
    };

    // The fullest bucket, and the lowest index when two are as full.
    let mut background = 0;
    for (bucket, &n) in counts.iter().enumerate().skip(1) {
        if n > counts[background] {
            background = bucket;
        }
    }
    let background = average(background);
    let mut text = if contrast(WHITE, background) >= contrast(NEAR_BLACK, background) {
        WHITE
    } else {
        NEAR_BLACK
    };
    if contrast(text, background) < 4.5 {
        text = BLACK;
    }

    let mut accent = None;
    let mut fullest = 0;
    for (bucket, &n) in counts.iter().enumerate() {
        if n == 0 || n <= fullest {
            continue;
        }
        let c = average(bucket);
        let max = c[0].max(c[1]).max(c[2]);
        let min = c[0].min(c[1]).min(c[2]);
        let saturation = if max == 0 {
            0.0
        } else {
            (max - min) as f32 / max as f32
        };
        if saturation >= 0.3 && contrast(c, background) >= 3.0 {
            fullest = n;
            accent = Some(c);
        }
    }
    let accent = accent.unwrap_or_else(|| mix(background, text, 0.6));

    Some(CoverColors {
        background,
        text,
        accent,
    })
}

/// Draws cover art into `r`: the picture scaled to fit without changing its shape, or a music
/// note when there is no picture.
///
/// `picture` is straight (not premultiplied) RGBA, `width` by `height` pixels, top down, as
/// [`Painter::image`] takes it. The largest rectangle of the picture's shape is centred in `r`
/// and the picture fills it; when that leaves part of `r` uncovered, `r` is first filled with
/// [`Palette::field`] at `radius`, so the picture sits in a band of the field colour. `radius`
/// rounds `r`'s corners: the picture's own corners are rounded only when it reaches them (a
/// band narrower than the radius), and are square in the middle of a letterbox.
///
/// A picture with a zero side, or a buffer shorter than `width * height * 4`, counts as no
/// picture: `r` is filled with the field colour and [`glyph::MUSIC`] is drawn centred in it at
/// 40% of the shorter side, in [`Palette::faint`].
///
/// A rectangle with a negative, zero or non-finite side draws nothing and leaves the bitmap
/// alone. A non-finite radius is treated as zero.
pub fn draw_cover(
    p: &mut Painter,
    pal: &Palette,
    picture: Option<(&[u8], u32, u32)>,
    r: Rect,
    radius: f32,
) {
    if !(r.x.is_finite() && r.y.is_finite() && r.w.is_finite() && r.h.is_finite())
        || r.w <= 0.0
        || r.h <= 0.0
    {
        return;
    }
    let radius = if radius.is_finite() {
        radius.max(0.0)
    } else {
        0.0
    };

    let picture = picture.filter(|&(rgba, width, height)| {
        width > 0
            && height > 0
            && (width as usize)
                .checked_mul(height as usize)
                .and_then(|n| n.checked_mul(4))
                .is_some_and(|n| rgba.len() >= n)
    });

    match picture {
        Some((rgba, width, height)) => {
            let (w, h) = (width as f32, height as f32);
            let scale = (r.w / w).min(r.h / h);
            let band = Rect::new(
                r.cx() - w * scale / 2.0,
                r.cy() - h * scale / 2.0,
                w * scale,
                h * scale,
            );
            // Half a logical pixel of slack, so rounding cannot fake a letterbox.
            if band.w < r.w - 0.5 || band.h < r.h - 0.5 {
                p.rbox(r, radius, pal.field);
            }
            let gap = (r.w - band.w).max(r.h - band.h) / 2.0;
            let corners = if gap < radius { radius } else { 0.0 };
            p.image(rgba, width, height, band, corners, 1.0);
        }
        None => {
            p.rbox(r, radius, pal.field);
            let side = r.w.min(r.h) * 0.4;
            p.icon(glyph::MUSIC, side, r.cx(), r.cy(), pal.faint);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gfx::Gfx;
    use crate::theme::{DARK, LIGHT};
    use mdshared_text::Bitmap;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-5
    }

    fn at(bmp: &Bitmap, x: i32, y: i32) -> Rgb {
        let o = (y as usize * bmp.width as usize + x as usize) * 4;
        [bmp.pixels[o + 2], bmp.pixels[o + 1], bmp.pixels[o]]
    }

    fn shot(pal: &Palette, w: u32, h: u32, draw: impl FnOnce(&mut Painter<'_>)) -> Bitmap {
        let mut bmp = Bitmap::new(w, h);
        let mut gfx = Gfx::new();
        {
            let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
            p.fill(Rect::new(0.0, 0.0, w as f32, h as f32), pal.pane);
            draw(&mut p);
        }
        bmp
    }

    fn solid(w: u32, h: u32, c: [u8; 4]) -> Vec<u8> {
        c.repeat((w * h) as usize)
    }

    #[test]
    fn black_has_luminance_zero_and_white_has_one() {
        assert_eq!(luminance([0, 0, 0]), 0.0);
        assert!(close(luminance([255, 255, 255]), 1.0));
        let grey = luminance([128, 128, 128]);
        assert!(grey > 0.2 && grey < 0.22, "mid grey is {grey}");
    }

    #[test]
    fn white_on_black_is_the_greatest_contrast_and_contrast_is_symmetric() {
        let most = contrast([255, 255, 255], [0, 0, 0]);
        assert!(close(most, 21.0), "white on black is {most}");
        assert!(close(most, contrast([0, 0, 0], [255, 255, 255])));
        assert_eq!(contrast([90, 140, 200], [90, 140, 200]), 1.0);
        assert!(contrast([10, 10, 10], [240, 240, 240]) > 12.0);
    }

    #[test]
    fn a_picture_of_one_colour_gives_that_colour_as_the_background() {
        let rgba = solid(8, 8, [37, 120, 200, 255]);
        let c = colors(&rgba, 8, 8).expect("one colour has colours");
        assert_eq!(c.background, [37, 120, 200]);
    }

    #[test]
    fn a_picture_that_is_seventy_percent_red_gives_red_behind_blue() {
        let mut rgba = Vec::new();
        for _ in 0..7 {
            rgba.extend(solid(10, 1, [255, 0, 0, 255]));
        }
        for _ in 0..3 {
            rgba.extend(solid(10, 1, [0, 0, 153, 255]));
        }
        let c = colors(&rgba, 10, 10).expect("a picture in two colours");
        assert_eq!(c.background, [255, 0, 0]);
        assert_eq!(c.accent, [0, 0, 153]);
    }

    #[test]
    fn a_grey_picture_gets_an_accent_mixed_from_its_own_colours() {
        let rgba = solid(5, 5, [128, 128, 128, 255]);
        let c = colors(&rgba, 5, 5).expect("grey has colours");
        assert_eq!(c.background, [128, 128, 128]);
        assert_eq!(c.text, NEAR_BLACK);
        assert_eq!(c.accent, mix(c.background, c.text, 0.6));
    }

    #[test]
    fn the_text_colour_contrasts_with_the_background_by_at_least_four_and_a_half() {
        for r in [0u8, 51, 102, 153, 204, 255] {
            for g in [0u8, 51, 102, 153, 204, 255] {
                for b in [0u8, 51, 102, 153, 204, 255] {
                    let rgba = [r, g, b, 255];
                    let c = colors(&rgba, 1, 1).expect("one opaque pixel");
                    let got = contrast(c.text, c.background);
                    assert!(got >= 4.5, "{:?} on {:?} gives {got}", c.text, c.background);
                }
            }
        }
    }

    #[test]
    fn transparent_pixels_are_left_out_of_the_colours() {
        let rgba = [
            0, 255, 0, 0, // invisible green
            0, 0, 255, 127, // too faint to count
            255, 0, 0, 255,
        ];
        let c = colors(&rgba, 3, 1).expect("the red pixel is seen");
        assert_eq!(c.background, [255, 0, 0]);
    }

    #[test]
    fn an_empty_short_or_wholly_transparent_picture_has_no_colours() {
        assert_eq!(colors(&[], 0, 4), None);
        assert_eq!(colors(&[], 4, 0), None);
        assert_eq!(colors(&[1, 2, 3, 255], 2, 2), None);
        assert_eq!(colors(&[1, 2, 3, 127, 4, 5, 6, 0], 2, 1), None);
    }

    #[test]
    fn the_same_picture_gives_the_same_colours_each_time() {
        let mut rgba = Vec::new();
        for y in 0..24u32 {
            for x in 0..24u32 {
                rgba.extend([(x * 10) as u8, (y * 9) as u8, (x * y) as u8, 255]);
            }
        }
        let once = colors(&rgba, 24, 24).expect("a gradient");
        let twice = colors(&rgba, 24, 24).expect("a gradient");
        assert_eq!(once, twice);
    }

    #[test]
    fn draw_cover_without_a_picture_fills_the_field_and_draws_the_note() {
        for pal in [&LIGHT, &DARK] {
            let bmp = shot(pal, 120, 120, |p| {
                draw_cover(p, pal, None, Rect::new(10.0, 10.0, 100.0, 100.0), 6.0)
            });
            assert_eq!(at(&bmp, 20, 20), pal.field);
            // The left stem of the note, at grid x = 6 of its 40 px square.
            assert_eq!(at(&bmp, 55, 60), pal.faint);
        }
    }

    #[test]
    fn a_wide_picture_keeps_its_shape_and_leaves_field_bands_above_and_below() {
        let rgba = solid(4, 1, [200, 60, 60, 255]);
        for pal in [&LIGHT, &DARK] {
            let bmp = shot(pal, 120, 120, |p| {
                draw_cover(
                    p,
                    pal,
                    Some((&rgba, 4, 1)),
                    Rect::new(10.0, 10.0, 100.0, 100.0),
                    0.0,
                )
            });
            assert_eq!(at(&bmp, 60, 20), pal.field);
            assert_eq!(at(&bmp, 60, 100), pal.field);
            assert_eq!(at(&bmp, 60, 60), [200, 60, 60]);
        }
    }

    #[test]
    fn a_short_picture_draws_the_placeholder() {
        for pal in [&LIGHT, &DARK] {
            let r = Rect::new(10.0, 10.0, 100.0, 100.0);
            let short = [0u8, 0, 0, 255];
            let drawn = shot(pal, 120, 120, |p| {
                draw_cover(p, pal, Some((&short, 4, 4)), r, 6.0)
            });
            let blank = shot(pal, 120, 120, |p| draw_cover(p, pal, None, r, 6.0));
            assert_eq!(drawn.pixels, blank.pixels);
        }
    }

    #[test]
    fn an_empty_or_endless_rectangle_draws_nothing() {
        let pal = &LIGHT;
        let before = shot(pal, 60, 60, |_| {});
        for r in [
            Rect::new(0.0, 0.0, 0.0, 30.0),
            Rect::new(0.0, 0.0, 30.0, -4.0),
            Rect::new(0.0, f32::NAN, 30.0, 30.0),
            Rect::new(0.0, 0.0, f32::INFINITY, 30.0),
        ] {
            let after = shot(pal, 60, 60, |p| draw_cover(p, pal, None, r, 4.0));
            assert_eq!(after.pixels, before.pixels, "{r:?} was drawn");
        }
    }

    fn picture(w: u32, h: u32, f: impl Fn(u32, u32) -> [u8; 4]) -> Vec<u8> {
        let mut out = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            for x in 0..w {
                out.extend(f(x, y));
            }
        }
        out
    }

    /// The sheet a person looks at: the placeholder and two synthetic pictures, each with the
    /// colours it suggests, light and dark. `cargo test -p mdshared-ui cover_sheet --
    /// --ignored`, then open target/specimen/cover.png.
    #[test]
    #[ignore]
    fn cover_sheet() {
        let mut bmp = Bitmap::new(560, 400);
        let mut gfx = Gfx::new();
        let square = picture(64, 64, |x, y| {
            if y < 40 {
                if x < 44 {
                    [214, 93, 42, 255]
                } else {
                    [236, 214, 168, 255]
                }
            } else if x < 24 {
                [32, 58, 74, 255]
            } else {
                [25, 25, 28, 255]
            }
        });
        let wide = picture(96, 48, |x, _| {
            if x < 60 {
                [180, 40, 110, 255]
            } else {
                [247, 214, 60, 255]
            }
        });
        {
            let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
            for (row, pal) in [&LIGHT, &DARK].into_iter().enumerate() {
                let top = row as f32 * 200.0;
                p.fill(Rect::new(0.0, top, 560.0, 200.0), pal.pane);
                let pics: [Option<(&[u8], u32, u32)>; 3] =
                    [None, Some((&square, 64, 64)), Some((&wide, 96, 48))];
                for (col, pic) in pics.into_iter().enumerate() {
                    let x = 20.0 + col as f32 * 185.0;
                    let r = Rect::new(x, top + 12.0, 140.0, 140.0);
                    draw_cover(&mut p, pal, pic, r, 10.0);
                    let swatches = match pic {
                        Some((rgba, w, h)) => {
                            let c = colors(rgba, w, h).expect("the pictures have colours");
                            [c.background, c.text, c.accent]
                        }
                        None => [pal.field, pal.faint, pal.line],
                    };
                    for (k, c) in swatches.into_iter().enumerate() {
                        p.rbox(
                            Rect::new(x + k as f32 * 30.0, top + 160.0, 24.0, 24.0),
                            4.0,
                            c,
                        );
                    }
                }
            }
        }
        let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/specimen");
        std::fs::create_dir_all(&out).expect("the specimen folder");
        bmp.write_png(&out.join("cover.png")).expect("the sheet");
    }
}
