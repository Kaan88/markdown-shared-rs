//! The window host at work (roadmap W1):
//! `cargo run -p mdshared-ui --features host --example host [-- --seconds N]`, and with
//! `--features host,a11y` on Windows its accessibility tree too.
//!
//! One window: a title row with the caption buttons (drag it, double-click it, rest on the
//! maximize button for the snap layouts), frosted over Mica where the system shows it (Windows
//! only; elsewhere the title row stays flat); a button
//! with a tooltip after a delay; a text field whose caret blinks at the system's rate; and a
//! worker thread that sends a message every second through the host's proxy. Each frame prints
//! the rectangle it drew, and each thing the window does is printed, so a run with nobody
//! looking still shows the damage, the wake-ups and the messages. `--seconds N` closes it after
//! N seconds by a timer of the host.

use std::time::{Duration, Instant};

use mdshared_app::backdrop::Backdrop;
use mdshared_ui::chrome::{self, WinButton};
use mdshared_ui::cursor::Cursor;
use mdshared_ui::gfx::{Face, Painter, Rect};
use mdshared_ui::host::{self, App, Button, Config, Cx, Event};
use mdshared_ui::overlay::Bubble;
use mdshared_ui::textfield::{FieldColors, TextField};
use mdshared_ui::theme::{LIGHT, Palette};
use mdshared_ui::{platform, window};

/// How long the pointer rests on the button before its tooltip shows.
const TIP_DELAY: Duration = Duration::from_millis(600);
const TIP_TEXT: &str = "Prints hello on stdout";

enum Msg {
    Tick(u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Target {
    Title,
    Caption(WinButton),
    Hello,
    Field,
}

struct Demo {
    pal: Palette,
    field: TextField,
    field_focused: bool,
    /// The field's caret stops, from the last frame (the painter measures them).
    stops: Vec<f32>,
    blink: Option<Duration>,
    hover: Option<Target>,
    hover_since: Option<Instant>,
    tip: bool,
    pressed: Option<Target>,
    hellos: u32,
    ticks: u32,
    frames: u32,
    until: Option<Instant>,
    seconds: Option<u64>,
}

fn title_row(w: f32) -> Rect {
    Rect::new(0.0, 0.0, w, chrome::TITLE_H)
}

fn caption(w: f32, b: WinButton) -> Rect {
    let n = match b {
        WinButton::Close => 0,
        WinButton::Max => 1,
        WinButton::Min => 2,
    };
    chrome::win_button_rect(w, n)
}

const HELLO: Rect = Rect::new(24.0, 72.0, 140.0, 32.0);
const FIELD: Rect = Rect::new(24.0, 128.0, 320.0, 32.0);
const FOOTER: Rect = Rect::new(24.0, 180.0, 600.0, 24.0);

/// The band the tooltip may cover, under or over the button.
fn tip_band(w: f32) -> Rect {
    Rect::new(0.0, HELLO.y - 40.0, w, HELLO.h + 80.0)
}

fn field_inner() -> Rect {
    FIELD.inset(8.0)
}

impl Demo {
    fn target(&self, w: f32, x: f32, y: f32) -> Option<Target> {
        for b in [WinButton::Min, WinButton::Max, WinButton::Close] {
            if caption(w, b).contains(x, y) {
                return Some(Target::Caption(b));
            }
        }
        if title_row(w).contains(x, y) {
            Some(Target::Title)
        } else if HELLO.contains(x, y) {
            Some(Target::Hello)
        } else if FIELD.contains(x, y) {
            Some(Target::Field)
        } else {
            None
        }
    }

    fn rect_of(t: Target, w: f32) -> Rect {
        match t {
            Target::Title => title_row(w),
            Target::Caption(b) => caption(w, b),
            Target::Hello => HELLO,
            Target::Field => FIELD,
        }
    }

    fn redraw(cx: &mut Cx<'_, Msg>, t: Option<Target>) {
        if let Some(t) = t {
            let w = cx.size().0;
            cx.request_redraw(Self::rect_of(t, w));
        }
    }

    /// Asks again for whichever timer is still to come: the host gives one wake-up, the
    /// earliest asked for.
    fn timers(&self, cx: &mut Cx<'_, Msg>) {
        if let Some(t) = self.until {
            cx.wake_at(t);
        }
        if let (Some(Target::Hello), Some(since), false) = (self.hover, self.hover_since, self.tip)
        {
            cx.wake_at(since + TIP_DELAY);
        }
    }

    fn hide_tip(&mut self, cx: &mut Cx<'_, Msg>) {
        if self.tip {
            self.tip = false;
            let w = cx.size().0;
            cx.request_redraw(tip_band(w));
        }
    }
}

impl App for Demo {
    type Message = Msg;

    fn start(&mut self, cx: &mut Cx<'_, Msg>) {
        let proxy = cx.proxy();
        std::thread::spawn(move || {
            let mut n = 0;
            loop {
                std::thread::sleep(Duration::from_secs(1));
                n += 1;
                if !proxy.send(Msg::Tick(n)) {
                    break;
                }
            }
        });
        cx.set_backdrop(Backdrop::Mica, &self.pal);
        self.blink = platform::caret_blink_time();
        if let Some(s) = self.seconds {
            self.until = Some(cx.now() + Duration::from_secs(s));
        }
        self.timers(cx);
        println!(
            "start: scale {}, size {:?}, caret blink {:?}",
            cx.scale(),
            cx.size(),
            self.blink
        );
    }

    fn event(&mut self, cx: &mut Cx<'_, Msg>, event: Event) {
        let w = cx.size().0;
        match event {
            Event::PointerMove { x, y } => {
                let t = self.target(w, x, y);
                if t != self.hover {
                    Self::redraw(cx, self.hover);
                    Self::redraw(cx, t);
                    self.hide_tip(cx);
                    self.hover = t;
                    self.hover_since = (t == Some(Target::Hello)).then(|| cx.now());
                    self.timers(cx);
                }
                cx.set_cursor(if t == Some(Target::Field) {
                    Cursor::Text
                } else {
                    Cursor::Default
                });
            }
            Event::PointerLeave => {
                Self::redraw(cx, self.hover);
                self.hide_tip(cx);
                self.hover = None;
            }
            Event::Press {
                x,
                y,
                button: Button::Left,
                clicks,
            } => {
                let t = self.target(w, x, y);
                self.hide_tip(cx);
                let was = self.field_focused;
                self.field_focused = t == Some(Target::Field);
                if was != self.field_focused {
                    cx.request_redraw(FIELD);
                }
                match t {
                    Some(Target::Title) if clicks == 2 => cx.title_double_click(),
                    Some(Target::Title) => cx.drag_window(),
                    Some(Target::Field) => {
                        let inner = field_inner();
                        self.field.press(x - inner.x, &self.stops, cx.mods().shift);
                        cx.request_redraw(FIELD);
                    }
                    Some(t) => {
                        self.pressed = Some(t);
                        Self::redraw(cx, Some(t));
                    }
                    None => {}
                }
            }
            Event::Release {
                button: Button::Left,
                ..
            } => {
                let Some(t) = self.pressed.take() else {
                    return;
                };
                Self::redraw(cx, Some(t));
                if self.hover != Some(t) {
                    return;
                }
                match t {
                    Target::Caption(WinButton::Close) => cx.exit(),
                    Target::Caption(WinButton::Max) => cx.toggle_maximize(),
                    Target::Caption(WinButton::Min) => cx.minimize(),
                    Target::Hello => {
                        self.hellos += 1;
                        println!("hello {}", self.hellos);
                        cx.request_redraw(FOOTER);
                    }
                    _ => {}
                }
            }
            Event::Key(k) if self.field_focused => {
                if let Some(name) = &k.name
                    && self
                        .field
                        .handle_key(name, k.mods, k.text.as_deref(), true)
                        .is_some()
                {
                    cx.request_redraw(FIELD);
                } else if k.name == Some(mdshared_ui::keys::KeyName::Escape) {
                    self.field_focused = false;
                    cx.request_redraw(FIELD);
                }
            }
            Event::Text(t) if self.field_focused => {
                if self.field.insert(&t) {
                    cx.request_redraw(FIELD);
                }
            }
            Event::Wake(now) => {
                if self.until.is_some_and(|u| u <= now) {
                    println!(
                        "wake: {} seconds are up, closing",
                        self.seconds.unwrap_or(0)
                    );
                    cx.exit();
                    return;
                }
                if self.hover == Some(Target::Hello)
                    && self.hover_since.is_some_and(|s| s + TIP_DELAY <= now)
                    && !self.tip
                {
                    println!("wake: tooltip");
                    self.tip = true;
                    cx.request_redraw(tip_band(w));
                }
                self.timers(cx);
            }
            Event::Focus(f) => {
                println!("focus {f}");
                cx.request_redraw_all();
            }
            Event::Resize { w, h } => println!("resize {w} x {h}"),
            Event::Scale(s) => println!("scale {s}"),
            Event::CloseRequested => {
                println!("close requested");
                cx.exit();
            }
            #[cfg(feature = "a11y")]
            Event::Action(req) => {
                println!("a11y action {:?} on {:?}", req.action, req.target_node);
                if req.action == accesskit::Action::Click && req.target_node == HELLO_NODE {
                    self.hellos += 1;
                    cx.request_redraw(FOOTER);
                }
            }
            _ => {}
        }
    }

    fn message(&mut self, cx: &mut Cx<'_, Msg>, m: Msg) {
        let Msg::Tick(n) = m;
        self.ticks = n;
        println!("message: tick {n}");
        cx.request_redraw(FOOTER);
    }

    fn draw(&mut self, cx: &mut Cx<'_, Msg>, p: &mut Painter, damage: Rect) {
        self.frames += 1;
        println!(
            "frame {}: drew {:.0},{:.0} {:.0}x{:.0}",
            self.frames, damage.x, damage.y, damage.w, damage.h
        );
        let pal = self.pal;
        let (w, h) = cx.size();
        p.fill(damage, pal.page);

        // The title row: flat, then marked for the backdrop, then the buttons over it.
        let title = title_row(w);
        p.fill(title, pal.title);
        cx.frost(p, title);
        p.text(
            Face::Semibold,
            13.0,
            16.0,
            title.cy(),
            "mdshared-ui host",
            pal.text,
            w / 2.0,
        );
        for b in [WinButton::Min, WinButton::Max, WinButton::Close] {
            let t = Some(Target::Caption(b));
            chrome::draw_win_button(
                p,
                &pal,
                b,
                caption(w, b),
                cx.maximized(),
                self.hover == t,
                self.pressed == t,
            );
        }
        // Snap layouts over the maximize button, less the resize band at the top.
        let mut max = caption(w, WinButton::Max);
        if !cx.maximized() {
            max.y += window::EDGE;
            max.h -= window::EDGE;
        }
        cx.set_maximize_button(Some(max));

        let fill = match (self.hover, self.pressed) {
            (Some(Target::Hello), Some(Target::Hello)) => pal.press,
            (Some(Target::Hello), _) => pal.hover,
            _ => pal.bar,
        };
        p.rbox(HELLO, 4.0, fill);
        p.text_center(Face::Ui, 14.0, HELLO, "Say hello", pal.text);

        p.rbox(FIELD, 4.0, pal.field);
        let inner = field_inner();
        let colors = FieldColors {
            text: pal.text,
            placeholder: pal.muted,
            selection: pal.selection,
            caret: pal.text,
        };
        let focused = self.field_focused && cx.focused();
        self.field.draw_blinking(
            p,
            cx.anim(),
            "field",
            self.blink,
            inner,
            Face::Ui,
            14.0,
            &colors,
            "Type here",
            focused,
        );
        self.stops = p.caret_stops(Face::Ui, 14.0, &self.field.display());
        if focused {
            // The caret's blink redraws the field only.
            cx.animating(FIELD);
        }

        let footer = format!(
            "worker ticks: {}, hellos: {}, frames drawn: {}",
            self.ticks, self.hellos, self.frames
        );
        p.text(
            Face::Ui,
            13.0,
            FOOTER.x,
            FOOTER.cy(),
            &footer,
            pal.muted,
            FOOTER.w,
        );

        if self.tip {
            Bubble::tip(p, TIP_TEXT, HELLO, w, h).draw(p, &pal);
        }
    }

    #[cfg(feature = "a11y")]
    fn accessibility(&mut self, cx: &mut Cx<'_, Msg>) -> accesskit::TreeUpdate {
        use accesskit::{Action, Node, Role, TreeId, TreeInfo, TreeUpdate};
        let s = cx.scale();
        let mut root = Node::new(Role::Window);
        root.set_label(cx.title().to_string());
        root.set_children(vec![HELLO_NODE, FIELD_NODE]);
        let mut b = Node::new(Role::Button);
        b.set_label("Say hello");
        b.set_author_id("Hello");
        b.set_bounds(host::access_rect(HELLO, s));
        b.add_action(Action::Click);
        let mut f = Node::new(Role::TextInput);
        f.set_label("Text");
        f.set_author_id("Field");
        f.set_value(self.field.text().to_string());
        f.set_bounds(host::access_rect(FIELD, s));
        let mut info = TreeInfo::new(ROOT_NODE);
        info.toolkit_name = Some("mdshared-ui".into());
        TreeUpdate {
            nodes: vec![(ROOT_NODE, root), (HELLO_NODE, b), (FIELD_NODE, f)],
            tree: Some(info),
            tree_id: TreeId::ROOT,
            focus: if self.field_focused {
                FIELD_NODE
            } else {
                ROOT_NODE
            },
        }
    }
}

#[cfg(feature = "a11y")]
const ROOT_NODE: accesskit::NodeId = accesskit::NodeId(1);
#[cfg(feature = "a11y")]
const HELLO_NODE: accesskit::NodeId = accesskit::NodeId(2);
#[cfg(feature = "a11y")]
const FIELD_NODE: accesskit::NodeId = accesskit::NodeId(3);

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
        field_focused: true,
        stops: Vec::new(),
        blink: None,
        hover: None,
        hover_since: None,
        tip: false,
        pressed: None,
        hellos: 0,
        ticks: 0,
        frames: 0,
        until: None,
        seconds,
    };
    let config = Config::new("mdshared-ui host")
        .size(720.0, 420.0)
        .min_size(480.0, 300.0)
        .app_id("mdshared-host");
    let started = Instant::now();
    if let Err(e) = host::run(config, demo) {
        eprintln!("host: {e}");
        std::process::exit(1);
    }
    println!("ended after {:.1} s", started.elapsed().as_secs_f64());
}
