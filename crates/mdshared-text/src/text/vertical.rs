//! Fit monochrome glyphs to pixel rows without changing their horizontal outline.
//! The text engine supplies both outlines on a cache miss; there is no shaping or blending here.

use swash::scale::Source;
use swash::scale::image::{Content, Image};
use swash::scale::outline::Outline;
use swash::zeno::{Format, Mask, Origin, Scratch, Vector};
use swash::{FontRef, tag_from_bytes};

#[cfg(test)]
#[path = "vertical_tests.rs"]
mod tests;

// These are device pixels. The coordinate cap also bounds distances between path points
// before zeno converts them to fixed point and computes edge products in i32.
const MAX_COORD: f32 = 4096.0;
const MAX_SIDE: u32 = 8192;
const MAX_MASK_BYTES: u32 = 4 * 1024 * 1024;
const FIT_LIMIT_EM: f32 = 4.0;

/// The TrueType hinter rounds fractional ppem to a whole pixel when `head.flags` bit 3 is set.
/// In that case keep the unhinted outline for every glyph at the requested fractional size.
/// A missing or truncated `head` cannot establish that fractional hinting is safe.
pub(super) fn can_hint_at_size(font: FontRef<'_>, size: f32) -> bool {
    size.is_finite()
        && size > 0.0
        && (size.fract() == 0.0
            || font
                .table(tag_from_bytes(b"head"))
                .and_then(|head| head.get(16..18))
                .is_some_and(|flags| u16::from_be_bytes([flags[0], flags[1]]) & (1 << 3) == 0))
}

#[derive(Default)]
pub(super) struct Vertical {
    pub(super) linear: Outline,
    pub(super) hinted: Outline,
    scratch: Scratch,
}

impl Vertical {
    /// Try the hinted Y coordinates with the exact unhinted X coordinates. Matching commands and
    /// nearby X coordinates are a geometric check, not proof of point identity. Keep the linear
    /// path untouched until the candidate passes the movement, bounds and raster-size checks.
    pub(super) fn fit(&mut self, size: f32, format: Format, offset: Vector) -> Option<Image> {
        let linear = self.linear.points();
        let hinted = self.hinted.points();
        let limit = size * FIT_LIMIT_EM;
        if self.linear.verbs() != self.hinted.verbs()
            || linear.len() != hinted.len()
            || !linear.iter().zip(hinted).all(|(a, b)| {
                (a.x - b.x).abs() <= 1.0 / 16.0
                    && (a.y - b.y).abs() <= limit
                    && a.x.abs() <= limit
                    && b.y.abs() <= limit
            })
        {
            return None;
        }
        // The accepted control points must all lie within four ems of the origin on each
        // axis. The curves stay within that box too. Stage the combined path in `hinted` so
        // even an in-budget fit that fails the pixel mask check cannot alter `linear`.
        for (hinted, linear) in self.hinted.points_mut().iter_mut().zip(linear) {
            hinted.x = linear.x;
        }
        let image = render_outline(&self.hinted, &mut self.scratch, format, offset)?;
        std::mem::swap(&mut self.linear, &mut self.hinted);
        Some(image)
    }

    /// Rasterize the unhinted fallback under the same limits as the fitted path.
    pub(super) fn render(&mut self, format: Format, offset: Vector) -> Option<Image> {
        render_outline(&self.linear, &mut self.scratch, format, offset)
    }
}

/// Use zeno's exact pixel dimensions, including the fractional X offset. Check the byte
/// product before allocating, rather than calling zeno's unchecked `Format::buffer_size`.
fn mask_bytes(format: Format, width: u32, height: u32) -> Option<usize> {
    let channels = if format == Format::Alpha { 1 } else { 4 };
    let bytes = width.checked_mul(height)?.checked_mul(channels)?;
    (width <= MAX_SIDE && height <= MAX_SIDE && (1..=MAX_MASK_BYTES).contains(&bytes))
        .then_some(bytes as usize)
}

/// Rasterize with the same origin and offsets as swash's monochrome renderer, but reject
/// non-finite or excessive coordinates before zeno sees them. The colour path is separate.
fn render_outline(
    outline: &Outline,
    scratch: &mut Scratch,
    format: Format,
    offset: Vector,
) -> Option<Image> {
    if !outline
        .points()
        .iter()
        .all(|p| p.x.abs() <= MAX_COORD && p.y.abs() <= MAX_COORD)
    {
        return None;
    }
    let mut bytes = None;
    let mut mask = Mask::with_scratch(outline.path(), scratch);
    mask.format(format)
        .origin(Origin::BottomLeft)
        .offset(offset)
        .render_offset(offset)
        .inspect(|fmt, w, h| bytes = mask_bytes(fmt, w, h));
    let bytes = bytes?;
    let mut data = Vec::new();
    data.try_reserve_exact(bytes).ok()?;
    data.resize(bytes, 0);
    let placement = mask.render_into(&mut data, None);
    Some(Image {
        source: Source::Outline,
        content: if format == Format::Alpha {
            Content::Mask
        } else {
            Content::SubpixelMask
        },
        placement,
        data,
    })
}
