//! The terminal's own background opacity, independent of the chrome's key and veil. While a
//! row is drawn its RGB is straight, so the usual glyph masks blend against the flat colour.
//! The alpha byte holds the desired opacity until the row is finished and premultiplied.

use crate::cover::luminance;
use crate::frost::MARK;
use crate::gfx::{Painter, Rgb};

/// How much of the terminal's default background to keep over the window backdrop. Pass this
/// to [`super::draw_screen_with_background`] or [`super::draw_rows_with_background`].
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

/// Alpha 254 belongs to the host's chrome, not to an already composited pixel.
fn avoid_mark(alpha: u8) -> u8 {
    if alpha == MARK { 255 } else { alpha }
}

fn premul(channel: u8, alpha: u8) -> u8 {
    ((u16::from(channel) * u16::from(alpha) + 127) / 255) as u8
}

/// Sets alpha in an already clipped device rectangle, keeping the colours straight for ink.
pub(super) fn mark(p: &mut Painter, rect: (i32, i32, i32, i32), alpha: u8) {
    for row in rows(p, rect) {
        for px in row.as_chunks_mut::<4>().0 {
            px[3] = alpha;
        }
    }
}

/// Finishes a device rectangle after the ink has been drawn. Existing `key_out` leaves these
/// pixels alone and the Windows host passes them through to DWM unchanged.
pub(super) fn finish(p: &mut Painter, rect: (i32, i32, i32, i32)) {
    for row in rows(p, rect) {
        for px in row.as_chunks_mut::<4>().0 {
            let alpha = px[3];
            if alpha != 255 {
                for c in &mut px[..3] {
                    *c = premul(*c, alpha);
                }
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
