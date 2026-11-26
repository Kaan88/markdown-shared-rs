//! A picture prepared once at the size it is drawn, then blitted: what an avatar, an icon or a
//! badge drawn every frame wants. [`Painter::image`] rescales its source on every call; a
//! [`Picture`] keeps the rescaled pixels and the rounded corners of the last size it drew, and
//! draws those again until the target size, the display scale or the corner radius changes.
//!
//! The caller owns the [`Picture`] (one for each picture it shows), the rectangle it is drawn
//! into and its lifetime. [`Picture::draw`] is the only drawing call: it takes the painter, the
//! rectangle in logical px, the corner radius and an alpha. The clip and the painter's offset
//! apply as they do to every other shape, and moving a picture does not prepare it again.
//!
//! [`Picture::load`] reads a file: PNG and JPEG through [`mdshared_text::image`], and any other
//! format the application names in a [`Decoder`] slot (GIF, WebP, ICO, SVG). The slot is asked
//! only when the built-in decoders refuse the bytes as a format they do not read.
//!
//! Deliberately not here: a cache of many pictures (a `Picture` holds the one size it drew
//! last); animation (the first frame of a GIF is what a decoder slot gives); colour management;
//! a decoder for any format of its own, which is the slot's job.

use crate::gfx::{Painter, Rect};
use mdshared_text::image::{Error, Image, Limits};

/// The largest prepared picture, a side in device px and a number of pixels. A picture drawn
/// larger than this is handed to [`Painter::image`] each time instead. 1024 px a side and a
/// million pixels cover the avatars, icons and badges this is for. The prepared buffer itself
/// is at most a few MiB, but `prepare_image` also allocates 16 bytes for each source row the
/// destination reads: a tall source scaled down can still take hundreds of MiB.
const MAX_PREPARED_SIDE: u32 = 1024;
const MAX_PREPARED_PIXELS: u64 = 1_000_000;

/// A decoder an application adds for a format the toolkit does not read: given a whole file
/// and the limits the caller allows, the first frame as straight RGBA, or `None` when it is
/// not one this decoder knows.
///
/// It is asked only after [`mdshared_text::image::decode`] has refused the bytes as
/// [`Error::Unsupported`], so it never overrides PNG or JPEG. The decoder should read the
/// header and check it against `limits` *before* decoding, the way the built-in decoders do:
/// a picture over the limits is refused afterwards as [`Error::TooLarge`], but by then the
/// slot has already allocated it. What it returns is also checked for a length that matches
/// its size, and a panic inside it is [`Error::Damaged`], as a damaged file is.
///
/// A closure `Fn(&[u8], Limits) -> Option<Image>` is a decoder, so a small one needs no
/// struct.
pub trait Decoder {
    /// The picture in `bytes`, or `None` for bytes this decoder does not recognise. `limits`
    /// is the caller's [`Limits`], to be checked before the pixels are allocated.
    fn decode(&self, bytes: &[u8], limits: Limits) -> Option<Image>;
}

impl<F> Decoder for F
where
    F: Fn(&[u8], Limits) -> Option<Image>,
{
    fn decode(&self, bytes: &[u8], limits: Limits) -> Option<Image> {
        self(bytes, limits)
    }
}

/// The pixels prepared at the last target size, and what makes them that size: the device
/// width and height, the display scale's bits and the corner radius in device px's bits.
#[derive(Clone, Debug)]
struct Prepared {
    key: (u32, u32, u32, u32),
    /// Straight RGBA, rounded corners already in alpha.
    rgba: Vec<u8>,
}

/// A picture kept at its drawn size: set once with [`new`](Self::new) or
/// [`load`](Self::load), then [`draw`](Self::draw)n every frame.
///
/// The source pixels are never changed. The first draw at a size prepares the picture at that
/// size in device pixels; later draws at the same size and scale only blend those pixels. A
/// change of target size, display scale or corner radius prepares it again and replaces the
/// old pixels; there is one prepared size at a time, not a cache of sizes. A prepared picture
/// matches what [`Painter::image`] draws at the same size, within the rounding of 8-bit pixels.
#[derive(Clone)]
pub struct Picture {
    image: Image,
    prepared: Option<Prepared>,
    /// How many times a size was prepared; the tests count it.
    prepares: u32,
}

impl std::fmt::Debug for Picture {
    /// The source size and whether a size is prepared, not the pixels of either.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Picture")
            .field("width", &self.image.width)
            .field("height", &self.image.height)
            .field(
                "prepared",
                &self.prepared.as_ref().map(|p| (p.key.0, p.key.1)),
            )
            .field("prepares", &self.prepares)
            .finish()
    }
}

impl Picture {
    /// A picture from its pixels, as [`Image::new`] or a decoder gives them. Nothing is
    /// prepared until the first [`draw`](Self::draw).
    pub fn new(image: Image) -> Picture {
        Picture {
            image,
            prepared: None,
            prepares: 0,
        }
    }

    /// Reads a whole file: PNG and JPEG as [`mdshared_text::image::decode`] reads them; any
    /// other format through `extra` when there is one and the built-in decoders refuse the
    /// bytes as unsupported.
    ///
    /// The built-in [`Limits`] are applied to the built-in decoders and to what the slot
    /// returns: a slot picture over them is [`Error::TooLarge`]. A damaged PNG stays
    /// [`Error::Damaged`]; the slot is not asked to reinterpret it. A slot that panics, or
    /// returns pixels whose length is not `width * height * 4` (or a zero side), is
    /// [`Error::Damaged`] too: a decoder is not allowed to take the application down.
    pub fn load(
        bytes: &[u8],
        limits: Limits,
        extra: Option<&dyn Decoder>,
    ) -> Result<Picture, Error> {
        match mdshared_text::image::decode(bytes, limits) {
            Ok(image) => Ok(Picture::new(image)),
            Err(error) => {
                if !matches!(error, Error::Unsupported(_)) {
                    return Err(error);
                }
                let Some(extra) = extra else {
                    return Err(error);
                };
                // A slot is application code; the built-in decoders' `guarded` is the model.
                let decoded = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    extra.decode(bytes, limits)
                }));
                let image = match decoded {
                    Ok(Some(image)) => image,
                    Ok(None) => return Err(error),
                    Err(_) => return Err(Error::Damaged),
                };
                let len = (image.width as usize)
                    .checked_mul(image.height as usize)
                    .and_then(|n| n.checked_mul(4));
                if image.width == 0 || image.height == 0 || len != Some(image.rgba.len()) {
                    return Err(Error::Damaged);
                }
                if !limits.allows(image.width, image.height) {
                    return Err(Error::TooLarge {
                        width: image.width,
                        height: image.height,
                    });
                }
                Ok(Picture::new(image))
            }
        }
    }

    /// The source picture's size in pixels: the size of the file, not of the rectangle it is
    /// drawn into.
    pub fn size(&self) -> (u32, u32) {
        (self.image.width, self.image.height)
    }

    /// Draws the picture into `dst`, its corners rounded by `radius` and the whole blended at
    /// `alpha` on top of its own transparency, as [`Painter::image`] does. The picture fills
    /// the rectangle exactly; the caller keeps the aspect ratio by the rectangle it gives.
    ///
    /// The first draw at a device size prepares the pixels; a later draw at the same size,
    /// scale and radius blits them, and the clip and offset still apply. The prepared size is
    /// `round(dst.w * scale)` by `round(dst.h * scale)`, which is a property of the rectangle
    /// and not of where it sits, so moving a picture by whole or fractional logical px does
    /// not prepare it again. `dev_rect` rounds the two edges separately and can place it a
    /// pixel narrower or wider than that; the blit is clipped to the placed rectangle, so a
    /// placement a pixel narrower loses the cached picture's last column or row, and one a
    /// pixel wider leaves that column or row of the rectangle unpainted. Nothing is painted
    /// past `dst`.
    ///
    /// An empty or non-finite rectangle, or a zero or NaN alpha, draws nothing; a target over
    /// 1024 device px a side or a million pixels, and one under a device pixel, is handed to
    /// [`Painter::image`], so no nonsense size asks for a large prepared buffer. An infinite
    /// or NaN radius rounds as far as half the smaller side, as it does in [`Painter::image`].
    ///
    /// This changes the picture (it keeps the prepared pixels), not the painter.
    pub fn draw(&mut self, painter: &mut Painter<'_>, dst: Rect, radius: f32, alpha: f32) {
        if !dst.x.is_finite()
            || !dst.y.is_finite()
            || !dst.w.is_finite()
            || !dst.h.is_finite()
            || alpha.is_nan()
            || !painter.scale.is_finite()
            || painter.scale <= 0.0
        {
            return;
        }
        let alpha = alpha.clamp(0.0, 1.0);
        if alpha <= 0.0 {
            return;
        }
        let scale = painter.scale;
        let wide = dst.w * scale;
        let tall = dst.h * scale;
        if !wide.is_finite() || !tall.is_finite() || wide < 1.0 || tall < 1.0 {
            self.draw_unprepared(painter, dst, radius, alpha);
            return;
        }
        let w = wide.round() as i64;
        let h = tall.round() as i64;
        if w > i64::from(MAX_PREPARED_SIDE)
            || h > i64::from(MAX_PREPARED_SIDE)
            || w.saturating_mul(h) > MAX_PREPARED_PIXELS as i64
        {
            // Too large to keep: the painter's own scaler, which allocates only the clip.
            self.draw_unprepared(painter, dst, radius, alpha);
            return;
        }
        let (x0, y0, x1, y1) = painter.dev_rect(dst);
        let placed_w = x1 as i64 - x0 as i64;
        let placed_h = y1 as i64 - y0 as i64;
        if placed_w <= 0 || placed_h <= 0 {
            return;
        }
        if (placed_w - w).abs() > 1 || (placed_h - h).abs() > 1 {
            // Only an extreme rectangle and scale get here; let the painter match it.
            self.draw_unprepared(painter, dst, radius, alpha);
            return;
        }
        let (w, h) = (w as u32, h as u32);
        let rad = (radius * scale)
            .min(w as f32 / 2.0)
            .min(h as f32 / 2.0)
            .max(0.0);
        let key = (w, h, scale.to_bits(), rad.to_bits());
        if self.prepared.as_ref().is_none_or(|p| p.key != key) {
            let Some(rgba) = crate::gfx::prepare_image(
                &self.image.rgba,
                self.image.width,
                self.image.height,
                w,
                h,
                rad,
            ) else {
                return;
            };
            self.prepared = Some(Prepared { key, rgba });
            self.prepares += 1;
        }
        if let Some(prepared) = &self.prepared {
            painter.blit_rgba(&prepared.rgba, w, h, x0, y0, x1, y1, alpha);
        }
    }

    /// The painter's own scaler, for a target too small or too large to keep.
    fn draw_unprepared(&self, painter: &mut Painter<'_>, dst: Rect, radius: f32, alpha: f32) {
        painter.image(
            &self.image.rgba,
            self.image.width,
            self.image.height,
            dst,
            radius,
            alpha,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gfx::Gfx;
    use mdshared_text::Bitmap;
    use mdshared_text::image::{Error, Format, Limits};

    const WHITE: [u8; 3] = [255, 255, 255];
    const MARKER: [u8; 3] = [255, 0, 255];

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

    /// Straight RGBA pixels from a function of the place.
    fn pixels(w: u32, h: u32, f: impl Fn(u32, u32) -> [u8; 4]) -> Image {
        let rgba = (0..h)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .flat_map(|(x, y)| f(x, y))
            .collect();
        Image::new(w, h, rgba).unwrap()
    }

    /// A source with a colour ramp, a bright band and a fully transparent part: opaque,
    /// half-transparent and cut-out pixels, so rounding and blending both show.
    fn sample() -> Image {
        pixels(30, 20, |x, y| {
            if x >= 20 {
                [0, 0, 0, 0]
            } else if x >= 15 {
                [240, 80, 20, 128]
            } else {
                [(x * 8) as u8, (y * 12) as u8, 200, 255]
            }
        })
    }

    fn rgba_at(bmp: &Bitmap, x: i32, y: i32) -> [u8; 4] {
        let o = (y as usize * bmp.width as usize + x as usize) * 4;
        [bmp.pixels[o + 2], bmp.pixels[o + 1], bmp.pixels[o], 255]
    }

    /// The pixels that are not white: how many, and the box they lie in (x0, y0, x1, y1, the
    /// far edges outside).
    fn ink(bmp: &Bitmap) -> (usize, (i32, i32, i32, i32)) {
        let (mut n, mut b) = (0, (i32::MAX, i32::MAX, i32::MIN, i32::MIN));
        for y in 0..bmp.height as i32 {
            for x in 0..bmp.width as i32 {
                if rgba_at(bmp, x, y) != [255, 255, 255, 255] {
                    n += 1;
                    b = (b.0.min(x), b.1.min(y), b.2.max(x + 1), b.3.max(y + 1));
                }
            }
        }
        (n, b)
    }

    /// The worst channel difference between two pages of the same size.
    fn worst(a: &Bitmap, b: &Bitmap) -> i32 {
        assert_eq!((a.width, a.height), (b.width, b.height));
        let mut worst = 0;
        for y in 0..a.height as i32 {
            for x in 0..a.width as i32 {
                for (p, q) in rgba_at(a, x, y).iter().zip(rgba_at(b, x, y)) {
                    worst = worst.max((i32::from(*p) - i32::from(q)).abs());
                }
            }
        }
        worst
    }

    #[test]
    fn a_prepared_picture_draws_what_the_painter_draws_at_the_same_size() {
        let source = sample();
        let dst = Rect::new(10.0, 8.0, 24.0, 16.0);
        for (scale, radius, alpha) in [(1.0, 5.0, 1.0), (1.5, 6.0, 0.75), (2.0, 0.0, 1.0)] {
            let direct = page(80, 60, scale, |p| {
                p.image(
                    &source.rgba,
                    source.width,
                    source.height,
                    dst,
                    radius,
                    alpha,
                );
            });
            let mut pic = Picture::new(source.clone());
            let prepared = page(80, 60, scale, |p| pic.draw(p, dst, radius, alpha));
            let d = worst(&direct, &prepared);
            assert!(d <= 2, "scale {scale}: the drawings differ by {d}");
            assert_eq!(pic.prepares, 1);
        }
    }

    #[test]
    fn a_second_draw_at_the_same_size_does_not_prepare_again() {
        let mut pic = Picture::new(sample());
        let dst = Rect::new(4.0, 4.0, 20.0, 12.0);
        let _ = page(40, 30, 1.0, |p| pic.draw(p, dst, 3.0, 1.0));
        assert_eq!(pic.prepares, 1);
        let _ = page(40, 30, 1.0, |p| pic.draw(p, dst, 3.0, 1.0));
        let _ = page(40, 30, 1.0, |p| {
            pic.draw(p, Rect::new(2.0, 1.0, 20.0, 12.0), 3.0, 1.0)
        });
        assert_eq!(pic.prepares, 1, "a move reprepared");
    }

    #[test]
    fn a_size_or_scale_change_prepares_again() {
        let mut pic = Picture::new(sample());
        let mut draw = |scale: f32, r: Rect| {
            let _ = page(60, 40, scale, |p| pic.draw(p, r, 0.0, 1.0));
            pic.prepares
        };
        // Device size 20 x 10 at scale 1.
        assert_eq!(draw(1.0, Rect::new(2.0, 2.0, 20.0, 10.0)), 1);
        assert_eq!(draw(1.0, Rect::new(2.0, 2.0, 20.0, 10.0)), 1);
        // A wider target: device 30 x 10.
        assert_eq!(draw(1.0, Rect::new(2.0, 2.0, 30.0, 10.0)), 2);
        // The same device size as the first, but at scale 2 (10 x 5 logical): the scale is
        // part of what was prepared, so it prepares again.
        assert_eq!(draw(2.0, Rect::new(1.0, 1.0, 10.0, 5.0)), 3);
        // And back: the prepared size from before is not kept, so it prepares once more.
        assert_eq!(draw(1.0, Rect::new(2.0, 2.0, 20.0, 10.0)), 4);
        // A radius change at the same size and scale is a new set of corners.
        let _ = page(60, 40, 1.0, |p| {
            pic.draw(p, Rect::new(2.0, 2.0, 20.0, 10.0), 4.0, 1.0)
        });
        assert_eq!(pic.prepares, 5);
    }

    #[test]
    fn a_move_at_a_fractional_scale_does_not_prepare_again() {
        for scale in [1.25, 1.5] {
            let mut pic = Picture::new(sample());
            for x in [1.0, 2.0, 1.5, 2.7, 3.0] {
                let _ = page(64, 48, scale, |p| {
                    pic.draw(p, Rect::new(x, 2.0, 15.0, 15.0), 4.0, 1.0)
                });
                assert_eq!(pic.prepares, 1, "scale {scale}, moved to {x}");
            }
        }
    }

    #[test]
    fn an_alpha_change_does_not_prepare_again() {
        let mut pic = Picture::new(sample());
        let dst = Rect::new(4.0, 3.0, 20.0, 14.0);
        let _ = page(40, 30, 1.5, |p| pic.draw(p, dst, 3.0, 1.0));
        let _ = page(40, 30, 1.5, |p| pic.draw(p, dst, 3.0, 0.35));
        assert_eq!(pic.prepares, 1, "an alpha change reprepared");
    }

    #[test]
    fn an_infinite_or_nan_radius_rounds_as_the_painter_does() {
        let source = sample();
        let dst = Rect::new(6.0, 6.0, 24.0, 24.0);
        for radius in [f32::INFINITY, f32::NAN] {
            let direct = page(40, 40, 1.0, |p| {
                p.image(&source.rgba, source.width, source.height, dst, radius, 1.0);
            });
            let mut pic = Picture::new(source.clone());
            let prepared = page(40, 40, 1.0, |p| pic.draw(p, dst, radius, 1.0));
            let d = worst(&direct, &prepared);
            assert!(d <= 2, "radius {radius}: the drawings differ by {d}");
            assert_eq!(pic.prepares, 1);
            // Half the smaller side is 12 device px: the very corner is not covered.
            assert_eq!(rgba_at(&prepared, 6, 6), [255, 255, 255, 255]);
        }
    }

    #[test]
    fn the_clip_and_the_offset_are_honoured() {
        let source = sample();
        let dst = Rect::new(10.0, 10.0, 40.0, 40.0);
        let mut pic = Picture::new(source.clone());
        let whole = page(80, 80, 1.0, |p| pic.draw(p, dst, 8.0, 1.0));
        // The clip: only its part, exactly the pixels the whole draw has there.
        let part = page(80, 80, 1.0, |p| {
            let old = p.clip(Rect::new(0.0, 0.0, 30.0, 25.0));
            pic.draw(p, dst, 8.0, 1.0);
            p.restore(old);
        });
        assert_eq!(ink(&part).1, (10, 10, 30, 25));
        assert_eq!(pic.prepares, 1, "the clipped draw prepared again");
        for y in 0..25 {
            for x in 0..30 {
                assert_eq!(rgba_at(&part, x, y), rgba_at(&whole, x, y), "{x},{y}");
            }
        }
        for y in 25..80 {
            for x in 30..80 {
                assert_eq!(rgba_at(&part, x, y), [255, 255, 255, 255], "{x},{y}");
            }
        }
        let prepares = pic.prepares;
        // The offset moves it by whole device pixels, and does not reprepare.
        let moved = page(80, 80, 1.0, |p| {
            let old = p.offset(7.0, -3.0);
            pic.draw(p, dst, 8.0, 1.0);
            p.restore_offset(old);
        });
        assert_eq!(pic.prepares, prepares, "the offset reprepared");
        for y in 3..80 {
            for x in 7..80 {
                assert_eq!(rgba_at(&moved, x, y - 3), rgba_at(&whole, x - 7, y));
            }
        }
    }

    #[test]
    fn a_picture_a_pixel_narrower_does_not_paint_past_its_rectangle() {
        // At scale 1.5, Rect(1, 6, 15, 15) is placed at device x 2..24, 22 px wide, while
        // round(15 * 1.5) is 23: the cached picture is one pixel wider than the placement.
        let mut pic = Picture::new(pixels(30, 20, |_, _| [10, 20, 30, 255]));
        let dst = Rect::new(1.0, 6.0, 15.0, 15.0);
        let bmp = page(60, 40, 1.5, |p| {
            // A marker in the device pixel just past the placement's right edge.
            p.fill_dev(24, 15, 25, 16, MARKER, 1.0);
            pic.draw(p, dst, 0.0, 1.0);
        });
        assert_eq!(pic.prepared.as_ref().unwrap().key.0, 23, "cached width");
        assert_eq!(
            rgba_at(&bmp, 24, 15),
            [MARKER[0], MARKER[1], MARKER[2], 255],
            "the pixel past the rectangle was painted"
        );
        // The last cached column that fits is drawn, and the picture starts at the left edge.
        assert_eq!(rgba_at(&bmp, 23, 15), [10, 20, 30, 255]);
        assert_eq!(rgba_at(&bmp, 2, 15), [10, 20, 30, 255]);
    }

    #[test]
    fn zero_nan_and_huge_sizes_neither_panic_nor_prepare() {
        let mut pic = Picture::new(sample());
        let blank = page(24, 24, 1.0, |_| ());
        // Nothing but nonsense: no pixels change and nothing is prepared.
        let none = page(24, 24, 1.0, |p| {
            pic.draw(p, Rect::new(0.0, 0.0, 0.0, 10.0), 0.0, 1.0);
            pic.draw(p, Rect::new(0.0, 0.0, 10.0, 0.0), 0.0, 1.0);
            pic.draw(p, Rect::new(0.0, 0.0, -10.0, 10.0), 0.0, 1.0);
            pic.draw(p, Rect::new(f32::NAN, 0.0, 10.0, 10.0), 0.0, 1.0);
            pic.draw(p, Rect::new(0.0, 0.0, f32::INFINITY, 10.0), 0.0, 1.0);
            pic.draw(p, Rect::new(0.0, 0.0, 10.0, 10.0), f32::NAN, 0.0);
            pic.draw(p, Rect::new(0.0, 0.0, 10.0, 10.0), 0.0, 0.0);
            pic.draw(p, Rect::new(0.0, 0.0, 10.0, 10.0), 0.0, f32::NAN);
        });
        assert_eq!(pic.prepares, 0);
        assert!(
            none.pixels == blank.pixels,
            "a nonsense size drew something"
        );
        // A huge target is not prepared: it is handed to the painter, which allocates only
        // the visible part; it draws the picture's top-left corner and nothing panics.
        let huge = page(24, 24, 1.0, |p| {
            pic.draw(p, Rect::new(0.0, 0.0, 1.0e9, 1.0e9), 0.0, 1.0);
        });
        assert_eq!(pic.prepares, 0, "a huge target was prepared");
        assert!(ink(&huge).0 > 0, "the huge target drew nothing");
        // A finite rectangle with a huge radius, an infinite one and a NaN one: all round as
        // far as half the smaller side, as `Painter::image` does, so all three are one size.
        let some = page(24, 24, 1.0, |p| {
            pic.draw(p, Rect::new(2.0, 2.0, 10.0, 10.0), 1.0e6, 1.0);
            pic.draw(p, Rect::new(2.0, 2.0, 10.0, 10.0), f32::INFINITY, 1.0);
            pic.draw(p, Rect::new(2.0, 2.0, 10.0, 10.0), f32::NAN, 1.0);
        });
        assert_eq!(
            pic.prepares, 1,
            "a huge, an infinite and a NaN radius all round to half the side"
        );
        for y in 0..24 {
            for x in 0..24 {
                let inside = (2..12).contains(&x) && (2..12).contains(&y);
                if !inside {
                    assert_eq!(rgba_at(&some, x, y), [255, 255, 255, 255], "{x},{y}");
                }
            }
        }
    }

    #[test]
    fn the_decoder_slot_reads_a_format_the_toolkit_refuses() {
        let gif = b"GIF89a a tiny picture";
        let slot = |bytes: &[u8], _: Limits| {
            bytes
                .starts_with(b"GIF89a")
                .then(|| pixels(2, 1, |x, _| [x as u8 * 200, 10, 20, 255]))
        };
        let picture = Picture::load(gif, Limits::DEFAULT, Some(&slot)).unwrap();
        assert_eq!(picture.size(), (2, 1));
        // Without a slot the built-in answer stands.
        assert!(matches!(
            Picture::load(gif, Limits::DEFAULT, None),
            Err(Error::Unsupported(Format::Gif))
        ));
        // A slot is asked only when the built-in decoders call the bytes unsupported.
        assert!(matches!(
            Picture::load(b"not a picture", Limits::DEFAULT, Some(&slot)),
            Err(Error::Unsupported(Format::Unknown))
        ));
        // A damaged PNG stays damaged; a slot that would take anything is not asked.
        let any = |_: &[u8], _: Limits| Some(pixels(1, 1, |_, _| [0, 0, 0, 255]));
        assert!(matches!(
            Picture::load(b"\x89PNG\r\n\x1a\n", Limits::DEFAULT, Some(&any)),
            Err(Error::Damaged)
        ));
        // What the slot returns is held to the limits.
        let wide = |_: &[u8], _: Limits| Some(pixels(2, 2, |_, _| [0, 0, 0, 255]));
        assert!(matches!(
            Picture::load(gif, Limits::new(1, 100), Some(&wide)),
            Err(Error::TooLarge {
                width: 2,
                height: 2
            })
        ));
    }

    #[test]
    fn a_slot_that_panics_or_returns_a_broken_image_is_damaged() {
        let gif = b"GIF89a a tiny picture";
        let boom = |_: &[u8], _: Limits| -> Option<Image> { panic!("the decoder gave up") };
        assert!(matches!(
            Picture::load(gif, Limits::DEFAULT, Some(&boom)),
            Err(Error::Damaged)
        ));
        // A picture whose pixels are not `width * height * 4` is not drawn.
        let short = |_: &[u8], _: Limits| {
            let mut image = pixels(2, 2, |_, _| [0, 0, 0, 255]);
            image.rgba.pop();
            Some(image)
        };
        assert!(matches!(
            Picture::load(gif, Limits::DEFAULT, Some(&short)),
            Err(Error::Damaged)
        ));
        let zero = |_: &[u8], _: Limits| {
            let mut image = pixels(1, 1, |_, _| [0, 0, 0, 255]);
            image.width = 0;
            image.rgba.clear();
            Some(image)
        };
        assert!(matches!(
            Picture::load(gif, Limits::DEFAULT, Some(&zero)),
            Err(Error::Damaged)
        ));
    }
}
