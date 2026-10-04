//! RenderingBinding - Concrete implementation of RendererBinding.
//!
//! This is the glue between the render trees and the FLUI engine.
//! It manages multiple independent render trees, each rooted in a RenderView.
//!
//! # Architecture
//!
//! ```text
//! RenderingBinding
//!   ├── root_pipeline_owner   - Root of PipelineOwner tree
//!   ├── render_views          - Map<ViewId, RenderView>
//!   └── semantics enablement  - fan-out via add_semantics_enabled_listener;
//!                               per-presentation announce/event delivery
//!                               lives on that presentation's SemanticsHost
//!                               (crate::semantics_host), not here
//! ```
//!
//! # Usage
//!
//! For most applications, use `UiRealm` instead ([`crate::ui_realm`]),
//! which owns this binding plus widgets support per window. Use
//! `RenderingBinding` directly only when working with the rendering
//! layer without widgets.

use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
};

use flui_foundation::geometry::Offset;
use flui_rendering::{
    binding::RendererBinding,
    hit_testing::HitTestResult,
    pipeline::{PipelineCell, PipelineOwner, TextContextHandle},
    view::{RenderView, ViewConfiguration},
};
use parking_lot::RwLock;

use flui_scheduler::{UpdateScheduler, WeakUpdateScheduler};

/// A subscriber to [`RenderingBinding::set_semantics_enabled`] changes.
///
/// Named alias for `Arc<dyn Fn(bool) + Send + Sync>` so the field and impl
/// signatures below read as intent rather than nested generics (kills the
/// `clippy::type_complexity` lint that fired on the inline form). Identical
/// to the type spelled out in `RendererBinding::add_semantics_enabled_listener`
/// (flui-rendering) — a type alias is transparent, so this satisfies the
/// trait without repeating the trait's own long-hand spelling here.
type SemanticsEnabledListener = Arc<dyn Fn(bool) + Send + Sync>;

/// Shared body for [`RenderingBinding::redirty_root_for_represent`]:
/// operates on the bare [`PipelineCell`] (rather than requiring a full
/// `RenderingBinding` reference) so a caller that only holds that
/// handle can reuse the identical logic instead of re-deriving it.
pub(crate) fn redirty_pipeline_root(pipeline_owner: &PipelineCell) {
    let render_invalidation_handle = pipeline_owner.with(|root_owner| {
        root_owner
            .root_id()
            .and_then(|root_id| root_owner.render_invalidation_handle(root_id))
    });
    if let Some(handle) = render_invalidation_handle
        && let Err(e) = handle.mark_needs_layout()
    {
        tracing::warn!(
            error = ?e,
            "redirty_root_for_represent: failed to re-mark the root dirty; \
             the withheld/re-enabled content may not present until \
             something else dirties the tree",
        );
    }
}

// ============================================================================
// RenderingBinding
// ============================================================================

/// Concrete binding for applications using the Rendering framework directly.
///
/// This is the glue that binds the framework to the FLUI engine.
/// For widget-based applications, use `UiRealm` instead.
///
/// # Responsibilities
///
/// - Managing the root [`PipelineOwner`] tree
/// - Managing [`RenderView`]s (add/remove)
/// - Creating [`ViewConfiguration`]s for views
/// - Coordinating frame production
/// - Fanning out semantics-enabled changes to listeners (per-presentation
///   announce/event delivery lives on that presentation's `SemanticsHost`
///   instead — see `crate::semantics_host`)
///
/// # Thread Safety
///
/// Owner-thread-confined (`!Send + !Sync`, transitively via
/// `PipelineCell`'s `Rc<RefCell<_>>`): a `PipelineOwner` belongs to exactly
/// one presentation on exactly one thread. Non-pipeline internal state
/// still uses `RwLock`/`Arc` for the same-thread checkout discipline
/// this binding shares with the rest of the realm.
///
/// This is enforced at compile time, not by convention -- pinned by
/// `assert_not_impl_any!(PipelineCell: Send, Sync)` and
/// `assert_not_impl_any!(PipelineOwner: Send, Sync)` in
/// `flui_rendering::pipeline::owner::cell`'s own tests. A clone taken from
/// this binding cannot cross a thread boundary:
///
/// ```compile_fail
/// use flui_runtime::renderer_binding::RenderingBinding;
/// use flui_rendering::binding::RendererBinding;
///
/// let binding =
///     RenderingBinding::new(flui_rendering::TextContextHandle::standalone());
/// let pipeline = binding.root_pipeline_owner().clone();
/// // error[E0277]: `Rc<RefCell<PipelineOwner>>` cannot be sent between
/// // threads safely -- `PipelineCell` is `!Send` by construction, so this
/// // never reaches the runtime deadlock the old `Arc<RwLock<_>>` shape
/// // risked; it fails to compile instead.
/// std::thread::spawn(move || {
///     pipeline.with(|owner| owner.root_id());
/// });
/// ```
pub struct RenderingBinding {
    /// Root of the PipelineOwner tree (shared with the owning `UiRealm`'s
    /// presentation).
    root_pipeline_owner: PipelineCell,

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

    /// The owning realm's scheduler, weak: `request_visual_update`'s
    /// device-metrics force-frame path needs to schedule a frame, but this
    /// binding must not keep a dead realm's scheduler alive (it is a plain
    /// field on `UiRealm`, not the other way around).
    scheduler: WeakUpdateScheduler,

    /// Keeps `scheduler`'s backing `UpdateScheduler` alive — but ONLY for the
    /// standalone constructor path ([`Self::new`]), which owns
    /// no external scheduler for anything else to keep alive. `None` for
    /// every [`Self::new_with_pipeline`] caller (production: the owning
    /// `UiRealm` holds the real strong root, per `scheduler`'s own doc).
    ///
    /// Without this field, `Self::new` passed a bare `&UpdateScheduler::new()`
    /// into `new_with_pipeline`, which only stores the *downgraded*
    /// `WeakUpdateScheduler` — the temporary `UpdateScheduler` had no other strong
    /// owner, so it dropped at the end of `new()`'s constructing statement,
    /// and `request_visual_update`'s upgrade silently, permanently failed
    /// from the moment construction returned. That made the device-metrics
    /// force-frame path a dead no-op for every standalone binding, with
    /// nothing in the constructor's signature hinting at it.
    standalone_scheduler: Option<UpdateScheduler>,
}

impl std::fmt::Debug for RenderingBinding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RenderingBinding")
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

impl RenderingBinding {
    /// Creates a new rendering binding with its own PipelineOwner, measuring
    /// text through `text`, and a fresh `UpdateScheduler` it owns for its own
    /// lifetime — test/standalone use only. Production always goes through
    /// [`new_with_pipeline`](Self::new_with_pipeline) with the owning
    /// realm's own scheduler.
    ///
    /// The constructed `UpdateScheduler` is kept alive internally (in this crate's
    /// private `standalone_scheduler` field) for exactly as long as this
    /// binding lives, so `request_visual_update`
    /// genuinely schedules a frame on it rather than silently failing an
    /// upgrade against an already-dead weak (see that field's doc for the
    /// bug this fixes). Nothing pumps this scheduler's frame loop
    /// automatically — there is no realm behind a standalone binding — so a
    /// scheduled callback sits queued, harmlessly, until the binding drops;
    /// a caller that wants it to actually fire must drive the scheduler
    /// itself.
    pub fn new(text: TextContextHandle) -> Self {
        let scheduler = UpdateScheduler::new();
        let mut binding =
            Self::new_with_pipeline(PipelineCell::new(PipelineOwner::new(text)), &scheduler);
        binding.standalone_scheduler = Some(scheduler);
        binding
    }

    /// Test/standalone construction sharing a CALLER-supplied
    /// [`PipelineCell`] rather than building a fresh one.
    ///
    /// Same self-owned-scheduler shape as [`Self::new`] (see
    /// `standalone_scheduler`'s field doc for the dead-weak-reference bug
    /// this avoids) — for `PresentationState::new_for_test`, which must
    /// share its exact caller-supplied pipeline with its renderer but has no
    /// realm above it to supply a live `&UpdateScheduler`.
    #[cfg(test)]
    pub(crate) fn new_for_test_with_pipeline(pipeline_owner: PipelineCell) -> Self {
        let scheduler = UpdateScheduler::new();
        let mut binding = Self::new_with_pipeline(pipeline_owner, &scheduler);
        binding.standalone_scheduler = Some(scheduler);
        binding
    }

    /// Creates a new rendering binding with a shared PipelineOwner, wired to
    /// `scheduler` for its (rare) device-metrics force-frame path.
    ///
    /// This allows the owning `UiRealm` to pass in the same
    /// [`PipelineCell`] that elements use, ensuring a single PipelineOwner
    /// instance at runtime, and its OWN scheduler — never a process-global
    /// one.
    pub fn new_with_pipeline(pipeline_owner: PipelineCell, scheduler: &UpdateScheduler) -> Self {
        Self::init_instances();
        Self {
            root_pipeline_owner: pipeline_owner,
            render_views: RwLock::new(HashMap::new()),
            semantics_enabled: AtomicBool::new(false),
            semantics_listeners: RwLock::new(Vec::new()),
            first_frame_deferred_count: AtomicU32::new(0),
            first_frame_sent: AtomicBool::new(false),
            scheduler: scheduler.downgrade(),
            standalone_scheduler: None,
        }
    }

    /// One-time construction-side setup.
    ///
    /// Not a `BindingBase`/singleton-macro hook any more (the singleton
    /// pattern this binding used to implement is retired) — a plain
    /// associated function [`new_with_pipeline`](Self::new_with_pipeline)
    /// calls once, right before building the value (there is nothing yet to
    /// take `&self` of).
    fn init_instances() {
        // Gesture state is owned by the entered `UiRealm`, which is the
        // authoritative instance driving input and frame-time coalescing for
        // its current presentation. This rendering binding deliberately does
        // not initialize a second gesture singleton with a disconnected arena.
        //
        // Painting has no binding at all: the app's font collection, fed from
        // one host scan, is built by `AppRuntime`'s `SharedEngineServices`
        // at realm install (`app/runtime.rs`), and nothing about painting is
        // process-wide.
        //
        // Semantics enablement is per-presentation now (`SemanticsHost`,
        // `crate::semantics_host`) -- there is no process-wide semantics
        // binding for this method to touch.
        tracing::info!("RenderingBinding initialized");
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
    // `RendererBinding` trait impl below and `UiRealm::defer_first_frame`
    // / `allow_first_frame` / `send_frames_to_engine` in
    // `crates/flui-runtime/src/ui_realm/`, which the production
    // `render_frame` path actually calls) forwards to this struct.
    //
    // # First-frame gate
    //
    // `send_frames_to_engine` is `first_frame_sent || deferred_count == 0`.
    // Layout/compositing-bits/paint always run; only the composite-to-engine
    // step is gated. The production split lives in `UiRealm::
    // render_frame` (`crates/flui-runtime/src/ui_realm/`): the
    // build/layout/paint pipeline always runs in `draw_frame_entered`, and
    // only the GPU `render_scene` (present) call is gated on
    // `send_frames_to_engine`. `run_frame` does not yet gate its own
    // semantics phase behind this flag — a documented, narrower scope for
    // this unit, not a silent gap.
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
    ///
    /// # Panics
    ///
    /// Panics when the deferral count is exhausted, preserving all existing deferrals.
    pub fn defer_first_frame(&self) {
        self.first_frame_deferred_count
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |count| {
                count.checked_add(1)
            })
            .expect("first-frame deferral count exhausted");
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
        self.first_frame_deferred_count
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |count| {
                count.checked_sub(1)
            })
            .expect("allow_first_frame called without matching defer_first_frame");

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
    /// (a deferred frame becoming presentable) and the realm's own
    /// scheduler's frames-disabled→enable re-enable edge (see `ADR-0035`
    /// and `emit_lifecycle_transition`'s doc in `runner.rs`), which has no
    /// retained scene to re-present either — so the shared logic lives
    /// here, once.
    ///
    /// Routed through [`PipelineOwner::render_invalidation_handle`]/
    /// [`RenderInvalidationHandle::mark_needs_layout`](flui_rendering::pipeline::RenderInvalidationHandle::mark_needs_layout),
    /// not a scheduler reach: a caller may not be the UI thread (an
    /// async-init splash screen resolving on an executor thread, or the
    /// re-enable listener firing from whatever thread drove the lifecycle
    /// change) — resolving a scheduler by any thread-local at fire time from
    /// the wrong thread would silently target the wrong instance and lose
    /// the wake. `RenderInvalidationHandle` is `Send + Sync`, captured over a bounded
    /// channel at `PipelineOwner` construction time, and its
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
    /// This is a convenience for using `RenderingBinding` directly,
    /// without `WidgetsBinding` (see the module doc). The production
    /// frame path (`UiRealm::render_frame_entered`) does **not** call
    /// this method — it drives the shared pipeline through
    /// `WidgetsBinding::run_frame_with_layout_builders` instead (the
    /// build-during-layout fixpoint this method does not need to settle)
    /// and consults [`Self::send_frames_to_engine`] /
    /// [`Self::mark_first_frame_sent`] directly at its own presentation
    /// step.
    pub fn draw_frame(&self) -> Option<flui_layer::LayerTree> {
        let layer_tree = self.root_pipeline_owner().with_mut(|guard| {
            let owner = guard.take_idle();
            let (owner, result) = owner.run_frame();
            *guard = owner;
            match result {
                Ok(layer_tree) => layer_tree,
                Err(e) => {
                    tracing::error!(error = ?e, "draw_frame: pipeline failed, dropping frame");
                    None
                }
            }
        });

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

    /// Sets whether semantics are enabled, fanning the change out to every
    /// registered listener.
    ///
    /// When enabled, the framework will maintain the semantics tree. This no
    /// longer forwards to a process-wide semantics binding (retired —
    /// enablement is per-presentation now, via `SemanticsHost`
    /// (`crate::semantics_host`)): a caller that owns a presentation's
    /// `SemanticsHost` and wants it to track this toggle registers
    /// [`Self::add_semantics_enabled_listener`] and calls
    /// `SemanticsHost::set_platform_semantics_enabled` from that listener.
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
        }
    }
}

// ============================================================================
// RendererBinding Implementation
// ============================================================================

impl RendererBinding for RenderingBinding {
    // ---- formerly PipelineManifold ----

    fn request_visual_update(&self) {
        // Upgrade-or-skip: if the owning realm's scheduler is already gone,
        // there is no frame left to schedule.
        //
        // Routes through the scheduler's gated pair (`ensure_visual_update`
        // -> `schedule_frame_if_enabled`), which no-ops while frames are
        // disabled instead of unconditionally requesting one.
        if let Some(scheduler) = self.scheduler.upgrade() {
            scheduler.ensure_visual_update();
        }
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
        self.root_pipeline_owner
            .with(|owner| owner.hit_test(position, result));
    }

    // ---- RendererBinding proper ----

    fn root_pipeline_owner(&self) -> &PipelineCell {
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
        let _prev = self.render_views.write().insert(view_id, view);
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

    // `RenderingBinding` is no longer singleton-backed — each test
    // below constructs its own, independent instance instead of sharing a
    // process-wide one, so there is nothing left for a test lock to
    // serialize (the retired semantics-binding test lock guarded exactly
    // this shared-singleton hazard; see AGENTS.md's "Testing quirks").

    /// The authoritative frame path: `draw_frame` returns the layer
    /// tree the pipeline produced (for the caller to wrap in a Scene
    /// and hand to the renderer), and withholds it while the first
    /// frame is deferred. Uses an isolated (non-singleton) binding so
    /// no other test's pipeline state can interfere.
    fn deferred_frame_returns_the_painted_tree_after_release() {
        use flui_foundation::geometry::Size;
        use flui_objects::RenderColoredBox;
        use flui_rendering::constraints::BoxConstraints;

        let owner = PipelineCell::new(PipelineOwner::new(TextContextHandle::standalone()));
        let root_id = owner.with_mut(|o| {
            let id = o.insert(Box::new(RenderColoredBox::red(40.0, 40.0))
                as Box<
                    dyn flui_rendering::traits::RenderObject<flui_rendering::protocol::BoxProtocol>,
                >);
            o.set_root_id(Some(id));
            o.set_root_constraints(Some(BoxConstraints::tight(Size::new(100.0, 100.0))));
            id
        });
        // Bound to a local, not passed as a bare temporary: `new_with_pipeline`
        // only stores a downgraded `WeakUpdateScheduler`, so a temporary here would
        // drop at the end of THIS statement and leave the binding holding a
        // permanently-dead weak (the exact bug `standalone_scheduler` fixes
        // for `Self::new` — this test's binding takes the explicit-scheduler
        // path instead, so it must keep its own scheduler alive the ordinary
        // way, via a local binding that outlives the statement).
        let scheduler = flui_scheduler::UpdateScheduler::new();
        let binding = RenderingBinding::new_with_pipeline(owner, &scheduler);

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
            .with_mut(|owner| owner.mark_needs_layout(root_id));
        let tree = binding
            .draw_frame()
            .expect("non-deferred frame with dirty work must return the layer tree");
        assert!(
            tree.len() > 1,
            "the produced layer tree must carry the painted content under its root",
        );
    }

    fn an_exhausted_deferral_count_preserves_all_existing_deferrals() {
        let binding = RenderingBinding::new(TextContextHandle::standalone());
        // Exhausting the public counter would require billions of calls. Seed
        // this failure boundary, then exercise only the public gate operations.
        binding
            .first_frame_deferred_count
            .store(u32::MAX, Ordering::Relaxed);
        for _ in 0..2 {
            assert!(
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    binding.defer_first_frame();
                }))
                .is_err()
            );
            assert!(!binding.send_frames_to_engine());
        }
        binding.allow_first_frame();
        assert!(
            !binding.send_frames_to_engine(),
            "other deferrals remain live"
        );
        binding.defer_first_frame();
        assert!(!binding.send_frames_to_engine());
    }

    #[test]
    fn draw_frame_returns_layer_tree_and_defers_when_gated() {
        crate::table_test::run_table(
            "draw_frame_returns_layer_tree_and_defers_when_gated",
            &[
                (
                    "deferred_frame_returns_the_painted_tree_after_release",
                    deferred_frame_returns_the_painted_tree_after_release,
                ),
                (
                    "an_exhausted_deferral_count_preserves_all_existing_deferrals",
                    an_exhausted_deferral_count_preserves_all_existing_deferrals,
                ),
            ],
        );
    }
}
