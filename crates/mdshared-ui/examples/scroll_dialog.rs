//! A dialog whose body is taller than the card, on the window host:
//! `cargo run -p mdshared-ui --features host --example scroll_dialog [-- --seconds N]`.
//!
//! The body is laid out with `layout::Flex::column()` and drawn through
//! [`scrollarea::ScrollArea::scrolled`]. The wheel over the body, Page Up and Page Down, Home
//! and End scroll it; Tab and Shift+Tab move the focus from row to row and the area brings the
//! focused row into view; a click on a row focuses it; Escape and the Close button end the run.
//! Each frame prints the offset, so a run with nobody looking still shows the arithmetic.

use std::time::{Duration, Instant};

use mdshared_ui::focus::{self, Focus};
use mdshared_ui::gfx::{Face, Painter, Rect};
use mdshared_ui::host::{self, App, Button, Config, Cx, Event};
use mdshared_ui::keys::KeyName;
use mdshared_ui::layout::{Flex, Item};
use mdshared_ui::overlay::{
    DIALOG_FOOTER, DIALOG_PAD, dialog_button_w, dialog_card, draw_backdrop, draw_dialog_button,
    draw_dialog_card,
};
use mdshared_ui::scrollarea::{self, ScrollArea};
use mdshared_ui::theme::{DARK, LIGHT, Palette};

const ROWS: usize = 14;
const ROW_H: f32 = 40.0;
const HEADER: f32 = 40.0;

struct Demo {
    pal: Palette,
    area: ScrollArea,
    focus: Focus<usize>,
    /// The body's rectangle and the rows in content pixels, from the last frame: the events
    /// need them before the next one is drawn.
    body: Rect,
    rows: Vec<Rect>,
    /// The Close button, from the last frame, for the press that may hit it.
    close: Rect,
    pointer: Option<(f32, f32)>,
    hover: Option<usize>,
    /// The row last brought into view, so the reveal is asked for only when the focus moves.
    revealed: Option<usize>,
    pressed: bool,
    until: Option<Instant>,
    seconds: Option<u64>,
    frames: u32,
}

fn card_of((w, h): (f32, f32)) -> Rect {
    dialog_card(560.0, 440.0, w, h)
}

fn body_of(card: Rect) -> Rect {
    Rect::new(
        card.x + DIALOG_PAD,
        card.y + DIALOG_PAD + HEADER,
        card.w - 2.0 * DIALOG_PAD,
        (card.h - 2.0 * DIALOG_PAD - HEADER - DIALOG_FOOTER - 8.0).max(0.0),
    )
}

fn footer_of(card: Rect) -> Rect {
    Rect::new(
        card.x + DIALOG_PAD,
        card.bottom() - DIALOG_FOOTER + 20.0,
        card.w - 2.0 * DIALOG_PAD,
        32.0,
    )
}

fn rows() -> (Flex, Vec<Item>) {
    (
        Flex::column().gap(8.0),
        (0..ROWS).map(|_| Item::fixed(ROW_H)).collect(),
    )
}

fn row_at(rows: &[Rect], point: (f64, f64)) -> Option<usize> {
    rows.iter()
        .position(|r| r.contains(point.0 as f32, point.1 as f32))
}

/// Moves the focus a row up or down, stopping at the ends.
fn step(focus: &mut Focus<usize>, up: bool) {
    let at = focus.focused().unwrap_or(0);
    let next = if up {
        at.saturating_sub(1)
    } else {
        (at + 1).min(ROWS - 1)
    };
    focus.focus(next);
}

impl Demo {
    fn over(&self, x: f32, y: f32) -> Option<usize> {
        self.area
            .to_content(self.body, x, y)
            .and_then(|p| row_at(&self.rows, p))
    }
}

impl App for Demo {
    type Message = ();

    fn start(&mut self, cx: &mut Cx<'_, ()>) {
        self.focus.begin_frame();
        for i in 0..ROWS {
            self.focus.add(i);
        }
        self.focus.end_frame();
        self.focus.focus(0);
        if let Some(s) = self.seconds {
            self.until = Some(cx.now() + Duration::from_secs(s));
            cx.wake_at(self.until.unwrap());
        }
    }

    fn event(&mut self, cx: &mut Cx<'_, ()>, event: Event) {
        match event {
            Event::PointerMove { x, y } => {
                self.pointer = Some((x, y));
                let over = self.over(x, y);
                if over != self.hover {
                    self.hover = over;
                    cx.request_redraw_all();
                }
            }
            Event::PointerLeave => {
                self.pointer = None;
                self.hover = None;
                cx.request_redraw_all();
            }
            Event::Press {
                x,
                y,
                button: Button::Left,
                ..
            } => {
                if self.close.contains(x, y) {
                    cx.exit();
                    return;
                }
                if let Some(i) = self.over(x, y) {
                    self.focus.focus(i);
                    self.pressed = true;
                    cx.request_redraw_all();
                }
            }
            Event::Release { .. } => {
                self.pressed = false;
                cx.request_redraw_all();
            }
            Event::Wheel { dx, dy, .. } => {
                if self.pointer.is_some_and(|(x, y)| self.body.contains(x, y))
                    && self.area.wheel(dx as f64, dy as f64, cx.mods().shift)
                {
                    cx.request_redraw_all();
                }
            }
            Event::Key(k) => {
                let shift = k.mods.shift;
                match k.name {
                    Some(KeyName::Tab) => {
                        if self.focus.tab(shift) {
                            cx.request_redraw_all();
                        }
                    }
                    Some(KeyName::Down) => {
                        step(&mut self.focus, false);
                        cx.request_redraw_all();
                    }
                    Some(KeyName::Up) => {
                        step(&mut self.focus, true);
                        cx.request_redraw_all();
                    }
                    Some(KeyName::Escape) => cx.exit(),
                    Some(name) => {
                        if self.area.key(name, k.mods) {
                            cx.request_redraw_all();
                        }
                    }
                    None => {}
                }
            }
            Event::Theme { dark } => {
                self.pal = if dark { DARK } else { LIGHT };
                cx.request_redraw_all();
            }
            Event::Wake(now) => {
                if self.until.is_some_and(|u| u <= now) {
                    println!("wake: the seconds are up, closing");
                    cx.exit();
                }
            }
            Event::CloseRequested => cx.exit(),
            _ => {}
        }
    }

    fn draw(&mut self, cx: &mut Cx<'_, ()>, p: &mut Painter, damage: Rect) {
        self.frames += 1;
        // The widgets this frame has, in Tab order; the focus keeps its id.
        self.focus.begin_frame();
        for i in 0..ROWS {
            self.focus.add(i);
        }
        self.focus.end_frame();
        let pal = self.pal;
        let (w, h) = cx.size();
        let (ox, oy) = self.area.offset();
        println!(
            "frame {}: drew {:.0}x{:.0}, offset {ox:.0}, {oy:.0}",
            self.frames, damage.w, damage.h
        );
        p.fill(Rect::new(0.0, 0.0, w, h), pal.page);
        draw_backdrop(p, &pal, w, h);

        let card = card_of((w, h));
        draw_dialog_card(p, &pal, card);
        p.text(
            Face::Semibold,
            16.0,
            card.x + DIALOG_PAD,
            card.y + DIALOG_PAD,
            "Preferences",
            pal.text,
            card.w - 2.0 * DIALOG_PAD,
        );

        let body = body_of(card);
        let (flex, items) = rows();
        let rects = self.area.layout_column(body, &flex, &items);
        self.body = body;
        self.rows = rects.clone();
        // The focused row is brought into view when the focus moves to it, not every frame:
        // the wheel and the page keys may then scroll inside it.
        let focused = self.focus.focused();
        if focused != self.revealed {
            self.revealed = focused;
            if let Some(i) = focused
                && let Some(r) = rects.get(i)
            {
                self.area.reveal_focus(
                    &self.focus,
                    i,
                    (f64::from(r.x), f64::from(r.y)),
                    (f64::from(r.w), f64::from(r.h)),
                );
            }
        }

        let saved = self.area.scrolled(p, body);
        for (i, r) in rects.iter().enumerate() {
            let hovered = self.hover == Some(i);
            let fill = if hovered { pal.hover } else { pal.field };
            p.frame(*r, 5.0, fill, pal.line);
            p.text(
                Face::Ui,
                13.0,
                r.x + 12.0,
                r.cy(),
                &format!("Setting {}", i + 1),
                pal.text,
                r.w - 24.0,
            );
            if self.focus.ring_visible(i) {
                focus::draw_ring(p, &pal, *r, 5.0);
            }
        }
        saved.restore(p);
        scrollarea::draw_bars(p, &pal, body, &self.area, self.hover.is_some());

        let footer = footer_of(card);
        let bw = dialog_button_w(p, "Close");
        let button = Rect::new(footer.right() - bw, footer.cy() - 16.0, bw, 32.0);
        self.close = button;
        let over_button = self.pointer.is_some_and(|(x, y)| button.contains(x, y));
        draw_dialog_button(p, &pal, button, "Close", true, over_button, self.pressed);
        p.text(
            Face::Ui,
            12.0,
            card.x + DIALOG_PAD,
            footer.cy(),
            "Wheel or Page Up / Page Down scrolls the body",
            pal.muted,
            card.w - 2.0 * DIALOG_PAD - bw - 16.0,
        );
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let mut seconds = None;
    while let Some(a) = args.next() {
        if a == "--seconds" {
            seconds = args.next().and_then(|s| s.parse().ok());
        }
    }
    let demo = Demo {
        pal: LIGHT,
        area: ScrollArea::new(),
        focus: Focus::new(),
        body: Rect::new(0.0, 0.0, 0.0, 0.0),
        rows: Vec::new(),
        close: Rect::new(0.0, 0.0, 0.0, 0.0),
        pointer: None,
        hover: None,
        revealed: None,
        pressed: false,
        until: None,
        seconds,
        frames: 0,
    };
    let config = Config::new("mdshared-ui scroll area")
        .size(760.0, 560.0)
        .min_size(520.0, 360.0)
        .app_id("mdshared-scrollarea");
    let started = Instant::now();
    if let Err(e) = host::run(config, demo) {
        eprintln!("host: {e}");
        std::process::exit(1);
    }
    println!("ended after {:.1} s", started.elapsed().as_secs_f64());
}
