//! Draw the fitting candidate with skrifa's pedantic setting, which swash does not expose.
//! This only supplies a path; the caller owns the unhinted outline and the raster guards.

use std::collections::VecDeque;

use skrifa::instance::{NormalizedCoord, Size};
use skrifa::outline::{
    DrawSettings, HintingInstance, OutlineGlyphCollection, OutlineGlyphFormat, OutlinePen,
    SmoothMode, Target,
};
use skrifa::{GlyphId, MetadataProvider};
use swash::zeno::{Point, Verb};
use swash::{CacheKey, FontRef};

const TARGET: Target = Target::Smooth {
    mode: SmoothMode::Lcd,
    symmetric_rendering: true,
    preserve_linear_metrics: true,
};
const CACHE_PER_FORMAT: usize = 8;

struct Cached {
    face: CacheKey,
    coords: Vec<NormalizedCoord>,
    instance: HintingInstance,
}

#[derive(Default)]
pub(super) struct Hinter {
    // As in swash, keep separate groups so reconfiguration can reuse the interpreter's
    // allocations. The front is the least recently used; CFF and CFF2 share a group.
    glyf: VecDeque<Cached>,
    cff: VecDeque<Cached>,
    other: VecDeque<Cached>,
    coords: Vec<NormalizedCoord>,
    pub(super) path: Path,
}

impl Hinter {
    pub(super) fn draw(
        &mut self,
        font: FontRef<'_>,
        index: u32,
        glyph: u16,
        size: f32,
        coords: impl Iterator<Item = i16>,
    ) -> Option<()> {
        self.path.points.clear();
        self.path.verbs.clear();
        let outlines = skrifa::FontRef::from_index(font.data, index)
            .ok()?
            .outline_glyphs();
        let group = match outlines.format()? {
            OutlineGlyphFormat::Glyf => &mut self.glyf,
            OutlineGlyphFormat::Cff | OutlineGlyphFormat::Cff2 => &mut self.cff,
            _ => &mut self.other,
        };
        let glyph = outlines.get(GlyphId::from(glyph))?;
        let size = Size::new(size);
        self.coords.clear();
        self.coords.extend(coords.map(NormalizedCoord::from_bits));
        let instance = instance(group, font.key, &outlines, size, &self.coords)?;
        // Unlike the default DrawSettings conversion, this rejects a partly fitted path
        // when the glyph program reports an error after it has already moved points.
        glyph
            .draw(DrawSettings::hinted(instance, true), &mut self.path)
            .ok()?;
        self.path.maybe_close();
        Some(())
    }
}

fn instance<'a>(
    group: &'a mut VecDeque<Cached>,
    face: CacheKey,
    outlines: &OutlineGlyphCollection<'_>,
    size: Size,
    coords: &[NormalizedCoord],
) -> Option<&'a HintingInstance> {
    let cached = if let Some(index) = group.iter().position(|entry| {
        entry.face == face && entry.instance.size() == size && entry.coords == coords
    }) {
        group.remove(index)?
    } else if group.len() == CACHE_PER_FORMAT {
        let mut entry = group.pop_front()?;
        entry
            .instance
            .reconfigure(outlines, size, coords, TARGET)
            .ok()?;
        entry.face = face;
        entry.coords.clear();
        entry.coords.extend_from_slice(coords);
        entry
    } else {
        Cached {
            face,
            coords: coords.to_vec(),
            instance: HintingInstance::new(outlines, size, coords, TARGET).ok()?,
        }
    };
    group.push_back(cached);
    group.back().map(|entry| &entry.instance)
}

/// Collect the same commands as swash's outline pen, including closing each contour.
#[derive(Default)]
pub(super) struct Path {
    pub(super) points: Vec<Point>,
    pub(super) verbs: Vec<Verb>,
}

impl Path {
    fn maybe_close(&mut self) {
        if !self.verbs.is_empty() && self.verbs.last() != Some(&Verb::Close) {
            self.close();
        }
    }
}

impl OutlinePen for Path {
    fn move_to(&mut self, x: f32, y: f32) {
        self.maybe_close();
        self.points.push(Point::new(x, y));
        self.verbs.push(Verb::MoveTo);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.points.push(Point::new(x, y));
        self.verbs.push(Verb::LineTo);
    }

    fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        self.points.extend([Point::new(cx, cy), Point::new(x, y)]);
        self.verbs.push(Verb::QuadTo);
    }

    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        self.points
            .extend([Point::new(cx0, cy0), Point::new(cx1, cy1), Point::new(x, y)]);
        self.verbs.push(Verb::CurveTo);
    }

    fn close(&mut self) {
        self.verbs.push(Verb::Close);
    }
}
