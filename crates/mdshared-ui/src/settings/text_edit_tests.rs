//! Settings-specific routing and pixels. The field tests cover clipboard I/O with a fake.

use super::*;
use crate::{
    anim::{Clock, ManualClock},
    gfx::Painter,
    keys::KeyName,
    settings::pages,
    textfield::ContextAction::{Copy, Cut, Paste, SelectAll},
    theme::LIGHT,
};
use mdshared_text::Bitmap;
use std::time::Duration;

const MODS: Mods = Mods {
    ctrl: false,
    shift: false,
    alt: false,
    control: false,
};
const WINDOW: Rect = Rect::new(0.0, 0.0, 800.0, 480.0);

fn enabled(menu: &[Item<ContextAction>], action: ContextAction) -> bool {
    menu.iter().find(|item| item.id == action).unwrap().enabled
}

#[test]
fn a_drag_selects_text_and_draws_the_same_pixels_as_keyboard_selection() {
    let mut form = Form::new("Settings", vec![Row::text(1, "Name", "one two")]);
    let mut gfx = Gfx::new();
    let mut bmp = Bitmap::new(800, 480);
    let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
    p.fill(WINDOW, LIGHT.page);
    form.draw(&mut p, &LIGHT, WINDOW, false);
    let before = p.bmp.pixels.clone();
    let area = form.text_area(0);
    let start = (area.x, area.cy());
    let end = (area.right() - 1.0, area.cy());
    let now = ManualClock::new().now();

    let menu = form.context_menu(Some(start)).unwrap();
    assert!(!enabled(&menu, Cut));
    assert!(!enabled(&menu, Copy));
    assert!(enabled(&menu, SelectAll));
    assert_eq!(form.pointer_cursor_at(start.0, start.1), Some(Cursor::Text));
    assert_eq!(
        form.pointer_press(start, MODS, 1, now, p.gfx, 1.0),
        Outcome::Redraw
    );
    assert_eq!(form.pointer_motion(end, MODS, now, p.gfx), Outcome::Redraw);
    assert_eq!(form.pointer_release(end, now, p.gfx), Outcome::None);
    assert_eq!(form.fields[0].selection_for_export(), Some("one two"));
    form.pointer_motion(start, MODS, now, p.gfx);
    assert_eq!(form.fields[0].selection_for_export(), Some("one two"));
    assert_eq!(form.value(1), Some(Value::Text("one two".into())));
    let menu = form.context_menu(None).unwrap();
    assert!(enabled(&menu, Cut));
    assert!(enabled(&menu, Copy));

    p.fill(WINDOW, LIGHT.page);
    form.draw(&mut p, &LIGHT, WINDOW, false);
    let dragged = p.bmp.pixels.clone();
    // Inside the selection band, well away from the field's edge and focus underline.
    let pixel = ((area.cy() - 7.0) as usize * 800 + area.x as usize + 4) * 4;
    assert_ne!(dragged[pixel..pixel + 4], before[pixel..pixel + 4]);
    // Select the same range from the keyboard. Focus was already on this field.
    form.key(&KeyName::End, MODS, None);
    form.key(
        &KeyName::Home,
        Mods {
            shift: true,
            ..MODS
        },
        None,
    );
    p.fill(WINDOW, LIGHT.page);
    form.draw(&mut p, &LIGHT, WINDOW, false);
    assert_eq!(p.bmp.pixels, dragged);
}

#[test]
fn a_paged_secret_row_can_be_selected_and_pasted_into_but_its_menu_cannot_export() {
    let _clipboard = crate::textfield::paste_fixture("new token");
    let mut form = pages::Form::new(
        "Settings",
        vec![pages::Page::new(
            "General",
            vec![
                Row::text(1, "Token", "secret").secret().into(),
                pages::Item::new(Row::text(2, "Disabled", "other")).disabled("Not available"),
            ],
        )],
    );
    let mut gfx = Gfx::new();
    let mut bmp = Bitmap::new(800, 480);
    form.layout(&mut Painter::new(&mut bmp, &mut gfx, 1.0), WINDOW);
    assert!(form.context_menu(None).is_none());
    let ctrl = form.control_rect(1);
    let at = (ctrl.cx(), ctrl.cy());
    assert_eq!(form.pointer_cursor_at(at.0, at.1), Some(Cursor::Text));
    assert!(form.context_menu(Some(at)).is_some());
    assert_eq!(form.focus_id(), Some(1));
    assert_eq!(form.apply_context_action(SelectAll), Outcome::Redraw);
    let menu = form.context_menu(None).unwrap();
    for action in [Cut, Copy] {
        assert!(!enabled(&menu, action));
        assert_eq!(form.apply_context_action(action), Outcome::None);
    }
    assert!(enabled(&menu, SelectAll));
    assert_eq!(form.value(1), Some(Value::Text("secret".into())));
    assert!(enabled(&menu, Paste));
    assert_eq!(
        form.apply_context_action(Paste),
        Outcome::Changed {
            id: 1,
            value: Value::Text("new token".into()),
        }
    );
    assert_eq!(form.value(1), Some(Value::Text("new token".into())));
    let ctrl = form.control_rect(2);
    assert_eq!(form.pointer_cursor_at(ctrl.cx(), ctrl.cy()), None);
    assert!(form.context_menu(Some((ctrl.cx(), ctrl.cy()))).is_none());
}

#[test]
fn an_open_dropdown_takes_input_before_the_text_under_it_in_both_forms() {
    fn rows() -> [Row; 2] {
        [
            Row::choice(
                1,
                "Mode",
                &["first", "second", "third", "fourth", "fifth"],
                0,
            ),
            Row::text(2, "Name", "one two"),
        ]
    }
    let mut single = Form::new("Settings", rows().into());
    let [choice, text] = rows();
    let mut paged = pages::Form::new(
        "Settings",
        vec![pages::Page::new(
            "General",
            vec![pages::Item::new(choice).dropdown(), text.into()],
        )],
    );
    let mut gfx = Gfx::new();
    let mut bmp = Bitmap::new(800, 480);
    let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
    let now = ManualClock::new().now();
    single.draw(&mut p, &LIGHT, WINDOW, false);
    paged.draw(&mut p, &LIGHT, WINDOW, false);

    macro_rules! check {
        ($form:ident, $ctrl:expr, $menu:expr) => {{
            let ctrl = $ctrl;
            // Near the aligned right edge, where the dropdown covers the field.
            let at = (ctrl.right() - 20.0, ctrl.cy());
            assert_eq!($form.pointer_cursor_at(at.0, at.1), Some(Cursor::Text));
            assert!($form.focus_row(1));
            assert_eq!($form.key(&KeyName::Enter, MODS, None), Outcome::Redraw);
            assert!($form.popup_open());
            assert_eq!($form.pointer_cursor_at(at.0, at.1), None);
            $form.draw(&mut p, &LIGHT, WINDOW, false);
            assert!($menu.contains(at.0, at.1));
            assert!($form.context_menu(Some(at)).is_none());
            assert_eq!(
                $form.pointer_press(at, MODS, 1, now, p.gfx, 1.0),
                Outcome::Redraw
            );
            assert_eq!($form.focus_id(), Some(1));
            $form.pointer_release(at, now, p.gfx);
            assert!(!$form.popup_open());
            assert_eq!($form.pointer_cursor_at(at.0, at.1), Some(Cursor::Text));
        }};
    }
    check!(
        single,
        single.control_rect(1),
        single.menu.as_ref().unwrap().1.rect()
    );
    check!(paged, paged.control_rect(2), paged.menu_rect().unwrap());
}

#[test]
fn an_edge_drag_scrolls_at_its_deadline_and_stops_on_release_or_context_menu() {
    let mut form = pages::Form::new(
        "Settings",
        vec![pages::Page::new(
            "General",
            vec![Row::text(1, "Name", "one two three ".repeat(25)).into()],
        )],
    );
    let mut gfx = Gfx::new();
    let mut bmp = Bitmap::new(800, 480);
    let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
    assert!(form.focus_row(1));
    form.key(&KeyName::Home, MODS, None);
    form.draw(&mut p, &LIGHT, WINDOW, false);
    let ctrl = form.control_rect(1);
    let start = (ctrl.x + 11.0, ctrl.cy());
    let end = (ctrl.right() + 10.0, ctrl.cy());
    let clock = ManualClock::new();
    let now = clock.now();
    form.pointer_press(start, MODS, 1, now, p.gfx, 1.0);
    assert_eq!(form.pointer_motion(end, MODS, now, p.gfx), Outcome::Redraw);
    assert!(enabled(&form.context_menu(None).unwrap(), Cut));
    assert_eq!(form.text_scroll_deadline(), None); // Opening the menu cancels capture.
    form.pointer_press(start, MODS, 1, now, p.gfx, 1.0);
    form.pointer_motion(end, MODS, now, p.gfx);
    let wake = form.text_scroll_deadline().unwrap();
    assert!(!form.tick_text_scroll(now, p.gfx));
    assert!(form.tick_text_scroll(wake, p.gfx));
    form.pointer_release(end, wake, p.gfx);
    assert_eq!(form.text_scroll_deadline(), None);
    assert!(!form.tick_text_scroll(wake + Duration::from_secs(1), p.gfx));
}

#[test]
fn consecutive_clicks_select_a_word_then_the_line_and_shift_uses_the_anchor() {
    let mut form = Form::new("Settings", vec![Row::text(1, "Name", "one two")]);
    let mut gfx = Gfx::new();
    let mut bmp = Bitmap::new(800, 480);
    form.layout(&mut Painter::new(&mut bmp, &mut gfx, 1.0), WINDOW);
    let area = form.text_area(0);
    let start = (area.x + 1.0, area.cy());
    let end = (area.right() - 1.0, area.cy());
    let now = ManualClock::new().now();
    for selected in [None, Some("one"), Some("one two")] {
        form.pointer_press(start, MODS, 1, now, &mut gfx, 1.0);
        form.pointer_release(start, now, &mut gfx);
        assert_eq!(form.fields[0].selection_for_export(), selected);
    }
    form.cancel_text_pointer();
    form.pointer_press(start, MODS, 1, now, &mut gfx, 1.0);
    form.pointer_release(start, now, &mut gfx);
    form.pointer_press(
        end,
        Mods {
            shift: true,
            ..MODS
        },
        1,
        now,
        &mut gfx,
        1.0,
    );
    form.pointer_release(end, now, &mut gfx);
    assert_eq!(form.fields[0].selection_for_export(), Some("one two"));
}
