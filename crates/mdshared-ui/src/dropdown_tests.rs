//! Tests of the standalone drop-down: the model's options, keys and clicks, where its menu is
//! placed, and its field's drawing against the settings dialog's own choice rows, pixel for
//! pixel.
//!
//! The sheet: `cargo test -p mdshared-ui dropdown_sheet -- --ignored`, then open
//! target/specimen/dropdown.png and dropdown-150.png.

use super::*;
use crate::anim::{Clock, ManualClock};
use crate::gfx::Gfx;
use crate::settings::pages::{self, Item as PageItem, Page};
use crate::settings::{Form, Row};
use crate::theme::{DARK, LIGHT};
use mdshared_text::Bitmap;

const FIELD: Rect = Rect {
    x: 40.0,
    y: 30.0,
    w: 160.0,
    h: DROPDOWN_H,
};
const AREA: Rect = Rect {
    x: 0.0,
    y: 0.0,
    w: 500.0,
    h: 400.0,
};
const NONE: Mods = Mods {
    ctrl: false,
    shift: false,
    alt: false,
    control: false,
};
const ALT: Mods = Mods {
    ctrl: false,
    shift: false,
    alt: true,
    control: false,
};
const CTRL: Mods = Mods {
    ctrl: true,
    shift: false,
    alt: false,
    control: false,
};

fn widths() -> Dropdown<&'static str> {
    Dropdown::new(
        [
            ("narrow", "Narrow"),
            ("medium", "Medium"),
            ("wide", "Wide"),
            ("full", "Full"),
            ("custom", "Custom"),
        ],
        1,
    )
}

/// Places the open menu in `area`, as drawing it does, so that its rows can be hit.
fn place<T>(d: &mut Dropdown<T>, area: Rect) {
    let mut bmp = Bitmap::new(area.right() as u32, area.bottom() as u32);
    let mut gfx = Gfx::new();
    let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
    draw_dropdown_menu(&mut p, &LIGHT, d, area);
}

fn key<T>(d: &mut Dropdown<T>, k: KeyName, m: Mods) -> Outcome {
    let out = d.key(FIELD, &k, m);
    place(d, AREA);
    out
}

// ---- the model ----------------------------------------------------------------------------

#[test]
fn a_new_dropdown_keeps_its_options_and_a_choice_inside_them() {
    let d = widths();
    assert_eq!(d.len(), 5);
    assert!(!d.is_empty() && !d.is_open() && !d.pressed());
    assert_eq!(d.selected(), Some(1));
    assert_eq!(d.selected_value(), Some(&"medium"));
    assert_eq!(d.selected_label(), "Medium");
    assert_eq!(d.value(4), Some(&"custom"));
    assert_eq!(d.label(2), Some("Wide"));
    assert_eq!((d.value(5), d.label(5)), (None, None));
    assert_eq!(d.options()[3], ("full", "Full".to_string()));
    let past = Dropdown::new([(1u8, "a"), (2, "b")], 99);
    assert_eq!(past.selected(), Some(1), "past the last chooses the last");
    let none: Dropdown<u8> = Dropdown::new(Vec::<(u8, String)>::new(), 3);
    assert!(none.is_empty());
    assert_eq!(none.selected(), None);
    assert_eq!(none.selected_value(), None);
    assert_eq!(none.selected_label(), "");
}

#[test]
fn select_says_whether_the_choice_changed_and_an_open_menu_follows_it() {
    let mut d = widths();
    assert!(!d.select(1));
    assert!(!d.select(5));
    assert!(d.select(3));
    assert_eq!(d.selected(), Some(3));
    d.open_menu(FIELD).unwrap();
    assert!(d.select(0));
    let menu = d.menu().unwrap();
    assert_eq!(menu.hover, Some(0));
    let ticked: Vec<bool> = menu.items.iter().map(|i| i.checked).collect();
    assert_eq!(ticked, [true, false, false, false, false]);
}

#[test]
fn an_empty_dropdown_or_a_nonsense_field_does_not_open() {
    let mut none: Dropdown<u8> = Dropdown::new(Vec::<(u8, &str)>::new(), 0);
    assert!(none.open_menu(FIELD).is_none());
    assert_eq!(none.key(FIELD, &KeyName::Space, NONE), Outcome::Unused);
    assert_eq!(none.key(FIELD, &KeyName::Down, NONE), Outcome::Unused);
    assert_eq!(none.press(FIELD, FIELD.cx(), FIELD.cy()), Outcome::Unused);
    assert!(!none.is_open());
    let mut d = widths();
    for r in [
        Rect::new(f32::NAN, 0.0, 100.0, 28.0),
        Rect::new(0.0, 0.0, f32::INFINITY, 28.0),
        Rect::new(0.0, 0.0, 0.0, 28.0),
    ] {
        assert!(d.open_menu(r).is_none(), "{r:?}");
        assert_eq!(d.key(r, &KeyName::Enter, NONE), Outcome::None);
        assert!(!d.hit(r, 0.0, 0.0));
        assert_eq!(d.press(r, 0.0, 0.0), Outcome::Unused);
    }
    assert!(!d.is_open());
    assert!(!d.hit(FIELD, f32::NAN, FIELD.cy()));
}

#[test]
fn closed_up_and_down_change_the_choice_and_stop_at_the_ends_without_opening() {
    let mut d = widths();
    assert_eq!(key(&mut d, KeyName::Down, NONE), Outcome::Changed(2));
    assert_eq!(key(&mut d, KeyName::Right, NONE), Outcome::Changed(3));
    assert_eq!(key(&mut d, KeyName::Up, NONE), Outcome::Changed(2));
    assert_eq!(key(&mut d, KeyName::Left, NONE), Outcome::Changed(1));
    assert_eq!(key(&mut d, KeyName::End, NONE), Outcome::Changed(4));
    assert_eq!(
        key(&mut d, KeyName::Down, NONE),
        Outcome::None,
        "at the end"
    );
    assert_eq!(key(&mut d, KeyName::End, NONE), Outcome::None);
    assert_eq!(key(&mut d, KeyName::Home, NONE), Outcome::Changed(0));
    assert_eq!(
        key(&mut d, KeyName::Up, NONE),
        Outcome::None,
        "at the start"
    );
    assert!(!d.is_open());
    assert_eq!(d.selected_value(), Some(&"narrow"));
}

#[test]
fn a_letter_chooses_the_next_option_that_begins_with_it_going_round() {
    let mut d = Dropdown::new(
        [(0, "Apple"), (1, "banana"), (2, "Avocado"), (3, "Cherry")],
        0,
    );
    assert_eq!(key(&mut d, KeyName::Char('a'), NONE), Outcome::Changed(2));
    assert_eq!(key(&mut d, KeyName::Char('a'), NONE), Outcome::Changed(0));
    assert_eq!(key(&mut d, KeyName::Char('B'), NONE), Outcome::Changed(1));
    assert_eq!(key(&mut d, KeyName::Char('z'), NONE), Outcome::None);
    assert_eq!(key(&mut d, KeyName::Char('c'), CTRL), Outcome::Unused);
    // Open, the letter moves the highlight and Enter takes it.
    assert_eq!(key(&mut d, KeyName::Space, NONE), Outcome::Opened);
    assert_eq!(key(&mut d, KeyName::Char('c'), NONE), Outcome::Redraw);
    assert_eq!(d.menu().unwrap().hover, Some(3));
    assert_eq!(key(&mut d, KeyName::Char('c'), NONE), Outcome::None);
    assert_eq!(key(&mut d, KeyName::Enter, NONE), Outcome::Changed(3));
}

#[test]
fn space_enter_alt_down_alt_up_and_f4_open_it_and_other_keys_are_not_its_own() {
    for (k, m) in [
        (KeyName::Space, NONE),
        (KeyName::Enter, NONE),
        (KeyName::Down, ALT),
        (KeyName::Up, ALT),
        (KeyName::F(4), NONE),
    ] {
        let mut d = widths();
        assert_eq!(key(&mut d, k.clone(), m), Outcome::Opened, "{k:?} {m:?}");
        assert!(d.is_open());
        assert_eq!(d.selected(), Some(1), "opening changes nothing");
        assert_eq!(
            d.menu().unwrap().hover,
            Some(1),
            "the choice is highlighted"
        );
    }
    let mut d = widths();
    for (k, m) in [
        (KeyName::Tab, NONE),
        (KeyName::Escape, NONE),
        (KeyName::Down, CTRL),
        (KeyName::Enter, CTRL),
        (KeyName::PageDown, NONE),
        (KeyName::F(5), NONE),
    ] {
        assert_eq!(key(&mut d, k.clone(), m), Outcome::Unused, "{k:?} {m:?}");
    }
    assert!(!d.is_open() && d.selected() == Some(1));
}

#[test]
fn open_the_arrows_move_the_highlight_enter_takes_it_and_escape_changes_nothing() {
    let mut d = widths();
    key(&mut d, KeyName::Enter, NONE);
    assert_eq!(key(&mut d, KeyName::Down, NONE), Outcome::Redraw);
    assert_eq!(key(&mut d, KeyName::Down, NONE), Outcome::Redraw);
    assert_eq!(d.menu().unwrap().hover, Some(3));
    assert_eq!(
        d.selected(),
        Some(1),
        "moving the highlight is not a choice"
    );
    assert_eq!(key(&mut d, KeyName::Escape, NONE), Outcome::Closed);
    assert!(!d.is_open());
    assert_eq!(d.selected(), Some(1));

    // Enter takes the highlight; Space and Alt+Up too; Tab closes as Escape does.
    for (k, m) in [
        (KeyName::Enter, NONE),
        (KeyName::Space, NONE),
        (KeyName::Up, ALT),
        (KeyName::Down, ALT),
        (KeyName::F(4), NONE),
    ] {
        let mut d = widths();
        key(&mut d, KeyName::Space, NONE);
        key(&mut d, KeyName::Up, NONE);
        assert_eq!(key(&mut d, k.clone(), m), Outcome::Changed(0), "{k:?}");
        assert!(!d.is_open());
        assert_eq!(d.selected_value(), Some(&"narrow"));
    }
    let mut d = widths();
    key(&mut d, KeyName::Space, NONE);
    key(&mut d, KeyName::Down, NONE);
    assert_eq!(key(&mut d, KeyName::Tab, NONE), Outcome::Closed);
    assert_eq!(d.selected(), Some(1));
    // Taking the option already chosen closes with no change.
    key(&mut d, KeyName::Space, NONE);
    assert_eq!(key(&mut d, KeyName::Enter, NONE), Outcome::Closed);
    // Home and End go to the ends; an open menu swallows keys it does not use.
    key(&mut d, KeyName::Space, NONE);
    assert_eq!(key(&mut d, KeyName::End, NONE), Outcome::Redraw);
    assert_eq!(d.menu().unwrap().hover, Some(4));
    assert_eq!(key(&mut d, KeyName::End, NONE), Outcome::None);
    assert_eq!(key(&mut d, KeyName::Home, NONE), Outcome::Redraw);
    assert_eq!(d.menu().unwrap().hover, Some(0));
    assert_eq!(key(&mut d, KeyName::PageDown, NONE), Outcome::None);
    assert_eq!(key(&mut d, KeyName::Left, NONE), Outcome::None);
    assert!(d.is_open());
    assert!(d.close());
    assert!(!d.close());
}

// ---- where the menu goes ------------------------------------------------------------------

#[test]
fn the_menu_opens_under_the_field_at_its_left_and_at_least_as_wide() {
    for w in [120.0, 160.0, 260.0] {
        let field = Rect::new(40.0, 30.0, w, DROPDOWN_H);
        let mut d = widths();
        d.open_menu(field).unwrap();
        place(&mut d, AREA);
        let menu = d.menu().unwrap();
        let r = menu.rect();
        assert_eq!(r.x, field.x, "width {w}");
        assert_eq!(r.y, field.bottom() + MENU_GAP);
        assert!(r.w >= field.w);
        assert!(!menu.opened_above() && !menu.opened_left());
        // The same as the geometry `Menu::around` is made of, round the field and its gaps.
        let anchor = Rect::new(
            field.x,
            field.y - MENU_GAP,
            field.w,
            field.h + 2.0 * MENU_GAP,
        );
        assert_eq!(crate::submenu::around(r.w, r.h, anchor, AREA).rect, r);
        // The chosen option is ticked, and its row is where the pointer finds it.
        assert!(menu.items[1].checked && !menu.items[0].checked);
        let row = menu.row_rect(1);
        assert!(row.y >= r.y && row.bottom() <= r.bottom());
    }
    // A field wider than the menu would be makes the menu as wide as itself.
    let field = Rect::new(40.0, 30.0, 260.0, DROPDOWN_H);
    let mut d = widths();
    d.open_menu(field);
    place(&mut d, AREA);
    assert_eq!(d.menu().unwrap().rect().w, 260.0);
}

#[test]
fn the_menu_opens_above_a_field_at_the_bottom_and_leftwards_at_the_right() {
    let low = Rect::new(40.0, AREA.bottom() - 40.0, 160.0, DROPDOWN_H);
    let mut d = widths();
    d.open_menu(low);
    place(&mut d, AREA);
    let menu = d.menu().unwrap();
    assert!(menu.opened_above());
    assert_eq!(menu.rect().bottom(), low.y - MENU_GAP);
    assert_eq!(menu.rect().x, low.x);

    let right = Rect::new(AREA.right() - 150.0, 30.0, 140.0, DROPDOWN_H);
    let mut d = widths();
    d.open_menu(right);
    place(&mut d, AREA);
    let menu = d.menu().unwrap();
    assert!(menu.opened_left());
    assert_eq!(menu.rect().right(), right.right());
    assert_eq!(menu.rect().y, right.bottom() + MENU_GAP);

    // In a short window the menu scrolls, and the chosen row is in view.
    let mut d = Dropdown::new((0..40).map(|i| (i, format!("Option {i}"))), 30);
    d.open_menu(FIELD);
    place(&mut d, AREA);
    let menu = d.menu().unwrap();
    assert!(menu.scrolls());
    let (r, row) = (menu.rect(), menu.row_rect(30));
    assert!(row.y >= r.y && row.bottom() <= r.bottom());
}

// ---- the pointer --------------------------------------------------------------------------

#[test]
fn a_click_on_the_field_opens_it_and_a_click_on_a_row_chooses_it() {
    let mut d = widths();
    let (x, y) = (FIELD.cx(), FIELD.cy());
    assert_eq!(d.press(FIELD, x, y), Outcome::Redraw);
    assert!(d.pressed() && !d.is_open(), "it opens on the release");
    assert_eq!(d.release(FIELD, x, y), Outcome::Opened);
    assert!(!d.pressed());
    place(&mut d, AREA);
    let row = d.menu().unwrap().row_rect(3);
    assert!(d.pointer(row.cx(), row.cy()));
    assert!(!d.pointer(row.cx(), row.cy()), "the same row again");
    assert_eq!(d.press(FIELD, row.cx(), row.cy()), Outcome::Redraw);
    assert_eq!(d.release(FIELD, row.cx(), row.cy()), Outcome::Changed(3));
    assert!(!d.is_open());
    assert_eq!(d.selected_value(), Some(&"full"));

    // A press on one row and a release on another chooses nothing and leaves it open.
    d.press(FIELD, x, y);
    d.release(FIELD, x, y);
    place(&mut d, AREA);
    let (a, b) = (d.menu().unwrap().row_rect(0), d.menu().unwrap().row_rect(4));
    d.press(FIELD, a.cx(), a.cy());
    assert_eq!(d.release(FIELD, b.cx(), b.cy()), Outcome::None);
    assert!(d.is_open());
    // A release in the menu with no press in it does nothing either.
    assert_eq!(d.release(FIELD, a.cx(), a.cy()), Outcome::None);
    assert!(d.is_open());
}

#[test]
fn a_press_outside_the_menu_closes_it_with_no_change_and_is_used_up() {
    for (x, y) in [(FIELD.cx(), FIELD.cy()), (450.0, 380.0)] {
        let mut d = widths();
        d.key(FIELD, &KeyName::Space, NONE);
        place(&mut d, AREA);
        d.key(FIELD, &KeyName::Down, NONE);
        assert_eq!(d.press(FIELD, x, y), Outcome::Closed);
        assert!(!d.is_open());
        assert_eq!(d.selected(), Some(1));
        // The release that follows does not open it again.
        assert_eq!(d.release(FIELD, x, y), Outcome::Unused);
        assert!(!d.is_open());
    }
    // Closed, a press elsewhere is someone else's, and a release off the field ends a press.
    let mut d = widths();
    assert_eq!(d.press(FIELD, 450.0, 380.0), Outcome::Unused);
    assert_eq!(d.release(FIELD, 450.0, 380.0), Outcome::Unused);
    assert_eq!(d.press(FIELD, FIELD.cx(), FIELD.cy()), Outcome::Redraw);
    assert_eq!(d.release(FIELD, 450.0, 380.0), Outcome::Redraw);
    assert!(!d.is_open() && !d.pressed());
    assert_eq!(d.press(FIELD, FIELD.cx(), FIELD.cy()), Outcome::Redraw);
    assert!(d.cancel());
    assert_eq!(d.release(FIELD, FIELD.cx(), FIELD.cy()), Outcome::Unused);
    assert!(!d.is_open());
}

#[test]
fn the_wheel_scrolls_an_open_menu_under_the_pointer_and_nothing_else() {
    let mut d = Dropdown::new((0..40).map(|i| (i, format!("Option {i}"))), 0);
    assert!(
        !d.wheel(FIELD.cx(), FIELD.cy(), 40.0),
        "a closed field ignores it"
    );
    d.open_menu(FIELD);
    place(&mut d, AREA);
    let r = d.menu().unwrap().rect();
    assert!(!d.wheel(r.x - 5.0, r.cy(), 40.0));
    assert!(!d.wheel(r.cx(), r.cy(), f32::NAN));
    assert!(d.wheel(r.cx(), r.cy(), 40.0));
    assert_eq!(d.menu().unwrap().scroll(), 40.0);
    assert_eq!(d.selected(), Some(0));
}

/// A long pseudo-random run of keys, presses, releases and moves: the choice stays inside the
/// options, and every answer tells the truth about what changed.
#[test]
fn a_random_run_of_events_keeps_the_choice_inside_and_the_answers_true() {
    let mut seed: u32 = 0x9e37_79b9;
    let mut next = move || {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        seed >> 8
    };
    let keys = [
        KeyName::Up,
        KeyName::Down,
        KeyName::Home,
        KeyName::End,
        KeyName::Enter,
        KeyName::Space,
        KeyName::Escape,
        KeyName::Tab,
        KeyName::F(4),
        KeyName::Char('o'),
        KeyName::Char('5'),
        KeyName::Left,
    ];
    let mut d = Dropdown::new((0..12).map(|i| (i, format!("Option {i}"))), 4);
    for step in 0..6000 {
        let (before, was_open) = (d.selected(), d.is_open());
        let x = (next() % 500) as f32;
        let y = (next() % 400) as f32;
        let out = match next() % 6 {
            0 | 1 => {
                let k = keys[(next() as usize) % keys.len()].clone();
                let m = if next() % 4 == 0 { ALT } else { NONE };
                d.key(FIELD, &k, m)
            }
            2 => d.press(FIELD, x, y),
            3 => d.release(FIELD, x, y),
            4 => {
                d.pointer(x, y);
                Outcome::None
            }
            _ => {
                d.wheel(x, y, 32.0);
                Outcome::None
            }
        };
        place(&mut d, AREA);
        let now = d.selected().unwrap();
        assert!(now < d.len(), "step {step}");
        match out {
            Outcome::Opened => assert!(!was_open && d.is_open(), "step {step}"),
            Outcome::Closed => assert!(was_open && !d.is_open() && d.selected() == before),
            Outcome::Changed(k) => {
                assert!(d.selected() == Some(k) && before != Some(k), "step {step}");
                assert!(!d.is_open());
            }
            _ => assert!(
                d.selected() == before && d.is_open() == was_open,
                "step {step}"
            ),
        }
    }
}

// ---- drawing ------------------------------------------------------------------------------

fn drawn<T>(pal: &Palette, d: &Dropdown<T>, look: Look) -> Bitmap {
    let mut bmp = Bitmap::new(240, 90);
    let mut gfx = Gfx::new();
    {
        let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
        p.fill(Rect::new(0.0, 0.0, 240.0, 90.0), pal.popup);
        draw_dropdown(&mut p, pal, FIELD, d, look);
    }
    bmp
}

fn at(bmp: &Bitmap, x: u32, y: u32) -> [u8; 3] {
    let o = ((y * bmp.width + x) * 4) as usize;
    [bmp.pixels[o + 2], bmp.pixels[o + 1], bmp.pixels[o]]
}

#[test]
fn the_looks_of_the_field_draw_differently_and_an_open_one_as_one_under_the_pointer() {
    for pal in [&LIGHT, &DARK] {
        let d = widths();
        let plain = Look::default();
        let hov = Look {
            hovered: true,
            ..plain
        };
        let focus = Look {
            focused: true,
            ..plain
        };
        let off = Look {
            disabled: true,
            ..plain
        };
        let empty: Dropdown<u8> = Dropdown::new(Vec::<(u8, &str)>::new(), 0);
        let mut other = widths();
        other.select(3);
        let all = [
            drawn(pal, &d, plain).pixels,
            drawn(pal, &d, hov).pixels,
            drawn(pal, &d, focus).pixels,
            drawn(pal, &d, off).pixels,
            drawn(pal, &empty, plain).pixels,
            drawn(pal, &other, plain).pixels,
        ];
        for i in 0..all.len() {
            for j in i + 1..all.len() {
                assert_ne!(all[i], all[j], "looks {i} and {j}");
            }
        }
        // Inside, clear of the text: the field colour at rest.
        assert_eq!(at(&drawn(pal, &d, plain), 150, 44), pal.field);
        // Open, it is drawn as under the pointer.
        let mut open = widths();
        open.open_menu(FIELD);
        assert_eq!(drawn(pal, &open, plain).pixels, all[1]);
        // The ring is accent outside the field and nothing further out.
        let ring = drawn(pal, &d, focus);
        assert_eq!(at(&ring, 120, 28), pal.accent);
        assert_eq!(at(&ring, 120, 25), pal.popup);
        // Disabled, it takes no ring and no hover, open or not.
        for l in [hov, focus] {
            let l = Look {
                disabled: true,
                ..l
            };
            assert_eq!(drawn(pal, &d, l).pixels, all[3]);
            assert_eq!(drawn(pal, &open, l).pixels, all[3]);
        }
    }
}

#[test]
fn a_nonsense_rectangle_draws_nothing() {
    let blank = {
        let mut bmp = Bitmap::new(60, 40);
        let mut gfx = Gfx::new();
        let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
        p.fill(Rect::new(0.0, 0.0, 60.0, 40.0), LIGHT.popup);
        bmp
    };
    let d = widths();
    for r in [
        Rect::new(f32::NAN, 0.0, 40.0, 20.0),
        Rect::new(0.0, 0.0, 40.0, f32::NEG_INFINITY),
        Rect::new(0.0, 0.0, 0.0, 0.0),
        Rect::new(10.0, 10.0, -40.0, 20.0),
    ] {
        let mut bmp = Bitmap::new(60, 40);
        let mut gfx = Gfx::new();
        let mut anim = Animator::new();
        {
            let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
            p.fill(Rect::new(0.0, 0.0, 60.0, 40.0), LIGHT.popup);
            let look = Look {
                focused: true,
                ..Look::default()
            };
            draw_dropdown(&mut p, &LIGHT, r, &d, look);
            draw_dropdown_animated(&mut p, &LIGHT, &mut anim, "d", r, &d, look);
        }
        assert_eq!(bmp.pixels, blank.pixels, "{r:?}");
    }
    // A closed drop-down's menu draws nothing.
    let mut bmp = Bitmap::new(60, 40);
    bmp.pixels.copy_from_slice(&blank.pixels);
    let mut gfx = Gfx::new();
    let mut anim = Animator::new();
    {
        let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
        let mut d = widths();
        let area = Rect::new(0.0, 0.0, 60.0, 40.0);
        draw_dropdown_menu(&mut p, &LIGHT, &mut d, area);
        draw_dropdown_menu_animated(&mut p, &LIGHT, &mut anim, "d", &mut d, area);
    }
    assert_eq!(bmp.pixels, blank.pixels);
}

fn sample(bmp: &Bitmap, scale: f32, x: f32, y: f32) -> [u8; 4] {
    let (x, y) = ((x * scale) as u32, (y * scale) as u32);
    let o = ((y * bmp.width + x) * 4) as usize;
    bmp.pixels[o..o + 4].try_into().unwrap()
}

/// `bmp` with the device pixels under `r` (grown out to whole pixels) set to `ground`.
fn paint_ground(bmp: &mut Bitmap, scale: f32, r: Rect, ground: [u8; 4]) {
    let x0 = (r.x * scale).floor().max(0.0) as u32;
    let y0 = (r.y * scale).floor().max(0.0) as u32;
    let x1 = ((r.right() * scale).ceil() as u32).min(bmp.width);
    let y1 = ((r.bottom() * scale).ceil() as u32).min(bmp.height);
    for y in y0..y1 {
        for x in x0..x1 {
            let o = ((y * bmp.width + x) * 4) as usize;
            bmp.pixels[o..o + 4].copy_from_slice(&ground);
        }
    }
}

/// A dialog's frame with the control at `ctrl` wiped to the ground beside it and drawn again
/// by `draw`: equal to the frame when `draw` draws what the dialog drew.
fn redrawn_over(frame: &Bitmap, scale: f32, ctrl: Rect, draw: impl FnOnce(&mut Painter)) -> Bitmap {
    let ground = sample(frame, scale, ctrl.x - 6.0, ctrl.cy());
    let mut b = Bitmap::new(frame.width, frame.height);
    b.pixels.copy_from_slice(&frame.pixels);
    paint_ground(&mut b, scale, ctrl.inset(-2.0), ground);
    let mut gfx = Gfx::new();
    {
        let mut p = Painter::new(&mut b, &mut gfx, scale);
        draw(&mut p);
    }
    b
}

fn frame_of(scale: f32, draw: impl FnOnce(&mut Painter, Rect)) -> Bitmap {
    let (w, h) = (900.0, 600.0);
    let mut bmp = Bitmap::new((w * scale) as u32, (h * scale) as u32);
    let mut gfx = Gfx::new();
    {
        let mut p = Painter::new(&mut bmp, &mut gfx, scale);
        draw(&mut p, Rect::new(0.0, 0.0, w, h));
    }
    bmp
}

const WIDTHS: [&str; 5] = ["Narrow", "Medium", "Wide", "Full", "Custom"];

/// The single-page settings dialog's drop-down row, at rest and under the pointer, is this
/// field's pixels, in both themes and at two scales.
#[test]
fn the_field_is_the_settings_dialogs_dropdown_pixel_for_pixel() {
    for pal in [&LIGHT, &DARK] {
        for scale in [1.0, 1.5] {
            for hover in [false, true] {
                let mut f = Form::new(
                    "Settings",
                    vec![
                        Row::header("Appearance"),
                        Row::choice(6, "Page width", &WIDTHS, 1).help("How wide the text runs"),
                    ],
                );
                let a = frame_of(scale, |p, win| {
                    f.layout(p, win);
                    let c = f.control_rect(1);
                    if hover {
                        f.pointer_move(c.cx(), c.cy());
                    }
                    f.draw(p, pal, win, false);
                });
                assert!(!f.is_segmented(1));
                let ctrl = f.control_rect(1);
                assert_eq!(ctrl.h, DROPDOWN_H);
                let d = Dropdown::new(WIDTHS.map(|w| (w, w)), 1);
                let look = Look {
                    hovered: hover,
                    ..Look::default()
                };
                let b = redrawn_over(&a, scale, ctrl, |p| draw_dropdown(p, pal, ctrl, &d, look));
                assert!(
                    a.pixels == b.pixels,
                    "hover {hover}, scale {scale}, dark {}",
                    pal.dark
                );
            }
        }
    }
}

/// The categorised settings dialog's drop-down rows, a disabled one included, are this field's
/// pixels too. Both draw with `field` now, so this says the standalone field is the dialog's;
/// that the dialog still draws what it drew is
/// `the_categorised_dialogs_dropdowns_are_still_drawn_as_before_the_move`, against a frozen copy.
#[test]
fn the_field_is_the_categorised_dialogs_dropdown_and_its_disabled_look() {
    for pal in [&LIGHT, &DARK] {
        for scale in [1.0, 1.5] {
            for (id, disabled) in [(1, false), (2, true)] {
                for hover in [false, true] {
                    let themes = ["System", "Light", "Dark"];
                    let mut f = pages::Form::new(
                        "Settings",
                        vec![Page::new(
                            "General",
                            vec![
                                PageItem::new(Row::choice(1, "Theme", &themes, 2)).dropdown(),
                                PageItem::new(Row::choice(2, "Accent", &themes, 0))
                                    .dropdown()
                                    .disabled("Follows the system"),
                            ],
                        )],
                    );
                    let a = frame_of(scale, |p, win| {
                        f.layout(p, win);
                        let c = f.control_rect(id);
                        if hover {
                            f.pointer_move(c.cx(), c.cy());
                        }
                        f.draw(p, pal, win, false);
                    });
                    let ctrl = f.control_rect(id);
                    let d = Dropdown::new(themes.map(|t| (t, t)), if id == 1 { 2 } else { 0 });
                    let look = Look {
                        hovered: hover,
                        disabled,
                        ..Look::default()
                    };
                    let b =
                        redrawn_over(&a, scale, ctrl, |p| draw_dropdown(p, pal, ctrl, &d, look));
                    assert!(
                        a.pixels == b.pixels,
                        "id {id}, hover {hover}, scale {scale}, dark {}",
                        pal.dark
                    );
                }
            }
        }
    }
}

#[test]
fn the_animated_field_at_rest_is_the_plain_one_and_fades_between() {
    let clock = ManualClock::new();
    let mut anim = Animator::new();
    let mut gfx = Gfx::new();
    let d = widths();
    let mut frame = |ms: u64, look: Look, anim: &mut Animator| {
        // A frame each 16 ms or less: the animator counts at most 50 ms a frame.
        let mut left = ms;
        loop {
            let step = left.min(16);
            left -= step;
            clock.advance_ms(step);
            anim.begin_frame(clock.now());
            let mut bmp = Bitmap::new(240, 90);
            {
                let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
                p.fill(Rect::new(0.0, 0.0, 240.0, 90.0), LIGHT.popup);
                draw_dropdown_animated(&mut p, &LIGHT, anim, "width", FIELD, &d, look);
            }
            let more = anim.end_frame().is_some();
            if left == 0 {
                break (bmp, more);
            }
        }
    };
    let plain = Look::default();
    let hov = Look {
        hovered: true,
        ..plain
    };
    let (rest, more) = frame(0, plain, &mut anim);
    assert!(!more && rest.pixels == drawn(&LIGHT, &d, plain).pixels);
    let (same, more) = frame(0, hov, &mut anim);
    assert!(more && same.pixels == rest.pixels);
    let (mid, more) = frame(40, hov, &mut anim);
    let full = drawn(&LIGHT, &d, hov);
    assert!(more && mid.pixels != rest.pixels && mid.pixels != full.pixels);
    let (end, more) = frame(100, hov, &mut anim);
    assert!(!more && end.pixels == full.pixels);
    for look in [
        Look {
            focused: true,
            ..plain
        },
        Look {
            disabled: true,
            ..hov
        },
    ] {
        frame(0, look, &mut anim);
        let (b, _) = frame(200, look, &mut anim);
        assert!(b.pixels == drawn(&LIGHT, &d, look).pixels, "{look:?}");
    }
    anim.set_system_reduced(true);
    let (b, more) = frame(0, hov, &mut anim);
    assert!(!more && b.pixels == full.pixels);
}

#[test]
fn the_animated_menu_enters_and_then_is_the_plain_menu_and_enters_again_after_a_close() {
    let clock = ManualClock::new();
    let mut anim = Animator::new();
    let mut gfx = Gfx::new();
    let mut d = widths();
    let mut frame = |ms: u64, d: &mut Dropdown<&str>, anim: &mut Animator| {
        // A frame each 16 ms or less: the animator counts at most 50 ms a frame.
        let mut left = ms;
        loop {
            let step = left.min(16);
            left -= step;
            clock.advance_ms(step);
            anim.begin_frame(clock.now());
            let mut bmp = Bitmap::new(500, 400);
            {
                let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
                p.fill(AREA, LIGHT.page);
                draw_dropdown_menu_animated(&mut p, &LIGHT, anim, "width", d, AREA);
            }
            let more = anim.end_frame().is_some();
            if left == 0 {
                break (bmp, more);
            }
        }
    };
    let plain = |d: &mut Dropdown<&str>| {
        let mut bmp = Bitmap::new(500, 400);
        let mut gfx = Gfx::new();
        {
            let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
            p.fill(AREA, LIGHT.page);
            draw_dropdown_menu(&mut p, &LIGHT, d, AREA);
        }
        bmp
    };
    d.open_menu(FIELD);
    let open = plain(&mut d);
    for _ in 0..2 {
        let (first, more) = frame(0, &mut d, &mut anim);
        assert!(more && first.pixels != open.pixels, "it enters");
        let (end, more) = frame(300, &mut d, &mut anim);
        assert!(!more && end.pixels == open.pixels);
        d.close();
        let (gone, more) = frame(0, &mut d, &mut anim);
        assert!(!more && gone.pixels == plain(&mut d).pixels);
        d.open_menu(FIELD);
    }
    anim.set_system_reduced(true);
    d.close();
    frame(0, &mut d, &mut anim);
    d.open_menu(FIELD);
    let (first, more) = frame(0, &mut d, &mut anim);
    assert!(!more && first.pixels == open.pixels);
}

/// The sheet a person looks at: the field at rest, under the pointer, focused, disabled and
/// empty, one open with its menu under it and one at the bottom whose menu opens above, light
/// and dark, at two scales. `cargo test -p mdshared-ui dropdown_sheet -- --ignored`, then open
/// target/specimen/dropdown.png and dropdown-150.png.
#[test]
#[ignore]
fn dropdown_sheet() {
    for (scale, file) in [(1.0, "dropdown.png"), (1.5, "dropdown-150.png")] {
        let (w, h) = (900.0, 640.0);
        let mut bmp = Bitmap::new((w * scale) as u32, (h * scale) as u32);
        let mut gfx = Gfx::new();
        let mut p = Painter::new(&mut bmp, &mut gfx, scale);
        for (half, pal) in [&LIGHT, &DARK].into_iter().enumerate() {
            let area = Rect::new(half as f32 * w / 2.0, 0.0, w / 2.0, h);
            p.fill(area, pal.popup);
            let x = area.x + 24.0;
            let looks = [
                Look::default(),
                Look {
                    hovered: true,
                    ..Look::default()
                },
                Look {
                    focused: true,
                    ..Look::default()
                },
                Look {
                    disabled: true,
                    ..Look::default()
                },
            ];
            for (k, look) in looks.into_iter().enumerate() {
                let r = Rect::new(x, 24.0 + k as f32 * 44.0, 160.0, DROPDOWN_H);
                draw_dropdown(&mut p, pal, r, &widths(), look);
            }
            let empty: Dropdown<u8> = Dropdown::new(Vec::<(u8, &str)>::new(), 0);
            let r = Rect::new(x, 24.0 + 4.0 * 44.0, 160.0, DROPDOWN_H);
            draw_dropdown(&mut p, pal, r, &empty, Look::default());
            // Open, with its menu under it and the highlight one row down.
            let below = Rect::new(x + 210.0, 24.0, 150.0, DROPDOWN_H);
            let mut open = widths();
            open.open_menu(below);
            open.key(below, &KeyName::Down, Mods::default());
            draw_dropdown(&mut p, pal, below, &open, Look::default());
            draw_dropdown_menu(&mut p, pal, &mut open, area);
            // At the bottom, its menu above it.
            let low = Rect::new(x + 210.0, h - 60.0, 150.0, DROPDOWN_H);
            let mut up = widths();
            up.open_menu(low);
            draw_dropdown(&mut p, pal, low, &up, Look::default());
            draw_dropdown_menu(&mut p, pal, &mut up, area);
        }
        let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/specimen");
        std::fs::create_dir_all(&out).unwrap();
        bmp.write_png(&out.join(file)).unwrap();
    }
}

// ---- the frozen reference ------------------------------------------------------------------

/// FROZEN REFERENCE, not to be edited: the categorised settings form's drop-down field as
/// `settings/pages.rs` drew it on `feat/anim` (6e0b9c0), before it moved to [`field`]: the body
/// of its `Row::Choice` arm, word for word, but the focus ring and the lookup of the label (its
/// `CONTROL_SIZE` was 13). `h` is the pointer over it or its menu open, `off` a disabled row.
fn frozen_pages_field(p: &mut Painter, pal: &Palette, ctrl: Rect, shown: &str, h: bool, off: bool) {
    let fill = if h {
        mix(pal.field, pal.text, 0.05)
    } else {
        pal.field
    };
    let edge = if h {
        mix(pal.line, pal.text, 0.18)
    } else {
        pal.line
    };
    p.frame(ctrl, 6.0, fill, edge);
    p.text(
        Face::Ui,
        13.0,
        ctrl.x + 11.0,
        ctrl.cy(),
        shown,
        if off { pal.faint } else { pal.text },
        ctrl.w - 36.0,
    );
    let arrow = if off { pal.faint } else { pal.muted };
    p.icon(glyph::DOWN, 10.0, ctrl.right() - 15.0, ctrl.cy(), arrow);
}

/// The categorised form draws its drop-down rows as it did before the field was shared: its
/// frame now, with each field wiped and drawn again by the frozen copy of the old code, is the
/// same frame, pixel for pixel. Live and disabled; at rest and under the pointer; light and
/// dark; scales 1 and 1.5.
#[test]
fn the_categorised_dialogs_dropdowns_are_still_drawn_as_before_the_move() {
    let themes = ["System", "Light", "Dark"];
    for pal in [&LIGHT, &DARK] {
        for scale in [1.0, 1.5] {
            for (id, off) in [(1, false), (2, true)] {
                for hover in [false, true] {
                    let mut f = pages::Form::new(
                        "Settings",
                        vec![Page::new(
                            "General",
                            vec![
                                PageItem::new(Row::choice(1, "Theme", &themes, 2)).dropdown(),
                                PageItem::new(Row::choice(2, "Accent", &themes, 0))
                                    .dropdown()
                                    .disabled("Follows the system"),
                            ],
                        )],
                    );
                    let a = frame_of(scale, |p, win| {
                        f.layout(p, win);
                        let c = f.control_rect(id);
                        if hover {
                            f.pointer_move(c.cx(), c.cy());
                        }
                        f.draw(p, pal, win, false);
                    });
                    let ctrl = f.control_rect(id);
                    let shown = if id == 1 { "Dark" } else { "System" };
                    let b = redrawn_over(&a, scale, ctrl, |p| {
                        frozen_pages_field(p, pal, ctrl, shown, hover && !off, off);
                    });
                    assert!(
                        a.pixels == b.pixels,
                        "id {id}, hover {hover}, scale {scale}, dark {}",
                        pal.dark
                    );
                }
            }
        }
    }
}

// ---- Up and Down at the ends, and a menu with no room -------------------------------------

#[test]
fn open_up_and_down_stop_at_the_first_and_the_last_option() {
    let mut d = widths();
    key(&mut d, KeyName::Space, NONE);
    assert_eq!(key(&mut d, KeyName::Up, NONE), Outcome::Redraw);
    assert_eq!(d.menu().unwrap().hover, Some(0));
    assert_eq!(
        key(&mut d, KeyName::Up, NONE),
        Outcome::None,
        "no going round"
    );
    assert_eq!(d.menu().unwrap().hover, Some(0));
    key(&mut d, KeyName::End, NONE);
    assert_eq!(d.menu().unwrap().hover, Some(4));
    assert_eq!(
        key(&mut d, KeyName::Down, NONE),
        Outcome::None,
        "no going round"
    );
    assert_eq!(d.menu().unwrap().hover, Some(4));
    assert_eq!(key(&mut d, KeyName::Up, NONE), Outcome::Redraw);
    assert_eq!(d.menu().unwrap().hover, Some(3));
    // With no highlight (the pointer left the rows), an arrow starts from the choice.
    if let Some(m) = d.menu_mut() {
        m.hover = None;
    }
    assert_eq!(key(&mut d, KeyName::Down, NONE), Outcome::Redraw);
    assert_eq!(d.menu().unwrap().hover, Some(1));
    assert_eq!(d.selected(), Some(1));
    // A single option: the arrows have nowhere to go.
    let mut one = Dropdown::new([(0, "Only")], 0);
    key(&mut one, KeyName::Space, NONE);
    assert_eq!(key(&mut one, KeyName::Up, NONE), Outcome::None);
    assert_eq!(key(&mut one, KeyName::Down, NONE), Outcome::None);
}

#[test]
fn a_menu_with_room_neither_below_nor_above_goes_to_the_roomier_side_and_keeps_off_the_field() {
    // Five rows want 170 px; this area leaves less than that on either side of the field.
    let area = Rect::new(0.0, 0.0, 500.0, 300.0);
    for (top, up) in [(120.0, false), (150.0, true)] {
        let field = Rect::new(40.0, top, 160.0, DROPDOWN_H);
        let mut d = widths();
        d.select(4);
        d.open_menu(field);
        place(&mut d, area);
        let menu = d.menu().unwrap();
        let r = menu.rect();
        assert_eq!(menu.opened_above(), up, "top {top}");
        assert!(
            r.y >= area.y && r.bottom() <= area.bottom(),
            "inside: {r:?}"
        );
        if up {
            assert_eq!(r.bottom(), field.y - MENU_GAP);
        } else {
            assert_eq!(r.y, field.bottom() + MENU_GAP);
        }
        assert!(menu.scrolls());
        let row = menu.row_rect(4);
        assert!(
            row.y >= r.y && row.bottom() <= r.bottom(),
            "the choice in view"
        );
    }
}

/// The placement against `Menu::around`'s on pseudo-random menus and anchors: the same where the
/// menu fits on one side; elsewhere inside the room, no taller than asked, as wide and as far
/// across as `around` puts it, and clear of an anchor that lies inside the room.
#[test]
fn the_clear_placement_is_arounds_but_never_over_the_anchor() {
    let mut seed: u32 = 7;
    let mut next = move || {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (seed >> 8) as f32 / (1u32 << 24) as f32
    };
    let room = AREA.inset(crate::submenu::MARGIN);
    let mut cut = 0;
    for _ in 0..4000 {
        let anchor = Rect::new(
            next() * 500.0,
            next() * 400.0,
            20.0 + next() * 200.0,
            next() * 40.0,
        );
        let (w, h) = (100.0 + next() * 200.0, next() * 450.0);
        let plain = crate::submenu::around(w, h, anchor, AREA);
        let clear = crate::submenu::around_clear(w, h, anchor, AREA);
        let below = anchor.bottom() >= room.y && anchor.bottom() + h <= room.bottom();
        let above = anchor.y - h >= room.y && anchor.y <= room.bottom();
        if below || above {
            assert_eq!(clear, plain);
            continue;
        }
        let below_room = (room.bottom() - anchor.bottom()).max(0.0);
        let above_room = (anchor.y - room.y).max(0.0);
        if below_room.max(above_room).min(room.h) < h.min(48.0) {
            assert_eq!(clear, plain, "a sliver falls back to covering the anchor");
            continue;
        }
        cut += 1;
        let r = clear.rect;
        assert!(
            r.y >= room.y && r.bottom() <= room.bottom() + 0.01,
            "{anchor:?} {r:?}"
        );
        assert!(r.h <= h);
        assert_eq!(
            (r.x, r.w, clear.left),
            (plain.rect.x, plain.rect.w, plain.left)
        );
        if anchor.y >= room.y && anchor.bottom() <= room.bottom() {
            assert!(
                r.bottom() <= anchor.y + 0.01 || r.y >= anchor.bottom() - 0.01,
                "{anchor:?} {r:?}"
            );
        }
    }
    assert!(cut > 100, "the run reached the case it is about ({cut})");
}

#[test]
fn an_open_menu_follows_the_field_when_the_page_scrolls() {
    let field = Rect::new(40.0, 60.0, 160.0, DROPDOWN_H);
    let mut d = widths();
    d.open_menu(field);
    place(&mut d, AREA);
    let before = d.menu().unwrap().rect();
    assert_eq!(before.y, field.bottom() + MENU_GAP);

    let scrolled = Rect::new(field.x, field.y + 90.0, field.w, field.h);
    d.set_field(scrolled);
    place(&mut d, AREA);
    let after = d.menu().unwrap().rect();
    assert_eq!(
        after.y,
        scrolled.bottom() + MENU_GAP,
        "the menu stayed where it opened"
    );
    assert_eq!((after.x, after.w), (before.x, before.w));

    // A wider field after a resize: the list is at least as wide again.
    let wider = Rect::new(scrolled.x, scrolled.y, 300.0, scrolled.h);
    d.set_field(wider);
    place(&mut d, AREA);
    assert!(
        d.menu().unwrap().rect().w >= 300.0,
        "the list kept the old width"
    );
    d.set_field(scrolled);
    place(&mut d, AREA);

    // A closed drop-down and an unusable field change nothing.
    d.set_field(Rect::new(f32::NAN, 0.0, 10.0, 10.0));
    place(&mut d, AREA);
    assert_eq!(d.menu().unwrap().rect(), after);
    d.close();
    d.set_field(field);
    assert!(d.menu().is_none());
}
