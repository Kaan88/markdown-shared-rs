//! All of one single-line field's input in one place: the mouse ([`Pointer`]'s click, drag,
//! double and triple click, Shift+click and edge scrolling), the right-click edit menu (a
//! [`Cascade`] of [`ContextAction`]s it opens, routes and closes itself), the keyboard
//! ([`TextField::handle_key`], or the open menu first), committed and composing input-method
//! text, and the one deadline all of that needs.
//!
//! # Why it exists
//!
//! [`TextField`], [`Pointer`] and [`TextField::context_menu`] are pieces, and every application
//! that put them together by hand left some out. Six applications built on this toolkit had a
//! field whose mouse handling was written in the application, and each was missing a different
//! part: the drag (the pointer moving with the button held never reached the field), the
//! triple click, the edge scrolling, the right-click menu, a press in the box's padding, the
//! menu's words in the application's language, or the drag ended when the window lost the
//! mouse. One of them wrapped the pieces in a module of its own of nearly five hundred lines.
//! `FieldInput` is that wrapper, once, with its tests.
//!
//! # What the caller does
//!
//! Keep one `FieldInput` beside each [`TextField`] (it does not own the field, so a field
//! inside another model works as well), and:
//!
//! 1. Give **every** pointer event to every field's input, wherever it happened: a press
//!    outside a field is what ends that field's drag and closes its menu. Each method returns
//!    a [`FieldOutcome`]; when it is [`taken`](FieldOutcome::taken), the event was this
//!    field's and goes nowhere else.
//! 2. Give keys, text and preedits to the input of the field that has the keyboard. An open
//!    menu takes them first, so Escape closes the menu and does not reach the application.
//! 3. Call [`cancel`](FieldInput::cancel) when the window loses the keyboard. On Windows winit
//!    sends no button release when a window loses the mouse in the middle of a drag (another
//!    window comes to the front, a dialog opens), so without this the next plain move after
//!    coming back would go on dragging.
//! 4. Wait for [`deadline`](FieldInput::deadline) and then call [`tick`](FieldInput::tick).
//!    It is the edge scroll of a drag held past the field's end, and the menu's submenu delay.
//! 5. Draw the field as before, then the menu with [`draw_menu`](FieldInput::draw_menu) after
//!    everything else.
//!
//! In a window of [`crate::host`] (feature `host`), steps 1 to 4 are two calls: `event` with
//! every [`Event`](crate::host::Event), and `timer_fired` from `App::timer_fired`.
//!
//! Each call is given the field's [`FieldArea`] as it was last drawn: the frame the user sees
//! (a press anywhere in it counts, the padding and a leading icon included) and the text
//! rectangle with its face, size and scale. Nothing here remembers a rectangle.
//!
//! # The right-click menu
//!
//! A right press in the frame ends a drag, moves the caret to the press when there is no
//! selection (a right press keeps a selection, so Copy copies what was selected), and opens
//! Cut, Copy, Paste and Select all at the pointer, worded by [`EditLabels`]. Shift+F10 opens it
//! at the caret. While it is open it takes every pointer event and key; a press outside it
//! closes it and goes no further, as a press outside a Windows menu does. A chosen item is
//! applied to the field and reported as [`FieldOutcome::Edited`] when the text changed, so
//! the application handles Cut and Paste exactly as it handles typing.
//!
//! Deliberately not here: the focus between fields (the caller's: give the keyboard to the
//! field whose press returned [`FieldOutcome::Focus`]); the field's drawing and caret blink
//! ([`TextField::draw_blinking`]); the pointer's cursor, which the caller sets from
//! [`cursor_at`](FieldInput::cursor_at); the platform input method's state; a menu drawn by
//! the application's own menu system (use [`TextField::context_menu_labelled`] and
//! [`TextField::apply_context_action`] for that); and Enter, Escape, Tab, Up and Down, which
//! come back [`Unused`](FieldOutcome::Unused) for the caller.

use std::time::{Duration, Instant};

use super::{ContextAction, EditLabels, FieldHit, Pointer, Preedit, TextField, pointer};
use crate::cursor::Cursor;
use crate::gfx::{Gfx, Painter, Rect};
use crate::keys::{KeyName, Mods};
use crate::menu::Look;
use crate::submenu::{Cascade, Outcome};
use crate::theme::Palette;

/// The timer name [`FieldInput`] uses in a window of [`crate::host`] unless it is given one
/// with [`FieldInput::timer_name`].
pub const INPUT_TIMER: &str = "mdshared-ui.textfield.input";

/// Where a field is, as it was last drawn.
#[derive(Clone, Copy, Debug)]
pub struct FieldArea {
    frame: Rect,
    text: FieldHit,
}

impl FieldArea {
    /// `frame` is the box the user sees and clicks; `text` is the rectangle the text is drawn
    /// in, with its face, size and the painter's scale (what is passed to
    /// [`TextField::draw`]). A frame smaller than the text rectangle, or one that is not made
    /// of numbers, counts as the text rectangle alone.
    pub fn new(frame: Rect, text: FieldHit) -> FieldArea {
        FieldArea { frame, text }
    }

    /// A field whose frame is its text rectangle.
    pub fn text_only(text: FieldHit) -> FieldArea {
        FieldArea {
            frame: text.area,
            text,
        }
    }

    /// The frame, as given.
    pub fn frame(&self) -> Rect {
        self.frame
    }

    /// The text rectangle and how its text is drawn, as given.
    pub fn text(&self) -> FieldHit {
        self.text
    }

    /// Whether a press at `at` lands on the field: inside the frame or the text rectangle.
    /// Nothing contains a point that is not made of numbers.
    pub fn contains(&self, at: (f32, f32)) -> bool {
        let (x, y) = at;
        if !(x.is_finite() && y.is_finite()) {
            return false;
        }
        let in_frame = finite(self.frame) && self.frame.contains(x, y);
        in_frame || (self.text.valid() && self.text.area.contains(x, y))
    }

    /// A point of the frame moved into the text rectangle, to the nearest end of the text: a
    /// press in the padding left of the text places the caret at its start, one right of it
    /// at its end. A point already inside is unchanged.
    pub(crate) fn nearest_in_text(&self, (x, y): (f32, f32)) -> (f32, f32) {
        let r = self.text.area;
        // `clamp` panics on a bound that is not a number; such a rectangle places nothing
        // anyway (the pointer refuses it), so the point goes on as it is.
        if !self.text.valid() || r.contains(x, y) {
            return (x, y);
        }
        let right = (r.right() - 0.5).max(r.x);
        (x.clamp(r.x, right), r.cy())
    }
}

fn finite(r: Rect) -> bool {
    [r.x, r.y, r.w, r.h, r.right(), r.bottom()]
        .into_iter()
        .all(f32::is_finite)
        && r.w > 0.0
        && r.h > 0.0
}

/// What an event did to a field. Match it with a `_` arm: more may be added.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum FieldOutcome {
    /// The event was not this field's: a press elsewhere, a key the field does not edit with
    /// (Enter, Escape, Tab, Up, Down, a shortcut), a move with nothing held. The caller
    /// handles it as it would without the field.
    Unused,
    /// The field took the event and nothing it draws changed. Do not pass it on.
    Taken,
    /// The field took the event and its caret, selection, scroll or menu changed: draw again.
    Redraw,
    /// A press landed on the field: give it the keyboard, and draw again.
    Focus,
    /// The text changed (typing, Backspace, Paste, Cut, a chosen menu item): draw again and act
    /// on the new text as after typing.
    Edited,
}

impl FieldOutcome {
    /// Whether the field took the event, so the caller does not pass it on.
    pub fn taken(self) -> bool {
        self != FieldOutcome::Unused
    }

    /// Whether the field needs drawing again.
    pub fn redraw(self) -> bool {
        matches!(
            self,
            FieldOutcome::Redraw | FieldOutcome::Focus | FieldOutcome::Edited
        )
    }
}

/// One field's input: see the [module](self).
pub struct FieldInput {
    /// The pointer, and the scale it was made for: made again when the scale changes, which
    /// also breaks a run of clicks.
    pointer: Option<(f32, Pointer)>,
    /// A double-click time and distance of the caller's, instead of the system's.
    timing: Option<(Duration, (f32, f32))>,
    menu: Option<Cascade<ContextAction>>,
    labels: EditLabels,
    look: Option<Look>,
    /// Where the pointer last was, for the wheel (the host's wheel event has no position).
    #[cfg_attr(not(feature = "host"), allow(dead_code))]
    last_at: Option<(f32, f32)>,
    timer: &'static str,
    /// A host timer was set by this input and not yet cancelled or fired.
    #[cfg_attr(not(feature = "host"), allow(dead_code))]
    armed: bool,
}

impl Default for FieldInput {
    fn default() -> FieldInput {
        FieldInput::new()
    }
}

impl FieldInput {
    /// An input with the system's double-click time and distance, the English menu, the
    /// toolkit's menu look and [`INPUT_TIMER`].
    pub fn new() -> FieldInput {
        FieldInput {
            pointer: None,
            timing: None,
            menu: None,
            labels: EditLabels::ENGLISH,
            look: None,
            last_at: None,
            timer: INPUT_TIMER,
            armed: false,
        }
    }

    /// The same input with the menu worded by `labels`.
    pub fn labels(mut self, labels: EditLabels) -> FieldInput {
        self.labels = labels;
        self
    }

    /// Words the menu by `labels` from the next time it opens; one open now keeps its words.
    pub fn set_labels(&mut self, labels: EditLabels) {
        self.labels = labels;
    }

    /// The same input with its menu drawn in `look` ([`Cascade::with_look`]).
    pub fn menu_look(mut self, look: Look) -> FieldInput {
        self.look = Some(look);
        self
    }

    /// The same input with this double-click time and distance (logical px on each axis)
    /// instead of the system's, as [`Pointer::new`] takes them.
    pub fn timing(mut self, interval: Duration, slop: (f32, f32)) -> FieldInput {
        self.timing = Some((interval, slop));
        self.pointer = None;
        self
    }

    /// The same input with its host timer called `name`. Give each field in a window its own
    /// name; two fields that share one can move each other's timer.
    pub fn timer_name(mut self, name: &'static str) -> FieldInput {
        self.timer = name;
        self
    }

    /// The name of the host timer this input sets.
    pub fn timer(&self) -> &'static str {
        self.timer
    }

    fn pointer_for(&mut self, scale: f32) -> &mut Pointer {
        if self.pointer.as_ref().is_none_or(|(s, _)| *s != scale) {
            let fresh = match self.timing {
                Some((interval, slop)) => Pointer::new(interval, slop),
                None => Pointer::system(scale),
            };
            self.pointer = Some((scale, fresh));
        }
        &mut self.pointer.as_mut().expect("set above").1
    }

    /// Whether a primary press on the field is held (a drag may be under way).
    pub fn is_dragging(&self) -> bool {
        self.pointer.as_ref().is_some_and(|(_, p)| p.is_held())
    }

    /// Whether the edit menu is open.
    pub fn menu_open(&self) -> bool {
        self.menu.is_some()
    }

    /// The open menu, for a caller that draws it its own way (`draw_animated`,
    /// `draw_in_presence`) or needs its rectangle.
    pub fn menu(&self) -> Option<&Cascade<ContextAction>> {
        self.menu.as_ref()
    }

    /// The open menu, mutable, for drawing it.
    pub fn menu_mut(&mut self) -> Option<&mut Cascade<ContextAction>> {
        self.menu.as_mut()
    }

    /// A primary (left) press at `at`. In the menu it chooses an item; outside the menu it
    /// closes it and goes no further. On the field it places the caret, or extends the
    /// selection with Shift, or selects a word on a double click and everything on a triple
    /// one, and returns [`FieldOutcome::Focus`]. Elsewhere it ends a drag and breaks a run of
    /// clicks, and returns [`FieldOutcome::Unused`].
    pub fn press(
        &mut self,
        field: &mut TextField,
        gfx: &mut Gfx,
        area: FieldArea,
        at: (f32, f32),
        mods: Mods,
        now: Instant,
    ) -> FieldOutcome {
        self.last_at = Some(at);
        if let Some(menu) = &mut self.menu {
            let outcome = menu.click(at.0, at.1);
            return self.menu_outcome(field, outcome);
        }
        if !area.contains(at) {
            self.end_drag();
            return FieldOutcome::Unused;
        }
        let into = area.nearest_in_text(at);
        self.pointer_for(area.text.scale)
            .press(field, gfx, area.text, into, mods, now);
        FieldOutcome::Focus
    }

    /// A secondary (right) press at `at`. On the field it ends a drag, moves the caret to the
    /// press when nothing is selected, and opens the edit menu there: [`FieldOutcome::Focus`].
    /// A menu open before is closed first; a right press elsewhere then returns
    /// [`FieldOutcome::Redraw`] and goes no further, and with no menu open
    /// [`FieldOutcome::Unused`].
    pub fn secondary_press(
        &mut self,
        field: &mut TextField,
        gfx: &mut Gfx,
        area: FieldArea,
        at: (f32, f32),
        now: Instant,
    ) -> FieldOutcome {
        self.last_at = Some(at);
        let was_open = self.menu.take().is_some();
        self.end_drag();
        if !area.contains(at) {
            return if was_open {
                FieldOutcome::Redraw
            } else {
                FieldOutcome::Unused
            };
        }
        if !field.has_selection() {
            let into = area.nearest_in_text(at);
            let pointer = self.pointer_for(area.text.scale);
            pointer.press(field, gfx, area.text, into, Mods::default(), now);
            pointer.cancel();
        }
        self.open_menu(field, at);
        FieldOutcome::Focus
    }

    /// A press of any other button (middle, back, forward) at `at`: ends a drag and closes the
    /// menu. [`FieldOutcome::Redraw`] when a menu closed, else [`FieldOutcome::Unused`]: the
    /// button is the caller's.
    pub fn other_press(&mut self, at: (f32, f32)) -> FieldOutcome {
        self.last_at = Some(at);
        self.end_drag();
        if self.menu.take().is_some() {
            FieldOutcome::Redraw
        } else {
            FieldOutcome::Unused
        }
    }

    /// The pointer moved to `at`. An open menu takes every move (its rows light under the
    /// pointer). A held press extends the selection, anywhere in the window, and starts edge
    /// scrolling past the field's ends; it takes the move even when nothing changed. With
    /// neither, [`FieldOutcome::Unused`].
    pub fn pointer_move(
        &mut self,
        field: &mut TextField,
        gfx: &mut Gfx,
        area: FieldArea,
        at: (f32, f32),
        now: Instant,
    ) -> FieldOutcome {
        self.last_at = Some(at);
        if let Some(menu) = &mut self.menu {
            return changed(menu.pointer(at.0, at.1, now));
        }
        match &mut self.pointer {
            Some((_, p)) if p.is_held() => changed(p.pointer_move(field, gfx, area.text, at, now)),
            _ => FieldOutcome::Unused,
        }
    }

    /// The primary button went up at `at`. Ends a held press there, extending the selection to
    /// `at` first if no move was delivered at that place; the run of clicks goes on, so the
    /// next press can be a double click. An open menu takes the release and does nothing with
    /// it.
    pub fn release(
        &mut self,
        field: &mut TextField,
        gfx: &mut Gfx,
        area: FieldArea,
        at: (f32, f32),
        now: Instant,
    ) -> FieldOutcome {
        self.last_at = Some(at);
        if self.menu.is_some() {
            return FieldOutcome::Taken;
        }
        match &mut self.pointer {
            Some((_, p)) if p.is_held() => {
                let moved = p.pointer_move(field, gfx, area.text, at, now);
                p.release();
                changed(moved)
            }
            _ => FieldOutcome::Unused,
        }
    }

    /// The wheel turned by `dy` (logical px, positive down) with the pointer at `at`: an open
    /// menu scrolls when it is too long for the window, and takes the wheel either way.
    pub fn wheel(&mut self, at: (f32, f32), dy: f32) -> FieldOutcome {
        match &mut self.menu {
            Some(menu) => changed(menu.wheel(at.0, at.1, dy)),
            None => FieldOutcome::Unused,
        }
    }

    /// A key, for the field that has the keyboard. An open menu takes every key: its own
    /// (Up, Down, Enter, Escape, an access key) and the rest, which it drops. Shift+F10
    /// opens the menu at the caret. Otherwise the editing keys of [`TextField::handle_key`]
    /// (with the clipboard shortcuts) edit the field, and end a drag; the others come back
    /// [`FieldOutcome::Unused`] for the caller.
    pub fn key(
        &mut self,
        field: &mut TextField,
        gfx: &mut Gfx,
        area: FieldArea,
        key: &KeyName,
        mods: Mods,
        typed: Option<&str>,
    ) -> FieldOutcome {
        if let Some(menu) = &mut self.menu {
            let outcome = menu.key(key);
            return self.menu_outcome(field, outcome);
        }
        if *key == KeyName::F(10) && mods.shift && !mods.ctrl && !mods.alt {
            self.end_drag();
            let at = caret_point(field, gfx, area);
            self.open_menu(field, at);
            return FieldOutcome::Redraw;
        }
        match field.handle_key(key, mods, typed, true) {
            Some(edited) => {
                self.end_drag();
                if edited {
                    FieldOutcome::Edited
                } else {
                    FieldOutcome::Redraw
                }
            }
            None => FieldOutcome::Unused,
        }
    }

    /// Text an input method committed, for the field that has the keyboard. Dropped while
    /// the menu is open.
    pub fn text(&mut self, field: &mut TextField, text: &str) -> FieldOutcome {
        if self.menu.is_some() {
            return FieldOutcome::Taken;
        }
        if field.insert(text) {
            self.end_drag();
            FieldOutcome::Edited
        } else {
            FieldOutcome::Taken
        }
    }

    /// What an input method is composing, for the field that has the keyboard.
    pub fn preedit(&mut self, field: &mut TextField, preedit: Option<Preedit>) -> FieldOutcome {
        changed(field.set_preedit(preedit))
    }

    /// Ends a drag, breaks a run of clicks and closes the menu: when the window loses the
    /// keyboard, when the field is hidden or removed, when the scale changes. True when a
    /// menu was closed (draw again). The text, caret and selection stay as they are.
    pub fn cancel(&mut self) -> bool {
        self.end_drag();
        self.menu.take().is_some()
    }

    /// Closes the menu if it is open; true when it was.
    pub fn close_menu(&mut self) -> bool {
        self.menu.take().is_some()
    }

    /// When [`tick`](Self::tick) is due: the next step of a drag's edge scroll, or the
    /// menu's submenu delay. `None` while there is nothing to wait for, so an idle field asks
    /// for no wake-up.
    pub fn deadline(&self) -> Option<Instant> {
        let drag = self
            .pointer
            .as_ref()
            .filter(|(_, p)| p.is_held())
            .and_then(|(_, p)| p.deadline());
        let menu = self.menu.as_ref().and_then(|m| m.deadline());
        match (drag, menu) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    /// Does what was due at `now`: scrolls a drag held past the field's end by a step, opens or
    /// closes the submenu the pointer rested on. [`FieldOutcome::Redraw`] when something moved,
    /// else [`FieldOutcome::Unused`]. Early or repeated calls do nothing.
    pub fn tick(
        &mut self,
        field: &mut TextField,
        gfx: &mut Gfx,
        area: FieldArea,
        now: Instant,
    ) -> FieldOutcome {
        let mut moved = false;
        if let Some(menu) = &mut self.menu {
            moved |= menu.tick(now);
        }
        if let Some((_, p)) = &mut self.pointer
            && p.is_held()
        {
            moved |= p.tick(field, gfx, area.text, now);
        }
        if moved {
            FieldOutcome::Redraw
        } else {
            FieldOutcome::Unused
        }
    }

    /// The cursor to show at `at`: the arrow while the menu is open, the I-beam over the field
    /// and anywhere during a drag, and `None` (the caller's choice) elsewhere.
    pub fn cursor_at(&self, area: FieldArea, at: (f32, f32)) -> Option<Cursor> {
        if self.menu.is_some() {
            Some(Cursor::Default)
        } else if self.is_dragging() || area.contains(at) {
            Some(Cursor::Text)
        } else {
            None
        }
    }

    /// Places the open menu inside `window` and draws it. Call it last, after everything else
    /// in the window, every frame the menu is open: the menu is placed when it is drawn, and
    /// a menu never drawn takes the pointer without showing anything. Nothing when closed.
    pub fn draw_menu(&mut self, p: &mut Painter, pal: &Palette, window: Rect) {
        if let Some(menu) = &mut self.menu {
            menu.draw_in(p, pal, window);
        }
    }

    /// What the open menu draws on, shadow included, for a caller that draws only what
    /// changed. `None` while closed.
    pub fn menu_bounds(&self) -> Option<Rect> {
        self.menu.as_ref().map(|m| m.presence_bounds())
    }

    fn end_drag(&mut self) {
        if let Some((_, p)) = &mut self.pointer {
            p.cancel();
        }
    }

    fn open_menu(&mut self, field: &TextField, at: (f32, f32)) {
        let nodes = field
            .context_menu_labelled(&self.labels)
            .into_iter()
            .map(Into::into)
            .collect();
        let (x, y) = (sane(at.0), sane(at.1));
        let mut menu = Cascade::at(nodes, x, y);
        if let Some(look) = self.look {
            menu = menu.with_look(look);
        }
        self.menu = Some(menu);
    }

    fn menu_outcome(
        &mut self,
        field: &mut TextField,
        outcome: Outcome<ContextAction>,
    ) -> FieldOutcome {
        match outcome {
            Outcome::Chosen(action) => {
                self.menu = None;
                let before = field.text().to_owned();
                field.apply_context_action(action);
                if field.text() != before {
                    FieldOutcome::Edited
                } else {
                    FieldOutcome::Redraw
                }
            }
            Outcome::Close | Outcome::Outside => {
                self.menu = None;
                FieldOutcome::Redraw
            }
            Outcome::Changed => FieldOutcome::Redraw,
            _ => FieldOutcome::Taken,
        }
    }
}

fn changed(moved: bool) -> FieldOutcome {
    if moved {
        FieldOutcome::Redraw
    } else {
        FieldOutcome::Taken
    }
}

fn sane(v: f32) -> f32 {
    if v.is_finite() { v } else { 0.0 }
}

/// The point under the caret where Shift+F10 opens the menu: at the caret's x within the text
/// rectangle, at its bottom edge. The frame's bottom left when the text rectangle is not
/// usable.
fn caret_point(field: &TextField, gfx: &mut Gfx, area: FieldArea) -> (f32, f32) {
    let hit = area.text;
    if !hit.valid() {
        return (area.frame.x, area.frame.bottom());
    }
    let line = hit.line(&field.display(), gfx);
    let x = pointer::snapped_origin(hit, field.scroll) + line.x(field.shaped_stop(field.caret));
    let right = hit.area.right().max(hit.area.x);
    (x.clamp(hit.area.x, right), hit.area.bottom())
}

#[cfg(feature = "host")]
mod hosted {
    use super::*;
    use crate::host::{Button, Cx, Event};

    /// What to do with the host timer after an event.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(super) enum Timer {
        Set(Instant),
        Cancel,
        Leave,
    }

    impl FieldInput {
        /// Routes one event of a [`crate::host`] window: pointer moves, presses and releases of
        /// every button, the wheel, and (when `focused`, the field having the keyboard) keys,
        /// committed text and preedits; it cancels on [`Event::Focus`]`(false)`, on
        /// [`Event::PressLost`] and on [`Event::Scale`]. It asks for a redraw itself when the field or its menu changed,
        /// and sets or cancels its timer ([`timer_name`](Self::timer_name)), whose coming the
        /// application passes to [`timer_fired`](Self::timer_fired).
        ///
        /// Give it every event, before the application's own handling, and stop when the
        /// outcome is [`taken`](FieldOutcome::taken). Focus, scale and every other event the
        /// field does not consume come back [`FieldOutcome::Unused`], so the application
        /// still sees them.
        pub fn event<M: 'static>(
            &mut self,
            cx: &mut Cx<'_, M>,
            field: &mut TextField,
            gfx: &mut Gfx,
            area: FieldArea,
            focused: bool,
            event: &Event,
        ) -> FieldOutcome {
            let (outcome, redraw) =
                self.route(field, gfx, area, focused, cx.mods(), cx.now(), event);
            self.apply_timer(cx);
            if redraw || outcome.redraw() {
                cx.request_redraw_all();
            }
            outcome
        }

        /// Passes on [`App::timer_fired`](crate::host::App::timer_fired): when `name` is this
        /// input's timer, does what was due ([`tick`](Self::tick)), asks for a redraw if
        /// something moved, sets the timer again if more is due, and returns
        /// [`FieldOutcome::Redraw`] or [`FieldOutcome::Taken`]. Any other name is
        /// [`FieldOutcome::Unused`].
        pub fn timer_fired<M: 'static>(
            &mut self,
            cx: &mut Cx<'_, M>,
            field: &mut TextField,
            gfx: &mut Gfx,
            area: FieldArea,
            name: &str,
        ) -> FieldOutcome {
            if name != self.timer {
                return FieldOutcome::Unused;
            }
            let outcome = self.fired(field, gfx, area, cx.now());
            self.apply_timer(cx);
            if outcome.redraw() {
                cx.request_redraw_all();
            }
            outcome
        }

        fn apply_timer<M: 'static>(&mut self, cx: &mut Cx<'_, M>) {
            match self.next_timer() {
                Timer::Set(at) => cx.timer_at(self.timer, at),
                Timer::Cancel => {
                    cx.cancel_timer(self.timer);
                }
                Timer::Leave => {}
            }
        }

        /// The event's outcome, and whether a menu was closed by a focus or scale change (the
        /// outcome is `Unused` then, so the application still sees the event).
        #[allow(clippy::too_many_arguments)]
        pub(super) fn route(
            &mut self,
            field: &mut TextField,
            gfx: &mut Gfx,
            area: FieldArea,
            focused: bool,
            mods: Mods,
            now: Instant,
            event: &Event,
        ) -> (FieldOutcome, bool) {
            let outcome = match event {
                Event::PointerMove { x, y } => self.pointer_move(field, gfx, area, (*x, *y), now),
                Event::Press { x, y, button, .. } => match button {
                    Button::Left => self.press(field, gfx, area, (*x, *y), mods, now),
                    Button::Right => self.secondary_press(field, gfx, area, (*x, *y), now),
                    _ => self.other_press((*x, *y)),
                },
                Event::Release {
                    x,
                    y,
                    button: Button::Left,
                } => self.release(field, gfx, area, (*x, *y), now),
                Event::Wheel { dy, .. } => match self.last_at {
                    Some(at) => self.wheel(at, *dy),
                    None => FieldOutcome::Unused,
                },
                Event::Key(k) if focused || self.menu.is_some() => match &k.name {
                    Some(name) => self.key(field, gfx, area, name, k.mods, k.text.as_deref()),
                    None => FieldOutcome::Unused,
                },
                Event::Text(t) if focused || self.menu.is_some() => self.text(field, t),
                Event::Preedit(p) if focused => self.preedit(field, p.clone()),
                Event::Focus(false) | Event::PressLost { .. } | Event::Scale(_) => {
                    return (FieldOutcome::Unused, self.cancel());
                }
                _ => FieldOutcome::Unused,
            };
            (outcome, false)
        }

        /// The timer came: it is no longer set. [`tick`](Self::tick), with `Taken` for nothing
        /// moved, since the timer was this input's.
        pub(super) fn fired(
            &mut self,
            field: &mut TextField,
            gfx: &mut Gfx,
            area: FieldArea,
            now: Instant,
        ) -> FieldOutcome {
            self.armed = false;
            match self.tick(field, gfx, area, now) {
                FieldOutcome::Unused => FieldOutcome::Taken,
                moved => moved,
            }
        }

        /// Sets the timer to the deadline, or cancels the one this input set when nothing is
        /// due. A timer some other input set under the same name is never cancelled here.
        pub(super) fn next_timer(&mut self) -> Timer {
            match self.deadline() {
                Some(at) => {
                    self.armed = true;
                    Timer::Set(at)
                }
                None if self.armed => {
                    self.armed = false;
                    Timer::Cancel
                }
                None => Timer::Leave,
            }
        }
    }
}

#[cfg(test)]
#[path = "input_tests.rs"]
mod tests;
