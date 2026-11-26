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
        let mut stops = Vec::with_capacity(text.len() + 1);
        for (i, _) in text.char_indices() {
            stops.push(self.measure_at(face, size, &text[..i], scale));
        }
        stops.push(self.measure_at(face, size, text, scale));
        stops
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
