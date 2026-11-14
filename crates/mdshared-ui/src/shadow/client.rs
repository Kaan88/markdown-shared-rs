//! The client-drawn shadow: painting the margin of a surface buffer and the geometry maths.
//! Pure code (no windowing), shared by the Wayland and X11 routes.

#![cfg_attr(
    not(all(target_os = "linux", feature = "client-margin")),
    allow(dead_code)
)]

use super::{Tiles, render};

/// The pre-rendered shadow of one scale/focus state, as premultiplied ARGB `u32`s (the shadow is
/// black, so a pixel is just `alpha << 24`).
pub struct Painter {
    /// Device px of the shadow's reach.
    pub m: u32,
    /// The eight tiles in the protocol's order (`Tiles::tiles`), as `u32`s.
    pix: [Vec<u32>; 8],
}

impl Painter {
    pub fn new(scale: f64, active: bool) -> Painter {
        Painter::from_tiles(&render(scale, active))
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
        }
    }

    /// Bytes of pre-rendered tiles kept.
    pub fn bytes(&self) -> usize {
        self.pix.iter().map(|v| v.len() * 4).sum()
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
