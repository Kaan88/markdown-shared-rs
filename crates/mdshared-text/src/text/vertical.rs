//! Fit monochrome glyphs to pixel rows without changing their horizontal outline.
//! The text engine supplies both outlines on a cache miss; there is no shaping or blending here.

use swash::scale::Source;
use swash::scale::image::{Content, Image};
use swash::scale::outline::Outline;
use swash::zeno::{Format, Mask, Origin, Scratch, Vector};
use swash::{FontRef, tag_from_bytes};

#[cfg(all(test, windows))]
#[path = "vertical_tests.rs"]
mod tests;

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
    /// Copy only Y. Swash asks its hinter to suppress horizontal adjustment, but the two draws
    /// can still use different precision, and a font's instructions can change the path. Use the
    /// original path if the commands differ or any point moves too far in X to pair confidently.
    pub(super) fn fit(&mut self) {
        let linear = self.linear.points();
        let hinted = self.hinted.points();
        if self.linear.verbs() != self.hinted.verbs()
            || linear.len() != hinted.len()
            || !linear
                .iter()
                .zip(hinted)
                .all(|(a, b)| (a.x - b.x).abs() <= 1.0 / 16.0 && b.y.is_finite())
        {
            return;
        }
        for (linear, hinted) in self.linear.points_mut().iter_mut().zip(hinted) {
            linear.y = hinted.y;
        }
    }

    /// Rasterize the resulting path exactly as swash's `Source::Outline` does, including the
    /// same fractional X offset for bounds and coverage. The caller keeps swash's colour path.
    pub(super) fn render(&mut self, format: Format, offset: Vector) -> Image {
        let mut data = Vec::new();
        let placement = Mask::with_scratch(self.linear.path(), &mut self.scratch)
            .format(format)
            .origin(Origin::BottomLeft)
            .offset(offset)
            .render_offset(offset)
            .inspect(|fmt, w, h| data.resize(fmt.buffer_size(w, h), 0))
            .render_into(&mut data, None);
        Image {
            source: Source::Outline,
            content: if format == Format::Alpha {
                Content::Mask
            } else {
                Content::SubpixelMask
            },
            placement,
            data,
        }
    }
}
