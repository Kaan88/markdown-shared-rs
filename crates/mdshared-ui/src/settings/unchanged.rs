//! The single-page form is what it was. The categorised form was added beside it
//! ([`super::pages`]); the application that still draws the single-page form must get the same
//! pixels and the same answers from it. [`super::frozen`] is the old file copied whole, and every
//! test here drives it and the live [`Form`](super::Form) through the same steps with the same
//! rows and compares every frame, byte for byte, in both themes.

use super::{Form, Row};
use crate::gfx::{Gfx, Painter, Rect};
use crate::keys::{KeyName, Mods};
use crate::theme::{DARK, LIGHT};
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

/// Every frame the steps below produce, as (what it shows, the pixels). The same text builds the
/// rows and drives the form for both types, so the only difference between the two runs is the
/// code under test.
macro_rules! frames {
    ($Form:ty, $Row:ty, $Outcome:ty, $Value:ty) => {{
        fn rows(extra: bool) -> Vec<$Row> {
            let mut v = vec![
                <$Row>::header("Editor"),
                <$Row>::toggle(1, "Word wrap", false).help("Wrap long lines"),
                <$Row>::number(3, "Font size", 14, 8, 32).unit("px"),
                <$Row>::header("Appearance"),
                <$Row>::choice(2, "Theme", &["System", "Light", "Dark"], 0),
                <$Row>::choice(
                    6,
                    "Page width",
                    &["Narrow", "Medium", "Wide", "Full", "Custom"],
                    1,
                )
                .help("How wide the text runs"),
                <$Row>::text(4, "Name", "abc").placeholder("none"),
                <$Row>::text(7, "Key", "secret").secret(),
                <$Row>::header("Other"),
                <$Row>::action(5, "Reset to defaults", "Reset"),
            ];
            for i in 0..8 {
                v.push(<$Row>::toggle(20 + i, format!("Option {i}"), i % 2 == 0));
            }
            if extra {
                v.push(<$Row>::header("Last"));
                v.push(<$Row>::toggle(40, "One more", true).help("Added later"));
            }
            v
        }
        fn snap(f: &mut $Form, w: u32, h: u32, scale: f32, dark: bool, caret: bool) -> Vec<u8> {
            let mut gfx = Gfx::new();
            let mut bmp = Bitmap::new(w, h);
            {
                let mut p = Painter::new(&mut bmp, &mut gfx, scale);
                let win = Rect::new(0.0, 0.0, w as f32 / scale, h as f32 / scale);
                f.draw(&mut p, if dark { &DARK } else { &LIGHT }, win, caret);
            }
            bmp.pixels
        }
        fn shoot(
            out: &mut Vec<(String, Vec<u8>)>,
            f: &mut $Form,
            name: &str,
            sz: (u32, u32, f32),
            what: &str,
        ) {
            for (dark, caret) in [(false, true), (true, true), (false, false)] {
                let theme = if dark { "dark" } else { "light" };
                out.push((
                    format!("{name}: {what} ({theme}, caret {caret})"),
                    snap(f, sz.0, sz.1, sz.2, dark, caret),
                ));
            }
        }
        let mut out: Vec<(String, Vec<u8>)> = Vec::new();
        for (w, h, scale) in [
            (1000u32, 800u32, 1.0f32),
            (1000, 520, 1.0),
            (700, 500, 1.0),
            (520, 360, 1.0),
            (420, 300, 1.5),
        ] {
            for single in [false, true] {
                let mut f = <$Form>::new("Settings", rows(false));
                if single {
                    f = f.single_page();
                }
                let sz = (w, h, scale);
                let mut name = format!("{w}x{h} at {scale}");
                if single {
                    name.push_str(", single page");
                }
                shoot(&mut out, &mut f, &name, sz, "as opened");
                f.key(&KeyName::Tab, NONE, None);
                shoot(&mut out, &mut f, &name, sz, "first Tab");
                for _ in 0..2 {
                    f.key(&KeyName::Tab, NONE, None);
                }
                shoot(&mut out, &mut f, &name, sz, "focus on the third control");
                f.wheel(300.0, 200.0, 70.0);
                shoot(&mut out, &mut f, &name, sz, "wheel");
                // Hover over each kind of control, and press a toggle without releasing.
                for i in [1usize, 2, 4, 5, 6, 7, 9] {
                    let c = f.control_rect(i);
                    f.pointer_move(c.cx(), c.cy());
                    shoot(&mut out, &mut f, &name, sz, &format!("hover on row {i}"));
                }
                let c = f.control_rect(1);
                f.press(c.cx(), c.cy(), NONE, 1);
                shoot(&mut out, &mut f, &name, sz, "toggle pressed");
                let o: $Outcome = f.release(c.cx(), c.cy());
                shoot(
                    &mut out,
                    &mut f,
                    &name,
                    sz,
                    &format!("toggle released: {o:?}"),
                );
                // Text: a click, typing, a selection.
                let c = f.control_rect(6);
                f.press(c.x + 40.0, c.cy(), NONE, 1);
                f.release(c.x + 40.0, c.cy());
                f.key(&KeyName::Char('x'), NONE, Some("x"));
                shoot(&mut out, &mut f, &name, sz, "typed into a text box");
                f.key(&KeyName::Char('a'), Mods { ctrl: true, ..NONE }, None);
                shoot(&mut out, &mut f, &name, sz, "select all in a text box");
                let c = f.control_rect(7);
                f.press(c.x + 10.0, c.cy(), NONE, 1);
                f.release(c.x + 10.0, c.cy());
                shoot(&mut out, &mut f, &name, sz, "the secret box focused");
                // A dropdown, open and closed.
                let c = f.control_rect(5);
                f.press(c.cx(), c.cy(), NONE, 1);
                f.release(c.cx(), c.cy());
                shoot(&mut out, &mut f, &name, sz, "a dropdown open");
                f.key(&KeyName::Down, NONE, None);
                shoot(&mut out, &mut f, &name, sz, "a dropdown, one down");
                f.key(&KeyName::Escape, NONE, None);
                shoot(&mut out, &mut f, &name, sz, "a dropdown closed again");
                // The steppers and a segmented control.
                let (m, _, pl) = f.stepper_parts(2);
                f.press(pl.cx(), pl.cy(), NONE, 1);
                f.release(pl.cx(), pl.cy());
                f.press(m.cx(), m.cy(), NONE, 1);
                shoot(&mut out, &mut f, &name, sz, "stepper held");
                f.release(m.cx(), m.cy());
                let s = f.segment_rect(4, 2);
                f.press(s.cx(), s.cy(), NONE, 1);
                f.release(s.cx(), s.cy());
                shoot(&mut out, &mut f, &name, sz, "a segment chosen");
                // The category list.
                f.jump_to_category(1);
                shoot(&mut out, &mut f, &name, sz, "category 1");
                f.jump_to_category(2);
                shoot(&mut out, &mut f, &name, sz, "category 2");
                for _ in 0..30 {
                    f.key(&KeyName::Tab, SHIFT, None);
                }
                shoot(
                    &mut out,
                    &mut f,
                    &name,
                    sz,
                    "Shift+Tab round to the Close button",
                );
                f.set_value(1, <$Value>::Bool(true));
                f.set_value(4, <$Value>::Text("changed".into()));
                shoot(&mut out, &mut f, &name, sz, "values set by the host");
                f.set_rows(rows(true));
                shoot(&mut out, &mut f, &name, sz, "rows replaced");
                let c = f.close_rect();
                f.pointer_move(c.cx(), c.cy());
                shoot(&mut out, &mut f, &name, sz, "hover on Close");
                f.press(c.cx(), c.cy(), NONE, 1);
                shoot(&mut out, &mut f, &name, sz, "Close pressed");
                f.release(c.cx(), c.cy());
            }
        }
        out
    }};
}

fn live() -> Vec<(String, Vec<u8>)> {
    frames!(Form, Row, super::Outcome, super::Value)
}

fn frozen() -> Vec<(String, Vec<u8>)> {
    use super::frozen as old;
    frames!(old::Form, old::Row, old::Outcome, old::Value)
}

#[test]
fn the_single_page_form_still_draws_what_it_drew() {
    let (now, was) = (live(), frozen());
    assert_eq!(now.len(), was.len(), "the same number of frames");
    assert!(now.len() > 300, "the steps ran: {} frames", now.len());
    for ((what, a), (_, b)) in now.iter().zip(&was) {
        assert!(a == b, "the single-page form draws differently: {what}");
    }
}

#[test]
fn the_single_page_form_still_answers_what_it_answered() {
    use super::frozen as old;
    macro_rules! answers {
        ($Form:ty, $Row:ty) => {{
            let mut out: Vec<String> = Vec::new();
            for (w, h) in [(1000u32, 800u32), (700, 400), (520, 360)] {
                for single in [false, true] {
                    let mut gfx = Gfx::new();
                    let mut bmp = Bitmap::new(w, h);
                    let mut f = <$Form>::new(
                        "Settings",
                        vec![
                            <$Row>::header("A"),
                            <$Row>::toggle(1, "One", false).help("h"),
                            <$Row>::choice(2, "Two", &["a", "b", "c"], 0),
                            <$Row>::number(3, "Three", 5, 0, 9).step(2),
                            <$Row>::text(4, "Four", "x"),
                            <$Row>::header("B"),
                            <$Row>::action(5, "Five", "Go"),
                        ],
                    );
                    if single {
                        f = f.single_page();
                    }
                    {
                        let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
                        f.layout(&mut p, Rect::new(0.0, 0.0, w as f32, h as f32));
                    }
                    out.push(format!("{w}x{h} single {single}: card {:?}", f.rect()));
                    out.push(format!("body {:?} nav {}", f.body_rect(), f.has_nav()));
                    for i in 0..7 {
                        out.push(format!(
                            "row {i}: {:?} {:?}",
                            f.rect_of_row(i),
                            f.control_rect(i)
                        ));
                    }
                    for k in [
                        KeyName::Tab,
                        KeyName::Right,
                        KeyName::Down,
                        KeyName::Space,
                        KeyName::Down,
                        KeyName::Left,
                        KeyName::Down,
                        KeyName::Char('+'),
                        KeyName::Enter,
                        KeyName::Escape,
                    ] {
                        out.push(format!(
                            "{k:?}: {:?} focus {:?}",
                            f.key(&k, NONE, None),
                            f.focus_id()
                        ));
                    }
                    out.push(format!("categories {:?}", f.categories()));
                }
            }
            out
        }};
    }
    let now = answers!(Form, Row);
    let was = answers!(old::Form, old::Row);
    assert_eq!(now, was);
}
