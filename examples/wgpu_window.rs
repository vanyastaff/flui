//! wgpu Window - Platform-driven GPU rendering integration test
//!
//! Demonstrates the full integration path:
//! flui-platform (Platform::run + PlatformWindow) -> raw-window-handle -> wgpu -> GPU
//!
//! Run with: cargo run --example wgpu_window

use flui_platform::traits::PlatformWindow;
use flui_platform::{current_platform, WindowOptions};
use flui_types::geometry::{px, Size};
use std::sync::{Arc, Mutex};

/// Wrapper that implements HasWindowHandle + HasDisplayHandle
/// by delegating to PlatformWindow trait methods.
///
/// This bridges the gap between `dyn PlatformWindow` (which has methods)
/// and wgpu's requirement for `HasWindowHandle + HasDisplayHandle` (traits).
struct PlatformWindowHandle {
    window: Arc<dyn PlatformWindow>,
}

impl raw_window_handle::HasWindowHandle for PlatformWindowHandle {
    fn window_handle(
        &self,
    ) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        self.window.window_handle()
    }
}

impl raw_window_handle::HasDisplayHandle for PlatformWindowHandle {
    fn display_handle(
        &self,
    ) -> Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError> {
        self.window.display_handle()
    }
}

/// GPU state created from a PlatformWindow
struct GpuState {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    frame_count: u64,
}

impl GpuState {
    fn new(window: &Arc<dyn PlatformWindow>) -> Self {
        let handle = PlatformWindowHandle {
            window: window.clone(),
        };

        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());

        // Safety: PlatformWindow outlives the surface (held in Arc)
        #[allow(unsafe_code)]
        let surface = unsafe {
            instance
                .create_surface_unsafe(wgpu::SurfaceTargetUnsafe::from_window(&handle).unwrap())
                .expect("Failed to create surface")
        };

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }))
        .expect("Failed to find GPU adapter");

        tracing::info!(
            "GPU adapter: {} ({:?})",
            adapter.get_info().name,
            adapter.get_info().backend
        );

        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("flui-device"),
            ..Default::default()
        }))
        .expect("Failed to create device");

        let size = window.physical_size();
        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .find(|f| f.is_srgb())
            .copied()
            .unwrap_or(caps.formats[0]);

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.0 as u32,
            height: size.height.0 as u32,
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        Self {
            surface,
            device,
            queue,
            config,
            frame_count: 0,
        }
    }

    fn resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.config.width = width;
            self.config.height = height;
            self.surface.configure(&self.device, &self.config);
            tracing::debug!("Surface resized to {}x{}", width, height);
        }
    }

    fn render_frame(&mut self) {
        self.frame_count += 1;

        let output = match self.surface.get_current_texture() {
            Ok(output) => output,
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                self.surface.configure(&self.device, &self.config);
                return;
            }
            Err(e) => {
                tracing::error!("Surface error: {:?}", e);
                return;
            }
        };

        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        // Animate color: cycle hue over time
        let t = (self.frame_count as f64 * 0.01).sin() * 0.5 + 0.5;
        let clear_color = wgpu::Color {
            r: 0.05 + 0.15 * t,
            g: 0.08 + 0.12 * (1.0 - t),
            b: 0.18 + 0.12 * t,
            a: 1.0,
        };

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("clear-encoder"),
            });

        {
            let _render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("clear-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(clear_color),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();

        if self.frame_count % 60 == 0 {
            tracing::info!("Frame {}", self.frame_count);
        }
    }
}

fn main() {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    tracing::info!("flui-platform + wgpu integration example");

    let platform = current_platform().expect("Failed to initialize platform");
    tracing::info!("Platform: {}", platform.name());

    let platform_for_ready = platform.clone();

    platform.run(Box::new(move || {
        let options = WindowOptions {
            title: "FLUI Platform + wgpu".to_string(),
            size: Size::new(px(800.0), px(600.0)),
            resizable: true,
            visible: true,
            decorated: true,
            min_size: None,
            max_size: None,
        };

        let window: Arc<dyn PlatformWindow> = Arc::from(
            platform_for_ready
                .open_window(options)
                .expect("Failed to open window"),
        );

        tracing::info!(
            "Window created: {:?} @ {:.1}x scale",
            window.physical_size(),
            window.scale_factor()
        );

        // Create GPU state from PlatformWindow
        let gpu = Arc::new(Mutex::new(GpuState::new(&window)));

        // Register frame callback
        let gpu_for_frame = Arc::clone(&gpu);
        window.on_request_frame(Box::new(move || {
            gpu_for_frame.lock().unwrap().render_frame();
        }));

        // Register resize callback
        let gpu_for_resize = Arc::clone(&gpu);
        window.on_resize(Box::new(move |size, scale_factor| {
            let width = (size.width.0 * scale_factor) as u32;
            let height = (size.height.0 * scale_factor) as u32;
            gpu_for_resize.lock().unwrap().resize(width, height);
        }));

        // Register input callback for logging
        window.on_input(Box::new(|input| {
            tracing::trace!("Input: {:?}", input);
            flui_platform::traits::DispatchEventResult {
                propagate: true,
                default_prevented: false,
            }
        }));

        // Request first frame
        window.request_redraw();

        tracing::info!("Setup complete - window is live with wgpu rendering");
    }));

    tracing::info!("Application finished");
}
