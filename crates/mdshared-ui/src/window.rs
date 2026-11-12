//! Frameless-window helpers that are pure functions of a pointer position and the window size.

use winit::window::ResizeDirection;

/// How close to the window edge a press resizes, in logical px. The window has no frame outside
/// its pixels to grab, so the band has to be wide enough to hit without aiming.
pub const EDGE: f32 = 8.0;
/// How far along an edge from a corner a press still resizes diagonally.
pub const CORNER: f32 = 20.0;

/// The resize direction for a pointer at logical `(x, y)` in a `w` x `h` window, or `None` when it
/// is not on the resize band. The caller returns `None` itself for maximized windows.
pub fn edge_at(x: f32, y: f32, w: f32, h: f32) -> Option<ResizeDirection> {
    if !(x < EDGE || x >= w - EDGE || y < EDGE || y >= h - EDGE) {
        return None;
    }
    // On an edge: the corner band along it turns the resize diagonal.
    let (l, r, t, b) = (x < CORNER, x >= w - CORNER, y < CORNER, y >= h - CORNER);
    let (l, r) = (l && (x < EDGE || t || b), r && (x >= w - EDGE || t || b));
    let (t, b) = (t && (y < EDGE || l || r), b && (y >= h - EDGE || l || r));
    Some(match (l, r, t, b) {
        (true, _, true, _) => ResizeDirection::NorthWest,
        (_, true, true, _) => ResizeDirection::NorthEast,
        (true, _, _, true) => ResizeDirection::SouthWest,
        (_, true, _, true) => ResizeDirection::SouthEast,
        (true, ..) => ResizeDirection::West,
        (_, true, ..) => ResizeDirection::East,
        (_, _, true, _) => ResizeDirection::North,
        (_, _, _, true) => ResizeDirection::South,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bands() {
        let (w, h) = (800.0, 600.0);
        assert_eq!(edge_at(400.0, 300.0, w, h), None);
        assert_eq!(edge_at(2.0, 300.0, w, h), Some(ResizeDirection::West));
        assert_eq!(edge_at(798.0, 300.0, w, h), Some(ResizeDirection::East));
        assert_eq!(edge_at(400.0, 1.0, w, h), Some(ResizeDirection::North));
        assert_eq!(edge_at(400.0, 599.0, w, h), Some(ResizeDirection::South));
        assert_eq!(edge_at(2.0, 2.0, w, h), Some(ResizeDirection::NorthWest));
        assert_eq!(edge_at(15.0, 2.0, w, h), Some(ResizeDirection::NorthWest));
        assert_eq!(
            edge_at(795.0, 595.0, w, h),
            Some(ResizeDirection::SouthEast)
        );
        assert_eq!(edge_at(25.0, 2.0, w, h), Some(ResizeDirection::North));
        assert_eq!(edge_at(12.0, 300.0, w, h), None);
    }
}
