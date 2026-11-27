//! Two fields wired with `textfield::FieldInput`, the whole of each field's input in three
//! calls: `event` for every event, `timer_fired` for its timer, `draw_menu` last. Try clicking,
//! dragging past either end, double and triple clicks, Shift+click, a click in the padding left
//! of the text (where an icon would be), right-click, Shift+F10, and switching windows in the
//! middle of a drag. The second field's menu is in Turkish.
//!
//! `cargo run -p mdshared-ui --features host --example field_input`
//! (`-- --seconds N` closes it after N seconds, for a smoke run.)

use std::time::{Duration, Instant};

use mdshared_ui::{
    cursor::Cursor,
    gfx::{Face, Gfx, Painter, Rect},
    host::{self, App, Config, Cx, Event},
    keys::KeyName,
    platform,
    textfield::{EditLabels, FieldArea, FieldColors, FieldHit, FieldInput, TextField},
    theme::LIGHT,
};

struct Field {
    text: TextField,
    input: FieldInput,
    label: &'static str,
}

struct Demo {
    gfx: Gfx,
    fields: [Field; 2],
    focused: usize,
    blink: Option<Duration>,
    quit_at: Option<Instant>,
}

/// Each field's frame and text rectangle. The frame leaves 28 px left of the text, as a
/// leading icon would.
fn area(cx: &Cx<'_, ()>, i: usize) -> FieldArea {
    let frame = Rect::new(24.0, 60.0 + 70.0 * i as f32, 340.0, 34.0);
    let text = Rect::new(
        frame.x + 28.0,
        frame.y + 5.0,
        frame.w - 38.0,
        frame.h - 10.0,
    );
    FieldArea::new(
        frame,
        FieldHit {
            area: text,
            face: Face::Ui,
            size: 15.0,
            scale: cx.scale(),
        },
    )
}

impl App for Demo {
    type Message = ();

    fn start(&mut self, cx: &mut Cx<'_, ()>) {
        if let Some(at) = self.quit_at {
            cx.wake_at(at);
        }
    }

    fn event(&mut self, cx: &mut Cx<'_, ()>, event: Event) {
        if let Event::Wake(now) = event
            && self.quit_at.is_some_and(|at| now >= at)
        {
            cx.exit();
            return;
        }
        // Every event to every field; the first that takes it ends its journey.
        for i in 0..self.fields.len() {
            let a = area(cx, i);
            let f = &mut self.fields[i];
            let outcome =
                f.input
                    .event(cx, &mut f.text, &mut self.gfx, a, self.focused == i, &event);
            if outcome == mdshared_ui::textfield::FieldOutcome::Focus {
                self.focused = i;
            }
            if outcome.taken() {
                self.set_cursor(cx, &event);
                return;
            }
        }
        match event {
            Event::PointerMove { .. } => self.set_cursor(cx, &event),
            Event::Key(k) if k.name == Some(KeyName::Tab) => {
                self.focused = 1 - self.focused;
                cx.request_redraw_all();
            }
            Event::Focus(_) => cx.request_redraw_all(),
            Event::CloseRequested => cx.exit(),
            _ => {}
        }
    }

    fn timer_fired(&mut self, cx: &mut Cx<'_, ()>, name: &str) {
        for i in 0..self.fields.len() {
            let a = area(cx, i);
            let f = &mut self.fields[i];
            if f.input
                .timer_fired(cx, &mut f.text, &mut self.gfx, a, name)
                .taken()
            {
                return;
            }
        }
    }

    fn draw(&mut self, cx: &mut Cx<'_, ()>, p: &mut Painter, _: Rect) {
        let (w, h) = cx.size();
        let window = Rect::new(0.0, 0.0, w, h);
        let pal = LIGHT;
        p.fill(window, pal.page);
        p.text(
            Face::Semibold,
            16.0,
            24.0,
            26.0,
            "FieldInput",
            pal.text,
            w - 48.0,
        );
        let colors = FieldColors {
            text: pal.text,
            placeholder: pal.muted,
            selection: pal.accent,
            caret: pal.text,
        };
        let focused = cx.focused();
        for i in 0..self.fields.len() {
            let a = area(cx, i);
            let f = &mut self.fields[i];
            p.text(
                Face::Ui,
                12.0,
                a.frame().x,
                a.frame().y - 10.0,
                f.label,
                pal.muted,
                300.0,
            );
            p.rbox(a.frame(), 5.0, pal.field);
            p.icon(
                mdshared_ui::glyph::SEARCH,
                14.0,
                a.frame().x + 14.0,
                a.frame().cy(),
                pal.muted,
            );
            f.text.draw_blinking(
                p,
                cx.anim(),
                ("field", i),
                self.blink,
                a.text().area,
                Face::Ui,
                15.0,
                &colors,
                "Type here",
                focused && self.focused == i,
            );
        }
        // The menus last, over everything.
        for f in &mut self.fields {
            f.input.draw_menu(p, &pal, window);
        }
    }
}

impl Demo {
    fn set_cursor(&self, cx: &mut Cx<'_, ()>, event: &Event) {
        let Event::PointerMove { x, y } = *event else {
            return;
        };
        let cursor = (0..self.fields.len())
            .find_map(|i| self.fields[i].input.cursor_at(area(cx, i), (x, y)))
            .unwrap_or(Cursor::Default);
        cx.set_cursor(cursor);
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let mut quit_at = None;
    while let Some(arg) = args.next() {
        if arg == "--seconds" {
            let secs: f64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(2.0);
            quit_at = Some(Instant::now() + Duration::from_secs_f64(secs.max(0.0)));
        }
    }
    let mut first = TextField::new();
    first.set_text("drag past either end of this line to select more of it than fits");
    let mut second = TextField::new();
    second.set_text("sağ tık: Türkçe menü");
    host::run(
        Config::new("FieldInput demo")
            .size(390.0, 230.0)
            .min_size(390.0, 230.0),
        Demo {
            gfx: Gfx::new(),
            fields: [
                Field {
                    text: first,
                    input: FieldInput::new().timer_name("field-0"),
                    label: "English menu",
                },
                Field {
                    text: second,
                    input: FieldInput::new()
                        .timer_name("field-1")
                        .labels(EditLabels::new("Kes", "Kopyala", "Yapıştır", "Tümünü seç")),
                    label: "Turkish menu",
                },
            ],
            focused: 0,
            blink: platform::caret_blink_time(),
            quit_at,
        },
    )?;
    Ok(())
}
