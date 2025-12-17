//! AppBinding - Combined application binding.
//!
//! This is the central coordinator that combines:
//! - `WidgetsBinding` (build phase, element tree)
//! - `RenderPipelineOwner` (layout/paint phases)
//! - `GestureBinding` (input handling)
//!
//! # Flutter Equivalent
//!
//! This corresponds to Flutter's combined binding classes:
//! - `WidgetsBinding`
//! - `RendererBinding`
//! - `GestureBinding`
//! - `SchedulerBinding`

use flui_interaction::GestureBinding;
use flui_rendering::pipeline::PipelineOwner as RenderPipelineOwner;
use flui_scheduler::Scheduler;
use flui_view::{View, WidgetsBinding};
use parking_lot::RwLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

/// Combined application binding.
///
/// AppBinding is the central coordinator for the FLUI framework.
/// It manages:
/// - Widget/Element tree via `WidgetsBinding`
/// - Render tree via `RenderPipelineOwner`
/// - Input handling via `GestureBinding`
/// - Frame scheduling via `Scheduler`
///
/// # Thread Safety
///
/// AppBinding is a singleton accessed via `instance()`. It uses internal
/// locking for thread-safe access to mutable state.
///
/// # Example
///
/// ```rust,ignore
/// let binding = AppBinding::instance();
/// binding.attach_root_widget(&MyApp);
/// binding.draw_frame();
/// ```
pub struct AppBinding {
    /// Widgets binding (build phase, element tree)
    widgets: RwLock<WidgetsBinding>,

    /// Render pipeline owner (layout/paint phases)
    render_pipeline: RwLock<RenderPipelineOwner>,

    /// Gesture binding (input handling)
    gestures: GestureBinding,

    /// Frame scheduler
    scheduler: Arc<Scheduler>,

    /// Whether a redraw is needed
    needs_redraw: AtomicBool,

    /// Whether the app is initialized
    initialized: AtomicBool,
}

impl AppBinding {
    /// Create a new AppBinding.
    fn new() -> Self {
        let scheduler = Arc::new(Scheduler::new());

        let mut widgets = WidgetsBinding::new();

        // Wire up frame scheduling
        let needs_redraw = Arc::new(AtomicBool::new(false));
        let needs_redraw_clone = needs_redraw.clone();
        widgets.set_on_need_frame(move || {
            needs_redraw_clone.store(true, Ordering::Relaxed);
        });

        Self {
            widgets: RwLock::new(widgets),
            render_pipeline: RwLock::new(RenderPipelineOwner::new()),
            gestures: GestureBinding::new(),
            scheduler,
            needs_redraw: AtomicBool::new(false),
            initialized: AtomicBool::new(false),
        }
    }

    /// Get the singleton instance.
    ///
    /// Creates the instance on first call.
    pub fn instance() -> &'static Self {
        static INSTANCE: OnceLock<AppBinding> = OnceLock::new();
        INSTANCE.get_or_init(|| {
            tracing::info!("Initializing AppBinding");
            AppBinding::new()
        })
    }

    /// Check if the binding is initialized.
    pub fn is_initialized(&self) -> bool {
        self.initialized.load(Ordering::Relaxed)
    }

    // ========================================================================
    // Widget/Element Layer
    // ========================================================================

    /// Attach a root widget.
    ///
    /// This creates the root element and schedules the first build.
    ///
    /// # Panics
    ///
    /// Panics if a root widget is already attached.
    pub fn attach_root_widget<V: View>(&self, view: &V) {
        let mut widgets = self.widgets.write();
        widgets.attach_root_widget(view);
        self.initialized.store(true, Ordering::Relaxed);
        self.request_redraw();
        tracing::debug!("Root widget attached");
    }

    /// Get read access to WidgetsBinding.
    pub fn widgets(&self) -> parking_lot::RwLockReadGuard<'_, WidgetsBinding> {
        self.widgets.read()
    }

    /// Get write access to WidgetsBinding.
    pub fn widgets_mut(&self) -> parking_lot::RwLockWriteGuard<'_, WidgetsBinding> {
        self.widgets.write()
    }

    // ========================================================================
    // Render Layer
    // ========================================================================

    /// Get read access to RenderPipelineOwner.
    pub fn render_pipeline(&self) -> parking_lot::RwLockReadGuard<'_, RenderPipelineOwner> {
        self.render_pipeline.read()
    }

    /// Get write access to RenderPipelineOwner.
    pub fn render_pipeline_mut(&self) -> parking_lot::RwLockWriteGuard<'_, RenderPipelineOwner> {
        self.render_pipeline.write()
    }

    // ========================================================================
    // Gesture Layer
    // ========================================================================

    /// Get the gesture binding.
    pub fn gestures(&self) -> &GestureBinding {
        &self.gestures
    }

    // ========================================================================
    // Scheduler
    // ========================================================================

    /// Get the scheduler.
    pub fn scheduler(&self) -> &Arc<Scheduler> {
        &self.scheduler
    }

    // ========================================================================
    // Frame Management
    // ========================================================================

    /// Request a redraw.
    pub fn request_redraw(&self) {
        self.needs_redraw.store(true, Ordering::Relaxed);
    }

    /// Check if a redraw is needed.
    pub fn needs_redraw(&self) -> bool {
        self.needs_redraw.load(Ordering::Relaxed)
    }

    /// Mark the frame as rendered.
    pub fn mark_rendered(&self) {
        self.needs_redraw.store(false, Ordering::Relaxed);
    }

    /// Draw a frame.
    ///
    /// This executes the complete rendering pipeline:
    /// 1. Build phase (WidgetsBinding)
    /// 2. Layout phase (RenderPipelineOwner)
    /// 3. Paint phase (RenderPipelineOwner)
    ///
    /// Returns true if any work was done.
    pub fn draw_frame(&self) -> bool {
        let mut did_work = false;

        // Phase 1: Build
        {
            let mut widgets = self.widgets.write();
            if widgets.has_pending_builds() {
                widgets.draw_frame();
                did_work = true;
            }
        }

        // Phase 2 & 3: Layout and Paint
        {
            let mut render = self.render_pipeline.write();
            if render.has_dirty_nodes() {
                render.flush_layout();
                render.flush_compositing_bits();
                render.flush_paint();
                did_work = true;
            }
        }

        if did_work {
            self.mark_rendered();
        }

        did_work
    }

    /// Check if there is pending work.
    pub fn has_pending_work(&self) -> bool {
        self.widgets.read().has_pending_builds() || self.render_pipeline.read().has_dirty_nodes()
    }
}

impl std::fmt::Debug for AppBinding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppBinding")
            .field("initialized", &self.initialized.load(Ordering::Relaxed))
            .field("needs_redraw", &self.needs_redraw.load(Ordering::Relaxed))
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_singleton() {
        let binding1 = AppBinding::instance();
        let binding2 = AppBinding::instance();
        assert!(std::ptr::eq(binding1, binding2));
    }

    #[test]
    fn test_needs_redraw() {
        let binding = AppBinding::instance();

        binding.mark_rendered();
        assert!(!binding.needs_redraw());

        binding.request_redraw();
        assert!(binding.needs_redraw());

        binding.mark_rendered();
        assert!(!binding.needs_redraw());
    }
}
