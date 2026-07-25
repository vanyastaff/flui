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
use flui_painting::PaintingBinding;
use flui_rendering::{
    binding::RendererBinding,
    hit_testing::HitTestResult,
    pipeline::PipelineOwner,
    view::{RenderView, ViewConfiguration},
};
use flui_scheduler::Scheduler;
use flui_semantics::{Assertiveness, SemanticsBinding};
use flui_types::Offset;
use parking_lot::RwLock;

/// A subscriber to [`RenderingFlutterBinding::set_semantics_enabled`] changes.
///
/// Named alias for `Arc<dyn Fn(bool) + Send + Sync>` so the field and impl
/// signatures below read as intent rather than nested generics (kills the
/// `clippy::type_complexity` lint that fired on the inline form). Identical
/// to the type spelled out in `RendererBinding::add_semantics_enabled_listener`
/// (flui-rendering) — a type alias is transparent, so this satisfies the
/// trait without repeating the trait's own long-hand spelling here.
type SemanticsEnabledListener = Arc<dyn Fn(bool) + Send + Sync>;

/// Shared body for [`RenderingFlutterBinding::redirty_root_for_represent`]:
/// operates on the bare `Arc<RwLock<PipelineOwner>>` (rather than requiring
/// a full `RenderingFlutterBinding` reference) so a caller that only holds
/// that handle — e.g. a `Send + Sync` closure captured once at
/// `AppBinding::instance()`'s bootstrap (the frames-disabled→enabled
/// re-dirty listener; see `ADR-0035`) — can reuse the identical logic
/// instead of re-deriving it.
pub(crate) fn redirty_pipeline_root(pipeline_owner: &RwLock<PipelineOwner>) {
    let root_owner = pipeline_owner.read();
    if let Some(root_id) = root_owner.root_id()
        && let Some(handle) = root_owner.repaint_handle(root_id)
    {
        drop(root_owner);
        if let Err(e) = handle.mark_needs_layout() {
            tracing::warn!(
                error = ?e,
                "redirty_root_for_represent: failed to re-mark the root dirty; \
                 the withheld/re-enabled content may not present until \
                 something else dirties the tree",
            );
        }
    }
}

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

    /// Whether semantics are enabled.
    semantics_enabled: AtomicBool,

    /// Listeners for semantics enabled changes.
    semantics_listeners: RwLock<Vec<SemanticsEnabledListener>>,

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
            .finish_non_exhaustive()
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
        let mut binding = Self {
            root_pipeline_owner: pipeline_owner,
            render_views: RwLock::new(HashMap::new()),
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
    //
    // This is the ONE canonical implementation of the first-frame deferral
    // gate. It used to be duplicated: `WidgetsBinding` (flui-view) carried
    // its own independent counter that neither matched this one's semantics
    // (no `first_frame_sent` latch, no panic on an unmatched `allow`) nor
    // was reachable from the production frame path; `RendererBinding`
    // (flui-rendering) declared a default `send_frames_to_engine() -> true`
    // plus a default `draw_frame()` gate that no real embedder overrode.
    // Both were deleted — see AGENTS.md's port-methodology note against
    // reintroducing a second copy of this state. Every consumer (the
    // `RendererBinding` trait impl below and `AppBinding::defer_first_frame`
    // / `allow_first_frame` / `send_frames_to_engine` in
    // `crates/flui-app/src/app/binding.rs`, which the production
    // `render_frame_entered` path actually calls) forwards to this struct.
    //
    // # Flutter Equivalence (oracle tag `3.44.0`)
    //
    // `packages/flutter/lib/src/rendering/binding.dart`,
    // `RendererBinding`:
    // ```dart
    // int _firstFrameDeferredCount = 0;
    // bool _firstFrameSent = false;
    // bool get sendFramesToEngine =>
    //     _firstFrameSent || _firstFrameDeferredCount == 0;
    // void deferFirstFrame() {
    //   assert(_firstFrameDeferredCount >= 0);
    //   _firstFrameDeferredCount += 1;
    // }
    // void allowFirstFrame() {
    //   assert(_firstFrameDeferredCount > 0);
    //   _firstFrameDeferredCount -= 1;
    //   if (!_firstFrameSent) {
    //     scheduleWarmUpFrame();
    //   }
    // }
    // ```
    // and `drawFrame`'s gate:
    // ```dart
    // rootPipelineOwner.flushLayout();
    // rootPipelineOwner.flushCompositingBits();
    // rootPipelineOwner.flushPaint();
    // if (sendFramesToEngine) {
    //   for (final RenderView renderView in renderViews) {
    //     renderView.compositeFrame();
    //   }
    //   rootPipelineOwner.flushSemantics();
    //   _firstFrameSent = true;
    // }
    // ```
    // Layout/compositing-bits/paint always run; only the composite-to-engine
    // step (and Flutter's semantics flush alongside it) is gated. FLUI's
    // production mirror of this split lives in `AppBinding::
    // render_frame_entered` (`crates/flui-app/src/app/binding.rs`): the
    // build/layout/paint pipeline always runs in `draw_frame_entered`, and
    // only the GPU `render_scene` (present) call is gated on
    // `send_frames_to_engine`. FLUI's `run_frame` does not yet gate its own
    // semantics phase behind this flag the way the oracle's `flushSemantics`
    // does — a documented, narrower scope than the oracle's for this unit,
    // not a silent gap.
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
    ///
    /// # Panics
    ///
    /// Panics if called without a matching prior
    /// [`defer_first_frame`](Self::defer_first_frame) call (i.e. the deferred
    /// count is already zero) — a caller-contract violation, mirroring the
    /// oracle's `assert(_firstFrameDeferredCount > 0)`.
    pub fn allow_first_frame(&self) {
        let prev = self
            .first_frame_deferred_count
            .fetch_sub(1, Ordering::Relaxed);
        assert!(
            prev > 0,
            "allow_first_frame called without matching defer_first_frame"
        );

        // Schedule a warm-up frame even if the count is not down to zero
        // yet: removing one deferral may uncover a NEW one further down the
        // widget tree (a subtree that only registers its own
        // `defer_first_frame` once an outer one clears), so every
        // `allow_first_frame` call gets a pass, not just the last.
        //
        // The withheld frame(s) already ran build/layout/paint while
        // deferred (the pipeline work happens unconditionally — see the
        // module note above `defer_first_frame`); their dirty flags are
        // already clear by the time the deferral lifts. The oracle's
        // `compositeFrame()` re-submits the RETAINED layer tree regardless
        // of dirty state, so its `scheduleWarmUpFrame` always has something
        // to present. FLUI has no such retained-scene re-present — without
        // re-marking the root dirty, the warm-up frame would find nothing
        // to do and the withheld content would sit blank until some
        // UNRELATED input dirtied the tree. See
        // [`redirty_root_for_represent`](Self::redirty_root_for_represent).
        if !self.first_frame_sent.load(Ordering::Relaxed) {
            self.redirty_root_for_represent();
        }
    }

    /// Re-dirty the root so the next frame actually produces content,
    /// without depending on some UNRELATED input dirtying the tree first.
    ///
    /// FLUI has no retained-scene layer to fall back on (see the
    /// first-frame-deferral module note above): a frame with nothing dirty
    /// produces `FramePaintOutcome::Idle`, not a repeat of the last
    /// `Scene`. Two callers hit this exact problem — [`allow_first_frame`]
    /// (a deferred frame becoming presentable) and the scheduler's
    /// frames-disabled→enabled re-enable edge (`AppBinding::instance()`'s
    /// bootstrap wiring; see `ADR-0035`), which has no retained scene to
    /// re-present either — so the shared logic lives here, once.
    ///
    /// Routed through [`PipelineOwner::repaint_handle`]/
    /// [`RepaintHandle::mark_needs_layout`](flui_rendering::pipeline::RepaintHandle::mark_needs_layout),
    /// NOT `Scheduler::instance()`: a caller may not be the UI thread (an
    /// async-init splash screen resolving on an executor thread, or the
    /// re-enable listener firing from whatever thread drove the lifecycle
    /// change) — `Scheduler::instance()` is thread-local, so resolving it at
    /// fire time from the wrong thread would schedule on THAT thread's
    /// fresh, undriven `Scheduler` and silently lose the wake (the
    /// fire-time-resolution hazard `AppBinding::new`'s wake-wiring comment
    /// warns against). `RepaintHandle` is `Send + Sync`, captured over a
    /// bounded channel at `PipelineOwner` construction time, and its
    /// `mark_needs_layout` fires the SAME visual-update notifier a local
    /// dirty mark does — safe and non-blocking from any thread.
    ///
    /// [`allow_first_frame`]: Self::allow_first_frame
    pub fn redirty_root_for_represent(&self) {
        redirty_pipeline_root(self.root_pipeline_owner());
    }

    /// Call this to pretend that no frames have been sent to the engine yet.
    ///
    /// This is useful for tests that want to call [`Self::defer_first_frame`] and
    /// [`Self::allow_first_frame`] since those methods only have an effect if no
    /// frames have been sent to the engine yet.
    pub fn reset_first_frame_sent(&self) {
        self.first_frame_sent.store(false, Ordering::Relaxed);
    }

    /// Latch that the first frame has been sent to the engine.
    ///
    /// Mirrors the oracle's `_firstFrameSent = true`, set unconditionally
    /// once `send_frames_to_engine()` is found `true` during a frame —
    /// regardless of whether that particular frame painted anything.
    /// Idempotent: once latched, [`Self::send_frames_to_engine`] stays
    /// `true` forever (barring [`Self::reset_first_frame_sent`], a test-only
    /// escape hatch), so calling this again is a harmless no-op.
    pub fn mark_first_frame_sent(&self) {
        self.first_frame_sent.store(true, Ordering::Relaxed);
    }

    /// Pump the rendering pipeline once and return the produced layer tree,
    /// gated by the first-frame deferral counter.
    ///
    /// Consumes the root [`PipelineOwner`] out of its lock, drives it
    /// through [`PipelineOwner::run_frame`] (layout, compositing bits,
    /// paint, semantics), and restores it — the owner is always left
    /// usable for the next frame, error or not. The layer tree this
    /// produces is withheld (`None`) while [`Self::send_frames_to_engine`]
    /// is `false`; the pipeline work itself (the warm-up cost) still runs
    /// either way, matching the oracle's `drawFrame` split (see the module
    /// note above `defer_first_frame`).
    ///
    /// This is a convenience for using `RenderingFlutterBinding` directly,
    /// without `WidgetsBinding` (see the module doc). `AppBinding`'s
    /// production frame path (`render_frame_entered`) does **not** call
    /// this method — it drives the shared pipeline through
    /// `WidgetsBinding::run_frame_with_layout_builders` instead (the
    /// build-during-layout fixpoint this method does not need to settle)
    /// and consults [`Self::send_frames_to_engine`] /
    /// [`Self::mark_first_frame_sent`] directly at its own presentation
    /// step.
    pub fn draw_frame(&self) -> Option<flui_layer::LayerTree> {
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
            self.mark_first_frame_sent();
            layer_tree
        } else {
            // Deferred-first-frame: pipeline work ran (warm-up), the
            // output is withheld until the deferral count drains.
            None
        }
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
            // Snapshot the listeners into an owned `Vec` and drop the read
            // guard before invoking any of them — a listener that calls
            // `add_semantics_enabled_listener`/`remove_semantics_enabled_listener`
            // from inside its own callback would otherwise try to acquire the
            // write lock while this thread still held the read guard and
            // deadlock (same read-then-write reentrancy `PaintingBinding`'s
            // `notify_listeners` guards against).
            let listeners = self.semantics_listeners.read().clone();
            for listener in &listeners {
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
        // Gesture state is owned by the entered `UiRealm`, which is the
        // authoritative instance driving input and frame-time coalescing for
        // its current presentation. This rendering binding deliberately does
        // not initialize a second gesture singleton with a disconnected arena.

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

    fn add_semantics_enabled_listener(&self, listener: SemanticsEnabledListener) {
        self.semantics_listeners.write().push(listener);
    }

    fn remove_semantics_enabled_listener(&self, listener: &SemanticsEnabledListener) {
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

    // The trait used to expose `render_views()` returning
    // `&RwLock<HashMap<u64, Arc<RwLock<RenderView>>>>` directly, which
    // leaked the implementer's lock topology through the trait surface.
    // The trait now exposes four narrow primitives instead; the
    // `self.render_views: RwLock<HashMap<u64, Arc<RwLock<RenderView>>>>`
    // field stays as private storage (HashMap is still the
    // canonical container -- it just isn't exposed as a lock graph anymore).
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
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// `RenderingFlutterBinding::instance()` is a process-wide singleton, so any
    /// test that mutates its shared state — the `semantics_enabled` flag, the
    /// first-frame deferral counter, or (through `allow_first_frame`) the
    /// global `Scheduler::instance()` — shares that state with every other
    /// test in this module under `cargo test`'s one-process, multi-threaded
    /// run (nextest's per-test process makes this belt and braces there, per
    /// AGENTS.md "Testing quirks"). They serialize through this lock (held for
    /// each test's duration) so they cannot interleave their writes and
    /// observe each other's state — the listener tests in particular assert
    /// exact callback counts that a concurrent toggle would corrupt.
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
        let _guard = SEMANTICS_TEST_LOCK.lock();
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

        // Add a render view via the `add_render_view_with_config`
        // default-impl helper, which delegates to `insert_render_view`
        // after deriving the view configuration.
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

    /// A listener that registers another listener from inside its own
    /// callback must not deadlock `set_semantics_enabled`.
    ///
    /// The notification loop used to run listener callbacks while still
    /// holding `semantics_listeners.read()`; a callback that then called
    /// `add_semantics_enabled_listener` (which takes the write lock) would
    /// block forever on `parking_lot`'s non-reentrant read-then-write on the
    /// same thread. The fix snapshots the listeners into an owned `Vec` and
    /// drops the read guard before invoking any of them.
    ///
    /// The whole scenario (attach, both toggles, detach) runs on one
    /// dedicated background thread rather than the test thread, for two
    /// reasons: `RenderingFlutterBinding::instance()` is owner-thread-local
    /// (ADR-0027) — a fresh OS thread's first `instance()` call leaks its own
    /// binding, so splitting the steps across threads would silently operate
    /// on two unrelated bindings — and the deadlock under test is itself a
    /// same-thread read-then-write lock reacquisition, which only reproduces
    /// when every step runs on that one thread. The test thread only bounds
    /// how long it waits for that thread's result over a channel: a bare
    /// `.join()` would hang forever (and, under `cargo test`'s shared-process
    /// threads, wedge the rest of the suite) if the deadlock regressed;
    /// `recv_timeout` turns that hang into an explicit `expect` panic
    /// instead.
    #[test]
    fn semantics_listener_that_registers_another_listener_does_not_deadlock() {
        use std::sync::{atomic::AtomicUsize, mpsc};
        use std::time::Duration;

        let _guard = SEMANTICS_TEST_LOCK.lock();

        let (result_tx, result_rx) = mpsc::channel();
        std::thread::spawn(move || {
            let binding = RenderingFlutterBinding::instance();
            binding.set_semantics_enabled(false);

            let late_calls = Arc::new(AtomicUsize::new(0));
            let late_calls_for_listener = Arc::clone(&late_calls);
            let late_listener: Arc<dyn Fn(bool) + Send + Sync> = Arc::new(move |_enabled| {
                late_calls_for_listener.fetch_add(1, Ordering::Relaxed);
            });

            // The listener is `'static`, so the reentrant call resolves the
            // thread's binding through its singleton accessor instead of
            // capturing this stack-local reference.
            let late_listener_for_reentrant = late_listener.clone();
            let reentrant_listener: Arc<dyn Fn(bool) + Send + Sync> = Arc::new(move |_enabled| {
                RenderingFlutterBinding::instance()
                    .add_semantics_enabled_listener(late_listener_for_reentrant.clone());
            });
            binding.add_semantics_enabled_listener(reentrant_listener.clone());

            // First toggle: `reentrant_listener` fires and registers
            // `late_listener`, but must not itself run in this same pass.
            binding.set_semantics_enabled(true);
            let after_first = late_calls.load(Ordering::Relaxed);

            // Second toggle: now observes `late_listener`, registered above.
            binding.set_semantics_enabled(false);
            let after_second = late_calls.load(Ordering::Relaxed);

            binding.remove_semantics_enabled_listener(&late_listener);
            binding.remove_semantics_enabled_listener(&reentrant_listener);

            let _ = result_tx.send((after_first, after_second));
        });

        let (after_first, after_second) = result_rx.recv_timeout(Duration::from_secs(5)).expect(
            "set_semantics_enabled deadlocked: a listener that registers another \
             listener from inside its own callback must not block on the \
             notification loop's own read guard",
        );

        assert_eq!(
            after_first, 0,
            "a listener registered mid-notification must not run in the same \
             pass that registered it",
        );
        assert_eq!(
            after_second, 1,
            "a later toggle must observe the listener registered mid-notification",
        );
    }
}
