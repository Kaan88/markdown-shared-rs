//! Side panes: the title row of a pane, a selectable row, and the splitter on its inner edge.
//! Pure drawing; the caller records the hits and keeps the scroll state.

use crate::gfx::{Face, Painter, Rect};
use crate::theme::Palette;

/// Row pitch and height in a pane's list.
pub const ROW: f32 = 30.0;
pub const ROW_H: f32 = 28.0;
/// The pane title row.
pub const HEAD: f32 = 40.0;

/// Fills the pane's title row (the top `HEAD` px of `r` are the title; the fill covers all of
/// `r`, as the pane background) and writes `title` in it.
pub fn draw_head(p: &mut Painter, pal: &Palette, r: Rect, title: &str) {
    p.fill(r, pal.pane);
    p.text(
        Face::Semibold,
        11.0,
        r.x + 16.0,
        r.y + HEAD / 2.0,
        title,
        pal.muted,
        r.w - 60.0,
    );
}

/// The background of a list row: the selected look (with the accent bar), the hover or pressed
/// fill, or nothing.
pub fn draw_row_fill(
    p: &mut Painter,
    pal: &Palette,
    row: Rect,
    hovered: bool,
    pressed: bool,
    selected: bool,
) {
    if selected {
        p.rbox(row, 5.0, pal.on);
        p.rbox(
            Rect::new(row.x, row.y + 7.0, 3.0, row.h - 14.0),
            1.5,
            pal.accent,
        );
    } else if hovered {
        p.rbox(row, 5.0, if pressed { pal.press } else { pal.hover });
    }
}

/// The splitter on a pane's inner edge, and the hairline that marks it. `right_edge` is true for
/// a pane on the left (whose inner edge is its right side). Returns the rectangle to hit-test
/// for the drag.
pub fn draw_splitter(p: &mut Painter, pal: &Palette, r: Rect, right_edge: bool, hot: bool) -> Rect {
    let px = p.px();
    let x = if right_edge { r.right() - px } else { r.x };
    if hot {
        p.fill(Rect::new(x - px, r.y, 3.0 * px, r.h), pal.accent);
    } else {
        p.fill(Rect::new(x, r.y, px, r.h), pal.line);
    }
    Rect::new(x - 3.0, r.y, 6.0, r.h)
}
