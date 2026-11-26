//! A caret blinking while a list scrolls, in two damage rectangles (roadmap K1, BT3):
//! `cargo run -p mdshared-ui --features host --example host_regions [-- --seconds N]`.
//!
//! The text field's caret blinks on the animator's deadline and asks for only the field's
//! rectangle; a named timer (`Cx::timer_at("scroll", ..)`) moves the list and asks for only the
//! list's rectangle. The two are on the same clock, so a blink and a scroll come at the same
//! instant: the frame is given both rectangles, draws each with its own clip, and presents both
//! with `present_with_damage`, leaving the gap between them alone. The application opts in with
//! [`App::damage_regions`]. Each call prints the rectangle it was given, so a run with nobody
//! looking shows whether the regions really were separate. `--seconds N` closes it after N
//! seconds by the host's one-shot timer.

use std::time::{Duration, Instant};

use mdshared_ui::gfx::{Face, Painter, Rect};
use mdshared_ui::host::{self, App, Config, Cx, Event};
use mdshared_ui::platform;
use mdshared_ui::textfield::{FieldColors, TextField};
use mdshared_ui::theme::{LIGHT, Palette};

const FIELD: Rect = Rect::new(24.0, 72.0, 360.0, 32.0);
const LIST: Rect = Rect::new(24.0, 136.0, 480.0, 260.0);
const ROW_H: f32 = 28.0;
const ROWS: usize = 30;

fn title_row(w: f32) -> Rect {
    Rect::new(0.0, 0.0, w, 44.0)
}

fn field_inner() -> Rect {
    FIELD.inset(8.0)
}

struct Demo {
    pal: Palette,
    field: TextField,
    blink: Option<Duration>,
    /// The list's scroll, in content px.
    scroll: f32,
    /// The instant the next scroll timer is due: a fixed cadence, so it meets the caret's
    /// blink (which runs from the same start) at the same instant.
    scroll_due: Option<Instant>,
    until: Option<Instant>,
    seconds: Option<u64>,
    draws: u32,
}

impl Demo {
    /// The rows in view, from the scroll.
    fn visible_rows(&self) -> impl Iterator<Item = (usize, Rect)> + '_ {
        let first = (self.scroll / ROW_H).floor() as usize;
        let offset = self.scroll % ROW_H;
        (0..=(LIST.h / ROW_H).ceil() as usize)
            .filter(move |k| first + k < ROWS)
            .map(move |k| {
                let r = Rect::new(
                    LIST.x,
                    LIST.y + k as f32 * ROW_H - offset,
                    LIST.w,
                    ROW_H - 2.0,
                );
                (first + k, r)
            })
    }

    fn schedule_scroll(&mut self, cx: &mut Cx<'_, ()>) {
        let half = self.blink.unwrap_or(Duration::from_millis(530));
        let due = match self.scroll_due {
            Some(t) => t + half,
            None => cx.now() + half,
        };
        self.scroll_due = Some(due);
        cx.timer_at("scroll", due);
    }
}

impl App for Demo {
    type Message = ();

    fn start(&mut self, cx: &mut Cx<'_, ()>) {
        self.blink = platform::caret_blink_time();
        if let Some(s) = self.seconds {
            self.until = Some(cx.now() + Duration::from_secs(s));
            cx.wake_at(self.until.unwrap());
        }
        self.schedule_scroll(cx);
        println!(
            "start: scale {}, caret blink {:?}, scroll every {:?}",
            cx.scale(),
            self.blink,
            self.blink.unwrap_or(Duration::from_millis(530))
        );
    }

    fn event(&mut self, cx: &mut Cx<'_, ()>, event: Event) {
        match event {
            Event::Wake(now) => {
                if self.until.is_some_and(|u| u <= now) {
                    println!(
                        "wake: {} seconds are up, closing",
                        self.seconds.unwrap_or(0)
                    );
                    cx.exit();
                }
            }
            Event::CloseRequested => cx.exit(),
            _ => {}
        }
    }

    fn timer_fired(&mut self, cx: &mut Cx<'_, ()>, name: &str) {
        if name != "scroll" {
            return;
        }
        // A list that scrolls a row every blink, wrapping at the end.
        self.scroll += ROW_H;
        if self.scroll >= (ROWS as f32) * ROW_H {
            self.scroll = 0.0;
        }
        println!("timer: scroll -> {:.0} px", self.scroll);
        // Only the list changed: the caret's own rectangle comes from the animator, which is
        // due at the same instant.
        cx.request_redraw(LIST);
        self.schedule_scroll(cx);
    }

    /// Two rectangles means two `draw` calls; they are never joined into their union.
    fn damage_regions(&self) -> bool {
        true
    }

    fn draw(&mut self, cx: &mut Cx<'_, ()>, p: &mut Painter, damage: Rect) {
        self.draws += 1;
        println!(
            "draw {}: region {:.0},{:.0} {:.0}x{:.0}",
            self.draws, damage.x, damage.y, damage.w, damage.h
        );
        let pal = self.pal;
        let (w, h) = cx.size();
        p.fill(damage, pal.page);

        let title = title_row(w);
        p.fill(title, pal.title);
        p.text(
            Face::Semibold,
            13.0,
            16.0,
            title.cy(),
            "two damage regions",
            pal.text,
            w - 32.0,
        );

        // The field. The demo treats it as having the keyboard, so the caret blinks even when
        // the run has no desktop focus; every frame asks for it (the animator forgets names a
        // frame does not ask for).
        p.rbox(FIELD, 4.0, pal.field);
        let colors = FieldColors {
            text: pal.text,
            placeholder: pal.muted,
            selection: pal.selection,
            caret: pal.text,
        };
        self.field.draw_blinking(
            p,
            cx.anim(),
            "caret",
            self.blink,
            field_inner(),
            Face::Ui,
            14.0,
            &colors,
            "The caret blinks here",
            true,
        );
        // The caret's blink redraws the field's rectangle only.
        cx.animating(FIELD);

        // The list, scrolled, inside its own clip.
        p.rbox(LIST, 6.0, pal.field);
        let clip = p.clip(LIST);
        for (i, row) in self.visible_rows() {
            if row.bottom() < LIST.y || row.y > LIST.bottom() {
                continue;
            }
            let fill = if i % 2 == 0 { pal.page } else { pal.field };
            p.fill(Rect::new(LIST.x, row.y, LIST.w, row.h), fill);
            p.text(
                Face::Ui,
                13.0,
                LIST.x + 14.0,
                row.cy(),
                &format!("Row {}", i + 1),
                pal.text,
                LIST.w - 28.0,
            );
        }
        p.restore(clip);

        let footer = format!("draws: {}, scroll: {:.0} px", self.draws, self.scroll);
        p.text(Face::Ui, 12.0, LIST.x, h - 18.0, &footer, pal.muted, LIST.w);
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
        field: TextField::new(),
        blink: None,
        scroll: 0.0,
        scroll_due: None,
        until: None,
        seconds,
        draws: 0,
    };
    let config = Config::new("mdshared-ui host regions")
        .size(640.0, 440.0)
        .min_size(420.0, 320.0)
        .app_id("mdshared-host-regions");
    let started = Instant::now();
    if let Err(e) = host::run(config, demo) {
        eprintln!("host: {e}");
        std::process::exit(1);
    }
    println!("ended after {:.1} s", started.elapsed().as_secs_f64());
}
