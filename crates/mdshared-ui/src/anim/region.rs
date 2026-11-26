//! A saved piece of the bitmap, and the blends of it with what was drawn there since.
//!
//! The bitmap is opaque and everything is drawn straight into it, so there is no layer to fade
//! and no old page to slide away once the new one is drawn. A [`Snapshot`] is the answer to
//! both: copy the pixels of a rectangle before drawing over them, draw, then mix the copy back
//! in. Mixed back at 30 %, what was drawn shows at 70 % opacity as a whole (a group fade: a
//! popup's shadow, frame and text fade as one thing, with no part showing through another).
//! Kept from the frame before a page switch, the copy is the old page, to cross-fade or slide
//! against the new one.
//!
//! Everything here is in device pixels, touches only the rectangle and does integer arithmetic:
//! a cross-fade is three multiplies a byte, a push is a copy.

use crate::frost::{Key, MARK, avoid_mark};
use mdshared_text::Bitmap;

/// The pixels of a rectangle of a bitmap, as they were when it was taken.
///
/// It remembers the size of the bitmap it was taken from, and blends only into a bitmap of
/// that size ([`fits`](Snapshot::fits)). After a resize the same device rectangle is another
/// part of the window (what was the content is now half content, half sidebar), so pixels put
/// back there would be the old page in the wrong place.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    /// The size of the bitmap it was taken from.
    of: (u32, u32),
    pixels: Vec<u8>,
}

/// `old` and `new` mixed, `a` of 256 parts `new`: exactly `old` at 0 and exactly `new` at 256.
#[inline]
fn mix(old: u8, new: u8, a: u32) -> u8 {
    ((old as u32 * (256 - a) + new as u32 * a + 128) >> 8) as u8
}

/// Preserve intentional chrome marks on the legacy plain path, but never create a new mark
/// from two premultiplied inputs.
fn mix_alpha(old: u8, new: u8, a: u32) -> u8 {
    let alpha = mix(old, new, a);
    if old == MARK || new == MARK {
        alpha
    } else {
        avoid_mark(alpha)
    }
}

/// Mixes only unlike kinds of pixel after decoding chrome. Two chrome pixels stay marked
/// (and keep the legacy colour mix), and the endpoints are always copied exactly.
fn mix_pixel(mut old: [u8; 4], mut new: [u8; 4], a: u32, key: Option<&Key>) -> [u8; 4] {
    if a == 0 {
        return old;
    }
    if a == 256 {
        return new;
    }
    let (was, now) = (old[3] == MARK, new[3] == MARK);
    let decoded = key.filter(|_| was != now);
    let alpha = if let Some(key) = decoded {
        let marked = if was { &mut old } else { &mut new };
        *marked = key.apply(u32::from_le_bytes(*marked)).to_le_bytes();
        // A decoded chrome pixel can have alpha 254; the intermediate result must not.
        avoid_mark(mix(old[3], new[3], a))
    } else {
        mix_alpha(old[3], new[3], a)
    };
    let channel =
        |i: usize| mix(old[i], new[i], a).min(if decoded.is_some() { alpha } else { 255 });
    [channel(0), channel(1), channel(2), alpha]
}

/// A fraction as parts of 256.
fn parts(t: f32) -> u32 {
    if t.is_nan() {
        return 256;
    }
    (t.clamp(0.0, 1.0) * 256.0).round() as u32
}

impl Snapshot {
    /// The pixels of the device rectangle `x0, y0` to `x1, y1` (the far edges are outside) of
    /// `bmp`, cut to the bitmap. A rectangle that misses the bitmap gives an empty snapshot,
    /// which every blend ignores.
    pub fn take(bmp: &Bitmap, x0: i32, y0: i32, x1: i32, y1: i32) -> Snapshot {
        let mut s = Snapshot::default();
        s.retake(bmp, x0, y0, x1, y1);
        s
    }

    /// As [`take`](Self::take), into this snapshot: its memory is used again.
    pub fn retake(&mut self, bmp: &Bitmap, x0: i32, y0: i32, x1: i32, y1: i32) {
        let (x0, y0) = (x0.max(0), y0.max(0));
        let (x1, y1) = (x1.min(bmp.width as i32), y1.min(bmp.height as i32));
        self.pixels.clear();
        self.of = (bmp.width, bmp.height);
        if x0 >= x1 || y0 >= y1 {
            (self.x, self.y, self.w, self.h) = (0, 0, 0, 0);
            return;
        }
        (self.x, self.y, self.w, self.h) = (x0, y0, x1 - x0, y1 - y0);
        let n = self.w as usize * 4;
        self.pixels.reserve(n * self.h as usize);
        for row in y0..y1 {
            let o = (row as usize * bmp.width as usize + x0 as usize) * 4;
            self.pixels.extend_from_slice(&bmp.pixels[o..o + n]);
        }
    }

    /// Nothing was saved.
    pub fn is_empty(&self) -> bool {
        self.w == 0 || self.h == 0
    }

    /// The device rectangle it was taken from: x0, y0, x1, y1.
    pub fn rect(&self) -> (i32, i32, i32, i32) {
        (self.x, self.y, self.x + self.w, self.y + self.h)
    }

    /// The width and height of the bitmap it was taken from.
    pub fn bitmap_size(&self) -> (u32, u32) {
        self.of
    }

    /// Whether the snapshot can be blended into `bmp`: `bmp` has the size of the bitmap it was
    /// taken from. False after the window was resized under it, larger or smaller, and for an
    /// empty one; the blends then do nothing.
    pub fn fits(&self, bmp: &Bitmap) -> bool {
        !self.is_empty()
            && self.of == (bmp.width, bmp.height)
            && self.x + self.w <= bmp.width as i32
            && self.y + self.h <= bmp.height as i32
            && bmp.pixels.len() >= bmp.width as usize * bmp.height as usize * 4
    }

    fn row(&self, row: i32) -> &[u8] {
        let n = self.w as usize * 4;
        &self.pixels[row as usize * n..(row as usize + 1) * n]
    }

    /// Where row `row` of the rectangle starts in `bmp`.
    fn at(&self, bmp: &Bitmap, row: i32) -> usize {
        ((self.y + row) as usize * bmp.width as usize + self.x as usize) * 4
    }

    /// Puts the saved pixels back.
    pub fn restore(&self, bmp: &mut Bitmap) {
        self.crossfade(bmp, 0.0);
    }

    /// Mixes the saved pixels with what `bmp` holds there now, in place: at `t` 0 the rectangle
    /// is the snapshot again, at 1 it is left as it is, between them it is `t` of the way from
    /// the snapshot to the present. So `t` is the opacity of everything drawn since the
    /// snapshot, and the progress of a cross-fade from an old page to a new one.
    pub fn crossfade(&self, bmp: &mut Bitmap, t: f32) {
        let a = parts(t);
        if a == 256 || !self.fits(bmp) {
            return;
        }
        let n = self.w as usize * 4;
        for row in 0..self.h {
            let o = self.at(bmp, row);
            let dst = &mut bmp.pixels[o..o + n];
            let old = self.row(row);
            if a == 0 {
                dst.copy_from_slice(old);
            } else {
                for (d, s) in dst
                    .as_chunks_mut::<4>()
                    .0
                    .iter_mut()
                    .zip(old.as_chunks::<4>().0)
                {
                    for i in 0..3 {
                        d[i] = mix(s[i], d[i], a);
                    }
                    d[3] = mix_alpha(s[3], d[3], a);
                }
            }
        }
    }

    /// [`crossfade`](Self::crossfade) for a bitmap whose chrome is frosted with `key`: where
    /// either side is a chrome pixel ([`crate::frost::MARK`]) and the other is not, the two are
    /// mixed as the compositor will show them, the chrome taken off the key
    /// ([`crate::frost::Key::apply`]) and both premultiplied, and the result is written with its
    /// own alpha, which [`crate::frost::key_out`] leaves alone. So a popup fading in over frosted
    /// chrome lets the backdrop through less and less, rather than counting as chrome (keyed
    /// out) until halfway and then as an opaque pixel. Where both sides are chrome the colours
    /// mix and the mark stays; where neither is, this is `crossfade`. With no key it is
    /// `crossfade`. A pixel written with its own alpha is premultiplied: draw nothing over it
    /// before the frame is presented (the next frame draws the region again from the back).
    pub fn crossfade_keyed(&self, bmp: &mut Bitmap, t: f32, key: Option<&Key>) {
        let Some(key) = key else {
            return self.crossfade(bmp, t);
        };
        let a = parts(t);
        if a == 256 || a == 0 || !self.fits(bmp) {
            return self.crossfade(bmp, t);
        }
        let n = self.w as usize * 4;
        for row in 0..self.h {
            let o = self.at(bmp, row);
            let dst = &mut bmp.pixels[o..o + n];
            let old = self.row(row);
            for (d, s) in dst
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip(old.as_chunks::<4>().0)
            {
                *d = mix_pixel(*s, *d, a, Some(key));
            }
        }
    }

    /// The present content of the rectangle moved by `dx, dy` device px and mixed over the
    /// snapshot at opacity `t`, in place: a new page that slides in while it fades in. Where
    /// the moved content has not reached (a band `dx` wide or `dy` high at the edge it comes
    /// from), its own edge row or column is repeated, so a page whose edge is its background,
    /// as a padded page's is, shows no seam. With no offset this is
    /// [`crossfade`](Self::crossfade).
    pub fn slide_fade(&self, bmp: &mut Bitmap, dx: i32, dy: i32, t: f32) {
        self.slide_fade_keyed(bmp, dx, dy, t, None);
    }

    pub(super) fn slide_fade_keyed(
        &self,
        bmp: &mut Bitmap,
        dx: i32,
        dy: i32,
        t: f32,
        key: Option<&Key>,
    ) {
        if dx == 0 && dy == 0 {
            return self.crossfade_keyed(bmp, t, key);
        }
        self.moved(bmp, dx, dy, parts(t), false, key);
    }

    /// The present content of the rectangle moved by `dx, dy` device px, with the snapshot moved
    /// along ahead of it, in place: the old page is pushed out as the new one comes in, and
    /// nothing is mixed. Meant for one axis at a time; an offset of the rectangle's whole width
    /// (or height) shows the snapshot alone, none shows the present alone.
    pub fn push(&self, bmp: &mut Bitmap, dx: i32, dy: i32) {
        if dx == 0 && dy == 0 {
            return;
        }
        self.moved(bmp, dx, dy, 256, true, None);
    }

    /// The shared loop of [`slide_fade`](Self::slide_fade) and [`push`](Self::push). The rows
    /// are walked away from the direction of the move, so the row a row reads from has not been
    /// written yet, and each source row is copied out before its own row is written.
    fn moved(&self, bmp: &mut Bitmap, dx: i32, dy: i32, a: u32, push: bool, key: Option<&Key>) {
        if !self.fits(bmp) {
            return;
        }
        let (w, h) = (self.w, self.h);
        let n = w as usize * 4;
        let mut source = vec![0u8; n];
        let rows: Box<dyn Iterator<Item = i32>> = if dy > 0 {
            Box::new((0..h).rev())
        } else {
            Box::new(0..h)
        };
        for row in rows {
            let from = row - dy;
            // The new content's row for this row, or, in a push, the old row that is ahead.
            let inside = (0..h).contains(&from);
            if inside || !push {
                let o = self.at(bmp, from.clamp(0, h - 1));
                source.copy_from_slice(&bmp.pixels[o..o + n]);
            } else {
                source.copy_from_slice(self.row(from.rem_euclid(h)));
            }
            let old = self.row(row);
            let ahead = self.row(if inside { row } else { from.rem_euclid(h) });
            let o = self.at(bmp, row);
            let dst = &mut bmp.pixels[o..o + n];
            for (col, (dst, old)) in dst
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip(old.as_chunks::<4>().0)
                .enumerate()
            {
                let from = col as i32 - dx;
                let px: [u8; 4] = if (0..w).contains(&from) || !push {
                    let s = from.clamp(0, w - 1) as usize * 4;
                    [source[s], source[s + 1], source[s + 2], source[s + 3]]
                } else {
                    let s = from.rem_euclid(w) as usize * 4;
                    [ahead[s], ahead[s + 1], ahead[s + 2], ahead[s + 3]]
                };
                *dst = mix_pixel(*old, px, a, key);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fading_premultiplied_pixels_never_turns_them_into_chrome() {
        let key = crate::frost::Key::new([220; 3], 0);
        let mut bmp = Bitmap::new(2, 1);
        bmp.pixels
            .copy_from_slice(&[21, 21, 21, 179, 21, 21, 21, 179]);
        let under = Snapshot::take(&bmp, 0, 0, 2, 1);
        for method in ["keyed", "plain", "slide"] {
            for step in 0..=256 {
                bmp.clear([220; 3]);
                let t = step as f32 / 256.0;
                match method {
                    "keyed" => under.crossfade_keyed(&mut bmp, t, Some(&key)),
                    "plain" => under.crossfade(&mut bmp, t),
                    _ => under.slide_fade(&mut bmp, 1, 0, t),
                }
                let alpha = (179.0 + 76.0 * t).round() as u8;
                assert_eq!(
                    bmp.pixels[3],
                    if alpha == 254 { 255 } else { alpha },
                    "{method}, step {step}"
                );
                let mut pixel = [u32::from_le_bytes(bmp.pixels[..4].try_into().unwrap())];
                let original = pixel[0];
                crate::frost::key_out(&key, &mut pixel, 1, (0, 0, 1, 1));
                assert_eq!(pixel[0], original, "{method}, step {step}");
            }
        }
    }

    /// A popup fading in over frosted chrome shows as the compositor will draw it: its alpha
    /// rises with `t`, where a plain crossfade turned it opaque at halfway. Where neither side is chrome the keyed fade is the plain one, byte for byte.
    #[test]
    fn a_fade_over_frosted_chrome_rises_through_the_backdrop() {
        use crate::frost::{Key, MARK, key_out, mark};
        let chrome = [0xf3, 0xf3, 0xf3];
        let key = Key::new(chrome, 0);
        // Alpha and red after the present: the bitmap's 32-bit pixels taken off the key.
        let present = |bmp: &Bitmap, x: u32| {
            let mut px: Vec<u32> = bmp
                .pixels
                .as_chunks::<4>()
                .0
                .iter()
                .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                .collect();
            key_out(&key, &mut px, bmp.width as usize, (0, 0, 20, 10));
            let v = px[(5 * bmp.width + x) as usize];
            (v >> 24, (v >> 16) & 0xff)
        };
        let fade = |t: f32, keyed: bool| {
            let mut bmp = painted(20, 10, |_, _| chrome);
            mark(&mut bmp, (0, 0, 20, 10));
            let under = Snapshot::take(&bmp, 0, 0, 20, 10);
            // The popup: opaque white over the left half.
            for y in 0..10 {
                for x in 0..10 {
                    let o = ((y * 20 + x) * 4) as usize;
                    bmp.pixels[o..o + 4].copy_from_slice(&[255, 255, 255, 255]);
                }
            }
            under.crossfade_keyed(&mut bmp, t, keyed.then_some(&key));
            bmp
        };
        let mut last = 0;
        for t in [0.2, 0.4, 0.6, 0.8] {
            let (a, r) = present(&fade(t, true), 2);
            let want = (t * 255.0).round() as u32;
            assert!(a.abs_diff(want) <= 2, "t {t}: alpha {a}, want {want}");
            assert!(
                r <= a && r.abs_diff(want) <= 2,
                "t {t}: premultiplied white {r}"
            );
            assert!(a > last, "alpha rises with t");
            last = a;
            // The chrome beside the popup is still chrome, keyed out.
            assert_eq!(present(&fade(t, true), 15).0, 0);
        }
        // The plain crossfade: the mix stays marked until halfway and is keyed as chrome, then
        // it is opaque, a jump from about 40 % to all of it.
        assert!(present(&fade(0.4, false), 2).0 < 160);
        assert_eq!(present(&fade(0.6, false), 2).0, 255);
        assert_ne!(MARK, 255);

        // Nothing marked: the keyed fade is the crossfade.
        let draw = |keyed: bool| {
            let mut bmp = painted(20, 10, |x, y| [x as u8 * 9, y as u8 * 20, 77]);
            let under = Snapshot::take(&bmp, 2, 2, 15, 6);
            for p in bmp.pixels.as_chunks_mut::<4>().0 {
                p[..3].copy_from_slice(&[10, 200, 90]);
            }
            under.crossfade_keyed(&mut bmp, 0.37, keyed.then_some(&key));
            bmp.pixels
        };
        assert_eq!(draw(true), draw(false));
    }

    /// A bitmap whose every pixel is a colour made from its place by `f`.
    fn painted(w: u32, h: u32, f: impl Fn(u32, u32) -> [u8; 3]) -> Bitmap {
        let mut bmp = Bitmap::new(w, h);
        for y in 0..h {
            for x in 0..w {
                let o = ((y * w + x) * 4) as usize;
                let c = f(x, y);
                bmp.pixels[o..o + 4].copy_from_slice(&[c[2], c[1], c[0], 255]);
            }
        }
        bmp
    }

    fn at(bmp: &Bitmap, x: u32, y: u32) -> [u8; 3] {
        let o = ((y * bmp.width + x) * 4) as usize;
        assert_eq!(bmp.pixels[o + 3], 255, "alpha at {x},{y}");
        [bmp.pixels[o + 2], bmp.pixels[o + 1], bmp.pixels[o]]
    }

    fn old_page(x: u32, y: u32) -> [u8; 3] {
        [x as u8 * 10, y as u8 * 10, 200]
    }

    fn new_page(x: u32, y: u32) -> [u8; 3] {
        [255 - x as u8 * 10, 40, y as u8 * 20]
    }

    /// The old page with a snapshot of the region 2,1 to 10,7 taken, then the new page drawn.
    fn switched() -> (Snapshot, Bitmap) {
        let old = painted(12, 8, old_page);
        let snap = Snapshot::take(&old, 2, 1, 10, 7);
        (snap, painted(12, 8, new_page))
    }

    fn inside(x: u32, y: u32) -> bool {
        (2..10).contains(&x) && (1..7).contains(&y)
    }

    #[test]
    fn a_snapshot_is_cut_to_the_bitmap_and_put_back_exactly() {
        let old = painted(12, 8, old_page);
        let snap = Snapshot::take(&old, -5, 3, 40, 99);
        assert_eq!(snap.rect(), (0, 3, 12, 8));
        assert!(!snap.is_empty() && snap.fits(&old));
        let mut bmp = painted(12, 8, new_page);
        snap.restore(&mut bmp);
        for y in 0..8 {
            for x in 0..12 {
                let want = if y >= 3 {
                    old_page(x, y)
                } else {
                    new_page(x, y)
                };
                assert_eq!(at(&bmp, x, y), want, "{x},{y}");
            }
        }
        // Off the bitmap: empty, and no blend touches anything.
        for empty in [
            Snapshot::take(&old, 12, 0, 20, 8),
            Snapshot::take(&old, 3, 3, 3, 9),
            Snapshot::default(),
        ] {
            assert!(empty.is_empty() && !empty.fits(&old));
            let mut bmp = painted(12, 8, new_page);
            empty.crossfade(&mut bmp, 0.0);
            empty.slide_fade(&mut bmp, 3, 0, 0.5);
            empty.push(&mut bmp, 0, 2);
            assert_eq!(bmp.pixels, painted(12, 8, new_page).pixels);
        }
        // Taken again into the same memory.
        let mut again = snap.clone();
        again.retake(&old, 2, 1, 10, 7);
        assert_eq!(again, Snapshot::take(&old, 2, 1, 10, 7));
    }

    #[test]
    fn a_crossfade_is_the_old_at_nought_the_new_at_one_and_the_mix_between() {
        for (t, a) in [(0.0, 0u32), (0.25, 64), (0.5, 128), (0.75, 192), (1.0, 256)] {
            let (snap, mut bmp) = switched();
            snap.crossfade(&mut bmp, t);
            for y in 0..8 {
                for x in 0..12 {
                    let (o, n) = (old_page(x, y), new_page(x, y));
                    let want = if inside(x, y) {
                        // Rounded to the nearest level, each channel on its own.
                        std::array::from_fn(|i| {
                            ((o[i] as u32 * (256 - a) + n[i] as u32 * a + 128) / 256) as u8
                        })
                    } else {
                        n
                    };
                    assert_eq!(at(&bmp, x, y), want, "t {t} at {x},{y}");
                    if inside(x, y) {
                        // Never further from the true mix than one level.
                        for i in 0..3 {
                            let exact = o[i] as f32 + (n[i] as f32 - o[i] as f32) * t;
                            assert!((want[i] as f32 - exact).abs() <= 1.0);
                        }
                    }
                }
            }
        }
        // Out of range and not a number: clamped, and no number is "finished".
        for (t, same_as) in [(-2.0, 0.0), (9.0, 1.0), (f32::NAN, 1.0)] {
            let (snap, mut a) = switched();
            let (_, mut b) = switched();
            snap.crossfade(&mut a, t);
            snap.crossfade(&mut b, same_as);
            assert_eq!(a.pixels, b.pixels);
        }
    }

    #[test]
    fn a_fade_of_a_flat_colour_is_what_the_painter_would_blend() {
        // White drawn over black and faded to a half is mid grey: a group at half opacity.
        let black = painted(4, 4, |_, _| [0, 0, 0]);
        let snap = Snapshot::take(&black, 0, 0, 4, 4);
        let mut bmp = painted(4, 4, |_, _| [255, 255, 255]);
        snap.crossfade(&mut bmp, 0.5);
        assert_eq!(at(&bmp, 1, 1), [128, 128, 128]);
    }

    #[test]
    fn a_slide_moves_the_new_page_and_repeats_its_edge_behind_it() {
        // Fully opaque, so only the move shows: 3 px to the right, then 2 px down.
        for (dx, dy) in [(3, 0), (-3, 0), (0, 2), (0, -2), (2, 1)] {
            let (snap, mut bmp) = switched();
            snap.slide_fade(&mut bmp, dx, dy, 1.0);
            for y in 0..8u32 {
                for x in 0..12u32 {
                    let want = if inside(x, y) {
                        let sx = (x as i32 - dx).clamp(2, 9) as u32;
                        let sy = (y as i32 - dy).clamp(1, 6) as u32;
                        new_page(sx, sy)
                    } else {
                        new_page(x, y)
                    };
                    assert_eq!(at(&bmp, x, y), want, "by {dx},{dy} at {x},{y}");
                }
            }
        }
        // Half opaque: the moved new page over the old one where it lies.
        let (snap, mut bmp) = switched();
        snap.slide_fade(&mut bmp, 0, 2, 0.5);
        let (o, n) = (old_page(5, 5), new_page(5, 3));
        let want: [u8; 3] = std::array::from_fn(|i| (o[i] as u32 + n[i] as u32).div_ceil(2) as u8);
        assert_eq!(at(&bmp, 5, 5), want);
        // Invisible: the old page, whatever the offset; and no offset is a cross-fade.
        let (snap, mut bmp) = switched();
        snap.slide_fade(&mut bmp, 4, 0, 0.0);
        assert_eq!(at(&bmp, 5, 5), old_page(5, 5));
        let (snap, mut a) = switched();
        let (_, mut b) = switched();
        snap.slide_fade(&mut a, 0, 0, 0.3);
        snap.crossfade(&mut b, 0.3);
        assert_eq!(a.pixels, b.pixels);
    }

    #[test]
    fn a_push_moves_the_old_page_out_ahead_of_the_new() {
        for (dx, dy) in [(3, 0), (-3, 0), (0, 2), (0, -2), (8, 0), (0, -6)] {
            let (snap, mut bmp) = switched();
            snap.push(&mut bmp, dx, dy);
            for y in 0..8u32 {
                for x in 0..12u32 {
                    let want = if inside(x, y) {
                        // In the region's own coordinates: 8 wide, 6 high.
                        let (sx, sy) = (x as i32 - 2 - dx, y as i32 - 1 - dy);
                        if (0..8).contains(&sx) && (0..6).contains(&sy) {
                            new_page(sx as u32 + 2, sy as u32 + 1)
                        } else {
                            old_page(sx.rem_euclid(8) as u32 + 2, sy.rem_euclid(6) as u32 + 1)
                        }
                    } else {
                        new_page(x, y)
                    };
                    assert_eq!(at(&bmp, x, y), want, "by {dx},{dy} at {x},{y}");
                }
            }
        }
        // The whole width: the old page alone. Nothing: the new page alone.
        let (snap, mut bmp) = switched();
        snap.push(&mut bmp, 8, 0);
        assert_eq!(at(&bmp, 4, 4), old_page(4, 4));
        let (snap, mut bmp) = switched();
        snap.push(&mut bmp, 0, 0);
        assert_eq!(bmp.pixels, painted(12, 8, new_page).pixels);
    }

    #[test]
    fn a_snapshot_larger_than_the_bitmap_now_is_ignored() {
        let (snap, _) = switched();
        let mut small = painted(9, 8, new_page);
        assert!(!snap.fits(&small));
        snap.crossfade(&mut small, 0.2);
        snap.slide_fade(&mut small, 1, 0, 0.2);
        snap.push(&mut small, 1, 0);
        assert_eq!(small.pixels, painted(9, 8, new_page).pixels);
    }

    #[test]
    fn a_snapshot_is_blended_only_into_a_bitmap_of_the_size_it_was_taken_from() {
        let (snap, same) = switched();
        assert_eq!(snap.bitmap_size(), (12, 8));
        assert!(snap.fits(&same));
        // Larger in either direction: the rectangle still lies inside, but it is no longer
        // the same part of the window, so nothing is blended.
        for (w, h) in [(13, 8), (12, 9), (20, 12), (8, 12)] {
            let mut other = painted(w, h, new_page);
            assert!(!snap.fits(&other), "{w} by {h}");
            snap.restore(&mut other);
            snap.crossfade(&mut other, 0.5);
            snap.slide_fade(&mut other, 2, 0, 0.5);
            snap.push(&mut other, 0, 1);
            assert_eq!(other.pixels, painted(w, h, new_page).pixels, "{w} by {h}");
        }
        // Taken again from the new bitmap, it fits that one.
        let mut snap = snap;
        let big = painted(20, 12, old_page);
        snap.retake(&big, 2, 1, 10, 7);
        assert!(snap.fits(&big) && !snap.fits(&same));
        assert_eq!(Snapshot::default().bitmap_size(), (0, 0));
    }
}
