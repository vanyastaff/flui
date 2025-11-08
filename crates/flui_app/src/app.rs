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
        }
    }

    /// Handle window resize
    pub fn resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.config.width = width;
            self.config.height = height;
            self.surface.configure(&self.device, &self.config);

            // Mark size changed for relayout
            self.last_size = None;

            tracing::info!("Window resized to {}x{}", width, height);
        }
    }

    /// Update and render a frame
    pub fn update(&mut self) {
        let size = Size::new(self.config.width as f32, self.config.height as f32);

        // Build phase - create/update element tree
        if !self.root_built {
            self.build_root();
            self.root_built = true;
        }

        // Check if size changed
        let size_changed = self.last_size.map_or(true, |last| last != size);
        if size_changed {
            self.last_size = Some(size);
        }

        // Layout phase - compute positions and sizes
        if let Some(root_id) = self.root_id {
            let constraints = BoxConstraints::tight(size);
            match self.pipeline.flush_layout(constraints) {
                Ok(_) => {
                    self.stats.layout_count += 1;
                    tracing::debug!("Layout complete for size {:?}", size);
                }
                Err(e) => {
                    tracing::error!("Layout failed: {:?}", e);
                }
            }
        }

        // Paint phase - generate layer tree
        if let Some(_root_id) = self.root_id {
            match self.pipeline.flush_paint() {
                Ok(Some(root_layer)) => {
                    // Note: BoxedLayer cannot be cloned (trait object), so we render directly
                    self.stats.paint_count += 1;

                    // Render to surface
                    self.render(root_layer);
                }
                Ok(None) => {
                    tracing::warn!("Paint phase returned no layer");
                }
                Err(e) => {
                    tracing::error!("Paint phase failed: {:?}", e);
                }
            }
        }

        self.stats.frame_count += 1;
        self.stats.log();
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
        use crate::wgpu_painter::WgpuPainter;

        // Get current frame
        let frame = match self.surface.get_current_texture() {
            Ok(frame) => frame,
            Err(e) => {
                tracing::warn!("Failed to get current texture: {:?}", e);
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

        // Create GPU painter with device and queue references
        tracing::debug!("Painting layer tree");
        let surface_format = self.config.format;
        let size = (self.config.width, self.config.height);

        // We need to clone device and queue for the painter
        // wgpu types are Arc-based internally, so cloning is cheap
        let mut painter = WgpuPainter::new(
            self.device.clone(),
            self.queue.clone(),
            surface_format,
            size,
        );

        // Collect all paint commands into the painter
        layer.paint(&mut painter);

        // Actually render to GPU
        painter.render(&view, &mut encoder);

        // Submit commands
        self.queue.submit(std::iter::once(encoder.finish()));
        frame.present();
    }
}
