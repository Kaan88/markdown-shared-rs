//! The application's side of a grid's state: how far back it looks, whether it has the focus,
//! its default cursor style, and the questions that need both the view and the screen.

use super::metrics::Metrics;
use crate::gfx::Rect;
use mdshared_term::{CursorShape, Point, Screen};
use std::ops::Range;

/// How a screen is looked at: the scroll offset into the scrollback, the focus, and whether the
/// cursor is to be drawn this frame. Plain state that the application keeps (or makes on each
/// frame) and passes to the drawing; built with [`View::new`] and the builder methods.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct View {
    scroll: usize,
    focused: bool,
    cursor_shown: bool,
    cursor_preference: Option<(CursorShape, bool)>,
}

impl Default for View {
    fn default() -> View {
        View::new()
    }
}

impl View {
    /// The live screen (no scroll offset), focused, with the cursor drawn.
    pub const fn new() -> View {
        View {
            scroll: 0,
            focused: true,
            cursor_shown: true,
            cursor_preference: None,
        }
    }

    /// The view looking `lines` lines back into the scrollback: the top row shows the line
    /// that many lines above the screen's top row. More than the scrollback holds counts as all
    /// of it (see [`scroll_in`](Self::scroll_in)).
    pub const fn scrolled(self, lines: usize) -> View {
        View {
            scroll: lines,
            ..self
        }
    }

    /// The view with or without the keyboard focus: without it the cursor is a hollow block and
    /// does not blink.
    pub const fn focused(self, on: bool) -> View {
        View {
            focused: on,
            ..self
        }
    }

    /// The view with the cursor drawn or not, whatever the program asked: off for the off half
    /// of a blink (which [`draw_screen_animated`](super::draw_screen_animated) does itself).
    pub const fn cursor_shown(self, on: bool) -> View {
        View {
            cursor_shown: on,
            ..self
        }
    }

    /// Sets the application's default cursor shape and whether it blinks. A program's
    /// explicit DECSCUSR style takes precedence for both until it resets the style to default.
    /// This does not show a hidden cursor or change the steady outline drawn without focus.
    /// The animated draw calls still need a blink interval and an animator that allows motion.
    pub const fn cursor_preference(self, shape: CursorShape, blinking: bool) -> View {
        View {
            cursor_preference: Some((shape, blinking)),
            ..self
        }
    }

    pub(super) fn cursor_style(&self, screen: &Screen) -> (CursorShape, bool) {
        if let Some(preference) = self.cursor_preference
            && !screen.cursor_style_is_explicit()
        {
            return preference;
        }
        let c = screen.cursor();
        (c.shape, c.blinking)
    }

    /// The scroll offset as given.
    pub fn scroll(&self) -> usize {
        self.scroll
    }

    /// Whether the view has the keyboard focus.
    pub fn is_focused(&self) -> bool {
        self.focused
    }

    /// Whether the cursor is to be drawn (when the program shows it and it is on the view).
    pub fn is_cursor_shown(&self) -> bool {
        self.cursor_shown
    }

    /// The scroll offset brought into what `screen`'s scrollback holds.
    pub fn scroll_in(&self, screen: &Screen) -> usize {
        self.scroll.min(screen.scrollback_len())
    }

    /// The absolute line number ([`Screen::first_line`]) of the view's top row.
    pub fn top_line(&self, screen: &Screen) -> u64 {
        screen.screen_start() - self.scroll_in(screen) as u64
    }

    /// The absolute line number shown in the view's row `row`.
    pub fn line_of_row(&self, screen: &Screen, row: usize) -> u64 {
        self.top_line(screen).saturating_add(row as u64)
    }

    /// The view row the cursor is on, or `None` when it is scrolled out of the view (whether
    /// the program shows the cursor is not asked). The row to redraw when the cursor blinks.
    pub fn cursor_row(&self, screen: &Screen) -> Option<usize> {
        let row = screen.cursor().row + self.scroll_in(screen);
        (row < screen.rows()).then_some(row)
    }

    /// The cell of the scrollback and the screen under the point `x, y` of `area`, in absolute
    /// line numbers, for [`Selection::start`](mdshared_term::Selection::start) and
    /// [`Selection::extend`](mdshared_term::Selection::extend): a point outside the grid takes
    /// the nearest cell ([`Metrics::cell_clamped`]), so a drag past the edge selects to it.
    pub fn point_at(
        &self,
        screen: &Screen,
        metrics: &Metrics,
        area: Rect,
        x: f32,
        y: f32,
    ) -> Point {
        let (row, col) = metrics.cell_clamped(area, x, y, screen.cols(), screen.rows());
        Point::new(self.line_of_row(screen, row), col)
    }

    /// The view rows to redraw for the screen rows `dirty`
    /// ([`Screen::take_dirty`](mdshared_term::Screen::take_dirty)): the same rows on the live
    /// screen, and all of them while the view is scrolled back and anything changed, since the
    /// screen's rows then sit lower in the view and output that scrolled the screen moved every
    /// line of it. Rows past the screen's last are dropped; nothing changed gives nothing.
    pub fn damage(&self, screen: &Screen, dirty: &[Range<usize>]) -> Vec<Range<usize>> {
        let rows = screen.rows();
        let clipped: Vec<Range<usize>> = dirty
            .iter()
            .map(|r| r.start.min(rows)..r.end.min(rows))
            .filter(|r| !r.is_empty())
            .collect();
        if clipped.is_empty() {
            return clipped;
        }
        if self.scroll_in(screen) > 0 {
            std::iter::once(0..rows).collect()
        } else {
            clipped
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::single_range_in_vec_init)]
    use super::*;

    fn scrolled_screen() -> Screen {
        let mut s = Screen::new(10, 4, 100);
        for i in 0..10 {
            s.feed(format!("line {i}\r\n").as_bytes());
        }
        s
    }

    #[test]
    fn a_view_starts_live_focused_and_with_the_cursor() {
        let v = View::new();
        assert_eq!(v, View::default());
        assert_eq!(
            (v.scroll(), v.is_focused(), v.is_cursor_shown()),
            (0, true, true)
        );
        let v = v.scrolled(3).focused(false).cursor_shown(false);
        assert_eq!(
            (v.scroll(), v.is_focused(), v.is_cursor_shown()),
            (3, false, false)
        );
    }

    #[test]
    fn the_scroll_offset_reads_lines_of_the_scrollback() {
        let s = scrolled_screen();
        // Ten lines and an empty one on a screen of four: seven went into the scrollback.
        assert_eq!(s.scrollback_len(), 7);
        let live = View::new();
        assert_eq!(live.top_line(&s), s.screen_start());
        let back = View::new().scrolled(2);
        assert_eq!(back.top_line(&s), s.screen_start() - 2);
        assert_eq!(s.line_at(back.line_of_row(&s, 0)).unwrap().text(), "line 5");
        // Past the scrollback: all of it, and no further.
        let far = View::new().scrolled(1000);
        assert_eq!(far.scroll_in(&s), 7);
        assert_eq!(far.top_line(&s), s.first_line());
    }

    #[test]
    fn the_cursor_row_moves_down_with_the_scroll_and_leaves_the_view() {
        let s = scrolled_screen();
        assert_eq!(s.cursor().row, 3);
        assert_eq!(View::new().cursor_row(&s), Some(3));
        assert_eq!(View::new().scrolled(0).cursor_row(&s), Some(3));
        assert_eq!(View::new().scrolled(1).cursor_row(&s), None);
        let mut top = Screen::new(10, 4, 100);
        top.feed(b"x");
        assert_eq!(
            View::new().scrolled(2).cursor_row(&top),
            Some(0),
            "no scrollback: no offset"
        );
    }

    #[test]
    fn a_point_names_the_line_shown_under_it() {
        let s = scrolled_screen();
        let m = Metrics::fixed(14.0, 1.0, 8.0, 16.0);
        let area = Rect::new(0.0, 0.0, 80.0, 64.0);
        let live = View::new();
        assert_eq!(
            live.point_at(&s, &m, area, 17.0, 20.0),
            Point::new(s.screen_start() + 1, 2)
        );
        let back = View::new().scrolled(3);
        assert_eq!(
            back.point_at(&s, &m, area, 17.0, 20.0),
            Point::new(s.screen_start() - 2, 2)
        );
        // Past the bottom right corner: the last cell.
        assert_eq!(
            live.point_at(&s, &m, area, 900.0, 900.0),
            Point::new(s.screen_start() + 3, 9)
        );
    }

    #[test]
    fn damage_is_the_dirty_rows_live_and_everything_scrolled_back() {
        let s = scrolled_screen();
        let live = View::new();
        assert_eq!(live.damage(&s, &[1..2, 3..9]), vec![1..2, 3..4]);
        assert_eq!(live.damage(&s, &[]), Vec::<Range<usize>>::new());
        assert_eq!(live.damage(&s, &[7..9]), Vec::<Range<usize>>::new());
        let back = View::new().scrolled(2);
        assert_eq!(back.damage(&s, &[1..2]), vec![0..4]);
        assert_eq!(back.damage(&s, &[]), Vec::<Range<usize>>::new());
    }
}
