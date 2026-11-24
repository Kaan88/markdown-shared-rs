//! A drop-down list on its own (a combo box that cannot be typed in): a field that shows the
//! chosen option with an arrow at its right, and opens a [`menu::Menu`](crate::menu::Menu) of the
//! options under itself. It is the field the settings dialog
//! ([`settings::Form`](crate::settings::Form)) draws in a choice row with many options, for a
//! window that is not a settings dialog.
//!
//! The model, [`Dropdown`], holds the options, the chosen one, the press under way and, while it
//! is open, the menu. The caller owns the field's rectangle (passed in to every call, so nothing
//! goes stale after a resize), the keyboard focus, the label beside the field and what a change
//! means. Events go in and an [`Outcome`] comes back: [`Dropdown::key`], [`Dropdown::press`],
//! [`Dropdown::release`], [`Dropdown::pointer`], [`Dropdown::wheel`]. While the menu is open the
//! caller sends it every key and every press, whatever is under the pointer, since an open menu
//! takes them all, and draws it last, over everything, with [`draw_dropdown_menu`].
//!
//! Keys, as a Windows combo box has them. Closed: Space, Enter, Alt+Down, Alt+Up and F4 open
//! it; Up and Left choose the option before, Down and Right the one after, Home and End the
//! first and the last, and a letter the next option that begins with it, all without opening.
//! Open: Up and Down move the highlight, Home and End go to the ends, a letter to the next
//! option that begins with it; Enter, Space, Alt+Up, Alt+Down and F4 choose the highlighted
//! option and close; Escape and Tab close with no change. The pointer: a click on the field
//! opens it (the press and the release both on the field, as in the dialog), a click on a row
//! chooses it, a press anywhere else closes the menu with no change.
//!
//! The menu is placed with [`Menu::around`](crate::menu::Menu::around), round the field grown
//! by [`MENU_GAP`] above and below: under the field with its left edges level and a gap of
//! [`MENU_GAP`] px, above it where there is no room below, and leftwards where there is no room
//! to the right. It is at least as wide as the field, opens with the chosen option ticked and
//! highlighted, and scrolls when it is taller than the window.
//!
//! The field's pixels are the settings dialog's: both draw it with the same function, and a test
//! draws a form's choice row and this field over the same ground and compares every pixel. The
//! focus ring is the exception: the dialog paints its own ring with the row's colour inside it,
//! and this field uses [`focus::draw_ring`](crate::focus::draw_ring), which needs to know nothing
//! of what is behind it.
//!
//! Deliberately not here:
//! - typing into the field (an editable combo box): that is a
//!   [`textfield`](crate::textfield) with a menu of completions;
//! - the label beside the field, which the caller draws;
//! - the menu's closing fade: the menu is gone once the model lets it go;
//! - a focus model: the caller passes [`Look::focused`] (see [`focus`](crate::focus));
//! - the mouse wheel turning the choice of a closed field, which Windows does and which moves
//!   settings by accident while a page scrolls;
//! - what a screen reader is told. The accessibility layer (roadmap W8, not built yet) reads the
//!   role from the type (a combo box), the value from [`Dropdown::selected`] and
//!   [`Dropdown::selected_label`], whether it is expanded from [`Dropdown::is_open`], the list's
//!   rectangle from [`Dropdown::menu`], and the name and the field's rectangle from the caller,
//!   which has the label and passes the rectangle to every call.
//!
//! ```text
//! // events
//! match dropdown.key(field, &key, mods) {
//!     Outcome::Changed(i) => apply(dropdown.value(i)),
//!     Outcome::Unused => { /* someone else's key */ }
//!     _ => window.request_redraw(),
//! }
//! // frame
//! let look = Look { hovered: dropdown.hit(field, mx, my), focused, ..Look::default() };
//! draw_dropdown(&mut p, &pal, field, &dropdown, look);
//! // ... everything else ...
//! draw_dropdown_menu(&mut p, &pal, &mut dropdown, window);
//! ```

use crate::anim::{Animator, AsId, Id, fluent};
use crate::focus::draw_ring;
use crate::gfx::{Face, Painter, Rect, Rgb, mix};
use crate::glyph;
use crate::keys::{KeyName, Mods};
use crate::menu::{Item, Menu, same_letter};
use crate::theme::Palette;

/// The height of the field, in logical px, as the settings dialog draws it.
pub const DROPDOWN_H: f32 = 28.0;
/// The gap between the field and the menu it opens, in logical px.
pub const MENU_GAP: f32 = 4.0;
/// The corner radius of the field.
const RADIUS: f32 = 6.0;
/// The size of the field's text, as in the dialog.
const TEXT: f32 = 13.0;

/// What an event did to a drop-down.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The event was not for the drop-down (a key it does not use, a press elsewhere while it is
    /// closed): the caller may give it to someone else.
    Unused,
    /// The drop-down took the event and nothing shows a change (Up on the first option).
    None,
    /// The drop-down took the event and wants a redraw: a pressed field, a highlight that moved.
    Redraw,
    /// The menu opened. Redraw.
    Opened,
    /// The menu closed with the choice unchanged. Redraw.
    Closed,
    /// The option at this index is now the chosen one. The menu, if it was open, is closed.
    Changed(usize),
}

/// A drop-down list's options and state. `T` is the caller's value for each option (an enum, a
/// `u32`); the model only stores it and gives it back. Options are kept in the order given.
pub struct Dropdown<T> {
    options: Vec<(T, String)>,
    selected: usize,
    /// The button went down on the closed field.
    pressed: bool,
    /// The open menu; its item ids are indices into `options`.
    menu: Option<Menu<usize>>,
    /// The row the press in the open menu went down on.
    menu_pressed: Option<usize>,
}

impl<T> Dropdown<T> {
    /// A closed drop-down of `options`, each a value and the label shown for it, with the option
    /// at `selected` chosen. An index past the last option chooses the last; with no options
    /// there is nothing chosen, the field shows nothing and it does not open.
    pub fn new<S: Into<String>>(
        options: impl IntoIterator<Item = (T, S)>,
        selected: usize,
    ) -> Dropdown<T> {
        let options: Vec<(T, String)> = options.into_iter().map(|(v, s)| (v, s.into())).collect();
        let selected = selected.min(options.len().saturating_sub(1));
        Dropdown {
            options,
            selected,
            pressed: false,
            menu: None,
            menu_pressed: None,
        }
    }

    /// The number of options.
    pub fn len(&self) -> usize {
        self.options.len()
    }

    /// Whether there are no options.
    pub fn is_empty(&self) -> bool {
        self.options.is_empty()
    }

    /// The options, each a value and its label, in order.
    pub fn options(&self) -> &[(T, String)] {
        &self.options
    }

    /// The value of option `i`, or None past the last.
    pub fn value(&self, i: usize) -> Option<&T> {
        self.options.get(i).map(|(v, _)| v)
    }

    /// The label of option `i`, or None past the last.
    pub fn label(&self, i: usize) -> Option<&str> {
        self.options.get(i).map(|(_, s)| s.as_str())
    }

    /// The index of the chosen option; None when there are no options.
    pub fn selected(&self) -> Option<usize> {
        (!self.options.is_empty()).then_some(self.selected)
    }

    /// The value of the chosen option; None when there are no options.
    pub fn selected_value(&self) -> Option<&T> {
        self.value(self.selected)
    }

    /// The label of the chosen option, which the field shows; empty when there are no options.
    pub fn selected_label(&self) -> &str {
        self.label(self.selected).unwrap_or("")
    }

    /// Chooses option `i`, for a change made elsewhere. Returns true if the choice changed; an
    /// index past the last changes nothing. An open menu moves its tick and highlight to it.
    pub fn select(&mut self, i: usize) -> bool {
        if i >= self.options.len() || i == self.selected {
            return false;
        }
        self.selected = i;
        if let Some(menu) = &mut self.menu {
            for item in &mut menu.items {
                item.checked = item.id == i;
            }
            menu.highlight(i);
        }
        true
    }

    /// Whether the menu is open.
    pub fn is_open(&self) -> bool {
        self.menu.is_some()
    }

    /// The open menu, for its rectangle ([`Menu::rect`], set when it is first drawn) and its
    /// highlight; None while closed.
    pub fn menu(&self) -> Option<&Menu<usize>> {
        self.menu.as_ref()
    }

    /// The open menu, to change how it behaves ([`Menu::access_keys`] does not apply: the
    /// labels are data); None while closed.
    pub fn menu_mut(&mut self) -> Option<&mut Menu<usize>> {
        self.menu.as_mut()
    }

    /// Whether a press went down on the closed field and the button has not gone up yet.
    pub fn pressed(&self) -> bool {
        self.pressed
    }

    /// Whether `(x, y)` is on the field in `field`. A rectangle or a point that is not finite,
    /// and a rectangle with no area, is never hit. The open menu is not the field: ask
    /// [`Menu::contains`] through [`menu`](Self::menu).
    pub fn hit(&self, field: Rect, x: f32, y: f32) -> bool {
        usable(field) && x.is_finite() && y.is_finite() && field.contains(x, y)
    }

    /// Opens the menu under `field` and returns it: one row per option, the chosen one ticked
    /// and highlighted, placed by [`Menu::around`] round `field` grown by [`MENU_GAP`] above and
    /// below, and at least as wide as `field`. An open menu is opened again in the new place.
    /// With no options, or a `field` that is not finite or has no area, nothing opens and the
    /// answer is None.
    pub fn open_menu(&mut self, field: Rect) -> Option<&mut Menu<usize>> {
        if self.options.is_empty() || !usable(field) {
            return None;
        }
        let items: Vec<Item<usize>> = self
            .options
            .iter()
            .enumerate()
            .map(|(k, (_, label))| Item::new(k, label.clone()).checked(k == self.selected))
            .collect();
        let anchor = Rect::new(
            field.x,
            field.y - MENU_GAP,
            field.w,
            field.h + 2.0 * MENU_GAP,
        );
        let mut menu = Menu::new(items, field.x, field.bottom(), false)
            .around(anchor)
            .min_width(field.w);
        menu.highlight(self.selected);
        self.pressed = false;
        self.menu_pressed = None;
        self.menu = Some(menu);
        self.menu.as_mut()
    }

    /// Closes the menu with no change. Returns true if it was open.
    pub fn close(&mut self) -> bool {
        self.menu_pressed = None;
        self.menu.take().is_some()
    }

    /// A key while the keyboard is on the drop-down (see the module doc for the keys). `field`
    /// is where the field is now, for a key that opens the menu.
    pub fn key(&mut self, field: Rect, key: &KeyName, mods: Mods) -> Outcome {
        let plain = !mods.ctrl && !mods.alt && !mods.control && !mods.shift;
        let alt_arrow = mods.alt && !mods.ctrl && matches!(key, KeyName::Up | KeyName::Down);
        let swing = alt_arrow || (*key == KeyName::F(4) && !mods.alt && !mods.ctrl);
        if let Some(hover) = self.menu.as_ref().map(|m| m.hover) {
            let to = match key {
                KeyName::Escape | KeyName::Tab => {
                    self.close();
                    return Outcome::Closed;
                }
                _ if swing => return self.commit(),
                KeyName::Enter | KeyName::Space => return self.commit(),
                KeyName::Up | KeyName::Down => {
                    if let Some(menu) = &mut self.menu {
                        menu.step(*key == KeyName::Down);
                    }
                    return Outcome::Redraw;
                }
                KeyName::Home => Some(0),
                KeyName::End => Some(self.options.len().saturating_sub(1)),
                KeyName::Char(c) if !mods.ctrl && !mods.alt && !mods.control => {
                    self.next_with_letter(hover.unwrap_or(self.selected), *c)
                }
                // An open menu takes every key.
                _ => None,
            };
            return match (to, &mut self.menu) {
                (Some(k), Some(menu)) if hover != Some(k) => {
                    if menu.highlight(k) {
                        Outcome::Redraw
                    } else {
                        Outcome::None
                    }
                }
                _ => Outcome::None,
            };
        }
        if self.options.is_empty() {
            return Outcome::Unused;
        }
        let last = self.options.len() - 1;
        match key {
            _ if swing || (plain && matches!(key, KeyName::Space | KeyName::Enter)) => {
                match self.open_menu(field) {
                    Some(_) => Outcome::Opened,
                    None => Outcome::None,
                }
            }
            KeyName::Up | KeyName::Left if plain => self.choose(self.selected.saturating_sub(1)),
            KeyName::Down | KeyName::Right if plain => self.choose((self.selected + 1).min(last)),
            KeyName::Home if plain => self.choose(0),
            KeyName::End if plain => self.choose(last),
            KeyName::Char(c) if !mods.ctrl && !mods.alt && !mods.control => {
                match self.next_with_letter(self.selected, *c) {
                    Some(k) => self.choose(k),
                    None => Outcome::None,
                }
            }
            _ => Outcome::Unused,
        }
    }

    /// The pointer moved to `(x, y)`: the open menu's highlight follows it. Returns true when
    /// the highlight changed. The field's own hover is the caller's ([`hit`](Self::hit)).
    pub fn pointer(&mut self, x: f32, y: f32) -> bool {
        match &mut self.menu {
            Some(menu) if x.is_finite() && y.is_finite() => menu.pointer(x, y),
            _ => false,
        }
    }

    /// The button went down at `(x, y)`. While the menu is open: on the menu, the row under the
    /// pointer is remembered for the release ([`Outcome::Redraw`]); anywhere else, the field
    /// included, the menu closes with no change ([`Outcome::Closed`]) and the press is used up.
    /// While closed: on the field the press is remembered ([`Outcome::Redraw`]), elsewhere it is
    /// [`Outcome::Unused`].
    pub fn press(&mut self, field: Rect, x: f32, y: f32) -> Outcome {
        self.pressed = false;
        if let Some(menu) = &mut self.menu {
            if x.is_finite() && y.is_finite() && menu.contains(x, y) {
                menu.pointer(x, y);
                self.menu_pressed = menu.hover;
                return Outcome::Redraw;
            }
            self.close();
            return Outcome::Closed;
        }
        if self.hit(field, x, y) && !self.options.is_empty() {
            self.pressed = true;
            return Outcome::Redraw;
        }
        Outcome::Unused
    }

    /// The button went up at `(x, y)`. While the menu is open, a release on the row the press
    /// went down on chooses it ([`Outcome::Changed`], or [`Outcome::Closed`] for the option
    /// already chosen); any other release does nothing ([`Outcome::None`]). While closed, a
    /// release on the field the press went down on opens the menu ([`Outcome::Opened`]); one
    /// elsewhere ends the press ([`Outcome::Redraw`]), and with no press it is
    /// [`Outcome::Unused`].
    pub fn release(&mut self, field: Rect, x: f32, y: f32) -> Outcome {
        if let Some(menu) = &mut self.menu {
            let pressed = self.menu_pressed.take();
            if x.is_finite() && y.is_finite() {
                menu.pointer(x, y);
            }
            if pressed.is_some() && menu.hover == pressed {
                return self.commit();
            }
            return Outcome::None;
        }
        if !std::mem::take(&mut self.pressed) {
            return Outcome::Unused;
        }
        if self.hit(field, x, y) {
            return match self.open_menu(field) {
                Some(_) => Outcome::Opened,
                None => Outcome::Redraw,
            };
        }
        Outcome::Redraw
    }

    /// The wheel turned by `dy` logical px over `(x, y)` (positive towards the end): an open menu
    /// under the pointer scrolls. Returns true if it moved. A closed field ignores the wheel.
    pub fn wheel(&mut self, x: f32, y: f32, dy: f32) -> bool {
        match &mut self.menu {
            Some(menu) if dy.is_finite() && menu.contains(x, y) => menu.scroll_by(dy),
            _ => false,
        }
    }

    /// Drops a press under way, on the field or in the menu, with no change (the window lost
    /// the pointer). The menu stays open. Returns true if there was one.
    pub fn cancel(&mut self) -> bool {
        let had = self.pressed || self.menu_pressed.is_some();
        self.pressed = false;
        self.menu_pressed = None;
        had
    }

    /// Closes the menu choosing its highlighted option.
    fn commit(&mut self) -> Outcome {
        let chosen = self.menu.as_ref().and_then(Menu::chosen);
        self.close();
        match chosen {
            Some(k) if k != self.selected && k < self.options.len() => {
                self.selected = k;
                Outcome::Changed(k)
            }
            _ => Outcome::Closed,
        }
    }

    /// Chooses option `k` with the menu closed.
    fn choose(&mut self, k: usize) -> Outcome {
        if self.select(k) {
            Outcome::Changed(k)
        } else {
            Outcome::None
        }
    }

    /// The first option after `from` (going round) whose label begins with `c`, ignoring case.
    fn next_with_letter(&self, from: usize, c: char) -> Option<usize> {
        let n = self.options.len();
        (1..=n).map(|d| (from + d) % n).find(|&k| {
            self.options[k]
                .1
                .chars()
                .next()
                .is_some_and(|first| same_letter(first, c))
        })
    }
}

/// How a field is drawn just now, from the caller's own state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Look {
    /// The pointer is over the field: its fill and edge darken a little, as in the dialog. An
    /// open drop-down draws so whatever this says.
    pub hovered: bool,
    /// The keyboard is on it: the focus ring round the field.
    pub focused: bool,
    /// It cannot be changed: no hover look, no ring, and the text and the arrow faint, as the
    /// settings dialog draws a disabled row.
    pub disabled: bool,
}

/// Draws the field in `r` (normally [`DROPDOWN_H`] high): a rounded box in the field colour
/// with an edge, the chosen option's label at its left, cut short to leave room, and a down
/// arrow at its right. An open drop-down draws with the hover look. The menu is drawn apart, by
/// [`draw_dropdown_menu`]. It draws only inside `r`, but for the focus ring, which lies three px
/// outside it. A rectangle that is not finite or has no area draws nothing. It changes no state.
pub fn draw_dropdown<T>(
    p: &mut Painter,
    pal: &Palette,
    r: Rect,
    dropdown: &Dropdown<T>,
    look: Look,
) {
    let hot = (look.hovered || dropdown.is_open()) && !look.disabled;
    draw_at(p, pal, r, dropdown, look, if hot { 1.0 } else { 0.0 });
}

/// [`draw_dropdown`] with motion: the hover look fades in and out ([`fluent::FADE`]). `id`
/// names this field in `anim`. At rest, and on every frame when the animator does not animate,
/// it draws exactly what [`draw_dropdown`] draws; a field first seen is at rest.
pub fn draw_dropdown_animated<T>(
    p: &mut Painter,
    pal: &Palette,
    anim: &mut Animator,
    id: impl AsId,
    r: Rect,
    dropdown: &Dropdown<T>,
    look: Look,
) {
    let hot = (look.hovered || dropdown.is_open()) && !look.disabled;
    let t = anim.toggle(Id::new(id).with("hover"), hot, fluent::FADE);
    draw_at(p, pal, r, dropdown, look, t);
}

/// Draws the open menu inside `area` (the window, or the part of it the menu may use), over
/// everything else: draw it last. Placing it sets its rectangle for the pointer. A closed
/// drop-down draws nothing.
pub fn draw_dropdown_menu<T>(
    p: &mut Painter,
    pal: &Palette,
    dropdown: &mut Dropdown<T>,
    area: Rect,
) {
    if let Some(menu) = &mut dropdown.menu {
        menu.draw_in(p, pal, area);
    }
}

/// [`draw_dropdown_menu`] with motion: the menu enters as other menus do
/// ([`Menu::draw_in_presence`], [`fluent::ENTER`]). `id` names this menu in `anim`. Once it has
/// entered it is [`draw_dropdown_menu`] pixel for pixel, and under reduced motion it is so from
/// the first frame. It leaves at once when the drop-down closes.
pub fn draw_dropdown_menu_animated<T>(
    p: &mut Painter,
    pal: &Palette,
    anim: &mut Animator,
    id: impl AsId,
    dropdown: &mut Dropdown<T>,
    area: Rect,
) {
    let id = Id::new(id).with("menu");
    match &mut dropdown.menu {
        Some(menu) => {
            let t = anim.presence(id, true, fluent::ENTER, fluent::EXIT);
            menu.draw_in_presence(p, pal, area, t);
        }
        // So that the next opening enters from nothing again.
        None => anim.forget(id),
    }
}

/// The field with the hover look `t` of the way in (0 none, 1 all).
fn draw_at<T>(p: &mut Painter, pal: &Palette, r: Rect, dropdown: &Dropdown<T>, look: Look, t: f32) {
    if !usable(r) {
        return;
    }
    if look.focused && !look.disabled {
        draw_ring(p, pal, r, RADIUS);
    }
    let t = if t.is_nan() { 0.0 } else { t.clamp(0.0, 1.0) };
    let shown = dropdown.selected_label();
    if t <= 0.0 || t >= 1.0 {
        field(p, pal, r, shown, t >= 1.0, look.disabled);
    } else {
        let (cold_fill, cold_edge) = field_colours(pal, false);
        let (hot_fill, hot_edge) = field_colours(pal, true);
        let colours = (mix(cold_fill, hot_fill, t), mix(cold_edge, hot_edge, t));
        field_in(p, pal, r, shown, colours, look.disabled);
    }
}

/// The fill and the edge of the field, with the hover look or without.
fn field_colours(pal: &Palette, hot: bool) -> (Rgb, Rgb) {
    if hot {
        (
            mix(pal.field, pal.text, 0.05),
            mix(pal.line, pal.text, 0.18),
        )
    } else {
        (pal.field, pal.line)
    }
}

/// The settings dialog's drop-down field: `shown` in a rounded box, with the down arrow at its
/// right; `hot` for the pointer over it or its menu open, `off` for a disabled row. The dialog's
/// choice rows draw with this, so that both draw the same pixels.
pub(crate) fn field(p: &mut Painter, pal: &Palette, r: Rect, shown: &str, hot: bool, off: bool) {
    field_in(p, pal, r, shown, field_colours(pal, hot), off);
}

fn field_in(
    p: &mut Painter,
    pal: &Palette,
    r: Rect,
    shown: &str,
    (fill, edge): (Rgb, Rgb),
    off: bool,
) {
    p.frame(r, RADIUS, fill, edge);
    p.text(
        Face::Ui,
        TEXT,
        r.x + 11.0,
        r.cy(),
        shown,
        if off { pal.faint } else { pal.text },
        r.w - 36.0,
    );
    let arrow = if off { pal.faint } else { pal.muted };
    p.icon(glyph::DOWN, 10.0, r.right() - 15.0, r.cy(), arrow);
}

/// A rectangle that can be drawn and hit: finite, with both sides positive.
fn usable(r: Rect) -> bool {
    [r.x, r.y, r.w, r.h].iter().all(|v| v.is_finite()) && r.w > 0.0 && r.h > 0.0
}

#[cfg(test)]
#[path = "dropdown_tests.rs"]
mod tests;
