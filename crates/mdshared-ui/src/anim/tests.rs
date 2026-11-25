//! The tests of the animator itself (`anim.rs`): values, motions, the frame and its deadline.
//! The other pieces of the module are tested in their own files.

use super::*;

const LINEAR_100: Motion = Motion::tween_ms(100, Easing::Linear);
const FRAME: Duration = Duration::from_millis(10);

#[test]
fn an_id_used_as_a_name_is_the_name_it_was_made_from() {
    let mut anim = Animator::new();
    anim.set("a", 1.0);
    assert_eq!(anim.get::<f32>("a"), Some(1.0));
    // An id is itself, however often it is passed on: nothing hashes it again.
    let id = Id::new("a");
    assert_eq!(anim.get::<f32>(id), Some(1.0));
    assert_eq!(anim.get::<f32>(&id), Some(1.0));
    assert_eq!(Id::new(id), id);
    assert_eq!(Id::new(Id::new(id)), id);
    assert_eq!((id.as_id(), "a".as_id()), (id, id));
    // A borrowed name is the name, and so is an owned one.
    assert_eq!(anim.get::<f32>(&"a"), Some(1.0));
    assert_eq!(anim.get::<f32>(String::from("a").as_str()), Some(1.0));
    // A value kept under an id by one call is found by every other call given that id.
    let part = id.with("fill");
    anim.begin_frame(Instant::now());
    assert_eq!(anim.value(part, 3.0_f32, LINEAR_100), 3.0);
    assert_eq!(anim.toggle(id.with("on"), true, LINEAR_100), 1.0);
    assert_eq!(
        anim.presence(id.with("there"), false, LINEAR_100, LINEAR_100),
        0.0
    );
    anim.keep(id);
    assert_eq!(anim.end_frame(), None);
    assert_eq!(anim.get::<f32>(part), Some(3.0));
    assert!(anim.settled(part));
    assert_eq!(anim.len(), 4);
    anim.forget(part);
    assert_eq!(anim.get::<f32>(part), None);
    assert_eq!(
        anim.get::<f32>(("a", "fill")),
        None,
        "a part is not a tuple"
    );
}

#[test]
fn a_value_too_large_for_an_f32_moves_smoothly_and_lands_exactly() {
    // Fifty million: an `f32` there has steps of four.
    const FAR: f64 = 5.0e7;
    assert_eq!((FAR + 1.0) as f32, FAR as f32, "the test's own premise");
    for motion in [LINEAR_100, Motion::spring(Spring::SNAPPY)] {
        let clock = ManualClock::new();
        let mut anim = Animator::new();
        let frame = |anim: &mut Animator, ms: u64, target: f64| {
            clock.advance_ms(ms);
            anim.begin_frame(clock.now());
            let v = anim.value("offset", target, motion);
            (v, anim.end_frame().is_some())
        };
        frame(&mut anim, 0, FAR);
        let target = FAR + 30.5;
        let (mut last, mut steps) = (FAR, Vec::new());
        for n in 0.. {
            let (v, more) = frame(&mut anim, if n == 0 { 0 } else { 5 }, target);
            assert!(v >= last && v <= target, "{motion:?}: {v} after {last}");
            if v > last {
                steps.push(v - last);
            }
            last = v;
            if !more {
                break;
            }
            assert!(n < 400, "{motion:?} never arrives");
        }
        // It ends on the target to the last digit, not on the nearest `f32`.
        assert_eq!(last, target, "{motion:?}");
        assert_eq!(anim.get::<f64>("offset"), Some(target));
        // And on the way it moved in steps an `f32` could not take there: many of them,
        // most smaller than four, none a jump.
        let fine = steps.iter().filter(|s| **s < 4.0).count();
        assert!(steps.len() >= 15 && fine >= 10, "{motion:?}: {steps:?}");
        assert!(steps.iter().all(|s| *s < 8.0), "{motion:?}: {steps:?}");
    }
}

#[test]
fn a_spring_stops_asking_for_frames_when_the_value_can_no_longer_be_seen_to_change() {
    // A colour: once the colour read is the target's, at most one more frame.
    let spring = Motion::spring(Spring::SMOOTH);
    let (from, to): (Rgb, Rgb) = ([0, 0, 0], [255, 100, 1]);
    let clock = ManualClock::new();
    let mut anim = Animator::new();
    let frame = |anim: &mut Animator, ms: u64, colour: Rgb| {
        clock.advance_ms(ms);
        anim.begin_frame(clock.now());
        let c = anim.value("colour", colour, spring);
        (c, anim.end_frame().is_some())
    };
    frame(&mut anim, 0, from);
    frame(&mut anim, 0, to);
    let (mut frames, mut same) = (0, 0);
    loop {
        let (c, more) = frame(&mut anim, 10, to);
        frames += 1;
        same += usize::from(c == to);
        if !more {
            assert_eq!(c, to);
            break;
        }
        assert!(frames < 200, "the colour never settles");
    }
    assert!(same <= 2, "{same} frames drew the same colour");
    // About a third of a second for the default spring; it was twice that when a colour
    // was held to a thousandth of a step.
    assert!((25..=38).contains(&frames), "{frames} frames of 10 ms");

    // The precisions the types give, and that a spring's own coarser threshold wins.
    assert_eq!(<f32 as Animatable>::precision(), 0.001);
    assert_eq!(<f64 as Animatable>::precision(), 0.001);
    assert_eq!(<[f32; 4] as Animatable>::precision(), 0.001);
    assert_eq!(<(f32, f32) as Animatable>::precision(), 0.25);
    assert_eq!(<Rect as Animatable>::precision(), 0.25);
    assert!(<Rgb as Animatable>::precision() < 0.5);
    let frames_for = |target: Rect, spring: Spring| {
        let clock = ManualClock::new();
        let mut anim = Animator::new();
        let rest = Rect::new(0.0, 0.0, 10.0, 10.0);
        anim.begin_frame(clock.now());
        anim.value("r", rest, Motion::spring(spring));
        anim.end_frame();
        let mut frames = 0;
        loop {
            clock.advance_ms(10);
            anim.begin_frame(clock.now());
            let r = anim.value("r", target, Motion::spring(spring));
            frames += 1;
            if anim.end_frame().is_none() {
                assert_eq!(r, target);
                return frames;
            }
            assert!(frames < 400);
        }
    };
    let far = Rect::new(300.0, 0.0, 10.0, 10.0);
    let by_type = frames_for(far, Spring::SMOOTH);
    let coarse = frames_for(far, Spring::SMOOTH.with_threshold(5.0));
    let fine = frames_for(far, Spring::SMOOTH.with_threshold(1e-6));
    assert!(coarse < by_type, "{coarse} and {by_type}");
    assert_eq!(
        fine, by_type,
        "a finer threshold than can be seen is not followed"
    );
}

/// An animator on a clock that is moved by hand, with frames of 10 ms.
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

    /// One frame at the clock's time that asks for one value.
    fn frame(&mut self, target: f32, motion: Motion) -> (f32, Option<Instant>) {
        self.anim.begin_frame(self.clock.now());
        let v = self.anim.value("x", target, motion);
        (v, self.anim.end_frame())
    }

    /// The clock moved by `ms`, then one frame.
    fn after(&mut self, ms: u64, target: f32, motion: Motion) -> (f32, Option<Instant>) {
        self.clock.advance_ms(ms);
        self.frame(target, motion)
    }
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-3
}

#[test]
fn a_value_first_seen_is_its_target_and_asks_for_no_frame() {
    let mut r = Rig::new();
    assert_eq!(r.frame(7.0, LINEAR_100), (7.0, None));
    assert!(!r.anim.animating());
    // Idle for an hour: still nothing.
    assert_eq!(r.after(3_600_000, 7.0, LINEAR_100), (7.0, None));
    assert!(r.anim.settled("x"));
    assert!(r.anim.settled("never heard of"));
}

#[test]
fn a_tween_runs_its_duration_and_then_the_window_sleeps() {
    let mut r = Rig::new();
    r.frame(0.0, LINEAR_100);
    // The frame that brings the new target draws the old value and asks for the next one,
    // one frame interval after its own start.
    let (v, deadline) = r.after(500, 1.0, LINEAR_100);
    assert_eq!(v, 0.0);
    assert_eq!(deadline, Some(r.clock.now() + FRAME));
    assert!(r.anim.animating());
    for step in 1..10 {
        let (v, deadline) = r.after(10, 1.0, LINEAR_100);
        assert!(close(v, step as f32 / 10.0), "step {step}: {v}");
        assert_eq!(deadline, Some(r.clock.now() + FRAME));
    }
    // The frame on which it arrives is drawn at the target and asks for nothing more.
    assert_eq!(r.after(10, 1.0, LINEAR_100), (1.0, None));
    assert!(!r.anim.animating());
    assert_eq!(r.after(10, 1.0, LINEAR_100), (1.0, None));
}

#[test]
fn a_tween_follows_its_easing() {
    let mut r = Rig::new();
    let motion = Motion::tween_ms(100, Easing::InQuad);
    r.frame(10.0, motion);
    r.frame(20.0, motion);
    assert!(close(r.after(50, 20.0, motion).0, 12.5));
    assert!(close(r.after(40, 20.0, motion).0, 18.1));
    assert_eq!(motion.length(), Some(Duration::from_millis(100)));
    assert_eq!(Motion::spring(Spring::SMOOTH).length(), None);
    assert_eq!(Motion::SNAP.length(), Some(Duration::ZERO));
    let wait = Duration::from_millis(300);
    assert_eq!(Motion::SNAP.delayed(wait).length(), Some(wait));
}

#[test]
fn a_delay_too_long_for_the_clock_never_panics_and_asks_for_no_busy_frames() {
    let never = fluent::FADE.delayed(Duration::MAX);
    assert_eq!(never.length(), Some(Duration::MAX));
    assert_eq!(
        Motion::spring(Spring::SMOOTH)
            .delayed(Duration::MAX)
            .length(),
        None
    );
    let mut r = Rig::new();
    // First seen: it is its target, as always.
    assert_eq!(r.frame(1.0, never), (1.0, None));
    // A new target waits for ever: the old value stays, and no frame is asked for.
    assert_eq!(r.after(10, 0.0, never), (1.0, None));
    assert_eq!(r.after(3_600_000, 0.0, never), (1.0, None));
    // So does a snap after such a wait, and one with reduced motion.
    assert_eq!(
        r.after(10, 2.0, Motion::SNAP.delayed(Duration::MAX)),
        (1.0, None)
    );
    r.anim.set_policy(Policy::Never);
    assert_eq!(r.after(10, 3.0, never), (1.0, None));
    // A later target with no wait is taken as usual.
    assert_eq!(r.after(10, 4.0, Motion::SNAP), (4.0, None));
}

#[test]
fn a_snap_with_an_unchanged_target_leaves_the_motion_in_flight_alone() {
    for motion in [LINEAR_100, Motion::spring(Spring::SNAPPY)] {
        let mut r = Rig::new();
        r.frame(0.0, motion);
        r.after(10, 1.0, motion);
        let (moving, _) = r.after(30, 1.0, motion);
        assert!(moving > 0.0 && moving < 1.0, "{motion:?}: {moving}");
        // The same target, now with a snap: it goes on moving, as for any other motion.
        let (v, deadline) = r.after(10, 1.0, Motion::SNAP);
        assert!(
            v > moving && v < 1.0,
            "{motion:?}: the snap stopped it at {v}"
        );
        assert!(deadline.is_some());
        assert!(!r.anim.settled("x"));
        // A snap with a new target is there at once.
        assert_eq!(r.after(10, 5.0, Motion::SNAP), (5.0, None));
        // And with motion off, the one in flight arrives, even with its target unchanged.
        r.after(10, 0.0, motion);
        r.after(30, 0.0, motion);
        r.anim.set_policy(Policy::Never);
        assert_eq!(r.after(10, 0.0, motion), (0.0, None));
    }
}

#[test]
fn a_delay_is_kept_by_a_snap_and_by_reduced_motion() {
    // A snap after a wait: the old value for the wait, one wake-up at its end, then the new
    // value at once and no further frame.
    let wait = Motion::SNAP.delayed(Duration::from_millis(300));
    let mut r = Rig::new();
    r.frame(0.0, wait);
    let (v, deadline) = r.frame(1.0, wait);
    assert_eq!(v, 0.0);
    assert_eq!(deadline, Some(r.clock.now() + Duration::from_millis(300)));
    // An event redraws on the way: still the old value, and the same moment to wake at.
    let (v, deadline) = r.after(100, 1.0, wait);
    assert_eq!(v, 0.0);
    assert_eq!(deadline, Some(r.clock.now() + Duration::from_millis(200)));
    assert_eq!(r.after(200, 1.0, wait), (1.0, None));
    // The target changed back during the wait: nothing ever shows.
    r.frame(0.0, wait);
    assert_eq!(r.after(100, 1.0, wait), (1.0, None), "it was there already");
    let (v, deadline) = r.after(10, 0.0, wait);
    assert!(v == 1.0 && deadline.is_some());
    assert_eq!(r.after(100, 1.0, wait), (1.0, None));

    // Reduced motion: a delayed tween keeps its delay and loses only its movement.
    let delayed = LINEAR_100.delayed(Duration::from_millis(400));
    for policy in [Policy::System, Policy::Never] {
        let mut r = Rig::new();
        r.anim.set_system_reduced(true);
        r.anim.set_policy(policy);
        r.frame(0.0, delayed);
        let (v, deadline) = r.frame(1.0, delayed);
        assert_eq!(v, 0.0, "{policy:?}");
        assert_eq!(deadline, Some(r.clock.now() + Duration::from_millis(400)));
        let (v, deadline) = r.after(399, 1.0, delayed);
        assert_eq!(v, 0.0);
        assert_eq!(deadline, Some(r.clock.now() + Duration::from_millis(10)));
        // Over: at the target at once, with nothing between and no further frame.
        assert_eq!(r.after(1, 1.0, delayed), (1.0, None));
    }

    // A tooltip: it appears half a second after it is asked for, and under reduced motion
    // it still waits, then is simply there; hidden, it is gone at once.
    let enter = fluent::ENTER.delayed(Duration::from_millis(500));
    let mut r = Rig::new();
    r.anim.set_system_reduced(true);
    let tip = |r: &mut Rig, ms: u64, shown: bool| {
        r.clock.advance_ms(ms);
        r.anim.begin_frame(r.clock.now());
        let t = r.anim.presence("tip", shown, enter, fluent::EXIT);
        (t, r.anim.end_frame())
    };
    let (t, deadline) = tip(&mut r, 0, true);
    assert_eq!(t, 0.0);
    assert_eq!(deadline, Some(r.clock.now() + Duration::from_millis(500)));
    assert_eq!(tip(&mut r, 250, true).0, 0.0);
    assert_eq!(tip(&mut r, 250, true), (1.0, None));
    assert_eq!(tip(&mut r, 1000, false), (0.0, None));

    // Motion switched off while a delayed tween is on its way (past its wait): it arrives.
    let mut r = Rig::new();
    let short = LINEAR_100.delayed(Duration::from_millis(20));
    r.frame(0.0, short);
    r.frame(1.0, short);
    r.after(20, 1.0, short);
    assert!(close(r.after(10, 1.0, short).0, 0.1));
    r.anim.set_policy(Policy::Never);
    assert_eq!(r.after(10, 1.0, short), (1.0, None));
}

#[test]
fn a_value_that_is_kept_goes_on_to_its_target_and_then_the_window_sleeps() {
    for motion in [LINEAR_100, Motion::spring(Spring::SNAPPY)] {
        let mut r = Rig::new();
        r.frame(0.0, motion);
        r.frame(1.0, motion);
        // From here on no frame asks for the value: the part it is in is not drawn.
        let kept = |r: &mut Rig, ms: u64| {
            r.clock.advance_ms(ms);
            r.anim.begin_frame(r.clock.now());
            r.anim.keep("x");
            // Kept, it is at this frame's time already.
            let v = r.anim.get::<f32>("x").unwrap();
            (v, r.anim.end_frame())
        };
        let (first, deadline) = kept(&mut r, 30);
        assert!(first > 0.0 && first < 1.0, "{motion:?}: {first}");
        assert_eq!(deadline, Some(r.clock.now() + FRAME));
        if motion == LINEAR_100 {
            assert!(close(first, 0.3));
        }
        // It moves on every kept frame, arrives, and the frames stop: before the fix it
        // stood still here and asked for a frame every interval for ever.
        let (mut last, mut frames) = (first, 0);
        loop {
            let (v, deadline) = kept(&mut r, 10);
            assert!(v >= last, "{motion:?}: {v} after {last}");
            last = v;
            frames += 1;
            if deadline.is_none() {
                break;
            }
            assert!(
                frames < 100,
                "{motion:?}: a kept value never stops asking for frames"
            );
        }
        assert_eq!(last, 1.0);
        assert!(r.anim.settled("x") && !r.anim.animating());
        // Kept at rest: remembered, and nothing asked.
        assert_eq!(kept(&mut r, 1000), (1.0, None));
        // Asked for again, it is where it arrived.
        assert_eq!(r.after(10, 1.0, motion), (1.0, None));
    }

    // A value kept while it waits out a delay sleeps to the end of the delay, not a frame.
    let delayed = LINEAR_100.delayed(Duration::from_millis(400));
    let mut r = Rig::new();
    r.frame(0.0, delayed);
    r.frame(1.0, delayed);
    r.clock.advance_ms(100);
    r.anim.begin_frame(r.clock.now());
    r.anim.keep("x");
    assert_eq!(
        r.anim.end_frame(),
        Some(r.clock.now() + Duration::from_millis(300))
    );
    // Motion switched off while it is kept: it is there after its wait.
    r.anim.set_policy(Policy::Never);
    r.clock.advance_ms(300);
    r.anim.begin_frame(r.clock.now());
    r.anim.keep("x");
    assert_eq!(r.anim.get::<f32>("x"), Some(1.0));
    assert_eq!(r.anim.end_frame(), None);
    // A name not known is not made by keeping it.
    r.anim.begin_frame(r.clock.now());
    r.anim.keep("nobody");
    r.anim.end_frame();
    assert!(r.anim.is_empty());
}

#[test]
fn a_spring_that_starts_where_it_already_is_carries_no_speed() {
    let spring = Motion::spring(Spring::SMOOTH);
    let mut r = Rig::new();
    r.frame(0.0, spring);
    r.frame(100.0, spring);
    let (a, _) = r.after(30, 100.0, spring);
    assert!(a > 0.0 && a < 100.0);
    // The target is moved to exactly where the value stands: it is at rest there, at once.
    let (b, deadline) = r.after(10, 100.0, spring);
    assert!(b > a);
    let here = r.anim.get::<f32>("x").unwrap();
    assert_eq!(r.frame(here, spring), (here, None));
    // The next move starts from rest: its first step is small, where a speed carried over
    // from the flight before would have thrown it on by about as much as the step `a..b`.
    assert!(deadline.is_some());
    r.frame(here + 100.0, spring);
    let (c, _) = r.after(10, here + 100.0, spring);
    // A second way to the same number: the spring's own step from rest.
    let (want, _) = Spring::SMOOTH.step(0.0, 0.0, 100.0, 0.01);
    assert!(close(c - here, want), "{} and {want}", c - here);
    assert!(
        c - here < (b - a) * 0.6,
        "it kept a speed: {} after {}",
        c - here,
        b - a
    );
}

#[test]
fn the_frame_interval_is_never_less_than_a_millisecond() {
    let mut r = Rig::new();
    r.anim.set_frame_interval(Duration::ZERO);
    r.frame(0.0, LINEAR_100);
    let (_, deadline) = r.frame(1.0, LINEAR_100);
    assert_eq!(deadline, Some(r.clock.now() + Duration::from_millis(1)));
    r.anim.set_frame_interval(Duration::from_millis(4));
    let (_, deadline) = r.after(4, 1.0, LINEAR_100);
    assert_eq!(deadline, Some(r.clock.now() + Duration::from_millis(4)));
}

#[test]
fn the_same_target_twice_in_a_frame_is_one_value() {
    let mut r = Rig::new();
    r.frame(0.0, LINEAR_100);
    r.clock.advance_ms(10);
    r.anim.begin_frame(r.clock.now());
    assert_eq!(r.anim.value("x", 1.0_f32, LINEAR_100), 0.0);
    assert_eq!(r.anim.value("x", 1.0_f32, LINEAR_100), 0.0);
    // A jump and then the value from it, as a dragged thumb does, is no mistake either.
    r.anim.set("x", 0.5_f32);
    assert_eq!(r.anim.value("x", 0.5_f32, LINEAR_100), 0.5);
    assert_eq!(r.anim.end_frame(), None);
    // Nor are targets that are not numbers, which never compare equal.
    r.anim.begin_frame(r.clock.now());
    assert!(r.anim.value("x", f32::NAN, LINEAR_100).is_nan());
    assert!(r.anim.value("x", f32::NAN, LINEAR_100).is_nan());
    r.anim.end_frame();
}

/// In a debug build the mistake is named where it is made. (A release build takes the
/// later target, as it always did.)
#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "two targets in one frame")]
fn two_targets_for_one_name_in_one_frame_are_caught_in_a_debug_build() {
    let mut r = Rig::new();
    r.frame(0.0, LINEAR_100);
    r.anim.begin_frame(r.clock.now());
    r.anim.value("x", 1.0_f32, LINEAR_100);
    r.anim.value("x", 2.0_f32, LINEAR_100);
}

#[test]
fn a_spring_with_no_damping_swings_for_as_long_as_it_is_asked_for() {
    for damping in [0.0, -5.0] {
        let swing = Motion::spring(Spring::new(400.0, damping));
        let mut r = Rig::new();
        r.frame(0.0, swing);
        r.frame(1.0, swing);
        let (mut low, mut high) = (f32::MAX, f32::MIN);
        for _ in 0..500 {
            let (v, deadline) = r.after(10, 1.0, swing);
            assert!(deadline.is_some(), "it never comes to rest");
            (low, high) = (low.min(v), high.max(v));
        }
        // It swings between where it started and as far beyond the target, no further.
        assert!(low > -0.01 && low < 0.05, "damping {damping}: lowest {low}");
        assert!(
            high < 2.01 && high > 1.95,
            "damping {damping}: highest {high}"
        );
        // Not asked for: forgotten like any other, and the window sleeps.
        r.clock.advance_ms(10);
        r.anim.begin_frame(r.clock.now());
        assert_eq!(r.anim.end_frame(), None);
    }
}

#[test]
fn a_new_target_on_the_way_starts_from_where_the_value_is() {
    let mut r = Rig::new();
    r.frame(0.0, LINEAR_100);
    r.frame(1.0, LINEAR_100);
    let (before, _) = r.after(40, 1.0, LINEAR_100);
    assert!(close(before, 0.4));
    // Turned round: the frame that says so draws no jump.
    let (turn, deadline) = r.after(10, 0.0, LINEAR_100);
    assert!(close(turn, 0.5));
    assert!(deadline.is_some());
    // And it takes the whole duration back from there: half of the way in 50 ms.
    assert!(close(r.after(50, 0.0, LINEAR_100).0, 0.25));
    assert!(close(r.after(40, 0.0, LINEAR_100).0, 0.05));
    assert_eq!(r.after(10, 0.0, LINEAR_100), (0.0, None));
}

#[test]
fn a_spring_settles_and_keeps_its_speed_when_its_target_moves() {
    let spring = Motion::spring(Spring::SMOOTH);
    let mut r = Rig::new();
    r.frame(0.0, spring);
    r.frame(100.0, spring);
    let mut last = 0.0;
    let mut speed_before = 0.0;
    for _ in 0..8 {
        let (v, deadline) = r.after(10, 100.0, spring);
        assert!(v > last && v < 100.0);
        assert!(deadline.is_some());
        speed_before = v - last;
        last = v;
    }
    // A further target: no jump, and the next step is no slower than the last (a tween
    // would have started again from rest).
    let (v, _) = r.after(10, 200.0, spring);
    let step = v - last;
    assert!(step > 0.0 && (step - speed_before).abs() < speed_before);
    let (next, _) = r.after(10, 200.0, spring);
    assert!(next - v >= speed_before * 0.9, "it lost its speed");
    // It arrives exactly, and then asks for nothing.
    let mut frames = 0;
    loop {
        let (v, deadline) = r.after(10, 200.0, spring);
        frames += 1;
        if deadline.is_none() {
            assert_eq!(v, 200.0);
            break;
        }
        assert!(frames < 300, "the spring never settles");
    }
    assert!(r.anim.settled("x"));
}

#[test]
fn a_stall_does_not_throw_an_animation_to_its_end() {
    let mut r = Rig::new();
    r.frame(0.0, LINEAR_100);
    r.frame(1.0, LINEAR_100);
    assert!(close(r.after(20, 1.0, LINEAR_100).0, 0.2));
    // The window was minimised for a minute: the frame after it moves by one step of at
    // most 50 ms, not to the end.
    let (v, deadline) = r.after(60_000, 1.0, LINEAR_100);
    assert!(close(v, 0.7), "after the stall it is at {v}");
    assert!(deadline.is_some());
    assert!(close(r.after(10, 1.0, LINEAR_100).0, 0.8));
    // A shorter limit, set by the application.
    r.anim.set_max_step(Duration::from_millis(15));
    assert!(close(r.after(60_000, 1.0, LINEAR_100).0, 0.95));
    // A clock that goes back moves nothing.
    r.clock.set(Duration::ZERO);
    assert!(close(r.frame(1.0, LINEAR_100).0, 0.95));
    // A limit below the frame interval is the frame interval: the wait the animator asked
    // for itself is never a stall.
    r.frame(0.0, LINEAR_100);
    r.anim.set_max_step(Duration::from_millis(1));
    assert!(close(r.after(10, 0.0, LINEAR_100).0, 0.855));
}

#[test]
fn a_delay_holds_the_value_and_lets_the_window_sleep_until_it_is_over() {
    let delayed = LINEAR_100.delayed(Duration::from_millis(400));
    assert_eq!(delayed.length(), Some(Duration::from_millis(500)));
    let mut r = Rig::new();
    r.frame(0.0, delayed);
    // The deadline is the end of the delay, not the next frame.
    let (v, deadline) = r.frame(1.0, delayed);
    assert_eq!(v, 0.0);
    assert_eq!(deadline, Some(r.clock.now() + Duration::from_millis(400)));
    // An event redraws half way through the wait: still held, and the deadline is the same
    // moment.
    let (v, deadline) = r.after(150, 1.0, delayed);
    assert_eq!(v, 0.0);
    assert_eq!(deadline, Some(r.clock.now() + Duration::from_millis(250)));
    // The wait that was asked for is not a stall: woken at the deadline, the delay is over.
    let (v, deadline) = r.after(250, 1.0, delayed);
    assert_eq!(v, 0.0);
    assert_eq!(deadline, Some(r.clock.now() + FRAME));
    assert!(close(r.after(10, 1.0, delayed).0, 0.1));
    assert!(close(r.after(50, 1.0, delayed).0, 0.6));
    assert_eq!(r.after(40, 1.0, delayed), (1.0, None));
}

#[test]
fn a_name_no_frame_asks_for_is_forgotten_and_starts_fresh() {
    let mut r = Rig::new();
    r.frame(0.0, LINEAR_100);
    r.frame(1.0, LINEAR_100);
    r.after(30, 1.0, LINEAR_100);
    assert_eq!(r.anim.len(), 1);
    assert!(close(r.anim.get::<f32>("x").unwrap(), 0.3));
    // A frame that does not draw it: gone, and the window sleeps though it was in flight.
    r.clock.advance_ms(10);
    r.anim.begin_frame(r.clock.now());
    assert_eq!(r.anim.end_frame(), None);
    assert!(r.anim.is_empty());
    assert_eq!(r.anim.get::<f32>("x"), None);
    // Seen again, it is its target.
    assert_eq!(r.after(10, 1.0, LINEAR_100), (1.0, None));

    // Kept by hand, it stays and goes on moving.
    r.frame(0.0, LINEAR_100);
    r.clock.advance_ms(30);
    r.anim.begin_frame(r.clock.now());
    r.anim.keep("x");
    assert!(r.anim.end_frame().is_some());
    assert!(close(r.after(20, 0.0, LINEAR_100).0, 0.5));
    // Forgotten by hand.
    r.anim.forget("x");
    assert_eq!(r.frame(0.0, LINEAR_100), (0.0, None));
}

#[test]
fn reduced_motion_makes_every_value_its_target_at_once() {
    let mut r = Rig::new();
    r.anim.set_system_reduced(true);
    assert!(!r.anim.animates());
    r.frame(0.0, LINEAR_100);
    assert_eq!(r.frame(1.0, LINEAR_100), (1.0, None));
    assert_eq!(r.frame(5.0, Motion::spring(Spring::BOUNCY)), (5.0, None));
    r.anim.begin_frame(r.clock.now());
    assert_eq!(r.anim.presence("p", true, LINEAR_100, LINEAR_100), 1.0);
    assert_eq!(r.anim.value_from("q", 0.0, 9.0, LINEAR_100), 9.0);
    assert_eq!(r.anim.end_frame(), None);

    // The application's switch wins over the system, both ways.
    r.anim.set_policy(Policy::Always);
    assert!(r.anim.animates());
    r.frame(5.0, LINEAR_100);
    let (v, deadline) = r.frame(6.0, LINEAR_100);
    assert_eq!(v, 5.0);
    assert!(deadline.is_some());
    // Switched off in flight: the value in flight arrives on the next frame.
    r.anim.set_system_reduced(false);
    r.anim.set_policy(Policy::Never);
    assert_eq!(r.after(10, 6.0, LINEAR_100), (6.0, None));
    r.anim.set_policy(Policy::System);
    assert_eq!(r.anim.policy(), Policy::System);
    assert!(r.anim.animates());
}

#[test]
fn snap_and_set_do_not_animate_and_finish_all_ends_what_does() {
    let mut r = Rig::new();
    r.frame(0.0, LINEAR_100);
    assert_eq!(r.frame(1.0, Motion::SNAP), (1.0, None));
    assert_eq!(r.frame(2.0, Motion::tween_ms(0, Easing::Linear)).0, 1.0);
    assert_eq!(r.after(1, 2.0, LINEAR_100), (2.0, None));
    // A jump, and a move from it.
    r.anim.begin_frame(r.clock.now());
    r.anim.set("x", 50.0);
    assert_eq!(r.anim.end_frame(), None);
    assert_eq!(r.frame(60.0, LINEAR_100).0, 50.0);
    assert!(close(r.after(50, 60.0, LINEAR_100).0, 55.0));
    r.anim.finish_all();
    assert_eq!(r.frame(60.0, LINEAR_100), (60.0, None));
    // A target that is not a number is taken as it is and asks for no frame.
    let (v, deadline) = r.frame(f32::NAN, LINEAR_100);
    assert!(v.is_nan());
    assert_eq!(deadline, None);
    // And the next real target is taken at once: there is nowhere to move from.
    assert_eq!(r.frame(3.0, LINEAR_100), (3.0, None));
}

#[test]
fn presence_enters_from_nothing_and_leaves_to_nothing() {
    let mut r = Rig::new();
    r.anim.set_max_step(Duration::from_secs(1));
    let frame = |r: &mut Rig, ms: u64, shown: bool| {
        r.clock.advance_ms(ms);
        r.anim.begin_frame(r.clock.now());
        let t = r.anim.presence("toast", shown, LINEAR_100, LINEAR_100);
        (t, r.anim.end_frame())
    };
    // Not shown and never seen: nothing, and no frame.
    assert_eq!(frame(&mut r, 0, false), (0.0, None));
    // Shown: it enters from 0, unlike a toggle.
    let (t, deadline) = frame(&mut r, 0, true);
    assert_eq!(t, 0.0);
    assert!(deadline.is_some());
    assert!(close(frame(&mut r, 60, true).0, 0.6));
    // Hidden on the way in: it leaves from where it is.
    assert!(close(frame(&mut r, 10, false).0, 0.7));
    assert!(close(frame(&mut r, 50, false).0, 0.35));
    assert_eq!(frame(&mut r, 50, false), (0.0, None));
    // A toggle first seen on is on.
    r.anim.begin_frame(r.clock.now());
    assert_eq!(r.anim.toggle("tick", true, LINEAR_100), 1.0);
    r.anim.end_frame();
}

#[test]
fn a_wake_up_the_application_asks_for_is_the_deadline_once() {
    let mut r = Rig::new();
    let at = r.clock.now() + Duration::from_secs(4);
    r.anim.wake_at(at + Duration::from_secs(1));
    r.anim.wake_at(at);
    assert_eq!(r.frame(0.0, LINEAR_100).1, Some(at));
    assert_eq!(r.frame(0.0, LINEAR_100).1, None);
    // Something in flight is sooner than a far wake-up.
    r.anim.wake_at(at);
    assert_eq!(r.frame(1.0, LINEAR_100).1, Some(r.clock.now() + FRAME));
}

#[test]
fn rects_points_and_colours_move_part_by_part() {
    let mut r = Rig::new();
    let (a, b) = (
        Rect::new(0.0, 10.0, 20.0, 30.0),
        Rect::new(100.0, 10.0, 40.0, 30.0),
    );
    let frame = |r: &mut Rig, ms: u64, rect: Rect, point: (f32, f32), colour: Rgb| {
        r.clock.advance_ms(ms);
        r.anim.begin_frame(r.clock.now());
        let out = (
            r.anim.value("rect", rect, LINEAR_100),
            r.anim.value("point", point, LINEAR_100),
            r.anim.value("colour", colour, LINEAR_100),
        );
        r.anim.end_frame();
        out
    };
    frame(&mut r, 0, a, (0.0, 0.0), [0, 0, 0]);
    frame(&mut r, 0, b, (8.0, -4.0), [255, 100, 1]);
    let (rect, point, colour) = frame(&mut r, 50, b, (8.0, -4.0), [255, 100, 1]);
    assert_eq!(rect, Rect::new(50.0, 10.0, 30.0, 30.0));
    assert_eq!(point, (4.0, -2.0));
    // Half way in the stored space, as `gfx::mix` gives it.
    assert_eq!(colour, crate::gfx::mix([0, 0, 0], [255, 100, 1], 0.5));
    assert_eq!(a.lerp(b, 0.25), Rect::new(25.0, 10.0, 25.0, 30.0));
    assert_eq!([0u8, 0, 0].lerp([10, 20, 30], 1.5), [15, 30, 45]);
    assert_eq!([250u8, 0, 0].lerp([255, 0, 0], 9.0), [255, 0, 0]);
    assert_eq!(
        [1.0f32, 2.0, 3.0, 4.0].lerp([3.0; 4], 0.5),
        [2.0, 2.5, 3.0, 3.5]
    );
}

#[test]
fn names_hash_the_same_every_time_and_parts_differ() {
    assert_eq!(Id::new("a"), Id::new("a"));
    assert_ne!(Id::new("a"), Id::new("b"));
    let id = Id::new(("switch", 3));
    assert_eq!(id.with("pill"), id.with("pill"));
    assert_ne!(id.with("pill"), id.with("hover"));
    assert_ne!(id.with(0), id.with(1));
    // An `Id` is itself a name, and names the same value each time.
    let mut anim = Animator::new();
    anim.set(id, 4.0);
    assert_eq!(anim.get::<f32>(id), Some(4.0));
    assert_eq!(anim.get::<f32>(id.with("pill")), None);
}

#[test]
fn the_real_clock_moves_forward() {
    let clock = SystemClock;
    let a = clock.now();
    assert!(clock.now() >= a);
    let manual = ManualClock::default();
    let start = manual.now();
    manual.advance_ms(250);
    assert_eq!(manual.now() - start, Duration::from_millis(250));
    assert_eq!(manual.elapsed(), Duration::from_millis(250));
}

#[test]
fn the_fluent_timings_are_whole_frames_at_sixty_hertz() {
    for (d, frames) in [
        (fluent::FASTER, 5.0),
        (fluent::FAST, 10.0),
        (fluent::NORMAL, 15.0),
        (fluent::SLOW, 20.0),
    ] {
        assert!((d.as_secs_f32() * 60.0 - frames).abs() < 0.05, "{d:?}");
    }
    // Entering decelerates, leaving accelerates, and a move sets off more gently than an
    // entrance and still lands softly.
    assert!(fluent::DECELERATE.at(0.25) > 0.6);
    assert!(fluent::ACCELERATE.at(0.75) < 0.4);
    assert!(fluent::POINT_TO_POINT.at(0.1) < fluent::DECELERATE.at(0.1));
    assert!(fluent::POINT_TO_POINT.at(0.5) > 0.8);
    assert_eq!(fluent::ENTER.length(), Some(fluent::FAST));
    assert_eq!(fluent::web::CURVE_LINEAR.at(0.3), 0.3);
}

#[test]
fn groups_say_which_regions_still_move_and_when() {
    let clock = ManualClock::new();
    let mut anim = Animator::new();
    let blink = Duration::from_millis(500);
    // A toast entering, the caret blinking, and nothing outside a group.
    anim.begin_frame(clock.now());
    anim.group("toast");
    let t = anim.presence("toast-in", true, LINEAR_100, LINEAR_100);
    anim.group("caret");
    anim.blink("caret", blink, blink);
    anim.ungroup();
    anim.value("still", 1.0_f32, LINEAR_100);
    let deadline = anim.end_frame().expect("something moves");
    assert_eq!(t, 0.0);
    let at = clock.now();
    assert!(anim.group_due("toast", deadline));
    assert!(
        !anim.group_due("caret", deadline),
        "the caret waits out its half-period"
    );
    assert!(anim.group_due("caret", at + blink));
    assert_eq!(anim.ungrouped_deadline(), None);
    assert_eq!(anim.group_deadlines().len(), 2);
    // Once the toast has arrived only the caret is listed.
    for _ in 0..12 {
        clock.advance(FRAME);
        anim.begin_frame(clock.now());
        anim.group("toast");
        anim.presence("toast-in", true, LINEAR_100, LINEAR_100);
        anim.group("caret");
        anim.blink("caret", blink, blink);
        anim.end_frame();
    }
    let names: Vec<Id> = anim.group_deadlines().iter().map(|g| g.0).collect();
    assert_eq!(names, vec![Id::new("caret")]);
}

/// A fade that arrives during a frame that only keeps its group was not drawn where it
/// arrived: the group is due once more, and then at rest.
#[test]
fn a_group_whose_motion_arrives_while_kept_is_due_once_more() {
    let clock = ManualClock::new();
    let mut anim = Animator::new();
    let toast = |anim: &mut Animator| {
        anim.group("toast");
        anim.presence("toast-in", false, LINEAR_100, LINEAR_100)
    };
    for _ in 0..10 {
        anim.begin_frame(clock.now());
        anim.group("toast");
        anim.presence("toast-in", true, LINEAR_100, LINEAR_100);
        anim.end_frame();
        clock.advance(FRAME);
    }
    // Leaving: drawn part of the way.
    for _ in 0..3 {
        clock.advance(FRAME);
        anim.begin_frame(clock.now());
        toast(&mut anim);
        anim.end_frame();
    }
    // A frame for the caret alone, after the fade's end: the toast is only kept.
    // (Frames for the caret alone, after the fade's end: the toast is only kept. A frame steps
    // at most 50 ms, so two of them.)
    clock.advance(Duration::from_millis(40));
    anim.begin_frame(clock.now());
    anim.keep_group("toast");
    anim.end_frame();
    clock.advance(Duration::from_millis(40));
    anim.begin_frame(clock.now());
    anim.keep_group("toast");
    let deadline = anim
        .end_frame()
        .expect("the toast's last state is still to be drawn");
    assert!(anim.group_due("toast", deadline));
    clock.advance(FRAME);
    anim.begin_frame(clock.now());
    assert_eq!(toast(&mut anim), 0.0);
    assert_eq!(anim.end_frame(), None);
    assert!(anim.group_deadlines().is_empty());
}

#[test]
fn a_kept_group_is_not_taken_for_new_after_a_frame_that_skipped_it() {
    let clock = ManualClock::new();
    let arrived = |keep: bool| {
        let mut anim = Animator::new();
        let ask = |anim: &mut Animator| {
            anim.group("toast");
            anim.presence("toast-in", true, LINEAR_100, LINEAR_100)
        };
        for _ in 0..15 {
            clock.advance(FRAME);
            anim.begin_frame(clock.now());
            ask(&mut anim);
            anim.end_frame();
        }
        // A frame that draws only the caret's line.
        clock.advance(FRAME);
        anim.begin_frame(clock.now());
        anim.group("caret");
        anim.blink(
            "caret",
            Duration::from_millis(500),
            Duration::from_millis(500),
        );
        if keep {
            anim.keep_group("toast");
        }
        anim.end_frame();
        clock.advance(FRAME);
        anim.begin_frame(clock.now());
        let t = ask(&mut anim);
        anim.end_frame();
        t
    };
    assert_eq!(arrived(true), 1.0, "kept: still there");
    assert_eq!(
        arrived(false),
        0.0,
        "not kept: forgotten, and it enters again"
    );
}
