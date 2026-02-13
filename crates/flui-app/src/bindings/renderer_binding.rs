//! RenderingFlutterBinding - Concrete implementation of RendererBinding.
//!
//! This is the glue between the render trees and the FLUI engine.
//! It manages multiple independent render trees, each rooted in a RenderView.
//!
//! # Flutter Equivalence
//!
//! Corresponds to Flutter's `RenderingFlutterBinding` class from
//! `rendering/binding.dart`:
//!
//! ```dart
//! class RenderingFlutterBinding extends BindingBase
//!     with GestureBinding, SchedulerBinding, ServicesBinding,
//!          SemanticsBinding, PaintingBinding, RendererBinding { }
//! ```
//!
//! # Architecture
//!
//! ```text
//! RenderingFlutterBinding
//!   ├── root_pipeline_owner   - Root of PipelineOwner tree
//!   ├── render_views          - Map<ViewId, RenderView>
//!   ├── mouse_tracker         - Hover event management
//!   └── semantics integration - Via SemanticsBinding
//! ```
//!
//! # Usage
//!
//! For most applications, use `WidgetsFlutterBinding` instead, which
//! includes this binding plus widgets support. Use `RenderingFlutterBinding`
//! directly only when working with the rendering layer without widgets.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;

use parking_lot::RwLock;

use flui_foundation::{impl_binding_singleton, BindingBase, HasInstance};
use flui_interaction::binding::GestureBinding;
use flui_painting::PaintingBinding;
use flui_rendering::binding::{HitTestable, PipelineManifold, RendererBinding};
use flui_rendering::hit_testing::HitTestResult;
use flui_rendering::input::MouseTracker;
use flui_rendering::pipeline::PipelineOwner;
use flui_rendering::view::{RenderView, ViewConfiguration};
use flui_scheduler::Scheduler;
use flui_semantics::{Assertiveness, SemanticsAction, SemanticsBinding};
use flui_types::Offset;

// ============================================================================
// RenderingFlutterBinding
// ============================================================================

/// Concrete binding for applications using the Rendering framework directly.
///
/// This is the glue that binds the framework to the FLUI engine.
/// For widget-based applications, use `WidgetsFlutterBinding` instead.
///
/// # Responsibilities
///
/// - Managing the root [`PipelineOwner`] tree
/// - Managing [`RenderView`]s (add/remove)
/// - Creating [`ViewConfiguration`]s for views
/// - Coordinating frame production
/// - Managing [`MouseTracker`] for hover events
/// - Integrating with [`SemanticsBinding`] for accessibility
///
/// # Thread Safety
///
/// This binding is thread-safe and can be accessed from multiple threads.
/// Internal state is protected by `RwLock`s.
pub struct RenderingFlutterBinding {
    /// Root of the PipelineOwner tree.
    root_pipeline_owner: RwLock<PipelineOwner>,

    /// Render views managed by this binding (viewId → RenderView).
    render_views: RwLock<HashMap<u64, Arc<RwLock<RenderView>>>>,

    /// Mouse tracker for hover notification.
    mouse_tracker: RwLock<MouseTracker>,

    /// Whether semantics are enabled.
    semantics_enabled: AtomicBool,

    /// Listeners for semantics enabled changes.
    semantics_listeners: RwLock<Vec<Arc<dyn Fn(bool) + Send + Sync>>>,

    /// Counter for deferred first frame.
    first_frame_deferred_count: AtomicU32,

    /// Whether the first frame has been sent.
    first_frame_sent: AtomicBool,
}

impl std::fmt::Debug for RenderingFlutterBinding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RenderingFlutterBinding")
            .field("render_views_count", &self.render_views.read().len())
            .field(
                "semantics_enabled",
                &self.semantics_enabled.load(Ordering::Relaxed),
            )
            .field(
                "first_frame_sent",
                &self.first_frame_sent.load(Ordering::Relaxed),
            )
            .finish()
    }
}

// Safety: All fields are thread-safe
unsafe impl Send for RenderingFlutterBinding {}
unsafe impl Sync for RenderingFlutterBinding {}

impl Default for RenderingFlutterBinding {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderingFlutterBinding {
    /// Creates a new rendering binding.
    pub fn new() -> Self {
        // Create a dummy hit test callback for now
        // In practice, this gets replaced when the binding is fully initialized
        let hit_test_callback: flui_rendering::input::MouseTrackerHitTest =
            Arc::new(|_position, _view_id| HitTestResult::new());

        let mut binding = Self {
            root_pipeline_owner: RwLock::new(PipelineOwner::new()),
            render_views: RwLock::new(HashMap::new()),
            mouse_tracker: RwLock::new(MouseTracker::new(hit_test_callback)),
            semantics_enabled: AtomicBool::new(false),
            semantics_listeners: RwLock::new(Vec::new()),
            first_frame_deferred_count: AtomicU32::new(0),
            first_frame_sent: AtomicBool::new(false),
        };
        binding.init_instances();
        binding
    }

    /// Returns an instance of the binding that implements [`RendererBinding`].
    ///
    /// If no binding has yet been initialized, creates and initializes one.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use flui_app::RenderingFlutterBinding;
    ///
    /// let binding = RenderingFlutterBinding::ensure_initialized();
    /// ```
    pub fn ensure_initialized() -> &'static Self {
        Self::instance()
    }

    // ========================================================================
    // First Frame Deferral
    // ========================================================================

    /// Tell the framework to not send the first frames to the engine until
    /// there is a corresponding call to [`allow_first_frame`](Self::allow_first_frame).
    ///
    /// Call this to perform asynchronous initialization work before the first
    /// frame is rendered (which takes down the splash screen). The framework
    /// will still do all the work to produce frames, but those frames are never
    /// sent to the engine and will not appear on screen.
    ///
    /// Calling this has no effect after the first frame has been sent.
    pub fn defer_first_frame(&self) {
        self.first_frame_deferred_count
            .fetch_add(1, Ordering::Relaxed);
    }

    /// Called after [`defer_first_frame`](Self::defer_first_frame) to tell
    /// the framework that it is ok to send the first frame to the engine now.
    ///
    /// For best performance, this method should only be called while the
    /// scheduler phase is idle.
    ///
    /// This method may only be called once for each corresponding call
    /// to [`defer_first_frame`](Self::defer_first_frame).
    pub fn allow_first_frame(&self) {
        let prev = self
            .first_frame_deferred_count
            .fetch_sub(1, Ordering::Relaxed);
        assert!(
            prev > 0,
            "allow_first_frame called without matching defer_first_frame"
        );

        // Schedule a warm up frame even if count is not zero yet
        if !self.first_frame_sent.load(Ordering::Relaxed) {
            Scheduler::instance().schedule_frame(Box::new(|_timing| {
                // Warm-up frame callback
            }));
        }
    }

    /// Call this to pretend that no frames have been sent to the engine yet.
    ///
    /// This is useful for tests that want to call [`defer_first_frame`] and
    /// [`allow_first_frame`] since those methods only have an effect if no
    /// frames have been sent to the engine yet.
    pub fn reset_first_frame_sent(&self) {
        self.first_frame_sent.store(false, Ordering::Relaxed);
    }

    // ========================================================================
    // Semantics Integration
    // ========================================================================

    /// Sets whether semantics are enabled.
    ///
    /// When enabled, the framework will maintain the semantics tree.
    pub fn set_semantics_enabled(&self, enabled: bool) {
        let was_enabled = self.semantics_enabled.swap(enabled, Ordering::Relaxed);
        if was_enabled != enabled {
            // Notify listeners
            let listeners = self.semantics_listeners.read();
            for listener in listeners.iter() {
                listener(enabled);
            }

            // Update SemanticsBinding if available
            if SemanticsBinding::is_initialized() {
                SemanticsBinding::instance().set_platform_semantics_enabled(enabled);
            }
        }
    }

    /// Announces a message via accessibility services.
    pub fn announce(&self, message: &str, assertiveness: Assertiveness) {
        if SemanticsBinding::is_initialized() {
            SemanticsBinding::instance().announce(message, assertiveness);
        }
    }

    // ========================================================================
    // Binding Accessors
    // ========================================================================

    /// Get the GestureBinding singleton.
    ///
    /// Equivalent to Flutter's `GestureBinding.instance`.
    pub fn gestures() -> &'static GestureBinding {
        GestureBinding::instance()
    }

    /// Get the Scheduler singleton.
    ///
    /// Equivalent to Flutter's `SchedulerBinding.instance`.
    pub fn scheduler() -> &'static Scheduler {
        Scheduler::instance()
    }

    /// Get the SemanticsBinding singleton.
    ///
    /// Equivalent to Flutter's `SemanticsBinding.instance`.
    pub fn semantics() -> &'static SemanticsBinding {
        SemanticsBinding::instance()
    }

    /// Get the PaintingBinding singleton.
    ///
    /// Equivalent to Flutter's `PaintingBinding.instance`.
    pub fn painting() -> &'static PaintingBinding {
        PaintingBinding::instance()
    }

    // ========================================================================
    // Memory Management
    // ========================================================================

    /// Handles memory pressure by clearing caches.
    ///
    /// Delegates to PaintingBinding to clear the image cache.
    pub fn handle_memory_pressure(&self) {
        Self::painting().handle_memory_pressure();
        tracing::info!("RenderingFlutterBinding: handled memory pressure");
    }
}

// ============================================================================
// BindingBase Implementation
// ============================================================================

impl BindingBase for RenderingFlutterBinding {
    fn init_instances(&mut self) {
        // Initialize GestureBinding first (provides hit testing)
        let _ = GestureBinding::instance();
        tracing::debug!("GestureBinding initialized via RenderingFlutterBinding");

        // Initialize scheduler
        let _ = Scheduler::instance();
        tracing::debug!("Scheduler initialized via RenderingFlutterBinding");

        // Initialize semantics binding
        let _ = SemanticsBinding::instance();
        tracing::debug!("SemanticsBinding initialized via RenderingFlutterBinding");

        // Initialize painting binding (image cache, shader warm-up)
        let _ = PaintingBinding::instance();
        tracing::debug!("PaintingBinding initialized via RenderingFlutterBinding");

        tracing::info!("RenderingFlutterBinding initialized");
    }
}

// Singleton pattern
impl_binding_singleton!(RenderingFlutterBinding);

// ============================================================================
// PipelineManifold Implementation
// ============================================================================

impl PipelineManifold for RenderingFlutterBinding {
    fn request_visual_update(&self) {
        Scheduler::instance().schedule_frame(Box::new(|_timing| {
            // Visual update frame callback
        }));
    }

    fn semantics_enabled(&self) -> bool {
        self.semantics_enabled.load(Ordering::Relaxed)
    }

    fn add_semantics_enabled_listener(&self, listener: Arc<dyn Fn(bool) + Send + Sync>) {
        self.semantics_listeners.write().push(listener);
    }

    fn remove_semantics_enabled_listener(&self, listener: &Arc<dyn Fn(bool) + Send + Sync>) {
        let mut listeners = self.semantics_listeners.write();
        listeners.retain(|l| !Arc::ptr_eq(l, listener));
    }
}

// ============================================================================
// HitTestable Implementation
// ============================================================================

impl HitTestable for RenderingFlutterBinding {
    fn hit_test_in_view(&self, result: &mut HitTestResult, position: Offset, view_id: u64) {
        let views = self.render_views.read();
        if let Some(view) = views.get(&view_id) {
            let view_guard = view.read();
            view_guard.hit_test(result, position);
        }
    }
}

// ============================================================================
// RendererBinding Implementation
// ============================================================================

impl RendererBinding for RenderingFlutterBinding {
    fn root_pipeline_owner(&self) -> &RwLock<PipelineOwner> {
        &self.root_pipeline_owner
    }

    fn render_views(&self) -> &RwLock<HashMap<u64, Arc<RwLock<RenderView>>>> {
        &self.render_views
    }

    fn mouse_tracker(&self) -> &RwLock<MouseTracker> {
        &self.mouse_tracker
    }

    fn send_frames_to_engine(&self) -> bool {
        self.first_frame_sent.load(Ordering::Relaxed)
            || self.first_frame_deferred_count.load(Ordering::Relaxed) == 0
    }

    fn create_view_configuration_for(&self, render_view: &RenderView) -> ViewConfiguration {
        if render_view.has_configuration() {
            render_view.configuration().clone()
        } else {
            // Default configuration for testing
            ViewConfiguration::default()
        }
    }

    fn draw_frame(&self) {
        // Call default implementation
        let root_owner = self.root_pipeline_owner();

        // Phase 3: Layout
        root_owner.write().flush_layout();

        // Phase 4: Compositing bits
        root_owner.write().flush_compositing_bits();

        // Phase 5: Paint
        root_owner.write().flush_paint();

        // Phase 6 & 7: Composite and Semantics (only if sending frames)
        if self.send_frames_to_engine() {
            // Composite each render view
            for (_, view) in self.render_views.read().iter() {
                let view_guard = view.read();
                let _result = view_guard.composite_frame();
                // In a real implementation, send to GPU here
            }

            // Phase 7: Semantics
            root_owner.write().flush_semantics();

            // Mark first frame sent
            self.first_frame_sent.store(true, Ordering::Relaxed);
        }
    }

    fn perform_semantics_action(
        &self,
        view_id: u64,
        node_id: i32,
        action: SemanticsAction,
        args: Option<flui_semantics::ActionArgs>,
    ) {
        // Look up the render view and delegate to its semantics owner
        let views = self.render_views.read();
        if let Some(_view) = views.get(&view_id) {
            // TODO: Get semantics owner from pipeline owner and perform action
            tracing::debug!(
                "perform_semantics_action: view={}, node={}, action={:?}, args={:?}",
                view_id,
                node_id,
                action,
                args
            );
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_singleton() {
        let binding1 = RenderingFlutterBinding::instance();
        let binding2 = RenderingFlutterBinding::instance();
        assert!(std::ptr::eq(binding1, binding2));
    }

    #[test]
    fn test_ensure_initialized() {
        let binding = RenderingFlutterBinding::ensure_initialized();
        assert!(RenderingFlutterBinding::is_initialized());
        assert!(std::ptr::eq(binding, RenderingFlutterBinding::instance()));
    }

    #[test]
    fn test_semantics_enabled() {
        let binding = RenderingFlutterBinding::instance();

        // Initially disabled
        assert!(!binding.semantics_enabled());

        // Enable
        binding.set_semantics_enabled(true);
        assert!(binding.semantics_enabled());

        // Disable
        binding.set_semantics_enabled(false);
        assert!(!binding.semantics_enabled());
    }

    #[test]
    fn test_send_frames_to_engine() {
        let binding = RenderingFlutterBinding::instance();
        binding.reset_first_frame_sent();

        // Initially should send (no deferrals)
        assert!(binding.send_frames_to_engine());

        // Defer first frame
        binding.defer_first_frame();
        assert!(!binding.send_frames_to_engine());

        // Allow first frame
        binding.allow_first_frame();
        assert!(binding.send_frames_to_engine());
    }

    #[test]
    fn test_render_view_management() {
        let binding = RenderingFlutterBinding::instance();

        // Add a render view
        let view = Arc::new(RwLock::new(RenderView::new()));
        binding.add_render_view(1, view.clone());

        assert!(binding.get_render_view(1).is_some());
        assert!(binding.get_render_view(2).is_none());

        // Remove
        let removed = binding.remove_render_view(1);
        assert!(removed.is_some());
        assert!(binding.get_render_view(1).is_none());
    }

    #[test]
    fn test_semantics_listener() {
        use std::sync::atomic::AtomicUsize;

        let binding = RenderingFlutterBinding::instance();
        let call_count = Arc::new(AtomicUsize::new(0));
        let call_count_clone = call_count.clone();

        let listener: Arc<dyn Fn(bool) + Send + Sync> = Arc::new(move |_enabled| {
            call_count_clone.fetch_add(1, Ordering::Relaxed);
        });

        binding.add_semantics_enabled_listener(listener.clone());

        // Toggle semantics
        binding.set_semantics_enabled(true);
        assert_eq!(call_count.load(Ordering::Relaxed), 1);

        binding.set_semantics_enabled(false);
        assert_eq!(call_count.load(Ordering::Relaxed), 2);

        // Remove listener
        binding.remove_semantics_enabled_listener(&listener);

        binding.set_semantics_enabled(true);
        // Should not increment (listener removed)
        assert_eq!(call_count.load(Ordering::Relaxed), 2);
    }
}
