//! A stack of things that come and go each on its own: toasts that pile up instead of replacing
//! one another (roadmap X4). Where a [`Transient`](super::Transient) is one slot, a [`Stack`]
//! holds several. Each enters and leaves by its own presence, and the ones above it move down
//! into its place when it has gone, by [`fluent::MOVE`]. The newest is at the bottom, nearest
//! the place a single toast stands. The stack holds no clock: a thing's time to leave is given
//! with it, [`tick`](Stack::tick) is told the time, and [`deadline`](Stack::deadline) says when
//! to wake.
//!
//! # Deliberately not here
//! - The look and the place: the caller draws each item ([`crate::overlay::Bubble`]), moved up
//!   by the `lift` it is given.
//! - Pausing a toast's time while the pointer is on it: the caller can [`keep`](Stack::keep) it.

use super::{Animator, AsId, Id, Motion, fluent};
use std::time::Instant;

/// One thing in the stack, as [`Stack::frame`] gives it.
#[derive(Debug)]
pub struct Shown<'a, T> {
    /// The thing's key, from [`Stack::push`].
    pub key: u64,
    pub item: &'a T,
    /// How present it is: above 0, up to 1. Draw it with this (`Bubble::draw_presence`).
    pub t: f32,
    /// How far above the bottom place it stands, in the units of the heights given to
    /// [`frame`](Stack::frame), on its way when a thing below it came or went.
    pub lift: f32,
}

#[derive(Debug)]
struct Entry<T> {
    key: u64,
    item: T,
    shown: bool,
    /// When it leaves of itself; `None` for when it is hidden.
    until: Option<Instant>,
}

/// A stack of things that enter and leave each on its own.
#[derive(Debug)]
pub struct Stack<T> {
    items: Vec<Entry<T>>,
    next: u64,
    enter: Motion,
    exit: Motion,
    max: usize,
}

impl<T> Default for Stack<T> {
    fn default() -> Stack<T> {
        Stack::new()
    }
}

impl<T> Stack<T> {
    /// An empty stack whose things enter by [`fluent::ENTER`], leave by [`fluent::EXIT`], and of
    /// which at most three show at once.
    pub const fn new() -> Stack<T> {
        Stack {
            items: Vec::new(),
            next: 0,
            enter: fluent::ENTER,
            exit: fluent::EXIT,
            max: 3,
        }
    }

    /// The same stack with other motions.
    pub fn with_motion(mut self, enter: Motion, exit: Motion) -> Stack<T> {
        self.enter = enter;
        self.exit = exit;
        self
    }

    /// The same stack showing at most `max` things (at least one): a push over that many sends
    /// the oldest away.
    pub fn with_max(mut self, max: usize) -> Stack<T> {
        self.max = max.max(1);
        self
    }

    /// Adds `item` at the bottom, to leave of itself at `until` (`None`: when hidden). Returns
    /// its key. The oldest shown things beyond the stack's most are sent away.
    pub fn push(&mut self, item: T, until: Option<Instant>) -> u64 {
        let key = self.next;
        self.next += 1;
        self.items.push(Entry {
            key,
            item,
            shown: true,
            until,
        });
        let mut over = self.shown().count().saturating_sub(self.max);
        for e in self.items.iter_mut().filter(|e| e.shown) {
            if over == 0 {
                break;
            }
            e.shown = false;
            over -= 1;
        }
        key
    }

    /// Sends the thing `key` away; it plays its exit in [`frame`](Self::frame).
    pub fn hide(&mut self, key: u64) {
        if let Some(e) = self.items.iter_mut().find(|e| e.key == key) {
            e.shown = false;
        }
    }

    /// Sends everything away.
    pub fn hide_all(&mut self) {
        for e in &mut self.items {
            e.shown = false;
        }
    }

    /// Gives the thing `key` a new time to leave (`None`: not until hidden), for a toast the
    /// pointer rests on or one whose message was said again.
    pub fn keep(&mut self, key: u64, until: Option<Instant>) {
        if let Some(e) = self.items.iter_mut().find(|e| e.key == key && e.shown) {
            e.until = until;
        }
    }

    /// Sends away the things whose time has come. True when one went: draw a frame.
    pub fn tick(&mut self, now: Instant) -> bool {
        let mut went = false;
        for e in self.items.iter_mut().filter(|e| e.shown) {
            if e.until.is_some_and(|u| now >= u) {
                e.shown = false;
                went = true;
            }
        }
        went
    }

    /// The earliest time a shown thing leaves of itself.
    pub fn deadline(&self) -> Option<Instant> {
        self.shown().filter_map(|e| e.until).min()
    }

    /// The things logically there, oldest first: not the ones leaving.
    pub fn current(&self) -> impl Iterator<Item = (u64, &T)> {
        self.shown().map(|e| (e.key, &e.item))
    }

    /// Whether nothing is there, nor leaving.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    fn shown(&self) -> impl Iterator<Item = &Entry<T>> {
        self.items.iter().filter(|e| e.shown)
    }

    /// The things to draw this frame, the bottom one first, each with its presence and its
    /// lift: the sum of `height` (plus `gap`) of the shown things below it, on its way by
    /// [`fluent::MOVE`] when that changes. A thing leaving keeps its place while it goes; the
    /// ones above move down when it has gone. `id` names the stack in `anim`. Things whose exit
    /// has played are dropped.
    pub fn frame(
        &mut self,
        anim: &mut Animator,
        id: impl AsId,
        height: impl Fn(&T) -> f32,
        gap: f32,
    ) -> Vec<Shown<'_, T>> {
        let id = Id::new(id);
        let (enter, exit) = (self.enter, self.exit);
        let ts: Vec<f32> = self
            .items
            .iter()
            .map(|e| anim.presence(id.with(("in", e.key)), e.shown, enter, exit))
            .collect();
        let mut i = 0;
        let mut kept = Vec::with_capacity(ts.len());
        self.items.retain(|e| {
            let t = ts[i];
            i += 1;
            let keep = e.shown || t > 0.0;
            if keep {
                kept.push(t);
            }
            keep
        });
        // The newest at the bottom; a thing leaving still holds its place.
        let mut below = 0.0_f32;
        let mut out = Vec::with_capacity(kept.len());
        for (e, &t) in self.items.iter().zip(&kept).rev() {
            let lift = anim.value(id.with(("lift", e.key)), below, fluent::MOVE);
            out.push(Shown {
                key: e.key,
                item: &e.item,
                t,
                lift,
            });
            below += height(&e.item) + gap;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anim::{Clock, ManualClock};
    use std::time::Duration;

    fn run(
        stack: &mut Stack<&'static str>,
        anim: &mut Animator,
        clock: &ManualClock,
    ) -> Vec<(&'static str, f32, f32)> {
        anim.begin_frame(clock.now());
        let out = stack
            .frame(anim, "toasts", |_| 36.0, 8.0)
            .into_iter()
            .map(|s| (*s.item, s.t, s.lift))
            .collect();
        anim.end_frame();
        out
    }

    /// Frames every 16 ms for `ms`, then the last frame's things.
    fn after(
        ms: u64,
        stack: &mut Stack<&'static str>,
        anim: &mut Animator,
        clock: &ManualClock,
    ) -> Vec<(&'static str, f32, f32)> {
        let mut out = run(stack, anim, clock);
        for _ in 0..ms / 16 {
            clock.advance_ms(16);
            out = run(stack, anim, clock);
        }
        out
    }

    #[test]
    fn toasts_pile_up_and_close_the_gap_when_one_goes() {
        let clock = ManualClock::new();
        let mut anim = Animator::new();
        let mut stack = Stack::new();
        let t0 = clock.now();
        stack.push("saved", Some(t0 + Duration::from_secs(3)));
        let one = after(400, &mut stack, &mut anim, &clock);
        assert_eq!(one, vec![("saved", 1.0, 0.0)]);
        // A second: at the bottom, and the first moves up by one height and the gap.
        stack.push("copied", Some(t0 + Duration::from_secs(5)));
        let two = after(600, &mut stack, &mut anim, &clock);
        assert_eq!(two, vec![("copied", 1.0, 0.0), ("saved", 1.0, 44.0)]);
        assert_eq!(stack.deadline(), Some(t0 + Duration::from_secs(3)));
        // The first goes of itself: it fades where it stands, then is dropped.
        assert!(stack.tick(t0 + Duration::from_secs(3)));
        assert_eq!(stack.deadline(), Some(t0 + Duration::from_secs(5)));
        let leaving = after(64, &mut stack, &mut anim, &clock);
        assert!(leaving[1].1 < 1.0 && leaving[1].2 == 44.0);
        let gone = after(600, &mut stack, &mut anim, &clock);
        assert_eq!(gone, vec![("copied", 1.0, 0.0)]);
        assert_eq!(stack.current().count(), 1);
    }

    #[test]
    fn a_push_over_the_most_sends_the_oldest_away_and_the_middle_one_down() {
        let clock = ManualClock::new();
        let mut anim = Animator::new();
        let mut stack = Stack::new().with_max(2);
        let a = stack.push("a", None);
        stack.push("b", None);
        run(&mut stack, &mut anim, &clock);
        stack.push("c", None);
        let names: Vec<_> = stack.current().map(|(_, s)| *s).collect();
        assert_eq!(names, ["b", "c"]);
        stack.hide(a);
        assert_eq!(stack.deadline(), None);
        // "a" was not shown and leaves; when it has, the others stand as two.
        let out = after(1200, &mut stack, &mut anim, &clock);
        assert_eq!(out, vec![("c", 1.0, 0.0), ("b", 1.0, 44.0)]);
        stack.hide_all();
        after(600, &mut stack, &mut anim, &clock);
        assert!(stack.is_empty());
    }
}
