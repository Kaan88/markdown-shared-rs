//! Confirmation for popup pickers. The caller owns placement, events and the saved colour;
//! this layer handles the buttons and tells the caller whether to close.

use super::{CONTENT_H, CONTENT_W, GAP, Layout, Look, Outcome, Part, Picker, ROW_H, focus_ring};
use crate::flyout::Flyout;
use crate::gfx::{Painter, Rect, Rgb};
use crate::keys::{KeyName, Mods};
use crate::overlay::draw_dialog_button;
use crate::theme::Palette;

/// A flyout sized for the picker and its confirmation buttons. Place it and pass it to
/// [`ConfirmationLayout::new`] before handling input or drawing.
pub fn confirmation_flyout(anchor: Rect) -> Flyout {
    let mut flyout = super::flyout(anchor);
    flyout.set_content_size((CONTENT_W, CONTENT_H + GAP + ROW_H));
    flyout
}

/// The two buttons in the confirmation row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Apply,
    Cancel,
}

/// Whether a popup event closes the picker. The caller closes on either terminal result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Confirmation {
    /// Close and keep this colour.
    Applied(Rgb),
    /// Close without saving. The picker itself has gone back to its opening colour.
    Cancelled,
    /// Keep the popup open. Its preview or focus may have changed.
    Open,
}

/// The card and its controls in logical pixels. Recompute after placing or moving the flyout.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ConfirmationLayout {
    card: Rect,
    /// The square, hue strip, swatches and hex field.
    pub picker: Layout,
    /// The Apply button.
    pub apply: Rect,
    /// The Cancel button.
    pub cancel: Rect,
}

impl ConfirmationLayout {
    /// Lays out the picker inside a placed flyout. In a short window the picker shrinks
    /// vertically first, leaving room for the buttons. An empty flyout has no button hits.
    pub fn new(flyout: &Flyout) -> Self {
        let card = flyout.rect();
        let content = flyout.content_rect();
        let h = content.h.clamp(0.0, CONTENT_H + GAP + ROW_H);
        let w = content.w.clamp(0.0, CONTENT_W);
        let row_h = h.min(ROW_H);
        let base_h = (h - row_h - GAP).max(0.0);
        let mut picker = Layout::new(content);
        // Keep the hex field and hue strip full height when only the square needs to shrink.
        let lost = CONTENT_H - base_h;
        let square_lost = lost.min(picker.square.h);
        picker.square.h -= square_lost;
        for r in [
            &mut picker.hue,
            &mut picker.old,
            &mut picker.new,
            &mut picker.hex,
        ] {
            r.y -= square_lost;
        }
        if lost > square_lost {
            let scale = base_h / (CONTENT_H - square_lost);
            for r in [
                &mut picker.hue,
                &mut picker.old,
                &mut picker.new,
                &mut picker.hex,
            ] {
                r.y = content.y + (r.y - content.y) * scale;
                r.h *= scale;
            }
        }
        let gap = 8.0_f32.min(w / 3.0);
        let button_w = 84.0_f32.min((w - gap) / 2.0);
        let y = content.y + h - row_h;
        let cancel = Rect::new(content.x + w - button_w, y, button_w, row_h);
        let apply = Rect::new(cancel.x - gap - button_w, y, button_w, row_h);
        Self {
            card,
            picker,
            apply,
            cancel,
        }
    }

    fn button_at(&self, x: f32, y: f32) -> Option<Button> {
        [(Button::Apply, self.apply), (Button::Cancel, self.cancel)]
            .into_iter()
            .find(|(_, r)| r.w > 0.0 && r.h > 0.0 && r.contains(x, y))
            .map(|(button, _)| button)
    }
}

impl Picker {
    /// The button with keyboard focus, if any. Otherwise [`Picker::focus`] gives the part
    /// with focus.
    pub fn confirmation_focus(&self) -> Option<Button> {
        self.button_focus
    }

    fn finish_confirmation(&mut self, button: Button) -> Confirmation {
        self.release();
        self.button_held = None;
        self.button_hover = None;
        self.button_focus = None;
        match button {
            Button::Apply => {
                self.show_hex();
                Confirmation::Applied(self.rgb())
            }
            Button::Cancel => {
                self.cancel();
                Confirmation::Cancelled
            }
        }
    }

    /// Sends a press to the popup. Outside the placed card it applies at once, including on
    /// the anchor when the anchor is outside the card. A button instead waits for release.
    /// Non-finite coordinates and an unplaced card never apply a colour.
    pub fn press_with_confirmation(
        &mut self,
        layout: &ConfirmationLayout,
        x: f32,
        y: f32,
    ) -> Confirmation {
        self.release();
        self.button_held = None;
        if !x.is_finite() || !y.is_finite() || !(layout.card.w > 0.0 && layout.card.h > 0.0) {
            return Confirmation::Open;
        }
        if !layout.card.contains(x, y) {
            return self.finish_confirmation(Button::Apply);
        }
        self.button_hover = layout.button_at(x, y);
        if let Some(button) = self.button_hover {
            self.set_focus(Part::Square);
            self.button_focus = Some(button);
            self.button_held = Some(button);
        } else if layout.picker.hex.bottom() > layout.picker.square.y {
            let l = &layout.picker;
            if self.press(l, x, y) != Outcome::None || self.held().is_some() {
                self.button_focus = None;
            }
        }
        Confirmation::Open
    }

    /// Moves the pointer, updating the button hover or dragging a held colour control.
    /// Returns whether the popup needs to be redrawn. Hex selection is still the caller's
    /// job, using the field and the caret stops from [`super::Cache`].
    pub fn pointer_with_confirmation(
        &mut self,
        layout: &ConfirmationLayout,
        x: f32,
        y: f32,
    ) -> bool {
        let hover = layout.button_at(x, y);
        let redraw = std::mem::replace(&mut self.button_hover, hover) != hover;
        self.drag(&layout.picker, x, y) != Outcome::None || redraw
    }

    /// Ends a drag or activates a button if the press and release were both on that button.
    /// Releasing outside, even after a drag, does not close the popup.
    pub fn release_with_confirmation(
        &mut self,
        layout: &ConfirmationLayout,
        x: f32,
        y: f32,
    ) -> Confirmation {
        self.release();
        self.button_hover = layout.button_at(x, y);
        if let Some(button) = self.button_held.take()
            && self.button_hover == Some(button)
        {
            return self.finish_confirmation(button);
        }
        Confirmation::Open
    }

    /// Handles a key in the popup. Enter applies and Escape cancels from any part. Tab cycles
    /// through square, hue, hex, Apply and Cancel (Shift reverses it); Space activates the
    /// focused button. Other keys go to the focused colour control, if any.
    pub fn key_with_confirmation(
        &mut self,
        key: &KeyName,
        mods: Mods,
        typed: Option<&str>,
    ) -> Confirmation {
        match key {
            KeyName::Enter => return self.finish_confirmation(Button::Apply),
            KeyName::Escape => return self.finish_confirmation(Button::Cancel),
            KeyName::Tab => match (self.button_focus, self.focus(), mods.shift) {
                (None, Part::Hex, false) | (Some(Button::Cancel), _, true) => {
                    self.set_focus(Part::Square);
                    self.button_focus = Some(Button::Apply);
                }
                (None, Part::Square, true) | (Some(Button::Apply), _, false) => {
                    self.button_focus = Some(Button::Cancel);
                }
                (Some(Button::Apply), _, true) => {
                    self.button_focus = None;
                    self.set_focus(Part::Hex);
                }
                (Some(Button::Cancel), _, false) => {
                    self.button_focus = None;
                    self.set_focus(Part::Square);
                }
                (None, _, back) => self.tab(back),
            },
            _ => {
                if let Some(button) = self.button_focus {
                    if *key == KeyName::Space && !mods.ctrl && !mods.alt {
                        return self.finish_confirmation(button);
                    }
                } else {
                    self.key(key, mods, typed);
                }
            }
        }
        Confirmation::Open
    }
}

/// How the confirmation popup is drawn.
#[derive(Clone, Copy, Debug, Default)]
pub struct ConfirmationLook {
    /// The keyboard focus ring and the caret's blink phase.
    pub picker: Look,
    /// Keep the buttons' system contrast colours instead of blending hover and press fills.
    pub high_contrast: bool,
}

/// Draws the picker with Apply and Cancel beneath it, with the same palette and button pieces
/// as the toolkit's dialogs. Button drawing is clipped to the card and to each button.
pub fn draw_picker_with_confirmation(
    p: &mut Painter,
    pal: &Palette,
    layout: &ConfirmationLayout,
    picker: &mut Picker,
    cache: &mut super::Cache,
    look: ConfirmationLook,
) {
    let card = layout.card;
    super::draw_picker(
        p,
        pal,
        card,
        &layout.picker,
        picker,
        cache,
        Look {
            ring: look.picker.ring && picker.button_focus.is_none(),
            caret_on: look.picker.caret_on,
        },
    );
    let clip = p.clip(card.inset(1.0));
    for (button, r, label, primary) in [
        (Button::Apply, layout.apply, "Apply", true),
        (Button::Cancel, layout.cancel, "Cancel", false),
    ] {
        if !(r.w > 0.0 && r.h > 0.0) {
            continue;
        }
        if look.picker.ring && picker.button_focus == Some(button) {
            focus_ring(p, pal, r, 5.0);
        }
        let clip = p.clip(r);
        draw_dialog_button(
            p,
            pal,
            r,
            label,
            primary,
            !look.high_contrast && picker.button_hover == Some(button),
            picker.button_held == Some(button),
        );
        p.restore(clip);
    }
    p.restore(clip);
}

#[cfg(test)]
mod tests;
