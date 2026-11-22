//! A frozen copy of the single-page form, [Form] and everything it draws with, exactly as
//! settings.rs stood on 2026-11-22 before the categorised form was added beside it. Tests only:
//! `the_single_page_form_still_draws_what_it_drew` draws both with the same rows and compares
//! every pixel, so a change to the old form that nobody opted into fails there. Delete it with the
//! single-page form, once no application draws that. Nothing in it is to be edited; the one change
//! from the original is the last item, inflate_y, a private trait here because the live file
//! already has it as an inherent method on the same type.

#![allow(dead_code, clippy::all)]

//! A settings dialog the applications fill with their own options: a card in the style of the
//! Keyboard shortcuts dialog, with rows described by data ([`Row`]) and a control at the right of
//! each. Settings apply live: every change comes back as [`Outcome::Changed`] at once and the
//! application applies and saves it; there is no OK or Cancel.
//!
//! Hosting: keep a [`Form`] in the UI state while the dialog is open (build it from the current
//! values each time it opens, or keep it and call [`Form::set_value`] when something else changed
//! a setting). Draw it last with [`Form::draw`] (a backdrop dims the window) and forward keys,
//! the pointer and the wheel to it instead of the page. The form updates its own rows when a
//! control changes, so the next frame is right even before the application has applied the value.
//!
//! All coordinates are logical px. No callbacks, no threads: every event returns an [`Outcome`].

use crate::gfx::{Face, Painter, Rect, mix};
use crate::glyph;
use crate::keys::{KeyName, Mods};
use crate::menu::{Item, Menu};
use crate::overlay::{
    DIALOG_FOOTER, DIALOG_PAD, dialog_button_w, dialog_card, draw_backdrop, draw_dialog_button,
    draw_dialog_card,
};
use crate::scroll;
use crate::textfield::{FieldColors, TextField};
use crate::theme::Palette as Colors;

/// The value of one control, as it comes back in [`Outcome::Changed`] and goes in
/// [`Form::set_value`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    Bool(bool),
    /// An index into the row's options.
    Choice(usize),
    Number(i32),
    Text(String),
}

/// One row of the form. `id` is the application's own number for the setting; `help` is one line
/// under the label, or empty.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Row {
    /// Starts a group; the groups become the category list when there is more than one.
    Header(String),
    Toggle {
        id: u32,
        label: String,
        help: String,
        value: bool,
    },
    /// A segmented control for up to four short options, otherwise a dropdown.
    Choice {
        id: u32,
        label: String,
        help: String,
        options: Vec<String>,
        value: usize,
    },
    /// A stepper: minus, the value with its unit, plus.
    Number {
        id: u32,
        label: String,
        help: String,
        value: i32,
        min: i32,
        max: i32,
        step: i32,
        unit: String,
    },
    Text {
        id: u32,
        label: String,
        help: String,
        value: String,
        placeholder: String,
        /// Drawn as dots and never copied: an API key.
        secret: bool,
    },
    /// A button; pressing it gives [`Outcome::Action`].
    Action {
        id: u32,
        label: String,
        button: String,
    },
}

impl Row {
    pub fn header(text: impl Into<String>) -> Row {
        Row::Header(text.into())
    }
    pub fn toggle(id: u32, label: impl Into<String>, value: bool) -> Row {
        Row::Toggle {
            id,
            label: label.into(),
            help: String::new(),
            value,
        }
    }
    pub fn choice(id: u32, label: impl Into<String>, options: &[&str], value: usize) -> Row {
        Row::Choice {
            id,
            label: label.into(),
            help: String::new(),
            options: options.iter().map(|s| s.to_string()).collect(),
            value: value.min(options.len().saturating_sub(1)),
        }
    }
    pub fn number(id: u32, label: impl Into<String>, value: i32, min: i32, max: i32) -> Row {
        Row::Number {
            id,
            label: label.into(),
            help: String::new(),
            value: value.clamp(min, max.max(min)),
            min,
            max: max.max(min),
            step: 1,
            unit: String::new(),
        }
    }
    pub fn text(id: u32, label: impl Into<String>, value: impl Into<String>) -> Row {
        Row::Text {
            id,
            label: label.into(),
            help: String::new(),
            value: value.into(),
            placeholder: String::new(),
            secret: false,
        }
    }
    pub fn action(id: u32, label: impl Into<String>, button: impl Into<String>) -> Row {
        Row::Action {
            id,
            label: label.into(),
            button: button.into(),
        }
    }
    /// The line under the label (not for a header).
    pub fn help(mut self, text: impl Into<String>) -> Row {
        match &mut self {
            Row::Toggle { help, .. }
            | Row::Choice { help, .. }
            | Row::Number { help, .. }
            | Row::Text { help, .. } => *help = text.into(),
            _ => {}
        }
        self
    }
    /// The step of a number row.
    pub fn step(mut self, by: i32) -> Row {
        if let Row::Number { step, .. } = &mut self {
            *step = by.max(1);
        }
        self
    }
    /// The unit shown after a number, "px" or "%".
    pub fn unit(mut self, text: impl Into<String>) -> Row {
        if let Row::Number { unit, .. } = &mut self {
            *unit = text.into();
        }
        self
    }
    pub fn placeholder(mut self, text: impl Into<String>) -> Row {
        if let Row::Text { placeholder, .. } = &mut self {
            *placeholder = text.into();
        }
        self
    }
    /// A text row whose value is a secret (an API key): drawn as dots, never copied or cut.
    pub fn secret(mut self) -> Row {
        if let Row::Text { secret, .. } = &mut self {
            *secret = true;
        }
        self
    }

    /// The application's id for the row; headers have none.
    pub fn id(&self) -> Option<u32> {
        match self {
            Row::Header(_) => None,
            Row::Toggle { id, .. }
            | Row::Choice { id, .. }
            | Row::Number { id, .. }
            | Row::Text { id, .. }
            | Row::Action { id, .. } => Some(*id),
        }
    }

    fn label(&self) -> &str {
        match self {
            Row::Header(t) => t,
            Row::Toggle { label, .. }
            | Row::Choice { label, .. }
            | Row::Number { label, .. }
            | Row::Text { label, .. }
            | Row::Action { label, .. } => label,
        }
    }

    fn help_text(&self) -> &str {
        match self {
            Row::Toggle { help, .. }
            | Row::Choice { help, .. }
            | Row::Number { help, .. }
            | Row::Text { help, .. } => help,
            _ => "",
        }
    }

    fn value(&self) -> Option<Value> {
        Some(match self {
            Row::Toggle { value, .. } => Value::Bool(*value),
            Row::Choice { value, .. } => Value::Choice(*value),
            Row::Number { value, .. } => Value::Number(*value),
            Row::Text { value, .. } => Value::Text(value.clone()),
            _ => return None,
        })
    }

    fn focusable(&self) -> bool {
        !matches!(self, Row::Header(_))
    }
}

/// What an event did. Redraw on everything but `None` and `Unused`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Nothing changed.
    None,
    /// The view changed (focus, hover, scroll, an open dropdown); nothing for the application.
    Redraw,
    /// A setting changed: apply it and save it. The form's own row already shows it.
    Changed { id: u32, value: Value },
    /// An action button was pressed.
    Action(u32),
    /// Escape, the Close button, or a click outside the card.
    Closed,
    /// A key the form does not use.
    Unused,
}

// ---- geometry --------------------------------------------------------------------------------

const ROW_H: f32 = 34.0;
const ROW_HELP_H: f32 = 40.0;
const HEADER_H: f32 = 38.0;
const HEADER_FIRST_H: f32 = 30.0;
const CONTROL_H: f32 = 28.0;
const NAV_W: f32 = 176.0;
const NAV_ITEM: f32 = 34.0;
const BODY_TOP: f32 = DIALOG_PAD + 36.0;
const MAX_CARD_H: f32 = 640.0;
/// The single-page form's headings are tighter and its card may be a little taller, so a form of
/// a dozen rows needs no scrolling in an ordinary window.
const SINGLE_HEADER_H: f32 = 32.0;
const SINGLE_HEADER_FIRST_H: f32 = 28.0;
const SINGLE_MAX_CARD_H: f32 = 760.0;
/// The card gets a category list at least this wide.
const NAV_MIN_CARD: f32 = 700.0;
const TOGGLE_W: f32 = 40.0;
const TOGGLE_H: f32 = 20.0;
const STEP_W: f32 = 30.0;
const TEXT_W: f32 = 230.0;

#[derive(Clone, Copy, PartialEq, Debug)]
enum Target {
    /// The row's label area or any part of it that is not a control.
    Row(usize),
    Toggle(usize),
    Segment(usize, usize),
    Dropdown(usize),
    Minus(usize),
    Plus(usize),
    Text(usize),
    Button(usize),
    Category(usize),
    Close,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Drag {
    None,
    Bar(f32),
    Text(usize),
}

/// How a choice row is drawn.
#[derive(Clone, Debug, PartialEq)]
enum Look {
    None,
    /// The width of each segment.
    Segmented(Vec<f32>),
    Dropdown(f32),
    Stepper(f32),
    Button(f32),
}

pub struct Form {
    title: String,
    rows: Vec<Row>,
    fields: Vec<TextField>,
    /// A row index, or `rows.len()` for the Close button.
    focus: Option<usize>,
    /// The focus ring shows once the keyboard has been used.
    focus_visible: bool,
    scroll: f32,
    hover: Option<Target>,
    pressed: Option<Target>,
    drag: Drag,
    bar_hover: bool,
    /// An open dropdown: its row, and the menu of its options.
    menu: Option<(usize, Menu<usize>)>,
    menu_pressed: Option<usize>,
    /// The category the user jumped to, and the scroll that put it at the top.
    pin: Option<(usize, f32)>,
    /// A category asked for before the form was laid out: jumped to by the next layout.
    pending_jump: Option<usize>,
    /// One page: the groups are headings in the scrolling body, with no category list.
    single_page: bool,

    // The last layout.
    laid: bool,
    window: Rect,
    card: Rect,
    footer: Rect,
    close: Rect,
    nav: Option<Rect>,
    body: Rect,
    /// Each row's top and height, from the top of the body's content.
    offs: Vec<(f32, f32)>,
    content: f32,
    looks: Vec<Look>,
    stops: Vec<Vec<f32>>,
    stops_text: Vec<String>,
}

impl Form {
    pub fn new(title: impl Into<String>, rows: Vec<Row>) -> Form {
        let mut f = Form {
            title: title.into(),
            rows: Vec::new(),
            fields: Vec::new(),
            focus: None,
            focus_visible: false,
            scroll: 0.0,
            hover: None,
            pressed: None,
            drag: Drag::None,
            bar_hover: false,
            menu: None,
            menu_pressed: None,
            pin: None,
            pending_jump: None,
            single_page: false,
            laid: false,
            window: Rect::default(),
            card: Rect::default(),
            footer: Rect::default(),
            close: Rect::default(),
            nav: None,
            body: Rect::default(),
            offs: Vec::new(),
            content: 0.0,
            looks: Vec::new(),
            stops: Vec::new(),
            stops_text: Vec::new(),
        };
        f.set_rows(rows);
        f.focus = f.rows.iter().position(Row::focusable);
        f
    }

    /// Makes this a single page: the groups stay as headings (tighter ones) in the body, there is
    /// never a category list, and the card is 600 px wide however many groups there are. The
    /// default is the list, which [`new`](Self::new) keeps.
    pub fn single_page(mut self) -> Form {
        self.single_page = true;
        self.laid = false;
        self
    }

    // ---- data --------------------------------------------------------------------------------

    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    /// Replaces the rows (the values changed elsewhere, say after "Reset to defaults"), keeping
    /// the scroll and the focus where they are. Text boxes the user is editing keep their caret
    /// when their text is unchanged.
    pub fn set_rows(&mut self, rows: Vec<Row>) {
        let old_ids: Vec<Option<u32>> = self.rows.iter().map(Row::id).collect();
        let mut old_fields = std::mem::take(&mut self.fields);
        let focus_id = self.focus.and_then(|f| self.rows.get(f)).and_then(Row::id);
        let was_close = self.focus == Some(self.rows.len()) && !self.rows.is_empty();
        self.fields = rows
            .iter()
            .map(|r| match r {
                Row::Text {
                    id, value, secret, ..
                } => {
                    let mut f = match old_ids.iter().position(|o| *o == Some(*id)) {
                        Some(k) if old_fields[k].text() == value => {
                            std::mem::take(&mut old_fields[k])
                        }
                        _ => {
                            let mut f = TextField::new();
                            f.set_text(value);
                            f
                        }
                    };
                    f.set_secret(*secret);
                    f
                }
                _ => TextField::new(),
            })
            .collect();
        self.rows = rows;
        self.stops = vec![Vec::new(); self.rows.len()];
        self.stops_text = vec![String::new(); self.rows.len()];
        self.menu = None;
        self.focus = if was_close {
            Some(self.rows.len())
        } else {
            focus_id.and_then(|id| self.rows.iter().position(|r| r.id() == Some(id)))
        };
        self.pin = None;
        self.laid = false;
        // A jump still waiting may name a category the new rows do not have.
        let heads = self.header_rows().len();
        self.pending_jump = self.pending_jump.filter(|&c| c < heads);
    }

    /// The current value of the setting `id`.
    pub fn value(&self, id: u32) -> Option<Value> {
        self.rows
            .iter()
            .find(|r| r.id() == Some(id))
            .and_then(Row::value)
    }

    /// Sets the value of `id` from the application's side. A value of the wrong kind is refused;
    /// a number is clamped and a choice kept inside its options. Returns true if it was set.
    pub fn set_value(&mut self, id: u32, value: Value) -> bool {
        let Some(i) = self.rows.iter().position(|r| r.id() == Some(id)) else {
            return false;
        };
        match (&mut self.rows[i], value) {
            (Row::Toggle { value, .. }, Value::Bool(v)) => *value = v,
            (Row::Choice { value, options, .. }, Value::Choice(v)) if v < options.len() => {
                *value = v
            }
            (
                Row::Number {
                    value, min, max, ..
                },
                Value::Number(v),
            ) => *value = v.clamp(*min, *max),
            (Row::Text { value, .. }, Value::Text(v)) => {
                self.fields[i].set_text(&v);
                *value = self.fields[i].text().to_string();
            }
            _ => return false,
        }
        true
    }

    /// What an input method is composing, for the text box with the focus (nothing else takes
    /// it). Returns true when that changes what is drawn.
    pub fn set_preedit(&mut self, preedit: Option<crate::textfield::Preedit>) -> bool {
        match self.focus {
            Some(f) if matches!(self.rows.get(f), Some(Row::Text { .. })) => {
                self.fields[f].set_preedit(preedit)
            }
            _ => false,
        }
    }

    /// The id of the row the keyboard focus is on, or `None` on the Close button.
    pub fn focus_id(&self) -> Option<u32> {
        self.focus.and_then(|f| self.rows.get(f)).and_then(Row::id)
    }

    /// Puts the keyboard focus on the control of setting `id` (the focus ring shows once a key is
    /// pressed). Returns false when there is no such control.
    pub fn focus_row(&mut self, id: u32) -> bool {
        match self.rows.iter().position(|r| r.id() == Some(id)) {
            Some(i) => {
                self.focus = Some(i);
                self.reveal_focus();
                true
            }
            None => false,
        }
    }

    /// Whether the keyboard focus is on the Close button.
    pub fn focus_on_close(&self) -> bool {
        self.focus == Some(self.rows.len())
    }

    /// The names of the groups ([`Row::Header`]), in order.
    pub fn categories(&self) -> Vec<&str> {
        self.rows
            .iter()
            .filter_map(|r| match r {
                Row::Header(t) => Some(t.as_str()),
                _ => None,
            })
            .collect()
    }

    fn header_rows(&self) -> Vec<usize> {
        (0..self.rows.len())
            .filter(|&i| matches!(self.rows[i], Row::Header(_)))
            .collect()
    }

    /// Whether the category list is showing (more than one group and a wide enough window).
    pub fn has_nav(&self) -> bool {
        self.nav.is_some()
    }

    // ---- layout ------------------------------------------------------------------------------

    fn row_height(&self, i: usize) -> f32 {
        match &self.rows[i] {
            Row::Header(_) => match (self.single_page, i == 0) {
                (false, true) => HEADER_FIRST_H,
                (false, false) => HEADER_H,
                (true, true) => SINGLE_HEADER_FIRST_H,
                (true, false) => SINGLE_HEADER_H,
            },
            r if r.help_text().is_empty() => ROW_H,
            _ => ROW_HELP_H,
        }
    }

    /// Places the card in `window` (the whole window, logical px) and every row in the card, and
    /// returns the card. Called by [`draw`](Self::draw); events use the last placement.
    pub fn layout(&mut self, p: &mut Painter, window: Rect) -> Rect {
        self.window = window;
        // Rows and their controls' widths.
        self.offs.clear();
        let mut y = 4.0;
        for i in 0..self.rows.len() {
            let h = self.row_height(i);
            self.offs.push((y, h));
            y += h;
        }
        self.content = y + 8.0;
        self.looks = (0..self.rows.len()).map(|i| self.look(p, i)).collect();
        for i in 0..self.rows.len() {
            if matches!(self.rows[i], Row::Text { .. }) {
                let shown = self.fields[i].display();
                if self.stops_text[i] != shown {
                    self.stops[i] = p.caret_stops(Face::Ui, 13.0, &shown);
                    self.stops_text[i] = shown.into_owned();
                }
            }
        }

        let groups = self.header_rows().len();
        let want_w = if groups > 1 && !self.single_page {
            780.0
        } else {
            600.0
        };
        let max_h = if self.single_page {
            SINGLE_MAX_CARD_H
        } else {
            MAX_CARD_H
        };
        let want_h = (BODY_TOP + self.content + 4.0 + DIALOG_FOOTER).min(max_h);
        let c = dialog_card(want_w, want_h, window.w, window.h);
        self.card = Rect::new(window.x + c.x, window.y + c.y, c.w, c.h);
        let r = self.card;
        self.footer = Rect::new(
            r.x + 1.0,
            r.bottom() - DIALOG_FOOTER,
            r.w - 2.0,
            DIALOG_FOOTER - 1.0,
        );
        let bw = dialog_button_w(p, "Close");
        self.close = Rect::new(
            r.right() - DIALOG_PAD - bw,
            self.footer.y + (DIALOG_FOOTER - 32.0) / 2.0,
            bw,
            32.0,
        );
        let top = r.y + BODY_TOP;
        let bottom = self.footer.y - 4.0;
        self.nav = (groups > 1 && !self.single_page && r.w >= NAV_MIN_CARD)
            .then(|| Rect::new(r.x + 12.0, top, NAV_W, (bottom - top).max(0.0)));
        let bx = match self.nav {
            Some(n) => n.right() + 8.0,
            None => r.x + 8.0,
        };
        self.body = Rect::new(bx, top, r.right() - 8.0 - bx, (bottom - top).max(0.0));
        self.laid = true;
        self.scroll = self.scroll.clamp(0.0, self.max_scroll()).round();
        if let Some(c) = self.pending_jump.take() {
            self.jump_to_category(c);
        }
        self.card
    }

    /// The row geometry is that of the current rows: laid out since they were last set.
    fn geometry_ready(&self) -> bool {
        self.laid && self.offs.len() >= self.rows.len()
    }

    fn max_scroll(&self) -> f32 {
        (self.content - self.body.h).max(0.0)
    }

    fn look(&self, p: &mut Painter, i: usize) -> Look {
        match &self.rows[i] {
            Row::Choice { options, .. } => {
                let widths: Vec<f32> = options
                    .iter()
                    .map(|o| (p.measure(Face::Ui, 13.0, o) + 26.0).ceil().max(54.0))
                    .collect();
                let total: f32 = widths.iter().sum();
                if options.len() <= 4 && total <= 330.0 {
                    Look::Segmented(widths)
                } else {
                    let w = widths.iter().copied().fold(0.0, f32::max) + 26.0;
                    Look::Dropdown(w.clamp(130.0, 280.0))
                }
            }
            Row::Number {
                min,
                max,
                value,
                unit,
                ..
            } => {
                let widest = [min, max, value]
                    .iter()
                    .map(|v| p.measure(Face::Ui, 13.0, &number_text(**v, unit)))
                    .fold(0.0, f32::max);
                Look::Stepper((widest + 24.0).ceil().max(64.0))
            }
            Row::Action { button, .. } => {
                Look::Button((p.measure(Face::Ui, 13.0, button) + 32.0).ceil().max(80.0))
            }
            _ => Look::None,
        }
    }

    fn row_rect(&self, i: usize) -> Rect {
        let (y, h) = self.offs.get(i).copied().unwrap_or_default();
        Rect::new(
            self.body.x + 4.0,
            self.body.y + y - self.scroll,
            self.body.w - 8.0,
            h,
        )
    }

    /// Where row `i` was last placed, scrolling included.
    pub fn rect_of_row(&self, i: usize) -> Rect {
        self.row_rect(i)
    }

    /// The control of row `i`: its toggle, segmented control, dropdown, stepper, text box or
    /// button, at the right of the row. Empty for a header.
    pub fn control_rect(&self, i: usize) -> Rect {
        let r = self.row_rect(i);
        let Some(row) = self.rows.get(i) else {
            return Rect::default();
        };
        let right = r.right() - 14.0;
        let (w, h) = match (row, self.looks.get(i)) {
            (Row::Toggle { .. }, _) => (TOGGLE_W, TOGGLE_H),
            (Row::Choice { .. }, Some(Look::Segmented(ws))) => {
                (ws.iter().sum::<f32>() + 4.0, CONTROL_H)
            }
            (Row::Choice { .. }, Some(Look::Dropdown(w))) => (*w, CONTROL_H),
            (Row::Number { .. }, Some(Look::Stepper(vw))) => (STEP_W * 2.0 + vw + 8.0, CONTROL_H),
            (Row::Text { .. }, _) => (TEXT_W, CONTROL_H),
            (Row::Action { .. }, Some(Look::Button(w))) => (*w, CONTROL_H),
            _ => return Rect::default(),
        };
        Rect::new(
            (right - w).round(),
            (r.cy() - h / 2.0).round(),
            w.round(),
            h,
        )
    }

    /// The Close button.
    pub fn close_rect(&self) -> Rect {
        self.close
    }

    /// The card as last laid out.
    pub fn rect(&self) -> Rect {
        self.card
    }

    /// The scrolling part of the card.
    pub fn body_rect(&self) -> Rect {
        self.body
    }

    /// The category list item `c`, if the list is showing.
    pub fn category_rect(&self, c: usize) -> Rect {
        match self.nav {
            Some(n) => Rect::new(n.x, n.y + c as f32 * NAV_ITEM, n.w, NAV_ITEM),
            None => Rect::default(),
        }
    }

    /// How far the body is scrolled, in logical px.
    pub fn scroll(&self) -> f32 {
        self.scroll
    }

    /// The part of a stepper: its minus, its value, its plus.
    pub fn stepper_parts(&self, i: usize) -> (Rect, Rect, Rect) {
        let c = self.control_rect(i);
        (
            Rect::new(c.x, c.y, STEP_W, c.h),
            Rect::new(c.x + STEP_W + 4.0, c.y, c.w - 2.0 * STEP_W - 8.0, c.h),
            Rect::new(c.right() - STEP_W, c.y, STEP_W, c.h),
        )
    }

    /// Segment `k` of a segmented choice row.
    pub fn segment_rect(&self, i: usize, k: usize) -> Rect {
        let c = self.control_rect(i);
        match self.looks.get(i) {
            Some(Look::Segmented(ws)) if k < ws.len() => Rect::new(
                c.x + 2.0 + ws[..k].iter().sum::<f32>(),
                c.y + 2.0,
                ws[k],
                c.h - 4.0,
            ),
            _ => Rect::default(),
        }
    }

    /// Whether choice row `i` is drawn as a segmented control (otherwise a dropdown).
    pub fn is_segmented(&self, i: usize) -> bool {
        matches!(self.looks.get(i), Some(Look::Segmented(_)))
    }

    fn text_area(&self, i: usize) -> Rect {
        let c = self.control_rect(i);
        Rect::new(c.x + 10.0, c.y, c.w - 20.0, c.h)
    }

    fn bar(&self) -> Option<(Rect, Rect)> {
        scroll::scrollbar(self.body, self.content, self.body.h, self.scroll)
    }

    // ---- hit testing -------------------------------------------------------------------------

    fn target_at(&self, x: f32, y: f32) -> Option<Target> {
        if !self.laid || !self.card.contains(x, y) {
            return None;
        }
        if self.close.contains(x, y) {
            return Some(Target::Close);
        }
        if let Some(n) = self.nav
            && n.contains(x, y)
        {
            let c = ((y - n.y) / NAV_ITEM).floor() as usize;
            return (c < self.header_rows().len()).then_some(Target::Category(c));
        }
        if !self.body.contains(x, y) {
            return None;
        }
        for i in 0..self.rows.len() {
            let r = self.row_rect(i);
            if y < r.y || y >= r.bottom() {
                continue;
            }
            return Some(match &self.rows[i] {
                Row::Header(_) => Target::Row(i),
                Row::Toggle { .. } if self.control_rect(i).contains(x, y) => Target::Toggle(i),
                Row::Choice { .. } if self.control_rect(i).contains(x, y) => {
                    match self.looks.get(i) {
                        Some(Look::Segmented(ws)) => (0..ws.len())
                            .find(|&k| self.segment_rect(i, k).inflate_y(2.0).contains(x, y))
                            .map_or(Target::Row(i), |k| Target::Segment(i, k)),
                        _ => Target::Dropdown(i),
                    }
                }
                Row::Number { .. } if self.control_rect(i).contains(x, y) => {
                    let (m, _, p) = self.stepper_parts(i);
                    if m.contains(x, y) {
                        Target::Minus(i)
                    } else if p.contains(x, y) {
                        Target::Plus(i)
                    } else {
                        Target::Row(i)
                    }
                }
                Row::Text { .. } if self.control_rect(i).contains(x, y) => Target::Text(i),
                Row::Action { .. } if self.control_rect(i).contains(x, y) => Target::Button(i),
                _ => Target::Row(i),
            });
        }
        None
    }

    // ---- changes -----------------------------------------------------------------------------

    fn changed(&self, i: usize) -> Outcome {
        match (self.rows[i].id(), self.rows[i].value()) {
            (Some(id), Some(value)) => Outcome::Changed { id, value },
            _ => Outcome::None,
        }
    }

    fn toggle(&mut self, i: usize, to: Option<bool>) -> Outcome {
        if let Row::Toggle { value, .. } = &mut self.rows[i] {
            let next = to.unwrap_or(!*value);
            if next != *value {
                *value = next;
                return self.changed(i);
            }
        }
        Outcome::None
    }

    fn pick(&mut self, i: usize, k: usize) -> Outcome {
        if let Row::Choice { value, options, .. } = &mut self.rows[i]
            && k < options.len()
            && *value != k
        {
            *value = k;
            return self.changed(i);
        }
        Outcome::None
    }

    /// Moves a choice by `d`; segmented and dropdown rows stop at their ends, `wrap` goes round.
    fn pick_by(&mut self, i: usize, d: i32, wrap: bool) -> Outcome {
        let Row::Choice { value, options, .. } = &self.rows[i] else {
            return Outcome::None;
        };
        let n = options.len() as i32;
        let next = *value as i32 + d;
        let next = if wrap {
            next.rem_euclid(n.max(1))
        } else {
            next.clamp(0, n - 1)
        };
        self.pick(i, next as usize)
    }

    fn number_by(&mut self, i: usize, dir: i32, times: i32) -> Outcome {
        if let Row::Number {
            value,
            min,
            max,
            step,
            ..
        } = &mut self.rows[i]
        {
            let next = (*value + dir * *step * times).clamp(*min, *max);
            if next != *value {
                *value = next;
                return self.changed(i);
            }
        }
        Outcome::None
    }

    fn number_to(&mut self, i: usize, end: bool) -> Outcome {
        if let Row::Number {
            value, min, max, ..
        } = &mut self.rows[i]
        {
            let next = if end { *max } else { *min };
            if next != *value {
                *value = next;
                return self.changed(i);
            }
        }
        Outcome::None
    }

    fn open_menu(&mut self, i: usize) {
        let Row::Choice { options, value, .. } = &self.rows[i] else {
            return;
        };
        let items: Vec<Item<usize>> = options
            .iter()
            .enumerate()
            .map(|(k, o)| Item::new(k, o.clone()).checked(k == *value))
            .collect();
        let c = self.control_rect(i);
        let mut menu = Menu::new(items, c.right(), c.bottom() + 4.0, true);
        menu.hover = Some(*value);
        self.menu = Some((i, menu));
    }

    /// Activates the control the keyboard is on.
    fn activate(&mut self, i: usize) -> Outcome {
        if i == self.rows.len() {
            return Outcome::Closed;
        }
        match &self.rows[i] {
            Row::Toggle { .. } => self.toggle(i, None),
            Row::Choice { .. } if self.is_segmented(i) => self.pick_by(i, 1, true),
            Row::Choice { .. } => {
                self.open_menu(i);
                Outcome::Redraw
            }
            Row::Action { id, .. } => Outcome::Action(*id),
            _ => Outcome::None,
        }
    }

    // ---- focus -------------------------------------------------------------------------------

    fn stops_list(&self) -> Vec<usize> {
        let mut v: Vec<usize> = (0..self.rows.len())
            .filter(|&i| self.rows[i].focusable())
            .collect();
        v.push(self.rows.len());
        v
    }

    fn move_focus(&mut self, forward: bool) {
        let stops = self.stops_list();
        let at = self.focus.and_then(|f| stops.iter().position(|&s| s == f));
        let next = match (at, forward) {
            (None, true) => 0,
            (None, false) => stops.len() - 1,
            (Some(a), true) => (a + 1) % stops.len(),
            (Some(a), false) => (a + stops.len() - 1) % stops.len(),
        };
        self.focus = Some(stops[next]);
        self.reveal_focus();
    }

    fn reveal_focus(&mut self) {
        let Some(f) = self.focus.filter(|&f| f < self.rows.len()) else {
            return;
        };
        if !self.laid {
            return;
        }
        let (mut top, h) = self.offs[f];
        if f > 0 && matches!(self.rows[f - 1], Row::Header(_)) {
            top = self.offs[f - 1].0;
        }
        if f == self.rows.iter().position(Row::focusable).unwrap_or(0) {
            top = 0.0;
        }
        if top < self.scroll {
            self.scroll = top;
        } else if self.offs[f].0 + h + 4.0 > self.scroll + self.body.h {
            self.scroll = self.offs[f].0 + h + 4.0 - self.body.h;
        }
        self.scroll = self.scroll.clamp(0.0, self.max_scroll()).round();
    }

    // ---- keyboard ----------------------------------------------------------------------------

    /// A key press; `typed` is the text the event produced. Escape closes (it closes an open
    /// dropdown first). Tab and Shift+Tab (and Up and Down) move the focus over the controls and
    /// the Close button; Space and Enter toggle, open or press; Left and Right change a choice or
    /// a number (Ctrl takes bigger steps), Home and End go to the ends; a text box takes the usual
    /// editing keys.
    pub fn key(&mut self, key: &KeyName, mods: Mods, typed: Option<&str>) -> Outcome {
        if let Some((i, menu)) = &mut self.menu {
            let i = *i;
            return match key {
                KeyName::Escape | KeyName::Tab => {
                    self.menu = None;
                    Outcome::Redraw
                }
                KeyName::Up => {
                    menu.step(false);
                    Outcome::Redraw
                }
                KeyName::Down => {
                    menu.step(true);
                    Outcome::Redraw
                }
                KeyName::Enter | KeyName::Space => {
                    let chosen = menu.chosen();
                    self.menu = None;
                    match chosen {
                        Some(k) => self.pick(i, k).or_redraw(),
                        None => Outcome::Redraw,
                    }
                }
                _ => Outcome::None,
            };
        }
        let was_visible = std::mem::replace(&mut self.focus_visible, true);
        if *key == KeyName::Escape {
            return Outcome::Closed;
        }
        let Some(f) = self.focus else {
            return match key {
                KeyName::Tab | KeyName::Down | KeyName::Up => {
                    self.move_focus(!mods.shift && *key != KeyName::Up);
                    Outcome::Redraw
                }
                _ => Outcome::Unused,
            };
        };
        if *key == KeyName::Tab {
            // The first Tab shows where the focus already is.
            if was_visible {
                self.move_focus(!mods.shift);
            }
            return Outcome::Redraw;
        }
        if f < self.rows.len()
            && let Row::Text { id, .. } = self.rows[f]
        {
            // Up and Down leave the box; everything else is the box's.
            return match self.fields[f].handle_key(key, mods, typed, true) {
                Some(true) => {
                    let text = self.fields[f].text().to_string();
                    if let Row::Text { value, .. } = &mut self.rows[f] {
                        value.clone_from(&text);
                    }
                    Outcome::Changed {
                        id,
                        value: Value::Text(text),
                    }
                }
                Some(false) => Outcome::Redraw,
                None => match key {
                    KeyName::Up | KeyName::Down => {
                        self.move_focus(*key == KeyName::Down);
                        Outcome::Redraw
                    }
                    _ => Outcome::Unused,
                },
            };
        }
        let step = if mods.ctrl { 10 } else { 1 };
        match key {
            KeyName::Up => {
                self.move_focus(false);
                Outcome::Redraw
            }
            KeyName::Down => {
                self.move_focus(true);
                Outcome::Redraw
            }
            KeyName::Enter | KeyName::Space => self.activate(f),
            _ if f >= self.rows.len() => Outcome::Unused,
            KeyName::Left | KeyName::Right => {
                let d = if *key == KeyName::Right { 1 } else { -1 };
                match &self.rows[f] {
                    Row::Toggle { .. } => self.toggle(f, Some(d > 0)),
                    Row::Choice { .. } => self.pick_by(f, d, false),
                    Row::Number { .. } => self.number_by(f, d, step),
                    _ => Outcome::Unused,
                }
            }
            KeyName::Char('+' | '=') => self.number_by(f, 1, step),
            KeyName::Char('-') => self.number_by(f, -1, step),
            KeyName::PageUp => self.number_by(f, 1, 10),
            KeyName::PageDown => self.number_by(f, -1, 10),
            KeyName::Home => match &self.rows[f] {
                Row::Number { .. } => self.number_to(f, false),
                Row::Choice { .. } => self.pick(f, 0),
                _ => Outcome::Unused,
            },
            KeyName::End => match &self.rows[f] {
                Row::Number { .. } => self.number_to(f, true),
                Row::Choice { options, .. } => {
                    let last = options.len().saturating_sub(1);
                    self.pick(f, last)
                }
                _ => Outcome::Unused,
            },
            _ => Outcome::Unused,
        }
    }

    // ---- pointer -----------------------------------------------------------------------------

    /// The pointer moved. Returns true when something needs redrawing (hover, a drag).
    pub fn pointer_move(&mut self, x: f32, y: f32) -> bool {
        if let Some((_, menu)) = &mut self.menu {
            return menu.pointer(x, y);
        }
        match self.drag {
            Drag::Bar(grab) => {
                if let Some((track, thumb)) = self.bar() {
                    let off = scroll::offset_for_thumb(
                        track,
                        thumb.h,
                        y - grab,
                        self.content,
                        self.body.h,
                    );
                    return self.set_scroll(off);
                }
                false
            }
            Drag::Text(i) => {
                let before = self.fields[i].caret();
                let rel = x - self.text_area(i).x;
                self.fields[i].drag(rel, &self.stops[i]);
                self.fields[i].caret() != before
            }
            Drag::None => {
                let hover = self.target_at(x, y);
                let on_bar = self.bar().is_some_and(|(t, _)| t.contains(x, y));
                let changed = hover != self.hover || on_bar != self.bar_hover;
                self.hover = hover;
                self.bar_hover = on_bar;
                changed
            }
        }
    }

    fn set_scroll(&mut self, to: f32) -> bool {
        let to = to.clamp(0.0, self.max_scroll()).round();
        let moved = to != self.scroll;
        self.scroll = to;
        if moved {
            self.pin = self.pin.filter(|p| p.1 == to);
        }
        moved
    }

    /// A mouse button went down; `clicks` is 1, or 2 for a double click. Controls act when the
    /// button comes up over the same control ([`release`](Self::release)); a text box places its
    /// caret at once; the scrollbar is grabbed; a press outside the card closes the dialog (and,
    /// with a dropdown open, only the dropdown).
    pub fn press(&mut self, x: f32, y: f32, mods: Mods, clicks: u8) -> Outcome {
        self.drag = Drag::None;
        self.pressed = None;
        self.menu_pressed = None;
        self.focus_visible = false;
        if let Some((_, menu)) = &mut self.menu {
            if menu.contains(x, y) {
                menu.pointer(x, y);
                self.menu_pressed = menu.hover;
            } else {
                self.menu = None;
            }
            return Outcome::Redraw;
        }
        if !self.card.contains(x, y) {
            return Outcome::Closed;
        }
        if let Some((track, thumb)) = self.bar()
            && track.contains(x, y)
        {
            let grab = if thumb.contains(x, y) {
                y - thumb.y
            } else {
                let off = scroll::offset_for_thumb(
                    track,
                    thumb.h,
                    y - thumb.h / 2.0,
                    self.content,
                    self.body.h,
                );
                self.set_scroll(off);
                thumb.h / 2.0
            };
            self.drag = Drag::Bar(grab);
            return Outcome::Redraw;
        }
        let target = self.target_at(x, y);
        self.pressed = target;
        match target {
            Some(Target::Text(i)) => {
                self.focus = Some(i);
                let rel = x - self.text_area(i).x;
                if clicks >= 2 {
                    self.fields[i].select_word_at(rel, &self.stops[i]);
                } else {
                    self.fields[i].press(rel, &self.stops[i], mods.shift);
                    self.drag = Drag::Text(i);
                }
                Outcome::Redraw
            }
            Some(
                Target::Toggle(i)
                | Target::Segment(i, _)
                | Target::Dropdown(i)
                | Target::Minus(i)
                | Target::Plus(i)
                | Target::Button(i),
            ) => {
                self.focus = Some(i);
                Outcome::Redraw
            }
            Some(Target::Close) => {
                self.focus = Some(self.rows.len());
                Outcome::Redraw
            }
            Some(Target::Category(_)) => Outcome::Redraw,
            Some(Target::Row(i)) if self.control_rect(i).contains(x, y) => {
                // The value of a stepper: not a button, but the row's control.
                self.focus = Some(i);
                Outcome::Redraw
            }
            _ => Outcome::None,
        }
    }

    /// The button went up: the control the press began on acts if the pointer is still on it.
    pub fn release(&mut self, x: f32, y: f32) -> Outcome {
        self.drag = Drag::None;
        if let Some((i, menu)) = &mut self.menu {
            let i = *i;
            let pressed = self.menu_pressed.take();
            menu.pointer(x, y);
            if pressed.is_some() && menu.hover == pressed {
                let k = menu.chosen();
                self.menu = None;
                if let Some(k) = k {
                    return self.pick(i, k).or_redraw();
                }
            }
            return Outcome::None;
        }
        let pressed = self.pressed.take();
        if pressed.is_none() || pressed != self.target_at(x, y) {
            return Outcome::None;
        }
        match pressed.unwrap() {
            Target::Toggle(i) => self.toggle(i, None),
            Target::Segment(i, k) => self.pick(i, k),
            Target::Dropdown(i) => {
                self.open_menu(i);
                Outcome::Redraw
            }
            Target::Minus(i) => self.number_by(i, -1, 1),
            Target::Plus(i) => self.number_by(i, 1, 1),
            Target::Button(i) => match self.rows[i].id() {
                Some(id) => Outcome::Action(id),
                None => Outcome::None,
            },
            Target::Close => Outcome::Closed,
            Target::Category(c) => {
                self.jump_to_category(c);
                Outcome::Redraw
            }
            Target::Text(_) | Target::Row(_) => Outcome::None,
        }
    }

    /// The wheel over `(x, y)`: `dy` logical px, positive towards the end. Scrolls the body (an
    /// open dropdown's menu when the pointer is over it). Returns true when something moved.
    pub fn wheel(&mut self, x: f32, y: f32, dy: f32) -> bool {
        if let Some((_, menu)) = &mut self.menu {
            return menu.contains(x, y) && menu.scroll_by(dy);
        }
        self.set_scroll(self.scroll + dy)
    }

    /// Scrolls so that group `c` starts at the top of the body (as far as the end of the content
    /// allows) and marks it as the current category.
    /// Before the form is laid out the jump waits for the layout.
    pub fn jump_to_category(&mut self, c: usize) {
        // A category that does not exist is no jump, now or later.
        if c >= self.header_rows().len() {
            return;
        }
        if !self.geometry_ready() {
            self.pending_jump = Some(c);
            self.pin = None;
            return;
        }
        let heads = self.header_rows();
        let Some(&h) = heads.get(c) else {
            return;
        };
        let to = (self.offs[h].0 - 4.0).clamp(0.0, self.max_scroll()).round();
        self.scroll = to;
        self.pin = Some((c, to));
    }

    /// The category the body shows at its top.
    pub fn current_category(&self) -> usize {
        if let Some(c) = self.pending_jump.filter(|&c| c < self.header_rows().len()) {
            return c;
        }
        if let Some((c, s)) = self.pin
            && s == self.scroll
        {
            return c;
        }
        if !self.geometry_ready() {
            return 0;
        }
        let heads = self.header_rows();
        let mut cur = 0;
        for (c, &h) in heads.iter().enumerate() {
            if self.offs[h].0 <= self.scroll + 8.0 {
                cur = c;
            }
        }
        cur
    }

    // ---- drawing -----------------------------------------------------------------------------

    /// Lays the dialog out in `window` and draws it over a dimmed window. `caret_on` is the blink
    /// phase of a focused text box.
    pub fn draw(&mut self, p: &mut Painter, pal: &Colors, window: Rect, caret_on: bool) {
        self.layout(p, window);
        draw_backdrop(p, pal, window.w, window.h);
        let r = self.card;
        draw_dialog_card(p, pal, r);
        p.text(
            Face::Semibold,
            17.0,
            r.x + DIALOG_PAD,
            r.y + DIALOG_PAD + 10.0,
            &self.title,
            pal.text,
            r.w - 2.0 * DIALOG_PAD,
        );

        // The category list.
        if let Some(n) = self.nav {
            let cur = self.current_category();
            for (c, name) in self.categories().iter().enumerate() {
                let it = self.category_rect(c);
                let hovered = self.hover == Some(Target::Category(c));
                if c == cur {
                    p.rbox(it.inset(1.0), 6.0, pal.on);
                    p.rbox(
                        Rect::new(it.x + 3.0, it.cy() - 8.0, 3.0, 16.0),
                        1.5,
                        pal.accent,
                    );
                } else if hovered {
                    p.rbox(it.inset(1.0), 6.0, pal.hover);
                }
                p.text(
                    Face::Ui,
                    13.0,
                    it.x + 16.0,
                    it.cy(),
                    name,
                    if c == cur { pal.text } else { pal.muted },
                    it.w - 28.0,
                );
            }
            p.fill(
                Rect::new(n.right() + 3.5, n.y, p.px().max(1.0), n.h),
                pal.popup_line,
            );
        }

        // The rows.
        let old = p.clip(self.body);
        for i in 0..self.rows.len() {
            let row = self.row_rect(i);
            if row.bottom() < self.body.y || row.y > self.body.bottom() {
                continue;
            }
            self.draw_row(p, pal, i, row, caret_on);
        }
        p.restore(old);
        if let Some((track, thumb)) = self.bar() {
            scroll::draw_scrollbar(
                p,
                pal,
                track,
                thumb,
                self.bar_hover || matches!(self.drag, Drag::Bar(_)),
            );
        }

        // The footer's Close button, primary as in the other dialogs.
        let close_focus = self.focus_visible && self.focus == Some(self.rows.len());
        if close_focus {
            ring(p, pal, self.close, 5.0, footer_bg(pal));
        }
        draw_dialog_button(
            p,
            pal,
            self.close,
            "Close",
            true,
            self.hover == Some(Target::Close),
            self.pressed == Some(Target::Close),
        );

        // An open dropdown, over everything.
        if let Some((_, menu)) = &mut self.menu {
            menu.draw_in(p, pal, window);
        }
    }

    fn draw_row(&mut self, p: &mut Painter, pal: &Colors, i: usize, row: Rect, caret_on: bool) {
        let row_bg = |hover: bool| {
            if hover {
                mix(pal.popup, pal.text, 0.035)
            } else {
                pal.popup
            }
        };
        if let Row::Header(text) = &self.rows[i] {
            p.text(
                Face::Semibold,
                13.0,
                row.x + 12.0,
                row.cy() + if i == 0 { 0.0 } else { 6.0 },
                text,
                pal.text,
                row.w - 24.0,
            );
            return;
        }
        let hovered = self.hover_in_row(i);
        if hovered {
            p.rbox(row, 6.0, row_bg(true));
        }
        // A hairline under the row, unless a header follows or it is the last.
        let last = i + 1 >= self.rows.len() || matches!(self.rows[i + 1], Row::Header(_));
        if !last && !hovered {
            let px = p.px().max(1.0);
            p.fill_a(
                Rect::new(row.x + 12.0, row.bottom() - px, row.w - 24.0, px),
                pal.popup_line,
                0.6,
            );
        }
        let ctrl = self.control_rect(i);
        let label_w = (ctrl.x - row.x - 12.0 - 16.0).max(40.0);
        let help = self.rows[i].help_text();
        let cy = row.cy() - if help.is_empty() { 0.0 } else { 7.5 };
        p.text(
            Face::Ui,
            13.5,
            row.x + 12.0,
            cy,
            self.rows[i].label(),
            pal.text,
            label_w,
        );
        if !help.is_empty() {
            p.text(
                Face::Ui,
                11.5,
                row.x + 12.0,
                row.cy() + 9.0,
                help,
                pal.muted,
                label_w,
            );
        }
        let focused = self.focus == Some(i);
        let ring_on = focused && self.focus_visible;
        let bg = row_bg(hovered);
        match &self.rows[i] {
            Row::Toggle { value, .. } => {
                if ring_on {
                    ring(p, pal, ctrl, TOGGLE_H / 2.0, bg);
                }
                let hov = self.hover == Some(Target::Toggle(i));
                draw_toggle(
                    p,
                    pal,
                    ctrl,
                    *value,
                    hov,
                    self.pressed == Some(Target::Toggle(i)),
                );
            }
            Row::Choice { options, value, .. } => match &self.looks[i] {
                Look::Segmented(_) => {
                    if ring_on {
                        ring(p, pal, ctrl, 6.0, bg);
                    }
                    p.frame(ctrl, 6.0, pal.field, pal.line);
                    for (k, o) in options.iter().enumerate() {
                        let s = self.segment_rect(i, k);
                        let on = k == *value;
                        let hov = self.hover == Some(Target::Segment(i, k));
                        if on {
                            p.rbox(s, 4.0, pal.accent);
                        } else if hov {
                            p.rbox(s, 4.0, pal.hover);
                        }
                        p.text_center(
                            if on { Face::Semibold } else { Face::Ui },
                            13.0,
                            s,
                            o,
                            if on { pal.on_accent } else { pal.text },
                        );
                    }
                }
                _ => {
                    if ring_on {
                        ring(p, pal, ctrl, 6.0, bg);
                    }
                    let hov = self.hover == Some(Target::Dropdown(i))
                        || matches!(&self.menu, Some((k, _)) if *k == i);
                    let fill = if hov {
                        mix(pal.field, pal.text, 0.05)
                    } else {
                        pal.field
                    };
                    p.frame(
                        ctrl,
                        6.0,
                        fill,
                        if hov {
                            mix(pal.line, pal.text, 0.18)
                        } else {
                            pal.line
                        },
                    );
                    let shown = options.get(*value).map_or("", String::as_str);
                    p.text(
                        Face::Ui,
                        13.0,
                        ctrl.x + 11.0,
                        ctrl.cy(),
                        shown,
                        pal.text,
                        ctrl.w - 36.0,
                    );
                    p.icon(glyph::DOWN, 10.0, ctrl.right() - 15.0, ctrl.cy(), pal.muted);
                }
            },
            Row::Number {
                value,
                min,
                max,
                unit,
                ..
            } => {
                if ring_on {
                    ring(p, pal, ctrl, 6.0, bg);
                }
                let (m, v, pl) = self.stepper_parts(i);
                for (rect, glyph, enabled, t) in [
                    (m, glyph::MINUS, value > min, Target::Minus(i)),
                    (pl, glyph::PLUS, value < max, Target::Plus(i)),
                ] {
                    let hov = enabled && self.hover == Some(t);
                    let press = hov && self.pressed == Some(t);
                    let fill = if press {
                        pal.press
                    } else if hov {
                        pal.hover
                    } else {
                        mix(pal.popup, pal.text, 0.04)
                    };
                    p.frame(rect, 6.0, fill, pal.popup_line);
                    p.icon(
                        glyph,
                        11.0,
                        rect.cx(),
                        rect.cy(),
                        if enabled { pal.text } else { pal.faint },
                    );
                }
                p.frame(v, 6.0, pal.field, pal.line);
                p.text_center(Face::Ui, 13.0, v, &number_text(*value, unit), pal.text);
            }
            Row::Text { placeholder, .. } => {
                let hov = self.hover == Some(Target::Text(i));
                let edge = if hov && !focused {
                    mix(pal.line, pal.text, 0.18)
                } else {
                    pal.line
                };
                p.frame(ctrl, 6.0, pal.field, edge);
                if focused {
                    // The text box of the find bar: an accent underline that follows the corners.
                    let old = p.clip(Rect::new(ctrl.x, ctrl.bottom() - 2.0, ctrl.w, 2.0));
                    p.rbox(ctrl, 6.0, pal.accent);
                    p.restore(old);
                }
                let colors = FieldColors {
                    text: pal.text,
                    placeholder: pal.faint,
                    selection: pal.selection,
                    caret: pal.text,
                };
                let area = self.text_area(i);
                self.fields[i].draw(
                    p,
                    area,
                    Face::Ui,
                    13.0,
                    &colors,
                    placeholder,
                    focused,
                    caret_on,
                );
            }
            Row::Action { button, .. } => {
                if ring_on {
                    ring(p, pal, ctrl, 6.0, bg);
                }
                let hov = self.hover == Some(Target::Button(i));
                draw_dialog_button(
                    p,
                    pal,
                    ctrl,
                    button,
                    false,
                    hov,
                    self.pressed == Some(Target::Button(i)),
                );
            }
            Row::Header(_) => {}
        }
    }

    fn hover_in_row(&self, i: usize) -> bool {
        match self.hover {
            Some(
                Target::Row(k)
                | Target::Toggle(k)
                | Target::Segment(k, _)
                | Target::Dropdown(k)
                | Target::Minus(k)
                | Target::Plus(k)
                | Target::Text(k)
                | Target::Button(k),
            ) => k == i,
            _ => false,
        }
    }
}

fn number_text(v: i32, unit: &str) -> String {
    if unit.is_empty() {
        v.to_string()
    } else {
        format!("{v} {unit}")
    }
}

fn footer_bg(pal: &Colors) -> crate::gfx::Rgb {
    mix(
        pal.popup,
        if pal.dark { [0, 0, 0] } else { pal.bar },
        if pal.dark { 0.22 } else { 0.7 },
    )
}

/// The focus ring: an accent outline two px outside `r`, with `bg` between it and the control
/// (draw the control after it).
fn ring(p: &mut Painter, pal: &Colors, r: Rect, radius: f32, bg: crate::gfx::Rgb) {
    p.rbox(r.inset(-3.0), radius + 3.0, pal.accent);
    p.rbox(r.inset(-1.5), radius + 1.5, bg);
}

/// The Windows 11 switch: a pill, with the knob at the end that says on.
fn draw_toggle(p: &mut Painter, pal: &Colors, r: Rect, on: bool, hovered: bool, pressed: bool) {
    let knob = if pressed {
        14.0
    } else if hovered {
        13.0
    } else {
        12.0
    };
    if on {
        let fill = if hovered {
            mix(pal.accent, pal.popup, 0.1)
        } else {
            pal.accent
        };
        p.rbox(r, r.h / 2.0, fill);
        p.rbox(
            Rect::new(r.right() - 4.0 - knob, r.cy() - knob / 2.0, knob, knob),
            knob / 2.0,
            pal.on_accent,
        );
    } else {
        let edge = if hovered {
            mix(pal.muted, pal.popup, 0.2)
        } else {
            mix(pal.muted, pal.popup, 0.35)
        };
        p.frame(
            r,
            r.h / 2.0,
            if hovered {
                mix(pal.field, pal.text, 0.05)
            } else {
                pal.field
            },
            edge,
        );
        p.rbox(
            Rect::new(r.x + 4.0, r.cy() - knob / 2.0, knob, knob),
            knob / 2.0,
            pal.muted,
        );
    }
}

impl Outcome {
    /// A change counts as one; no change still wants the dropdown redrawn closed.
    fn or_redraw(self) -> Outcome {
        match self {
            Outcome::None => Outcome::Redraw,
            o => o,
        }
    }
}

trait InflateY {
    fn inflate_y(self, d: f32) -> Rect;
}

impl InflateY for Rect {
    fn inflate_y(self, d: f32) -> Rect {
        Rect::new(self.x, self.y - d, self.w, self.h + 2.0 * d)
    }
}
