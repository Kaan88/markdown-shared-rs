//! The font of a grid: the family, the characters shaped once each, and the glyph cache they are
//! drawn from.

use super::metrics::Metrics;
use mdshared_text::text::Brush;
use mdshared_text::{Bitmap, CacheStats, Subpixel, TextEngine, push_emoji_runs, shared_fonts};
use parley::{
    Alignment, AlignmentOptions, FontContext, FontFamily, FontStyle, FontWeight, Layout,
    LayoutContext, LineHeight, StyleProperty,
};
use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};

/// The fixed-width families the grid is drawn in unless told otherwise: the same list as
/// [`Face::Mono`](crate::gfx::Face::Mono), so a terminal looks like the toolkit's code text.
pub const MONO_FAMILY: &str = if cfg!(target_os = "macos") {
    "SF Mono, Menlo, Apple Color Emoji, monospace"
} else if cfg!(target_os = "windows") {
    "Cascadia Mono, Consolas, Courier New, Segoe UI Emoji, monospace"
} else {
    "DejaVu Sans Mono, Noto Sans Mono, Liberation Mono, Noto Color Emoji, monospace"
};

/// The most shaped characters kept; past it the shapes are dropped and made again as they are
/// drawn. A screen of text uses a few hundred.
pub const MAX_SHAPED: usize = 4096;

/// The glyph cache's cap in bytes: room for the masks of a few thousand glyphs at a large size.
const GLYPH_CACHE: usize = 4 << 20;

/// One character laid out by itself, with what placing it in a cell needs.
struct Shaped {
    layout: Layout<Brush>,
    /// Device px.
    advance: f32,
    /// Device px from the layout's top to its baseline.
    baseline: f32,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct ShapeKey {
    ch: char,
    bold: bool,
    italic: bool,
    /// The font size in device px, as bits.
    size: u32,
}

/// The multiply-rotate hash the glyph cache uses: the key is a few integers built here.
#[derive(Default)]
struct KeyHasher(u64);

impl Hasher for KeyHasher {
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.write_u64(u64::from(b));
        }
    }
    fn write_u8(&mut self, n: u8) {
        self.write_u64(u64::from(n));
    }
    fn write_u32(&mut self, n: u32) {
        self.write_u64(u64::from(n));
    }
    fn write_u64(&mut self, n: u64) {
        self.0 = (self.0.rotate_left(5) ^ n).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
    }
    fn finish(&self) -> u64 {
        self.0
    }
}

/// The font a grid is drawn in, and its caches. One per window (or per grid, for grids in
/// different fonts), kept by the application beside its [`Gfx`](crate::gfx::Gfx) and given to
/// every draw call.
///
/// Each character is laid out by parley once, by itself, in each of regular, bold, italic and
/// bold italic and at each size, and then drawn from that layout wherever it appears and in
/// whatever colour its cell has: the colour is given to the text engine at drawing time
/// ([`TextEngine::draw_layout_clipped_tinted`]), so neither the shapes nor the glyph masks
/// depend on it. Drawing a frame of a full screen is a hash lookup and a blit of cached masks
/// per character; nothing is shaped except a character, a style or a size never drawn before.
///
/// It is its own text engine, not the painter's: the painter's engine is private to it and its
/// layout cache is keyed by colour, which a screen of true colour text would flood.
pub struct CellFont {
    family: String,
    fonts: FontContext,
    layouts: LayoutContext<Brush>,
    engine: TextEngine,
    shaped: HashMap<ShapeKey, Shaped, BuildHasherDefault<KeyHasher>>,
    /// The most shapes kept: [`MAX_SHAPED`], less in a test.
    cap: usize,
}

impl Default for CellFont {
    fn default() -> CellFont {
        CellFont::new()
    }
}

impl CellFont {
    /// The grid's font in [`MONO_FAMILY`], with glyph edges in the system's subpixel order (as
    /// the painter's text).
    pub fn new() -> CellFont {
        CellFont::with_family(MONO_FAMILY)
    }

    /// The grid's font in `family`, a list of families as CSS writes it (the first installed one
    /// wins, a generic such as `monospace` last). A family that is not fixed-width still draws
    /// on the grid, one character per cell, but looks it. Empty means [`MONO_FAMILY`].
    pub fn with_family(family: &str) -> CellFont {
        let family = if family.trim().is_empty() {
            MONO_FAMILY
        } else {
            family
        };
        let mut engine = TextEngine::new(GLYPH_CACHE);
        engine.set_subpixel(crate::platform::subpixel_order());
        CellFont {
            family: family.to_string(),
            fonts: shared_fonts(),
            layouts: LayoutContext::new(),
            engine,
            shaped: HashMap::default(),
            cap: MAX_SHAPED,
        }
    }

    /// The family list the grid is drawn in.
    pub fn family(&self) -> &str {
        &self.family
    }

    /// Switches the antialiasing of glyphs drawn from now on (see
    /// [`TextEngine::set_subpixel`]); what is on screen is the caller's to repaint.
    pub fn set_subpixel(&mut self, subpixel: Subpixel) {
        self.engine.set_subpixel(subpixel);
    }

    /// The cells for text of `font_size` logical px on a display of `scale`, measured from the
    /// font: as wide as the advance of a digit zero, rounded to the device pixel, and as tall as
    /// the font's line (its ascent, descent and line gap), rounded up, with the baseline where
    /// the font puts it in that line. Cheap enough to call on every frame, but it lays out a
    /// character the first time each size is asked for; a size or scale that is not a finite
    /// positive number counts as 1.
    pub fn metrics(&mut self, font_size: f32, scale: f32) -> Metrics {
        let font_size = if font_size.is_finite() && font_size > 0.0 {
            font_size.min(512.0)
        } else {
            1.0
        };
        let scale = if scale.is_finite() && scale > 0.0 {
            scale.min(64.0)
        } else {
            1.0
        };
        let size = font_size * scale;
        let (advance, line_height, baseline) = {
            let shaped = self.shaped('0', false, false, size);
            let line = shaped.layout.lines().next();
            let line_height = line.map_or(size * 1.2, |l| l.metrics().line_height);
            (shaped.advance, line_height, shaped.baseline)
        };
        let w = (advance.round() as i32).max(1);
        let h = ((line_height - 0.01).ceil() as i32).max(1);
        let base = (baseline + (h as f32 - line_height) / 2.0).round() as i32;
        Metrics::from_device(font_size, scale, w, h, base)
    }

    /// How many characters are shaped and kept.
    pub fn shaped_len(&self) -> usize {
        self.shaped.len()
    }

    /// The glyph cache's counts: hits, misses, bytes held.
    pub fn glyph_stats(&self) -> CacheStats {
        self.engine.stats
    }

    /// The shape of `ch` at `size` device px, laid out now if it was not before.
    fn shaped(&mut self, ch: char, bold: bool, italic: bool, size: f32) -> &Shaped {
        let key = ShapeKey {
            ch,
            bold,
            italic,
            size: size.to_bits(),
        };
        if !self.shaped.contains_key(&key) {
            if self.shaped.len() >= self.cap {
                self.shaped.clear();
            }
            let shaped = self.shape(ch, bold, italic, size);
            self.shaped.insert(key, shaped);
        }
        &self.shaped[&key]
    }

    fn shape(&mut self, ch: char, bold: bool, italic: bool, size: f32) -> Shaped {
        let mut buf = [0u8; 4];
        let text: &str = ch.encode_utf8(&mut buf);
        let mut b = self
            .layouts
            .ranged_builder(&mut self.fonts, text, 1.0, true);
        b.push_default(StyleProperty::FontFamily(FontFamily::from(
            self.family.as_str(),
        )));
        b.push_default(StyleProperty::FontSize(size));
        b.push_default(StyleProperty::LineHeight(LineHeight::MetricsRelative(1.0)));
        b.push_default(StyleProperty::FontWeight(FontWeight::new(if bold {
            700.0
        } else {
            400.0
        })));
        if italic {
            b.push_default(StyleProperty::FontStyle(FontStyle::Italic));
        }
        b.push_default(StyleProperty::Brush([0, 0, 0, 255]));
        push_emoji_runs(&mut b, text, &self.family);
        let mut layout = b.build(text);
        layout.break_all_lines(None);
        layout.align(Alignment::Start, AlignmentOptions::default());
        let baseline = layout
            .lines()
            .next()
            .map_or(size * 0.8, |l| l.metrics().baseline);
        Shaped {
            advance: layout.full_width(),
            baseline,
            layout,
        }
    }

    /// Draws `ch` into `bmp`, touching only `clip` (device px), in a span of `span_w` device px
    /// from `x0` with its baseline at `baseline_y`: centred in the span by whole pixels (a glyph
    /// of the grid's own font fills its cell; one from a fallback font of another width is
    /// centred on it), in `rgb`.
    #[allow(clippy::too_many_arguments)] // drawing call: a place plus a style
    pub(super) fn draw(
        &mut self,
        bmp: &mut Bitmap,
        clip: (i32, i32, i32, i32),
        ch: char,
        (bold, italic): (bool, bool),
        metrics: &Metrics,
        (x0, span_w, baseline_y): (i32, i32, i32),
        rgb: [u8; 3],
        opaque_ink: bool,
    ) {
        let size = metrics.font_device();
        self.shaped(ch, bold, italic, size);
        let key = ShapeKey {
            ch,
            bold,
            italic,
            size: size.to_bits(),
        };
        let Some(shaped) = self.shaped.get(&key) else {
            return;
        };
        let ox = x0 + ((span_w as f32 - shaped.advance) / 2.0).round() as i32;
        let oy = baseline_y as f32 - shaped.baseline;
        if opaque_ink {
            self.engine.draw_layout_clipped_tinted_opaque_ink(
                bmp,
                &shaped.layout,
                ox as f32,
                oy,
                clip,
                rgb,
            );
        } else {
            self.engine
                .draw_layout_clipped_tinted(bmp, &shaped.layout, ox as f32, oy, clip, rgb);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cell_is_as_wide_as_a_digit_and_as_tall_as_a_line() {
        let mut font = CellFont::new();
        let m = font.metrics(14.0, 1.0);
        let (w, h) = m.cell_device();
        // Every fixed-width system font is a little over half as wide as it is high.
        assert!((6..=10).contains(&w), "{w}");
        assert!((14..=22).contains(&h), "{h}");
        assert!(m.baseline() > h / 2 && m.baseline() < h, "{}", m.baseline());
        // Twice the scale, about twice the cell.
        let (w2, h2) = font.metrics(14.0, 2.0).cell_device();
        assert!(
            (w2 - 2 * w).abs() <= 1 && (h2 - 2 * h).abs() <= 2,
            "{w2}x{h2} against {w}x{h}"
        );
        // Asking again lays nothing out.
        let n = font.shaped_len();
        assert_eq!(font.metrics(14.0, 1.0), m);
        assert_eq!(font.shaped_len(), n);
    }

    #[test]
    fn nonsense_sizes_and_an_empty_family_still_give_a_font() {
        let mut font = CellFont::with_family("   ");
        assert_eq!(font.family(), MONO_FAMILY);
        for v in [f32::NAN, f32::INFINITY, -1.0, 0.0] {
            let m = font.metrics(v, v);
            assert_eq!((m.font_size(), m.scale()), (1.0, 1.0));
        }
    }

    #[test]
    fn the_shapes_kept_are_bounded() {
        let mut font = CellFont::new();
        font.cap = 40;
        for code in 0x4E00u32..0x4E00 + 100 {
            font.shaped(char::from_u32(code).unwrap(), false, false, 14.0);
            assert!(font.shaped_len() <= 40);
        }
        assert!(font.shaped_len() > 0);
    }
}
