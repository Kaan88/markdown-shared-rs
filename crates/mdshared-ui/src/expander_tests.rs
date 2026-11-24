use super::*;
use crate::anim::{Clock, ManualClock};
use crate::gfx::{Gfx, Rgb};
use crate::keys::{KeyName, Mods};
use crate::theme::{DARK, LIGHT};
use mdshared_text::Bitmap;
use std::time::Duration;

const BODY: f32 = 100.0;

fn area() -> Rect {
    Rect::new(10.0, 20.0, 200.0, 140.0)
}

fn header_on(pal: &Palette, ground: Rgb, expanded: bool, look: Look) -> Bitmap {
    let mut bmp = Bitmap::new(200, 40);
    let mut gfx = Gfx::new();
    {
        let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
        p.fill(Rect::new(0.0, 0.0, 200.0, 40.0), ground);
        draw_header(
            &mut p,
            pal,
            Rect::new(0.0, 0.0, 200.0, HEADER_H),
            "Details",
            expanded,
            look,
        );
    }
    bmp
}

fn header_bitmap(pal: &Palette, expanded: bool, look: Look) -> Bitmap {
    header_on(pal, pal.page, expanded, look)
}

fn rgb_at(bmp: &Bitmap, x: usize, y: usize) -> Rgb {
    let o = (y * 200 + x) * 4;
    [bmp.pixels[o + 2], bmp.pixels[o + 1], bmp.pixels[o]]
}

#[test]
fn a_click_on_the_header_toggles_and_a_click_on_the_body_does_not() {
    let mut e = Expander::new(false);
    let head = header_rect(area());
    let click = |e: &mut Expander, x: f32, y: f32| {
        if e.header_hit(head, x, y) {
            e.toggle();
        }
    };
    click(&mut e, head.x + 3.0, head.y + 3.0);
    assert!(e.expanded, "a click on the header did not open it");
    click(&mut e, head.cx(), head.bottom() + 3.0);
    assert!(e.expanded, "a click on the body closed it");
    click(&mut e, head.cx(), head.cy());
    assert!(!e.expanded, "a second click on the header did not close it");
}

#[test]
fn the_header_takes_its_own_rectangle_and_no_more() {
    let e = Expander::new(false);
    let head = header_rect(area());
    assert!(e.header_hit(head, head.x, head.y));
    assert!(e.header_hit(head, head.right() - 0.5, head.bottom() - 0.5));
    assert!(!e.header_hit(head, head.right(), head.cy()), "right edge");
    assert!(!e.header_hit(head, head.cx(), head.bottom()), "bottom edge");
    assert!(!e.header_hit(head, head.x - 1.0, head.cy()));
    assert!(!e.header_hit(head, f32::NAN, head.cy()));
    assert!(!e.header_hit(Rect::new(0.0, 0.0, f32::NAN, HEADER_H), 1.0, 1.0));
    assert!(!e.header_hit(Rect::new(0.0, 0.0, 0.0, HEADER_H), 0.0, 1.0));
}

#[test]
fn space_and_enter_toggle_and_other_keys_do_not() {
    let mut e = Expander::new(false);
    assert!(e.key(KeyName::Space, Mods::default()));
    assert!(e.expanded);
    assert!(e.key(KeyName::Enter, Mods::default()));
    assert!(!e.expanded);
    for key in [
        KeyName::Char('x'),
        KeyName::Tab,
        KeyName::Escape,
        KeyName::Up,
        KeyName::Down,
        KeyName::Left,
        KeyName::Right,
        KeyName::Home,
        KeyName::End,
        KeyName::F(5),
        KeyName::Backspace,
        KeyName::Delete,
        KeyName::PageUp,
        KeyName::PageDown,
    ] {
        let before = e.expanded;
        assert!(
            !e.key(key.clone(), Mods::default()),
            "{key:?} was taken by the expander"
        );
        assert_eq!(e.expanded, before, "{key:?} changed the expander");
    }
}

#[test]
fn ctrl_and_alt_leave_the_expander_to_the_applications_shortcuts() {
    let ctrl = Mods {
        ctrl: true,
        ..Mods::default()
    };
    let alt = Mods {
        alt: true,
        ..Mods::default()
    };
    let mut e = Expander::new(false);
    assert!(!e.key(KeyName::Enter, ctrl), "Ctrl+Enter was taken");
    assert!(!e.key(KeyName::Space, ctrl), "Ctrl+Space was taken");
    assert!(!e.key(KeyName::Enter, alt), "Alt+Enter was taken");
    assert!(!e.key(KeyName::Space, alt), "Alt+Space was taken");
    assert!(!e.expanded, "a shortcut changed the expander");
    // Shift is not a shortcut modifier here: Shift+Enter still toggles.
    let shift = Mods {
        shift: true,
        ..Mods::default()
    };
    assert!(e.key(KeyName::Enter, shift));
    assert!(e.expanded);
}

#[test]
fn set_answers_whether_it_changed_anything() {
    let mut e = Expander::new(false);
    assert!(e.set(true));
    assert!(e.expanded);
    assert!(!e.set(true));
    assert!(e.set(false));
    assert!(!e.expanded);
    assert!(!e.set(false));
    assert!(e.toggle());
    assert!(!e.toggle());
}

#[test]
fn a_header_is_the_top_of_the_area_and_a_nonsense_area_gives_nothing() {
    let a = area();
    assert_eq!(header_rect(a), Rect::new(a.x, a.y, a.w, HEADER_H));
    assert_eq!(
        header_rect(Rect::new(5.0, 6.0, 100.0, 20.0)),
        Rect::new(5.0, 6.0, 100.0, 20.0),
        "a short area keeps a short header"
    );
    assert_eq!(
        header_rect(Rect::new(f32::NAN, f32::INFINITY, f32::NAN, 90.0)),
        Rect::new(0.0, 0.0, 0.0, HEADER_H)
    );
    assert_eq!(
        header_rect(Rect::new(0.0, 0.0, -5.0, 90.0)).w,
        0.0,
        "a negative width is not a rectangle"
    );
    assert_eq!(
        header_rect(Rect::new(0.0, 0.0, 100.0, f32::INFINITY)).h,
        HEADER_H,
        "an endless area gets the ordinary header"
    );
}

#[test]
fn the_body_height_runs_from_zero_to_the_measured_height_and_back() {
    let clock = ManualClock::new();
    let mut anim = Animator::new();
    anim.set_max_step(Duration::from_secs(1));
    let mut frame = |ms: u64, expanded: bool| {
        clock.advance_ms(ms);
        anim.begin_frame(clock.now());
        let h = body_height(&mut anim, "details", expanded, BODY);
        (h, anim.end_frame().is_some())
    };
    let (h, more) = frame(0, false);
    assert_eq!(h, 0.0);
    assert!(!more, "a closed body at rest asks for no frame");
    // The frame that changes the target still shows the old place.
    let (h, more) = frame(0, true);
    assert_eq!(h, 0.0);
    assert!(more, "an opening body asks for another frame");
    let (h, more) = frame(100, true);
    assert!(h > 0.0 && h < BODY, "half way: {h}");
    assert!(more);
    let (h, more) = frame(300, true);
    assert_eq!(h, BODY);
    assert!(!more, "an open body at rest asks for no frame");
    // And back down.
    let (h, more) = frame(0, false);
    assert_eq!(h, BODY);
    assert!(more);
    let (h, more) = frame(100, false);
    assert!(h > 0.0 && h < BODY, "half way down: {h}");
    assert!(more);
    let (h, more) = frame(300, false);
    assert_eq!(h, 0.0);
    assert!(!more);
}

#[test]
fn reduced_motion_jumps_to_the_end_with_no_frame_asked() {
    let clock = ManualClock::new();
    let mut anim = Animator::new();
    anim.set_system_reduced(true);
    let mut frame = |expanded: bool| {
        anim.begin_frame(clock.now());
        let h = body_height(&mut anim, "details", expanded, BODY);
        (h, anim.end_frame().is_some())
    };
    assert_eq!(frame(false), (0.0, false));
    assert_eq!(frame(true), (BODY, false), "reduced motion waited");
    assert_eq!(frame(false), (0.0, false), "reduced motion waited");
}

#[test]
fn nonsense_heights_are_empty() {
    let clock = ManualClock::new();
    let mut anim = Animator::new();
    anim.begin_frame(clock.now());
    for (id, full) in [
        ("nan", f32::NAN),
        ("inf", f32::INFINITY),
        ("negative", -20.0),
    ] {
        assert_eq!(
            body_height(&mut anim, id, true, full),
            0.0,
            "{id} drew a body"
        );
    }
    assert_eq!(
        height(&mut anim, "nan", true, f32::NAN),
        HEADER_H,
        "a nonsense body still has a header"
    );
    anim.end_frame();
}

#[test]
fn the_layout_height_is_the_header_plus_what_shows() {
    let clock = ManualClock::new();
    let mut closed = Animator::new();
    closed.begin_frame(clock.now());
    assert_eq!(height(&mut closed, "closed", false, BODY), HEADER_H);
    closed.end_frame();
    let mut open = Animator::new();
    open.begin_frame(clock.now());
    assert_eq!(
        height(&mut open, "open", true, BODY),
        HEADER_H + BODY,
        "an expander first seen open is at its full height"
    );
    open.end_frame();
}

#[test]
fn the_chevron_points_right_when_closed_and_down_when_open() {
    for pal in [&LIGHT, &DARK] {
        let closed = header_bitmap(pal, false, Look::default());
        let open = header_bitmap(pal, true, Look::default());
        assert_ne!(closed.pixels, open.pixels, "the chevron did not change");
        let mut changed = 0;
        for y in 0..40 {
            for x in 0..200 {
                let o = (y * 200 + x) * 4;
                if closed.pixels[o..o + 4] != open.pixels[o..o + 4] {
                    changed += 1;
                    assert!(x < 32, "the change reached the title at {x},{y}");
                }
            }
        }
        assert!(changed > 8, "only {changed} pixels of chevron changed");
    }
}

#[test]
fn every_look_of_the_header_draws_differently() {
    for pal in [&LIGHT, &DARK] {
        let plain = header_bitmap(pal, false, Look::default()).pixels;
        let hovered = Look {
            hovered: true,
            ..Look::default()
        };
        let pressed = Look {
            pressed: true,
            ..Look::default()
        };
        let focused = Look {
            focused: true,
            ..Look::default()
        };
        let disabled = Look {
            disabled: true,
            ..Look::default()
        };
        for (name, look) in [
            ("hovered", hovered),
            ("pressed", pressed),
            ("focused", focused),
            ("disabled", disabled),
        ] {
            assert_ne!(
                plain,
                header_bitmap(pal, false, look).pixels,
                "the {name} header looks like the plain one"
            );
        }
        // A disabled header takes no hover or focus mark.
        let quiet = header_bitmap(pal, false, disabled).pixels;
        let quiet_hot = header_bitmap(
            pal,
            false,
            Look {
                hovered: true,
                focused: true,
                ..disabled
            },
        )
        .pixels;
        assert_eq!(quiet, quiet_hot, "a disabled header showed the pointer");
    }
}

#[test]
fn at_rest_and_disabled_the_header_leaves_the_background_showing() {
    for pal in [&LIGHT, &DARK] {
        let plain = header_on(pal, pal.pane, false, Look::default());
        assert_eq!(
            rgb_at(&plain, 8, 2),
            pal.pane,
            "the resting header painted a box over the pane"
        );
        let disabled = Look {
            disabled: true,
            ..Look::default()
        };
        let quiet = header_on(pal, pal.pane, false, disabled);
        assert_eq!(
            rgb_at(&quiet, 8, 2),
            pal.pane,
            "the disabled header painted a box over the pane"
        );
        for (name, look) in [
            (
                "hovered",
                Look {
                    hovered: true,
                    ..Look::default()
                },
            ),
            (
                "pressed",
                Look {
                    pressed: true,
                    ..Look::default()
                },
            ),
        ] {
            let filled = header_on(pal, pal.pane, false, look);
            assert_ne!(
                rgb_at(&filled, 8, 2),
                pal.pane,
                "the {name} header did not fill"
            );
        }
    }
}

#[test]
fn the_returned_body_rect_is_under_the_header_and_as_tall_as_the_animation() {
    let clock = ManualClock::new();
    let mut anim = Animator::new();
    anim.set_max_step(Duration::from_secs(1));
    let a = area();
    let mut frame = |ms: u64, expanded: bool| {
        clock.advance_ms(ms);
        anim.begin_frame(clock.now());
        let mut bmp = Bitmap::new(240, 180);
        let mut gfx = Gfx::new();
        let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
        p.fill(Rect::new(0.0, 0.0, 240.0, 180.0), LIGHT.page);
        let body = draw_expander_animated(
            &mut p,
            &LIGHT,
            &mut anim,
            "expander",
            a,
            "Details",
            expanded,
            Look::default(),
            BODY,
        );
        (body, anim.end_frame().is_some())
    };
    let (body, more) = frame(0, false);
    assert_eq!(
        (body.x, body.y, body.w, body.h),
        (a.x, a.y + HEADER_H, a.w, 0.0)
    );
    assert!(!more);
    frame(0, true);
    let (body, more) = frame(100, true);
    assert_eq!((body.x, body.y, body.w), (a.x, a.y + HEADER_H, a.w));
    assert!(body.h > 0.0 && body.h < BODY, "half way: {}", body.h);
    assert!(more);
    let (body, _) = frame(300, true);
    assert_eq!(body.h, BODY);
}

#[test]
fn the_body_rect_stops_at_the_bottom_of_the_area() {
    let clock = ManualClock::new();
    let mut anim = Animator::new();
    let a = Rect::new(0.0, 0.0, 200.0, HEADER_H + 30.0);
    anim.begin_frame(clock.now());
    let mut bmp = Bitmap::new(200, 80);
    let mut gfx = Gfx::new();
    let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
    let body = draw_expander_animated(
        &mut p,
        &LIGHT,
        &mut anim,
        "short",
        a,
        "Details",
        true,
        Look::default(),
        BODY,
    );
    assert_eq!(body.h, 30.0, "the body ran past its area");
    // An area shorter than the header has a short header and no room for a body.
    let tiny = Rect::new(0.0, 0.0, 200.0, 20.0);
    let body = draw_expander_animated(
        &mut p,
        &LIGHT,
        &mut anim,
        "tiny",
        tiny,
        "Details",
        true,
        Look::default(),
        BODY,
    );
    assert_eq!(
        (body.y, body.h),
        (20.0, 0.0),
        "the body ignored the short header"
    );
    anim.end_frame();
}

/// The sheet a person looks at: an open expander and the header's looks while closed, light and
/// dark, at two scales. `cargo test -p mdshared-ui expander_sheet -- --ignored`, then open
/// target/specimen/expander.png and expander-150.png.
#[test]
#[ignore]
fn expander_sheet() {
    for (scale, file) in [(1.0, "expander.png"), (1.5, "expander-150.png")] {
        let mut bmp = Bitmap::new((560.0 * scale) as u32, (440.0 * scale) as u32);
        let mut gfx = Gfx::new();
        {
            let mut p = Painter::new(&mut bmp, &mut gfx, scale);
            let clock = ManualClock::new();
            let mut anim = Animator::new();
            anim.set_max_step(Duration::from_secs(1));
            for (half, pal) in [&LIGHT, &DARK].into_iter().enumerate() {
                let oy = half as f32 * 220.0;
                p.fill(Rect::new(0.0, oy, 560.0, 220.0), pal.page);
                let o = p.offset(0.0, oy);
                let card = |x: f32| Rect::new(x, 16.0, 250.0, 180.0);
                let body = draw_expander_animated(
                    &mut p,
                    pal,
                    &mut anim,
                    ("open", half),
                    card(20.0),
                    "Pinned files",
                    true,
                    Look::default(),
                    BODY,
                );
                if body.h > 0.0 {
                    let clip = p.clip(body);
                    p.fill(body, pal.field);
                    for line in 0..4 {
                        p.fill(
                            Rect::new(
                                body.x + 12.0,
                                body.y + 10.0 + line as f32 * 22.0,
                                body.w - 24.0 - line as f32 * 18.0,
                                9.0,
                            ),
                            pal.line,
                        );
                    }
                    p.restore(clip);
                }
                let hot = Look {
                    hovered: true,
                    ..Look::default()
                };
                let held = Look {
                    pressed: true,
                    ..Look::default()
                };
                let focus = Look {
                    focused: true,
                    ..Look::default()
                };
                let off = Look {
                    disabled: true,
                    ..Look::default()
                };
                for (k, (title, look)) in [
                    ("Hovered", hot),
                    ("Pressed", held),
                    ("Focused", focus),
                    ("Disabled", off),
                ]
                .into_iter()
                .enumerate()
                {
                    draw_header(
                        &mut p,
                        pal,
                        Rect::new(300.0, 16.0 + 40.0 * k as f32, 240.0, HEADER_H),
                        title,
                        false,
                        look,
                    );
                }
                p.restore_offset(o);
            }
            anim.begin_frame(clock.now());
            anim.end_frame();
        }
        let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/specimen");
        bmp.write_png(&out.join(file)).unwrap();
    }
}
