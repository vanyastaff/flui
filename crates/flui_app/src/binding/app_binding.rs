//! AppBinding - Combined application binding
//!
//! This is the main binding that combines all framework bindings:
//! - GestureBinding (events)
//! - SchedulerBinding (frame callbacks)
//! - RendererBinding (rendering pipeline)
//!
//! It provides a global singleton accessed via `ensure_initialized()`.

use super::{BindingBase, GestureBinding, RendererBinding, SchedulerBinding};
use flui_core::{pipeline::PipelineOwner, view::View};
use flui_engine::Scene;
use flui_types::constraints::BoxConstraints;
use parking_lot::RwLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock, Weak};

/// Combined application binding
///
/// # Architecture
///
/// ```text
/// AppBinding (singleton)
///   ├─ pipeline_owner: Arc<RwLock<PipelineOwner>> - Single source of truth
///   ├─ needs_redraw: Arc<AtomicBool> - On-demand rendering flag
///   ├─ GestureBinding (EventRouter)
///   ├─ SchedulerBinding (wraps flui-scheduler)
///   └─ RendererBinding (rendering)
/// ```
///
/// # Usage
///
/// ```rust,ignore
/// let binding = AppBinding::ensure_initialized();
/// binding.attach_root_widget(MyApp::new());
/// ```
///
/// # Thread-Safety
///
/// The binding is thread-safe and can be accessed from any thread.
/// It uses OnceLock for lazy initialization with thread-safe guarantees.
pub struct AppBinding {
    /// Core pipeline - single source of truth for element tree and rendering
    pipeline_owner: Arc<RwLock<PipelineOwner>>,

    /// On-demand rendering flag - set when redraw is needed
    needs_redraw: Arc<AtomicBool>,

    /// Gesture binding (event routing)
    pub gesture: GestureBinding,

    /// Scheduler binding (wraps flui-scheduler for frame scheduling, task prioritization, vsync)
    pub scheduler: SchedulerBinding,

    /// Renderer binding (rendering)
    pub renderer: RendererBinding,
}

impl AppBinding {
    /// Ensure binding is initialized (idempotent)
    ///
    /// Returns the global singleton binding, initializing it if necessary.
    /// Subsequent calls return the same instance.
    ///
    /// # Thread-Safety
    ///
    /// This method is thread-safe. If multiple threads call it concurrently,
    /// only one thread will initialize the binding.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // First call initializes
    /// let binding1 = AppBinding::ensure_initialized();
    ///
    /// // Second call returns same instance
    /// let binding2 = AppBinding::ensure_initialized();
    ///
    /// assert!(Arc::ptr_eq(&binding1, &binding2));
    /// ```
    pub fn ensure_initialized() -> Arc<Self> {
        Self::instance_internal()
            .get_or_init(|| {
                let _span = tracing::info_span!("init_bindings").entered();

                // Create shared pipeline_owner - single source of truth
                let pipeline_owner = Arc::new(RwLock::new(PipelineOwner::new()));
                let needs_redraw = Arc::new(AtomicBool::new(false));

                let mut binding = Self {
                    pipeline_owner: pipeline_owner.clone(),
                    needs_redraw: needs_redraw.clone(),
                    gesture: GestureBinding::new(),
                    scheduler: SchedulerBinding::new(),
                    renderer: RendererBinding::new(),
                };

                // Initialize all bindings
                binding.gesture.init();
                binding.scheduler.init();
                binding.renderer.init();

                // Wire up frame callbacks with Weak reference to avoid circular refs
                binding.wire_up(Arc::downgrade(&pipeline_owner), needs_redraw);

                tracing::info!("Bindings initialized");
                Arc::new(binding)
            })
            .clone()
    }

    /// Wire up bindings
    ///
    /// Connects the scheduler to pipeline for automatic rebuilds.
    /// Uses Weak reference to avoid circular references.
    fn wire_up(&self, pipeline_weak: Weak<RwLock<PipelineOwner>>, needs_redraw: Arc<AtomicBool>) {
        tracing::debug!("Wiring up scheduler callbacks to pipeline");

        // Add persistent frame callback to flush rebuild queue
        // Uses Weak to avoid circular reference (AppBinding -> Scheduler -> callback -> PipelineOwner -> AppBinding)
        self.scheduler
            .scheduler()
            .add_persistent_frame_callback(Arc::new(move |_timing| {
                if let Some(pipeline) = pipeline_weak.upgrade() {
                    let mut owner = pipeline.write();
                    // Only mark for redraw if there were actual changes
                    if owner.flush_rebuild_queue() {
                        needs_redraw.store(true, Ordering::Relaxed);
                    }
                } else {
                    tracing::warn!("Pipeline dropped during frame callback");
                }
            }));

        tracing::debug!("Scheduler wired up with Weak reference");
    }

    /// Internal helper to access the singleton instance
    fn instance_internal() -> &'static OnceLock<Arc<Self>> {
        static INSTANCE: OnceLock<Arc<AppBinding>> = OnceLock::new();
        &INSTANCE
    }

    /// Get instance if already initialized
    ///
    /// Returns None if `ensure_initialized()` has not been called yet.
    pub fn instance() -> Option<Arc<Self>> {
        Self::instance_internal().get().cloned()
    }

    // ========================================================================
    // Pipeline methods (moved from PipelineBinding)
    // ========================================================================

    /// Attach root widget to the pipeline
    ///
    /// Converts the View to an Element and sets it as the pipeline root.
    /// PipelineOwner automatically schedules it for initial build.
    ///
    /// # Parameters
    ///
    /// - `widget`: The root widget (typically MaterialApp or similar)
    ///
    /// # Panics
    ///
    /// Panics if a root widget is already attached.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let binding = AppBinding::ensure_initialized();
    /// binding.attach_root_widget(MyApp);
    /// ```
    pub fn attach_root_widget<V>(&self, widget: V)
    where
        V: flui_core::view::StatelessView + Sync,
    {
        let mut pipeline = self.pipeline_owner.write();
        pipeline
            .attach(widget)
            .expect("Failed to attach root widget");
        self.request_redraw();
    }

    /// Get shared reference to the pipeline owner
    ///
    /// This is the main access point for direct pipeline operations.
    #[must_use]
    pub fn pipeline(&self) -> Arc<RwLock<PipelineOwner>> {
        self.pipeline_owner.clone()
    }

    // ========================================================================
    // On-demand rendering methods
    // ========================================================================

    /// Request a redraw
    ///
    /// Called by signal updates, animations, window events, etc.
    /// The event loop should check `needs_redraw()` before requesting a frame.
    pub fn request_redraw(&self) {
        self.needs_redraw.store(true, Ordering::Relaxed);
    }

    /// Check if redraw is needed
    ///
    /// Returns true if any changes require rendering a new frame.
    pub fn needs_redraw(&self) -> bool {
        self.needs_redraw.load(Ordering::Relaxed)
    }

    /// Mark frame as rendered
    ///
    /// Called after rendering a frame to clear the dirty flag.
    pub fn mark_rendered(&self) {
        self.needs_redraw.store(false, Ordering::Relaxed);
    }

    /// Draw a frame
    ///
    /// Executes the complete rendering pipeline (build → layout → paint).
    /// Returns a Scene ready for GPU rendering.
    ///
    /// # Parameters
    ///
    /// - `constraints`: Root layout constraints (typically window size)
    pub fn draw_frame(&self, constraints: BoxConstraints) -> Scene {
        self.renderer.draw_frame(&self.pipeline_owner, constraints)
    }

    /// Get frame budget statistics
    ///
    /// Access to frame timing, budget tracking, and performance statistics.
    pub fn frame_budget(&self) -> Arc<parking_lot::Mutex<flui_scheduler::FrameBudget>> {
        self.scheduler.scheduler().budget()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ensure_initialized() {
        let binding = AppBinding::ensure_initialized();

        // Should have all components initialized
        assert_eq!(binding.scheduler.scheduler().target_fps(), 60);
        assert!(!binding.scheduler.scheduler().is_frame_scheduled());
    }

    #[test]
    fn test_singleton() {
        let binding1 = AppBinding::ensure_initialized();
        let binding2 = AppBinding::ensure_initialized();

        // Should be same instance (pointer equality)
        assert!(Arc::ptr_eq(&binding1, &binding2));
    }

    #[test]
    fn test_instance_before_init() {
        let _ = AppBinding::ensure_initialized();
        assert!(AppBinding::instance().is_some());
    }

    #[test]
    fn test_needs_redraw() {
        let binding = AppBinding::ensure_initialized();

        // Initially should need redraw (from attach or signal)
        binding.mark_rendered();
        assert!(!binding.needs_redraw());

        binding.request_redraw();
        assert!(binding.needs_redraw());

        binding.mark_rendered();
        assert!(!binding.needs_redraw());
    }
}
