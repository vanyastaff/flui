//! wgpu embedder - winit + wgpu integration
//!
//! This embedder creates a window using winit and renders using wgpu.
//! It bridges platform events to the binding layer and renders scenes to the GPU.

use crate::binding::WidgetsFlutterBinding;
use flui_types::{
    constraints::BoxConstraints,
    events::{PointerButton, PointerDeviceKind, PointerEventData},
    Offset, Size,
};
use std::sync::Arc;
use std::time::Instant;
use winit::{
    event::*,
    event_loop::{ControlFlow, EventLoop},
    window::Window,
};

/// wgpu embedder for FLUI apps
///
/// # Architecture
///
/// ```text
/// WgpuEmbedder
///   ├─ Window (winit)
///   ├─ GPU (wgpu::Device, Queue, Surface)
///   └─ Binding (WidgetsFlutterBinding)
/// ```
///
/// # Event Flow
///
/// ```text
/// winit events → handle_event() → GestureBinding → EventRouter → Widgets
/// vsync → render_frame() → SchedulerBinding → PipelineOwner → Scene → wgpu
/// ```
pub struct WgpuEmbedder {
    /// Binding to framework
    binding: Arc<WidgetsFlutterBinding>,

    /// winit window
    window: Arc<Window>,

    /// wgpu device (GPU access)
    device: wgpu::Device,

    /// wgpu queue (command submission)
    queue: wgpu::Queue,

    /// wgpu surface (window rendering target)
    surface: wgpu::Surface<'static>,

    /// Surface configuration
    config: wgpu::SurfaceConfiguration,

    /// App start time (for timestamps)
    start_time: Instant,
}

impl WgpuEmbedder {
    /// Create a new wgpu embedder
    ///
    /// # Parameters
    ///
    /// - `binding`: The framework binding (usually from `WidgetsFlutterBinding::ensure_initialized()`)
    /// - `event_loop`: The winit event loop
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let binding = WidgetsFlutterBinding::ensure_initialized();
    /// let event_loop = EventLoop::new();
    /// let embedder = WgpuEmbedder::new(binding, &event_loop).await;
    /// ```
    pub async fn new(
        binding: Arc<WidgetsFlutterBinding>,
        event_loop: &EventLoop<()>,
    ) -> Self {
        tracing::info!("Initializing wgpu embedder");

        // 1. Create window
        let window_attributes = Window::default_attributes()
            .with_title("FLUI App")
            .with_inner_size(winit::dpi::PhysicalSize::new(800, 600));

        let window = Arc::new(
            event_loop
                .create_window(window_attributes)
                .expect("Failed to create window"),
        );

        tracing::debug!("Window created");

        // 2. Initialize wgpu
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });

        let surface = instance
            .create_surface(Arc::clone(&window))
            .expect("Failed to create surface");

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .expect("Failed to find suitable GPU adapter");

        tracing::info!(
            adapter = ?adapter.get_info(),
            "GPU adapter found"
        );

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("FLUI Device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                memory_hints: Default::default(),
                trace: Default::default(),
            })
            .await
            .expect("Failed to create device");

        tracing::debug!("wgpu device created");

        // 3. Configure surface
        let size = window.inner_size();
        let surface_caps = surface.get_capabilities(&adapter);
        let surface_format = surface_caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(surface_caps.formats[0]);

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: size.width,
            height: size.height,
            present_mode: wgpu::PresentMode::Fifo, // VSync
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        tracing::info!(
            width = config.width,
            height = config.height,
            format = ?config.format,
            "Surface configured"
        );

        Self {
            binding,
            window,
            device,
            queue,
            surface,
            config,
            start_time: Instant::now(),
        }
    }

    /// Run the event loop
    ///
    /// This method blocks until the window is closed.
    pub fn run(mut self, event_loop: EventLoop<()>) -> ! {
        tracing::info!("Starting event loop");

        event_loop
            .run(move |event, elwt| {
                elwt.set_control_flow(ControlFlow::Poll);

                match event {
                    Event::AboutToWait => {
                        // Request redraw every frame
                        self.window.request_redraw();
                    }

                    Event::WindowEvent { event, .. } => match event {
                        WindowEvent::RedrawRequested => {
                            self.render_frame();
                        }
                        other => {
                            self.handle_window_event(other, elwt);
                        }
                    },

                    _ => {}
                }
            })
            .expect("Event loop error");

        // Unreachable, but needed to satisfy !
        std::process::exit(0)
    }

    /// Handle window events
    fn handle_window_event(&mut self, event: WindowEvent, elwt: &winit::event_loop::ActiveEventLoop) {
        match event {
            WindowEvent::CloseRequested => {
                tracing::info!("Window close requested");
                elwt.exit();
            }

            WindowEvent::Resized(size) => {
                tracing::debug!(width = size.width, height = size.height, "Window resized");

                self.config.width = size.width;
                self.config.height = size.height;
                self.surface.configure(&self.device, &self.config);
            }

            WindowEvent::CursorMoved { position, .. } => {
                // Convert to FLUI event
                let data = PointerEventData::new(
                    Offset::new(position.x as f32, position.y as f32),
                    PointerDeviceKind::Mouse,
                );

                let event = flui_types::Event::Pointer(flui_types::PointerEvent::Move(data));
                self.binding.gesture.handle_event(event);
            }

            WindowEvent::MouseInput { state, button, .. } => {
                // Get current cursor position
                // TODO: Track cursor position properly
                let position = Offset::ZERO;

                let data = PointerEventData::new(position, PointerDeviceKind::Mouse)
                    .with_button(convert_mouse_button(button));

                let event = match state {
                    ElementState::Pressed => {
                        flui_types::Event::Pointer(flui_types::PointerEvent::Down(data))
                    }
                    ElementState::Released => {
                        flui_types::Event::Pointer(flui_types::PointerEvent::Up(data))
                    }
                };

                self.binding.gesture.handle_event(event);
            }

            _ => {
                // TODO: Handle other events (keyboard, scroll, etc.)
            }
        }
    }

    /// Render a frame
    fn render_frame(&mut self) {
        let timestamp = self.start_time.elapsed();

        // 1. Begin frame (scheduler callbacks)
        self.binding.scheduler.handle_begin_frame(timestamp);

        // 2. Draw frame (build + layout + paint)
        let constraints = BoxConstraints::tight(Size::new(
            self.config.width as f32,
            self.config.height as f32,
        ));

        let scene = self.binding.renderer.draw_frame(constraints);

        // 3. Render scene to wgpu
        match self.surface.get_current_texture() {
            Ok(output) => {
                let view = output
                    .texture
                    .create_view(&wgpu::TextureViewDescriptor::default());

                let mut encoder = self
                    .device
                    .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                        label: Some("Render Encoder"),
                    });

                {
                    let _render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("Render Pass"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: &view,
                            resolve_target: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Clear(wgpu::Color {
                                    r: 1.0,
                                    g: 1.0,
                                    b: 1.0,
                                    a: 1.0,
                                }),
                                store: wgpu::StoreOp::Store,
                            },
                        })],
                        depth_stencil_attachment: None,
                        occlusion_query_set: None,
                        timestamp_writes: None,
                    });

                    // TODO: Render scene layers to wgpu
                    // For now, just clear to white
                    tracing::trace!(size = ?scene.size(), "Scene rendered");
                }

                self.queue.submit(std::iter::once(encoder.finish()));
                output.present();
            }
            Err(wgpu::SurfaceError::Lost) => {
                tracing::warn!("Surface lost, reconfiguring");
                self.surface.configure(&self.device, &self.config);
            }
            Err(wgpu::SurfaceError::OutOfMemory) => {
                tracing::error!("Out of memory");
                std::process::exit(1);
            }
            Err(e) => {
                tracing::error!(error = ?e, "Surface error");
            }
        }

        // 4. Post-frame callbacks
        self.binding.scheduler.handle_draw_frame();
    }
}

/// Convert winit mouse button to FLUI pointer button
fn convert_mouse_button(button: MouseButton) -> PointerButton {
    match button {
        MouseButton::Left => PointerButton::Primary,
        MouseButton::Right => PointerButton::Secondary,
        MouseButton::Middle => PointerButton::Middle,
        MouseButton::Other(n) => PointerButton::Other(n as u8),
        _ => PointerButton::Primary, // Default for unknown buttons
    }
}
