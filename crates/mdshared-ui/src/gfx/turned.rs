//! An icon turned about its centre: [`Painter::icon_rotated`], for a chevron that turns as a
//! folder opens (`glyph::draw_disclosure`).
//!
//! The glyph's path is turned before it is stroked, not its bitmap afterwards, so a turned icon
//! is as crisp as an upright one: the stroke is rasterised once more at the new angle with the
//! same width, caps and corners [`Painter::icon`] gives it. At no turn (0 degrees, or a whole
//! number of turns) it is `icon` itself, the same cached mask and the same pixels.
//!
//! A turned mask is not cached. An icon is turned while it animates, and every frame of the
//! animation asks for another angle; caching each would grow the cache with masks that are
//! never asked for again (the cache is only cleared at 500 entries). Stroking one small glyph a
//! frame for the few icons that are turning just then costs little, and the cache keeps only
//! upright icons as it did before.
//!
//! Deliberately not here: turning a bitmap (a picture, a text), mirroring, and any angle the
//! caller does not give: this draws one icon at one angle, and the animation belongs to the
//! caller (or to `glyph::draw_disclosure_animated`).

use super::{Gfx, IconMask, Painter, Rgb};

impl Gfx {
    /// The mask of an icon `side` device px across, turned `degrees` clockwise on the screen
    /// about the centre of its square. Not cached: see the module doc.
    fn icon_turned(&mut self, glyph: &str, side: f32, degrees: f32) -> IconMask {
        let s = side / crate::glyph::GRID;
        let c = side / 2.0;
        let (sin, cos) = degrees.to_radians().sin_cos();
        // The grid scaled to device px, then turned about the square's centre. With y going
        // down the screen, this turns clockwise: the point right of the centre goes below it.
        // zeno maps (x, y) to (x * xx + y * yx + dx, x * xy + y * yy + dy).
        let t = zeno::Transform::new(
            s * cos,
            s * sin,
            -s * sin,
            s * cos,
            c - c * cos + c * sin,
            c - c * sin - c * cos,
        );
        let mut stroke = zeno::Stroke::new(crate::glyph::STROKE);
        stroke.cap(zeno::Cap::Round).join(zeno::Join::Round);
        let (data, at) = zeno::Mask::new(glyph)
            .style(stroke)
            .transform(Some(t))
            .render();
        IconMask {
            left: at.left,
            top: at.top,
            width: at.width as usize,
            data,
        }
    }
}

impl Painter<'_> {
    /// An icon from [`glyph`](crate::glyph), `size` logical px across, centred on a point and
    /// turned `degrees` clockwise about its centre, in colour `c`.
    ///
    /// At 0 degrees, or any whole number of turns (360, -720, ...), this is
    /// [`icon`](Self::icon) exactly, pixel for pixel. At any other angle the glyph's path is
    /// turned and then stroked, so the lines stay as sharp as an upright icon's; the mask is
    /// not cached (see the module doc). A turned icon can reach a little outside the square an
    /// upright one keeps to (a square's corner turned by 45 degrees), but the glyphs keep well
    /// inside their grid. An angle or a size that is not a number draws nothing; so does a size
    /// that is not positive.
    pub fn icon_rotated(&mut self, glyph: &str, size: f32, cx: f32, cy: f32, degrees: f32, c: Rgb) {
        if !degrees.is_finite() {
            return;
        }
        let turn = degrees.rem_euclid(360.0);
        // rem_euclid can answer 360 for a tiny negative angle; both ends are no turn at all.
        if turn == 0.0 || turn == 360.0 {
            self.icon(glyph, size, cx, cy, c);
            return;
        }
        let side = size * self.scale;
        if !(side.is_finite() && side > 0.0) {
            return;
        }
        let mask = self.gfx.icon_turned(glyph, side, turn);
        self.blit_icon_mask(&mask, side, cx, cy, c);
    }

    /// Blends an icon's mask in colour `c`, its square `side` device px across centred on the
    /// logical point `cx, cy`, inside the clip and moved by the offset. The one blend both
    /// [`icon`](Self::icon) and [`icon_rotated`](Self::icon_rotated) use.
    pub(super) fn blit_icon_mask(&mut self, mask: &IconMask, side: f32, cx: f32, cy: f32, c: Rgb) {
        if mask.width == 0 {
            return;
        }
        let x0 = (cx * self.scale - side / 2.0).round() as i32 + mask.left + self.origin.0;
        let y0 = (cy * self.scale - side / 2.0).round() as i32 + mask.top + self.origin.1;
        let (xa, xb) = (
            x0.max(self.clip.0),
            (x0 + mask.width as i32).min(self.clip.2),
        );
        if xa >= xb {
            return;
        }
        for (row, cover) in mask.data.chunks_exact(mask.width).enumerate() {
            let y = y0 + row as i32;
            if y < self.clip.1 || y >= self.clip.3 {
                continue;
            }
            let o = (y as usize * self.bmp.width as usize + xa as usize) * 4;
            let dst = &mut self.bmp.pixels[o..o + (xb - xa) as usize * 4];
            let cover = &cover[(xa - x0) as usize..(xb - x0) as usize];
            for (d, &a) in dst.as_chunks_mut::<4>().0.iter_mut().zip(cover) {
                // 255 has to come out as the colour itself.
                let a = a as u32 + (a as u32 >> 7);
                let ia = 256 - a;
                d[0] = ((c[2] as u32 * a + d[0] as u32 * ia) >> 8) as u8;
                d[1] = ((c[1] as u32 * a + d[1] as u32 * ia) >> 8) as u8;
                d[2] = ((c[0] as u32 * a + d[2] as u32 * ia) >> 8) as u8;
            }
        }
    }
}

#[cfg(test)]
#[path = "turned_tests.rs"]
mod tests;
