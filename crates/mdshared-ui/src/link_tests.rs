//! Tests for [`crate::link`]: the rectangle and the hit, the two keys, the hand cursor, the
//! pixels of the colours and the underline, the focus ring, and a sheet to look at.
//!
//! The sheet is `cargo test -p mdshared-ui link_sheet -- --ignored`, which writes
//! `target/specimen/link.png` and `link-150.png`; a person opens them to judge the look.

use super::*;
use crate::gfx::{Face, Gfx, Rect};
use crate::keys::KeyName;
use crate::theme::{DARK, LIGHT};
use mdshared_text::Bitmap;

const PAGE_W: f32 = 220.0;
const PAGE_H: f32 = 48.0;
const AT: f32 = 12.0;
const DOWN: f32 = 10.0;

fn painter<'a>(bmp: &'a mut Bitmap, gfx: &'a mut Gfx) -> Painter<'a> {
    Painter::new(bmp, gfx, 1.0)
}

/// A page with one link drawn on it, and the rectangle the link asked for.
fn drawn(pal: &Palette, text: &str, look: Look) -> (Bitmap, Rect) {
    let mut bmp = Bitmap::new(PAGE_W as u32, PAGE_H as u32);
    let mut gfx = Gfx::new();
    let r = {
        let mut p = painter(&mut bmp, &mut gfx);
        p.fill(Rect::new(0.0, 0.0, PAGE_W, PAGE_H), pal.page);
        let r = link_rect(&mut p, AT, DOWN, text);
        draw_link(&mut p, pal, r, text, look);
        r
    };
    (bmp, r)
}

/// A page with `text` drawn in `colour` at the link's place, and no underline or ring: what a
/// link with that colour and no marks must draw.
fn text_only(pal: &Palette, r: Rect, text: &str, colour: [u8; 3]) -> Bitmap {
    let mut bmp = Bitmap::new(PAGE_W as u32, PAGE_H as u32);
    let mut gfx = Gfx::new();
    {
        let mut p = painter(&mut bmp, &mut gfx);
        p.fill(Rect::new(0.0, 0.0, PAGE_W, PAGE_H), pal.page);
        p.text(Face::Ui, LINK_TEXT, r.x, r.cy(), text, colour, r.w);
    }
    bmp
}

/// The same page with nothing drawn on it.
fn blank(pal: &Palette) -> Bitmap {
    let mut bmp = Bitmap::new(PAGE_W as u32, PAGE_H as u32);
    let mut gfx = Gfx::new();
    {
        let mut p = painter(&mut bmp, &mut gfx);
        p.fill(Rect::new(0.0, 0.0, PAGE_W, PAGE_H), pal.page);
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

/// The longest horizontal run of pixels that hold exactly `c`, anywhere on the page.
fn longest_run(bmp: &Bitmap, c: [u8; 3]) -> usize {
    let mut best = 0;
    for y in 0..bmp.height as usize {
        let mut run = 0;
        for x in 0..bmp.width as usize {
            if at(bmp, x, y) == c {
                run += 1;
                best = best.max(run);
            } else {
                run = 0;
            }
        }
    }
    best
}

/// Whether every pixel outside `r` is the page's own colour.
fn nothing_outside(bmp: &Bitmap, pal: &Palette, r: Rect) -> bool {
    let page = blank(pal);
    (0..bmp.height as usize).all(|y| {
        (0..bmp.width as usize)
            .all(|x| r.contains(x as f32, y as f32) || at(bmp, x, y) == at(&page, x, y))
    })
}

#[test]
fn the_rectangle_is_as_wide_as_the_text_and_the_link_height_tall() {
    let mut bmp = Bitmap::new(8, 8);
    let mut gfx = Gfx::new();
    let mut p = painter(&mut bmp, &mut gfx);
    let r = link_rect(&mut p, 10.0, 9.0, "Open the file");
    assert_eq!((r.x, r.y, r.h), (10.0, 9.0, LINK_H));
    assert_eq!(r.w, measure(&mut p, "Open the file"));
    assert!(r.w > 0.0);
    // Longer text is wider; empty text has no width at all.
    assert!(link_rect(&mut p, 10.0, 9.0, "Open the file today").w > r.w);
    assert_eq!(link_rect(&mut p, 10.0, 9.0, "").w, 0.0);
    // The model asks the same questions and keeps its text.
    let link = Link::new("Open the file");
    assert_eq!(link.text(), "Open the file");
    assert_eq!(link.measure(&mut p), r.w);
    assert_eq!(link.rect(&mut p, 10.0, 9.0), r);
    assert_eq!(Link::default().text(), "");
}

#[test]
fn a_hit_is_inside_the_rectangle_and_not_outside_it() {
    let r = Rect::new(10.0, 9.0, 40.0, 22.0);
    // The middle, the left and top edges.
    assert!(hit(r, 30.0, 20.0));
    assert!(hit(r, 10.0, 20.0));
    assert!(hit(r, 30.0, 9.0));
    // The right and bottom edges are outside, as `Rect::contains` has them.
    assert!(!hit(r, 50.0, 20.0));
    assert!(!hit(r, 30.0, 31.0));
    assert!(!hit(r, 9.9, 20.0));
    assert!(!hit(r, 30.0, 8.9));
    // Nonsense points are outside.
    assert!(!hit(r, f32::NAN, 20.0));
    assert!(!hit(r, 30.0, f32::INFINITY));
    assert!(!hit(r, f32::NEG_INFINITY, 20.0));
    // A link with no text has no area to press.
    let empty = Rect::new(10.0, 9.0, 0.0, LINK_H);
    assert!(!hit(empty, 10.0, 20.0));
    // The model answers the same.
    assert!(Link::new("Open").hit(r, 30.0, 20.0));
    assert!(!Link::new("Open").hit(r, 60.0, 20.0));
}

#[test]
fn enter_and_space_activate_a_link_and_the_other_keys_do_not() {
    assert!(key(KeyName::Enter));
    assert!(key(KeyName::Space));
    for other in [
        KeyName::Tab,
        KeyName::Escape,
        KeyName::Backspace,
        KeyName::Delete,
        KeyName::Home,
        KeyName::End,
        KeyName::PageUp,
        KeyName::PageDown,
        KeyName::Up,
        KeyName::Down,
        KeyName::Left,
        KeyName::Right,
        KeyName::Char('e'),
        KeyName::F(1),
    ] {
        assert!(!key(other.clone()), "{other:?} activated a link");
    }
    let link = Link::new("Open");
    assert!(link.key(KeyName::Enter));
    assert!(link.key(KeyName::Space));
    assert!(!link.key(KeyName::Escape));
}

#[test]
fn a_link_asks_for_the_hand_cursor() {
    assert_eq!(cursor(), Cursor::Hand);
    assert_eq!(Link::new("Open").cursor(), Cursor::Hand);
    assert_eq!(Link::default().cursor(), Cursor::Hand);
}

#[test]
fn the_text_is_drawn_in_the_link_colour() {
    for pal in [&LIGHT, &DARK] {
        let (bmp, r) = drawn(pal, "Open the file", Look::default());
        // The accent is the palette's link colour. A link at rest is that text and nothing
        // else: no underline, no ring, no fill behind it.
        assert_eq!(
            bmp.pixels,
            text_only(pal, r, "Open the file", pal.accent).pixels,
            "the link is not drawn in the accent"
        );
        // Ordinary text in the same place is a different picture, so the link is not drawn in
        // the text colour.
        assert_ne!(
            bmp.pixels,
            text_only(pal, r, "Open the file", pal.text).pixels,
            "the link is in the text colour"
        );
        // And at rest there is no underline: no long run of the accent anywhere.
        assert!(longest_run(&bmp, pal.accent) < (r.w * 0.5) as usize);
    }
}

#[test]
fn the_underline_is_there_when_hovered_or_pressed_and_absent_when_not() {
    for pal in [&LIGHT, &DARK] {
        let (rest, r) = drawn(pal, "Open the file", Look::default());
        let (over, _) = drawn(
            pal,
            "Open the file",
            Look {
                hovered: true,
                ..Look::default()
            },
        );
        let (down, _) = drawn(
            pal,
            "Open the file",
            Look {
                pressed: true,
                ..Look::default()
            },
        );
        let wide = (r.w * 0.8) as usize;
        assert!(longest_run(&rest, pal.accent) < (r.w * 0.5) as usize);
        assert!(
            longest_run(&over, pal.accent) >= wide,
            "no underline under the pointer"
        );
        assert!(
            longest_run(&down, mix(pal.accent, pal.text, PRESS)) >= wide,
            "no underline while pressed"
        );
        // Pressing deepens the accent, so the two pictures differ.
        assert_ne!(over.pixels, down.pixels, "a press looks like a hover");
    }
}

#[test]
fn the_keyboard_draws_the_ring_outside_the_link() {
    for pal in [&LIGHT, &DARK] {
        let (rest, r) = drawn(pal, "Open", Look::default());
        let (focused, _) = drawn(
            pal,
            "Open",
            Look {
                focused: true,
                ..Look::default()
            },
        );
        assert_ne!(rest.pixels, focused.pixels, "the keyboard left no mark");
        let band = Rect::new(r.x - 3.0, r.y - 3.0, r.w + 6.0, r.h + 6.0);
        assert_eq!(
            count_in(&rest, pal.accent, band),
            count_in(&rest, pal.accent, r),
            "a link at rest drew outside its rectangle"
        );
        assert!(
            count_in(&focused, pal.accent, band) > count_in(&focused, pal.accent, r),
            "the ring is not outside the link"
        );
    }
}

#[test]
fn a_disabled_link_is_quiet_whatever_else_is_set() {
    for pal in [&LIGHT, &DARK] {
        let (plain, r) = drawn(
            pal,
            "Open",
            Look {
                disabled: true,
                ..Look::default()
            },
        );
        let (loud, _) = drawn(
            pal,
            "Open",
            Look {
                hovered: true,
                pressed: true,
                focused: true,
                disabled: true,
            },
        );
        assert_eq!(plain.pixels, loud.pixels, "a disabled link took a mark");
        assert_eq!(
            plain.pixels,
            text_only(pal, r, "Open", pal.faint).pixels,
            "a disabled link is not the faint text and nothing else"
        );
        assert_eq!(
            count_in(&plain, pal.accent, r),
            0,
            "a disabled link is blue"
        );
        assert_eq!(
            longest_run(&plain, pal.accent),
            0,
            "a disabled link was underlined or ringed"
        );
    }
}

#[test]
fn a_link_with_no_text_or_no_room_draws_nothing() {
    let page = blank(&LIGHT);
    for (r, text) in [
        (Rect::new(10.0, 10.0, 40.0, LINK_H), ""),
        (Rect::new(10.0, 10.0, 0.0, LINK_H), "Open"),
        (Rect::new(10.0, 10.0, 40.0, 0.0), "Open"),
        (Rect::new(10.0, 10.0, -5.0, LINK_H), "Open"),
        (Rect::new(f32::NAN, 10.0, 40.0, LINK_H), "Open"),
        (Rect::new(10.0, f32::INFINITY, 40.0, LINK_H), "Open"),
        (Rect::new(10.0, 10.0, f32::NAN, LINK_H), "Open"),
    ] {
        let mut bmp = page.clone();
        let mut gfx = Gfx::new();
        {
            let mut p = painter(&mut bmp, &mut gfx);
            draw_link(
                &mut p,
                &LIGHT,
                r,
                text,
                Look {
                    hovered: true,
                    pressed: true,
                    focused: true,
                    disabled: false,
                },
            );
        }
        assert_eq!(bmp.pixels, page.pixels, "drew {r:?} with {text:?}");
    }
}

#[test]
fn a_link_draws_nothing_outside_its_rectangle_except_the_ring() {
    for pal in [&LIGHT, &DARK] {
        for look in [
            Look::default(),
            Look {
                hovered: true,
                ..Look::default()
            },
            Look {
                pressed: true,
                ..Look::default()
            },
            Look {
                disabled: true,
                ..Look::default()
            },
        ] {
            let (bmp, r) = drawn(pal, "Open the file", look);
            assert!(nothing_outside(&bmp, pal, r), "{look:?} drew outside");
        }
        // Text wider than the rectangle is elided inside it, and the underline stays inside.
        let mut bmp = blank(pal);
        let mut gfx = Gfx::new();
        let r = Rect::new(AT, DOWN, 30.0, LINK_H);
        {
            let mut p = painter(&mut bmp, &mut gfx);
            draw_link(
                &mut p,
                pal,
                r,
                "Open the file",
                Look {
                    hovered: true,
                    ..Look::default()
                },
            );
        }
        assert!(nothing_outside(&bmp, pal, r), "an elided link overflowed");
    }
}

/// The sheet a person looks at: the five states, light and dark, at scale 1 and 1.5.
/// `cargo test -p mdshared-ui link_sheet -- --ignored`, then open
/// `target/specimen/link.png` and `link-150.png`.
#[test]
#[ignore]
fn link_sheet() {
    let (w, h) = (320.0, 320.0);
    let states = [
        ("A link at rest", Look::default()),
        (
            "A link under the pointer",
            Look {
                hovered: true,
                ..Look::default()
            },
        ),
        (
            "A link pressed",
            Look {
                hovered: true,
                pressed: true,
                ..Look::default()
            },
        ),
        (
            "A link with the keyboard",
            Look {
                focused: true,
                ..Look::default()
            },
        ),
        (
            "A link that cannot be used",
            Look {
                disabled: true,
                ..Look::default()
            },
        ),
    ];
    for (scale, file) in [(1.0, "link.png"), (1.5, "link-150.png")] {
        let mut bmp = Bitmap::new((w * scale) as u32, (h * scale) as u32);
        let mut gfx = Gfx::new();
        let mut p = Painter::new(&mut bmp, &mut gfx, scale);
        for (half, pal) in [&LIGHT, &DARK].into_iter().enumerate() {
            let oy = half as f32 * (h / 2.0);
            p.fill(Rect::new(0.0, oy, w, h / 2.0), pal.page);
            let old = p.offset(0.0, oy);
            for (k, (label, look)) in states.into_iter().enumerate() {
                let y = 16.0 + k as f32 * 28.0;
                let r = link_rect(&mut p, 16.0, y, label);
                draw_link(&mut p, pal, r, label, look);
            }
            p.restore_offset(old);
        }
        let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/specimen");
        std::fs::create_dir_all(&out).unwrap();
        bmp.write_png(&out.join(file)).unwrap();
    }
}
