//! The cursor the pointer shows: one small enum of the cursors the widgets ask for, the
//! helpers that turn the hits they already answer into one, [`pick`] to choose among them, and
//! [`CursorState`] so the loop tells the window only when the icon changed.
//!
//! The application owns the events, the window and the call to `Window::set_cursor`; nothing
//! here reads a pointer, draws anything or remembers a rectangle. On a pointer move the
//! application asks its widgets' hits in the order they are stacked, topmost first:
//! [`table::Table::hit`](crate::table::Table::hit) through [`of_header`],
//! [`splitter::Splitter::hit`](crate::splitter::Splitter::hit) through [`of_splitter`], and
//! [`window::edge_at`](crate::window::edge_at) through [`of_window_edge`]. It passes the
//! answers to [`pick`], then [`CursorState::set`] with what that gave it. The state answers
//! `Some(icon)` only when the icon is not the one the window was last told, so most moves cost
//! nothing.
//!
//! A drag keeps its cursor after the pointer leaves the area that started it. The widgets that
//! drag (the table's edge and the splitter's handle) already answer their hit while the drag
//! is on; for any other drag the application lists the held cursor before the hover ones:
//! `pick([held, over_text, over_slider])`. The first `Some` wins, so a held [`Cursor::Grab`]
//! or [`Cursor::ResizeEw`] is not replaced by whatever the pointer is over now.
//!
//! Deliberately not here: custom cursor images (winit's `CustomCursor`, for an application
//! that wants a drawn pointer); animated cursors; hiding the cursor while typing, which is the
//! text field's caller's business and needs a clock; the eight one-way resize arrows of a
//! window's border (a window loop that wants `NwResize` rather than [`Cursor::ResizeNwse`] can
//! use winit's own `CursorIcon::from(ResizeDirection)`, which this crate does not replace);
//! and drag-to-reorder of table columns, which the table does not do yet: when it does, the
//! application passes [`Cursor::Grab`] while the column is held and [`Cursor::Grabbing`] once
//! it moves.

use winit::window::{CursorIcon, ResizeDirection};

use crate::splitter::Axis;
use crate::table::HeaderHit;

/// What the pointer shows over a part of a window: the cursors this toolkit's widgets ask
/// for, and the ones an application's own areas can ask for with them.
#[non_exhaustive]
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum Cursor {
    /// The system's ordinary arrow: nothing under the pointer is pressed, dragged or resized.
    #[default]
    Default,
    /// Over text that a click places a caret in.
    Text,
    /// Over something a click activates: a link, a header title, a button.
    Hand,
    /// Over something that can be dragged and is not held yet.
    Grab,
    /// Over something that is being dragged.
    Grabbing,
    /// Over something that cannot be used now, such as a disabled control.
    NotAllowed,
    /// On a vertical edge dragged left and right: a table column's edge, a splitter that cuts
    /// side by side panes.
    ResizeEw,
    /// On a horizontal edge dragged up and down: a splitter that cuts a top pane from a bottom
    /// one.
    ResizeNs,
    /// On a corner dragged from the top left to the bottom right, or the other way: a
    /// frameless window's NorthWest or SouthEast corner.
    ResizeNwse,
    /// On a corner dragged from the top right to the bottom left, or the other way: a
    /// frameless window's NorthEast or SouthWest corner.
    ResizeNesw,
    /// Over something that can be moved as a whole.
    Move,
    /// The application is busy and nothing under the pointer can be used until it is done.
    Wait,
    /// The application is busy but the window still answers.
    Progress,
    /// Over a point picked with precision, such as a colour or a sample on a waveform.
    Crosshair,
}

impl Cursor {
    /// The icon winit draws for this cursor. The only name that changes is [`Cursor::Hand`],
    /// which winit calls `Pointer`; the other cursors have an icon of the same name. Every
    /// cursor here has an icon: nothing is silently drawn as the arrow.
    pub fn icon(self) -> CursorIcon {
        match self {
            Cursor::Default => CursorIcon::Default,
            Cursor::Text => CursorIcon::Text,
            Cursor::Hand => CursorIcon::Pointer,
            Cursor::Grab => CursorIcon::Grab,
            Cursor::Grabbing => CursorIcon::Grabbing,
            Cursor::NotAllowed => CursorIcon::NotAllowed,
            Cursor::ResizeEw => CursorIcon::EwResize,
            Cursor::ResizeNs => CursorIcon::NsResize,
            Cursor::ResizeNwse => CursorIcon::NwseResize,
            Cursor::ResizeNesw => CursorIcon::NeswResize,
            Cursor::Move => CursorIcon::Move,
            Cursor::Wait => CursorIcon::Wait,
            Cursor::Progress => CursorIcon::Progress,
            Cursor::Crosshair => CursorIcon::Crosshair,
        }
    }

    /// The resize cursor for a window's resize direction: East and West are [`ResizeEw`],
    /// North and South are [`ResizeNs`], NorthWest and SouthEast are [`ResizeNwse`], and
    /// NorthEast and SouthWest are [`ResizeNesw`].
    ///
    /// The direction is not kept, only its axis or diagonal, so the two opposite directions
    /// of a pair share the double-headed cursor. All eight directions have an answer; there is
    /// no `Option`.
    ///
    /// [`ResizeEw`]: Cursor::ResizeEw
    /// [`ResizeNs`]: Cursor::ResizeNs
    /// [`ResizeNwse`]: Cursor::ResizeNwse
    /// [`ResizeNesw`]: Cursor::ResizeNesw
    pub fn of_resize(dir: ResizeDirection) -> Cursor {
        match dir {
            ResizeDirection::East | ResizeDirection::West => Cursor::ResizeEw,
            ResizeDirection::North | ResizeDirection::South => Cursor::ResizeNs,
            ResizeDirection::NorthWest | ResizeDirection::SouthEast => Cursor::ResizeNwse,
            ResizeDirection::NorthEast | ResizeDirection::SouthWest => Cursor::ResizeNesw,
        }
    }
}

/// The cursor for what a table's header hit is: an edge is [`Cursor::ResizeEw`] (a drag
/// changes the column's width), a title is [`Cursor::Hand`] (a click sorts by it).
///
/// `None` (the pointer is not in the header) gives `None`. A column whose title does nothing
/// should not be passed as a hit at all; the table reports a title for every column, so the
/// caller that knows a title is inert passes `None` instead. The table cannot reorder its
/// columns, so [`Cursor::Grab`] and [`Cursor::Grabbing`] are never asked for here.
///
/// While an edge is in hand the table answers that edge wherever the pointer is, so the cursor
/// stays [`Cursor::ResizeEw`] for the whole drag without the caller remembering it.
pub fn of_header(hit: Option<HeaderHit>) -> Option<Cursor> {
    match hit? {
        HeaderHit::Title(_) => Some(Cursor::Hand),
        HeaderHit::Edge(_) => Some(Cursor::ResizeEw),
    }
}

/// The cursor for a splitter's handle: [`Cursor::ResizeEw`] for an [`Axis::Horizontal`]
/// splitter (its handle is a vertical line between side-by-side panes, dragged left and
/// right), [`Cursor::ResizeNs`] for an [`Axis::Vertical`] one (a horizontal line between a top
/// and a bottom pane, dragged up and down).
///
/// `hit` is [`Splitter::hit`](crate::splitter::Splitter::hit)'s answer and is also true while
/// the handle is in hand; false gives `None`. Reading the axis from the splitter's own
/// vocabulary is deliberate: `Axis::Horizontal` names the way the panes lie, not the way the
/// handle is dragged, and the two are at right angles.
pub fn of_splitter(axis: Axis, hit: bool) -> Option<Cursor> {
    if !hit {
        return None;
    }
    Some(match axis {
        Axis::Horizontal => Cursor::ResizeEw,
        Axis::Vertical => Cursor::ResizeNs,
    })
}

/// The cursor for a frameless window's border: `None` (the pointer is off
/// [`window::EDGE`](crate::window::EDGE)'s band) gives `None`; a direction from
/// [`window::edge_at`](crate::window::edge_at) gives [`Cursor::of_resize`] of it.
pub fn of_window_edge(edge: Option<ResizeDirection>) -> Option<Cursor> {
    edge.map(Cursor::of_resize)
}

/// The cursor to show from the candidates the areas under the pointer answer, in the order
/// the caller lists them: the first `Some` wins, and `Default` is the answer when every
/// candidate is `None` or the list is empty.
///
/// The caller lists the topmost area first, so a button over a background answers before the
/// background. A drag in progress goes before all of them: `pick([held, hover])` keeps the
/// held cursor even when the pointer is over something else, or outside the window's areas,
/// until the release passes `None` for `held`.
///
/// It reads no further than the first answer, so a lazy iterator or one with side effects is
/// read only as far as needed. Nothing is remembered between calls.
pub fn pick(candidates: impl IntoIterator<Item = Option<Cursor>>) -> Cursor {
    candidates.into_iter().flatten().next().unwrap_or_default()
}

/// The cursor the window was last told, so a pointer move that changes nothing costs no
/// `Window::set_cursor` call.
///
/// The application makes one of these beside its window, calls [`set`](Self::set) with
/// [`pick`]'s answer on every pointer move, and calls the window's `set_cursor` only with the
/// `Some` icon it gets back. [`forget`](Self::forget) is for the pointer leaving the window or
/// the window losing focus.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct CursorState {
    /// The cursor last passed to [`set`](CursorState::set), or `None` before the first call
    /// and after [`forget`](CursorState::forget).
    current: Option<Cursor>,
}

impl CursorState {
    /// A state that has told no window anything yet: the first [`set`](Self::set) always
    /// answers an icon.
    pub const fn new() -> CursorState {
        CursorState { current: None }
    }

    /// The cursor the window was last told, or `None` before the first [`set`](Self::set) and
    /// after [`forget`](Self::forget).
    pub fn current(&self) -> Option<Cursor> {
        self.current
    }

    /// Makes `cursor` the one the window should show. Answers the icon for the caller to pass
    /// to `Window::set_cursor`, but only when it differs from the cursor set last time;
    /// setting the same cursor again answers `None` and the caller tells the window nothing.
    ///
    /// After [`forget`](Self::forget) the same cursor answers its icon again, because the
    /// window may no longer show it.
    pub fn set(&mut self, cursor: Cursor) -> Option<CursorIcon> {
        if self.current == Some(cursor) {
            return None;
        }
        self.current = Some(cursor);
        Some(cursor.icon())
    }

    /// The pointer left the window, or the window lost focus: forgets what was shown, so the
    /// next [`set`](Self::set) answers its icon even if it is the cursor that was last set.
    ///
    /// This does not tell any window anything. If the platform keeps the last icon while the
    /// pointer is outside, the caller sets the system's default cursor itself at this moment.
    pub fn forget(&mut self) {
        self.current = None;
    }
}

#[cfg(test)]
#[path = "cursor_tests.rs"]
mod tests;
