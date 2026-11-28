use super::*;
use crate::colour_picker::{Cache, HexForm};
use crate::gfx::Gfx;
use crate::theme::{DARK, LIGHT};
use crate::tokens::Tokens;
use mdshared_text::Bitmap;

const OLD: Rgb = [0x12, 0x34, 0x56];
const NEW: Rgb = [0xab, 0xcd, 0xef];

fn open() -> (Picker, ConfirmationLayout) {
    let mut flyout = confirmation_flyout(Rect::new(30.0, 20.0, 50.0, 30.0));
    flyout.place_in(Rect::new(0.0, 0.0, 360.0, 400.0));
    let mut picker = Picker::new(OLD, HexForm::DEFAULT);
    picker.set_rgb(NEW);
    (picker, ConfirmationLayout::new(&flyout))
}

#[test]
fn apply_and_cancel_act_only_on_release_over_the_same_button() {
    for cancel in [false, true] {
        let (mut p, l) = open();
        let (r, other) = if cancel {
            (l.cancel, l.apply)
        } else {
            (l.apply, l.cancel)
        };
        assert_eq!(
            p.release_with_confirmation(&l, r.cx(), r.cy()),
            Confirmation::Open
        );
        assert_eq!(
            p.press_with_confirmation(&l, r.cx(), r.cy()),
            Confirmation::Open
        );
        assert_eq!(p.rgb(), NEW);
        assert_eq!(
            p.release_with_confirmation(&l, other.cx(), other.cy()),
            Confirmation::Open
        );
        assert_eq!(
            p.press_with_confirmation(&l, r.cx(), r.cy()),
            Confirmation::Open
        );
        assert_eq!(
            p.release_with_confirmation(&l, r.cx(), r.cy()),
            if cancel {
                Confirmation::Cancelled
            } else {
                Confirmation::Applied(NEW)
            },
        );
        assert_eq!(p.rgb(), if cancel { OLD } else { NEW });
        assert_eq!(p.hex(), if cancel { "#123456" } else { "#abcdef" });
        assert_eq!(p.confirmation_focus(), None);
    }
}

#[test]
fn a_press_let_go_answers_nothing_and_keeps_the_colour() {
    for r in [open().1.apply, open().1.cancel] {
        let (mut p, l) = open();
        assert!(!p.let_go(), "nothing held");
        p.press_with_confirmation(&l, r.cx(), r.cy());
        assert!(p.let_go());
        assert!(!p.let_go(), "once");
        // The release over the same button, which would answer, answers nothing.
        assert_eq!(
            p.release_with_confirmation(&l, r.cx(), r.cy()),
            Confirmation::Open
        );
        assert_eq!(p.rgb(), NEW);
    }
    // A part held: let go where the drag left it, and a move after it drags nothing.
    let (mut p, l) = open();
    let sq = l.picker.square;
    p.press_with_confirmation(&l, sq.x + 10.0, sq.y + 10.0);
    p.pointer_with_confirmation(&l, sq.x + 40.0, sq.y + 30.0);
    let dragged = p.rgb();
    assert!(p.held().is_some());
    assert!(p.let_go());
    assert_eq!(p.held(), None);
    p.pointer_with_confirmation(&l, sq.right() - 2.0, sq.bottom() - 2.0);
    assert_eq!(p.rgb(), dragged);
}

#[test]
fn enter_applies_and_escape_restores_the_opening_colour() {
    let (mut p, _) = open();
    assert_eq!(
        p.key_with_confirmation(&KeyName::Enter, Mods::default(), None),
        Confirmation::Applied(NEW)
    );
    assert_eq!(
        p.key_with_confirmation(&KeyName::Escape, Mods::default(), None),
        Confirmation::Cancelled
    );
    assert_eq!(p.rgb(), OLD);
    assert_eq!(p.field().text(), "#123456");
}

#[test]
fn outside_press_applies_but_outside_release_and_invalid_coordinates_do_not() {
    let (mut p, l) = open();
    let sq = l.picker.square;
    assert_eq!(
        p.press_with_confirmation(&l, sq.x, sq.y),
        Confirmation::Open
    );
    assert_eq!(p.rgb(), [255; 3]);
    assert!(p.pointer_with_confirmation(&l, -20.0, sq.y + sq.h / 2.0));
    assert_eq!(p.rgb(), [128; 3]);
    assert_eq!(
        p.release_with_confirmation(&l, -20.0, sq.y),
        Confirmation::Open
    );
    assert_eq!(
        p.press_with_confirmation(&l, f32::NAN, sq.y),
        Confirmation::Open
    );
    assert_eq!(
        p.press_with_confirmation(&l, 30.0, 30.0),
        Confirmation::Applied([128; 3])
    );
}

#[test]
fn tab_visits_both_buttons_in_order_and_space_activates_the_focused_button() {
    let (mut p, _) = open();
    let mods = Mods::default();
    for (part, button) in [
        (Part::Hue, None),
        (Part::Hex, None),
        (Part::Square, Some(Button::Apply)),
        (Part::Square, Some(Button::Cancel)),
        (Part::Square, None),
    ] {
        assert_eq!(
            p.key_with_confirmation(&KeyName::Tab, mods, None),
            Confirmation::Open
        );
        assert_eq!((p.focus(), p.confirmation_focus()), (part, button));
    }
    let shift = Mods {
        shift: true,
        ..mods
    };
    p.key_with_confirmation(&KeyName::Tab, shift, None);
    assert_eq!(p.confirmation_focus(), Some(Button::Cancel));
    p.key_with_confirmation(&KeyName::Tab, shift, None);
    assert_eq!(p.confirmation_focus(), Some(Button::Apply));
    assert_eq!(
        p.key_with_confirmation(&KeyName::Space, mods, None),
        Confirmation::Applied(NEW)
    );
    p.key_with_confirmation(&KeyName::Tab, shift, None);
    assert_eq!(
        p.key_with_confirmation(&KeyName::Space, mods, None),
        Confirmation::Cancelled
    );
}

#[test]
fn short_placement_keeps_both_buttons_inside_and_below_the_hex_field() {
    let mut flyout = confirmation_flyout(Rect::default());
    let mut picker = Picker::new(OLD, HexForm::DEFAULT);
    assert_eq!(
        picker.press_with_confirmation(&ConfirmationLayout::new(&flyout), 10.0, 10.0),
        Confirmation::Open,
    );
    for height in [50.0, 100.0, 200.0, 400.0] {
        let card = flyout.place_in(Rect::new(0.0, 0.0, 160.0, height));
        let l = ConfirmationLayout::new(&flyout);
        assert!(l.picker.hex.bottom() <= l.apply.y);
        assert!(l.apply.right() < l.cancel.x);
        for r in [l.apply, l.cancel] {
            assert!(r.w > 0.0 && r.h > 0.0);
            assert!(card.contains(r.x, r.y) && card.contains(r.right(), r.bottom()));
        }
    }
}

fn shot(pal: &Palette, high_contrast: bool, scale: f32) -> (Bitmap, ConfirmationLayout) {
    let (mut picker, l) = open();
    let mut bmp = Bitmap::new((360.0 * scale) as u32, (400.0 * scale) as u32);
    bmp.clear(pal.page);
    for _ in 0..3 {
        picker.key_with_confirmation(&KeyName::Tab, Mods::default(), None);
    }
    picker.pointer_with_confirmation(&l, l.cancel.cx(), l.cancel.cy());
    draw_picker_with_confirmation(
        &mut Painter::new(&mut bmp, &mut Gfx::new(), scale),
        pal,
        &l,
        &mut picker,
        &mut Cache::new(),
        ConfirmationLook {
            picker: Look {
                ring: true,
                caret_on: false,
            },
            high_contrast,
        },
    );
    (bmp, l)
}

#[test]
fn the_picker_paints_both_buttons_and_the_keyboard_focus_ring() {
    for (pal, hc) in [
        (LIGHT, false),
        (DARK, false),
        (Tokens::high_contrast(true).palette, true),
    ] {
        for scale in [1.0, 1.5] {
            let (bmp, l) = shot(&pal, hc, scale);
            let pixel = |x: f32, y: f32| {
                let o = ((y * scale) as usize * bmp.width as usize + (x * scale) as usize) * 4;
                [bmp.pixels[o + 2], bmp.pixels[o + 1], bmp.pixels[o]]
            };
            assert_eq!(pixel(l.apply.x + 10.0, l.apply.cy()), pal.accent);
            assert_eq!(pixel(l.apply.cx(), l.apply.y - 2.0), pal.accent);
            let cancel = if hc {
                crate::gfx::mix(pal.popup, pal.text, 0.04)
            } else {
                pal.hover
            };
            assert_eq!(pixel(l.cancel.x + 10.0, l.cancel.cy()), cancel);
        }
    }
}

#[test]
#[ignore = "writes images for visual review"]
fn confirmation_specimen() {
    let dir = std::env::var_os("CARGO_TARGET_DIR")
        .map_or_else(
            || std::path::PathBuf::from("../../target"),
            std::path::PathBuf::from,
        )
        .join("specimen");
    std::fs::create_dir_all(&dir).unwrap();
    for (name, pal) in [("light", LIGHT), ("dark", DARK)] {
        for scale in [1.0, 1.5] {
            let (bmp, _) = shot(&pal, false, scale);
            bmp.write_png(&dir.join(format!("colour-confirm-{name}-{scale}.png")))
                .unwrap();
        }
    }
}
