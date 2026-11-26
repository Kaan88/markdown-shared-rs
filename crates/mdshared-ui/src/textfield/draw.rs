//! Drawing a single-line field into the caller's text rectangle.

use super::{FieldColors, TextField};
use crate::gfx::{Face, Painter, Rect};

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
        let stops = p.caret_stops(face, size, &shown);
        let total = stops.last().copied().unwrap_or(0.0);
        let caret_idx = shown[..caret_b].chars().count().min(stops.len() - 1);
        let anchor_idx = shown[..anchor_b].chars().count().min(stops.len() - 1);
        let cx = stops[caret_idx];

        // Keep the caret in view, then keep the scroll within the text.
        self.scroll = reveal_scroll(self.scroll, cx, total, area.w);

        let cy = area.cy();
        let h = size * 1.25;
        let top = cy - h / 2.0;
        let old = p.clip(area);
        let ox = area.x - self.scroll;
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
            if focused && caret_idx != anchor_idx {
                let (a, b) = (
                    stops[caret_idx.min(anchor_idx)],
                    stops[caret_idx.max(anchor_idx)],
                );
                p.fill_a(Rect::new(ox + a, top, b - a, h), colors.selection, 0.35);
            }
            p.text(face, size, ox, cy, &shown, colors.text, f32::INFINITY);
            if let Some(pre) = composing {
                let at = self.caret;
                p.underline_range(
                    face,
                    size,
                    ox,
                    cy,
                    &shown,
                    at..at + pre.text.len(),
                    colors.text,
                );
                // The clause the input method has selected, a second line under it.
                if let Some((a, b)) = pre.cursor.filter(|c| c.0 < c.1) {
                    let a_x = p.measure(face, size, &shown[..at + a]);
                    let b_x = p.measure(face, size, &shown[..at + b]);
                    p.fill_a(
                        Rect::new(ox + a_x, top, b_x - a_x, h),
                        colors.selection,
                        0.25,
                    );
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
