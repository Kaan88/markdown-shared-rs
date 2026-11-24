//! Soft drop shadows as a web page's `box-shadow` draws them: the box, moved, blurred by a
//! Gaussian whose deviation is half the blur, darkening only what lies outside the box. A
//! shadow has one or more layers ([`Shade`]), each `dx dy blur alpha` as CSS writes them, so a
//! look designed in a browser can be drawn here as it was seen there.
//!
//! [`Painter::soft_shadow`] works the blur out exactly, not with stacked boxes: a box's
//! blur is the product of two error functions, one across and one down, and a rounded corner
//! takes away the blur of the little piece the curve cuts off the square corner, summed over
//! a few strips of it. Everything a row or a column shares is computed once for it, so each
//! pixel costs a few multiplications per layer and is blended once, rounded to the nearest
//! value: a shadow never sinks a dark page to black the way many faint boxes, each rounded
//! down, do.
//!
//! The pixels inside the box are not touched, as CSS leaves them, and that is also where most
//! of a large card's area is: the caller draws the box over it next. A pixel the box's rounded
//! corner covers in part gets the shadow for the part it leaves.
//!
//! Deliberately not here: coloured shadows (they are black, at an alpha), a spread, and inset
//! shadows. [`Painter::shadow_with`] and the shadows of the existing widgets are unchanged; a
//! widget draws this shadow only when its caller asks (`menu::Look::soft_shadow`,
//! `settings::Style`).

use super::{Painter, Rect};

/// One layer of a soft shadow, in logical px: what CSS writes as
/// `box-shadow: <dx> <dy> <blur> rgb(0 0 0 / <alpha>)`. The blur is CSS's: the Gaussian's
/// standard deviation is half of it, and the shadow fades out about one and a half blurs
/// from the box's edge. `alpha` is the darkness of the shadow where it is whole, 0 to 1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shade {
    pub dx: f32,
    pub dy: f32,
    pub blur: f32,
    pub alpha: f32,
}

impl Shade {
    pub const fn new(dx: f32, dy: f32, blur: f32, alpha: f32) -> Shade {
        Shade {
            dx,
            dy,
            blur,
            alpha,
        }
    }

    /// A popup's shadow (a menu, a dialog card): a wide one well below the box and a tight one
    /// just under it, `0 8px 24px 55%, 0 2px 6px 35%`. At strength 1 it is the darkest the
    /// popup should cast; a look's shadow setting scales it.
    pub const POPUP: [Shade; 2] = [
        Shade::new(0.0, 8.0, 24.0, 0.55),
        Shade::new(0.0, 2.0, 6.0, 0.35),
    ];

    /// The shadow of a control a menu hangs from ([`crate::menu::join_under`]): `0 2px 10px 45%`.
    pub const TAB: [Shade; 1] = [Shade::new(0.0, 2.0, 10.0, 0.45)];
}

/// What a shadow's strength is multiplied by on a light palette: a black shadow shows more on
/// a light page, so the same look casts it under half as dark there.
pub const LIGHT_SHADOW: f32 = 0.45;

/// The largest blur and offset a layer is drawn with, in logical px; a larger one is taken as
/// this, so a hand-edited look cannot make a shadow cover the screen.
const MAX_REACH: f32 = 200.0;

/// How far a layer is drawn past its box, in standard deviations: beyond it a layer of alpha 1
/// darkens by under 0.4 of 255.
const SIGMAS: f32 = 3.0;

/// The most strips a rounded corner is cut into.
const MAX_STRIPS: usize = 48;

/// Everything [`Painter::soft_shadow`] with these layers can touch around `r`, in logical px:
/// for the damage rectangle of something that fades or moves with its shadow. An empty
/// rectangle at `r` when no layer draws.
pub fn soft_bounds(r: Rect, layers: &[Shade]) -> Rect {
    let (mut x0, mut y0, mut x1, mut y1) = (r.x, r.y, r.right(), r.bottom());
    for s in layers.iter().filter_map(|s| s.sane()) {
        let reach = s.blur / 2.0 * SIGMAS + 1.0;
        x0 = x0.min(r.x + s.dx - reach);
        y0 = y0.min(r.y + s.dy - reach);
        x1 = x1.max(r.right() + s.dx + reach);
        y1 = y1.max(r.bottom() + s.dy + reach);
    }
    let (x0, y0) = (x0.floor(), y0.floor());
    Rect::new(x0, y0, x1.ceil() - x0, y1.ceil() - y0)
}

impl Shade {
    /// The layer with its blur and offsets held to [`MAX_REACH`], or None when it draws
    /// nothing: a number that is not finite, or an alpha of 0 or less.
    fn sane(self) -> Option<Shade> {
        let finite = [self.dx, self.dy, self.blur, self.alpha]
            .iter()
            .all(|v| v.is_finite());
        if !finite || self.alpha <= 0.0 {
            return None;
        }
        Some(Shade {
            dx: self.dx.clamp(-MAX_REACH, MAX_REACH),
            dy: self.dy.clamp(-MAX_REACH, MAX_REACH),
            blur: self.blur.clamp(0.0, MAX_REACH),
            alpha: self.alpha.min(1.0),
        })
    }
}

/// The standard normal distribution's cumulative function, within 2e-7 (Abramowitz and Stegun's
/// 7.1.26 for the error function). 0 below -6 and 1 above 6.
fn phi(t: f32) -> f32 {
    if t <= -6.0 {
        return 0.0;
    }
    if t >= 6.0 {
        return 1.0;
    }
    let z = t.abs() * std::f32::consts::FRAC_1_SQRT_2;
    let u = 1.0 / (1.0 + 0.327_591_1 * z);
    let poly = ((((1.061_405_4 * u - 1.453_152) * u + 1.421_413_8) * u - 0.284_496_74) * u
        + 0.254_829_6)
        * u;
    let erf = 1.0 - poly * (-z * z).exp();
    if t < 0.0 {
        0.5 * (1.0 - erf)
    } else {
        0.5 * (1.0 + erf)
    }
}

/// One layer worked out for the columns and rows of the area drawn: its blur across each
/// column and down each row, and for its rounded corners, each strip's share of each.
struct Layer {
    alpha: f32,
    /// The blur across, per column: how much of a column's Gaussian falls between the box's
    /// sides.
    across: Vec<f32>,
    /// The same down, per row.
    down: Vec<f32>,
    /// Strips each corner is cut into; 0 for a square box.
    strips: usize,
    /// Per column, `strips` values: how much of the column's Gaussian falls on the part of
    /// each strip the curve cuts off, at the left corner and the right one together.
    cut_across: Vec<f32>,
    /// Per row, `strips` values: how much of the row's Gaussian falls on each strip's height,
    /// at the top corners and the bottom ones together.
    cut_down: Vec<f32>,
    /// Per column and per row, whether any strip there counts.
    cut_col: Vec<bool>,
    cut_row: Vec<bool>,
}

/// Below this a strip's share is left out.
const NEGLIGIBLE: f32 = 1e-5;

impl Layer {
    /// The layer for the box `(x0, y0, x1, y1)` (device px, already moved) with corners of
    /// `rad` device px and a deviation of `sigma`, over columns from `cx0` and rows from `cy0`.
    #[allow(clippy::too_many_arguments)] // a box, a blur and an area: no natural grouping
    fn new(
        alpha: f32,
        (x0, y0, x1, y1): (f32, f32, f32, f32),
        rad: f32,
        sigma: f32,
        cx0: i32,
        cols: usize,
        cy0: i32,
        rows: usize,
    ) -> Layer {
        let at = |v: i32, i: usize| (v + i as i32) as f32 + 0.5;
        let across: Vec<f32> = (0..cols)
            .map(|c| {
                let x = at(cx0, c);
                phi((x1 - x) / sigma) - phi((x0 - x) / sigma)
            })
            .collect();
        let down: Vec<f32> = (0..rows)
            .map(|r| {
                let y = at(cy0, r);
                phi((y1 - y) / sigma) - phi((y0 - y) / sigma)
            })
            .collect();
        // The corner the curve cuts off, as strips across: strip k lies `t` into the corner
        // from the box's top (or bottom), `dt` high, and the curve leaves `cut` of it.
        let strips = if rad > 0.0 {
            ((rad / sigma.min(1.0)).ceil() as usize).clamp(1, MAX_STRIPS)
        } else {
            0
        };
        let dt = if strips > 0 { rad / strips as f32 } else { 0.0 };
        let cuts: Vec<(f32, f32)> = (0..strips)
            .map(|k| {
                let t = (k as f32 + 0.5) * dt;
                let h = rad - t;
                (t, rad - (rad * rad - h * h).max(0.0).sqrt())
            })
            .collect();
        let mut cut_across = vec![0.0; cols * strips];
        let mut cut_col = vec![false; cols];
        // Past six deviations from a corner's columns (or rows) a strip's share is 0.
        let far = 6.0 * sigma + rad;
        for c in 0..cols {
            let x = at(cx0, c);
            if strips == 0 || ((x - x0).abs() > far && (x - x1).abs() > far) {
                continue;
            }
            let mut any = false;
            for (k, &(_, cut)) in cuts.iter().enumerate() {
                let left = phi((x0 + cut - x) / sigma) - phi((x0 - x) / sigma);
                let right = phi((x1 - x) / sigma) - phi((x1 - cut - x) / sigma);
                let v = left + right;
                any |= v > NEGLIGIBLE;
                cut_across[c * strips + k] = v;
            }
            cut_col[c] = any;
        }
        let mut cut_down = vec![0.0; rows * strips];
        let mut cut_row = vec![false; rows];
        for r in 0..rows {
            let y = at(cy0, r);
            if strips == 0 || ((y - y0).abs() > far && (y - y1).abs() > far) {
                continue;
            }
            let mut any = false;
            for (k, &(t, _)) in cuts.iter().enumerate() {
                let band = |mid: f32| {
                    phi((mid + dt / 2.0 - y) / sigma) - phi((mid - dt / 2.0 - y) / sigma)
                };
                let v = band(y0 + t) + band(y1 - t);
                any |= v > NEGLIGIBLE;
                cut_down[r * strips + k] = v;
            }
            cut_row[r] = any;
        }
        Layer {
            alpha,
            across,
            down,
            strips,
            cut_across,
            cut_down,
            cut_col,
            cut_row,
        }
    }

    /// How much of the blurred box covers the pixel at column `c`, row `r` of the area, 0 to 1.
    #[inline]
    fn cover(&self, c: usize, r: usize) -> f32 {
        let mut v = self.across[c] * self.down[r];
        if self.cut_row[r] && self.cut_col[c] {
            let n = self.strips;
            let a = &self.cut_across[c * n..c * n + n];
            let d = &self.cut_down[r * n..r * n + n];
            v -= a.iter().zip(d).map(|(a, d)| a * d).sum::<f32>();
        }
        v.clamp(0.0, 1.0)
    }
}

impl Painter<'_> {
    /// A soft drop shadow round the box `r` with corners of `radius`, as CSS's `box-shadow`
    /// draws `layers` (see [`Shade`]), each layer's alpha times `k`: 1 is the layers as given,
    /// 0.5 half as dark. Black, blended once per pixel, outside the box only: the pixels the
    /// box covers wholly are not touched, and one its rounded corner covers in part is
    /// shadowed for the part it leaves, as `rbox`/`frame` of the same box and radius cover it.
    /// So the box is drawn over it next, as CSS draws an element over its shadow. Logical px;
    /// the blur and the offsets are multiplied by the scale, so the shadow at 150% is the
    /// shadow at 100% made larger. It honours the clip and the offset.
    ///
    /// Nothing is drawn for `k` that is 0, negative or not a number, for an empty box, or for
    /// a layer with a number that is not finite or an alpha of 0 or less. `k` is held to 4, a
    /// layer's alpha (times `k`) to 1, its blur and offsets to 200 px. A radius that is not a
    /// number is taken as `rbox` takes it. [`soft_bounds`] says how far it reaches.
    pub fn soft_shadow(&mut self, r: Rect, radius: f32, layers: &[Shade], k: f32) {
        if k.is_nan() || k <= 0.0 {
            return;
        }
        let k = k.min(4.0);
        if ![r.x, r.y, r.w, r.h].iter().all(|v| v.is_finite()) {
            return;
        }
        let (x0, y0, x1, y1) = self.dev_rect(r);
        // A box too large for device px to hold its size draws nothing, like an empty one.
        let (Some(w), Some(h)) = (x1.checked_sub(x0), y1.checked_sub(y0)) else {
            return;
        };
        if w <= 0 || h <= 0 {
            return;
        }
        let scale = if self.scale.is_finite() && self.scale > 0.0 {
            self.scale
        } else {
            1.0
        };
        // The corners' radius, as `rbox_dev` holds it for this box.
        let rad = (radius * scale)
            .min(w as f32 / 2.0)
            .min(h as f32 / 2.0)
            .max(0.0);
        // Each layer's box (moved), its deviation and how far it reaches, in device px.
        let mut boxes = Vec::with_capacity(layers.len());
        let (mut bx0, mut by0, mut bx1, mut by1) = (x0, y0, x1, y1);
        for s in layers.iter().filter_map(|s| s.sane()) {
            let alpha = (s.alpha * k).min(1.0);
            // CSS's blur is twice the deviation; under half a device px it is taken as that.
            let sigma = (s.blur * scale / 2.0).max(0.5);
            let (dx, dy) = (s.dx * scale, s.dy * scale);
            let b = (
                x0 as f32 + dx,
                y0 as f32 + dy,
                x1 as f32 + dx,
                y1 as f32 + dy,
            );
            let reach = SIGMAS * sigma + 1.0;
            bx0 = bx0.min((b.0 - reach).floor() as i32);
            by0 = by0.min((b.1 - reach).floor() as i32);
            bx1 = bx1.max((b.2 + reach).ceil() as i32);
            by1 = by1.max((b.3 + reach).ceil() as i32);
            boxes.push((alpha, b, sigma));
        }
        let c = self.clip;
        let (bx0, by0, bx1, by1) = (bx0.max(c.0), by0.max(c.1), bx1.min(c.2), by1.min(c.3));
        if boxes.is_empty() || bx1 <= bx0 || by1 <= by0 {
            return;
        }
        let (cols, rows) = ((bx1 - bx0) as usize, (by1 - by0) as usize);
        let layers: Vec<Layer> = boxes
            .iter()
            .map(|&(alpha, b, sigma)| Layer::new(alpha, b, rad, sigma, bx0, cols, by0, rows))
            .collect();
        let band = rad.ceil() as i32;
        let span = (bx0, bx1);
        for y in by0..by1 {
            if y < y0 || y >= y1 {
                self.soft_span(&layers, (y, (y - by0) as usize), span, (bx0, bx1), |_| 1.0);
                continue;
            }
            // Beside the box, and the box's corner pixels it covers in part, each with the
            // share of the shadow the box leaves (as `rbox_dev` covers them).
            self.soft_span(&layers, (y, (y - by0) as usize), span, (bx0, x0), |_| 1.0);
            self.soft_span(&layers, (y, (y - by0) as usize), span, (x1, bx1), |_| 1.0);
            let fy = y as f32 + 0.5;
            let qy = if fy < y0 as f32 + rad {
                y0 as f32 + rad - fy
            } else if fy > y1 as f32 - rad {
                fy - (y1 as f32 - rad)
            } else {
                0.0
            };
            if qy > 0.0 && band > 0 {
                let mid = x0 + w / 2;
                self.soft_span(
                    &layers,
                    (y, (y - by0) as usize),
                    span,
                    (x0, (x0 + band).min(mid)),
                    |x| {
                        let qx = (x0 as f32 + rad - (x as f32 + 0.5)).max(0.0);
                        1.0 - (rad - qx.hypot(qy) + 0.5).clamp(0.0, 1.0)
                    },
                );
                self.soft_span(
                    &layers,
                    (y, (y - by0) as usize),
                    span,
                    ((x1 - band).max(mid), x1),
                    |x| {
                        let qx = ((x as f32 + 0.5) - (x1 as f32 - rad)).max(0.0);
                        1.0 - (rad - qx.hypot(qy) + 0.5).clamp(0.0, 1.0)
                    },
                );
            }
        }
    }

    /// Row `y` of a soft shadow from `from.0` to `from.1` (device px, cut to the columns of
    /// the area drawn, `area`, which is inside the clip; `row` is `y`'s row in it), each pixel
    /// darkened by the layers' shadow times its `part`, rounded to the nearest value.
    fn soft_span(
        &mut self,
        layers: &[Layer],
        (y, row): (i32, usize),
        area: (i32, i32),
        from: (i32, i32),
        part: impl Fn(i32) -> f32,
    ) {
        let width = self.bmp.width as usize;
        for x in from.0.max(area.0)..from.1.min(area.1) {
            let part = part(x);
            if part <= 0.0 {
                continue;
            }
            let col = (x - area.0) as usize;
            let mut keep = 1.0;
            for l in layers {
                keep *= 1.0 - l.alpha * l.cover(col, row);
            }
            let a = (((1.0 - keep) * part * 256.0 + 0.5) as u32).min(256);
            if a == 0 {
                continue;
            }
            let o = (y as usize * width + x as usize) * 4;
            for d in &mut self.bmp.pixels[o..o + 3] {
                *d = ((*d as u32 * (256 - a) + 128) >> 8) as u8;
            }
        }
    }
}

#[cfg(test)]
#[path = "soft_tests.rs"]
mod tests;
