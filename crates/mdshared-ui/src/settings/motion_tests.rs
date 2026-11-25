//! The form in motion ([`Form::draw_animated`]) and its slider row: at rest and with an
//! animator that does not animate it is the plain draw, pixel for pixel; in motion it is not;
//! the slider row's keys, press, drag and release change its value and say so.

use super::{Form, Outcome, Row, Value};
use crate::anim::{Animator, Clock, ManualClock, Policy};
use crate::gfx::{Gfx, Painter, Rect};
use crate::keys::{KeyName, Mods};
use crate::theme::{DARK, LIGHT, Palette};
use mdshared_text::Bitmap;

const NONE: Mods = Mods {
    ctrl: false,
    shift: false,
    alt: false,
    control: false,
};
const CTRL: Mods = Mods {
    ctrl: true,
    shift: false,
    alt: false,
    control: false,
};
const W: f32 = 700.0;
const H: f32 = 420.0;

const TOGGLE: u32 = 1;
const SEGMENTS: u32 = 2;
const SLIDER: u32 = 5;

fn rows() -> Vec<Row> {
    let mut rows = vec![
        Row::header("General"),
        Row::toggle(TOGGLE, "Wrap lines", false).help("Long lines go on under"),
        Row::choice(SEGMENTS, "Theme", &["Dark", "Light", "Auto"], 0),
        Row::choice(
            3,
            "Font",
            &["One", "Two", "Three", "Four", "Five", "Six"],
            1,
        ),
        Row::number(4, "Tab width", 4, 1, 8),
        Row::number(SLIDER, "Volume", 40, 0, 100).step(5).unit("%"),
        Row::header("More"),
    ];
    for k in 0..8 {
        rows.push(Row::toggle(10 + k, format!("Option {k}"), k % 2 == 0));
    }
    rows
}

fn form() -> Form {
    let mut f = Form::new("Settings", rows()).single_page();
    assert!(f.show_as_slider(SLIDER));
    f
}

/// The form drawn over a window of one grey: by `draw`, or by `draw_animated` in `anim`.
fn shot(f: &mut Form, pal: &Palette, anim: Option<&mut Animator>) -> Vec<u8> {
    let mut bmp = Bitmap::new(W as u32, H as u32);
    bmp.clear([128; 3]);
    let mut gfx = Gfx::new();
    {
        let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
        let window = Rect::new(0.0, 0.0, W, H);
        match anim {
            Some(a) => f.draw_animated(&mut p, pal, window, false, a, "settings"),
            None => f.draw(&mut p, pal, window, false),
        }
    }
    bmp.pixels
}

/// A window's frames on a clock moved by hand.
struct Frames {
    clock: ManualClock,
    anim: Animator,
}

impl Frames {
    fn new() -> Frames {
        Frames {
            clock: ManualClock::new(),
            anim: Animator::new(),
        }
    }
    /// One frame `ms` after the last: the animated drawing.
    fn frame(&mut self, f: &mut Form, pal: &Palette, ms: u64) -> Vec<u8> {
        self.clock.advance_ms(ms);
        self.anim.begin_frame(self.clock.now());
        let px = shot(f, pal, Some(&mut self.anim));
        self.anim.end_frame();
        px
    }
    /// Frames of 16 ms until nothing moves; the last one's drawing.
    fn settle(&mut self, f: &mut Form, pal: &Palette) -> Vec<u8> {
        let mut px = self.frame(f, pal, 16);
        for _ in 0..60 {
            if !self.anim.animating() {
                break;
            }
            px = self.frame(f, pal, 16);
        }
        assert!(!self.anim.animating(), "everything arrives");
        px
    }
}

fn index_of(f: &Form, id: u32) -> usize {
    f.rows().iter().position(|r| r.id() == Some(id)).unwrap()
}

fn centre(r: Rect) -> (f32, f32) {
    (r.cx(), r.cy())
}

/// A change made by `change`: the frame that asks still draws the old state and so equals
/// nothing in particular; the next one is in motion (unlike the plain draw); once settled the
/// animated draw is the plain draw again.
fn moves_then_rests(pal: &Palette, what: &str, change: impl Fn(&mut Form)) {
    let mut f = form();
    let mut fr = Frames::new();
    let first = fr.frame(&mut f, pal, 0);
    assert!(
        first == shot(&mut f, pal, None),
        "{what}: first seen at rest"
    );
    change(&mut f);
    fr.frame(&mut f, pal, 16);
    let moving = fr.frame(&mut f, pal, 16);
    assert!(moving != shot(&mut f, pal, None), "{what}: in motion");
    let rest = fr.settle(&mut f, pal);
    assert!(rest == shot(&mut f, pal, None), "{what}: at rest again");
}

#[test]
fn the_animated_form_is_the_plain_form_at_rest_in_both_themes() {
    for pal in [&LIGHT, &DARK] {
        let mut f = form();
        let mut fr = Frames::new();
        assert!(fr.frame(&mut f, pal, 0) == shot(&mut f, pal, None));
        // Hover, focus and pressed looks that have had time to arrive.
        let i = index_of(&f, TOGGLE);
        let (x, y) = centre(f.control_rect(i));
        f.pointer_move(x, y);
        f.key(&KeyName::Tab, NONE, None);
        let rest = fr.settle(&mut f, pal);
        assert!(rest == shot(&mut f, pal, None), "hovered and focused");
    }
}

#[test]
fn an_animator_that_does_not_animate_draws_the_plain_form_on_every_frame() {
    let mut f = form();
    let clock = ManualClock::new();
    let mut anim = Animator::new();
    anim.set_policy(Policy::Never);
    let mut frame = |f: &mut Form| {
        clock.advance_ms(16);
        anim.begin_frame(clock.now());
        let px = shot(f, &LIGHT, Some(&mut anim));
        anim.end_frame();
        px
    };
    frame(&mut f);
    let t = index_of(&f, TOGGLE);
    let s = index_of(&f, SLIDER);
    let seg = index_of(&f, SEGMENTS);
    let changes: [&dyn Fn(&mut Form); 5] = [
        &|f| {
            f.set_value(TOGGLE, Value::Bool(true));
        },
        &|f| {
            let (x, y) = centre(f.control_rect(t));
            f.pointer_move(x, y);
        },
        &|f| {
            f.set_value(SEGMENTS, Value::Choice(2));
        },
        &|f| {
            f.set_value(SLIDER, Value::Number(95));
        },
        &|f| {
            let (x, y) = centre(f.segment_rect(seg, 1));
            f.pointer_move(x, y);
            let r = f.slider_rect(s);
            f.pointer_move(r.x + 2.0, r.cy());
        },
    ];
    for (k, change) in changes.iter().enumerate() {
        change(&mut f);
        frame(&mut f);
        assert!(frame(&mut f) == shot(&mut f, &LIGHT, None), "change {k}");
    }
}

#[test]
fn a_switch_slides_when_it_turns_over() {
    moves_then_rests(&LIGHT, "switch", |f| {
        f.set_value(TOGGLE, Value::Bool(true));
    });
}

#[test]
fn a_rows_hover_fill_fades_in() {
    moves_then_rests(&LIGHT, "row hover", |f| {
        let r = f.rect_of_row(index_of(f, 4));
        f.pointer_move(r.x + 20.0, r.cy());
    });
}

#[test]
fn the_chosen_segment_slides_to_the_new_choice() {
    for pal in [&LIGHT, &DARK] {
        moves_then_rests(pal, "segment", |f| {
            f.set_value(SEGMENTS, Value::Choice(2));
        });
    }
}

#[test]
fn the_neutral_chosen_segment_slides_too() {
    let mut f = form().style({
        let mut s = super::Style::CLASSIC;
        s.neutral_segments = true;
        s
    });
    let mut fr = Frames::new();
    assert!(fr.frame(&mut f, &DARK, 0) == shot(&mut f, &DARK, None));
    f.set_value(SEGMENTS, Value::Choice(1));
    fr.frame(&mut f, &DARK, 16);
    assert!(fr.frame(&mut f, &DARK, 16) != shot(&mut f, &DARK, None));
    assert!(fr.settle(&mut f, &DARK) == shot(&mut f, &DARK, None));
}

#[test]
fn a_slider_rows_thumb_glides_to_a_value_set_by_a_key() {
    moves_then_rests(&LIGHT, "slider", |f| {
        assert!(f.focus_row(SLIDER));
        let out = f.key(&KeyName::End, NONE, None);
        assert_eq!(
            out,
            Outcome::Changed {
                id: SLIDER,
                value: Value::Number(100)
            }
        );
    });
}

#[test]
fn the_scrollbar_widens_under_the_pointer() {
    moves_then_rests(&LIGHT, "scrollbar", |f| {
        let b = f.body_rect();
        f.pointer_move(b.right() - 3.0, b.y + 10.0);
    });
}

#[test]
fn under_high_contrast_a_hover_changes_at_once() {
    let mut f = form();
    assert!(f.set_high_contrast(true));
    assert!(!f.set_high_contrast(true));
    let mut fr = Frames::new();
    fr.frame(&mut f, &DARK, 0);
    let r = f.rect_of_row(index_of(&f, 4));
    f.pointer_move(r.x + 20.0, r.cy());
    assert!(fr.frame(&mut f, &DARK, 16) == shot(&mut f, &DARK, None));
    let b = f.body_rect();
    f.pointer_move(b.right() - 3.0, b.y + 10.0);
    assert!(fr.frame(&mut f, &DARK, 16) == shot(&mut f, &DARK, None));
    f.set_value(SEGMENTS, Value::Choice(1));
    assert!(fr.frame(&mut f, &DARK, 16) == shot(&mut f, &DARK, None));
    fr.frame(&mut f, &DARK, 16);
    // A switch is motion, not a highlight: it still slides.
    f.set_value(TOGGLE, Value::Bool(true));
    fr.frame(&mut f, &DARK, 16);
    assert!(fr.frame(&mut f, &DARK, 16) != shot(&mut f, &DARK, None));
}

#[test]
fn a_dropdowns_menu_hover_row_fades() {
    let mut f = form();
    let mut fr = Frames::new();
    fr.frame(&mut f, &LIGHT, 0);
    let i = index_of(&f, 3);
    let (x, y) = centre(f.control_rect(i));
    f.press(x, y, NONE, 1);
    assert_eq!(f.release(x, y), Outcome::Redraw);
    assert!(fr.frame(&mut f, &LIGHT, 16) == shot(&mut f, &LIGHT, None));
    f.key(&KeyName::Down, NONE, None);
    fr.frame(&mut f, &LIGHT, 16);
    assert!(fr.frame(&mut f, &LIGHT, 16) != shot(&mut f, &LIGHT, None));
    assert!(fr.settle(&mut f, &LIGHT) == shot(&mut f, &LIGHT, None));
}

// ---- the slider row ------------------------------------------------------------------------

fn changed(value: i32) -> Outcome {
    Outcome::Changed {
        id: SLIDER,
        value: Value::Number(value),
    }
}

/// A form laid out once, with the keyboard on its slider row.
fn laid() -> (Form, usize) {
    let mut f = form();
    shot(&mut f, &LIGHT, None);
    let i = index_of(&f, SLIDER);
    assert!(f.focus_row(SLIDER));
    (f, i)
}

#[test]
fn a_number_row_is_a_slider_only_when_the_form_is_told_and_by_its_id() {
    let mut f = Form::new(
        "s",
        vec![
            Row::number(1, "a", 44, 0, 100).step(10),
            Row::toggle(2, "b", true),
        ],
    );
    // Not told: a stepper, as ever.
    shot(&mut f, &LIGHT, None);
    assert_eq!(f.slider_rect(0), Rect::default());
    assert!(!f.show_as_slider(2), "a toggle is not a number row");
    assert!(!f.show_as_slider(9), "no such row yet");
    assert!(f.show_as_slider(1));
    assert!(f.show_as_slider(1), "asking twice is asking once");
    shot(&mut f, &LIGHT, None);
    assert!(f.slider_rect(0).w > 0.0);
    assert_eq!(f.slider_rect(1), Rect::default());
    // The value is the number row's: not snapped by being shown, clamped as a number is.
    assert_eq!(f.value(1), Some(Value::Number(44)));
    assert!(f.set_value(1, Value::Number(-7)));
    assert_eq!(f.value(1), Some(Value::Number(0)));
    assert!(!f.set_value(1, Value::Bool(true)));
    // The marks are ids: they outlast new rows, and mark a row that comes later.
    f.set_rows(vec![
        Row::header("h"),
        Row::number(9, "later", 3, 0, 10),
        Row::number(1, "again", 50, 0, 100),
    ]);
    shot(&mut f, &LIGHT, None);
    assert!(f.slider_rect(1).w > 0.0 && f.slider_rect(2).w > 0.0);
    assert_eq!(f.slider_rect(0), Rect::default());
}

#[test]
fn a_form_never_told_of_a_slider_draws_as_before() {
    let mut a = Form::new("s", rows()).single_page();
    let mut b = Form::new("s", rows()).single_page();
    b.show_as_slider(777);
    assert!(shot(&mut a, &DARK, None) == shot(&mut b, &DARK, None));
    // And told, the row is drawn otherwise.
    b.show_as_slider(SLIDER);
    assert!(shot(&mut a, &DARK, None) != shot(&mut b, &DARK, None));
}

#[test]
fn a_slider_rows_keys_step_page_and_go_to_the_ends() {
    let (mut f, _) = laid();
    assert_eq!(f.key(&KeyName::Right, NONE, None), changed(45));
    assert_eq!(f.key(&KeyName::Left, NONE, None), changed(40));
    assert_eq!(f.key(&KeyName::Right, CTRL, None), changed(65));
    assert_eq!(f.key(&KeyName::PageDown, NONE, None), changed(40));
    assert_eq!(f.key(&KeyName::PageUp, NONE, None), changed(65));
    assert_eq!(f.key(&KeyName::Char('-'), NONE, None), changed(60));
    assert_eq!(f.key(&KeyName::Char('+'), NONE, None), changed(65));
    assert_eq!(f.key(&KeyName::End, NONE, None), changed(100));
    assert_eq!(f.key(&KeyName::End, NONE, None), Outcome::None);
    assert_eq!(f.key(&KeyName::Right, NONE, None), Outcome::None);
    assert_eq!(f.key(&KeyName::Home, NONE, None), changed(0));
    assert_eq!(f.value(SLIDER), Some(Value::Number(0)));
    assert_eq!(f.key(&KeyName::Enter, NONE, None), Outcome::None);
    assert_eq!(f.key(&KeyName::Char('x'), NONE, None), Outcome::Unused);
    // Up and Down move the keyboard, as from every row of this form.
    assert_eq!(f.key(&KeyName::Down, NONE, None), Outcome::Redraw);
    assert_ne!(f.focus_id(), Some(SLIDER));
}

#[test]
fn a_press_on_the_track_moves_the_value_and_a_drag_follows_the_pointer() {
    let (mut f, i) = laid();
    let r = f.slider_rect(i);
    assert!(r.w > 0.0 && f.control_rect(i).contains(r.x, r.cy()));
    // The far end of the track: the value goes there at once.
    assert_eq!(f.press(r.right() - 1.0, r.cy(), NONE, 1), changed(100));
    assert_eq!(f.focus_id(), Some(SLIDER));
    // Back to the low end with the button down, reported as it happens.
    assert_eq!(f.pointer(r.x + 1.0, r.cy()), changed(0));
    assert_eq!(f.pointer(r.x - 50.0, r.cy() + 40.0), Outcome::None);
    // Nothing new to report when the button goes up.
    assert_eq!(f.release(r.x, r.cy()), Outcome::Redraw);
    assert_eq!(f.value(SLIDER), Some(Value::Number(0)));
    // After the release the pointer only hovers.
    assert_ne!(f.pointer(r.cx(), r.cy()), changed(50));
    assert_eq!(f.value(SLIDER), Some(Value::Number(0)));
}

#[test]
fn a_drag_followed_by_pointer_move_is_reported_on_release() {
    let (mut f, i) = laid();
    let r = f.slider_rect(i);
    let thumb_x = r.x + 9.0 + (r.w - 18.0) * 0.4;
    // On the thumb: held where it was taken, so nothing changes yet.
    assert_eq!(f.press(thumb_x, r.cy(), NONE, 1), Outcome::Redraw);
    assert!(f.pointer_move(r.right(), r.cy()));
    assert_eq!(f.value(SLIDER), Some(Value::Number(100)));
    assert_eq!(f.release(r.right(), r.cy()), changed(100));
}

#[test]
fn a_slider_row_survives_its_rows_being_replaced_under_a_drag() {
    let (mut f, i) = laid();
    let r = f.slider_rect(i);
    f.press(r.cx(), r.cy(), NONE, 1);
    f.set_rows(vec![Row::toggle(1, "only", true)]);
    assert!(!matches!(f.pointer(r.x, r.cy()), Outcome::Changed { .. }));
    assert!(!f.pointer_move(r.x, r.cy()));
    f.release(r.x, r.cy());
    shot(&mut f, &LIGHT, None);
}

#[test]
fn the_plain_draw_shows_a_slider_rows_value() {
    let (mut f, i) = laid();
    let a = shot(&mut f, &LIGHT, None);
    f.set_value(SLIDER, Value::Number(90));
    let b = shot(&mut f, &LIGHT, None);
    assert!(a != b);
    // The thumb at 90 is near the right end of the track: the accent reaches there.
    let r = f.slider_rect(i);
    let x = (r.x + 9.0 + (r.w - 18.0) * 0.6) as usize;
    let y = r.cy() as usize;
    let k = (y * W as usize + x) * 4;
    let px = [b[k + 2], b[k + 1], b[k]];
    assert_eq!(px, LIGHT.accent);
}

#[test]
fn a_slider_row_draws_in_odd_windows_without_panicking() {
    for (w, h) in [(0.0, 0.0), (30.0, 900.0), (f32::NAN, 100.0)] {
        let mut f = form();
        let mut bmp = Bitmap::new(200, 200);
        let mut gfx = Gfx::new();
        let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
        let mut anim = Animator::new();
        anim.begin_frame(ManualClock::new().now());
        f.draw_animated(&mut p, &DARK, Rect::new(0.0, 0.0, w, h), true, &mut anim, 7);
        anim.end_frame();
        let i = index_of(&f, SLIDER);
        f.press(f.slider_rect(i).cx(), 50.0, NONE, 1);
        f.pointer(10.0, 10.0);
        f.release(10.0, 10.0);
    }
}

/// For a person to judge: the form with its slider row at rest with the keyboard on it, and
/// a frame half way through a switch turning on, the chosen segment moving and the slider's
/// thumb gliding to its end; light and dark, scales 1 and 1.5. Written to `target/specimen/`
/// (`cargo test -p mdshared-ui --lib settings_motion_sheet -- --ignored`).
#[test]
#[ignore]
fn settings_motion_sheet() {
    let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/specimen");
    std::fs::create_dir_all(&out).unwrap();
    for (scale, sfx) in [(1.0, ""), (1.5, "-150")] {
        for (pal, theme) in [(&LIGHT, "light"), (&DARK, "dark")] {
            let mut f = form();
            let clock = ManualClock::new();
            let mut anim = Animator::new();
            let mut frame = |f: &mut Form, ms: u64, name: Option<String>| {
                clock.advance_ms(ms);
                anim.begin_frame(clock.now());
                let mut bmp = Bitmap::new((W * scale) as u32, (H * scale) as u32);
                bmp.clear([128; 3]);
                let mut gfx = Gfx::new();
                {
                    let mut p = Painter::new(&mut bmp, &mut gfx, scale);
                    let window = Rect::new(0.0, 0.0, W, H);
                    f.draw_animated(&mut p, pal, window, false, &mut anim, "settings");
                }
                anim.end_frame();
                if let Some(name) = name {
                    bmp.write_png(&out.join(name)).unwrap();
                }
            };
            frame(&mut f, 0, None);
            f.focus_row(SLIDER);
            f.key(&KeyName::Tab, NONE, None);
            frame(&mut f, 16, None);
            frame(
                &mut f,
                200,
                Some(format!("settings-slider-{theme}{sfx}.png")),
            );
            f.set_value(TOGGLE, Value::Bool(true));
            f.set_value(SEGMENTS, Value::Choice(2));
            f.key(&KeyName::End, NONE, None);
            frame(&mut f, 16, None);
            frame(&mut f, 16, None);
            frame(
                &mut f,
                50,
                Some(format!("settings-motion-{theme}{sfx}.png")),
            );
        }
    }
}

#[test]
fn a_drag_ends_when_the_rows_are_replaced_with_another_slider_in_its_place() {
    let (mut f, i) = laid();
    let r = f.slider_rect(i);
    f.press(r.cx(), r.cy(), NONE, 1);
    // Another slider at the same index.
    let mut rows = rows();
    rows[i] = Row::number(6, "Other", 3, 0, 10);
    f.show_as_slider(6);
    f.set_rows(rows);
    shot(&mut f, &LIGHT, None);
    assert!(!matches!(f.pointer(r.x, r.cy()), Outcome::Changed { .. }));
    assert!(!f.pointer_move(r.right(), r.cy()) || f.value(6) == Some(Value::Number(3)));
    assert_eq!(f.value(6), Some(Value::Number(3)));
    assert!(!matches!(f.release(r.x, r.cy()), Outcome::Changed { .. }));
    assert_eq!(f.value(6), Some(Value::Number(3)));
}

#[test]
fn a_drag_ends_when_a_row_is_put_in_above_it() {
    let (mut f, i) = laid();
    let r = f.slider_rect(i);
    f.press(r.cx(), r.cy(), NONE, 1);
    let mut rows = rows();
    rows.insert(1, Row::number(8, "Above", 1, 0, 5));
    f.set_rows(rows);
    shot(&mut f, &LIGHT, None);
    assert!(!matches!(
        f.pointer(r.right(), r.cy()),
        Outcome::Changed { .. }
    ));
    assert!(!matches!(
        f.release(r.right(), r.cy()),
        Outcome::Changed { .. }
    ));
    assert_eq!(f.value(SLIDER), Some(Value::Number(40)));
    assert_eq!(f.value(8), Some(Value::Number(1)));
}

#[test]
fn a_dropdown_opened_during_a_drag_lets_the_thumb_go() {
    let (mut f, i) = laid();
    let r = f.slider_rect(i);
    f.press(r.cx(), r.cy(), NONE, 1);
    let before = f.value(SLIDER);
    // The keyboard goes up to the dropdown and opens it, the button still down.
    f.key(&KeyName::Up, NONE, None);
    f.key(&KeyName::Up, NONE, None);
    assert_eq!(f.focus_id(), Some(3));
    assert_eq!(f.key(&KeyName::Enter, NONE, None), Outcome::Redraw);
    assert!(!matches!(f.pointer(r.x, r.cy()), Outcome::Changed { .. }));
    assert_eq!(f.value(SLIDER), before);
}

#[test]
fn a_hand_built_row_with_its_ends_crossed_does_not_panic() {
    let crossed = Row::Number {
        id: 1,
        label: "x".into(),
        help: String::new(),
        value: 5,
        min: 10,
        max: 0,
        step: 1,
        unit: String::new(),
    };
    let mut f = Form::new("s", vec![crossed]);
    f.show_as_slider(1);
    let mut fr = Frames::new();
    fr.frame(&mut f, &LIGHT, 0);
    f.focus_row(1);
    assert_eq!(
        f.key(&KeyName::End, NONE, None),
        Outcome::Changed {
            id: 1,
            value: Value::Number(10)
        }
    );
    f.key(&KeyName::Home, NONE, None);
    f.key(&KeyName::PageUp, NONE, None);
    f.key(&KeyName::Left, CTRL, None);
    let r = f.slider_rect(0);
    f.press(r.right() - 1.0, r.cy(), NONE, 1);
    f.pointer(r.x, r.cy());
    f.release(r.x, r.cy());
    assert_eq!(f.value(1), Some(Value::Number(0)));
    fr.frame(&mut f, &LIGHT, 16);
}

#[test]
fn whole_numbers_beyond_an_f32s_are_exact_by_the_keys_and_inside_the_ends_by_the_pointer() {
    const BIG: i32 = 16_777_217; // 2^24 + 1: no f32 holds it.
    let mut f = Form::new("s", vec![Row::number(1, "big", BIG, BIG - 2, BIG + 2)]);
    f.show_as_slider(1);
    shot(&mut f, &LIGHT, None);
    f.focus_row(1);
    let n = |v: i32| Outcome::Changed {
        id: 1,
        value: Value::Number(v),
    };
    assert_eq!(f.key(&KeyName::Right, NONE, None), n(BIG + 1));
    assert_eq!(f.key(&KeyName::Right, NONE, None), n(BIG + 2));
    assert_eq!(f.key(&KeyName::Left, NONE, None), n(BIG + 1));
    assert_eq!(f.key(&KeyName::Home, NONE, None), n(BIG - 2));
    assert_eq!(f.key(&KeyName::Right, NONE, None), n(BIG - 1));
    let r = f.slider_rect(0);
    assert_eq!(f.press(r.right() - 1.0, r.cy(), NONE, 1), n(BIG + 2));
    for k in 0..=40 {
        f.pointer(r.x - 20.0 + k as f32 * 6.0, r.cy());
        match f.value(1) {
            Some(Value::Number(v)) => assert!((BIG - 2..=BIG + 2).contains(&v), "{v}"),
            other => panic!("{other:?}"),
        }
    }
    f.pointer(r.x, r.cy());
    f.release(r.x, r.cy());
    assert_eq!(f.value(1), Some(Value::Number(BIG - 2)));
    // The widest range: the keys' arithmetic does not overflow.
    let mut f = Form::new("s", vec![Row::number(1, "all", 0, i32::MIN, i32::MAX)]);
    f.show_as_slider(1);
    shot(&mut f, &LIGHT, None);
    f.focus_row(1);
    assert_eq!(f.key(&KeyName::End, NONE, None), n(i32::MAX));
    assert_eq!(f.key(&KeyName::PageUp, NONE, None), Outcome::None);
    assert_eq!(f.key(&KeyName::Right, NONE, None), Outcome::None);
    assert_eq!(f.key(&KeyName::Home, NONE, None), n(i32::MIN));
    assert_eq!(f.key(&KeyName::PageDown, NONE, None), Outcome::None);
    assert!(matches!(
        f.key(&KeyName::PageUp, NONE, None),
        Outcome::Changed { .. }
    ));
}
