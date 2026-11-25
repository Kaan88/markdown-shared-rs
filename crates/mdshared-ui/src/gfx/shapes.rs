//! More shapes for the [`Painter`]: a picture scaled into a rectangle, paths filled and
//! stroked, rings and arcs, a row of columns for a waveform or a meter, and a gradient. All of
//! them take logical px, honour the clip and the offset, and blend at an alpha, as the boxes
//! and the text do.
//!
//! None of them keeps anything between calls: a shape is rasterized each time it is drawn.
//! That suits what changes every frame (a progress ring, a meter) and small things. A large
//! picture that is drawn every frame at one size is better scaled once by its owner and
//! [`blit`](Painter::blit)ted; a cache of scaled pictures is not here.

use super::{Painter, Rect, Rgb, mix};
use zeno::{Command, Point};

/// A shape made of straight lines and curves, in logical px, for
/// [`Painter::fill_path`] and [`Painter::stroke_path`]. Built point by point, or read from
/// the path data of an SVG ([`Path::svg`]), the notation the icons of [`glyph`](crate::glyph)
/// are written in.
#[derive(Clone, Debug, Default)]
pub struct Path {
    commands: Vec<Command>,
}

impl Path {
    pub fn new() -> Path {
        Path::default()
    }

    /// The path an SVG `d` attribute describes (`"M2 8h12"`), every command of it that parses:
    /// what follows a mistake is left out, as the icon rasterizer leaves it out.
    pub fn svg(d: &str) -> Path {
        Path {
            commands: zeno::PathData::commands(&d).collect(),
        }
    }

    /// A closed path through `points`: a polygon.
    pub fn polygon(points: &[(f32, f32)]) -> Path {
        let mut path = Path::new();
        for (i, &(x, y)) in points.iter().enumerate() {
            path = if i == 0 {
                path.move_to(x, y)
            } else {
                path.line_to(x, y)
            };
        }
        if points.len() > 1 {
            path = path.close();
        }
        path
    }

    /// Starts a new piece at a point.
    pub fn move_to(mut self, x: f32, y: f32) -> Path {
        self.commands.push(Command::MoveTo(Point::new(x, y)));
        self
    }

    /// A straight line to a point.
    pub fn line_to(mut self, x: f32, y: f32) -> Path {
        self.commands.push(Command::LineTo(Point::new(x, y)));
        self
    }

    /// A quadratic curve to `(x, y)` that leans towards the control point `(cx, cy)`.
    pub fn quad_to(mut self, cx: f32, cy: f32, x: f32, y: f32) -> Path {
        self.commands
            .push(Command::QuadTo(Point::new(cx, cy), Point::new(x, y)));
        self
    }

    /// A cubic curve to `(x, y)` with two control points.
    pub fn curve_to(mut self, c1: (f32, f32), c2: (f32, f32), x: f32, y: f32) -> Path {
        self.commands.push(Command::CurveTo(
            Point::new(c1.0, c1.1),
            Point::new(c2.0, c2.1),
            Point::new(x, y),
        ));
        self
    }

    /// Joins the piece's end to its start.
    pub fn close(mut self) -> Path {
        self.commands.push(Command::Close);
        self
    }

    /// Whether the path has nothing in it.
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }
}

/// Which way a gradient runs.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Along {
    /// From the left edge to the right.
    X,
    /// From the top edge to the bottom.
    Y,
}

/// How much of the pixel at `x, y` a rounded rectangle covers at its corners: 1 away from
/// them. The rectangle is in device px, far edges outside.
fn corner_cover(x: i32, y: i32, r: (i32, i32, i32, i32), rad: f32) -> f32 {
    if rad <= 0.0 {
        return 1.0;
    }
    let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
    let past = |f: f32, lo: i32, hi: i32| {
        if f < lo as f32 + rad {
            lo as f32 + rad - f
        } else if f > hi as f32 - rad {
            f - (hi as f32 - rad)
        } else {
            0.0
        }
    };
    let (qx, qy) = (past(fx, r.0, r.2), past(fy, r.1, r.3));
    if qx <= 0.0 || qy <= 0.0 {
        1.0
    } else {
        (rad - qx.hypot(qy) + 0.5).clamp(0.0, 1.0)
    }
}

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
        let (x0, y0, x1, y1) = self.dev_rect(dst);
        let pixels = (width as usize).checked_mul(height as usize);
        let enough = pixels
            .and_then(|n| n.checked_mul(4))
            .is_some_and(|n| rgba.len() >= n);
        if width == 0 || height == 0 || x1 <= x0 || y1 <= y0 || !enough || alpha.is_nan() {
            return;
        }
        let alpha = alpha.clamp(0.0, 1.0);
        let c = self.clip;
        let (vx0, vy0, vx1, vy1) = (x0.max(c.0), y0.max(c.1), x1.min(c.2), y1.min(c.3));
        if vx0 >= vx1 || vy0 >= vy1 || alpha <= 0.0 {
            return;
        }
        let cols = Taps::new(vx0 - x0..vx1 - x0, x1 - x0, width);
        let rows = Taps::new(vy0 - y0..vy1 - y0, y1 - y0, height);
        // Across first: the source rows the visible rows read, each squeezed to the visible
        // columns, in premultiplied alpha.
        let (row_lo, row_hi) = rows.reach();
        let vis_w = (vx1 - vx0) as usize;
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
        // Then down, and onto the bitmap.
        let rad = (radius * self.scale)
            .min((x1 - x0) as f32 / 2.0)
            .min((y1 - y0) as f32 / 2.0)
            .max(0.0);
        for (r, &(first, start, n)) in rows.cells.iter().enumerate() {
            let y = vy0 + r as i32;
            let o = (y as usize * self.bmp.width as usize + vx0 as usize) * 4;
            let dst_row = &mut self.bmp.pixels[o..o + vis_w * 4];
            for (i, d) in dst_row.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                let mut px = [0f32; 4];
                for (j, w) in rows.weights[start..start + n].iter().enumerate() {
                    let s = across[(first - row_lo + j) * vis_w + i];
                    for (p, s) in px.iter_mut().zip(s) {
                        *p += s * w;
                    }
                }
                let cover = alpha * corner_cover(vx0 + i as i32, y, (x0, y0, x1, y1), rad);
                let keep = 1.0 - px[3].min(1.0) * cover;
                // RGBA sums, BGRA framebuffer.
                for ch in 0..3 {
                    let v = px[2 - ch] * cover + d[ch] as f32 * keep;
                    d[ch] = (v + 0.5).min(255.0) as u8;
                }
            }
        }
    }

    /// Blends a coverage mask, one byte a device pixel, in a colour at an alpha, inside the
    /// clip. `left, top` is where the mask's first pixel goes.
    fn blend_mask(&mut self, left: i32, top: i32, width: usize, data: &[u8], c: Rgb, alpha: f32) {
        if width == 0 || alpha.is_nan() || alpha <= 0.0 {
            return;
        }
        let alpha = (alpha.min(1.0) * 256.0) as u32;
        let (xa, xb) = (
            left.max(self.clip.0),
            (left + width as i32).min(self.clip.2),
        );
        if xa >= xb {
            return;
        }
        for (row, cover) in data.chunks_exact(width).enumerate() {
            let y = top + row as i32;
            if y < self.clip.1 || y >= self.clip.3 {
                continue;
            }
            let o = (y as usize * self.bmp.width as usize + xa as usize) * 4;
            let dst = &mut self.bmp.pixels[o..o + (xb - xa) as usize * 4];
            let cover = &cover[(xa - left) as usize..(xb - left) as usize];
            for (d, &a) in dst.as_chunks_mut::<4>().0.iter_mut().zip(cover) {
                // Full coverage at full alpha has to come out as the colour itself.
                let a = ((a as u32 + (a as u32 >> 7)) * alpha) >> 8;
                let ia = 256 - a;
                d[0] = ((c[2] as u32 * a + d[0] as u32 * ia) >> 8) as u8;
                d[1] = ((c[1] as u32 * a + d[1] as u32 * ia) >> 8) as u8;
                d[2] = ((c[0] as u32 * a + d[2] as u32 * ia) >> 8) as u8;
            }
        }
    }

    /// Rasterizes a path in a style and blends it.
    fn draw_path<'s>(
        &mut self,
        path: &Path,
        style: impl Into<zeno::Style<'s>>,
        c: Rgb,
        alpha: f32,
    ) {
        if path.is_empty() {
            return;
        }
        let s = self.scale;
        let place =
            zeno::Transform::scale(s, s).then_translate(self.origin.0 as f32, self.origin.1 as f32);
        let (data, at) = zeno::Mask::new(&path.commands)
            .style(style)
            .transform(Some(place))
            .render();
        self.blend_mask(at.left, at.top, at.width as usize, &data, c, alpha);
    }

    /// Fills the inside of a path with a colour at an alpha, antialiased. A point is inside by
    /// the non-zero rule: a piece drawn the other way round inside another cuts a hole.
    pub fn fill_path(&mut self, path: &Path, c: Rgb, alpha: f32) {
        self.draw_path(path, zeno::Fill::NonZero, c, alpha);
    }

    /// Draws the line of a path `width` logical px wide, with round ends and round corners (as
    /// the icons are drawn), in a colour at an alpha.
    pub fn stroke_path(&mut self, path: &Path, width: f32, c: Rgb, alpha: f32) {
        if width.is_nan() || width <= 0.0 {
            return;
        }
        let mut stroke = zeno::Stroke::new(width);
        stroke.cap(zeno::Cap::Round).join(zeno::Join::Round);
        self.draw_path(path, stroke, c, alpha);
    }

    /// A ring: a circle line of radius `r` (to the middle of the line) and `width` thick round
    /// `cx, cy`. The track of a circular progress mark. A `width` of twice the radius or more
    /// is a filled disc.
    pub fn ring(&mut self, cx: f32, cy: f32, r: f32, width: f32, c: Rgb, alpha: f32) {
        self.arc(cx, cy, r, width, 0.0, 1.0, c, alpha);
    }

    /// A part of a ring, with round ends: from the angle `from` to the angle `to`, both in
    /// turns clockwise from the top (0 is twelve o'clock, 0.25 three o'clock), so
    /// `arc(.., 0.0, progress, ..)` is a progress ring and adding the same number to both turns
    /// a spinner. The two may be given either way round and outside 0 to 1; a span of a whole
    /// turn or more is the whole [`ring`](Self::ring), and no span draws nothing.
    ///
    /// The centre is not snapped to a pixel: an arc that moves by a fraction of a pixel moves
    /// smoothly.
    #[allow(clippy::too_many_arguments)] // a circle, a part of it and a style: no natural grouping
    pub fn arc(
        &mut self,
        cx: f32,
        cy: f32,
        r: f32,
        width: f32,
        from: f32,
        to: f32,
        c: Rgb,
        alpha: f32,
    ) {
        let numbers = [cx, cy, r, width, from, to, alpha];
        if numbers.iter().any(|v| !v.is_finite()) || r < 0.0 || width <= 0.0 || alpha <= 0.0 {
            return;
        }
        let (from, to) = (from.min(to), from.max(to));
        let span = to - from;
        if span <= 0.0 {
            return;
        }
        let whole = span >= 1.0;
        let s = self.scale;
        let (cx, cy) = (cx * s + self.origin.0 as f32, cy * s + self.origin.1 as f32);
        let (r, half) = (r * s, width * s / 2.0);
        // The middle of the arc and half its opening, in radians, and its two ends.
        let tau = std::f32::consts::TAU;
        let (mid, open) = ((from + to) / 2.0 * tau, span / 2.0 * tau);
        let end = |turns: f32| {
            let a = turns * tau;
            (cx + r * a.sin(), cy - r * a.cos())
        };
        let (e0, e1) = (end(from), end(to));
        let reach = r + half + 1.0;
        let (x0, x1) = ((cx - reach).floor() as i32, (cx + reach).ceil() as i32);
        let (y0, y1) = ((cy - reach).floor() as i32, (cy + reach).ceil() as i32);
        for y in y0.max(self.clip.1)..y1.min(self.clip.3) {
            for x in x0.max(self.clip.0)..x1.min(self.clip.2) {
                let (dx, dy) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
                let on_ring = (dx.hypot(dy) - r).abs();
                let d = if whole {
                    on_ring
                } else {
                    // The pixel's own angle from the top, clockwise, as an offset from the
                    // middle of the arc, in -pi..pi.
                    let mut off = dx.atan2(-dy) - mid;
                    off -= (off / tau).round() * tau;
                    if off.abs() <= open {
                        on_ring
                    } else {
                        let d0 = (x as f32 + 0.5 - e0.0).hypot(y as f32 + 0.5 - e0.1);
                        let d1 = (x as f32 + 0.5 - e1.0).hypot(y as f32 + 0.5 - e1.1);
                        d0.min(d1)
                    }
                };
                let cover = (half - d + 0.5).clamp(0.0, 1.0);
                if cover > 0.0 {
                    self.span(y, x, x + 1, c, alpha.min(1.0) * cover);
                }
            }
        }
    }

    /// A row of columns across `area`, one for each of `values`: column `i` stands from the
    /// fraction `lo` to the fraction `hi` of the area's height, measured from its bottom (0 is
    /// the bottom edge, 1 the top). A waveform is one column a pixel with the least and the
    /// most of the samples under it; a level meter or a spectrum is a few wide columns from 0.
    ///
    /// The columns share the width evenly, their sides on whole device pixels, with `gap`
    /// logical px left free at the right of each (0 for a waveform); a column narrower than a
    /// device pixel after the gap is drawn one pixel wide. Top and bottom are antialiased, so
    /// a level that moves by a fraction of a pixel moves smoothly. Fractions outside 0 to 1
    /// are cut to the area; a column whose `hi` is not above its `lo` is not drawn.
    pub fn columns(&mut self, area: Rect, values: &[(f32, f32)], gap: f32, c: Rgb, alpha: f32) {
        if values.is_empty() || area.w <= 0.0 || area.h <= 0.0 || alpha.is_nan() || alpha <= 0.0 {
            return;
        }
        let alpha = alpha.min(1.0);
        let n = values.len() as f32;
        let gap = (gap.max(0.0) * self.scale).round() as i32;
        let (top, bottom) = (
            area.y * self.scale + self.origin.1 as f32,
            area.bottom() * self.scale + self.origin.1 as f32,
        );
        let right = self.dev_x(area.right());
        for (i, &(lo, hi)) in values.iter().enumerate() {
            let (lo, hi) = (lo.clamp(0.0, 1.0), hi.clamp(0.0, 1.0));
            // Also false for a number that is not one.
            if hi.partial_cmp(&lo) != Some(std::cmp::Ordering::Greater) {
                continue;
            }
            // Inside the area, and never thinner than a pixel.
            let x0 = self.dev_x(area.x + area.w * i as f32 / n).min(right - 1);
            let next = self.dev_x(area.x + area.w * (i + 1) as f32 / n);
            let x1 = (next - gap).max(x0 + 1).min(right);
            let (y0, y1) = (bottom - hi * (bottom - top), bottom - lo * (bottom - top));
            for y in (y0.floor() as i32).max(self.clip.1)..(y1.ceil() as i32).min(self.clip.3) {
                // How much of this pixel row the column covers.
                let cover = (y1.min(y as f32 + 1.0) - y0.max(y as f32)).clamp(0.0, 1.0);
                self.span(y, x0, x1, c, alpha * cover);
            }
        }
    }

    /// A rounded rectangle filled with a gradient from `from` to `to` [`Along`] an axis,
    /// blended at `alpha`. The colours are mixed in the stored (gamma) space, as
    /// [`mix`](super::mix) and every blend here do, a pixel's colour taken at its middle: the
    /// first row or column is half a step from `from`, the last half a step from `to`.
    #[allow(clippy::too_many_arguments)] // a box, two colours and a direction
    pub fn gradient(&mut self, r: Rect, radius: f32, from: Rgb, to: Rgb, along: Along, alpha: f32) {
        let (x0, y0, x1, y1) = self.dev_rect(r);
        if x1 <= x0 || y1 <= y0 || alpha.is_nan() || alpha <= 0.0 {
            return;
        }
        let alpha = alpha.min(1.0);
        let rad = (radius * self.scale)
            .min((x1 - x0) as f32 / 2.0)
            .min((y1 - y0) as f32 / 2.0)
            .max(0.0);
        let band = rad.ceil() as i32;
        let at = |i: i32, lo: i32, hi: i32| {
            mix(
                from,
                to,
                (i - lo) as f32 / (hi - lo) as f32 + 0.5 / (hi - lo) as f32,
            )
        };
        for y in y0.max(self.clip.1)..y1.min(self.clip.3) {
            // A row away from the corners of a top-to-bottom gradient is one colour.
            let plain = y >= y0 + band && y < y1 - band;
            if along == Along::Y && plain {
                self.span(y, x0, x1, at(y, y0, y1), alpha);
                continue;
            }
            for x in x0.max(self.clip.0)..x1.min(self.clip.2) {
                let c = match along {
                    Along::X => at(x, x0, x1),
                    Along::Y => at(y, y0, y1),
                };
                let cover = corner_cover(x, y, (x0, y0, x1, y1), rad);
                self.span(y, x, x + 1, c, alpha * cover);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Face, Gfx};
    use super::*;
    use mdshared_text::Bitmap;

    const WHITE: Rgb = [255, 255, 255];
    const BLACK: Rgb = [0, 0, 0];
    const RED: Rgb = [200, 30, 30];

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

    fn at(bmp: &Bitmap, x: i32, y: i32) -> Rgb {
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

    /// The sum of how dark every pixel is, in whole pixels of full ink: the area of a black
    /// shape, antialiased edges counted by their share.
    fn area(bmp: &Bitmap) -> f32 {
        let mut sum = 0.0;
        for y in 0..bmp.height as i32 {
            for x in 0..bmp.width as i32 {
                sum += (255 - at(bmp, x, y)[1] as i32) as f32 / 255.0;
            }
        }
        sum
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

    #[test]
    fn a_filled_path_covers_its_inside_and_a_reversed_piece_cuts_a_hole() {
        let square = Path::polygon(&[(10.0, 10.0), (50.0, 10.0), (50.0, 40.0), (10.0, 40.0)]);
        let bmp = page(60, 50, 1.0, |p| p.fill_path(&square, BLACK, 1.0));
        assert_eq!(ink(&bmp), (40 * 30, (10, 10, 50, 40)));
        assert_eq!(at(&bmp, 30, 25), BLACK);
        // The same square with a smaller one inside it, drawn the other way round.
        let frame = Path::new()
            .move_to(10.0, 10.0)
            .line_to(50.0, 10.0)
            .line_to(50.0, 40.0)
            .line_to(10.0, 40.0)
            .close()
            .move_to(20.0, 20.0)
            .line_to(20.0, 30.0)
            .line_to(40.0, 30.0)
            .line_to(40.0, 20.0)
            .close();
        let bmp = page(60, 50, 1.0, |p| p.fill_path(&frame, RED, 1.0));
        assert_eq!(ink(&bmp).0, 40 * 30 - 20 * 10);
        assert_eq!((at(&bmp, 15, 15), at(&bmp, 30, 25)), (RED, WHITE));
        // A triangle: half the area of its box, with antialiased sides.
        let tri = Path::polygon(&[(10.0, 40.0), (50.0, 40.0), (50.0, 10.0)]);
        let bmp = page(60, 50, 1.0, |p| p.fill_path(&tri, BLACK, 1.0));
        assert!((area(&bmp) - 600.0).abs() < 6.0, "{}", area(&bmp));
        // Half alpha is half the ink; none is none; an empty path is nothing.
        let half = page(60, 50, 1.0, |p| p.fill_path(&square, BLACK, 0.5));
        assert!((at(&half, 30, 25)[0] as i32 - 128).abs() <= 1);
        let none = page(60, 50, 1.0, |p| {
            p.fill_path(&square, BLACK, 0.0);
            p.fill_path(&Path::new(), BLACK, 1.0);
            p.fill_path(&Path::polygon(&[]), BLACK, 1.0);
            p.stroke_path(&square, 0.0, BLACK, 1.0);
            p.stroke_path(&square, f32::NAN, BLACK, 1.0);
        });
        assert_eq!(ink(&none).0, 0);
        assert!(Path::new().is_empty() && !square.is_empty());
    }

    #[test]
    fn a_stroked_path_is_a_line_of_the_width_asked_for_with_round_ends() {
        let line = Path::new().move_to(20.0, 20.0).line_to(60.0, 20.0);
        let bmp = page(80, 40, 1.0, |p| p.stroke_path(&line, 6.0, BLACK, 1.0));
        // 40 long and 6 thick, plus a half disc of radius 3 at each end.
        let want = 40.0 * 6.0 + std::f32::consts::PI * 9.0;
        assert!((area(&bmp) - want).abs() < 4.0, "{} of {want}", area(&bmp));
        assert_eq!(ink(&bmp).1, (17, 17, 63, 23));
        assert_eq!((at(&bmp, 40, 20), at(&bmp, 40, 24)), (BLACK, WHITE));
        // Curves, and the icons' own notation: the same path either way.
        let built = Path::new()
            .move_to(10.0, 30.0)
            .quad_to(40.0, 0.0, 70.0, 30.0)
            .curve_to((60.0, 38.0), (20.0, 38.0), 10.0, 30.0);
        let read = Path::svg("M10 30Q40 0 70 30C60 38 20 38 10 30");
        let a = page(80, 40, 1.0, |p| p.stroke_path(&built, 2.0, RED, 1.0));
        let b = page(80, 40, 1.0, |p| p.stroke_path(&read, 2.0, RED, 1.0));
        assert!(a.pixels == b.pixels && ink(&a).0 > 150);
        // The scale, the offset and the clip apply as to everything else.
        let big = page(160, 80, 2.0, |p| p.stroke_path(&line, 6.0, BLACK, 1.0));
        assert_eq!(ink(&big).1, (34, 34, 126, 46));
        let moved = page(80, 40, 1.0, |p| {
            let o = p.offset(5.0, 9.0);
            p.stroke_path(&line, 6.0, BLACK, 1.0);
            p.restore_offset(o);
        });
        assert_eq!(ink(&moved).1, (22, 26, 68, 32));
        let cut = page(80, 40, 1.0, |p| {
            let old = p.clip(Rect::new(30.0, 0.0, 10.0, 20.0));
            p.stroke_path(&line, 6.0, BLACK, 1.0);
            p.restore(old);
        });
        assert_eq!(ink(&cut), (10 * 3, (30, 17, 40, 20)));
    }

    #[test]
    fn a_ring_has_the_area_of_its_line_and_an_arc_its_share_of_it() {
        let tau = std::f32::consts::TAU;
        let ring = page(100, 100, 1.0, |p| p.ring(50.0, 50.0, 30.0, 6.0, BLACK, 1.0));
        let full = tau * 30.0 * 6.0;
        assert!(
            (area(&ring) - full).abs() < full * 0.01,
            "{} of {full}",
            area(&ring)
        );
        assert_eq!(ink(&ring).1, (17, 17, 83, 83));
        assert_eq!(
            (at(&ring, 50, 20), at(&ring, 50, 50), at(&ring, 80, 50)),
            (BLACK, WHITE, BLACK)
        );
        // A quarter from the top, clockwise: the top right quadrant, with a round end past
        // each end of it.
        let quarter = page(100, 100, 1.0, |p| {
            p.arc(50.0, 50.0, 30.0, 6.0, 0.0, 0.25, BLACK, 1.0)
        });
        let want = full / 4.0 + std::f32::consts::PI * 9.0;
        assert!(
            (area(&quarter) - want).abs() < want * 0.02,
            "{} of {want}",
            area(&quarter)
        );
        assert_eq!((at(&quarter, 50, 20), at(&quarter, 80, 50)), (BLACK, BLACK));
        assert_eq!((at(&quarter, 20, 50), at(&quarter, 50, 80)), (WHITE, WHITE));
        assert_eq!(at(&quarter, 71, 29), BLACK, "half past one");
        assert_eq!(ink(&quarter).1, (47, 17, 83, 53));
        // Either way round, and from any turn: the same arc.
        let draw = |from: f32, to: f32| {
            page(100, 100, 1.0, |p| {
                p.arc(50.0, 50.0, 30.0, 6.0, from, to, BLACK, 1.0)
            })
        };
        assert!(draw(0.25, 0.0).pixels == quarter.pixels);
        let far = draw(3.0, 3.25);
        let mut worst = 0;
        for (a, b) in far.pixels.iter().zip(&quarter.pixels) {
            worst = worst.max((*a as i32 - *b as i32).abs());
        }
        assert!(worst <= 3, "three turns on it differs by {worst}");
        // Across the top: from nine o'clock to three.
        let over = draw(-0.25, 0.25);
        assert_eq!(
            (at(&over, 20, 50), at(&over, 50, 20), at(&over, 80, 50)),
            (BLACK, BLACK, BLACK)
        );
        assert_eq!(at(&over, 50, 80), WHITE);
        // A whole turn or more is the ring; no span, or nonsense, is nothing.
        assert!(draw(0.1, 1.1).pixels == ring.pixels && draw(0.0, 5.0).pixels == ring.pixels);
        let none = page(100, 100, 1.0, |p| {
            p.arc(50.0, 50.0, 30.0, 6.0, 0.4, 0.4, BLACK, 1.0);
            p.arc(50.0, 50.0, 30.0, 0.0, 0.0, 0.5, BLACK, 1.0);
            p.arc(50.0, f32::NAN, 30.0, 6.0, 0.0, 0.5, BLACK, 1.0);
            p.arc(50.0, 50.0, -1.0, 6.0, 0.0, 0.5, BLACK, 1.0);
            p.ring(50.0, 50.0, 30.0, 6.0, BLACK, 0.0);
        });
        assert_eq!(ink(&none).0, 0);
        // Half alpha, the clip, the offset and the scale.
        let half = page(100, 100, 1.0, |p| p.ring(50.0, 50.0, 30.0, 6.0, BLACK, 0.5));
        assert!((at(&half, 50, 20)[0] as i32 - 128).abs() <= 1);
        let cut = page(100, 100, 1.0, |p| {
            let old = p.clip(Rect::new(0.0, 0.0, 50.0, 50.0));
            p.ring(50.0, 50.0, 30.0, 6.0, BLACK, 1.0);
            p.restore(old);
        });
        assert_eq!(ink(&cut).1, (17, 17, 50, 50));
        let moved = page(100, 100, 1.0, |p| {
            let o = p.offset(-10.0, 5.0);
            p.ring(50.0, 50.0, 30.0, 6.0, BLACK, 1.0);
            p.restore_offset(o);
        });
        assert_eq!(ink(&moved).1, (7, 22, 73, 88));
        let big = page(200, 200, 2.0, |p| p.ring(50.0, 50.0, 30.0, 6.0, BLACK, 1.0));
        assert!((area(&big) - 4.0 * full).abs() < 4.0 * full * 0.01);
        // A line as thick as the circle is wide is a disc.
        let disc = page(100, 100, 1.0, |p| {
            p.ring(50.0, 50.0, 10.0, 20.0, BLACK, 1.0)
        });
        let want = std::f32::consts::PI * 400.0;
        assert!((area(&disc) - want).abs() < want * 0.01);
    }

    #[test]
    fn columns_stand_from_the_bottom_by_their_fractions() {
        let area = Rect::new(10.0, 10.0, 40.0, 100.0);
        let bmp = page(60, 120, 1.0, |p| {
            p.columns(
                area,
                &[(0.0, 1.0), (0.0, 0.5), (0.25, 0.75), (0.0, 0.0)],
                0.0,
                BLACK,
                1.0,
            );
        });
        // Four columns of 10 px: full height, the lower half, the middle half, nothing.
        let column =
            |x: i32| -> Vec<i32> { (0..120).filter(|&y| at(&bmp, x, y) == BLACK).collect() };
        assert_eq!(
            (column(10).len(), column(10)[0], column(19)[0]),
            (100, 10, 10)
        );
        assert_eq!((column(20).len(), column(20)[0]), (50, 60));
        assert_eq!(
            (column(30).len(), column(30)[0], column(39).len()),
            (50, 35, 50)
        );
        assert!(column(40).is_empty() && column(9).is_empty() && column(50).is_empty());
        assert_eq!(ink(&bmp).0, 1000 + 500 + 500);
        // A gap at the right of each; a level between two pixels is a row of grey.
        let bmp = page(60, 120, 1.0, |p| {
            p.columns(area, &[(0.0, 0.255), (0.0, 1.0)], 3.0, BLACK, 1.0);
        });
        assert_eq!(
            (at(&bmp, 26, 100), at(&bmp, 27, 100), at(&bmp, 30, 100)),
            (BLACK, WHITE, BLACK)
        );
        assert_eq!((at(&bmp, 46, 50), at(&bmp, 47, 50)), (BLACK, WHITE));
        assert_eq!((at(&bmp, 15, 85), at(&bmp, 15, 83)), (BLACK, WHITE));
        assert!(
            (at(&bmp, 15, 84)[0] as i32 - 128).abs() <= 2,
            "half a pixel of level"
        );
        // One column a pixel, as a waveform: 40 values in 40 px, each its own pixel.
        let wave: Vec<(f32, f32)> = (0..40)
            .map(|i| (0.5 - i as f32 / 80.0, 0.5 + i as f32 / 80.0))
            .collect();
        let bmp = page(60, 120, 1.0, |p| p.columns(area, &wave, 0.0, RED, 1.0));
        for i in 1..40 {
            let ys: Vec<i32> = (0..120).filter(|&y| at(&bmp, 10 + i, y) != WHITE).collect();
            let tall = (ys.len() as f32 - i as f32 * 2.5).abs();
            assert!(tall <= 2.0, "column {i} is {} px", ys.len());
            assert!(
                (ys[0] + ys[ys.len() - 1] - 119).abs() <= 1,
                "centred on the middle"
            );
        }
        // More values than pixels: every column still a pixel wide, none lost to the gap.
        let many = vec![(0.0, 1.0); 80];
        let bmp = page(60, 120, 1.0, |p| p.columns(area, &many, 5.0, BLACK, 1.0));
        assert_eq!(ink(&bmp).1, (10, 10, 50, 110));
        // Out of range is cut to the area; nonsense and nothing draw nothing.
        let bmp = page(60, 120, 1.0, |p| {
            p.columns(area, &[(-3.0, 9.0)], 0.0, BLACK, 1.0)
        });
        assert_eq!(ink(&bmp), (4000, (10, 10, 50, 110)));
        let none = page(60, 120, 1.0, |p| {
            p.columns(area, &[], 0.0, BLACK, 1.0);
            p.columns(
                area,
                &[(f32::NAN, 1.0), (0.5, f32::NAN), (0.6, 0.4)],
                0.0,
                BLACK,
                1.0,
            );
            p.columns(
                Rect::new(10.0, 10.0, 0.0, 100.0),
                &[(0.0, 1.0)],
                0.0,
                BLACK,
                1.0,
            );
            p.columns(area, &[(0.0, 1.0)], 0.0, BLACK, 0.0);
        });
        assert_eq!(ink(&none).0, 0);
        // The clip and the offset.
        let cut = page(60, 120, 1.0, |p| {
            let old = p.clip(Rect::new(0.0, 0.0, 15.0, 30.0));
            let o = p.offset(2.0, 3.0);
            p.columns(area, &[(0.0, 1.0)], 0.0, BLACK, 1.0);
            p.restore_offset(o);
            p.restore(old);
        });
        assert_eq!(ink(&cut).1, (12, 13, 15, 30));
    }

    #[test]
    fn a_gradient_runs_from_one_colour_to_the_other_along_its_axis() {
        let r = Rect::new(10.0, 10.0, 100.0, 40.0);
        let (from, to) = ([0, 0, 0], [200, 100, 50]);
        let across = page(120, 60, 1.0, |p| {
            p.gradient(r, 0.0, from, to, Along::X, 1.0)
        });
        let down = page(120, 60, 1.0, |p| {
            p.gradient(r, 0.0, from, to, Along::Y, 1.0)
        });
        assert_eq!(ink(&across).1, (10, 10, 110, 50));
        assert_eq!(ink(&down).1, (10, 10, 110, 50));
        // Across: every row the same, each column a step on, the ends half a step in.
        for x in 10..110 {
            let want = mix(from, to, (x - 10) as f32 / 100.0 + 0.005);
            assert_eq!(at(&across, x, 10), want, "column {x}");
            assert_eq!(at(&across, x, 49), want);
        }
        assert_eq!(
            (at(&across, 10, 30), at(&across, 109, 30)),
            ([1, 1, 0], [199, 100, 50])
        );
        // Down: every column the same.
        for y in 10..50 {
            let want = mix(from, to, (y - 10) as f32 / 40.0 + 0.0125);
            assert_eq!(
                (at(&down, 10, y), at(&down, 109, y)),
                (want, want),
                "row {y}"
            );
        }
        // Rounded: the corners of a box of that radius, the middle untouched by them.
        let round = page(120, 60, 1.0, |p| {
            p.gradient(r, 12.0, BLACK, BLACK, Along::Y, 1.0)
        });
        let boxed = page(120, 60, 1.0, |p| p.rbox(r, 12.0, BLACK));
        let mut worst = 0;
        for (a, b) in round.pixels.iter().zip(&boxed.pixels) {
            worst = worst.max((*a as i32 - *b as i32).abs());
        }
        assert!(worst <= 2, "the corners differ from a box's by {worst}");
        let round = page(120, 60, 1.0, |p| {
            p.gradient(r, 12.0, from, to, Along::X, 1.0)
        });
        assert_eq!(
            (at(&round, 10, 10), at(&round, 60, 10)),
            (WHITE, at(&across, 60, 10))
        );
        // Alpha, clip, offset, and nothing for nothing.
        let half = page(120, 60, 1.0, |p| {
            p.gradient(r, 0.0, BLACK, BLACK, Along::X, 0.5)
        });
        assert!((at(&half, 60, 30)[0] as i32 - 128).abs() <= 1);
        let cut = page(120, 60, 1.0, |p| {
            let old = p.clip(Rect::new(0.0, 0.0, 50.0, 20.0));
            let o = p.offset(5.0, 5.0);
            p.gradient(r, 0.0, from, to, Along::Y, 1.0);
            p.restore_offset(o);
            p.restore(old);
        });
        assert_eq!(ink(&cut).1, (15, 15, 50, 20));
        let none = page(120, 60, 1.0, |p| {
            p.gradient(
                Rect::new(10.0, 10.0, 0.0, 40.0),
                0.0,
                from,
                to,
                Along::X,
                1.0,
            );
            p.gradient(r, 0.0, from, to, Along::X, 0.0);
            p.gradient(r, 0.0, from, to, Along::X, f32::NAN);
        });
        assert_eq!(ink(&none).0, 0);
    }

    /// The sheet a person looks at: every shape of this file, in both a light and a dark
    /// setting, at two scales. `cargo test -p mdshared-ui shapes_sheet -- --ignored`, then open
    /// target/specimen/shapes.png and shapes-150.png.
    #[test]
    #[ignore]
    fn shapes_sheet() {
        let art = picture(64, 64, |x, y| {
            let d = ((x as f32 - 32.0).hypot(y as f32 - 32.0) / 32.0).min(1.0);
            [(255.0 * (1.0 - d)) as u8, (x * 4) as u8, (y * 4) as u8, 255]
        });
        for (scale, file) in [(1.0, "shapes.png"), (1.5, "shapes-150.png")] {
            let mut bmp = Bitmap::new((720.0 * scale) as u32, (400.0 * scale) as u32);
            let mut gfx = Gfx::new();
            let mut p = Painter::new(&mut bmp, &mut gfx, scale);
            for (half, (bg, fg, accent)) in [
                ([251, 251, 251], [31, 31, 31], [0, 95, 184]),
                ([32, 32, 32], [230, 230, 230], [96, 205, 255]),
            ]
            .into_iter()
            .enumerate()
            {
                let oy = half as f32 * 200.0;
                p.fill(Rect::new(0.0, oy, 720.0, 200.0), bg);
                let o = p.offset(0.0, oy);
                p.image(&art, 64, 64, Rect::new(20.0, 20.0, 96.0, 96.0), 12.0, 1.0);
                p.image(&art, 64, 64, Rect::new(130.0, 20.0, 32.0, 32.0), 4.0, 1.0);
                p.image(&art, 64, 64, Rect::new(130.0, 64.0, 32.0, 32.0), 16.0, 0.5);
                p.ring(220.0, 68.0, 36.0, 6.0, mix(bg, fg, 0.15), 1.0);
                p.arc(220.0, 68.0, 36.0, 6.0, 0.0, 0.68, accent, 1.0);
                p.arc(310.0, 68.0, 14.0, 3.0, 0.1, 0.8, fg, 1.0);
                let wave: Vec<(f32, f32)> = (0..180)
                    .map(|i| {
                        let a = ((i as f32 * 0.21).sin() * (i as f32 * 0.037).cos()).abs() * 0.48;
                        (0.5 - a - 0.01, 0.5 + a + 0.01)
                    })
                    .collect();
                p.columns(Rect::new(350.0, 20.0, 180.0, 60.0), &wave, 0.0, accent, 1.0);
                let bars: Vec<(f32, f32)> = (0..16)
                    .map(|i| (0.0, 0.15 + 0.8 * ((i as f32 * 0.9).sin().abs())))
                    .collect();
                p.columns(Rect::new(350.0, 96.0, 180.0, 40.0), &bars, 3.0, fg, 0.8);
                p.gradient(
                    Rect::new(550.0, 20.0, 150.0, 50.0),
                    8.0,
                    accent,
                    bg,
                    Along::X,
                    1.0,
                );
                p.gradient(
                    Rect::new(550.0, 84.0, 150.0, 50.0),
                    8.0,
                    fg,
                    bg,
                    Along::Y,
                    0.6,
                );
                let heart = Path::svg(
                    "M40 150C30 138 20 146 20 156C20 168 34 176 40 184C46 176 60 168 60 156C60 146 50 138 40 150Z",
                );
                p.fill_path(&heart, [220, 60, 80], 1.0);
                let squiggle = Path::new()
                    .move_to(90.0, 170.0)
                    .curve_to((110.0, 130.0), (130.0, 210.0), 150.0, 170.0)
                    .quad_to(165.0, 140.0, 180.0, 170.0);
                p.stroke_path(&squiggle, 3.0, fg, 1.0);
                p.text(
                    Face::Ui,
                    11.0,
                    200.0,
                    170.0,
                    "image  ring  arc  columns  gradient  path",
                    fg,
                    400.0,
                );
                p.restore_offset(o);
            }
            let out =
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/specimen");
            bmp.write_png(&out.join(file)).unwrap();
        }
    }
}
