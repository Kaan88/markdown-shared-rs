//! The overlay scrollbar: geometry (pure functions of rectangles and lengths) and drawing.

use crate::gfx::{Painter, Rect};
use crate::theme::Palette;

/// The track and thumb of an overlay scrollbar for `area`, or `None` when everything fits.
/// `content`, `view` and `offset` may be in any one unit.
pub fn scrollbar(area: Rect, content: f32, view: f32, offset: f32) -> Option<(Rect, Rect)> {
    if content <= view + 0.5 || area.h < 24.0 {
        return None;
    }
    let track = Rect::new(area.right() - 12.0, area.y + 2.0, 12.0, area.h - 4.0);
    let len = (track.h * view / content).clamp(28.0_f32.min(track.h), track.h);
    let t = (offset / (content - view)).clamp(0.0, 1.0);
    let thumb = Rect::new(track.x, track.y + t * (track.h - len), track.w, len);
    Some((track, thumb))
}

/// The scroll offset that puts the thumb's top at `thumb_y`.
pub fn offset_for_thumb(track: Rect, thumb_h: f32, thumb_y: f32, content: f32, view: f32) -> f32 {
    let span = (track.h - thumb_h).max(1.0);
    ((thumb_y - track.y) / span).clamp(0.0, 1.0) * (content - view).max(0.0)
}

/// Thin at rest, wider while hovered or dragged.
pub fn draw_scrollbar(p: &mut Painter, pal: &Palette, track: Rect, thumb: Rect, active: bool) {
    let w = if active { 8.0 } else { 3.0 };
    if active {
        p.rbox_a(
            Rect::new(track.right() - w - 3.0, track.y, w, track.h),
            w / 2.0,
            pal.thumb,
            0.10,
        );
    }
    let r = Rect::new(track.right() - w - 3.0, thumb.y, w, thumb.h);
    p.rbox_a(r, w / 2.0, pal.thumb, if active { 0.85 } else { 0.55 });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrollbar_geometry() {
        let area = Rect::new(0.0, 0.0, 100.0, 404.0);
        assert!(scrollbar(area, 300.0, 404.0, 0.0).is_none());
        let (track, thumb) = scrollbar(area, 800.0, 400.0, 0.0).unwrap();
        assert_eq!(track.h, 400.0);
        assert_eq!(thumb.h, 200.0);
        assert_eq!(thumb.y, track.y);
        let (_, end) = scrollbar(area, 800.0, 400.0, 400.0).unwrap();
        assert_eq!(end.bottom(), track.bottom());
        let back = offset_for_thumb(track, end.h, end.y, 800.0, 400.0);
        assert!((back - 400.0).abs() < 0.01);
        // A huge document keeps a grabbable thumb.
        let (_, tiny) = scrollbar(area, 1e7, 400.0, 0.0).unwrap();
        assert_eq!(tiny.h, 28.0);
    }
}
