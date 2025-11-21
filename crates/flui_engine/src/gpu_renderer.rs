//! GPU Renderer - High-level GPU rendering abstraction
//!
//! Encapsulates all wgpu resources and rendering logic, providing a clean
//! interface for the application layer without exposing low-level GPU details.
//!
//! # Architecture
//!
//! ```text
//! FluiApp (application layer)
//!     ├─ winit::Window (window management)
//!     ├─ wgpu::Instance (creates Surface from Window)
//!     └─ GpuRenderer (rendering layer - NO window knowledge!)
//!         ├─ wgpu::Surface (passed from app)
//!         ├─ wgpu::Device
//!         ├─ wgpu::Queue
//!         ├─ SurfaceConfiguration
//!         └─ WgpuPainter
//! ```
//!
//! # Benefits
//!
//! - **Encapsulation**: All GPU details hidden from app layer
//! - **Separation of Concerns**: Engine doesn't know about windows/winit
//! - **Testability**: Easy to mock surface for testing
//! - **Future-proof**: Easy to add new rendering backends

use crate::layer::CanvasLayer;
use crate::painter::WgpuPainter;
use crate::renderer::WgpuRenderer as WgpuRendererWrapper;

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
/// // Create surface in app layer (flui_app)
/// let instance = wgpu::Instance::default();
/// let surface = instance.create_surface(window)?;
/// let size = window.inner_size();
///
/// // Pass surface to engine (NO window reference!)
/// let mut renderer = GpuRenderer::new(surface, size.width, size.height);
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
    /// Create a new GPU renderer with a window (async version, PREFERRED)
    ///
    /// This method creates the surface internally from the window, ensuring that
    /// the surface is created from the same wgpu::Instance that GpuRenderer uses.
    /// This avoids lifetime and instance mismatch issues.
    ///
    /// # Arguments
    ///
    /// * `window` - Window handle (Arc<Window> from winit)
    /// * `width` - Initial surface width in pixels
    /// * `height` - Initial surface height in pixels
    ///
    /// # Panics
    ///
    /// Panics if GPU initialization fails (no adapter, device creation fails, etc.)
    pub async fn new_async_with_window<W>(window: W, width: u32, height: u32) -> Self
    where
        W: Into<wgpu::SurfaceTarget<'static>>,
    {
        // Create wgpu instance with platform-specific backends
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            #[cfg(feature = "webgpu")]
            backends: wgpu::Backends::GL | wgpu::Backends::BROWSER_WEBGPU,
            #[cfg(all(feature = "android", not(feature = "webgpu")))]
            backends: wgpu::Backends::VULKAN, // Vulkan mandatory for Android
            #[cfg(all(feature = "ios", not(feature = "webgpu")))]
            backends: wgpu::Backends::METAL, // Metal mandatory for iOS
            #[cfg(all(
                feature = "desktop",
                not(any(feature = "webgpu", feature = "android", feature = "ios"))
            ))]
            backends: wgpu::Backends::all(), // Auto-detect on desktop
            #[cfg(not(any(
                feature = "desktop",
                feature = "android",
                feature = "ios",
                feature = "webgpu"
            )))]
            backends: wgpu::Backends::all(), // Fallback
            ..Default::default()
        });

        // Create surface from window using raw_window_handle
        // IMPORTANT: Surface is created from the SAME instance that we'll use for adapter/device
        let surface = instance
            .create_surface(window)
            .expect("Failed to create surface from window");

        // Delegate to existing new_async implementation
        Self::new_async_impl(instance, surface, width, height).await
    }

    /// Create a new GPU renderer with an existing surface (async version for WASM)
    ///
    /// In WebAssembly, we can't use pollster::block_on because the browser's
    /// event loop doesn't support blocking. Use this method instead.
    ///
    /// # Arguments
    ///
    /// * `surface` - Pre-created wgpu surface (created by app layer from window)
    /// * `width` - Initial surface width in pixels
    /// * `height` - Initial surface height in pixels
    ///
    /// # Panics
    ///
    /// Panics if GPU initialization fails (no adapter, device creation fails, etc.)
    ///
    /// # Note
    ///
    /// DEPRECATED for desktop/mobile - use `new_async_with_window` instead to avoid
    /// instance mismatch issues. Only use this for WASM where surface lifetime is managed externally.
    pub async fn new_async(surface: wgpu::Surface<'static>, width: u32, height: u32) -> Self {
        // Create wgpu instance with platform-specific backends
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            #[cfg(feature = "webgpu")]
            backends: wgpu::Backends::GL | wgpu::Backends::BROWSER_WEBGPU,
            #[cfg(all(feature = "android", not(feature = "webgpu")))]
            backends: wgpu::Backends::VULKAN, // Vulkan mandatory for Android
            #[cfg(all(feature = "ios", not(feature = "webgpu")))]
            backends: wgpu::Backends::METAL, // Metal mandatory for iOS
            #[cfg(all(
                feature = "desktop",
                not(any(feature = "webgpu", feature = "android", feature = "ios"))
            ))]
            backends: wgpu::Backends::all(), // Auto-detect on desktop
            #[cfg(not(any(
                feature = "desktop",
                feature = "android",
                feature = "ios",
                feature = "webgpu"
            )))]
            backends: wgpu::Backends::all(), // Fallback
            ..Default::default()
        });

        Self::new_async_impl(instance, surface, width, height).await
    }

    /// Internal implementation - creates renderer from instance and surface
    async fn new_async_impl(
        instance: wgpu::Instance,
        surface: wgpu::Surface<'static>,
        width: u32,
        height: u32,
    ) -> Self {
        // Request adapter (async)
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .expect("Failed to find suitable GPU adapter");

        tracing::debug!(adapter_info = ?adapter.get_info(), "GPU Adapter initialized");
        tracing::debug!(backend = ?adapter.get_info().backend, "GPU Backend");

        // Request device and queue (async)
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                required_features: wgpu::Features::empty(),
                #[cfg(feature = "webgpu")]
                required_limits: wgpu::Limits::downlevel_webgl2_defaults(),
                #[cfg(all(feature = "mobile-gpu-limits", not(feature = "webgpu")))]
                required_limits: wgpu::Limits {
                    // Modern mobile GPUs (Adreno 6xx+, Mali G7x+, Apple A12+) support 8192x8192+
                    // This covers all flagship and mid-range devices from 2018+
                    max_texture_dimension_2d: 8192,
                    max_texture_dimension_3d: 2048,

                    // Conservative buffer limits for broad compatibility
                    max_storage_buffers_per_shader_stage: 4,
                    max_uniform_buffer_binding_size: 16 << 10, // 16KB
                    max_storage_buffer_binding_size: 128 << 20, // 128MB

                    // Use default limits for other parameters (more permissive than downlevel)
                    ..wgpu::Limits::default()
                },
                #[cfg(not(any(feature = "webgpu", feature = "mobile-gpu-limits")))]
                required_limits: wgpu::Limits::default(),
                label: Some("FLUI GPU Device"),
                memory_hints: wgpu::MemoryHints::default(),
                trace: Default::default(),
            })
            .await
            .expect("Failed to create GPU device");

        // Configure surface
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface.get_capabilities(&adapter).formats[0],
            width,
            height,
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

        tracing::debug!(
            width = config.width,
            height = config.height,
            format = ?config.format,
            "GPU renderer created"
        );

        Self {
            surface,
            device,
            queue,
            config,
            painter: Some(painter),
        }
    }

    /// Create a new GPU renderer with an existing surface
    ///
    /// Initializes all wgpu resources: instance, adapter, device, queue, and painter.
    ///
    /// # Arguments
    ///
    /// * `surface` - Pre-created wgpu surface (created by app layer from window)
    /// * `width` - Initial surface width in pixels
    /// * `height` - Initial surface height in pixels
    ///
    /// # Panics
    ///
    /// Panics if GPU initialization fails (no adapter, device creation fails, etc.)
    ///
    /// # Note
    ///
    /// On WebAssembly, use `new_async()` instead as this method uses `pollster::block_on`
    /// which doesn't work in browser environments.
    pub fn new(surface: wgpu::Surface<'static>, width: u32, height: u32) -> Self {
        // Create wgpu instance
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            #[cfg(target_os = "android")]
            backends: wgpu::Backends::GL, // Use OpenGL ES on Android (Vulkan crashes on x86_64 emulator)
            #[cfg(not(target_os = "android"))]
            backends: wgpu::Backends::all(),
            ..Default::default()
        });

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

        // Configure surface
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface.get_capabilities(&adapter).formats[0],
            width,
            height,
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

        tracing::debug!(
            width = config.width,
            height = config.height,
            format = ?config.format,
            "GPU renderer created"
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

            tracing::debug!(width = width, height = height, "GPU renderer resized");
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
        tracing::trace!("GpuRenderer::render() START");

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
