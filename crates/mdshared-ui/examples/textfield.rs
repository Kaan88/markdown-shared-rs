//! Try mouse selection, edge scrolling and the context menu on two fields:
//! `cargo run -p mdshared-ui --features host --example textfield`.

use std::time::Duration;

use mdshared_ui::{
    cursor::Cursor,
    gfx::{Face, Gfx, Painter, Rect},
    host::{self, App, Button, Config, Cx, Event},
    keys::KeyName,
    platform,
    submenu::{Cascade, Outcome},
    textfield::{ContextAction, FieldColors, FieldHit, Pointer, TextField},
    theme::LIGHT,
};

const NAMES: [&str; 2] = ["Editable field", "Secret field"];
const SCROLL: &str = "edge-scroll";

struct Demo {
    gfx: Gfx,
    fields: [TextField; 2],
    pointer: [Pointer; 2],
    focused: usize,
    popup: Option<Cascade<ContextAction>>,
    blink: Option<Duration>,
}

fn hit(cx: &Cx<'_, ()>, index: usize) -> FieldHit {
    FieldHit {
        area: Rect::new(32.0, 75.0 + 68.0 * index as f32, 310.0, 26.0),
        face: Face::Ui,
        size: 16.0,
        scale: cx.scale(),
    }
}

impl Demo {
    fn popup_result(&mut self, cx: &mut Cx<'_, ()>, outcome: Outcome<ContextAction>) {
        match outcome {
            Outcome::Chosen(action) => {
                self.fields[self.focused].apply_context_action(action);
                self.popup = None;
            }
            Outcome::Close | Outcome::Outside => self.popup = None,
            Outcome::Changed => {}
            _ => return,
        }
        cx.request_redraw_all();
    }

    fn arm_scroll(&self, cx: &mut Cx<'_, ()>) {
        if let Some(at) = self.pointer[self.focused].deadline() {
            cx.timer_at(SCROLL, at);
        } else {
            cx.cancel_timer(SCROLL);
        }
    }
}

impl App for Demo {
    type Message = ();

    fn start(&mut self, cx: &mut Cx<'_, ()>) {
        self.pointer = std::array::from_fn(|_| Pointer::system(cx.scale()));
    }

    fn event(&mut self, cx: &mut Cx<'_, ()>, event: Event) {
        let index = self.focused;
        match event {
            Event::PointerMove { x, y } => {
                let cursor = if let Some(menu) = &mut self.popup {
                    if menu.pointer(x, y, cx.now()) {
                        cx.request_redraw_all();
                    }
                    Cursor::Default
                } else {
                    if self.pointer[index].pointer_move(
                        &mut self.fields[index],
                        &mut self.gfx,
                        hit(cx, index),
                        (x, y),
                        cx.now(),
                    ) {
                        cx.request_redraw_all();
                    }
                    (0..2)
                        .find_map(|i| hit(cx, i).cursor((x, y)))
                        .unwrap_or_default()
                };
                cx.set_cursor(cursor);
                self.arm_scroll(cx);
            }
            Event::PointerLeave => cx.set_cursor(Cursor::Default),
            Event::Press { x, y, button, .. } => {
                if button == Button::Left
                    && let Some(menu) = &mut self.popup
                {
                    let outcome = menu.click(x, y);
                    self.popup_result(cx, outcome);
                    return;
                }
                self.popup = None;
                let target = (0..2).find(|&i| hit(cx, i).cursor((x, y)).is_some());
                for (i, pointer) in self.pointer.iter_mut().enumerate() {
                    if button != Button::Left || target != Some(i) {
                        pointer.cancel();
                    }
                }
                if let Some(i) = target {
                    match button {
                        Button::Left => {
                            self.focused = i;
                            self.pointer[i].press(
                                &mut self.fields[i],
                                &mut self.gfx,
                                hit(cx, i),
                                (x, y),
                                cx.mods(),
                                cx.now(),
                            );
                            cx.set_cursor(Cursor::Text);
                        }
                        Button::Right => {
                            self.focused = i;
                            let nodes = self.fields[i]
                                .context_menu()
                                .into_iter()
                                .map(Into::into)
                                .collect();
                            self.popup = Some(Cascade::at(nodes, x, y));
                            cx.set_cursor(Cursor::Default);
                        }
                        _ => {}
                    }
                }
                self.arm_scroll(cx);
                cx.request_redraw_all();
            }
            Event::Release {
                button: Button::Left,
                ..
            } => {
                self.pointer[index].release();
                self.arm_scroll(cx);
            }
            Event::Key(key) => {
                let Some(name) = key.name else { return };
                if let Some(menu) = &mut self.popup {
                    let outcome = menu.key(&name);
                    self.popup_result(cx, outcome);
                    return;
                }
                self.pointer[index].cancel();
                self.arm_scroll(cx);
                if name == KeyName::Tab {
                    self.focused = 1 - index;
                } else if self.fields[index].is_secret()
                    && key.mods.ctrl
                    && matches!(name, KeyName::Char('v' | 'V'))
                {
                    return;
                } else {
                    self.fields[index].handle_key(&name, key.mods, key.text.as_deref(), true);
                }
                cx.request_redraw_all();
            }
            Event::Text(text) => {
                self.fields[index].insert(&text);
                cx.request_redraw_all();
            }
            Event::Preedit(preedit) => {
                self.fields[index].set_preedit(preedit);
                cx.request_redraw_all();
            }
            Event::Focus(false) => {
                self.pointer[index].cancel();
                self.popup = None;
                self.arm_scroll(cx);
                cx.request_redraw_all();
            }
            Event::Scale(scale) => {
                self.pointer = std::array::from_fn(|_| Pointer::system(scale));
                self.arm_scroll(cx);
            }
            Event::CloseRequested => cx.exit(),
            _ => {}
        }
    }

    fn timer_fired(&mut self, cx: &mut Cx<'_, ()>, name: &str) {
        if name == SCROLL {
            let i = self.focused;
            if self.pointer[i].tick(&mut self.fields[i], &mut self.gfx, hit(cx, i), cx.now()) {
                cx.request_redraw_all();
            }
            self.arm_scroll(cx);
        }
    }

    fn draw(&mut self, cx: &mut Cx<'_, ()>, p: &mut Painter, _: Rect) {
        let (w, h) = cx.size();
        let area = Rect::new(0.0, 0.0, w, h);
        let pal = LIGHT;
        p.fill(area, pal.page);
        p.text(
            Face::Semibold,
            16.0,
            24.0,
            28.0,
            "Text field mouse demo",
            pal.text,
            330.0,
        );
        let colors = FieldColors {
            text: pal.text,
            placeholder: pal.muted,
            selection: pal.accent,
            caret: pal.text,
        };
        for (i, label) in NAMES.into_iter().enumerate() {
            let area = hit(cx, i).area;
            let focused = cx.focused() && self.focused == i;
            p.text(
                Face::Ui,
                13.0,
                area.x - 6.0,
                area.y - 13.0,
                label,
                pal.muted,
                area.w,
            );
            p.rbox(area.inset(-6.0), 5.0, pal.field);
            self.fields[i].draw_blinking(
                p,
                cx.anim(),
                i,
                self.blink,
                area,
                Face::Ui,
                16.0,
                &colors,
                "Type here",
                focused,
            );
        }
        if let Some(menu) = &mut self.popup {
            menu.draw_in(p, &pal, area);
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut fields = std::array::from_fn(|_| TextField::new());
    fields[0]
        .set_text("Click, Shift+click, or drag past an edge to select more of this long line.");
    fields[1].set_text("not a real password");
    fields[1].set_secret(true);
    host::run(
        Config::new("Text field mouse demo")
            .size(410.0, 290.0)
            .min_size(368.0, 265.0),
        Demo {
            gfx: Gfx::new(),
            fields,
            pointer: std::array::from_fn(|_| Pointer::system(1.0)),
            focused: 0,
            popup: None,
            blink: platform::caret_blink_time(),
        },
    )?;
    Ok(())
}
