//! Text rows shown as colours ([`Form::show_as_colour`]): a form never told draws and answers as
//! the frozen form; a told one draws a swatch, opens its picker from the swatch and the keys,
//! puts every colour picked into the row in the row's own form and reports it, cancels by
//! Escape and by a press outside, follows typing both ways, and closes at `set_rows`. With the
//! ignored timing test and the sheet for a person to look at.
//!
//! `cargo test --release -p mdshared-ui colour_picker_draw_time -- --ignored --nocapture` for
//! the timings, `cargo test -p mdshared-ui colour_sheet -- --ignored` for the sheet (written to
//! `specimen/` under `CARGO_TARGET_DIR`, or the workspace's `target`).

use super::{Form, Outcome, Row, Value};
use crate::colour_picker::{Hsv, Layout, hsv_to_rgb};
use crate::gfx::{Gfx, Painter, Rect, Rgb};
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
const ALT: Mods = Mods {
    ctrl: false,
    shift: false,
    alt: true,
    control: false,
};
const W: f32 = 760.0;
const H: f32 = 620.0;

const ACCENT: u32 = 1;
const UPPER: u32 = 2;
const NAME: u32 = 3;
const BAD: u32 = 4;
const WRAP: u32 = 5;

fn rows() -> Vec<Row> {
    let mut v = vec![
        Row::header("Look"),
        Row::toggle(WRAP, "Wrap lines", false),
        Row::text(ACCENT, "Accent", "#3366cc").help("The colour of the highlights"),
        Row::text(UPPER, "colour.playing", "CC8800"),
        Row::text(NAME, "Name", "abc"),
        Row::text(BAD, "colour.broken", "not a colour"),
        Row::header("More"),
    ];
    for k in 0..6 {
        v.push(Row::toggle(10 + k, format!("Option {k}"), k % 2 == 0));
    }
    v
}

fn told() -> Form {
    let mut f = Form::new("Look", rows()).single_page();
    assert!(f.show_as_colour(ACCENT));
    assert!(f.show_as_colour(UPPER));
    assert!(f.show_as_colour(BAD));
    f
}

fn index(f: &Form, id: u32) -> usize {
    f.rows().iter().position(|r| r.id() == Some(id)).unwrap()
}

fn shot_at(f: &mut Form, pal: &Palette, w: f32, h: f32, scale: f32) -> Bitmap {
    let mut bmp = Bitmap::new((w * scale) as u32, (h * scale) as u32);
    bmp.clear([128; 3]);
    let mut gfx = Gfx::new();
    {
        let mut p = Painter::new(&mut bmp, &mut gfx, scale);
        f.draw(&mut p, pal, Rect::new(0.0, 0.0, w, h), false);
    }
    bmp
}

fn shot(f: &mut Form, pal: &Palette) -> Bitmap {
    shot_at(f, pal, W, H, 1.0)
}

fn pixel(bmp: &Bitmap, x: f32, y: f32) -> Rgb {
    let o = (y as usize * bmp.width as usize + x as usize) * 4;
    let p = &bmp.pixels[o..o + 3];
    [p[2], p[1], p[0]]
}

fn click(f: &mut Form, x: f32, y: f32) -> (Outcome, Outcome) {
    let a = f.press(x, y, NONE, 1);
    let b = f.release(x, y);
    (a, b)
}

/// A laid-out told form with the picker of `id` open from its swatch.
fn opened(id: u32) -> (Form, Layout) {
    let mut f = told();
    shot(&mut f, &LIGHT);
    let s = f.swatch_rect(index(&f, id));
    assert_eq!(
        click(&mut f, s.cx(), s.cy()),
        (Outcome::Redraw, Outcome::Redraw)
    );
    assert_eq!(f.colour_open(), Some(id));
    let l = f.picker_layout().unwrap();
    (f, l)
}

fn text(id: u32, t: &str) -> Outcome {
    Outcome::Changed {
        id,
        value: Value::Text(t.into()),
    }
}

#[test]
fn a_confirming_colour_row_reports_only_on_apply_or_outside_press() {
    for apply_with_button in [true, false] {
        let mut f = told();
        assert!(f.show_as_colour_with_confirmation(UPPER));
        f.set_rows(rows());
        shot(&mut f, &LIGHT);
        let swatch = f.swatch_rect(index(&f, UPPER));
        click(&mut f, swatch.cx(), swatch.cy());
        let l = f.picker_confirmation_layout().unwrap();
        assert_eq!(f.picker_layout(), Some(l.picker));
        let sq = l.picker.square;
        assert_eq!(f.press(sq.x, sq.y, NONE, 1), Outcome::Redraw);
        assert_eq!(f.pointer(sq.x, sq.bottom()), Outcome::Redraw);
        assert_eq!(f.release(sq.x, sq.bottom()), Outcome::Redraw);
        assert_eq!(f.colour(UPPER), Some([0xcc, 0x88, 0x00]));

        if apply_with_button {
            assert_eq!(
                click(&mut f, l.apply.cx(), l.apply.cy()),
                (Outcome::Redraw, text(UPPER, "000000")),
            );
        } else {
            // A press on the anchor applies instead of reopening or acting on the form.
            assert_eq!(
                click(&mut f, swatch.cx(), swatch.cy()),
                (text(UPPER, "000000"), Outcome::None),
            );
        }
        assert_eq!(f.colour(UPPER), Some([0; 3]));
        assert_eq!(f.colour_open(), None);
        assert!(f.picker_confirmation_layout().is_none());
    }
}

#[test]
fn a_confirming_colour_row_cancels_without_rewriting_even_an_invalid_original() {
    for cancel_with_button in [true, false] {
        let mut f = told();
        assert!(f.show_as_colour_with_confirmation(BAD));
        shot(&mut f, &LIGHT);
        assert!(f.focus_row(BAD));
        assert_eq!(f.key(&KeyName::Enter, NONE, None), Outcome::Redraw);
        let l = f.picker_confirmation_layout().unwrap();
        click(&mut f, l.picker.hex.cx(), l.picker.hex.cy());
        f.key(&KeyName::Char('a'), Mods { ctrl: true, ..NONE }, None);
        for ch in "00cc44".chars() {
            assert_eq!(
                f.key(&KeyName::Char(ch), NONE, Some(&ch.to_string())),
                Outcome::Redraw,
            );
        }
        if cancel_with_button {
            assert_eq!(
                click(&mut f, l.cancel.cx(), l.cancel.cy()),
                (Outcome::Redraw, Outcome::Redraw),
            );
        } else {
            assert_eq!(f.key(&KeyName::Escape, NONE, None), Outcome::Redraw);
        }
        assert_eq!(f.value(BAD), Some(Value::Text("not a colour".into())));
        assert_eq!(f.colour_open(), None);
    }
}

#[test]
fn a_form_never_told_draws_and_answers_as_the_frozen_form_with_hex_rows() {
    use super::frozen as old;
    macro_rules! run {
        ($Form:ty, $Row:ty) => {{
            let mut out: Vec<(String, Vec<u8>)> = Vec::new();
            let rows = vec![
                <$Row>::header("Look"),
                <$Row>::text(1, "Accent", "#3366cc"),
                <$Row>::text(2, "colour.x", "CC8800"),
                <$Row>::toggle(3, "Wrap", true),
            ];
            for (dark, scale) in [(false, 1.0f32), (true, 1.0), (false, 1.5), (true, 1.5)] {
                let mut f = <$Form>::new("Look", rows.clone()).single_page();
                let snap = |f: &mut $Form, what: &str, out: &mut Vec<(String, Vec<u8>)>| {
                    let mut bmp = Bitmap::new((640.0 * scale) as u32, (420.0 * scale) as u32);
                    let mut gfx = Gfx::new();
                    {
                        let mut p = Painter::new(&mut bmp, &mut gfx, scale);
                        let pal = if dark { &DARK } else { &LIGHT };
                        f.draw(&mut p, pal, Rect::new(0.0, 0.0, 640.0, 420.0), true);
                    }
                    out.push((format!("{what} dark {dark} at {scale}"), bmp.pixels));
                };
                snap(&mut f, "opened", &mut out);
                let c = f.control_rect(1);
                // Where a swatch would be: the left end of the field.
                f.pointer_move(c.x + 5.0, c.cy());
                snap(&mut f, "hover", &mut out);
                let a = f.press(c.x + 5.0, c.cy(), NONE, 1);
                let b = f.release(c.x + 5.0, c.cy());
                snap(&mut f, &format!("clicked {a:?} {b:?}"), &mut out);
                let e = f.key(&KeyName::Enter, NONE, None);
                let k = f.key(&KeyName::Char('f'), NONE, Some("f"));
                let d = f.key(&KeyName::Down, ALT, None);
                let w = f.wheel(c.x, c.cy(), 30.0);
                snap(&mut f, &format!("keys {e:?} {k:?} {d:?} {w}"), &mut out);
            }
            out
        }};
    }
    let now = run!(Form, Row);
    let was = run!(old::Form, old::Row);
    assert_eq!(now.len(), was.len());
    for ((what, a), (what_was, b)) in now.iter().zip(&was) {
        assert_eq!(what, what_was);
        assert!(a == b, "draws differently: {what}");
    }
}

#[test]
fn the_marks_follow_ids_and_only_mark_text_rows() {
    let mut f = Form::new("Look", rows());
    assert!(!f.show_as_colour(WRAP), "a toggle is no colour");
    assert!(!f.show_as_colour(99), "no such row yet");
    assert!(f.show_as_colour(ACCENT));
    let mut secret = rows();
    secret.push(Row::text(50, "Key", "#123456").secret());
    f.set_rows(secret);
    assert!(!f.show_as_colour(50), "a secret is never a colour");
    shot(&mut f, &DARK);
    assert!(f.swatch_rect(index(&f, ACCENT)).w > 0.0);
    assert_eq!(f.swatch_rect(index(&f, WRAP)), Rect::default());
    assert_eq!(f.swatch_rect(index(&f, 50)), Rect::default());
    f.set_rows(vec![Row::text(99, "Later", "#000000")]);
    shot(&mut f, &DARK);
    assert!(f.swatch_rect(0).w > 0.0, "an id marked before its row came");
    assert_eq!(f.colour(99), Some([0; 3]));
    assert_eq!(f.colour(WRAP), None);
}

#[test]
fn the_row_draws_its_colour_in_a_swatch_before_a_narrower_field() {
    for pal in [&LIGHT, &DARK] {
        let mut plain = Form::new("Look", rows()).single_page();
        let mut f = told();
        let a = shot(&mut plain, pal);
        let b = shot(&mut f, pal);
        assert!(a.pixels != b.pixels);
        let i = index(&f, ACCENT);
        let s = f.swatch_rect(i);
        assert_eq!(s.h, f.control_rect(i).h);
        assert_eq!(pixel(&b, s.cx(), s.cy()), [0x33, 0x66, 0xcc]);
        let s = f.swatch_rect(index(&f, UPPER));
        assert_eq!(pixel(&b, s.cx(), s.cy()), [0xcc, 0x88, 0x00]);
        // Not a colour: the field's colour, a slash through it.
        let s = f.swatch_rect(index(&f, BAD));
        assert_eq!(pixel(&b, s.x + 5.0, s.y + 5.0), pal.field);
        // The control is where it was; the label is untouched.
        assert_eq!(f.control_rect(i), plain.control_rect(i));
        // Typing in the field is the row's as before, and the swatch follows.
        let c = f.control_rect(i);
        click(&mut f, c.right() - 4.0, c.cy());
        for _ in 0..2 {
            f.key(&KeyName::Backspace, NONE, None);
        }
        assert_eq!(
            f.key(&KeyName::Char('0'), NONE, Some("0")),
            text(ACCENT, "#33660")
        );
        assert_eq!(
            f.key(&KeyName::Char('0'), NONE, Some("0")),
            text(ACCENT, "#336600")
        );
        let b = shot(&mut f, pal);
        let s = f.swatch_rect(i);
        assert_eq!(pixel(&b, s.cx(), s.cy()), [0x33, 0x66, 0x00]);
        assert_eq!(f.colour(ACCENT), Some([0x33, 0x66, 0x00]));
    }
}

#[test]
fn a_click_on_the_swatch_opens_the_picker_over_the_form_inside_the_window() {
    for pal in [&LIGHT, &DARK] {
        let mut f = told();
        let closed = shot(&mut f, pal);
        let i = index(&f, ACCENT);
        let s = f.swatch_rect(i);
        // A press alone does not open it, and a release away does nothing.
        f.press(s.cx(), s.cy(), NONE, 1);
        assert_eq!(f.release(s.cx() + 100.0, s.cy() + 100.0), Outcome::None);
        assert_eq!(f.colour_open(), None);
        click(&mut f, s.cx(), s.cy());
        assert_eq!(f.colour_open(), Some(ACCENT));
        assert_eq!(f.focus_id(), Some(ACCENT));
        let open = shot(&mut f, pal);
        let card = f.picker_rect();
        assert!(card.w > 200.0 && card.h > 200.0);
        assert!(card.y >= f.control_rect(i).bottom(), "under its row");
        assert!(card.x >= 0.0 && card.right() <= W && card.bottom() <= H);
        let l = f.picker_layout().unwrap();
        assert!(closed.pixels != open.pixels);
        // The square shows the row's hue at its top right.
        let hue = hsv_to_rgb(Hsv::new(220.0, 1.0, 1.0));
        let got = pixel(&open, l.square.right() - 1.0, l.square.y + 1.0);
        assert!((0..3).all(|k| got[k].abs_diff(hue[k]) <= 8), "{got:?}");
        // The card covers what was under it.
        assert_eq!(pixel(&open, card.x + 4.0, card.bottom() - 4.0), pal.popup);
    }
}

#[test]
fn picking_on_the_square_puts_the_hex_in_the_rows_form_and_reports_it() {
    let (mut f, l) = opened(UPPER);
    let sq = l.square;
    assert_eq!(f.press(sq.x, sq.y, NONE, 1), text(UPPER, "FFFFFF"));
    assert_eq!(f.colour(UPPER), Some([255; 3]));
    // Form::pointer reports each change of the drag at once.
    assert_eq!(f.pointer(sq.right(), sq.y), text(UPPER, "FFAA00"));
    assert_eq!(f.pointer(sq.right(), sq.y), Outcome::None, "no move");
    assert_eq!(f.pointer(sq.right(), sq.bottom()), text(UPPER, "000000"));
    assert_eq!(
        f.release(sq.right(), sq.bottom()),
        Outcome::Redraw,
        "reported already"
    );
    // pointer_move moves it and leaves the report to the release.
    f.press(sq.x, sq.bottom(), NONE, 1);
    assert!(f.pointer_move(sq.x + sq.w / 4.0, sq.y));
    assert_eq!(f.value(UPPER), Some(Value::Text("FFEABF".into())));
    assert_eq!(f.release(sq.x + sq.w / 4.0, sq.y), text(UPPER, "FFEABF"));
    assert_eq!(f.colour_open(), Some(UPPER), "still open");
    // The hash form too.
    let (mut f, l) = opened(ACCENT);
    assert_eq!(
        f.press(l.square.right(), l.square.y, NONE, 1),
        Outcome::Changed {
            id: ACCENT,
            value: Value::Text("#0055ff".into())
        }
    );
}

#[test]
fn the_strip_changes_the_hue_and_the_markers_follow_on_screen() {
    let (mut f, l) = opened(ACCENT);
    let before = shot(&mut f, &DARK);
    let o = f.press(l.hue.x + l.hue.w / 3.0, l.hue.cy(), NONE, 1);
    assert!(matches!(o, Outcome::Changed { id: ACCENT, .. }));
    f.release(0.0, 0.0);
    let after = shot(&mut f, &DARK);
    assert!(before.pixels != after.pixels);
    let rgb = f.colour(ACCENT).unwrap();
    assert!(rgb[1] > rgb[0] && rgb[1] > rgb[2], "green: {rgb:?}");
}

#[test]
fn arrows_tab_and_enter_work_inside_the_picker() {
    let mut f = told();
    shot(&mut f, &LIGHT);
    assert!(f.focus_row(ACCENT));
    // Enter on the row opens it from the keyboard, with the square first.
    assert_eq!(f.key(&KeyName::Enter, NONE, None), Outcome::Redraw);
    assert_eq!(f.colour_open(), Some(ACCENT));
    let start = f.colour(ACCENT).unwrap();
    // Left takes saturation down: a change, reported with the text.
    let o = f.key(&KeyName::Left, SHIFT, None);
    assert!(matches!(o, Outcome::Changed { id: ACCENT, .. }), "{o:?}");
    assert_ne!(f.colour(ACCENT).unwrap(), start);
    // Tab stays inside the picker: the hue strip, the field, the square.
    for _ in 0..3 {
        assert_eq!(f.key(&KeyName::Tab, NONE, None), Outcome::Redraw);
        assert_eq!(f.focus_id(), Some(ACCENT));
        assert_eq!(f.colour_open(), Some(ACCENT));
    }
    // Up and Down are brightness, not the form's focus.
    f.key(&KeyName::Down, SHIFT, None);
    assert_eq!(f.focus_id(), Some(ACCENT));
    // Enter keeps it and closes; the next Enter opens it again.
    let kept = f.value(ACCENT);
    assert_eq!(f.key(&KeyName::Enter, NONE, None), Outcome::Redraw);
    assert_eq!(f.colour_open(), None);
    assert_eq!(f.value(ACCENT), kept);
    assert_eq!(f.key(&KeyName::F(4), NONE, None), Outcome::Redraw);
    assert_eq!(f.key(&KeyName::Escape, NONE, None), Outcome::Redraw);
    assert_eq!(f.key(&KeyName::Down, ALT, None), Outcome::Redraw);
    assert_eq!(f.colour_open(), Some(ACCENT));
    // Escape closes the picker, not the dialog; the next Escape the dialog.
    f.key(&KeyName::Escape, NONE, None);
    assert_eq!(f.key(&KeyName::Escape, NONE, None), Outcome::Closed);
    // Enter on a text row that is not a colour is what it was.
    f.focus_row(NAME);
    assert_eq!(f.key(&KeyName::Enter, NONE, None), Outcome::Unused);
}

#[test]
fn escape_and_a_press_outside_cancel_back_to_the_text_it_opened_with() {
    // Escape after a change: the old text, reported.
    let (mut f, l) = opened(UPPER);
    f.press(l.square.x, l.square.y, NONE, 1);
    f.release(l.square.x, l.square.y);
    assert_eq!(f.value(UPPER), Some(Value::Text("FFFFFF".into())));
    assert_eq!(f.key(&KeyName::Escape, NONE, None), text(UPPER, "CC8800"));
    assert_eq!(f.colour_open(), None);
    // A press outside the card, after a change.
    let (mut f, l) = opened(ACCENT);
    f.key(&KeyName::Right, NONE, None);
    f.press(l.hue.cx(), l.hue.cy(), NONE, 1);
    f.release(l.hue.cx(), l.hue.cy());
    let card = f.picker_rect();
    assert_eq!(
        f.press(card.right() + 30.0, card.y, NONE, 1),
        text(ACCENT, "#3366cc")
    );
    assert_eq!(f.colour_open(), None);
    assert_eq!(f.release(card.right() + 30.0, card.y), Outcome::None);
    // With nothing changed, a cancel reports nothing.
    let (mut f, _) = opened(ACCENT);
    assert_eq!(f.key(&KeyName::Escape, NONE, None), Outcome::Redraw);
    // A press on the swatch of an open picker cancels and does not open it again.
    let (mut f, l) = opened(ACCENT);
    f.press(l.square.x, l.square.y, NONE, 1);
    f.release(l.square.x, l.square.y);
    let s = f.swatch_rect(index(&f, ACCENT));
    let (a, b) = click(&mut f, s.cx(), s.cy());
    assert_eq!((a, b), (text(ACCENT, "#3366cc"), Outcome::None));
    assert_eq!(f.colour_open(), None);
    // A text that was not a colour comes back exactly.
    let (mut f, l) = opened(BAD);
    assert!(matches!(
        f.press(l.square.x, l.square.y, NONE, 1),
        Outcome::Changed { id: BAD, .. }
    ));
    f.release(l.square.x, l.square.y);
    assert_eq!(f.value(BAD), Some(Value::Text("ffffff".into())));
    assert_eq!(
        f.key(&KeyName::Escape, NONE, None),
        text(BAD, "not a colour")
    );
}

#[test]
fn the_hex_stays_in_step_both_ways() {
    // Typing in the picker's field moves the markers and the row.
    let (mut f, l) = opened(ACCENT);
    click(&mut f, l.hex.cx(), l.hex.cy());
    f.key(&KeyName::Char('a'), Mods { ctrl: true, ..NONE }, None);
    let before = shot(&mut f, &LIGHT);
    let mut last = Outcome::None;
    for ch in "00cc44".chars() {
        let s = ch.to_string();
        last = f.key(&KeyName::Char(ch), NONE, Some(&s));
    }
    assert_eq!(last, text(ACCENT, "#00cc44"), "in the row's own form");
    let after = shot(&mut f, &LIGHT);
    assert!(before.pixels != after.pixels, "the markers moved");
    // The new swatch in the card shows it.
    let l = f.picker_layout().unwrap();
    assert_eq!(pixel(&after, l.new.cx(), l.new.cy()), [0x00, 0xcc, 0x44]);
    assert_eq!(pixel(&after, l.old.cx(), l.old.cy()), [0x33, 0x66, 0xcc]);
    // The application sets the value while it is open: the picker follows.
    assert!(f.set_value(ACCENT, Value::Text("#ff0000".into())));
    let l = f.picker_layout().unwrap();
    let shown = shot(&mut f, &LIGHT);
    let got = pixel(&shown, l.square.right() - 1.0, l.square.y + 1.0);
    assert!(
        got[0] > 240 && got[1] < 12 && got[2] < 12,
        "red hue: {got:?}"
    );
    // That text counts as reported: closing reports nothing.
    assert_eq!(f.key(&KeyName::Enter, NONE, None), Outcome::Redraw);
}

#[test]
fn set_rows_closes_the_picker_and_the_wheel_waits_while_it_is_open() {
    let (mut f, _) = opened(ACCENT);
    let scroll = f.scroll();
    assert!(!f.wheel(300.0, 300.0, 200.0));
    assert_eq!(f.scroll(), scroll);
    f.set_rows(rows());
    assert_eq!(f.colour_open(), None);
    shot(&mut f, &DARK);
    assert!(
        f.swatch_rect(index(&f, ACCENT)).w > 0.0,
        "the mark outlives set_rows"
    );
    // A swatch pressed before set_rows is no longer pressed.
    let s = f.swatch_rect(index(&f, ACCENT));
    f.press(s.cx(), s.cy(), NONE, 1);
    f.set_rows(rows());
    assert_eq!(f.release(s.cx(), s.cy()), Outcome::None);
    assert_eq!(f.colour_open(), None);
}

#[test]
fn the_picker_opens_above_a_row_near_the_bottom_and_stays_in_a_small_window() {
    let mut f = told();
    shot_at(&mut f, &LIGHT, 700.0, 330.0, 1.0);
    let i = index(&f, BAD);
    let s = f.swatch_rect(i);
    click(&mut f, s.cx(), s.cy());
    shot_at(&mut f, &LIGHT, 700.0, 330.0, 1.0);
    let card = f.picker_rect();
    assert!(card.bottom() <= f.control_rect(i).y, "above: {card:?}");
    assert!(card.y >= 0.0);
    // A window smaller than the card: inside it, cut to it, and no panic.
    shot_at(&mut f, &LIGHT, 200.0, 150.0, 1.0);
    let card = f.picker_rect();
    assert!(card.x >= 0.0 && card.y >= 0.0 && card.right() <= 200.0 && card.bottom() <= 150.0);
}

#[test]
fn odd_windows_and_points_do_not_panic() {
    let (mut f, _) = opened(ACCENT);
    for (w, h) in [
        (1.0, 1.0),
        (0.0, 0.0),
        (f32::NAN, 400.0),
        (500.0, -20.0),
        (3000.0, 40.0),
    ] {
        let mut bmp = Bitmap::new(64, 64);
        let mut gfx = Gfx::new();
        let mut p = Painter::new(&mut bmp, &mut gfx, 1.5);
        f.draw(&mut p, &DARK, Rect::new(0.0, 0.0, w, h), true);
        for (x, y) in [
            (f32::NAN, 1.0),
            (1.0, f32::INFINITY),
            (-1e9, 1e9),
            (5.0, 5.0),
        ] {
            f.pointer(x, y);
            f.pointer_move(x, y);
            f.press(x, y, NONE, 2);
            f.release(x, y);
            if f.colour_open().is_none() {
                f.focus_row(ACCENT);
                f.key(&KeyName::Enter, NONE, None);
            }
        }
    }
}

#[test]
fn draw_animated_draws_the_picker_too() {
    use crate::anim::{Animator, Clock, ManualClock};
    let (mut f, _) = opened(ACCENT);
    let plain = shot(&mut f, &DARK);
    let clock = ManualClock::new();
    let mut anim = Animator::new();
    let mut bmp = Bitmap::new(W as u32, H as u32);
    for _ in 0..40 {
        clock.advance(std::time::Duration::from_millis(16));
        bmp.clear([128; 3]);
        anim.begin_frame(clock.now());
        let mut gfx = Gfx::new();
        let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
        f.draw_animated(
            &mut p,
            &DARK,
            Rect::new(0.0, 0.0, W, H),
            false,
            &mut anim,
            "look",
        );
        anim.end_frame();
    }
    assert!(bmp.pixels == plain.pixels, "at rest the same as draw");
}

/// Where the sheet and the timings go: `specimen/` under `CARGO_TARGET_DIR`, or the
/// workspace's `target`.
fn specimen_dir() -> std::path::PathBuf {
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target"));
    target.join("specimen")
}

/// A form as mdplay's Look dialog might hold it, 1100 x 700, the picker open on its accent.
fn big_open(scale: f32) -> (Form, Bitmap, Gfx) {
    let mut v = vec![Row::header("Look"), Row::text(ACCENT, "Accent", "#3366cc")];
    for k in 0..8 {
        v.push(Row::text(
            20 + k,
            format!("colour.part{k}"),
            format!("#{:02x}6699", k * 30),
        ));
    }
    v.push(Row::header("More"));
    for k in 0..8 {
        v.push(Row::toggle(40 + k, format!("Option {k}"), k % 2 == 0).help("With help"));
    }
    let mut f = Form::new("Look", v);
    f.show_as_colour(ACCENT);
    for k in 0..8 {
        f.show_as_colour(20 + k);
    }
    let bmp = Bitmap::new((1100.0 * scale) as u32, (700.0 * scale) as u32);
    let mut gfx = Gfx::new();
    let mut bmp2 = bmp;
    {
        let mut p = Painter::new(&mut bmp2, &mut gfx, scale);
        f.draw(&mut p, &DARK, Rect::new(0.0, 0.0, 1100.0, 700.0), true);
    }
    let s = f.swatch_rect(1);
    click(&mut f, s.cx(), s.cy());
    assert_eq!(f.colour_open(), Some(ACCENT));
    (f, bmp2, gfx)
}

/// Not a test: the time of a whole frame of a 1100 x 700 form with the picker open, light and
/// dark, at 1 and 1.5, the median of 50 draws, at rest and while the hue is dragged (a new
/// square each frame). Run in a release build:
/// `cargo test --release -p mdshared-ui colour_picker_draw_time -- --ignored --nocapture`.
#[test]
#[ignore]
fn colour_picker_draw_time() {
    for scale in [1.0f32, 1.5] {
        for (name, pal) in [("light", &LIGHT), ("dark", &DARK)] {
            for dragging in [false, true] {
                let (mut f, mut bmp, mut gfx) = big_open(scale);
                let l = f.picker_layout().unwrap();
                if dragging {
                    f.press(l.hue.x + 2.0, l.hue.cy(), NONE, 1);
                }
                let mut times = Vec::new();
                for k in 0..51 {
                    if dragging {
                        f.pointer(l.hue.x + 2.0 + (k % 40) as f32 * 4.0, l.hue.cy());
                    }
                    let t = std::time::Instant::now();
                    {
                        let mut p = Painter::new(&mut bmp, &mut gfx, scale);
                        f.draw(&mut p, pal, Rect::new(0.0, 0.0, 1100.0, 700.0), true);
                    }
                    if k > 0 {
                        times.push(t.elapsed());
                    }
                }
                times.sort();
                println!(
                    "1100 x 700 at {scale}, {name}, {}: median {:?} (fastest {:?}, slowest {:?}) of {}",
                    if dragging {
                        "dragging the hue"
                    } else {
                        "at rest"
                    },
                    times[times.len() / 2],
                    times[0],
                    times[times.len() - 1],
                    times.len()
                );
            }
        }
    }
}

/// Not a test: the picker open over the form, light and dark and high contrast, at 1 and 1.5,
/// with the keyboard's ring and without, for a person (and mdplay's shot runner) to look at.
/// `cargo test -p mdshared-ui colour_sheet -- --ignored`, then `specimen/colour_*.png` under
/// the target folder.
#[test]
#[ignore]
fn colour_sheet() {
    use crate::tokens::Tokens;
    let out = specimen_dir();
    let pals = [
        ("light", LIGHT),
        ("dark", DARK),
        ("hc_light", Tokens::high_contrast(false).palette),
        ("hc_dark", Tokens::high_contrast(true).palette),
    ];
    for (name, pal) in &pals {
        for scale in [1.0f32, 1.5] {
            for ring in [false, true] {
                let mut f = told();
                shot_at(&mut f, pal, W, H, scale);
                let i = index(&f, ACCENT);
                if ring {
                    f.focus_row(ACCENT);
                    f.key(&KeyName::Enter, NONE, None);
                    f.key(&KeyName::Left, SHIFT, None);
                    f.key(&KeyName::Tab, NONE, None);
                } else {
                    let s = f.swatch_rect(i);
                    click(&mut f, s.cx(), s.cy());
                    let l = f.picker_layout().unwrap();
                    f.press(
                        l.square.x + l.square.w * 0.7,
                        l.square.y + l.square.h * 0.3,
                        NONE,
                        1,
                    );
                    f.release(0.0, 0.0);
                }
                let bmp = shot_at(&mut f, pal, W, H, scale);
                let file = format!(
                    "colour_{name}_{}{}.png",
                    if scale == 1.0 { "100" } else { "150" },
                    if ring { "_keys" } else { "" }
                );
                bmp.write_png(&out.join(file)).unwrap();
            }
        }
    }
}

/// An open dropdown takes a press on a swatch as it does any press: the menu closes (or picks
/// the item it shows there), and no picker opens.
#[test]
fn an_open_dropdown_takes_a_press_on_a_swatch() {
    let mut v = rows();
    v.insert(1, Row::choice(99, "Theme", &["System", "Light", "Dark"], 0));
    let mut f = Form::new("Look", v).single_page();
    assert!(f.show_as_colour(ACCENT));
    shot(&mut f, &LIGHT);
    f.open_menu(index(&f, 99));
    assert!(f.menu.is_some());
    let s = f.swatch_rect(index(&f, ACCENT));
    click(&mut f, s.cx(), s.cy());
    assert_eq!(f.colour_open(), None, "no picker opened under the menu");
    assert!(f.menu.is_none(), "the press closed the menu");
    // With the menu closed, the swatch opens its picker as before.
    click(&mut f, s.cx(), s.cy());
    assert_eq!(f.colour_open(), Some(ACCENT));
}

/// The pixels of `r` in `f` drawn with the caret's blink phase `caret_on`.
fn area_with_caret(f: &mut Form, r: Rect, caret_on: bool) -> Vec<u8> {
    let mut bmp = Bitmap::new(W as u32, H as u32);
    bmp.clear([128; 3]);
    let mut gfx = Gfx::new();
    {
        let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
        f.draw(&mut p, &LIGHT, Rect::new(0.0, 0.0, W, H), caret_on);
    }
    let mut out = Vec::new();
    for y in r.y as usize..r.bottom() as usize {
        for x in r.x as usize..r.right() as usize {
            let o = (y * bmp.width as usize + x) * 4;
            out.extend_from_slice(&bmp.pixels[o..o + 3]);
        }
    }
    out
}

/// While a row's picker is open the keys go to the picker, so the row's own field shows no
/// caret; with the picker closed the focused field blinks again.
#[test]
fn the_rows_field_shows_no_caret_while_its_picker_is_open() {
    let (mut f, _) = opened(ACCENT);
    let r = f.control_rect(index(&f, ACCENT));
    assert_eq!(
        area_with_caret(&mut f, r, true),
        area_with_caret(&mut f, r, false),
        "no caret in the row while the picker holds the keys"
    );
    f.key(&KeyName::Escape, NONE, None);
    assert_eq!(f.colour_open(), None);
    assert_ne!(
        area_with_caret(&mut f, r, true),
        area_with_caret(&mut f, r, false),
        "the focused field blinks again once the picker is closed"
    );
}

/// The left-right arrow over a draggable number is not asked for while a picker is open (its
/// card may cover the row), as while a dropdown is; it comes back when the picker closes.
#[test]
fn no_drag_cursor_while_a_picker_is_open() {
    use crate::cursor::Cursor;
    let mut v = rows();
    v.insert(1, Row::number(98, "Corners", 8, 0, 24));
    let mut f = Form::new("Look", v).single_page();
    assert!(f.show_as_colour(ACCENT));
    assert!(f.make_draggable(98));
    shot(&mut f, &LIGHT);
    let c = f.control_rect(index(&f, 98));
    let at = (c.x as i32..c.right() as i32)
        .map(|x| (x as f32, c.cy()))
        .find(|&(x, y)| f.cursor_at(x, y) == Some(Cursor::ResizeEw))
        .expect("a point over the draggable value");
    let s = f.swatch_rect(index(&f, ACCENT));
    click(&mut f, s.cx(), s.cy());
    assert_eq!(f.colour_open(), Some(ACCENT));
    assert_eq!(f.cursor_at(at.0, at.1), None);
    f.key(&KeyName::Escape, NONE, None);
    assert_eq!(f.colour_open(), None);
    assert_eq!(f.cursor_at(at.0, at.1), Some(Cursor::ResizeEw));
}
