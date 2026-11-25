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
    /// At most this many kept, the leaving ones counted ([`with_max_drawn`](Stack::with_max_drawn)).
    max_drawn: Option<usize>,
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
            max_drawn: None,
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

    /// The same stack keeping at most `max` things (at least one) in all, the ones leaving
    /// counted: over that many, the oldest leaving ones are dropped at once, with no rest of
    /// their exit, so pushes faster than the exit do not pile up. A shown thing is dropped (with
    /// no exit) only when nothing is leaving, which happens only with `max` under
    /// [`with_max`](Self::with_max)'s: keep it at least that. Without it only the things shown
    /// are counted.
    pub fn with_max_drawn(mut self, max: usize) -> Stack<T> {
        self.max_drawn = Some(max.max(1));
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
        self.cap(key);
        key
    }

    /// Sends away the oldest shown things beyond the most shown, sparing `keep` (the one just
    /// pushed or brought back), then drops what is beyond the most kept: the oldest leaving
    /// ones first.
    fn cap(&mut self, keep: u64) {
        let mut over = self.shown().count().saturating_sub(self.max);
        for e in self.items.iter_mut().filter(|e| e.shown && e.key != keep) {
            if over == 0 {
                break;
            }
            e.shown = false;
            over -= 1;
        }
        let Some(max) = self.max_drawn else {
            return;
        };
        while self.items.len() > max {
            let at = self
                .items
                .iter()
                .position(|e| !e.shown)
                .or_else(|| self.items.iter().position(|e| e.key != keep))
                .unwrap_or(0);
            self.items.remove(at);
        }
    }

    /// Puts `item` in the place of the thing `key`, with a new time to leave, at once: a status
    /// that replaces itself (a zoom step, progress turning into a result) plays no exit and no
    /// entrance. A thing that is leaving comes back, turning round where it is in its exit, and
    /// the oldest others are sent away if that makes more than the most shown. False when there
    /// is no such thing.
    pub fn replace(&mut self, key: u64, item: T, until: Option<Instant>) -> bool {
        match self.items.iter_mut().find(|e| e.key == key) {
            Some(e) => {
                (e.item, e.until, e.shown) = (item, until, true);
                self.cap(key);
                true
            }
            None => false,
        }
    }

    /// Gives the thing `key` a new time to leave (`None`: not until hidden), and brings it back
    /// if it is leaving: it turns round where it is in its exit and stays in its place, and the
    /// oldest others are sent away if that makes more than the most shown. True when there is
    /// such a thing, shown or leaving.
    pub fn revive(&mut self, key: u64, until: Option<Instant>) -> bool {
        match self.items.iter_mut().find(|e| e.key == key) {
            Some(e) => {
                (e.shown, e.until) = (true, until);
                self.cap(key);
                true
            }
            None => false,
        }
    }

    /// Takes the thing `key` away at once, with no exit; the ones above move down into its
    /// place.
    pub fn remove(&mut self, key: u64) {
        self.items.retain(|e| e.key != key);
    }

    /// Takes everything away at once, with no exit.
    pub fn clear(&mut self) {
        self.items.clear();
    }

    /// The thing for which `is` holds, the newest shown one first, else the newest leaving one:
    /// its key and whether it is still shown.
    pub fn find(&self, mut is: impl FnMut(&T) -> bool) -> Option<(u64, bool)> {
        let mut leaving = None;
        for e in self.items.iter().rev() {
            if is(&e.item) {
                if e.shown {
                    return Some((e.key, true));
                }
                leaving = leaving.or(Some((e.key, false)));
            }
        }
        leaving
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

impl<T: PartialEq> Stack<T> {
    /// [`push`](Self::push), unless an equal thing is there or leaving: then that one is kept
    /// (brought back if it was leaving, [`revive`](Self::revive)) with the new time to leave,
    /// so a confirmation said again does not stand twice. Returns the key.
    pub fn show(&mut self, item: T, until: Option<Instant>) -> u64 {
        match self.find(|t| *t == item) {
            Some((key, _)) => {
                self.revive(key, until);
                key
            }
            None => self.push(item, until),
        }
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
    fn replace_remove_and_clear_act_at_once() {
        let clock = ManualClock::new();
        let mut anim = Animator::new();
        let mut stack = Stack::new();
        let zoom = stack.push("110%", None);
        after(400, &mut stack, &mut anim, &clock);
        assert!(stack.replace(zoom, "120%", None));
        // In place, at once: no exit, no entrance, same key.
        assert_eq!(run(&mut stack, &mut anim, &clock), vec![("120%", 1.0, 0.0)]);
        assert!(!stack.replace(99, "x", None));
        let saved = stack.push("saved", None);
        after(600, &mut stack, &mut anim, &clock);
        stack.remove(saved);
        // Gone at once; the one above moves down into its place.
        let now = run(&mut stack, &mut anim, &clock);
        assert_eq!(now.len(), 1);
        assert_eq!(
            after(600, &mut stack, &mut anim, &clock),
            vec![("120%", 1.0, 0.0)]
        );
        stack.push("copied", None);
        stack.clear();
        assert!(stack.is_empty());
        assert!(run(&mut stack, &mut anim, &clock).is_empty());
    }

    #[test]
    fn show_brings_back_an_equal_one_that_is_leaving_instead_of_a_second() {
        let clock = ManualClock::new();
        let mut anim = Animator::new();
        let mut stack = Stack::new();
        let a = stack.show("copied", None);
        after(400, &mut stack, &mut anim, &clock);
        stack.hide(a);
        let leaving = after(48, &mut stack, &mut anim, &clock);
        assert!(leaving[0].1 < 1.0);
        assert_eq!(stack.find(|s| *s == "copied"), Some((a, false)));
        assert_eq!(stack.show("copied", None), a, "the same one, not a second");
        let back = after(600, &mut stack, &mut anim, &clock);
        assert_eq!(back, vec![("copied", 1.0, 0.0)]);
        // A different one is pushed as before.
        assert_ne!(stack.show("saved", None), a);
        assert_eq!(stack.current().count(), 2);
    }

    /// Bringing one back keeps to the most shown: the oldest other is sent away.
    #[test]
    fn a_toast_brought_back_keeps_to_the_most_shown() {
        let clock = ManualClock::new();
        let mut anim = Animator::new();
        let mut stack = Stack::new().with_max(1);
        let copied = stack.show("copied", None);
        after(400, &mut stack, &mut anim, &clock);
        stack.show("saved", None);
        after(32, &mut stack, &mut anim, &clock);
        assert_eq!(stack.show("copied", None), copied);
        let shown: Vec<_> = stack.current().map(|(_, s)| *s).collect();
        assert_eq!(shown, ["copied"], "\"saved\" is sent away");
        // The same through replace.
        let saved = stack.find(|s| *s == "saved").unwrap().0;
        assert!(stack.replace(saved, "saved!", None));
        let shown: Vec<_> = stack.current().map(|(_, s)| *s).collect();
        assert_eq!(shown, ["saved!"]);
    }

    /// Over the most kept, the leaving ones go first: a shown progress toast older than them
    /// stays, and can still be replaced by its result.
    #[test]
    fn with_max_drawn_drops_the_leaving_ones_first() {
        let clock = ManualClock::new();
        let mut anim = Animator::new();
        let mut stack = Stack::new().with_max(3).with_max_drawn(4);
        let progress = stack.push("exporting", None);
        let b = stack.push("b", None);
        let c = stack.push("c", None);
        run(&mut stack, &mut anim, &clock);
        stack.hide(b);
        stack.hide(c);
        stack.push("d", None);
        stack.push("e", None);
        let kept: Vec<_> = stack.find(|s| *s == "exporting").into_iter().collect();
        assert_eq!(kept, [(progress, true)]);
        assert!(stack.replace(progress, "exported", None));
        assert!(stack.items.len() <= 4);
    }

    #[test]
    fn with_max_drawn_counts_the_leaving_ones_too() {
        let clock = ManualClock::new();
        let mut anim = Animator::new();
        let mut stack = Stack::new().with_max(2).with_max_drawn(3);
        for s in ["a", "b", "c", "d", "e"] {
            stack.push(s, None);
            run(&mut stack, &mut anim, &clock);
            clock.advance_ms(16);
        }
        let drawn = run(&mut stack, &mut anim, &clock);
        let names: Vec<_> = drawn.iter().map(|d| d.0).collect();
        assert_eq!(names, ["e", "d", "c"], "at most three kept, newest first");
        let shown: Vec<_> = stack.current().map(|(_, s)| *s).collect();
        assert_eq!(shown, ["d", "e"]);
        // Without it, the leaving ones are not counted.
        let mut loose = Stack::new().with_max(2);
        let mut anim = Animator::new();
        for s in ["a", "b", "c", "d", "e"] {
            loose.push(s, None);
            run(&mut loose, &mut anim, &clock);
            clock.advance_ms(16);
        }
        assert!(run(&mut loose, &mut anim, &clock).len() > 3);
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
