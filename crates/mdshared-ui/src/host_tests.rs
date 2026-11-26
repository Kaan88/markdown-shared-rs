//! The window host's pure parts: the damage and its device rectangle, the folding of the
//! animator's deadline with the application's timer, the covered and minimized window, the
//! click count, the reduction of winit's positions, buttons and wheel to logical px, the copy of
//! the bitmap into the window's buffer, and the config's defaults. The window itself is run by
//! `examples/host.rs`.

use super::frame::*;
use super::input::*;
use super::timers::NamedTimers;
use super::*;
use crate::anim::Animator;
use crate::anim::{Clock, ManualClock, Motion};
use std::time::Duration;
use winit::dpi::PhysicalPosition;
use winit::event::{MouseButton, MouseScrollDelta};

/// A small LCG with a fixed seed: the same "random" input every run.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

// ---- damage ----

#[test]
fn no_damage_is_empty_and_gives_no_device_rectangle() {
    let d = Damage::default();
    assert!(d.is_empty());
    assert_eq!(d.device(1.0, 100, 100), None);
}

#[test]
fn rectangles_are_joined_into_the_one_that_holds_them_all() {
    let mut d = Damage::default();
    d.add(Rect::new(10.0, 10.0, 5.0, 5.0));
    d.add(Rect::new(40.0, 2.0, 10.0, 3.0));
    assert!(!d.is_empty());
    assert_eq!(d.rect, Some(Rect::new(10.0, 2.0, 40.0, 13.0)));
}

#[test]
fn the_join_matches_the_least_and_greatest_edges_of_random_rectangles() {
    let mut rng = Lcg(7);
    for _ in 0..200 {
        let n = 1 + rng.below(6) as usize;
        let rects: Vec<Rect> = (0..n)
            .map(|_| {
                Rect::new(
                    rng.below(500) as f32,
                    rng.below(500) as f32,
                    1.0 + rng.below(200) as f32,
                    1.0 + rng.below(200) as f32,
                )
            })
            .collect();
        let mut d = Damage::default();
        for r in &rects {
            d.add(*r);
        }
        let x0 = rects.iter().map(|r| r.x).fold(f32::INFINITY, f32::min);
        let y0 = rects.iter().map(|r| r.y).fold(f32::INFINITY, f32::min);
        let x1 = rects.iter().map(|r| r.right()).fold(0.0, f32::max);
        let y1 = rects.iter().map(|r| r.bottom()).fold(0.0, f32::max);
        assert_eq!(d.rect, Some(Rect::new(x0, y0, x1 - x0, y1 - y0)));
        // And every rectangle's every corner is inside it.
        let u = d.rect.unwrap();
        for r in &rects {
            assert!(r.x >= u.x && r.y >= u.y && r.right() <= u.right() && r.bottom() <= u.bottom());
        }
    }
}

#[test]
fn empty_negative_and_non_numeric_rectangles_add_nothing() {
    let mut d = Damage::default();
    for r in [
        Rect::new(0.0, 0.0, 0.0, 10.0),
        Rect::new(0.0, 0.0, 10.0, -1.0),
        Rect::new(f32::NAN, 0.0, 10.0, 10.0),
        Rect::new(0.0, f32::INFINITY, 10.0, 10.0),
        Rect::new(0.0, 0.0, f32::NAN, 10.0),
        Rect::new(0.0, 0.0, 10.0, f32::INFINITY),
    ] {
        d.add(r);
    }
    assert!(d.is_empty());
}

#[test]
fn the_whole_window_is_the_whole_bitmap() {
    let mut d = Damage::default();
    d.add(Rect::new(3.0, 3.0, 1.0, 1.0));
    d.all();
    assert_eq!(d.device(1.5, 300, 200), Some((0, 0, 300, 200)));
}

#[test]
fn the_device_rectangle_is_rounded_outwards() {
    let mut d = Damage::default();
    d.add(Rect::new(10.3, 20.6, 5.0, 5.0));
    // 15.45..22.95 across, 30.9..38.4 down.
    assert_eq!(d.device(1.5, 1000, 1000), Some((15, 30, 23, 39)));
    // Every device pixel the rectangle touches is in it, by a second way: sample the
    // rectangle finely and see that each sample's pixel is inside.
    let (x0, y0, x1, y1) = d.device(1.5, 1000, 1000).unwrap();
    let r = d.rect.unwrap();
    for i in 0..=100 {
        for j in 0..=100 {
            let x = (r.x + r.w * i as f32 / 100.0) * 1.5;
            let y = (r.y + r.h * j as f32 / 100.0) * 1.5;
            let (px, py) = (x.floor() as u32, y.floor() as u32);
            assert!(px >= x0 && px <= x1 && py >= y0 && py <= y1, "{x} {y}");
        }
    }
}

#[test]
fn the_device_rectangle_is_cut_to_the_bitmap() {
    let mut d = Damage::default();
    d.add(Rect::new(-50.0, 90.0, 100.0, 100.0));
    assert_eq!(d.device(1.0, 100, 100), Some((0, 90, 50, 100)));
    let mut far = Damage::default();
    far.add(Rect::new(5000.0, 5000.0, 10.0, 10.0));
    assert_eq!(far.device(1.0, 100, 100), None);
    let mut huge = Damage::default();
    huge.add(Rect::new(-1e30, -1e30, 1e31, 1e31));
    assert_eq!(huge.device(2.0, 100, 100), Some((0, 0, 100, 100)));
}

#[test]
fn a_bitmap_of_no_size_has_no_damage_and_a_nonsense_scale_counts_as_one() {
    let mut d = Damage::default();
    d.add(Rect::new(1.0, 1.0, 2.0, 2.0));
    assert_eq!(d.device(1.0, 0, 100), None);
    assert_eq!(d.device(1.0, 100, 0), None);
    for s in [f32::NAN, 0.0, -2.0, f32::INFINITY] {
        assert_eq!(d.device(s, 100, 100), Some((1, 1, 3, 3)), "scale {s}");
    }
}

#[test]
fn merging_joins_the_rectangles_and_keeps_a_whole_window() {
    let mut a = Damage::default();
    a.add(Rect::new(0.0, 0.0, 10.0, 10.0));
    let mut b = Damage::default();
    b.add(Rect::new(20.0, 20.0, 10.0, 10.0));
    a.merge(b);
    assert_eq!(a.rect, Some(Rect::new(0.0, 0.0, 30.0, 30.0)));
    assert!(!a.full);
    let mut whole = Damage::default();
    whole.all();
    a.merge(whole);
    assert!(a.full);
    a.merge(Damage::default());
    assert!(a.full);
}

// ---- regions of damage ----

#[test]
fn two_far_apart_rectangles_stay_two_pieces_with_the_gap_between_them() {
    let a = Rect::new(10.0, 10.0, 20.0, 10.0);
    let b = Rect::new(200.0, 100.0, 20.0, 10.0);
    let mut d = Damage::with_pieces(true);
    d.add(a);
    d.add(b);
    assert_eq!(d.regions(), &[a, b][..]);
    assert_eq!(
        d.pieces(1.0, 400, 300),
        vec![(10, 10, 30, 20), (200, 100, 220, 110)]
    );
    // The union is one rectangle from one corner to the other, holding the gap; neither piece
    // does, so a frame that drew the union would paint {the gap} that nothing changed.
    assert_eq!(d.device(1.0, 400, 300), Some((10, 10, 220, 110)));
    for &(x0, y0, x1, y1) in &d.pieces(1.0, 400, 300) {
        assert!(
            !(x0 <= 120 && 120 < x1 && y0 <= 50 && 50 < y1),
            "piece holds the gap"
        );
    }
}

#[test]
fn overlapping_rectangles_join_into_one_piece_and_touching_ones_do_not() {
    let mut d = Damage::with_pieces(true);
    d.add(Rect::new(0.0, 0.0, 20.0, 20.0));
    d.add(Rect::new(10.0, 10.0, 20.0, 20.0));
    assert_eq!(d.regions(), &[Rect::new(0.0, 0.0, 30.0, 30.0)][..]);
    assert_eq!(d.pieces(1.0, 100, 100), vec![(0, 0, 30, 30)]);

    let mut touching = Damage::with_pieces(true);
    touching.add(Rect::new(0.0, 0.0, 10.0, 10.0));
    touching.add(Rect::new(10.0, 0.0, 10.0, 10.0));
    assert_eq!(
        touching.pieces(1.0, 100, 100),
        vec![(0, 0, 10, 10), (10, 0, 20, 10)]
    );
}

#[test]
fn a_join_that_grows_the_rectangle_restarts_the_scan_so_no_pieces_overlap() {
    // D, then A, then C: joining C with A grows it to reach D, which the scan had passed.
    let mut d = Damage::with_pieces(true);
    d.add(Rect::new(0.0, 0.0, 10.0, 5.0));
    d.add(Rect::new(20.0, 0.0, 10.0, 10.0));
    d.add(Rect::new(0.0, 8.0, 22.0, 22.0));
    assert_eq!(d.regions(), &[Rect::new(0.0, 0.0, 30.0, 30.0)][..]);
}

#[test]
fn random_rectangles_leave_pieces_that_never_overlap() {
    let mut rng = Lcg(19);
    for _ in 0..100 {
        let mut d = Damage::with_pieces(true);
        for _ in 0..10 {
            d.add(Rect::new(
                rng.below(200) as f32,
                rng.below(200) as f32,
                1.0 + rng.below(80) as f32,
                1.0 + rng.below(80) as f32,
            ));
        }
        let rs = d.regions();
        for i in 0..rs.len() {
            for j in i + 1..rs.len() {
                let (a, b) = (rs[i], rs[j]);
                let overlap =
                    a.x < b.right() && b.x < a.right() && a.y < b.bottom() && b.y < a.bottom();
                assert!(!overlap, "{a:?} overlaps {b:?}");
            }
        }
    }
}

#[test]
fn ten_thousand_disjoint_rectangles_stay_at_most_sixteen_pieces_and_the_union_holds_them() {
    let mut d = Damage::with_pieces(true);
    for i in 0..10_000 {
        d.add(Rect::new(
            (i % 100) as f32 * 20.0,
            (i / 100) as f32 * 20.0,
            5.0,
            5.0,
        ));
    }
    assert!(d.regions().len() <= KEEP_PIECES);
    let pieces = d.pieces(1.0, 5000, 5000);
    assert_eq!(
        pieces.len(),
        1,
        "ten thousand pieces fall back to the union"
    );
    assert_eq!(pieces[0], (0, 0, 1985, 1985));
    assert_eq!(d.device(1.0, 5000, 5000), Some((0, 0, 1985, 1985)));

    // An application that did not ask keeps no pieces at all, as before.
    let mut plain = Damage::default();
    for i in 0..10_000 {
        plain.add(Rect::new(i as f32 % 700.0, i as f32 / 700.0, 5.0, 5.0));
    }
    assert!(plain.regions().is_empty());
    assert!(plain.pieces(1.0, 1000, 1000).is_empty());
}

#[test]
fn an_application_that_did_not_ask_for_regions_keeps_no_pieces() {
    let mut d = Damage::default();
    d.add(Rect::new(0.0, 0.0, 10.0, 10.0));
    d.add(Rect::new(20.0, 20.0, 10.0, 10.0));
    assert!(d.regions().is_empty());
    assert!(d.pieces(1.0, 100, 100).is_empty());
    // The frame still gets the union, exactly as before.
    assert_eq!(d.device(1.0, 100, 100), Some((0, 0, 30, 30)));
}

#[test]
fn a_whole_window_is_one_piece_and_pieces_outside_the_bitmap_are_left_out() {
    let mut d = Damage::with_pieces(true);
    d.add(Rect::new(1.0, 1.0, 2.0, 2.0));
    d.all();
    assert!(d.regions().is_empty());
    assert_eq!(d.pieces(1.0, 100, 80), vec![(0, 0, 100, 80)]);

    let mut far = Damage::with_pieces(true);
    far.add(Rect::new(5000.0, 5000.0, 10.0, 10.0));
    far.add(Rect::new(2.0, 3.0, 4.0, 5.0));
    assert_eq!(far.pieces(1.0, 100, 80), vec![(2, 3, 6, 8)]);
    assert!(Damage::default().pieces(1.0, 0, 0).is_empty());
}

#[test]
fn the_hosts_region_loop_paints_each_piece_and_leaves_the_gap() {
    use super::run::paint_regions;
    use crate::gfx::{Gfx, Painter};
    use mdshared_text::Bitmap;

    let px = |bmp: &Bitmap, x: u32, y: u32| {
        let i = ((y * bmp.width + x) * 4) as usize;
        [bmp.pixels[i + 2], bmp.pixels[i + 1], bmp.pixels[i]]
    };
    let base = [10u8, 20, 30];
    let hot = [200u8, 10, 10];
    let mut bmp = Bitmap::new(240, 120);
    let mut gfx = Gfx::new();
    let mut anim = Animator::new();
    anim.begin_frame(Instant::now());
    {
        let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
        p.fill(Rect::new(0.0, 0.0, 240.0, 120.0), base);
    }
    let mut d = Damage::with_pieces(true);
    d.add(Rect::new(10.0, 10.0, 20.0, 10.0));
    d.add(Rect::new(200.0, 80.0, 20.0, 10.0));
    let pieces = d.pieces(1.0, 240, 120);
    assert_eq!(pieces.len(), 2);
    // The loop `Host::frame` runs for a regions application, with a closure standing in for
    // `App::draw`: one painter and clip per piece, the animator's group reset each time.
    let mut drawn = Vec::new();
    paint_regions(
        &mut bmp,
        &mut gfx,
        &mut anim,
        None,
        1.0,
        &pieces,
        |p, anim, region, _clip| {
            if drawn.is_empty() {
                // A group set in the first region must not leak into the second.
                anim.group("first");
            }
            drawn.push(region);
            let _: f32 = anim.value_from(
                format!("v{}", drawn.len()),
                0.0,
                1.0,
                Motion::tween_ms(100, crate::anim::Easing::Linear),
            );
            p.fill(Rect::new(0.0, 0.0, 240.0, 120.0), hot);
        },
    );
    let _ = anim.end_frame();
    assert!(
        anim.ungrouped_deadline().is_some(),
        "the second region's value is not in the first region's group"
    );
    assert_eq!(
        drawn,
        vec![
            Rect::new(10.0, 10.0, 20.0, 10.0),
            Rect::new(200.0, 80.0, 20.0, 10.0)
        ]
    );
    for (x, y) in [(15, 15), (205, 85)] {
        assert_eq!(px(&bmp, x, y), hot, "a changed region at ({x}, {y})");
    }
    // The gap inside the union but in neither piece still holds the old frame.
    for (x, y) in [(120, 50), (15, 85), (205, 15)] {
        assert_eq!(px(&bmp, x, y), base, "the gap at ({x}, {y})");
    }
}

// ---- deadlines ----

#[test]
fn with_no_deadline_the_loop_waits_for_an_event() {
    let d = Deadlines::default();
    assert_eq!(d.next(false), None);
    assert_eq!(d.next(true), None);
}

#[test]
fn the_earliest_of_the_animator_and_the_timer_wakes_the_loop() {
    let t0 = Instant::now();
    let (a, b) = (
        t0 + Duration::from_millis(16),
        t0 + Duration::from_millis(500),
    );
    let d = Deadlines {
        anim: Some(a),
        timer: Some(b),
    };
    assert_eq!(d.next(false), Some(a));
    let d = Deadlines {
        anim: Some(b),
        timer: Some(a),
    };
    assert_eq!(d.next(false), Some(a));
    let d = Deadlines {
        anim: None,
        timer: Some(b),
    };
    assert_eq!(d.next(false), Some(b));
}

#[test]
fn the_earliest_timer_asked_for_wins() {
    let t0 = Instant::now();
    let mut rng = Lcg(11);
    for _ in 0..100 {
        let mut d = Deadlines::default();
        let asks: Vec<Instant> = (0..1 + rng.below(5))
            .map(|_| t0 + Duration::from_millis(rng.below(10_000)))
            .collect();
        for &at in &asks {
            d.wake_at(at);
        }
        assert_eq!(d.timer, asks.iter().copied().min());
    }
}

#[test]
fn a_covered_window_wakes_only_for_the_timer_and_drops_the_animators_deadline() {
    let t0 = Instant::now();
    let mut d = Deadlines {
        anim: Some(t0 + Duration::from_millis(16)),
        timer: Some(t0 + Duration::from_millis(500)),
    };
    assert_eq!(d.next(true), Some(t0 + Duration::from_millis(500)));
    let due = d.take_due(t0, true);
    assert_eq!(due, Due::default());
    assert_eq!(d.anim, None);
    assert!(d.timer.is_some());
}

#[test]
fn only_the_deadlines_that_came_are_taken() {
    let t0 = Instant::now();
    let mut d = Deadlines {
        anim: Some(t0 + Duration::from_millis(16)),
        timer: Some(t0 + Duration::from_millis(500)),
    };
    assert_eq!(d.take_due(t0, false), Due::default());
    let due = d.take_due(t0 + Duration::from_millis(16), false);
    assert_eq!(
        due,
        Due {
            anim: true,
            timer: false
        }
    );
    assert_eq!(d.anim, None);
    let due = d.take_due(t0 + Duration::from_millis(600), false);
    assert_eq!(
        due,
        Due {
            anim: false,
            timer: true
        }
    );
    assert_eq!(d, Deadlines::default());
}

#[test]
fn an_animation_and_a_tooltip_delay_fold_into_the_earliest_of_the_two() {
    let clock = ManualClock::new();
    let mut anim = Animator::new();
    let t0 = clock.now();
    // A value set off on its way: the animator asks for the next frame one interval on.
    anim.begin_frame(t0);
    let _: f32 = anim.value("x", 0.0, Motion::tween_ms(200, crate::anim::Easing::Linear));
    let _ = anim.end_frame();
    clock.advance_ms(1);
    anim.begin_frame(clock.now());
    let _: f32 = anim.value("x", 1.0, Motion::tween_ms(200, crate::anim::Easing::Linear));
    let moving = anim
        .end_frame()
        .expect("a value in motion asks for a frame");
    let mut d = Deadlines {
        anim: Some(moving),
        timer: None,
    };
    // A tooltip 600 ms away: the animation's frame comes first.
    d.wake_at(clock.now() + Duration::from_millis(600));
    assert_eq!(d.next(false), Some(moving));
    // A timer sooner than the frame wins.
    d.wake_at(clock.now() + Duration::from_millis(2));
    assert_eq!(d.next(false), Some(clock.now() + Duration::from_millis(2)));
}

#[test]
fn a_blinking_caret_and_a_timer_fold_into_the_earliest_of_the_two() {
    let clock = ManualClock::new();
    let mut anim = Animator::new();
    let mut field = crate::textfield::TextField::new();
    let half = Duration::from_millis(530);
    anim.begin_frame(clock.now());
    assert!(field.blink_caret(&mut anim, "field", Some(half), true));
    let blink = anim.end_frame().expect("a focused caret blinks");
    assert_eq!(blink, clock.now() + half);
    let mut d = Deadlines {
        anim: Some(blink),
        timer: None,
    };
    d.wake_at(clock.now() + Duration::from_secs(3));
    assert_eq!(d.next(false), Some(blink));
    d.wake_at(clock.now() + Duration::from_millis(100));
    assert_eq!(
        d.next(false),
        Some(clock.now() + Duration::from_millis(100))
    );
    // Unfocused, the caret asks for nothing and only the timer is left.
    anim.begin_frame(clock.now());
    assert!(!field.blink_caret(&mut anim, "field", Some(half), false));
    d.anim = anim.end_frame();
    assert_eq!(d.next(false), d.timer);
}

#[test]
fn a_timer_asked_of_the_animator_comes_back_as_its_deadline() {
    let clock = ManualClock::new();
    let mut anim = Animator::new();
    anim.begin_frame(clock.now());
    anim.wake_at(clock.now() + Duration::from_millis(4000));
    let d = Deadlines {
        anim: anim.end_frame(),
        timer: Some(clock.now() + Duration::from_millis(5000)),
    };
    assert_eq!(
        d.next(false),
        Some(clock.now() + Duration::from_millis(4000))
    );
}

#[test]
fn the_loop_wakes_at_the_earliest_of_the_animator_and_a_named_timer() {
    let t0 = Instant::now();
    let ms = Duration::from_millis;
    let mut timers = NamedTimers::default();
    timers.set("autosave", t0 + ms(500));
    let anim = t0 + ms(16);
    let d = Deadlines {
        anim: Some(anim),
        timer: None,
    };
    // The animator's deadline comes first.
    assert_eq!(next_wake(&d, &timers, false), Some(anim));
    // A named timer sooner than it wins.
    timers.set("caret", t0 + ms(2));
    assert_eq!(next_wake(&d, &timers, false), Some(t0 + ms(2)));
    // Hidden, the animator's deadline is left out but the named timers stay.
    assert_eq!(next_wake(&d, &timers, true), Some(t0 + ms(2)));
    timers.cancel("caret");
    assert_eq!(next_wake(&d, &timers, true), Some(t0 + ms(500)));
    assert_eq!(
        next_wake(&Deadlines::default(), &NamedTimers::default(), true),
        None
    );
}

// ---- clicks ----

/// Windows' default double-click rectangle, 4 x 4 device px, at scale 1: 2 px each way.
const SLOP: (f32, f32) = (2.0, 2.0);

#[test]
fn presses_close_in_time_and_place_count_up() {
    let t0 = Instant::now();
    let ms = Duration::from_millis;
    let mut c = Clicks::default();
    let iv = ms(500);
    assert_eq!(c.press(t0, (10.0, 10.0), Button::Left, iv, SLOP), 1);
    assert_eq!(
        c.press(t0 + ms(200), (12.0, 9.0), Button::Left, iv, SLOP),
        2
    );
    assert_eq!(
        c.press(t0 + ms(400), (13.0, 10.0), Button::Left, iv, SLOP),
        3
    );
}

#[test]
fn a_late_far_or_other_press_starts_counting_again() {
    let t0 = Instant::now();
    let ms = Duration::from_millis;
    let iv = ms(500);
    let mut c = Clicks::default();
    c.press(t0, (10.0, 10.0), Button::Left, iv, SLOP);
    assert_eq!(
        c.press(t0 + ms(501), (10.0, 10.0), Button::Left, iv, SLOP),
        1
    );
    assert_eq!(
        c.press(t0 + ms(600), (20.0, 10.0), Button::Left, iv, SLOP),
        1
    );
    assert_eq!(
        c.press(t0 + ms(700), (20.0, 10.0), Button::Right, iv, SLOP),
        1
    );
    assert_eq!(
        c.press(t0 + ms(800), (20.0, 10.0), Button::Right, iv, SLOP),
        2
    );
    // A clock that went back is no time at all; a position that is not a number is no match.
    assert_eq!(c.press(t0, (20.0, 10.0), Button::Right, iv, SLOP), 3);
    assert_eq!(c.press(t0, (f32::NAN, 10.0), Button::Right, iv, SLOP), 1);
}

#[test]
fn the_click_count_stops_at_three() {
    let t0 = Instant::now();
    let iv = Duration::from_millis(500);
    let mut c = Clicks::default();
    let counts: Vec<u32> = (0..6)
        .map(|i| {
            c.press(
                t0 + Duration::from_millis(100 * i),
                (5.0, 5.0),
                Button::Left,
                iv,
                SLOP,
            )
        })
        .collect();
    assert_eq!(counts, [1, 2, 3, 3, 3, 3]);
    assert_eq!(MAX_CLICKS, 3);
}

#[test]
fn the_slop_is_half_the_systems_rectangle_in_logical_px() {
    assert_eq!(click_slop((4, 4), 1.0), (2.0, 2.0));
    assert_eq!(click_slop((4, 4), 2.0), (1.0, 1.0));
    assert_eq!(click_slop((6, 10), 1.0), (3.0, 5.0));
    for s in [0.0, -1.0, f32::NAN, f32::INFINITY] {
        assert_eq!(click_slop((4, 4), s), (2.0, 2.0), "scale {s}");
    }
}

#[test]
fn a_press_on_the_edge_of_the_rectangle_counts_and_one_past_it_does_not() {
    let t0 = Instant::now();
    let iv = Duration::from_millis(500);
    let ms = Duration::from_millis;
    let mut c = Clicks::default();
    c.press(t0, (10.0, 10.0), Button::Left, iv, (2.0, 3.0));
    assert_eq!(
        c.press(t0 + ms(10), (12.0, 13.0), Button::Left, iv, (2.0, 3.0)),
        2
    );
    let mut c = Clicks::default();
    c.press(t0, (10.0, 10.0), Button::Left, iv, (2.0, 3.0));
    assert_eq!(
        c.press(t0 + ms(10), (12.5, 10.0), Button::Left, iv, (2.0, 3.0)),
        1
    );
    let mut c = Clicks::default();
    c.press(t0, (10.0, 10.0), Button::Left, iv, (2.0, 3.0));
    assert_eq!(
        c.press(t0 + ms(10), (10.0, 6.5), Button::Left, iv, (2.0, 3.0)),
        1
    );
}

// ---- a window that cannot be seen ----

#[test]
fn a_zero_size_or_the_systems_word_is_a_minimized_window() {
    assert!(minimized((0, 0), None));
    assert!(minimized((0, 600), Some(false)));
    assert!(minimized((800, 0), None));
    assert!(minimized((800, 600), Some(true)));
    assert!(!minimized((800, 600), Some(false)));
    assert!(!minimized((800, 600), None));
    assert!(!minimized((1, 1), None));
}

#[test]
fn a_covered_or_minimized_window_is_hidden_until_both_are_over() {
    let mut v = Visibility::default();
    assert!(!v.hidden());
    // Minimized (Windows: no `Occluded` ever comes).
    assert!(!v.set_minimized(true));
    assert!(v.hidden());
    // Restored: seen again, once.
    assert!(v.set_minimized(false));
    assert!(!v.hidden());
    assert!(!v.set_minimized(false));
    // Covered and minimized: undoing one leaves it hidden.
    assert!(!v.set_occluded(true));
    assert!(!v.set_minimized(true));
    assert!(!v.set_occluded(false));
    assert!(v.hidden());
    assert!(v.set_minimized(false));
    assert!(!v.hidden());
    // A repeat of the same word changes nothing.
    assert!(!v.set_occluded(false));
}

#[test]
fn a_window_the_application_hides_is_hidden_until_it_is_shown_again() {
    let mut v = Visibility::default();
    assert!(!v.app_hidden());
    assert!(!v.set_app_hidden(true));
    assert!(v.hidden());
    assert!(v.app_hidden());
    assert!(!v.os_hidden());
    // A repeat changes nothing.
    assert!(!v.set_app_hidden(true));
    // Shown: seen again once, which redraws everything.
    assert!(v.set_app_hidden(false));
    assert!(!v.hidden());
    assert!(!v.set_app_hidden(false));
    // Hidden by the application while covered stays hidden when the cover goes.
    v.set_app_hidden(true);
    v.set_occluded(true);
    v.set_app_hidden(false);
    assert!(v.hidden());
    v.set_occluded(false);
    assert!(!v.hidden());
}

#[test]
fn nothing_is_drawn_while_the_application_hides_the_window_but_timers_still_come() {
    let t0 = Instant::now();
    let mut v = Visibility::default();
    let mut damage = Damage::default();
    damage.add(Rect::new(10.0, 10.0, 20.0, 20.0));
    assert!(wants_frame(v.hidden(), &damage));
    v.set_app_hidden(true);
    assert!(!wants_frame(v.hidden(), &damage));
    // The animator's deadline is dropped while hidden; the named and one-shot timers are not.
    let d = Deadlines {
        anim: Some(t0 + Duration::from_millis(16)),
        timer: Some(t0 + Duration::from_secs(1)),
    };
    assert_eq!(d.next(v.hidden()), Some(t0 + Duration::from_secs(1)));
    let mut timers = NamedTimers::default();
    timers.set("autosave", t0 + Duration::from_secs(30));
    assert_eq!(timers.next(), Some(t0 + Duration::from_secs(30)));
    // But a window hidden by the application is drawn again, whole, when shown.
    let mut damage = Damage::default();
    damage.add(Rect::new(10.0, 10.0, 20.0, 20.0));
    if v.set_app_hidden(false) {
        damage.all();
    }
    assert!(damage.full);
}

#[test]
fn a_minimized_window_wakes_for_timers_only_and_comes_back_drawn_whole() {
    let t0 = Instant::now();
    let mut v = Visibility::default();
    let mut d = Deadlines {
        anim: Some(t0 + Duration::from_millis(16)),
        timer: Some(t0 + Duration::from_secs(3)),
    };
    let mut damage = Damage::default();
    // What the loop does on a `Resized(0, 0)`: the window is minimized.
    if v.set_minimized(minimized((0, 0), Some(true))) {
        damage.all();
    }
    assert!(v.hidden());
    // The caret's next blink does not wake it; the timer does.
    assert_eq!(d.next(v.hidden()), Some(t0 + Duration::from_secs(3)));
    assert_eq!(
        d.take_due(t0 + Duration::from_millis(20), v.hidden()),
        Due::default()
    );
    assert_eq!(d.anim, None);
    // Restored to its size: seen again, and drawn whole.
    if v.set_minimized(minimized((800, 600), Some(false))) {
        damage.all();
    }
    assert!(!v.hidden());
    assert!(damage.full);
}

// ---- reduction ----

#[test]
fn device_positions_become_logical_px() {
    assert_eq!(logical(300.0, 150.0, 1.5), (200.0, 100.0));
    assert_eq!(logical(-30.0, 0.0, 2.0), (-15.0, 0.0));
    assert_eq!(logical(f64::NAN, f64::INFINITY, 1.0), (0.0, 0.0));
    for s in [0.0, -1.0, f32::NAN, f32::INFINITY] {
        assert_eq!(logical(10.0, 20.0, s), (10.0, 20.0), "scale {s}");
    }
}

#[test]
fn the_five_buttons_are_reported_and_others_are_not() {
    assert_eq!(button(MouseButton::Left), Some(Button::Left));
    assert_eq!(button(MouseButton::Right), Some(Button::Right));
    assert_eq!(button(MouseButton::Middle), Some(Button::Middle));
    assert_eq!(button(MouseButton::Back), Some(Button::Back));
    assert_eq!(button(MouseButton::Forward), Some(Button::Forward));
    assert_eq!(button(MouseButton::Other(7)), None);
}

#[test]
fn a_wheels_notches_are_wheel_lines_and_a_touchpads_pixels_are_logical() {
    assert_eq!(
        wheel(MouseScrollDelta::LineDelta(0.0, -1.0), 1.5, false),
        Event::Wheel {
            dx: 0.0,
            dy: -WHEEL_LINE,
            precise: false
        }
    );
    assert_eq!(
        wheel(
            MouseScrollDelta::PixelDelta(PhysicalPosition::new(3.0, 30.0)),
            1.5,
            false
        ),
        Event::Wheel {
            dx: 2.0,
            dy: 20.0,
            precise: true
        }
    );
}

#[test]
fn shift_turns_a_vertical_wheel_sideways_but_not_a_sideways_one() {
    assert_eq!(
        wheel(MouseScrollDelta::LineDelta(0.0, 2.0), 1.0, true),
        Event::Wheel {
            dx: 2.0 * WHEEL_LINE,
            dy: 0.0,
            precise: false
        }
    );
    assert_eq!(
        wheel(MouseScrollDelta::LineDelta(1.0, 2.0), 1.0, true),
        Event::Wheel {
            dx: WHEEL_LINE,
            dy: 2.0 * WHEEL_LINE,
            precise: false
        }
    );
}

#[test]
fn a_fraction_of_a_line_is_a_touchpad() {
    // A precision touchpad on Windows: line deltas that are not whole notches.
    assert_eq!(
        wheel(MouseScrollDelta::LineDelta(0.0, 0.25), 1.0, false),
        Event::Wheel {
            dx: 0.0,
            dy: 0.25 * WHEEL_LINE,
            precise: true
        }
    );
}

#[test]
fn a_wheel_that_is_not_a_number_does_not_move() {
    assert_eq!(
        wheel(
            MouseScrollDelta::LineDelta(f32::NAN, f32::INFINITY),
            1.0,
            false
        ),
        Event::Wheel {
            dx: 0.0,
            dy: 0.0,
            precise: false
        }
    );
    assert_eq!(
        wheel(
            MouseScrollDelta::PixelDelta(PhysicalPosition::new(f64::NAN, 4.0)),
            f32::NAN,
            false
        ),
        Event::Wheel {
            dx: 0.0,
            dy: 4.0,
            precise: true
        }
    );
}

// ---- the present ----

#[test]
fn the_copy_takes_only_the_rectangle_and_turns_bgra_bytes_into_pixels() {
    let (w, h) = (4usize, 3usize);
    let src: Vec<u8> = (0..w * h)
        .flat_map(|i| [i as u8, 0x10, 0x20, 0xff])
        .collect();
    let mut dst = vec![0u32; w * h];
    copy_rect(&src, &mut dst, w, (1, 1, 3, 3));
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            let want = if (1..3).contains(&x) && (1..3).contains(&y) {
                0xff20_1000 | i as u32
            } else {
                0
            };
            assert_eq!(dst[i], want, "({x}, {y})");
        }
    }
}

#[test]
fn a_copy_outside_the_buffers_or_of_nothing_writes_nothing_and_does_not_panic() {
    let src = vec![0xffu8; 4 * 4 * 4];
    let mut dst = vec![0u32; 16];
    copy_rect(&src, &mut dst, 4, (3, 0, 1, 4));
    copy_rect(&src, &mut dst, 4, (9, 9, 20, 20));
    assert!(dst.iter().all(|&p| p == 0));
    copy_rect(&src, &mut dst, 4, (0, 2, 99, 99));
    assert_eq!(dst.iter().filter(|&&p| p == u32::MAX).count(), 8);
    copy_rect(&src, &mut [], 0, (0, 0, 4, 4));
    copy_rect(&[], &mut dst, 4, (0, 0, 4, 4));
}

// ---- config ----

#[test]
fn a_config_takes_nonsense_sizes_as_its_defaults() {
    let c = Config::new("T")
        .size(f32::NAN, -1.0)
        .min_size(0.0, f32::INFINITY);
    assert_eq!(c.size, (900.0, 640.0));
    assert_eq!(c.min_size, (520.0, 360.0));
    let c = Config::new("T").size(800.0, 600.0).min_size(400.0, 300.0);
    assert_eq!((c.size, c.min_size), ((800.0, 600.0), (400.0, 300.0)));
    assert_eq!(c.app_id, "T");
    assert_eq!(Config::new("T").app_id("purge").app_id, "purge");
}

#[cfg(feature = "a11y")]
#[test]
fn accessibility_bounds_are_device_px() {
    let r = access_rect(Rect::new(10.0, 20.0, 30.0, 40.0), 1.5);
    assert_eq!((r.x0, r.y0, r.x1, r.y1), (15.0, 30.0, 60.0, 90.0));
    let r = access_rect(Rect::new(10.0, 20.0, 30.0, 40.0), f32::NAN);
    assert_eq!((r.x0, r.y0, r.x1, r.y1), (10.0, 20.0, 40.0, 60.0));
}
