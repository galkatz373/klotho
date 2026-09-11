//! Windowed presenter. Swapchain present; sim still does not wait.

use std::sync::Arc;

use klotho_manifest::{GpuBudget, Observer, VisualManifest};
use winit::window::Window;

use crate::gpu::WgpuPresenter;

/// wgpu surface + clustered presenter for a live window.
pub struct WindowedPresenter {
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    gpu: WgpuPresenter,
    /// Held so the surface target stays valid.
    _window: Arc<Window>,
}

impl WindowedPresenter {
    /// Create a swapchain presenter for `window`. `None` if no adapter.
    pub fn try_new(window: Arc<Window>) -> Option<Self> {
        let instance = wgpu::Instance::default();
        let surface = instance.create_surface(window.clone()).ok()?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }))
        .ok()?;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("klotho-window"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::downlevel_defaults(),
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            memory_hints: wgpu::MemoryHints::Performance,
            trace: wgpu::Trace::Off,
        }))
        .ok()?;
        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(caps.formats[0]);
        let size = window.inner_size();
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);
        let gpu =
            WgpuPresenter::from_device(device, queue, config.width, config.height, format, false);
        Some(Self {
            surface,
            config,
            gpu,
            _window: window,
        })
    }

    /// Underlying GPU presenter (uploads, last_drawn).
    pub fn gpu_mut(&mut self) -> &mut WgpuPresenter {
        &mut self.gpu
    }

    /// Resize the swapchain.
    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(self.gpu.device(), &self.config);
        self.gpu.resize(width, height);
    }

    /// Draw one swapchain frame. Returns false if the surface was lost.
    pub fn present_frame(
        &mut self,
        vis: &VisualManifest,
        observer: Observer,
        budget: GpuBudget,
    ) -> bool {
        let Ok(frame) = self.surface.get_current_texture() else {
            return false;
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        self.gpu.present_to(&view, vis, observer, budget);
        frame.present();
        true
    }
}
