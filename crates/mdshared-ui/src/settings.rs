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
                Row::Text { id, value, .. } => match old_ids.iter().position(|o| *o == Some(*id)) {
                    Some(k) if old_fields[k].text() == value => std::mem::take(&mut old_fields[k]),
                    _ => {
                        let mut f = TextField::new();
                        f.set_text(value);
                        f
                    }
                },
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
            Row::Header(_) => {
                if i == 0 {
                    HEADER_FIRST_H
                } else {
                    HEADER_H
                }
            }
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
            if matches!(self.rows[i], Row::Text { .. })
                && self.stops_text[i] != self.fields[i].text()
            {
                self.stops[i] = p.caret_stops(Face::Ui, 13.0, self.fields[i].text());
                self.stops_text[i] = self.fields[i].text().to_string();
            }
        }

        let groups = self.header_rows().len();
        let want_w = if groups > 1 { 780.0 } else { 600.0 };
        let want_h = (BODY_TOP + self.content + 4.0 + DIALOG_FOOTER).min(MAX_CARD_H);
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
        self.nav = (groups > 1 && r.w >= NAV_MIN_CARD)
            .then(|| Rect::new(r.x + 12.0, top, NAV_W, (bottom - top).max(0.0)));
        let bx = match self.nav {
            Some(n) => n.right() + 8.0,
            None => r.x + 8.0,
        };
        self.body = Rect::new(bx, top, r.right() - 8.0 - bx, (bottom - top).max(0.0));
        self.laid = true;
        self.scroll = self.scroll.clamp(0.0, self.max_scroll()).round();
        self.card
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
    pub fn jump_to_category(&mut self, c: usize) {
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
        if let Some((c, s)) = self.pin
            && s == self.scroll
        {
            return c;
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

impl Rect {
    fn inflate_y(self, d: f32) -> Rect {
        Rect::new(self.x, self.y - d, self.w, self.h + 2.0 * d)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gfx::Gfx;
    use mdshared_text::Bitmap;

    const NONE: Mods = Mods {
        ctrl: false,
        shift: false,
        alt: false,
    };
    const SHIFT: Mods = Mods {
        ctrl: false,
        shift: true,
        alt: false,
    };

    const WRAP: u32 = 1;
    const THEME: u32 = 2;
    const SIZE: u32 = 3;
    const NAME: u32 = 4;
    const RESET: u32 = 5;
    const WIDTH: u32 = 6;

    fn rows() -> Vec<Row> {
        vec![
            Row::header("Editor"),
            Row::toggle(WRAP, "Word wrap", false).help("Wrap long lines"),
            Row::number(SIZE, "Font size", 14, 8, 32).unit("px"),
            Row::header("Appearance"),
            Row::choice(THEME, "Theme", &["System", "Light", "Dark"], 0),
            Row::choice(
                WIDTH,
                "Page width",
                &["Narrow", "Medium", "Wide", "Full", "Custom"],
                1,
            ),
            Row::text(NAME, "Name", "abc").placeholder("none"),
            Row::header("Other"),
            Row::action(RESET, "Reset to defaults", "Reset"),
        ]
    }

    struct T {
        f: Form,
        gfx: Gfx,
        bmp: Bitmap,
    }

    impl T {
        fn new(rows: Vec<Row>, w: f32, h: f32) -> T {
            let mut t = T {
                f: Form::new("Settings", rows),
                gfx: Gfx::new(),
                bmp: Bitmap::new(w as u32, h as u32),
            };
            t.lay(w, h);
            t
        }
        fn lay(&mut self, w: f32, h: f32) {
            let mut p = Painter::new(&mut self.bmp, &mut self.gfx, 1.0);
            self.f.layout(&mut p, Rect::new(0.0, 0.0, w, h));
        }
        fn row(&self, id: u32) -> usize {
            self.f.rows.iter().position(|r| r.id() == Some(id)).unwrap()
        }
        fn click(&mut self, x: f32, y: f32) -> Outcome {
            let a = self.f.press(x, y, NONE, 1);
            match self.f.release(x, y) {
                Outcome::None => a,
                o => o,
            }
        }
        fn click_rect(&mut self, r: Rect) -> Outcome {
            self.click(r.cx(), r.cy())
        }
        fn key(&mut self, k: KeyName) -> Outcome {
            self.f.key(&k, NONE, None)
        }
    }

    fn changed(id: u32, value: Value) -> Outcome {
        Outcome::Changed { id, value }
    }

    #[test]
    fn rows_are_34_or_40_px_and_stack() {
        let t = T::new(rows(), 1000.0, 800.0);
        let h = |id| t.f.rect_of_row(t.row(id)).h;
        assert_eq!(h(WRAP), 40.0, "with help");
        assert_eq!(h(SIZE), 34.0, "without");
        let (a, b) = (t.f.rect_of_row(t.row(WRAP)), t.f.rect_of_row(t.row(SIZE)));
        assert_eq!(a.bottom(), b.y);
        // Controls sit at the right edge of their row and inside the body.
        for r in &t.f.rows {
            if let Some(id) = r.id() {
                let c = t.f.control_rect(t.row(id));
                let row = t.f.rect_of_row(t.row(id));
                assert!(c.w > 0.0 && c.right() <= row.right() - 10.0, "{id}: {c:?}");
                assert!(c.y >= row.y && c.bottom() <= row.bottom(), "{id}");
            }
        }
    }

    #[test]
    fn the_card_fits_its_window_and_the_body_scrolls() {
        let mut t = T::new(rows(), 1000.0, 800.0);
        assert!(t.f.rect().h < 640.0);
        assert_eq!(t.f.max_scroll(), 0.0);
        assert!(!t.f.wheel(500.0, 400.0, 100.0));
        // A short window: the card shrinks and the rows scroll.
        t.lay(1000.0, 400.0);
        let card = t.f.rect();
        assert!(card.y >= 0.0 && card.bottom() <= 400.0);
        assert!(t.f.max_scroll() > 0.0);
        let first = t.f.rect_of_row(1).y;
        assert!(t.f.wheel(500.0, 200.0, 50.0));
        assert_eq!(t.f.scroll(), 50.0);
        assert_eq!(t.f.rect_of_row(1).y, first - 50.0);
        assert!(t.f.wheel(500.0, 200.0, 1e6));
        assert!(!t.f.wheel(500.0, 200.0, 1.0));
        let last = t.f.rect_of_row(t.f.rows.len() - 1);
        assert!(last.bottom() <= t.f.body_rect().bottom());
        assert!(t.f.wheel(500.0, 200.0, -1e6));
        assert_eq!(t.f.scroll(), 0.0);
    }

    #[test]
    fn rows_scrolled_out_of_the_body_are_not_hit() {
        let mut t = T::new(rows(), 1000.0, 400.0);
        t.f.wheel(500.0, 200.0, 1e6);
        let r = t.f.control_rect(t.row(WRAP));
        assert!(r.bottom() < t.f.body_rect().y);
        assert_eq!(t.f.target_at(r.cx(), r.cy()), None);
        // And a click there (on the title) changes nothing.
        assert_eq!(t.click_rect(r), Outcome::None);
        assert_eq!(t.f.value(WRAP), Some(Value::Bool(false)));
    }

    #[test]
    fn categories_appear_for_two_groups_in_a_wide_window() {
        let mut t = T::new(rows(), 1000.0, 600.0);
        assert!(t.f.has_nav());
        assert_eq!(t.f.categories(), ["Editor", "Appearance", "Other"]);
        t.lay(500.0, 600.0);
        assert!(!t.f.has_nav(), "too narrow");
        // One group: no list.
        let one = vec![Row::header("Only"), Row::toggle(1, "A", true)];
        assert!(!T::new(one, 1000.0, 600.0).f.has_nav());
        // None at all is fine too.
        let none = vec![Row::toggle(1, "A", true)];
        assert!(!T::new(none, 1000.0, 600.0).f.has_nav());
    }

    #[test]
    fn a_category_click_scrolls_to_its_header() {
        // Enough rows to scroll in a short window.
        let mut v = vec![Row::header("A")];
        v.extend((0..8).map(|i| Row::toggle(10 + i, format!("A{i}"), false)));
        v.push(Row::header("B"));
        v.extend((0..8).map(|i| Row::toggle(30 + i, format!("B{i}"), false)));
        v.push(Row::header("C"));
        v.extend((0..3).map(|i| Row::toggle(50 + i, format!("C{i}"), false)));
        let mut t = T::new(v, 1000.0, 420.0);
        assert!(t.f.has_nav() && t.f.max_scroll() > 0.0);
        assert_eq!(t.f.current_category(), 0);
        let r = t.f.category_rect(1);
        assert_eq!(t.click_rect(r), Outcome::Redraw);
        let header = t.f.rect_of_row(9);
        assert_eq!(
            header.y,
            t.f.body_rect().y + 4.0 - 4.0 + 0.0 + (header.y - t.f.body_rect().y)
        );
        assert!(
            (header.y - t.f.body_rect().y).abs() <= 5.0,
            "B is at the top: {header:?}"
        );
        assert_eq!(t.f.current_category(), 1);
        // The last category cannot reach the top, but still counts once chosen.
        let r = t.f.category_rect(2);
        t.click_rect(r);
        assert_eq!(t.f.current_category(), 2);
        assert_eq!(t.f.scroll(), t.f.max_scroll());
        // Scrolling by hand goes back to following the scroll.
        t.f.wheel(500.0, 200.0, -1e6);
        assert_eq!(t.f.current_category(), 0);
        t.f.wheel(500.0, 200.0, 300.0);
        assert!(t.f.current_category() >= 1);
    }

    #[test]
    fn a_toggle_changes_on_click_and_keys() {
        let mut t = T::new(rows(), 1000.0, 800.0);
        let c = t.f.control_rect(t.row(WRAP));
        assert_eq!(t.click_rect(c), changed(WRAP, Value::Bool(true)));
        assert_eq!(t.f.value(WRAP), Some(Value::Bool(true)));
        assert_eq!(t.key(KeyName::Space), changed(WRAP, Value::Bool(false)));
        assert_eq!(t.key(KeyName::Enter), changed(WRAP, Value::Bool(true)));
        assert_eq!(t.key(KeyName::Right), Outcome::None, "already on");
        assert_eq!(t.key(KeyName::Left), changed(WRAP, Value::Bool(false)));
        // A press that moves off the control before the release does nothing.
        t.f.press(c.cx(), c.cy(), NONE, 1);
        assert_eq!(t.f.release(c.x - 100.0, c.cy()), Outcome::None);
        assert_eq!(t.f.value(WRAP), Some(Value::Bool(false)));
        // The label is not the control.
        let row = t.f.rect_of_row(t.row(WRAP));
        assert_eq!(t.click(row.x + 20.0, row.cy()), Outcome::None);
    }

    #[test]
    fn a_segmented_choice_hit_tests_each_segment() {
        let mut t = T::new(rows(), 1000.0, 800.0);
        let i = t.row(THEME);
        assert!(t.f.is_segmented(i));
        assert!(!t.f.is_segmented(t.row(WIDTH)), "five options: a dropdown");
        let seg = t.f.segment_rect(i, 2);
        assert_eq!(t.click_rect(seg), changed(THEME, Value::Choice(2)));
        // The same one again changes nothing.
        assert_eq!(t.click_rect(seg), Outcome::Redraw);
        let seg = t.f.segment_rect(i, 0);
        assert_eq!(t.click_rect(seg), changed(THEME, Value::Choice(0)));
        // Keys: Right and Left stop at the ends, Space goes round.
        t.f.focus = Some(i);
        assert_eq!(t.key(KeyName::Left), Outcome::None);
        assert_eq!(t.key(KeyName::Right), changed(THEME, Value::Choice(1)));
        assert_eq!(t.key(KeyName::End), changed(THEME, Value::Choice(2)));
        assert_eq!(t.key(KeyName::Space), changed(THEME, Value::Choice(0)));
        assert_eq!(t.key(KeyName::Home), Outcome::None);
    }

    #[test]
    fn a_dropdown_opens_a_menu_and_chooses() {
        let mut t = T::new(rows(), 1000.0, 800.0);
        let i = t.row(WIDTH);
        let c = t.f.control_rect(i);
        assert_eq!(t.click_rect(c), Outcome::Redraw);
        let (_, menu) = t.f.menu.as_ref().expect("open");
        assert_eq!(menu.items.len(), 5);
        assert!(menu.items[1].checked && !menu.items[0].checked);
        assert_eq!(menu.hover, Some(1));
        // Place it as the draw would, then click its fourth item.
        {
            let mut p = Painter::new(&mut t.bmp, &mut t.gfx, 1.0);
            t.f.menu.as_mut().unwrap().1.place(&mut p, 1000.0, 800.0);
        }
        let r = t.f.menu.as_ref().unwrap().1.row_rect(3);
        assert_eq!(t.click_rect(r), changed(WIDTH, Value::Choice(3)));
        assert!(t.f.menu.is_none());
        // Keyboard: open, Down, Enter.
        t.f.focus = Some(i);
        assert_eq!(t.key(KeyName::Enter), Outcome::Redraw);
        assert!(t.f.menu.is_some());
        t.key(KeyName::Down);
        assert_eq!(t.key(KeyName::Enter), changed(WIDTH, Value::Choice(4)));
        // Escape closes only the menu; a second one closes the dialog.
        t.key(KeyName::Space);
        assert!(t.f.menu.is_some());
        assert_eq!(t.key(KeyName::Escape), Outcome::Redraw);
        assert!(t.f.menu.is_none());
        assert_eq!(t.key(KeyName::Escape), Outcome::Closed);
        // A click outside the menu closes just the menu.
        t.click_rect(c);
        assert!(t.f.menu.is_some());
        assert_eq!(t.f.press(5.0, 5.0, NONE, 1), Outcome::Redraw);
        assert!(t.f.menu.is_none());
        // Left and Right change the choice without opening it.
        t.f.focus = Some(i);
        assert_eq!(t.key(KeyName::Left), changed(WIDTH, Value::Choice(3)));
    }

    #[test]
    fn the_stepper_steps_and_clamps() {
        let mut t = T::new(rows(), 1000.0, 800.0);
        let i = t.row(SIZE);
        let (m, v, p) = t.f.stepper_parts(i);
        assert_eq!(t.click_rect(p), changed(SIZE, Value::Number(15)));
        assert_eq!(t.click_rect(m), changed(SIZE, Value::Number(14)));
        assert_eq!(
            t.click_rect(v),
            Outcome::Redraw,
            "the value is not a button"
        );
        t.f.focus = Some(i);
        assert_eq!(t.key(KeyName::End), changed(SIZE, Value::Number(32)));
        assert_eq!(
            t.click_rect(p),
            Outcome::Redraw,
            "at the maximum: no change"
        );
        assert_eq!(t.key(KeyName::Right), Outcome::None);
        assert_eq!(t.key(KeyName::Home), changed(SIZE, Value::Number(8)));
        assert_eq!(t.click_rect(m), Outcome::Redraw);
        let ctrl = Mods { ctrl: true, ..NONE };
        assert_eq!(
            t.f.key(&KeyName::Right, ctrl, None),
            changed(SIZE, Value::Number(18))
        );
        assert_eq!(t.key(KeyName::PageDown), changed(SIZE, Value::Number(8)));
        assert_eq!(t.key(KeyName::Char('+')), changed(SIZE, Value::Number(9)));
        assert_eq!(t.key(KeyName::Char('-')), changed(SIZE, Value::Number(8)));
        // A step other than 1.
        let mut u = T::new(
            vec![Row::number(1, "Width", 100, 0, 150).step(25)],
            1000.0,
            600.0,
        );
        u.f.focus = Some(0);
        assert_eq!(u.key(KeyName::Right), changed(1, Value::Number(125)));
        assert_eq!(u.key(KeyName::Right), changed(1, Value::Number(150)));
        // set_value clamps.
        assert!(u.f.set_value(1, Value::Number(1000)));
        assert_eq!(u.f.value(1), Some(Value::Number(150)));
        assert!(!u.f.set_value(1, Value::Bool(true)));
        assert!(!u.f.set_value(99, Value::Bool(true)));
        // And the constructor does.
        assert_eq!(
            Row::number(1, "x", 99, 0, 10).value(),
            Some(Value::Number(10))
        );
    }

    #[test]
    fn a_text_row_edits_and_reports_each_change() {
        let mut t = T::new(rows(), 1000.0, 800.0);
        let i = t.row(NAME);
        let c = t.f.control_rect(i);
        t.f.stops[i] = (0..=3).map(|k| k as f32 * 8.0).collect();
        t.f.stops_text[i] = "abc".into();
        assert_eq!(t.f.press(c.x + 200.0, c.cy(), NONE, 1), Outcome::Redraw);
        t.f.release(c.x + 200.0, c.cy());
        assert_eq!(t.f.focus, Some(i));
        assert_eq!(
            t.f.key(&KeyName::Char('d'), NONE, Some("d")),
            changed(NAME, Value::Text("abcd".into()))
        );
        assert_eq!(
            t.key(KeyName::Backspace),
            changed(NAME, Value::Text("abc".into()))
        );
        assert_eq!(t.f.value(NAME), Some(Value::Text("abc".into())));
        // Caret movement is not a change; Space and Enter belong to the box.
        assert_eq!(t.key(KeyName::Left), Outcome::Redraw);
        assert_eq!(
            t.f.key(&KeyName::Space, NONE, Some(" ")),
            changed(NAME, Value::Text("ab c".into()))
        );
        assert_eq!(t.key(KeyName::Enter), Outcome::Unused);
        // Escape closes; Tab and the arrows leave.
        assert_eq!(t.key(KeyName::Down), Outcome::Redraw);
        assert_eq!(t.f.focus_id(), Some(RESET));
        t.f.focus = Some(i);
        assert_eq!(t.key(KeyName::Escape), Outcome::Closed);
        // set_value puts text into the box too.
        assert!(t.f.set_value(NAME, Value::Text("new".into())));
        assert_eq!(t.f.fields[i].text(), "new");
    }

    #[test]
    fn an_action_button_reports_its_id() {
        let mut t = T::new(rows(), 1000.0, 800.0);
        let c = t.f.control_rect(t.row(RESET));
        assert_eq!(t.click_rect(c), Outcome::Action(RESET));
        t.f.focus = Some(t.row(RESET));
        assert_eq!(t.key(KeyName::Enter), Outcome::Action(RESET));
        assert_eq!(t.key(KeyName::Space), Outcome::Action(RESET));
    }

    #[test]
    fn close_button_escape_and_outside_clicks_close() {
        let mut t = T::new(rows(), 1000.0, 800.0);
        let c = t.f.close_rect();
        assert_eq!(t.click_rect(c), Outcome::Closed);
        assert_eq!(t.key(KeyName::Escape), Outcome::Closed);
        assert_eq!(t.f.press(2.0, 2.0, NONE, 1), Outcome::Closed);
        // A click on the card's blank part does not.
        let r = t.f.rect();
        assert_eq!(t.f.press(r.x + 5.0, r.y + 5.0, NONE, 1), Outcome::None);
        // The Close button is a focus stop: Enter on it closes.
        t.f.focus = Some(t.f.rows.len());
        assert_eq!(t.key(KeyName::Enter), Outcome::Closed);
    }

    #[test]
    fn tab_cycles_the_controls_then_close_and_back() {
        let mut t = T::new(rows(), 1000.0, 800.0);
        let order: Vec<u32> = [WRAP, SIZE, THEME, WIDTH, NAME, RESET].to_vec();
        // The focus starts on the first control but shows only once the keyboard is used.
        assert_eq!(t.f.focus_id(), Some(WRAP));
        assert!(!t.f.focus_visible);
        assert_eq!(t.key(KeyName::Tab), Outcome::Redraw);
        assert!(t.f.focus_visible);
        assert_eq!(t.f.focus_id(), Some(WRAP), "the first Tab shows the focus");
        for id in &order[1..] {
            t.key(KeyName::Tab);
            assert_eq!(t.f.focus_id(), Some(*id));
        }
        t.key(KeyName::Tab);
        assert!(t.f.focus_on_close());
        t.key(KeyName::Tab);
        assert_eq!(t.f.focus_id(), Some(WRAP), "wraps");
        // Shift+Tab goes back, wrapping to the Close button.
        t.f.key(&KeyName::Tab, SHIFT, None);
        assert!(t.f.focus_on_close());
        t.f.key(&KeyName::Tab, SHIFT, None);
        assert_eq!(t.f.focus_id(), Some(RESET));
        // A click takes the focus and hides the ring.
        let c = t.f.control_rect(t.row(SIZE));
        t.click_rect(c);
        assert_eq!(t.f.focus_id(), Some(SIZE));
        assert!(!t.f.focus_visible);
    }

    #[test]
    fn the_focus_scrolls_into_view() {
        let mut t = T::new(rows(), 1000.0, 330.0);
        assert!(t.f.max_scroll() > 0.0);
        t.key(KeyName::Tab);
        for _ in 0..6 {
            t.key(KeyName::Tab);
            if let Some(f) = t.f.focus.filter(|&f| f < t.f.rows.len()) {
                let r = t.f.rect_of_row(f);
                let b = t.f.body_rect();
                assert!(
                    r.y >= b.y && r.bottom() <= b.bottom(),
                    "row {f}: {r:?} in {b:?}"
                );
            }
        }
        // Back to the first control: the first header is in view again.
        t.f.focus = Some(t.f.rows.len());
        t.key(KeyName::Tab);
        assert_eq!(t.f.scroll(), 0.0);
    }

    #[test]
    fn the_scrollbar_drags() {
        let mut t = T::new(rows(), 1000.0, 330.0);
        let (_, thumb) = t.f.bar().unwrap();
        assert_eq!(t.f.press(thumb.cx(), thumb.cy(), NONE, 1), Outcome::Redraw);
        assert!(t.f.pointer_move(thumb.cx(), thumb.cy() + 30.0));
        assert!(t.f.scroll() > 0.0);
        t.f.release(0.0, 0.0);
        let s = t.f.scroll();
        t.f.pointer_move(thumb.cx(), thumb.cy() + 60.0);
        assert_eq!(t.f.scroll(), s);
    }

    #[test]
    fn set_rows_keeps_focus_and_scroll() {
        let mut t = T::new(rows(), 1000.0, 330.0);
        t.f.focus = Some(t.row(SIZE));
        t.f.wheel(500.0, 150.0, 40.0);
        let mut next = rows();
        if let Row::Toggle { value, .. } = &mut next[1] {
            *value = true;
        }
        t.f.set_rows(next);
        t.lay(1000.0, 330.0);
        assert_eq!(t.f.focus_id(), Some(SIZE));
        assert_eq!(t.f.scroll(), 40.0);
        assert_eq!(t.f.value(WRAP), Some(Value::Bool(true)));
    }

    #[test]
    fn hover_tracks_controls() {
        let mut t = T::new(rows(), 1000.0, 800.0);
        let c = t.f.control_rect(t.row(WRAP));
        assert!(t.f.pointer_move(c.cx(), c.cy()));
        assert_eq!(t.f.hover, Some(Target::Toggle(t.row(WRAP))));
        assert!(!t.f.pointer_move(c.cx() + 1.0, c.cy()));
        let seg = t.f.segment_rect(t.row(THEME), 1);
        assert!(t.f.pointer_move(seg.cx(), seg.cy()));
        assert_eq!(t.f.hover, Some(Target::Segment(t.row(THEME), 1)));
    }

    #[test]
    fn draws_at_any_size_in_both_themes() {
        for (w, h, scale) in [(1000u32, 700u32, 1.0f32), (420, 300, 1.5), (80, 60, 1.0)] {
            let mut gfx = Gfx::new();
            let mut bmp = Bitmap::new(w, h);
            let mut p = Painter::new(&mut bmp, &mut gfx, scale);
            let win = Rect::new(0.0, 0.0, w as f32 / scale, h as f32 / scale);
            let mut f = Form::new("Settings", rows());
            f.key(&KeyName::Tab, NONE, None);
            f.draw(&mut p, &crate::theme::LIGHT, win, true);
            f.draw(&mut p, &crate::theme::DARK, win, false);
            let c = f.control_rect(f.rows.len() - 1);
            f.press(c.cx(), c.cy(), NONE, 1);
            f.draw(&mut p, &crate::theme::DARK, win, false);
        }
    }
}
