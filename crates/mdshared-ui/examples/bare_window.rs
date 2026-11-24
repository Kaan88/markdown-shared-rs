//! The reference for check C1 (launch time): a bare winit window filled through softbuffer, with
//! nothing of the toolkit in it. `examples/purge_shape.rs` is measured against it:
//! `cargo run --release -p mdshared-ui --example bare_window -- --first-frame` prints the time
//! from the start of `main` to the first frame presented and exits.

#[cfg(not(windows))]
fn main() {
    println!("bare_window is for Windows so far, like purge_shape");
}

#[cfg(windows)]
fn main() {
    bare::run();
}

// softbuffer is a dev-dependency on Windows only.
#[cfg(windows)]
mod bare {
    use std::num::NonZeroU32;
    use std::rc::Rc;
    use std::time::Instant;

    use winit::application::ApplicationHandler;
    use winit::dpi::LogicalSize;
    use winit::event::WindowEvent;
    use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
    use winit::window::{Window, WindowId};

    type Surface = softbuffer::Surface<Rc<Window>, Rc<Window>>;

    struct App {
        started: Instant,
        exit_after_first: bool,
        window: Option<(Rc<Window>, Surface)>,
    }

    impl App {
        fn draw(&mut self) {
            let Some((window, surface)) = self.window.as_mut() else {
                return;
            };
            let size = window.inner_size();
            let (Some(w), Some(h)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
            else {
                return;
            };
            if surface.resize(w, h).is_err() {
                return;
            }
            let Ok(mut buf) = surface.buffer_mut() else {
                return;
            };
            buf.fill(0x00f3_f3f3);
            let _ = buf.present();
        }
    }

    impl ApplicationHandler for App {
        fn resumed(&mut self, el: &ActiveEventLoop) {
            if self.window.is_some() {
                return;
            }
            let attrs = Window::default_attributes()
                .with_title("Bare window")
                .with_inner_size(LogicalSize::new(1000.0, 680.0))
                .with_visible(false);
            let window = Rc::new(el.create_window(attrs).expect("a window"));
            let context = softbuffer::Context::new(window.clone()).expect("softbuffer");
            let surface = softbuffer::Surface::new(&context, window.clone()).expect("surface");
            self.window = Some((window.clone(), surface));
            self.draw();
            window.set_visible(true);
            println!(
                "first frame after {:.1} ms",
                self.started.elapsed().as_secs_f64() * 1000.0
            );
            if self.exit_after_first {
                el.exit();
            }
        }

        fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
            match event {
                WindowEvent::CloseRequested => el.exit(),
                WindowEvent::RedrawRequested | WindowEvent::Resized(_) => self.draw(),
                _ => {}
            }
        }
    }

    pub fn run() {
        let started = Instant::now();
        let event_loop = EventLoop::new().expect("an event loop");
        event_loop.set_control_flow(ControlFlow::Wait);
        let mut app = App {
            started,
            exit_after_first: std::env::args().any(|a| a == "--first-frame"),
            window: None,
        };
        event_loop.run_app(&mut app).expect("the event loop");
    }
}
