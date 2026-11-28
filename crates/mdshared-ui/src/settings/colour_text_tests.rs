//! An open colour picker's hex field on the pointer route (`pointer_press`, `pointer_motion`,
//! `pointer_release`, `context_menu`, `apply_context_action`): the clicks counted by the form, a
//! press in the field's padding, a drag past its ends, the I-beam, the edit menu (from an Apply
//! or Cancel button too), and a lost press. What the field then holds is read back by typing a
//! colour over the selection: only a selection of the whole hex gives exactly the colour typed.
//! No test touches the system clipboard, so Cut, Copy and Paste are left to the field's own
//! tests; Select all goes through the same path.

use super::{Form, Outcome, Row, Value};
use crate::colour_picker::Layout;
use crate::cursor::Cursor;
use crate::gfx::{Gfx, Painter, Rect};
use crate::keys::{KeyName, Mods};
use crate::textfield::ContextAction;
use crate::theme::LIGHT;
use mdshared_text::Bitmap;
use std::time::{Duration, Instant};

const NONE: Mods = Mods {
    ctrl: false,
    shift: false,
    alt: false,
    control: false,
};
const SHIFT: Mods = Mods {
    shift: true,
    ..NONE
};
const ACCENT: u32 = 1;

fn form(confirm: bool) -> Form {
    let mut f = Form::new(
        "Look",
        vec![
            Row::header("Look"),
            Row::text(ACCENT, "Accent", "#3366cc"),
            Row::text(2, "Name", "abc"),
        ],
    )
    .single_page();
    if confirm {
        assert!(f.show_as_colour_with_confirmation(ACCENT));
    } else {
        assert!(f.show_as_colour(ACCENT));
    }
    f
}

fn draw(f: &mut Form) {
    let mut bmp = Bitmap::new(760, 620);
    let mut gfx = Gfx::new();
    let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
    f.draw(&mut p, &LIGHT, Rect::new(0.0, 0.0, 760.0, 620.0), false);
}

/// A drawn form with the accent row's picker open from its swatch, and the picker's layout.
fn opened(confirm: bool) -> (Form, Layout) {
    let mut f = form(confirm);
    draw(&mut f);
    let s = f.swatch_rect(1);
    f.press(s.cx(), s.cy(), NONE, 1);
    f.release(s.cx(), s.cy());
    assert_eq!(f.colour_open(), Some(ACCENT));
    draw(&mut f);
    let l = f.picker_layout().unwrap();
    (f, l)
}

/// Types `hex` and returns the last outcome.
fn type_hex(f: &mut Form, hex: &str) -> Outcome {
    let mut last = Outcome::None;
    for ch in hex.chars() {
        last = f.key(&KeyName::Char(ch), NONE, Some(&ch.to_string()));
    }
    last
}

fn changed(t: &str) -> Outcome {
    Outcome::Changed {
        id: ACCENT,
        value: Value::Text(t.into()),
    }
}

#[test]
fn a_triple_click_in_the_hex_field_selects_the_whole_hex() {
    let (mut f, l) = opened(false);
    let mut gfx = Gfx::new();
    let at = (l.hex.cx(), l.hex.cy());
    let now = Instant::now();
    for _ in 0..3 {
        assert_eq!(
            f.pointer_press(at, NONE, 1, now, &mut gfx, 1.0),
            Outcome::Redraw
        );
        assert_eq!(f.pointer_release(at, now, &mut gfx), Outcome::None);
    }
    assert_eq!(f.colour_open(), Some(ACCENT), "the picker stays open");
    assert_eq!(type_hex(&mut f, "00cc44"), changed("#00cc44"));
}

#[test]
fn a_press_in_the_padding_and_a_drag_past_the_far_end_select_the_whole_hex() {
    let (mut f, l) = opened(false);
    let mut gfx = Gfx::new();
    let now = Instant::now();
    // The right padding: outside the text, inside the field's box.
    let right = (l.hex.right() - 2.0, l.hex.cy());
    assert_eq!(
        f.pointer_press(right, NONE, 1, now, &mut gfx, 1.0),
        Outcome::Redraw
    );
    assert_eq!(
        f.pointer_cursor_at(0.0, 0.0),
        Some(Cursor::Text),
        "while held"
    );
    let far = (l.hex.x - 80.0, l.hex.cy());
    assert_eq!(f.pointer_motion(far, NONE, now, &mut gfx), Outcome::Redraw);
    f.pointer_release(far, now, &mut gfx);
    assert_eq!(type_hex(&mut f, "00cc44"), changed("#00cc44"));
}

#[test]
fn shift_and_a_press_in_the_other_padding_extend_from_the_caret() {
    let (mut f, l) = opened(false);
    let mut gfx = Gfx::new();
    let now = Instant::now();
    let right = (l.hex.right() - 2.0, l.hex.cy());
    let left = (l.hex.x + 2.0, l.hex.cy());
    f.pointer_press(right, NONE, 1, now, &mut gfx, 1.0);
    f.pointer_release(right, now, &mut gfx);
    let later = now + Duration::from_secs(5);
    f.pointer_press(left, SHIFT, 1, later, &mut gfx, 1.0);
    f.pointer_release(left, later, &mut gfx);
    assert_eq!(type_hex(&mut f, "00cc44"), changed("#00cc44"));
}

#[test]
fn the_hex_field_asks_for_the_i_beam_and_nothing_else_in_the_picker_does() {
    let (f, l) = opened(false);
    assert_eq!(
        f.pointer_cursor_at(l.hex.cx(), l.hex.cy()),
        Some(Cursor::Text)
    );
    assert_eq!(
        f.pointer_cursor_at(l.hex.x + 2.0, l.hex.cy()),
        Some(Cursor::Text)
    );
    assert_eq!(f.pointer_cursor_at(l.square.cx(), l.square.cy()), None);
    assert_eq!(f.pointer_cursor_at(l.hue.cx(), l.hue.cy()), None);
}

#[test]
fn the_edit_menu_opens_on_the_hex_field_and_select_all_selects_the_hex() {
    let (mut f, l) = opened(false);
    assert!(
        f.context_menu(Some((l.square.cx(), l.square.cy())))
            .is_none()
    );
    let items = f.context_menu(Some((l.hex.cx(), l.hex.cy()))).unwrap();
    let ids: Vec<_> = items.iter().map(|i| i.id).collect();
    assert_eq!(
        ids,
        [
            ContextAction::Cut,
            ContextAction::Copy,
            ContextAction::Paste,
            ContextAction::SelectAll
        ]
    );
    // The field now has the keyboard: the menu also opens for it from the keyboard.
    assert!(f.context_menu(None).is_some());
    assert_eq!(
        f.apply_context_action(ContextAction::SelectAll),
        Outcome::Redraw
    );
    // Cut and Copy with the selection made are now offered (Paste follows the clipboard).
    let items = f.context_menu(None).unwrap();
    assert!(items[0].enabled && items[1].enabled && items[3].enabled);
    assert_eq!(type_hex(&mut f, "00cc44"), changed("#00cc44"));
}

#[test]
fn with_the_square_focused_the_keyboard_menu_is_not_the_hex_fields() {
    let (mut f, l) = opened(false);
    let mut gfx = Gfx::new();
    let sq = (l.square.cx(), l.square.cy());
    f.pointer_press(sq, NONE, 1, Instant::now(), &mut gfx, 1.0);
    f.pointer_release(sq, Instant::now(), &mut gfx);
    assert!(f.context_menu(None).is_none());
    assert_eq!(
        f.apply_context_action(ContextAction::SelectAll),
        Outcome::None
    );
}

#[test]
fn a_confirming_picker_gives_the_hex_field_the_keyboard_from_its_buttons_and_stages_edits() {
    let (mut f, _) = opened(true);
    let l = f.picker_confirmation_layout().unwrap();
    let hex = l.picker.hex;
    // Tab from the square to the hex field, then to Apply.
    for _ in 0..3 {
        f.key(&KeyName::Tab, NONE, None);
    }
    assert!(f.context_menu(None).is_none(), "Apply has the keyboard");
    assert!(f.context_menu(Some((hex.cx(), hex.cy()))).is_some());
    assert_eq!(
        f.apply_context_action(ContextAction::SelectAll),
        Outcome::Redraw
    );
    assert_eq!(type_hex(&mut f, "00cc44"), Outcome::Redraw, "staged");
    assert_eq!(f.key(&KeyName::Enter, NONE, None), changed("#00cc44"));
}

#[test]
fn a_lost_press_ends_the_hex_drag_and_the_old_release_ends_it_too() {
    let (mut f, l) = opened(false);
    let mut gfx = Gfx::new();
    let now = Instant::now();
    let right = (l.hex.right() - 2.0, l.hex.cy());
    let far = (l.hex.x - 80.0, l.hex.cy());
    f.pointer_press(right, NONE, 1, now, &mut gfx, 1.0);
    assert_eq!(f.cancel(), Outcome::Redraw);
    assert_eq!(f.cancel(), Outcome::None, "nothing held now");
    assert_eq!(f.pointer_motion(far, NONE, now, &mut gfx), Outcome::None);
    assert_eq!(f.text_scroll_deadline(), None);
    // The drag did not go on to select the hex: a colour typed goes after it, no colour.
    assert_eq!(type_hex(&mut f, "00cc44"), Outcome::Redraw);

    let (mut f, l) = opened(false);
    let right = (l.hex.right() - 2.0, l.hex.cy());
    let far = (l.hex.x - 80.0, l.hex.cy());
    f.pointer_press(right, NONE, 1, now, &mut gfx, 1.0);
    f.release(right.0, right.1);
    assert_eq!(f.pointer_motion(far, NONE, now, &mut gfx), Outcome::None);
    assert_eq!(f.text_scroll_deadline(), None);
}

#[test]
fn a_press_outside_the_card_on_the_pointer_route_still_cancels_the_picker() {
    let (mut f, l) = opened(false);
    let mut gfx = Gfx::new();
    let at = (l.hex.cx(), l.hex.cy());
    let now = Instant::now();
    for _ in 0..3 {
        f.pointer_press(at, NONE, 1, now, &mut gfx, 1.0);
        f.pointer_release(at, now, &mut gfx);
    }
    assert_eq!(type_hex(&mut f, "00cc44"), changed("#00cc44"));
    let out = f.pointer_press((5.0, 5.0), NONE, 1, Instant::now(), &mut gfx, 1.0);
    assert_eq!(out, changed("#3366cc"));
    assert_eq!(f.colour_open(), None);
}
