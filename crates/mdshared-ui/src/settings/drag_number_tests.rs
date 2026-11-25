//! Draggable number rows ([`Form::make_draggable`]): a drag on the value's text changes it by
//! the step for every 4 px (16 with Shift), held inside the ends; Escape puts it back; the
//! release and `set_rows` end the drag; a row or a form without the mark answers and draws as
//! the form on main (and as the frozen copy of the old form) did; the cursor follows. No clock.

use super::drag_number::{FINE_PX_PER_STEP, PX_PER_STEP};
use super::{Form, Outcome, Row, Value, frozen};
use crate::cursor::Cursor;
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
const SHIFT: Mods = Mods {
    ctrl: false,
    shift: true,
    alt: false,
    control: false,
};
const W: f32 = 1000.0;
const H: f32 = 800.0;

const WRAP: u32 = 1;
const SIZE: u32 = 3;
const WIDTH: u32 = 6;
const STEPPED: u32 = 8;

fn rows(size: i32) -> Vec<Row> {
    vec![
        Row::header("Editor"),
        Row::toggle(WRAP, "Word wrap", false).help("Wrap long lines"),
        Row::number(SIZE, "Font size", size, 8, 32).unit("px"),
        Row::number(STEPPED, "Width", 100, 0, 1000).step(25),
        Row::choice(
            WIDTH,
            "Page width",
            &["Narrow", "Medium", "Wide", "Full", "Custom"],
            1,
        ),
    ]
}

fn lay(f: &mut Form) {
    let mut gfx = Gfx::new();
    let mut bmp = Bitmap::new(W as u32, H as u32);
    let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
    f.layout(&mut p, Rect::new(0.0, 0.0, W, H));
}

fn shot(f: &mut Form, pal: &Palette) -> Vec<u8> {
    let mut gfx = Gfx::new();
    let mut bmp = Bitmap::new(W as u32, H as u32);
    {
        let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
        f.draw(&mut p, pal, Rect::new(0.0, 0.0, W, H), true);
    }
    bmp.pixels
}

fn index_of(f: &Form, id: u32) -> usize {
    f.rows().iter().position(|r| r.id() == Some(id)).unwrap()
}

/// A laid out form with `ids` marked draggable.
fn form(ids: &[u32]) -> Form {
    let mut f = Form::new("Settings", rows(14));
    for &id in ids {
        f.make_draggable(id);
    }
    lay(&mut f);
    f
}

/// The middle of row `id`'s value text.
fn value_at(f: &Form, id: u32) -> (f32, f32) {
    let v = f.stepper_parts(index_of(f, id)).1;
    (v.cx(), v.cy())
}

fn changed(id: u32, v: i32) -> Outcome {
    Outcome::Changed {
        id,
        value: Value::Number(v),
    }
}

fn number(f: &Form, id: u32) -> i32 {
    match f.value(id) {
        Some(Value::Number(v)) => v,
        other => panic!("not a number: {other:?}"),
    }
}

#[test]
fn a_drag_right_or_left_changes_the_value_by_the_step_for_every_four_px() {
    assert_eq!(PX_PER_STEP, 4.0);
    let mut f = form(&[SIZE, STEPPED]);
    let (x, y) = value_at(&f, SIZE);
    assert_eq!(f.press(x, y, NONE, 1), Outcome::Redraw);
    assert_eq!(f.pointer(x + 3.0, y), Outcome::None, "less than 4 px");
    assert_eq!(f.pointer(x + 4.0, y), changed(SIZE, 15));
    assert_eq!(
        f.pointer(x + 12.0, y + 30.0),
        changed(SIZE, 17),
        "up or down is no matter"
    );
    assert_eq!(f.pointer(x + 13.0, y), Outcome::None);
    assert_eq!(f.pointer(x - 4.0, y), changed(SIZE, 13));
    assert_eq!(
        f.pointer(x - 3.5, y),
        changed(SIZE, 14),
        "towards zero on both sides"
    );
    assert_eq!(f.pointer(x - 8.0, y), changed(SIZE, 12));
    assert_eq!(f.release(x - 8.0, y), Outcome::Redraw, "already reported");
    assert_eq!(number(&f, SIZE), 12);
    assert!(
        !matches!(f.pointer(x + 40.0, y), Outcome::Changed { .. }),
        "the release ended it"
    );
    assert_eq!(number(&f, SIZE), 12);

    // A step of 25: 25 for every 4 px.
    let (x, y) = value_at(&f, STEPPED);
    f.press(x, y, NONE, 1);
    assert_eq!(f.pointer(x + 8.0, y), changed(STEPPED, 150));
    assert_eq!(f.pointer(x - 4.0, y), changed(STEPPED, 75));
    assert_eq!(f.release(x - 4.0, y), Outcome::Redraw);
}

#[test]
fn the_drag_stops_at_both_ends_and_takes_nonsense_places() {
    let mut f = form(&[SIZE]);
    let (x, y) = value_at(&f, SIZE);
    f.press(x, y, NONE, 1);
    assert_eq!(f.pointer(x + 1000.0, y), changed(SIZE, 32));
    assert_eq!(f.pointer(x + 2000.0, y), Outcome::None, "held at the top");
    assert_eq!(f.pointer(f32::INFINITY, y), Outcome::None);
    assert_eq!(f.pointer(f32::NAN, y), Outcome::None);
    assert_eq!(f.pointer(x - 1000.0, y), changed(SIZE, 8));
    assert_eq!(
        f.pointer(f32::NEG_INFINITY, y),
        Outcome::None,
        "held at the bottom"
    );
    assert_eq!(f.pointer(f32::MAX, f32::NAN), changed(SIZE, 32));
    assert_eq!(f.release(f32::NAN, f32::NAN), Outcome::Redraw);
    assert_eq!(number(&f, SIZE), 32);
}

#[test]
fn with_shift_the_drag_is_one_step_for_every_sixteen_px_and_changing_shift_does_not_jump() {
    assert_eq!(FINE_PX_PER_STEP, 16.0);
    let mut f = form(&[SIZE]);
    let (x, y) = value_at(&f, SIZE);
    // Shift held at the press.
    f.press(x, y, SHIFT, 1);
    assert_eq!(f.pointer(x + 15.0, y), Outcome::None);
    assert_eq!(f.pointer(x + 16.0, y), changed(SIZE, 15));
    assert_eq!(f.pointer(x + 48.0, y), changed(SIZE, 17));
    f.release(x + 48.0, y);

    // Shift pressed and let go during a drag.
    let mut f = form(&[SIZE]);
    f.press(x, y, NONE, 1);
    assert_eq!(f.pointer_with(x + 8.0, y, NONE), changed(SIZE, 16));
    assert_eq!(f.pointer_with(x + 8.0, y, SHIFT), Outcome::None, "no jump");
    assert_eq!(f.pointer_with(x + 20.0, y, SHIFT), Outcome::None);
    assert_eq!(f.pointer_with(x + 24.0, y, SHIFT), changed(SIZE, 17));
    assert_eq!(f.pointer_with(x + 24.0, y, NONE), Outcome::None, "no jump");
    assert_eq!(f.pointer_with(x + 28.0, y, NONE), changed(SIZE, 18));
    assert_eq!(f.pointer_with(x + 20.0, y, NONE), changed(SIZE, 16));
    assert_eq!(f.release(x + 20.0, y), Outcome::Redraw);
    // Shift first seen at a place that is no number: the pace waits for a real place.
    let mut f = form(&[SIZE]);
    f.press(x, y, NONE, 1);
    assert_eq!(f.pointer_with(x + 40.0, y, NONE), changed(SIZE, 24));
    assert_eq!(f.pointer_with(f32::NAN, y, SHIFT), Outcome::None);
    assert_eq!(f.pointer_with(x + 40.0, y, SHIFT), Outcome::None, "no jump");
    assert_eq!(f.pointer_with(x + 56.0, y, SHIFT), changed(SIZE, 25));
    f.release(x + 56.0, y);
    // Without a drag, pointer_with is pointer.
    let mut a = form(&[SIZE]);
    let mut b = form(&[SIZE]);
    let c = a.control_rect(index_of(&a, WRAP));
    assert_eq!(
        a.pointer_with(c.cx(), c.cy(), SHIFT),
        b.pointer(c.cx(), c.cy())
    );
    assert_eq!(shot(&mut a, &LIGHT), shot(&mut b, &LIGHT));
}

#[test]
fn escape_puts_back_the_value_the_drag_began_with_and_ends_the_drag() {
    let mut f = form(&[SIZE]);
    let (x, y) = value_at(&f, SIZE);
    f.press(x, y, NONE, 1);
    assert_eq!(f.pointer(x + 24.0, y), changed(SIZE, 20));
    assert_eq!(f.key(&KeyName::Escape, NONE, None), changed(SIZE, 14));
    assert_eq!(number(&f, SIZE), 14);
    assert_ne!(
        f.pointer(x + 40.0, y),
        changed(SIZE, 24),
        "the drag is over"
    );
    assert_eq!(number(&f, SIZE), 14);
    assert_eq!(
        f.release(x + 40.0, y),
        Outcome::None,
        "and its release does nothing"
    );
    assert_eq!(number(&f, SIZE), 14);

    // Escape before the value moved: nothing to put back, and the dialog stays open.
    f.press(x, y, NONE, 1);
    assert_eq!(f.key(&KeyName::Escape, NONE, None), Outcome::Redraw);
    // Escape with no drag closes, as always.
    assert_eq!(f.key(&KeyName::Escape, NONE, None), Outcome::Closed);

    // Under pointer_move, which reports nothing, Escape still reports the value put back.
    f.press(x, y, NONE, 1);
    assert!(f.pointer_move(x + 8.0, y));
    assert_eq!(number(&f, SIZE), 16);
    assert_eq!(f.key(&KeyName::Escape, NONE, None), changed(SIZE, 14));
}

#[test]
fn under_pointer_move_the_change_is_reported_on_release() {
    let mut f = form(&[SIZE]);
    let (x, y) = value_at(&f, SIZE);
    f.press(x, y, NONE, 1);
    assert!(!f.pointer_move(x + 2.0, y));
    assert!(f.pointer_move(x + 8.0, y));
    assert_eq!(number(&f, SIZE), 16);
    assert_eq!(f.release(x + 8.0, y), changed(SIZE, 16));
    // Moved and back where it began: nothing to report, as after a click.
    f.press(x, y, NONE, 1);
    assert!(f.pointer_move(x + 8.0, y));
    assert!(f.pointer_move(x, y));
    assert_eq!(f.release(x, y), Outcome::None);
    assert_eq!(number(&f, SIZE), 16);
}

#[test]
fn a_click_on_a_draggable_value_and_its_buttons_and_keys_work_as_before() {
    let mut f = form(&[SIZE]);
    let i = index_of(&f, SIZE);
    let (x, y) = value_at(&f, SIZE);
    assert_eq!(f.press(x, y, NONE, 1), Outcome::Redraw);
    assert_eq!(
        f.release(x, y),
        Outcome::None,
        "a click on the value is no button"
    );
    let (m, _, p) = f.stepper_parts(i);
    for (r, want) in [(p, 15), (p, 16), (m, 15)] {
        f.press(r.cx(), r.cy(), NONE, 1);
        assert_eq!(f.release(r.cx(), r.cy()), changed(SIZE, want));
    }
    assert_eq!(f.key(&KeyName::Right, NONE, None), changed(SIZE, 16));
    assert_eq!(f.key(&KeyName::End, NONE, None), changed(SIZE, 32));
    // A press on a button and a move does not drag.
    f.press(m.cx(), m.cy(), NONE, 1);
    f.pointer_move(m.cx() - 40.0, m.cy());
    assert_eq!(number(&f, SIZE), 32);
    assert_eq!(f.cursor_at(m.cx(), m.cy()), None);
}

/// The steps a drag takes, given to the live form and to the frozen copy of the old one; the
/// outcomes and the frames as text and pixels.
fn drag_steps_live(f: &mut Form, id: u32) -> Vec<(String, Vec<u8>)> {
    let (x, y) = value_at(f, id);
    let mut out = Vec::new();
    let o = f.press(x, y, NONE, 1);
    out.push((format!("press {o:?}"), shot(f, &LIGHT)));
    for dx in [4.0, 12.0, -20.0, 400.0] {
        let o = f.pointer_move(x + dx, y);
        out.push((format!("move {dx} {o}"), shot(f, &DARK)));
    }
    let o = f.key(&KeyName::Escape, NONE, None);
    out.push((format!("escape {o:?}"), shot(f, &LIGHT)));
    let o = f.release(x + 400.0, y);
    out.push((format!("release {o:?} {:?}", f.value(id)), shot(f, &LIGHT)));
    out
}

fn drag_steps_frozen(f: &mut frozen::Form, id: u32) -> Vec<(String, Vec<u8>)> {
    let shot = |f: &mut frozen::Form, pal: &Palette| {
        let mut gfx = Gfx::new();
        let mut bmp = Bitmap::new(W as u32, H as u32);
        {
            let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
            f.draw(&mut p, pal, Rect::new(0.0, 0.0, W, H), true);
        }
        bmp.pixels
    };
    {
        let mut gfx = Gfx::new();
        let mut bmp = Bitmap::new(W as u32, H as u32);
        let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
        f.layout(&mut p, Rect::new(0.0, 0.0, W, H));
    }
    let i = f.rows().iter().position(|r| r.id() == Some(id)).unwrap();
    let v = f.stepper_parts(i).1;
    let (x, y) = (v.cx(), v.cy());
    let mut out = Vec::new();
    let o = f.press(x, y, NONE, 1);
    out.push((format!("press {o:?}"), shot(f, &LIGHT)));
    for dx in [4.0, 12.0, -20.0, 400.0] {
        let o = f.pointer_move(x + dx, y);
        out.push((format!("move {dx} {o}"), shot(f, &DARK)));
    }
    let o = f.key(&KeyName::Escape, NONE, None);
    out.push((format!("escape {o:?}"), shot(f, &LIGHT)));
    let o = f.release(x + 400.0, y);
    out.push((format!("release {o:?} {:?}", f.value(id)), shot(f, &LIGHT)));
    out
}

fn frozen_rows() -> Vec<frozen::Row> {
    vec![
        frozen::Row::header("Editor"),
        frozen::Row::toggle(WRAP, "Word wrap", false).help("Wrap long lines"),
        frozen::Row::number(SIZE, "Font size", 14, 8, 32).unit("px"),
        frozen::Row::number(STEPPED, "Width", 100, 0, 1000).step(25),
        frozen::Row::choice(
            WIDTH,
            "Page width",
            &["Narrow", "Medium", "Wide", "Full", "Custom"],
            1,
        ),
    ]
}

#[test]
fn a_row_without_the_mark_ignores_a_drag_as_the_old_form_did() {
    let old = drag_steps_frozen(&mut frozen::Form::new("Settings", frozen_rows()), SIZE);
    for marks in [&[][..], &[STEPPED][..], &[WRAP, 999][..]] {
        let mut f = Form::new("Settings", rows(14));
        for &id in marks {
            f.make_draggable(id);
        }
        lay(&mut f);
        let new = drag_steps_live(&mut f, SIZE);
        assert_eq!(old.len(), new.len());
        for ((a, pa), (b, pb)) in old.iter().zip(&new) {
            assert_eq!(a, b, "marks {marks:?}");
            assert!(pa == pb, "marks {marks:?}: {a} drew other pixels");
        }
        assert_eq!(
            f.cursor_at(value_at(&f, SIZE).0, value_at(&f, SIZE).1),
            None
        );
    }
    // The steps say what they did: nothing changed, and Escape closed.
    assert!(old.iter().any(|(s, _)| s.starts_with("escape Closed")));
    assert!(old.last().unwrap().0.contains("Number(14)"));
}

#[test]
fn a_marked_form_draws_as_an_unmarked_one_at_rest_hovered_and_during_a_drag() {
    for pal in [&LIGHT, &DARK] {
        let mut plain = form(&[]);
        let mut told = form(&[SIZE, STEPPED]);
        assert!(shot(&mut plain, pal) == shot(&mut told, pal), "at rest");
        let (x, y) = value_at(&told, SIZE);
        assert_eq!(plain.pointer_move(x, y), told.pointer_move(x, y));
        assert!(shot(&mut plain, pal) == shot(&mut told, pal), "hovered");
        // Mid-drag the row is the stepper showing the new value.
        assert_eq!(plain.press(x, y, NONE, 1), told.press(x, y, NONE, 1));
        assert_eq!(told.pointer(x + 12.0, y), changed(SIZE, 17));
        assert!(plain.set_value(SIZE, Value::Number(17)));
        assert!(
            shot(&mut plain, pal) == shot(&mut told, pal),
            "during a drag"
        );
    }
}

#[test]
fn set_rows_ends_a_drag_and_the_mark_outlasts_it() {
    let mut f = form(&[SIZE]);
    let (x, y) = value_at(&f, SIZE);
    f.press(x, y, NONE, 1);
    assert_eq!(f.pointer(x + 8.0, y), changed(SIZE, 16));
    f.set_rows(rows(20));
    assert_eq!(f.pointer(x + 40.0, y), Outcome::None, "the drag is over");
    assert_eq!(
        f.key(&KeyName::Escape, NONE, None),
        Outcome::Closed,
        "nothing to put back"
    );
    assert_eq!(number(&f, SIZE), 20);
    assert!(!matches!(f.release(x + 40.0, y), Outcome::Changed { .. }));
    // Laid out again, the rebuilt row is still draggable.
    lay(&mut f);
    let (x, y) = value_at(&f, SIZE);
    f.press(x, y, NONE, 1);
    assert_eq!(f.pointer(x + 4.0, y), changed(SIZE, 21));
    f.release(x + 4.0, y);
}

#[test]
fn the_mark_names_number_rows_by_id_and_a_slider_stays_a_slider() {
    let mut f = Form::new("Settings", rows(14));
    assert!(!f.make_draggable(WRAP), "a toggle is not a number row");
    assert!(!f.make_draggable(77), "no such row yet");
    assert!(f.make_draggable(SIZE));
    assert!(f.make_draggable(SIZE), "asking twice is asking once");
    let mut more = rows(14);
    more.push(Row::number(77, "Later", 0, 0, 10));
    f.set_rows(more);
    lay(&mut f);
    let (x, y) = value_at(&f, 77);
    f.press(x, y, NONE, 1);
    assert_eq!(
        f.pointer(x + 4.0, y),
        changed(77, 1),
        "marked before it came"
    );
    f.release(x + 4.0, y);
    // A toggle with a marked id ignores a drag.
    let c = f.control_rect(index_of(&f, WRAP));
    f.press(c.cx(), c.cy(), NONE, 1);
    assert_eq!(f.pointer(c.cx() + 40.0, c.cy()), Outcome::None);
    assert_eq!(f.cursor_at(c.cx(), c.cy()), None);
    f.release(c.cx() + 40.0, c.cy());

    // Also a slider: the thumb is dragged, the value text is not.
    let mut f = Form::new("Settings", rows(14));
    f.make_draggable(SIZE);
    f.show_as_slider(SIZE);
    lay(&mut f);
    let i = index_of(&f, SIZE);
    let s = f.slider_rect(i);
    let (x, y) = (s.x - 20.0, s.cy());
    assert_eq!(f.cursor_at(x, y), None);
    f.press(x, y, NONE, 1);
    assert!(!matches!(f.pointer(x - 40.0, y), Outcome::Changed { .. }));
    assert_eq!(number(&f, SIZE), 14);
}

#[test]
fn crossed_ends_a_zero_step_and_the_whole_i32_range_do_not_panic() {
    let crossed = Row::Number {
        id: 1,
        label: "Crossed".into(),
        help: String::new(),
        value: 5,
        min: 10,
        max: 0,
        step: 0,
        unit: String::new(),
    };
    let mut f = Form::new(
        "Settings",
        vec![crossed, Row::number(2, "All", 0, i32::MIN, i32::MAX)],
    );
    f.make_draggable(1);
    f.make_draggable(2);
    lay(&mut f);
    let (x, y) = value_at(&f, 1);
    f.press(x, y, NONE, 1);
    assert_eq!(f.pointer(x + 4.0, y), changed(1, 6), "a step of 0 is 1");
    assert_eq!(
        f.pointer(x + 1e6, y),
        changed(1, 10),
        "the ends read swapped"
    );
    assert_eq!(f.pointer(x - 1e6, y), changed(1, 0));
    assert_eq!(f.key(&KeyName::Escape, NONE, None), changed(1, 5));

    // The whole i32 range with the largest step.
    if let Some(Row::Number { step, .. }) = f.rows.get_mut(1) {
        *step = i32::MAX;
    }
    let (x, y) = value_at(&f, 2);
    f.press(x, y, NONE, 1);
    assert_eq!(f.pointer(x + 4.0, y), changed(2, i32::MAX));
    assert_eq!(f.pointer(x - 4.0, y), changed(2, -i32::MAX));
    assert_eq!(f.pointer(f32::MIN, y), changed(2, i32::MIN));
    assert_eq!(f.pointer(f32::MAX, y), changed(2, i32::MAX));
    f.release(0.0, 0.0);
}

#[test]
fn the_cursor_is_the_left_right_arrow_over_a_draggable_value_and_through_its_drag() {
    assert_eq!(Cursor::ResizeEw.icon(), winit::window::CursorIcon::EwResize);
    let mut f = form(&[SIZE]);
    let i = index_of(&f, SIZE);
    let (x, y) = value_at(&f, SIZE);
    assert_eq!(f.cursor_at(x, y), Some(Cursor::ResizeEw));
    let (m, _, p) = f.stepper_parts(i);
    assert_eq!(f.cursor_at(m.cx(), m.cy()), None, "the buttons are buttons");
    assert_eq!(f.cursor_at(p.cx(), p.cy()), None);
    let label = f.rect_of_row(i);
    assert_eq!(f.cursor_at(label.x + 30.0, label.cy()), None, "the label");
    let (sx, sy) = value_at(&f, STEPPED);
    assert_eq!(f.cursor_at(sx, sy), None, "a row not marked");
    assert_eq!(f.cursor_at(f32::NAN, f32::NAN), None);
    // Held through the drag, wherever the pointer goes.
    f.press(x, y, NONE, 1);
    f.pointer(-500.0, -500.0);
    assert_eq!(f.cursor_at(-500.0, -500.0), Some(Cursor::ResizeEw));
    f.release(-500.0, -500.0);
    assert_eq!(f.cursor_at(-500.0, -500.0), None);
    // Not while a dropdown is open over the form.
    let d = f.control_rect(index_of(&f, WIDTH));
    f.press(d.cx(), d.cy(), NONE, 1);
    f.release(d.cx(), d.cy());
    assert_eq!(f.cursor_at(x, y), None);
    // A form never told answers None everywhere, and before a layout too.
    let plain = form(&[]);
    assert_eq!(plain.cursor_at(x, y), None);
    let mut unlaid = Form::new("Settings", rows(14));
    unlaid.make_draggable(SIZE);
    assert_eq!(unlaid.cursor_at(x, y), None);
}
