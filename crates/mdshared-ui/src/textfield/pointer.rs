//! Mouse selection in a single-line field. The caller owns focus, pointer capture, the clock and
//! wake-ups. Hit testing uses the same caret measurements as drawing, without needing a bitmap.
//! Deliberately not here: opening or drawing a popup, or controlling the platform input method.

use std::time::{Duration, Instant};

use super::{TextField, draw::reveal_scroll, line::Line};
use crate::{
    clicks::{ClickCounter, click_slop, within},
    cursor::Cursor,
    gfx::{Face, Gfx, Rect},
    keys::Mods,
    platform,
};

/// Where and how the field is drawn. Use the text rectangle passed to [`TextField::draw`], not
/// its surrounding box, and the painter's scale. All coordinates and the font size are logical.
#[derive(Clone, Copy, Debug)]
pub struct FieldHit {
    pub area: Rect,
    pub face: Face,
    pub size: f32,
    pub scale: f32,
}

impl FieldHit {
    fn valid(self) -> bool {
        let r = self.area;
        [
            r.x,
            r.y,
            r.w,
            r.h,
            r.right(),
            r.bottom(),
            self.size * self.scale,
        ]
        .into_iter()
        .all(f32::is_finite)
            && r.w > 0.0
            && r.h > 0.0
            && self.size > 0.0
            && self.scale > 0.0
    }

    /// The I-beam inside the text rectangle, no cursor request elsewhere.
    pub fn cursor(self, at: (f32, f32)) -> Option<Cursor> {
        (self.valid() && self.area.contains(at.0, at.1)).then_some(Cursor::Text)
    }

    fn line(self, shown: &str, gfx: &mut Gfx) -> Line {
        let layout = gfx.field_layout(self.face, self.size, shown, self.scale, [0; 3]);
        Line::new(&layout, self.scale)
    }
}

const SCROLL_INTERVAL: Duration = Duration::from_millis(16);
const MAX_STEP: f32 = 24.0;

#[derive(Debug)]
struct Held {
    start: (f32, f32),
    at: (f32, f32),
    moved: bool,
    base: Option<(usize, usize)>,
}

/// Mouse input for one [`TextField`]. Route the primary button and pointer moves here and give
/// the field focus when a press is accepted. Pass other presses and lost focus to [`Self::cancel`].
/// The caller continues to deliver move and release events after the pointer leaves the field.
#[derive(Debug)]
pub struct Pointer {
    clicks: ClickCounter,
    interval: Duration,
    slop: (f32, f32),
    held: Option<Held>,
    deadline: Option<Instant>,
}

impl Pointer {
    /// Uses the platform's double-click settings (or its fallback) at the given device scale.
    /// Construct again when the scale or settings change.
    pub fn system(scale: f32) -> Self {
        Self::new(
            platform::double_click_time(),
            click_slop(platform::double_click_size(), scale),
        )
    }

    /// Uses the given double-click time and allowed distance on each axis, in logical pixels.
    /// Negative or non-finite distances are treated as zero.
    pub fn new(interval: Duration, slop: (f32, f32)) -> Self {
        let sane = |v: f32| if v.is_finite() { v.max(0.0) } else { 0.0 };
        Self {
            clicks: ClickCounter::default(),
            interval,
            slop: (sane(slop.0), sane(slop.1)),
            held: None,
            deadline: None,
        }
    }

    /// Handles a primary press inside the field and returns true when accepted. Shift extends
    /// from the old anchor; otherwise a single click places the caret, a double click selects a
    /// word and a triple click selects everything. The caller should end any native IME
    /// composition before routing the press; this drops any remaining uncommitted preedit.
    pub fn press(
        &mut self,
        field: &mut TextField,
        gfx: &mut Gfx,
        hit: FieldHit,
        at: (f32, f32),
        mods: Mods,
        now: Instant,
    ) -> bool {
        if hit.cursor(at).is_none() {
            self.cancel();
            return false;
        }
        self.release();
        let count = self.clicks.press(now, at, (), self.interval, self.slop);
        if mods.shift {
            self.clicks.reset();
        }
        field.set_preedit(None);
        let shown = field.display();
        let line = hit.line(&shown, gfx);
        let scroll = field
            .scroll
            .clamp(0.0, (line.width + 1.0 - hit.area.w).max(0.0));
        let x = at.0 - snapped_origin(hit, scroll);
        let word = (count == 2 && !mods.shift && !field.secret).then(|| line.word_at(&shown, x));
        field.scroll = scroll;
        let base = if mods.shift || count == 1 {
            field.move_to_stop(line.hit(x), mods.shift);
            None
        } else {
            if let Some(word) = word {
                field.anchor = word.start;
                field.move_to_stop(
                    super::line::Stop {
                        byte: word.end,
                        affinity: parley::Affinity::Upstream,
                    },
                    true,
                );
            } else {
                field.select_all();
            }
            Some(field.sel_range())
        };
        reveal(field, &line, hit.area.w);
        self.held = Some(Held {
            start: at,
            at,
            moved: false,
            base,
        });
        true
    }

    /// Extends the selection while the primary button is held; returns whether to redraw.
    /// Outside the left or right edge it also starts horizontal auto-scroll, if there is more
    /// text. Pass the returned [`Self::deadline`] to the event loop to keep scrolling at rest.
    pub fn pointer_move(
        &mut self,
        field: &mut TextField,
        gfx: &mut Gfx,
        hit: FieldHit,
        at: (f32, f32),
        now: Instant,
    ) -> bool {
        let Some(held) = &mut self.held else {
            return false;
        };
        if !hit.valid() || !at.0.is_finite() || !at.1.is_finite() {
            self.deadline = None;
            return false;
        }
        let side = |x: f32| {
            if x < hit.area.x {
                -1
            } else if x >= hit.area.right() {
                1
            } else {
                0
            }
        };
        let previous = held.at;
        let previous_side = side(previous.0);
        let new_side = side(at.0);
        held.at = at;
        let beyond_slop = !within(at, held.start, self.slop);
        if beyond_slop || new_side != 0 {
            self.clicks.reset();
            held.moved = true;
        }
        if !held.moved && (held.base.is_some() || previous == at) {
            return false;
        }
        if new_side != 0 && new_side == previous_side && self.deadline.is_some_and(|at| now < at) {
            return false;
        }
        self.step(field, gfx, hit, now)
    }

    /// Advances a held drag once its scroll deadline arrives. Returns whether to redraw.
    pub fn tick(
        &mut self,
        field: &mut TextField,
        gfx: &mut Gfx,
        hit: FieldHit,
        now: Instant,
    ) -> bool {
        if self.deadline.is_some_and(|at| now >= at) {
            self.step(field, gfx, hit, now)
        } else {
            false
        }
    }

    /// The next wake-up needed for auto-scroll, if any.
    pub fn deadline(&self) -> Option<Instant> {
        self.deadline
    }

    /// Ends the drag and stops auto-scroll without breaking the click sequence.
    pub fn release(&mut self) {
        self.held = None;
        self.deadline = None;
    }

    /// Ends the drag and breaks the click sequence, for example on lost focus or right click.
    pub fn cancel(&mut self) {
        self.release();
        self.clicks.reset();
    }

    fn step(&mut self, field: &mut TextField, gfx: &mut Gfx, hit: FieldHit, now: Instant) -> bool {
        self.deadline = None;
        let Some(held) = &self.held else {
            return false;
        };
        if !hit.valid() {
            return false;
        }
        let line = hit.line(&field.display(), gfx);
        let before = (field.caret(), field.scroll, field.affinity);
        let x = held.at.0 - hit.area.x;
        let width = hit.area.w;
        let (x, outside) = edge_target(x, width, field.scroll, &line);
        let x = if outside == 0 {
            held.at.0 - snapped_origin(hit, field.scroll)
        } else {
            x + field.scroll
        };
        field.move_to_stop(line.hit(x), true);
        // Dragging after a multi-click keeps at least the original word (or whole field).
        if let Some((lo, hi)) = held
            .base
            .filter(|&(lo, hi)| field.text.is_char_boundary(lo) && field.text.is_char_boundary(hi))
        {
            if field.caret < lo {
                field.anchor = hi;
            } else {
                field.anchor = lo;
                if field.caret < hi {
                    field.caret = hi;
                    field.affinity = Some(parley::Affinity::Upstream);
                }
            }
        }
        reveal(field, &line, width);
        let more = match outside {
            -1 => field.scroll > 0.0,
            1 => field.scroll < (line.width + 1.0 - width).max(0.0),
            _ => false,
        };
        let changed = before != (field.caret(), field.scroll, field.affinity);
        if more && changed {
            self.deadline = now.checked_add(SCROLL_INTERVAL);
        }
        changed
    }
}

fn snapped_origin(hit: FieldHit, scroll: f32) -> f32 {
    ((hit.area.x - scroll) * hit.scale).round() / hit.scale
}

fn reveal(field: &mut TextField, line: &Line, width: f32) {
    field.scroll = reveal_scroll(
        field.scroll,
        line.x(field.shaped_stop(field.caret)),
        line.width,
        width,
    );
}

/// A bounded look-ahead from the edge, advancing at least to the next hidden stop. A tiny
/// overshoot must still scroll even when the caret stops are far apart.
fn edge_target(x: f32, width: f32, scroll: f32, line: &Line) -> (f32, i8) {
    if x < 0.0 {
        let edge = line.edges().filter(|&s| s < scroll).reduce(f32::max);
        (edge.map_or(x, |s| x.max(-MAX_STEP).min(s - scroll)), -1)
    } else if x >= width {
        let edge = scroll + (width - 1.0).max(0.0);
        let next = line.edges().filter(|&s| s > edge).reduce(f32::min);
        (
            next.map_or(x, |s| x.min(width + MAX_STEP).max(s - scroll)),
            1,
        )
    } else {
        (x, 0)
    }
}

#[cfg(test)]
#[path = "pointer_tests.rs"]
mod tests;
