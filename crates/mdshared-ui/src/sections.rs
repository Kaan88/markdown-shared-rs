//! The arithmetic of a list whose rows differ in height and fall in groups: a file manager's
//! "grouped by type" view, a library's albums with their tracks, a chat's days. It answers the
//! questions [`rows::Rows`](crate::rows::Rows) answers for rows of one height (which entries
//! the viewport shows, where one is, which one a point falls in, the scrollbar, where a key
//! moves the cursor, the offset that reveals an entry), and three more: the heading that sticks
//! to the top while its group scrolls under it, collapsing a group, and rows measured after
//! they were first laid out with an estimate.
//!
//! Headings and rows share one index space, in order: a group's heading (when it has one),
//! then its rows, then the next group's. An index stays the same whether its group is
//! collapsed or not, so a [`select::Selection`](crate::select::Selection) over these indices
//! needs nothing new; [`Sections::entry`] says what an index is. A collapsed group's rows are
//! there with no height: nothing finds them, and the cursor passes over them.
//!
//! Heights are kept in a Fenwick tree of `f64`, so an offset is exact on lists of millions of
//! rows and measuring one row, or finding the row at an offset, costs a logarithm. A row is
//! laid out at the estimate until [`Sections::set_height`] says what it measured; a row above
//! the top of the viewport that changes height moves the offset with it, so what is on screen
//! stays where it is.
//!
//! It draws nothing and knows nothing of an event loop. Deliberately not here: the drawing of
//! a heading and its chevron (the caller's, at [`Sections::rect`] and [`Sections::sticky`]);
//! a rubber band across groups; columns; animation of a group opening (the caller can animate
//! [`Sections::rect`]'s answers).

use std::ops::Range;

use crate::gfx::Rect;
use crate::rows::Nav;
use crate::scroll;

/// Sums of heights with O(log n) update, prefix and search.
#[derive(Clone, Debug, Default)]
struct Fenwick {
    /// One-based.
    tree: Vec<f64>,
}

impl Fenwick {
    fn len(&self) -> usize {
        self.tree.len().saturating_sub(1)
    }

    /// Adds a value at the end: the new node covers the values from just past its low bit to
    /// itself, which are the new value and the sum of the last of the ones before.
    fn push(&mut self, v: f64) {
        if self.tree.is_empty() {
            self.tree.push(0.0);
        }
        let n = self.len();
        let i = n + 1;
        let node = v + self.prefix(n) - self.prefix(i - i.isolate_lowest_one());
        self.tree.push(node);
    }

    fn add(&mut self, i: usize, delta: f64) {
        let mut i = i + 1;
        while i < self.tree.len() {
            self.tree[i] += delta;
            i += i.isolate_lowest_one();
        }
    }

    /// The sum of the first `i` values.
    fn prefix(&self, i: usize) -> f64 {
        let mut i = i.min(self.len());
        let mut sum = 0.0;
        while i > 0 {
            sum += self.tree[i];
            i -= i.isolate_lowest_one();
        }
        sum
    }

    /// The index whose span holds `y`: the first `i` whose prefix through it passes `y`. Empty
    /// spans are passed over. `len()` when `y` is at or past the total.
    fn search(&self, y: f64) -> usize {
        let n = self.len();
        let mut pos = 0;
        let mut rest = y;
        let mut step = n.checked_ilog2().map_or(0, |b| 1usize << b);
        while step > 0 {
            if pos + step <= n && self.tree[pos + step] <= rest {
                pos += step;
                rest -= self.tree[pos];
            }
            step >>= 1;
        }
        pos
    }
}

/// What an index of a [`Sections`] stands for.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Entry {
    /// The heading of a group.
    Heading(usize),
    /// Row `row` of group `group`, counted within the group.
    Row { group: usize, row: usize },
}

#[derive(Clone, Copy, PartialEq, Debug)]
struct Group {
    /// The index of the group's first entry: its heading, or its first row.
    start: usize,
    heading: bool,
    rows: usize,
    collapsed: bool,
}

impl Group {
    fn first_row(&self) -> usize {
        self.start + usize::from(self.heading)
    }

    fn end(&self) -> usize {
        self.first_row() + self.rows
    }
}

/// Where the list was scrolled to, held by the entry at the top of the viewport and how far
/// into it: what [`Sections::anchor`] takes before the list changes and
/// [`Sections::restore`] puts back after.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Anchor {
    pub entry: usize,
    pub offset: f64,
}

/// A list of groups of rows, each group with an optional heading, seen through a viewport
/// `view` tall and scrolled by `scroll`. See the [module](self).
#[derive(Clone, Debug)]
pub struct Sections {
    /// The height of the viewport.
    pub view: f32,
    /// The offset from the top of the first entry. Answers clamp it, so an offset left past
    /// either end does not change what they say.
    pub scroll: f64,
    /// Whether a group's heading sticks to the top of the viewport while its rows scroll
    /// under it. On by default.
    pub sticky_headings: bool,
    /// Whether the cursor stops on headings ([`nav`](Self::nav)); off by default, so the
    /// arrow keys go from row to row across groups.
    pub headings_take_cursor: bool,
    estimate: f32,
    groups: Vec<Group>,
    /// Each entry's own height, measured or estimated, kept while its group is collapsed.
    heights: Vec<f32>,
    measured: Vec<bool>,
    /// The heights as laid out: 0 for a collapsed group's rows.
    laid: Fenwick,
}

impl Sections {
    /// An empty list at its top, whose rows are laid out `estimate` tall until measured.
    pub fn new(view: f32, estimate: f32) -> Sections {
        Sections {
            view,
            scroll: 0.0,
            sticky_headings: true,
            headings_take_cursor: false,
            estimate: if estimate.is_finite() {
                estimate.max(0.0)
            } else {
                0.0
            },
            groups: Vec::new(),
            heights: Vec::new(),
            measured: Vec::new(),
            laid: Fenwick::default(),
        }
    }

    /// Adds a group of `rows` rows at the end, with a heading `heading` tall (none for `None`),
    /// open, and answers its number. A list with no headings is one group added with `None`.
    pub fn add_group(&mut self, rows: usize, heading: Option<f32>) -> usize {
        let start = self.heights.len();
        if let Some(h) = heading {
            let h = clean(h);
            self.heights.push(h);
            self.measured.push(true);
            self.laid.push(f64::from(h));
        }
        for _ in 0..rows {
            self.heights.push(self.estimate);
            self.measured.push(false);
            self.laid.push(f64::from(self.estimate));
        }
        self.groups.push(Group {
            start,
            heading: heading.is_some(),
            rows,
            collapsed: false,
        });
        self.groups.len() - 1
    }

    /// Removes every group.
    pub fn clear(&mut self) {
        self.groups.clear();
        self.heights.clear();
        self.measured.clear();
        self.laid = Fenwick::default();
        self.scroll = 0.0;
    }

    /// How many entries there are, headings and rows, collapsed or not.
    pub fn len(&self) -> usize {
        self.heights.len()
    }

    /// Whether there are no entries.
    pub fn is_empty(&self) -> bool {
        self.heights.is_empty()
    }

    /// How many groups there are.
    pub fn groups(&self) -> usize {
        self.groups.len()
    }

    /// The group an index falls in.
    fn group_of(&self, i: usize) -> Option<usize> {
        if i >= self.len() {
            return None;
        }
        Some(self.groups.partition_point(|g| g.start <= i) - 1)
    }

    /// What index `i` is, or `None` past the last entry.
    pub fn entry(&self, i: usize) -> Option<Entry> {
        let gi = self.group_of(i)?;
        let g = &self.groups[gi];
        Some(if g.heading && i == g.start {
            Entry::Heading(gi)
        } else {
            Entry::Row {
                group: gi,
                row: i - g.first_row(),
            }
        })
    }

    /// The index of an entry, or `None` for one that is not there.
    pub fn index(&self, entry: Entry) -> Option<usize> {
        match entry {
            Entry::Heading(gi) => self.groups.get(gi).filter(|g| g.heading).map(|g| g.start),
            Entry::Row { group, row } => self
                .groups
                .get(group)
                .filter(|g| row < g.rows)
                .map(|g| g.first_row() + row),
        }
    }

    /// The rows of group `group`, as indices; empty for no such group.
    pub fn rows_of(&self, group: usize) -> Range<usize> {
        self.groups
            .get(group)
            .map_or(0..0, |g| g.first_row()..g.end())
    }

    /// Whether entry `i` is laid out: a heading, or a row of an open group.
    pub fn is_shown(&self, i: usize) -> bool {
        self.group_of(i).is_some_and(|gi| {
            let g = &self.groups[gi];
            !g.collapsed || (g.heading && i == g.start)
        })
    }

    /// Entry `i`'s height: as laid out, so 0 for a row of a collapsed group.
    pub fn height(&self, i: usize) -> f32 {
        if self.is_shown(i) {
            self.heights[i]
        } else {
            0.0
        }
    }

    /// Whether row `i` has been measured, rather than laid out at the estimate. Headings count
    /// as measured.
    pub fn is_measured(&self, i: usize) -> bool {
        self.measured.get(i).copied().unwrap_or(false)
    }

    /// Says what entry `i` measured. A row that lies wholly above the top of the viewport moves
    /// the offset by its change, so the entries on screen stay where they are. Returns whether
    /// the laid-out height changed (a collapsed row keeps the height for when it opens).
    pub fn set_height(&mut self, i: usize, height: f32) -> bool {
        if i >= self.len() {
            return false;
        }
        let height = clean(height);
        self.measured[i] = true;
        let old = self.heights[i];
        if old == height {
            return false;
        }
        self.heights[i] = height;
        if !self.is_shown(i) {
            return false;
        }
        let delta = f64::from(height) - f64::from(old);
        let above = self.laid.prefix(i + 1) <= self.clamp_scroll(self.scroll);
        self.laid.add(i, delta);
        if above {
            self.scroll += delta;
        }
        true
    }

    /// Whether group `group` is collapsed.
    pub fn is_collapsed(&self, group: usize) -> bool {
        self.groups.get(group).is_some_and(|g| g.collapsed)
    }

    /// Collapses or opens group `group`; returns whether that changed anything. A group whose
    /// heading has scrolled above the viewport (its heading is the sticky one) is scrolled back
    /// to its heading when it collapses, so the heading clicked is still under the pointer.
    pub fn set_collapsed(&mut self, group: usize, collapsed: bool) -> bool {
        let Some(&g) = self.groups.get(group) else {
            return false;
        };
        if g.collapsed == collapsed {
            return false;
        }
        let heading_top = self.top(g.start);
        self.groups[group].collapsed = collapsed;
        for i in g.first_row()..g.end() {
            let h = f64::from(self.heights[i]);
            self.laid.add(i, if collapsed { -h } else { h });
        }
        if collapsed && g.heading && heading_top < self.scroll {
            self.scroll = heading_top;
        }
        self.scroll = self.clamp_scroll(self.scroll);
        true
    }

    /// Collapses an open group or opens a collapsed one.
    pub fn toggle(&mut self, group: usize) -> bool {
        let collapsed = self.is_collapsed(group);
        self.set_collapsed(group, !collapsed)
    }

    /// The offset of entry `i`'s top from the top of the list. Past the last entry, the end.
    pub fn top(&self, i: usize) -> f64 {
        self.laid.prefix(i)
    }

    /// The height of the whole list as laid out.
    pub fn content(&self) -> f64 {
        self.laid.prefix(self.len())
    }

    /// The furthest the list can be scrolled; 0 when it fits.
    pub fn max_scroll(&self) -> f64 {
        (self.content() - f64::from(self.view.max(0.0))).max(0.0)
    }

    /// `scroll` kept between the top of the list and [`max_scroll`](Self::max_scroll).
    pub fn clamp_scroll(&self, scroll: f64) -> f64 {
        if scroll.is_nan() {
            return 0.0;
        }
        scroll.clamp(0.0, self.max_scroll())
    }

    /// The entry laid out at offset `y` from the top of the list, or `None` past the end.
    fn at_offset(&self, y: f64) -> Option<usize> {
        if y.is_nan() || y < 0.0 {
            return None;
        }
        let i = self.laid.search(y);
        (i < self.len()).then_some(i)
    }

    /// The entries the viewport touches, in order, without the rows of collapsed groups. A
    /// sticky heading is not among them unless it is in its place too; ask
    /// [`sticky`](Self::sticky).
    pub fn visible(&self) -> Vec<usize> {
        if self.view <= 0.0 || self.is_empty() {
            return Vec::new();
        }
        let s = self.clamp_scroll(self.scroll);
        let Some(first) = self.at_offset(s) else {
            return Vec::new();
        };
        let bottom = s + f64::from(self.view);
        let mut out = Vec::new();
        let mut i = Some(first);
        while let Some(at) = i {
            if self.top(at) >= bottom {
                break;
            }
            out.push(at);
            i = self.next_shown(at);
        }
        out
    }

    /// The next entry after `i` that is laid out.
    fn next_shown(&self, i: usize) -> Option<usize> {
        let mut j = i + 1;
        while j < self.len() {
            let gi = self.group_of(j)?;
            let g = self.groups[gi];
            if g.collapsed && j >= g.first_row() {
                j = g.end();
                continue;
            }
            return Some(j);
        }
        None
    }

    /// The entry before `i` that is laid out.
    fn previous_shown(&self, i: usize) -> Option<usize> {
        let mut j = i.checked_sub(1)?;
        loop {
            let gi = self.group_of(j)?;
            let g = self.groups[gi];
            if g.collapsed && j >= g.first_row() {
                if g.heading {
                    return Some(g.start);
                }
                j = g.start.checked_sub(1)?;
                continue;
            }
            return Some(j);
        }
    }

    /// Where entry `i` is in `area` at the current offset: as tall as its laid-out height and
    /// as wide as the area, on screen or not. The caller asks [`visible`](Self::visible) first.
    pub fn rect(&self, area: Rect, i: usize) -> Rect {
        let s = self.clamp_scroll(self.scroll);
        Rect::new(
            area.x,
            area.y + (self.top(i) - s) as f32,
            area.w,
            self.height(i),
        )
    }

    /// The heading that sticks to the top of `area`, and where to draw it, or `None` when no
    /// heading is above the viewport (or headings do not stick). It is the heading of the group
    /// at the top of the viewport, at the top of the area, pushed up by the next group's heading
    /// as that one arrives. The caller draws it after the entries, over them.
    pub fn sticky(&self, area: Rect) -> Option<(usize, Rect)> {
        if !self.sticky_headings {
            return None;
        }
        let s = self.clamp_scroll(self.scroll);
        let gi = self.group_of(self.at_offset(s)?)?;
        let g = self.groups[gi];
        if !g.heading || self.top(g.start) >= s {
            return None;
        }
        let h = f64::from(self.heights[g.start]);
        let mut y = 0.0;
        if let Some(next) = self.groups.get(gi + 1) {
            y = (self.top(next.start) - s - h).min(0.0);
        }
        Some((
            g.start,
            Rect::new(area.x, area.y + y as f32, area.w, h as f32),
        ))
    }

    /// Which entry a point falls in, or `None` outside `area` or past the last entry. A point
    /// on the sticky heading is on that heading.
    pub fn entry_at(&self, area: Rect, x: f32, y: f32) -> Option<usize> {
        if !area.contains(x, y) {
            return None;
        }
        if let Some((h, r)) = self.sticky(area)
            && r.contains(x, y)
        {
            return Some(h);
        }
        let s = self.clamp_scroll(self.scroll);
        self.at_offset(f64::from(y - area.y) + s)
    }

    /// The overlay scrollbar's track and thumb for `area`, or `None` when the list fits.
    pub fn thumb(&self, area: Rect) -> Option<(Rect, Rect)> {
        scroll::scrollbar(
            area,
            self.content() as f32,
            self.view,
            self.clamp_scroll(self.scroll) as f32,
        )
    }

    /// Whether the cursor may rest on entry `i`.
    fn takes_cursor(&self, i: usize) -> bool {
        self.is_shown(i)
            && (self.headings_take_cursor || !matches!(self.entry(i), Some(Entry::Heading(_))))
    }

    fn next_stop(&self, i: Option<usize>) -> Option<usize> {
        let mut j = match i {
            Some(i) => self.next_shown(i),
            None => (!self.is_empty()).then_some(0),
        };
        while let Some(at) = j {
            if self.takes_cursor(at) {
                return Some(at);
            }
            j = self.next_shown(at);
        }
        None
    }

    fn previous_stop(&self, i: Option<usize>) -> Option<usize> {
        let mut j = match i {
            Some(i) => self.previous_shown(i),
            None => self.len().checked_sub(1),
        };
        while let Some(at) = j {
            if self.takes_cursor(at) {
                return Some(at);
            }
            j = self.previous_shown(at);
        }
        None
    }

    /// Where `nav` moves the cursor from `cursor`, over the entries that take it (rows of open
    /// groups, and headings when [`headings_take_cursor`](Self::headings_take_cursor)). With no
    /// cursor the forward keys start at the first and the others at the last; at an end the
    /// cursor stays. A page moves by the viewport's height. `None` when nothing takes the
    /// cursor.
    pub fn nav(&self, cursor: Option<usize>, nav: Nav) -> Option<usize> {
        let first = self.next_stop(None)?;
        let last = self.previous_stop(None)?;
        let at = cursor.filter(|&c| c < self.len());
        Some(match (nav, at) {
            (Nav::Home, _) => first,
            (Nav::End, _) => last,
            (Nav::Down | Nav::PageDown, None) => first,
            (Nav::Up | Nav::PageUp, None) => last,
            (Nav::Down, Some(c)) => self.next_stop(Some(c)).unwrap_or(last.max(c)),
            (Nav::Up, Some(c)) => self.previous_stop(Some(c)).unwrap_or(first.min(c)),
            (Nav::PageDown, Some(c)) => {
                let y = self.top(c) + f64::from(self.view.max(0.0));
                let target = self.at_offset(y).unwrap_or(last);
                let stop = if self.takes_cursor(target) {
                    Some(target)
                } else {
                    self.next_stop(Some(target))
                };
                stop.filter(|&t| t > c)
                    .or_else(|| self.next_stop(Some(c)))
                    .unwrap_or(last.max(c))
            }
            (Nav::PageUp, Some(c)) => {
                let y = (self.top(c) - f64::from(self.view.max(0.0))).max(0.0);
                let target = self.at_offset(y).unwrap_or(first);
                let stop = if self.takes_cursor(target) {
                    Some(target)
                } else {
                    self.next_stop(Some(target))
                };
                stop.filter(|&t| t < c)
                    .or_else(|| self.previous_stop(Some(c)))
                    .unwrap_or(first.min(c))
            }
        })
    }

    /// The offset that shows entry `i` whole, moved only when it is not: one above goes to the
    /// top (below its group's sticky heading, which would cover it), one below to the bottom.
    /// Within the list.
    pub fn reveal(&self, i: usize) -> f64 {
        let s = self.clamp_scroll(self.scroll);
        if i >= self.len() {
            return s;
        }
        let top = self.top(i);
        let bottom = top + f64::from(self.height(i));
        let cover = match self.entry(i) {
            Some(Entry::Row { group, .. }) if self.sticky_headings => {
                let g = self.groups[group];
                if g.heading {
                    f64::from(self.heights[g.start])
                } else {
                    0.0
                }
            }
            _ => 0.0,
        };
        let to = if top - cover < s {
            top - cover
        } else if bottom > s + f64::from(self.view) {
            bottom - f64::from(self.view)
        } else {
            s
        };
        self.clamp_scroll(to)
    }

    /// The entry at the top of the viewport and how far into it the viewport begins: take it
    /// before groups are added or rows re-measured in bulk, and [`restore`](Self::restore) it
    /// after, so the view does not move.
    pub fn anchor(&self) -> Option<Anchor> {
        let s = self.clamp_scroll(self.scroll);
        let entry = self.at_offset(s)?;
        Some(Anchor {
            entry,
            offset: s - self.top(entry),
        })
    }

    /// The offset that puts `anchor` back at the top of the viewport, within the list; sets
    /// [`scroll`](Self::scroll) to it and answers it.
    pub fn restore(&mut self, anchor: Anchor) -> f64 {
        let to = self.top(anchor.entry.min(self.len())) + anchor.offset.max(0.0);
        self.scroll = self.clamp_scroll(to);
        self.scroll
    }
}

/// A height that is a number and not negative.
fn clean(h: f32) -> f32 {
    if h.is_finite() { h.max(0.0) } else { 0.0 }
}

#[cfg(test)]
#[path = "sections_tests.rs"]
mod tests;
