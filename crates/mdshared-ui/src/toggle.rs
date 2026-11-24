//! A toggle switch on its own: the Windows 11 pill with a knob at the end that says on, the one
//! the settings dialog ([`settings::Form`](crate::settings::Form)) draws in its toggle rows, for
//! a window that is not a settings dialog.
//!
//! The model, [`Toggle`], holds the value and whether a press on the switch is under way. The
//! caller owns the rectangle (passed in to every call, so nothing goes stale after a resize), the
//! keyboard focus, the label beside the switch and what a change means. Events go in and come
//! back as plain answers: [`Toggle::key`] (Space turns it over), [`Toggle::press`] and
//! [`Toggle::release`] (a click turns it over when the button goes up on the switch it went down
//! on), [`Toggle::set`]. Drawing is [`draw_toggle`], and [`draw_toggle_animated`], whose knob
//! slides and whose fill fades in; at rest that is [`draw_toggle`] pixel for pixel.
//!
//! The pixels are the settings dialog's: both draw the switch with the same function, and a test
//! draws a form's toggle row and this switch over the same ground and compares every pixel. The
//! focus ring is the exception: the dialog paints its own ring with the row's colour inside it,
//! and this switch uses [`focus::draw_ring`](crate::focus::draw_ring), which needs to know
//! nothing of what is behind it.
//!
//! Deliberately not here:
//! - the label and the "On"/"Off" text a Windows switch can show beside it, which the caller
//!   draws with the text it wants;
//! - Left and Right, and Enter: Space is the key of a switch, as on Windows;
//! - a focus model: the caller passes [`Look::focused`] (see [`focus`](crate::focus));
//! - dragging the knob across, which a Windows switch allows and a click covers;
//! - what a screen reader is told. The accessibility layer (roadmap W8, not built yet) reads
//!   the role from the type (a toggle button), the state from [`Toggle::on`], and the name and
//!   the rectangle from the caller, which has the label and passes the rectangle to every call.
//!
//! ```text
//! // events
//! if toggle.key(&key, mods) { apply(toggle.on()) }
//! toggle.press(r, x, y);
//! if toggle.release(r, x, y) { apply(toggle.on()) }
//! // frame
//! let look = Look { hovered: toggle.hit(r, mx, my), pressed: toggle.pressed(), ..Look::default() };
//! draw_toggle_animated(&mut p, &pal, &mut anim, "wrap", r, &toggle, look);
//! ```

use crate::anim::{Animator, AsId, Id, fluent};
use crate::focus::draw_ring;
use crate::gfx::{Painter, Rect, mix};
use crate::keys::{KeyName, Mods};
use crate::theme::Palette;

/// The width of the switch, in logical px, as the settings dialog draws it.
pub const TOGGLE_W: f32 = 40.0;
/// The height of the switch, in logical px.
pub const TOGGLE_H: f32 = 20.0;

/// How much the disabled look washes the switch with the popup colour.
const WASH: f32 = 0.55;

/// A switch's value and the press under way on it. `Copy`: it is two flags.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Toggle {
    on: bool,
    pressed: bool,
}

impl Toggle {
    /// A switch that is `on` or off, with no press under way.
    pub fn new(on: bool) -> Toggle {
        Toggle { on, pressed: false }
    }

    /// Whether the switch is on.
    pub fn on(&self) -> bool {
        self.on
    }

    /// Sets the value, for a change made elsewhere (the setting was changed in a menu). Returns
    /// true if the value changed.
    pub fn set(&mut self, on: bool) -> bool {
        let changed = self.on != on;
        self.on = on;
        changed
    }

    /// Whether a press went down on the switch and the button has not gone up yet: the caller
    /// draws [`Look::pressed`] from this while the pointer is still on it.
    pub fn pressed(&self) -> bool {
        self.pressed
    }

    /// Whether `(x, y)` is on the switch in `r`. A rectangle or a point that is not finite, and a
    /// rectangle with no area, is never hit.
    pub fn hit(&self, r: Rect, x: f32, y: f32) -> bool {
        usable(r) && x.is_finite() && y.is_finite() && r.contains(x, y)
    }

    /// A key while the keyboard is on the switch. Space, with no Ctrl or Alt, turns it over and
    /// returns true; every other key returns false and changes nothing, so the caller can give
    /// it to someone else (Tab to the focus model, say).
    pub fn key(&mut self, key: &KeyName, mods: Mods) -> bool {
        if *key == KeyName::Space && !mods.ctrl && !mods.alt && !mods.control {
            self.on = !self.on;
            self.pressed = false;
            return true;
        }
        false
    }

    /// The button went down at `(x, y)`. On the switch in `r`, the press is remembered and the
    /// answer is true (draw the pressed look); elsewhere any earlier press is dropped and the
    /// answer is false. The value does not change until [`release`](Self::release).
    pub fn press(&mut self, r: Rect, x: f32, y: f32) -> bool {
        self.pressed = self.hit(r, x, y);
        self.pressed
    }

    /// The button went up at `(x, y)`. If the press went down on the switch and the pointer is
    /// still on it, the switch turns over and the answer is true. Otherwise nothing changes and
    /// the answer is false. Either way the press is over.
    pub fn release(&mut self, r: Rect, x: f32, y: f32) -> bool {
        let was = std::mem::take(&mut self.pressed);
        if was && self.hit(r, x, y) {
            self.on = !self.on;
            return true;
        }
        false
    }

    /// Drops a press under way without changing the value (the window lost the pointer).
    /// Returns true if there was one.
    pub fn cancel(&mut self) -> bool {
        std::mem::take(&mut self.pressed)
    }
}

/// How a switch is drawn just now, from the caller's own state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Look {
    /// The pointer is over it: the knob grows and the colours lighten, as in the dialog.
    pub hovered: bool,
    /// The button is down on it: the knob grows a little more.
    pub pressed: bool,
    /// The keyboard is on it: the focus ring round the pill.
    pub focused: bool,
    /// It cannot be changed: drawn with no hover or press look and no ring, washed with the
    /// popup colour as the settings dialog draws a disabled row. That is right on a popup, a
    /// dialog or a card; on another ground the wash shows as a faint halo one px wide.
    pub disabled: bool,
}

/// Draws the switch in `r` (normally [`TOGGLE_W`] by [`TOGGLE_H`]): an accent pill with the
/// knob at the right when on, a field-coloured pill with an edge and the knob at the left when
/// off. It draws only inside `r`, but for the focus ring, which lies three px outside it, and
/// the disabled wash, one px outside. A rectangle that is not finite or has no area draws
/// nothing. It changes no state.
pub fn draw_toggle(p: &mut Painter, pal: &Palette, r: Rect, toggle: &Toggle, look: Look) {
    draw_at(p, pal, r, if toggle.on { 1.0 } else { 0.0 }, look);
}

/// [`draw_toggle`] with motion: when the value turns over, the knob slides to the other end
/// ([`fluent::MOVE`]) while the accent fill fades in or out under it and the knob's colour turns.
/// `id` names this switch in `anim`. At rest, and on every frame when the animator does not
/// animate, it draws exactly what [`draw_toggle`] draws; a switch first seen is at rest. The
/// hover and press looks change at once, as in the dialog.
pub fn draw_toggle_animated(
    p: &mut Painter,
    pal: &Palette,
    anim: &mut Animator,
    id: impl AsId,
    r: Rect,
    toggle: &Toggle,
    look: Look,
) {
    let t = anim.toggle(Id::new(id).with("on"), toggle.on, fluent::MOVE);
    draw_at(p, pal, r, t, look);
}

/// The switch with the knob `t` of the way from off (0) to on (1). At 0 and 1 this is exactly
/// the settings dialog's switch.
fn draw_at(p: &mut Painter, pal: &Palette, r: Rect, t: f32, look: Look) {
    if !usable(r) {
        return;
    }
    let t = if t.is_nan() { 0.0 } else { t.clamp(0.0, 1.0) };
    let live = !look.disabled;
    if look.focused && live {
        draw_ring(p, pal, r, r.h / 2.0);
    }
    let (hovered, pressed) = (look.hovered && live, look.pressed && live);
    if t <= 0.0 || t >= 1.0 {
        switch(p, pal, r, t >= 1.0, hovered, pressed);
    } else {
        let knob = knob_size(hovered, pressed);
        let (off_fill, off_edge) = off_colours(pal, hovered);
        p.frame(r, r.h / 2.0, off_fill, off_edge);
        p.rbox_a(r, r.h / 2.0, on_fill(pal, hovered), t);
        let from = r.x + 4.0;
        let to = r.right() - 4.0 - knob;
        p.rbox(
            Rect::new(from + (to - from) * t, r.cy() - knob / 2.0, knob, knob),
            knob / 2.0,
            mix(pal.muted, pal.on_accent, t),
        );
    }
    if !live {
        p.rbox_a(r.inset(-1.0), r.h / 2.0, pal.popup, WASH);
    }
}

fn knob_size(hovered: bool, pressed: bool) -> f32 {
    if pressed {
        14.0
    } else if hovered {
        13.0
    } else {
        12.0
    }
}

fn on_fill(pal: &Palette, hovered: bool) -> crate::gfx::Rgb {
    if hovered {
        mix(pal.accent, pal.popup, 0.1)
    } else {
        pal.accent
    }
}

/// The fill and the edge of the pill when off.
fn off_colours(pal: &Palette, hovered: bool) -> (crate::gfx::Rgb, crate::gfx::Rgb) {
    let edge = if hovered {
        mix(pal.muted, pal.popup, 0.2)
    } else {
        mix(pal.muted, pal.popup, 0.35)
    };
    let fill = if hovered {
        mix(pal.field, pal.text, 0.05)
    } else {
        pal.field
    };
    (fill, edge)
}

/// The Windows 11 switch: a pill, with the knob at the end that says on. The settings dialog's
/// toggle rows draw with this; it is the drawing that was `settings::draw_toggle`, moved here
/// unchanged so that both draw the same pixels.
pub(crate) fn switch(
    p: &mut Painter,
    pal: &Palette,
    r: Rect,
    on: bool,
    hovered: bool,
    pressed: bool,
) {
    let knob = knob_size(hovered, pressed);
    if on {
        p.rbox(r, r.h / 2.0, on_fill(pal, hovered));
        p.rbox(
            Rect::new(r.right() - 4.0 - knob, r.cy() - knob / 2.0, knob, knob),
            knob / 2.0,
            pal.on_accent,
        );
    } else {
        let (fill, edge) = off_colours(pal, hovered);
        p.frame(r, r.h / 2.0, fill, edge);
        p.rbox(
            Rect::new(r.x + 4.0, r.cy() - knob / 2.0, knob, knob),
            knob / 2.0,
            pal.muted,
        );
    }
}

/// A rectangle that can be drawn and hit: finite, with both sides positive.
fn usable(r: Rect) -> bool {
    [r.x, r.y, r.w, r.h].iter().all(|v| v.is_finite()) && r.w > 0.0 && r.h > 0.0
}

#[cfg(test)]
#[path = "toggle_tests.rs"]
mod tests;
