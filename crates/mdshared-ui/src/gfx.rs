//! Drawing for the chrome: clipped, antialiased boxes and the page's own text path (parley lays
//! out, `mdshared_text::TextEngine` rasterizes with subpixel masks), so UI text and document text
//! look the same. Callers work in logical px; the painter multiplies by the display scale and
//! snaps box edges to device pixels.

use mdshared_text::{Bitmap, TextEngine, shared_fonts};
use parley::{
    Alignment, AlignmentOptions, FontContext, FontFamily, FontWeight, Layout, LayoutContext,
    LineHeight, StyleProperty,
};
use std::collections::HashMap;
use std::rc::Rc;

mod covered;
#[cfg(test)]
pub(crate) use covered::WHOLE_SHADOW;
mod shapes;
pub use shapes::{Along, Path};

pub type Rgb = [u8; 3];
type Brush = [u8; 4];

pub const fn rgb(hex: u32) -> Rgb {
    [(hex >> 16) as u8, (hex >> 8) as u8, hex as u8]
}

/// `a` moved `t` of the way towards `b`.
pub fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let ch = |i: usize| (a[i] as f32 + (b[i] as f32 - a[i] as f32) * t).round() as u8;
    [ch(0), ch(1), ch(2)]
}

/// A rectangle in logical px.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect { x, y, w, h }
    }
    pub fn right(self) -> f32 {
        self.x + self.w
    }
    pub fn bottom(self) -> f32 {
        self.y + self.h
    }
    pub fn cx(self) -> f32 {
        self.x + self.w / 2.0
    }
    pub fn cy(self) -> f32 {
        self.y + self.h / 2.0
    }
    pub fn contains(self, x: f32, y: f32) -> bool {
        x >= self.x && y >= self.y && x < self.right() && y < self.bottom()
    }
    /// Shrinks every side by `d` (grows when negative).
    pub fn inset(self, d: f32) -> Rect {
        Rect::new(
            self.x + d,
            self.y + d,
            (self.w - 2.0 * d).max(0.0),
            (self.h - 2.0 * d).max(0.0),
        )
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Face {
    Ui,
    Semibold,
    /// Interface text in a fixed-width family ([`MONO_FAMILY`]): code, paths and anything that
    /// must line up in columns. It is the regular weight, and it is measured and elided like the
    /// other faces.
    Mono,
}

/// The interface's family list. On macOS the system's own (SF, which fontique reaches as the
/// generic `system-ui`), then Helvetica Neue.
const UI_FAMILY: &str = if cfg!(target_os = "macos") {
    "system-ui, Helvetica Neue, sans-serif"
} else {
    "Segoe UI Variable Text, Segoe UI, Inter, Noto Sans, Cantarell, DejaVu Sans, sans-serif"
};

/// The fixed-width family list of [`Face::Mono`], chosen the way the interface one is: the first
/// family installed wins, and the system's generic comes last. A family list that ends in a
/// generic names the colour emoji families before it, as [`mdshared_text::add_fallbacks`] asks;
/// no font file is shipped: these are the systems' own.
const MONO_FAMILY: &str = if cfg!(target_os = "macos") {
    "SF Mono, Menlo, Apple Color Emoji, monospace"
} else if cfg!(target_os = "windows") {
    "Cascadia Mono, Consolas, Courier New, Segoe UI Emoji, monospace"
} else {
    "DejaVu Sans Mono, Noto Sans Mono, Liberation Mono, Noto Color Emoji, monospace"
};

/// An icon stroked at one size: coverage, one byte per device pixel, and where its top left
/// corner sits from the corner of the icon's square.
struct IconMask {
    left: i32,
    top: i32,
    width: usize,
    data: Vec<u8>,
}

/// Font contexts, the glyph cache and a cache of laid-out labels. One per window.
pub struct Gfx {
    fonts: FontContext,
    layouts: LayoutContext<Brush>,
    engine: TextEngine,
    cache: HashMap<u64, Rc<Layout<Brush>>>,
    icons: HashMap<u64, Rc<IconMask>>,
}

fn key(face: Face, size: f32, color: Rgb, wrap: f32, text: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (face, size.to_bits(), color, wrap.to_bits(), text).hash(&mut h);
    h.finish()
}

impl Default for Gfx {
    fn default() -> Self {
        Gfx::new()
    }
}

impl Gfx {
    /// UI text follows the system's subpixel order (ClearType's on Windows), read once here.
    pub fn new() -> Gfx {
        let mut engine = TextEngine::new(2 << 20);
        engine.set_subpixel(crate::platform::subpixel_order());
        Gfx {
            fonts: shared_fonts(),
            layouts: LayoutContext::new(),
            engine,
            cache: HashMap::new(),
            icons: HashMap::new(),
        }
    }

    /// A laid-out label. `size` is in device px; `wrap` is a device-px line width, or 0 for one
    /// line. Layouts are cached, since the chrome draws the same labels every frame.
    fn layout(
        &mut self,
        face: Face,
        size: f32,
        color: Rgb,
        wrap: f32,
        text: &str,
    ) -> Rc<Layout<Brush>> {
        let k = key(face, size, color, wrap, text);
        if let Some(l) = self.cache.get(&k) {
            return l.clone();
        }
        if self.cache.len() > 4000 {
            self.cache.clear();
        }
        let mut b = self
            .layouts
            .ranged_builder(&mut self.fonts, text, 1.0, true);
        let family = if face == Face::Mono {
            MONO_FAMILY
        } else {
            UI_FAMILY
        };
        b.push_default(StyleProperty::FontFamily(FontFamily::from(family)));
        b.push_default(StyleProperty::FontSize(size));
        let weight = if face == Face::Semibold { 600.0 } else { 400.0 };
        b.push_default(StyleProperty::FontWeight(FontWeight::new(weight)));
        if wrap > 0.0 {
            b.push_default(StyleProperty::LineHeight(LineHeight::FontSizeRelative(
                1.45,
            )));
        }
        b.push_default(StyleProperty::Brush([color[0], color[1], color[2], 255]));
        mdshared_text::push_emoji_runs(&mut b, text, family);
        let mut layout = b.build(text);
        layout.break_all_lines(if wrap > 0.0 { Some(wrap) } else { None });
        layout.align(Alignment::Start, AlignmentOptions::default());
        let layout = Rc::new(layout);
        self.cache.insert(k, layout.clone());
        layout
    }

    /// The mask of an icon `side` device px across. The chrome draws the same few icons every
    /// frame, so each is stroked once per size.
    fn icon(&mut self, glyph: &str, side: f32) -> Rc<IconMask> {
        let k = key(Face::Ui, side, [0; 3], 0.0, glyph);
        if let Some(m) = self.icons.get(&k) {
            return m.clone();
        }
        if self.icons.len() > 500 {
            self.icons.clear();
        }
        let s = side / crate::glyph::GRID;
        let mut stroke = zeno::Stroke::new(crate::glyph::STROKE);
        stroke.cap(zeno::Cap::Round).join(zeno::Join::Round);
        let (data, at) = zeno::Mask::new(glyph)
            .style(stroke)
            .transform(Some(zeno::Transform::scale(s, s)))
            .render();
        let mask = Rc::new(IconMask {
            left: at.left,
            top: at.top,
            width: at.width as usize,
            data,
        });
        self.icons.insert(k, mask.clone());
        mask
    }
}

/// Draws into one bitmap with a clip rectangle. Coordinates are logical px.
pub struct Painter<'a> {
    pub bmp: &'a mut Bitmap,
    pub gfx: &'a mut Gfx,
    pub scale: f32,
    /// Device px: x0, y0, x1, y1.
    clip: (i32, i32, i32, i32),
    /// Device px added to everything drawn in logical px: see [`offset`](Painter::offset).
    origin: (i32, i32),
}

impl<'a> Painter<'a> {
    pub fn new(bmp: &'a mut Bitmap, gfx: &'a mut Gfx, scale: f32) -> Painter<'a> {
        let clip = (0, 0, bmp.width as i32, bmp.height as i32);
        Painter {
            bmp,
            gfx,
            scale,
            clip,
            origin: (0, 0),
        }
    }

    /// One device pixel, in logical px: the width of a hairline.
    pub fn px(&self) -> f32 {
        1.0 / self.scale
    }

    fn dev_x(&self, v: f32) -> i32 {
        (v * self.scale).round() as i32 + self.origin.0
    }

    fn dev_y(&self, v: f32) -> i32 {
        (v * self.scale).round() as i32 + self.origin.1
    }

    /// Moves everything drawn from now on by `dx, dy` logical px, on top of any offset already
    /// set, and returns the offset before, to be given back to
    /// [`restore_offset`](Self::restore_offset). The move is rounded to whole device pixels, so
    /// what is drawn moved is pixel for pixel what is drawn in place: text and box edges stay
    /// as sharp. For something that slides: a toast rising into place, a page coming in.
    ///
    /// It moves what is given in logical px, a [`clip`](Self::clip) set afterwards included.
    /// It does not move a clip already set, nor the calls that take device px
    /// ([`fill_dev`](Self::fill_dev), [`blit`](Self::blit)): those say where in the bitmap, and
    /// a caller that wants them moved adds [`origin`](Self::origin) itself.
    /// [`dev_rect`](Self::dev_rect) gives the moved rectangle.
    pub fn offset(&mut self, dx: f32, dy: f32) -> (i32, i32) {
        let old = self.origin;
        self.origin = (
            old.0 + (dx * self.scale).round() as i32,
            old.1 + (dy * self.scale).round() as i32,
        );
        old
    }

    pub fn restore_offset(&mut self, origin: (i32, i32)) {
        self.origin = origin;
    }

    /// The offset in force, in device px: zero unless [`offset`](Self::offset) set one.
    pub fn origin(&self) -> (i32, i32) {
        self.origin
    }

    /// Saves the pixels of `r` (cut to the clip) as they are now: before drawing something
    /// there that is to be faded ([`fade`](Self::fade)), or to keep the old content of a region
    /// that is about to change ([`crate::anim::PageTransition`]).
    pub fn snapshot(&self, r: Rect) -> crate::anim::Snapshot {
        let (x0, y0, x1, y1) = self.dev_rect(r);
        let c = self.clip;
        crate::anim::Snapshot::take(self.bmp, x0.max(c.0), y0.max(c.1), x1.min(c.2), y1.min(c.3))
    }

    /// Fades everything drawn since `under` was taken, in its rectangle, to `opacity` as one
    /// thing: at 1 it stays as drawn, at 0 it is gone, and at a half a popup's shadow, frame
    /// and text are half there together, with none of them showing through another. This is
    /// how a group of drawing calls gets an opacity in a painter that has no layers; the cost
    /// is a copy and a blend of the rectangle. See [`crate::anim::pop`] for the usual wrapper.
    pub fn fade(&mut self, under: &crate::anim::Snapshot, opacity: f32) {
        under.crossfade(self.bmp, opacity);
    }

    /// Restricts drawing to `r` (intersected with the bitmap) and returns the previous clip, to be
    /// given back to [`restore`](Self::restore).
    pub fn clip(&mut self, r: Rect) -> (i32, i32, i32, i32) {
        let old = self.clip;
        self.clip = (
            self.dev_x(r.x).max(old.0),
            self.dev_y(r.y).max(old.1),
            self.dev_x(r.right()).min(old.2),
            self.dev_y(r.bottom()).min(old.3),
        );
        old
    }

    pub fn restore(&mut self, clip: (i32, i32, i32, i32)) {
        self.clip = clip;
    }

    #[inline]
    fn span(&mut self, y: i32, x0: i32, x1: i32, c: Rgb, a: f32) {
        let (x0, x1) = (x0.max(self.clip.0), x1.min(self.clip.2));
        if y < self.clip.1 || y >= self.clip.3 || x0 >= x1 || a <= 0.0 {
            return;
        }
        let o = (y as usize * self.bmp.width as usize + x0 as usize) * 4;
        let row = &mut self.bmp.pixels[o..o + (x1 - x0) as usize * 4];
        if a >= 1.0 {
            let px = [c[2], c[1], c[0], 255];
            for d in row.as_chunks_mut::<4>().0 {
                d.copy_from_slice(&px);
            }
        } else {
            let a = (a * 256.0) as u32;
            let ia = 256 - a;
            for d in row.as_chunks_mut::<4>().0 {
                d[0] = ((c[2] as u32 * a + d[0] as u32 * ia) >> 8) as u8;
                d[1] = ((c[1] as u32 * a + d[1] as u32 * ia) >> 8) as u8;
                d[2] = ((c[0] as u32 * a + d[2] as u32 * ia) >> 8) as u8;
            }
        }
    }

    pub fn fill(&mut self, r: Rect, c: Rgb) {
        self.rbox_a(r, 0.0, c, 1.0);
    }

    pub fn fill_a(&mut self, r: Rect, c: Rgb, a: f32) {
        self.rbox_a(r, 0.0, c, a);
    }

    pub fn rbox(&mut self, r: Rect, radius: f32, c: Rgb) {
        self.rbox_a(r, radius, c, 1.0);
    }

    /// A rounded rectangle with antialiased corners, blended at `alpha`.
    pub fn rbox_a(&mut self, r: Rect, radius: f32, c: Rgb, alpha: f32) {
        let (x0, y0, x1, y1) = (
            self.dev_x(r.x),
            self.dev_y(r.y),
            self.dev_x(r.right()),
            self.dev_y(r.bottom()),
        );
        self.rbox_dev(x0, y0, x1, y1, radius * self.scale, c, alpha);
    }

    #[allow(clippy::too_many_arguments)] // drawing call: a rectangle plus style, no natural grouping
    fn rbox_dev(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, rad: f32, c: Rgb, alpha: f32) {
        if x1 <= x0 || y1 <= y0 {
            return;
        }
        let rad = rad
            .min((x1 - x0) as f32 / 2.0)
            .min((y1 - y0) as f32 / 2.0)
            .max(0.0);
        let band = rad.ceil() as i32;
        for y in y0.max(self.clip.1)..y1.min(self.clip.3) {
            let fy = y as f32 + 0.5;
            let qy = if fy < y0 as f32 + rad {
                y0 as f32 + rad - fy
            } else if fy > y1 as f32 - rad {
                fy - (y1 as f32 - rad)
            } else {
                0.0
            };
            if qy <= 0.0 || band == 0 {
                self.span(y, x0, x1, c, alpha);
                continue;
            }
            self.span(y, x0 + band, x1 - band, c, alpha);
            for i in 0..band {
                for x in [x0 + i, x1 - 1 - i] {
                    let fx = x as f32 + 0.5;
                    let qx = if fx < x0 as f32 + rad {
                        x0 as f32 + rad - fx
                    } else if fx > x1 as f32 - rad {
                        fx - (x1 as f32 - rad)
                    } else {
                        0.0
                    };
                    let cover = (rad - qx.hypot(qy) + 0.5).clamp(0.0, 1.0);
                    self.span(y, x, x + 1, c, alpha * cover);
                    if x0 + i == x1 - 1 - i {
                        break;
                    }
                }
            }
        }
    }

    /// A filled box with a one-device-pixel outline.
    pub fn frame(&mut self, r: Rect, radius: f32, fill: Rgb, stroke: Rgb) {
        let (x0, y0, x1, y1) = (
            self.dev_x(r.x),
            self.dev_y(r.y),
            self.dev_x(r.right()),
            self.dev_y(r.bottom()),
        );
        let rad = radius * self.scale;
        self.rbox_dev(x0, y0, x1, y1, rad, stroke, 1.0);
        self.rbox_dev(
            x0 + 1,
            y0 + 1,
            x1 - 1,
            y1 - 1,
            (rad - 1.0).max(0.0),
            fill,
            1.0,
        );
    }

    /// `r` in device px, as the painter rounds it: x0, y0, x1, y1.
    pub fn dev_rect(&self, r: Rect) -> (i32, i32, i32, i32) {
        (
            self.dev_x(r.x),
            self.dev_y(r.y),
            self.dev_x(r.right()),
            self.dev_y(r.bottom()),
        )
    }

    /// A plain box in device px, blended at `alpha`, for marks the workspace reports in device px.
    pub fn fill_dev(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, c: Rgb, alpha: f32) {
        self.rbox_dev(x0, y0, x1, y1, 0.0, c, alpha);
    }

    /// A soft drop shadow around `r`, for popups: a few expanding translucent boxes. It is
    /// [`shadow_with`](Self::shadow_with) at strength 1.
    pub fn shadow(&mut self, r: Rect, radius: f32, dark: bool) {
        self.shadow_with(r, radius, dark, 1.0);
    }

    /// The shadow of [`shadow`](Self::shadow) with each of its boxes `k` times as dark: 1 is
    /// that shadow, pixel for pixel, 0.5 half as dark, 2 twice. It reaches as far at every
    /// strength, so a popup's bounds do not depend on it. `k` is held to 0..=4; 0, a negative
    /// number and NaN draw nothing.
    pub fn shadow_with(&mut self, r: Rect, radius: f32, dark: bool, k: f32) {
        if k.is_nan() || k <= 0.0 {
            return;
        }
        let steps = 10;
        let base: f32 = if dark { 0.024 } else { 0.010 };
        let strength = base * k.min(4.0);
        for i in (1..=steps).rev() {
            let d = i as f32 * 1.4;
            let rr = Rect::new(r.x - d, r.y - d + 3.0, r.w + 2.0 * d, r.h + 2.0 * d);
            self.rbox_a(rr, radius + d, [0, 0, 0], strength);
        }
    }

    /// A filled polygon (even-odd), antialiased by sampling each pixel on a 4 x 4 grid. For the
    /// logo's few small shapes; it tests every sample against every edge.
    pub fn polygon(&mut self, points: &[(f32, f32)], c: Rgb) {
        const GRID: i32 = 4;
        if points.len() < 3 {
            return;
        }
        let s = self.scale;
        let (ox, oy) = (self.origin.0 as f32, self.origin.1 as f32);
        let pts: Vec<(f32, f32)> = points
            .iter()
            .map(|&(x, y)| (x * s + ox, y * s + oy))
            .collect();
        let (mut x0, mut x1, mut y0, mut y1) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
        for &(x, y) in &pts {
            (x0, x1, y0, y1) = (x0.min(x), x1.max(x), y0.min(y), y1.max(y));
        }
        let inside = |px: f32, py: f32| {
            let mut odd = false;
            let mut a = pts[pts.len() - 1];
            for &b in &pts {
                if (a.1 > py) != (b.1 > py) && px < a.0 + (py - a.1) / (b.1 - a.1) * (b.0 - a.0) {
                    odd = !odd;
                }
                a = b;
            }
            odd
        };
        for y in y0.floor() as i32..y1.ceil() as i32 {
            for x in x0.floor() as i32..x1.ceil() as i32 {
                let mut hits = 0;
                for sy in 0..GRID {
                    for sx in 0..GRID {
                        let px = x as f32 + (sx as f32 + 0.5) / GRID as f32;
                        let py = y as f32 + (sy as f32 + 0.5) / GRID as f32;
                        hits += i32::from(inside(px, py));
                    }
                }
                self.span(y, x, x + 1, c, hits as f32 / (GRID * GRID) as f32);
            }
        }
    }

    /// Copies `src` rows `sy..sy + h` to device position `(dx, dy)`, clipped.
    pub fn blit(&mut self, src: &Bitmap, sy: i32, h: i32, dx: i32, dy: i32) {
        let x0 = dx.max(self.clip.0);
        let x1 = (dx + src.width as i32).min(self.clip.2);
        if x0 >= x1 {
            return;
        }
        let n = (x1 - x0) as usize * 4;
        for row in 0..h.min(src.height as i32 - sy) {
            let y = dy + row;
            if y < self.clip.1 || y >= self.clip.3 || sy + row < 0 {
                continue;
            }
            let s = ((sy + row) as usize * src.width as usize + (x0 - dx) as usize) * 4;
            let d = (y as usize * self.bmp.width as usize + x0 as usize) * 4;
            self.bmp.pixels[d..d + n].copy_from_slice(&src.pixels[s..s + n]);
        }
    }

    /// The width of one line of text, in logical px.
    pub fn measure(&mut self, face: Face, size: f32, text: &str) -> f32 {
        if text.is_empty() {
            return 0.0;
        }
        self.gfx
            .layout(face, size * self.scale, [0, 0, 0], 0.0, text)
            .width()
            / self.scale
    }

    /// `text` shortened with an ellipsis so it fits `max` logical px.
    pub fn elide(&mut self, face: Face, size: f32, text: &str, max: f32) -> String {
        if self.measure(face, size, text) <= max {
            return text.to_string();
        }
        // Trailing spaces and dots are dropped before the ellipsis, so "notes.md" never reads
        // "notes.…".
        let cut = |end: usize| {
            format!(
                "{}…",
                text[..end].trim_end_matches(|c: char| c.is_whitespace() || c == '.')
            )
        };
        let ends: Vec<usize> = text.char_indices().map(|(i, _)| i).collect();
        let (mut lo, mut hi) = (0, ends.len());
        while lo < hi {
            let mid = (lo + hi).div_ceil(2);
            if mid < ends.len() && self.measure(face, size, &cut(ends[mid])) <= max {
                lo = mid;
            } else {
                hi = mid - 1;
            }
        }
        cut(ends[lo])
    }

    fn draw_layout(&mut self, layout: &Layout<Brush>, x: f32, y: f32) {
        let (w, h) = (
            layout.width().ceil() as i32 + 2,
            layout.height().ceil() as i32 + 2,
        );
        let (x, y) = (x + self.origin.0 as f32, y + self.origin.1 as f32);
        let (xi, yi) = (x.floor() as i32, y.floor() as i32);
        let c = self.clip;
        if xi >= c.2 || yi >= c.3 || xi + w <= c.0 || yi + h <= c.1 {
            return;
        }
        self.gfx
            .engine
            .draw_layout_clipped(self.bmp, layout, x, y, self.clip);
    }

    /// One line of text starting at `x`, vertically centred on `cy`, elided to `max` logical px.
    /// Returns the width drawn.
    #[allow(clippy::too_many_arguments)] // drawing call: a rectangle plus style, no natural grouping
    pub fn text(
        &mut self,
        face: Face,
        size: f32,
        x: f32,
        cy: f32,
        text: &str,
        c: Rgb,
        max: f32,
    ) -> f32 {
        if text.is_empty() || max <= 0.0 {
            return 0.0;
        }
        let shown;
        let mut text = text;
        if max.is_finite() && self.measure(face, size, text) > max {
            shown = self.elide(face, size, text, max);
            text = &shown;
        }
        let layout = self.gfx.layout(face, size * self.scale, c, 0.0, text);
        let y = (cy * self.scale - layout.height() / 2.0).round();
        self.draw_layout(&layout, (x * self.scale).round(), y);
        layout.width() / self.scale
    }

    /// One line centred in `r`.
    pub fn text_center(&mut self, face: Face, size: f32, r: Rect, text: &str, c: Rgb) {
        let w = self.measure(face, size, text).min(r.w);
        self.text(face, size, r.x + (r.w - w) / 2.0, r.cy(), text, c, r.w);
    }

    /// One line ending at `right`.
    pub fn text_right(
        &mut self,
        face: Face,
        size: f32,
        right: f32,
        cy: f32,
        text: &str,
        c: Rgb,
    ) -> f32 {
        let w = self.measure(face, size, text);
        self.text(face, size, right - w, cy, text, c, f32::INFINITY);
        w
    }

    /// A 1 device px underline below the characters `range` (byte offsets) of `text`, which was
    /// or would be drawn by [`text`](Self::text) at `x` and `cy` with the same `face` and `size`.
    /// The position comes from the layout's baseline; the extent of the characters is measured as
    /// a prefix, so kerning across the boundary may move it by a fraction of a pixel.
    #[allow(clippy::too_many_arguments)] // drawing call: as `text`
    pub fn underline_range(
        &mut self,
        face: Face,
        size: f32,
        x: f32,
        cy: f32,
        text: &str,
        range: std::ops::Range<usize>,
        c: Rgb,
    ) {
        if range.start >= range.end || range.end > text.len() {
            return;
        }
        let before = self.measure(face, size, &text[..range.start]);
        let width = self.measure(face, size, &text[range.clone()]);
        let layout = self.gfx.layout(face, size * self.scale, c, 0.0, text);
        let top = (cy * self.scale - layout.height() / 2.0).round();
        let baseline = layout
            .lines()
            .next()
            .map_or(layout.height() * 0.8, |l| l.metrics().baseline);
        let y = (top + baseline.round() + 2.0) / self.scale;
        self.fill(Rect::new(x + before, y, width, 1.0 / self.scale), c);
    }

    /// A 1 device px line through the characters `range` (byte offsets) of `text`, which was or
    /// would be drawn by [`text`](Self::text) at `x` and `cy` with the same `face` and `size`: the
    /// mirror of [`underline_range`](Self::underline_range), for struck-out text. The line sits
    /// half the lower case's height above the baseline, taken as three tenths of the font size
    /// (the layout's metrics carry no x-height); the extent of the characters is measured as a
    /// prefix, so kerning across the boundary may move it by a fraction of a pixel. As with the
    /// underline, a range that is empty or past the end of `text` draws nothing, and its ends
    /// must be character boundaries.
    #[allow(clippy::too_many_arguments)] // drawing call: as `text`
    pub fn strikethrough_range(
        &mut self,
        face: Face,
        size: f32,
        x: f32,
        cy: f32,
        text: &str,
        range: std::ops::Range<usize>,
        c: Rgb,
    ) {
        if range.start >= range.end || range.end > text.len() {
            return;
        }
        let before = self.measure(face, size, &text[..range.start]);
        let width = self.measure(face, size, &text[range.clone()]);
        let layout = self.gfx.layout(face, size * self.scale, c, 0.0, text);
        let top = (cy * self.scale - layout.height() / 2.0).round();
        let baseline = layout
            .lines()
            .next()
            .map_or(layout.height() * 0.8, |l| l.metrics().baseline);
        let y = (top + baseline.round() - 0.3 * size * self.scale).round();
        self.fill(
            Rect::new(x + before, y / self.scale, width, 1.0 / self.scale),
            c,
        );
    }

    /// An icon from [`glyph`](crate::glyph), `size` logical px across, centred on a point.
    pub fn icon(&mut self, glyph: &str, size: f32, cx: f32, cy: f32, c: Rgb) {
        let side = size * self.scale;
        let mask = self.gfx.icon(glyph, side);
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

    /// Wrapped text in a column `w` wide starting at `(x, y)`. Returns the height used.
    #[allow(clippy::too_many_arguments)] // drawing call: a rectangle plus style, no natural grouping
    pub fn paragraph(
        &mut self,
        face: Face,
        size: f32,
        x: f32,
        y: f32,
        w: f32,
        text: &str,
        c: Rgb,
    ) -> f32 {
        if text.is_empty() {
            return 0.0;
        }
        let layout = self
            .gfx
            .layout(face, size * self.scale, c, (w * self.scale).max(1.0), text);
        self.draw_layout(&layout, (x * self.scale).round(), (y * self.scale).round());
        layout.height() / self.scale
    }

    /// The x offset of each character boundary of `text` (one entry per `char`, plus the end), in
    /// logical px, for carets and selections in a text field.
    pub fn caret_stops(&mut self, face: Face, size: f32, text: &str) -> Vec<f32> {
        let mut stops = Vec::with_capacity(text.len() + 1);
        for (i, _) in text.char_indices() {
            stops.push(self.measure(face, size, &text[..i]));
        }
        stops.push(self.measure(face, size, text));
        stops
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::glyph;

    /// Every icon parses to the end, stays on its grid and puts ink on it.
    #[test]
    fn every_icon_draws_inside_its_square() {
        let mut gfx = Gfx::new();
        for (name, path) in glyph::ALL {
            let parsed = zeno::PathData::commands(path).count();
            let asked = path.bytes().filter(|b| b.is_ascii_alphabetic()).count();
            assert!(parsed >= asked, "{name}: {parsed} commands of {asked}");
            let m = gfx.icon(path, 64.0);
            let h = m.data.len() / m.width.max(1);
            assert!(m.width >= 32 || h >= 32, "{name} is {} by {h}", m.width);
            assert!(m.left >= 0 && m.top >= 0, "{name} starts outside");
            assert!(
                m.left + m.width as i32 <= 64 && m.top + h as i32 <= 64,
                "{name} ends outside"
            );
            assert!(m.data.contains(&255), "{name} has no ink");
        }
    }

    /// The sheet a person looks at: every icon at the sizes the apps use, in both themes.
    /// `cargo test -p mdshared-ui icon_sheet -- --ignored`, then open target/icons.png.
    #[test]
    #[ignore]
    fn icon_sheet() {
        let sizes = [8.5, 10.0, 13.0, 15.0, 16.0, 20.0, 32.0];
        let (cell, cols) = (180.0, 6);
        let rows = glyph::ALL.len().div_ceil(cols);
        let themes = [
            (rgb(0xfbfbfb), rgb(0x1f1f1f)),
            (rgb(0x1f1f1f), rgb(0xededed)),
        ];
        for (scale, file) in [(1.0, "icons.png"), (1.5, "icons-150.png")] {
            let mut bmp = Bitmap::new(
                (cell * cols as f32 * 2.0 * scale) as u32,
                (rows as f32 * 62.0 * scale) as u32,
            );
            let mut gfx = Gfx::new();
            let mut p = Painter::new(&mut bmp, &mut gfx, scale);
            for (half, (bg, fg)) in themes.into_iter().enumerate() {
                let ox = half as f32 * cell * cols as f32;
                p.fill(
                    Rect::new(ox, 0.0, cell * cols as f32, rows as f32 * 62.0),
                    bg,
                );
                for (i, (name, path)) in glyph::ALL.iter().enumerate() {
                    let (x, y) = (ox + (i % cols) as f32 * cell, (i / cols) as f32 * 62.0);
                    let mut cx = x + 12.0;
                    for s in sizes {
                        p.icon(path, s, cx + s / 2.0, y + 22.0, fg);
                        cx += s + 6.0;
                    }
                    p.text(Face::Ui, 10.0, x + 12.0, y + 50.0, name, fg, cell);
                }
            }
            let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
            bmp.write_png(&out.join(file)).unwrap();
        }
    }

    /// A white page with `draw` on it, at the scale of one.
    fn marked(draw: impl FnOnce(&mut Painter<'_>)) -> Bitmap {
        let mut bmp = Bitmap::new(120, 48);
        let mut gfx = Gfx::new();
        {
            let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
            p.fill(Rect::new(0.0, 0.0, 120.0, 48.0), [255, 255, 255]);
            draw(&mut p);
        }
        bmp
    }

    /// The rows of `bmp` that hold `c` exactly, and on each the first and last column that does.
    fn rows_of(bmp: &Bitmap, c: [u8; 3]) -> Vec<(usize, usize, usize)> {
        let wanted = [c[2], c[1], c[0]];
        (0..bmp.height as usize)
            .filter_map(|y| {
                let cols: Vec<usize> = (0..bmp.width as usize)
                    .filter(|&x| {
                        let at = (y * bmp.width as usize + x) * 4;
                        bmp.pixels[at..at + 3] == wanted
                    })
                    .collect();
                Some((y, *cols.first()?, *cols.last()?))
            })
            .collect()
    }

    #[test]
    fn a_strikethrough_draws_a_line_where_the_underline_is_not() {
        let text = "struck out";
        let red = [255, 0, 0];
        let plain = |p: &mut Painter| {
            p.text(Face::Ui, 14.0, 12.0, 24.0, text, [0, 0, 0], 200.0);
        };
        let underline = marked(|p| {
            plain(p);
            p.underline_range(Face::Ui, 14.0, 12.0, 24.0, text, 0..text.len(), red);
        });
        let strike = marked(|p| {
            plain(p);
            p.strikethrough_range(Face::Ui, 14.0, 12.0, 24.0, text, 0..text.len(), red);
        });
        let under = rows_of(&underline, red);
        let through = rows_of(&strike, red);
        assert_eq!(under.len(), 1, "the underline is one device px: {under:?}");
        assert_eq!(through.len(), 1, "the strike is one device px: {through:?}");
        assert!(
            through[0].0 < under[0].0,
            "the strike is above the baseline: {through:?} against {under:?}"
        );
        // The same characters, so the same span: only its height differs.
        assert_eq!(
            (through[0].1, through[0].2),
            (under[0].1, under[0].2),
            "the strike crosses exactly what the underline runs under"
        );
        // An empty range draws nothing, as the underline does.
        let none = marked(|p| {
            p.strikethrough_range(Face::Ui, 14.0, 12.0, 24.0, text, 4..4, red);
        });
        assert!(rows_of(&none, red).is_empty());
    }

    /// Every kind of drawing the painter has, around the point 20, 16.
    fn scene(p: &mut Painter<'_>) {
        p.shadow(Rect::new(20.0, 16.0, 60.0, 30.0), 6.0, false);
        p.frame(
            Rect::new(20.0, 16.0, 60.0, 30.0),
            6.0,
            [240, 240, 240],
            [90, 90, 90],
        );
        p.rbox_a(Rect::new(24.5, 20.0, 12.0, 8.0), 3.0, [200, 30, 30], 0.6);
        p.text(Face::Ui, 13.0, 26.0, 31.0, "Offset", [10, 10, 10], 50.0);
        p.underline_range(Face::Ui, 13.0, 26.0, 31.0, "Offset", 0..3, [0, 0, 200]);
        p.icon(glyph::CHECK, 12.0, 70.0, 26.0, [0, 120, 0]);
        p.polygon(&[(60.0, 36.0), (72.0, 36.0), (66.0, 44.0)], [120, 0, 120]);
        p.paragraph(
            Face::Ui,
            10.0,
            22.0,
            34.0,
            30.0,
            "two lines of it",
            [60, 60, 60],
        );
        let old = p.clip(Rect::new(20.0, 16.0, 8.0, 30.0));
        p.fill(Rect::new(0.0, 40.0, 200.0, 3.0), [0, 0, 0]);
        p.restore(old);
    }

    #[test]
    fn an_offset_moves_everything_by_whole_device_pixels_and_changes_nothing_else() {
        for (scale, by, moved) in [
            (1.0, (7.0, 5.0), (7, 5)),
            (1.0, (-3.4, 0.0), (-3, 0)),
            (1.5, (4.0, 2.0), (6, 3)),
            (1.25, (0.0, 9.0), (0, 11)),
        ] {
            let (w, h) = (180u32, 110u32);
            let draw = |by: Option<(f32, f32)>| {
                let mut bmp = Bitmap::new(w, h);
                let mut gfx = Gfx::new();
                {
                    let mut p = Painter::new(&mut bmp, &mut gfx, scale);
                    p.fill(Rect::new(0.0, 0.0, 200.0, 200.0), [255, 255, 255]);
                    assert_eq!(p.origin(), (0, 0));
                    if let Some((dx, dy)) = by {
                        let before = p.offset(dx, dy);
                        assert_eq!(before, (0, 0));
                        assert_eq!(p.origin(), moved);
                        // What `dev_rect` gives is where the drawing lands.
                        let at = p.dev_rect(Rect::new(20.0, 16.0, 60.0, 30.0));
                        assert_eq!(at.0, (20.0 * scale).round() as i32 + moved.0);
                        assert_eq!(at.1, (16.0 * scale).round() as i32 + moved.1);
                        scene(&mut p);
                        p.restore_offset(before);
                        assert_eq!(p.origin(), (0, 0));
                    } else {
                        scene(&mut p);
                    }
                }
                bmp
            };
            let (still, shifted) = (draw(None), draw(Some(by)));
            assert_ne!(still.pixels, shifted.pixels, "{by:?} moved nothing");
            let px = |bmp: &Bitmap, x: i32, y: i32| {
                if x < 0 || y < 0 || x >= w as i32 || y >= h as i32 {
                    return [255, 255, 255, 255];
                }
                let o = (y as usize * w as usize + x as usize) * 4;
                [
                    bmp.pixels[o],
                    bmp.pixels[o + 1],
                    bmp.pixels[o + 2],
                    bmp.pixels[o + 3],
                ]
            };
            for y in 0..h as i32 {
                for x in 0..w as i32 {
                    assert_eq!(
                        px(&shifted, x, y),
                        px(&still, x - moved.0, y - moved.1),
                        "scale {scale}, by {by:?}, at {x},{y}"
                    );
                }
            }
        }
    }

    #[test]
    fn offsets_add_up_and_leave_device_pixel_calls_and_an_earlier_clip_alone() {
        let bmp = marked(|p| {
            let old = p.clip(Rect::new(0.0, 0.0, 50.0, 48.0));
            let first = p.offset(10.0, 0.0);
            let second = p.offset(5.0, 2.0);
            assert_eq!((first, second, p.origin()), ((0, 0), (10, 0), (15, 2)));
            // Logical: moved, and cut by the clip that was set before the offset.
            p.fill(Rect::new(30.0, 10.0, 20.0, 4.0), [255, 0, 0]);
            // Device: where it says.
            p.fill_dev(2, 30, 6, 34, [0, 0, 255], 1.0);
            p.restore_offset(second);
            p.restore_offset(first);
            p.restore(old);
        });
        assert_eq!(
            rows_of(&bmp, [255, 0, 0]),
            vec![(12, 45, 49), (13, 45, 49), (14, 45, 49), (15, 45, 49)]
        );
        assert_eq!(rows_of(&bmp, [0, 0, 255]).first(), Some(&(30, 2, 5)));
    }

    #[test]
    fn a_fade_gives_what_was_drawn_since_the_snapshot_one_opacity() {
        let bmp = marked(|p| {
            let r = Rect::new(10.0, 10.0, 40.0, 20.0);
            let under = p.snapshot(r);
            assert_eq!(under.rect(), (10, 10, 50, 30));
            // Two boxes that overlap: faded one by one the overlap would be darker.
            p.fill(Rect::new(10.0, 10.0, 30.0, 20.0), [0, 0, 0]);
            p.fill(Rect::new(20.0, 10.0, 30.0, 20.0), [0, 0, 0]);
            p.fade(&under, 0.5);
            // Cut to the clip.
            let old = p.clip(Rect::new(0.0, 0.0, 20.0, 15.0));
            assert_eq!(p.snapshot(r).rect(), (10, 10, 20, 15));
            p.restore(old);
        });
        let grey = rows_of(&bmp, [128, 128, 128]);
        assert_eq!(grey.len(), 20);
        assert!(grey.iter().all(|&(_, x0, x1)| (x0, x1) == (10, 49)));
        assert!(rows_of(&bmp, [0, 0, 0]).is_empty());
    }

    #[test]
    fn the_monospace_face_measures_every_character_alike_and_the_interface_face_does_not() {
        let mut bmp = Bitmap::new(8, 8);
        let mut gfx = Gfx::new();
        let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
        let four = p.measure(Face::Mono, 13.0, "WWMW");
        for s in ["iiii", "....", "0000", "abcd"] {
            let w = p.measure(Face::Mono, 13.0, s);
            assert!(
                (w - four).abs() < 0.01,
                "{s} measures {w} where four wide ones measure {four}"
            );
        }
        assert_ne!(
            p.measure(Face::Ui, 13.0, "iiii"),
            p.measure(Face::Ui, 13.0, "WWWW"),
            "the interface face is not fixed width"
        );
        // The family reaches the drawing: one word is not the same pixels in the two faces.
        let mono = marked(|p| {
            p.text(Face::Mono, 13.0, 12.0, 24.0, "code", [0, 0, 0], 200.0);
        });
        let ui = marked(|p| {
            p.text(Face::Ui, 13.0, 12.0, 24.0, "code", [0, 0, 0], 200.0);
        });
        assert_ne!(
            mono.pixels, ui.pixels,
            "the monospace family drew nothing new"
        );
    }
}
