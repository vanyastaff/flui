//! GPU Renderer - High-level GPU rendering abstraction
//!
//! Encapsulates all wgpu resources and rendering logic, providing a clean
//! interface for the application layer without exposing low-level GPU details.
//!
//! # Architecture
//!
//! ```text
//! FluiApp (high-level)
//!     ↓
//! GpuRenderer (this module)
//!     ├─ wgpu::Surface
//!     ├─ wgpu::Device
//!     ├─ wgpu::Queue
//!     ├─ SurfaceConfiguration
//!     └─ WgpuPainter
//! ```
//!
//! # Benefits
//!
//! - **Encapsulation**: All GPU details hidden from app layer
//! - **Separation of Concerns**: Clear boundary between UI and GPU layers
//! - **Testability**: Easy to mock for testing
//! - **Future-proof**: Easy to add new rendering backends

use crate::layer::CanvasLayer;
use crate::painter::WgpuPainter;
use crate::renderer::WgpuRenderer as WgpuRendererWrapper;
use std::sync::Arc;
use winit::window::Window;

/// High-level GPU rendering abstraction
///
/// Manages all wgpu resources (device, queue, surface, painter) and provides
/// a clean interface for rendering without exposing low-level GPU details.
///
/// # Example
///
/// ```rust,ignore
/// use flui_engine::GpuRenderer;
/// use std::sync::Arc;
///
/// let window = /* your winit window */;
/// let mut renderer = GpuRenderer::new(window);
///
/// // Resize on window resize
/// renderer.resize(1920, 1080);
///
/// // Render a layer
/// let layer = /* your CanvasLayer */;
/// renderer.render(&layer)?;
/// ```
pub struct GpuRenderer {
    /// wgpu surface (render target)
    surface: wgpu::Surface<'static>,

    /// wgpu device (GPU handle)
    device: wgpu::Device,

    /// wgpu queue (command submission)
    queue: wgpu::Queue,

    /// Surface configuration
    config: wgpu::SurfaceConfiguration,

    /// GPU painter (persistent, reused every frame)
    /// Wrapped in Option to allow temporary ownership transfer without allocation
    painter: Option<WgpuPainter>,
}

impl GpuRenderer {
    /// Create a new GPU renderer for a window (async version for WASM)
    ///
    /// In WebAssembly, we can't use pollster::block_on because the browser's
    /// event loop doesn't support blocking. Use this method instead.
    ///
    /// # Arguments
    ///
    /// * `window` - The window to render to
    ///
    /// # Panics
    ///
    /// Panics if GPU initialization fails (no adapter, device creation fails, etc.)
    pub async fn new_async(window: Arc<Window>) -> Self {
        // Create wgpu instance
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            #[cfg(target_arch = "wasm32")]
            backends: wgpu::Backends::GL | wgpu::Backends::BROWSER_WEBGPU,
            #[cfg(not(target_arch = "wasm32"))]
            backends: wgpu::Backends::all(),
            ..Default::default()
        });

        // Create surface
        let surface = instance
            .create_surface(Arc::clone(&window))
            .expect("Failed to create surface");

        // Request adapter (async)
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .expect("Failed to find suitable GPU adapter");

        // Request device and queue (async)
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                required_features: wgpu::Features::empty(),
                #[cfg(target_arch = "wasm32")]
                required_limits: wgpu::Limits::downlevel_webgl2_defaults(),
                #[cfg(not(target_arch = "wasm32"))]
                required_limits: wgpu::Limits::default(),
                label: Some("FLUI GPU Device"),
                memory_hints: wgpu::MemoryHints::default(),
                trace: Default::default(),
            })
            .await
            .expect("Failed to create GPU device");

        // Get window size and configure surface
        let size = window.inner_size();
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface.get_capabilities(&adapter).formats[0],
            width: size.width,
            height: size.height,
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        // Create GPU painter (persistent, created once)
        let painter = WgpuPainter::new(
            device.clone(),
            queue.clone(),
            config.format,
            (config.width, config.height),
        );

        tracing::info!(
            "GPU renderer initialized (async): {}x{}, format={:?}",
            config.width,
            config.height,
            config.format
        );

        Self {
            surface,
            device,
            queue,
            config,
            painter: Some(painter),
        }
    }

    /// Create a new GPU renderer for a window
    ///
    /// Initializes all wgpu resources: instance, surface, adapter, device, queue, and painter.
    ///
    /// # Arguments
    ///
    /// * `window` - The window to render to
    ///
    /// # Panics
    ///
    /// Panics if GPU initialization fails (no adapter, device creation fails, etc.)
    ///
    /// # Note
    ///
    /// On WebAssembly, use `new_async()` instead as this method uses `pollster::block_on`
    /// which doesn't work in browser environments.
    pub fn new(window: Arc<Window>) -> Self {
        // Create wgpu instance
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });

        // Create surface
        let surface = instance
            .create_surface(Arc::clone(&window))
            .expect("Failed to create surface");

        // Request adapter
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::default(),
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }))
        .expect("Failed to find suitable GPU adapter");

        // Request device and queue
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            label: Some("FLUI GPU Device"),
            memory_hints: wgpu::MemoryHints::default(),
            trace: Default::default(),
        }))
        .expect("Failed to create GPU device");

        // Get window size and configure surface
        let size = window.inner_size();
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface.get_capabilities(&adapter).formats[0],
            width: size.width,
            height: size.height,
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        // Create GPU painter (persistent, created once)
        let painter = WgpuPainter::new(
            device.clone(),
            queue.clone(),
            config.format,
            (config.width, config.height),
        );

        tracing::info!(
            "GPU renderer initialized: {}x{}, format={:?}",
            config.width,
            config.height,
            config.format
        );

        Self {
            surface,
            device,
            queue,
            config,
            painter: Some(painter),
        }
    }

    /// Resize the rendering surface
    ///
    /// Updates surface configuration and painter viewport.
    /// Call this when the window is resized.
    ///
    /// # Arguments
    ///
    /// * `width` - New width in pixels
    /// * `height` - New height in pixels
    pub fn resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.config.width = width;
            self.config.height = height;
            self.surface.configure(&self.device, &self.config);

            if let Some(painter) = &mut self.painter {
                painter.resize(width, height);
            }

            tracing::info!("GPU renderer resized to {}x{}", width, height);
        }
    }

    /// Render a layer to the surface
    ///
    /// Acquires the current frame, renders the layer using the painter,
    /// and presents the result.
    ///
    /// # Arguments
    ///
    /// * `layer` - The layer to render
    ///
    /// # Returns
    ///
    /// `Ok(())` on success, or `RenderError` if rendering fails
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// match renderer.render(&layer) {
    ///     Ok(()) => {},
    ///     Err(RenderError::SurfaceLost) => {
    ///         // Surface was lost, will recover next frame
    ///     }
    ///     Err(e) => {
    ///         eprintln!("Render error: {:?}", e);
    ///     }
    /// }
    /// ```
    pub fn render(&mut self, layer: &CanvasLayer) -> Result<(), RenderError> {
        // Get current frame
        let frame = self.surface.get_current_texture().map_err(|e| match e {
            wgpu::SurfaceError::Lost => {
                tracing::warn!("Surface lost, reconfiguring...");
                self.surface.configure(&self.device, &self.config);
                RenderError::SurfaceLost
            }
            wgpu::SurfaceError::Outdated => {
                tracing::warn!("Surface outdated, reconfiguring...");
                self.surface.configure(&self.device, &self.config);
                RenderError::SurfaceOutdated
            }
            wgpu::SurfaceError::OutOfMemory => {
                tracing::error!("Out of GPU memory!");
                RenderError::OutOfMemory
            }
            wgpu::SurfaceError::Timeout => {
                tracing::warn!("Surface timeout");
                RenderError::Timeout
            }
            wgpu::SurfaceError::Other => {
                tracing::error!("Unknown surface error occurred");
                RenderError::PainterError("Unknown surface error".to_string())
            }
        })?;

        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        // Create command encoder
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("FLUI Render Encoder"),
            });

        // Clear screen
        {
            let _render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Clear Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.1,
                            g: 0.1,
                            b: 0.1,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
        }

        // CRITICAL: Zero-allocation rendering via painter reuse!
        // Take ownership temporarily (field becomes None, zero allocation)
        let painter = self
            .painter
            .take()
            .expect("Painter should always exist during render");

        // Create renderer wrapper (stack allocation, just one pointer field)
        let mut renderer_wrapper = WgpuRendererWrapper::new(painter);
        layer.render(&mut renderer_wrapper);

        // Extract painter and render accumulated commands to GPU
        let mut painter = renderer_wrapper.into_painter();
        painter
            .render(&view, &mut encoder)
            .map_err(|e| RenderError::PainterError(e.to_string()))?;

        // Put painter back (zero allocation, just moves Option)
        self.painter = Some(painter);

        // Submit commands and present
        self.queue.submit(std::iter::once(encoder.finish()));
        frame.present();

        Ok(())
    }

    /// Get current viewport size
    ///
    /// Returns (width, height) in pixels
    pub fn size(&self) -> (u32, u32) {
        (self.config.width, self.config.height)
    }

    /// Get the surface texture format
    pub fn format(&self) -> wgpu::TextureFormat {
        self.config.format
    }
}

/// GPU rendering errors
#[derive(Debug)]
pub enum RenderError {
    /// Surface was lost and needs reconfiguration
    SurfaceLost,

    /// Surface is outdated and needs reconfiguration
    SurfaceOutdated,

    /// Out of GPU memory
    OutOfMemory,

    /// Surface acquisition timed out
    Timeout,

    /// Error from the painter during rendering
    PainterError(String),
}

impl std::fmt::Display for RenderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RenderError::SurfaceLost => write!(f, "Surface was lost"),
            RenderError::SurfaceOutdated => write!(f, "Surface is outdated"),
            RenderError::OutOfMemory => write!(f, "Out of GPU memory"),
            RenderError::Timeout => write!(f, "Surface acquisition timed out"),
            RenderError::PainterError(msg) => write!(f, "Painter error: {}", msg),
        }
    }
}

impl std::error::Error for RenderError {}
