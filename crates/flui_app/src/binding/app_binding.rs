//! AppBinding - Combined application binding
//!
//! This is the main binding that combines all framework bindings:
//! - GestureBinding (events)
//! - SchedulerBinding (frame callbacks)
//! - RendererBinding (rendering pipeline)
//! - PipelineBinding (widget tree and pipeline)
//!
//! It provides a global singleton accessed via `ensure_initialized()`.

use super::{BindingBase, GestureBinding, PipelineBinding, RendererBinding, SchedulerBinding};
use flui_core::pipeline::PipelineOwner;
use parking_lot::RwLock;
use std::sync::{Arc, OnceLock};

/// Combined application binding
///
/// # Architecture
///
/// ```text
/// AppBinding (singleton)
///   ├─ GestureBinding (EventRouter)
///   ├─ SchedulerBinding (wraps flui-scheduler)
///   ├─ RendererBinding (rendering)
///   └─ PipelineBinding (widget tree and pipeline)
/// ```
///
/// # Usage
///
/// ```rust,ignore
/// let binding = AppBinding::ensure_initialized();
/// binding.pipeline.attach_root_widget(MyApp::new());
/// ```
///
/// # Thread-Safety
///
/// The binding is thread-safe and can be accessed from any thread.
/// It uses OnceLock for lazy initialization with thread-safe guarantees.
pub struct AppBinding {
    /// Gesture binding (event routing)
    pub gesture: GestureBinding,

    /// Scheduler binding (wraps flui-scheduler for frame scheduling, task prioritization, vsync)
    pub scheduler: SchedulerBinding,

    /// Renderer binding (rendering)
    pub renderer: RendererBinding,

    /// Pipeline binding (widget tree and pipeline)
    pub pipeline: PipelineBinding,
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
                tracing::info!("Initializing AppBinding");

                // Create shared pipeline_owner
                let pipeline_owner = Arc::new(RwLock::new(PipelineOwner::new()));

                let mut binding = Self {
                    gesture: GestureBinding::new(),
                    scheduler: SchedulerBinding::new(), // Wraps flui-scheduler with 60 FPS target
                    renderer: RendererBinding::new(pipeline_owner.clone()),
                    pipeline: PipelineBinding::new(pipeline_owner),
                };

                // Initialize all bindings
                binding.gesture.init();
                binding.scheduler.init();
                binding.renderer.init();
                binding.pipeline.init();

                // Wire up frame callbacks
                binding.wire_up();

                tracing::info!("AppBinding initialized");
                Arc::new(binding)
            })
            .clone()
    }

    /// Wire up bindings
    ///
    /// Connects the scheduler to pipeline for automatic rebuilds.
    /// This is called once during initialization.
    fn wire_up(&self) {
        // Wire scheduler callbacks to pipeline for frame-driven updates
        // This creates the build → layout → paint coordination

        tracing::debug!("Wiring up scheduler callbacks to pipeline");

        // Register persistent frame callback for RebuildQueue processing
        // This integrates signal-driven rebuilds with scheduler's frame lifecycle
        let pipeline_owner = self.pipeline.pipeline_owner();

        self.scheduler.scheduler().add_persistent_frame_callback(Arc::new(move |_timing| {
            // Flush rebuild queue at the start of every frame
            // This processes all pending signal-driven rebuilds
            let mut owner = pipeline_owner.write();
            owner.flush_rebuild_queue();

            tracing::trace!("[SCHEDULER] Flushed rebuild queue at frame start");
        }));

        // Frame flow (integrated):
        //   WgpuEmbedder::render_frame()
        //     → scheduler.begin_frame()
        //       → [PERSISTENT CALLBACKS] flush_rebuild_queue()
        //       → [ONE-TIME CALLBACKS] animations, tickers, etc.
        //     → renderer.draw_frame()
        //       → pipeline.build_frame() [NO flush_rebuild_queue here anymore]
        //         → build/layout/paint pipelines [respect frame budget]
        //     → scheduler.end_frame() [record timing]
        //
        // Benefits:
        // ✓ Signal rebuilds go through scheduler's callback system
        // ✓ RebuildQueue processing happens BEFORE build_frame()
        // ✓ Frame budget enforced for all work
        // ✓ Clean separation: Scheduler manages WHEN, Pipeline manages WHAT

        tracing::debug!("Scheduler integration: RebuildQueue → persistent frame callback (issue #43)");
    }

    /// Internal helper to access the singleton instance
    fn instance_internal() -> &'static OnceLock<Arc<Self>> {
        static INSTANCE: OnceLock<Arc<AppBinding>> = OnceLock::new();
        &INSTANCE
    }

    /// Get instance if already initialized
    ///
    /// Returns None if `ensure_initialized()` has not been called yet.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// if let Some(binding) = AppBinding::instance() {
    ///     // Use binding...
    /// }
    /// ```
    pub fn instance() -> Option<Arc<Self>> {
        Self::instance_internal().get().cloned()
    }

    /// Get frame budget statistics
    ///
    /// Access to frame timing, budget tracking, and performance statistics.
    /// This delegates to the production flui-scheduler FrameBudget.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let binding = AppBinding::ensure_initialized();
    /// let budget = binding.frame_budget();
    /// let stats = budget.lock().phase_stats();
    /// println!("Build: {:.2}ms, Layout: {:.2}ms", stats.build_ms, stats.layout_ms);
    /// ```
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
        assert_eq!(binding.scheduler.scheduler().target_fps(), 60); // Default 60 FPS
        assert!(!binding.scheduler.scheduler().is_frame_scheduled()); // No frame scheduled yet
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
        // This test is tricky because we can't reset the singleton
        // Just verify it returns Some after initialization
        let _ = AppBinding::ensure_initialized();
        assert!(AppBinding::instance().is_some());
    }
}
