//! A single-page settings form with a confirming colour row. Click the Accent swatch to
//! open the picker. The row keeps its previous value until a colour is applied.

use mdshared_ui::gfx::{Painter, Rect};
use mdshared_ui::host::{self, App, Button, Config, Cx, Event};
use mdshared_ui::settings::{Form, Outcome, Row};
use mdshared_ui::theme::{DARK, LIGHT};
use mdshared_ui::tokens::Tokens;

const ACCENT: u32 = 1;

struct Demo {
    form: Form,
    dark: bool,
}

impl App for Demo {
    type Message = ();

    fn event(&mut self, cx: &mut Cx<'_, ()>, event: Event) {
        let outcome = match event {
            Event::CloseRequested => Outcome::Closed,
            Event::Theme { dark } => {
                self.dark = dark;
                Outcome::Redraw
            }
            Event::PointerMove { x, y } => self.form.pointer(x, y),
            Event::Press {
                x,
                y,
                button: Button::Left,
                clicks,
            } => self.form.press(x, y, cx.mods(), clicks.min(2) as u8),
            Event::Release {
                x,
                y,
                button: Button::Left,
            } => self.form.release(x, y),
            Event::Key(k) => match k.name {
                Some(name) if !k.repeat || self.form.colour_open().is_some() => {
                    if k.repeat
                        && matches!(
                            name,
                            mdshared_ui::keys::KeyName::Escape | mdshared_ui::keys::KeyName::Enter
                        )
                    {
                        return;
                    }
                    self.form.key(&name, k.mods, k.text.as_deref())
                }
                _ => Outcome::None,
            },
            _ => Outcome::None,
        };
        match outcome {
            Outcome::Closed => cx.exit(),
            Outcome::Changed { id, value } => {
                println!("Applied row {id}: {value:?}");
                cx.request_redraw_all();
            }
            Outcome::Redraw => cx.request_redraw_all(),
            _ => {}
        }
    }

    fn draw(&mut self, cx: &mut Cx<'_, ()>, p: &mut Painter, _: Rect) {
        let hc = cx.high_contrast();
        let pal = if hc {
            Tokens::high_contrast(self.dark).palette
        } else if self.dark {
            DARK
        } else {
            LIGHT
        };
        let (w, h) = cx.size();
        let window = Rect::new(0.0, 0.0, w, h);
        p.fill(window, pal.page);
        self.form.set_high_contrast(hc);
        self.form.draw(p, &pal, window, true);
    }
}

fn main() {
    let mut form = Form::new(
        "Colour picker",
        vec![Row::text(ACCENT, "Accent", "#3366cc").help("Click the swatch to choose a colour.")],
    )
    .single_page();
    form.show_as_colour_with_confirmation(ACCENT);
    let config = Config::new("Colour picker")
        .size(760.0, 620.0)
        .min_size(400.0, 320.0);
    if let Err(e) = host::run(config, Demo { form, dark: false }) {
        eprintln!("{e}");
    }
}
