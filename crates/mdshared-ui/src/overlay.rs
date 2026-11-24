//! Things drawn above the regions that do not depend on any application state: the toast, link
//! hint and tooltip bubbles, and the pieces of a modal dialog (backdrop, card, footer, buttons,
//! keycaps). The application chooses the texts, the card size and the hit ids.

use crate::anim::pop;
use crate::gfx::{Face, LIGHT_SHADOW, Painter, Rect, Shade, mix};
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

    /// Draws the bubble as it enters or leaves: `t` is how present it is, 0 gone and 1 there,
    /// from [`Animator::presence`](crate::anim::Animator::presence) or a
    /// [`Transient`](crate::anim::Transient) that holds the bubble while it leaves. The bubble
    /// fades as a whole, and a toast also rises [`TOAST_RISE`] px into its place and sinks as it
    /// goes; a tooltip and a link hint only fade, since they appear where the eye already is.
    /// At 1 this is [`draw`](Self::draw), pixel for pixel; at 0 it draws nothing. What it
    /// touches is still [`bounds`](Self::bounds), grown downwards by the rise for a toast.
    pub fn draw_presence(&self, p: &mut Painter, pal: &Palette, t: f32) {
        let rise = if self.kind == Kind::Toast {
            TOAST_RISE
        } else {
            0.0
        };
        pop(p, self.bounds(), t, (0.0, rise), |p| self.draw(p, pal));
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
                p.shadow_under_frame(r, 8.0, pal.dark, 1.0);
                p.frame(r, 8.0, pal.popup, pal.popup_line);
                p.text_center(Face::Ui, 13.0, r.inset(10.0), &self.text, pal.text);
            }
            Kind::Tip => {
                p.shadow_under_frame(r, 5.0, pal.dark, 1.0);
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

/// How far below its place a toast starts when it is drawn by
/// [`Bubble::draw_presence`], in logical px.
pub const TOAST_RISE: f32 = 8.0;

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

/// [`draw_backdrop`] at a part of its strength: `t` is how present the dialog is, 0 gone and 1
/// there. At 1 it is `draw_backdrop`.
pub fn draw_backdrop_presence(p: &mut Painter, pal: &Palette, w: f32, h: f32, t: f32) {
    if t.is_nan() || t >= 1.0 {
        return draw_backdrop(p, pal, w, h);
    }
    p.fill_a(
        Rect::new(0.0, 0.0, w, h),
        [0, 0, 0],
        if pal.dark { 0.45 } else { 0.28 } * t.max(0.0),
    );
}

/// Everything a dialog card at `card` touches, its shadow included: the bounds to give
/// [`pop`] so that a dialog fades and rises as one thing.
///
/// ```
/// # use mdshared_ui::{anim::pop, gfx::*, overlay::*, theme::LIGHT};
/// # let (mut bmp, mut gfx) = (mdshared_text::Bitmap::new(400, 300), Gfx::new());
/// # let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
/// # let (pal, t) = (&LIGHT, 0.5);
/// let card = dialog_card(320.0, 200.0, 400.0, 300.0);
/// draw_backdrop_presence(&mut p, pal, 400.0, 300.0, t);
/// pop(&mut p, dialog_bounds(card), t, (0.0, DIALOG_RISE), |p| {
///     let footer = draw_dialog_card(p, pal, card);
///     // ... the dialog's text and buttons
/// #   let _ = footer;
/// });
/// ```
pub fn dialog_bounds(card: Rect) -> Rect {
    card.inset(-18.0)
}

/// How far below its place a dialog starts when it enters through [`pop`], in logical px.
pub const DIALOG_RISE: f32 = 10.0;

/// Draws the card's shadow, body and button footer. Returns the footer rectangle; buttons go
/// at `footer.y + (DIALOG_FOOTER - 32.0) / 2.0`, 32 px high.
pub fn draw_dialog_card(p: &mut Painter, pal: &Palette, r: Rect) -> Rect {
    draw_dialog_card_with(p, pal, r, 10.0, None)
}

/// [`draw_dialog_card`] with corners of `radius` (the footer's follow them) and a choice of
/// shadow: `None` is the toolkit's (twice on a dark palette), `Some(k)` the soft popup shadow
/// a web page draws ([`Painter::soft_shadow`] with [`Shade::POPUP`]) at strength `k`, times
/// [`LIGHT_SHADOW`] on a light palette; 0 draws none. `draw_dialog_card` is this with 10 and
/// `None`, pixel for pixel. The soft shadow reaches further than [`dialog_bounds`]; its
/// bounds are [`soft_bounds`](crate::gfx::soft_bounds)`(card, &Shade::POPUP)`.
pub fn draw_dialog_card_with(
    p: &mut Painter,
    pal: &Palette,
    r: Rect,
    radius: f32,
    soft: Option<f32>,
) -> Rect {
    match soft {
        Some(k) => {
            let k = k * if pal.dark { 1.0 } else { LIGHT_SHADOW };
            p.soft_shadow(r, radius, &Shade::POPUP, k);
        }
        None => {
            // The frame below covers the card's middle opaquely, so the shadow leaves it out:
            // the pixels are those of the whole shadow, at a fraction of the cost under a large
            // dialog.
            p.shadow_under_frame(r, radius, pal.dark, 1.0);
            if pal.dark {
                p.shadow_under_frame(r, radius, true, 1.0);
            }
        }
    }
    p.frame(r, radius, pal.popup, pal.popup_line);
    let footer = Rect::new(
        r.x + 1.0,
        r.bottom() - DIALOG_FOOTER,
        r.w - 2.0,
        DIALOG_FOOTER - 1.0,
    );
    let old = p.clip(footer);
    p.rbox(
        Rect::new(footer.x, footer.y - 12.0, footer.w, footer.h + 12.0),
        (radius - 1.0).max(0.0),
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

/// A dialog button that cannot be pressed: a quiet frame and a faint label, with no hover or
/// press look, since neither would mean anything. It draws exactly what mdedit's approved dialogs
/// draw: `replace_ui::dialog_button` gives a disabled button the same 6 px corners and `field`
/// fill whether it is primary or not, and ignores the role once it is disabled. So does this. It
/// refuses to hint that one button leads while it cannot be pressed; the role parameter is still
/// taken so that a caller passes the button's role and its call site does not change, and is named
/// `_primary` because the drawing does not use it. The caller draws it and records no hit for it;
/// [`draw_dialog_button`] is untouched and draws as it did.
pub fn draw_dialog_button_disabled(
    p: &mut Painter,
    pal: &Palette,
    r: Rect,
    label: &str,
    _primary: bool,
) {
    p.frame(r, 6.0, pal.field, pal.line);
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
    fn a_bubble_that_is_there_draws_as_before_and_one_on_its_way_is_fainter_and_lower() {
        let white = |x: usize, y: usize, bmp: &Bitmap| at(bmp, x as f32, y as f32) == LIGHT.popup;
        for kind in [Kind::Toast, Kind::Tip, Kind::Hint] {
            let bubble = Bubble {
                kind,
                rect: Rect::new(60.0, 14.0, 80.0, 26.0),
                text: "Saved".into(),
            };
            let page = |draw: &dyn Fn(&mut Painter<'_>)| {
                let mut bmp = Bitmap::new(200, 60);
                let mut gfx = Gfx::new();
                let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
                p.fill(PAGE, LIGHT.page);
                draw(&mut p);
                bmp
            };
            let plain = page(&|p| bubble.draw(p, &LIGHT));
            let there = page(&|p| bubble.draw_presence(p, &LIGHT, 1.0));
            let gone = page(&|p| bubble.draw_presence(p, &LIGHT, 0.0));
            let half = page(&|p| bubble.draw_presence(p, &LIGHT, 0.5));
            assert!(there.pixels == plain.pixels, "{kind:?} at 1");
            assert!(gone.pixels == page(&|_| ()).pixels, "{kind:?} at 0");
            assert!(half.pixels != plain.pixels && half.pixels != gone.pixels);
            // Half way the fill is half the popup colour over the page, and a toast is 4 px
            // below its place: its first rows are still the page.
            let low = if kind == Kind::Toast { 4 } else { 0 };
            assert!(white(100, 20, &plain));
            let mid = at(&half, 64.0, 18.0 + low as f32);
            assert_eq!(mid, mix(LIGHT.page, LIGHT.popup, 0.5), "{kind:?}");
            if kind == Kind::Toast {
                let shadow_only = at(&half, 100.0, 15.0);
                assert_ne!(shadow_only, mid, "the toast has not moved down");
            }
            // Nothing outside the bounds (grown by the rise) is touched.
            let b = bubble.bounds();
            let clean = page(&|_| ());
            for y in 0..60usize {
                for x in 0..200usize {
                    let inside = b.contains(
                        x as f32,
                        y as f32 - if y as f32 >= b.bottom() { 9.0 } else { 0.0 },
                    );
                    if !inside {
                        assert_eq!(
                            at(&half, x as f32, y as f32),
                            at(&clean, x as f32, y as f32),
                            "{kind:?} at {x},{y}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn the_backdrop_dims_by_how_present_the_dialog_is() {
        for pal in [&LIGHT, &DARK] {
            let full = shot(pal, |p| draw_backdrop(p, pal, 200.0, 60.0));
            let there = shot(pal, |p| draw_backdrop_presence(p, pal, 200.0, 60.0, 1.0));
            let half = shot(pal, |p| draw_backdrop_presence(p, pal, 200.0, 60.0, 0.5));
            let gone = shot(pal, |p| draw_backdrop_presence(p, pal, 200.0, 60.0, 0.0));
            assert!(there.pixels == full.pixels);
            assert!(gone.pixels == shot(pal, |_| ()).pixels);
            let level = |b: &Bitmap| at(b, 5.0, 5.0)[1] as i32;
            assert!(level(&full) < level(&half) && level(&half) < level(&gone));
        }
        // The bounds of a dialog hold its shadow.
        let card = dialog_card(120.0, 40.0, 200.0, 200.0);
        let b = dialog_bounds(card);
        assert!(b.x < card.x - 14.0 && b.bottom() > card.bottom() + 17.0);
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
                // Both roles are the one disabled look, as mdedit draws them: the accent can no
                // longer show which button leads.
                let other = shot(pal, |p| {
                    draw_dialog_button_disabled(p, pal, BUTTON, "Replace", !primary)
                });
                assert_eq!(
                    quiet.pixels, other.pixels,
                    "a disabled button has one look whatever its role (primary {primary})"
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

    /// The disabled branch of mdedit's `replace_ui::dialog_button`, copied whole as it stands in
    /// that app: a plain 6 px frame in `field` and `line`, and a faint Semibold label, whatever
    /// the button's role. The shared disabled button must draw exactly these pixels.
    fn disabled_before(p: &mut Painter, pal: &Palette, r: Rect, label: &str, _primary: bool) {
        p.frame(r, 6.0, pal.field, pal.line);
        p.text_center(Face::Semibold, 13.0, r, label, pal.faint);
    }

    #[test]
    fn a_disabled_dialog_button_draws_as_mdedits_replace_dialog_draws_it() {
        for pal in [&LIGHT, &DARK] {
            for primary in [false, true] {
                let was = shot(pal, |p| disabled_before(p, pal, BUTTON, "Replace", primary));
                let now = shot(pal, |p| {
                    draw_dialog_button_disabled(p, pal, BUTTON, "Replace", primary)
                });
                assert_eq!(
                    was.pixels,
                    now.pixels,
                    "primary {primary}, {} theme",
                    if pal.dark { "dark" } else { "light" }
                );
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

    /// `draw_dialog_card` as it stood before its shadow left out what the card covers, copied
    /// whole: the whole shadow (twice on a dark palette), then the card and its footer.
    fn card_before(p: &mut Painter, pal: &Palette, r: Rect) -> Rect {
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

    /// A window with a backdrop and `draw` on it at `scale`. Under the backdrop is a pattern
    /// that differs from one pixel to the next, so that a shadow blended where it was not
    /// before cannot round away to the same bytes.
    fn window(pal: &Palette, scale: f32, draw: impl FnOnce(&mut Painter<'_>)) -> Bitmap {
        let mut bmp = Bitmap::new(420, 340);
        for (i, px) in bmp.pixels.chunks_mut(4).enumerate() {
            let v = (i * 37 % 251) as u8;
            px.copy_from_slice(&[v, 255 - v, v / 2 + 60, 255]);
        }
        let mut gfx = Gfx::new();
        {
            let mut p = Painter::new(&mut bmp, &mut gfx, scale);
            p.rbox(Rect::new(30.0, 40.0, 150.0, 90.0), 6.0, pal.accent);
            draw_backdrop(&mut p, pal, 420.0, 340.0);
            draw(&mut p);
        }
        bmp
    }

    #[test]
    fn a_dialog_card_draws_as_it_did_when_its_whole_shadow_was_blended() {
        let cards = [
            Rect::new(40.0, 30.0, 200.0, 160.0),
            Rect::new(17.3, 21.65, 150.45, 111.15),
            Rect::new(33.7, 12.2, 181.9, 123.35),
            Rect::new(60.5, 50.5, 40.0, 70.0),
            Rect::new(80.25, 40.75, 30.1, 20.3),
            Rect::new(100.0, 100.0, 12.0, 4.0),
            Rect::new(-20.4, -10.6, 120.0, 90.0),
            Rect::new(150.9, 120.1, 300.0, 200.0),
        ];
        for pal in [&LIGHT, &DARK] {
            for scale in [1.0, 1.25, 1.5, 1.75, 2.0] {
                for r in cards {
                    let was = window(pal, scale, |p| {
                        card_before(p, pal, r);
                    });
                    let now = window(pal, scale, |p| {
                        draw_dialog_card(p, pal, r);
                    });
                    assert!(
                        was.pixels == now.pixels,
                        "{r:?} at {scale}, dark {}",
                        pal.dark
                    );
                }
            }
        }
    }

    #[test]
    fn a_toast_and_a_tip_draw_as_they_did_when_their_whole_shadow_was_blended() {
        for pal in [&LIGHT, &DARK] {
            for scale in [1.0, 1.25, 1.5, 1.75, 2.0] {
                for kind in [Kind::Toast, Kind::Tip] {
                    for rect in [
                        Rect::new(60.0, 40.0, 180.0, 36.0),
                        Rect::new(41.35, 77.7, 133.3, 23.45),
                    ] {
                        let bubble = Bubble {
                            kind,
                            rect,
                            text: "Saved to the library".to_string(),
                        };
                        let drawn = |whole: bool| {
                            crate::gfx::WHOLE_SHADOW.with(|w| w.set(whole));
                            let bmp = window(pal, scale, |p| bubble.draw(p, pal));
                            crate::gfx::WHOLE_SHADOW.with(|w| w.set(false));
                            bmp.pixels
                        };
                        assert!(
                            drawn(true) == drawn(false),
                            "{kind:?} {rect:?} at {scale}, dark {}",
                            pal.dark
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn a_card_drawn_with_the_classic_corners_and_shadow_is_draw_dialog_card() {
        let r = Rect::new(17.3, 21.65, 150.45, 111.15);
        for pal in [&LIGHT, &DARK] {
            for scale in [1.0, 1.5] {
                let was = window(pal, scale, |p| {
                    card_before(p, pal, r);
                });
                let now = window(pal, scale, |p| {
                    draw_dialog_card_with(p, pal, r, 10.0, None);
                });
                assert!(was.pixels == now.pixels, "at {scale}, dark {}", pal.dark);
            }
        }
    }

    #[test]
    fn a_soft_card_casts_the_popup_shadow_at_its_strength_and_none_at_zero() {
        use crate::gfx::soft_bounds;
        // On white, so the shadow's alpha reads back; the card's middle is its own colour.
        let draw = |pal: &Palette, k: Option<f32>, radius: f32| {
            let mut bmp = Bitmap::new(420, 340);
            bmp.clear([255, 255, 255]);
            let mut gfx = Gfx::new();
            {
                let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
                draw_dialog_card_with(&mut p, pal, Rect::new(60.0, 40.0, 300.0, 200.0), radius, k);
            }
            bmp
        };
        let alpha = |b: &Bitmap, y: f32| 1.0 - at(b, 210.0, y)[1] as f32 / 255.0;
        let dark = draw(&DARK, Some(0.6), 8.0);
        // Just under the card about a third dark, a sixth 8 px down, nothing 46 px down.
        assert!(
            (alpha(&dark, 240.0) - 0.35).abs() < 0.02,
            "{}",
            alpha(&dark, 240.0)
        );
        assert!(
            (alpha(&dark, 248.0) - 0.18).abs() < 0.02,
            "{}",
            alpha(&dark, 248.0)
        );
        assert_eq!(alpha(&dark, 286.0), 0.0);
        // Above the card it is faint: the shadow falls.
        assert!(alpha(&dark, 39.0) < 0.12);
        // .45 of it on a light palette; none at strength 0, and nothing past its bounds.
        let light = draw(&LIGHT, Some(0.6), 8.0);
        let (l, d) = (alpha(&light, 240.0), alpha(&dark, 240.0));
        // Each layer at .45: under half as dark, a little over .45 of it where they overlap.
        assert!(l > 0.4 * d && l < 0.5 * d, "light {l}, dark {d}");
        let none = draw(&DARK, Some(0.0), 8.0);
        assert_eq!(alpha(&none, 240.0), 0.0);
        assert_eq!(alpha(&none, 39.0), 0.0);
        let b = soft_bounds(Rect::new(60.0, 40.0, 300.0, 200.0), &Shade::POPUP);
        for y in 0..340 {
            for x in [0, 30, 210, 400] {
                if !b.contains(x as f32 + 0.5, y as f32 + 0.5) {
                    assert_eq!(at(&dark, x as f32, y as f32), [255, 255, 255], "({x}, {y})");
                }
            }
        }
        // The corners follow the radius: a square card's corner pixel is its line, an 8 px
        // card's is outside its curve.
        let square = draw(&DARK, Some(0.6), 0.0);
        assert_eq!(at(&square, 60.0, 40.0), DARK.popup_line);
        assert_ne!(at(&dark, 60.0, 40.0), DARK.popup_line);
    }
}
