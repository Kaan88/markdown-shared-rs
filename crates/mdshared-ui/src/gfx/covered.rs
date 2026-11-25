//! A shadow under a card: [`Painter::shadow_with`] as it looks once an opaque
//! [`frame`](Painter::frame) of the same rectangle is drawn over it, at a fraction of the cost.
//!
//! A shadow is ten translucent boxes, each blended over its whole area, the inside included.
//! When a card is framed right after, the frame's fill overwrites the inside with alpha 1, so
//! all the blending there is lost work: under a large dialog it is most of a frame's time. The
//! shadow drawn here leaves out a band of the card where the frame is known to overwrite every
//! pixel, and draws the rest through clips around it. Every pixel the shadow still blends is
//! blended exactly as `shadow_with` blends it, in the same order, so once the frame is drawn
//! the bitmap is byte for byte what `shadow_with` followed by `frame` gives.
//!
//! Deliberately not here: a public entry point. The saving holds only when the caller draws
//! `frame(r, radius, ..)` next with the same rectangle, radius, clip and offset; inside the
//! crate the callers are the dialog card, the toast and the tip of [`crate::overlay`], which do.

use super::{Painter, Rect};

#[cfg(test)]
thread_local! {
    /// Tests set this to have [`Painter::shadow_under_frame`] draw the whole shadow, as the
    /// callers drew it before, so that a widget's drawing can be compared with the old one.
    pub(crate) static WHOLE_SHADOW: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

impl Painter<'_> {
    /// [`shadow_with`](Self::shadow_with)`(r, radius, dark, k)` for a card that
    /// [`frame`](Self::frame)`(r, radius, ..)` draws over next: after the frame, the pixels are
    /// those of the whole shadow and the frame, but the shadow is not blended where the frame's
    /// fill covers fully.
    ///
    /// Why the band left out is covered: `frame` fills the device box `(x0 + 1, y0 + 1, x1 -
    /// 1, y1 - 1)` with corners of radius `rad`, the frame's radius in device px less one, held
    /// to half the box as [`rbox_dev`](Self::rbox_dev) holds it. That fill writes a row whose
    /// centre lies more than `rad` from both the box's top and bottom as one span of alpha 1
    /// across the box's whole width. With `band = ceil(rad)`, every row from `y0 + 1 + band`
    /// to `y1 - 1 - band` (exclusive) is such a row, at any scale and any device rounding of
    /// `r`, because the band is worked out from the same device box `frame` uses. A pixel
    /// outside the clip is written by neither call. So the band `(x0 + 1, y0 + 1 + band, x1 -
    /// 1, y1 - 1 - band)` is left out; when it is empty the whole shadow is drawn.
    pub(crate) fn shadow_under_frame(&mut self, r: Rect, radius: f32, dark: bool, k: f32) {
        #[cfg(test)]
        if WHOLE_SHADOW.with(|w| w.get()) {
            return self.shadow_with(r, radius, dark, k);
        }
        let (x0, y0, x1, y1) = self.dev_rect(r);
        // The fill's device box and radius, worked out as `frame` and `rbox_dev` work them out.
        let (fx0, fy0, fx1, fy1) = (x0 + 1, y0 + 1, x1 - 1, y1 - 1);
        let rad = (radius * self.scale - 1.0)
            .max(0.0)
            .min((fx1 - fx0) as f32 / 2.0)
            .min((fy1 - fy0) as f32 / 2.0)
            .max(0.0);
        let band = rad.ceil() as i32;
        let (ex0, ey0, ex1, ey1) = (fx0, fy0 + band, fx1, fy1 - band);
        if ex1 <= ex0 || ey1 <= ey0 {
            return self.shadow_with(r, radius, dark, k);
        }
        // The clip less the band, as four disjoint pieces: above, below, left of, right of it.
        let c = self.clip;
        let pieces = [
            (c.0, c.1, c.2, c.3.min(ey0)),
            (c.0, c.1.max(ey1), c.2, c.3),
            (c.0, c.1.max(ey0), c.2.min(ex0), c.3.min(ey1)),
            (c.0.max(ex1), c.1.max(ey0), c.2, c.3.min(ey1)),
        ];
        for piece in pieces {
            if piece.2 > piece.0 && piece.3 > piece.1 {
                self.clip = piece;
                self.shadow_with(r, radius, dark, k);
            }
        }
        self.clip = c;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gfx::Gfx;
    use mdshared_text::Bitmap;

    const SCALES: [f32; 5] = [1.0, 1.25, 1.5, 1.75, 2.0];

    /// A background with something in every channel, so that a blend shows.
    fn ground(w: u32, h: u32) -> Bitmap {
        let mut bmp = Bitmap::new(w, h);
        for (i, px) in bmp.pixels.chunks_mut(4).enumerate() {
            let v = (i * 37 % 251) as u8;
            px.copy_from_slice(&[v, 255 - v, v / 2 + 60, 255]);
        }
        bmp
    }

    fn drawn(scale: f32, whole: bool, f: impl Fn(&mut Painter)) -> Vec<u8> {
        let mut bmp = ground(220, 200);
        let mut gfx = Gfx::new();
        WHOLE_SHADOW.with(|w| w.set(whole));
        f(&mut Painter::new(&mut bmp, &mut gfx, scale));
        WHOLE_SHADOW.with(|w| w.set(false));
        bmp.pixels
    }

    #[test]
    fn a_framed_shadow_is_the_whole_shadow_and_the_frame_pixel_for_pixel() {
        let rects = [
            Rect::new(20.3, 17.6, 70.45, 55.15),
            Rect::new(8.0, 9.0, 40.0, 30.0),
            Rect::new(31.7, 22.2, 13.3, 9.9),
            Rect::new(5.5, 4.5, 3.0, 2.0),
            Rect::new(-12.25, -8.75, 60.6, 40.4),
            Rect::new(90.1, 70.9, 80.0, 90.0),
        ];
        let clips = [None, Some(Rect::new(10.5, 12.25, 50.0, 41.75))];
        for scale in SCALES {
            for r in rects {
                for radius in [0.0, 1.0, 5.0, 8.0, 10.0, 40.0, f32::NAN] {
                    for (dark, k) in [(false, 1.0), (true, 1.0), (true, 2.5), (false, 0.0)] {
                        for clip in clips {
                            for offset in [(0.0, 0.0), (3.3, -2.6)] {
                                let draw = |p: &mut Painter| {
                                    p.offset(offset.0, offset.1);
                                    if let Some(c) = clip {
                                        p.clip(c);
                                    }
                                    p.shadow_under_frame(r, radius, dark, k);
                                    p.frame(r, radius, [250, 248, 240], [90, 90, 96]);
                                };
                                assert!(
                                    drawn(scale, false, draw) == drawn(scale, true, draw),
                                    "{r:?} radius {radius} at {scale}, dark {dark}, k {k}, \
                                     clip {clip:?}, offset {offset:?}"
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn the_clip_is_given_back_as_it_was() {
        let mut bmp = Bitmap::new(100, 100);
        let mut gfx = Gfx::new();
        let mut p = Painter::new(&mut bmp, &mut gfx, 1.5);
        p.clip(Rect::new(3.0, 4.0, 60.0, 70.0));
        let before = p.clip;
        p.shadow_under_frame(Rect::new(10.0, 10.0, 40.0, 40.0), 10.0, true, 1.0);
        assert_eq!(p.clip, before);
    }
}
