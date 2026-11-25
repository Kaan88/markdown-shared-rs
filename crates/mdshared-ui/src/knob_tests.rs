use super::*;
use crate::gfx::{Gfx, Rgb};
use crate::slider::{DB_FLOOR, Scale, gain_to_db};
use crate::theme::{DARK, LIGHT};
use mdshared_text::Bitmap;

/// The knob's rectangle: a circle of radius 60 round (80, 80), its ring's middle at 57.
const K: Rect = Rect::new(20.0, 20.0, 120.0, 120.0);
/// The fader's column: the cap's middle travels from y 171 at the bottom to 29 at the top.
const F: Rect = Rect::new(60.0, 20.0, 120.0, 160.0);

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() <= 1e-3 * b.abs().max(1.0)
}

fn knob() -> Knob {
    Knob::new(Slider::new(0.0, 100.0, 50.0), 50.0)
}

fn volume() -> Fader {
    Fader::new(Slider::new(DB_FLOOR, 6.0, 0.0), 0.0)
}

fn gain_fader() -> Fader {
    Fader::new(Slider::new(0.0, 1.0, 1.0).scale(Scale::Db), 1.0)
}

fn at(bmp: &Bitmap, x: f32, y: f32) -> Rgb {
    let o = (y as usize * bmp.width as usize + x as usize) * 4;
    [bmp.pixels[o + 2], bmp.pixels[o + 1], bmp.pixels[o]]
}

fn shot(pal: &Palette, w: u32, h: u32, draw: impl FnOnce(&mut Painter<'_>)) -> Bitmap {
    let mut bmp = Bitmap::new(w, h);
    let mut gfx = Gfx::new();
    {
        let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
        p.fill(Rect::new(0.0, 0.0, w as f32, h as f32), pal.pane);
        draw(&mut p);
    }
    bmp
}

/// The colour at a point of a ring: `turns` clockwise from the top, which is how
/// [`Painter::arc`] names a direction.
fn on_ring(bmp: &Bitmap, cx: f32, cy: f32, rad: f32, turns: f32) -> Rgb {
    let a = turns * std::f32::consts::TAU;
    at(
        bmp,
        (cx + rad * a.sin()).round(),
        (cy - rad * a.cos()).round(),
    )
}

fn count(bmp: &Bitmap, c: Rgb) -> usize {
    bmp.pixels
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|px| [px[2], px[1], px[0]] == c)
        .count()
}

/// The row a tick of the fader is drawn on: the one of the three around `y` that has it at
/// the tick's left end, in `c`.
fn tick_row(bmp: &Bitmap, y: f32, c: Rgb) -> Option<f32> {
    let row = y.floor();
    [row, row - 1.0, row + 1.0]
        .into_iter()
        .find(|&r| at(bmp, 141.0, r) == c)
}

/// Two colours the same but for the rounding of an antialiased edge.
fn near(a: Rgb, b: Rgb) -> bool {
    a.iter()
        .zip(b)
        .all(|(x, y)| (*x as i32 - y as i32).abs() <= 2)
}

#[test]
fn a_drag_up_raises_the_knob_and_a_drag_down_lowers_it_by_the_travel() {
    let mut k = knob();
    assert!(k.press(K, 80.0, 80.0) && k.dragging());
    // Half the travel up is half the range: 50 to 100, and pinned there.
    assert!(k.drag(900.0, -20.0, false));
    assert_eq!(k.value(), 100.0);
    // Back to the press is back to 50, and half the travel down is 0.
    assert!(k.drag(0.0, 80.0, false));
    assert_eq!(k.value(), 50.0);
    assert!(k.drag(80.0, 130.0, false));
    assert_eq!(k.value(), 25.0);
    assert!(k.release() && !k.dragging() && !k.release());
    // From 25, the same 50 px up is a quarter of the range again: 50.
    assert!(k.press(K, 80.0, 80.0));
    assert!(k.drag(80.0, 30.0, false));
    assert_eq!(k.value(), 50.0);
}

#[test]
fn a_fine_drag_is_ten_times_slower_and_turning_it_on_does_not_jump() {
    let mut k = knob();
    assert!(k.press(K, 80.0, 80.0));
    assert!(k.drag(80.0, 30.0, true));
    assert!(close(k.value(), 52.5), "{}", k.value());
    // Turning fine off where the pointer is re-bases the anchor: no jump, then coarse again.
    assert!(!k.drag(80.0, 30.0, false), "nothing moved on the toggle");
    assert!(close(k.value(), 52.5));
    assert!(k.drag(80.0, 20.0, false));
    assert!(close(k.value(), 57.5), "{}", k.value());
    // And turning it back on in the middle of the coarse drag does not jump either.
    assert!(k.drag(80.0, 0.0, false));
    assert!(close(k.value(), 67.5));
    assert!(!k.drag(80.0, 0.0, true));
    assert!(close(k.value(), 67.5));
    assert!(k.drag(80.0, -10.0, true));
    assert!(close(k.value(), 68.0), "{}", k.value());
}

#[test]
fn reset_goes_to_the_default_kept_between_the_ends() {
    let mut k = Knob::new(Slider::new(0.0, 1.0, 0.8), 0.5);
    assert!(k.reset() && close(k.value(), 0.5));
    assert!(!k.reset(), "already at the default");
    assert_eq!(k.default_value(), 0.5);
    let mut clamped = Knob::new(Slider::new(0.0, 1.0, 0.8), 7.0);
    assert!(clamped.reset() && clamped.value() == 1.0);
    let mut nan = Knob::new(Slider::new(0.0, 1.0, 0.8), f32::NAN);
    assert!(!nan.reset() && nan.value() == 0.8);
    // Snapped, as the slider snaps.
    let mut snapped = Knob::new(Slider::new(0.0, 10.0, 2.0).step(1.0), 4.4);
    assert!(snapped.reset() && snapped.value() == 4.0);
}

#[test]
fn the_wheel_and_the_keys_are_the_sliders_and_the_slider_is_there_to_read() {
    let mut k = knob();
    assert!(k.wheel(2.0) && close(k.value(), 54.0));
    assert!(k.wheel(-1.0) && close(k.value(), 52.0));
    assert!(!k.wheel(0.0) && !k.wheel(f32::NAN));
    assert!(k.step_by(Step::More) && close(k.value(), 54.0));
    assert!(k.step_by(Step::Min) && k.value() == 0.0);
    assert_eq!((k.fraction(), k.slider().max()), (0.0, 100.0));
    k.slider_mut().set(25.0);
    assert_eq!((k.value(), k.fraction()), (25.0, 0.25));
}

#[test]
fn the_sweep_runs_from_the_lower_left_round_the_top_to_the_lower_right() {
    let k = knob();
    assert_eq!(k.angle_of(0.0), KNOB_START);
    assert_eq!(k.angle_of(1.0), KNOB_START + KNOB_SWEEP);
    assert_eq!(k.angle_of(0.5), 0.0, "the middle points straight up");
    assert!(close(KNOB_SWEEP * 360.0, 270.0));
    // A quarter of the way is 67.5 degrees clockwise from the top: the upper left.
    let a = k.angle_of(0.25) * std::f32::consts::TAU;
    assert!(a.sin() < -0.9 && a.cos() > 0.3, "{} {}", a.sin(), a.cos());
    // Outside the ends, and not a number: the nearer end.
    assert_eq!(
        (k.angle_of(-3.0), k.angle_of(9.0)),
        (k.angle_of(0.0), k.angle_of(1.0))
    );
    assert_eq!(k.angle_of(f32::NAN), k.angle_of(0.0));
}

#[test]
fn a_press_off_the_knobs_circle_is_not_taken() {
    let mut k = knob();
    assert!(
        !k.press(K, 20.0, 20.0),
        "the corner of the square is off it"
    );
    assert!(!k.press(K, f32::NAN, 80.0));
    assert!(!k.dragging() && !k.drag(80.0, 0.0, false) && !k.release());
    assert_eq!(k.value(), 50.0);
    assert!(k.press(K, 80.0, 21.0), "the top of the circle is on it");
    assert!(!k.press(Rect::new(0.0, 0.0, 0.0, 0.0), 0.0, 0.0));
    // The centre of a bipolar knob is its slider's zero.
    let pan = Knob::new(Slider::new(-1.0, 1.0, 0.0), 0.0).bipolar();
    assert!(close(pan.centre(), 0.5));
    assert!(pan.is_bipolar());
    let odd = Knob::new(Slider::new(10.0, 20.0, 15.0), 15.0).bipolar();
    assert!(close(odd.centre(), 0.5), "no zero in range: the middle");
    let positive = Knob::new(Slider::new(0.0, 1.0, 0.5), 0.5).bipolar();
    assert!(close(positive.centre(), 0.0));
}

#[test]
fn the_knob_draws_its_ring_its_value_arc_its_body_and_its_pointer() {
    let quiet = |pal: &Palette| mix(pal.line, pal.text, 0.18);
    for pal in [&LIGHT, &DARK] {
        // The value at a half: the left half of the sweep is accent, the right half quiet.
        let bmp = shot(pal, 160, 160, |p| {
            draw_knob(p, pal, K, &knob(), slider::Look::default())
        });
        assert_eq!(on_ring(&bmp, 80.0, 80.0, 57.0, -0.25), pal.accent);
        assert_eq!(on_ring(&bmp, 80.0, 80.0, 57.0, 0.25), quiet(pal));
        // The body: the disc's fill in the middle, its edge all round, and no paint past it.
        assert_eq!(at(&bmp, 80.0, 80.0), pal.popup);
        assert!(near(at(&bmp, 130.0, 80.0), mix(pal.line, pal.text, 0.25)));
        assert_eq!(at(&bmp, 10.0, 10.0), pal.pane);
        // The pointer runs up from near the middle, in the accent.
        assert_eq!(at(&bmp, 80.0, 45.0), pal.accent);
        assert_eq!(at(&bmp, 62.0, 80.0), pal.popup, "no pointer across");
    }
}

#[test]
fn a_bipolar_knob_draws_no_arc_at_its_centre_and_both_ways_from_it() {
    let quiet = |pal: &Palette| mix(pal.line, pal.text, 0.18);
    let pan = |value: f32| Knob::new(Slider::new(-1.0, 1.0, value), 0.0).bipolar();
    for pal in [&LIGHT, &DARK] {
        let centre = shot(pal, 160, 160, |p| {
            draw_knob(p, pal, K, &pan(0.0), slider::Look::default())
        });
        for turns in [-0.25, -0.125, 0.125, 0.25] {
            assert_eq!(
                on_ring(&centre, 80.0, 80.0, 57.0, turns),
                quiet(pal),
                "the ring at {turns} turns"
            );
        }
        // A twelfth of the sweep either way: accent on the side of the value, quiet away
        // from it, and the quarter turns beyond it.
        let above = shot(pal, 160, 160, |p| {
            draw_knob(p, pal, K, &pan(0.5), slider::Look::default())
        });
        assert_eq!(on_ring(&above, 80.0, 80.0, 57.0, 0.125), pal.accent);
        assert_eq!(on_ring(&above, 80.0, 80.0, 57.0, -0.125), quiet(pal));
        assert_eq!(on_ring(&above, 80.0, 80.0, 57.0, -0.25), quiet(pal));
        // And the other way for a value below the middle.
        let below = shot(pal, 160, 160, |p| {
            draw_knob(p, pal, K, &pan(-0.5), slider::Look::default())
        });
        assert_eq!(on_ring(&below, 80.0, 80.0, 57.0, -0.125), pal.accent);
        assert_eq!(on_ring(&below, 80.0, 80.0, 57.0, 0.125), quiet(pal));
        assert_eq!(on_ring(&below, 80.0, 80.0, 57.0, 0.25), quiet(pal));
        // A plain knob at its low end has no arc either, as the centre of a pan has none.
        let low = shot(pal, 160, 160, |p| {
            draw_knob(
                p,
                pal,
                K,
                &Knob::new(Slider::new(0.0, 1.0, 0.0), 0.0),
                slider::Look::default(),
            )
        });
        for turns in [-0.25, -0.125, 0.125, 0.25] {
            assert_eq!(on_ring(&low, 80.0, 80.0, 57.0, turns), quiet(pal));
        }
    }
}

#[test]
fn a_knob_under_the_pointer_in_hand_or_with_the_keyboard_looks_different() {
    for pal in [&LIGHT, &DARK] {
        let plain = shot(pal, 160, 160, |p| {
            draw_knob(p, pal, K, &knob(), slider::Look::default())
        });
        let hot = shot(pal, 160, 160, |p| {
            draw_knob(
                p,
                pal,
                K,
                &knob(),
                slider::Look {
                    hot: true,
                    ..slider::Look::default()
                },
            )
        });
        assert_ne!(
            at(&hot, 130.0, 80.0),
            at(&plain, 130.0, 80.0),
            "a stronger edge"
        );
        let held = shot(pal, 160, 160, |p| {
            draw_knob(
                p,
                pal,
                K,
                &knob(),
                slider::Look {
                    hot: true,
                    pressed: true,
                    ..slider::Look::default()
                },
            )
        });
        assert_ne!(at(&held, 80.0, 80.0), pal.popup, "a darker disc");
        let focus = shot(pal, 160, 160, |p| {
            draw_knob(
                p,
                pal,
                K,
                &knob(),
                slider::Look {
                    focused: true,
                    ..slider::Look::default()
                },
            )
        });
        assert_eq!(at(&plain, 80.0, 20.0), pal.pane);
        assert_ne!(at(&focus, 80.0, 20.0), pal.pane, "a ring inside the edge");
        // Disabled: quiet, with no accent anywhere and no hover or keyboard look.
        let off = shot(pal, 160, 160, |p| {
            draw_knob(
                p,
                pal,
                K,
                &knob(),
                slider::Look {
                    disabled: true,
                    hot: true,
                    focused: true,
                    pressed: true,
                    ..slider::Look::default()
                },
            )
        });
        assert_eq!(count(&off, pal.accent), 0);
        assert!(near(at(&off, 130.0, 80.0), pal.line));
    }
}

#[test]
fn a_fader_drag_maps_through_the_decibel_scale_and_fine_is_a_tenth() {
    let mut f = gain_fader();
    // A press at the bottom of the track is silence and takes hold there.
    assert!(f.press(F, 120.0, 171.0));
    assert_eq!(f.value(), 0.0);
    // Up half the travel is half the track, which on the decibel scale is 30 dB down.
    assert!(f.drag(F, 120.0, 100.0, false));
    assert!(
        close(gain_to_db(f.value()), -30.0),
        "{}",
        gain_to_db(f.value())
    );
    assert!(close(f.fraction(), 0.5));
    // A tenth of that with fine, from the top of the track: 0 dB kept for 0.99 of the track.
    let mut fine = gain_fader();
    assert!(fine.press(F, 120.0, 29.0));
    assert!(fine.drag(F, 120.0, 43.2, true));
    assert!(
        close(gain_to_db(fine.value()), -0.6),
        "{}",
        gain_to_db(fine.value())
    );
    // Toggling fine mid-drag does not jump, either way.
    let before = fine.value();
    assert!(!fine.drag(F, 120.0, 43.2, false));
    assert_eq!(fine.value(), before);
    assert!(fine.drag(F, 120.0, 57.4, false));
    assert!(
        close(gain_to_db(fine.value()), -6.6),
        "{}",
        gain_to_db(fine.value())
    );
}

#[test]
fn a_press_on_the_track_jumps_and_one_on_the_cap_keeps_the_value() {
    let mut f = gain_fader();
    assert!(!f.press(F, 120.0, 15.0), "off r");
    assert!(!f.dragging());
    // The cap at the top of the track, a little left of its middle: no jump.
    f.slider_mut().set(1.0);
    let cap_y = f.slider().thumb_center(F).1;
    assert_eq!(cap_y, 29.0);
    assert!(f.press(F, 115.0, 35.0), "on the cap, below the middle");
    assert_eq!(f.value(), 1.0);
    assert!(f.drag(F, 115.0, 45.0, false));
    assert!(
        close(gain_to_db(f.value()), -4.225),
        "{}",
        gain_to_db(f.value())
    );
    assert!(f.release() && !f.dragging() && !f.release());
    // Off the cap, the value jumps to the pointer, as on a slider.
    assert!(f.press(F, 120.0, 100.0));
    assert!(close(gain_to_db(f.value()), -30.0));
    // A rectangle too short for its cap has no drag to make.
    let short = Rect::new(60.0, 20.0, 120.0, 10.0);
    assert!(!f.drag(short, 120.0, 30.0, false));
    assert!(!f.drag(F, 120.0, f32::NAN, false));
}

#[test]
fn the_fader_resets_wheels_and_steps_as_its_slider_does() {
    let mut f = Fader::new(Slider::new(0.0, 1.0, 0.25).scale(Scale::Db), 1.0);
    assert!(f.reset() && f.value() == 1.0);
    assert!(!f.reset());
    assert_eq!(f.default_value(), 1.0);
    assert!(f.wheel(-2.0));
    assert!(
        close(gain_to_db(f.value()), -2.4),
        "{}",
        gain_to_db(f.value())
    );
    assert!(!f.wheel(f32::NAN) && !f.wheel(0.0));
    assert!(f.step_by(Step::Min) && f.value() == 0.0);
    assert_eq!(
        (f.slider().axis(), f.slider().max()),
        (slider::Axis::Vertical, 1.0)
    );
    assert!(f.slider_mut().set(0.5) && close(gain_to_db(f.value()), -6.0206));
    // An upright slider given to a fader stays upright, and a horizontal one is stood up.
    let flat = Slider::new(0.0, 1.0, 0.5);
    assert_eq!(
        Fader::new(flat, 0.5).slider().axis(),
        slider::Axis::Vertical
    );
}

#[test]
fn the_ticks_are_in_order_from_the_top_and_inside_the_faders_rectangle() {
    let f = volume();
    let ticks = f.ticks(F, DEFAULT_TICKS);
    // Every mark of the list is inside a slider from the floor to +6 dB.
    assert_eq!(ticks.len(), DEFAULT_TICKS.len());
    assert_eq!((ticks[0].0, ticks[0].1), (29.0, 6.0));
    for pair in ticks.windows(2) {
        assert!(pair[0].0 < pair[1].0, "{ticks:?}");
    }
    for &(y, mark) in &ticks {
        assert!(y >= F.y && y <= F.bottom(), "{mark} at {y}");
        assert!(close(y, 171.0 - 142.0 * f.slider().fraction_of(mark)));
    }
    // The last is the decibel floor, at the bottom of the track.
    assert_eq!(ticks.last().unwrap().1, DB_FLOOR);
    assert!(close(ticks.last().unwrap().0, 171.0));
    // Zero dB is a tenth of the range below +6, because the range is the floor to +6.
    let zero = ticks.iter().find(|t| t.1 == 0.0).unwrap().0;
    assert!(close(zero, 171.0 - 142.0 * (60.0 / 66.0)));
    // Given out of order, the answer is still from the top down; the misfits are left out.
    let few = f.ticks(F, &[-6.0, 0.0, 12.0, -100.0, f32::NAN]);
    assert_eq!(few.len(), 2);
    assert_eq!((few[0].1, few[1].1), (0.0, -6.0));
    assert!(
        f.ticks(Rect::new(0.0, 0.0, 0.0, 0.0), DEFAULT_TICKS)
            .is_empty()
    );
    assert!(
        f.ticks(Rect::new(0.0, 0.0, f32::NAN, 100.0), DEFAULT_TICKS)
            .is_empty()
    );
    // A mark outside the ends is left out.
    assert!(f.ticks(F, &[7.0]).is_empty());
    assert!(f.ticks(F, &[-61.0]).is_empty());
}

#[test]
fn the_fader_draws_its_track_cap_ticks_and_labels() {
    let quiet = |pal: &Palette| mix(pal.line, pal.text, 0.18);
    for pal in [&LIGHT, &DARK] {
        let mut f = volume();
        f.slider_mut().set(-27.0);
        let bmp = shot(pal, 240, 200, |p| {
            draw_fader(p, pal, F, &f, slider::Look::default(), true)
        });
        // The track below the cap is accent, above it quiet; the cap straddles the track.
        assert_eq!(at(&bmp, 120.0, 140.0), pal.accent);
        assert_eq!(at(&bmp, 120.0, 60.0), quiet(pal));
        assert_eq!(
            at(&bmp, 110.0, 96.0),
            pal.popup,
            "the cap is wider than the track"
        );
        assert_eq!(
            at(&bmp, 120.0, 100.0),
            pal.accent,
            "the line across the cap"
        );
        assert_eq!(at(&bmp, 120.0, 91.0), mix(pal.line, pal.text, 0.25));
        // The zero mark is longer and in the text colour; the others are faint.
        let tick = |mark: f32| {
            f.ticks(F, DEFAULT_TICKS)
                .into_iter()
                .find(|t| t.1 == mark)
                .unwrap()
                .0
        };
        let zero = tick_row(&bmp, tick(0.0), pal.text).expect("the zero tick");
        assert_eq!(at(&bmp, 148.0, zero), pal.text, "eight px long");
        assert_eq!(at(&bmp, 149.0, zero), pal.pane);
        let below = tick_row(&bmp, tick(-6.0), pal.faint).expect("the -6 tick");
        assert_eq!(at(&bmp, 145.0, below), pal.faint);
        assert_eq!(at(&bmp, 146.0, below), pal.pane, "five px long");
        // The labels are on the left of the cap; without them that strip is bare.
        let strip = |bmp: &Bitmap| {
            (60..101)
                .flat_map(|x| (0..200).map(move |y| (x, y)))
                .filter(|&(x, y)| at(bmp, x as f32, y as f32) != pal.pane)
                .count()
        };
        assert!(strip(&bmp) > 20, "{}", strip(&bmp));
        let plain = shot(pal, 240, 200, |p| {
            draw_fader(p, pal, F, &f, slider::Look::default(), false)
        });
        assert!(plain.pixels != bmp.pixels, "labels left nothing out");
        assert_eq!(strip(&plain), 0);
        // In hand the cap's edge is stronger; with the keyboard on it a ring goes round it.
        let edge = mix(pal.line, pal.text, 0.25);
        let held = shot(pal, 240, 200, |p| {
            draw_fader(
                p,
                pal,
                F,
                &f,
                slider::Look {
                    pressed: true,
                    ..slider::Look::default()
                },
                false,
            )
        });
        assert_ne!(at(&held, 120.0, 91.0), edge);
        let focus = shot(pal, 240, 200, |p| {
            draw_fader(
                p,
                pal,
                F,
                &f,
                slider::Look {
                    focused: true,
                    ..slider::Look::default()
                },
                false,
            )
        });
        assert_eq!(at(&plain, 136.0, 100.0), pal.pane);
        assert_ne!(at(&focus, 136.0, 100.0), pal.pane);
        // Disabled: no accent, a quiet cap and quiet marks.
        let off = shot(pal, 240, 200, |p| {
            draw_fader(
                p,
                pal,
                F,
                &f,
                slider::Look {
                    disabled: true,
                    ..slider::Look::default()
                },
                true,
            )
        });
        assert_eq!(count(&off, pal.accent), 0);
        assert_eq!(at(&off, 110.0, 96.0), pal.pane);
        // The ends: at the top of the track the cap is at the top and the whole track is
        // accent; at silence the cap is at the bottom and none of it is.
        let mut full = volume();
        full.slider_mut().set(6.0);
        let top = shot(pal, 240, 200, |p| {
            draw_fader(p, pal, F, &full, slider::Look::default(), false)
        });
        assert_eq!(at(&top, 120.0, 40.0), pal.accent);
        assert_eq!(at(&top, 112.0, 24.0), pal.popup);
        let mut empty = volume();
        empty.slider_mut().set(DB_FLOOR);
        let none = shot(pal, 240, 200, |p| {
            draw_fader(p, pal, F, &empty, slider::Look::default(), false)
        });
        assert_eq!(at(&none, 120.0, 140.0), quiet(pal));
        assert_eq!(at(&none, 120.0, 60.0), quiet(pal));
        assert_eq!(at(&none, 112.0, 166.0), pal.popup);
    }
}

#[test]
fn the_marks_the_caller_gives_are_the_ones_drawn_and_the_floor_reads_infinity() {
    // Quarter of the range each: marks that are not decibels at all.
    let f = Fader::new(Slider::new(0.0, 4.0, 2.0), 2.0);
    let marks = [1.0, 2.0, 3.0];
    assert_eq!(f.ticks(F, &marks), {
        let mut want: Vec<(f32, f32)> = marks
            .iter()
            .map(|&v| (171.0 - 142.0 * (v / 4.0), v))
            .collect();
        want.sort_by(|a, b| a.0.total_cmp(&b.0));
        want
    });
    let bmp = shot(&LIGHT, 240, 200, |p| {
        draw_fader_with(p, &LIGHT, F, &f, slider::Look::default(), &marks, true)
    });
    // A mark at 1 of 4 is a quarter up the track, and it is drawn and labelled.
    let y: f32 = 171.0 - 142.0 * 0.25;
    let row = tick_row(&bmp, y, LIGHT.faint).expect("the quarter tick");
    assert!((60..101).any(|x| at(&bmp, x as f32, row) != LIGHT.pane));
    // The floor of a decibel slider is silence, and its label says so.
    assert_eq!(tick_label(&volume(), DB_FLOOR), "-inf");
    assert_eq!(tick_label(&volume(), 0.0), "0");
    assert_eq!(tick_label(&volume(), -6.0), "-6");
    assert_eq!(tick_label(&volume(), 0.5), "0.5");
    // Zero and the low end of a plain fader are numbers, not silence.
    let plain = Fader::new(Slider::new(0.0, 10.0, 5.0), 5.0);
    assert_eq!(tick_label(&plain, 0.0), "0");
    assert_eq!(tick_label(&plain, 10.0), "10");
}

#[test]
fn an_empty_or_unmappable_rectangle_draws_nothing_and_does_not_panic() {
    for r in [
        Rect::new(10.0, 10.0, 0.0, 0.0),
        Rect::new(10.0, 10.0, 0.0, 100.0),
        Rect::new(10.0, 10.0, 100.0, -20.0),
        Rect::new(10.0, 10.0, f32::NAN, 100.0),
        Rect::new(10.0, 10.0, 100.0, f32::INFINITY),
        Rect::new(f32::NAN, 10.0, 100.0, 100.0),
    ] {
        for pal in [&LIGHT, &DARK] {
            let blank = shot(pal, 200, 200, |_| {});
            let knob_pixels = shot(pal, 200, 200, |p| {
                draw_knob(p, pal, r, &knob(), slider::Look::default())
            });
            let fader_pixels = shot(pal, 200, 200, |p| {
                draw_fader(p, pal, r, &volume(), slider::Look::default(), true)
            });
            assert!(knob_pixels.pixels == blank.pixels, "{r:?}");
            assert!(fader_pixels.pixels == blank.pixels, "{r:?}");
        }
    }
    // A knob too small for its body, and a fader too short for its cap: nothing either.
    let blank = shot(&LIGHT, 200, 200, |_| {});
    let tiny = Rect::new(10.0, 10.0, 10.0, 10.0);
    let knob_pixels = shot(&LIGHT, 200, 200, |p| {
        draw_knob(p, &LIGHT, tiny, &knob(), slider::Look::default())
    });
    assert!(knob_pixels.pixels == blank.pixels);
    let short = Rect::new(10.0, 10.0, 100.0, 10.0);
    let fader_pixels = shot(&LIGHT, 200, 200, |p| {
        draw_fader(p, &LIGHT, short, &volume(), slider::Look::default(), false)
    });
    assert!(fader_pixels.pixels == blank.pixels);
}

/// The sheet a person looks at: knobs along the ends and the middle of an ordinary range,
/// a bipolar knob either side of its centre, one disabled, and faders on the decibel scale
/// at zero, -6, silence and disabled, light and dark, at two scales.
/// `cargo test -p mdshared-ui knob_sheet -- --ignored`, then open target/specimen/knob.png
/// and knob-150.png.
#[test]
#[ignore]
fn knob_sheet() {
    for (scale, file) in [(1.0, "knob.png"), (1.5, "knob-150.png")] {
        let mut bmp = Bitmap::new((700.0 * scale) as u32, (820.0 * scale) as u32);
        let mut gfx = Gfx::new();
        let mut p = Painter::new(&mut bmp, &mut gfx, scale);
        for (half, pal) in [&LIGHT, &DARK].into_iter().enumerate() {
            let oy = half as f32 * 410.0;
            p.fill(Rect::new(0.0, oy, 700.0, 410.0), pal.pane);
            let o = p.offset(0.0, oy);
            let off = slider::Look {
                disabled: true,
                ..slider::Look::default()
            };
            let hot = slider::Look {
                hot: true,
                ..slider::Look::default()
            };
            let plain = Slider::new(0.0, 100.0, 0.0);
            let knobs: [(Knob, slider::Look, &str); 7] = [
                (Knob::new(plain, 0.0), slider::Look::default(), "0"),
                (Knob::new(Slider::new(0.0, 100.0, 50.0), 50.0), hot, "50"),
                (
                    Knob::new(Slider::new(0.0, 100.0, 100.0), 100.0),
                    slider::Look::default(),
                    "100",
                ),
                (
                    Knob::new(Slider::new(-1.0, 1.0, -1.0), 0.0).bipolar(),
                    slider::Look::default(),
                    "L",
                ),
                (
                    Knob::new(Slider::new(-1.0, 1.0, 0.0), 0.0).bipolar(),
                    slider::Look::default(),
                    "C",
                ),
                (
                    Knob::new(Slider::new(-1.0, 1.0, 1.0), 0.0).bipolar(),
                    slider::Look::default(),
                    "R",
                ),
                (Knob::new(Slider::new(0.0, 100.0, 60.0), 50.0), off, "off"),
            ];
            for (i, (k, look, name)) in knobs.into_iter().enumerate() {
                let x = 18.0 + 96.0 * i as f32;
                let r = Rect::new(x, 12.0, 88.0, 88.0);
                draw_knob(&mut p, pal, r, &k, look);
                p.text_center(
                    Face::Ui,
                    10.0,
                    Rect::new(r.x, 102.0, r.w, 14.0),
                    name,
                    pal.muted,
                );
            }
            let faders: [(f32, &str, bool, slider::Look); 4] = [
                (0.0, "0 dB", false, slider::Look::default()),
                (-6.0, "-6 dB", true, slider::Look::default()),
                (DB_FLOOR, "silence", true, slider::Look::default()),
                (-12.0, "off", true, off),
            ];
            for (i, (value, name, labels, look)) in faders.into_iter().enumerate() {
                let x = 24.0 + 168.0 * i as f32;
                let mut f = volume();
                f.slider_mut().set(value);
                let r = Rect::new(x, 128.0, 128.0, 250.0);
                draw_fader(&mut p, pal, r, &f, look, labels);
                p.text_center(
                    Face::Ui,
                    10.0,
                    Rect::new(r.x, 388.0, r.w, 0.0),
                    name,
                    pal.muted,
                );
            }
            p.restore_offset(o);
        }
        let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/specimen");
        std::fs::create_dir_all(&out).unwrap();
        bmp.write_png(&out.join(file)).unwrap();
    }
}
