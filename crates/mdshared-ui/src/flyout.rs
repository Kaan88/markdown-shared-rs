//! A flyout: a card anchored to a rectangle (a button, a cell, a word), with any content the
//! application draws in it: a line of text, a check box, a button, a small form.
//!
//! A [`Flyout`] is the card's model. It is made with the anchor and the size of the content
//! ([`Flyout::new`]) and placed inside an area ([`Flyout::place_in`]) the way
//! [`Menu::around`](crate::menu::Menu::around) places a menu, by the same geometry
//! ([`submenu::around`](crate::submenu::around)): below the anchor with its left edge at the
//! anchor's left, flipped above where it would cross the bottom of the area, flipped to the left
//! where it would cross the right, and pushed inside the area when it fits on neither side, so it
//! stays on screen at any corner. A flyout can prefer to open above ([`Side::Above`]), and
//! stands a [`GAP`] away from its anchor ([`Flyout::gap`]).
//!
//! The application owns the content, the events and the time. It lays out and draws the content
//! inside [`Flyout::content_rect`], the card less its [`PADDING`], after the card itself
//! ([`draw_card`], or [`draw_card_animated`] for a fade). It passes a press to
//! [`Flyout::pointer_down`] and a key to [`Flyout::key`] first, and acts on the [`Outcome`]:
//!
//! - [`Outcome::Inside`]: the press is on the card; hand it to the content.
//! - [`Outcome::Outside`]: the press is away from the card and the anchor; close the flyout,
//!   and the press may go on to what is under it.
//! - [`Outcome::Anchor`]: the press is on the anchor; close the flyout and keep the press (and
//!   its release) from the anchor. A click on the anchor of an open flyout closes it and does
//!   not open it again at once, as in WinUI: an anchor that acts on its release would otherwise
//!   reopen what its press closed.
//! - [`Outcome::Close`]: Escape; close the flyout.
//! - [`Outcome::Unused`]: any other key, or a pointer that is not a number; hand the key to the
//!   content that has the focus.
//!
//! The focus: a flyout is a focus scope of its own in [`focus::Focus`](crate::focus::Focus).
//! The application picks an id for the scope, of its own id type, and:
//!
//! 1. opens it with [`open_scope`] when the flyout opens (and, optionally, names the widget
//!    of the content that takes the focus first);
//! 2. each frame, declares its widgets as before and the content's widgets inside the scope
//!    ([`Focus::add_in`](crate::focus::Focus::add_in)): Tab then visits only those, wrapping
//!    inside the card;
//! 3. closes it with [`close_scope`] when the flyout closes, for whatever reason (Escape, a
//!    press outside or on the anchor, a button of the content): the focus goes back to the
//!    widget that had it when the flyout opened, as in WinUI.
//!
//! That widget is not always the anchor. A flyout opened by a click or Enter on a button that
//! has the focus gives it back to the button; one opened by a shortcut (Ctrl+K in an editor)
//! on a toolbar button gives it back to the editor. The anchor is only a rectangle here and
//! need not be a widget that takes the focus at all (a cell, a word). Nothing here moves the
//! focus to the anchor; an application that wants that calls
//! [`Focus::focus`](crate::focus::Focus::focus) itself before [`open_scope`].
//!
//! ```text
//! // Opening, on the anchor's click, its Enter or a shortcut:
//! fly = Some(Flyout::new(button, (240.0, 96.0)));
//! flyout::open_scope(&mut focus, Id::Flyout, None);
//! // Each frame:
//! focus.begin_frame();
//! focus.add(Id::Editor);
//! focus.add(Id::Button);
//! if fly.is_some() { focus.add_in(Id::Flyout, Id::Check); focus.add_in(Id::Flyout, Id::Ok); }
//! focus.end_frame();
//! // On a press or a key:
//! match fly.pointer_down(x, y) {            // or fly.key(&key)
//!     Outcome::Inside | Outcome::Unused => { /* the content's */ }
//!     _ => { fly = None; flyout::close_scope(&mut focus, Id::Flyout); }
//! }
//! ```
//!
//! Nothing here reads a clock or remembers anything but the last placement: the card's
//! rectangle is worked out again by every [`Flyout::place_in`], which the application calls
//! each frame before it asks [`Flyout::rect`], [`Flyout::content_rect`] or passes a press.
//!
//! Deliberately not here: the content's own layout, drawing, hits and keys (the application's,
//! with [`check`](crate::check), [`overlay::draw_dialog_button`](crate::overlay::draw_dialog_button)
//! and the rest); a pointer that leaves the card closing it (only a press does); a tail or
//! arrow pointing at the anchor; a card centred on its anchor, or placed to its left or right
//! (the menu's geometry is the one used); a backdrop or a modal flyout (that is a dialog,
//! [`overlay::draw_backdrop`](crate::overlay::draw_backdrop)); a card taller than its area
//! scrolling (it is cut to the area, and the content is the caller's to fit); right-to-left;
//! several flyouts open at once beyond what nested focus scopes give; and an `a11y` builder: a
//! caller reports an open flyout as `a11y::dialog` with `modal` false (a group like a dialog
//! that is not modal), with its content as children.

use crate::anim::pop;
use crate::focus::Focus;
use crate::gfx::{Painter, Rect};
use crate::keys::KeyName;
use crate::menu::MENU_DROP;
use crate::theme::Palette;

/// The room between the card's edge and its content, on every side, in logical px.
pub const PADDING: f32 = 16.0;

/// The distance between the anchor and the card above or below it, in logical px.
pub const GAP: f32 = 4.0;

/// The corner radius of the card, a menu's.
pub const RADIUS: f32 = 8.0;

/// How far the card's shadow reaches past its rectangle, as for a menu: what
/// [`draw_card_animated`] saves and fades.
const SHADOW: f32 = 18.0;

/// The side of the anchor a flyout opens on when it fits there.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Side {
    /// Below the anchor, flipped above when there is no room below. The default.
    #[default]
    Below,
    /// Above the anchor, flipped below when there is no room above.
    Above,
}

/// What a press or a key did to a [`Flyout`]. See the module's documentation for what the
/// application does with each.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// A press on the card: the content's.
    Inside,
    /// A press away from the card and the anchor: close the flyout. The press may go on to what
    /// is under it.
    Outside,
    /// A press on the anchor, away from the card: close the flyout, and keep this press and its
    /// release from the anchor so that it does not open the flyout again.
    Anchor,
    /// Escape: close the flyout.
    Close,
    /// Not the flyout's: a key other than Escape, for the content that has the focus, or a
    /// press at a point that is not a number, which is ignored.
    Unused,
}

/// A card anchored to a rectangle, its content drawn by the application. See the module's
/// documentation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Flyout {
    anchor: Rect,
    /// The content's width and height, finite and not negative.
    content: (f32, f32),
    side: Side,
    gap: f32,
    padding: f32,
    /// Where the last [`place_in`](Flyout::place_in) put the card.
    rect: Rect,
    went_left: bool,
    went_up: bool,
}

/// A length as given, with NaN, infinities and negative lengths taken as none.
fn length(v: f32) -> f32 {
    if v.is_finite() { v.max(0.0) } else { 0.0 }
}

/// Whether every side of `r` is a number.
fn finite(r: Rect) -> bool {
    [r.x, r.y, r.w, r.h].iter().all(|v| v.is_finite())
}

/// `r` turned upside down about the line y = 0.
fn mirrored(r: Rect) -> Rect {
    Rect::new(r.x, -r.bottom(), r.w, r.h)
}

impl Flyout {
    /// A flyout by `anchor` for content `content_size.0` wide and `content_size.1` high, in
    /// logical px. It opens below the anchor, [`GAP`] px from it, with [`PADDING`] round the
    /// content. A size that is NaN, infinite or negative is taken as none. The card is not
    /// placed until [`place_in`](Self::place_in): until then its rectangle is empty at the
    /// origin, and every press is outside it.
    pub fn new(anchor: Rect, content_size: (f32, f32)) -> Flyout {
        Flyout {
            anchor,
            content: (length(content_size.0), length(content_size.1)),
            side: Side::Below,
            gap: GAP,
            padding: PADDING,
            rect: Rect::default(),
            went_left: false,
            went_up: false,
        }
    }

    /// The same flyout opening on `side` of its anchor first.
    pub fn side(mut self, side: Side) -> Self {
        self.side = side;
        self
    }

    /// The same flyout `px` from its anchor (above or below it), instead of [`GAP`]. NaN,
    /// infinite and negative gaps are none.
    pub fn gap(mut self, px: f32) -> Self {
        self.gap = length(px);
        self
    }

    /// The same flyout with `px` of room round its content, instead of [`PADDING`]. NaN,
    /// infinite and negative paddings are none.
    pub fn padding(mut self, px: f32) -> Self {
        self.padding = length(px);
        self
    }

    /// Moves the anchor: the window was resized or the page scrolled under an open flyout. The
    /// next [`place_in`](Self::place_in) places the card by the new one.
    pub fn set_anchor(&mut self, anchor: Rect) {
        self.anchor = anchor;
    }

    /// Changes the content's size, taken as [`new`](Self::new) takes it. The next
    /// [`place_in`](Self::place_in) places a card of the new size.
    pub fn set_content_size(&mut self, content_size: (f32, f32)) {
        self.content = (length(content_size.0), length(content_size.1));
    }

    /// The rectangle the flyout is anchored to.
    pub fn anchor(&self) -> Rect {
        self.anchor
    }

    /// The card's size before it is placed: the content's size and the padding on both sides.
    pub fn card_size(&self) -> (f32, f32) {
        (
            self.content.0 + 2.0 * self.padding,
            self.content.1 + 2.0 * self.padding,
        )
    }

    /// Places the card inside `area` (the window, or the part of it a popup may use) and
    /// returns its rectangle, its edges at whole logical px. The geometry is
    /// [`submenu::around`](crate::submenu::around), with the anchor grown by the gap above and
    /// below: the card opens on its [`Side`], flips to the other side where it would leave the
    /// area, flips to the left (its right edge at the anchor's right) where it would cross the
    /// right, and is pushed inside the area, over the anchor, where it fits on neither side. It
    /// stays [`submenu::MARGIN`](crate::submenu::MARGIN) px inside the area; a card larger than
    /// the area is cut to it, and [`content_rect`](Self::content_rect) with it.
    ///
    /// An anchor with a side that is not a number is taken as a point at the area's top left
    /// corner. An area that is not a number gives whatever the arithmetic gives, and no panic.
    pub fn place_in(&mut self, area: Rect) -> Rect {
        let anchor = if finite(self.anchor) {
            self.anchor
        } else {
            Rect::new(area.x, area.y, 0.0, 0.0)
        };
        let grown = Rect::new(
            anchor.x,
            anchor.y - self.gap,
            anchor.w,
            anchor.h + 2.0 * self.gap,
        );
        let (w, h) = self.card_size();
        let (rect, left, above) = match self.side {
            Side::Below => {
                let placed = crate::submenu::around(w, h, grown, area);
                (placed.rect, placed.left, placed.above)
            }
            Side::Above => {
                // Above is below in the area turned upside down.
                let placed = crate::submenu::around(w, h, mirrored(grown), mirrored(area));
                (mirrored(placed.rect), placed.left, !placed.above)
            }
        };
        // The edges are rounded, not the place and the size apart: an edge that lies on the
        // margin stays on it.
        let (x0, x1) = (rect.x.round(), rect.right().round());
        let (y0, y1) = (rect.y.round(), rect.bottom().round());
        self.rect = Rect::new(x0, y0, x1 - x0, y1 - y0);
        self.went_left = left;
        self.went_up = above;
        self.rect
    }

    /// The card's rectangle at the last [`place_in`](Self::place_in).
    pub fn rect(&self) -> Rect {
        self.rect
    }

    /// The part of the card the content goes in: [`rect`](Self::rect) less the padding on
    /// every side, never of a negative size. It is the content's size unless the area cut the
    /// card.
    pub fn content_rect(&self) -> Rect {
        self.rect.inset(self.padding)
    }

    /// Whether the last placement put the card above its anchor.
    pub fn opened_above(&self) -> bool {
        self.went_up
    }

    /// Whether the last placement put the card to the left, its right edge at the anchor's
    /// right.
    pub fn opened_left(&self) -> bool {
        self.went_left
    }

    /// Whether `x`, `y` is on the card.
    pub fn contains(&self, x: f32, y: f32) -> bool {
        self.rect.contains(x, y)
    }

    /// A press of the pointer at `x`, `y`: [`Outcome::Inside`] on the card (also where the card
    /// was pushed over the anchor), [`Outcome::Anchor`] on the anchor, [`Outcome::Outside`]
    /// anywhere else, and [`Outcome::Unused`] for a point that is not a number. It changes
    /// nothing; the application closes the flyout.
    pub fn pointer_down(&self, x: f32, y: f32) -> Outcome {
        if !x.is_finite() || !y.is_finite() {
            Outcome::Unused
        } else if self.rect.contains(x, y) {
            Outcome::Inside
        } else if self.anchor.contains(x, y) {
            Outcome::Anchor
        } else {
            Outcome::Outside
        }
    }

    /// A key: Escape is [`Outcome::Close`], anything else [`Outcome::Unused`], for the content.
    /// Tab is the content's too: the focus scope keeps it inside the card.
    pub fn key(&self, key: &KeyName) -> Outcome {
        match key {
            KeyName::Escape => Outcome::Close,
            _ => Outcome::Unused,
        }
    }
}

/// Opens the flyout's focus scope `scope` in `focus`. It does not move the focus first: the
/// widget that has it now, wherever it is (the anchor, or an editor a shortcut opened the
/// flyout from), or none, is the one [`close_scope`] gives it back to. At the next
/// [`end_frame`](Focus::end_frame) the focus moves to `first` if the content declares it, else
/// to the content's first widget, and Tab visits only the content's widgets. The rings stay as
/// they were: shown when a key opened the flyout, hidden after a click.
pub fn open_scope<K: Copy + Eq>(focus: &mut Focus<K>, scope: K, first: Option<K>) {
    match first {
        Some(id) => focus.push_scope_on(scope, id),
        None => focus.push_scope(scope),
    }
}

/// Closes the flyout's focus scope `scope` in `focus`, returning the focus to the widget that
/// had it when [`open_scope`] opened the scope (nothing, if nothing had it; if that widget is no
/// longer declared, wherever [`Focus::pop_scope`] sends it). It closes only
/// a scope that is the innermost one open: a scope the content opened inside the flyout (a menu
/// of its own) is closed by its owner first. Returns whether a scope was closed.
pub fn close_scope<K: Copy + Eq>(focus: &mut Focus<K>, scope: K) -> bool {
    focus.scope() == Some(scope) && focus.pop_scope()
}

/// Draws a flyout card at `r`: its shadow, its body and its one-pixel border, the way a menu
/// draws its own (the popup colours, the menu's corner radius and shadow). The content goes on
/// top, drawn by the caller. A rectangle with a side that is not a number, or of no size,
/// draws nothing.
pub fn draw_card(p: &mut Painter, pal: &Palette, r: Rect) {
    if !finite(r) || r.w <= 0.0 || r.h <= 0.0 {
        return;
    }
    p.shadow(r, RADIUS, pal.dark);
    p.frame(r, RADIUS, pal.popup, pal.popup_line);
}

/// Draws `flyout`'s card and its content as it opens or closes: `t` is how present it is, 0
/// gone and 1 there, from [`Animator::presence`](crate::anim::Animator::presence) or a
/// [`Transient`](crate::anim::Transient) that holds the flyout while it closes. The card with
/// what `content` draws on it (given the [`content_rect`](Flyout::content_rect)) fades as a
/// whole and moves [`MENU_DROP`] px into place from the anchor's side, as a menu does. At 1
/// (and for a NaN) this is [`draw_card`] then `content`, pixel for pixel; at 0 nothing is drawn
/// and `content` is not called. Under reduced motion the animator's presence is only 0 or 1,
/// so the card appears and goes at once. The flyout must have been placed
/// ([`Flyout::place_in`]). `content` is called at once, inside this call, as
/// [`anim::pop`](crate::anim::pop) calls its own; nothing keeps it.
pub fn draw_card_animated(
    p: &mut Painter,
    pal: &Palette,
    flyout: &Flyout,
    t: f32,
    content: impl FnOnce(&mut Painter, Rect),
) {
    let drop = if flyout.opened_above() {
        MENU_DROP
    } else {
        -MENU_DROP
    };
    let (rect, inner) = (flyout.rect(), flyout.content_rect());
    pop(p, rect.inset(-SHADOW), t, (0.0, drop), |p| {
        draw_card(p, pal, rect);
        content(p, inner);
    });
}

#[cfg(test)]
#[path = "flyout_tests.rs"]
mod tests;
