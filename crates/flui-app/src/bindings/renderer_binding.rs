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

use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
};

use flui_foundation::{BindingBase, HasInstance, impl_binding_singleton};
use flui_interaction::MouseTracker;
use flui_painting::PaintingBinding;
use flui_rendering::{
    binding::RendererBinding,
    hit_testing::HitTestResult,
    pipeline::PipelineOwner,
    view::{RenderView, ViewConfiguration},
};
use flui_scheduler::Scheduler;
use flui_semantics::{Assertiveness, SemanticsAction, SemanticsBinding};
use flui_types::Offset;
use parking_lot::RwLock;

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
    /// Root of the PipelineOwner tree (shared with AppBinding when used
    /// together).
    root_pipeline_owner: Arc<RwLock<PipelineOwner>>,

    /// Render views managed by this binding (viewId → RenderView).
    render_views: RwLock<HashMap<u64, Arc<RwLock<RenderView>>>>,

    /// Mouse tracker for hover notification.
    ///
    /// Cycle 4 U-6: switched from the deleted rendering-side
    /// `flui_rendering::input::MouseTracker` to the canonical
    /// `flui_interaction::MouseTracker`. The latter is `Clone` with
    /// `Arc<Mutex<inner>>` interior mutability, so the previous
    /// `RwLock<...>` outer wrap is dropped -- it was double-wrapping
    /// the same mutability concern.
    mouse_tracker: MouseTracker,

    /// Whether semantics are enabled.
    semantics_enabled: AtomicBool,

    /// Listeners for semantics enabled changes.
    #[allow(clippy::type_complexity)]
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

impl Default for RenderingFlutterBinding {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderingFlutterBinding {
    /// Creates a new rendering binding with its own PipelineOwner.
    ///
    /// Used by the singleton pattern. For sharing a PipelineOwner with
    /// AppBinding, use [`new_with_pipeline`](Self::new_with_pipeline) instead.
    pub fn new() -> Self {
        Self::new_with_pipeline(Arc::new(RwLock::new(PipelineOwner::new())))
    }

    /// Creates a new rendering binding with a shared PipelineOwner.
    ///
    /// This allows AppBinding to pass in the same `Arc<RwLock<PipelineOwner>>`
    /// that elements use, ensuring a single PipelineOwner instance at runtime.
    pub fn new_with_pipeline(pipeline_owner: Arc<RwLock<PipelineOwner>>) -> Self {
        // Cycle 4 U-6: the pre-cycle dummy `MouseTrackerHitTest`
        // callback constructed here is gone -- the canonical
        // `flui_interaction::MouseTracker` is parameterless. The
        // hit-test function is passed at the `update_*` call site
        // by the gesture binding, not stored on the tracker.
        let mut binding = Self {
            root_pipeline_owner: pipeline_owner,
            render_views: RwLock::new(HashMap::new()),
            mouse_tracker: MouseTracker::new(),
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
    /// there is a corresponding call to
    /// [`allow_first_frame`](Self::allow_first_frame).
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
    /// This is useful for tests that want to call [`Self::defer_first_frame`] and
    /// [`Self::allow_first_frame`] since those methods only have an effect if no
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
        // The gesture binding is owned by `AppBinding` (a plain field), which is
        // the single authoritative instance driving input and hit testing. We
        // deliberately do not touch `GestureBinding::instance()` here — a second
        // lazily-initialized global would be a distinct allocation with its own
        // arena that never receives the real pointer registrations.

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
// RendererBinding Implementation
// ============================================================================

impl RendererBinding for RenderingFlutterBinding {
    // ---- formerly PipelineManifold ----

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

    // ---- formerly ViewHitTestable ----

    fn hit_test_in_view(&self, result: &mut HitTestResult, position: Offset, _view_id: u64) {
        // Hits route through the REAL render tree (the pipeline
        // owner's), not the per-view registry — the registry views
        // are bare metric holders with no children, so testing them
        // produced no targets. Leaf-first entries come back from the
        // owner's hit-test walk (RenderState.offset-aware).
        //
        // The position arrives in LOGICAL pixels already: every
        // platform converter normalizes at event build (winit and
        // Win32 divide raw physical coordinates by the scale factor;
        // macOS NSPoint is logical natively). The render tree lives in
        // logical pixels too, so the position passes through unscaled
        // — dividing by the DPR here would shrink every hit a second
        // time on scaled displays.
        let owner = self.root_pipeline_owner.read();
        owner.hit_test(position, result);
    }

    // ---- RendererBinding proper ----

    fn root_pipeline_owner(&self) -> &RwLock<PipelineOwner> {
        &self.root_pipeline_owner
    }

    // R-6 reshape (cycle 4 Wave 2 U-1): the pre-cycle `render_views()`
    // returned `&RwLock<HashMap<u64, Arc<RwLock<RenderView>>>>` and
    // leaked the implementer's lock topology through the trait surface.
    // Post-cycle the trait exposes four primitives; the
    // `self.render_views: RwLock<HashMap<u64, Arc<RwLock<RenderView>>>>`
    // private field stays as private storage (HashMap is still the
    // canonical container; we just don't expose the lock graph anymore).
    fn render_view(&self, view_id: u64) -> Option<Arc<RwLock<RenderView>>> {
        self.render_views.read().get(&view_id).cloned()
    }

    fn render_view_ids(&self) -> Vec<u64> {
        self.render_views.read().keys().copied().collect()
    }

    fn insert_render_view(&self, view_id: u64, view: Arc<RwLock<RenderView>>) {
        self.render_views.write().insert(view_id, view);
    }

    fn remove_render_view_by_id(&self, view_id: u64) -> Option<Arc<RwLock<RenderView>>> {
        self.render_views.write().remove(&view_id)
    }

    fn mouse_tracker(&self) -> &MouseTracker {
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

    fn draw_frame(&self) -> Option<flui_layer::LayerTree> {
        // Single authoritative frame path: run_frame produces the
        // layer tree; the caller (platform binding / embedder) wraps
        // it in a Scene and submits it to the renderer. The previous
        // shape dropped the tree here while a divergent
        // `composite_frame()` loop computed per-view metadata that
        // never reached a compositor — both dead ends are gone.
        let root_owner = self.root_pipeline_owner();
        let layer_tree = {
            let mut guard = root_owner.write();
            let owner = std::mem::take(&mut *guard);
            let (owner, result) = owner.run_frame();
            *guard = owner;
            match result {
                Ok(layer_tree) => layer_tree,
                Err(e) => {
                    tracing::error!(error = ?e, "draw_frame: pipeline failed, dropping frame");
                    None
                }
            }
        };

        if self.send_frames_to_engine() {
            // First non-deferred frame marks the gate open.
            self.first_frame_sent.store(true, Ordering::Relaxed);
            layer_tree
        } else {
            // Deferred-first-frame: pipeline work ran (warm-up), the
            // output is withheld until the deferral count drains.
            None
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

    /// `RenderingFlutterBinding::instance()` is a process-wide singleton, so the
    /// tests that toggle its `semantics_enabled` flag share one `AtomicBool`.
    /// They serialize through this lock (held for each test's duration) so they
    /// cannot interleave their writes and observe each other's state — the
    /// listener test in particular asserts exact callback counts that a
    /// concurrent toggle would corrupt.
    static SEMANTICS_TEST_LOCK: parking_lot::Mutex<()> = parking_lot::Mutex::new(());

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
        let _guard = SEMANTICS_TEST_LOCK.lock();
        let binding = RenderingFlutterBinding::instance();

        // Establish a known starting state under the lock rather than assuming
        // the process-wide default (the listener test toggles the same flag).
        binding.set_semantics_enabled(false);
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

    /// The authoritative frame path: `draw_frame` returns the layer
    /// tree the pipeline produced (for the caller to wrap in a Scene
    /// and hand to the renderer), and withholds it while the first
    /// frame is deferred. Uses an isolated (non-singleton) binding so
    /// no other test's pipeline state can interfere.
    #[test]
    fn draw_frame_returns_layer_tree_and_defers_when_gated() {
        use flui_objects::RenderColoredBox;
        use flui_rendering::constraints::BoxConstraints;
        use flui_types::{Size, geometry::px};

        let owner = Arc::new(RwLock::new(PipelineOwner::new()));
        let root_id = {
            let mut o = owner.write();
            let id = o.insert(Box::new(RenderColoredBox::red(40.0, 40.0))
                as Box<
                    dyn flui_rendering::traits::RenderObject<flui_rendering::protocol::BoxProtocol>,
                >);
            o.set_root_id(Some(id));
            o.set_root_constraints(Some(BoxConstraints::tight(Size::new(px(100.0), px(100.0)))));
            id
        };
        let binding = RenderingFlutterBinding::new_with_pipeline(owner);

        // Deferred: the pipeline still runs (warm-up) but the output
        // is withheld.
        binding.defer_first_frame();
        assert!(
            binding.draw_frame().is_none(),
            "deferred first frame must withhold the layer tree",
        );
        binding.allow_first_frame();

        // The deferred pass consumed the dirty work — re-mark so the
        // next frame paints again.
        binding
            .root_pipeline_owner()
            .write()
            .mark_needs_layout(root_id);
        let tree = binding
            .draw_frame()
            .expect("non-deferred frame with dirty work must return the layer tree");
        assert!(
            !tree.is_empty(),
            "the produced layer tree must contain the painted root",
        );
    }

    /// Pointer positions arrive in LOGICAL pixels from every platform
    /// converter (winit/Win32 divide by the scale factor at event
    /// build; NSPoint is logical natively) — `hit_test_in_view` must
    /// not divide by the DPR again. The distinguishing probe: at DPR 2
    /// a logical point OUTSIDE the box must miss; the old double
    /// division mapped it back inside and produced phantom hits.
    #[test]
    fn hit_test_in_view_takes_logical_positions_without_rescaling() {
        use flui_objects::RenderColoredBox;
        use flui_rendering::constraints::BoxConstraints;
        use flui_types::{Offset, geometry::px};

        let owner = Arc::new(RwLock::new(PipelineOwner::new()));
        {
            let mut o = owner.write();
            o.set_device_pixel_ratio(2.0);
            let id = o.insert(Box::new(RenderColoredBox::red(40.0, 40.0))
                as Box<
                    dyn flui_rendering::traits::RenderObject<flui_rendering::protocol::BoxProtocol>,
                >);
            o.set_root_id(Some(id));
            // LOOSE constraints so the box keeps its preferred 40×40
            // and points outside it exist inside the 100×100 window.
            o.set_root_constraints(Some(BoxConstraints::new(
                px(0.0),
                px(100.0),
                px(0.0),
                px(100.0),
            )));
        }
        let binding = RenderingFlutterBinding::new_with_pipeline(owner);
        let _ = binding.draw_frame();

        let mut inside = flui_interaction::routing::HitTestResult::new();
        binding.hit_test_in_view(&mut inside, Offset::new(px(30.0), px(30.0)), 0);
        assert!(
            !inside.is_empty(),
            "logical (30,30) lies inside the 40×40 box and must hit",
        );

        let mut outside = flui_interaction::routing::HitTestResult::new();
        binding.hit_test_in_view(&mut outside, Offset::new(px(60.0), px(60.0)), 0);
        assert!(
            outside.is_empty(),
            "logical (60,60) lies outside the 40×40 box and must miss; \
             a hit means the position was divided by the DPR a second \
             time ((60,60)/2 = (30,30) lands back inside the box)",
        );
    }

    #[test]
    fn test_render_view_management() {
        let binding = RenderingFlutterBinding::instance();

        // Add a render view (R-6 reshape: use the new
        // `add_render_view_with_config` default-impl helper which
        // delegates to `insert_render_view` after deriving the
        // view-configuration).
        let view = Arc::new(RwLock::new(RenderView::new()));
        binding.add_render_view_with_config(1, view.clone());

        assert!(binding.render_view(1).is_some());
        assert!(binding.render_view(2).is_none());

        // Remove
        let removed = binding.remove_render_view_by_id(1);
        assert!(removed.is_some());
        assert!(binding.render_view(1).is_none());
    }

    #[test]
    fn test_semantics_listener() {
        use std::sync::atomic::AtomicUsize;

        let _guard = SEMANTICS_TEST_LOCK.lock();
        let binding = RenderingFlutterBinding::instance();

        // Force a known `false` baseline *before* attaching the listener, so
        // the listener only counts the toggles this test performs and the
        // first `set_semantics_enabled(true)` is an observable state change.
        binding.set_semantics_enabled(false);

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

        // Leave the shared singleton disabled for the next test.
        binding.set_semantics_enabled(false);
    }
}
