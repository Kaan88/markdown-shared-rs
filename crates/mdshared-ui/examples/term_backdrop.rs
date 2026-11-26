//! The Windows host's terminal backdrop next to an opaque reference, with no shell process.
//! `cargo run -p mdshared-ui --features host,term --example term_backdrop -- --seconds 2`
//! Add `--mica` or `--off` to switch from Acrylic. Without `--seconds`, close with Alt+F4.

use std::time::Duration;

use mdshared_app::backdrop::Backdrop;
use mdshared_term::{Point, Screen, Selection, SelectionKind};
use mdshared_ui::gfx::{Face, Painter, Rect};
use mdshared_ui::host::{self, App, Config, Cx, Event};
use mdshared_ui::termview::{self, BackgroundOpacity, CellFont, Scheme, View};
use mdshared_ui::theme::DARK;

struct Demo {
    font: CellFont,
    screen: Screen,
    selection: Selection,
    backdrop: Backdrop,
    seconds: Option<u64>,
}

impl App for Demo {
    type Message = ();

    fn start(&mut self, cx: &mut Cx<'_, ()>) {
        cx.set_backdrop(self.backdrop, &DARK);
        if let Some(seconds) = self.seconds {
            cx.timer_at("exit", cx.now() + Duration::from_secs(seconds));
        }
    }

    fn event(&mut self, _: &mut Cx<'_, ()>, _: Event) {}

    fn timer_fired(&mut self, cx: &mut Cx<'_, ()>, name: &str) {
        if name == "exit" {
            cx.exit();
        }
    }

    fn draw(&mut self, cx: &mut Cx<'_, ()>, p: &mut Painter, _: Rect) {
        let (w, h) = cx.size();
        p.fill(Rect::new(0.0, 0.0, w, h), DARK.bar);
        let view = View::new().focused(cx.focused());
        let scheme = Scheme::from_palette(&DARK);
        let metrics = self.font.metrics(16.0, p.scale);
        let (cw, ch) = metrics.cell_device();
        let width = cw as f32 * self.screen.cols() as f32 / p.scale + 8.0;
        let height = ch as f32 * self.screen.rows() as f32 / p.scale + 8.0;
        for (i, opacity) in [0.65, 1.0].into_iter().enumerate() {
            let x = 14.0 + (width + 14.0) * i as f32;
            let label = if i == 0 {
                "65% (with padding)"
            } else {
                "Opaque"
            };
            p.text(Face::Ui, 14.0, x, 15.0, label, DARK.text, width);
            termview::draw_screen_with_background(
                p,
                &mut self.font,
                &scheme,
                &metrics,
                Rect::new(x, 30.0, width, height),
                &self.screen,
                Some(&self.selection),
                view,
                BackgroundOpacity::new(opacity).with_padding(true),
            );
        }
        println!(
            "{:?}: backdrop active={}, focused={}, high contrast={}",
            self.backdrop,
            p.frost_key().is_some(),
            cx.focused(),
            cx.high_contrast()
        );
    }
}

fn main() -> Result<(), winit::error::EventLoopError> {
    let mut args = std::env::args().skip(1);
    let mut backdrop = Backdrop::Acrylic;
    let mut seconds = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--mica" => backdrop = Backdrop::Mica,
            "--off" => backdrop = Backdrop::Off,
            "--seconds" => seconds = args.next().and_then(|s| s.parse::<u64>().ok()),
            _ => {}
        }
    }
    let mut screen = Screen::new(30, 7, 0);
    screen.feed(
        "Default background and text\r\n╭─────╮ ╳\r\n\x1b[44m Explicit background \x1b[0m\r\n\
         Selected text\r\n\x1b[4mUnderlined\x1b[0m\r\nCursor: \x1b[6 q"
            .as_bytes(),
    );
    let line = screen.screen_start() + 3;
    let mut selection = Selection::start(&screen, Point::new(line, 0), SelectionKind::Stream, 1);
    selection.extend(&screen, Point::new(line, 7));
    host::run(
        Config::new("Terminal background").size(650.0, 215.0),
        Demo {
            font: CellFont::new(),
            screen,
            selection,
            backdrop,
            seconds,
        },
    )
}
