//! The arithmetic of a list of rows of one height: which of them the viewport shows, where one
//! is, which one a point falls in, the scrollbar of the whole list, where a key moves the cursor
//! and what offset brings a row into view. It draws nothing and knows nothing of an event loop:
//! the caller keeps the count and the offset, and owns the hits and the keyboard. It refuses
//! columns, ranges and rows of different heights; a caller that needs those keeps its own
//! arithmetic.

use std::ops::Range;

use crate::gfx::Rect;
use crate::scroll;

/// A key that moves the cursor of a list. A page is the rows the viewport holds whole, never
/// fewer than one.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Nav {
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
}

/// A list of `count` rows, each `row_h` tall, seen through a viewport `view` tall and scrolled by
/// `scroll`. A row's pitch is its height plus any gap the caller wants between rows; the caller
/// draws the row itself inside the rectangle [`rect`](Rows::rect) names.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Rows {
    /// The pitch of one row.
    pub row_h: f32,
    /// The height of the list's viewport.
    pub view: f32,
    /// How many rows there are.
    pub count: usize,
    /// The offset the list is scrolled by, from the top of the first row. Answers clamp it, so an
    /// offset left past either end does not change what they say.
    pub scroll: f32,
}

impl Rows {
    /// A list at its top.
    pub fn new(row_h: f32, view: f32, count: usize) -> Rows {
        Rows {
            row_h,
            view,
            count,
            scroll: 0.0,
        }
    }

    /// The height of all the rows together.
    pub fn content(&self) -> f32 {
        self.count as f32 * self.row_h
    }

    /// The furthest the list can be scrolled; 0 when every row fits, so a short list never
    /// scrolls.
    pub fn max_scroll(&self) -> f32 {
        (self.content() - self.view).max(0.0)
    }

    /// `scroll` kept between the top of the list and [`max_scroll`](Self::max_scroll).
    pub fn clamp_scroll(&self, scroll: f32) -> f32 {
        scroll.clamp(0.0, self.max_scroll())
    }

    /// The rows the viewport touches: the first, and one past the last. Empty for an empty list;
    /// a viewport taller than the list holds all of it. A row is in it when any part of its
    /// rectangle is on screen, so the first and last are often only partly there.
    pub fn visible(&self) -> Range<usize> {
        if self.count == 0 || self.row_h <= 0.0 || self.view <= 0.0 {
            return 0..0;
        }
        let scroll = self.clamp_scroll(self.scroll);
        let first = ((scroll / self.row_h).floor() as usize).min(self.count);
        let end = (((scroll + self.view) / self.row_h).ceil() as usize).clamp(first, self.count);
        first..end
    }

    /// Where row `i` is in `area` at the current offset, whether or not it is on screen: as tall
    /// as the row's pitch, and as wide as the area. A row past the last is answered like any
    /// other and lands below the list; the caller asks [`visible`](Self::visible) first.
    pub fn rect(&self, area: Rect, i: usize) -> Rect {
        Rect::new(
            area.x,
            area.y + i as f32 * self.row_h - self.scroll,
            area.w,
            self.row_h,
        )
    }

    /// Which row a point falls in, or `None` outside `area` or past the last row. The list's own
    /// scroll is taken into account, so the answer is the row drawn there.
    pub fn row_at(&self, area: Rect, x: f32, y: f32) -> Option<usize> {
        if self.row_h <= 0.0 || !area.contains(x, y) {
            return None;
        }
        let at = (y - area.y + self.scroll) / self.row_h;
        if !at.is_finite() || at < 0.0 {
            return None;
        }
        let i = at.floor() as usize;
        (i < self.count).then_some(i)
    }

    /// The overlay scrollbar's track and thumb for `area`, or `None` when every row fits: the
    /// same scrollbar the panes draw, from the shared range of lengths.
    pub fn thumb(&self, area: Rect) -> Option<(Rect, Rect)> {
        scroll::scrollbar(
            area,
            self.content(),
            self.view,
            self.clamp_scroll(self.scroll),
        )
    }

    /// How many rows a page key moves: the whole rows the viewport holds, at least one, so a
    /// page key always moves on a list with rows whatever its height.
    pub fn page(&self) -> usize {
        if self.row_h <= 0.0 {
            return 1;
        }
        ((self.view / self.row_h).floor() as usize).max(1)
    }

    /// Where `nav` moves the cursor from `cursor`: never past the first row or the last, and with
    /// no cursor the forward keys start at the first row and the others at the last. `None` for
    /// an empty list. A cursor past the last row counts as the last one.
    pub fn nav(&self, cursor: Option<usize>, nav: Nav) -> Option<usize> {
        if self.count == 0 {
            return None;
        }
        let last = self.count - 1;
        let at = cursor.map(|c| c.min(last));
        Some(match nav {
            Nav::Down => at.map_or(0, |c| (c + 1).min(last)),
            Nav::Up => at.map_or(last, |c| c.saturating_sub(1)),
            Nav::Home => 0,
            Nav::End => last,
            Nav::PageDown => at.map_or(0, |c| (c + self.page()).min(last)),
            Nav::PageUp => at.map_or(last, |c| c.saturating_sub(self.page())),
        })
    }

    /// The offset that shows row `i` whole, moved only when the row is not whole already: one
    /// above the viewport goes to its top, one below it to its bottom. The answer stays within
    /// the list, so revealing the first row gives 0 on a list scrolled past its end.
    pub fn reveal(&self, i: usize) -> f32 {
        let top = i as f32 * self.row_h;
        let bottom = top + self.row_h;
        let scroll = self.clamp_scroll(self.scroll);
        let to = if top < scroll {
            top
        } else if bottom > scroll + self.view {
            bottom - self.view
        } else {
            scroll
        };
        self.clamp_scroll(to)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AREA: Rect = Rect::new(0.0, 200.0, 200.0, 100.0);

    #[test]
    fn the_rows_answer_the_same_at_the_top_in_the_middle_and_at_the_end() {
        // Ten rows of 25 px, 100 px of viewport: four rows at a time, and 150 px to scroll.
        let top = Rows::new(25.0, 100.0, 10);
        assert_eq!(
            (top.content(), top.max_scroll(), top.page()),
            (250.0, 150.0, 4)
        );
        assert_eq!(top.visible(), 0..4);
        assert_eq!(top.rect(AREA, 0), Rect::new(0.0, 200.0, 200.0, 25.0));
        assert_eq!(top.rect(AREA, 3).y, 275.0);
        assert_eq!(top.row_at(AREA, 5.0, 200.0), Some(0));
        assert_eq!(top.row_at(AREA, 5.0, 275.0), Some(3));
        assert_eq!(top.row_at(AREA, 5.0, 299.9), Some(3));
        assert_eq!(top.row_at(AREA, 5.0, 199.9), None, "above the area");
        assert_eq!(top.row_at(AREA, 5.0, 300.0), None, "below the area");
        assert_eq!(top.row_at(AREA, -1.0, 250.0), None, "left of the area");
        assert_eq!(top.reveal(3), 0.0, "it is whole already");
        assert_eq!(top.reveal(4), 25.0);
        assert_eq!(top.nav(None, Nav::Down), Some(0));
        assert_eq!(top.nav(None, Nav::PageDown), Some(0));
        assert_eq!(top.nav(None, Nav::Up), Some(9));
        assert_eq!(top.nav(None, Nav::PageUp), Some(9));
        assert_eq!(top.nav(Some(0), Nav::Up), Some(0), "the top holds");
        assert_eq!(top.nav(Some(0), Nav::Down), Some(1));
        assert_eq!(top.nav(Some(0), Nav::PageDown), Some(4));
        assert_eq!(top.nav(Some(0), Nav::End), Some(9));
        assert_eq!(top.thumb(AREA), scroll::scrollbar(AREA, 250.0, 100.0, 0.0));

        let middle = Rows {
            scroll: 75.0,
            ..top
        };
        assert_eq!(middle.visible(), 3..7);
        assert_eq!(
            middle.rect(AREA, 3).y,
            200.0,
            "row 3 is at the viewport's top"
        );
        assert_eq!(middle.row_at(AREA, 5.0, 200.0), Some(3));
        assert_eq!(middle.row_at(AREA, 5.0, 224.9), Some(3));
        assert_eq!(middle.row_at(AREA, 5.0, 225.0), Some(4));
        assert_eq!(middle.reveal(5), 75.0, "it is whole already");
        assert_eq!(middle.reveal(2), 50.0, "above the viewport: to its top");
        assert_eq!(middle.reveal(7), 100.0, "below it: to its bottom");
        assert_eq!(middle.nav(Some(4), Nav::Up), Some(3));
        assert_eq!(middle.nav(Some(4), Nav::Down), Some(5));
        assert_eq!(middle.nav(Some(4), Nav::PageUp), Some(0));
        assert_eq!(middle.nav(Some(4), Nav::PageDown), Some(8));
        assert_eq!(middle.nav(Some(4), Nav::Home), Some(0));
        assert_eq!(middle.nav(Some(4), Nav::End), Some(9));

        let end = Rows {
            scroll: 150.0,
            ..top
        };
        assert_eq!(end.visible(), 6..10);
        assert_eq!(
            end.rect(AREA, 9).bottom(),
            300.0,
            "the last row ends the area"
        );
        assert_eq!(end.row_at(AREA, 5.0, 299.9), Some(9));
        assert_eq!(end.reveal(9), 150.0, "it is whole already");
        assert_eq!(end.reveal(6), 150.0, "the first row shown is whole already");
        assert_eq!(end.reveal(4), 100.0);
        assert_eq!(end.reveal(0), 0.0);
        assert_eq!(end.nav(Some(9), Nav::Down), Some(9), "the end holds");
        assert_eq!(end.nav(Some(9), Nav::PageDown), Some(9));
        assert_eq!(end.nav(Some(9), Nav::PageUp), Some(5));
        // An offset past the end answers like the end; it is not dragged back by the question.
        let past = Rows {
            scroll: 900.0,
            ..top
        };
        assert_eq!(past.visible(), 6..10);
        assert_eq!(past.reveal(0), 0.0);
        assert_eq!(past.thumb(AREA), end.thumb(AREA));
    }

    #[test]
    fn an_empty_list_answers_nothing_and_a_short_one_shows_all_of_it() {
        let empty = Rows::new(25.0, 100.0, 0);
        assert_eq!(empty.visible(), 0..0);
        assert_eq!(empty.content(), 0.0);
        assert_eq!(empty.max_scroll(), 0.0);
        assert_eq!(empty.page(), 4, "a page is still a page");
        for nav in [
            Nav::Up,
            Nav::Down,
            Nav::Home,
            Nav::End,
            Nav::PageUp,
            Nav::PageDown,
        ] {
            assert_eq!(empty.nav(None, nav), None);
            assert_eq!(empty.nav(Some(0), nav), None);
        }
        assert_eq!(empty.reveal(0), 0.0);
        assert_eq!(empty.row_at(AREA, 5.0, 205.0), None);
        assert_eq!(empty.thumb(AREA), None);

        // Three rows in more room than they need: everything shows, nothing moves.
        let short = Rows::new(25.0, 400.0, 3);
        assert_eq!(short.visible(), 0..3);
        assert_eq!(short.max_scroll(), 0.0);
        assert_eq!(short.clamp_scroll(120.0), 0.0);
        assert_eq!(short.thumb(AREA), None, "no scrollbar when everything fits");
        for i in 0..3 {
            assert_eq!(short.reveal(i), 0.0);
            assert_eq!(short.rect(AREA, i).y, 200.0 + 25.0 * i as f32);
        }
        assert_eq!(short.row_at(AREA, 5.0, 274.9), Some(2));
        assert_eq!(short.row_at(AREA, 5.0, 275.0), None, "past the last row");
        assert_eq!(short.nav(Some(0), Nav::PageDown), Some(2));
        assert_eq!(short.nav(None, Nav::Up), Some(2));
        // A zero height would divide by nothing: it answers no rows rather than a panic.
        let flat = Rows::new(0.0, 100.0, 5);
        assert_eq!(flat.visible(), 0..0);
        assert_eq!(flat.row_at(AREA, 5.0, 205.0), None);
        assert_eq!(flat.page(), 1);
        assert_eq!(flat.nav(Some(0), Nav::PageDown), Some(1));
    }
}
