//! The client-drawn shadow: painting the margin of a surface buffer and the geometry maths.
//! Pure code (no windowing), shared by the Wayland and X11 routes.

#![cfg_attr(
    not(all(target_os = "linux", feature = "client-margin")),
    allow(dead_code)
)]

use super::{Tiles, coverage, metrics, peak, render};

/// The largest corner radius drawn, device px.
const MAX_RADIUS: u32 = 48;

/// The pre-rendered shadow of one scale/focus state, as premultiplied ARGB `u32`s (the shadow is
/// black, so a pixel is just `alpha << 24`), and with a radius the rounded corners too.
pub struct Painter {
    /// Device px of the shadow's reach.
    pub m: u32,
    /// The eight tiles in the protocol's order (`Tiles::tiles`), as `u32`s.
    pix: [Vec<u32>; 8],
    /// The rounded corners, when the radius is above 0.
    round: Option<Round>,
}

/// What rounded corners need, for a radius `r` (device px).
struct Round {
    r: u32,
    /// The coverage of the top-left corner square, `r * r`, row-major (`corners::mask`).
    cov: Vec<u8>,
    /// The shadow's alpha beneath a top corner square and beneath a bottom one, `r * r` each,
    /// indexed like `cov`: by the pixel's distance in from the vertical and the horizontal edge.
    /// The shadow of a rounded rectangle differs from the plain rectangle's by at most about one
    /// alpha step (the cut corner is small against the blur), so the margin keeps its tiles; only
    /// the pixels the rounding uncovers inside the content's corner squares need a value.
    shadow: [Vec<u8>; 2],
}

impl Round {
    fn new(scale: f64, active: bool, r: u32) -> Round {
        let (_, sigma, drop) = metrics(scale);
        let peak = peak(active);
        let table = |dy: f64| -> Vec<u8> {
            let mut out = Vec::with_capacity((r * r) as usize);
            for v in 0..r {
                // Inside the window by `v + 0.5`; the shadow's edge is `dy` further in (top: the
                // drop) or out (bottom).
                let av = coverage(dy - (v as f64 + 0.5), sigma);
                for u in 0..r {
                    let ah = coverage(-(u as f64 + 0.5), sigma);
                    out.push((ah * av * peak * 255.0).round().clamp(0.0, 255.0) as u8);
                }
            }
            out
        };
        Round {
            r,
            cov: crate::corners::mask(r),
            shadow: [table(drop), table(-drop)],
        }
    }
}

impl Painter {
    pub fn new(scale: f64, active: bool) -> Painter {
        Painter::with_radius(scale, active, 0)
    }

    /// A painter whose window corners have a radius of `radius` device px (0: square; at most 48).
    pub fn with_radius(scale: f64, active: bool, radius: u32) -> Painter {
        let mut p = Painter::from_tiles(&render(scale, active));
        let r = radius.min(MAX_RADIUS);
        if r > 0 {
            p.round = Some(Round::new(scale, active, r));
        }
        p
    }

    /// The corner radius in device px, 0 for square corners.
    pub fn radius(&self) -> u32 {
        self.round.as_ref().map_or(0, |r| r.r)
    }

    pub fn from_tiles(t: &Tiles) -> Painter {
        let conv = |i: usize| -> Vec<u32> {
            t.tiles[i]
                .2
                .as_chunks::<4>()
                .0
                .iter()
                .map(|p| u32::from_le_bytes(*p))
                .collect()
        };
        Painter {
            m: t.m,
            pix: std::array::from_fn(conv),
            round: None,
        }
    }

    /// Bytes of pre-rendered tiles kept.
    pub fn bytes(&self) -> usize {
        let round = self.round.as_ref().map_or(0, |r| {
            r.cov.len() + r.shadow.iter().map(Vec::len).sum::<usize>()
        });
        self.pix.iter().map(|v| v.len() * 4).sum::<usize>() + round
    }

    /// Fills the margin of a `w` x `h` surface buffer (row-major, stride `w`) with the shadow and
    /// leaves every pixel of the content rectangle untouched. `m` is the margin in force: the
    /// tile depth, or 0 (maximized, tiled: nothing is painted). A surface too small to have a
    /// content rectangle, or a buffer shorter than `w * h`, is left alone.
    pub fn paint(&self, pixels: &mut [u32], w: u32, h: u32, m: u32) {
        let (w, h, k) = (w as usize, h as usize, self.m as usize);
        if m == 0 || m != self.m || w <= 2 * k || h <= 2 * k || pixels.len() < w * h {
            return;
        }
        let [
            top,
            top_right,
            right,
            bottom_right,
            bottom,
            bottom_left,
            left,
            top_left,
        ] = &self.pix;
        // Top and bottom bands: a corner tile's row, a run of the edge tile's value, a corner row.
        for y in 0..k {
            let row = &mut pixels[y * w..(y + 1) * w];
            row[..k].copy_from_slice(&top_left[y * k..(y + 1) * k]);
            row[k..w - k].fill(top[y]);
            row[w - k..].copy_from_slice(&top_right[y * k..(y + 1) * k]);
            let yb = h - k + y;
            let row = &mut pixels[yb * w..(yb + 1) * w];
            row[..k].copy_from_slice(&bottom_left[y * k..(y + 1) * k]);
            row[k..w - k].fill(bottom[y]);
            row[w - k..].copy_from_slice(&bottom_right[y * k..(y + 1) * k]);
        }
        // Left and right bands: the side tiles are `k` wide and one row tall.
        for y in k..h - k {
            let row = &mut pixels[y * w..(y + 1) * w];
            row[..k].copy_from_slice(left);
            row[w - k..].copy_from_slice(right);
        }
    }

    /// Cuts the rounded corners out of the content copied into a `w` x `h` surface buffer (the
    /// content sits `m` px in from every edge). The corner squares are rebuilt from `content` (the
    /// app's BGRA bytes, `w - 2m` px wide): each pixel is the content at its coverage over the
    /// shadow beneath, premultiplied. `damage` is the content rectangles copied this frame, or
    /// `None` for all of it; only the corner squares they touch are done. A surface or content
    /// too small for the corners, or without a margin, is left alone.
    pub fn paint_corners(
        &self,
        pixels: &mut [u32],
        w: u32,
        h: u32,
        m: u32,
        content: &[u8],
        damage: Option<&[(usize, usize, usize, usize)]>,
    ) {
        let Some(round) = &self.round else { return };
        let (k, r) = (m as usize, round.r as usize);
        let (w, h) = (w as usize, h as usize);
        if m == 0 || m != self.m || w <= 2 * k + 2 * r || h <= 2 * k + 2 * r || pixels.len() < w * h
        {
            return;
        }
        let (cw, ch) = (w - 2 * k, h - 2 * k);
        if content.len() < cw * ch * 4 {
            return;
        }
        for (right, bottom) in [(false, false), (true, false), (false, true), (true, true)] {
            // The corner square in content coordinates.
            let (cx, cy) = (
                if right { cw - r } else { 0 },
                if bottom { ch - r } else { 0 },
            );
            let touched = damage.is_none_or(|rects| {
                rects
                    .iter()
                    .any(|d| d.0 < cx + r && cx < d.2 && d.1 < cy + r && cy < d.3)
            });
            if !touched {
                continue;
            }
            let shadow = &round.shadow[bottom as usize];
            for v in 0..r {
                for u in 0..r {
                    // The coverage mask is for the top-left corner; the others are mirrored.
                    let (mu, mv) = (
                        if right { r - 1 - u } else { u },
                        if bottom { r - 1 - v } else { v },
                    );
                    let cov = round.cov[mv * r + mu] as u32;
                    let under = shadow[mv * r + mu] as u32;
                    let alpha = cov + (under * (255 - cov) + 127) / 255;
                    let src = ((cy + v) * cw + cx + u) * 4;
                    let part = |c: u8| (c as u32 * cov + 127) / 255;
                    let px = (alpha << 24)
                        | (part(content[src + 2]) << 16)
                        | (part(content[src + 1]) << 8)
                        | part(content[src]);
                    pixels[(cy + k + v) * w + cx + k + u] = px;
                }
            }
        }
    }

    /// The window as the client route puts it on the screen, for a picture: `content` (BGRA
    /// bytes, `cw` x `ch` px) with the shadow margin and the rounded corners, composited over a
    /// flat `backdrop`. Returns the width, the height and opaque BGRA bytes.
    pub fn preview(
        &self,
        content: &[u8],
        cw: u32,
        ch: u32,
        backdrop: [u8; 3],
    ) -> (u32, u32, Vec<u8>) {
        let m = self.m;
        let (w, h) = (cw + 2 * m, ch + 2 * m);
        let mut surface = vec![0u32; w as usize * h as usize];
        for y in 0..ch as usize {
            let row = &mut surface[(y + m as usize) * w as usize + m as usize..][..cw as usize];
            let src = &content[y * cw as usize * 4..][..cw as usize * 4];
            for (px, c) in row.iter_mut().zip(src.as_chunks::<4>().0) {
                *px = u32::from_le_bytes([c[0], c[1], c[2], 255]);
            }
        }
        self.paint(&mut surface, w, h, m);
        self.paint_corners(&mut surface, w, h, m, content, None);
        let mut out = Vec::with_capacity(surface.len() * 4);
        for px in surface {
            let a = px >> 24;
            // Premultiplied over the backdrop: the pixel plus what the backdrop keeps of itself.
            let over = |shift: u32, back: u8| {
                (((px >> shift) & 255) + (back as u32 * (255 - a) + 127) / 255).min(255) as u8
            };
            out.extend_from_slice(&[
                over(0, backdrop[2]),
                over(8, backdrop[1]),
                over(16, backdrop[0]),
                255,
            ]);
        }
        (w, h, out)
    }
}

/// Decides when the margin of a buffer has to be painted again. softbuffer's Wayland backend swaps
/// two buffers and resizes each one lazily when it becomes the back buffer, so after any change
/// (size, margin, scale, focus) BOTH buffers hold something else in their margin, one after the
/// other: two repaints. Otherwise a buffer of nonzero age still has the margin it was given.
#[derive(Debug, Default)]
pub struct Gate {
    left: u8,
    size: (u32, u32),
}

impl Gate {
    /// The look of the margin changed: repaint the next two frames.
    pub fn invalidate(&mut self) {
        self.left = 2;
    }

    /// Whether the buffer about to be presented (`age` as softbuffer reports it, 0 = undefined
    /// contents) needs its margin painted.
    pub fn needed(&mut self, size: (u32, u32), age: u8) -> bool {
        if size != self.size {
            self.size = size;
            self.left = 2;
        }
        if age == 0 || self.left > 0 {
            self.left = self.left.saturating_sub(1);
            return true;
        }
        false
    }
}

/// Whether a pointer at surface position `(x, y)` (device px) lands on the window: inside the
/// content rectangle grown by `band` px, which is what the winit patch sets as the input region.
/// Outside, the click goes through to what is behind the window.
pub fn takes_input(x: f64, y: f64, w: u32, h: u32, m: u32, band: u32) -> bool {
    if m == 0 {
        return x >= 0.0 && y >= 0.0 && x < w as f64 && y < h as f64;
    }
    let b = band.min(m) as f64;
    let (m, w, h) = (m as f64, w as f64, h as f64);
    x >= m - b && y >= m - b && x < w - m + b && y < h - m + b
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONTENT: u32 = 0xFF12_3456;

    fn surface(w: u32, h: u32) -> Vec<u32> {
        vec![CONTENT; (w * h) as usize]
    }

    #[test]
    fn paint_fills_only_the_margin() {
        let p = Painter::new(1.0, true);
        let m = p.m;
        let (w, h) = (200u32, 150u32);
        let mut buf = surface(w, h);
        p.paint(&mut buf, w, h, m);
        for y in 0..h {
            for x in 0..w {
                let inside = x >= m && x < w - m && y >= m && y < h - m;
                let v = buf[(y * w + x) as usize];
                if inside {
                    assert_eq!(v, CONTENT, "content pixel ({x},{y}) touched");
                } else {
                    assert_ne!(v, CONTENT, "margin pixel ({x},{y}) not painted");
                    assert_eq!(v & 0x00FF_FFFF, 0, "shadow is black ({x},{y})");
                }
            }
        }
    }

    #[test]
    fn margin_matches_the_tiles_and_is_symmetric_where_it_should() {
        let p = Painter::new(1.0, true);
        let m = p.m as usize;
        let (w, h) = (300usize, 200usize);
        let mut buf = surface(w as u32, h as u32);
        p.paint(&mut buf, w as u32, h as u32, p.m);
        // Along the middle of the top edge the alpha rises toward the window.
        let col = |x: usize, y: usize| buf[y * w + x] >> 24;
        assert!(col(150, m - 1) > col(150, m / 2) && col(150, m / 2) > col(150, 0));
        // The corners are mirror images left/right, and the bottom is darker than the top.
        assert_eq!(col(0, 0), col(w - 1, 0));
        assert_eq!(col(m - 1, m - 1), col(w - m, m - 1));
        assert!(col(150, h - m) > col(150, m - 1));
        // The side bands are continuous with the corners above and below.
        assert_eq!(col(5, m - 1), col(5, m));
    }

    #[test]
    fn paint_does_nothing_without_a_margin_or_room() {
        let p = Painter::new(1.0, true);
        let mut buf = surface(100, 100);
        p.paint(&mut buf, 100, 100, 0);
        assert!(buf.iter().all(|&v| v == CONTENT));
        // Too small to have a content rectangle (2 * 36 >= 60): untouched, no panic.
        let mut small = surface(60, 60);
        p.paint(&mut small, 60, 60, p.m);
        assert!(small.iter().all(|&v| v == CONTENT));
        // A buffer shorter than w * h is left alone too.
        let mut short = vec![CONTENT; 10];
        p.paint(&mut short, 200, 200, p.m);
        assert!(short.iter().all(|&v| v == CONTENT));
    }

    #[test]
    fn hidpi_margin_scales() {
        let p = Painter::new(2.0, true);
        assert_eq!(p.m, 72);
        let (w, h) = (400u32, 300u32);
        let mut buf = surface(w, h);
        p.paint(&mut buf, w, h, 72);
        assert_eq!(buf[(150 * w + 100) as usize], CONTENT);
        assert_ne!(buf[(150 * w + 3) as usize], CONTENT);
    }

    #[test]
    fn gate_repaints_two_frames_after_a_change_then_none() {
        let mut g = Gate::default();
        let s = (672, 472);
        // First sight of a size: two frames (both buffers), even though their age is 1 and 2.
        assert!(g.needed(s, 1));
        assert!(g.needed(s, 2));
        assert!(!g.needed(s, 2));
        assert!(!g.needed(s, 1));
        // A buffer of age 0 (fresh, undefined) is always painted.
        assert!(g.needed(s, 0));
        assert!(!g.needed(s, 2));
        // A resize or a look change starts it again.
        assert!(g.needed((700, 500), 2));
        assert!(g.needed((700, 500), 2));
        assert!(!g.needed((700, 500), 2));
        g.invalidate();
        assert!(g.needed((700, 500), 2));
        assert!(g.needed((700, 500), 2));
        assert!(!g.needed((700, 500), 2));
    }

    /// A `w` x `h` surface with the content rectangle (`m` px in from every edge) filled with
    /// `CONTENT`, and the same content as the app's BGRA bytes.
    fn surface_with_content(w: u32, h: u32, m: u32) -> (Vec<u32>, Vec<u8>) {
        let (cw, ch) = ((w - 2 * m) as usize, (h - 2 * m) as usize);
        let mut buf = vec![0u32; (w * h) as usize];
        copy_content(&mut buf, w, m, cw, ch);
        let bytes = CONTENT.to_le_bytes();
        (buf, bytes.repeat(cw * ch))
    }

    fn copy_content(buf: &mut [u32], w: u32, m: u32, cw: usize, ch: usize) {
        for y in 0..ch {
            let at = (y + m as usize) * w as usize + m as usize;
            buf[at..at + cw].fill(CONTENT);
        }
    }

    const W: u32 = 300;
    const H: u32 = 200;

    #[test]
    fn rounded_paint_touches_the_margin_and_the_corner_squares_only() {
        let p = Painter::with_radius(1.0, true, 8);
        assert_eq!(p.radius(), 8);
        let m = p.m;
        let (mut buf, content) = surface_with_content(W, H, m);
        p.paint(&mut buf, W, H, m);
        let (cw, ch, r) = (W - 2 * m, H - 2 * m, 8u32);
        for y in 0..H {
            for x in 0..W {
                let inside = x >= m && x < W - m && y >= m && y < H - m;
                let v = buf[(y * W + x) as usize];
                if inside {
                    assert_eq!(v, CONTENT, "paint touched content ({x},{y})");
                } else {
                    assert_ne!(v, CONTENT, "margin pixel ({x},{y}) not painted");
                    assert_eq!(v & 0x00FF_FFFF, 0, "shadow is black ({x},{y})");
                }
            }
        }
        p.paint_corners(&mut buf, W, H, m, &content, None);
        let in_square = |cx: u32, cy: u32| (cx < r || cx >= cw - r) && (cy < r || cy >= ch - r);
        for cy in 0..ch {
            for cx in 0..cw {
                let v = buf[((cy + m) * W + cx + m) as usize];
                if !in_square(cx, cy) {
                    assert_eq!(v, CONTENT, "outside the squares ({cx},{cy}) changed");
                }
            }
        }
        let at = |cx: u32, cy: u32| buf[((cy + m) * W + cx + m) as usize];
        // The outermost pixel of a corner is the shadow alone: no colour, partial alpha.
        for (cx, cy) in [(0, 0), (cw - 1, 0), (0, ch - 1), (cw - 1, ch - 1)] {
            let v = at(cx, cy);
            assert_eq!(v & 0x00FF_FFFF, 0, "({cx},{cy}) has no colour");
            assert!(
                v >> 24 > 20 && v >> 24 < 255,
                "({cx},{cy}) shows the shadow"
            );
        }
        // The innermost pixel of each square is the content, whole.
        for (cx, cy) in [
            (r - 1, r - 1),
            (cw - r, r - 1),
            (r - 1, ch - r),
            (cw - r, ch - r),
        ] {
            assert_eq!(at(cx, cy), CONTENT, "({cx},{cy}) is inside the arc");
        }
        // On the arc: partial coverage, a darker blend with alpha between the two.
        let arc = at(1, 3);
        assert!(arc >> 24 > at(0, 0) >> 24 && arc >> 24 < 255 || arc == CONTENT);
    }

    #[test]
    fn rounded_corners_mirror_left_to_right() {
        let p = Painter::with_radius(1.0, true, 8);
        let m = p.m;
        let (mut buf, content) = surface_with_content(W, H, m);
        p.paint(&mut buf, W, H, m);
        p.paint_corners(&mut buf, W, H, m, &content, None);
        for y in 0..H {
            for x in 0..W / 2 {
                let (a, b) = (buf[(y * W + x) as usize], buf[(y * W + W - 1 - x) as usize]);
                // Only the corner stretches differ from a mirror image by nothing at all.
                if y < m + 8 + 8 || y >= H - m - 8 - 8 {
                    assert_eq!(a, b, "({x},{y}) against its mirror");
                }
            }
        }
    }

    #[test]
    fn only_the_corners_in_the_damage_are_redone() {
        let p = Painter::with_radius(1.0, true, 8);
        let m = p.m;
        let (cw, ch) = ((W - 2 * m) as usize, (H - 2 * m) as usize);
        let (mut buf, content) = surface_with_content(W, H, m);
        p.paint(&mut buf, W, H, m);
        let at = |buf: &[u32], cx: u32, cy: u32| buf[((cy + m) * W + cx + m) as usize];
        // Content copied again (as a damage copy does) but the damage is in the middle: the
        // corners are not touched, and a caller that copied them would have to name them.
        p.paint_corners(&mut buf, W, H, m, &content, Some(&[(100, 50, 120, 60)]));
        assert_eq!(at(&buf, 0, 0), CONTENT);
        // Damage over the top-left corner only.
        p.paint_corners(&mut buf, W, H, m, &content, Some(&[(0, 0, 3, 3)]));
        assert_ne!(at(&buf, 0, 0), CONTENT);
        assert_eq!(
            at(&buf, cw as u32 - 1, 0),
            CONTENT,
            "top-right was not in the damage"
        );
        assert_eq!(at(&buf, 0, ch as u32 - 1), CONTENT);
        // An empty damage list touches nothing; a rectangle that only ends at the square's edge
        // does not touch it.
        let before = buf.clone();
        p.paint_corners(&mut buf, W, H, m, &content, Some(&[]));
        p.paint_corners(&mut buf, W, H, m, &content, Some(&[(8, 8, 50, 50)]));
        assert_eq!(buf, before);
        // The whole content copied again: every corner is redone, and doing it twice from the
        // same content gives the same pixels (the mask is built from the content, not the buffer).
        copy_content(&mut buf, W, m, cw, ch);
        p.paint_corners(&mut buf, W, H, m, &content, None);
        let once = buf.clone();
        copy_content(&mut buf, W, m, cw, ch);
        p.paint_corners(&mut buf, W, H, m, &content, None);
        assert_eq!(buf, once);
        assert_ne!(at(&buf, cw as u32 - 1, ch as u32 - 1), CONTENT);
    }

    #[test]
    fn the_margin_is_the_same_with_and_without_rounding() {
        // The rounded rectangle's blurred shadow differs from the plain one's by about one alpha
        // step, so the margin keeps the plain tiles: painting it is the same call either way.
        for (scale, r) in [(1.0, 8), (2.0, 16), (1.0, 4), (1.5, 12)] {
            let round = Painter::with_radius(scale, true, r);
            let plain = Painter::new(scale, true);
            let m = round.m;
            let (w, h) = (m * 2 + 400, m * 2 + 300);
            let mut a = vec![0u32; (w * h) as usize];
            let mut b = a.clone();
            round.paint(&mut a, w, h, m);
            plain.paint(&mut b, w, h, m);
            assert_eq!(a, b, "scale {scale} radius {r}");
        }
    }

    #[test]
    fn the_shadow_under_a_cut_corner_follows_the_margin() {
        // Beside the corner square the margin has the plain shadow; under the square the table
        // continues it: no jump across the content's edge, and the top corner is lighter than
        // the bottom one (the shadow is dropped).
        let p = Painter::with_radius(1.0, true, 8);
        let round = p.round.as_ref().unwrap();
        let r = 8usize;
        let (top, bottom) = (&round.shadow[0], &round.shadow[1]);
        assert!(top[0] < bottom[0]);
        // Deeper inside the window the shadow is darker (it is the window's own, blurred).
        assert!(top[r * r - 1] > top[0] && bottom[r * r - 1] > bottom[0]);
    }

    #[test]
    fn corners_are_left_alone_without_room_or_margin() {
        let p = Painter::with_radius(1.0, true, 8);
        let m = p.m;
        let (mut buf, content) = surface_with_content(W, H, m);
        let before = buf.clone();
        // No margin in force (maximized): nothing is cut.
        p.paint_corners(&mut buf, W, H, 0, &content, None);
        assert_eq!(buf, before);
        // Content shorter than the surface says: nothing is cut, no panic.
        p.paint_corners(&mut buf, W, H, m, &content[..100], None);
        assert_eq!(buf, before);
        // A surface too small for the corners cuts nothing.
        let (mut small, small_content) = surface_with_content(2 * m + 12, 2 * m + 12, m);
        let before = small.clone();
        p.paint_corners(&mut small, 2 * m + 12, 2 * m + 12, m, &small_content, None);
        assert_eq!(small, before);
        p.paint(&mut small, 2 * m + 12, 2 * m + 12, m);
        // A square painter has no corners to cut.
        let sq = Painter::new(1.0, true);
        assert_eq!(sq.radius(), 0);
        let (mut buf, content) = surface_with_content(W, H, m);
        let before = buf.clone();
        sq.paint_corners(&mut buf, W, H, m, &content, None);
        assert_eq!(buf, before);
    }

    #[test]
    fn preview_composes_the_window_over_a_backdrop() {
        let p = Painter::with_radius(1.0, true, 8);
        let m = p.m;
        let (cw, ch) = (120u32, 80u32);
        let content = [0x56u8, 0x34, 0x12, 0xFF].repeat((cw * ch) as usize);
        let backdrop = [200u8, 200, 205];
        let (w, h, px) = p.preview(&content, cw, ch, backdrop);
        assert_eq!((w, h), (cw + 2 * m, ch + 2 * m));
        assert_eq!(px.len(), (w * h * 4) as usize);
        let at = |x: u32, y: u32| {
            let o = ((y * w + x) * 4) as usize;
            [px[o + 2], px[o + 1], px[o], px[o + 3]]
        };
        // Far in the margin the backdrop shows; the middle is the content; alpha is opaque.
        let far = at(0, 0);
        assert!(
            far[0].abs_diff(200) <= 1 && far[2].abs_diff(205) <= 1,
            "{far:?}"
        );
        assert_eq!(at(w / 2, h / 2), [0x12, 0x34, 0x56, 255]);
        // The window's own corner pixel is the backdrop with the shadow on it: darker than the
        // backdrop, and not the content colour.
        let corner = at(m, m);
        assert!(
            corner[0] < 195 && corner != [0x12, 0x34, 0x56, 255],
            "{corner:?}"
        );
        assert!(px.chunks(4).all(|c| c[3] == 255));
    }

    #[test]
    fn input_region_logic() {
        // 36 px margin, band 6, a 672x472 surface: the content starts at 36.
        let t = |x, y| takes_input(x, y, 672, 472, 36, 6);
        assert!(!t(0.0, 0.0), "the shadow's corner lets clicks through");
        assert!(!t(10.0, 200.0), "far left of the content");
        assert!(!t(29.9, 200.0), "just beyond the resize band");
        assert!(t(30.0, 200.0), "the band takes input");
        assert!(t(36.0, 36.0), "the content corner");
        assert!(t(400.0, 300.0));
        assert!(t(641.9, 300.0), "right band, up to 6 px past the content");
        assert!(!t(642.0, 300.0));
        assert!(!t(300.0, 470.0), "bottom shadow");
        // No margin (maximized): the whole surface.
        assert!(takes_input(0.0, 0.0, 100, 100, 0, 6));
        assert!(!takes_input(100.0, 50.0, 100, 100, 0, 6));
    }
}
