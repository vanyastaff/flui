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

// CanvasLayer now accessed via Layer enum
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

    /// Offscreen renderer for shader masks
    offscreen_renderer: Option<crate::layer::offscreen_renderer::OffscreenRenderer>,
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
    /// * `window` - Window handle (`Arc<Window>` from winit)
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

        // Create offscreen renderer for shader masks
        let mut offscreen_renderer = crate::layer::offscreen_renderer::OffscreenRenderer::new(
            std::sync::Arc::new(device.clone()),
            std::sync::Arc::new(queue.clone()),
            config.format,
        );

        // Pre-warm shader pipelines for better first-frame performance
        offscreen_renderer.warmup();

        tracing::debug!(
            width = config.width,
            height = config.height,
            format = ?config.format,
            "GPU renderer created with shader mask support"
        );

        Self {
            surface,
            device,
            queue,
            config,
            painter: Some(painter),
            offscreen_renderer: Some(offscreen_renderer),
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

        // Create offscreen renderer for shader masks
        let mut offscreen_renderer = crate::layer::offscreen_renderer::OffscreenRenderer::new(
            std::sync::Arc::new(device.clone()),
            std::sync::Arc::new(queue.clone()),
            config.format,
        );

        // Pre-warm shader pipelines for better first-frame performance
        offscreen_renderer.warmup();

        tracing::debug!(
            width = config.width,
            height = config.height,
            format = ?config.format,
            "GPU renderer created with shader mask support"
        );

        Self {
            surface,
            device,
            queue,
            config,
            painter: Some(painter),
            offscreen_renderer: Some(offscreen_renderer),
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
    pub fn render(&mut self, layer: &crate::layer::Layer) -> Result<(), RenderError> {
        use crate::layer::Layer;

        // Dispatch to appropriate rendering method based on layer type
        match layer {
            Layer::Canvas(canvas_layer) => self.render_canvas_layer(canvas_layer),
            Layer::ShaderMask(shader_mask_layer) => {
                self.render_shader_mask_layer(shader_mask_layer)
            }
            Layer::BackdropFilter(backdrop_filter_layer) => {
                self.render_backdrop_filter_layer(backdrop_filter_layer)
            }
            Layer::Cached(cached_layer) => {
                // Render the wrapped layer
                // The CachedLayer handles its own caching logic
                let inner = cached_layer.inner();
                self.render(&inner)
            }
        }
    }

    /// Render a ShaderMaskLayer (offscreen rendering + shader mask + composite)
    ///
    /// # Architecture
    ///
    /// ```text
    /// 1. Create offscreen texture (from TexturePool)
    /// 2. Render child content to offscreen texture
    /// 3. Apply shader mask (WGSL shader)
    /// 4. Composite masked result to framebuffer
    /// 5. Return texture to pool
    /// ```
    ///
    /// # TODO: GPU Implementation
    ///
    /// This is a placeholder showing the architecture. Full implementation requires:
    ///
    /// 1. **Offscreen Texture Creation**:
    ///    ```rust,ignore
    ///    let texture = device.create_texture(&wgpu::TextureDescriptor {
    ///        size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
    ///        format: wgpu::TextureFormat::Rgba8UnormSrgb,
    ///        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
    ///        ..Default::default()
    ///    });
    ///    ```
    ///
    /// 2. **Shader Pipeline Creation** (use ShaderCache):
    ///    ```rust,ignore
    ///    let shader_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
    ///        label: Some("Shader Mask Shader"),
    ///        source: wgpu::ShaderSource::Wgsl(shader_cache.get_source(shader_type).into()),
    ///    });
    ///
    ///    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
    ///        vertex: wgpu::VertexState {
    ///            module: &shader_module,
    ///            entry_point: "vs_main",
    ///            buffers: &[fullscreen_quad_layout],
    ///        },
    ///        fragment: Some(wgpu::FragmentState {
    ///            module: &shader_module,
    ///            entry_point: "fs_main",
    ///            targets: &[wgpu::ColorTargetState {
    ///                format: surface_format,
    ///                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
    ///                write_mask: wgpu::ColorWrites::ALL,
    ///            }],
    ///        }),
    ///        ..Default::default()
    ///    });
    ///    ```
    ///
    /// 3. **Bind Group for Texture + Uniforms**:
    ///    ```rust,ignore
    ///    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
    ///        layout: &bind_group_layout,
    ///        entries: &[
    ///            wgpu::BindGroupEntry {
    ///                binding: 0,
    ///                resource: wgpu::BindingResource::TextureView(&texture_view),
    ///            },
    ///            wgpu::BindGroupEntry {
    ///                binding: 1,
    ///                resource: wgpu::BindingResource::Sampler(&sampler),
    ///            },
    ///            wgpu::BindGroupEntry {
    ///                binding: 2,
    ///                resource: uniform_buffer.as_entire_binding(),
    ///            },
    ///        ],
    ///    });
    ///    ```
    ///
    /// 4. **Render Pass Execution**:
    ///    ```rust,ignore
    ///    let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
    ///        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
    ///            view: &view,
    ///            ops: wgpu::Operations {
    ///                load: wgpu::LoadOp::Load,
    ///                store: wgpu::StoreOp::Store,
    ///            },
    ///            ..Default::default()
    ///        })],
    ///        ..Default::default()
    ///    });
    ///
    ///    render_pass.set_pipeline(&pipeline);
    ///    render_pass.set_bind_group(0, &bind_group, &[]);
    ///    render_pass.set_vertex_buffer(0, fullscreen_quad_buffer.slice(..));
    ///    render_pass.draw(0..6, 0..1); // 6 vertices for fullscreen quad
    ///    ```
    fn render_shader_mask_layer(
        &mut self,
        shader_mask_layer: &crate::layer::ShaderMaskLayer,
    ) -> Result<(), RenderError> {
        tracing::debug!(
            bounds = ?shader_mask_layer.bounds(),
            shader = ?shader_mask_layer.shader,
            "Rendering shader mask layer"
        );

        // TODO: Full implementation requires child layer reference
        // Current ShaderMaskLayer doesn't store child layer - this is architectural limitation
        // For now, we can only validate the integration with a placeholder child texture
        //
        // To complete implementation, ShaderMaskLayer needs to be refactored to:
        // 1. Store child layer reference: `child: Arc<Layer>`
        // 2. Render child to offscreen texture first
        // 3. Pass child texture to offscreen_renderer.render_masked()
        //
        // Architecture note: This would align with Flutter's ShaderMask widget which
        // wraps a child widget and applies shader as mask.

        let offscreen_renderer = self
            .offscreen_renderer
            .as_mut()
            .ok_or_else(|| RenderError::PainterError("OffscreenRenderer not initialized".into()))?;

        // For demonstration: create a dummy child texture
        // In real implementation, this would be the pre-rendered child content
        let dummy_child_texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Dummy Child Texture"),
            size: wgpu::Extent3d {
                width: shader_mask_layer.bounds().width() as u32,
                height: shader_mask_layer.bounds().height() as u32,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.config.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });

        // Call offscreen renderer with shader mask parameters
        let _masked_result = offscreen_renderer.render_masked(
            shader_mask_layer.bounds(),
            &shader_mask_layer.shader,
            shader_mask_layer.blend_mode,
            &dummy_child_texture,
        );

        tracing::debug!("Shader mask rendering complete (with dummy child texture)");

        // TODO: Composite masked_result to framebuffer
        // This requires access to current frame's texture view
        // For now, the integration is demonstrated but not fully functional

        Ok(())
    }

    /// Render a BackdropFilterLayer (framebuffer capture + filter + composite)
    ///
    /// # Architecture
    ///
    /// ```text
    /// 1. Capture current framebuffer in bounds
    /// 2. Apply image filter (blur, color adjustments)
    /// 3. Render filtered backdrop to framebuffer
    /// 4. Composite with blend mode
    /// ```
    ///
    /// # Implementation
    ///
    /// For blur filters, uses two-pass separable Gaussian blur:
    /// - Horizontal pass: blur in X direction
    /// - Vertical pass: blur in Y direction (on horizontally-blurred result)
    ///
    /// This is more efficient than 2D blur (O(n) vs O(n²) per pixel).
    fn render_backdrop_filter_layer(
        &mut self,
        backdrop_filter_layer: &crate::layer::BackdropFilterLayer,
    ) -> Result<(), RenderError> {
        use flui_types::painting::ImageFilter;

        tracing::debug!(
            bounds = ?backdrop_filter_layer.bounds(),
            filter = ?backdrop_filter_layer.filter,
            "Rendering backdrop filter layer"
        );

        // Phase 2.2: Framebuffer capture
        // For now, we create a placeholder backdrop texture
        // In full implementation, this would capture the actual framebuffer content
        let bounds = backdrop_filter_layer.bounds();
        let _backdrop_texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Backdrop Capture Texture"),
            size: wgpu::Extent3d {
                width: bounds.width().max(1.0) as u32,
                height: bounds.height().max(1.0) as u32,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.config.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });

        // Phase 2.3: Apply image filter
        match &backdrop_filter_layer.filter {
            ImageFilter::Blur { sigma_x, sigma_y } => {
                tracing::debug!(sigma_x, sigma_y, "Applying Gaussian blur filter");

                // Two-pass separable Gaussian blur
                // Pass 1: Horizontal blur
                // Pass 2: Vertical blur on horizontally-blurred result

                // For now, we log the blur parameters
                // Full GPU compute shader implementation will be added in next iteration
                tracing::info!(
                    "Blur filter configured: sigma_x={}, sigma_y={}",
                    sigma_x,
                    sigma_y
                );
            }
            ImageFilter::Dilate { radius } => {
                tracing::debug!(radius, "Dilate filter requested (not yet implemented)");
            }
            ImageFilter::Erode { radius } => {
                tracing::debug!(radius, "Erode filter requested (not yet implemented)");
            }
            ImageFilter::Matrix(_) => {
                tracing::debug!("Matrix filter requested (not yet implemented)");
            }
            ImageFilter::ColorAdjust(_) => {
                tracing::debug!("ColorAdjust filter requested (not yet implemented)");
            }
            ImageFilter::Compose(_) => {
                tracing::debug!("Compose filter requested (not yet implemented)");
            }
            #[cfg(debug_assertions)]
            ImageFilter::OverflowIndicator { .. } => {
                tracing::debug!("OverflowIndicator filter requested (not yet implemented)");
            }
        }

        // Phase 2.4: Composite filtered backdrop
        // The filtered backdrop would be rendered to the framebuffer here
        // With blend mode applied

        tracing::debug!(
            blend_mode = ?backdrop_filter_layer.blend_mode,
            "Backdrop filter rendering complete (infrastructure ready, GPU compute pending)"
        );

        Ok(())
    }

    /// Internal method to render a CanvasLayer
    fn render_canvas_layer(
        &mut self,
        layer: &crate::layer::CanvasLayer,
    ) -> Result<(), RenderError> {
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
