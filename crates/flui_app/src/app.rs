//! FluiApp - Main application structure
//!
//! This module provides the FluiApp struct, which manages the application lifecycle,
//! element tree, and rendering pipeline integration with wgpu.

use flui_core::foundation::ElementId;
use flui_core::pipeline::PipelineOwner;
use flui_core::view::{AnyView, BuildContext};
use flui_core::Size;
use flui_engine::{CanvasLayer, GpuRenderer, WindowStateTracker};
use flui_types::{BoxConstraints, Offset};
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

/// Event coalescing buffer for high-frequency events
///
/// Reduces CPU overhead by batching consecutive Move events.
/// Only the last Move event per frame is processed.
///
/// **Note**: Currently unused but reserved for future mouse event optimization.
#[derive(Debug, Default)]
#[allow(dead_code)]
struct EventCoalescer {
    /// Last coalesced mouse move position (if any this frame)
    last_move: Option<Offset>,
    /// Number of events coalesced this session
    coalesced_count: u64,
}

impl FrameStats {
    /// Log statistics to console
    pub fn log(&self) {
        if self.frame_count.is_multiple_of(60) && self.frame_count > 0 {
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

    /// Window state tracker for focus and visibility
    window_state: WindowStateTracker,

    /// Event coalescing buffer (batches high-frequency mouse moves)
    /// Currently unused but reserved for future optimization
    #[allow(dead_code)]
    event_coalescer: EventCoalescer,

    // ===== GPU Rendering (encapsulated in single abstraction) =====
    /// GPU renderer - encapsulates all wgpu resources (device, queue, surface, painter)
    /// This is the only GPU-related field - clean separation of concerns!
    renderer: GpuRenderer,

    /// Cleanup callback called on app shutdown
    /// Use this to clean up resources, stop background tasks, etc.
    on_cleanup: Option<Box<dyn FnOnce() + Send>>,

    /// Window event callbacks for system events
    /// Handles focus, minimization, DPI changes, etc.
    event_callbacks: crate::event_callbacks::WindowEventCallbacks,
}

impl FluiApp {
    /// Create FluiApp from pre-initialized components
    ///
    /// This is used internally by platform-specific initialization code (e.g., WASM).
    /// Most users should use `new()` instead.
    #[doc(hidden)]
    #[deprecated(note = "Use FluiApp::new() instead. WASM support is now integrated directly.")]
    pub fn from_components(root_view: Box<dyn AnyView>, window: Arc<Window>) -> Self {
        // Simplified - just delegate to new()
        Self::new(root_view, window)
    }

    /// Create a new Flui application (async version for WASM)
    ///
    /// # Arguments
    ///
    /// * `root_view` - The root view of the application (type-erased)
    /// * `window` - The window to render to
    ///
    /// # Returns
    ///
    /// A new `FluiApp` instance with GPU renderer initialized asynchronously
    ///
    /// # Note
    ///
    /// Use this method on WebAssembly where `pollster::block_on` doesn't work.
    /// For native platforms, use `new()` instead.
    pub async fn new_async(root_view: Box<dyn AnyView>, window: Arc<Window>) -> Self {
        // Create GPU renderer - encapsulates ALL wgpu initialization (async)!
        let renderer = GpuRenderer::new_async(window).await;

        Self {
            pipeline: PipelineOwner::new(),
            root_view,
            root_id: None,
            stats: FrameStats::default(),
            last_size: None,
            root_built: false,
            window_state: WindowStateTracker::new(),
            event_coalescer: EventCoalescer::default(),
            renderer,
            on_cleanup: None,
            event_callbacks: crate::event_callbacks::WindowEventCallbacks::new(),
        }
    }

    /// Create a new Flui application
    ///
    /// # Arguments
    ///
    /// * `root_view` - The root view of the application (type-erased)
    /// * `window` - The window to render to
    ///
    /// # Returns
    ///
    /// A new `FluiApp` instance with GPU renderer initialized
    pub fn new(root_view: Box<dyn AnyView>, window: Arc<Window>) -> Self {
        // Create GPU renderer - encapsulates ALL wgpu initialization!
        let renderer = GpuRenderer::new(window);

        Self {
            pipeline: PipelineOwner::new(),
            root_view,
            root_id: None,
            stats: FrameStats::default(),
            last_size: None,
            root_built: false,
            window_state: WindowStateTracker::new(),
            event_coalescer: EventCoalescer::default(),
            renderer,
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
    /// # Note
    ///
    /// Focus and visibility events are automatically integrated with WindowStateTracker
    /// to ensure proper event handling state management. Your custom callbacks
    /// will be called in addition to the internal WindowStateTracker updates.
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

    /// Get mutable reference to window state tracker
    ///
    /// The window state tracker is automatically synchronized with window events
    /// (focus, visibility), but you can access it directly if needed.
    pub fn window_state_mut(&mut self) -> &mut WindowStateTracker {
        &mut self.window_state
    }

    /// Handle a window event
    ///
    /// This dispatches the event to registered callbacks AND synchronizes
    /// the WindowStateTracker state with window events.
    ///
    /// You typically don't need to call this manually - it's called
    /// automatically by the event loop.
    ///
    /// # Integration with WindowStateTracker
    ///
    /// The WindowStateTracker is automatically updated based on window events:
    /// - Focus changes → reset pointer state when focus lost
    /// - Minimization → skip event processing when minimized
    /// - This prevents stuck button states and improves efficiency
    pub fn handle_window_event(&mut self, event: &winit::event::WindowEvent) {
        // IMPORTANT: Update WindowStateTracker BEFORE user callbacks
        // This ensures WindowStateTracker state is correct before any user code runs
        match event {
            winit::event::WindowEvent::Focused(focused) => {
                self.window_state.on_focus_changed(*focused);
            }
            winit::event::WindowEvent::Occluded(occluded) => {
                // Occluded = true means window is NOT visible (minimized/covered)
                // So we need to invert it for is_visible
                self.window_state.on_visibility_changed(!occluded);
            }
            _ => {
                // Other events don't affect WindowStateTracker state
            }
        }

        // Then dispatch to user callbacks
        self.event_callbacks.handle_event(event);
    }

    /// Handle window resize
    pub fn resize(&mut self, width: u32, height: u32) {
        // Delegate to GPU renderer - it handles all GPU-related resize logic
        self.renderer.resize(width, height);

        // Mark size changed for relayout
        self.last_size = None;

        // CRITICAL: Request layout immediately on resize to prevent visual glitches
        // Without this, the UI continues rendering with old layout until next update()
        if let Some(root_id) = self.root_id {
            self.pipeline.request_layout(root_id);
            tracing::debug!("Requested layout after resize to {}x{}", width, height);
        }
    }

    /// Update and render a frame
    ///
    /// Returns `true` if another redraw is needed (e.g., for animations or pending work),
    /// `false` if the frame is stable and no redraw is needed.
    pub fn update(&mut self) -> bool {
        // Create frame span - tracing-forest will automatically track timing
        let frame_span = tracing::info_span!(
            "frame",
            num = self.stats.frame_count,
        );
        let _frame_guard = frame_span.enter();

        let (width, height) = self.renderer.size();
        let size = Size::new(width as f32, height as f32);

        // Build phase - create/update element tree
        if !self.root_built {
            let _build_span = tracing::debug_span!("build_root").entered();
            self.build_root();
            self.root_built = true;
            tracing::debug!("Root built");
        }

        // Check if there are pending rebuilds (from signals, etc.)
        let has_pending_rebuilds = self.pipeline.rebuild_queue().has_pending();
        if has_pending_rebuilds {
            let dirty_count = self.pipeline.dirty_count();
            let _build_span = tracing::debug_span!("rebuild", dirty = dirty_count).entered();
            tracing::debug!("Processing");
        }

        // Check if size changed
        let size_changed = self.last_size != Some(size);
        if size_changed {
            self.last_size = Some(size);
            tracing::debug!(w = size.width, h = size.height, "Window resized");
        }

        // Layout phase - always run but pipeline will skip if no dirty elements
        if self.root_id.is_some() {
            let constraints = BoxConstraints::tight(size);
            match self.pipeline.flush_layout(constraints) {
                Ok(Some(layout_size)) => {
                    self.stats.layout_count += 1;
                    tracing::debug!(w = layout_size.width, h = layout_size.height, "Layout complete");
                }
                Ok(None) => {
                    tracing::trace!("Layout skipped");
                }
                Err(e) => {
                    tracing::error!(?e, "Layout failed");
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
                    tracing::debug!("Paint complete");
                }
                Ok(None) => {
                    tracing::trace!("Paint skipped");
                }
                Err(e) => {
                    tracing::error!(?e, "Paint failed");
                }
            }
        }

        self.stats.frame_count += 1;
        self.stats.log();

        // Only request redraw if there's more work to do
        // Check if there are still pending rebuilds or dirty elements
        let has_more_work = self.pipeline.rebuild_queue().has_pending()
            || self.pipeline.has_dirty_layout()
            || self.pipeline.has_dirty_paint();

        // Return false to prevent continuous redraw loop
        // The window will request redraw when:
        // 1. User resizes (resize event triggers request_redraw)
        // 2. State changes via signals (signals trigger request_redraw)
        // 3. Animations (when implemented)
        has_more_work
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
        let root_element = with_build_context(&ctx, || self.root_view.build_any());

        // Mount the element and get the real root ID
        let root_id = self.pipeline.set_root(root_element);
        self.root_id = Some(root_id);
        self.stats.rebuild_count += 1;

        // Request layout for the entire tree
        self.pipeline.request_layout(root_id);

        tracing::info!("Root view built with ID: {:?}", root_id);
    }

    /// Render a layer tree to the GPU surface
    ///
    /// Delegates rendering to GpuRenderer which handles all GPU details.
    fn render(&mut self, layer: Box<CanvasLayer>) {
        // Clean delegation - ALL GPU logic is in GpuRenderer!
        match self.renderer.render(&layer) {
            Ok(()) => {
                tracing::debug!("Frame rendered successfully");
            }
            Err(
                flui_engine::RenderError::SurfaceLost | flui_engine::RenderError::SurfaceOutdated,
            ) => {
                // Surface was lost/outdated - GpuRenderer already reconfigured it
                // Will retry next frame automatically
                tracing::debug!("Surface lost/outdated, will retry next frame");
            }
            Err(e) => {
                tracing::error!("Render error: {:?}", e);
            }
        }
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
