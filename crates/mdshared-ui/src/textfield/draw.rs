//! Drawing a single-line field into the caller's text rectangle.

use super::{
    FieldColors, TextField,
    line::{Line, Stop},
};
use crate::gfx::{Face, Painter, Rect};
use parley::Affinity;

impl TextField {
    /// Draws text, selection and caret inside `area` (the text area, already inset from the box
    /// the caller drew), clipped to it and vertically centred, scrolling horizontally so the caret
    /// stays visible. A selection and the caret (when `caret_on`) show only when `focused`. When
    /// the text is empty, `placeholder` is drawn in the placeholder colour.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        p: &mut Painter,
        area: Rect,
        face: Face,
        size: f32,
        colors: &FieldColors,
        placeholder: &str,
        focused: bool,
        caret_on: bool,
    ) {
        // While an input method composes, its text is drawn in place at the caret, underlined,
        // with the input method's caret; the selection is not shown.
        // A secret shows neither its text nor what is being composed for it.
        let composing = self.preedit.as_ref().filter(|_| focused && !self.secret);
        let shown: std::borrow::Cow<str> = match composing {
            Some(pre) => {
                let mut s = self.text.clone();
                s.insert_str(self.caret, &pre.text);
                s.into()
            }
            // Owned, so the scroll below can still change.
            None if self.secret => self.display().into_owned().into(),
            None => self.text.as_str().into(),
        };
        let (caret_b, anchor_b) = match composing {
            Some(pre) => (self.caret + pre.caret(), self.caret + pre.caret()),
            None if self.secret => (self.masked(self.caret), self.masked(self.anchor)),
            None => (self.caret, self.anchor),
        };
        let (layout, scale) = p.field_layout(face, size, &shown, colors.text);
        let line = Line::new(&layout, scale);
        let cx = line.x(Stop {
            byte: caret_b,
            affinity: self
                .affinity
                .filter(|_| composing.is_none())
                .unwrap_or(Affinity::Downstream),
        });

        // Keep the caret in view, then keep the scroll within the text.
        self.scroll = reveal_scroll(self.scroll, cx, line.width, area.w);

        let cy = area.cy();
        let h = size * 1.25;
        let top = cy - h / 2.0;
        let old = p.clip(area);
        let ox = ((area.x - self.scroll) * scale).round() / scale;
        if shown.is_empty() {
            p.text(
                face,
                size,
                area.x,
                cy,
                placeholder,
                colors.placeholder,
                area.w,
            );
        } else {
            if focused && caret_b != anchor_b {
                line.spans(caret_b.min(anchor_b)..caret_b.max(anchor_b), |s| {
                    p.fill_a(
                        Rect::new(ox + s.start, top, s.end - s.start, h),
                        colors.selection,
                        0.35,
                    );
                });
            }
            p.paint_field_layout(&layout, ox, cy);
            if let Some(pre) = composing {
                let at = self.caret;
                let baseline = layout.get(0).map_or(0.0, |l| l.metrics().baseline);
                let underline =
                    ((cy * scale - layout.height() / 2.0).round() + baseline.round() + 2.0) / scale;
                line.spans(at..at + pre.text.len(), |s| {
                    p.fill(
                        Rect::new(ox + s.start, underline, s.end - s.start, p.px()),
                        colors.text,
                    );
                });
                // The clause the input method has selected, a second line under it.
                if let Some((a, b)) = pre.cursor.filter(|c| c.0 < c.1) {
                    line.spans(at + a..at + b, |s| {
                        p.fill_a(
                            Rect::new(ox + s.start, top, s.end - s.start, h),
                            colors.selection,
                            0.25,
                        );
                    });
                }
            }
        }
        if focused && caret_on {
            let x = if shown.is_empty() { area.x } else { ox + cx };
            p.fill(Rect::new(x, top, 1.0f32.max(p.px()), h), colors.caret);
        }
        p.restore(old);
    }
}

/// Move the smallest distance that reveals the caret and keep the scroll within the text.
pub(super) fn reveal_scroll(mut scroll: f32, cx: f32, total: f32, width: f32) -> f32 {
    let max_scroll = (total + 1.0 - width).max(0.0);
    if cx - scroll > width - 1.0 {
        scroll = cx - (width - 1.0);
    }
    if cx < scroll {
        scroll = cx;
    }
    scroll.clamp(0.0, max_scroll)
}
