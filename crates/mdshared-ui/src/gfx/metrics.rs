//! Single-line measurements shared by painting and input. No bitmap needed.

use super::{Face, Gfx};

impl Gfx {
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
