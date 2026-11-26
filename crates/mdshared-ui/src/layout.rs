//! The arithmetic of a row or a column of boxes: given an area and a list of items, each with a
//! fixed or a flexible size, a least and a most size, where each box goes. Padding inside the
//! area, gaps between the boxes, alignment along both axes and wrapping onto further lines are
//! settings of a [`Flex`]; the answer is one [`Rect`] per item, in the order of the items.
//!
//! It is the flexible box of CSS cut down to what a screen of controls needs. Free space along
//! the main axis goes to the items that grow, in proportion to their weights, and space that is
//! missing is taken from the items that shrink; an item that reaches its least or most size
//! stops there and the others share the rest, as CSS's "resolve flexible lengths" does (one
//! difference: CSS gives growers whose weights add up to less than 1 only that fraction of the
//! space; here they always share all of it). What is left over when nothing grows is placed by
//! [`Justify`]. Across the main axis an item without
//! a size of its own fills its line, and one with a size is placed by [`Align`].
//!
//! The edges come out on a pixel grid (a whole logical pixel by default; [`Flex::snap`] sets the
//! step, `1.0 / scale` for device pixels, 0 for none). Each edge is snapped from its exact place,
//! so neighbours share an edge, a run of boxes covers exactly the snapped length it was given,
//! and no pixel is lost or counted twice to rounding. (A [`Rect`] keeps a width, not a right
//! edge, so with a step that is not a whole number `right()` of one box and `x` of the next can
//! differ in the last bit of a float.)
//!
//! Every length that comes in is cleaned first: a negative one or one that is not finite is 0
//! (where an item's doc does not say otherwise), and one above [`MAX_LENGTH`] is held there, so
//! no arithmetic overflows and every rectangle that comes out is finite.
//!
//! The caller owns everything: it builds the items each frame from its own state (the text a
//! label measures, whether a panel is open), calls [`Flex::layout`] with the rectangle it has,
//! and draws into the rectangles it gets back. Nothing is kept between calls, so a resize needs
//! no telling, and a nested layout is a second call with one of the rectangles as its area.
//!
//! Deliberately not here: a tree of nested layouts, drawing, hit testing (the caller keeps the
//! rectangles and asks [`Rect::contains`]), percentages (the caller multiplies), reversed order,
//! text baselines, an aspect ratio, and grids with tracks across rows (`grid` lays out cells of
//! one size; `table` lays out columns with saved widths).

use crate::gfx::Rect;

/// The longest length the layout takes, in logical pixels; longer ones are held to it. Far past
/// any screen, and small enough that sums and products of a few of them stay finite.
pub const MAX_LENGTH: f32 = 1e9;

/// Which way the items run: the main axis. A row lays them out left to right, a column top to
/// bottom; the other axis is the cross axis.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[non_exhaustive]
pub enum Direction {
    /// Left to right; widths are main sizes, heights cross sizes.
    Row,
    /// Top to bottom; heights are main sizes, widths cross sizes.
    Column,
}

/// Where the space left along the main axis goes when no item grows to take it (none has a
/// weight, or every one that has is at its most size). When the items do not fit, they always
/// start at the start and run past the end, whatever this says, so the first of them is never
/// pushed out of the area.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
#[non_exhaustive]
pub enum Justify {
    /// Packed at the start (the left of a row, the top of a column). The default.
    #[default]
    Start,
    /// Packed in the middle, the space split evenly before and after.
    Center,
    /// Packed at the end.
    End,
    /// The first at the start, the last at the end, the space shared between them. One item is
    /// placed at the start.
    SpaceBetween,
    /// A half share before the first and after the last, a whole share between each two.
    SpaceAround,
    /// Equal shares before the first, between each two and after the last.
    SpaceEvenly,
}

/// Where an item with a cross size of its own sits across its line. An item without one fills
/// the line, whatever the alignment.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
#[non_exhaustive]
pub enum Align {
    /// At the top of a row's line, the left of a column's. The default.
    #[default]
    Start,
    /// In the middle.
    Center,
    /// At the bottom of a row's line, the right of a column's.
    End,
}

/// Space kept free inside the edges of an area, in logical pixels. Values that are negative or
/// not finite count as 0.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Insets {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl Insets {
    /// No padding.
    pub const ZERO: Insets = Insets::new(0.0, 0.0, 0.0, 0.0);

    /// The four sides in the order left, top, right, bottom.
    pub const fn new(left: f32, top: f32, right: f32, bottom: f32) -> Insets {
        Insets {
            left,
            top,
            right,
            bottom,
        }
    }

    /// The same on every side.
    pub const fn all(d: f32) -> Insets {
        Insets::new(d, d, d, d)
    }

    /// `x` on the left and right, `y` at the top and bottom.
    pub const fn xy(x: f32, y: f32) -> Insets {
        Insets::new(x, y, x, y)
    }

    /// The rectangle inside `r` once these insets are taken off its sides. Too much padding for
    /// the rectangle gives a size of 0 at the place the left and top padding reach, never a
    /// negative size. A width or height that is negative or not finite counts as 0; a place that
    /// is not finite puts the whole rectangle at the origin with no size (the padding is then
    /// taken from there).
    pub fn shrink(self, r: Rect) -> Rect {
        let r = clean_rect(r);
        let (l, t) = (clean(self.left), clean(self.top));
        let (rt, b) = (clean(self.right), clean(self.bottom));
        Rect::new(
            r.x + l,
            r.y + t,
            (r.w - l - rt).max(0.0),
            (r.h - t - b).max(0.0),
        )
    }
}

/// One box of a layout: its size along the main axis (a basis, a weight to grow by, a weight
/// to shrink by, a least and a most size) and, optionally, its size across it and its own
/// alignment. Built with [`Item::fixed`] or [`Item::flex`] and the builder methods; the fields
/// are private so that more can be added later.
///
/// The main size is found this way: the natural size, [`Item::natural`], is the basis held
/// between the least and the most size, and decides whether the line has room to spare or room
/// missing. When there is room to spare, the items that grow share it by their weights; when
/// room is missing, the items that shrink give it up in proportion to their weight times their
/// basis (so a wide item gives more than a narrow one). As in CSS, the sharing counts from the
/// basis, not from the natural size, so an item raised to its least size gets less than an equal
/// share on top of it. No item goes below its least size or above its most size; when the least
/// size is more than the most, the least size wins.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Item {
    basis: f32,
    grow: f32,
    shrink: f32,
    min: f32,
    max: f32,
    cross: Option<f32>,
    align: Option<Align>,
}

impl Item {
    /// An item `size` long along the main axis that neither grows nor shrinks (until
    /// [`grow`](Item::grow) or [`shrink`](Item::shrink) say otherwise). A size that is negative
    /// or not finite is 0.
    pub fn fixed(size: f32) -> Item {
        Item {
            basis: clean(size),
            grow: 0.0,
            shrink: 0.0,
            min: 0.0,
            max: f32::INFINITY,
            cross: None,
            align: None,
        }
    }

    /// An item with no length of its own that takes `weight` shares of the space the others
    /// leave: two items of weight 1 and 2 get a third and two thirds. It shrinks to its least
    /// size (0 unless [`min`](Item::min) says) when there is no space. A weight that is
    /// negative or not finite is 0, so the item stays at its least size.
    pub fn flex(weight: f32) -> Item {
        Item {
            grow: clean(weight),
            ..Item::fixed(0.0)
        }
    }

    /// The item grows by `weight` shares of the space to spare, from its basis. A weight that is
    /// negative or not finite is 0.
    pub fn grow(mut self, weight: f32) -> Item {
        self.grow = clean(weight);
        self
    }

    /// The item gives up room when the items do not fit, `weight` times its basis against the
    /// others' (1 is the usual weight). A weight that is negative or not finite is 0: the item
    /// keeps its basis.
    pub fn shrink(mut self, weight: f32) -> Item {
        self.shrink = clean(weight);
        self
    }

    /// The least main size: the item never comes out shorter. A value that is negative or not
    /// finite is 0.
    pub fn min(mut self, size: f32) -> Item {
        self.min = clean(size);
        self
    }

    /// The most main size: the item never comes out longer, unless its least size is more. A
    /// value that is negative and finite is 0; NaN and either infinity are no limit.
    pub fn max(mut self, size: f32) -> Item {
        self.max = if size.is_finite() {
            size.clamp(0.0, MAX_LENGTH)
        } else {
            f32::INFINITY
        };
        self
    }

    /// The item's size across the main axis (its height in a row, its width in a column). It is
    /// placed in its line by its alignment, and held to the line's extent when it is larger and
    /// the layout does not wrap. Without one the item fills its line. A value that is negative
    /// or not finite is 0.
    pub fn cross(mut self, size: f32) -> Item {
        self.cross = Some(clean(size));
        self
    }

    /// This item's own alignment across its line, over the layout's [`Flex::align`].
    pub fn align(mut self, align: Align) -> Item {
        self.align = Some(align);
        self
    }

    /// The main size before any growing or shrinking: the basis held between the least and the
    /// most size, the least winning when they disagree.
    pub fn natural(&self) -> f32 {
        self.basis.min(self.max).max(self.min)
    }

    fn bounds(&self) -> (f32, f32) {
        (self.min, self.max.max(self.min))
    }
}

/// The settings of a row or a column: its direction, padding, gaps, alignment along both axes,
/// wrapping and the pixel grid. Small and `Copy`; build one where it is used and call
/// [`layout`](Flex::layout).
///
/// ```
/// use mdshared_ui::gfx::Rect;
/// use mdshared_ui::layout::{Align, Flex, Insets, Item};
///
/// // A toolbar: an icon, a title that takes the rest, two buttons.
/// let bar = Flex::row().padding(Insets::xy(8.0, 4.0)).gap(6.0).align(Align::Center);
/// let items = [
///     Item::fixed(24.0).cross(24.0),
///     Item::flex(1.0).min(60.0),
///     Item::fixed(80.0),
///     Item::fixed(80.0),
/// ];
/// let r = bar.layout(Rect::new(0.0, 0.0, 400.0, 40.0), &items);
/// assert_eq!(r[0], Rect::new(8.0, 8.0, 24.0, 24.0));
/// assert_eq!(r[1], Rect::new(38.0, 4.0, 182.0, 32.0));
/// assert_eq!(r[3].right(), 392.0);
/// ```
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Flex {
    direction: Direction,
    padding: Insets,
    gap: f32,
    line_gap: f32,
    justify: Justify,
    align: Align,
    wrap: bool,
    snap: f32,
}

impl Flex {
    /// A layout in the given direction with no padding, no gaps, the items at the start, no
    /// wrapping, edges on whole logical pixels.
    pub fn new(direction: Direction) -> Flex {
        Flex {
            direction,
            padding: Insets::ZERO,
            gap: 0.0,
            line_gap: 0.0,
            justify: Justify::Start,
            align: Align::Start,
            wrap: false,
            snap: 1.0,
        }
    }

    /// Items left to right.
    pub fn row() -> Flex {
        Flex::new(Direction::Row)
    }

    /// Items top to bottom.
    pub fn column() -> Flex {
        Flex::new(Direction::Column)
    }

    /// Space kept free inside the area's edges.
    pub fn padding(mut self, padding: Insets) -> Flex {
        self.padding = padding;
        self
    }

    /// Space between two neighbours on one line. Negative or not finite is 0.
    pub fn gap(mut self, gap: f32) -> Flex {
        self.gap = clean(gap);
        self
    }

    /// Space between two lines when the layout wraps. Negative or not finite is 0.
    pub fn line_gap(mut self, gap: f32) -> Flex {
        self.line_gap = clean(gap);
        self
    }

    /// Where the space left along the main axis goes.
    pub fn justify(mut self, justify: Justify) -> Flex {
        self.justify = justify;
        self
    }

    /// Where items with a cross size sit across their line, unless an item says otherwise.
    pub fn align(mut self, align: Align) -> Flex {
        self.align = align;
        self
    }

    /// Whether items that do not fit on one line go on to the next. A line takes items while
    /// their natural sizes and the gaps fit; an item longer than a whole line gets a line of its
    /// own. Each line then shares its own space among its own items, and the lines are stacked
    /// from the start of the cross axis, `line_gap` apart. A line is as thick as the largest
    /// cross size of its items; a line where no item has a cross size takes an equal share of the
    /// cross space the other lines leave (and counts as 0 in [`cross_extent`](Flex::cross_extent)),
    /// so give items a cross size when the layout wraps.
    pub fn wrap(mut self, wrap: bool) -> Flex {
        self.wrap = wrap;
        self
    }

    /// The step of the grid the edges are snapped to, in logical pixels: 1 (the default) for
    /// whole logical pixels, `1.0 / scale` for device pixels, 0 to keep the exact edges. A step
    /// that is negative or not finite is 0. Positions are snapped in the window's coordinates,
    /// so the boxes of a nested layout fall on the same grid as their parent.
    pub fn snap(mut self, step: f32) -> Flex {
        self.snap = clean(step);
        self
    }

    /// The rectangles of `items` laid out in `area`, one for each item in their order. An empty
    /// list gives an empty vector. The area is cleaned as [`Insets::shrink`] cleans it: a size
    /// that is negative or not finite is 0, a place that is not finite empties the area. Items that do not fit, even shrunk to their least sizes, run past the end of the
    /// area; the caller clips. A rectangle never has a negative size.
    pub fn layout(&self, area: Rect, items: &[Item]) -> Vec<Rect> {
        let mut out = Vec::with_capacity(items.len());
        self.layout_into(area, items, &mut out);
        out
    }

    /// [`layout`](Flex::layout) into a vector the caller keeps, cleared first, so the output is
    /// not allocated again each frame (a few small working vectors still are).
    pub fn layout_into(&self, area: Rect, items: &[Item], out: &mut Vec<Rect>) {
        out.clear();
        if items.is_empty() {
            return;
        }
        let inner = self.padding.shrink(area);
        let (main0, cross0, main_len, cross_len) = self.split(inner);
        let lines = self.lines(main_len, items);
        // Cross extent of each line: the largest cross size among its items, or for a single
        // line that does not wrap, the whole inner extent.
        let mut extents: Vec<f32> = Vec::with_capacity(lines.len());
        if !self.wrap {
            extents.push(cross_len);
        } else {
            for line in &lines {
                extents.push(line_extent(&items[line.clone()]));
            }
            // Lines where no item has a cross size share what the sized lines leave.
            let bare = (lines.iter())
                .filter(|l| items[(*l).clone()].iter().all(|i| i.cross.is_none()))
                .count();
            if bare > 0 {
                let used: f32 = extents.iter().sum::<f32>()
                    + self.line_gap * (lines.len().saturating_sub(1)) as f32;
                let share = ((cross_len - used).max(0.0)) / bare as f32;
                for (e, line) in extents.iter_mut().zip(&lines) {
                    if items[line.clone()].iter().all(|i| i.cross.is_none()) {
                        *e = share;
                    }
                }
            }
        }
        let mut sizes = Vec::new();
        let mut cross_at = cross0;
        for (line, &extent) in lines.iter().zip(&extents) {
            let line_items = &items[line.clone()];
            resolve(main_len, self.gap, line_items, &mut sizes);
            let used = sizes.iter().sum::<f32>() + self.gap * (sizes.len() - 1) as f32;
            let (lead, between) = justify(self.justify, main_len - used, sizes.len());
            let mut at = main0 + lead;
            for (item, &size) in line_items.iter().zip(&sizes) {
                let (c, cl) = match item.cross {
                    None => (cross_at, extent),
                    Some(s) => {
                        let s = if self.wrap { s } else { s.min(extent) };
                        let free = extent - s;
                        let off = match item.align.unwrap_or(self.align) {
                            Align::Start => 0.0,
                            Align::Center => free / 2.0,
                            Align::End => free,
                        };
                        (cross_at + off, s)
                    }
                };
                out.push(self.rect(at, at + size, c, c + cl));
                at += size + self.gap + between;
            }
            cross_at += extent + self.line_gap;
        }
    }

    /// The main length the items want with nothing grown or shrunk: their natural sizes, the
    /// gaps between them and the padding at both ends of the main axis, all on one line. Below
    /// this the items shrink or run past the end; a scroll area sizes its content with it.
    pub fn natural(&self, items: &[Item]) -> f32 {
        let (p0, p1) = self.main_padding();
        let sum: f32 = items.iter().map(Item::natural).sum();
        sum + self.gap * items.len().saturating_sub(1) as f32 + p0 + p1
    }

    /// The cross length the items need when laid out along an area `main` long (the area's
    /// whole main length, padding included; an infinity is no limit, so a wrapping layout makes
    /// one line; NaN or a negative length is 0): with wrapping, the lines' extents, the gaps between
    /// lines and the cross padding; without, the largest cross size and the cross padding.
    /// Items without a cross size count as 0 here, as they fill whatever they are given.
    pub fn cross_extent(&self, main: f32, items: &[Item]) -> f32 {
        let (p0, p1) = self.cross_padding();
        if items.is_empty() {
            return p0 + p1;
        }
        let (m0, m1) = self.main_padding();
        let main_len = if main == f32::INFINITY {
            f32::INFINITY
        } else {
            (clean(main) - m0 - m1).max(0.0)
        };
        let lines = self.lines(main_len, items);
        let sum: f32 = lines.iter().map(|l| line_extent(&items[l.clone()])).sum();
        sum + self.line_gap * (lines.len() - 1) as f32 + p0 + p1
    }

    /// Splits the items into lines: one line holding all of them when the layout does not wrap.
    fn lines(&self, main_len: f32, items: &[Item]) -> Vec<std::ops::Range<usize>> {
        if !self.wrap {
            return std::iter::once(0..items.len()).collect();
        }
        let mut lines = Vec::new();
        let mut start = 0;
        let mut used = 0.0f32;
        for (i, item) in items.iter().enumerate() {
            let n = item.natural();
            // A little slack, so a line exactly as long as `natural` said is not broken by the
            // rounding of taking the padding off again.
            if i > start && used + self.gap + n > main_len + main_len.abs() * 1e-6 + 1e-3 {
                lines.push(start..i);
                start = i;
                used = n;
            } else if i == start {
                used = n;
            } else {
                used += self.gap + n;
            }
        }
        lines.push(start..items.len());
        lines
    }

    /// (main start, cross start, main length, cross length) of a rectangle.
    fn split(&self, r: Rect) -> (f32, f32, f32, f32) {
        match self.direction {
            Direction::Row => (r.x, r.y, r.w, r.h),
            Direction::Column => (r.y, r.x, r.h, r.w),
        }
    }

    fn main_padding(&self) -> (f32, f32) {
        let p = &self.padding;
        match self.direction {
            Direction::Row => (clean(p.left), clean(p.right)),
            Direction::Column => (clean(p.top), clean(p.bottom)),
        }
    }

    fn cross_padding(&self) -> (f32, f32) {
        let p = &self.padding;
        match self.direction {
            Direction::Row => (clean(p.top), clean(p.bottom)),
            Direction::Column => (clean(p.left), clean(p.right)),
        }
    }

    /// A rectangle from main and cross edges, each snapped on its own.
    fn rect(&self, m0: f32, m1: f32, c0: f32, c1: f32) -> Rect {
        let (m0, m1) = (self.on_grid(m0), self.on_grid(m1));
        let (c0, c1) = (self.on_grid(c0), self.on_grid(c1));
        let (mw, cw) = ((m1 - m0).max(0.0), (c1 - c0).max(0.0));
        match self.direction {
            Direction::Row => Rect::new(m0, c0, mw, cw),
            Direction::Column => Rect::new(c0, m0, cw, mw),
        }
    }

    fn on_grid(&self, v: f32) -> f32 {
        let k = (v / self.snap).round();
        if self.snap > 0.0 && k.is_finite() {
            k * self.snap
        } else {
            v
        }
    }
}

/// The main sizes of one line's items, written into `out`: the flexible lengths resolved as
/// CSS does it. Items are frozen at their natural size when they cannot take part (no weight in
/// the direction needed); then, round after round, the free space is shared among the rest by
/// weight, each is held to its bounds, and when the holding moved the total, the items that hit
/// a bound in the direction of the move are frozen there. Each round freezes at least one item,
/// so it ends within as many rounds as there are items.
fn resolve(main_len: f32, gap: f32, items: &[Item], out: &mut Vec<f32>) {
    out.clear();
    out.extend(items.iter().map(Item::natural));
    let avail = main_len - gap * items.len().saturating_sub(1) as f32;
    let growing = avail - out.iter().sum::<f32>() > 0.0;
    let weight = |i: &Item| {
        if growing { i.grow } else { i.shrink * i.basis }
    };
    let mut frozen: Vec<bool> = (items.iter().zip(out.iter()))
        .map(|(i, &n)| weight(i) <= 0.0 || (growing && i.basis > n) || (!growing && i.basis < n))
        .collect();
    for _ in 0..=items.len() {
        if frozen.iter().all(|f| *f) {
            break;
        }
        let fixed: f32 = (out.iter().zip(&frozen))
            .filter(|(_, f)| **f)
            .map(|(s, _)| *s)
            .sum();
        let bases: f32 = (items.iter().zip(&frozen))
            .filter(|(_, f)| !**f)
            .map(|(i, _)| i.basis)
            .sum();
        let free = avail - fixed - bases;
        let total: f32 = (items.iter().zip(&frozen))
            .filter(|(_, f)| !**f)
            .map(|(i, _)| weight(i))
            .sum();
        let mut violation = 0.0f32;
        let mut targets = vec![0.0f32; items.len()];
        for (k, item) in items.iter().enumerate() {
            if frozen[k] {
                continue;
            }
            let target = item.basis + free * weight(item) / total;
            let (lo, hi) = item.bounds();
            let held = target.min(hi).max(lo);
            violation += held - target;
            targets[k] = held;
            out[k] = held;
        }
        if violation.abs() <= 1e-4 {
            break;
        }
        for (k, item) in items.iter().enumerate() {
            if frozen[k] {
                continue;
            }
            let (lo, hi) = item.bounds();
            let at_min = targets[k] <= lo && violation > 0.0;
            let at_max = targets[k] >= hi && violation < 0.0;
            if at_min || at_max {
                frozen[k] = true;
            }
        }
    }
}

/// (space before the first item, extra space between two) for what is left on a line. Space
/// that is missing (a negative `free`) puts the items at the start.
fn justify(justify: Justify, free: f32, n: usize) -> (f32, f32) {
    if free <= 0.0 || n == 0 {
        return (0.0, 0.0);
    }
    let n = n as f32;
    match justify {
        Justify::Start => (0.0, 0.0),
        Justify::Center => (free / 2.0, 0.0),
        Justify::End => (free, 0.0),
        Justify::SpaceBetween if n > 1.0 => (0.0, free / (n - 1.0)),
        Justify::SpaceBetween => (0.0, 0.0),
        Justify::SpaceAround => (free / n / 2.0, free / n),
        Justify::SpaceEvenly => (free / (n + 1.0), free / (n + 1.0)),
    }
}

/// The largest cross size among a line's items; 0 when none has one.
fn line_extent(items: &[Item]) -> f32 {
    items.iter().filter_map(|i| i.cross).fold(0.0, f32::max)
}

/// A length that is finite, not negative and at most [`MAX_LENGTH`], or 0.
fn clean(v: f32) -> f32 {
    if v.is_finite() {
        v.clamp(0.0, MAX_LENGTH)
    } else {
        0.0
    }
}

/// A rectangle whose place is finite and whose size is finite and not negative; an empty one at
/// the origin when its place is not finite.
fn clean_rect(r: Rect) -> Rect {
    if !(r.x.is_finite() && r.y.is_finite()) {
        return Rect::new(0.0, 0.0, 0.0, 0.0);
    }
    let place = |v: f32| v.clamp(-MAX_LENGTH, MAX_LENGTH);
    Rect::new(place(r.x), place(r.y), clean(r.w), clean(r.h))
}

#[cfg(test)]
#[path = "layout_tests.rs"]
mod tests;
