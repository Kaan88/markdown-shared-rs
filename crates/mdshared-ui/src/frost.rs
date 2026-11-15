//! A system backdrop behind the chrome ("frosted" title row, bars and panes).
//!
//! The chrome is drawn exactly as without a backdrop: opaque, in its flat colours, its text
//! blended against them with subpixel masks. Nothing in the `Painter` changes. Two small steps
//! turn that into pixels the compositor can lay over a backdrop:
//!
//! 1. After a chrome region is drawn, [`mark`] writes [`MARK`] into the alpha byte of its pixels.
//!    Whatever is drawn over the chrome later with an opaque fill (a menu, a dialog card, a
//!    tooltip) writes alpha 255 again, as every opaque span of the `Painter` does, so those stay
//!    opaque to the pixel; blended drawing (their shadows, a dialog's dimming) leaves the mark.
//! 2. When the frame is copied to the window, [`key_out`] rewrites the marked pixels: the flat
//!    colour the chrome was drawn on (the [`Key`]) is taken out again and becomes transparency.
//!    A pixel of exactly the key colour becomes fully transparent; any other pixel gets the
//!    smallest alpha that still gives its colour over the key, and is written premultiplied.
//!
//! Laid over a backdrop `d`, a pixel `c` drawn on the key `k` then shows `c + keep * (d - k)`,
//! where `keep` is what the pixel lets through. Where the backdrop equals the key the result is
//! the frame as it was drawn, subpixel text included; the further the backdrop is from the key,
//! the more the text's edges take its colour. A `veil` keeps part of the key colour over the
//! backdrop for backdrops that stray far (Acrylic).
//!
//! The bitmap the `Painter` draws into is never changed beyond the alpha mark, so partial
//! repaints keep blending against the flat colours.

use crate::gfx::Rgb;
use crate::theme::Palette;
use mdshared_app::backdrop::Backdrop;
use mdshared_text::Bitmap;

/// The alpha byte of a chrome pixel that [`key_out`] may make transparent.
pub const MARK: u8 = 254;

/// The colour the chrome counts as "nothing drawn": the darkest of the title row, bar and pane
/// colours in a dark palette and the lightest in a light one, so the other two become a faint
/// lightening or darkening of the backdrop and keep their difference from it.
pub fn key_colour(pal: &Palette) -> Rgb {
    let sum = |c: &Rgb| c.iter().map(|&v| v as u32).sum::<u32>();
    let all = [pal.title, pal.bar, pal.pane];
    let pick = if pal.dark {
        all.iter().min_by_key(|c| sum(c))
    } else {
        all.iter().max_by_key(|c| sum(c))
    };
    *pick.unwrap_or(&pal.bar)
}

/// How much of the key colour stays over the backdrop, 0 to 255. Mica is nearly a flat colour
/// and needs none; Acrylic shows the colours behind the window and gets a veil so the chrome's
/// text keeps its contrast.
pub fn veil(backdrop: Backdrop) -> u8 {
    match backdrop {
        Backdrop::Off => 255,
        Backdrop::Mica => 0,
        Backdrop::Acrylic => 150,
    }
}

/// Writes [`MARK`] into the alpha byte of the device-px rectangle `(x0, y0, x1, y1)` of `bmp`
/// (clamped to it). Call it after drawing a chrome region and before drawing anything over it.
pub fn mark(bmp: &mut Bitmap, rect: (i32, i32, i32, i32)) {
    let (w, h) = (bmp.width as i32, bmp.height as i32);
    let (x0, x1) = (rect.0.clamp(0, w) as usize, rect.2.clamp(0, w) as usize);
    let (y0, y1) = (rect.1.clamp(0, h) as usize, rect.3.clamp(0, h) as usize);
    let stride = bmp.width as usize;
    for y in y0..y1 {
        let row = &mut bmp.pixels[(y * stride + x0) * 4..(y * stride + x1.max(x0)) * 4];
        for px in row.as_chunks_mut::<4>().0 {
            px[3] = MARK;
        }
    }
}

/// A key colour and a veil, with what [`key_out`] needs worked out once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Key {
    /// Red, green, blue.
    k: [u32; 3],
    /// The key colour as the low 24 bits of a pixel.
    packed: u32,
    /// What a pixel of the key colour becomes.
    empty: u32,
    /// 255 less the veil: how much of the backdrop a pixel of the key colour lets through.
    through: u32,
}

/// `n / 255`, rounded to nearest, for `n` up to 255 * 255.
fn div255(n: u32) -> u32 {
    (n + 127) / 255
}

impl Key {
    pub fn new(key: Rgb, veil: u8) -> Key {
        let k = [key[0] as u32, key[1] as u32, key[2] as u32];
        let through = 255 - veil as u32;
        let mut out = Key {
            k,
            packed: (k[0] << 16) | (k[1] << 8) | k[2],
            empty: 0,
            through,
        };
        out.empty = out.pixel(k, 0);
        out
    }

    /// The premultiplied pixel for the colour `c` (red, green, blue) that needs alpha `alpha`
    /// over the key.
    fn pixel(&self, c: [u32; 3], alpha: u32) -> u32 {
        // What of the backdrop still shows: the part the pixel does not cover, less the veil.
        let keep = (255 - alpha) * self.through / 255;
        let a = 255 - keep;
        let ch = |i: usize| c[i].saturating_sub(div255(keep * self.k[i])).min(a);
        (a << 24) | (ch(0) << 16) | (ch(1) << 8) | ch(2)
    }

    /// The pixel `px` (`0xAARRGGBB`, alpha ignored) taken off the key colour: premultiplied, with
    /// the smallest alpha that gives its colour over the key, raised by the veil.
    pub fn apply(&self, px: u32) -> u32 {
        let rgb = px & 0x00ff_ffff;
        if rgb == self.packed {
            return self.empty;
        }
        let c = [(rgb >> 16) & 0xff, (rgb >> 8) & 0xff, rgb & 0xff];
        // The alpha a channel needs: lighter than the key is white laid over it, darker is
        // black; both rounded up, so the colour it leaves stays within 0..=255.
        let need = |i: usize| {
            let (c, k) = (c[i], self.k[i]);
            if c > k {
                ((c - k) * 255).div_ceil(255 - k)
            } else if c < k {
                ((k - c) * 255).div_ceil(k)
            } else {
                0
            }
        };
        let alpha = need(0).max(need(1)).max(need(2)).min(255);
        self.pixel(c, alpha)
    }
}

/// Rewrites the marked pixels (alpha byte [`MARK`]) of the rectangle `(x0, y0, x1, y1)` of
/// `pixels`, rows of `stride` pixels of `0xAARRGGBB`: each becomes [`Key::apply`] of itself.
/// Unmarked pixels are left alone. The rectangle is clamped to the buffer.
pub fn key_out(key: &Key, pixels: &mut [u32], stride: usize, rect: (usize, usize, usize, usize)) {
    if stride == 0 {
        return;
    }
    let rows = pixels.len() / stride;
    let (x0, x1) = (rect.0.min(stride), rect.2.min(stride));
    let (y0, y1) = (rect.1.min(rows), rect.3.min(rows));
    if x0 >= x1 {
        return;
    }
    let mark = (MARK as u32) << 24;
    for y in y0..y1 {
        for px in &mut pixels[y * stride + x0..y * stride + x1] {
            if *px & 0xff00_0000 == mark {
                *px = key.apply(*px);
            }
        }
    }
}

/// The overlap of two device-px rectangles `(x0, y0, x1, y1)`, if any.
pub fn overlap(
    a: (usize, usize, usize, usize),
    b: (usize, usize, usize, usize),
) -> Option<(usize, usize, usize, usize)> {
    let r = (a.0.max(b.0), a.1.max(b.1), a.2.min(b.2), a.3.min(b.3));
    (r.0 < r.2 && r.1 < r.3).then_some(r)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{DARK, LIGHT};

    fn parts(px: u32) -> (u32, [u32; 3]) {
        (px >> 24, [(px >> 16) & 0xff, (px >> 8) & 0xff, px & 0xff])
    }

    /// A premultiplied pixel laid over the opaque colour `d`.
    fn over(px: u32, d: [u32; 3]) -> [u32; 3] {
        let (a, c) = parts(px);
        [0, 1, 2].map(|i| c[i] + div255((255 - a) * d[i]))
    }

    /// A spread of colours: every step of 15 on each channel, plus the palettes' own.
    fn colours() -> Vec<[u32; 3]> {
        let mut out = Vec::new();
        for r in (0..=255).step_by(15) {
            for g in (0..=255).step_by(15) {
                for b in (0..=255).step_by(15) {
                    out.push([r, g, b]);
                }
            }
        }
        for p in [DARK, LIGHT] {
            for c in [
                p.title, p.bar, p.pane, p.text, p.muted, p.hover, p.on, p.line, p.accent,
            ] {
                out.push([c[0] as u32, c[1] as u32, c[2] as u32]);
            }
        }
        out
    }

    fn pack(c: [u32; 3]) -> u32 {
        ((MARK as u32) << 24) | (c[0] << 16) | (c[1] << 8) | c[2]
    }

    #[test]
    fn the_key_colour_becomes_fully_transparent_without_a_veil() {
        for pal in [DARK, LIGHT] {
            let k = key_colour(&pal);
            let key = Key::new(k, 0);
            assert_eq!(key.apply(pack([k[0] as u32, k[1] as u32, k[2] as u32])), 0);
        }
    }

    #[test]
    fn over_the_key_colour_every_pixel_is_what_was_drawn() {
        for k in [
            key_colour(&DARK),
            key_colour(&LIGHT),
            [0, 0, 0],
            [255, 255, 255],
            [0, 128, 255],
        ] {
            for veil in [0u8, 1, 96, 150, 254, 255] {
                let key = Key::new(k, veil);
                let kk = [k[0] as u32, k[1] as u32, k[2] as u32];
                for c in colours() {
                    let got = over(key.apply(pack(c)), kk);
                    for i in 0..3 {
                        assert!(
                            got[i].abs_diff(c[i]) <= 1,
                            "key {k:?} veil {veil} colour {c:?} gave {got:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn pixels_are_valid_premultiplied_colours() {
        for k in [
            key_colour(&DARK),
            key_colour(&LIGHT),
            [0, 0, 0],
            [255, 255, 255],
            [7, 200, 90],
        ] {
            for veil in [0u8, 150, 255] {
                let key = Key::new(k, veil);
                for c in colours() {
                    let (a, p) = parts(key.apply(pack(c)));
                    assert!(p.iter().all(|&v| v <= a), "{c:?} on {k:?}: {p:?} > {a}");
                    assert!(a >= veil as u32, "the veil is the least alpha");
                }
            }
        }
    }

    #[test]
    fn a_full_veil_is_opaque_and_unchanged() {
        let key = Key::new(key_colour(&DARK), 255);
        for c in colours() {
            assert_eq!(key.apply(pack(c)), 0xff00_0000 | (pack(c) & 0x00ff_ffff));
        }
    }

    #[test]
    fn text_colours_stay_nearly_opaque_and_fills_near_the_key_nearly_clear() {
        for pal in [DARK, LIGHT] {
            let key = Key::new(key_colour(&pal), 0);
            let alpha = |c: Rgb| key.apply(pack([c[0] as u32, c[1] as u32, c[2] as u32])) >> 24;
            assert!(alpha(pal.text) > 200, "text {}", alpha(pal.text));
            for c in [pal.title, pal.bar, pal.pane] {
                assert!(alpha(c) < 16, "chrome fill {c:?}: {}", alpha(c));
            }
            assert!(alpha(pal.hover) < 64, "hover {}", alpha(pal.hover));
        }
    }

    #[test]
    fn over_another_backdrop_a_pixel_moves_by_what_it_lets_through() {
        // c + keep * (d - k), for a backdrop d lighter than the key.
        let k = key_colour(&DARK);
        let kk = [k[0] as u32, k[1] as u32, k[2] as u32];
        let d = [90, 60, 120];
        let key = Key::new(k, 0);
        for c in colours() {
            let px = key.apply(pack(c));
            let keep = 255 - (px >> 24);
            let got = over(px, d);
            for i in 0..3 {
                let want = c[i] as i32 + div255(keep * d[i]) as i32 - div255(keep * kk[i]) as i32;
                assert!(
                    (got[i] as i32 - want).abs() <= 1,
                    "{c:?}: {got:?} vs {want}"
                );
            }
        }
    }

    #[test]
    fn only_marked_pixels_inside_the_rectangle_change() {
        let key = Key::new([0x26, 0x26, 0x26], 0);
        let marked = pack([0x26, 0x26, 0x26]);
        let opaque = 0xff26_2626;
        let mut px = vec![marked; 16];
        px[5] = opaque;
        key_out(&key, &mut px, 4, (1, 1, 3, 3));
        for (i, &p) in px.iter().enumerate() {
            let (x, y) = (i % 4, i / 4);
            let inside = (1..3).contains(&x) && (1..3).contains(&y);
            let want = match (inside, i == 5) {
                (true, true) => opaque,
                (true, false) => 0,
                (false, _) => marked,
            };
            assert_eq!(p, want, "pixel {i}");
        }
        // A rectangle past the buffer is clamped, an empty one does nothing.
        let mut px = vec![marked; 16];
        key_out(&key, &mut px, 4, (2, 2, 99, 99));
        assert_eq!(px.iter().filter(|&&p| p == 0).count(), 4);
        key_out(&key, &mut px, 4, (3, 0, 1, 4));
        key_out(&key, &mut px, 0, (0, 0, 4, 4));
        assert_eq!(px.iter().filter(|&&p| p == 0).count(), 4);
    }

    #[test]
    fn mark_stamps_the_alpha_byte_and_an_opaque_fill_clears_it() {
        let mut bmp = Bitmap::new(8, 6);
        bmp.clear([10, 20, 30]);
        mark(&mut bmp, (2, 1, 6, 4));
        mark(&mut bmp, (-5, -5, 1, 1));
        mark(&mut bmp, (7, 5, 99, 99));
        for y in 0..6 {
            for x in 0..8 {
                let a = bmp.pixels[(y * 8 + x) * 4 + 3];
                let inside = ((2..6).contains(&x) && (1..4).contains(&y))
                    || (x, y) == (0, 0)
                    || (x, y) == (7, 5);
                assert_eq!(a == MARK, inside, "({x},{y}) alpha {a}");
                assert_eq!(
                    &bmp.pixels[(y * 8 + x) * 4..(y * 8 + x) * 4 + 3],
                    &[30, 20, 10]
                );
            }
        }
        // What the Painter draws over the chrome: an opaque box clears the mark, a blended one
        // (a shadow, a dialog's dimming) keeps it.
        let mut gfx = crate::gfx::Gfx::new();
        let mut p = crate::gfx::Painter::new(&mut bmp, &mut gfx, 1.0);
        p.fill(crate::gfx::Rect::new(2.0, 1.0, 2.0, 1.0), [200, 200, 200]);
        p.fill_a(crate::gfx::Rect::new(4.0, 1.0, 2.0, 1.0), [0, 0, 0], 0.5);
        assert_eq!(bmp.pixels[(8 + 2) * 4 + 3], 255);
        assert_eq!(bmp.pixels[(8 + 3) * 4 + 3], 255);
        assert_eq!(bmp.pixels[(8 + 4) * 4 + 3], MARK);
        assert_eq!(bmp.pixels[(8 + 5) * 4 + 3], MARK);
    }

    #[test]
    fn the_key_is_the_darkest_chrome_colour_in_dark_and_the_lightest_in_light() {
        assert_eq!(key_colour(&DARK), DARK.pane);
        assert_eq!(key_colour(&LIGHT), LIGHT.bar);
        assert_eq!(veil(Backdrop::Off), 255);
        assert_eq!(veil(Backdrop::Mica), 0);
    }

    #[test]
    fn overlaps() {
        assert_eq!(
            overlap((0, 0, 10, 10), (5, 5, 20, 20)),
            Some((5, 5, 10, 10))
        );
        assert_eq!(overlap((0, 0, 10, 10), (10, 0, 20, 10)), None);
    }
}
