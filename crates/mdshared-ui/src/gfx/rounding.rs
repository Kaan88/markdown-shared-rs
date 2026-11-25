//! Rounding off what was drawn square: a pane whose content (rows, fills, other widgets) is
//! drawn edge to edge in a rectangle, then given round corners and a hairline border after,
//! as a web page's `border-radius` with `overflow: hidden` and a `border` shows a card.
//!
//! [`Painter::round_off`] paints a colour over what lies outside the rounded corners, with the
//! antialiased edge [`Painter::rbox`] draws, so content and an `rbox` of the same rectangle
//! and radius meet without a seam. [`Painter::outline`] draws the border alone, a ring one
//! device pixel wide, inside the rectangle. Neither touches anything else: opt-in, and no
//! existing drawing changes.

use super::{Painter, Rect, Rgb};

/// How much of the pixel centred at `fx, fy` lies inside the rounded rectangle `x0..x1`,
/// `y0..y1` of corner radius `rad` (device px): 1 inside, 0 outside, a ramp one pixel wide
/// across the arc, as `rbox` draws it.
fn cover(x0: f32, y0: f32, x1: f32, y1: f32, rad: f32, fx: f32, fy: f32) -> f32 {
    if fx < x0 || fx > x1 || fy < y0 || fy > y1 {
        return 0.0;
    }
    let qx = if fx < x0 + rad {
        x0 + rad - fx
    } else if fx > x1 - rad {
        fx - (x1 - rad)
    } else {
        0.0
    };
    let qy = if fy < y0 + rad {
        y0 + rad - fy
    } else if fy > y1 - rad {
        fy - (y1 - rad)
    } else {
        0.0
    };
    if qx <= 0.0 || qy <= 0.0 {
        return 1.0;
    }
    (rad - qx.hypot(qy) + 0.5).clamp(0.0, 1.0)
}

/// The device pixels along a rectangle's edges, `band` deep, each once: whole rows at the top
/// and bottom, then the left and right columns between them.
fn edge_pixels(x0: i32, y0: i32, x1: i32, y1: i32, band: i32) -> Vec<(i32, i32)> {
    let mut out = Vec::new();
    let top = y0..(y0 + band).min(y1);
    let bottom = (y1 - band).max(top.end)..y1;
    for y in top.clone().chain(bottom.clone()) {
        for x in x0..x1 {
            out.push((x, y));
        }
    }
    let left = x0..(x0 + band).min(x1);
    let right = (x1 - band).max(left.end)..x1;
    for y in top.end..bottom.start {
        for x in left.clone().chain(right.clone()) {
            out.push((x, y));
        }
    }
    out
}

impl Painter<'_> {
    /// Paints `outside` over the parts of `r` that lie outside its rounded corners of `radius`
    /// (logical px): drawn after a pane's content, it rounds the content off. The radius is held
    /// to half the shorter side, as `rbox` holds it; 0 or a number that is not finite does
    /// nothing.
    pub fn round_off(&mut self, r: Rect, radius: f32, outside: Rgb) {
        let (x0, y0, x1, y1) = self.dev_rect(r);
        let Some(rad) = self.device_radius(x0, y0, x1, y1, radius) else {
            return;
        };
        let band = rad.ceil() as i32;
        let (fx0, fy0, fx1, fy1) = (x0 as f32, y0 as f32, x1 as f32, y1 as f32);
        for (x, y) in edge_pixels(x0, y0, x1, y1, band) {
            let c = cover(fx0, fy0, fx1, fy1, rad, x as f32 + 0.5, y as f32 + 0.5);
            if c < 1.0 {
                self.span(y, x, x + 1, outside, 1.0 - c);
            }
        }
    }

    /// A ring one device pixel wide just inside the rounded rectangle `r` of `radius` (logical
    /// px), in `c`, leaving the inside as it is: a card's border drawn over its content. A
    /// radius that is not finite is taken as 0.
    pub fn outline(&mut self, r: Rect, radius: f32, c: Rgb) {
        let (x0, y0, x1, y1) = self.dev_rect(r);
        if x1 <= x0 || y1 <= y0 {
            return;
        }
        let rad = self.device_radius(x0, y0, x1, y1, radius).unwrap_or(0.0);
        let inner = (rad - 1.0).max(0.0);
        let band = (rad.ceil() as i32).max(1);
        let (fx0, fy0, fx1, fy1) = (x0 as f32, y0 as f32, x1 as f32, y1 as f32);
        for (x, y) in edge_pixels(x0, y0, x1, y1, band) {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            let outer = cover(fx0, fy0, fx1, fy1, rad, fx, fy);
            let inside = cover(fx0 + 1.0, fy0 + 1.0, fx1 - 1.0, fy1 - 1.0, inner, fx, fy);
            let a = (outer - inside).clamp(0.0, 1.0);
            if a > 0.0 {
                self.span(y, x, x + 1, c, a);
            }
        }
    }

    /// `radius` in device px for the device rectangle given, held to half its shorter side;
    /// none when it rounds nothing.
    fn device_radius(&self, x0: i32, y0: i32, x1: i32, y1: i32, radius: f32) -> Option<f32> {
        if x1 <= x0 || y1 <= y0 || !radius.is_finite() {
            return None;
        }
        let rad = (radius * self.scale)
            .min((x1 - x0) as f32 / 2.0)
            .min((y1 - y0) as f32 / 2.0);
        (rad > 0.0).then_some(rad)
    }
}

#[cfg(test)]
mod tests {
    use super::super::Gfx;
    use super::*;
    use mdshared_text::Bitmap;

    const BG: Rgb = [10, 20, 30];
    const FILL: Rgb = [200, 100, 50];
    const LINE: Rgb = [90, 90, 90];

    fn at(b: &Bitmap, x: usize, y: usize) -> Rgb {
        let i = (y * b.width as usize + x) * 4;
        [b.pixels[i + 2], b.pixels[i + 1], b.pixels[i]]
    }

    fn draw(scale: f32, f: impl FnOnce(&mut Painter)) -> Bitmap {
        let mut bmp = Bitmap::new((40.0 * scale) as u32, (30.0 * scale) as u32);
        let mut gfx = Gfx::new();
        f(&mut Painter::new(&mut bmp, &mut gfx, scale));
        bmp
    }

    #[test]
    fn rounding_off_square_content_matches_a_rounded_box_pixel_for_pixel() {
        for scale in [1.0, 1.5, 2.0] {
            let r = Rect::new(4.0, 3.0, 30.0, 20.0);
            let rounded = draw(scale, |p| {
                p.fill(Rect::new(0.0, 0.0, 40.0, 30.0), BG);
                p.fill(r, FILL);
                p.round_off(r, 8.0, BG);
            });
            let boxed = draw(scale, |p| {
                p.fill(Rect::new(0.0, 0.0, 40.0, 30.0), BG);
                p.rbox(r, 8.0, FILL);
            });
            // The same coverage blended the other way round: at most a rounding step apart.
            for (a, b) in rounded.pixels.iter().zip(&boxed.pixels) {
                assert!(a.abs_diff(*b) <= 2, "at {scale}: {a} against {b}");
            }
            // The corner itself is the outside colour, the middle of an edge the content.
            // (The rectangle's first device row and column, as the painter rounds them.)
            let (x0, y0) = (
                (4.0 * scale).round() as usize,
                (3.0 * scale).round() as usize,
            );
            assert_eq!(at(&rounded, x0, y0), BG);
            assert_eq!(at(&rounded, (19.0 * scale) as usize, y0), FILL);
        }
    }

    #[test]
    fn rounding_off_leaves_everything_inside_the_arcs_alone() {
        let r = Rect::new(4.0, 3.0, 30.0, 20.0);
        let before = draw(1.0, |p| p.fill(r, FILL));
        let after = draw(1.0, |p| {
            p.fill(r, FILL);
            p.round_off(r, 8.0, BG);
        });
        // Outside the corner squares nothing changed.
        for y in 0..30 {
            for x in 0..40 {
                let corner = !(12..26).contains(&x) && !(11..15).contains(&y);
                if !corner {
                    assert_eq!(at(&before, x, y), at(&after, x, y), "{x},{y}");
                }
            }
        }
    }

    #[test]
    fn an_outline_is_a_ring_one_pixel_wide_inside_its_rectangle() {
        let r = Rect::new(4.0, 3.0, 30.0, 20.0);
        let b = draw(1.0, |p| {
            p.fill(Rect::new(0.0, 0.0, 40.0, 30.0), FILL);
            p.outline(r, 6.0, LINE);
        });
        // The straight edges: the first and last pixel rows and columns of the rectangle.
        assert_eq!(at(&b, 19, 3), LINE, "top");
        assert_eq!(at(&b, 19, 22), LINE, "bottom");
        assert_eq!(at(&b, 4, 13), LINE, "left");
        assert_eq!(at(&b, 33, 13), LINE, "right");
        // Inside and outside it, nothing.
        assert_eq!(at(&b, 19, 4), FILL);
        assert_eq!(at(&b, 5, 13), FILL);
        assert_eq!(at(&b, 19, 2), FILL);
        assert_eq!(at(&b, 34, 13), FILL);
        // The corner square's own corner is outside the arc.
        assert_eq!(at(&b, 4, 3), FILL);
    }

    #[test]
    fn a_radius_of_nothing_rounds_nothing_and_an_outline_is_then_square() {
        let r = Rect::new(4.0, 3.0, 30.0, 20.0);
        let plain = draw(1.0, |p| p.fill(r, FILL));
        for radius in [0.0, -3.0, f32::NAN, f32::INFINITY] {
            let b = draw(1.0, |p| {
                p.fill(r, FILL);
                p.round_off(r, radius, BG);
            });
            assert_eq!(b.pixels, plain.pixels, "{radius}");
        }
        let b = draw(1.0, |p| p.outline(r, f32::NAN, LINE));
        assert_eq!(at(&b, 4, 3), LINE, "a square corner");
        // An empty rectangle draws nothing.
        let empty = draw(1.0, |p| {
            p.round_off(Rect::new(5.0, 5.0, 0.0, 10.0), 4.0, BG);
            p.outline(Rect::new(5.0, 5.0, 10.0, -1.0), 4.0, LINE);
        });
        assert!(empty.pixels.iter().all(|v| *v == 0));
    }
}
