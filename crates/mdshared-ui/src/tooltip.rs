//! A tooltip's timing: which control's tip waits to show, when it shows, and when it may not.
//! The model both Markdown applications kept a copy of (roadmap D6). It holds no clock: every
//! call that needs the time is given it, and [`deadline`](Tooltip::deadline) says when the
//! application's loop should wake next. Drawing is [`crate::overlay::Bubble::tip`] (and
//! [`Bubble::draw_presence`](crate::overlay::Bubble::draw_presence) with a fade from
//! [`crate::anim::document::TIP`]); what text a control's tip has is the application's.
//!
//! The rules, as Windows' tooltips keep them:
//! - resting on a control shows its tip after [`TIP_DELAY`](crate::anim::document::TIP_DELAY);
//! - moving to a neighbour while a tip shows shows the neighbour's at once;
//! - a press hides the tip, and the pressed control shows none again until the pointer has
//!   left it;
//! - a key, a scroll, the window losing the keyboard, or a popup opening hides it
//!   ([`hide`](Tooltip::hide)).
//!
//! # Deliberately not here
//! - The tip's text and anchor: the application's hit test gives them.
//! - A tip that follows the pointer, or stays while the pointer is on it: Windows' do not.
//! - Accessibility: a control's tip is its description in the tree ([`crate::a11y`]), given by
//!   the application with the control's node.

use crate::gfx::Rect;
use std::time::{Duration, Instant};

/// The control under the pointer that has a tip: its key, the text and where it is.
#[derive(Clone, Debug, PartialEq)]
pub struct Over<K> {
    pub key: K,
    pub text: String,
    /// The control's rectangle, which the tip is placed under ([`crate::overlay::Bubble::tip`]).
    pub anchor: Rect,
}

/// The tip waiting to show, or showing.
#[derive(Clone, Debug, PartialEq)]
struct Pending<K> {
    over: Over<K>,
    due: Instant,
    shown: bool,
}

/// A window's tooltip. `K` names a control (the application's hit-test value).
#[derive(Clone, Debug)]
pub struct Tooltip<K> {
    tip: Option<Pending<K>>,
    /// The control pressed last, which shows no tip until the pointer has left it.
    blocked: Option<K>,
    delay: Duration,
}

impl<K> Default for Tooltip<K> {
    fn default() -> Tooltip<K> {
        Tooltip::new()
    }
}

impl<K> Tooltip<K> {
    /// No tip, with [`TIP_DELAY`](crate::anim::document::TIP_DELAY).
    pub const fn new() -> Tooltip<K> {
        Tooltip {
            tip: None,
            blocked: None,
            delay: crate::anim::document::TIP_DELAY,
        }
    }

    /// The same, waiting `delay` before a first tip shows.
    pub fn with_delay(mut self, delay: Duration) -> Tooltip<K> {
        self.delay = delay;
        self
    }

    /// Hides the tip, showing or waiting: a key, a scroll, a popup, the window losing the
    /// keyboard. The next tip waits the whole delay again.
    pub fn hide(&mut self) {
        self.tip = None;
    }

    /// When the loop should wake to show the waiting tip; `None` when nothing waits.
    pub fn deadline(&self) -> Option<Instant> {
        self.tip.as_ref().filter(|t| !t.shown).map(|t| t.due)
    }

    /// The tip that shows now: its text and the control's rectangle.
    pub fn shown(&self) -> Option<&Over<K>> {
        self.tip.as_ref().filter(|t| t.shown).map(|t| &t.over)
    }

    /// Shows the waiting tip if its time has come. True when it began to show now: draw it.
    pub fn tick(&mut self, now: Instant) -> bool {
        match self.tip.as_mut() {
            Some(t) if !t.shown && now >= t.due => {
                t.shown = true;
                true
            }
            _ => false,
        }
    }
}

impl<K: PartialEq> Tooltip<K> {
    /// The pointer is over `over` now (`None`: over nothing with a tip, or left the window).
    /// A new control's tip waits the delay, or shows at once when another tip was showing;
    /// the same control keeps its tip and its time, with the text and anchor given now. True
    /// when what shows changed (a tip went or moved): redraw where it was and where it is.
    pub fn hover(&mut self, now: Instant, over: Option<Over<K>>) -> bool {
        if self
            .blocked
            .as_ref()
            .is_some_and(|b| over.as_ref().is_none_or(|o| o.key != *b))
        {
            self.blocked = None;
        }
        let over = over.filter(|o| self.blocked.as_ref() != Some(&o.key));
        let was = self.shown().map(|o| (o.anchor, o.text.clone()));
        match (over, self.tip.as_mut()) {
            (Some(o), Some(t)) if t.over.key == o.key => t.over = o,
            (Some(o), tip) => {
                let warm = tip.is_some_and(|t| t.shown);
                self.tip = Some(Pending {
                    over: o,
                    due: if warm { now } else { now + self.delay },
                    shown: warm,
                });
            }
            (None, _) => self.tip = None,
        }
        was != self.shown().map(|o| (o.anchor, o.text.clone()))
    }

    /// A press: the tip goes, and the control under the pointer (`key`, `None` for none) shows
    /// none again until the pointer has left it.
    pub fn press(&mut self, key: Option<K>) {
        self.tip = None;
        self.blocked = key;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn over(key: u8) -> Option<Over<u8>> {
        Some(Over {
            key,
            text: format!("tip {key}"),
            anchor: Rect::new(key as f32 * 40.0, 0.0, 32.0, 32.0),
        })
    }

    const D: Duration = crate::anim::document::TIP_DELAY;

    #[test]
    fn a_tip_shows_after_the_delay_and_a_neighbour_s_at_once() {
        let t0 = Instant::now();
        let mut tip = Tooltip::new();
        assert!(!tip.hover(t0, over(1)));
        assert_eq!(tip.deadline(), Some(t0 + D));
        assert!(!tip.tick(t0 + D - Duration::from_millis(1)));
        assert!(tip.shown().is_none());
        assert!(tip.tick(t0 + D));
        assert_eq!(tip.shown().unwrap().text, "tip 1");
        assert_eq!(tip.deadline(), None);
        // Moving along the same control changes nothing.
        assert!(!tip.hover(t0 + D * 2, over(1)));
        // A neighbour while a tip shows: at once.
        assert!(tip.hover(t0 + D * 2, over(2)));
        assert_eq!(tip.shown().unwrap().key, 2);
        // Off every control: gone, and the next waits again.
        assert!(tip.hover(t0 + D * 3, None));
        tip.hover(t0 + D * 3, over(3));
        assert!(tip.shown().is_none());
        assert_eq!(tip.deadline(), Some(t0 + D * 4));
    }

    #[test]
    fn a_pressed_control_shows_no_tip_until_the_pointer_left_it() {
        let t0 = Instant::now();
        let mut tip = Tooltip::new().with_delay(Duration::ZERO);
        tip.hover(t0, over(1));
        tip.tick(t0);
        tip.press(Some(1));
        assert!(tip.shown().is_none());
        tip.hover(t0, over(1));
        assert_eq!(tip.deadline(), None, "blocked while still over it");
        tip.hover(t0, None);
        tip.hover(t0, over(1));
        assert!(tip.tick(t0), "shows again once the pointer has been away");
        // Leaving to a neighbour also lifts the block.
        tip.press(Some(1));
        tip.hover(t0, over(2));
        assert!(tip.tick(t0));
        assert_eq!(tip.shown().unwrap().key, 2);
    }

    #[test]
    fn hide_takes_the_tip_and_the_warmth() {
        let t0 = Instant::now();
        let mut tip = Tooltip::new();
        tip.hover(t0, over(1));
        tip.tick(t0 + D);
        tip.hide();
        assert!(tip.shown().is_none() && tip.deadline().is_none());
        tip.hover(t0 + D, over(2));
        assert_eq!(tip.deadline(), Some(t0 + D * 2));
    }
}
