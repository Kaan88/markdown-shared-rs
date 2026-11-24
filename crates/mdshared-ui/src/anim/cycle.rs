//! Things that repeat for as long as they are shown: a spinner that turns, a bar that runs along
//! its track, a caret that blinks.
//!
//! A value on its way to a target arrives, and the window sleeps. These never arrive, so they
//! are not values with targets: each is a clock that starts the first time its name is asked
//! for, and the drawing reads how far round it is. The rule of the [`Animator`] holds for them
//! as for everything else: asking is what keeps it going. A frame that asks gets another frame;
//! the first frame that does not ask forgets the name, and the window sleeps again. Nothing is
//! started, stopped or cleaned up.
//!
//! [`Animator::cycle`] is the phase of something that moves all the time, and asks for a frame
//! every frame interval. [`Animator::blink`] is on or off, and asks for a frame only at the
//! moments it changes: a blinking caret wakes the window twice a second, not sixty times.
//! [`Periodic`] is a cycle with its settings: one that goes there and back, and what it shows
//! when nothing may move.
//!
//! With reduced motion (or [`Policy::Never`](super::Policy::Never)) nothing here moves and no
//! frame is asked for: a cycle stands at one phase, a blink is steadily on.
//!
//! Deliberately not here: a number of repeats after which it stops (the caller stops asking), a
//! phase shared by several windows (each animator has its own time), and easing of a cycle
//! (the caller puts the phase through an [`Easing`](super::Easing) if it wants one).

use super::{Animator, AsId, Id};
use std::time::Duration;

/// The clock of one thing that repeats.
#[derive(Clone, Copy, Debug)]
pub(super) struct Running {
    /// The animator's time at which it was first asked for.
    start: Duration,
    /// The frame that last asked for it.
    pub(super) seen: u64,
    /// The animator's time at which it next needs a frame.
    pub(super) next: Duration,
}

/// The least wait a blink asks for: a toggle a hair away is not a reason to spin.
const LEAST_WAIT: Duration = Duration::from_millis(1);

/// A cycle and how it is read: its period, whether it goes there and back, and the phase it
/// stands at when nothing animates.
///
/// ```
/// # use mdshared_ui::anim::{Animator, Clock, ManualClock, Periodic};
/// # use std::time::Duration;
/// # let clock = ManualClock::new();
/// # let mut anim = Animator::new();
/// # anim.begin_frame(clock.now());
/// // A dot that swells and shrinks once a second, and is half size when motion is reduced.
/// const PULSE: Periodic = Periodic::new(Duration::from_millis(500)).ping_pong().still_at(0.5);
/// let size = 4.0 + 4.0 * PULSE.phase(&mut anim, "recording-dot");
/// # assert_eq!(size, 4.0);
/// # assert!(anim.end_frame().is_some());
/// ```
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Periodic {
    period: Duration,
    ping_pong: bool,
    still: f32,
}

impl Periodic {
    /// A cycle that runs from 0 up to (not including) 1 in `period` and starts again at 0. A
    /// period of zero is no cycle: it stands at its [still](Self::still_at) phase.
    pub const fn new(period: Duration) -> Periodic {
        Periodic {
            period,
            ping_pong: false,
            still: 0.0,
        }
    }

    /// The same, going there and back: from 0 to 1 in one period and from 1 back to 0 in the
    /// next, with no jump at either end. For something that breathes or sways.
    pub const fn ping_pong(mut self) -> Periodic {
        self.ping_pong = true;
        self
    }

    /// The phase given when nothing animates (reduced motion): the one picture that stands for
    /// the whole cycle. It is 0 unless set. A number that is not one is taken as 0.
    pub const fn still_at(mut self, phase: f32) -> Periodic {
        self.still = if phase.is_finite() { phase } else { 0.0 };
        self
    }

    /// The time of one run from 0 to 1.
    pub const fn period(self) -> Duration {
        self.period
    }

    /// The phase `elapsed` after the cycle began: the arithmetic alone, with no animator and no
    /// clock, for a render that must come out the same every time. From 0 up to (not including)
    /// 1; there and back, from 0 to 1 and both included.
    pub fn at(self, elapsed: Duration) -> f32 {
        let period = self.period.as_nanos();
        if period == 0 {
            return self.still;
        }
        // Whole nanoseconds, so that an hour in, the phase is as exact as in the first second.
        let whole = if self.ping_pong { period * 2 } else { period };
        let t = (elapsed.as_nanos() % whole) as f64 / period as f64;
        if self.ping_pong {
            (if t <= 1.0 { t } else { 2.0 - t }) as f32
        } else {
            // A phase a hair under 1 must not round up to 1: the cycle never shows its end.
            (t as f32).min(1.0 - f32::EPSILON)
        }
    }

    /// The phase now of the cycle named `id`, which began the first time `id` was asked for.
    /// Asking keeps it running: the animator asks for the next frame one frame interval on,
    /// for as long as some frame asks, and forgets `id` at the end of the first frame that
    /// does not. When the animator does not animate it is the [still](Self::still_at) phase on
    /// every frame, and no frame is asked for.
    pub fn phase(self, anim: &mut Animator, id: impl AsId) -> f32 {
        if !anim.animates() || self.period.is_zero() {
            return self.still;
        }
        let interval = anim.frame_interval;
        self.at(anim.run(id.as_id(), |_| interval))
    }
}

impl Animator {
    /// How far round a thing that repeats every `period` is: from 0 up to (not including) 1,
    /// starting at 0 the first time `id` is asked for. For a spinner, a bar that runs along its
    /// track, anything that moves for as long as it is shown.
    ///
    /// Asking is what keeps it moving: [`end_frame`](Self::end_frame) asks for the next frame
    /// one frame interval on while some frame asks for `id`, and the first frame that does not
    /// ask forgets it, so the window sleeps as soon as the thing is no longer drawn. Asked for
    /// again later, it starts again at 0.
    ///
    /// With reduced motion (the animator [does not animate](Self::animates)), and for a period
    /// of zero, it is 0 on every frame and asks for no frame; a caller with a better still
    /// picture than phase 0 asks [`animates`](Self::animates) first, or uses a [`Periodic`]
    /// with [`still_at`](Periodic::still_at). The time is the animator's own, so a stall or a
    /// minute spent minimised does not throw the phase on (see the stall guard of
    /// [`Animator`]).
    pub fn cycle(&mut self, id: impl AsId, period: Duration) -> f32 {
        Periodic::new(period).phase(self, id)
    }

    /// Whether a thing that blinks is on: on for `on`, off for `off`, and so on, starting on
    /// the first time `id` is asked for. For a text caret.
    ///
    /// Unlike a [`cycle`](Self::cycle) it does not ask for a frame every interval: the deadline
    /// [`end_frame`](Self::end_frame) gives is the moment it next changes, so a caret that
    /// blinks every half second wakes the window twice a second. As with everything, asking is
    /// what keeps it going, and the first frame that does not ask forgets it.
    /// [`forget`](Self::forget) starts it again, on: what an editor does on every key, so the
    /// caret is solid while the user types.
    ///
    /// It does not blink, and is steadily on with no frame asked for, with reduced motion and
    /// when either time is zero (a system whose caret blinking is switched off gives no time).
    pub fn blink(&mut self, id: impl AsId, on: Duration, off: Duration) -> bool {
        if !self.animates() || on.is_zero() || off.is_zero() {
            return true;
        }
        let (on, whole) = (on.as_nanos(), on.as_nanos() + off.as_nanos());
        // How long until it changes, from where in its period it is.
        let left = |elapsed: Duration| {
            let at = elapsed.as_nanos() % whole;
            let left = if at < on { on - at } else { whole - at };
            Duration::from_nanos(u64::try_from(left).unwrap_or(u64::MAX)).max(LEAST_WAIT)
        };
        self.run(id.as_id(), left).as_nanos() % whole < on
    }

    /// The time since `id` was first asked for, on the animator's own clock; asking keeps it.
    /// `next` says, from that time, how long until `id` next needs a frame.
    fn run(&mut self, id: Id, next: impl FnOnce(Duration) -> Duration) -> Duration {
        let (now, frame) = (self.time, self.frame);
        let run = self.runs.entry(id.0).or_insert(Running {
            start: now,
            seen: frame,
            next: now,
        });
        run.seen = frame;
        let elapsed = now.saturating_sub(run.start);
        run.next = now.saturating_add(next(elapsed));
        elapsed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anim::{Clock, Easing, ManualClock, Motion, Policy};
    use std::time::Instant;

    const FRAME: Duration = Duration::from_millis(10);
    const SECOND: Duration = Duration::from_secs(1);

    /// An animator on a clock moved by hand, with frames of 10 ms.
    struct Rig {
        clock: ManualClock,
        anim: Animator,
    }

    impl Rig {
        fn new() -> Rig {
            let mut anim = Animator::new();
            anim.set_frame_interval(FRAME);
            Rig {
                clock: ManualClock::new(),
                anim,
            }
        }

        /// One frame `ms` later in which `ask` asks the animator for what it wants.
        fn frame<T>(
            &mut self,
            ms: u64,
            ask: impl FnOnce(&mut Animator) -> T,
        ) -> (T, Option<Instant>) {
            self.clock.advance_ms(ms);
            self.anim.begin_frame(self.clock.now());
            let got = ask(&mut self.anim);
            (got, self.anim.end_frame())
        }
    }

    #[test]
    fn a_cycle_runs_from_nought_towards_one_and_starts_again_while_it_is_asked_for() {
        let mut r = Rig::new();
        let ask = |a: &mut Animator| a.cycle("spin", SECOND);
        // First asked: at 0, and the next frame is one interval on.
        let (phase, deadline) = r.frame(0, ask);
        assert_eq!(phase, 0.0);
        assert_eq!(deadline, Some(r.clock.now() + FRAME));
        assert!(r.anim.animating());
        assert_eq!(r.anim.len(), 1);
        // Every frame asks for the next, and the phase is the time gone by, over the period.
        for step in 1..=250u64 {
            let (phase, deadline) = r.frame(10, ask);
            let want = (step * 10 % 1000) as f32 / 1000.0;
            assert!(
                (phase - want).abs() < 1e-6,
                "step {step}: {phase}, not {want}"
            );
            assert!((0.0..1.0).contains(&phase));
            assert_eq!(deadline, Some(r.clock.now() + FRAME));
        }
        // Not asked for: forgotten, and the window sleeps.
        assert_eq!(r.frame(10, |_| ()).1, None);
        assert!(r.anim.is_empty() && !r.anim.animating());
        // Asked for again, it starts again at 0.
        assert_eq!(r.frame(730, ask).0, 0.0);
        // Two names are two clocks; one name asked twice in a frame is one.
        let (both, _) = r.frame(250, |a| {
            (
                a.cycle("spin", SECOND),
                a.cycle("other", SECOND),
                a.cycle("spin", SECOND),
            )
        });
        // The frame before was 250 ms ago, which the stall guard counts as 50.
        assert_eq!(both, (0.05, 0.0, 0.05));
        // `forget` starts one again.
        r.anim.forget("spin");
        assert_eq!(r.frame(10, ask).0, 0.0);
    }

    #[test]
    fn with_reduced_motion_a_cycle_stands_still_and_asks_for_no_frame() {
        let mut r = Rig::new();
        r.frame(0, |a| a.cycle("spin", SECOND));
        r.frame(10, |a| a.cycle("spin", SECOND));
        for policy in [Policy::System, Policy::Never] {
            r.anim.set_system_reduced(true);
            r.anim.set_policy(policy);
            for _ in 0..3 {
                assert_eq!(r.frame(330, |a| a.cycle("spin", SECOND)), (0.0, None));
            }
            assert!(
                r.anim.is_empty(),
                "nothing is kept for a thing that stands still"
            );
            // A periodic stands at the phase it was told to.
            let still = Periodic::new(SECOND).still_at(0.75);
            assert_eq!(r.frame(10, |a| still.phase(a, "bar")), (0.75, None));
        }
        // The application's own switch over the system: it runs.
        r.anim.set_policy(Policy::Always);
        r.frame(0, |a| a.cycle("spin", SECOND));
        let (phase, deadline) = r.frame(10, |a| a.cycle("spin", SECOND));
        assert_eq!(phase, 0.01);
        assert!(deadline.is_some());
        // No period is no cycle, whatever the policy; nothing is kept and nothing asked.
        assert_eq!(
            r.frame(10, |a| a.cycle("none", Duration::ZERO)),
            (0.0, None)
        );
        let none = Periodic::new(Duration::ZERO).still_at(0.3);
        assert_eq!(r.frame(10, |a| none.phase(a, "none")), (0.3, None));
        assert_eq!(none.at(SECOND), 0.3);
    }

    #[test]
    fn a_periodic_goes_round_or_there_and_back_and_is_exact_after_hours() {
        let round = Periodic::new(SECOND);
        assert_eq!(round.period(), SECOND);
        let back = Periodic::new(SECOND).ping_pong();
        let ms = Duration::from_millis;
        for (elapsed, want_round, want_back) in [
            (0, 0.0, 0.0),
            (250, 0.25, 0.25),
            (500, 0.5, 0.5),
            (750, 0.75, 0.75),
            (1000, 0.0, 1.0),
            (1250, 0.25, 0.75),
            (1500, 0.5, 0.5),
            (2000, 0.0, 0.0),
            (2250, 0.25, 0.25),
            // A hundred hours on, to the millisecond.
            (360_000_000 + 250, 0.25, 0.25),
            (360_000_000 + 1750, 0.75, 0.25),
        ] {
            assert_eq!(round.at(ms(elapsed)), want_round, "round at {elapsed}");
            assert_eq!(back.at(ms(elapsed)), want_back, "back at {elapsed}");
        }
        // Against a second way of computing it, on times from a fixed pseudo-random sequence:
        // the remainder in floating point, where the first way is in whole nanoseconds.
        let odd = Periodic::new(ms(1600));
        let sway = odd.ping_pong();
        let mut seed = 0x2545_F491_u64;
        for _ in 0..2000 {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let elapsed = Duration::from_micros((seed >> 33) % 600_000_000);
            let turns = elapsed.as_secs_f64() / 1.6;
            let phase = odd.at(elapsed);
            assert!((0.0..1.0).contains(&phase), "{elapsed:?}: {phase}");
            assert!((phase as f64 - turns.fract()).abs() < 1e-6, "{elapsed:?}");
            let there = (turns / 2.0).fract() * 2.0;
            let want = if there <= 1.0 { there } else { 2.0 - there };
            let swing = sway.at(elapsed);
            assert!((0.0..=1.0).contains(&swing), "{elapsed:?}: {swing}");
            assert!((swing as f64 - want).abs() < 1e-6, "{elapsed:?}");
        }
        // The last nanosecond of a turn is still under 1.
        let last = round.at(SECOND - Duration::from_nanos(1));
        assert!(last < 1.0 && last > 0.999);
        // There and back has no jump at its turn: a millisecond either side is a thousandth.
        assert!((back.at(ms(999)) - back.at(ms(1001))).abs() < 1e-6);
        assert!(back.at(ms(1999)) < 0.002 && back.at(ms(2001)) < 0.002);
        // A still phase that is not a number is 0.
        assert_eq!(
            Periodic::new(SECOND).still_at(f32::NAN),
            Periodic::new(SECOND)
        );
        // Through the animator it is the same arithmetic on the animator's time.
        let mut r = Rig::new();
        r.frame(0, |a| back.phase(a, "sway"));
        let mut seen = Vec::new();
        for _ in 0..220 {
            seen.push(r.frame(10, |a| back.phase(a, "sway")).0);
        }
        assert_eq!(seen[24], 0.25);
        assert_eq!(seen[99], 1.0);
        assert_eq!(seen[149], 0.5);
        assert_eq!(seen[199], 0.0);
        assert!(
            seen.windows(2).all(|w| (w[1] - w[0]).abs() < 0.011),
            "no jump"
        );
    }

    #[test]
    fn a_blink_wakes_the_window_only_when_it_changes() {
        let (on, off) = (Duration::from_millis(500), Duration::from_millis(300));
        let ask = |a: &mut Animator| a.blink("caret", on, off);
        let mut r = Rig::new();
        // First asked: on, and the next frame is when it goes off, not one interval on.
        let (lit, deadline) = r.frame(0, ask);
        assert!(lit);
        assert_eq!(deadline, Some(r.clock.now() + on));
        // Woken exactly then, again and again: it alternates, each wait the length of the
        // state it is in, and nothing in between is asked for. A minute of blinking is 150
        // frames, where a cycle would have drawn 6000.
        let mut frames = 0;
        let mut total = Duration::ZERO;
        let mut lit = true;
        while total < Duration::from_secs(60) {
            let wait = if lit { on } else { off };
            let (now_lit, deadline) = r.frame(wait.as_millis() as u64, ask);
            lit = !lit;
            assert_eq!(now_lit, lit, "after {total:?}");
            let next = if lit { on } else { off };
            assert_eq!(deadline, Some(r.clock.now() + next), "after {total:?}");
            total += wait;
            frames += 1;
        }
        assert_eq!(frames, 150);
        // A frame drawn early for another reason changes nothing and waits for what is left.
        let mut r = Rig::new();
        r.frame(0, ask);
        let (lit, deadline) = r.frame(20, ask);
        assert!(lit);
        assert_eq!(deadline, Some(r.clock.now() + Duration::from_millis(480)));
        let (lit, deadline) = r.frame(480, ask);
        assert!(!lit);
        assert_eq!(deadline, Some(r.clock.now() + off));
        // A key was typed while it is off: forgotten, it starts again, on.
        r.frame(40, ask);
        r.anim.forget("caret");
        let (lit, deadline) = r.frame(10, ask);
        assert!(lit);
        assert_eq!(deadline, Some(r.clock.now() + on));
        // Something in flight is sooner than the blink; once it has arrived the blink's own
        // moment is the deadline again.
        let linear = Motion::tween_ms(30, Easing::Linear);
        r.frame(0, |a| {
            (a.blink("caret", on, off), a.value("x", 0.0_f32, linear))
        });
        let (_, deadline) = r.frame(0, |a| {
            (a.blink("caret", on, off), a.value("x", 1.0_f32, linear))
        });
        assert_eq!(deadline, Some(r.clock.now() + FRAME));
        r.frame(10, |a| {
            (a.blink("caret", on, off), a.value("x", 1.0_f32, linear))
        });
        r.frame(10, |a| {
            (a.blink("caret", on, off), a.value("x", 1.0_f32, linear))
        });
        let (got, deadline) = r.frame(10, |a| {
            (a.blink("caret", on, off), a.value("x", 1.0_f32, linear))
        });
        assert_eq!(got, (true, 1.0));
        assert_eq!(deadline, Some(r.clock.now() + Duration::from_millis(470)));
        // Not asked for: forgotten, the window sleeps.
        assert_eq!(r.frame(10, |_| ()).1, None);
        assert!(r.anim.is_empty());
    }

    #[test]
    fn a_blink_is_steadily_on_with_reduced_motion_and_without_times() {
        let half = Duration::from_millis(500);
        let mut r = Rig::new();
        for (on, off) in [
            (Duration::ZERO, half),
            (half, Duration::ZERO),
            (Duration::ZERO, Duration::ZERO),
        ] {
            for ms in [0, 300, 700] {
                assert_eq!(r.frame(ms, |a| a.blink("caret", on, off)), (true, None));
            }
        }
        r.anim.set_system_reduced(true);
        for ms in [0, 400, 400, 400] {
            assert_eq!(r.frame(ms, |a| a.blink("caret", half, half)), (true, None));
        }
        assert!(r.anim.is_empty());
        // Times too long to count and a toggle a hair away: no overflow, and never a wait of
        // nothing.
        r.anim.set_system_reduced(false);
        let (lit, deadline) = r.frame(0, |a| a.blink("slow", Duration::MAX, Duration::MAX));
        assert!(lit && deadline.is_some());
        let tiny = Duration::from_nanos(3);
        let (_, deadline) = r.frame(10, |a| a.blink("fast", tiny, tiny));
        assert_eq!(deadline, Some(r.clock.now() + LEAST_WAIT));
    }
}
