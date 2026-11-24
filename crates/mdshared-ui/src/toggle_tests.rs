//! Tests of the standalone toggle switch: the model's keys and clicks, and its drawing against
//! the settings dialog's own toggle rows, pixel for pixel.
//!
//! The sheet: `cargo test -p mdshared-ui toggle_sheet -- --ignored`, then open
//! target/specimen/toggle.png and toggle-150.png.

use super::*;
use crate::anim::{Clock, ManualClock};
use crate::gfx::Gfx;
use crate::settings::pages::{self, Item, Page};
use crate::settings::{Form, Row};
use crate::theme::{DARK, LIGHT};
use mdshared_text::Bitmap;

const R: Rect = Rect {
    x: 20.0,
    y: 14.0,
    w: TOGGLE_W,
    h: TOGGLE_H,
};

fn mods(ctrl: bool, shift: bool, alt: bool) -> Mods {
    Mods {
        ctrl,
        shift,
        alt,
        control: false,
    }
}

#[test]
fn a_new_switch_holds_its_value_and_set_says_whether_it_changed() {
    let mut t = Toggle::new(false);
    assert!(!t.on() && !t.pressed());
    assert!(!t.set(false));
    assert!(t.set(true));
    assert!(t.on());
    assert!(!t.set(true));
    assert_eq!(Toggle::default(), Toggle::new(false));
}

#[test]
fn space_turns_it_over_and_no_other_key_does() {
    let mut t = Toggle::new(false);
    assert!(t.key(&KeyName::Space, Mods::default()));
    assert!(t.on());
    assert!(
        t.key(&KeyName::Space, mods(false, true, false)),
        "Shift+Space"
    );
    assert!(!t.on());
    for (key, m) in [
        (KeyName::Space, mods(true, false, false)),
        (KeyName::Space, mods(false, false, true)),
        (KeyName::Enter, Mods::default()),
        (KeyName::Left, Mods::default()),
        (KeyName::Right, Mods::default()),
        (KeyName::Tab, Mods::default()),
        (KeyName::Escape, Mods::default()),
        (KeyName::Char(' '), Mods::default()),
    ] {
        assert!(!t.key(&key, m), "{key:?} {m:?}");
        assert!(!t.on(), "{key:?} changed it");
    }
}

#[test]
fn a_click_turns_it_over_when_the_button_goes_up_on_the_switch() {
    let mut t = Toggle::new(false);
    assert!(t.press(R, R.cx(), R.cy()));
    assert!(t.pressed() && !t.on(), "nothing changes on the press");
    assert!(t.release(R, R.x + 1.0, R.y + 1.0));
    assert!(t.on() && !t.pressed());
    // Released off the switch: no change, and the press is over.
    assert!(t.press(R, R.cx(), R.cy()));
    assert!(!t.release(R, R.right() + 5.0, R.cy()));
    assert!(t.on() && !t.pressed());
    // A release with no press, and a press elsewhere, do nothing.
    assert!(!t.release(R, R.cx(), R.cy()));
    assert!(!t.press(R, 0.0, 0.0));
    assert!(!t.release(R, R.cx(), R.cy()));
    assert!(t.on());
    // A cancelled press does not turn it over.
    assert!(t.press(R, R.cx(), R.cy()));
    assert!(t.cancel());
    assert!(!t.cancel());
    assert!(!t.release(R, R.cx(), R.cy()));
    assert!(t.on());
    // Space during a press turns it over once and ends the press.
    assert!(t.press(R, R.cx(), R.cy()));
    assert!(t.key(&KeyName::Space, Mods::default()));
    assert!(!t.release(R, R.cx(), R.cy()));
    assert!(!t.on());
}

#[test]
fn nonsense_rectangles_and_points_are_never_hit() {
    let t = Toggle::new(false);
    assert!(t.hit(R, R.x, R.y));
    assert!(!t.hit(R, R.right(), R.cy()), "the right edge is outside");
    for r in [
        Rect::new(f32::NAN, 0.0, 40.0, 20.0),
        Rect::new(0.0, 0.0, f32::INFINITY, 20.0),
        Rect::new(0.0, 0.0, 0.0, 20.0),
        Rect::new(0.0, 0.0, 40.0, -20.0),
    ] {
        assert!(!t.hit(r, 1.0, 1.0), "{r:?}");
    }
    for (x, y) in [(f32::NAN, R.cy()), (R.cx(), f32::INFINITY)] {
        assert!(!t.hit(R, x, y));
    }
    let mut t = t;
    assert!(!t.press(Rect::new(0.0, 0.0, f32::NAN, 1.0), 0.0, 0.0));
}

/// The model against a plain one written out again, on a long pseudo-random run of presses,
/// releases, keys and cancels at points on and off the switch.
#[test]
fn a_random_run_of_events_agrees_with_a_plain_model() {
    let mut seed: u32 = 0x2545_f491;
    let mut next = move || {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        seed >> 8
    };
    let mut t = Toggle::new(false);
    let (mut on, mut down) = (false, false);
    for step in 0..5000 {
        let x = R.x - 10.0 + (next() % 60) as f32;
        let y = R.y - 5.0 + (next() % 30) as f32;
        let inside = x >= R.x && x < R.right() && y >= R.y && y < R.bottom();
        match next() % 4 {
            0 => {
                assert_eq!(t.press(R, x, y), inside);
                down = inside;
            }
            1 => {
                let turned = down && inside;
                assert_eq!(t.release(R, x, y), turned, "step {step}");
                on ^= turned;
                down = false;
            }
            2 => {
                assert!(t.key(&KeyName::Space, Mods::default()));
                on = !on;
                down = false;
            }
            _ => {
                assert_eq!(t.cancel(), down);
                down = false;
            }
        }
        assert_eq!((t.on(), t.pressed()), (on, down), "step {step}");
    }
}

// ---- drawing ------------------------------------------------------------------------------

/// The switch on a popup, as a dialog or a card draws it.
fn drawn(pal: &Palette, scale: f32, on: bool, look: Look) -> Bitmap {
    let mut bmp = Bitmap::new((80.0 * scale) as u32, (48.0 * scale) as u32);
    let mut gfx = Gfx::new();
    {
        let mut p = Painter::new(&mut bmp, &mut gfx, scale);
        p.fill(Rect::new(0.0, 0.0, 80.0, 48.0), pal.popup);
        draw_toggle(&mut p, pal, R, &Toggle::new(on), look);
    }
    bmp
}

fn at(bmp: &Bitmap, x: u32, y: u32) -> [u8; 3] {
    let o = ((y * bmp.width + x) * 4) as usize;
    [bmp.pixels[o + 2], bmp.pixels[o + 1], bmp.pixels[o]]
}

#[test]
fn on_off_hovered_pressed_focused_and_disabled_draw_differently() {
    for pal in [&LIGHT, &DARK] {
        let plain = Look::default();
        let hov = Look {
            hovered: true,
            ..plain
        };
        let held = Look {
            pressed: true,
            ..hov
        };
        let focus = Look {
            focused: true,
            ..plain
        };
        let off = Look {
            disabled: true,
            ..plain
        };
        let all: Vec<Vec<u8>> = [false, true]
            .into_iter()
            .flat_map(|on| [plain, hov, held, focus, off].map(|l| drawn(pal, 1.0, on, l).pixels))
            .collect();
        for i in 0..all.len() {
            for j in i + 1..all.len() {
                assert_ne!(all[i], all[j], "looks {i} and {j}");
            }
        }
        // On: the accent at the left of the pill, clear of the knob.
        assert_eq!(at(&drawn(pal, 1.0, true, plain), 27, 24), pal.accent);
        // Off: the field there.
        assert_eq!(at(&drawn(pal, 1.0, false, plain), 50, 24), pal.field);
        // The ring is accent outside the pill, and nothing is drawn further out.
        let ring = drawn(pal, 1.0, false, focus);
        assert_eq!(at(&ring, 40, 12), pal.accent);
        assert_eq!(at(&ring, 40, 9), pal.popup);
        // A disabled switch shows no ring, hover or press.
        let quiet = drawn(pal, 1.0, true, off).pixels;
        for l in [hov, held, focus] {
            let l = Look {
                disabled: true,
                ..l
            };
            assert_eq!(drawn(pal, 1.0, true, l).pixels, quiet);
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
    for r in [
        Rect::new(f32::NAN, 0.0, 40.0, 20.0),
        Rect::new(0.0, 0.0, 40.0, f32::INFINITY),
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
            draw_toggle(&mut p, &LIGHT, r, &Toggle::new(true), look);
            draw_toggle_animated(&mut p, &LIGHT, &mut anim, "t", r, &Toggle::new(true), look);
        }
        assert_eq!(bmp.pixels, blank.pixels, "{r:?}");
    }
}

/// The pixels of a whole frame, and the 4 bytes of the pixel at a logical point.
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

/// A dialog's frame, and the same frame with the control at `ctrl` wiped to the ground beside
/// it and drawn again by `draw`: equal when `draw` draws what the dialog drew.
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

const W: u32 = 900;
const H: u32 = 600;

fn frame_of(scale: f32, draw: impl FnOnce(&mut Painter, Rect)) -> Bitmap {
    let mut bmp = Bitmap::new((W as f32 * scale) as u32, (H as f32 * scale) as u32);
    let mut gfx = Gfx::new();
    {
        let mut p = Painter::new(&mut bmp, &mut gfx, scale);
        draw(&mut p, Rect::new(0.0, 0.0, W as f32, H as f32));
    }
    bmp
}

/// The single-page settings dialog's toggle rows, off and on, at rest, under the pointer and
/// pressed, are this switch's pixels, in both themes and at two scales.
#[test]
fn the_switch_is_the_settings_dialogs_toggle_pixel_for_pixel() {
    for pal in [&LIGHT, &DARK] {
        for scale in [1.0, 1.5] {
            for row in [1usize, 2] {
                for state in 0..3 {
                    let mut f = Form::new(
                        "Settings",
                        vec![
                            Row::header("Editor"),
                            Row::toggle(1, "Word wrap", false),
                            Row::toggle(2, "Line numbers", true).help("At the left"),
                        ],
                    );
                    let a = frame_of(scale, |p, win| {
                        f.layout(p, win);
                        let c = f.control_rect(row);
                        if state >= 1 {
                            f.pointer_move(c.cx(), c.cy());
                        }
                        if state == 2 {
                            f.press(c.cx(), c.cy(), Mods::default(), 1);
                        }
                        f.draw(p, pal, win, false);
                    });
                    let ctrl = f.control_rect(row);
                    assert_eq!((ctrl.w, ctrl.h), (TOGGLE_W, TOGGLE_H));
                    let look = Look {
                        hovered: state >= 1,
                        pressed: state == 2,
                        ..Look::default()
                    };
                    let b = redrawn_over(&a, scale, ctrl, |p| {
                        draw_toggle(p, pal, ctrl, &Toggle::new(row == 2), look);
                    });
                    assert!(
                        a.pixels == b.pixels,
                        "row {row}, state {state}, scale {scale}, dark {}",
                        pal.dark
                    );
                }
            }
        }
    }
}

/// The categorised settings dialog's toggle rows, disabled ones included, are this switch's
/// pixels too.
#[test]
fn the_switch_is_the_categorised_dialogs_toggle_and_its_disabled_look() {
    for pal in [&LIGHT, &DARK] {
        for scale in [1.0, 1.5] {
            for (id, on, disabled) in [(1, false, false), (2, true, false), (3, true, true)] {
                for hover in [false, true] {
                    let mut f = pages::Form::new(
                        "Settings",
                        vec![Page::new(
                            "General",
                            vec![
                                Item::new(Row::toggle(1, "Word wrap", false)),
                                Item::new(Row::toggle(2, "Line numbers", true)),
                                Item::new(Row::toggle(3, "Minimap", true)).disabled("Needs more"),
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
                    let look = Look {
                        hovered: hover,
                        disabled,
                        ..Look::default()
                    };
                    let b = redrawn_over(&a, scale, ctrl, |p| {
                        draw_toggle(p, pal, ctrl, &Toggle::new(on), look);
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

#[test]
fn the_animated_switch_at_rest_is_the_plain_one_and_slides_between() {
    let clock = ManualClock::new();
    let mut anim = Animator::new();
    let mut gfx = Gfx::new();
    let mut frame = |ms: u64, on: bool, look: Look, anim: &mut Animator| {
        // A frame each 16 ms or less: the animator counts at most 50 ms a frame.
        let mut left = ms;
        loop {
            let step = left.min(16);
            left -= step;
            clock.advance_ms(step);
            anim.begin_frame(clock.now());
            let mut bmp = Bitmap::new(80, 48);
            {
                let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
                p.fill(Rect::new(0.0, 0.0, 80.0, 48.0), LIGHT.popup);
                draw_toggle_animated(&mut p, &LIGHT, anim, "wrap", R, &Toggle::new(on), look);
            }
            let more = anim.end_frame().is_some();
            if left == 0 {
                break (bmp, more);
            }
        }
    };
    let plain = Look::default();
    let (off, on) = (
        drawn(&LIGHT, 1.0, false, plain),
        drawn(&LIGHT, 1.0, true, plain),
    );
    // First seen: at rest, whichever way it is set.
    let (first, more) = frame(0, true, plain, &mut anim);
    assert!(!more && first.pixels == on.pixels);
    // Turned off: the on switch still, then between, then exactly the off one.
    let (same, more) = frame(0, false, plain, &mut anim);
    assert!(more && same.pixels == on.pixels);
    let (mid, more) = frame(80, false, plain, &mut anim);
    assert!(more && mid.pixels != on.pixels && mid.pixels != off.pixels);
    // Half way the knob is near the middle: neither end's knob colour is at either end.
    assert_ne!(at(&mid, R.x as u32 + 10, 24), at(&off, R.x as u32 + 10, 24));
    let (end, more) = frame(200, false, plain, &mut anim);
    assert!(!more && end.pixels == off.pixels);
    // Every look at rest is the plain one.
    for look in [
        Look {
            hovered: true,
            ..plain
        },
        Look {
            focused: true,
            ..plain
        },
        Look {
            disabled: true,
            ..plain
        },
    ] {
        let (b, _) = frame(0, false, look, &mut anim);
        assert!(
            b.pixels == drawn(&LIGHT, 1.0, false, look).pixels,
            "{look:?}"
        );
    }
    // Under reduced motion it jumps.
    anim.set_system_reduced(true);
    let (b, more) = frame(0, true, plain, &mut anim);
    assert!(!more && b.pixels == on.pixels);
}

/// The sheet a person looks at: off, on, under the pointer, pressed, focused and disabled, and
/// three frames of the slide, light and dark, at two scales. `cargo test -p mdshared-ui toggle_sheet -- --ignored`, then open
/// target/specimen/toggle.png and toggle-150.png.
#[test]
#[ignore]
fn toggle_sheet() {
    use crate::gfx::Face;
    for (scale, file) in [(1.0, "toggle.png"), (1.5, "toggle-150.png")] {
        let (w, h) = (640.0, 420.0);
        let mut bmp = Bitmap::new((w * scale) as u32, (h * scale) as u32);
        let mut gfx = Gfx::new();
        let mut p = Painter::new(&mut bmp, &mut gfx, scale);
        for (half, pal) in [&LIGHT, &DARK].into_iter().enumerate() {
            let oy = half as f32 * h / 2.0;
            p.fill(Rect::new(0.0, oy, w, h / 2.0), pal.popup);
            let looks = [
                ("rest", Look::default()),
                (
                    "hover",
                    Look {
                        hovered: true,
                        ..Look::default()
                    },
                ),
                (
                    "pressed",
                    Look {
                        hovered: true,
                        pressed: true,
                        ..Look::default()
                    },
                ),
                (
                    "focused",
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
            ];
            for (row, on) in [false, true].into_iter().enumerate() {
                let y = oy + 30.0 + row as f32 * 50.0;
                for (k, (name, look)) in looks.iter().enumerate() {
                    let x = 24.0 + k as f32 * 110.0;
                    let r = Rect::new(x, y, TOGGLE_W, TOGGLE_H);
                    draw_toggle(&mut p, pal, r, &Toggle::new(on), *look);
                    p.text(Face::Ui, 11.5, x + 48.0, r.cy(), name, pal.muted, 60.0);
                }
            }
            // The slide, at a quarter, half and three quarters.
            let y = oy + 140.0;
            for (k, t) in [0.25, 0.5, 0.75].into_iter().enumerate() {
                let r = Rect::new(24.0 + k as f32 * 110.0, y, TOGGLE_W, TOGGLE_H);
                draw_at(&mut p, pal, r, t, Look::default());
                let label = format!("t = {t}");
                p.text(Face::Ui, 11.5, r.x + 48.0, r.cy(), &label, pal.muted, 60.0);
            }
        }
        let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/specimen");
        std::fs::create_dir_all(&out).unwrap();
        bmp.write_png(&out.join(file)).unwrap();
    }
}
