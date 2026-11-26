//! Tests for [`crate::controls::button`]: the rectangle and the hit, the two keys, the hand
//! cursor, the pixels of each kind in each state, light and dark, and a sheet to look at.
//!
//! The sheet is `cargo test -p mdshared-ui controls_sheet -- --ignored`, which writes
//! `target/specimen/controls.png` and `controls-150.png`; a person opens them to judge the look.

use super::*;
use crate::gfx::{Gfx, Rect};
use crate::theme::{DARK, LIGHT};
use mdshared_text::Bitmap;

const PAGE_W: u32 = 200;
const PAGE_H: u32 = 64;

/// The box the pixel tests draw every button in, so a change of the label's width does not move
/// the pixels around.
const R: Rect = Rect::new(12.0, 12.0, 120.0, HEIGHT);

fn blank(pal: &Palette) -> Bitmap {
    let mut bmp = Bitmap::new(PAGE_W, PAGE_H);
    let mut gfx = Gfx::new();
    {
        let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
        p.fill(Rect::new(0.0, 0.0, PAGE_W as f32, PAGE_H as f32), pal.page);
    }
    bmp
}

fn drawn(pal: &Palette, kind: Kind, look: Look) -> Bitmap {
    let mut bmp = blank(pal);
    let mut gfx = Gfx::new();
    {
        let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
        draw(&mut p, pal, R, "Save", kind, look);
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

/// The states a button can be drawn in, by name, for the pairwise pixel test.
fn states() -> [(&'static str, Look); 6] {
    [
        ("rest", Look::default()),
        (
            "hover",
            Look {
                hovered: true,
                ..Look::default()
            },
        ),
        (
            "press",
            Look {
                hovered: true,
                pressed: true,
                ..Look::default()
            },
        ),
        (
            "focus",
            Look {
                focused: true,
                ..Look::default()
            },
        ),
        (
            "disabled",
            Look {
                disabled: true,
                ..Look::default()
            },
        ),
        (
            "default",
            Look {
                default: true,
                ..Look::default()
            },
        ),
    ]
}

#[test]
fn the_rectangle_is_as_wide_as_the_label_plus_its_padding() {
    let mut bmp = Bitmap::new(8, 8);
    let mut gfx = Gfx::new();
    let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
    let r = button_rect(&mut p, 10.0, 9.0, "Save", Kind::Secondary);
    assert_eq!((r.x, r.y, r.h), (10.0, 9.0, HEIGHT));
    assert_eq!(r.w, width(&mut p, "Save", Kind::Secondary));
    assert!(r.w > 2.0 * PAD);
    // A longer label is wider; an empty one has the padding only.
    assert!(button_rect(&mut p, 10.0, 9.0, "Save now", Kind::Secondary).w > r.w);
    assert_eq!(
        button_rect(&mut p, 10.0, 9.0, "", Kind::Secondary).w,
        2.0 * PAD
    );
    // The model asks the same questions and keeps its text and kind.
    let b = Button::new("Save", Kind::Secondary);
    assert_eq!(b.text(), "Save");
    assert_eq!(b.kind(), Kind::Secondary);
    assert_eq!(b.width(&mut p), r.w);
    assert_eq!(b.rect(&mut p, 10.0, 9.0), r);
    // The filled kinds are drawn semibold, as a dialog's primary button is, so a longer label
    // measured in that face still fits.
    let primary = Button::new("Save", Kind::Primary);
    assert!(primary.width(&mut p) >= b.width(&mut p));
    let d = Button::default();
    assert_eq!((d.text(), d.kind()), ("", Kind::Secondary));
    assert_eq!(d.width(&mut p), 2.0 * PAD);
}

#[test]
fn a_hit_is_inside_the_button_and_not_outside_it() {
    let r = Rect::new(10.0, 9.0, 40.0, HEIGHT);
    // The middle and the left and top edges.
    assert!(hit(r, 30.0, 20.0));
    assert!(hit(r, 10.0, 20.0));
    assert!(hit(r, 30.0, 9.0));
    // The right and bottom edges are outside, as `Rect::contains` has them.
    assert!(!hit(r, 50.0, 20.0));
    assert!(!hit(r, 30.0, 9.0 + HEIGHT));
    assert!(!hit(r, 9.9, 20.0));
    // Nonsense points are outside.
    assert!(!hit(r, f32::NAN, 20.0));
    assert!(!hit(r, 30.0, f32::INFINITY));
    assert!(!hit(r, f32::NEG_INFINITY, 20.0));
    // A button with no area cannot be pressed.
    assert!(!hit(Rect::new(10.0, 9.0, 0.0, HEIGHT), 10.0, 20.0));
    // The model answers the same.
    let b = Button::new("Save", Kind::Secondary);
    assert!(b.hit(r, 30.0, 20.0));
    assert!(!b.hit(r, 60.0, 20.0));
}

#[test]
fn enter_and_space_activate_a_button_and_the_other_keys_do_not() {
    assert!(key(&KeyName::Enter, Mods::default()));
    assert!(key(&KeyName::Space, Mods::default()));
    for other in [
        KeyName::Tab,
        KeyName::Escape,
        KeyName::Backspace,
        KeyName::Delete,
        KeyName::Home,
        KeyName::End,
        KeyName::Up,
        KeyName::Down,
        KeyName::Left,
        KeyName::Right,
        KeyName::Char('s'),
        KeyName::F(1),
    ] {
        assert!(
            !key(&other.clone(), Mods::default()),
            "{other:?} activated a button"
        );
    }
    // Ctrl or Alt with Enter or Space is somebody else's shortcut.
    for k in [KeyName::Enter, KeyName::Space] {
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
            assert!(!key(&k.clone(), mods), "{k:?} with {mods:?}");
        }
        assert!(key(
            &k.clone(),
            Mods {
                shift: true,
                ..Mods::default()
            }
        ));
    }
    let b = Button::new("Save", Kind::Secondary);
    assert!(b.key(&KeyName::Enter, Mods::default()));
    assert!(b.key(&KeyName::Space, Mods::default()));
    assert!(!b.key(&KeyName::Escape, Mods::default()));
}

#[test]
fn a_button_asks_for_the_hand_cursor() {
    assert_eq!(cursor(), Cursor::Hand);
    assert_eq!(Button::new("Save", Kind::Primary).cursor(), Cursor::Hand);
    assert_eq!(Button::default().cursor(), Cursor::Hand);
}

#[test]
fn every_kind_and_state_draws_differently_in_both_palettes() {
    for pal in [&LIGHT, &DARK] {
        let theme = if pal.dark { "dark" } else { "light" };
        for kind in Kind::ALL {
            let all = states();
            for i in 0..all.len() {
                for j in i + 1..all.len() {
                    assert_ne!(
                        drawn(pal, kind, all[i].1).pixels,
                        drawn(pal, kind, all[j].1).pixels,
                        "a {} button's {} and {} look the same in {theme}",
                        kind.name(),
                        all[i].0,
                        all[j].0
                    );
                }
            }
        }
    }
}

#[test]
fn the_four_kinds_differ_at_rest_in_both_palettes() {
    for pal in [&LIGHT, &DARK] {
        let rest: Vec<Vec<u8>> = Kind::ALL
            .iter()
            .map(|&k| drawn(pal, k, Look::default()).pixels)
            .collect();
        for i in 0..rest.len() {
            for j in i + 1..rest.len() {
                assert_ne!(
                    rest[i],
                    rest[j],
                    "{} and {} look the same in {}",
                    Kind::ALL[i].name(),
                    Kind::ALL[j].name(),
                    if pal.dark { "dark" } else { "light" }
                );
            }
        }
    }
}

#[test]
fn each_kind_is_filled_in_its_own_colour() {
    for pal in [&LIGHT, &DARK] {
        // A spot inside the fill, clear of the rounded corners and of the label.
        let (x, y) = ((R.x + 4.0) as usize, R.cy() as usize);
        let primary = drawn(pal, Kind::Primary, Look::default());
        assert_eq!(at(&primary, x, y), pal.accent, "primary is not the accent");
        let destructive = drawn(pal, Kind::Destructive, Look::default());
        assert_eq!(at(&destructive, x, y), pal.danger, "destructive is not red");
        let secondary = drawn(pal, Kind::Secondary, Look::default());
        assert_eq!(
            at(&secondary, x, y),
            mix(pal.popup, pal.text, 0.04),
            "secondary is not the quiet fill"
        );
        // Subtle paints nothing at rest: the page shows through.
        let subtle = drawn(pal, Kind::Subtle, Look::default());
        assert_eq!(at(&subtle, x, y), at(&blank(pal), x, y), "subtle is filled");
    }
}

#[test]
fn a_press_counts_only_while_the_pointer_is_on_the_button() {
    for pal in [&LIGHT, &DARK] {
        for kind in Kind::ALL {
            let rest = drawn(pal, kind, Look::default()).pixels;
            let hovered = drawn(
                pal,
                kind,
                Look {
                    hovered: true,
                    ..Look::default()
                },
            )
            .pixels;
            let pressed = drawn(
                pal,
                kind,
                Look {
                    hovered: true,
                    pressed: true,
                    ..Look::default()
                },
            )
            .pixels;
            let away = drawn(
                pal,
                kind,
                Look {
                    pressed: true,
                    ..Look::default()
                },
            )
            .pixels;
            let theme = if pal.dark { "dark" } else { "light" };
            assert_ne!(
                hovered,
                rest,
                "{} ignored the pointer in {theme}",
                kind.name()
            );
            assert_ne!(
                pressed,
                hovered,
                "{} ignored the press in {theme}",
                kind.name()
            );
            assert_eq!(
                away,
                rest,
                "{} stayed pressed with the pointer away in {theme}",
                kind.name()
            );
        }
    }
}

#[test]
fn the_keyboard_draws_the_ring_outside_the_button() {
    for pal in [&LIGHT, &DARK] {
        for kind in Kind::ALL {
            let rest = drawn(pal, kind, Look::default());
            let focused = drawn(
                pal,
                kind,
                Look {
                    focused: true,
                    ..Look::default()
                },
            );
            assert_ne!(
                rest.pixels,
                focused.pixels,
                "{} left no mark in {}",
                kind.name(),
                if pal.dark { "dark" } else { "light" }
            );
            let band = R.inset(-3.0);
            assert_eq!(
                count_in(&rest, pal.accent, band),
                count_in(&rest, pal.accent, R),
                "{} at rest drew outside its rectangle",
                kind.name()
            );
            assert!(
                count_in(&focused, pal.accent, band) > count_in(&focused, pal.accent, R),
                "{}'s ring is not outside it",
                kind.name()
            );
        }
    }
}

#[test]
fn a_disabled_button_is_the_same_quiet_frame_whatever_its_kind() {
    for pal in [&LIGHT, &DARK] {
        let quiet = Look {
            disabled: true,
            ..Look::default()
        };
        let base = drawn(pal, Kind::Secondary, quiet);
        for kind in Kind::ALL {
            assert_eq!(
                drawn(pal, kind, quiet).pixels,
                base.pixels,
                "a disabled {} differs from a disabled secondary",
                kind.name()
            );
            // No hover, press, focus or default mark is drawn on it.
            let loud = drawn(
                pal,
                kind,
                Look {
                    disabled: true,
                    hovered: true,
                    pressed: true,
                    focused: true,
                    default: true,
                },
            );
            assert_eq!(
                loud.pixels,
                base.pixels,
                "a disabled {} took a mark",
                kind.name()
            );
        }
        // The accent shows nowhere: no kind is promised while it cannot be used.
        assert_eq!(count_in(&base, pal.accent, R.inset(-3.0)), 0);
        // The label is the faint colour: a button-sized frame with the label in faint, drawn by
        // hand, is the same picture.
        let mut reference = Bitmap::new(PAGE_W, PAGE_H);
        let mut gfx = Gfx::new();
        {
            let mut p = Painter::new(&mut reference, &mut gfx, 1.0);
            p.fill(Rect::new(0.0, 0.0, PAGE_W as f32, PAGE_H as f32), pal.page);
            p.frame(R, RADIUS, pal.field, pal.line);
            p.text_center(crate::gfx::Face::Semibold, TEXT, R, "Save", pal.faint);
        }
        assert_eq!(
            base.pixels, reference.pixels,
            "a disabled button is not the quiet frame with a faint label"
        );
    }
}

#[test]
fn a_default_button_has_an_accent_line_and_primary_a_lighter_one() {
    for pal in [&LIGHT, &DARK] {
        // Both filled kinds take a lighter line on their own fill, which reads against it.
        for kind in [Kind::Primary, Kind::Destructive] {
            let b = drawn(
                pal,
                kind,
                Look {
                    default: true,
                    ..Look::default()
                },
            );
            let fill = style(pal, kind).fill.unwrap();
            let line = mix(pal.on_accent, fill, 0.5);
            assert!(
                count_in(&b, line, R) > 0,
                "a default {} has no lighter line",
                kind.name()
            );
        }
        // A destructive default draws no accent: the mark follows the danger fill.
        let destructive = drawn(
            pal,
            Kind::Destructive,
            Look {
                default: true,
                ..Look::default()
            },
        );
        assert_eq!(
            count_in(&destructive, pal.accent, R.inset(-3.0)),
            0,
            "a default destructive drew the accent"
        );
        // The other kinds mark the default in the accent just inside their edge.
        for kind in [Kind::Secondary, Kind::Subtle] {
            let b = drawn(
                pal,
                kind,
                Look {
                    default: true,
                    ..Look::default()
                },
            );
            assert!(
                count_in(&b, pal.accent, R) > 0,
                "a default {} has no accent line",
                kind.name()
            );
        }
    }
}

#[test]
fn a_button_with_no_room_draws_nothing() {
    let page = blank(&LIGHT);
    for r in [
        Rect::new(10.0, 10.0, 0.0, HEIGHT),
        Rect::new(10.0, 10.0, 40.0, 0.0),
        Rect::new(10.0, 10.0, -5.0, HEIGHT),
        Rect::new(f32::NAN, 10.0, 40.0, HEIGHT),
        Rect::new(10.0, f32::INFINITY, 40.0, HEIGHT),
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
                "Save",
                Kind::Primary,
                Look {
                    hovered: true,
                    pressed: true,
                    focused: true,
                    ..Look::default()
                },
            );
        }
        assert_eq!(bmp.pixels, page.pixels, "drew {r:?}");
    }
    // An empty label still draws its box, and no text.
    let empty = {
        let mut bmp = blank(&LIGHT);
        let mut gfx = Gfx::new();
        let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
        draw(&mut p, &LIGHT, R, "", Kind::Primary, Look::default());
        bmp
    };
    assert_eq!(
        at(&empty, (R.x + 4.0) as usize, R.cy() as usize),
        LIGHT.accent
    );
}

#[test]
fn the_destructive_label_reads_against_the_red_in_both_palettes() {
    for pal in [&LIGHT, &DARK] {
        assert!(
            crate::cover::contrast(ON_DANGER, pal.danger) >= 4.5,
            "the destructive label is below 4.5:1 in {}",
            if pal.dark { "dark" } else { "light" }
        );
    }
}

/// The sheet a person looks at: four kinds in six states, light and dark, at scale 1 and 1.5.
/// `cargo test -p mdshared-ui controls_sheet -- --ignored`, then open
/// `target/specimen/controls.png` and `controls-150.png`.
#[test]
#[ignore]
fn controls_sheet() {
    use crate::controls::{field, menu_button};
    let (w, h) = (560.0, 720.0);
    for (scale, file) in [(1.0, "controls.png"), (1.5, "controls-150.png")] {
        let mut bmp = Bitmap::new((w * scale) as u32, (h * scale) as u32);
        let mut gfx = Gfx::new();
        let mut p = Painter::new(&mut bmp, &mut gfx, scale);
        for (half, pal) in [&LIGHT, &DARK].into_iter().enumerate() {
            let oy = half as f32 * (h / 2.0);
            p.fill(Rect::new(0.0, oy, w, h / 2.0), pal.page);
            let old = p.offset(0.0, oy);
            // The four kinds, each in the six states as one row.
            let mut y = 12.0;
            for kind in Kind::ALL {
                for (k, (_, look)) in states().into_iter().enumerate() {
                    let r = Rect::new(12.0 + 84.0 * k as f32, y, 80.0, HEIGHT);
                    draw(&mut p, pal, r, kind.name(), kind, look);
                }
                y += HEIGHT + 6.0;
            }
            // The icon button, the menu button and the split button.
            let icon = Rect::new(12.0, y + 8.0, 32.0, HEIGHT);
            p.rbox(icon, 5.0, pal.hover);
            p.icon(crate::glyph::SEARCH, 16.0, icon.cx(), icon.cy(), pal.text);
            let mb = menu_button::MenuButton::new("Sort by", Kind::Secondary);
            let mbr = mb.rect(&mut p, 60.0, y + 8.0);
            menu_button::draw(&mut p, pal, mbr, mb.text(), mb.kind(), Look::default());
            let sb = menu_button::SplitButton::new("Run", Kind::Primary);
            let sbr = sb.rect(&mut p, 220.0, y + 8.0);
            menu_button::draw_split(
                &mut p,
                pal,
                sbr,
                sb.text(),
                sb.kind(),
                menu_button::SplitLook::default(),
            );
            // The framed field: empty with its icon, holding text, and in an error.
            let mut f = field::FramedField::new()
                .with_icon(crate::glyph::SEARCH)
                .with_placeholder("Search");
            let fr = Rect::new(12.0, y + 50.0, 240.0, field::HEIGHT);
            field::draw(&mut p, pal, fr, &mut f, field::Look::default(), false);
            let mut g = field::FramedField::new();
            g.field_mut().set_text("release notes");
            field::draw(
                &mut p,
                pal,
                Rect::new(270.0, y + 50.0, 240.0, field::HEIGHT),
                &mut g,
                field::Look {
                    focused: true,
                    ..field::Look::default()
                },
                true,
            );
            let mut e = field::FramedField::new();
            e.set_error("A name is needed");
            field::draw(
                &mut p,
                pal,
                Rect::new(12.0, y + 110.0, 300.0, field::HEIGHT),
                &mut e,
                field::Look::default(),
                false,
            );
            p.restore_offset(old);
        }
        let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/specimen");
        std::fs::create_dir_all(&out).unwrap();
        bmp.write_png(&out.join(file)).unwrap();
    }
}
