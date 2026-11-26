//! Tests for [`crate::controls::button`]: the rectangle and the hit, the keys, the hand cursor,
//! the pixels of each kind in each state in the four palettes, exact colours at key pixels,
//! contrast, one-device-pixel lines at fractional scales, and a sheet to look at.
//!
//! The sheet is `cargo test -p mdshared-ui controls_sheet -- --ignored`, which writes
//! `target/specimen/controls.png`, `controls-125.png` and `controls-150.png`; a person opens them
//! to judge the look.

use super::*;
use crate::gfx::{Face, Gfx, Rect};
use crate::theme::{DARK, LIGHT};
use crate::tokens::{HIGH_CONTRAST_DARK, HIGH_CONTRAST_LIGHT};
use mdshared_text::Bitmap;

const PAGE_W: u32 = 200;
const PAGE_H: u32 = 64;

/// The box the pixel tests draw every button in, so a change of the label's width does not move
/// the pixels around.
const R: Rect = Rect::new(12.0, 12.0, 120.0, HEIGHT);

/// The four palettes every pixel and contrast test runs over.
fn palettes() -> [(&'static str, &'static Palette); 4] {
    [
        ("light", &LIGHT),
        ("dark", &DARK),
        ("high-contrast light", &HIGH_CONTRAST_LIGHT),
        ("high-contrast dark", &HIGH_CONTRAST_DARK),
    ]
}

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

/// The same drawing at `scale`, on a page `scale` times as large, for the device-pixel tests.
fn drawn_at(pal: &Palette, kind: Kind, look: Look, scale: f32) -> Bitmap {
    let mut bmp = Bitmap::new(
        (PAGE_W as f32 * scale) as u32,
        (PAGE_H as f32 * scale) as u32,
    );
    let area = Rect::new(0.0, 0.0, bmp.width as f32, bmp.height as f32);
    let mut gfx = Gfx::new();
    {
        let mut p = Painter::new(&mut bmp, &mut gfx, scale);
        p.fill(area, pal.page);
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

/// Whether two drawings hold the same pixels inside `area`.
fn same_in(a: &Bitmap, b: &Bitmap, area: Rect) -> bool {
    (0..a.height as usize).all(|y| {
        (0..a.width as usize)
            .all(|x| !area.contains(x as f32, y as f32) || at(a, x, y) == at(b, x, y))
    })
}

/// A spot inside the fill, clear of the rounded corners and of the label.
fn spot() -> (usize, usize) {
    ((R.x + 4.0) as usize, R.cy() as usize)
}

fn hovered() -> Look {
    Look {
        hovered: true,
        ..Look::default()
    }
}

fn pressed() -> Look {
    Look {
        hovered: true,
        pressed: true,
        ..Look::default()
    }
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
    // The filled kinds are drawn semibold, as a dialog's primary button is.
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
    // Ctrl, Alt or the Mac's Control with Enter or Space is somebody else's shortcut.
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
            Mods {
                control: true,
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
fn every_kind_and_state_draws_differently_in_every_palette() {
    // Pressed and open are the same fill by design: a menu opened from the keyboard must look
    // pressed whatever the pointer does, and `an_open_button_is_drawn_pressed` checks that.
    let states = [
        ("rest", Look::default()),
        ("hover", hovered()),
        ("press", pressed()),
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
    ];
    for (name, pal) in palettes() {
        for kind in Kind::ALL {
            for i in 0..states.len() {
                for j in i + 1..states.len() {
                    assert_ne!(
                        drawn(pal, kind, states[i].1).pixels,
                        drawn(pal, kind, states[j].1).pixels,
                        "a {} button's {} and {} look the same in {name}",
                        kind.name(),
                        states[i].0,
                        states[j].0
                    );
                }
            }
        }
    }
}

#[test]
fn the_four_kinds_differ_at_rest_in_every_palette() {
    for (name, pal) in palettes() {
        let rest: Vec<Vec<u8>> = Kind::ALL
            .iter()
            .map(|&k| drawn(pal, k, Look::default()).pixels)
            .collect();
        for i in 0..rest.len() {
            for j in i + 1..rest.len() {
                assert_ne!(
                    rest[i],
                    rest[j],
                    "{} and {} look the same in {name}",
                    Kind::ALL[i].name(),
                    Kind::ALL[j].name()
                );
            }
        }
    }
}

#[test]
fn each_kind_and_state_is_filled_in_its_exact_colour() {
    // The fill rules written out again, so a change to one is caught here as well as in the
    // drawing: rest, hover and press fills of each kind, and the two filled kinds' inks.
    let fills = |pal: &Palette, kind: Kind| -> (Option<Rgb>, Rgb, Rgb, Rgb) {
        match kind {
            Kind::Primary => (
                Some(pal.accent),
                mix(pal.accent, pal.popup, 0.1),
                mix(pal.accent, pal.popup, 0.2),
                pal.on_accent,
            ),
            Kind::Secondary => (
                Some(mix(pal.popup, pal.text, 0.04)),
                pal.hover,
                pal.press,
                pal.text,
            ),
            Kind::Subtle => (None, pal.hover, pal.press, pal.text),
            Kind::Destructive if high_contrast(pal, false) => {
                (Some(pal.page), pal.hover, pal.press, pal.text)
            }
            Kind::Destructive => {
                let ink = destructive_ink(pal, pal.danger);
                let away = shade(pal, ink);
                (
                    Some(pal.danger),
                    mix(pal.danger, away, 0.1),
                    mix(pal.danger, away, 0.2),
                    ink,
                )
            }
        }
    };
    for (name, pal) in palettes() {
        for kind in Kind::ALL {
            let (rest, hover, press, _) = fills(pal, kind);
            let (x, y) = spot();
            if let Some(c) = rest {
                assert_eq!(
                    at(&drawn(pal, kind, Look::default()), x, y),
                    c,
                    "{} at rest in {name}",
                    kind.name()
                );
            }
            assert_eq!(
                at(&drawn(pal, kind, hovered()), x, y),
                hover,
                "{} hovered in {name}",
                kind.name()
            );
            assert_eq!(
                at(&drawn(pal, kind, pressed()), x, y),
                press,
                "{} pressed in {name}",
                kind.name()
            );
            assert_eq!(
                at(
                    &drawn(
                        pal,
                        kind,
                        Look {
                            open: true,
                            ..Look::default()
                        }
                    ),
                    x,
                    y
                ),
                press,
                "{} open in {name}",
                kind.name()
            );
        }
        // The destructive ink is the one chosen for that palette, at a pixel inside a letter
        // that is solid: the glyph is antialiased, so check the rule instead of a pixel.
        let ink = destructive_ink(pal, pal.danger);
        assert!(crate::cover::contrast(ink, pal.danger) >= 4.5);
        // A secondary button's edge is the popup line at rest.
        let b = drawn(pal, Kind::Secondary, Look::default());
        assert_eq!(at(&b, R.cx() as usize, R.y as usize), pal.popup_line);
    }
}

#[test]
fn an_open_button_is_drawn_pressed_wherever_the_pointer_is() {
    for (name, pal) in palettes() {
        for kind in Kind::ALL {
            let open = drawn(
                pal,
                kind,
                Look {
                    open: true,
                    ..Look::default()
                },
            );
            assert_eq!(
                open.pixels,
                drawn(pal, kind, pressed()).pixels,
                "an open {} is not drawn pressed in {name}",
                kind.name()
            );
            assert_ne!(
                open.pixels,
                drawn(pal, kind, Look::default()).pixels,
                "an open {} is at rest in {name}",
                kind.name()
            );
            assert_ne!(
                open.pixels,
                drawn(pal, kind, hovered()).pixels,
                "an open {} is only hovered in {name}",
                kind.name()
            );
        }
    }
}

#[test]
fn a_press_counts_only_while_the_pointer_is_on_the_button() {
    for (name, pal) in palettes() {
        for kind in Kind::ALL {
            let rest = drawn(pal, kind, Look::default()).pixels;
            let hover = drawn(pal, kind, hovered()).pixels;
            let press = drawn(pal, kind, pressed()).pixels;
            let away = drawn(
                pal,
                kind,
                Look {
                    pressed: true,
                    ..Look::default()
                },
            )
            .pixels;
            assert_ne!(hover, rest, "{} ignored the pointer in {name}", kind.name());
            assert_ne!(press, hover, "{} ignored the press in {name}", kind.name());
            assert_eq!(
                away,
                rest,
                "{} stayed pressed with the pointer away in {name}",
                kind.name()
            );
        }
    }
}

#[test]
fn the_keyboard_draws_the_ring_outside_the_button() {
    for (name, pal) in palettes() {
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
                "{} left no mark in {name}",
                kind.name()
            );
            let band = R.inset(-3.0);
            assert_eq!(
                count_in(&rest, pal.accent, band),
                count_in(&rest, pal.accent, R),
                "{} at rest drew outside its rectangle in {name}",
                kind.name()
            );
            assert!(
                count_in(&focused, pal.accent, band) > count_in(&focused, pal.accent, R),
                "{}'s ring is not outside it in {name}",
                kind.name()
            );
        }
    }
}

#[test]
fn a_disabled_button_is_the_same_quiet_frame_whatever_its_kind() {
    for (name, pal) in palettes() {
        let quiet = Look {
            disabled: true,
            ..Look::default()
        };
        let base = drawn(pal, Kind::Secondary, quiet);
        for kind in Kind::ALL {
            assert_eq!(
                drawn(pal, kind, quiet).pixels,
                base.pixels,
                "a disabled {} differs from a disabled secondary in {name}",
                kind.name()
            );
            // No hover, press, open, focus or default mark is drawn on it.
            let loud = drawn(
                pal,
                kind,
                Look {
                    disabled: true,
                    hovered: true,
                    pressed: true,
                    open: true,
                    focused: true,
                    default: true,
                    high_contrast: true,
                },
            );
            assert_eq!(
                loud.pixels,
                base.pixels,
                "a disabled {} took a mark in {name}",
                kind.name()
            );
        }
        // The accent shows nowhere: no kind is promised while it cannot be used.
        assert_eq!(count_in(&base, pal.accent, R.inset(-3.0)), 0);
        // The label is the faint colour, in semibold, as the dialog's disabled button draws it.
        let mut reference = Bitmap::new(PAGE_W, PAGE_H);
        let mut gfx = Gfx::new();
        {
            let mut p = Painter::new(&mut reference, &mut gfx, 1.0);
            p.fill(Rect::new(0.0, 0.0, PAGE_W as f32, PAGE_H as f32), pal.page);
            p.frame(R, RADIUS, pal.field, pal.line);
            p.text_center(Face::Semibold, TEXT, R, "Save", pal.faint);
        }
        assert_eq!(
            base.pixels, reference.pixels,
            "a disabled button is not the quiet frame with a faint label in {name}"
        );
    }
}

#[test]
fn a_default_button_marks_its_edge_in_the_accent_or_the_label_ink() {
    for (name, pal) in palettes() {
        for kind in Kind::ALL {
            let b = drawn(
                pal,
                kind,
                Look {
                    default: true,
                    ..Look::default()
                },
            );
            let rest = drawn(pal, kind, Look::default());
            assert_ne!(rest.pixels, b.pixels, "{} has no default mark", kind.name());
            let ink = style(pal, kind, high_contrast(pal, false)).ink;
            let line = default_line(pal, kind, ink);
            assert!(
                count_in(&b, line, R) > 0,
                "a default {} has no {} line in {name}",
                kind.name(),
                if line == pal.accent { "accent" } else { "ink" }
            );
            if matches!(kind, Kind::Primary | Kind::Destructive) {
                // The filled kinds' mark is their label's ink, which reads on the fill.
                assert_eq!(line, ink, "the default {} line is not its ink", kind.name());
            }
        }
    }
}

#[test]
fn a_default_line_reads_against_its_fill_and_is_one_device_pixel_wide() {
    for (name, pal) in palettes() {
        for kind in Kind::ALL {
            let ink = style(pal, kind, high_contrast(pal, false)).ink;
            let line = default_line(pal, kind, ink);
            if kind != Kind::Subtle {
                let fill = style(pal, kind, high_contrast(pal, false))
                    .fill
                    .unwrap_or(pal.field);
                assert!(
                    crate::cover::contrast(line, fill) >= 3.0,
                    "a default {} line is under 3:1 in {name}",
                    kind.name()
                );
            }
        }
        // At a fractional scale the line is still one device pixel: scan the column through the
        // middle of the top edge and count the line's pixels.
        for scale in [1.0f32, 1.25, 1.5] {
            let b = drawn_at(
                pal,
                Kind::Secondary,
                Look {
                    default: true,
                    ..Look::default()
                },
                scale,
            );
            let line = pal.accent;
            let column = (R.cx() * scale).floor() as usize;
            let mut run = 0;
            for y in 0..b.height as usize {
                if at(&b, column, y) == line {
                    run += 1;
                } else if run > 0 {
                    break;
                }
            }
            assert_eq!(
                run, 1,
                "the default line is {run} device pixels at scale {scale} in {name}"
            );
        }
    }
}

#[test]
fn the_destructive_label_reads_against_its_fill_in_every_state() {
    for (name, pal) in palettes() {
        let hc = high_contrast(pal, false);
        let s = style(pal, Kind::Destructive, hc);
        for (state, fill) in [
            ("rest", s.fill),
            ("hover", Some(s.hover)),
            ("press", Some(s.press)),
        ] {
            let fill = fill.expect("destructive has a fill");
            assert!(
                crate::cover::contrast(s.ink, fill) >= 4.5,
                "the destructive label is {:.2}:1 on its {state} fill in {name}",
                crate::cover::contrast(s.ink, fill)
            );
        }
        // Under high contrast the destructive colours are the palette's own, unmixed.
        if hc {
            assert_eq!(s.fill, Some(pal.page));
            assert_eq!(s.ink, pal.text);
            assert_eq!(s.border, Some(pal.danger));
            assert_eq!((s.hover, s.press), (pal.hover, pal.press));
        }
    }
}

#[test]
fn the_four_palettes_are_told_apart_and_the_high_contrast_flag_is_honoured() {
    // The two toolkit palettes are high contrast by identity; any palette is with the flag.
    assert!(high_contrast(&HIGH_CONTRAST_LIGHT, false));
    assert!(high_contrast(&HIGH_CONTRAST_DARK, false));
    assert!(!high_contrast(&LIGHT, false));
    assert!(!high_contrast(&DARK, false));
    assert!(high_contrast(&LIGHT, true));
    // The flag changes the destructive drawing on a normal palette, and only that kind.
    let loud = drawn(
        &DARK,
        Kind::Destructive,
        Look {
            high_contrast: true,
            ..Look::default()
        },
    );
    assert_ne!(
        loud.pixels,
        drawn(&DARK, Kind::Destructive, Look::default()).pixels
    );
    // The flagged dark palette draws the high-contrast way: the page as its fill.
    let (x, y) = spot();
    assert_eq!(at(&loud, x, y), DARK.page);
    // Only destructive changes: a primary button is the same either way.
    let flagged = drawn(
        &LIGHT,
        Kind::Primary,
        Look {
            high_contrast: true,
            ..Look::default()
        },
    );
    let plain = drawn(&LIGHT, Kind::Primary, Look::default());
    assert!(
        same_in(&flagged, &plain, R),
        "the flag moved a primary button"
    );
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
                    open: true,
                    focused: true,
                    default: true,
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

/// The sheet a person looks at: four kinds in seven states, the icon, menu and split buttons and
/// three framed fields, in the four palettes, at scales 1, 1.25 and 1.5.
/// `cargo test -p mdshared-ui controls_sheet -- --ignored`, then open
/// `target/specimen/controls.png`, `controls-125.png` and `controls-150.png`.
#[test]
#[ignore]
fn controls_sheet() {
    use crate::controls::{field, icon_button, menu_button};
    let (w, h) = (560.0, 1200.0);
    let states = [
        ("rest", Look::default()),
        ("hover", hovered()),
        ("press", pressed()),
        (
            "open",
            Look {
                open: true,
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
    ];
    for (scale, file) in [
        (1.0, "controls.png"),
        (1.25, "controls-125.png"),
        (1.5, "controls-150.png"),
    ] {
        let mut bmp = Bitmap::new((w * scale) as u32, (h * scale) as u32);
        let mut gfx = Gfx::new();
        let mut p = Painter::new(&mut bmp, &mut gfx, scale);
        for (panel, (_, pal)) in palettes().into_iter().enumerate() {
            let oy = panel as f32 * (h / 4.0);
            p.fill(Rect::new(0.0, oy, w, h / 4.0), pal.page);
            let old = p.offset(0.0, oy);
            let mut y = 8.0;
            for kind in Kind::ALL {
                for (k, (_, look)) in states.iter().enumerate() {
                    let r = Rect::new(8.0 + 78.0 * k as f32, y, 74.0, HEIGHT);
                    draw(&mut p, pal, r, kind.name(), kind, *look);
                }
                y += HEIGHT + 4.0;
            }
            let icon = icon_button::IconButton::new(crate::glyph::SEARCH, "Search");
            let ir = icon.rect(8.0, y + 6.0);
            icon_button::draw(
                &mut p,
                pal,
                ir,
                icon.icon(),
                Look {
                    hovered: true,
                    ..Look::default()
                },
            );
            let mb = menu_button::MenuButton::new("Sort by", Kind::Secondary);
            let mbr = mb.rect(&mut p, 48.0, y + 6.0);
            menu_button::draw(
                &mut p,
                pal,
                mbr,
                mb.text(),
                mb.kind(),
                Look {
                    open: true,
                    ..Look::default()
                },
            );
            let sb = menu_button::SplitButton::new("Run", Kind::Primary);
            let sbr = sb.rect(&mut p, 200.0, y + 6.0);
            menu_button::draw_split(
                &mut p,
                pal,
                sbr,
                sb.text(),
                sb.kind(),
                menu_button::SplitLook {
                    chevron: Look {
                        open: true,
                        ..Look::default()
                    },
                    ..menu_button::SplitLook::default()
                },
            );
            let mut f = field::FramedField::new()
                .with_icon(crate::glyph::SEARCH)
                .with_placeholder("Search");
            field::draw(
                &mut p,
                pal,
                Rect::new(8.0, y + 46.0, 230.0, field::HEIGHT),
                &mut f,
                field::Look::default(),
                false,
            );
            let mut g = field::FramedField::new();
            g.field_mut().set_text("release notes");
            field::draw(
                &mut p,
                pal,
                Rect::new(260.0, y + 46.0, 230.0, field::HEIGHT),
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
                Rect::new(8.0, y + 100.0, 300.0, field::HEIGHT),
                &mut e,
                field::Look {
                    high_contrast: high_contrast(pal, false),
                    ..field::Look::default()
                },
                false,
            );
            p.restore_offset(old);
        }
        let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/specimen");
        std::fs::create_dir_all(&out).unwrap();
        bmp.write_png(&out.join(file)).unwrap();
    }
}
