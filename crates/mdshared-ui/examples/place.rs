//! Opens an undecorated grey window, moves it to the left (or right) half of its work area with
//! `place::set` after a second, and exits after N seconds:
//! `cargo run -p mdshared-ui --example place -- [seconds] [left|right]`.
//! Prints the work area, the halves, what `set` returned, and the window's position and size
//! before and after. Nothing is clicked or typed: it only opens a window.

#[cfg(not(target_os = "linux"))]
fn main() {
    println!("the place example is for Linux");
}

#[cfg(target_os = "linux")]
fn main() {
    use std::num::NonZeroU32;
    use std::rc::Rc;
    use std::time::{Duration, Instant};

    use mdshared_ui::place;
    use winit::application::ApplicationHandler;
    use winit::dpi::LogicalSize;
    use winit::event::WindowEvent;
    use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
    use winit::window::{Window, WindowId};

    struct App {
        start: Instant,
        until: Instant,
        right: bool,
        placed: bool,
        window: Option<Rc<Window>>,
        surface: Option<softbuffer::Surface<Rc<Window>, Rc<Window>>>,
    }

    impl App {
        fn report(&self, what: &str) {
            if let Some(w) = &self.window {
                println!(
                    "{what}: position {:?}, outer {:?}, inner {:?}, scale {}",
                    w.outer_position().ok(),
                    w.outer_size(),
                    w.inner_size(),
                    w.scale_factor()
                );
            }
        }
    }

    impl ApplicationHandler for App {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            if self.window.is_some() {
                return;
            }
            let attributes = Window::default_attributes()
                .with_title("mdshared place test")
                .with_decorations(false)
                .with_inner_size(LogicalSize::new(500.0, 300.0));
            let window = Rc::new(event_loop.create_window(attributes).unwrap());
            let surface = softbuffer::Context::new(window.clone())
                .and_then(|context| softbuffer::Surface::new(&context, window.clone()))
                .unwrap();
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
                WindowEvent::Resized(_) => window.request_redraw(),
                WindowEvent::RedrawRequested => {
                    let size = window.inner_size();
                    let (Some(w), Some(h)) =
                        (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
                    else {
                        return;
                    };
                    let surface = self.surface.as_mut().unwrap();
                    surface.resize(w, h).unwrap();
                    let mut buffer = surface.buffer_mut().unwrap();
                    buffer.fill(0xFFC8C8C8);
                    buffer.present().unwrap();
                }
                _ => {}
            }
        }

        fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
            let now = Instant::now();
            if now >= self.until {
                self.report("at the end");
                event_loop.exit();
                return;
            }
            let place_at = self.start + Duration::from_secs(1);
            if !self.placed && now >= place_at {
                self.placed = true;
                let Some(window) = self.window.clone() else {
                    return;
                };
                self.report("before");
                let area = place::work_area(&window);
                println!("work area: {area:?}");
                for m in window.available_monitors() {
                    println!(
                        "  monitor {:?} at {:?} size {:?}",
                        m.name(),
                        m.position(),
                        m.size()
                    );
                }
                if let Some(area) = area {
                    let (left, right) = place::halves(area);
                    let rect = if self.right { right } else { left };
                    println!("halves: {left:?} {right:?}; asking for {rect:?}");
                    println!("set returned {}", place::set(&window, rect));
                }
            }
            let next = if self.placed { self.until } else { place_at };
            event_loop.set_control_flow(ControlFlow::WaitUntil(next.min(self.until)));
        }
    }

    let args: Vec<String> = std::env::args().skip(1).collect();
    let secs: u64 = args.first().and_then(|s| s.parse().ok()).unwrap_or(4);
    let now = Instant::now();
    let mut app = App {
        start: now,
        until: now + Duration::from_secs(secs),
        right: args.iter().any(|a| a == "right"),
        placed: false,
        window: None,
        surface: None,
    };
    let event_loop = EventLoop::new().unwrap();
    event_loop.run_app(&mut app).unwrap();
}
