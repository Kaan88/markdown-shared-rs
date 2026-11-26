//! CPU text: parley shapes and lays out, swash rasterizes, this module blends. Ported from the
//! Phase 0 text spike (`spikes/text`), which the user accepted after the subpixel fix. The glyph
//! cache is byte-capped, and text uses subpixel masks in the order [`Subpixel`] names (RGB by
//! default), or greyscale coverage with `Subpixel::None`. Only the height of an outline is
//! grid-fitted; its horizontal geometry and parley's layout are left alone.

use crate::bitmap::Bitmap;
use parley::{FontData, Layout, PositionedLayoutItem};
use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};
use swash::scale::image::Content;
use swash::scale::{Render, ScaleContext, Source, StrikeWith};
use swash::zeno::{Format, Vector};
use swash::{CacheKey, FontRef};

mod vertical;
use vertical::{Vertical, can_hint_at_size};

pub type Brush = [u8; 4];

/// The fourth byte of a [`Brush`] is not alpha: it says where the run sits on the line. Anything
/// but these two is the baseline.
pub const RAISED: u8 = 254;
pub const LOWERED: u8 = 253;
/// `<mark>`: on the baseline, with a highlight behind it. A distinct brush also makes the marked
/// text a glyph run of its own, which is what the highlight is measured from.
pub const MARKED: u8 = 252;

/// How far a run with this brush is moved off the baseline, in the run's own font size: up 40%
/// for superscript, down 20% for subscript.
pub fn baseline_shift(brush: Brush, size: f32) -> f32 {
    match brush[3] {
        RAISED => (-0.4 * size).round(),
        LOWERED => (0.2 * size).round(),
        _ => 0.0,
    }
}

/// How glyph edges are antialiased: per colour channel for an LCD's subpixel stripes, or one
/// coverage value for all three.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum Subpixel {
    /// Red, green, blue stripes from left to right, the common LCD layout (and the default).
    #[default]
    Rgb,
    /// Blue, green, red stripes: the same masks mirrored.
    Bgr,
    /// Greyscale antialiasing, for screens without a known stripe order (OLED, rotated panels,
    /// remote sessions) or when the system turns ClearType off.
    None,
}

impl Subpixel {
    /// zeno shifts each channel's outline by these fractions of a pixel. Channel 0 is red: moving
    /// the outline right by 0.3 samples it 0.3 px to the left, where an RGB panel's red stripe is.
    /// `[0.3, 0.0, -0.3]` is deliberate, because zeno's own `Format::Subpixel` was measured to
    /// swap red and blue against Qt.
    fn format(self) -> Format {
        match self {
            Subpixel::Rgb => Format::CustomSubpixel([0.3, 0.0, -0.3]),
            Subpixel::Bgr => Format::CustomSubpixel([-0.3, 0.0, 0.3]),
            Subpixel::None => Format::Alpha,
        }
    }
}

/// Per-channel preblend tables, so glyph coverage becomes colour without a separate gamma pass.
struct Lut(Box<[[u8; 256]; 8]>);

impl Lut {
    /// The gamma and contrast the text spike settled on.
    fn new(gamma: f32, contrast: f32) -> Self {
        let mut table = Box::new([[0u8; 256]; 8]);
        for (bucket, tab) in table.iter_mut().enumerate() {
            let src = bucket as f32 / 7.0;
            let dst = 1.0 - src;
            let (ls, ld) = (src.powf(gamma), dst.powf(gamma));
            let adj = contrast * ld;
            for (i, v) in tab.iter_mut().enumerate() {
                let raw = i as f32 / 255.0;
                let a = raw + (1.0 - raw) * adj * raw;
                let r = if (dst - src).abs() < 1.0 / 256.0 {
                    a
                } else {
                    let out = (ls * a + ld * (1.0 - a)).powf(1.0 / gamma);
                    (out - dst) / (src - dst)
                };
                *v = (r.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
            }
        }
        Lut(table)
    }

    #[inline]
    fn channel(&self, c: u8) -> &[u8; 256] {
        &self.0[(c >> 5) as usize]
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct Key {
    font: u64,
    index: u32,
    glyph: u16,
    /// Exact device-size bits: a whole-pixel size and a nearby fraction can hint differently.
    size: u32,
    sub: u8,
    coords: u64,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum GlyphPixels {
    Mask,
    StraightRgba,
    PremultipliedRgba,
}

struct Entry {
    left: i32,
    top: i32,
    w: u32,
    h: u32,
    pixels: GlyphPixels,
    data: Box<[u8]>,
    /// The engine's tick when this glyph was last drawn, for least-recently-used eviction.
    last_used: u64,
}

/// Map slot and box overhead per cached glyph, so the cap bounds real memory.
const ENTRY_OVERHEAD: usize = 96;

/// Multiply-rotate hashing (the FxHash scheme) for the glyph key, which is a handful of integers
/// built by this module: SipHash's flood resistance buys nothing here and cost a visible share of
/// every cached glyph draw.
#[derive(Default)]
struct KeyHasher(u64);

impl Hasher for KeyHasher {
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.write_u64(b as u64);
        }
    }
    #[inline]
    fn write_u8(&mut self, n: u8) {
        self.write_u64(n as u64);
    }
    #[inline]
    fn write_u16(&mut self, n: u16) {
        self.write_u64(n as u64);
    }
    #[inline]
    fn write_u32(&mut self, n: u32) {
        self.write_u64(n as u64);
    }
    #[inline]
    fn write_u64(&mut self, n: u64) {
        self.0 = (self.0.rotate_left(5) ^ n).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
    }
    #[inline]
    fn write_usize(&mut self, n: usize) {
        self.write_u64(n as u64);
    }
    #[inline]
    fn finish(&self) -> u64 {
        self.0
    }
}

type GlyphMap = HashMap<Key, Entry, BuildHasherDefault<KeyHasher>>;

#[derive(Default, Clone, Copy, Debug)]
pub struct CacheStats {
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
    pub bytes: usize,
    pub peak_bytes: usize,
    pub entries: usize,
}

impl CacheStats {
    pub fn hit_rate(&self) -> f32 {
        let total = self.hits + self.misses;
        if total == 0 {
            0.0
        } else {
            self.hits as f32 / total as f32
        }
    }
}

pub struct TextEngine {
    scaler: ScaleContext,
    vertical: Vertical,
    fonts: HashMap<(u64, u32), (FontData, u32, CacheKey)>,
    map: GlyphMap,
    tick: u64,
    cap: usize,
    pub stats: CacheStats,
    lut: Lut,
    subpixel: Subpixel,
}

impl TextEngine {
    /// `cap` bounds the raster cache in bytes. The plan's hard cap is 4 MB. Text starts with
    /// [`Subpixel::Rgb`] masks.
    pub fn new(cap: usize) -> Self {
        TextEngine {
            scaler: ScaleContext::new(),
            vertical: Vertical::default(),
            fonts: HashMap::new(),
            map: GlyphMap::default(),
            tick: 0,
            cap,
            stats: CacheStats::default(),
            lut: Lut::new(1.4, 0.5),
            subpixel: Subpixel::default(),
        }
    }

    pub fn subpixel(&self) -> Subpixel {
        self.subpixel
    }

    /// Switches the antialiasing mode for glyphs drawn from now on. A change empties the glyph
    /// cache, since every cached mask was rasterized for the old mode; pixels already drawn are
    /// the caller's to repaint.
    pub fn set_subpixel(&mut self, subpixel: Subpixel) {
        if subpixel == self.subpixel {
            return;
        }
        self.subpixel = subpixel;
        self.map.clear();
        self.stats.bytes = 0;
        self.stats.entries = 0;
    }

    fn raster(
        &mut self,
        key: Key,
        font: &FontData,
        coords: impl Iterator<Item = i16> + Clone,
        size: f32,
    ) -> Entry {
        let cache_key = (font.data.id(), font.index);
        let (data, offset, font_key) = self.fonts.entry(cache_key).or_insert_with(|| {
            let r = FontRef::from_index(font.data.as_ref(), font.index as usize).expect("font");
            (font.clone(), r.offset, r.key)
        });
        let font_ref = FontRef {
            data: data.data.as_ref(),
            offset: *offset,
            key: *font_key,
        };
        let mut unhinted = self
            .scaler
            .builder(font_ref)
            .size(size)
            .hint(false)
            .normalized_coords(coords.clone())
            .build();
        let format = self.subpixel.format();
        let offset = Vector::new(key.sub as f32 / 4.0, 0.0);
        // Keep colour sources as they were. Only the monochrome fallback uses fitted outlines.
        let color = Render::new(&[
            Source::ColorOutline(0),
            Source::ColorBitmap(StrikeWith::BestFit),
        ])
        .format(format)
        .offset(offset)
        .render(&mut unhinted, key.glyph);
        let image = if color.is_some() {
            color
        } else if unhinted.scale_outline_into(key.glyph, &mut self.vertical.linear) {
            let fitted = if can_hint_at_size(font_ref, size)
                && self
                    .vertical
                    .draw_hinted(font_ref, font.index, key.glyph, size, coords)
            {
                self.vertical.fit(size, format, offset)
            } else {
                None
            };
            fitted.or_else(|| self.vertical.render(format, offset))
        } else {
            None
        };
        let tick = self.tick;
        let Some(image) = image else {
            return Entry {
                left: 0,
                top: 0,
                w: 0,
                h: 0,
                pixels: GlyphPixels::Mask,
                data: Box::new([]),
                last_used: tick,
            };
        };
        let (w, h) = (image.placement.width, image.placement.height);
        let (pixels, data) = match image.content {
            Content::Color => {
                // Swash layers colour outlines onto transparent pixels, but bitmap strikes
                // carry straight RGBA. Keep the distinction until the glyph is composited.
                let pixels = if matches!(image.source, Source::ColorOutline(_)) {
                    GlyphPixels::PremultipliedRgba
                } else {
                    GlyphPixels::StraightRgba
                };
                (pixels, image.data.into_boxed_slice())
            }
            Content::SubpixelMask => {
                let mut data = Vec::with_capacity((w * h * 3) as usize);
                for pixel in image.data.as_chunks::<4>().0 {
                    data.extend_from_slice(&pixel[..3]);
                }
                (GlyphPixels::Mask, data.into_boxed_slice())
            }
            Content::Mask => {
                let mut data = Vec::with_capacity((w * h * 3) as usize);
                for &a in &image.data {
                    data.extend_from_slice(&[a, a, a]);
                }
                (GlyphPixels::Mask, data.into_boxed_slice())
            }
        };
        Entry {
            left: image.placement.left,
            top: image.placement.top,
            w,
            h,
            pixels,
            data,
            last_used: tick,
        }
    }

    /// Called when the cache is over `cap`: drops the least recently drawn glyphs until it is at
    /// three quarters of the cap. Eviction is rare (a warm document fits in a fraction of the cap),
    /// so this sorts the entries by last use instead of keeping an LRU order on every draw, and the
    /// quarter of headroom keeps a cache at its cap from paying the sort on every miss.
    fn evict(&mut self) {
        if self.stats.bytes <= self.cap {
            return;
        }
        let target = self.cap - self.cap / 4;
        let mut order: Vec<(u64, Key)> = self.map.iter().map(|(k, e)| (e.last_used, *k)).collect();
        order.sort_unstable_by_key(|&(tick, _)| tick);
        for (_, key) in order {
            if self.stats.bytes <= target {
                break;
            }
            if let Some(entry) = self.map.remove(&key) {
                self.stats.bytes -= entry.data.len() + ENTRY_OVERHEAD;
                self.stats.evictions += 1;
                self.stats.entries -= 1;
            }
        }
    }

    /// Draws a laid-out block at `(ox, oy)`, in device px. Only glyphs are drawn; the caller draws
    /// chips, underlines and rules.
    pub fn draw_layout(&mut self, fb: &mut Bitmap, layout: &Layout<Brush>, ox: f32, oy: f32) {
        let clip = (0, 0, fb.width as i32, fb.height as i32);
        self.draw_layout_clipped(fb, layout, ox, oy, clip);
    }

    /// [`draw_layout`](Self::draw_layout) that touches only the pixels inside `clip`, given in
    /// device px as `(x0, y0, x1, y1)` (end-exclusive, intersected with the bitmap). The pixels it
    /// writes are exactly the ones an unclipped draw writes inside `clip`.
    pub fn draw_layout_clipped(
        &mut self,
        fb: &mut Bitmap,
        layout: &Layout<Brush>,
        ox: f32,
        oy: f32,
        clip: (i32, i32, i32, i32),
    ) {
        self.draw_runs(fb, layout, (ox, oy), clip, None, false);
    }

    /// [`draw_layout_clipped`](Self::draw_layout_clipped) with every glyph in the colour `rgb`
    /// instead of its run's own brush (the brush's fourth byte, the baseline shift, is kept).
    /// The pixels are exactly those of a layout built with `rgb` as its brush, so one shaped
    /// layout can be drawn in any colour: a terminal's grid shapes each character once and
    /// draws it in whatever colour each cell asks for. The glyph cache does not depend on the
    /// colour, so a colour never drawn before costs no rasterizing.
    pub fn draw_layout_clipped_tinted(
        &mut self,
        fb: &mut Bitmap,
        layout: &Layout<Brush>,
        ox: f32,
        oy: f32,
        clip: (i32, i32, i32, i32),
        rgb: [u8; 3],
    ) {
        self.draw_runs(fb, layout, (ox, oy), clip, Some(rgb), false);
    }

    /// Draws tinted glyphs as [`Self::draw_layout_clipped_tinted`] does on opaque pixels.
    /// On a premultiplied translucent destination, blends ink with a single coverage for
    /// all three channels so the edge composites correctly over an unknown backdrop. Colour
    /// glyphs use their own colours and alpha. Pixels with zero coverage are left alone.
    ///
    /// This is plain premultiplied blending: alpha can become 254. If using `mdshared-ui`'s
    /// frosted chrome key, keep marked chrome out of the destination and finish the drawn
    /// region with `mdshared_ui::frost::finish_premultiplied` before presenting it.
    #[allow(clippy::too_many_arguments)] // same coordinates and tint as the existing draw
    pub fn draw_layout_clipped_tinted_premultiplied(
        &mut self,
        fb: &mut Bitmap,
        layout: &Layout<Brush>,
        ox: f32,
        oy: f32,
        clip: (i32, i32, i32, i32),
        rgb: [u8; 3],
    ) {
        self.draw_runs(fb, layout, (ox, oy), clip, Some(rgb), true);
    }

    /// The drawing of both: each run's brush, or `tint` in its place.
    fn draw_runs(
        &mut self,
        fb: &mut Bitmap,
        layout: &Layout<Brush>,
        (ox, oy): (f32, f32),
        clip: (i32, i32, i32, i32),
        tint: Option<[u8; 3]>,
        premultiplied: bool,
    ) {
        let clip = (
            clip.0.max(0),
            clip.1.max(0),
            clip.2.min(fb.width as i32),
            clip.3.min(fb.height as i32),
        );
        if clip.0 >= clip.2 || clip.1 >= clip.3 {
            return;
        }
        for line in layout.lines() {
            let metrics = line.metrics();
            if oy + metrics.baseline + metrics.line_height < clip.1 as f32
                || oy + metrics.baseline - metrics.line_height > clip.3 as f32
            {
                continue;
            }
            for item in line.items() {
                let PositionedLayoutItem::GlyphRun(glyph_run) = item else {
                    continue;
                };
                let run = glyph_run.run();
                // Borrowed, not cloned: the font is only needed (and cloned once into `fonts`)
                // when a glyph misses the cache.
                let font = run.font();
                let size = run.font_size();
                let coords = run.normalized_coords();
                let coord_hash = coords.iter().fold(0u64, |h, c| {
                    (h ^ (c.to_bits() as u16 as u64)).wrapping_mul(0x0000_0100_0000_01b3)
                });
                let own = glyph_run.style().brush;
                let brush = tint.map_or(own, |c| [c[0], c[1], c[2], own[3]]);
                let shift = baseline_shift(brush, size);
                for glyph in glyph_run.positioned_glyphs() {
                    let (x, y) = (ox + glyph.x, (oy + glyph.y).round() + shift);
                    let floor_x = x.floor();
                    let sub = (((x - floor_x) * 4.0) as u8).min(3);
                    let key = Key {
                        font: font.data.id(),
                        index: font.index,
                        glyph: glyph.id as u16,
                        size: size.to_bits(),
                        sub,
                        coords: coord_hash,
                    };
                    self.tick += 1;
                    let tick = self.tick;
                    if let Some(entry) = self.map.get_mut(&key) {
                        entry.last_used = tick;
                        self.stats.hits += 1;
                        blit(
                            fb,
                            clip,
                            entry,
                            (floor_x as i32, y as i32),
                            brush,
                            &self.lut,
                            premultiplied,
                        );
                        continue;
                    }
                    self.stats.misses += 1;
                    let entry = self.raster(key, font, coords.iter().map(|c| c.to_bits()), size);
                    self.stats.bytes += entry.data.len() + ENTRY_OVERHEAD;
                    self.stats.entries += 1;
                    self.stats.peak_bytes = self.stats.peak_bytes.max(self.stats.bytes);
                    self.map.insert(key, entry);
                    self.evict();
                    let Some(entry) = self.map.get(&key) else {
                        continue;
                    };
                    blit(
                        fb,
                        clip,
                        entry,
                        (floor_x as i32, y as i32),
                        brush,
                        &self.lut,
                        premultiplied,
                    );
                }
            }
        }
    }
}

#[inline]
fn blit(
    fb: &mut Bitmap,
    clip: (i32, i32, i32, i32),
    entry: &Entry,
    (gx, gy): (i32, i32),
    brush: Brush,
    lut: &Lut,
    premultiplied: bool,
) {
    if entry.w == 0 || entry.h == 0 {
        return;
    }
    let (x0, y0) = (gx + entry.left, gy - entry.top);
    let (w, h) = (entry.w as i32, entry.h as i32);
    let (lr, lg, lb) = (
        lut.channel(brush[0]),
        lut.channel(brush[1]),
        lut.channel(brush[2]),
    );
    let bpp = if entry.pixels == GlyphPixels::Mask {
        3
    } else {
        4
    };
    let cs = x0.max(clip.0);
    let ce = (x0 + w).min(clip.2);
    for row in y0.max(clip.1)..(y0 + h).min(clip.3) {
        if ce <= cs {
            continue;
        }
        let so = ((row - y0) * w + (cs - x0)) as usize * bpp;
        let d0 = (row as usize * fb.width as usize + cs as usize) * 4;
        let n = (ce - cs) as usize;
        let src = &entry.data[so..so + n * bpp];
        let dst = &mut fb.pixels[d0..d0 + n * 4];
        if entry.pixels != GlyphPixels::Mask {
            for (s, d) in src
                .as_chunks::<4>()
                .0
                .iter()
                .zip(dst.as_chunks_mut::<4>().0.iter_mut())
            {
                let a = s[3] as u32;
                if a == 0 {
                    continue;
                }
                if premultiplied && d[3] < 255 {
                    if entry.pixels == GlyphPixels::PremultipliedRgba {
                        for (dst, src) in d.iter_mut().zip([s[2], s[1], s[0], s[3]]) {
                            let keep = (u32::from(*dst) * (255 - a) + 127) / 255;
                            *dst = src.saturating_add(keep as u8);
                        }
                    } else {
                        crate::bitmap::blend_premultiplied(d, [s[0], s[1], s[2]], s[3]);
                    }
                    continue;
                }
                let ia = 255 - a;
                // Keep the opaque drawing's historical interpretation of both colour sources.
                d[0] = ((s[2] as u32 * a + d[0] as u32 * ia + 127) / 255) as u8;
                d[1] = ((s[1] as u32 * a + d[1] as u32 * ia + 127) / 255) as u8;
                d[2] = ((s[0] as u32 * a + d[2] as u32 * ia + 127) / 255) as u8;
            }
        } else {
            for (s, d) in src
                .as_chunks::<3>()
                .0
                .iter()
                .zip(dst.as_chunks_mut::<4>().0.iter_mut())
            {
                if s[0] | s[1] | s[2] == 0 {
                    continue;
                }
                let (ar, ag, ab) = (
                    lr[s[0] as usize] as u32,
                    lg[s[1] as usize] as u32,
                    lb[s[2] as usize] as u32,
                );
                if premultiplied && d[3] < 255 {
                    // Collapse the effective subpixel mask; LCD fringes assume a known ground.
                    let a = ((ar + ag + ab + 1) / 3) as u8;
                    crate::bitmap::blend_premultiplied(d, [brush[0], brush[1], brush[2]], a);
                    continue;
                }
                d[0] = ((brush[2] as u32 * ab + d[0] as u32 * (255 - ab) + 127) / 255) as u8;
                d[1] = ((brush[1] as u32 * ag + d[1] as u32 * (255 - ag) + 127) / 255) as u8;
                d[2] = ((brush[0] as u32 * ar + d[2] as u32 * (255 - ar) + 127) / 255) as u8;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use parley::{Alignment, AlignmentOptions, FontFamily, LayoutContext, StyleProperty};

    /// A wrapped paragraph in the UI's sans face, `size` device px, `width` device px wide.
    pub(super) fn paragraph(text: &str, size: f32, width: f32, brush: Brush) -> Layout<Brush> {
        let mut fonts = crate::shared_fonts();
        let mut layouts = LayoutContext::<Brush>::new();
        let mut b = layouts.ranged_builder(&mut fonts, text, 1.0, true);
        b.push_default(StyleProperty::FontFamily(FontFamily::from(
            "Segoe UI, Noto Sans, DejaVu Sans, sans-serif",
        )));
        b.push_default(StyleProperty::FontSize(size));
        b.push_default(StyleProperty::Brush(brush));
        let mut layout = b.build(text);
        layout.break_all_lines(Some(width));
        layout.align(Alignment::Start, AlignmentOptions::default());
        layout
    }

    const SAMPLE: &str = "The quick brown fox jumps over the lazy dog. Pack my box with five dozen \
        liquor jugs! 0123456789 (sphinx of black quartz, judge my vow) — Ünïcödé façade, 日本語. ";

    #[test]
    #[cfg(windows)]
    fn a_swash_colour_outline_keeps_its_premultiplied_edge_on_translucent_pixels() {
        let path = std::path::Path::new(&std::env::var_os("WINDIR").unwrap_or_default())
            .join("Fonts/seguiemj.ttf");
        let Ok(bytes) = std::fs::read(path) else {
            eprintln!("skipping colour outline test: Segoe UI Emoji is absent");
            return;
        };
        let font = FontData::new(bytes.into(), 0);
        let glyph = FontRef::from_index(font.data.as_ref(), 0)
            .unwrap()
            .charmap()
            .map('😀');
        let mut engine = TextEngine::new(1 << 20);
        let size = 32.0_f32;
        let entry = engine.raster(
            Key {
                font: font.data.id(),
                index: 0,
                glyph,
                size: size.to_bits(),
                sub: 0,
                coords: 0,
            },
            &font,
            std::iter::empty(),
            size,
        );
        assert_eq!(entry.pixels, GlyphPixels::PremultipliedRgba);
        let mut bmp = Bitmap::new(entry.w, entry.h);
        let clip = (0, 0, entry.w as i32, entry.h as i32);
        let at = (-entry.left, entry.top);
        bmp.pixels.fill(0);
        blit(&mut bmp, clip, &entry, at, [0; 4], &engine.lut, true);
        let mut edges = 0;
        for (rgba, bgra) in entry
            .data
            .as_chunks::<4>()
            .0
            .iter()
            .zip(bmp.pixels.as_chunks::<4>().0)
        {
            let [r, g, b, a] = *rgba;
            if (1..255).contains(&a) && r.max(g).max(b) > 0 {
                assert_eq!(*bgra, [b, g, r, a]);
                edges += 1;
            }
        }
        assert!(
            edges > 0,
            "the colour outline must exercise partial coverage"
        );
    }

    #[test]
    fn partial_glyph_coverage_composites_over_black_and_white_without_a_halo() {
        let lut = Lut(Box::new(std::array::from_fn(|_| {
            std::array::from_fn(|i| if i == 1 { 0 } else { i as u8 })
        })));
        let ink = [220, 200, 180];
        for (pixels, data, a) in [
            (GlyphPixels::Mask, vec![1, 1, 1, 32, 64, 96], 64),
            (
                GlyphPixels::StraightRgba,
                vec![0, 0, 0, 0, 220, 200, 180, 64],
                64,
            ),
        ] {
            let entry = Entry {
                left: 0,
                top: 0,
                w: 2,
                h: 1,
                pixels,
                data: data.into_boxed_slice(),
                last_used: 0,
            };
            let mut bmp = Bitmap::new(2, 1);
            bmp.pixels
                .copy_from_slice(&[21, 21, 21, 179, 21, 21, 21, 179]);
            blit(
                &mut bmp,
                (0, 0, 2, 1),
                &entry,
                (0, 0),
                [ink[0], ink[1], ink[2], 255],
                &lut,
                true,
            );
            assert_eq!(&bmp.pixels[..4], &[21, 21, 21, 179]);
            assert_eq!(
                bmp.pixels[7],
                179 + (76.0 * f64::from(a) / 255.0).round() as u8
            );
            for (bgr, fg) in ink.into_iter().rev().enumerate() {
                for desktop in [0.0, 255.0] {
                    let actual = f64::from(bmp.pixels[4 + bgr])
                        + f64::from(255 - bmp.pixels[7]) * desktop / 255.0;
                    let back = 21.0 + 76.0 * desktop / 255.0;
                    let expected = back + (f64::from(fg) - back) * f64::from(a) / 255.0;
                    assert!(
                        (actual - expected).abs() <= 1.0,
                        "{pixels:?}: {actual} vs {expected}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_clipped_draw_is_the_unclipped_draw_cropped() {
        let layout = paragraph(&SAMPLE.repeat(3), 17.0, 300.0, [20, 90, 200, 255]);
        let (w, h) = (360u32, layout.height().ceil() as u32 + 30);
        let background = |bm: &mut Bitmap| {
            // A non-uniform background, so a write outside the clip cannot hide.
            for (i, p) in bm.pixels.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                p.copy_from_slice(&[(i * 7) as u8, (i / 3) as u8, 200, 255]);
            }
        };
        let mut engine = TextEngine::new(4 << 20);
        let mut full = Bitmap::new(w, h);
        background(&mut full);
        engine.draw_layout(&mut full, &layout, 7.6, 11.0);
        let clips = [
            (0, 0, w as i32, h as i32),
            (13, 9, 141, 47),
            (-50, -50, 90, 4000),
            (200, 30, 9999, 60),
            (50, 50, 50, 80),
            (100, 100, 40, 40),
        ];
        for clip in clips {
            let mut clipped = Bitmap::new(w, h);
            background(&mut clipped);
            let before = clipped.pixels.clone();
            engine.draw_layout_clipped(&mut clipped, &layout, 7.6, 11.0, clip);
            for y in 0..h as i32 {
                for x in 0..w as i32 {
                    let o = (y as usize * w as usize + x as usize) * 4;
                    let inside = x >= clip.0 && x < clip.2 && y >= clip.1 && y < clip.3;
                    let want = if inside { &full.pixels } else { &before };
                    assert_eq!(
                        clipped.pixels[o..o + 4],
                        want[o..o + 4],
                        "clip {clip:?} at {x},{y}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_tinted_draw_is_the_draw_of_a_layout_built_in_that_colour() {
        let red = paragraph(SAMPLE, 15.0, 300.0, [200, 30, 30, 255]);
        let blue = paragraph(SAMPLE, 15.0, 300.0, [20, 60, 220, 255]);
        let (w, h) = (320u32, red.height().ceil() as u32 + 10);
        let mut engine = TextEngine::new(4 << 20);
        let clip = (0, 0, w as i32, h as i32);
        let mut want = Bitmap::new(w, h);
        want.clear([250, 245, 230]);
        engine.draw_layout_clipped(&mut want, &blue, 3.25, 2.0, clip);
        let mut got = Bitmap::new(w, h);
        got.clear([250, 245, 230]);
        engine.draw_layout_clipped_tinted(&mut got, &red, 3.25, 2.0, clip, [20, 60, 220]);
        assert!(got.pixels == want.pixels);
        // The masks were shared: the tinted draw rasterized nothing new.
        let misses = engine.stats.misses;
        engine.draw_layout_clipped_tinted(&mut got, &red, 3.25, 2.0, clip, [0, 160, 0]);
        assert_eq!(engine.stats.misses, misses);
    }

    #[test]
    fn a_small_cache_stays_under_its_cap_and_draws_the_same() {
        let layouts: Vec<_> = [14.0, 18.0, 23.0, 31.0]
            .iter()
            .map(|&size| paragraph(&SAMPLE.repeat(2), size, 600.0, [0, 0, 0, 255]))
            .collect();
        let h = layouts
            .iter()
            .map(|l| l.height().ceil() as u32)
            .sum::<u32>()
            + 40;
        let draw_all = |engine: &mut TextEngine, check_cap: bool| {
            let mut fb = Bitmap::new(640, h);
            fb.clear([255, 255, 255]);
            let mut y = 5.0;
            for layout in &layouts {
                engine.draw_layout(&mut fb, layout, 3.3, y);
                y += layout.height().ceil() + 8.0;
                if check_cap {
                    assert!(engine.stats.bytes <= engine.cap, "{:?}", engine.stats);
                }
                assert_eq!(engine.stats.entries, engine.map.len());
            }
            fb
        };
        let mut roomy = TextEngine::new(16 << 20);
        let expected = draw_all(&mut roomy, true);
        assert_eq!(roomy.stats.evictions, 0);
        let total = roomy.stats.bytes;

        let cap = total * 2 / 3;
        let mut tight = TextEngine::new(cap);
        for _ in 0..3 {
            let got = draw_all(&mut tight, true);
            assert!(got.pixels == expected.pixels, "eviction changed the output");
        }
        assert!(tight.stats.evictions > 0);
        assert!(
            tight.stats.peak_bytes <= cap + 64 * 1024,
            "{:?}",
            tight.stats
        );

        // The glyphs drawn last are the ones kept: redrawing only the last layout hits.
        let mut fb = Bitmap::new(640, h);
        let misses = tight.stats.misses;
        tight.draw_layout(&mut fb, layouts.last().unwrap(), 3.3, 5.0);
        assert_eq!(tight.stats.misses, misses, "{:?}", tight.stats);
    }

    #[test]
    fn bgr_mirrors_the_rgb_masks_and_none_is_grey() {
        let layout = paragraph(SAMPLE, 15.0, 500.0, [40, 40, 40, 255]);
        let draw = |mode: Subpixel| {
            let mut engine = TextEngine::new(4 << 20);
            engine.set_subpixel(mode);
            assert_eq!(engine.subpixel(), mode);
            let mut fb = Bitmap::new(520, layout.height().ceil() as u32 + 10);
            fb.clear([255, 255, 255]);
            engine.draw_layout(&mut fb, &layout, 4.4, 3.0);
            fb
        };
        let (rgb, bgr, grey) = (
            draw(Subpixel::Rgb),
            draw(Subpixel::Bgr),
            draw(Subpixel::None),
        );
        let mut coloured = 0;
        for ((a, b), g) in rgb
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .zip(bgr.pixels.as_chunks::<4>().0)
            .zip(grey.pixels.as_chunks::<4>().0)
        {
            // A grey brush on white: BGR's blue is RGB's red and the other way round.
            assert_eq!([a[2], a[1], a[0]], [b[0], b[1], b[2]]);
            assert!(g[0] == g[1] && g[1] == g[2], "greyscale pixel {g:?}");
            coloured += (a[0] != a[2]) as usize;
        }
        assert!(coloured > 100, "the RGB masks should have colour fringes");
        assert_ne!(rgb.pixels, grey.pixels);

        // Switching modes drops the masks cached for the old one.
        let mut engine = TextEngine::new(4 << 20);
        let mut fb = Bitmap::new(520, rgb.height);
        fb.clear([255, 255, 255]);
        engine.draw_layout(&mut fb, &layout, 4.4, 3.0);
        assert!(engine.stats.entries > 0);
        engine.set_subpixel(Subpixel::Bgr);
        assert_eq!((engine.stats.entries, engine.stats.bytes), (0, 0));
        fb.clear([255, 255, 255]);
        engine.draw_layout(&mut fb, &layout, 4.4, 3.0);
        assert!(fb.pixels == bgr.pixels);
    }

    /// Not a correctness test: `cargo test --release -p mdshared-text text_draw_speed -- --ignored
    /// --nocapture` prints the time per glyph of a warm-cache draw.
    #[test]
    #[ignore]
    fn text_draw_speed() {
        let text = SAMPLE.repeat(40);
        let layout = paragraph(&text, 16.0, 1100.0, [30, 30, 30, 255]);
        let mut fb = Bitmap::new(1200, layout.height().ceil() as u32 + 20);
        let mut engine = TextEngine::new(4 << 20);
        engine.draw_layout(&mut fb, &layout, 10.3, 5.0);
        let glyphs: usize = layout
            .lines()
            .flat_map(|l| l.items())
            .map(|i| match i {
                PositionedLayoutItem::GlyphRun(r) => r.positioned_glyphs().count(),
                _ => 0,
            })
            .sum();
        for _ in 0..3 {
            let n = 300;
            let t = std::time::Instant::now();
            for _ in 0..n {
                fb.clear([255, 255, 255]);
                engine.draw_layout(&mut fb, &layout, 10.3, 5.0);
            }
            let total = t.elapsed().as_secs_f64();
            let t = std::time::Instant::now();
            for _ in 0..n {
                fb.clear([255, 255, 255]);
            }
            let clear = t.elapsed().as_secs_f64();
            println!(
                "{glyphs} glyphs: {:.1} ns/glyph (draw only, clear subtracted)",
                (total - clear) / (n * glyphs) as f64 * 1e9
            );
        }
    }
}
