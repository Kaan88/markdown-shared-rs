//! The terminal's own background opacity, independent of the chrome's key and veil. Background
//! runs replace pixels with premultiplied colour before the ink is composited over them.

use crate::cover::luminance;
use crate::frost::avoid_mark;
use crate::gfx::{Painter, Rgb};
use mdshared_text::bitmap::blend_premultiplied;

/// How much of the terminal's default background to keep over the window backdrop. Pass this
/// to [`super::draw_screen_with_background`] or [`super::draw_rows_with_background`].
///
/// The result is already premultiplied. The toolkit's ordinary scrollbars (including animated
/// and horizontal variants) and [`Painter::text`] are not safe directly over those pixels.
/// Reserve an opaque gutter for a scrollbar or paint an opaque backing under the overlay,
/// including its antialiased edges, before drawing it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BackgroundOpacity {
    requested: u8,
    pub(super) padding: bool,
}

impl BackgroundOpacity {
    /// Requests an opacity from 0 (all backdrop) to 1 (the original opaque drawing). Finite
    /// values outside that range are clamped; non-finite values give the original drawing.
    /// The renderer may raise the opacity to protect text contrast. Padding stays opaque.
    pub fn new(opacity: f32) -> Self {
        let requested = if opacity.is_finite() {
            (opacity.clamp(0.0, 1.0) * 255.0).round() as u8
        } else {
            255
        };
        Self {
            requested: avoid_mark(requested),
            padding: false,
        }
    }

    /// Makes the slack at the right and bottom of the grid use the same opacity as a blank
    /// default cell when `on` is true. There is no change to hit testing or layout.
    pub fn with_padding(mut self, on: bool) -> Self {
        self.padding = on;
        self
    }

    pub(super) fn active(self, p: &Painter) -> Option<Self> {
        (self.requested < 255 && p.frost_key().is_some()).then_some(self)
    }

    pub(super) fn for_ink(self, background: Rgb, ink: Rgb) -> u8 {
        let ink = luminance(ink);
        // Black and white bound each channel and therefore the luminance of any desktop.
        // Check the *nearer* end, not just contrast against white and black separately: if
        // the interval contains the ink, some intermediate desktop can match it.
        let safe = |a| {
            let lo = background.map(|c| premul(c, a));
            let hi = lo.map(|c| c + (255 - a));
            let (lo, hi) = (luminance(lo), luminance(hi));
            (ink + 0.05) >= (hi + 0.05) * 4.5 || (lo + 0.05) >= (ink + 0.05) * 4.5
        };
        if safe(self.requested) {
            return self.requested;
        }
        // Don't recolour program-supplied ink that already fails against the solid scheme.
        // Keep its original background rather than making its contrast any less predictable.
        if !safe(255) {
            return 255;
        }
        let (mut low, mut high) = (self.requested + 1, 255);
        while low < high {
            let mid = low + (high - low) / 2;
            if safe(mid) {
                high = mid;
            } else {
                low = mid + 1;
            }
        }
        avoid_mark(low)
    }
}

fn premul(channel: u8, alpha: u8) -> u8 {
    ((u16::from(channel) * u16::from(alpha) + 127) / 255) as u8
}

/// Replaces an already clipped device rectangle instead of stacking on last frame's alpha.
pub(super) fn replace(p: &mut Painter, rect: (i32, i32, i32, i32), [r, g, b]: Rgb, alpha: u8) {
    let pixel = [premul(b, alpha), premul(g, alpha), premul(r, alpha), alpha];
    for row in rows(p, rect) {
        for px in row.as_chunks_mut::<4>().0 {
            *px = pixel;
        }
    }
}

/// Makes sure the chrome key pass does not mistake already composited ink for chrome.
pub(super) fn finish(p: &mut Painter, rect: (i32, i32, i32, i32)) {
    for row in rows(p, rect) {
        for px in row.as_chunks_mut::<4>().0 {
            px[3] = avoid_mark(px[3]);
        }
    }
}

/// Blends a partly covered box-drawing piece. On opaque pixels keep the painter's exact
/// rounding; on translucent pixels the source covers RGB and alpha together.
pub(super) fn blend_piece(p: &mut Painter, rect: (i32, i32, i32, i32), rgb: Rgb, alpha: f32) {
    let coverage = (alpha * 255.0).round() as u8;
    let opaque = (alpha * 256.0) as u32;
    for row in rows(p, rect) {
        for px in row.as_chunks_mut::<4>().0 {
            if px[3] == 255 {
                for (d, s) in px.iter_mut().zip(rgb.into_iter().rev()) {
                    *d = ((u32::from(s) * opaque + u32::from(*d) * (256 - opaque)) >> 8) as u8;
                }
            } else {
                blend_premultiplied(px, rgb, coverage);
            }
        }
    }
}

fn rows<'a>(
    p: &'a mut Painter,
    (x0, y0, x1, y1): (i32, i32, i32, i32),
) -> impl Iterator<Item = &'a mut [u8]> {
    let width = p.bmp.width as i32;
    let (x0, x1) = (x0.clamp(0, width), x1.clamp(0, width));
    let stride = p.bmp.width.max(1) as usize * 4;
    p.bmp
        .pixels
        .chunks_exact_mut(stride)
        .take(y1.max(0) as usize)
        .skip(y0.max(0) as usize)
        .map(move |row| &mut row[x0 as usize * 4..x1.max(x0) as usize * 4])
}
