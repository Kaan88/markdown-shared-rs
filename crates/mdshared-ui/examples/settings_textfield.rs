//! Try selection and the edit menu in a settings page. Open the Mode dropdown and hover
//! over the part of the Name field it covers: the cursor should stay an arrow.
//! `cargo run -p mdshared-ui --features host --example settings_textfield`

use mdshared_ui::{
    gfx::{Gfx, Painter, Rect},
    host::{self, App, Button, Config, Cx, Event},
    keys::KeyName,
    settings::{Outcome, Row, pages},
    submenu::{self, Cascade},
    textfield::ContextAction,
    theme::LIGHT,
};

const SCROLL: &str = "selection-scroll";

struct Demo {
    gfx: Gfx,
    form: pages::Form,
    menu: Option<Cascade<ContextAction>>,
    pointer: Option<(f32, f32)>,
}

impl Demo {
    fn menu_result(&mut self, outcome: submenu::Outcome<ContextAction>) -> Outcome {
        match outcome {
            submenu::Outcome::Chosen(action) => {
                self.form.apply_context_action(action);
                self.menu = None;
                Outcome::Redraw
            }
            submenu::Outcome::Close | submenu::Outcome::Outside => {
                self.menu = None;
                Outcome::Redraw
            }
            submenu::Outcome::Changed => Outcome::Redraw,
            _ => Outcome::None,
        }
    }

    fn update_input(&self, cx: &mut Cx<'_, ()>) {
        let cursor = self
            .pointer
            .filter(|_| self.menu.is_none())
            .and_then(|(x, y)| self.form.pointer_cursor_at(x, y));
        cx.set_cursor(cursor.unwrap_or_default());
        if let Some(at) = self.form.text_scroll_deadline() {
            cx.timer_at(SCROLL, at);
        } else {
            cx.cancel_timer(SCROLL);
        }
    }
}

impl App for Demo {
    type Message = ();

    fn event(&mut self, cx: &mut Cx<'_, ()>, event: Event) {
        let out = match event {
            Event::PointerMove { x, y } => {
                self.pointer = Some((x, y));
                if let Some(menu) = &mut self.menu {
                    if menu.pointer(x, y, cx.now()) {
                        Outcome::Redraw
                    } else {
                        Outcome::None
                    }
                } else {
                    self.form
                        .pointer_motion((x, y), cx.mods(), cx.now(), &mut self.gfx)
                }
            }
            Event::PointerLeave => {
                self.pointer = None;
                Outcome::None
            }
            Event::Press {
                x,
                y,
                button,
                clicks,
            } => {
                self.pointer = Some((x, y));
                match button {
                    Button::Left => match &mut self.menu {
                        Some(menu) => {
                            let out = menu.click(x, y);
                            self.menu_result(out)
                        }
                        None => self.form.pointer_press(
                            (x, y),
                            cx.mods(),
                            clicks.min(3) as u8,
                            cx.now(),
                            &mut self.gfx,
                            cx.scale(),
                        ),
                    },
                    Button::Right => {
                        self.menu = self.form.context_menu(Some((x, y))).map(|items| {
                            Cascade::at(items.into_iter().map(Into::into).collect(), x, y)
                        });
                        Outcome::Redraw
                    }
                    _ => Outcome::None,
                }
            }
            Event::Release {
                x,
                y,
                button: Button::Left,
            } if self.menu.is_none() => self.form.pointer_release((x, y), cx.now(), &mut self.gfx),
            Event::Wheel { dy, .. } if self.menu.is_none() => {
                if self.pointer.is_some_and(|(x, y)| self.form.wheel(x, y, dy)) {
                    Outcome::Redraw
                } else {
                    Outcome::None
                }
            }
            Event::Key(key) => {
                let name = key.name.unwrap_or(KeyName::Char('\0'));
                if let Some(menu) = &mut self.menu {
                    let out = menu.key(&name);
                    self.menu_result(out)
                } else {
                    self.form.key(&name, key.mods, key.text.as_deref())
                }
            }
            Event::Text(text) if self.menu.is_none() => {
                self.form
                    .key(&KeyName::Char('\0'), Default::default(), Some(&text))
            }
            Event::Preedit(preedit) if self.menu.is_none() => {
                self.form.set_preedit(preedit);
                Outcome::Redraw
            }
            Event::Focus(false) | Event::Scale(_) => {
                self.form.cancel_text_pointer();
                self.menu = None;
                Outcome::Redraw
            }
            Event::CloseRequested => Outcome::Closed,
            _ => Outcome::None,
        };
        match out {
            Outcome::Closed => cx.exit(),
            Outcome::None | Outcome::Unused => {}
            _ => cx.request_redraw_all(),
        }
        self.update_input(cx);
    }

    fn timer_fired(&mut self, cx: &mut Cx<'_, ()>, name: &str) {
        if name == SCROLL {
            if self.form.tick_text_scroll(cx.now(), &mut self.gfx) {
                cx.request_redraw_all();
            }
            self.update_input(cx);
        }
    }

    fn draw(&mut self, cx: &mut Cx<'_, ()>, p: &mut Painter, _: Rect) {
        let (w, h) = cx.size();
        let window = Rect::new(0.0, 0.0, w, h);
        p.fill(window, LIGHT.page);
        self.form
            .draw(p, &LIGHT, window, cx.focused() && self.menu.is_none());
        if let Some(menu) = &mut self.menu {
            menu.draw_in(p, &LIGHT, window);
        }
        self.update_input(cx);
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let general = pages::Page::new(
        "General",
        vec![
            pages::Item::new(Row::choice(1, "Mode", &["First", "Second", "Third"], 0)).dropdown(),
            Row::text(
                2,
                "Name",
                "one two three — drag past either edge to select more of this line",
            )
            .into(),
            Row::text(3, "Secret", "not a real password")
                .secret()
                .into(),
        ],
    );
    host::run(
        Config::new("Settings text editing")
            .size(790.0, 440.0)
            .min_size(640.0, 300.0),
        Demo {
            gfx: Gfx::new(),
            form: pages::Form::new("Settings text editing", vec![general]),
            menu: None,
            pointer: None,
        },
    )?;
    Ok(())
}
