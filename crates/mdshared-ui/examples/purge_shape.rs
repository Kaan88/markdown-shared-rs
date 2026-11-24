//! The shape of Purge's window, the consumer that lets the Purge pieces reach `main` (roadmap,
//! "Purge (fast track)"): `cargo run -p mdshared-ui --features a11y --example purge_shape`.
//!
//! This is the W8 spike: it holds just enough to prove that a UI Automation client can
//! - find elements by AutomationId (AccessKit's author id),
//! - Invoke a button and Toggle a check box,
//! - SelectionItem-select a row of a virtualized 2,000-row list, and
//! - ScrollItem-scroll such a row into view.
//!
//! `tests/uia/purge_shape.ps1` is that client. The window grows into the full shape later
//! (search field, grouped list, dialog). Each action the window receives is printed on stdout.
//!
//! The list realizes only the rows on screen, plus one screen above and below, as UIA nodes.
//! The list node carries the full row count (`size_of_set`) and each row its position
//! (`position_in_set`, 0-based in AccessKit, 1-based in UIA), so each row reports "5 of 2,000". AccessKit has no UIA ItemContainer or
//! Scroll pattern, so a client reaches a far row by ScrollItem-ing the last realized row again
//! and again. Each step realizes the next band.

#[cfg(not(windows))]
fn main() {
    println!("purge_shape is for Windows (UI Automation) so far");
}

#[cfg(windows)]
fn main() {
    shape::run();
}

#[cfg(windows)]
mod shape {
    use std::num::NonZeroU32;
    use std::rc::Rc;
    use std::sync::{Arc, Mutex};

    use accesskit::{
        Action, ActionRequest, ActivationHandler, Node, NodeId, Rect as AkRect, Role, Toggled,
        TreeId, TreeInfo, TreeUpdate,
    };
    use mdshared_text::bitmap::Bitmap;
    use mdshared_ui::check::{Tick, draw_tick};
    use mdshared_ui::gfx::{Face, Gfx, Painter, Rect};
    use mdshared_ui::keys::Mods;
    use mdshared_ui::rows::{Nav, Rows};
    use mdshared_ui::select::Selection;
    use mdshared_ui::slider::draw_progress;
    use mdshared_ui::theme::{LIGHT, Palette};
    use winit::application::ApplicationHandler;
    use winit::dpi::LogicalSize;
    use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
    use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
    use winit::keyboard::{Key, NamedKey};
    use winit::window::{Window, WindowId};

    const COUNT: usize = 2_000;
    const ROW_H: f32 = 40.0;
    const TOP: f32 = 56.0;
    const HEAD_H: f32 = 28.0;
    const FOOT_H: f32 = 28.0;

    const ROOT: NodeId = NodeId(1);
    const BUTTON: NodeId = NodeId(2);
    const CHECK: NodeId = NodeId(3);
    const PROGRESS: NodeId = NodeId(4);
    const LIST: NodeId = NodeId(5);
    const FOOTER: NodeId = NodeId(6);
    const ROW0: u64 = 1_000;

    fn row_id(i: usize) -> NodeId {
        NodeId(ROW0 + i as u64)
    }

    #[derive(Clone, Copy, PartialEq)]
    enum Focus {
        Button,
        Check,
        List,
    }

    /// The window's model: everything drawn and everything in the accessibility tree comes
    /// from here.
    struct Model {
        size: (f32, f32),
        scale: f32,
        scroll: f32,
        sel: Selection,
        show_components: bool,
        invoked: u32,
        focus: Focus,
        pointer: (f32, f32),
        mods: Mods,
    }

    impl Model {
        fn button_rect(&self) -> Rect {
            Rect::new(16.0, 12.0, 120.0, 32.0)
        }
        fn check_rect(&self) -> Rect {
            Rect::new(152.0, 18.0, 20.0, 20.0)
        }
        fn progress_rect(&self) -> Rect {
            Rect::new(self.size.0 - 216.0, 26.0, 200.0, 4.0)
        }
        fn list_area(&self) -> Rect {
            let y = TOP + HEAD_H;
            Rect::new(0.0, y, self.size.0, (self.size.1 - y - FOOT_H).max(0.0))
        }
        fn rows(&self) -> Rows {
            let mut r = Rows::new(ROW_H, self.list_area().h, COUNT);
            r.scroll = self.scroll;
            r
        }
        /// The rows that are UIA nodes: the visible ones and one screen either side.
        fn realized(&self) -> std::ops::Range<usize> {
            let v = self.rows().visible();
            let page = v.len().max(1);
            v.start.saturating_sub(page)..(v.end + page).min(COUNT)
        }
        fn fraction(&self) -> f32 {
            (self.sel.len() as f32 / COUNT as f32).clamp(0.0, 1.0)
        }
        fn footer(&self) -> String {
            format!(
                "{} selected, uninstall pressed {} times, components {}",
                self.sel.len(),
                self.invoked,
                if self.show_components { "on" } else { "off" }
            )
        }

        fn ak_rect(&self, r: Rect) -> AkRect {
            let s = f64::from(self.scale);
            AkRect {
                x0: f64::from(r.x) * s,
                y0: f64::from(r.y) * s,
                x1: f64::from(r.x + r.w) * s,
                y1: f64::from(r.y + r.h) * s,
            }
        }

        fn focus_node(&self) -> NodeId {
            match self.focus {
                Focus::Button => BUTTON,
                Focus::Check => CHECK,
                Focus::List => match self.sel.cursor() {
                    Some(i) if self.realized().contains(&i) => row_id(i),
                    _ => LIST,
                },
            }
        }

        /// The whole tree. A full tree each time is simplest for the spike; at 2,000 rows only
        /// about 60 row nodes are realized, so it stays small.
        fn tree(&self) -> TreeUpdate {
            let mut nodes = Vec::new();
            let mut root = Node::new(Role::Window);
            root.set_label("Purge shape");
            root.set_children(vec![BUTTON, CHECK, PROGRESS, LIST, FOOTER]);
            nodes.push((ROOT, root));

            let mut b = Node::new(Role::Button);
            b.set_label("Uninstall");
            b.set_author_id("UninstallButton");
            b.set_bounds(self.ak_rect(self.button_rect()));
            b.add_action(Action::Click);
            b.add_action(Action::Focus);
            nodes.push((BUTTON, b));

            let mut c = Node::new(Role::CheckBox);
            c.set_label("Show components");
            c.set_author_id("ShowComponents");
            c.set_bounds(self.ak_rect(self.check_rect()));
            c.set_toggled(if self.show_components {
                Toggled::True
            } else {
                Toggled::False
            });
            c.add_action(Action::Click);
            c.add_action(Action::Focus);
            nodes.push((CHECK, c));

            let mut p = Node::new(Role::ProgressIndicator);
            p.set_label("Selected");
            p.set_author_id("SelectedProgress");
            p.set_bounds(self.ak_rect(self.progress_rect()));
            p.set_min_numeric_value(0.0);
            p.set_max_numeric_value(100.0);
            p.set_numeric_value(f64::from(self.fraction() * 100.0));
            nodes.push((PROGRESS, p));

            let area = self.list_area();
            let rows = self.rows();
            let mut list = Node::new(Role::ListBox);
            list.set_label("Programs");
            list.set_author_id("ProgramList");
            list.set_bounds(self.ak_rect(area));
            list.set_multiselectable();
            list.set_size_of_set(COUNT);
            list.add_action(Action::Focus);
            let band = self.realized();
            list.set_children(band.clone().map(row_id).collect::<Vec<_>>());
            nodes.push((LIST, list));
            for i in band {
                let mut r = Node::new(Role::ListBoxOption);
                r.set_label(format!("Program {:04}", i + 1));
                r.set_author_id(format!("Row{i}"));
                r.set_bounds(self.ak_rect(rows.rect(area, i)));
                // 0-based here; UIA's PositionInSet is this plus one. The set's size comes from
                // the list (AccessKit reads it from the item's container).
                r.set_position_in_set(i);
                r.set_selected(self.sel.contains(i));
                r.add_action(Action::Click);
                r.add_action(Action::ScrollIntoView);
                r.add_action(Action::Focus);
                nodes.push((row_id(i), r));
            }

            let mut f = Node::new(Role::Label);
            f.set_value(self.footer());
            f.set_author_id("FooterCount");
            nodes.push((FOOTER, f));

            let mut info = TreeInfo::new(ROOT);
            info.toolkit_name = Some("mdshared-ui".into());
            TreeUpdate {
                nodes,
                tree: Some(info),
                tree_id: TreeId::ROOT,
                focus: self.focus_node(),
            }
        }

        fn reveal(&mut self, i: usize) {
            self.scroll = self.rows().reveal(i);
        }

        /// What an assistive technology asked for. True when something changed.
        fn action(&mut self, req: &ActionRequest) -> bool {
            let id = req.target_node;
            println!("a11y action {:?} on {:?}", req.action, id);
            match (req.action, id) {
                (Action::Click, BUTTON) => self.invoked += 1,
                (Action::Click, CHECK) => self.show_components = !self.show_components,
                (Action::Focus, BUTTON) => self.focus = Focus::Button,
                (Action::Focus, CHECK) => self.focus = Focus::Check,
                (Action::Focus, LIST) => self.focus = Focus::List,
                (action, NodeId(n)) if n >= ROW0 && ((n - ROW0) as usize) < COUNT => {
                    let i = (n - ROW0) as usize;
                    match action {
                        Action::Click => {
                            self.sel.select_only(i);
                            self.focus = Focus::List;
                        }
                        Action::ScrollIntoView => self.reveal(i),
                        Action::Focus => {
                            self.sel.set_cursor(i);
                            self.focus = Focus::List;
                        }
                        _ => return false,
                    }
                }
                _ => return false,
            }
            true
        }

        fn click(&mut self) -> bool {
            let (x, y) = self.pointer;
            if self.button_rect().contains(x, y) {
                self.invoked += 1;
                self.focus = Focus::Button;
            } else if self.check_rect().contains(x, y) {
                self.show_components = !self.show_components;
                self.focus = Focus::Check;
            } else if let Some(i) = self.rows().row_at(self.list_area(), x, y) {
                self.sel.click(i, self.mods);
                self.focus = Focus::List;
            } else {
                return false;
            }
            true
        }

        fn key(&mut self, key: &Key) -> bool {
            let nav = match key {
                Key::Named(NamedKey::Tab) => {
                    self.focus = match (self.focus, self.mods.shift) {
                        (Focus::Button, false) | (Focus::List, true) => Focus::Check,
                        (Focus::Check, false) | (Focus::Button, true) => Focus::List,
                        (Focus::List, false) | (Focus::Check, true) => Focus::Button,
                    };
                    return true;
                }
                Key::Named(NamedKey::Space) | Key::Named(NamedKey::Enter) => {
                    match self.focus {
                        Focus::Button => self.invoked += 1,
                        Focus::Check => self.show_components = !self.show_components,
                        Focus::List => return false,
                    }
                    return true;
                }
                Key::Named(NamedKey::ArrowDown) => Nav::Down,
                Key::Named(NamedKey::ArrowUp) => Nav::Up,
                Key::Named(NamedKey::PageDown) => Nav::PageDown,
                Key::Named(NamedKey::PageUp) => Nav::PageUp,
                Key::Named(NamedKey::Home) => Nav::Home,
                Key::Named(NamedKey::End) => Nav::End,
                _ => return false,
            };
            if self.focus != Focus::List {
                return false;
            }
            let rows = self.rows();
            match self.sel.nav(&rows, nav, self.mods.shift) {
                Some(i) => {
                    self.reveal(i);
                    true
                }
                None => false,
            }
        }

        fn draw(&self, p: &mut Painter, pal: &Palette) {
            let b = self.button_rect();
            let fill = if self.focus == Focus::Button {
                pal.press
            } else {
                pal.hover
            };
            p.fill(b, fill);
            p.text_center(Face::Semibold, 14.0, b, "Uninstall", pal.text);
            let mut tick = Tick::new(self.show_components);
            if self.focus == Focus::Check {
                tick = tick.with_focus();
            }
            draw_tick(p, pal, self.check_rect(), tick);
            p.text(
                Face::Ui,
                14.0,
                180.0,
                28.0,
                "Show components",
                pal.text,
                200.0,
            );
            draw_progress(p, pal, self.progress_rect(), self.fraction());

            let head = Rect::new(0.0, TOP, self.size.0, HEAD_H);
            p.fill(head, pal.bar);
            p.text(
                Face::Semibold,
                13.0,
                16.0,
                head.cy(),
                "Name",
                pal.muted,
                300.0,
            );

            let area = self.list_area();
            let rows = self.rows();
            let saved = p.clip(area);
            for i in rows.visible() {
                let r = rows.rect(area, i);
                if self.sel.contains(i) {
                    p.fill(r, pal.selection);
                }
                if self.focus == Focus::List && self.sel.cursor() == Some(i) {
                    p.fill(Rect::new(r.x, r.y, 3.0, r.h), pal.accent);
                }
                let name = format!("Program {:04}", i + 1);
                p.text(Face::Ui, 14.0, 16.0, r.cy(), &name, pal.text, r.w - 32.0);
            }
            p.restore(saved);

            let f = Rect::new(0.0, self.size.1 - FOOT_H, self.size.0, FOOT_H);
            p.fill(f, pal.bar);
            let footer = self.footer();
            p.text(Face::Ui, 13.0, 16.0, f.cy(), &footer, pal.muted, f.w - 32.0);
        }
    }

    /// Serves the latest tree at once when a screen reader or UIA client first asks, on
    /// whichever thread asks. With the event-loop proxy alone the first answer is a placeholder
    /// tree until the loop sends the real one (AccessKit's docs); this way the first answer is
    /// the window as it is.
    struct Latest(Arc<Mutex<Option<TreeUpdate>>>);

    impl ActivationHandler for Latest {
        fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
            let tree = self.0.lock().ok()?.clone();
            println!("a11y: initial tree requested, served: {}", tree.is_some());
            tree
        }
    }

    struct State {
        latest: Arc<Mutex<Option<TreeUpdate>>>,
        window: Rc<Window>,
        surface: softbuffer::Surface<Rc<Window>, Rc<Window>>,
        adapter: accesskit_winit::Adapter,
        gfx: Gfx,
        pal: Palette,
        model: Model,
    }

    impl State {
        fn changed(&mut self) {
            let update = self.model.tree();
            if let Ok(mut l) = self.latest.lock() {
                *l = Some(update.clone());
            }
            self.adapter.update_if_active(|| update);
            self.window.request_redraw();
        }

        fn draw(&mut self) {
            let m = &self.model;
            let (w, h) = (
                (m.size.0 * m.scale).round().max(1.0) as u32,
                (m.size.1 * m.scale).round().max(1.0) as u32,
            );
            let mut bmp = Bitmap::new(w, h);
            let pal = self.pal;
            bmp.clear(pal.page);
            {
                let mut p = Painter::new(&mut bmp, &mut self.gfx, m.scale);
                m.draw(&mut p, &pal);
            }
            let (Some(nw), Some(nh)) = (NonZeroU32::new(w), NonZeroU32::new(h)) else {
                return;
            };
            if self.surface.resize(nw, nh).is_err() {
                return;
            }
            let Ok(mut buf) = self.surface.buffer_mut() else {
                return;
            };
            for (dst, px) in buf.iter_mut().zip(bmp.pixels.as_chunks::<4>().0) {
                *dst = u32::from_le_bytes([px[0], px[1], px[2], 0]);
            }
            let _ = buf.present();
        }
    }

    struct App {
        proxy: EventLoopProxy<accesskit_winit::Event>,
        state: Option<State>,
    }

    impl ApplicationHandler<accesskit_winit::Event> for App {
        fn resumed(&mut self, el: &ActiveEventLoop) {
            if self.state.is_some() {
                return;
            }
            let attrs = Window::default_attributes()
                .with_title("Purge shape")
                .with_inner_size(LogicalSize::new(900.0, 640.0))
                .with_min_inner_size(LogicalSize::new(760.0, 520.0))
                .with_visible(false);
            let window = Rc::new(el.create_window(attrs).expect("a window"));
            let latest = Arc::new(Mutex::new(None));
            let adapter = accesskit_winit::Adapter::with_mixed_handlers(
                el,
                &window,
                Latest(latest.clone()),
                self.proxy.clone(),
            );
            let context = softbuffer::Context::new(window.clone()).expect("softbuffer");
            let surface = softbuffer::Surface::new(&context, window.clone()).expect("surface");
            let scale = window.scale_factor() as f32;
            let size = window.inner_size().to_logical::<f32>(f64::from(scale));
            let mut state = State {
                latest,
                window: window.clone(),
                surface,
                adapter,
                gfx: Gfx::new(),
                pal: LIGHT,
                model: Model {
                    size: (size.width, size.height),
                    scale,
                    scroll: 0.0,
                    sel: Selection::default(),
                    show_components: false,
                    invoked: 0,
                    focus: Focus::List,
                    pointer: (0.0, 0.0),
                    mods: Mods::default(),
                },
            };
            if let Ok(mut l) = state.latest.lock() {
                *l = Some(state.model.tree());
            }
            state.draw();
            window.set_visible(true);
            self.state = Some(state);
        }

        fn user_event(&mut self, _el: &ActiveEventLoop, event: accesskit_winit::Event) {
            let Some(s) = self.state.as_mut() else { return };
            match event.window_event {
                // Not sent with a direct activation handler; kept for completeness.
                accesskit_winit::WindowEvent::InitialTreeRequested => s.changed(),
                accesskit_winit::WindowEvent::ActionRequested(req) => {
                    if s.model.action(&req) {
                        s.changed();
                    }
                }
                accesskit_winit::WindowEvent::AccessibilityDeactivated => {
                    println!("a11y: deactivated");
                }
            }
        }

        fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
            let Some(s) = self.state.as_mut() else { return };
            s.adapter.process_event(&s.window, &event);
            let m = &mut s.model;
            let changed = match event {
                WindowEvent::CloseRequested => {
                    el.exit();
                    false
                }
                WindowEvent::Resized(size) => {
                    let l = size.to_logical::<f32>(f64::from(m.scale));
                    m.size = (l.width, l.height);
                    m.scroll = m.rows().clamp_scroll(m.scroll);
                    true
                }
                WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                    m.scale = scale_factor as f32;
                    true
                }
                WindowEvent::RedrawRequested => {
                    s.draw();
                    false
                }
                WindowEvent::ModifiersChanged(mods) => {
                    let st = mods.state();
                    m.mods = Mods {
                        ctrl: st.control_key(),
                        shift: st.shift_key(),
                        alt: st.alt_key(),
                        ..Mods::default()
                    };
                    false
                }
                WindowEvent::CursorMoved { position, .. } => {
                    let l = position.to_logical::<f32>(f64::from(m.scale));
                    m.pointer = (l.x, l.y);
                    false
                }
                WindowEvent::MouseInput {
                    state: ElementState::Pressed,
                    button: MouseButton::Left,
                    ..
                } => m.click(),
                WindowEvent::MouseWheel { delta, .. } => {
                    let dy = match delta {
                        MouseScrollDelta::LineDelta(_, y) => y * ROW_H * 3.0,
                        MouseScrollDelta::PixelDelta(p) => p.y as f32 / m.scale,
                    };
                    m.scroll = m.rows().clamp_scroll(m.scroll - dy);
                    true
                }
                WindowEvent::KeyboardInput { event, .. } if event.state.is_pressed() => {
                    m.key(&event.logical_key)
                }
                _ => false,
            };
            if changed {
                s.changed();
            }
        }
    }

    pub fn run() {
        let event_loop = EventLoop::<accesskit_winit::Event>::with_user_event()
            .build()
            .expect("an event loop");
        event_loop.set_control_flow(ControlFlow::Wait);
        let mut app = App {
            proxy: event_loop.create_proxy(),
            state: None,
        };
        event_loop.run_app(&mut app).expect("the event loop");
    }
}
