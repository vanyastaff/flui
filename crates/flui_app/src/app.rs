//! FluiApp - Main application structure
//!
//! This module provides the FluiApp struct, which manages the application lifecycle,
//! element tree, and rendering pipeline integration with wgpu.

use flui_core::foundation::ElementId;
use flui_core::pipeline::PipelineOwner;
use flui_core::view::{AnyView, BuildContext};
use flui_core::{BoxedLayer, Size};
use flui_engine::EventRouter;
use flui_types::BoxConstraints;
use std::sync::Arc;
use winit::window::Window;

/// Performance statistics for debugging and optimization
#[derive(Debug, Default)]
pub(crate) struct FrameStats {
    /// Total number of frames rendered
    pub frame_count: u64,
    /// Number of frames where rebuild happened
    pub rebuild_count: u64,
    /// Number of frames where layout happened
    pub layout_count: u64,
    /// Number of frames where paint happened
    pub paint_count: u64,
}

impl FrameStats {
    /// Log statistics to console
    pub fn log(&self) {
        if self.frame_count % 60 == 0 && self.frame_count > 0 {
            tracing::info!(
                "Performance: {} frames | Rebuilds: {} ({:.1}%) | Layouts: {} ({:.1}%) | Paints: {} ({:.1}%)",
                self.frame_count,
                self.rebuild_count,
                (self.rebuild_count as f64 / self.frame_count as f64) * 100.0,
                self.layout_count,
                (self.layout_count as f64 / self.frame_count as f64) * 100.0,
                self.paint_count,
                (self.paint_count as f64 / self.frame_count as f64) * 100.0,
            );
        }
    }
}

/// FluiApp - Main application structure
///
/// Manages the Flui application lifecycle, including:
/// - Element tree management via PipelineOwner
/// - Three-phase rendering: Build → Layout → Paint
/// - Integration with winit/wgpu for window management and GPU rendering
///
/// # Example
///
/// ```rust,ignore
/// use flui_app::run_app;
/// use flui_core::view::View;
///
/// #[derive(Debug)]
/// struct MyApp;
///
/// impl View for MyApp {
///     fn build(self, ctx: &BuildContext) -> impl IntoElement {
///         // Build your UI here
///         todo!()
///     }
/// }
///
/// run_app(Box::new(MyApp))?;
/// ```
pub struct FluiApp {
    /// Pipeline owner that manages the rendering pipeline
    pipeline: PipelineOwner,

    /// Root view (type-erased)
    root_view: Box<dyn AnyView>,

    /// Root element ID
    root_id: Option<ElementId>,

    /// Performance statistics
    stats: FrameStats,

    /// Last known window size for change detection
    last_size: Option<Size>,

    /// Whether the root has been initially built
    root_built: bool,

    /// Event router for gesture and pointer events
    event_router: EventRouter,

    /// wgpu instance
    instance: wgpu::Instance,

    /// wgpu surface
    surface: wgpu::Surface<'static>,

    /// wgpu device
    device: wgpu::Device,

    /// wgpu queue
    queue: wgpu::Queue,

    /// wgpu surface configuration
    config: wgpu::SurfaceConfiguration,

    /// Window reference
    window: Arc<Window>,

    /// GPU painter (persistent, created once)
    painter: flui_engine::painter::WgpuPainter,

    /// Cleanup callback called on app shutdown
    /// Use this to clean up resources, stop background tasks, etc.
    on_cleanup: Option<Box<dyn FnOnce() + Send>>,

    /// Window event callbacks for system events
    /// Handles focus, minimization, DPI changes, etc.
    event_callbacks: crate::event_callbacks::WindowEventCallbacks,
}

impl FluiApp {
    /// Create a new Flui application
    ///
    /// # Arguments
    ///
    /// * `root_view` - The root view of the application (type-erased)
    /// * `window` - The window to render to
    ///
    /// # Returns
    ///
    /// A new `FluiApp` instance with wgpu initialized
    pub fn new(root_view: Box<dyn AnyView>, window: Arc<Window>) -> Self {
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
        .expect("Failed to find adapter");

        // Request device and queue
        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                label: None,
                memory_hints: wgpu::MemoryHints::default(),
                trace: Default::default(),
            },
        ))
        .expect("Failed to create device");

        // Get window size
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

        // Create GPU painter once (not every frame!)
        let painter = flui_engine::painter::WgpuPainter::new(
            device.clone(),
            queue.clone(),
            config.format,
            (config.width, config.height),
        );

        Self {
            pipeline: PipelineOwner::new(),
            root_view,
            root_id: None,
            stats: FrameStats::default(),
            last_size: None,
            root_built: false,
            event_router: EventRouter::new(),
            instance,
            surface,
            device,
            queue,
            config,
            window,
            painter,
            on_cleanup: None,
            event_callbacks: crate::event_callbacks::WindowEventCallbacks::new(),
        }
    }

    /// Set cleanup callback
    ///
    /// This callback will be called when the app is shutting down.
    /// Use it to clean up resources, stop background tasks, close connections, etc.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// app.set_on_cleanup(|| {
    ///     println!("Cleaning up...");
    ///     // Stop background tasks
    ///     // Close database connections
    ///     // Save state to disk
    /// });
    /// ```
    pub fn set_on_cleanup<F>(&mut self, cleanup: F)
    where
        F: FnOnce() + Send + 'static,
    {
        self.on_cleanup = Some(Box::new(cleanup));
    }

    /// Perform cleanup
    ///
    /// This is called automatically on Drop, but can be called manually
    /// for more controlled shutdown.
    ///
    /// # Note
    ///
    /// After calling this, the cleanup callback is consumed and won't
    /// be called again on Drop.
    pub fn cleanup(&mut self) {
        if let Some(cleanup) = self.on_cleanup.take() {
            tracing::info!("Running cleanup callback...");
            cleanup();
            tracing::info!("Cleanup complete");
        }
    }

    /// Get mutable reference to window event callbacks
    ///
    /// Use this to register callbacks for system events like focus changes,
    /// minimization, DPI changes, theme changes, etc.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let mut app = FluiApp::new(root_view, window);
    ///
    /// app.event_callbacks_mut().on_focus(|focused| {
    ///     if focused {
    ///         println!("Window gained focus");
    ///     } else {
    ///         println!("Window lost focus");
    ///     }
    /// });
    ///
    /// app.event_callbacks_mut().on_minimized(|minimized| {
    ///     if minimized {
    ///         println!("Window minimized - pausing background tasks");
    ///     } else {
    ///         println!("Window restored - resuming");
    ///     }
    /// });
    /// ```
    pub fn event_callbacks_mut(&mut self) -> &mut crate::event_callbacks::WindowEventCallbacks {
        &mut self.event_callbacks
    }

    /// Handle a window event
    ///
    /// This dispatches the event to registered callbacks.
    /// You typically don't need to call this manually - it's called
    /// automatically by the event loop.
    pub fn handle_window_event(&mut self, event: &winit::event::WindowEvent) {
        self.event_callbacks.handle_event(event);
    }

    /// Handle window resize
    pub fn resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.config.width = width;
            self.config.height = height;
            self.surface.configure(&self.device, &self.config);

            // Update painter viewport (critical for correct rendering!)
            self.painter.resize(width, height);

            // Mark size changed for relayout
            self.last_size = None;

            // CRITICAL: Request layout immediately on resize to prevent visual glitches
            // Without this, the UI continues rendering with old layout until next update()
            if let Some(root_id) = self.root_id {
                self.pipeline.request_layout(root_id);
                tracing::debug!("Requested layout after resize to {}x{}", width, height);
            }

            tracing::info!("Window resized to {}x{}", width, height);
        }
    }

    /// Update and render a frame
    ///
    /// Returns `true` if another redraw is needed (e.g., for animations or pending work),
    /// `false` if the frame is stable and no redraw is needed.
    pub fn update(&mut self) -> bool {
        let size = Size::new(self.config.width as f32, self.config.height as f32);
        let mut needs_redraw = false;

        // Build phase - create/update element tree
        if !self.root_built {
            self.build_root();
            self.root_built = true;
            needs_redraw = true; // Initial build always needs redraw
        }

        // Check if there are pending rebuilds (from signals, etc.)
        let has_pending_rebuilds = self.pipeline.rebuild_queue().has_pending();
        if has_pending_rebuilds {
            tracing::debug!("Processing pending rebuilds");
            needs_redraw = true;
        }

        // Check if size changed
        let size_changed = self.last_size.map_or(true, |last| last != size);
        if size_changed {
            self.last_size = Some(size);
            needs_redraw = true;
        }

        // Layout phase - always run but pipeline will skip if no dirty elements
        if self.root_id.is_some() {
            let constraints = BoxConstraints::tight(size);
            match self.pipeline.flush_layout(constraints) {
                Ok(Some(_size)) => {
                    self.stats.layout_count += 1;
                    tracing::debug!("Layout complete for size {:?}", size);
                    needs_redraw = true;
                }
                Ok(None) => {
                    // No layout happened (no dirty elements or no root)
                    tracing::debug!("Layout skipped - no changes");
                }
                Err(e) => {
                    tracing::error!("Layout failed: {:?}", e);
                }
            }
        }

        // Paint phase - only if layout ran or paint is dirty
        if let Some(_root_id) = self.root_id {
            match self.pipeline.flush_paint() {
                Ok(Some(root_layer)) => {
                    self.stats.paint_count += 1;

                    // Render to surface
                    self.render(root_layer);
                    needs_redraw = false; // Frame rendered, no immediate redraw needed
                }
                Ok(None) => {
                    tracing::debug!("Paint phase skipped - no dirty elements");
                }
                Err(e) => {
                    tracing::error!("Paint phase failed: {:?}", e);
                }
            }
        }

        self.stats.frame_count += 1;
        self.stats.log();

        needs_redraw
    }

    /// Build the root view
    fn build_root(&mut self) {
        use flui_core::view::with_build_context;
        use flui_core::ElementId;

        // Create a temporary BuildContext for initial build
        // We use ElementId::new(1) as a placeholder - it will be replaced by set_root
        let temp_id = ElementId::new(1);
        let ctx = BuildContext::new(self.pipeline.tree().clone(), temp_id);

        // Build the view within a context guard (sets up thread-local)
        let root_element = with_build_context(&ctx, || {
            self.root_view.build_any()
        });

        // Mount the element and get the real root ID
        let root_id = self.pipeline.set_root(root_element);
        self.root_id = Some(root_id);
        self.stats.rebuild_count += 1;

        // Request layout for the entire tree
        self.pipeline.request_layout(root_id);

        tracing::info!("Root view built with ID: {:?}", root_id);
    }

    /// Render a layer tree to the wgpu surface
    fn render(&mut self, layer: BoxedLayer) {
        // Get current frame
        let frame = match self.surface.get_current_texture() {
            Ok(frame) => frame,
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                // Surface is outdated or lost - reconfigure and retry next frame
                tracing::warn!("Surface outdated/lost, reconfiguring...");
                self.surface.configure(&self.device, &self.config);
                return;
            }
            Err(wgpu::SurfaceError::OutOfMemory) => {
                tracing::error!("Out of GPU memory!");
                return;
            }
            Err(wgpu::SurfaceError::Timeout) => {
                tracing::warn!("Surface timeout, skipping frame");
                return;
            }
            Err(e) => {
                tracing::error!("Unknown surface error: {:?}", e);
                return;
            }
        };

        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        // Create command encoder
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Render Encoder"),
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

        // Use the persistent painter (created once, not every frame!)
        tracing::debug!("Painting layer tree");

        // Collect all paint commands into the painter
        layer.paint(&mut self.painter);

        // Actually render to GPU
        if let Err(e) = self.painter.render(&view, &mut encoder) {
            tracing::error!("Failed to render frame: {:?}", e);
            return;
        }

        // Submit commands
        self.queue.submit(std::iter::once(encoder.finish()));
        frame.present();
    }
}

/// Automatic cleanup on drop
///
/// Ensures cleanup callback is called even if shutdown is not graceful.
impl Drop for FluiApp {
    fn drop(&mut self) {
        // Call cleanup if it hasn't been called yet
        if self.on_cleanup.is_some() {
            tracing::info!("FluiApp dropping, running cleanup...");
            self.cleanup();
        }
    }
}
