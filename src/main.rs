mod renderer;

use renderer::Renderer;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

use std::sync::Arc;

struct App {
    image_path: String,
    renderer: Option<Renderer>,
    cursor_pos: (f64, f64),
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let window = Arc::new(
            event_loop
                .create_window(Window::default_attributes().with_title("Pluey"))
                .unwrap(),
        );

        let renderer = pollster::block_on(Renderer::new(
            event_loop.owned_display_handle(),
            window,
            &self.image_path,
        ))
        .unwrap();

        renderer.window().request_redraw();

        self.renderer = Some(renderer);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        let renderer = self.renderer.as_mut().unwrap();

        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }

            WindowEvent::Resized(size) => {
                renderer.resize(size);
                renderer.window().request_redraw();
            }

            WindowEvent::RedrawRequested => {
                if let Err(e) = renderer.render() {
                    eprintln!("render error: {e:?}");
                }
            }

            WindowEvent::CursorMoved { position, .. } => {
                self.cursor_pos = (position.x, position.y);
                if renderer.cursor_moved(self.cursor_pos) {
                    renderer.window().request_redraw();
                }
            }

            WindowEvent::MouseInput { state, button, .. } => {
                if button == winit::event::MouseButton::Left {
                    match state {
                        winit::event::ElementState::Pressed => renderer.start_drag(self.cursor_pos),
                        winit::event::ElementState::Released => renderer.end_drag(),
                    }
                }
            }

            WindowEvent::MouseWheel { delta, .. } => {
                let scroll_y = match delta {
                    winit::event::MouseScrollDelta::LineDelta(_, y) => y,
                    winit::event::MouseScrollDelta::PixelDelta(p) => (p.y / 20.0) as f32,
                };
                renderer.zoom(scroll_y, self.cursor_pos);
                renderer.window().request_redraw();
            }

            _ => {}
        }
    }
}

 fn main() {
    let path = match std::env::args().nth(1) {
        Some(p) => p,
        None => {
            eprintln!("usage: pluey <image path>");
            std::process::exit(1);
        }
    };

    if !std::path::Path::new(&path).is_file() {
        eprintln!("error: '{path}' does not exist or is not a file");
        std::process::exit(1);
    }

    let event_loop = EventLoop::new().unwrap();
    event_loop.set_control_flow(ControlFlow::Wait);

    let mut app = App {
        image_path: path,
        renderer: None,
        cursor_pos: (0.0, 0.0),
    };
    let _ = event_loop.run_app(&mut app);
}
