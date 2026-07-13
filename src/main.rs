mod renderer;

use anyhow::Result;
use anyhow::bail;
use renderer::Renderer;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

use std::sync::Arc;

struct App {
    image_path: Option<String>,
    renderer: Option<Renderer>,
    cursor_pos: (f64, f64),
    proxy: winit::event_loop::EventLoopProxy<UserEvent>,
}

pub enum UserEvent {
    ImageLoaded(Result<LoadedImageData>),
}
pub struct LoadedImageData {
    pub rgba: image::RgbaImage,
    pub width: u32,
    pub height: u32,
}

pub fn load_image_async(
    path: impl AsRef<std::path::Path>,
    proxy: winit::event_loop::EventLoopProxy<UserEvent>,
) {
    let path = path.as_ref().to_path_buf();

    std::thread::spawn(move || {
        let result = image::open(&path).map(|img| {
            let rgba = img.to_rgba8();
            let (width, height) = rgba.dimensions();
            LoadedImageData {
                rgba,
                width,
                height,
            }
        });
        let _ = proxy.send_event(UserEvent::ImageLoaded(result.map_err(anyhow::Error::from)));
    });
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let window = Arc::new(
            event_loop
                .create_window(Window::default_attributes().with_title("Pluey"))
                .expect("could not create a window"),
        );

        let renderer = pollster::block_on(Renderer::new(
            event_loop.owned_display_handle(),
            window,
            self.image_path.as_deref(),
        ))
        .expect("could not create a renderer");

        renderer.window().request_redraw();

        self.renderer = Some(renderer);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        let renderer = self.renderer.as_mut().unwrap();

        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }

            WindowEvent::KeyboardInput {
                event:
                    winit::event::KeyEvent {
                        physical_key:
                            winit::keyboard::PhysicalKey::Code(winit::keyboard::KeyCode::KeyQ),
                        state: winit::event::ElementState::Pressed,
                        repeat: false,
                        ..
                    },
                ..
            } => {
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
                if button == winit::event::MouseButton::Left
                    || button == winit::event::MouseButton::Middle
                {
                    match state {
                        winit::event::ElementState::Pressed => renderer.start_drag(self.cursor_pos),
                        winit::event::ElementState::Released => renderer.end_drag(),
                    }
                }
            }

            #[allow(clippy::cast_possible_truncation)]
            WindowEvent::MouseWheel { delta, .. } => {
                let scroll_y = match delta {
                    winit::event::MouseScrollDelta::LineDelta(_, y) => y,
                    winit::event::MouseScrollDelta::PixelDelta(p) => (p.y / 20.0) as f32,
                };
                renderer.zoom(scroll_y, self.cursor_pos);
                renderer.window().request_redraw();
            }

            #[allow(clippy::cast_possible_truncation)]
            #[cfg(any(target_os = "macos", target_os = "ios"))]
            WindowEvent::PinchGesture { delta, .. } => {
                renderer.zoom((delta as f32) * 5.0, self.cursor_pos);
                renderer.window().request_redraw();
            }

            WindowEvent::DroppedFile(file) => {
                load_image_async(&file, self.proxy.clone());
            }

            _ => {}
        }
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: UserEvent) {
        let UserEvent::ImageLoaded(result) = event;
        let Some(renderer) = self.renderer.as_mut() else {
            return;
        };

        match result {
            Ok(data) => {
                renderer.apply_loaded_image(&data);
                renderer.window().request_redraw();
            }
            Err(e) => eprintln!("failed to decode image: {e}"),
        }
    }
}

fn main() -> Result<()> {
    let path = std::env::args().nth(1);

    if let Some(ref path) = path
        && !std::path::Path::new(&path).is_file()
    {
        bail!("'{path}' does not exist or is not a file");
    }

    let event_loop = EventLoop::<UserEvent>::with_user_event().build().unwrap();
    event_loop.set_control_flow(ControlFlow::Wait);

    let proxy = event_loop.create_proxy();

    let mut app = App {
        image_path: path,
        renderer: None,
        cursor_pos: (0.0, 0.0),
        proxy,
    };
    let _ = event_loop.run_app(&mut app);

    Ok(())
}
