//! Tests for [`crate::controls::icon_button`]: the square and its hit, the keys, the glyph's
//! pixels in each state and both palettes, its disabled look, and the tooltip it gives the
//! tooltip model.

use super::*;
use crate::controls::{button, button::Look};
use crate::gfx::{Gfx, Rect};
use crate::keys::{KeyName, Mods};
use crate::theme::LIGHT;
use crate::tooltip::Tooltip;
use mdshared_text::Bitmap;
use std::time::Duration;

const PAGE: u32 = 64;
const R: Rect = Rect::new(12.0, 12.0, SIZE, SIZE);

fn drawn(pal: &Palette, look: Look) -> Bitmap {
    let mut bmp = Bitmap::new(PAGE, PAGE);
    let mut gfx = Gfx::new();
    {
        let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
        p.fill(Rect::new(0.0, 0.0, PAGE as f32, PAGE as f32), pal.page);
        draw(&mut p, pal, R, crate::glyph::SEARCH, look);
    }
    bmp
}

fn at(bmp: &Bitmap, x: usize, y: usize) -> [u8; 3] {
    let o = (y * bmp.width as usize + x) * 4;
    [bmp.pixels[o + 2], bmp.pixels[o + 1], bmp.pixels[o]]
}

#[test]
fn the_icon_button_is_a_square_of_the_buttons_height_and_holds_its_glyph_and_tip() {
    let b = IconButton::new(crate::glyph::SEARCH, "Search (Ctrl+F)");
    assert_eq!(b.icon(), crate::glyph::SEARCH);
    assert_eq!(b.name(), "Search (Ctrl+F)");
    assert_eq!(b.rect(10.0, 9.0), Rect::new(10.0, 9.0, SIZE, SIZE));
    assert_eq!(SIZE, button::HEIGHT);
    let named = IconButton::new(crate::glyph::SEARCH, "Find");
    assert_eq!(named.name(), "Find");
    assert_eq!(rect(10.0, 9.0), b.rect(10.0, 9.0));
    // The hit is the square's, not the glyph's.
    assert!(b.hit(R, R.x, R.y));
    assert!(b.hit(R, R.cx(), R.cy()));
    assert!(!b.hit(R, R.right(), R.cy()));
    assert!(!b.hit(R, f32::NAN, R.cy()));
    // The keys are a push button's.
    assert!(named.key(&KeyName::Enter, Mods::default()));
    assert!(named.key(&KeyName::Space, Mods::default()));
    assert!(!named.key(&KeyName::Escape, Mods::default()));
    assert!(!named.key(
        &KeyName::Enter,
        Mods {
            ctrl: true,
            ..Mods::default()
        }
    ));
}

#[test]
fn the_icon_button_draws_its_glyph_and_every_state_differs_in_both_palettes() {
    for (theme, pal) in palettes() {
        let states = [
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
        ];
        for i in 0..states.len() {
            for j in i + 1..states.len() {
                assert_ne!(
                    drawn(pal, states[i].1).pixels,
                    drawn(pal, states[j].1).pixels,
                    "{} and {} look the same in {theme}",
                    states[i].0,
                    states[j].0
                );
            }
        }
        // The glyph is drawn at rest: the middle of the square is not the page's colour.
        let rest = drawn(pal, Look::default());
        let page = at(&rest, 0, 0);
        let ink = (R.y as usize..R.bottom() as usize)
            .any(|y| (R.x as usize..R.right() as usize).any(|x| at(&rest, x, y) != page));
        assert!(ink, "no glyph is drawn in {theme}");
    }
}

#[test]
fn a_disabled_icon_button_takes_no_mark() {
    for (_, pal) in palettes() {
        let quiet = Look {
            disabled: true,
            ..Look::default()
        };
        let base = drawn(pal, quiet);
        let loud = drawn(
            pal,
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
            base.pixels, loud.pixels,
            "a disabled icon button took a mark"
        );
    }
}

#[test]
fn an_icon_button_with_no_room_draws_nothing() {
    let mut bmp = Bitmap::new(PAGE, PAGE);
    let mut gfx = Gfx::new();
    {
        let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
        p.fill(Rect::new(0.0, 0.0, PAGE as f32, PAGE as f32), LIGHT.page);
    }
    let page = bmp.clone();
    for r in [
        Rect::new(10.0, 10.0, 0.0, SIZE),
        Rect::new(10.0, 10.0, SIZE, 0.0),
        Rect::new(f32::NAN, 10.0, SIZE, SIZE),
        Rect::new(10.0, f32::INFINITY, SIZE, SIZE),
    ] {
        let mut bmp = page.clone();
        let mut gfx = Gfx::new();
        let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
        draw(
            &mut p,
            &LIGHT,
            r,
            crate::glyph::SEARCH,
            Look {
                hovered: true,
                pressed: true,
                focused: true,
                ..Look::default()
            },
        );
        assert_eq!(bmp.pixels, page.pixels, "drew {r:?}");
    }
}

#[test]
fn the_icon_button_gives_the_tooltip_model_its_text_and_anchor() {
    let b = IconButton::new(crate::glyph::SEARCH, "Search (Ctrl+F)");
    let over = b.over("search", R);
    assert_eq!(over.key, "search");
    assert_eq!(over.text, "Search (Ctrl+F)");
    assert_eq!(over.anchor, R);
    // After the delay the tooltip model shows it, and the bubble is placed under the square.
    let mut tip: Tooltip<&str> = Tooltip::new();
    let start = std::time::Instant::now();
    assert!(!tip.hover(start, Some(over)));
    assert!(tip.shown().is_none());
    let due = tip.deadline().expect("no deadline for the waiting tip");
    assert!(tip.tick(due));
    assert_eq!(
        tip.shown().map(|o| (o.key, o.anchor)),
        Some(("search", R)),
        "the icon button's tip did not show at the anchor"
    );
    // The bubble sits under the square's middle, within its window.
    let centred = Rect::new(90.0, 12.0, SIZE, SIZE);
    let mut bmp = Bitmap::new(200, 120);
    let mut gfx = Gfx::new();
    let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
    let bubble = crate::overlay::Bubble::tip(&mut p, "Search (Ctrl+F)", centred, 200.0, 120.0);
    assert_eq!(bubble.rect.y, centred.bottom() + 6.0);
    assert!((bubble.rect.cx() - centred.cx()).abs() <= 1.0);
    // A press hides it, and the delay starts again after the pointer leaves.
    let shown_at = due;
    tip.press(Some("search"));
    assert!(tip.shown().is_none());
    tip.hover(
        shown_at + Duration::from_millis(1),
        Some(b.over("search", R)),
    );
    assert!(
        tip.shown().is_none(),
        "the tip returned at once after a press"
    );
}

/// The four palettes the pixel tests run over.
fn palettes() -> [(&'static str, &'static Palette); 4] {
    use crate::theme::{DARK, LIGHT};
    use crate::tokens::{HIGH_CONTRAST_DARK, HIGH_CONTRAST_LIGHT};
    [
        ("light", &LIGHT),
        ("dark", &DARK),
        ("high-contrast light", &HIGH_CONTRAST_LIGHT),
        ("high-contrast dark", &HIGH_CONTRAST_DARK),
    ]
}
