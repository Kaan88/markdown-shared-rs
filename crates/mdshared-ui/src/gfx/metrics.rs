//! Single-line measurements shared by painting and input. No bitmap needed.

use super::{Brush, Face, Gfx, Painter, Rgb};
use parley::Layout;
use std::rc::Rc;

impl Gfx {
    pub(crate) fn field_layout(
        &mut self,
        face: Face,
        size: f32,
        text: &str,
        scale: f32,
        ink: Rgb,
    ) -> Rc<Layout<Brush>> {
        self.layout(face, size * scale, ink, 0.0, text)
    }
    pub(super) fn measure_at(&mut self, face: Face, size: f32, text: &str, scale: f32) -> f32 {
        if text.is_empty() {
            return 0.0;
        }
        self.layout(face, size * scale, [0, 0, 0], 0.0, text)
            .width()
            / scale
    }

    pub(crate) fn caret_stops_at(
        &mut self,
        face: Face,
        size: f32,
        text: &str,
        scale: f32,
    ) -> Vec<f32> {
        if text.is_empty() {
            return vec![0.0];
        }
        let layout = self.field_layout(face, size, text, scale, [0; 3]);
        // Runs arrive in visual order. Record their edges by byte, then return them in the
        // character order the field uses. At a bidi boundary prefer the following cluster.
        let mut xs = vec![0.0; text.len() + 1];
        for line in layout.lines() {
            let metrics = line.metrics();
            let mut x = metrics.offset + metrics.inline_min_coord;
            for run in line.runs() {
                let mut left = x;
                for cluster in run.visual_clusters() {
                    let bytes = cluster.text_range();
                    let right = left + cluster.advance();
                    let (start, end) = if run.is_rtl() {
                        (right, left)
                    } else {
                        (left, right)
                    };
                    if bytes.end == text.len() {
                        xs[bytes.end] = end / scale;
                    }
                    // A combining mark or a joined emoji has no caret edge of its own.
                    xs[bytes].fill(start / scale);
                    left = right;
                }
                x += run.advance();
            }
        }
        text.char_indices()
            .map(|(b, _)| xs[b])
            .chain([xs[text.len()]])
            .collect()
    }
}

impl Painter<'_> {
    pub(crate) fn field_layout(
        &mut self,
        face: Face,
        size: f32,
        text: &str,
        ink: Rgb,
    ) -> (Rc<Layout<Brush>>, f32) {
        (
            self.gfx.field_layout(face, size, text, self.scale, ink),
            self.scale,
        )
    }

    pub(crate) fn paint_field_layout(&mut self, layout: &Layout<Brush>, x: f32, cy: f32) {
        let y = (cy * self.scale - layout.height() / 2.0).round();
        self.draw_layout(layout, (x * self.scale).round(), y);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caret_stops_for_a_paste_use_one_layout_including_cluster_interiors() {
        let mut gfx = Gfx::new();
        let text = "a\u{301}b".repeat(512);
        let stops = gfx.caret_stops_at(Face::Ui, 14.0, &text, 1.5);
        assert_eq!(gfx.cached_layouts(), 1);
        assert_eq!(stops.len(), text.chars().count() + 1);
        assert_eq!(stops[0], 0.0);
        assert_eq!(stops[0], stops[1]); // No independent edge for the combining accent.
        assert!(stops[2] > stops[1]);
        let width = gfx.measure_at(Face::Ui, 14.0, &text, 1.5);
        assert!((stops.last().unwrap() - width).abs() < 0.1);
        let rtl = gfx.caret_stops_at(Face::Ui, 14.0, "אב", 1.5);
        assert!(rtl[0] > rtl[1] && rtl[1] > rtl[2]);
        assert_eq!(gfx.caret_stops_at(Face::Ui, 14.0, "", 1.5), [0.0]);
    }
}
