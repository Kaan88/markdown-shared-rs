//! Opens an undecorated window filled grey (a white 1000x700 backdrop window with `backdrop`), attaches the compositor shadow, and exits
//! after N seconds: `cargo run -p mdshared-ui --example shadow -- [seconds] [noshadow] [backdrop]`.
//! Prints the time `Shadow::attach` took and the tile memory. The window is set up like the
//! apps' (decorations off, softbuffer present of the whole frame).

#[cfg(not(target_os = "linux"))]
fn main() {
    println!("the shadow example is for Linux");
}

#[cfg(target_os = "linux")]
fn main() {
    use std::num::NonZeroU32;
    use std::rc::Rc;
    use std::time::{Duration, Instant};

    use mdshared_ui::shadow::Shadow;
    use winit::application::ApplicationHandler;
    use winit::dpi::LogicalSize;
    use winit::event::WindowEvent;
    use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
    use winit::window::{Window, WindowId};

    struct App {
        until: Instant,
        shadow_on: bool,
        window: Option<Rc<Window>>,
        surface: Option<softbuffer::Surface<Rc<Window>, Rc<Window>>>,
        shadow: Option<Shadow>,
        frames: u32,
        backdrop: bool,
    }

    impl ApplicationHandler for App {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            if self.window.is_some() {
                return;
            }
            let attributes = Window::default_attributes()
                .with_title(if self.backdrop {
                    "mdshared backdrop"
                } else {
                    "mdshared shadow test"
                })
                .with_decorations(false)
                .with_inner_size(if self.backdrop {
                    LogicalSize::new(1000.0, 700.0)
                } else {
                    LogicalSize::new(600.0, 400.0)
                });
            let window = Rc::new(event_loop.create_window(attributes).unwrap());
            let surface = softbuffer::Context::new(window.clone())
                .and_then(|context| softbuffer::Surface::new(&context, window.clone()))
                .unwrap();
            if self.shadow_on {
                let t = Instant::now();
                self.shadow = Shadow::attach(&window);
                println!(
                    "attach: {:?} in {:.2} ms, {} bytes of tiles",
                    self.shadow.as_ref().map(|s| s.kind()),
                    t.elapsed().as_secs_f64() * 1000.0,
                    self.shadow.as_ref().map_or(0, |s| s.bytes())
                );
            }
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
                    if let Some(s) = &mut self.shadow {
                        s.update(scale_factor, true, window.is_maximized());
                    }
                }
                WindowEvent::Focused(active) => {
                    if let Some(s) = &mut self.shadow {
                        s.update(window.scale_factor(), active, window.is_maximized());
                    }
                }
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
                    buffer.fill(if self.backdrop {
                        0xFFF8F8F8
                    } else {
                        0xFFC8C8C8
                    });
                    buffer.present().unwrap();
                    self.frames += 1;
                }
                _ => {}
            }
            if Instant::now() >= self.until {
                println!("frames drawn: {}", self.frames);
                event_loop.exit();
            }
        }

        fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
            event_loop.set_control_flow(ControlFlow::WaitUntil(self.until));
            if Instant::now() >= self.until {
                println!("frames drawn: {}", self.frames);
                event_loop.exit();
            }
        }
    }

    let args: Vec<String> = std::env::args().skip(1).collect();
    let secs: u64 = args.first().and_then(|s| s.parse().ok()).unwrap_or(5);
    let mut app = App {
        until: Instant::now() + Duration::from_secs(secs),
        shadow_on: !args.iter().any(|a| a == "noshadow" || a == "backdrop"),
        window: None,
        surface: None,
        shadow: None,
        frames: 0,
        backdrop: args.iter().any(|a| a == "backdrop"),
    };
    EventLoop::new().unwrap().run_app(&mut app).unwrap();
}
