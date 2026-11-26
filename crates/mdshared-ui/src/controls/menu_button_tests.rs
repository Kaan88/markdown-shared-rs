//! Tests for [`crate::controls::menu_button`]: the widths and the chevron's room, the keys, the
//! menu's placement around the button, the two halves of a split button and their hit, the
//! pixels of each half in both palettes, and the quiet disabled look.

use super::*;
use crate::gfx::{Gfx, Rect};
use crate::menu::{Item, Menu};
use crate::theme::{DARK, LIGHT};
use mdshared_text::Bitmap;

const PAGE_W: u32 = 200;
const PAGE_H: u32 = 64;
const R: Rect = Rect::new(12.0, 12.0, 140.0, button::HEIGHT);

fn blank(pal: &Palette) -> Bitmap {
    let mut bmp = Bitmap::new(PAGE_W, PAGE_H);
    let mut gfx = Gfx::new();
    {
        let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
        p.fill(Rect::new(0.0, 0.0, PAGE_W as f32, PAGE_H as f32), pal.page);
    }
    bmp
}

fn drawn_menu(pal: &Palette, look: button::Look) -> Bitmap {
    let mut bmp = blank(pal);
    let mut gfx = Gfx::new();
    {
        let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
        draw(&mut p, pal, R, "Sort by", Kind::Secondary, look);
    }
    bmp
}

fn drawn_split(pal: &Palette, kind: Kind, look: SplitLook) -> Bitmap {
    let mut bmp = blank(pal);
    let mut gfx = Gfx::new();
    {
        let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
        draw_split(&mut p, pal, R, "Run", kind, look);
    }
    bmp
}

fn blank_of(pal: &Palette) -> Bitmap {
    blank(pal)
}

#[test]
fn a_menu_button_leaves_room_for_its_chevron_after_the_label() {
    let mut bmp = Bitmap::new(8, 8);
    let mut gfx = Gfx::new();
    let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
    let label = "Sort by";
    let w = menu_button_w(&mut p, label, Kind::Secondary);
    assert_eq!(
        w,
        (button::width(&mut p, label, Kind::Secondary) + GAP + CHEVRON).round()
    );
    assert!(w > button::width(&mut p, label, Kind::Secondary));
    // An empty label still has room for the chevron.
    assert_eq!(
        menu_button_w(&mut p, "", Kind::Secondary),
        (button::width(&mut p, "", Kind::Secondary) + GAP + CHEVRON).round()
    );
    let r = Rect::new(20.0, 20.0, w, button::HEIGHT);
    let c = chevron_rect(r);
    assert_eq!(c.w, CHEVRON);
    assert_eq!(c.h, CHEVRON);
    assert_eq!(c.right(), r.right() - button::PAD);
    assert_eq!(c.cy(), r.cy());
    let l = label_rect(r);
    // 12 px from the left edge, and 6 px short of the chevron: the label's exact width.
    assert_eq!(l.x, r.x + button::PAD);
    assert_eq!(l.right(), c.x - GAP);
    // The width is rounded with the whole button, so it may differ by less than a pixel.
    let measured = p.measure(button::face(Kind::Secondary), button::TEXT, label);
    assert!((l.w - measured).abs() <= 1.0);
    // The model asks the same questions and keeps its text and kind.
    let b = MenuButton::new(label, Kind::Subtle);
    assert_eq!(b.text(), label);
    assert_eq!(b.kind(), Kind::Subtle);
    assert_eq!(b.width(&mut p), w);
    assert_eq!(b.rect(&mut p, 20.0, 20.0), r);
    assert_eq!(MenuButton::default().kind(), Kind::Secondary);
}

#[test]
fn enter_space_down_and_alt_down_open_a_menu_button() {
    for k in [KeyName::Enter, KeyName::Space] {
        assert_eq!(key(&k, Mods::default()), Press::Activate, "{k:?}");
    }
    assert_eq!(key(&KeyName::Down, Mods::default()), Press::Open, "Down");
    assert_eq!(
        key(
            &KeyName::Down,
            Mods {
                alt: true,
                ..Mods::default()
            }
        ),
        Press::Open,
        "Alt+Down"
    );
    // Shift does not matter.
    assert_eq!(
        key(
            &KeyName::Down,
            Mods {
                shift: true,
                ..Mods::default()
            }
        ),
        Press::Open
    );
    // Ctrl does, and so does Alt with Enter or Space.
    assert_eq!(
        key(
            &KeyName::Down,
            Mods {
                ctrl: true,
                ..Mods::default()
            }
        ),
        Press::None
    );
    assert_eq!(
        key(
            &KeyName::Enter,
            Mods {
                alt: true,
                ..Mods::default()
            }
        ),
        Press::None
    );
    for other in [
        KeyName::Tab,
        KeyName::Escape,
        KeyName::Up,
        KeyName::Left,
        KeyName::Right,
        KeyName::Char('s'),
        KeyName::F(4),
    ] {
        assert_eq!(key(&other, Mods::default()), Press::None, "{other:?}");
    }
    let b = MenuButton::new("Sort by", Kind::Secondary);
    assert!(b.key(&KeyName::Enter, Mods::default()));
    assert!(b.key(&KeyName::Down, Mods::default()));
    assert!(!b.key(&KeyName::Up, Mods::default()));
}

#[test]
fn a_split_button_activates_on_enter_and_space_and_opens_on_down() {
    let b = SplitButton::new("Run", Kind::Primary);
    assert_eq!(b.key(&KeyName::Enter, Mods::default()), Press::Activate);
    assert_eq!(b.key(&KeyName::Space, Mods::default()), Press::Activate);
    assert_eq!(b.key(&KeyName::Down, Mods::default()), Press::Open);
    assert_eq!(
        b.key(
            &KeyName::Down,
            Mods {
                alt: true,
                ..Mods::default()
            }
        ),
        Press::Open
    );
    assert_eq!(b.key(&KeyName::Escape, Mods::default()), Press::None);
    // Alt+Enter is not the main action.
    assert_eq!(
        b.key(
            &KeyName::Enter,
            Mods {
                alt: true,
                ..Mods::default()
            }
        ),
        Press::None
    );
    // The widths stack the chevron half on the main half.
    let mut bmp = Bitmap::new(8, 8);
    let mut gfx = Gfx::new();
    let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
    assert_eq!(
        b.width(&mut p),
        (button::width(&mut p, "Run", Kind::Primary) + CHEVRON_W).round()
    );
    let r = b.rect(&mut p, 10.0, 9.0);
    let h = b.halves(r);
    assert_eq!(h.chevron.w, CHEVRON_W);
    assert_eq!(h.chevron.right(), r.right());
    assert_eq!(h.main.right(), h.chevron.x);
    assert_eq!(SplitButton::default().kind(), Kind::Secondary);
}

#[test]
fn the_two_halves_share_the_button_and_hit_where_they_are() {
    let r = Rect::new(10.0, 10.0, 120.0, button::HEIGHT);
    let h = halves(r);
    assert_eq!(
        h.main,
        Rect::new(10.0, 10.0, 120.0 - CHEVRON_W, button::HEIGHT)
    );
    assert_eq!(
        h.chevron,
        Rect::new(130.0 - CHEVRON_W, 10.0, CHEVRON_W, button::HEIGHT)
    );
    // The main action takes the left and the middle.
    assert_eq!(hit(r, 12.0, 20.0), Some(Half::Main));
    assert_eq!(hit(r, 60.0, 20.0), Some(Half::Main));
    // The chevron takes its last px and the line where the halves meet.
    assert_eq!(hit(r, 129.9, 20.0), Some(Half::Chevron));
    assert_eq!(hit(r, h.chevron.x, 20.0), Some(Half::Chevron));
    // Outside, or not finite, is no half at all.
    assert_eq!(hit(r, 130.0, 20.0), None);
    assert_eq!(hit(r, 9.9, 20.0), None);
    assert_eq!(hit(r, f32::NAN, 20.0), None);
    assert_eq!(hit(r, 60.0, f32::INFINITY), None);
    // A rectangle narrower than the chevron is all chevron, and the main half is empty.
    let tiny = Rect::new(10.0, 10.0, 10.0, button::HEIGHT);
    let h = halves(tiny);
    assert_eq!(h.main.w, 0.0);
    assert_eq!(h.chevron, Rect::new(10.0, 10.0, 10.0, button::HEIGHT));
    assert_eq!(hit(tiny, 10.0, 20.0), Some(Half::Chevron));
    // A rectangle that cannot be drawn has no halves.
    let bad = halves(Rect::new(10.0, 10.0, f32::NAN, button::HEIGHT));
    assert_eq!(bad.main.w, 0.0);
    assert_eq!(
        hit(Rect::new(10.0, 10.0, 0.0, button::HEIGHT), 10.0, 10.0),
        None
    );
}

#[test]
fn the_menu_opens_around_the_button_below_and_above_near_an_edge() {
    let mut bmp = Bitmap::new(600, 600);
    let mut gfx = Gfx::new();
    let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
    let area = Rect::new(0.0, 0.0, 600.0, 600.0);
    let items = || {
        vec![
            Item::new(1u32, "Sort by name"),
            Item::new(2, "Sort by size"),
        ]
    };
    // In the open: below the button, its left edge at the button's.
    let btn = Rect::new(40.0, 40.0, 120.0, button::HEIGHT);
    let mut m = menu_under(Menu::new(items(), 0.0, 0.0, false), btn);
    m.place_in(&mut p, area);
    assert_eq!(m.rect().y, btn.bottom(), "the menu is not under the button");
    assert_eq!(m.rect().x, btn.x, "the menu is not at the button's left");
    assert!(!m.opened_above());
    // Near the bottom: above the button, its bottom on the button's top.
    let low = Rect::new(40.0, 560.0, 120.0, button::HEIGHT);
    let mut m = menu_under(Menu::new(items(), 0.0, 0.0, false), low);
    m.place_in(&mut p, area);
    assert!(m.opened_above(), "the menu did not flip above");
    assert_eq!(m.rect().bottom(), low.y);
    // Near the right: its right edge on the button's.
    let right = Rect::new(460.0, 200.0, 120.0, button::HEIGHT);
    let mut m = menu_under(Menu::new(items(), 0.0, 0.0, false), right);
    m.place_in(&mut p, area);
    assert!(m.opened_left(), "the menu did not flip left");
    assert_eq!(m.rect().right(), right.right());
}

#[test]
fn a_menu_button_draws_its_chevron_and_lights_up_with_the_pointer() {
    for pal in [&LIGHT, &DARK] {
        let theme = if pal.dark { "dark" } else { "light" };
        let rest = drawn_menu(pal, button::Look::default());
        let hover = drawn_menu(
            pal,
            button::Look {
                hovered: true,
                ..button::Look::default()
            },
        );
        assert_ne!(rest.pixels, hover.pixels, "no hover in {theme}");
        // The chevron is there: the chevron's square differs from plain button drawing.
        let mut plain = blank_of(pal);
        let mut gfx = Gfx::new();
        {
            let mut p = Painter::new(&mut plain, &mut gfx, 1.0);
            button::draw(
                &mut p,
                pal,
                R,
                "Sort by",
                Kind::Secondary,
                button::Look::default(),
            );
        }
        assert_ne!(
            rest.pixels, plain.pixels,
            "a menu button is a plain button in {theme}"
        );
        // The chevron itself is ink in the chevron's square, where a plain button has none.
        let c = chevron_rect(R);
        let differs = (c.y as usize..c.bottom() as usize).any(|y| {
            (c.x as usize..c.right() as usize).any(|x| at(&rest, x, y) != at(&plain, x, y))
        });
        assert!(differs, "no chevron is drawn in {theme}");
        // Disabled is the same quiet drawing as a disabled button, and the chevron is still
        // there (faint): the only difference between the two is that chevron.
        let quiet = button::Look {
            disabled: true,
            ..button::Look::default()
        };
        let quiet_menu = drawn_menu(pal, quiet);
        let loud = button::Look {
            disabled: true,
            hovered: true,
            pressed: true,
            open: true,
            focused: true,
            default: true,
            high_contrast: true,
        };
        assert_eq!(
            drawn_menu(pal, loud).pixels,
            quiet_menu.pixels,
            "a disabled menu button took a mark in {theme}"
        );
        let mut quiet_bmp = blank_of(pal);
        let mut gfx = Gfx::new();
        let mut p = Painter::new(&mut quiet_bmp, &mut gfx, 1.0);
        button::draw(&mut p, pal, R, "Sort by", Kind::Secondary, quiet);
        assert_ne!(
            quiet_menu.pixels, quiet_bmp.pixels,
            "a disabled menu button lost its chevron in {theme}"
        );
    }
}

#[test]
fn the_two_halves_of_a_split_button_light_up_on_their_own() {
    for pal in [&LIGHT, &DARK] {
        let theme = if pal.dark { "dark" } else { "light" };
        for kind in Kind::ALL {
            let main = drawn_split(
                pal,
                kind,
                SplitLook {
                    main: button::Look {
                        hovered: true,
                        ..button::Look::default()
                    },
                    chevron: button::Look::default(),
                },
            );
            let chev = drawn_split(
                pal,
                kind,
                SplitLook {
                    main: button::Look::default(),
                    chevron: button::Look {
                        hovered: true,
                        ..button::Look::default()
                    },
                },
            );
            let rest = drawn_split(pal, kind, SplitLook::default());
            assert_ne!(
                main.pixels,
                rest.pixels,
                "{} main took no hover",
                kind.name()
            );
            assert_ne!(
                chev.pixels,
                rest.pixels,
                "{} chevron took no hover",
                kind.name()
            );
            assert_ne!(
                main.pixels,
                chev.pixels,
                "{}'s halves light up together in {theme}",
                kind.name()
            );
        }
    }
}

#[test]
fn a_focused_or_default_split_button_marks_the_whole_button() {
    for pal in [&LIGHT, &DARK] {
        let rest = drawn_split(pal, Kind::Secondary, SplitLook::default());
        let focused = drawn_split(
            pal,
            Kind::Secondary,
            SplitLook {
                main: button::Look {
                    focused: true,
                    ..button::Look::default()
                },
                ..SplitLook::default()
            },
        );
        assert_ne!(rest.pixels, focused.pixels, "the keyboard left no ring");
        // The accent line runs outside the whole button.
        let band = R.inset(-3.0);
        assert_eq!(
            count_in(&rest, pal.accent, band),
            count_in(&rest, pal.accent, R)
        );
        assert!(count_in(&focused, pal.accent, band) > count_in(&focused, pal.accent, R));
        // A default mark on either half covers the whole button.
        let default = drawn_split(
            pal,
            Kind::Secondary,
            SplitLook {
                chevron: button::Look {
                    default: true,
                    ..button::Look::default()
                },
                ..SplitLook::default()
            },
        );
        assert_ne!(rest.pixels, default.pixels);
    }
}

#[test]
fn a_disabled_split_button_is_quiet_whatever_half_asks() {
    for pal in [&LIGHT, &DARK] {
        let quiet = SplitLook {
            main: button::Look {
                disabled: true,
                ..button::Look::default()
            },
            chevron: button::Look {
                disabled: true,
                ..button::Look::default()
            },
        };
        let base = drawn_split(pal, Kind::Primary, quiet);
        for kind in Kind::ALL {
            assert_eq!(
                drawn_split(pal, kind, quiet).pixels,
                base.pixels,
                "a disabled {} differs",
                kind.name()
            );
            let loud = drawn_split(
                pal,
                kind,
                SplitLook {
                    main: button::Look {
                        disabled: true,
                        hovered: true,
                        pressed: true,
                        open: true,
                        focused: true,
                        default: true,
                        high_contrast: true,
                    },
                    chevron: button::Look {
                        disabled: true,
                        hovered: true,
                        pressed: true,
                        open: true,
                        focused: true,
                        default: true,
                        high_contrast: true,
                    },
                },
            );
            assert_eq!(
                loud.pixels,
                base.pixels,
                "a disabled {} took a mark",
                kind.name()
            );
        }
        assert_eq!(count_in(&base, pal.accent, R.inset(-3.0)), 0);
    }
}

#[test]
fn a_menu_or_split_button_with_no_room_draws_nothing() {
    let page = blank(&LIGHT);
    for r in [
        Rect::new(10.0, 10.0, 0.0, button::HEIGHT),
        Rect::new(10.0, 10.0, 60.0, 0.0),
        Rect::new(f32::NAN, 10.0, 60.0, button::HEIGHT),
        Rect::new(10.0, f32::INFINITY, 60.0, button::HEIGHT),
        Rect::new(10.0, 10.0, f32::NAN, button::HEIGHT),
    ] {
        let mut bmp = page.clone();
        let mut gfx = Gfx::new();
        {
            let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
            draw(
                &mut p,
                &LIGHT,
                r,
                "Sort by",
                Kind::Primary,
                button::Look {
                    hovered: true,
                    pressed: true,
                    focused: true,
                    ..button::Look::default()
                },
            );
            draw_split(
                &mut p,
                &LIGHT,
                r,
                "Run",
                Kind::Primary,
                SplitLook {
                    main: button::Look {
                        hovered: true,
                        ..button::Look::default()
                    },
                    chevron: button::Look {
                        hovered: true,
                        ..button::Look::default()
                    },
                },
            );
        }
        assert_eq!(bmp.pixels, page.pixels, "drew {r:?}");
    }
}

#[test]
fn a_menu_button_whose_menu_is_open_is_drawn_pressed() {
    for (name, pal) in palettes() {
        let open = drawn_menu(
            pal,
            button::Look {
                open: true,
                ..button::Look::default()
            },
        );
        let pressed = drawn_menu(
            pal,
            button::Look {
                hovered: true,
                pressed: true,
                ..button::Look::default()
            },
        );
        assert_eq!(
            open.pixels, pressed.pixels,
            "an open menu button is not drawn pressed in {name}"
        );
        assert_ne!(
            open.pixels,
            drawn_menu(pal, button::Look::default()).pixels,
            "an open menu button is at rest in {name}"
        );
    }
}

#[test]
fn a_split_chevron_whose_menu_is_open_is_drawn_pressed() {
    for (name, pal) in palettes() {
        for kind in Kind::ALL {
            let open = drawn_split(
                pal,
                kind,
                SplitLook {
                    chevron: button::Look {
                        open: true,
                        ..button::Look::default()
                    },
                    ..SplitLook::default()
                },
            );
            let pressed = drawn_split(
                pal,
                kind,
                SplitLook {
                    chevron: button::Look {
                        hovered: true,
                        pressed: true,
                        ..button::Look::default()
                    },
                    ..SplitLook::default()
                },
            );
            let rest = drawn_split(pal, kind, SplitLook::default());
            assert_eq!(
                open.pixels,
                pressed.pixels,
                "an open {} chevron is not drawn pressed in {name}",
                kind.name()
            );
            assert_ne!(
                open.pixels,
                rest.pixels,
                "an open {} chevron is at rest in {name}",
                kind.name()
            );
        }
    }
}

#[test]
fn the_split_border_and_default_mark_are_one_device_pixel_wide() {
    for (name, pal) in palettes() {
        for scale in [1.0f32, 1.25, 1.5] {
            // The secondary edge, scanned down a column through the main half.
            let b = drawn_split_at(pal, Kind::Secondary, SplitLook::default(), scale);
            let x = ((R.x + 30.0) * scale).floor() as usize;
            assert_eq!(
                first_run(&b, x, pal.popup_line),
                1,
                "the split's border is not one device pixel at scale {scale} in {name}"
            );
            // And the default mark, in the accent.
            let d = drawn_split_at(
                pal,
                Kind::Secondary,
                SplitLook {
                    main: button::Look {
                        default: true,
                        ..button::Look::default()
                    },
                    ..SplitLook::default()
                },
                scale,
            );
            assert_eq!(
                first_run(&d, x, pal.accent),
                1,
                "the split's default mark is not one device pixel at scale {scale} in {name}"
            );
        }
    }
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

/// A split button drawn at `scale`, for the device-pixel tests.
fn drawn_split_at(pal: &Palette, kind: Kind, look: SplitLook, scale: f32) -> Bitmap {
    let mut bmp = Bitmap::new(
        (PAGE_W as f32 * scale) as u32,
        (PAGE_H as f32 * scale) as u32,
    );
    let area = Rect::new(0.0, 0.0, bmp.width as f32, bmp.height as f32);
    let mut gfx = Gfx::new();
    {
        let mut p = Painter::new(&mut bmp, &mut gfx, scale);
        p.fill(area, pal.page);
        draw_split(&mut p, pal, R, "Run", kind, look);
    }
    bmp
}

/// The first run of pixels holding exactly `c`, scanned down column `x` from the top.
fn first_run(bmp: &Bitmap, x: usize, c: [u8; 3]) -> usize {
    let mut run = 0;
    for y in 0..bmp.height as usize {
        if at(bmp, x, y) == c {
            run += 1;
        } else if run > 0 {
            break;
        }
    }
    run
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
