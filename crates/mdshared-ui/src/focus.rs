//! Which widget has the keyboard, and the Tab order over the widgets a frame declares.
//!
//! A window's widgets come and go: a pane closes, a dialog opens, a field is hidden. The
//! application declares, once per frame, the widgets that take the keyboard now, by calling
//! [`Focus::begin_frame`], one [`Focus::add`] per widget in the order Tab should visit them,
//! and [`Focus::end_frame`]. The model answers which one holds the focus
//! ([`Focus::focused`], [`Focus::is_focused`]) and whether its ring shows
//! ([`Focus::ring_visible`], which follows the last input: a key shows rings and a pointer
//! hides them until the next key). It moves the focus for Tab ([`Focus::tab`]), a click
//! ([`Focus::focus`]), a blur ([`Focus::blur`]) or a closing scope ([`Focus::pop_scope`]).
//!
//! A widget that is no longer declared loses the focus, and the focus goes to the widget that
//! took its place in the order, else to the one before it, else nowhere. A scope
//! ([`Focus::push_scope`]) is a dialog or a menu: Tab visits only the widgets declared in it,
//! and closing it ([`Focus::pop_scope`]) returns the focus to the widget that had it before.
//! Scopes nest.
//!
//! The caller owns the widgets, their values, their hits and their keys; the model holds only
//! ids, so the id type is the application's own (`K: Copy + Eq`, a `u32` or an enum). It draws
//! nothing but the focus ring ([`draw_ring`]), and reads no clock. Deliberately not here: the
//! arrow keys, Enter and Escape; modal window behaviour beyond Tab containment (no backdrop,
//! no dismissal); a text field's own focus and caret; the focused look of a widget, which each
//! widget takes as a flag ([`slider::Look::focused`](crate::slider::Look),
//! [`Tick`](crate::check::Tick)); what a screen reader should say (roadmap W8); and a second
//! focus model for [`settings::Form`](crate::settings::Form), which keeps its own.
//!
//! ```text
//! focus.begin_frame();
//! focus.add(Widget::FileMenu);
//! focus.add_with(Widget::Find, Options::no_tab_stop());
//! focus.add(Widget::Editor);
//! focus.end_frame();
//! ```

use crate::gfx::{Painter, Path, Rect};
use crate::theme::Palette;

/// The thickness of the focus ring in logical px. It runs one px outside the rectangle it
/// marks, so it reaches three px outside it.
pub const RING: f32 = 2.0;

/// The distance from the marked rectangle's edge to the middle of the ring's line.
const GAP: f32 = 1.0 + RING / 2.0;

/// How a widget joins the Tab order. [`add`](Focus::add) uses [`Options::new`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Options {
    /// Tab stops at the widget. A widget that is not a Tab stop can still take the focus from
    /// a press, and Tab passes over it.
    pub tab_stop: bool,
}

impl Default for Options {
    fn default() -> Options {
        Options::new()
    }
}

impl Options {
    /// The options of an ordinary widget: Tab stops at it.
    pub const fn new() -> Options {
        Options { tab_stop: true }
    }

    /// The options of a widget Tab passes over, such as a click-only control or a label with a
    /// link in it. A press can still focus it.
    pub const fn no_tab_stop() -> Options {
        Options { tab_stop: false }
    }
}

/// One widget as a frame declared it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Decl<K> {
    id: K,
    /// The scope it was declared in; `None` is the base, what is left when no scope is open.
    scope: Option<K>,
    tab_stop: bool,
}

/// A scope that is open, and what closing it has to restore.
#[derive(Clone, Copy, Debug)]
struct Scope<K> {
    id: K,
    /// The widget that had the focus when the scope opened, if any.
    before: Option<K>,
    /// The Tab stop that followed `before` in the order then, for when `before` is gone.
    next: Option<K>,
    /// The Tab stop that came before `before`, for the same reason.
    previous: Option<K>,
}

/// A scope pushed since the last ended frame: its first widget takes the focus then.
#[derive(Clone, Copy, Debug)]
struct Pending<K> {
    scope: K,
    /// The widget the caller named, which wins over the scope's first.
    on: Option<K>,
}

/// The keyboard focus over the widgets an application declares each frame, the Tab order, the
/// ring's visibility and the stack of open scopes.
///
/// The id type is the application's: any `Copy + Eq` type, so a `u32`, an enum or a newtype.
/// A `Focus` holds no widget, no rectangle and no event; the application asks it where the
/// focus is and tells it what the user did. Its cost is one small entry per declared widget and
/// one per open scope.
#[derive(Clone, Debug)]
pub struct Focus<K> {
    /// What the last [`begin_frame`](Focus::begin_frame)..[`end_frame`](Focus::end_frame)
    /// declared, in declaration order. A widget is declared once: a second call with the same
    /// id in one frame is ignored, and the first call's options stand.
    declared: Vec<Decl<K>>,
    current: Option<K>,
    /// Rings show: the last input that moved the focus or was typed was the keyboard.
    ring: bool,
    scopes: Vec<Scope<K>>,
    pending: Option<Pending<K>>,
    /// The ids that were in the active scope's order at the last ended frame, which says where
    /// a widget that disappears was standing.
    last_order: Vec<K>,
}

impl<K> Default for Focus<K> {
    fn default() -> Focus<K> {
        Focus {
            declared: Vec::new(),
            current: None,
            ring: false,
            scopes: Vec::new(),
            pending: None,
            last_order: Vec::new(),
        }
    }
}

impl<K> Focus<K> {
    /// A model with nothing declared and no focus.
    pub fn new() -> Focus<K> {
        Focus::default()
    }
}

impl<K: Copy + Eq> Focus<K> {
    // ---- declaration ---------------------------------------------------------------------

    /// Starts a frame's declaration: the widgets of the last frame are forgotten, the focus is
    /// kept until [`end_frame`](Focus::end_frame) settles it. Call it once per frame, before
    /// the adds and the drawing that records what exists.
    pub fn begin_frame(&mut self) {
        self.declared.clear();
    }

    /// Declares a widget in the base scope as a Tab stop, after everything declared before it.
    /// A second declaration of the same id in one frame is ignored, options and all.
    pub fn add(&mut self, id: K) {
        self.declare(None, id, Options::new());
    }

    /// [`add`](Focus::add) with the widget's options: a widget that is not a Tab stop is
    /// skipped by Tab but can still take the focus from a press.
    pub fn add_with(&mut self, id: K, options: Options) {
        self.declare(None, id, options);
    }

    /// Declares a widget inside `scope`, in that scope's own order. Tab visits it only while
    /// `scope` is the innermost open one. Declaring into a scope that is not on the stack is
    /// allowed and harmless: the widget exists for the focus it may already have, but Tab does
    /// not reach it.
    pub fn add_in(&mut self, scope: K, id: K) {
        self.declare(Some(scope), id, Options::new());
    }

    /// [`add_in`](Focus::add_in) with the widget's options.
    pub fn add_in_with(&mut self, scope: K, id: K, options: Options) {
        self.declare(Some(scope), id, options);
    }

    fn declare(&mut self, scope: Option<K>, id: K, options: Options) {
        if self.declared.iter().any(|d| d.id == id) {
            return;
        }
        self.declared.push(Decl {
            id,
            scope,
            tab_stop: options.tab_stop,
        });
    }

    /// Ends the frame's declaration and settles the focus.
    ///
    /// A scope opened since the last frame takes the focus, on the widget it named or on the
    /// first it declared (none when it declared none, and it keeps waiting for the first
    /// widget of a later frame). A focused widget that is no longer declared anywhere is
    /// replaced by the Tab stop that took its place in the order, else the one before it, else
    /// none. A focused widget that is still declared keeps the focus, even when Tab would not
    /// visit it (a non-Tab-stop widget, or one behind a scope opened without
    /// [`push_scope`](Focus::push_scope)).
    pub fn end_frame(&mut self) {
        let active = self.active_scope();
        let order = self.order(active);

        let mut settled = false;
        let mut named = None;
        if let Some(pending) = self.pending.as_ref()
            && Some(pending.scope) == active
        {
            if order.is_empty() {
                self.current = None;
                settled = true;
            } else {
                named = Some(
                    pending
                        .on
                        .filter(|id| order.iter().any(|&(x, _)| x == *id))
                        .unwrap_or(order[0].0),
                );
            }
        }
        if let Some(id) = named {
            self.current = Some(id);
            self.pending = None;
            settled = true;
        }

        if !settled
            && let Some(current) = self.current
            && !self.declared.iter().any(|d| d.id == current)
        {
            // The widget is gone: take its place in the order, else the last stop left.
            let stops: Vec<K> = order
                .iter()
                .filter(|&&(_, tab_stop)| tab_stop)
                .map(|&(id, _)| id)
                .collect();
            let at = self
                .last_order
                .iter()
                .position(|&id| id == current)
                .unwrap_or(self.last_order.len());
            self.current = stops.get(at).copied().or_else(|| stops.last().copied());
        }

        self.last_order = order.iter().map(|&(id, _)| id).collect();
    }

    // ---- questions -----------------------------------------------------------------------

    /// The widget with the focus, or `None` when nothing has it.
    pub fn focused(&self) -> Option<K> {
        self.current
    }

    /// Whether `id` is the widget with the focus. It does not ask whether the widget is
    /// declared: an id that has vanished keeps the focus until [`end_frame`](Focus::end_frame)
    /// moves it.
    pub fn is_focused(&self, id: K) -> bool {
        self.current == Some(id)
    }

    /// Whether `id` has the focus and its ring shows. The ring shows only after a key moved the
    /// focus or was typed into it; a pointer press or a click hides it until the next key.
    pub fn ring_visible(&self, id: K) -> bool {
        self.ring && self.current == Some(id)
    }

    // ---- input ---------------------------------------------------------------------------

    /// Moves the focus to the next Tab stop in the active scope (the previous one with
    /// `shift`), wrapping round at either end, and shows the rings: this was the keyboard.
    /// Returns whether there was a Tab stop to move to; when the active scope has none, the
    /// focus stays where it is and the answer is `false`, so the application may let Tab do
    /// something else (move between panes, leave the window).
    pub fn tab(&mut self, shift: bool) -> bool {
        self.ring = true;
        let order = self.order(self.active_scope());
        let stops: Vec<usize> = order
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.1)
            .map(|(i, _)| i)
            .collect();
        if stops.is_empty() {
            return false;
        }
        let at = self
            .current
            .and_then(|id| order.iter().position(|&(x, _)| x == id));
        let next = match at {
            Some(i) if shift => stops.iter().rev().find(|&&s| s < i).copied(),
            Some(i) => stops.iter().find(|&&s| s > i).copied(),
            None => None,
        }
        .unwrap_or_else(|| {
            if shift {
                stops[stops.len() - 1]
            } else {
                stops[0]
            }
        });
        self.current = Some(order[next].0);
        true
    }

    /// Puts the focus on `id` because the pointer chose it (a click, a press on a field), and
    /// hides the rings until the next key. Returns whether the focus was somewhere else, which
    /// is what an application needs to know to redraw. `id` need not be declared here: a press
    /// usually arrives after the frame that drew the widget.
    pub fn focus(&mut self, id: K) -> bool {
        let changed = self.current != Some(id);
        self.current = Some(id);
        self.ring = false;
        changed
    }

    /// Takes the focus off every widget, hiding the rings. Nothing has the focus until a Tab or
    /// a [`focus`](Focus::focus) gives it back.
    pub fn blur(&mut self) {
        self.current = None;
        self.ring = false;
    }

    /// Records that the keyboard was used (a key typed into the focused field, an arrow key
    /// that moved inside a widget): the rings show again until the pointer is used.
    pub fn key_used(&mut self) {
        self.ring = true;
    }

    /// Records that the pointer was used (a press, a wheel turn, a drag): the rings hide until
    /// the next key.
    pub fn pointer_used(&mut self) {
        self.ring = false;
    }

    // ---- scopes --------------------------------------------------------------------------

    /// Opens a scope: a dialog, a menu, a popover. At the next
    /// [`end_frame`](Focus::end_frame) the focus moves to the first widget declared in it
    /// (none when it declares none yet), and Tab then visits only that scope's widgets, in
    /// their own declaration order, wrapping inside it. While a scope is open, the widgets
    /// declared outside it are still declared, so they keep a focus they are given, but Tab
    /// does not reach them.
    ///
    /// The ring's visibility does not change: a dialog opened from the keyboard keeps the
    /// rings the last key showed, and one opened by a click opens without.
    ///
    /// A scope id names one open scope, so pushing the same id twice for two different things
    /// makes them one as far as Tab is concerned.
    pub fn push_scope(&mut self, scope: K) {
        self.open(scope, None);
    }

    /// [`push_scope`](Focus::push_scope), with `id` taking the focus instead of the scope's
    /// first widget when the scope declares it. A caller uses it for a dialog that starts on
    /// its text field, or a menu that starts on the item the pointer is over.
    pub fn push_scope_on(&mut self, scope: K, id: K) {
        self.open(scope, Some(id));
    }

    fn open(&mut self, scope: K, on: Option<K>) {
        let active = self.active_scope();
        let order = self.order(active);
        let before = self.current;
        let at = before.and_then(|id| order.iter().position(|&(x, _)| x == id));
        let stops: Vec<(usize, K)> = order
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.1)
            .map(|(i, &(id, _))| (i, id))
            .collect();
        let next = at.and_then(|i| stops.iter().find(|&&(s, _)| s > i).map(|&(_, id)| id));
        let previous =
            at.and_then(|i| stops.iter().rev().find(|&&(s, _)| s < i).map(|&(_, id)| id));
        self.scopes.push(Scope {
            id: scope,
            before,
            next,
            previous,
        });
        self.pending = Some(Pending { scope, on });
    }

    /// Closes the innermost scope and returns the focus to where it was before that scope
    /// opened: the widget that had it, if it is still declared; otherwise the Tab stop that
    /// followed it then, otherwise the one before it, otherwise the scope below's first Tab
    /// stop, otherwise nothing. Does nothing and answers `false` when no scope is open.
    ///
    /// The widgets are those declared in the last ended frame, because a scope usually closes
    /// between frames, while the application handles the key or click that closes it.
    pub fn pop_scope(&mut self) -> bool {
        let Some(scope) = self.scopes.pop() else {
            return false;
        };
        if let Some(pending) = self.pending
            && !self.scopes.iter().any(|s| s.id == pending.scope)
        {
            self.pending = None;
        }
        let active = self.active_scope();
        let order = self.order(active);
        let stops: Vec<K> = order
            .iter()
            .filter(|&&(_, tab_stop)| tab_stop)
            .map(|&(id, _)| id)
            .collect();
        let declared = |id: K| order.iter().any(|&(x, _)| x == id);
        self.current = match scope.before {
            Some(before) if declared(before) => Some(before),
            Some(_) => scope
                .next
                .filter(|&id| declared(id))
                .or_else(|| scope.previous.filter(|&id| declared(id)))
                .or_else(|| stops.first().copied()),
            None => None,
        };
        true
    }

    /// The innermost open scope, or `None` when only the base is open.
    pub fn scope(&self) -> Option<K> {
        self.scopes.last().map(|s| s.id)
    }

    fn active_scope(&self) -> Option<K> {
        self.scopes.last().map(|s| s.id)
    }

    /// The widgets declared in `scope`, in order, with whether Tab stops at each.
    fn order(&self, scope: Option<K>) -> Vec<(K, bool)> {
        self.declared
            .iter()
            .filter(|d| d.scope == scope)
            .map(|d| (d.id, d.tab_stop))
            .collect()
    }
}

/// Draws the focus ring around `r`: an accent line [`RING`] px thick that starts one px outside
/// `r`, so it reaches three px outside and never covers the rounded widget. The corners are
/// rounded by `radius`, clamped to half the shorter side; a radius that is not finite counts as
/// zero.
///
/// The widget is drawn after it, as the settings dialog draws its controls; the ring lies
/// outside `r` and needs no background of its own. Its corners follow `radius`, so the four
/// square corners of `r` outside the rounded ones are between the ring and the widget, as they
/// are outside the widget itself. A rectangle with a side that is not positive or not finite
/// draws nothing.
pub fn draw_ring(p: &mut Painter, pal: &Palette, r: Rect, radius: f32) {
    let numbers = [r.x, r.y, r.w, r.h];
    if numbers.iter().any(|v| !v.is_finite()) || r.w <= 0.0 || r.h <= 0.0 {
        return;
    }
    let outer = r.inset(-GAP);
    let radius = if radius.is_finite() {
        radius.max(0.0)
    } else {
        0.0
    };
    let radius = (radius + GAP)
        .min(outer.w / 2.0)
        .min(outer.h / 2.0)
        .max(0.0);
    p.stroke_path(&rounded_rect(outer, radius), RING, pal.accent, 1.0);
}

/// A closed rounded rectangle through the corners of `r`, each corner a quarter circle of
/// `radius` approximated by a cubic (with the constant that keeps it within a fraction of a
/// pixel of a circle).
fn rounded_rect(r: Rect, radius: f32) -> Path {
    const KAPPA: f32 = 0.552_284_8;
    let (x0, y0, x1, y1) = (r.x, r.y, r.right(), r.bottom());
    let radius = radius.min(r.w / 2.0).min(r.h / 2.0).max(0.0);
    let k = radius * KAPPA;
    Path::new()
        .move_to(x0 + radius, y0)
        .line_to(x1 - radius, y0)
        .curve_to(
            (x1 - radius + k, y0),
            (x1, y0 + radius - k),
            x1,
            y0 + radius,
        )
        .line_to(x1, y1 - radius)
        .curve_to(
            (x1, y1 - radius + k),
            (x1 - radius + k, y1),
            x1 - radius,
            y1,
        )
        .line_to(x0 + radius, y1)
        .curve_to(
            (x0 + radius - k, y1),
            (x0, y1 - radius + k),
            x0,
            y1 - radius,
        )
        .line_to(x0, y0 + radius)
        .curve_to(
            (x0, y0 + radius - k),
            (x0 + radius - k, y0),
            x0 + radius,
            y0,
        )
        .close()
}

#[cfg(test)]
#[path = "focus_tests.rs"]
mod tests;
