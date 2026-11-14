//! Opens an undecorated window that models the apps and shows a shadow route:
//! `MD_SHADOW=client|kwin|none cargo run -p mdshared-ui --example shadow [--features client-margin] -- [seconds] [backdrop|backdrop-dark] [maximize]`
//!
//! The window content (light grey, a 1 px darker border, a fake 40 px title row, a red 4 px dot in
//! each corner) is rendered into its own bitmap of the CONTENT size; the present step copies it
//! into the softbuffer surface buffer at the margin offset and has the shadow painted around it
//! (`Shadow::paint_frame`). Dragging the title row moves the window, dragging near a content
//! edge resizes it (`drag_resize_window`). Prints the route, the attach time, the paint cost.
//! `backdrop` opens a plain white 1000x700 window (`backdrop-dark`: dark grey) to put behind it.
//! `maximize` maximizes the window after one second (for the scripted checks).
//!
//! The client route needs `--features client-margin` plus the patched winit and softbuffer (see
//! docs/LINUX.md); without it the example only knows KWin's route and "none".

#[cfg(not(target_os = "linux"))]
fn main() {
    println!("the shadow example is for Linux");
}

#[cfg(target_os = "linux")]
fn main() {
    use std::num::NonZeroU32;
    use std::rc::Rc;
    use std::time::{Duration, Instant};

    use mdshared_ui::shadow::{Mode, RESIZE_BAND, Shadow};
    use winit::application::ApplicationHandler;
    use winit::dpi::LogicalSize;
    use winit::event::{ElementState, MouseButton, WindowEvent};
    use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
    use winit::window::{CursorIcon, ResizeDirection, Window, WindowId};

    const TITLE_ROW: u32 = 40;

    /// The content as the app renders it: independent of the surface and the margin.
    fn render_content(w: u32, h: u32, scale: f64) -> Vec<u32> {
        let mut c = vec![0xFFC8C8C8u32; (w * h) as usize];
        let title = (TITLE_ROW as f64 * scale) as u32;
        for y in 0..title.min(h) {
            c[(y * w) as usize..((y + 1) * w) as usize].fill(0xFFB4B4B4);
        }
        let border = 0xFF707070;
        for x in 0..w {
            c[x as usize] = border;
            c[((h - 1) * w + x) as usize] = border;
        }
        for y in 0..h {
            c[(y * w) as usize] = border;
            c[(y * w + w - 1) as usize] = border;
        }
        // A red dot in every corner of the content: shows exactly where it begins and ends.
        let d = 4.min(w / 2).min(h / 2);
        for (cx, cy) in [(0, 0), (w - d, 0), (0, h - d), (w - d, h - d)] {
            for y in cy..cy + d {
                for x in cx..cx + d {
                    c[(y * w + x) as usize] = 0xFFE00000;
                }
            }
        }
        c
    }

    struct App {
        until: Instant,
        mode: Mode,
        window: Option<Rc<Window>>,
        surface: Option<softbuffer::Surface<Rc<Window>, Rc<Window>>>,
        shadow: Option<Shadow>,
        frames: u32,
        painted: u32,
        paint_time: Duration,
        backdrop: Option<u32>,
        maximize_at: Option<Instant>,
        content: Vec<u32>,
        content_key: (u32, u32),
        pointer: (f64, f64),
        active: bool,
    }

    impl App {
        fn scale(&self) -> f64 {
            self.window.as_ref().map_or(1.0, |w| w.scale_factor())
        }

        /// What the pointer at surface position `p` would do: resize edge, move, or nothing.
        fn hit(&self, p: (f64, f64)) -> Option<ResizeDirection> {
            let s = self.scale();
            let (x, y) = match &self.shadow {
                Some(sh) => sh.to_content(p.0, p.1),
                None => p,
            };
            let (sw, sh_) = {
                let size = self.window.as_ref()?.inner_size();
                match &self.shadow {
                    Some(sh) => sh.content_size(size.width, size.height),
                    None => (size.width, size.height),
                }
            };
            // The band reaches RESIZE_BAND px outside the content too (negative `x`).
            let band = RESIZE_BAND as f64 * s;
            let (w, h) = (sw as f64, sh_ as f64);
            let (l, r) = (x < band, x >= w - band);
            let (t, b) = (y < band, y >= h - band);
            Some(match (l, r, t, b) {
                (true, _, true, _) => ResizeDirection::NorthWest,
                (_, true, true, _) => ResizeDirection::NorthEast,
                (true, _, _, true) => ResizeDirection::SouthWest,
                (_, true, _, true) => ResizeDirection::SouthEast,
                (true, ..) => ResizeDirection::West,
                (_, true, ..) => ResizeDirection::East,
                (_, _, true, _) => ResizeDirection::North,
                (_, _, _, true) => ResizeDirection::South,
                _ => return None,
            })
        }
    }

    impl ApplicationHandler for App {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            if self.window.is_some() {
                return;
            }
            let backdrop = self.backdrop.is_some();
            let mut attributes = Window::default_attributes()
                .with_title(if backdrop {
                    "mdshared backdrop"
                } else {
                    "mdshared shadow test"
                })
                .with_decorations(false)
                .with_inner_size(if backdrop {
                    LogicalSize::new(1000.0, 700.0)
                } else {
                    LogicalSize::new(600.0, 400.0)
                });
            if !backdrop {
                // `prepare` grows the initial (content) size by the margin on Wayland.
                attributes = Shadow::prepare(attributes, self.mode, event_loop);
            }
            let window = Rc::new(event_loop.create_window(attributes).unwrap());
            let mut surface = softbuffer::Context::new(window.clone())
                .and_then(|context| softbuffer::Surface::new(&context, window.clone()))
                .unwrap();
            if !backdrop {
                let t = Instant::now();
                self.shadow = Shadow::attach_mode(&window, self.mode);
                println!(
                    "attach: {:?} in {:.2} ms, {} bytes, margin {}",
                    self.shadow.as_ref().map(|s| s.kind()),
                    t.elapsed().as_secs_f64() * 1000.0,
                    self.shadow.as_ref().map_or(0, |s| s.bytes()),
                    self.shadow.as_ref().map_or(0, |s| s.margin()),
                );
                #[cfg(feature = "client-margin")]
                // `MD_ALPHA=0` leaves softbuffer's XRGB8888 in place: the experiment that shows an
                // unpatched softbuffer cannot show the margin.
                if self.shadow.as_ref().is_some_and(|s| s.wants_alpha())
                    && std::env::var_os("MD_ALPHA").is_none_or(|v| v != "0")
                {
                    let on = surface.set_premultiplied_alpha(true);
                    println!("softbuffer premultiplied alpha: {on}");
                }
            }
            let _ = &mut surface;
            self.window = Some(window.clone());
            self.surface = Some(surface);
            window.request_redraw();
        }

        fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
            let Some(window) = self.window.clone() else {
                return;
            };
            match event {
                WindowEvent::CloseRequested => event_loop.exit(),
                WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                    let active = self.active;
                    if let Some(s) = &mut self.shadow {
                        s.update(&window, scale_factor, active);
                    }
                    window.request_redraw();
                }
                WindowEvent::Focused(active) => {
                    self.active = active;
                    if let Some(s) = &mut self.shadow
                        && s.update(&window, window.scale_factor(), active)
                    {
                        window.request_redraw();
                    }
                }
                WindowEvent::Resized(size) => {
                    let active = self.active;
                    if let Some(s) = &mut self.shadow {
                        s.update(&window, window.scale_factor(), active);
                        println!(
                            "resized: surface {}x{} margin {} content {:?}",
                            size.width,
                            size.height,
                            s.margin(),
                            s.content_size(size.width, size.height)
                        );
                    }
                    window.request_redraw();
                }
                WindowEvent::CursorMoved { position, .. } => {
                    self.pointer = (position.x, position.y);
                    let icon = match self.hit(self.pointer) {
                        Some(ResizeDirection::North | ResizeDirection::South) => {
                            CursorIcon::NsResize
                        }
                        Some(ResizeDirection::East | ResizeDirection::West) => CursorIcon::EwResize,
                        Some(ResizeDirection::NorthWest | ResizeDirection::SouthEast) => {
                            CursorIcon::NwseResize
                        }
                        Some(_) => CursorIcon::NeswResize,
                        None => CursorIcon::Default,
                    };
                    window.set_cursor(icon);
                }
                WindowEvent::MouseInput {
                    state: ElementState::Pressed,
                    button: MouseButton::Left,
                    ..
                } => {
                    if let Some(dir) = self.hit(self.pointer) {
                        let _ = window.drag_resize_window(dir);
                    } else {
                        let (_, y) = match &self.shadow {
                            Some(s) => s.to_content(self.pointer.0, self.pointer.1),
                            None => self.pointer,
                        };
                        if y < TITLE_ROW as f64 * self.scale() {
                            let _ = window.drag_window();
                        }
                    }
                }
                WindowEvent::RedrawRequested => {
                    let size = window.inner_size();
                    let (Some(w), Some(h)) =
                        (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
                    else {
                        return;
                    };
                    let scale = window.scale_factor();
                    let (sw, sh) = (size.width, size.height);
                    let (m, (cw, ch)) = match &self.shadow {
                        Some(s) => (s.margin(), s.content_size(sw, sh)),
                        None => (0, (sw, sh)),
                    };
                    // Render the content separately, only when its size changed.
                    if let Some(shade) = self.backdrop {
                        self.content = vec![shade; (cw * ch) as usize];
                        self.content_key = (cw, ch);
                    } else if self.content_key != (cw, ch) {
                        self.content = render_content(cw, ch, scale);
                        self.content_key = (cw, ch);
                    }
                    let surface = self.surface.as_mut().unwrap();
                    surface.resize(w, h).unwrap();
                    let mut buffer = surface.buffer_mut().unwrap();
                    let age = buffer.age();
                    // The present step: copy the content to the margin offset, row by row.
                    for y in 0..ch {
                        let to = ((y + m) * sw + m) as usize;
                        buffer[to..to + cw as usize].copy_from_slice(
                            &self.content[(y * cw) as usize..((y + 1) * cw) as usize],
                        );
                    }
                    if let Some(s) = &mut self.shadow {
                        let t = Instant::now();
                        if s.paint_frame(&mut buffer, sw, sh, age) {
                            self.painted += 1;
                            self.paint_time += t.elapsed();
                        }
                    }
                    window.pre_present_notify();
                    buffer.present().unwrap();
                    self.frames += 1;
                }
                _ => {}
            }
            self.tick(event_loop);
        }

        fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
            let mut next = self.until;
            if let Some(at) = self.maximize_at {
                next = next.min(at);
            }
            event_loop.set_control_flow(ControlFlow::WaitUntil(next));
            self.tick(event_loop);
        }
    }

    impl App {
        fn tick(&mut self, event_loop: &ActiveEventLoop) {
            if let (Some(at), Some(w)) = (self.maximize_at, &self.window)
                && Instant::now() >= at
            {
                self.maximize_at = None;
                w.set_maximized(true);
            }
            if Instant::now() >= self.until {
                println!(
                    "frames drawn: {}, margin painted in {} ({:.3} ms total)",
                    self.frames,
                    self.painted,
                    self.paint_time.as_secs_f64() * 1000.0
                );
                event_loop.exit();
            }
        }
    }

    let args: Vec<String> = std::env::args().skip(1).collect();
    let secs: u64 = args.first().and_then(|s| s.parse().ok()).unwrap_or(5);
    let mut app = App {
        until: Instant::now() + Duration::from_secs(secs),
        mode: Mode::from_env(),
        window: None,
        surface: None,
        shadow: None,
        frames: 0,
        painted: 0,
        paint_time: Duration::ZERO,
        backdrop: args.iter().find_map(|a| match a.as_str() {
            "backdrop" => Some(0xFFF8F8F8),
            "backdrop-dark" => Some(0xFF202226),
            _ => None,
        }),
        maximize_at: args
            .iter()
            .any(|a| a == "maximize")
            .then(|| Instant::now() + Duration::from_secs(1)),
        content: Vec::new(),
        content_key: (0, 0),
        pointer: (0.0, 0.0),
        active: true,
    };
    EventLoop::new().unwrap().run_app(&mut app).unwrap();
}
