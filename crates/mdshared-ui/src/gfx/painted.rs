//! Pictures through the painter: one squeezed into a rectangle on every call, and pixels
//! already squeezed to a device size laid down as they are. [`Painter::image`] is the first;
//! [`Painter::blit_rgba`] and [`prepare_image`] serve [`crate::picture`], which keeps a
//! picture at its drawn size so the squeezing happens once.
//!
//! The scaler is one for both: a picture shrunk is the average of the source under each
//! destination pixel, weighed in premultiplied alpha; enlarged, neighbours are mixed. The
//! rectangle, the corners, the clip, the offset and the alpha are the painter's, as for every
//! other shape.

use super::shapes::corner_cover;
use super::{Painter, Rect};

/// Where each destination cell of an axis reads from: for cell `i` of `n` across a source `len`
/// long, the first source index and the weights of it and the ones after it, summing to one.
/// Shrinking, a cell is the average of the source under it (each source pixel weighted by its
/// overlap), so fine detail does not turn to moiré; enlarging, it is two neighbours mixed.
struct Taps {
    /// Per cell: the first source index, and where its weights start and how many there are.
    cells: Vec<(usize, usize, usize)>,
    weights: Vec<f32>,
}

impl Taps {
    fn new(cells: std::ops::Range<i32>, n: i32, len: u32) -> Taps {
        let mut taps = Taps {
            cells: Vec::with_capacity(cells.len()),
            weights: Vec::new(),
        };
        let ratio = len as f64 / n as f64;
        for i in cells {
            let start = taps.weights.len();
            let first;
            if ratio > 1.0 {
                let (a, b) = (i as f64 * ratio, ((i + 1) as f64 * ratio).min(len as f64));
                first = (a.floor() as usize).min(len as usize - 1);
                let mut s = first;
                while (s as f64) < b && s < len as usize {
                    let overlap = b.min(s as f64 + 1.0) - a.max(s as f64);
                    taps.weights.push((overlap / (b - a)).max(0.0) as f32);
                    s += 1;
                }
                if taps.weights.len() == start {
                    taps.weights.push(1.0);
                }
            } else {
                let at = ((i as f64 + 0.5) * ratio - 0.5).clamp(0.0, len as f64 - 1.0);
                first = at.floor() as usize;
                let f = (at - first as f64) as f32;
                if first + 1 < len as usize && f > 0.0 {
                    taps.weights.extend([1.0 - f, f]);
                } else {
                    taps.weights.push(1.0);
                }
            }
            taps.cells.push((first, start, taps.weights.len() - start));
        }
        taps
    }

    /// The source indices any cell reads: the first, and one past the last.
    fn reach(&self) -> (usize, usize) {
        let lo = self.cells.iter().map(|c| c.0).min().unwrap_or(0);
        let hi = self.cells.iter().map(|c| c.0 + c.2).max().unwrap_or(0);
        (lo, hi)
    }
}

/// A picture squeezed to the visible part of a device rectangle, in premultiplied alpha.
struct Scaled {
    /// The visible columns of each source row, `vis_w` wide.
    across: Vec<[f32; 4]>,
    /// The first source row `across` holds.
    row_lo: usize,
    /// How many visible columns there are.
    vis_w: usize,
    /// Where each visible destination row reads from.
    rows: Taps,
}

impl Scaled {
    /// Squeezes the visible part of `rgba` into the device rectangle `(x0, y0)-(x1, y1)`:
    /// `None` when the picture is empty, the rectangle is empty, the buffer is short, or the
    /// clipped part is empty.
    #[allow(clippy::too_many_arguments)] // one rectangle, its source and its visible part
    fn take(
        rgba: &[u8],
        width: u32,
        height: u32,
        x0: i32,
        y0: i32,
        x1: i32,
        y1: i32,
        (vx0, vy0, vx1, vy1): (i32, i32, i32, i32),
    ) -> Option<Scaled> {
        let pixels = (width as usize).checked_mul(height as usize);
        let enough = pixels
            .and_then(|n| n.checked_mul(4))
            .is_some_and(|n| rgba.len() >= n);
        if width == 0 || height == 0 || !enough {
            return None;
        }
        // Sides in 64 bits: a hand-made rectangle may reach from i32::MIN to i32::MAX, and
        // the difference of two i32 values would overflow. Beyond the i32 range of a side
        // nothing is drawn, so no index stored in an i32 overflows.
        const MAX_SIDE: i64 = i32::MAX as i64;
        let (dw, dh) = (x1 as i64 - x0 as i64, y1 as i64 - y0 as i64);
        let (vw, vh) = (vx1 as i64 - vx0 as i64, vy1 as i64 - vy0 as i64);
        if dw <= 0 || dh <= 0 || dw > MAX_SIDE || dh > MAX_SIDE || vw <= 0 || vh <= 0 {
            return None;
        }
        let cols = Taps::new(
            (vx0 as i64 - x0 as i64) as i32..(vx1 as i64 - x0 as i64) as i32,
            dw as i32,
            width,
        );
        let rows = Taps::new(
            (vy0 as i64 - y0 as i64) as i32..(vy1 as i64 - y0 as i64) as i32,
            dh as i32,
            height,
        );
        // Across first: the source rows the visible rows read, each squeezed to the visible
        // columns, in premultiplied alpha.
        let (row_lo, row_hi) = rows.reach();
        let vis_w = vw as usize;
        let stride = width as usize * 4;
        let mut across = vec![[0f32; 4]; vis_w * (row_hi - row_lo)];
        for (k, sy) in (row_lo..row_hi).enumerate() {
            let line = &rgba[sy * stride..(sy + 1) * stride];
            let out = &mut across[k * vis_w..(k + 1) * vis_w];
            for (px, &(first, start, n)) in out.iter_mut().zip(&cols.cells) {
                let mut sum = [0f32; 4];
                for (j, w) in cols.weights[start..start + n].iter().enumerate() {
                    let s = &line[(first + j) * 4..(first + j) * 4 + 4];
                    let a = s[3] as f32 / 255.0 * w;
                    sum[0] += s[0] as f32 * a;
                    sum[1] += s[1] as f32 * a;
                    sum[2] += s[2] as f32 * a;
                    sum[3] += a;
                }
                *px = sum;
            }
        }
        Some(Scaled {
            across,
            row_lo,
            vis_w,
            rows,
        })
    }

    /// Visible destination cell `(i, r)`, premultiplied: the vertical pass.
    fn pixel(&self, r: usize, i: usize) -> [f32; 4] {
        let (first, start, n) = self.rows.cells[r];
        let mut px = [0f32; 4];
        for (j, w) in self.rows.weights[start..start + n].iter().enumerate() {
            let s = self.across[(first - self.row_lo + j) * self.vis_w + i];
            for (p, s) in px.iter_mut().zip(s) {
                *p += s * w;
            }
        }
        px
    }
}

/// One premultiplied source pixel `px` laid over the BGRA destination pixel `d`, with
/// `cover` of the source pixel present.
#[inline]
fn blend_pixel(d: &mut [u8], px: [f32; 4], cover: f32) {
    let keep = 1.0 - px[3].min(1.0) * cover;
    // RGBA sums, BGRA framebuffer.
    for ch in 0..3 {
        let v = px[2 - ch] * cover + d[ch] as f32 * keep;
        d[ch] = (v + 0.5).min(255.0) as u8;
    }
}

/// A picture squeezed to `w` by `h` device pixels, as straight RGBA with the rounded corners
/// of `radius` (device px) already folded into alpha: what a [`Picture`](crate::picture) draws
/// again and again.
///
/// `None` when the picture is empty, a side is zero, or the buffer is short. The radius is cut
/// to half the smaller side as the painter cuts it, so an infinite or NaN radius rounds fully.
///
/// The work is not bounded by `w * h` alone: it allocates `w` by the number of source rows the
/// destination reads, 16 bytes a sample, before the `w * h * 4` bytes of the result. A tall
/// source scaled down can therefore take hundreds of MiB even when the destination is small,
/// and the caller keeps both sizes small enough to be worth that.
pub(crate) fn prepare_image(
    rgba: &[u8],
    width: u32,
    height: u32,
    w: u32,
    h: u32,
    radius: f32,
) -> Option<Vec<u8>> {
    let (wi, hi) = (i32::try_from(w).ok()?, i32::try_from(h).ok()?);
    let scaled = Scaled::take(rgba, width, height, 0, 0, wi, hi, (0, 0, wi, hi))?;
    let rad = radius.min(w as f32 / 2.0).min(h as f32 / 2.0).max(0.0);
    let mut out = Vec::with_capacity((w as usize).checked_mul(h as usize)?.checked_mul(4)?);
    for r in 0..h as usize {
        for i in 0..w as usize {
            let px = scaled.pixel(r, i);
            let cover = corner_cover(i as i32, r as i32, (0, 0, wi, hi), rad);
            let a = px[3].min(1.0) * cover;
            let straight = |v: f32| {
                if px[3] > 0.0 {
                    (v / px[3]).round().clamp(0.0, 255.0) as u8
                } else {
                    0
                }
            };
            out.extend_from_slice(&[
                straight(px[0]),
                straight(px[1]),
                straight(px[2]),
                (a * 255.0).round().clamp(0.0, 255.0) as u8,
            ]);
        }
    }
    Some(out)
}

impl Painter<'_> {
    /// Draws a picture scaled into `dst`: `rgba` is `width` by `height` pixels of straight
    /// (not premultiplied) RGBA, top down, as [`mdshared_text::decode_png_rgba`] gives them.
    /// The picture fills the rectangle exactly (the caller keeps the aspect ratio by the
    /// rectangle it gives), its corners rounded by `radius`, the whole blended at `alpha` on
    /// top of the picture's own transparency. Only the pixels inside the clip are computed.
    ///
    /// Shrunk, each pixel is the average of the picture under it, weighed in premultiplied
    /// alpha so that a cut-out gets no dark fringe; enlarged, neighbours are mixed. A buffer
    /// shorter than `width * height * 4`, an empty picture or an empty rectangle draws nothing.
    #[allow(clippy::too_many_arguments)] // a picture, where it goes and how: no natural grouping
    pub fn image(
        &mut self,
        rgba: &[u8],
        width: u32,
        height: u32,
        dst: Rect,
        radius: f32,
        alpha: f32,
    ) {
        if alpha.is_nan() {
            return;
        }
        let alpha = alpha.clamp(0.0, 1.0);
        if alpha <= 0.0 {
            return;
        }
        let (x0, y0, x1, y1) = self.dev_rect(dst);
        let c = self.clip;
        let (vx0, vy0, vx1, vy1) = (x0.max(c.0), y0.max(c.1), x1.min(c.2), y1.min(c.3));
        let Some(scaled) = Scaled::take(rgba, width, height, x0, y0, x1, y1, (vx0, vy0, vx1, vy1))
        else {
            return;
        };
        // Then down, and onto the bitmap.
        let rad = (radius * self.scale)
            .min((x1 as i64 - x0 as i64) as f32 / 2.0)
            .min((y1 as i64 - y0 as i64) as f32 / 2.0)
            .max(0.0);
        let vis_w = scaled.vis_w;
        for r in 0..scaled.rows.cells.len() {
            let y = vy0 + r as i32;
            let o = (y as usize * self.bmp.width as usize + vx0 as usize) * 4;
            let dst_row = &mut self.bmp.pixels[o..o + vis_w * 4];
            for (i, d) in dst_row.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                let px = scaled.pixel(r, i);
                let cover = alpha * corner_cover(vx0 + i as i32, y, (x0, y0, x1, y1), rad);
                blend_pixel(d, px, cover);
            }
        }
    }

    /// Lays prepared straight RGBA pixels, `w` by `h`, at the device position `dx, dy`: what
    /// [`prepare_image`] gives, drawn without sampling it again. The painter's clip, the
    /// rectangle ending at `right, bottom` and `alpha` apply as they do to
    /// [`image`](Self::image), and a buffer too short for the size draws nothing. The bound is
    /// the placed rectangle's far edge, so cached pixels are not painted into what lies past it.
    #[allow(clippy::too_many_arguments)] // prepared pixels, where they go, and how far
    pub(crate) fn blit_rgba(
        &mut self,
        rgba: &[u8],
        w: u32,
        h: u32,
        dx: i32,
        dy: i32,
        right: i32,
        bottom: i32,
        alpha: f32,
    ) {
        if w == 0 || h == 0 || alpha.is_nan() {
            return;
        }
        let alpha = alpha.clamp(0.0, 1.0);
        if alpha <= 0.0 {
            return;
        }
        let enough = (w as usize)
            .checked_mul(h as usize)
            .and_then(|n| n.checked_mul(4))
            .is_some_and(|n| rgba.len() >= n);
        if !enough {
            return;
        }
        let x0 = dx.max(self.clip.0);
        let x1 = dx.saturating_add(w as i32).min(self.clip.2).min(right);
        let y0 = dy.max(self.clip.1);
        let y1 = dy.saturating_add(h as i32).min(self.clip.3).min(bottom);
        if x0 >= x1 || y0 >= y1 {
            return;
        }
        for y in y0..y1 {
            let sy = (y - dy) as usize;
            let s = (sy * w as usize + (x0 - dx) as usize) * 4;
            let d = (y as usize * self.bmp.width as usize + x0 as usize) * 4;
            let n = (x1 - x0) as usize;
            let src = &rgba[s..s + n * 4];
            let dst = &mut self.bmp.pixels[d..d + n * 4];
            for (i, d) in dst.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                let s = &src[i * 4..i * 4 + 4];
                let a = s[3] as f32 / 255.0 * alpha;
                let px = [s[0] as f32 * a, s[1] as f32 * a, s[2] as f32 * a, a];
                blend_pixel(d, px, 1.0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::Gfx;
    use super::*;
    use mdshared_text::Bitmap;

    const WHITE: [u8; 3] = [255, 255, 255];
    const BLACK: [u8; 3] = [0, 0, 0];

    /// A white page `w` by `h` with `draw` on it, at a scale.
    fn page(w: u32, h: u32, scale: f32, draw: impl FnOnce(&mut Painter<'_>)) -> Bitmap {
        let mut bmp = Bitmap::new(w, h);
        let mut gfx = Gfx::new();
        {
            let mut p = Painter::new(&mut bmp, &mut gfx, scale);
            p.fill(Rect::new(0.0, 0.0, w as f32, h as f32), WHITE);
            draw(&mut p);
        }
        bmp
    }

    fn at(bmp: &Bitmap, x: i32, y: i32) -> [u8; 3] {
        let o = (y as usize * bmp.width as usize + x as usize) * 4;
        [bmp.pixels[o + 2], bmp.pixels[o + 1], bmp.pixels[o]]
    }

    /// The pixels that are not white: how many, and the box they lie in (x0, y0, x1, y1, the
    /// far edges outside).
    fn ink(bmp: &Bitmap) -> (usize, (i32, i32, i32, i32)) {
        let (mut n, mut b) = (0, (i32::MAX, i32::MAX, i32::MIN, i32::MIN));
        for y in 0..bmp.height as i32 {
            for x in 0..bmp.width as i32 {
                if at(bmp, x, y) != WHITE {
                    n += 1;
                    b = (b.0.min(x), b.1.min(y), b.2.max(x + 1), b.3.max(y + 1));
                }
            }
        }
        (n, b)
    }

    /// A picture `w` by `h`: straight RGBA from a function of the place.
    fn picture(w: u32, h: u32, f: impl Fn(u32, u32) -> [u8; 4]) -> Vec<u8> {
        (0..h)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .flat_map(|(x, y)| f(x, y))
            .collect()
    }

    #[test]
    fn a_picture_at_its_own_size_is_copied_and_only_into_its_rectangle() {
        let pic = picture(20, 10, |x, y| [x as u8 * 10, y as u8 * 20, 77, 255]);
        let bmp = page(60, 40, 1.0, |p| {
            p.image(&pic, 20, 10, Rect::new(5.0, 8.0, 20.0, 10.0), 0.0, 1.0);
        });
        for y in 0..40 {
            for x in 0..60 {
                let inside = (5..25).contains(&x) && (8..18).contains(&y);
                let want = if inside {
                    [(x - 5) as u8 * 10, (y - 8) as u8 * 20, 77]
                } else {
                    WHITE
                };
                assert_eq!(at(&bmp, x, y), want, "{x},{y}");
            }
        }
        // Too few bytes, no size, no rectangle, no opacity: nothing, and no panic.
        let blank = page(60, 40, 1.0, |_| ());
        let r = Rect::new(5.0, 8.0, 20.0, 10.0);
        let none = page(60, 40, 1.0, |p| {
            p.image(&pic[..100], 20, 10, r, 0.0, 1.0);
            p.image(&pic, 0, 10, r, 0.0, 1.0);
            p.image(&pic, 20, 10, Rect::new(5.0, 8.0, 0.0, 10.0), 0.0, 1.0);
            p.image(&pic, 20, 10, r, 0.0, 0.0);
            p.image(&pic, 20, 10, r, 0.0, f32::NAN);
            p.image(&pic, u32::MAX, u32::MAX, r, 0.0, 1.0);
            p.image(&pic, 20, 10, Rect::new(500.0, 8.0, 20.0, 10.0), 0.0, 1.0);
        });
        assert!(none.pixels == blank.pixels);
    }

    #[test]
    fn a_picture_blends_by_its_own_alpha_times_the_alpha_asked_for() {
        // Black at full, half and no alpha, on white: three pixels of each.
        let pic = picture(9, 1, |x, _| [0, 0, 0, [255, 128, 0][x as usize / 3]]);
        let level = |alpha: f32, band: i32| {
            let bmp = page(40, 20, 1.0, |p| {
                p.image(&pic, 9, 1, Rect::new(5.0, 5.0, 30.0, 10.0), 0.0, alpha);
            });
            // Inside each band, where only pixels of one alpha are mixed.
            at(&bmp, 5 + 3 + 10 * band, 10)[0] as i32
        };
        assert_eq!((level(1.0, 0), level(1.0, 2)), (0, 255));
        assert!((level(1.0, 1) - 127).abs() <= 1);
        assert!((level(0.5, 0) - 128).abs() <= 1);
        assert!((level(0.5, 1) - 191).abs() <= 1);
        assert_eq!(level(0.5, 2), 255);
    }

    #[test]
    fn a_picture_shrunk_is_averaged_and_one_enlarged_is_mixed_between_its_pixels() {
        // One-pixel stripes shrunk by 4.3: flat grey, not the moiré plain sampling gives.
        let stripes = picture(430, 43, |x, _| {
            let v = if x % 2 == 0 { 255 } else { 0 };
            [v, v, v, 255]
        });
        let bmp = page(120, 30, 1.0, |p| {
            p.image(
                &stripes,
                430,
                43,
                Rect::new(10.0, 10.0, 100.0, 10.0),
                0.0,
                1.0,
            );
        });
        let levels: Vec<u8> = (10..110).map(|x| at(&bmp, x, 15)[1]).collect();
        let (lo, hi) = (levels.iter().min().unwrap(), levels.iter().max().unwrap());
        assert!(hi - lo <= 20 && (110..=146).contains(lo), "{lo}..{hi}");
        // A cut-out shrunk: opaque white next to clear black leaves no dark fringe on white.
        let cut = picture(400, 8, |x, _| if x < 203 { [255; 4] } else { [0, 0, 0, 0] });
        let bmp = page(120, 30, 1.0, |p| {
            p.image(&cut, 400, 8, Rect::new(10.0, 10.0, 100.0, 10.0), 0.0, 1.0);
        });
        assert_eq!(ink(&bmp).0, 0);
        // Two pixels, black and white, enlarged to 40: black, a ramp, white.
        let two = picture(2, 1, |x, _| {
            [x as u8 * 255, x as u8 * 255, x as u8 * 255, 255]
        });
        let bmp = page(60, 20, 1.0, |p| {
            p.image(&two, 2, 1, Rect::new(10.0, 5.0, 40.0, 10.0), 0.0, 1.0);
        });
        let row: Vec<u8> = (10..50).map(|x| at(&bmp, x, 10)[0]).collect();
        assert_eq!((row[0], row[9], row[30], row[39]), (0, 0, 255, 255));
        assert!(row.windows(2).all(|w| w[1] >= w[0]), "{row:?}");
        assert!((row[19] as i32 - 121).abs() <= 8 && (row[20] as i32 - 134).abs() <= 8);
    }

    #[test]
    fn a_picture_is_cut_by_the_clip_moved_by_the_offset_and_rounded_at_its_corners() {
        let pic = picture(40, 40, |_, _| [0, 0, 0, 255]);
        let r = Rect::new(10.0, 10.0, 40.0, 40.0);
        // The clip: only its part, and exactly the pixels the whole draw has there.
        let whole = page(80, 80, 1.0, |p| p.image(&pic, 40, 40, r, 8.0, 1.0));
        let part = page(80, 80, 1.0, |p| {
            let old = p.clip(Rect::new(0.0, 0.0, 30.0, 25.0));
            p.image(&pic, 40, 40, r, 8.0, 1.0);
            p.restore(old);
        });
        assert_eq!(ink(&part).1, (10, 10, 30, 25));
        for y in 0..25 {
            for x in 0..30 {
                assert_eq!(at(&part, x, y), at(&whole, x, y));
            }
        }
        // The corners are those of a box of the same radius.
        let boxed = page(80, 80, 1.0, |p| p.rbox(r, 8.0, BLACK));
        let mut worst = 0;
        for y in 0..80 {
            for x in 0..80 {
                worst = worst.max((at(&whole, x, y)[0] as i32 - at(&boxed, x, y)[0] as i32).abs());
            }
        }
        assert!(worst <= 2, "the corners differ from a box's by {worst}");
        assert_eq!(at(&whole, 10, 10), WHITE);
        assert_eq!(at(&whole, 30, 30), BLACK);
        // The offset moves it by whole pixels; the scale makes it larger.
        let moved = page(80, 80, 1.0, |p| {
            let o = p.offset(7.0, -3.0);
            p.image(&pic, 40, 40, r, 8.0, 1.0);
            p.restore_offset(o);
        });
        for y in 3..80 {
            for x in 7..80 {
                assert_eq!(at(&moved, x, y - 3), at(&whole, x - 7, y));
            }
        }
        let scaled = page(160, 160, 1.5, |p| p.image(&pic, 40, 40, r, 0.0, 1.0));
        assert_eq!(ink(&scaled).1, (15, 15, 75, 75));
    }
}
