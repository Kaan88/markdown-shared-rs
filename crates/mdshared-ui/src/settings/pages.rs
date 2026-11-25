//! The settings dialog as pages: a category list at the left, one page at a time at the right,
//! and nothing that scrolls. It is built beside the single-page [`Form`](super::Form), which
//! draws and answers exactly as it did; an application moves to this one when it chooses to.
//!
//! The shape (docs/settings-redesign.md in mdedit, after the symmetry pass of 2026-11-21): a
//! card of at most 780 x 420, 6 px in from the window; a 40 px header with the title and a ×
//! button, and no footer; the list, whose current item is the page shown; the page, whose rows
//! all end their controls at one right edge (R) and come in a few heights decided by the page.
//! A page that does not fit its area is not scrolled: [`Form::fit`] says so, and the
//! application's test fails, naming the page and the pixels over.
//!
//! The rows are the single-page form's [`Row`]s, each wrapped in an [`Item`] that adds what a
//! page needs: a child indented under its parent, a disabled row with its reason in the help
//! slot, a text field on its own line, a button beside the control. A [`Row::Header`] on a page
//! is a gap between two groups; its text is not drawn. The ids, [`Value`] and [`Outcome`] are
//! the single-page form's, so a host's code for what a change does stays as it is.
//!
//! Keyboard: the dialog opens with the keyboard in the list. Up, Down, Home, End, PageUp and
//! PageDown there show another page at once; a letter goes to the next page starting with it.
//! Tab, Right or Enter go to the page's first control; Tab walks the page's controls (a row's
//! button after its control), then the ×, then back to the list; Shift+Tab goes back. Ctrl+PageUp
//! and Ctrl+PageDown change page from anywhere and leave the keyboard in the list. Disabled rows
//! are skipped. Escape closes an open dropdown, then the dialog.

use super::motion::Fx;
use super::{Outcome, Row, Value, draw_toggle, number_text, ring};
use crate::gfx::{Face, Painter, Rect, mix};
use crate::glyph;
use crate::keys::{KeyName, Mods};
use crate::menu::{Item as MenuItem, Menu};
use crate::overlay::{draw_backdrop, draw_dialog_button, draw_dialog_button_disabled};
use crate::textfield::{FieldColors, TextField};
use crate::theme::Palette as Colors;

// ---- the numbers of the design ---------------------------------------------------------------

/// The card at its largest, and the margin it keeps from the window's edges.
const CARD_W: f32 = 780.0;
const CARD_H: f32 = 420.0;
const MARGIN: f32 = 6.0;
const RADIUS: f32 = 10.0;
/// The strip over both columns: the title at the left, the × at the right, a hairline under.
const HEADER_H: f32 = 40.0;
/// The one inset: the title, the list's text, the page's text and the controls' right edge.
const PAD: f32 = 24.0;
const CLOSE: f32 = 28.0;
const CLOSE_RIGHT: f32 = 12.0;
/// The list: the highlight's width (narrower in a card under 640 wide), the gap each side of it,
/// the item height (shorter in a card under 400 tall).
const NAV_W: f32 = 168.0;
const NAV_W_SMALL: f32 = 112.0;
const NAV_GAP: f32 = 8.0;
const NARROW_BELOW: f32 = 640.0;
const NAV_ITEM: f32 = 34.0;
const NAV_ITEM_SMALL: f32 = 28.0;
const SHORT_BELOW: f32 = 400.0;
const MARK_INSET: f32 = 4.0;
const MARK_W: f32 = 3.0;
const MARK_H: f32 = 16.0;
const NAV_TEXT: f32 = 16.0;
/// Air above the first and below the last row of a page.
const PAGE_TOP: f32 = 4.0;
const PAGE_BOTTOM: f32 = 4.0;
/// A plain row; a row whose help is beside its label; one whose help is under it (+ a line).
const ROW_H: f32 = 34.0;
const BESIDE_H: f32 = 40.0;
const UNDER_H: f32 = 45.0;
const HELP_LINE: f32 = 14.0;
/// Help under a row runs to at most this many lines; under a stacked field, one more.
const UNDER_LINES: usize = 2;
const STACK_LINES: usize = 3;
/// A text field on its own line: the label's line, the field, the help, the air under.
const STACK_LABEL_H: f32 = 24.0;
const STACK_GAP: f32 = 2.0;
const STACK_BOTTOM: f32 = 6.0;
const GAP_H: f32 = 8.0;
const CHILD_INDENT: f32 = 16.0;
/// The control column: segmented groups, dropdowns and inline text fields are this wide.
const COL_W: f32 = 240.0;
const COL_W_SMALL: f32 = 192.0;
const CONTROL_H: f32 = 28.0;
const TOGGLE_W: f32 = 40.0;
const TOGGLE_H: f32 = 20.0;
const STEPPER_W: f32 = 164.0;
const STEP_W: f32 = 28.0;
const BUTTON_W: f32 = 96.0;
const ICON_W: f32 = 28.0;
const BUTTON_GAP: f32 = 8.0;
const LABEL_GAP: f32 = 12.0;
const LABEL_SIZE: f32 = 13.5;
const HELP_SIZE: f32 = 11.5;
const CONTROL_SIZE: f32 = 13.0;

// ---- the data --------------------------------------------------------------------------------

/// A button on a row, at the right of its control: "Choose…", "Save", or an icon (remove,
/// fetch again). Pressing it gives [`Outcome::Action`] with its own `id`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Button {
    pub id: u32,
    /// What it says; for an icon button, what it does (not drawn).
    pub text: String,
    /// A [`glyph`](crate::glyph) drawn instead of the text, on a 28 px square button.
    pub icon: Option<&'static str>,
    pub enabled: bool,
}

/// One row of a page: a [`Row`] and how the page shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    row: Row,
    child: bool,
    disabled: Option<String>,
    stacked: bool,
    dropdown: bool,
    button: Option<Button>,
}

impl Item {
    pub fn new(row: Row) -> Item {
        Item {
            row,
            child: false,
            disabled: None,
            stacked: false,
            dropdown: false,
            button: None,
        }
    }
    /// An 8 px gap between two groups of a page.
    pub fn gap() -> Item {
        Item::new(Row::Header(String::new()))
    }
    /// A setting that matters only while the row above is on: its label is indented under that
    /// row's, with a thin guide; its control stays in the column.
    pub fn child(mut self) -> Item {
        self.child = true;
        self
    }
    /// Disabled, with the reason in the place of the help line ("Needs the switch above.").
    /// Drawn faint, skipped by Tab, deaf to the pointer.
    pub fn disabled(mut self, reason: impl Into<String>) -> Item {
        self.disabled = Some(reason.into());
        self
    }
    /// A text row whose field is on its own line under the label, the full width of the page
    /// (a path, an address, a key). Other rows are not stacked.
    pub fn stacked(mut self) -> Item {
        self.stacked = matches!(self.row, Row::Text { .. });
        self
    }
    /// A choice drawn as a dropdown whatever its option count, for a list that grows or
    /// shrinks with what is fetched (a model list), so the row keeps one shape. Other rows are
    /// not changed.
    pub fn dropdown(mut self) -> Item {
        self.dropdown = matches!(self.row, Row::Choice { .. });
        self
    }
    /// A text button at the right of the control.
    pub fn button(mut self, id: u32, text: impl Into<String>, enabled: bool) -> Item {
        self.button = Some(Button {
            id,
            text: text.into(),
            icon: None,
            enabled,
        });
        self
    }
    /// An icon button at the right of the control; `text` says what it does.
    pub fn icon_button(
        mut self,
        id: u32,
        icon: &'static str,
        text: impl Into<String>,
        enabled: bool,
    ) -> Item {
        self.button = Some(Button {
            id,
            text: text.into(),
            icon: Some(icon),
            enabled,
        });
        self
    }
    pub fn row(&self) -> &Row {
        &self.row
    }
    pub fn is_disabled(&self) -> bool {
        self.disabled.is_some()
    }
    fn is_gap(&self) -> bool {
        matches!(self.row, Row::Header(_))
    }
    fn button_live(&self) -> bool {
        self.button.as_ref().is_some_and(|b| b.enabled) && !self.is_disabled()
    }
    /// The help slot: the reason while disabled, else the row's help.
    fn shown_help(&self) -> &str {
        match &self.disabled {
            Some(why) => why,
            None => self.row.help_text(),
        }
    }
    fn kind(&self) -> Kind {
        match &self.row {
            Row::Header(_) => Kind::Gap,
            Row::Toggle { .. } => Kind::Toggle,
            // The count decides, never the text, so a page keeps its shape when an option is
            // reworded or a model offers one more level.
            Row::Choice { options, .. } if options.len() <= 3 && !self.dropdown => Kind::Segmented,
            Row::Choice { .. } => Kind::Dropdown,
            Row::Number { .. } => Kind::Stepper,
            Row::Text { .. } if self.stacked => Kind::Stacked,
            Row::Text { .. } => Kind::Text,
            Row::Action { .. } => Kind::Action,
        }
    }
}

impl From<Row> for Item {
    fn from(row: Row) -> Item {
        Item::new(row)
    }
}

/// One page: its name in the list and its rows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Page {
    pub name: String,
    pub items: Vec<Item>,
    help_under: bool,
}

impl Page {
    pub fn new(name: impl Into<String>, items: Vec<Item>) -> Page {
        Page {
            name: name.into(),
            items,
            help_under: false,
        }
    }
    /// Help goes under its row whatever it says now: for a page whose help is written at run
    /// time (what a server said), so a new message does not re-lay the page.
    pub fn help_under(mut self) -> Page {
        self.help_under = true;
        self
    }
}

/// How each page fits at one window size: [`Form::fit`].
#[derive(Clone, Debug, PartialEq)]
pub struct Fit {
    pub pages: Vec<PageFit>,
    /// The list's height and the room it has.
    pub list_needs: f32,
    pub list_room: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PageFit {
    pub name: String,
    /// The page's height, its air above and below included, and the page area's.
    pub content: f32,
    pub room: f32,
    /// What is cut or out of place on it: a label, a help line, a value, a button's text.
    pub problems: Vec<String>,
}

impl Fit {
    /// Everything that does not fit, one line each; empty when all of it does.
    pub fn problems(&self) -> Vec<String> {
        let mut out = Vec::new();
        if self.list_needs > self.list_room {
            out.push(format!(
                "the list needs {} of {} px, over by {}",
                self.list_needs,
                self.list_room,
                self.list_needs - self.list_room
            ));
        }
        for pg in &self.pages {
            if pg.content > pg.room {
                out.push(format!(
                    "{}: {} of {} px, over by {}",
                    pg.name,
                    pg.content,
                    pg.room,
                    pg.content - pg.room
                ));
            }
            out.extend(pg.problems.iter().map(|p| format!("{}: {p}", pg.name)));
        }
        out
    }
}

// ---- geometry --------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
enum Kind {
    #[default]
    Gap,
    Toggle,
    Segmented,
    Dropdown,
    Stepper,
    Text,
    Stacked,
    Action,
}

/// The card and its columns for one window.
#[derive(Clone, Copy, Debug, Default)]
struct Geo {
    card: Rect,
    close: Rect,
    /// The first list item's rectangle; the others follow at `item_h`.
    nav: Rect,
    item_h: f32,
    divider_x: f32,
    page: Rect,
    /// The page's text starts at `l`, its controls end at `r`.
    l: f32,
    r: f32,
    col: f32,
}

fn shape(window: Rect) -> Geo {
    let cw = (window.w - 2.0 * MARGIN).clamp(0.0, CARD_W).round();
    let ch = (window.h - 2.0 * MARGIN).clamp(0.0, CARD_H).round();
    let card = Rect::new(
        window.x + ((window.w - cw) / 2.0).round(),
        window.y + ((window.h - ch) / 2.0).round(),
        cw,
        ch,
    );
    let wide = cw >= NARROW_BELOW;
    let item_h = if ch < SHORT_BELOW {
        NAV_ITEM_SMALL
    } else {
        NAV_ITEM
    };
    let top = card.y + HEADER_H;
    // The first item is centred on the first plain row of the page.
    let nav = Rect::new(
        card.x + NAV_GAP,
        top + PAGE_TOP + (ROW_H - item_h) / 2.0,
        if wide { NAV_W } else { NAV_W_SMALL },
        item_h,
    );
    let divider_x = nav.right() + NAV_GAP;
    let px = divider_x + 1.0;
    let page = Rect::new(
        px,
        top,
        (card.right() - px).max(0.0),
        (card.bottom() - top).max(0.0),
    );
    Geo {
        card,
        close: Rect::new(
            card.right() - CLOSE_RIGHT - CLOSE,
            card.y + (HEADER_H - CLOSE) / 2.0,
            CLOSE,
            CLOSE,
        ),
        nav,
        item_h,
        divider_x,
        page,
        l: px + PAD,
        r: card.right() - PAD,
        col: if wide { COL_W } else { COL_W_SMALL },
    }
}

impl Geo {
    fn item(&self, c: usize) -> Rect {
        Rect::new(
            self.nav.x,
            self.nav.y + c as f32 * self.item_h,
            self.nav.w,
            self.item_h,
        )
    }
}

/// One row as laid out, every rectangle in window coordinates.
#[derive(Clone, Debug, Default)]
struct Placed {
    kind: Kind,
    y: f32,
    h: f32,
    label_x: f32,
    label_cy: f32,
    label_w: f32,
    ctrl: Rect,
    button: Option<Rect>,
    help: Vec<String>,
    help_x: f32,
    help_cy: f32,
    help_w: f32,
}

struct Laid {
    placed: Vec<Placed>,
    /// The page's height from the top of its area, air included.
    content: f32,
    problems: Vec<String>,
}

/// The words of `text` in lines of at most `w`; a word wider than `w` has a line of its own.
fn wrap(p: &mut Painter, text: &str, w: f32) -> Vec<String> {
    let mut lines = Vec::new();
    let mut cur = String::new();
    for word in text.split_whitespace() {
        if cur.is_empty() {
            cur.push_str(word);
            continue;
        }
        let next = format!("{cur} {word}");
        if p.measure(Face::Ui, HELP_SIZE, &next) <= w {
            cur = next;
        } else {
            lines.push(std::mem::replace(&mut cur, word.to_string()));
        }
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    lines
}

/// The control of a row and its button, the control vertically centred on `cy`.
fn control_rects(it: &Item, g: &Geo, cy: f32) -> (Rect, Option<Rect>) {
    let kind = it.kind();
    let h = if kind == Kind::Toggle {
        TOGGLE_H
    } else {
        CONTROL_H
    };
    let w = match kind {
        Kind::Toggle => TOGGLE_W,
        Kind::Stepper => STEPPER_W,
        Kind::Action => BUTTON_W,
        _ => g.col,
    };
    let button = it.button.as_ref().map(|b| {
        let bw = if b.icon.is_some() { ICON_W } else { BUTTON_W };
        Rect::new(g.r - bw, (cy - CONTROL_H / 2.0).round(), bw, CONTROL_H)
    });
    // A column-wide control gives up room for its button and keeps the column's left edge; a
    // fixed-width one moves left of it.
    let (x, w) = match (button, kind) {
        (Some(b), Kind::Segmented | Kind::Dropdown | Kind::Text) => {
            (g.r - g.col, g.col - b.w - BUTTON_GAP)
        }
        (Some(b), _) => (b.x - BUTTON_GAP - w, w),
        (None, _) => (g.r - w, w),
    };
    (
        Rect::new(x.round(), (cy - h / 2.0).round(), w.round(), h),
        button,
    )
}

/// Lays out `page` in `g`: every row's rectangles, the page's height, and what is cut.
fn place(p: &mut Painter, page: &Page, g: &Geo) -> Laid {
    // The page's rule first: help sits beside its label only when every help line of the page
    // fits there, counting what a row says while disabled, so switching a setting does not
    // re-lay the page.
    let mut under = page.help_under;
    for it in &page.items {
        if matches!(it.kind(), Kind::Gap | Kind::Stacked) {
            continue;
        }
        let indent = if it.child { CHILD_INDENT } else { 0.0 };
        let (ctrl, _) = control_rects(it, g, 0.0);
        let room = ctrl.x - (g.l + indent) - LABEL_GAP;
        for help in [it.row.help_text(), it.disabled.as_deref().unwrap_or("")] {
            if !help.is_empty() && p.measure(Face::Ui, HELP_SIZE, help) > room {
                under = true;
            }
        }
    }
    let mut problems = Vec::new();
    let mut placed = Vec::with_capacity(page.items.len());
    let mut y = g.page.y + PAGE_TOP;
    for it in &page.items {
        let kind = it.kind();
        if kind == Kind::Gap {
            placed.push(Placed {
                y,
                h: GAP_H,
                ..Placed::default()
            });
            y += GAP_H;
            continue;
        }
        let lx = g.l + if it.child { CHILD_INDENT } else { 0.0 };
        let label = it.row.label();
        let help_text = it.shown_help();
        let full = g.r - lx;
        let pl = if kind == Kind::Stacked {
            let help = wrap(p, help_text, full);
            if help.len() > STACK_LINES {
                problems.push(format!("\"{label}\": help is {} lines", help.len()));
            }
            let n = help.len().min(STACK_LINES) as f32;
            let h = STACK_LABEL_H
                + CONTROL_H
                + if n > 0.0 {
                    STACK_GAP + HELP_LINE * n
                } else {
                    0.0
                }
                + STACK_BOTTOM;
            let button = it.button.as_ref().map(|b| {
                let bw = if b.icon.is_some() { ICON_W } else { BUTTON_W };
                Rect::new(g.r - bw, y + STACK_LABEL_H, bw, CONTROL_H)
            });
            let fw = button.map_or(full, |b| b.x - BUTTON_GAP - lx);
            Placed {
                kind,
                y,
                h,
                label_x: lx,
                label_cy: y + STACK_LABEL_H / 2.0 - 1.0,
                label_w: full,
                ctrl: Rect::new(lx, y + STACK_LABEL_H, fw.round(), CONTROL_H),
                button,
                help,
                help_x: lx,
                help_cy: y + STACK_LABEL_H + CONTROL_H + STACK_GAP + HELP_LINE / 2.0,
                help_w: full,
            }
        } else if help_text.is_empty() {
            let (ctrl, button) = control_rects(it, g, y + ROW_H / 2.0);
            Placed {
                kind,
                y,
                h: ROW_H,
                label_x: lx,
                label_cy: y + ROW_H / 2.0,
                label_w: ctrl.x - lx - LABEL_GAP,
                ctrl,
                button,
                ..Placed::default()
            }
        } else if !under {
            let (ctrl, button) = control_rects(it, g, y + BESIDE_H / 2.0);
            let room = ctrl.x - lx - LABEL_GAP;
            Placed {
                kind,
                y,
                h: BESIDE_H,
                label_x: lx,
                label_cy: y + 13.5,
                label_w: room,
                ctrl,
                button,
                help: vec![help_text.to_string()],
                help_x: lx,
                help_cy: y + 29.5,
                help_w: room,
            }
        } else {
            let (ctrl, button) = control_rects(it, g, y + 16.0);
            let help = wrap(p, help_text, full);
            if help.len() > UNDER_LINES {
                problems.push(format!("\"{label}\": help is {} lines", help.len()));
            }
            let n = help.len().clamp(1, UNDER_LINES) as f32;
            Placed {
                kind,
                y,
                h: UNDER_H + HELP_LINE * (n - 1.0),
                label_x: lx,
                label_cy: y + 16.0,
                label_w: ctrl.x - lx - LABEL_GAP,
                ctrl,
                button,
                help,
                help_x: lx,
                help_cy: y + 36.0,
                help_w: full,
            }
        };
        check_row(p, it, &pl, g, &mut problems);
        y += pl.h;
        placed.push(pl);
    }
    Laid {
        placed,
        content: y + PAGE_BOTTOM - g.page.y,
        problems,
    }
}

/// What of one laid-out row would be cut or stand outside the page.
fn check_row(p: &mut Painter, it: &Item, pl: &Placed, g: &Geo, out: &mut Vec<String>) {
    let label = it.row.label();
    let need = p.measure(Face::Ui, LABEL_SIZE, label);
    if need > pl.label_w + 0.5 {
        out.push(format!(
            "\"{label}\": label needs {need:.0} of {:.0}",
            pl.label_w
        ));
    }
    for line in &pl.help {
        let w = p.measure(Face::Ui, HELP_SIZE, line);
        if w > pl.help_w + 0.5 {
            out.push(format!(
                "\"{label}\": help line needs {w:.0} of {:.0}",
                pl.help_w
            ));
        }
    }
    let c = pl.ctrl;
    let right = pl.button.map_or(c.right(), |b| b.right());
    if c.x < g.l - 0.5 || right > g.r + 0.5 || c.w <= 0.0 {
        out.push(format!("\"{label}\": control is outside the page"));
    }
    let mut fits = |text: &str, face: Face, room: f32, what: &str| {
        let w = p.measure(face, CONTROL_SIZE, text);
        if w > room + 0.5 {
            out.push(format!(
                "\"{label}\": {what} \"{text}\" needs {w:.0} of {room:.0}"
            ));
        }
    };
    match (&it.row, pl.kind) {
        (Row::Choice { options, .. }, Kind::Segmented) => {
            let sw = (c.w - 4.0) / options.len().max(1) as f32;
            for o in options {
                fits(o, Face::Semibold, sw - 8.0, "option");
            }
        }
        // A dropdown shows only the chosen option; the others are in its menu.
        (Row::Choice { options, value, .. }, _) => {
            if let Some(o) = options.get(*value) {
                fits(o, Face::Ui, c.w - 36.0, "option");
            }
        }
        (
            Row::Number {
                value,
                min,
                max,
                unit,
                ..
            },
            _,
        ) => {
            let inner = c.w - 2.0 * STEP_W - 8.0 - 8.0;
            for v in [*min, *max, *value] {
                fits(&number_text(v, unit), Face::Ui, inner, "value");
            }
        }
        (Row::Action { button, .. }, _) => fits(button, Face::Ui, c.w - 16.0, "button"),
        _ => {}
    }
    if let (Some(b), Some(r)) = (&it.button, pl.button)
        && b.icon.is_none()
    {
        fits(&b.text, Face::Ui, r.w - 16.0, "button");
    }
}

// ---- the form --------------------------------------------------------------------------------

/// Where the keyboard is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Focus {
    List,
    /// Item `i`'s control on the current page.
    Control(usize),
    /// Item `i`'s button.
    Button(usize),
    Close,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Target {
    Category(usize),
    Close,
    /// A row, away from its controls (or a disabled one).
    Row(usize),
    Toggle(usize),
    Segment(usize, usize),
    Dropdown(usize),
    Minus(usize),
    Plus(usize),
    Text(usize),
    /// An action row's button.
    Action(usize),
    /// A row's own button.
    Button(usize),
}

impl Target {
    fn row(self) -> Option<usize> {
        match self {
            Target::Row(i)
            | Target::Toggle(i)
            | Target::Segment(i, _)
            | Target::Dropdown(i)
            | Target::Minus(i)
            | Target::Plus(i)
            | Target::Text(i)
            | Target::Action(i)
            | Target::Button(i) => Some(i),
            Target::Category(_) | Target::Close => None,
        }
    }
}

/// The text box of one text row, and the caret stops of what it last showed.
#[derive(Default)]
struct Slot {
    field: TextField,
    shown: String,
    stops: Vec<f32>,
}

pub struct Form {
    title: String,
    pages: Vec<Page>,
    /// One per item of every page; only text rows use theirs.
    slots: Vec<Vec<Slot>>,
    current: usize,
    focus: Focus,
    /// The focus ring shows once the keyboard has been used.
    focus_visible: bool,
    hover: Option<Target>,
    pressed: Option<Target>,
    /// A text box whose selection the pointer is dragging.
    drag: Option<usize>,
    menu: Option<(usize, Menu<usize>)>,
    menu_pressed: Option<usize>,

    // The last layout.
    laid: bool,
    geo: Geo,
    /// The current page's rows, valid while `placed_page` is the current page.
    placed: Vec<Placed>,
    placed_page: Option<usize>,
    /// The system's contrast theme: `draw_animated` changes highlights at once (motion.rs).
    high_contrast: bool,
}

impl Form {
    pub fn new(title: impl Into<String>, pages: Vec<Page>) -> Form {
        let mut f = Form {
            title: title.into(),
            pages: Vec::new(),
            slots: Vec::new(),
            current: 0,
            focus: Focus::List,
            focus_visible: false,
            hover: None,
            pressed: None,
            drag: None,
            menu: None,
            menu_pressed: None,
            laid: false,
            geo: Geo::default(),
            placed: Vec::new(),
            placed_page: None,
            high_contrast: false,
        };
        f.set_pages(pages);
        f
    }

    // ---- data --------------------------------------------------------------------------------

    pub fn pages(&self) -> &[Page] {
        &self.pages
    }

    /// The names of the pages, in the list's order.
    pub fn page_names(&self) -> Vec<&str> {
        self.pages.iter().map(|p| p.name.as_str()).collect()
    }

    /// The page shown.
    pub fn page(&self) -> usize {
        self.current
    }

    /// The page that holds the setting or button `id`.
    pub fn page_of(&self, id: u32) -> Option<usize> {
        self.find(id).map(|(c, _)| c)
    }

    /// Replaces the pages (values or what is disabled changed elsewhere), keeping the page shown,
    /// the keyboard where it is, and the caret of a text box whose text is unchanged.
    pub fn set_pages(&mut self, pages: Vec<Page>) {
        let focus_id = self.focus_id();
        let mut old: Vec<(u32, TextField)> = Vec::new();
        for (c, page) in self.pages.iter().enumerate() {
            for (i, it) in page.items.iter().enumerate() {
                if let (Row::Text { id, .. }, Some(slot)) =
                    (&it.row, self.slots.get_mut(c).and_then(|s| s.get_mut(i)))
                {
                    old.push((*id, std::mem::take(&mut slot.field)));
                }
            }
        }
        self.slots = pages
            .iter()
            .map(|page| {
                page.items
                    .iter()
                    .map(|it| {
                        let mut slot = Slot::default();
                        if let Row::Text {
                            id, value, secret, ..
                        } = &it.row
                        {
                            match old.iter().position(|(o, f)| o == id && f.text() == value) {
                                Some(k) => slot.field = old.swap_remove(k).1,
                                None => slot.field.set_text(value),
                            }
                            slot.field.set_secret(*secret);
                        }
                        slot
                    })
                    .collect()
            })
            .collect();
        self.pages = pages;
        self.current = self.current.min(self.pages.len().saturating_sub(1));
        self.menu = None;
        self.hover = None;
        self.pressed = None;
        self.drag = None;
        self.placed_page = None;
        if matches!(self.focus, Focus::Control(_) | Focus::Button(_)) {
            self.focus = Focus::List;
            if let Some(id) = focus_id
                && let Some((c, f)) = self.find(id)
                && c == self.current
                && self.stops().contains(&f)
            {
                self.focus = f;
            }
        }
    }

    /// The page and focus stop of the setting or button `id`.
    fn find(&self, id: u32) -> Option<(usize, Focus)> {
        for (c, page) in self.pages.iter().enumerate() {
            for (i, it) in page.items.iter().enumerate() {
                if it.row.id() == Some(id) && !it.is_gap() {
                    return Some((c, Focus::Control(i)));
                }
                if it.button.as_ref().is_some_and(|b| b.id == id) {
                    return Some((c, Focus::Button(i)));
                }
            }
        }
        None
    }

    fn item_of(&self, id: u32) -> Option<(usize, usize)> {
        match self.find(id) {
            Some((c, Focus::Control(i))) => Some((c, i)),
            _ => None,
        }
    }

    /// The current value of the setting `id`.
    pub fn value(&self, id: u32) -> Option<Value> {
        let (c, i) = self.item_of(id)?;
        self.pages[c].items[i].row.value()
    }

    /// Sets the value of `id` from the application's side, as [`super::Form::set_value`] does.
    pub fn set_value(&mut self, id: u32, value: Value) -> bool {
        let Some((c, i)) = self.item_of(id) else {
            return false;
        };
        match (&mut self.pages[c].items[i].row, value) {
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
                let field = &mut self.slots[c][i].field;
                field.set_text(&v);
                *value = field.text().to_string();
            }
            _ => return false,
        }
        true
    }

    /// What an input method is composing, for the text box with the focus.
    pub fn set_preedit(&mut self, preedit: Option<crate::textfield::Preedit>) -> bool {
        match self.focus {
            Focus::Control(i) if self.kind(i).is_some_and(is_text) => {
                self.slots[self.current][i].field.set_preedit(preedit)
            }
            _ => false,
        }
    }

    /// The id of the setting or button the keyboard is on; `None` in the list or on the ×.
    pub fn focus_id(&self) -> Option<u32> {
        let it = |i: usize| self.pages.get(self.current)?.items.get(i);
        match self.focus {
            Focus::Control(i) => it(i)?.row.id(),
            Focus::Button(i) => it(i)?.button.as_ref().map(|b| b.id),
            Focus::List | Focus::Close => None,
        }
    }

    /// Whether the keyboard is in the category list.
    pub fn focus_in_list(&self) -> bool {
        self.focus == Focus::List
    }

    /// Whether the keyboard is on the × button.
    pub fn focus_on_close(&self) -> bool {
        self.focus == Focus::Close
    }

    /// Shows the page that has setting or button `id` and puts the keyboard on it. False when
    /// there is no such control or it is disabled.
    pub fn focus_row(&mut self, id: u32) -> bool {
        let Some((c, f)) = self.find(id) else {
            return false;
        };
        let live = match f {
            Focus::Control(i) => !self.pages[c].items[i].is_disabled(),
            Focus::Button(i) => self.pages[c].items[i].button_live(),
            _ => false,
        };
        if !live {
            return false;
        }
        self.show_page(c);
        self.focus = f;
        true
    }

    /// Shows page `c` (the keyboard goes to the list if it was on the page left). False when
    /// there is no such page.
    pub fn show_page(&mut self, c: usize) -> bool {
        if c >= self.pages.len() {
            return false;
        }
        if c != self.current {
            self.current = c;
            self.menu = None;
            self.hover = None;
            self.pressed = None;
            self.drag = None;
            if matches!(self.focus, Focus::Control(_) | Focus::Button(_)) {
                self.focus = Focus::List;
            }
        }
        true
    }

    fn kind(&self, i: usize) -> Option<Kind> {
        Some(self.pages.get(self.current)?.items.get(i)?.kind())
    }

    fn item(&self, i: usize) -> Option<&Item> {
        self.pages.get(self.current)?.items.get(i)
    }

    // ---- layout ------------------------------------------------------------------------------

    /// Places the card in `window` (the whole window, logical px) and the current page in it, and
    /// returns the card. Called by [`draw`](Self::draw); events use the last placement.
    pub fn layout(&mut self, p: &mut Painter, window: Rect) -> Rect {
        self.geo = shape(window);
        self.laid = true;
        match self.pages.get(self.current) {
            Some(page) => {
                self.placed = place(p, page, &self.geo).placed;
                self.placed_page = Some(self.current);
                for (i, it) in page.items.iter().enumerate() {
                    if matches!(it.row, Row::Text { .. }) {
                        let slot = &mut self.slots[self.current][i];
                        let shown = slot.field.display();
                        if slot.shown != shown {
                            slot.stops = p.caret_stops(Face::Ui, CONTROL_SIZE, &shown);
                            slot.shown = shown.into_owned();
                        }
                    }
                }
            }
            None => {
                self.placed.clear();
                self.placed_page = None;
            }
        }
        self.geo.card
    }

    /// How every page, and the list, fit in `window` with `p`'s fonts and scale. Nothing is
    /// changed; a test for each size an application promises asserts that
    /// [`Fit::problems`] is empty.
    pub fn fit(&self, p: &mut Painter, window: Rect) -> Fit {
        let g = shape(window);
        let pages = self
            .pages
            .iter()
            .map(|page| {
                let laid = place(p, page, &g);
                PageFit {
                    name: page.name.clone(),
                    content: laid.content,
                    room: g.page.h,
                    problems: laid.problems,
                }
            })
            .collect();
        let mut problems = Vec::new();
        for (c, page) in self.pages.iter().enumerate() {
            let need = p.measure(Face::Ui, CONTROL_SIZE, &page.name);
            let room = g.nav.w - NAV_TEXT - 8.0;
            if need > room + 0.5 {
                problems.push(format!(
                    "list item {c} \"{}\" needs {need:.0} of {room:.0}",
                    page.name
                ));
            }
        }
        let mut fit = Fit {
            pages,
            list_needs: g.nav.y - g.page.y + self.pages.len() as f32 * g.item_h,
            list_room: g.page.h,
        };
        if let Some(first) = fit.pages.first_mut() {
            first.problems.extend(problems);
        }
        fit
    }

    fn placed(&self, i: usize) -> Option<&Placed> {
        if self.placed_page != Some(self.current) {
            return None;
        }
        self.placed.get(i)
    }

    /// The card as last laid out.
    pub fn rect(&self) -> Rect {
        self.geo.card
    }

    /// The × button.
    pub fn close_rect(&self) -> Rect {
        self.geo.close
    }

    /// The page area: right of the list, under the header.
    pub fn page_rect(&self) -> Rect {
        self.geo.page
    }

    /// The list's item for page `c`.
    pub fn list_item_rect(&self, c: usize) -> Rect {
        if c < self.pages.len() && self.laid {
            self.geo.item(c)
        } else {
            Rect::default()
        }
    }

    /// The row of setting `id` on the current page, as last laid out; empty elsewhere.
    pub fn row_rect(&self, id: u32) -> Rect {
        match self.on_page(id).and_then(|i| self.placed(i)) {
            Some(pl) => Rect::new(self.geo.page.x, pl.y, self.geo.page.w, pl.h),
            None => Rect::default(),
        }
    }

    /// The control of setting `id` on the current page; empty elsewhere.
    pub fn control_rect(&self, id: u32) -> Rect {
        self.on_page(id)
            .and_then(|i| self.placed(i))
            .map_or(Rect::default(), |pl| pl.ctrl)
    }

    /// The button with id `id` on the current page; empty elsewhere.
    pub fn button_rect(&self, id: u32) -> Rect {
        match self.find(id) {
            Some((c, Focus::Button(i))) if c == self.current => {
                self.placed(i).and_then(|pl| pl.button).unwrap_or_default()
            }
            _ => Rect::default(),
        }
    }

    /// The minus, the value and the plus of the stepper of setting `id`.
    pub fn stepper_parts(&self, id: u32) -> (Rect, Rect, Rect) {
        stepper_parts(self.control_rect(id))
    }

    /// Segment `k` of the segmented choice of setting `id`.
    pub fn segment_rect(&self, id: u32, k: usize) -> Rect {
        match self.on_page(id) {
            Some(i) => self.segment(i, k),
            None => Rect::default(),
        }
    }

    fn on_page(&self, id: u32) -> Option<usize> {
        match self.item_of(id) {
            Some((c, i)) if c == self.current => Some(i),
            _ => None,
        }
    }

    fn segment(&self, i: usize, k: usize) -> Rect {
        let (Some(pl), Some(Row::Choice { options, .. })) =
            (self.placed(i), self.item(i).map(|t| &t.row))
        else {
            return Rect::default();
        };
        if pl.kind != Kind::Segmented || k >= options.len() {
            return Rect::default();
        }
        let c = pl.ctrl;
        let sw = (c.w - 4.0) / options.len() as f32;
        Rect::new(c.x + 2.0 + sw * k as f32, c.y + 2.0, sw, c.h - 4.0)
    }

    fn text_area(&self, i: usize) -> Rect {
        let c = self.placed(i).map_or(Rect::default(), |pl| pl.ctrl);
        Rect::new(c.x + 10.0, c.y, c.w - 20.0, c.h)
    }

    // ---- hit testing -------------------------------------------------------------------------

    fn target_at(&self, x: f32, y: f32) -> Option<Target> {
        let g = &self.geo;
        if !self.laid || !g.card.contains(x, y) {
            return None;
        }
        if g.close.contains(x, y) {
            return Some(Target::Close);
        }
        if x < g.divider_x {
            return (0..self.pages.len())
                .find(|&c| g.item(c).contains(x, y))
                .map(Target::Category);
        }
        if !g.page.contains(x, y) || self.placed_page != Some(self.current) {
            return None;
        }
        let i = self
            .placed
            .iter()
            .position(|pl| pl.kind != Kind::Gap && y >= pl.y && y < pl.y + pl.h)?;
        let (it, pl) = (self.item(i)?, &self.placed[i]);
        if it.is_disabled() {
            return Some(Target::Row(i));
        }
        if let Some(b) = pl.button
            && b.contains(x, y)
        {
            return Some(if it.button_live() {
                Target::Button(i)
            } else {
                Target::Row(i)
            });
        }
        if !pl.ctrl.contains(x, y) {
            return Some(Target::Row(i));
        }
        Some(match pl.kind {
            Kind::Toggle => Target::Toggle(i),
            Kind::Segmented => {
                let n = match &it.row {
                    Row::Choice { options, .. } => options.len(),
                    _ => 0,
                };
                (0..n)
                    .find(|&k| self.segment(i, k).inflate_y(2.0).contains(x, y))
                    .map_or(Target::Row(i), |k| Target::Segment(i, k))
            }
            Kind::Dropdown => Target::Dropdown(i),
            Kind::Stepper => {
                let (m, _, pl) = stepper_parts(pl.ctrl);
                if m.contains(x, y) {
                    Target::Minus(i)
                } else if pl.contains(x, y) {
                    Target::Plus(i)
                } else {
                    Target::Row(i)
                }
            }
            Kind::Text | Kind::Stacked => Target::Text(i),
            Kind::Action => Target::Action(i),
            Kind::Gap => Target::Row(i),
        })
    }

    // ---- changes -----------------------------------------------------------------------------

    fn row_mut(&mut self, i: usize) -> Option<&mut Row> {
        Some(&mut self.pages.get_mut(self.current)?.items.get_mut(i)?.row)
    }

    fn changed(&self, i: usize) -> Outcome {
        match self.item(i).map(|it| (it.row.id(), it.row.value())) {
            Some((Some(id), Some(value))) => Outcome::Changed { id, value },
            _ => Outcome::None,
        }
    }

    fn toggle(&mut self, i: usize, to: Option<bool>) -> Outcome {
        if let Some(Row::Toggle { value, .. }) = self.row_mut(i) {
            let next = to.unwrap_or(!*value);
            if next != *value {
                *value = next;
                return self.changed(i);
            }
        }
        Outcome::None
    }

    fn pick(&mut self, i: usize, k: usize) -> Outcome {
        if let Some(Row::Choice { value, options, .. }) = self.row_mut(i)
            && k < options.len()
            && *value != k
        {
            *value = k;
            return self.changed(i);
        }
        Outcome::None
    }

    fn pick_by(&mut self, i: usize, d: i32, wrap: bool) -> Outcome {
        let Some(Row::Choice { value, options, .. }) = self.item(i).map(|t| &t.row) else {
            return Outcome::None;
        };
        let n = options.len() as i32;
        if n == 0 {
            return Outcome::None;
        }
        let next = *value as i32 + d;
        let next = if wrap {
            next.rem_euclid(n)
        } else {
            next.clamp(0, n - 1)
        };
        self.pick(i, next as usize)
    }

    fn number_by(&mut self, i: usize, dir: i32, times: i32) -> Outcome {
        if let Some(Row::Number {
            value,
            min,
            max,
            step,
            ..
        }) = self.row_mut(i)
        {
            let by = dir.saturating_mul(*step).saturating_mul(times);
            let next = value.saturating_add(by).clamp(*min, *max);
            if next != *value {
                *value = next;
                return self.changed(i);
            }
        }
        Outcome::None
    }

    fn number_to(&mut self, i: usize, end: bool) -> Outcome {
        if let Some(Row::Number {
            value, min, max, ..
        }) = self.row_mut(i)
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
        let Some(Row::Choice { options, value, .. }) = self.item(i).map(|t| &t.row) else {
            return;
        };
        let items: Vec<MenuItem<usize>> = options
            .iter()
            .enumerate()
            .map(|(k, o)| MenuItem::new(k, o.clone()).checked(k == *value))
            .collect();
        let value = *value;
        let c = self.placed(i).map_or(Rect::default(), |pl| pl.ctrl);
        let mut menu = Menu::new(items, c.right(), c.bottom() + 4.0, true);
        menu.hover = Some(value);
        self.menu = Some((i, menu));
    }

    fn activate(&mut self, f: Focus) -> Outcome {
        match f {
            Focus::Close => Outcome::Closed,
            Focus::List => Outcome::None,
            Focus::Button(i) => match self.item(i) {
                Some(it) if it.button_live() => it
                    .button
                    .as_ref()
                    .map_or(Outcome::None, |b| Outcome::Action(b.id)),
                _ => Outcome::None,
            },
            Focus::Control(i) => match self.kind(i) {
                Some(Kind::Toggle) => self.toggle(i, None),
                Some(Kind::Segmented) => self.pick_by(i, 1, true),
                Some(Kind::Dropdown) => {
                    self.open_menu(i);
                    Outcome::Redraw
                }
                Some(Kind::Action) => match self.item(i).and_then(|t| t.row.id()) {
                    Some(id) => Outcome::Action(id),
                    None => Outcome::None,
                },
                _ => Outcome::None,
            },
        }
    }

    // ---- focus -------------------------------------------------------------------------------

    /// The current page's stops in Tab order: each enabled row's control, then its button.
    fn stops(&self) -> Vec<Focus> {
        let mut v = Vec::new();
        let Some(page) = self.pages.get(self.current) else {
            return v;
        };
        for (i, it) in page.items.iter().enumerate() {
            if it.is_gap() || it.is_disabled() {
                continue;
            }
            v.push(Focus::Control(i));
            if it.button_live() {
                v.push(Focus::Button(i));
            }
        }
        v
    }

    /// Tab (`forward`) or Shift+Tab: the list, the page's stops, the ×, round again.
    fn tab(&mut self, forward: bool) {
        let mut ring = vec![Focus::List];
        ring.extend(self.stops());
        ring.push(Focus::Close);
        let at = ring.iter().position(|&f| f == self.focus).unwrap_or(0);
        let n = ring.len();
        self.focus = ring[if forward {
            (at + 1) % n
        } else {
            (at + n - 1) % n
        }];
    }

    /// Up and Down on the page: the stop before or after, not leaving the page.
    fn step_focus(&mut self, forward: bool) {
        let stops = self.stops();
        if let Some(at) = stops.iter().position(|&f| f == self.focus) {
            let next = if forward {
                (at + 1).min(stops.len() - 1)
            } else {
                at.saturating_sub(1)
            };
            self.focus = stops[next];
        }
    }

    fn show_by(&mut self, d: isize) -> Outcome {
        let n = self.pages.len() as isize;
        let next = (self.current as isize + d).clamp(0, (n - 1).max(0)) as usize;
        if next == self.current {
            return Outcome::None;
        }
        self.show_page(next);
        Outcome::Redraw
    }

    // ---- keyboard ----------------------------------------------------------------------------

    /// A key press; `typed` is the text the event produced.
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
        let redraw = |o: Outcome| {
            if o == Outcome::None && !was_visible {
                Outcome::Redraw
            } else {
                o
            }
        };
        if *key == KeyName::Escape {
            return Outcome::Closed;
        }
        if mods.ctrl && matches!(key, KeyName::PageUp | KeyName::PageDown) {
            let d = if *key == KeyName::PageUp { -1 } else { 1 };
            let moved = self.show_by(d);
            let was = std::mem::replace(&mut self.focus, Focus::List);
            return if was != Focus::List {
                Outcome::Redraw
            } else {
                redraw(moved)
            };
        }
        if *key == KeyName::Tab {
            self.tab(!mods.shift);
            return Outcome::Redraw;
        }
        match self.focus {
            Focus::List => redraw(self.list_key(key, mods)),
            Focus::Close => match key {
                KeyName::Enter | KeyName::Space => Outcome::Closed,
                _ => redraw(Outcome::Unused),
            },
            Focus::Button(i) => match key {
                KeyName::Enter | KeyName::Space => redraw(self.activate(Focus::Button(i))),
                KeyName::Up | KeyName::Down => {
                    self.step_focus(*key == KeyName::Down);
                    Outcome::Redraw
                }
                _ => redraw(Outcome::Unused),
            },
            Focus::Control(i) => redraw(self.control_key(i, key, mods, typed)),
        }
    }

    fn list_key(&mut self, key: &KeyName, mods: Mods) -> Outcome {
        let last = self.pages.len().saturating_sub(1);
        match key {
            KeyName::Up => self.show_by(-1),
            KeyName::Down => self.show_by(1),
            KeyName::Home | KeyName::PageUp => self.show_by(-(self.current as isize)),
            KeyName::End | KeyName::PageDown => self.show_by((last - self.current) as isize),
            KeyName::Right | KeyName::Enter => match self.stops().first() {
                Some(&f) => {
                    self.focus = f;
                    Outcome::Redraw
                }
                None => Outcome::None,
            },
            KeyName::Char(ch) if !mods.ctrl && !mods.alt => {
                let n = self.pages.len();
                let starts = |c: usize| {
                    self.pages[c]
                        .name
                        .chars()
                        .next()
                        .is_some_and(|f| f.to_lowercase().eq(ch.to_lowercase()))
                };
                match (1..=n).map(|d| (self.current + d) % n).find(|&c| starts(c)) {
                    Some(c) if c != self.current => {
                        self.show_page(c);
                        Outcome::Redraw
                    }
                    _ => Outcome::None,
                }
            }
            _ => Outcome::Unused,
        }
    }

    fn control_key(&mut self, i: usize, key: &KeyName, mods: Mods, typed: Option<&str>) -> Outcome {
        let Some(kind) = self.kind(i) else {
            return Outcome::Unused;
        };
        if is_text(kind) {
            let c = self.current;
            // Up and Down leave the box; everything else is the box's.
            return match self.slots[c][i].field.handle_key(key, mods, typed, true) {
                Some(true) => {
                    let text = self.slots[c][i].field.text().to_string();
                    let id = match self.row_mut(i) {
                        Some(Row::Text { id, value, .. }) => {
                            value.clone_from(&text);
                            *id
                        }
                        _ => return Outcome::Redraw,
                    };
                    Outcome::Changed {
                        id,
                        value: Value::Text(text),
                    }
                }
                Some(false) => Outcome::Redraw,
                None => match key {
                    KeyName::Up | KeyName::Down => {
                        self.step_focus(*key == KeyName::Down);
                        Outcome::Redraw
                    }
                    _ => Outcome::Unused,
                },
            };
        }
        let step = if mods.ctrl { 10 } else { 1 };
        match key {
            KeyName::Up | KeyName::Down => {
                self.step_focus(*key == KeyName::Down);
                Outcome::Redraw
            }
            KeyName::Enter | KeyName::Space => self.activate(Focus::Control(i)),
            KeyName::Left | KeyName::Right => {
                let d = if *key == KeyName::Right { 1 } else { -1 };
                match kind {
                    Kind::Toggle => self.toggle(i, Some(d > 0)),
                    Kind::Segmented | Kind::Dropdown => self.pick_by(i, d, false),
                    Kind::Stepper => self.number_by(i, d, step),
                    _ => Outcome::Unused,
                }
            }
            KeyName::Char('+' | '=') => self.number_by(i, 1, step),
            KeyName::Char('-') => self.number_by(i, -1, step),
            KeyName::PageUp => self.number_by(i, 1, 10),
            KeyName::PageDown => self.number_by(i, -1, 10),
            KeyName::Home => match kind {
                Kind::Stepper => self.number_to(i, false),
                Kind::Segmented | Kind::Dropdown => self.pick(i, 0),
                _ => Outcome::Unused,
            },
            KeyName::End => match (kind, self.item(i).map(|t| &t.row)) {
                (Kind::Stepper, _) => self.number_to(i, true),
                (_, Some(Row::Choice { options, .. })) => {
                    let last = options.len().saturating_sub(1);
                    self.pick(i, last)
                }
                _ => Outcome::Unused,
            },
            _ => Outcome::Unused,
        }
    }

    // ---- pointer -----------------------------------------------------------------------------

    /// The pointer moved. Returns true when something needs redrawing.
    pub fn pointer_move(&mut self, x: f32, y: f32) -> bool {
        if let Some((_, menu)) = &mut self.menu {
            return menu.pointer(x, y);
        }
        if let Some(i) = self.drag {
            let c = self.current;
            let rel = x - self.text_area(i).x;
            let slot = &mut self.slots[c][i];
            let before = slot.field.caret();
            slot.field.drag(rel, &slot.stops);
            return slot.field.caret() != before;
        }
        let hover = self.target_at(x, y);
        let changed = hover != self.hover;
        self.hover = hover;
        changed
    }

    /// A mouse button went down; `clicks` is 1, or 2 for a double click. Controls act when the
    /// button comes up over the same control; a text box places its caret at once; a press
    /// outside the card closes the dialog (with a dropdown open, only the dropdown).
    pub fn press(&mut self, x: f32, y: f32, mods: Mods, clicks: u8) -> Outcome {
        self.drag = None;
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
        if !self.geo.card.contains(x, y) {
            return Outcome::Closed;
        }
        let target = self.target_at(x, y);
        self.pressed = target;
        match target {
            Some(Target::Text(i)) => {
                self.focus = Focus::Control(i);
                let c = self.current;
                let rel = x - self.text_area(i).x;
                let slot = &mut self.slots[c][i];
                if clicks >= 2 {
                    slot.field.select_word_at(rel, &slot.stops);
                } else {
                    slot.field.press(rel, &slot.stops, mods.shift);
                    self.drag = Some(i);
                }
                Outcome::Redraw
            }
            Some(Target::Button(i)) => {
                self.focus = Focus::Button(i);
                Outcome::Redraw
            }
            Some(Target::Close) => {
                self.focus = Focus::Close;
                Outcome::Redraw
            }
            Some(Target::Category(_)) => Outcome::Redraw,
            Some(Target::Row(i)) => {
                // The value of a stepper is the row's control, though not a button.
                let on_ctrl = self.placed(i).is_some_and(|pl| pl.ctrl.contains(x, y))
                    && self.item(i).is_some_and(|it| !it.is_disabled());
                if on_ctrl {
                    self.focus = Focus::Control(i);
                    Outcome::Redraw
                } else {
                    Outcome::None
                }
            }
            Some(t) => {
                if let Some(i) = t.row() {
                    self.focus = Focus::Control(i);
                }
                Outcome::Redraw
            }
            None => Outcome::None,
        }
    }

    /// The button went up: the control the press began on acts if the pointer is still on it.
    pub fn release(&mut self, x: f32, y: f32) -> Outcome {
        self.drag = None;
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
        let Some(pressed) = self.pressed.take() else {
            return Outcome::None;
        };
        if Some(pressed) != self.target_at(x, y) {
            return Outcome::None;
        }
        match pressed {
            Target::Toggle(i) => self.toggle(i, None),
            Target::Segment(i, k) => self.pick(i, k),
            Target::Dropdown(i) => {
                self.open_menu(i);
                Outcome::Redraw
            }
            Target::Minus(i) => self.number_by(i, -1, 1),
            Target::Plus(i) => self.number_by(i, 1, 1),
            Target::Action(i) => self.activate(Focus::Control(i)),
            Target::Button(i) => self.activate(Focus::Button(i)),
            Target::Close => Outcome::Closed,
            Target::Category(c) => {
                self.show_page(c);
                self.focus = Focus::List;
                Outcome::Redraw
            }
            Target::Text(_) | Target::Row(_) => Outcome::None,
        }
    }

    /// The wheel: only an open dropdown's menu scrolls; a page never does. Returns true when
    /// something moved.
    pub fn wheel(&mut self, x: f32, y: f32, dy: f32) -> bool {
        match &mut self.menu {
            Some((_, menu)) => menu.contains(x, y) && menu.scroll_by(dy),
            None => false,
        }
    }

    // ---- drawing -----------------------------------------------------------------------------

    /// Lays the dialog out in `window` and draws it over a dimmed window. `caret_on` is the blink
    /// phase of a focused text box.
    pub fn draw(&mut self, p: &mut Painter, pal: &Colors, window: Rect, caret_on: bool) {
        self.draw_with(p, pal, window, caret_on, None);
    }

    /// [`draw`](Self::draw), with the motion of [`draw_animated`](Self::draw_animated) when
    /// `fx` is given. With `None` this is the drawing `draw` has always done.
    fn draw_with(
        &mut self,
        p: &mut Painter,
        pal: &Colors,
        window: Rect,
        caret_on: bool,
        mut fx: Option<&mut Fx<'_>>,
    ) {
        self.layout(p, window);
        draw_backdrop(p, pal, window.w, window.h);
        let g = self.geo;
        let r = g.card;
        p.shadow(r, RADIUS, pal.dark);
        if pal.dark {
            p.shadow(r, RADIUS, true);
        }
        p.frame(r, RADIUS, pal.popup, pal.popup_line);
        let px = p.px().max(1.0);

        // The header: the title, the ×, a hairline under both columns.
        p.text(
            Face::Semibold,
            17.0,
            r.x + PAD,
            r.y + HEADER_H / 2.0,
            &self.title,
            pal.text,
            (g.close.x - r.x - PAD - 8.0).max(0.0),
        );
        let close_hover = self.hover == Some(Target::Close);
        if self.focus_visible && self.focus == Focus::Close {
            ring(p, pal, g.close, 6.0, pal.popup);
        }
        if close_hover {
            let fill = if self.pressed == Some(Target::Close) {
                pal.press
            } else {
                pal.hover
            };
            p.rbox(g.close, 6.0, fill);
        }
        p.icon(
            glyph::CLOSE,
            13.0,
            g.close.cx(),
            g.close.cy(),
            if close_hover { pal.text } else { pal.muted },
        );
        p.fill(
            Rect::new(r.x + 1.0, r.y + HEADER_H, r.w - 2.0, px),
            pal.popup_line,
        );
        p.fill(
            Rect::new(
                g.divider_x,
                r.y + HEADER_H,
                px,
                (r.h - HEADER_H - 1.0).max(0.0),
            ),
            pal.popup_line,
        );

        // The list.
        let old = p.clip(Rect::new(r.x, r.y + HEADER_H, g.divider_x - r.x, g.page.h));
        for c in 0..self.pages.len() {
            let it = g.item(c);
            let cur = c == self.current;
            if cur {
                if self.focus_visible && self.focus == Focus::List {
                    ring(p, pal, it, 6.0, pal.popup);
                }
                p.rbox(it, 6.0, pal.on);
                p.rbox(
                    Rect::new(it.x + MARK_INSET, it.cy() - MARK_H / 2.0, MARK_W, MARK_H),
                    MARK_W / 2.0,
                    pal.accent,
                );
            } else {
                let hovered = self.hover == Some(Target::Category(c));
                let fade = match fx.as_deref_mut() {
                    Some(fx) => fx.category(c, hovered),
                    None => f32::from(u8::from(hovered)),
                };
                if fade >= 1.0 {
                    p.rbox(it, 6.0, pal.hover);
                } else if fade > 0.0 {
                    p.rbox_a(it, 6.0, pal.hover, fade);
                }
            }
            p.text(
                Face::Ui,
                CONTROL_SIZE,
                it.x + NAV_TEXT,
                it.cy(),
                &self.pages[c].name,
                if cur { pal.text } else { pal.muted },
                it.w - NAV_TEXT - 8.0,
            );
        }
        p.restore(old);

        // The page. Its rows' motion is named under the page.
        let mut fx = fx.map(|fx| fx.under(("page", self.current)));
        let old = p.clip(g.page);
        let n = self.placed.len();
        for i in 0..n {
            if self.placed[i].kind == Kind::Gap {
                continue;
            }
            let next_is_row = self
                .placed
                .get(i + 1)
                .is_some_and(|pl| pl.kind != Kind::Gap);
            self.draw_row(p, pal, i, next_is_row, caret_on, fx.as_mut());
        }
        p.restore(old);

        // An open dropdown, over everything.
        if let Some((row, menu)) = &mut self.menu {
            match fx.as_mut() {
                Some(fx) => fx.menu(p, pal, window, *row, menu),
                None => menu.draw_in(p, pal, window),
            }
        }
    }

    fn draw_row(
        &mut self,
        p: &mut Painter,
        pal: &Colors,
        i: usize,
        next_is_row: bool,
        caret: bool,
        mut fx: Option<&mut Fx<'_>>,
    ) {
        let g = self.geo;
        let Some(it) = self.item(i).cloned() else {
            return;
        };
        let pl = self.placed[i].clone();
        let off = it.is_disabled();
        let px = p.px().max(1.0);
        if it.child {
            p.fill(
                Rect::new(g.l + 3.0, pl.y + 1.0, 2.0 * px, pl.h - 2.0),
                mix(pal.popup, pal.text, 0.14),
            );
        }
        if next_is_row {
            p.fill_a(
                Rect::new(pl.label_x, pl.y + pl.h - px, g.r - pl.label_x, px),
                pal.popup_line,
                0.6,
            );
        }
        p.text(
            Face::Ui,
            LABEL_SIZE,
            pl.label_x,
            pl.label_cy,
            it.row.label(),
            if off { pal.faint } else { pal.text },
            pl.label_w,
        );
        let max_lines = if pl.kind == Kind::Stacked {
            STACK_LINES
        } else {
            UNDER_LINES
        };
        for (k, line) in pl.help.iter().take(max_lines).enumerate() {
            p.text(
                Face::Ui,
                HELP_SIZE,
                pl.help_x,
                pl.help_cy + HELP_LINE * k as f32,
                line,
                pal.muted,
                pl.help_w,
            );
        }

        let focus = |f: Focus| self.focus == f;
        let ring_on = self.focus_visible && focus(Focus::Control(i));
        let hov = |t: Target| !off && self.hover == Some(t);
        let held = |t: Target| !off && self.hover == Some(t) && self.pressed == Some(t);

        // The row's own button.
        if let (Some(b), Some(br)) = (&it.button, pl.button) {
            let live = it.button_live();
            if self.focus_visible && focus(Focus::Button(i)) {
                ring(p, pal, br, 5.0, pal.popup);
            }
            let (h, pr) = (
                live && hov(Target::Button(i)),
                live && held(Target::Button(i)),
            );
            match b.icon {
                Some(icon) => {
                    if live {
                        draw_dialog_button(p, pal, br, "", false, h, pr);
                    } else {
                        draw_dialog_button_disabled(p, pal, br, "", false);
                    }
                    let c = if live { pal.text } else { pal.faint };
                    p.icon(icon, 14.0, br.cx(), br.cy(), c);
                }
                None if live => draw_dialog_button(p, pal, br, &b.text, false, h, pr),
                None => draw_dialog_button_disabled(p, pal, br, &b.text, false),
            }
        }

        let ctrl = pl.ctrl;
        let bg = pal.popup;
        match &it.row {
            Row::Toggle { value, id, .. } => {
                if ring_on {
                    ring(p, pal, ctrl, TOGGLE_H / 2.0, bg);
                }
                let (h, pr) = (hov(Target::Toggle(i)), held(Target::Toggle(i)));
                match fx.as_deref_mut() {
                    Some(fx) => fx.toggle(p, pal, (i, *id), ctrl, *value, h, pr),
                    None => draw_toggle(p, pal, ctrl, *value, h, pr),
                }
                if off {
                    p.rbox_a(ctrl.inset(-1.0), TOGGLE_H / 2.0, pal.popup, 0.55);
                }
            }
            Row::Choice { options, value, .. } if pl.kind == Kind::Segmented => {
                if ring_on {
                    ring(p, pal, ctrl, 6.0, bg);
                }
                p.frame(ctrl, 6.0, pal.field, pal.line);
                let fill = if off {
                    mix(pal.accent, pal.popup, 0.55)
                } else {
                    pal.accent
                };
                let pill = fx.and_then(|fx| {
                    let to = self.segment(i, *value);
                    fx.pill(i, it.row.id(), ctrl, to)
                });
                if let Some(pill) = pill {
                    // In motion: the hovered segment's fill, then the chosen fill on its way.
                    for k in 0..options.len() {
                        if k != *value && hov(Target::Segment(i, k)) {
                            p.rbox(self.segment(i, k), 4.0, pal.hover);
                        }
                    }
                    p.rbox(pill, 4.0, fill);
                }
                for (k, o) in options.iter().enumerate() {
                    let s = self.segment(i, k);
                    let on = k == *value;
                    if pill.is_some() {
                        // Drawn above.
                    } else if on {
                        p.rbox(s, 4.0, fill);
                    } else if hov(Target::Segment(i, k)) {
                        p.rbox(s, 4.0, pal.hover);
                    }
                    // In motion a label reads as chosen while the fill is under its middle.
                    let on = pill.map_or(on, |r| r.contains(s.cx(), s.cy()));
                    let c = if on {
                        pal.on_accent
                    } else if off {
                        pal.faint
                    } else {
                        pal.text
                    };
                    let face = if on { Face::Semibold } else { Face::Ui };
                    p.text_center(face, CONTROL_SIZE, s, o, c);
                }
            }
            Row::Choice { options, value, .. } => {
                if ring_on {
                    ring(p, pal, ctrl, 6.0, bg);
                }
                let open = matches!(&self.menu, Some((k, _)) if *k == i);
                let h = hov(Target::Dropdown(i)) || open;
                let shown = options.get(*value).map_or("", String::as_str);
                // The same field as the standalone drop-down's.
                crate::dropdown::field(p, pal, ctrl, shown, h, off);
            }
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
                let (m, v, plus) = stepper_parts(ctrl);
                for (rect, gl, enabled, t) in [
                    (m, glyph::MINUS, value > min && !off, Target::Minus(i)),
                    (plus, glyph::PLUS, value < max && !off, Target::Plus(i)),
                ] {
                    let fill = if enabled && held(t) {
                        pal.press
                    } else if enabled && hov(t) {
                        pal.hover
                    } else {
                        mix(pal.popup, pal.text, 0.04)
                    };
                    p.frame(rect, 6.0, fill, pal.popup_line);
                    let c = if enabled { pal.text } else { pal.faint };
                    p.icon(gl, 11.0, rect.cx(), rect.cy(), c);
                }
                p.frame(v, 6.0, pal.field, pal.line);
                let c = if off { pal.faint } else { pal.text };
                p.text_center(Face::Ui, CONTROL_SIZE, v, &number_text(*value, unit), c);
            }
            Row::Text { placeholder, .. } => {
                let focused = focus(Focus::Control(i));
                let edge = if hov(Target::Text(i)) && !focused {
                    mix(pal.line, pal.text, 0.18)
                } else {
                    pal.line
                };
                p.frame(ctrl, 6.0, pal.field, edge);
                if focused {
                    // The accent underline of the find bar's box, following the corners.
                    let old = p.clip(Rect::new(ctrl.x, ctrl.bottom() - 2.0, ctrl.w, 2.0));
                    p.rbox(ctrl, 6.0, pal.accent);
                    p.restore(old);
                }
                let colors = FieldColors {
                    text: if off { pal.faint } else { pal.text },
                    placeholder: pal.faint,
                    selection: pal.selection,
                    caret: pal.text,
                };
                let area = Rect::new(ctrl.x + 10.0, ctrl.y, ctrl.w - 20.0, ctrl.h);
                let c = self.current;
                self.slots[c][i].field.draw(
                    p,
                    area,
                    Face::Ui,
                    CONTROL_SIZE,
                    &colors,
                    placeholder,
                    focused,
                    caret,
                );
            }
            Row::Action { button, .. } => {
                if ring_on {
                    ring(p, pal, ctrl, 6.0, bg);
                }
                if off {
                    draw_dialog_button_disabled(p, pal, ctrl, button, false);
                } else {
                    let t = Target::Action(i);
                    draw_dialog_button(p, pal, ctrl, button, false, hov(t), held(t));
                }
            }
            Row::Header(_) => {}
        }
    }
}

fn is_text(k: Kind) -> bool {
    matches!(k, Kind::Text | Kind::Stacked)
}

fn stepper_parts(c: Rect) -> (Rect, Rect, Rect) {
    (
        Rect::new(c.x, c.y, STEP_W, c.h),
        Rect::new(
            c.x + STEP_W + 4.0,
            c.y,
            (c.w - 2.0 * STEP_W - 8.0).max(0.0),
            c.h,
        ),
        Rect::new(c.right() - STEP_W, c.y, STEP_W, c.h),
    )
}

mod motion;

#[cfg(test)]
mod frozen;
#[cfg(test)]
mod motion_tests;
#[cfg(test)]
mod tests;
