//! An InfoBar in WinUI's sense: a tinted card that says something at one of four severities,
//! with a semibold title, a message that wraps, up to two action buttons and a close button.
//! Several of them stack one under the other inside any rectangle, so they can sit in a page as
//! well as across the top of a window.
//!
//! The model ([`InfoBar`]) holds only what the bar says; it reads no clock and calls nothing.
//! The caller owns the events: it passes the pointer's position to [`InfoBar::hit`], and the
//! hit under the pointer to [`draw_infobar`] as its hover state, then acts on the hit itself.
//! A caller that also tracks a held press or the keyboard passes all three to
//! [`draw_infobar_with`] in a [`Look`]. To move the keyboard through a bar's buttons, and to
//! know where each one is, it asks [`InfoBar::parts`] for their rectangles, which come from the
//! same layout that drawing and hits use. The height depends on the width because the message
//! wraps, so it is asked for with [`InfoBar::height`] and the same painter that draws.
//!
//! Deliberately not here: the pointer or keyboard state (the caller keeps it, and decides the
//! order focus moves in), dismissing a bar or storing a list of bars (the caller owns both),
//! motion (there is none, so an idle window stays idle), and the accessibility tree. In the
//! later accessibility layer (W8) a bar is a group or a status whose name is its title and
//! message, and each action and the close button is a button; the model here carries
//! everything that tree needs.
//!
//! Also not here: telling a high-contrast palette by anything but identity. A [`Palette`] has
//! no field that says it is high contrast, so only [`crate::tokens::HIGH_CONTRAST_LIGHT`] and
//! [`crate::tokens::HIGH_CONTRAST_DARK`] themselves get the high-contrast colours. A palette
//! made from one of them, such as `Tokens::high_contrast(true).with_accent(c)`, is not equal to
//! it and is taken as light or dark from its `dark` flag. [`crate::tokens::Tokens::from_system`]
//! never makes such a palette: a high-contrast look keeps its own accent.
//!
//! The action buttons are drawn by [`crate::notice::bar_button`], the one the bars across the
//! top of the page use, and the focus ring by [`crate::focus::draw_ring`], as for the other
//! widgets. The per-severity colours are defined once, in [`severity_colors`], for light, dark
//! and the two high-contrast palettes.

use crate::focus::draw_ring;
use crate::gfx::{Face, Painter, Rect, Rgb, mix, rgb};
use crate::glyph;
use crate::notice::{bar_button, bar_button_w};
use crate::theme::Palette;
use crate::tokens::{HIGH_CONTRAST_DARK, HIGH_CONTRAST_LIGHT};

/// The most action buttons a bar carries; [`InfoBar::action`] ignores any past this.
pub const MAX_ACTIONS: usize = 2;
/// The padding inside a bar, at every side.
pub const PAD: f32 = 12.0;
/// The gap between the message and the row of action buttons, and between two of them.
pub const GAP: f32 = 8.0;
/// The gap [`stack_layout`] leaves between one bar and the next.
pub const STACK_GAP: f32 = 8.0;
/// The square the severity's glyph is drawn in.
pub const ICON: f32 = 16.0;
/// The close button's square, as in the bars of [`crate::notice`].
pub const CLOSE_SIZE: f32 = 24.0;
/// The height of an action button, as [`bar_button`] draws it.
pub const ACTION_H: f32 = 24.0;

/// The title's face size, and the line it gets.
const TITLE_TEXT: f32 = 13.0;
const TITLE_H: f32 = 18.0;
/// The gap between the title's line and the message below it.
const TITLE_GAP: f32 = 2.0;
/// The message's face size.
const MESSAGE_TEXT: f32 = 12.5;
/// The corners of the card.
const RADIUS: f32 = 6.0;
/// The corners of an action button (as [`bar_button`] draws it) and of the close button's
/// square.
const BUTTON_RADIUS: f32 = 5.0;
/// How far the close button's square leans from the card's fill towards the text colour, under
/// the pointer and under the pointer while pressed (the lifts [`bar_button`] uses).
const CLOSE_HOVER: f32 = 0.10;
const CLOSE_PRESS: f32 = 0.16;

/// How serious a bar's news is. Each severity has a glyph and a colour of its own; the glyph
/// carries the meaning where colour does not (a high-contrast scheme).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Severity {
    /// Neutral news.
    Info,
    /// Something worked.
    Success,
    /// Something needs attention but did not fail.
    Warning,
    /// Something failed.
    Error,
}

impl Severity {
    /// The glyph [`crate::glyph`] draws for this severity: a circle with an i, a tick, a
    /// triangle with an exclamation mark, or a cross.
    pub fn glyph(self) -> &'static str {
        match self {
            Severity::Info => glyph::INFO,
            Severity::Success => glyph::CHECK,
            Severity::Warning => glyph::WARN,
            Severity::Error => glyph::CLOSE,
        }
    }
}

/// One of a bar's action buttons: the caller's id, given back in [`Hit::Action`], and the label
/// drawn on the button.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Action {
    /// The caller's id, given back in [`Hit::Action`].
    pub id: u32,
    /// The text on the button.
    pub label: String,
}

impl Action {
    /// An action with `id` and `label`; the id is the caller's and comes back in a hit.
    pub fn new(id: u32, label: impl Into<String>) -> Action {
        Action {
            id,
            label: label.into(),
        }
    }
}

/// What a point of a bar belongs to.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Hit {
    /// The action with this id.
    Action(u32),
    /// The close button.
    Close,
}

/// Where a bar's controls are in the rectangle it was asked about, from [`InfoBar::parts`].
/// These are the rectangles [`InfoBar::hit`] answers for and [`draw_infobar`] draws in.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Parts {
    /// Each action's id and button, in the order they are drawn, left to right.
    pub actions: Vec<(u32, Rect)>,
    /// The close button's square, or `None` when the bar cannot be closed.
    pub close: Option<Rect>,
}

/// How [`draw_infobar_with`] draws a bar's controls just now, from the caller's own state. Each
/// field names the control it applies to, or `None` for none. A hit that is not on the bar (an
/// action id it does not have, or [`Hit::Close`] on a bar that cannot be closed) marks nothing.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Look {
    /// The control under the pointer: it is lightened, as [`draw_infobar`] draws it.
    pub hover: Option<Hit>,
    /// The control a held press started on. It is drawn pressed while the pointer is still over
    /// it (`hover` names the same control), and as only hovered or at rest otherwise, as
    /// [`bar_button`] draws a press.
    pub pressed: Option<Hit>,
    /// The control the keyboard is on: the accent ring of [`crate::focus::draw_ring`] is drawn
    /// just outside it.
    pub focused: Option<Hit>,
}

/// An InfoBar's data:its severity, a title, a message, up to [`MAX_ACTIONS`] actions and
/// whether it can be closed.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct InfoBar {
    severity: Severity,
    title: String,
    message: String,
    actions: Vec<Action>,
    closable: bool,
}

impl InfoBar {
    /// A closable bar of `severity` with `title` above `message`. Either text may be empty; a
    /// bar with no title starts its message at the top, and one with neither text nor actions is
    /// a card of the smallest height.
    pub fn new(
        severity: Severity,
        title: impl Into<String>,
        message: impl Into<String>,
    ) -> InfoBar {
        InfoBar {
            severity,
            title: title.into(),
            message: message.into(),
            actions: Vec::new(),
            closable: true,
        }
    }

    /// Adds an action button with `id` and `label`. A third and later call adds nothing: a bar
    /// has room for two.
    pub fn action(mut self, id: u32, label: impl Into<String>) -> InfoBar {
        if self.actions.len() < MAX_ACTIONS {
            self.actions.push(Action::new(id, label));
        }
        self
    }

    /// Sets whether the bar has a close button. It has one unless this says otherwise.
    pub fn closable(mut self, on: bool) -> InfoBar {
        self.closable = on;
        self
    }

    /// The bar's severity.
    pub fn severity(&self) -> Severity {
        self.severity
    }

    /// The title drawn above the message.
    pub fn title(&self) -> &str {
        &self.title
    }

    /// The message, wrapped to the width the bar is laid out in.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// The actions in the order they are drawn, at most [`MAX_ACTIONS`] of them.
    pub fn actions(&self) -> &[Action] {
        &self.actions
    }

    /// Whether the close button is drawn. Its absence also removes [`Hit::Close`].
    pub fn is_closable(&self) -> bool {
        self.closable
    }

    /// The height this bar wants in a rectangle `width` wide, in which its message wraps. It is
    /// never negative; a width that is not a finite number is taken as zero, which makes the
    /// message one word per line.
    pub fn height(&self, p: &mut Painter, width: f32) -> f32 {
        self.layout(p, Rect::new(0.0, 0.0, finite(width), 0.0))
            .height
    }

    /// What the point `(x, y)` of `rect` belongs to, or `None` when it belongs to no control.
    /// The close button is answered first when the two would overlap in a very small rectangle.
    pub fn hit(&self, p: &mut Painter, rect: Rect, x: f32, y: f32) -> Option<Hit> {
        if !rect.contains(x, y) {
            return None;
        }
        let l = self.layout(p, rect);
        if let Some(close) = l.close
            && close.contains(x, y)
        {
            return Some(Hit::Close);
        }
        l.actions
            .iter()
            .find(|(_, r)| r.contains(x, y))
            .map(|&(id, _)| Hit::Action(id))
    }

    /// The rectangle of each action button and of the close button when the bar is drawn in
    /// `rect`, so that a caller can move the keyboard through them or place something by them.
    /// It is the layout [`InfoBar::hit`] and [`draw_infobar`] use, so a point of `rect` in one
    /// of these rectangles hits that control. Where the close button and an action would
    /// overlap in a very small rectangle, the close button is the one hit. Nothing is kept.
    pub fn parts(&self, p: &mut Painter, rect: Rect) -> Parts {
        let l = self.layout(p, rect);
        Parts {
            actions: l.actions,
            close: l.close,
        }
    }

    fn layout(&self, p: &mut Painter, rect: Rect) -> Layout {
        let close = self.closable.then(|| {
            Rect::new(
                rect.right() - PAD - CLOSE_SIZE,
                rect.y + PAD,
                CLOSE_SIZE,
                CLOSE_SIZE,
            )
        });
        let reserved = if close.is_some() {
            CLOSE_SIZE + 4.0
        } else {
            0.0
        };
        let text_x = rect.x + PAD + ICON + GAP;
        let text_w = (rect.right() - PAD - reserved - text_x).max(0.0);
        let title_h = if self.title.is_empty() { 0.0 } else { TITLE_H };
        let title = Rect::new(text_x, rect.y + PAD, text_w, title_h);
        let message_top = rect.y + PAD + title_h + if title_h > 0.0 { TITLE_GAP } else { 0.0 };
        let message_h = p.paragraph_height(Face::Ui, MESSAGE_TEXT, text_w, &self.message);
        let message = Rect::new(text_x, message_top, text_w, message_h);
        let mut actions_top =
            message_top + message_h + if self.actions.is_empty() { 0.0 } else { GAP };
        if close.is_some() {
            // The actions are right-aligned under the close button; under a short title and
            // message they would reach up into it.
            actions_top = actions_top.max(rect.y + PAD + CLOSE_SIZE + GAP);
        }
        let widths: Vec<f32> = self
            .actions
            .iter()
            .map(|a| bar_button_w(p, &a.label))
            .collect();
        let total = widths.iter().sum::<f32>() + GAP * widths.len().saturating_sub(1) as f32;
        let mut x = (rect.right() - PAD - total).max(rect.x + PAD);
        let mut actions = Vec::with_capacity(self.actions.len());
        for (action, w) in self.actions.iter().zip(&widths) {
            actions.push((action.id, Rect::new(x, actions_top, *w, ACTION_H)));
            x += w + GAP;
        }
        // From the top edge: the padding, the title's line and the message, then the actions.
        let content = if self.actions.is_empty() {
            (message_top - rect.y) + message_h
        } else {
            (actions_top - rect.y) + ACTION_H
        };
        // A close button is the tallest thing a short bar may otherwise be too short for.
        let lowest = content.max(PAD + if self.closable { CLOSE_SIZE } else { 0.0 });
        Layout {
            height: lowest + PAD,
            icon: Rect::new(
                rect.x + PAD,
                rect.y + PAD + (TITLE_H - ICON) / 2.0,
                ICON,
                ICON,
            ),
            title,
            message,
            actions,
            close,
        }
    }
}

/// Everything [`draw_infobar`] places, computed once. The height is the bar's own, from its
/// top edge to its bottom edge; every other rectangle is in the frame it was laid out in.
struct Layout {
    height: f32,
    icon: Rect,
    title: Rect,
    message: Rect,
    actions: Vec<(u32, Rect)>,
    close: Option<Rect>,
}

/// `v` when it is a finite number, and zero otherwise.
fn finite(v: f32) -> f32 {
    if v.is_finite() { v.max(0.0) } else { 0.0 }
}

/// A severity's colours on one palette: `ink` for the glyph and the outline, `fill` for the
/// card behind, and `line` for the one-pixel frame.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SeverityColors {
    /// The glyph's colour, and the frame's on a high-contrast palette.
    pub ink: Rgb,
    /// The colour of the card behind the text.
    pub fill: Rgb,
    /// The colour of the one-pixel frame.
    pub line: Rgb,
}

/// The colours of `severity` on `pal`, in one place for light, dark and the two high-contrast
/// palettes. Info, warning and error take the palette's accent, warn and danger inks, so an
/// application's own accent reaches them. Success has no palette entry, and the four fills and
/// the frame are this module's own: they are light or dark by the palette's flag, and the high
/// contrast palettes (recognised by identity with [`HIGH_CONTRAST_LIGHT`] and
/// [`HIGH_CONTRAST_DARK`]) keep the page as the fill and the ink as the frame. A palette built
/// from one of those two, for example with another accent, is not recognised: see the module's
/// "Also not here".
pub fn severity_colors(severity: Severity, pal: &Palette) -> SeverityColors {
    use Severity::*;
    let high_contrast = *pal == HIGH_CONTRAST_LIGHT || *pal == HIGH_CONTRAST_DARK;
    let ink = match severity {
        Info => pal.accent,
        Warning => pal.warn,
        Error => pal.danger,
        Success if high_contrast && pal.dark => rgb(0x7ee787),
        Success if high_contrast => rgb(0x006400),
        Success if pal.dark => rgb(0x6ccb5f),
        Success => rgb(0x0f7b0f),
    };
    let fill = if high_contrast {
        pal.page
    } else if pal.dark {
        match severity {
            Info => rgb(0x1b2a33),
            Success => rgb(0x1d2b1d),
            Warning => rgb(0x332a1c),
            Error => rgb(0x34201e),
        }
    } else {
        match severity {
            Info => rgb(0xeef5fc),
            Success => rgb(0xeffaef),
            Warning => rgb(0xfff7e6),
            Error => rgb(0xfdf1f0),
        }
    };
    let line = if high_contrast {
        ink
    } else {
        mix(fill, ink, 0.45)
    };
    SeverityColors { ink, fill, line }
}

/// Draws `bar` in `rect`, which is usually one of [`stack_layout`]'s rectangles. `hover` is the
/// hit the pointer is over, from [`InfoBar::hit`], or `None`. Nothing is recorded: the caller
/// asks [`InfoBar::hit`] for its own hits.
///
/// The card is filled and framed in the severity's colours, the glyph is drawn at the left, the
/// title in the semibold face, the message wrapped below it and the actions right-aligned under
/// it; the first action is filled with the accent and the second is quiet. With `closable`, a
/// cross sits at the top right and takes the close hover. Everything is clipped to `rect`, so a
/// bar squeezed shorter than its content loses the bottom rather than drawing over what is
/// below it.
///
/// It is [`draw_infobar_with`] with only the hover set: no press and no focus ring.
pub fn draw_infobar(p: &mut Painter, pal: &Palette, rect: Rect, bar: &InfoBar, hover: Option<Hit>) {
    draw_infobar_with(
        p,
        pal,
        rect,
        bar,
        Look {
            hover,
            ..Look::default()
        },
    );
}

/// Draws `bar` in `rect` as [`draw_infobar`] does, with the hover, the press and the keyboard
/// focus of `look`, and changes nothing.
///
/// A pressed action is drawn as [`bar_button`] draws a press, and the pressed close button's
/// square leans further towards the text colour than its hover does; either shows only while
/// the pointer is still over that control. The focused control gets the accent ring of
/// [`crate::focus::draw_ring`] just outside its rectangle (the button, or the close button's
/// square), hovered or not. The ring lies in the card's padding, so the clip to `rect` cuts it
/// only when the bar is squeezed shorter than its content.
pub fn draw_infobar_with(p: &mut Painter, pal: &Palette, rect: Rect, bar: &InfoBar, look: Look) {
    let colors = severity_colors(bar.severity, pal);
    let l = bar.layout(p, rect);
    let clip = p.clip(rect);
    p.frame(rect, RADIUS, colors.fill, colors.line);
    p.icon(
        bar.severity.glyph(),
        ICON,
        l.icon.cx(),
        l.icon.cy(),
        colors.ink,
    );
    if !bar.title.is_empty() {
        p.text(
            Face::Semibold,
            TITLE_TEXT,
            l.title.x,
            l.title.cy(),
            &bar.title,
            pal.text,
            l.title.w,
        );
    }
    if !bar.message.is_empty() {
        p.paragraph(
            Face::Ui,
            MESSAGE_TEXT,
            l.message.x,
            l.message.y,
            l.message.w,
            &bar.message,
            pal.text,
        );
    }
    for (k, (action, (_, button))) in bar.actions.iter().zip(&l.actions).enumerate() {
        let id = Some(Hit::Action(action.id));
        if look.focused == id {
            draw_ring(p, pal, *button, BUTTON_RADIUS);
        }
        bar_button(
            p,
            pal,
            button.x,
            button.cy(),
            &action.label,
            k == 0,
            look.hover == id,
            look.pressed == id,
        );
    }
    if let Some(close) = l.close {
        if look.focused == Some(Hit::Close) {
            draw_ring(p, pal, close, BUTTON_RADIUS);
        }
        if look.hover == Some(Hit::Close) {
            let lift = if look.pressed == Some(Hit::Close) {
                CLOSE_PRESS
            } else {
                CLOSE_HOVER
            };
            p.rbox(close, BUTTON_RADIUS, mix(colors.fill, pal.text, lift));
        }
        p.icon(glyph::CLOSE, 10.0, close.cx(), close.cy(), pal.muted);
    }
    p.restore(clip);
}

/// Places `bars` one under the other, with [`STACK_GAP`] between them, starting at the top left
/// of `area`: the rectangle of each, in the order given. Every bar is as tall as
/// [`InfoBar::height`] says for the area's width; a bar that does not fit in what is left of
/// the area gets the height that is left, and the ones after it get none, so every rectangle
/// stays inside `area` and no two overlap. A width or height that is not a finite number is
/// taken as zero.
pub fn stack_layout(p: &mut Painter, area: Rect, bars: &[InfoBar]) -> Vec<Rect> {
    let x = if area.x.is_finite() { area.x } else { 0.0 };
    let top = if area.y.is_finite() { area.y } else { 0.0 };
    let w = finite(area.w);
    let bottom = top + finite(area.h);
    let mut y = top;
    let mut out = Vec::with_capacity(bars.len());
    for bar in bars {
        let remaining = (bottom - y).max(0.0);
        let h = bar.height(p, w).min(remaining);
        out.push(Rect::new(x, y, w, h));
        y = (y + h + STACK_GAP).min(bottom);
    }
    out
}

#[cfg(test)]
#[path = "infobar_tests.rs"]
mod tests;
