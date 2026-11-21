//! Things drawn above the regions that do not depend on any application state: the toast, link
//! hint and tooltip bubbles, and the pieces of a modal dialog (backdrop, card, footer, buttons,
//! keycaps). The application chooses the texts, the card size and the hit ids.

use crate::gfx::{Face, Painter, Rect, mix};
use crate::theme::Palette;

// ---- toast, link hint, tooltip ---------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Kind {
    Toast,
    Hint,
    Tip,
}

/// A small text popup, placed but not yet drawn.
pub struct Bubble {
    pub kind: Kind,
    pub rect: Rect,
    pub text: String,
}

impl Bubble {
    /// What drawing it touches, shadow included.
    pub fn bounds(&self) -> Rect {
        match self.kind {
            Kind::Hint => self.rect.inset(-2.0),
            _ => Rect::new(
                self.rect.x - 18.0,
                self.rect.y - 16.0,
                self.rect.w + 36.0,
                self.rect.h + 38.0,
            ),
        }
    }

    /// A status message centred near the bottom of `page`.
    pub fn toast(p: &mut Painter, text: &str, page: Rect) -> Bubble {
        let w = (p.measure(Face::Ui, 13.0, text) + 36.0)
            .min(page.w - 32.0)
            .max(80.0);
        let rect = Rect::new(
            (page.cx() - w / 2.0).round(),
            (page.bottom() - 36.0 - 28.0).round(),
            w.round(),
            36.0,
        );
        Bubble {
            kind: Kind::Toast,
            rect,
            text: text.to_string(),
        }
    }

    /// A one-line hint in the bottom left corner of `page` (the link under the pointer).
    pub fn hint(p: &mut Painter, text: &str, page: Rect) -> Bubble {
        let max = (page.w * 0.7).min(640.0);
        let w = (p.measure(Face::Ui, 12.0, text) + 20.0).min(max);
        let rect = Rect::new(page.x + 6.0, page.bottom() - 6.0 - 24.0, w.ceil(), 24.0);
        Bubble {
            kind: Kind::Hint,
            rect,
            text: text.to_string(),
        }
    }

    /// A tooltip under `anchor` (above it when there is no room) in a `win_w` x `win_h` window.
    pub fn tip(p: &mut Painter, text: &str, anchor: Rect, win_w: f32, win_h: f32) -> Bubble {
        let w = (p.measure(Face::Ui, 12.0, text) + 18.0).min(win_w - 16.0);
        let h = 26.0;
        let x = (anchor.cx() - w / 2.0).clamp(8.0, (win_w - w - 8.0).max(8.0));
        let mut y = anchor.bottom() + 6.0;
        if y + h > win_h - 4.0 {
            y = anchor.y - h - 6.0;
        }
        Bubble {
            kind: Kind::Tip,
            rect: Rect::new(x.round(), y.round(), w.ceil(), h),
            text: text.to_string(),
        }
    }

    pub fn draw(&self, p: &mut Painter, pal: &Palette) {
        let r = self.rect;
        match self.kind {
            Kind::Hint => {
                p.frame(r, 5.0, pal.popup, pal.popup_line);
                p.text(
                    Face::Ui,
                    12.0,
                    r.x + 10.0,
                    r.cy(),
                    &self.text,
                    pal.muted,
                    r.w - 20.0,
                );
            }
            Kind::Toast => {
                p.shadow(r, 8.0, pal.dark);
                p.frame(r, 8.0, pal.popup, pal.popup_line);
                p.text_center(Face::Ui, 13.0, r.inset(10.0), &self.text, pal.text);
            }
            Kind::Tip => {
                p.shadow(r, 5.0, pal.dark);
                p.frame(r, 5.0, pal.popup, pal.popup_line);
                p.text(
                    Face::Ui,
                    12.0,
                    r.x + 9.0,
                    r.cy(),
                    &self.text,
                    pal.text,
                    r.w - 18.0,
                );
            }
        }
    }
}

// ---- dialogs ---------------------------------------------------------------------------------

/// Padding inside a dialog card, and the height of its button footer.
pub const DIALOG_PAD: f32 = 24.0;
pub const DIALOG_FOOTER: f32 = 72.0;
/// A `cw` x `ch` card centred in a `w` x `h` window, shrunk to fit small windows.
pub fn dialog_card(cw: f32, ch: f32, w: f32, h: f32) -> Rect {
    let cw = cw.min(w - 32.0).max(200.0);
    let ch = ch.min(h - 48.0).max(160.0);
    Rect::new(
        ((w - cw) / 2.0).round(),
        ((h - ch) / 2.0).round(),
        cw.round(),
        ch.round(),
    )
}

/// Dims the whole `w` x `h` window behind a dialog.
pub fn draw_backdrop(p: &mut Painter, pal: &Palette, w: f32, h: f32) {
    p.fill_a(
        Rect::new(0.0, 0.0, w, h),
        [0, 0, 0],
        if pal.dark { 0.45 } else { 0.28 },
    );
}

/// Draws the card's shadow, body and button footer. Returns the footer rectangle; buttons go
/// at `footer.y + (DIALOG_FOOTER - 32.0) / 2.0`, 32 px high.
pub fn draw_dialog_card(p: &mut Painter, pal: &Palette, r: Rect) -> Rect {
    p.shadow(r, 10.0, pal.dark);
    if pal.dark {
        p.shadow(r, 10.0, true);
    }
    p.frame(r, 10.0, pal.popup, pal.popup_line);
    let footer = Rect::new(
        r.x + 1.0,
        r.bottom() - DIALOG_FOOTER,
        r.w - 2.0,
        DIALOG_FOOTER - 1.0,
    );
    let old = p.clip(footer);
    p.rbox(
        Rect::new(footer.x, footer.y - 12.0, footer.w, footer.h + 12.0),
        9.0,
        mix(
            pal.popup,
            if pal.dark { [0, 0, 0] } else { pal.bar },
            if pal.dark { 0.22 } else { 0.7 },
        ),
    );
    p.restore(old);
    let px = p.px();
    p.fill(Rect::new(footer.x, footer.y, footer.w, px), pal.popup_line);
    footer
}

/// A dialog button: the accent fill when `primary`, a quiet frame otherwise. `pressed` is "a
/// press that started on this button is held"; it shows only while `hovered`.
pub fn draw_dialog_button(
    p: &mut Painter,
    pal: &Palette,
    r: Rect,
    label: &str,
    primary: bool,
    hovered: bool,
    pressed: bool,
) {
    let press = pressed && hovered;
    if primary {
        let fill = if press {
            mix(pal.accent, pal.popup, 0.2)
        } else if hovered {
            mix(pal.accent, pal.popup, 0.1)
        } else {
            pal.accent
        };
        p.rbox(r, 5.0, fill);
        p.text_center(Face::Semibold, 13.0, r, label, pal.on_accent);
    } else {
        let fill = if press {
            pal.press
        } else if hovered {
            pal.hover
        } else {
            mix(pal.popup, pal.text, 0.04)
        };
        p.frame(r, 5.0, fill, pal.popup_line);
        p.text_center(Face::Ui, 13.0, r, label, pal.text);
    }
}

/// A dialog button that cannot be pressed: a quiet fill and a faint label, with no hover or press
/// look, since neither would mean anything. A primary button keeps a hint of the accent so that
/// the footer's hierarchy still reads, but nothing about it looks pressable. The caller draws it
/// and records no hit for it; [`draw_dialog_button`] is untouched and draws as it did.
pub fn draw_dialog_button_disabled(
    p: &mut Painter,
    pal: &Palette,
    r: Rect,
    label: &str,
    primary: bool,
) {
    if primary {
        p.rbox(r, 5.0, mix(pal.field, pal.accent, 0.35));
    } else {
        p.frame(r, 5.0, pal.field, pal.line);
    }
    p.text_center(Face::Semibold, 13.0, r, label, pal.faint);
}

/// The width a dialog button with this label gets.
pub fn dialog_button_w(p: &mut Painter, label: &str) -> f32 {
    (p.measure(Face::Semibold, 13.0, label) + 40.0)
        .max(96.0)
        .round()
}

/// Splits "Ctrl+Shift+F" into keys, keeping "Ctrl++" as Ctrl and "+".
pub fn key_parts(combo: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = combo;
    while let Some(i) = rest.find('+') {
        if i == 0 {
            break;
        }
        out.push(&rest[..i]);
        rest = &rest[i + 1..];
    }
    if !rest.is_empty() {
        out.push(rest);
    }
    out
}

/// Draws a shortcut as keycaps, right-aligned inside `row` with 12 px of padding. `keys` lists
/// alternatives separated by ", " (shown with "or" between), each like "Ctrl+Shift+F".
pub fn draw_keycaps(p: &mut Painter, pal: &Palette, row: Rect, keys: &str) {
    let mut x = row.right() - 12.0;
    let alts: Vec<&str> = keys.split(", ").collect();
    for (ai, alt) in alts.iter().enumerate().rev() {
        for part in key_parts(alt).iter().rev() {
            let kw = (p.measure(Face::Ui, 12.0, part) + 14.0).max(24.0).round();
            x -= kw;
            let cap = Rect::new(x, row.cy() - 11.0, kw, 22.0);
            p.frame(
                cap,
                4.0,
                mix(pal.popup, pal.text, 0.07),
                mix(pal.popup_line, pal.text, 0.08),
            );
            let px = p.px();
            p.fill(
                Rect::new(cap.x + 3.0, cap.bottom() - 1.0 - px, cap.w - 6.0, px),
                mix(pal.popup_line, pal.text, 0.12),
            );
            p.text_center(Face::Ui, 12.0, cap, part, pal.text);
            x -= 4.0;
        }
        if ai > 0 {
            let ow = p.measure(Face::Ui, 12.0, "or");
            x -= ow + 4.0;
            p.text(Face::Ui, 12.0, x, row.cy(), "or", pal.faint, ow + 2.0);
            x -= 8.0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gfx::Gfx;
    use crate::theme::{DARK, LIGHT};
    use mdshared_text::Bitmap;

    const PAGE: Rect = Rect::new(0.0, 0.0, 200.0, 60.0);
    const BUTTON: Rect = Rect::new(20.0, 12.0, 96.0, 32.0);

    /// A popup page with `draw` on it.
    fn shot(pal: &Palette, draw: impl FnOnce(&mut Painter<'_>)) -> Bitmap {
        let mut bmp = Bitmap::new(200, 60);
        let mut gfx = Gfx::new();
        {
            let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
            p.fill(PAGE, pal.popup);
            draw(&mut p);
        }
        bmp
    }

    fn at(bmp: &Bitmap, x: f32, y: f32) -> [u8; 3] {
        let o = (y as usize * bmp.width as usize + x as usize) * 4;
        [bmp.pixels[o + 2], bmp.pixels[o + 1], bmp.pixels[o]]
    }

    #[test]
    fn a_disabled_dialog_button_draws_differently_from_an_enabled_one() {
        for pal in [&LIGHT, &DARK] {
            for primary in [false, true] {
                let enabled = shot(pal, |p| {
                    draw_dialog_button(p, pal, BUTTON, "Replace", primary, false, false)
                });
                let quiet = shot(pal, |p| {
                    draw_dialog_button_disabled(p, pal, BUTTON, "Replace", primary)
                });
                assert_ne!(enabled.pixels, quiet.pixels, "primary {primary}");
                // Nothing about it looks pressable: no accent fill under the label.
                assert_ne!(
                    at(&quiet, BUTTON.x + 6.0, BUTTON.cy()),
                    pal.accent,
                    "primary {primary}: a disabled button looks pressable"
                );
                // A disabled primary still reads as the leading button and a disabled secondary
                // does not.
                let other = shot(pal, |p| {
                    draw_dialog_button_disabled(p, pal, BUTTON, "Replace", !primary)
                });
                assert_ne!(
                    quiet.pixels, other.pixels,
                    "the two disabled looks are the same"
                );
            }
            let accent = shot(pal, |p| {
                draw_dialog_button(p, pal, BUTTON, "Replace", true, false, false)
            });
            let quiet = shot(pal, |p| {
                draw_dialog_button_disabled(p, pal, BUTTON, "Replace", true)
            });
            assert_eq!(at(&accent, BUTTON.x + 6.0, BUTTON.cy()), pal.accent);
            assert_ne!(at(&quiet, BUTTON.x + 6.0, BUTTON.cy()), pal.accent);
        }
    }

    /// `draw_dialog_button` as it was when this test was written, copied whole: the enabled button
    /// must keep drawing exactly these pixels, in every pointer state.
    fn before(
        p: &mut Painter,
        pal: &Palette,
        r: Rect,
        label: &str,
        primary: bool,
        hovered: bool,
        pressed: bool,
    ) {
        let press = pressed && hovered;
        if primary {
            let fill = if press {
                mix(pal.accent, pal.popup, 0.2)
            } else if hovered {
                mix(pal.accent, pal.popup, 0.1)
            } else {
                pal.accent
            };
            p.rbox(r, 5.0, fill);
            p.text_center(Face::Semibold, 13.0, r, label, pal.on_accent);
        } else {
            let fill = if press {
                pal.press
            } else if hovered {
                pal.hover
            } else {
                mix(pal.popup, pal.text, 0.04)
            };
            p.frame(r, 5.0, fill, pal.popup_line);
            p.text_center(Face::Ui, 13.0, r, label, pal.text);
        }
    }

    #[test]
    fn the_dialog_button_draws_as_it_did_before_the_disabled_one_was_added() {
        for pal in [&LIGHT, &DARK] {
            for primary in [false, true] {
                for (hovered, pressed) in
                    [(false, false), (true, false), (false, true), (true, true)]
                {
                    let was = shot(pal, |p| {
                        before(p, pal, BUTTON, "Replace", primary, hovered, pressed)
                    });
                    let now = shot(pal, |p| {
                        draw_dialog_button(p, pal, BUTTON, "Replace", primary, hovered, pressed)
                    });
                    assert_eq!(
                        was.pixels, now.pixels,
                        "primary {primary}, hovered {hovered}, pressed {pressed}"
                    );
                }
            }
        }
    }

    #[test]
    fn keycaps() {
        assert_eq!(key_parts("Ctrl+Shift+F"), ["Ctrl", "Shift", "F"]);
        assert_eq!(key_parts("Ctrl++"), ["Ctrl", "+"]);
        assert_eq!(key_parts("Ctrl+-"), ["Ctrl", "-"]);
        assert_eq!(key_parts("F5"), ["F5"]);
    }

    #[test]
    fn cards_fit_small_windows() {
        let r = dialog_card(600.0, 700.0, 520.0, 360.0);
        assert!(r.x >= 0.0 && r.y >= 0.0 && r.right() <= 520.0 && r.bottom() <= 360.0);
        let r = dialog_card(460.0, 236.0, 1000.0, 700.0);
        assert_eq!((r.x, r.y, r.w, r.h), (270.0, 232.0, 460.0, 236.0));
    }
}
