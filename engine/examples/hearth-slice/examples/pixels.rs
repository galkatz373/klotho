//! Live Hearth window. Keys 1–4 switch goldens. Esc quits.
//!
//! ```text
//! cargo run -p hearth-slice --example pixels
//! ```

use std::sync::Arc;

use hearth_slice::{PixelScene, golden_camera, stage};
use klotho_manifest::{GpuBudget, VisualManifest};
use klotho_render::WindowedPresenter;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

fn main() {
    let event_loop = EventLoop::new().expect("event loop");
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = App {
        window: None,
        gpu: None,
        scene: PixelScene::DoorOpen,
        vis: None,
    };
    eprintln!("Hearth pixels. 1 door  2 barrel  3 fire  4 HUD  Esc quit");
    event_loop.run_app(&mut app).expect("run");
}

struct App {
    window: Option<Arc<Window>>,
    gpu: Option<WindowedPresenter>,
    scene: PixelScene,
    vis: Option<VisualManifest>,
}

impl App {
    fn load_scene(&mut self, scene: PixelScene) {
        self.scene = scene;
        let (h, vis, _ui, _) = stage(scene);
        if let Some(gpu) = self.gpu.as_mut() {
            gpu.gpu_mut().upload_cas(&vis, &h.cas);
        }
        self.vis = Some(vis);
        if let Some(w) = &self.window {
            w.request_redraw();
        }
        let _ = h;
        eprintln!("scene {}", scene.stem());
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title("Klotho — Hearth")
            .with_inner_size(winit::dpi::LogicalSize::new(1280.0, 720.0));
        let window = Arc::new(event_loop.create_window(attrs).expect("window"));
        let gpu = WindowedPresenter::try_new(Arc::clone(&window)).expect("wgpu adapter");
        self.window = Some(window);
        self.gpu = Some(gpu);
        self.load_scene(self.scene);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(gpu) = self.gpu.as_mut() {
                    gpu.resize(size.width, size.height);
                }
            }
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        physical_key: PhysicalKey::Code(code),
                        state: ElementState::Pressed,
                        ..
                    },
                ..
            } => match code {
                KeyCode::Escape => event_loop.exit(),
                KeyCode::Digit1 | KeyCode::Numpad1 => self.load_scene(PixelScene::DoorOpen),
                KeyCode::Digit2 | KeyCode::Numpad2 => self.load_scene(PixelScene::BarrelCarried),
                KeyCode::Digit3 | KeyCode::Numpad3 => self.load_scene(PixelScene::OneFire),
                KeyCode::Digit4 | KeyCode::Numpad4 => self.load_scene(PixelScene::HudOwed),
                _ => {}
            },
            WindowEvent::RedrawRequested => {
                let observer = golden_camera();
                if let (Some(gpu), Some(vis)) = (self.gpu.as_mut(), self.vis.as_ref()) {
                    let _ = gpu.present_frame(vis, observer, GpuBudget::HEARTH);
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }
}
