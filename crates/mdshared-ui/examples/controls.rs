//! Everyday controls at work: the four kinds of push button and their states, an icon button with
//! a tooltip, a menu button, a split button and a framed text field with its icon, clear button
//! and error state.
//! `cargo run -p mdshared-ui --features host --example controls [-- --seconds N]`, and with
//! `--features host,a11y` its accessibility tree too.
//!
//! Tab moves the keyboard between the controls; Enter or Space presses the one with the keyboard
//! (a menu button opens its menu); Down and Alt+Down open a menu button's or a split button's
//! menu; Escape closes a menu, or clears the field and leaves it. The pointer hovers, presses and
//! clicks; the clear button empties the field and the keyboard stays on it; typing in the field
//! and pressing Validate with it empty shows the error. Each frame and each thing a person does
//! is printed, so a run with nobody looking still shows the damage, the wake-ups and the
//! instructions. `--seconds N` closes it after N seconds by a timer of the host.

use std::time::{Duration, Instant};

#[cfg(feature = "a11y")]
use mdshared_ui::a11y;
use mdshared_ui::controls::button::{self, Button, Kind, Look as ButtonLook};
use mdshared_ui::controls::field::{self, FramedField};
use mdshared_ui::controls::icon_button::{self, IconButton};
use mdshared_ui::controls::menu_button::{self, Half, MenuButton, SplitButton, SplitLook};
use mdshared_ui::focus::Focus;
use mdshared_ui::gfx::{Face, Painter, Rect};
use mdshared_ui::host::{self, App, Config, Cx, Event};
use mdshared_ui::keys::KeyName;
use mdshared_ui::menu::{Item, Menu};
use mdshared_ui::overlay::Bubble;
use mdshared_ui::platform;
use mdshared_ui::theme::{DARK, LIGHT, Palette};
use mdshared_ui::tooltip::Tooltip;

const W: f32 = 760.0;
const H: f32 = 560.0;
const X0: f32 = 24.0;
const KINDS_Y: f32 = 48.0;
const STATES_Y: f32 = 96.0;
const ROW_Y: f32 = 150.0;
const FIELD_Y: f32 = 206.0;
const FOOTER_Y: f32 = 500.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Target {
    Button(usize),
    Icon,
    Menu,
    Split(Half),
    Field,
    Clear,
    Validate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Cmd {
    ByName,
    BySize,
    RunAll,
    RunCurrent,
}

struct Demo {
    pal: Palette,
    focus: Focus<Target>,
    buttons: [Button; 4],
    icon: IconButton,
    menu_button: MenuButton,
    split: SplitButton,
    field: FramedField,
    /// Where each control was drawn last frame, for the hit test before the next one. The clear
    /// button is in it before the field, so it takes its presses.
    hits: Vec<(Target, Rect)>,
    /// The text field's caret stops, from the last frame (the painter measures them).
    stops: Vec<f32>,
    blink: Option<Duration>,
    hover: Option<Target>,
    pressed: Option<Target>,
    tip: Tooltip<Target>,
    menu: Option<Menu<Cmd>>,
    menu_from: Option<Target>,
    clipless: u32,
    seconds: Option<u64>,
    until: Option<Instant>,
    frames: u32,
}

fn texts() -> [(&'static str, Kind); 4] {
    [
        ("Save", Kind::Primary),
        ("Cancel", Kind::Secondary),
        ("Details", Kind::Subtle),
        ("Delete", Kind::Destructive),
    ]
}

impl Demo {
    fn new(seconds: Option<u64>) -> Demo {
        let kinds = texts();
        let mut focus = Focus::new();
        focus.focus(Target::Button(0));
        Demo {
            pal: LIGHT,
            focus,
            buttons: kinds.map(|(text, kind)| Button::new(text, kind)),
            icon: IconButton::new(mdshared_ui::glyph::SEARCH).tip("Search (Ctrl+F)"),
            menu_button: MenuButton::new("Sort by", Kind::Secondary),
            split: SplitButton::new("Run", Kind::Primary),
            field: {
                let mut f = FramedField::new()
                    .with_icon(mdshared_ui::glyph::SEARCH)
                    .with_placeholder("Search notes");
                // A search box is the one place Escape means "clear", so it is turned on here.
                f.set_escape_clears(true);
                f
            },
            hits: Vec::new(),
            stops: Vec::new(),
            blink: platform::caret_blink_time(),
            hover: None,
            pressed: None,
            tip: Tooltip::new(),
            menu: None,
            menu_from: None,
            clipless: 0,
            seconds,
            until: None,
            frames: 0,
        }
    }

    fn say(&mut self, what: &str) {
        println!("{what}");
    }

    fn target_at(&self, x: f32, y: f32) -> Option<Target> {
        self.hits
            .iter()
            .find(|(_, r)| r.contains(x, y))
            .map(|&(t, _)| t)
    }

    fn open_menu(&mut self, from: Target, anchor: Rect, items: Vec<Item<Cmd>>) {
        println!("open the menu from {from:?} at {anchor:?}");
        let mut menu = menu_button::menu_under(Menu::new(items, 0.0, 0.0, false), anchor);
        menu.highlight(0);
        self.menu = Some(menu);
        self.menu_from = Some(from);
        self.tip.hide();
    }

    fn choose(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::ByName => self.say("sort by name"),
            Cmd::BySize => self.say("sort by size"),
            Cmd::RunAll => self.say("run all"),
            Cmd::RunCurrent => self.say("run the current one"),
        }
        self.menu = None;
        self.menu_from = None;
    }

    fn activate(&mut self, t: Target) {
        match t {
            Target::Button(i) => {
                self.clipless += 1;
                println!("button \"{}\" pressed ({})", texts()[i].0, self.clipless);
            }
            Target::Icon => {
                self.clipless += 1;
                println!("the search icon was pressed ({})", self.clipless);
            }
            Target::Menu => {
                let anchor = self
                    .hits
                    .iter()
                    .find(|(t, _)| *t == Target::Menu)
                    .map(|&(_, r)| r)
                    .unwrap_or_default();
                self.open_menu(
                    Target::Menu,
                    anchor,
                    vec![
                        Item::new(Cmd::ByName, "Name"),
                        Item::new(Cmd::BySize, "Size"),
                    ],
                );
            }
            Target::Split(Half::Main) => self.say("run the current search"),
            Target::Split(Half::Chevron) => {
                let anchor = self
                    .hits
                    .iter()
                    .find(|(t, _)| *t == Target::Split(Half::Chevron))
                    .map(|_| self.split_rect())
                    .unwrap_or_default();
                self.open_menu(
                    Target::Split(Half::Chevron),
                    anchor,
                    vec![
                        Item::new(Cmd::RunAll, "Run all"),
                        Item::new(Cmd::RunCurrent, "Run the current one"),
                    ],
                );
            }
            Target::Validate => {
                if self.field.is_empty() {
                    self.field.set_error("Type something to search for");
                    self.say("validate: empty, the error shows");
                } else {
                    self.field.clear_error();
                    println!("validate: searching for \"{}\"", self.field.text());
                }
            }
            Target::Field | Target::Clear => {}
        }
    }

    fn split_rect(&self) -> Rect {
        self.hits
            .iter()
            .filter(|(t, _)| matches!(t, Target::Split(_)))
            .fold(Rect::default(), |acc, &(_, r)| {
                if acc.w == 0.0 {
                    r
                } else {
                    Rect::new(acc.x, acc.y, acc.right() - acc.x, acc.h)
                }
            })
    }

    fn field_rect(&self) -> Rect {
        self.hits
            .iter()
            .find(|(t, _)| *t == Target::Field)
            .map(|&(_, r)| r)
            .unwrap_or_default()
    }

    fn tip_anchor(&self) -> Option<Rect> {
        self.hits
            .iter()
            .find(|(t, _)| *t == Target::Icon)
            .map(|&(_, r)| r)
    }

    /// Asks for the next wake the tooltip or the `--seconds` timer needs.
    fn timers<M>(&mut self, cx: &mut Cx<'_, M>) {
        if let Some(at) = self.tip.deadline() {
            cx.wake_at(at);
        }
        if let Some(at) = self.until {
            cx.wake_at(at);
        }
    }
}

impl App for Demo {
    type Message = ();

    fn start(&mut self, cx: &mut Cx<'_, ()>) {
        if let Some(s) = self.seconds {
            self.until = Some(cx.now() + Duration::from_secs(s));
            println!("closing after {s} seconds");
        }
        self.timers(cx);
    }

    fn event(&mut self, cx: &mut Cx<'_, ()>, e: Event) {
        match e {
            Event::PointerMove { x, y } => {
                let t = self.target_at(x, y);
                if t != self.hover {
                    println!("pointer over {t:?}");
                    self.hover = t;
                    cx.request_redraw_all();
                }
                let over = match t {
                    Some(Target::Icon) => {
                        self.tip_anchor().map(|r| self.icon.over(Target::Icon, r))
                    }
                    _ => None,
                };
                if self.tip.hover_at(cx.now(), over, (x, y)) {
                    println!(
                        "tooltip shown: {:?}",
                        self.tip.shown().map(|o| o.text.clone())
                    );
                    cx.request_redraw_all();
                }
                self.timers(cx);
            }
            Event::PointerLeave => {
                self.hover = None;
                self.tip.hide();
                cx.request_redraw_all();
            }
            Event::Press { x, y, button, .. } => {
                if button != host::Button::Left {
                    return;
                }
                if let Some(menu) = self.menu.as_mut() {
                    if menu.rect().contains(x, y) {
                        menu.pointer(x, y);
                        cx.request_redraw_all();
                        return;
                    }
                    println!("a press outside the menu closes it");
                    self.menu = None;
                    self.menu_from = None;
                }
                self.tip.hide();
                if self.field_rect().contains(x, y) {
                    self.focus.focus(Target::Field);
                    self.focus.pointer_used();
                    let r = self.field_rect();
                    match self.field.press(r, x, y) {
                        field::Hit::Cleared => {
                            self.say("the clear button emptied the field; it keeps the keyboard");
                            self.pressed = None;
                        }
                        field::Hit::Text => {
                            self.pressed = Some(Target::Field);
                            let area = self.field.text_rect(r);
                            let stops = self.stops.clone();
                            self.field.field_mut().press(x - area.x, &stops, false);
                        }
                        field::Hit::Outside => {}
                    }
                    cx.request_redraw_all();
                    return;
                }
                let t = self.target_at(x, y);
                self.pressed = t;
                if let Some(t) = t {
                    self.focus.focus(t);
                    self.focus.pointer_used();
                }
                cx.request_redraw_all();
            }
            Event::Release { x, y, button } => {
                if button != host::Button::Left {
                    return;
                }
                if let Some(menu) = self.menu.as_mut()
                    && menu.rect().contains(x, y)
                {
                    if let Some(cmd) = menu.chosen() {
                        self.choose(cmd);
                    }
                    cx.request_redraw_all();
                    return;
                }
                let t = self.target_at(x, y);
                if let (Some(pressed), Some(released)) = (self.pressed, t)
                    && pressed == released
                {
                    self.activate(released);
                }
                self.pressed = None;
                cx.request_redraw_all();
            }
            Event::Key(k) => {
                self.tip.hide();
                if let Some(menu) = self.menu.as_mut() {
                    match k.name {
                        Some(KeyName::Down) => {
                            menu.step(true);
                        }
                        Some(KeyName::Up) => {
                            menu.step(false);
                        }
                        Some(KeyName::Enter) | Some(KeyName::Space) => {
                            if let Some(cmd) = menu.chosen() {
                                self.choose(cmd);
                            }
                        }
                        Some(KeyName::Escape) | Some(KeyName::Left) => {
                            println!("the menu closes");
                            self.menu = None;
                            self.menu_from = None;
                        }
                        _ => {}
                    }
                    cx.request_redraw_all();
                    return;
                }
                if k.name == Some(KeyName::Tab) {
                    self.focus.tab(k.mods.shift);
                    println!("keyboard on {:?}", self.focus.focused());
                    cx.request_redraw_all();
                    return;
                }
                if self.focus.is_focused(Target::Field)
                    && let Some(name) = &k.name
                {
                    let outcome = self.field.key(name, k.mods, k.text.as_deref(), true);
                    match outcome {
                        field::Outcome::Cleared => {
                            println!("Escape cleared the field; it keeps the keyboard");
                        }
                        field::Outcome::Used(_) => {}
                        field::Outcome::Unused => {
                            if *name == KeyName::Escape {
                                self.focus.blur();
                                println!("Escape left the field");
                            }
                        }
                    }
                    cx.request_redraw_all();
                    return;
                }
                let Some(t) = self.focus.focused() else {
                    return;
                };
                let Some(name) = &k.name else { return };
                match t {
                    Target::Menu => {
                        if menu_button::key(name, k.mods) != menu_button::Press::None {
                            self.activate(t);
                        }
                    }
                    Target::Split(half) => match self.split.key(name, k.mods) {
                        menu_button::Press::Activate => self.activate(Target::Split(Half::Main)),
                        menu_button::Press::Open => self.activate(Target::Split(Half::Chevron)),
                        menu_button::Press::None => {
                            let _ = half;
                        }
                    },
                    _ => {
                        if button::key(name, k.mods) {
                            self.activate(t);
                        }
                    }
                }
                cx.request_redraw_all();
            }
            Event::Text(text) => {
                if self.focus.is_focused(Target::Field) {
                    self.field.field_mut().insert(&text);
                    cx.request_redraw_all();
                }
            }
            Event::Wake(now) => {
                if self.until.is_some_and(|u| u <= now) {
                    println!("wake: the seconds are up, closing");
                    cx.exit();
                    return;
                }
                if self.tip.tick(now) {
                    println!("wake: tooltip shown");
                    cx.request_redraw_all();
                }
                self.timers(cx);
            }
            Event::Theme { dark } => {
                self.pal = if dark { DARK } else { LIGHT };
                println!("theme: {}", if dark { "dark" } else { "light" });
                cx.request_redraw_all();
            }
            Event::Focus(f) => {
                println!("window focus {f}");
                cx.request_redraw_all();
            }
            Event::CloseRequested => {
                println!("close requested");
                cx.exit();
            }
            #[cfg(feature = "a11y")]
            Event::Action(req) => {
                println!("a11y action {:?} on {:?}", req.action, req.target_node);
                if req.action == accesskit::Action::Click {
                    let t = [
                        (a11y::id("Save"), Target::Button(0)),
                        (a11y::id("Cancel"), Target::Button(1)),
                        (a11y::id("Details"), Target::Button(2)),
                        (a11y::id("Delete"), Target::Button(3)),
                        (a11y::id("Search"), Target::Icon),
                        (a11y::id("Sort"), Target::Menu),
                        (a11y::id("Run"), Target::Split(Half::Main)),
                        (a11y::id("RunMenu"), Target::Split(Half::Chevron)),
                        (a11y::id("Validate"), Target::Validate),
                    ]
                    .into_iter()
                    .find(|(id, _)| *id == req.target_node)
                    .map(|(_, t)| t);
                    if let Some(t) = t {
                        self.activate(t);
                        cx.request_redraw_all();
                    }
                }
            }
            _ => {}
        }
    }

    fn draw(&mut self, cx: &mut Cx<'_, ()>, p: &mut Painter, damage: Rect) {
        self.frames += 1;
        println!(
            "frame {}: drew {:.0},{:.0} {:.0}x{:.0}",
            self.frames, damage.x, damage.y, damage.w, damage.h
        );
        let pal = self.pal;
        p.fill(damage, pal.page);
        let page = Rect::new(0.0, 0.0, W, H);
        self.focus.begin_frame();
        for i in 0..self.buttons.len() {
            self.focus.add(Target::Button(i));
        }
        for t in [
            Target::Icon,
            Target::Menu,
            Target::Split(Half::Main),
            Target::Field,
            Target::Validate,
        ] {
            self.focus.add(t);
        }
        self.hits.clear();

        // The four kinds, in a row, with the keyboard on the first and that one the default.
        for i in 0..self.buttons.len() {
            let b = &self.buttons[i];
            let r = b.rect(p, X0 + 150.0 * i as f32, KINDS_Y);
            self.hits.push((Target::Button(i), r));
            let look = ButtonLook {
                hovered: self.hover == Some(Target::Button(i)),
                pressed: self.pressed == Some(Target::Button(i)),
                focused: self.focus.ring_visible(Target::Button(i)),
                default: i == 0,
                disabled: false,
            };
            button::draw(p, &pal, r, b.text(), b.kind(), look);
            if i == 3 {
                p.text(
                    Face::Ui,
                    12.0,
                    X0 + 150.0 * i as f32,
                    KINDS_Y - 14.0,
                    "the four kinds",
                    pal.muted,
                    300.0,
                );
            }
        }
        // The states, on a secondary button: rest, pointer, press, keyboard, disabled.
        let states: [(&str, ButtonLook); 5] = [
            ("rest", ButtonLook::default()),
            (
                "pointer",
                ButtonLook {
                    hovered: true,
                    ..ButtonLook::default()
                },
            ),
            (
                "press",
                ButtonLook {
                    hovered: true,
                    pressed: true,
                    ..ButtonLook::default()
                },
            ),
            (
                "keyboard",
                ButtonLook {
                    focused: true,
                    ..ButtonLook::default()
                },
            ),
            (
                "disabled",
                ButtonLook {
                    disabled: true,
                    ..ButtonLook::default()
                },
            ),
        ];
        for (k, (name, look)) in states.into_iter().enumerate() {
            let r = Rect::new(X0 + 132.0 * k as f32, STATES_Y, 120.0, button::HEIGHT);
            button::draw(p, &pal, r, name, Kind::Secondary, look);
        }

        // The icon button, the menu button and the split button.
        let ir = self.icon.rect(X0, ROW_Y);
        self.hits.push((Target::Icon, ir));
        icon_button::draw(
            p,
            &pal,
            ir,
            self.icon.icon(),
            ButtonLook {
                hovered: self.hover == Some(Target::Icon),
                pressed: self.pressed == Some(Target::Icon),
                focused: self.focus.ring_visible(Target::Icon),
                default: false,
                disabled: false,
            },
        );
        let mr = self.menu_button.rect(p, X0 + 48.0, ROW_Y);
        self.hits.push((Target::Menu, mr));
        menu_button::draw(
            p,
            &pal,
            mr,
            self.menu_button.text(),
            self.menu_button.kind(),
            ButtonLook {
                hovered: self.hover == Some(Target::Menu),
                pressed: self.menu_from == Some(Target::Menu),
                focused: self.focus.ring_visible(Target::Menu),
                default: false,
                disabled: false,
            },
        );
        let sr = self.split.rect(p, X0 + 200.0, ROW_Y);
        let halves = self.split.halves(sr);
        self.hits.push((Target::Split(Half::Main), halves.main));
        self.hits
            .push((Target::Split(Half::Chevron), halves.chevron));
        menu_button::draw_split(
            p,
            &pal,
            sr,
            self.split.text(),
            self.split.kind(),
            SplitLook {
                main: ButtonLook {
                    hovered: self.hover == Some(Target::Split(Half::Main)),
                    pressed: self.pressed == Some(Target::Split(Half::Main)),
                    focused: self.focus.ring_visible(Target::Split(Half::Main)),
                    default: false,
                    disabled: false,
                },
                chevron: ButtonLook {
                    hovered: self.hover == Some(Target::Split(Half::Chevron)),
                    pressed: self.pressed == Some(Target::Split(Half::Chevron))
                        || self.menu_from == Some(Target::Split(Half::Chevron)),
                    focused: self.focus.ring_visible(Target::Split(Half::Chevron)),
                    default: false,
                    disabled: false,
                },
            },
        );
        // The framed field with its icon and clear button, and the error under it.
        let fr = Rect::new(X0, FIELD_Y, 380.0, field::HEIGHT);
        // The clear button is in the hits before the field, so a point on it is its own.
        if let Some(c) = self.field.clear_rect(fr) {
            self.hits.push((Target::Clear, c));
        }
        self.hits.push((Target::Field, fr));
        let field_look = field::Look {
            focused: self.focus.is_focused(Target::Field) && cx.focused(),
            hovered_clear: self.hover == Some(Target::Clear),
            pressed_clear: self.pressed == Some(Target::Clear),
            disabled: false,
        };
        field::draw_blinking(
            p,
            &pal,
            cx.anim(),
            "search",
            self.blink,
            fr,
            &mut self.field,
            field_look,
        );
        self.stops = p.caret_stops(Face::Ui, field::TEXT, &self.field.field().display());
        if field_look.focused {
            cx.animating(fr);
        }
        let vr = Rect::new(X0 + 396.0, FIELD_Y, 96.0, button::HEIGHT);
        self.hits.push((Target::Validate, vr));
        button::draw(
            p,
            &pal,
            vr,
            "Validate",
            Kind::Primary,
            ButtonLook {
                hovered: self.hover == Some(Target::Validate),
                pressed: self.pressed == Some(Target::Validate),
                focused: self.focus.ring_visible(Target::Validate),
                default: false,
                disabled: false,
            },
        );

        let help = format!(
            "Tab moves, Enter/Space presses, Down opens a menu, Escape clears the field. \
             Keyboard: {:?}. Frames: {}",
            self.focus.focused(),
            self.frames
        );
        p.text(Face::Ui, 12.0, X0, FOOTER_Y, &help, pal.muted, W - 2.0 * X0);

        // The menu, over everything, placed around its button.
        if let Some(menu) = self.menu.as_mut() {
            menu.draw_in(p, &pal, page);
        }
        // And the tooltip, while the pointer rests on the icon button.
        if let Some(over) = self.tip.shown() {
            let text = over.text.clone();
            let anchor = over.anchor;
            Bubble::tip(p, &text, anchor, W, H).draw(p, &pal);
        }
        self.focus.end_frame();
    }

    #[cfg(feature = "a11y")]
    fn accessibility(&mut self, cx: &mut Cx<'_, ()>, tree: &mut a11y::Tree) -> accesskit::NodeId {
        let root = tree.root();
        let ids = [
            a11y::id("Save"),
            a11y::id("Cancel"),
            a11y::id("Details"),
            a11y::id("Delete"),
            a11y::id("Search"),
            a11y::id("Sort"),
            a11y::id("Run"),
            a11y::id("RunMenu"),
            a11y::id("Field"),
            a11y::id("Clear"),
            a11y::id("Validate"),
        ];
        tree.add(root, a11y::window(cx.title(), cx.scale(), ids.to_vec()));
        for (i, b) in self.buttons.iter().enumerate() {
            let r = self
                .hits
                .iter()
                .find(|(t, _)| *t == Target::Button(i))
                .map(|&(_, r)| r)
                .unwrap_or_default();
            tree.add(ids[i], a11y::button(b.text(), i_name(i), r, false));
        }
        let rect_of = |target: Target| {
            self.hits
                .iter()
                .find(|(t, _)| *t == target)
                .map(|&(_, r)| r)
                .unwrap_or_default()
        };
        tree.add(
            a11y::id("Search"),
            a11y::button(self.icon.tip_text(), "Search", rect_of(Target::Icon), false),
        );
        tree.add(
            a11y::id("Sort"),
            a11y::button("Sort by", "Sort", rect_of(Target::Menu), false),
        );
        tree.add(
            a11y::id("Run"),
            a11y::button("Run", "Run", rect_of(Target::Split(Half::Main)), false),
        );
        tree.add(
            a11y::id("RunMenu"),
            a11y::button(
                "Run options",
                "RunMenu",
                rect_of(Target::Split(Half::Chevron)),
                false,
            ),
        );
        tree.add(
            a11y::id("Field"),
            a11y::text_input(
                "Search notes",
                "Field",
                rect_of(Target::Field),
                self.field.text(),
                false,
            ),
        );
        tree.add(
            a11y::id("Clear"),
            a11y::button(
                "Clear",
                "Clear",
                rect_of(Target::Clear),
                self.field.is_empty(),
            ),
        );
        tree.add(
            a11y::id("Validate"),
            a11y::button("Validate", "Validate", rect_of(Target::Validate), false),
        );
        a11y::id("Field")
    }
}

#[cfg(feature = "a11y")]
fn i_name(i: usize) -> &'static str {
    ["Save", "Cancel", "Details", "Delete"][i]
}

fn main() {
    let mut seconds = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        if a == "--seconds" {
            seconds = args.next().and_then(|s| s.parse().ok());
        }
    }
    let config = Config::new("Everyday controls").size(W, H);
    if let Err(e) = host::run(config, Demo::new(seconds)) {
        eprintln!("host: {e}");
        std::process::exit(1);
    }
}
