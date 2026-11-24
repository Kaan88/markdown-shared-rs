//! A menu joined to the control that opens it: the control (a menu-bar title, a toolbar
//! button, a field) drawn as a tab of the menu, in the menu's colour with the menu's line round
//! its top and sides, so the two read as one shape hanging from the bar.
//!
//! The caller owns the order and the label. For a frame with a joined menu it places the menu
//! flush under the control (`Menu::around(control)` does, when there is room below), and then:
//!
//! 1. [`join_under`] with the control's rectangle, before the menu: the tab's shadow, so the tab
//!    and the menu cast one shadow;
//! 2. the menu, as always ([`Menu::draw_in`](super::Menu::draw_in), or a cascade's);
//! 3. [`join_over`] with the control's rectangle and [`Menu::rect`](super::Menu::rect): the tab,
//!    the seam between them opened, the menu's corner under a tab at its edge made square, and
//!    a small concave curve where the tab's side meets the menu's top inside its width;
//! 4. the control's label or icon, over the tab, in the menu's text colour.
//!
//! [`joins`] says whether the two stand so that they can join; `join_over` draws nothing and
//! answers false when they do not (the menu was flipped above the control, or pushed sideways
//! off it), and the caller draws its control as it does when no menu is open.
//!
//! Deliberately not here: placing the menu (that is `Menu::around`), the control's label, a
//! joined submenu (submenus stand beside their row, apart), the tab of a menu that opens above
//! its control, and motion: a joined menu that fades in or out is drawn by the caller with the
//! tab inside the same [`anim::pop`](crate::anim::pop) as the menu, or with the tab shown
//! only once the menu is there.

use super::Look;
use crate::gfx::{Painter, Rect, Shade};
use crate::theme::Palette;

/// The largest the concave curve between a tab's side and the menu's top grows, in logical px,
/// whatever the corners' radius.
pub const FILLET: f32 = 8.0;

fn real(r: Rect) -> bool {
    r.x.is_finite()
        && r.y.is_finite()
        && r.w.is_finite()
        && r.h.is_finite()
        && r.w > 0.0
        && r.h > 0.0
}

/// Whether a menu at `menu` joins the control at `tab`: both are real rectangles (finite, of
/// some size), the menu's top is the control's foot (within half a logical px), and the menu
/// spans the control from side to side (within half a px). A menu flipped above its control,
/// pushed sideways off it, or narrower than it does not join.
pub fn joins(tab: Rect, menu: Rect) -> bool {
    real(tab)
        && real(menu)
        && (menu.y - tab.bottom()).abs() <= 0.5
        && menu.x <= tab.x + 0.5
        && menu.right() >= tab.right() - 0.5
}

/// Before the menu is drawn: the shadow of the control at `tab`, the menu's shadow
/// ([`Painter::shadow_with`]) with the look's radius and strength, so that tab and menu cast one
/// shadow; in a look with the [soft shadow](Look::soft_shadow), the control's own soft one
/// ([`Shade::TAB`], at the look's strength, times [`LIGHT_SHADOW`](crate::gfx::LIGHT_SHADOW)
/// on a light palette). Nothing when the look has no shadow, its strength is 0, or `tab` is not
/// a real rectangle. The caller calls it only for a menu that will join ([`joins`], after
/// `Menu::place_in`), since a menu that does not leaves the control as it is.
pub fn join_under(p: &mut Painter, pal: &Palette, look: &Look, tab: Rect) {
    let look = look.sane();
    if look.shadow && real(tab) {
        if look.soft_shadow {
            p.soft_shadow(tab, look.radius, &Shade::TAB, look.soft_strength(pal));
        } else {
            p.shadow_with(tab, look.radius, pal.dark, look.shadow_strength);
        }
    }
}

/// After the menu is drawn: the control at `tab` drawn as the menu's tab, joined to the menu at
/// `menu` (its [`rect`](super::Menu::rect)) flush under it. The tab is filled with the
/// palette's `popup`, with `popup_line` round its top and sides, its top corners rounded by the
/// look's radius; the menu's line under it is opened. Where the tab stands at the menu's edge
/// the menu's corner there is made square, without touching what the menu drew inside its
/// rounded corner. Where the tab's side stands inside the menu's width, a concave curve of
/// `min(radius, FILLET)` px (less when the menu reaches less far past the tab, or the tab is
/// shorter) carries the tab's side line into the menu's top line. A square look (radius 0) has
/// square corners and no curve.
///
/// Returns false, and draws nothing, when the menu does not join the tab ([`joins`]). A tab
/// that stands on the menu's rounded corner without reaching its edge (less than the radius
/// in from it) keeps that corner as the menu drew it.
pub fn join_over(p: &mut Painter, pal: &Palette, look: &Look, tab: Rect, menu: Rect) -> bool {
    if !joins(tab, menu) {
        return false;
    }
    let look = look.sane();
    let r = look.radius;
    // The tab, open at its foot: its frame drawn taller and cut at the menu's top.
    let clip = p.clip(Rect::new(tab.x, tab.y, tab.w, menu.y - tab.y));
    p.frame(
        Rect::new(tab.x, tab.y, tab.w, tab.h + r + 4.0),
        r,
        pal.popup,
        pal.popup_line,
    );
    p.restore(clip);
    let (mx0, my0, mx1, my1) = p.dev_rect(menu);
    let (tx0, ty0, tx1, _) = p.dev_rect(tab);
    // Within half a px a tab is flush with the menu's side; it is never wider than the menu.
    let (tx0, tx1) = (tx0.max(mx0), tx1.min(mx1));
    // The seam: the menu's line under the tab, between the tab's own side lines.
    p.fill_dev(tx0 + 1, my0, tx1 - 1, my0 + 1, pal.popup, 1.0);
    let rad = r * p.scale;
    let fillet = r.min(FILLET).min(tab.h).max(0.0) * p.scale;
    // How far the tab stands from each side of the menu's line, in device px.
    let tall = (my0 - ty0) as f32;
    for side in [Side::Left, Side::Right] {
        let room = match side {
            Side::Left => tx0 - mx0,
            Side::Right => mx1 - tx1,
        };
        if room <= 0 {
            square_corner(p, pal, (mx0, my0, mx1, my1), rad, side);
        } else {
            let f = fillet.min(room as f32).min(tall);
            let at = match side {
                Side::Left => tx0,
                Side::Right => tx1,
            };
            curve(p, pal, at, my0, f, side);
        }
    }
    true
}

#[derive(Clone, Copy, PartialEq)]
enum Side {
    Left,
    Right,
}

/// Makes the menu's top corner on `side` square, as `Painter::frame` draws a box of radius 0:
/// the side's line straight up to the top, the top row (under the tab, so opened) the fill.
/// Only the pixels the rounded frame's fill did not wholly cover are painted, so a row's
/// highlight or anything else drawn inside the corner is kept. `menu` is in device px, `rad`
/// the corner's radius in device px.
fn square_corner(p: &mut Painter, pal: &Palette, menu: (i32, i32, i32, i32), rad: f32, side: Side) {
    let (x0, y0, x1, y1) = menu;
    // The frame's outer box and its fill's box, with the radii `rbox_dev` would give them.
    let outer = rad
        .min((x1 - x0) as f32 / 2.0)
        .min((y1 - y0) as f32 / 2.0)
        .max(0.0);
    let inner = (rad - 1.0)
        .max(0.0)
        .min((x1 - x0 - 2) as f32 / 2.0)
        .min((y1 - y0 - 2) as f32 / 2.0)
        .max(0.0);
    let band = outer.ceil() as i32;
    if band == 0 {
        return;
    }
    let (ix0, iy0, ix1) = (x0 + 1, y0 + 1, x1 - 1);
    for y in y0..(y0 + band).min(y1) {
        for k in 0..band.min(x1 - x0) {
            let x = match side {
                Side::Left => x0 + k,
                Side::Right => x1 - 1 - k,
            };
            if k == 0 {
                p.fill_dev(x, y, x + 1, y + 1, pal.popup_line, 1.0);
                continue;
            }
            // How much of this pixel the fill's rounded box covered.
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            let covered = if y < iy0 || x < ix0 || x >= ix1 {
                0.0
            } else {
                let qx = match side {
                    Side::Left => (ix0 as f32 + inner - fx).max(0.0),
                    Side::Right => (fx - (ix1 as f32 - inner)).max(0.0),
                };
                let qy = (iy0 as f32 + inner - fy).max(0.0);
                if qx <= 0.0 || qy <= 0.0 {
                    1.0
                } else {
                    (inner - qx.hypot(qy) + 0.5).clamp(0.0, 1.0)
                }
            };
            if covered < 1.0 {
                p.fill_dev(x, y, x + 1, y + 1, pal.popup, 1.0);
            }
        }
    }
}

/// The concave curve at the foot of the tab's side at device x `at` (the tab's left edge, or
/// its right edge for `Side::Right`), over the menu's top line at device y `top`: a quarter
/// circle of radius `f` device px, centred `f` outside the tab and `f` above the menu, whose
/// one-pixel line meets the tab's side line at its top and the menu's line at its foot, with
/// the menu's fill between it and the corner. Inside the circle (the bar's side) nothing is
/// drawn. A curve under a device pixel is none.
fn curve(p: &mut Painter, pal: &Palette, at: i32, top: i32, f: f32, side: Side) {
    if f < 1.0 {
        return;
    }
    let cy = top as f32 - f;
    let (cx, xs) = match side {
        // The tab's left line is the column `at`; the curve lies left of it, and over it.
        Side::Left => (at as f32 - f, (at as f32 - f).floor() as i32..at + 1),
        // The tab's right line is the column `at - 1`; the curve lies right of it.
        Side::Right => (at as f32 + f, at - 1..(at as f32 + f).ceil() as i32),
    };
    for y in cy.floor() as i32..top + 1 {
        for x in xs.clone() {
            let (dx, dy) = ((x as f32 + 0.5 - cx).abs(), y as f32 + 0.5 - cy);
            let towards_tab = match side {
                Side::Left => x as f32 + 0.5 >= cx,
                Side::Right => x as f32 + 0.5 <= cx,
            };
            if dy < 0.0 || !towards_tab {
                continue;
            }
            let d = dx.hypot(dy);
            // The line is the ring from `f` to `f + 1` out; the fill lies beyond it.
            let line = (d - f + 0.5).clamp(0.0, 1.0);
            let fill = (d - f - 0.5).clamp(0.0, 1.0);
            if line > 0.0 {
                p.fill_dev(x, y, x + 1, y + 1, pal.popup_line, line);
            }
            if fill > 0.0 {
                p.fill_dev(x, y, x + 1, y + 1, pal.popup, fill);
            }
        }
    }
}

#[cfg(test)]
#[path = "menu_join_tests.rs"]
mod tests;
