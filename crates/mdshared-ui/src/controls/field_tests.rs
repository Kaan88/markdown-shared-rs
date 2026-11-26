//! Tests for [`crate::controls::field`]: the box and the error band, the leading icon's and the
//! clear button's room, the press on the clear button and the focus it keeps, the error's colour
//! and message, the focused edge, Escape, the editing keys and a quiet disabled look.

use super::*;
use crate::gfx::{Gfx, Rect};
use crate::textfield::FieldColors;
use crate::theme::{DARK, LIGHT};
use mdshared_text::Bitmap;

const PAGE_W: u32 = 260;
const PAGE_H: u32 = 80;
const R: Rect = Rect::new(10.0, 10.0, 220.0, HEIGHT);

fn blank(pal: &Palette) -> Bitmap {
    let mut bmp = Bitmap::new(PAGE_W, PAGE_H);
    let mut gfx = Gfx::new();
    {
        let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
        p.fill(Rect::new(0.0, 0.0, PAGE_W as f32, PAGE_H as f32), pal.page);
    }
    bmp
}

fn drawn(pal: &Palette, field: &mut FramedField, look: Look, caret_on: bool) -> Bitmap {
    let mut bmp = blank(pal);
    let mut gfx = Gfx::new();
    {
        let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
        draw(&mut p, pal, R, field, look, caret_on);
    }
    bmp
}

/// The colour of a pixel as RGB.
fn at(bmp: &Bitmap, x: usize, y: usize) -> [u8; 3] {
    let o = (y * bmp.width as usize + x) * 4;
    [bmp.pixels[o + 2], bmp.pixels[o + 1], bmp.pixels[o]]
}

/// How many pixels in `area` hold exactly `c`.
fn count_in(bmp: &Bitmap, c: [u8; 3], area: Rect) -> usize {
    let mut n = 0;
    for y in 0..bmp.height as usize {
        for x in 0..bmp.width as usize {
            if area.contains(x as f32, y as f32) && at(bmp, x, y) == c {
                n += 1;
            }
        }
    }
    n
}

/// How many pixels in `area` are not `background`: ink of any shade or colour.
fn ink_in(bmp: &Bitmap, background: [u8; 3], area: Rect) -> usize {
    let mut n = 0;
    for y in 0..bmp.height as usize {
        for x in 0..bmp.width as usize {
            if area.contains(x as f32, y as f32) && at(bmp, x, y) != background {
                n += 1;
            }
        }
    }
    n
}

/// `r` without the frame's own one-pixel edge at the top and bottom.
fn inner_of(r: Rect) -> Rect {
    Rect::new(r.x, r.y + 2.0, r.w, (r.h - 4.0).max(0.0))
}

fn filled(text: &str) -> FramedField {
    let mut f = FramedField::new();
    f.field_mut().set_text(text);
    f
}

#[test]
fn the_field_is_as_high_as_its_box_and_an_error_takes_a_band_below() {
    let mut f = FramedField::new();
    assert_eq!(f.height(), HEIGHT);
    assert_eq!(f.error(), None);
    assert_eq!(f.error_rect(R), None);
    assert_eq!(f.text(), "");
    assert!(f.is_empty());
    f.set_error("A name is needed");
    assert_eq!(f.error(), Some("A name is needed"));
    assert_eq!(f.height(), HEIGHT + ERROR_H);
    let band = f.error_rect(R).unwrap();
    assert_eq!(band, Rect::new(R.x, R.bottom(), R.w, ERROR_H));
    f.clear_error();
    assert_eq!(f.height(), HEIGHT);
    assert_eq!(f.error(), None);
    // The builders set the icon and the placeholder and keep the text.
    let g = FramedField::new()
        .with_icon(crate::glyph::SEARCH)
        .with_placeholder("Search");
    assert_eq!(g.icon(), Some(crate::glyph::SEARCH));
    assert_eq!(g.placeholder(), "Search");
    assert!(g.is_empty());
    assert_eq!(FramedField::default().icon(), None);
}

#[test]
fn the_clear_button_is_on_the_right_and_only_while_there_is_text() {
    let mut f = FramedField::new();
    assert_eq!(
        f.clear_rect(R),
        None,
        "an empty field offers a clear button"
    );
    f.field_mut().set_text("abc");
    let c = f.clear_rect(R).unwrap();
    assert_eq!(c.w, CLEAR);
    assert_eq!(c.h, CLEAR);
    assert_eq!(c.right(), R.right() - CLEAR_PAD);
    assert_eq!(c.cy(), R.cy());
    // Clearing takes it away again.
    assert!(f.clear());
    assert_eq!(f.clear_rect(R), None);
    assert!(!f.clear(), "an empty field said it cleared something");
}

#[test]
fn a_leading_icon_and_the_clear_button_leave_their_room_beside_the_text() {
    let plain = FramedField::new();
    let t = plain.text_rect(R);
    assert_eq!(t.x, R.x + PAD);
    assert_eq!(t.right(), R.right() - PAD - CLEAR - CLEAR_PAD);
    assert_eq!(t.y, R.y);
    assert_eq!(t.h, R.h);
    assert_eq!(plain.icon_rect(R), None);
    let icon = FramedField::new().with_icon(crate::glyph::SEARCH);
    assert_eq!(icon.icon_rect(R), Some(Rect::new(R.x, R.y, ICON_W, R.h)));
    assert_eq!(icon.text_rect(R).x, R.x + ICON_W);
    // The text does not shift when the first character arrives: the clear button's room is
    // reserved whether or not it shows.
    let mut with_text = icon.clone();
    with_text.field_mut().set_text("abc");
    assert_eq!(with_text.text_rect(R), icon.text_rect(R));
    // A box narrower than its furniture still gives a rectangle that cannot be drawn in.
    let narrow = plain.text_rect(Rect::new(0.0, 0.0, 8.0, HEIGHT));
    assert_eq!(narrow.w, 0.0);
}

#[test]
fn pressing_the_clear_button_empties_the_field_and_the_text_area_does_not() {
    let mut f = filled("abc");
    let c = f.clear_rect(R).unwrap();
    let t = f.text_rect(R);
    // The text area places the caret and leaves the text alone.
    assert_eq!(f.press(R, t.cx(), t.cy()), Hit::Text);
    assert_eq!(f.text(), "abc");
    // The clear button empties it and puts the caret at the start.
    assert_eq!(f.press(R, c.cx(), c.cy()), Hit::Cleared);
    assert_eq!(f.text(), "");
    assert_eq!(f.field().caret(), (0, 0));
    // Off the box nothing happens.
    assert_eq!(f.press(R, R.right() + 1.0, R.cy()), Hit::Outside);
    assert_eq!(
        f.press(R, R.cx(), f.error_rect(R).map_or(R.bottom(), |b| b.cy())),
        Hit::Outside
    );
    // A press on the box of a field with no text is the text area, not a clear button.
    assert_eq!(f.press(R, c.cx(), c.cy()), Hit::Text);
    // Nonsense is outside.
    assert_eq!(f.press(R, f32::NAN, R.cy()), Hit::Outside);
}

#[test]
fn the_field_keeps_the_focus_after_the_clear_button() {
    for pal in [&LIGHT, &DARK] {
        let mut f = filled("abc");
        let focused = Look {
            focused: true,
            ..Look::default()
        };
        let before = drawn(pal, &mut f, focused, true);
        let c = f.clear_rect(R).unwrap();
        assert_eq!(f.press(R, c.cx(), c.cy()), Hit::Cleared);
        let after = drawn(pal, &mut f, focused, true);
        assert_ne!(
            before.pixels, after.pixels,
            "the clear button changed nothing"
        );
        // The focused, empty field still draws its caret at the start of the text area; an
        // unfocused one draws nothing there. That is the focus still being on the field.
        let area = f.text_rect(R);
        let mut same = FramedField::new();
        let unfocused = drawn(pal, &mut same, Look::default(), false);
        assert_eq!(
            count_in(&unfocused, pal.text, area),
            0,
            "an unfocused empty field drew ink"
        );
        assert!(
            count_in(&after, pal.text, area) > 0,
            "the caret went away with the focus"
        );
        assert_eq!(
            at(&after, area.x as usize, area.cy() as usize),
            pal.text,
            "the caret is not at the start"
        );
        assert_eq!(at(&after, area.cx() as usize, R.cy() as usize), pal.field);
    }
}

#[test]
fn an_error_turns_the_frame_and_draws_its_message_under_the_box() {
    for pal in [&LIGHT, &DARK] {
        let theme = if pal.dark { "dark" } else { "light" };
        let mut plain = FramedField::new();
        let clean = drawn(pal, &mut plain, Look::default(), false);
        let band = Rect::new(R.x, R.bottom(), R.w, ERROR_H);
        assert_eq!(
            count_in(&clean, pal.danger, band),
            0,
            "a field without an error drew a message in {theme}"
        );
        let mut e = FramedField::new();
        e.set_error("A name is needed");
        let with = drawn(pal, &mut e, Look::default(), false);
        assert_ne!(
            clean.pixels, with.pixels,
            "the error drew nothing in {theme}"
        );
        assert!(
            count_in(&with, pal.danger, band) > 0,
            "no red message in the band in {theme}"
        );
        // The frame itself is the danger colour.
        assert_eq!(
            at(&with, R.cx() as usize, R.y as usize),
            pal.danger,
            "the frame is not red in {theme}"
        );
        // Nothing was drawn below the error's band.
        assert_eq!(
            count_in(
                &with,
                pal.danger,
                Rect::new(0.0, band.bottom() + 1.0, PAGE_W as f32, 1.0)
            ),
            0
        );
    }
}

#[test]
fn the_focused_field_draws_the_accent_edge_and_a_line_along_the_bottom() {
    for pal in [&LIGHT, &DARK] {
        let theme = if pal.dark { "dark" } else { "light" };
        let mut f = filled("abc");
        let rest = drawn(pal, &mut f, Look::default(), true);
        let focused = drawn(
            pal,
            &mut f,
            Look {
                focused: true,
                ..Look::default()
            },
            true,
        );
        assert_ne!(rest.pixels, focused.pixels, "no focus mark in {theme}");
        assert_eq!(
            at(&focused, R.cx() as usize, R.y as usize),
            pal.accent,
            "the focused frame is not the accent in {theme}"
        );
        assert_eq!(
            at(&rest, R.cx() as usize, R.y as usize),
            mix(pal.line, pal.text, 0.3),
            "the resting frame is not the quiet edge in {theme}"
        );
        // The thicker bottom line sits just inside the frame.
        assert_eq!(
            at(&focused, R.cx() as usize, (R.bottom() - 2.0) as usize),
            pal.accent,
            "no accent line along the bottom in {theme}"
        );
    }
}

#[test]
fn escape_clears_only_when_it_is_turned_on_and_only_with_text() {
    let none = Mods::default();
    let mut f = filled("abc");
    assert!(!f.escape_clears(), "Escape clears by default");
    assert_eq!(f.key(&KeyName::Escape, none, None, false), Outcome::Unused);
    assert_eq!(f.text(), "abc", "Escape cleared an ordinary field");
    f.set_escape_clears(true);
    assert!(f.escape_clears());
    assert_eq!(f.key(&KeyName::Escape, none, None, false), Outcome::Cleared);
    assert!(f.is_empty());
    // An empty field's Escape is the caller's, so a dialog still closes.
    assert_eq!(f.key(&KeyName::Escape, none, None, false), Outcome::Unused);
    // Ctrl or Alt with Escape is somebody else's key, even with text.
    f.field_mut().set_text("x");
    for mods in [
        Mods {
            ctrl: true,
            ..Mods::default()
        },
        Mods {
            alt: true,
            ..Mods::default()
        },
    ] {
        assert_eq!(f.key(&KeyName::Escape, mods, None, false), Outcome::Unused);
        assert_eq!(f.text(), "x");
    }
}

#[test]
fn editing_keys_go_to_the_field_and_the_rest_is_the_callers() {
    let none = Mods::default();
    let mut f = FramedField::new();
    assert_eq!(
        f.key(&KeyName::Char('a'), none, Some("a"), false),
        Outcome::Used(true)
    );
    assert_eq!(f.text(), "a");
    assert_eq!(
        f.key(&KeyName::Char('b'), none, Some("b"), false),
        Outcome::Used(true)
    );
    assert_eq!(f.text(), "ab");
    assert_eq!(
        f.key(&KeyName::Left, none, None, false),
        Outcome::Used(false)
    );
    assert_eq!(f.field().caret(), (1, 1));
    assert_eq!(
        f.key(&KeyName::Backspace, none, None, false),
        Outcome::Used(true)
    );
    assert_eq!(f.text(), "b");
    // Enter, Tab and the arrows are the caller's.
    for k in [KeyName::Enter, KeyName::Tab, KeyName::Up, KeyName::Down] {
        assert_eq!(f.key(&k, none, None, false), Outcome::Unused, "{k:?}");
    }
    assert_eq!(f.text(), "b");
}

#[test]
fn the_field_shows_its_placeholder_while_empty_and_not_after() {
    for pal in [&LIGHT, &DARK] {
        // An empty field with no placeholder draws nothing but its frame: its text area, inside
        // the frame, is the field's own fill.
        let mut bare = FramedField::new();
        let quiet = drawn(pal, &mut bare, Look::default(), false);
        let inner = inner_of(bare.text_rect(R));
        assert_eq!(
            ink_in(&quiet, pal.field, inner),
            0,
            "an empty field with no placeholder drew ink"
        );
        let mut f = FramedField::new().with_placeholder("Search");
        let area = f.text_rect(R);
        let empty = drawn(pal, &mut f, Look::default(), false);
        let mut reference = blank(pal);
        let mut gfx = Gfx::new();
        let mut rf = FramedField::new();
        {
            let mut p = Painter::new(&mut reference, &mut gfx, 1.0);
            p.frame(R, RADIUS, pal.field, mix(pal.line, pal.text, 0.3));
            let colors = FieldColors {
                text: pal.text,
                placeholder: pal.muted,
                selection: pal.selection,
                caret: pal.text,
            };
            rf.field_mut().draw(
                &mut p,
                area,
                crate::gfx::Face::Ui,
                TEXT,
                &colors,
                "Search",
                false,
                false,
            );
        }
        assert_eq!(
            empty.pixels, reference.pixels,
            "the placeholder is not the muted colour, or is not drawn"
        );
        f.field_mut().set_text("abc");
        let filled = drawn(pal, &mut f, Look::default(), false);
        assert!(
            ink_in(&filled, pal.field, area) > 0,
            "the text is not drawn"
        );
        assert_ne!(
            empty.pixels, filled.pixels,
            "a filled field still shows its placeholder"
        );
    }
}

#[test]
fn a_disabled_field_is_quiet_and_offers_no_clear_button() {
    for pal in [&LIGHT, &DARK] {
        let mut f = filled("abc");
        let quiet = Look {
            disabled: true,
            ..Look::default()
        };
        let base = drawn(pal, &mut f, quiet, false);
        // The clear button takes no hover or press mark while the field is disabled.
        let loud = drawn(
            pal,
            &mut f,
            Look {
                disabled: true,
                hovered_clear: true,
                pressed_clear: true,
                focused: true,
            },
            true,
        );
        assert_eq!(loud.pixels, base.pixels, "a disabled field took a mark");
        // The text is the faint colour: a disabled field drawn by hand is the same picture.
        let area = f.text_rect(R);
        assert_eq!(count_in(&base, pal.text, area), 0, "the text is not faint");
        let mut reference = blank(pal);
        let mut gfx = Gfx::new();
        let mut rf = filled("abc");
        {
            let mut p = Painter::new(&mut reference, &mut gfx, 1.0);
            p.frame(R, RADIUS, pal.field, pal.line);
            let colors = FieldColors {
                text: pal.faint,
                placeholder: pal.faint,
                selection: pal.selection,
                caret: pal.faint,
            };
            rf.field_mut().draw(
                &mut p,
                area,
                crate::gfx::Face::Ui,
                TEXT,
                &colors,
                "",
                false,
                false,
            );
        }
        assert_eq!(base.pixels, reference.pixels);
        assert_eq!(at(&base, R.cx() as usize, R.y as usize), pal.line);
    }
}

#[test]
fn a_field_with_no_room_draws_nothing() {
    let page = blank(&LIGHT);
    let mut f = filled("abc");
    f.set_error("A name is needed");
    for r in [
        Rect::new(10.0, 10.0, 0.0, HEIGHT),
        Rect::new(10.0, 10.0, 80.0, 0.0),
        Rect::new(10.0, 10.0, -5.0, HEIGHT),
        Rect::new(f32::NAN, 10.0, 80.0, HEIGHT),
        Rect::new(10.0, f32::INFINITY, 80.0, HEIGHT),
        Rect::new(10.0, 10.0, f32::NAN, HEIGHT),
    ] {
        let mut bmp = page.clone();
        let mut gfx = Gfx::new();
        {
            let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
            draw(
                &mut p,
                &LIGHT,
                r,
                &mut f,
                Look {
                    focused: true,
                    hovered_clear: true,
                    pressed_clear: true,
                    disabled: false,
                },
                true,
            );
        }
        assert_eq!(bmp.pixels, page.pixels, "drew {r:?}");
    }
}
