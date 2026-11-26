//! The frame kept between scroll calls: [`DocumentRenderer::view_frame`] shifts the rows that stay
//! on screen and paints only the band that came into view, reporting `band`/`shift` to the shell.
//! [`DocumentRenderer::view`] draws a fresh viewport bitmap the same way, for tests.

use super::floats::FloatBook;
use super::pager::Pager;
use super::paint::CachedBlock;
use super::{DocumentRenderer, Frame, Options, PageLayout, Slice};
use crate::bitmap::Bitmap;
use mdshared_md_core::model::Document;
use std::collections::HashMap;

const MAX_WIDTH: u32 = 8192;

pub(super) fn layout_size(options: &Options) -> (f32, f32) {
    let scale = if options.scale.is_finite() {
        options.scale.clamp(0.1, 8.0)
    } else {
        1.0
    };
    (
        options.width.max(120.0).min(MAX_WIDTH as f32 / scale),
        scale,
    )
}

fn layout_device_width(options: &Options) -> u32 {
    let (logical_width, scale) = layout_size(options);
    (logical_width * scale).round() as u32
}

/// Cap the bytes passed to `Bitmap::new`, including on 32-bit hosts. Page renders can be
/// taller than a viewport, so height is bounded by the remaining byte budget, not the width cap.
pub(super) fn raster_size(width: u32, rows: u32) -> (u32, u32) {
    const MAX_RASTER_BYTES: u32 = 256 * 1024 * 1024;
    let width = width.clamp(1, MAX_WIDTH);
    let stride = width.checked_mul(4).expect("bounded width");
    (width, rows.clamp(1, MAX_RASTER_BYTES / stride))
}

impl DocumentRenderer {
    /// Draws the viewport into a frame kept between calls. When the new view still overlaps the last
    /// one, those rows are shifted in place and only the band that came into view is painted.
    ///
    /// `top` is in the current coordinates (see [`top`](Self::top)). Blocks the viewport shows are
    /// measured first if they were estimated, which keeps the block at `top` where it is; the
    /// frame's `top` is the row actually drawn (clamped to the page).
    pub fn view_frame(
        &mut self,
        doc: &Document,
        options: &Options,
        top: f32,
        height: f32,
    ) -> &Frame {
        let width = layout_device_width(options);
        self.view_frame_inner(doc, options, top, width, height)
    }

    /// Draws a kept viewport frame at an explicit device-pixel size. The width used to lay out
    /// text may be larger than the bitmap, in which case the drawing is clipped. Empty
    /// dimensions become one pixel; excessive dimensions are reduced to fit the raster budget.
    pub fn view_frame_sized(
        &mut self,
        doc: &Document,
        options: &Options,
        top: f32,
        size: (u32, u32),
    ) -> &Frame {
        self.view_frame_inner(doc, options, top, size.0, size.1 as f32)
    }

    fn view_frame_inner(
        &mut self,
        doc: &Document,
        options: &Options,
        top: f32,
        width: u32,
        height: f32,
    ) -> &Frame {
        let (width, rows) = raster_size(width, height.ceil() as u32);
        let height = height.clamp(1.0, rows as f32).max(1.0);
        self.ensure_layout(doc, options);
        self.store.begin_frame();
        self.set_top(top);
        self.view.height = height;
        let top = self.settle_view(doc, height).round();
        self.set_top(top);
        let stale = std::mem::take(&mut self.view.stale);
        // Rows an animated image changed, cut to this viewport.
        let repaint = self.view.repaint.take().and_then(|(y0, y1)| {
            let (a, b) = (y0.max(top).floor(), y1.min(top + rows as f32).ceil());
            (b > a).then_some((a, b - a))
        });
        let background = options.theme.bg;
        let page = self.page.as_ref().expect("laid out");
        let mut pager = Pager::new(
            doc,
            options,
            &mut self.fonts,
            &mut self.layouts,
            false,
            &mut self.store,
            &self.details,
            self.gap,
        );
        let same_size = self
            .frame
            .as_ref()
            .is_some_and(|frame| frame.bitmap.width == width && frame.bitmap.height == rows);
        if same_size
            && !stale
            && repaint.is_none()
            && self.frame.as_ref().is_some_and(|f| f.top == top)
        {
            // Nothing changed, so there is nothing for the caller to send again.
            let frame = self.frame.as_mut().expect("frame");
            frame.band = Some((0, 0));
            frame.shift = 0;
            self.view.span = Some(frame_span(page, top, rows as f32));
            return self.frame.as_ref().expect("frame");
        }
        let mut band = (top, rows as f32);
        let mut shift = 0i32;
        let mut whole = true;
        if same_size {
            let frame = self.frame.as_mut().expect("frame");
            let dy = frame.top as i64 - top as i64;
            if let (false, 0, Some(changed)) = (stale, dy, repaint) {
                // Nothing moved; only an animated image went to its next frame.
                band = changed;
                whole = false;
            } else if !stale && repaint.is_none() && dy.unsigned_abs() < rows as u64 {
                shift_rows(&mut frame.bitmap, dy as i32);
                // `dy` is old top minus new top: positive when the view scrolled up and the newly
                // exposed rows are at the top, negative when it scrolled down and they are at the
                // bottom.
                band = if dy > 0 {
                    (top, dy as f32)
                } else {
                    (top + rows as f32 + dy as f32, (-dy) as f32)
                };
                // Row `r` of the new frame was row `r - dy` of the old one.
                shift = -(dy as i32);
                whole = false;
            } else {
                frame.bitmap.clear(background);
            }
            frame.top = top;
        } else {
            let mut bitmap = Bitmap::new(width, rows);
            bitmap.clear(background);
            self.frame = Some(Frame {
                bitmap,
                top,
                stats: crate::text::CacheStats::default(),
                band: None,
                shift: 0,
            });
            band = (top, rows as f32);
        }
        let frame_top = top;
        // Blocks can reach past the band, so draw the band on its own bitmap and copy it in;
        // otherwise the overlap would be blended twice.
        collect(page, &self.cache, &self.floats, &mut pager, band.0, band.1);
        let stats = if band.1 + 0.5 >= rows as f32 && (band.0 - frame_top).abs() < 0.5 {
            let frame = self.frame.as_mut().expect("frame");
            pager.draw(&mut frame.bitmap, frame_top, &mut self.engine)
        } else {
            let band_rows = (band.1.ceil() as u32).clamp(1, rows);
            let mut band_bitmap = Bitmap::new(width, band_rows);
            band_bitmap.clear(background);
            let stats = pager.draw(&mut band_bitmap, band.0, &mut self.engine);
            let frame = self.frame.as_mut().expect("frame");
            copy_rows(
                &mut frame.bitmap,
                &band_bitmap,
                (band.0 - frame_top) as usize,
            );
            stats
        };
        let first = (band.0 - frame_top).round().max(0.0) as u32;
        let band_rows = (first, band.1.ceil().max(0.0) as u32);
        self.view.span = Some(frame_span(page, top, rows as f32));
        let frame = self.frame.as_mut().expect("frame");
        frame.stats = stats;
        frame.band = (!whole).then_some(band_rows);
        frame.shift = shift;
        self.frame.as_ref().expect("frame")
    }

    /// Renders one viewport slice into a fresh bitmap, reusing the contexts and glyph cache. Like
    /// a frame, it measures what it shows and moves the anchor to `top`.
    pub fn view(&mut self, doc: &Document, options: &Options, top: f32, height: f32) -> Slice {
        let (_, rows) = raster_size(layout_device_width(options), height.ceil() as u32);
        self.ensure_layout(doc, options);
        let height = height.clamp(1.0, rows as f32).max(1.0);
        self.store.begin_frame();
        self.set_top(top);
        self.view.height = height;
        let top = self.settle_view(doc, height);
        let page = self.page.as_ref().expect("laid out");
        let total_height = page.total_height;
        let mut pager = Pager::new(
            doc,
            options,
            &mut self.fonts,
            &mut self.layouts,
            false,
            &mut self.store,
            &self.details,
            self.gap,
        );
        collect(page, &self.cache, &self.floats, &mut pager, top, height);
        pager.finish_slice(top, height, total_height, &mut self.engine)
    }
}

#[cfg(test)]
#[test]
fn bitmap_dimensions_are_bounded_before_allocating() {
    assert_eq!(raster_size(u32::MAX, u32::MAX), (8192, 8192));
    assert_eq!(raster_size(1000, u32::MAX), (1000, 67_108));
    assert_eq!(raster_size(0, 0), (1, 1));
}

/// The first and last block index a frame at `top` with `rows` rows shows.
fn frame_span(page: &PageLayout, top: f32, rows: f32) -> (usize, usize) {
    let first = page.first_below(top).min(page.len());
    let mut last = first;
    while last < page.len() && page.y_at(last + 1) < top + rows {
        last += 1;
    }
    (first, last)
}

/// Queues the paints of every block that reaches `top..top + height`, from the paint cache
/// (every such block is measured and cached by then), and lays out the footnote list if it shows.
fn collect(
    page: &PageLayout,
    cache: &HashMap<u32, CachedBlock>,
    floats: &FloatBook,
    pager: &mut Pager,
    top: f32,
    height: f32,
) {
    let bottom = top + height;
    let mut queue = |index: usize| {
        let span = page.spans[index];
        if let Some(cached) = cache.get(&span.block) {
            let origin = span.y - cached.phase;
            pager.paints.extend(cached.paints.iter().map(|paint| {
                let mut paint = paint.clone();
                paint.shift(origin);
                paint
            }));
        }
    };
    let mut index = page.first_below(top);
    // A floated image is painted by the block that holds it and can hang below that block: the
    // blocks above whose floats are open here come first, as they do on the page.
    for owner in floats.owners(index.min(page.len())) {
        queue(owner);
    }
    while index < page.len() && page.spans[index].y < bottom {
        queue(index);
        index += 1;
    }
    let foot = &page.foot;
    if foot.height > 0.0 && foot.y + foot.height > top && foot.y < bottom {
        pager.y = foot.y;
        pager.notes.clear();
        pager.footnotes();
    }
}

/// Copies `source` into `target` starting at row `row`.
fn copy_rows(target: &mut Bitmap, source: &Bitmap, row: usize) {
    let stride = target.width as usize * 4;
    let rows = source.height as usize;
    let start = row * stride;
    target.pixels[start..start + rows * stride].copy_from_slice(&source.pixels[..rows * stride]);
}

/// Moves the rows of `bitmap` down by `dy` rows when `dy` is positive, up when negative.
fn shift_rows(bitmap: &mut Bitmap, dy: i32) {
    let stride = bitmap.width as usize * 4;
    let rows = bitmap.height as usize;
    let shift = dy.unsigned_abs() as usize;
    if shift == 0 || shift >= rows {
        return;
    }
    if dy > 0 {
        // The view scrolled up: the content moves down.
        bitmap
            .pixels
            .copy_within(0..(rows - shift) * stride, shift * stride);
    } else {
        // The view scrolled down: the content moves up.
        bitmap.pixels.copy_within(shift * stride..rows * stride, 0);
    }
}
