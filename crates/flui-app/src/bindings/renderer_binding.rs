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
//!   └── semantics enablement  - fan-out via add_semantics_enabled_listener;
//!                               per-presentation announce/event delivery
//!                               lives on that presentation's SemanticsHost
//!                               (crate::app::semantics_host), not here
//! ```
//!
//! # Usage
//!
//! For most applications, use `UiRealm` instead (`crate::app::ui_realm`),
//! which owns this binding plus widgets support per window. Use
//! `RenderingFlutterBinding` directly only when working with the rendering
//! layer without widgets.

use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
};

use flui_painting::PaintingBinding;
use flui_rendering::{
    binding::RendererBinding,
    hit_testing::HitTestResult,
    pipeline::{PipelineCell, PipelineOwner},
    view::{RenderView, ViewConfiguration},
};
use flui_types::Offset;
use parking_lot::RwLock;

use flui_scheduler::{Scheduler, WeakScheduler};

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
/// operates on the bare [`PipelineCell`] (rather than requiring a full
/// `RenderingFlutterBinding` reference) so a caller that only holds that
/// handle can reuse the identical logic instead of re-deriving it.
pub(crate) fn redirty_pipeline_root(pipeline_owner: &PipelineCell) {
    let repaint_handle = pipeline_owner.with(|root_owner| {
        root_owner
            .root_id()
            .and_then(|root_id| root_owner.repaint_handle(root_id))
    });
    if let Some(handle) = repaint_handle
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
// RenderingFlutterBinding
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
///   instead — see `crate::app::semantics_host`)
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
/// use flui_app::bindings::RenderingFlutterBinding;
/// use flui_rendering::binding::RendererBinding;
///
/// let binding = RenderingFlutterBinding::new();
/// let pipeline = binding.root_pipeline_owner().clone();
/// // error[E0277]: `Rc<RefCell<PipelineOwner>>` cannot be sent between
/// // threads safely -- `PipelineCell` is `!Send` by construction, so this
/// // never reaches the runtime deadlock the old `Arc<RwLock<_>>` shape
/// // risked; it fails to compile instead.
/// std::thread::spawn(move || {
///     pipeline.with(|owner| owner.root_id());
/// });
/// ```
pub struct RenderingFlutterBinding {
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
    scheduler: WeakScheduler,

    /// Keeps `scheduler`'s backing `Scheduler` alive — but ONLY for the
    /// standalone constructor path ([`Self::new`] / [`Default`]), which owns
    /// no external scheduler for anything else to keep alive. `None` for
    /// every [`Self::new_with_pipeline`] caller (production: the owning
    /// `UiRealm` holds the real strong root, per `scheduler`'s own doc).
    ///
    /// Without this field, `Self::new()` passed a bare `&Scheduler::new()`
    /// into `new_with_pipeline`, which only stores the *downgraded*
    /// `WeakScheduler` — the temporary `Scheduler` had no other strong
    /// owner, so it dropped at the end of `new()`'s constructing statement,
    /// and `request_visual_update`'s upgrade silently, permanently failed
    /// from the moment construction returned. That made the device-metrics
    /// force-frame path a dead no-op for every standalone binding, with
    /// nothing in the constructor's signature hinting at it.
    standalone_scheduler: Option<Scheduler>,
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
    /// Creates a new rendering binding with its own PipelineOwner and a
    /// fresh `Scheduler` it owns for its own lifetime — test/standalone use
    /// only. Production always goes through
    /// [`new_with_pipeline`](Self::new_with_pipeline) with the owning
    /// realm's own scheduler.
    ///
    /// The constructed `Scheduler` is kept alive internally (in this crate's
    /// private `standalone_scheduler` field) for exactly as long as this
    /// binding lives, so `request_visual_update`
    /// genuinely schedules a frame on it rather than silently failing an
    /// upgrade against an already-dead weak (see that field's doc for the
    /// bug this fixes). Nothing pumps this scheduler's frame loop
    /// automatically — there is no realm behind a standalone binding — so a
    /// scheduled callback sits queued, harmlessly, until the binding drops;
    /// a caller that wants it to actually fire must drive the scheduler
    /// itself.
    pub fn new() -> Self {
        let scheduler = Scheduler::new();
        let mut binding =
            Self::new_with_pipeline(PipelineCell::new(PipelineOwner::new()), &scheduler);
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
    pub fn new_with_pipeline(pipeline_owner: PipelineCell, scheduler: &Scheduler) -> Self {
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
        // Painting has no process-wide binding to eagerly initialize here
        // either: `PaintingBinding` stopped being a singleton as the
        // remaining singletons move behind explicit owners -- its owned
        // value lives on `AppRuntime`'s `SharedEngineServices`, constructed
        // at realm install (`app/runtime.rs`). See `Self::painting`'s doc
        // comment for why `RenderingFlutterBinding` cannot simply reach that
        // value instead.
        //
        // Semantics enablement is per-presentation now (`SemanticsHost`,
        // `app/semantics_host.rs`) -- there is no process-wide semantics
        // binding for this method to touch.
        tracing::info!("RenderingFlutterBinding initialized");
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
    /// (a deferred frame becoming presentable) and the realm's own
    /// scheduler's frames-disabled→enable re-enable edge (see `ADR-0035`
    /// and `emit_lifecycle_transition`'s doc in `runner.rs`), which has no
    /// retained scene to re-present either — so the shared logic lives
    /// here, once.
    ///
    /// Routed through [`PipelineOwner::repaint_handle`]/
    /// [`RepaintHandle::mark_needs_layout`](flui_rendering::pipeline::RepaintHandle::mark_needs_layout),
    /// not a scheduler reach: a caller may not be the UI thread (an
    /// async-init splash screen resolving on an executor thread, or the
    /// re-enable listener firing from whatever thread drove the lifecycle
    /// change) — resolving a scheduler by any thread-local at fire time from
    /// the wrong thread would silently target the wrong instance and lose
    /// the wake. `RepaintHandle` is `Send + Sync`, captured over a bounded
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
        let layer_tree = self.root_pipeline_owner().with_mut(|guard| {
            let owner = std::mem::take(guard);
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
    /// (`crate::app::semantics_host`)): a caller that owns a presentation's
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

    // ========================================================================
    // Binding Accessors
    // ========================================================================

    /// Returns a fresh [`PaintingBinding`] value; its `ImageCache` is
    /// throwaway, but `font_system()` still reaches shared state (see
    /// below).
    ///
    /// **Inert by construction, not just as a compile-preserving stopgap.**
    /// `PaintingBinding` left the ambient-singleton graph as the remaining
    /// singletons here move behind explicit owners: its real, long-lived
    /// value now lives on `AppRuntime`'s `SharedEngineServices`
    /// (`app/runtime.rs`), constructed once at realm install.
    /// `RenderingFlutterBinding` has no field or accessor reaching that
    /// runtime-owned value — it is itself still a process-wide singleton,
    /// structurally unrelated to `AppRuntime` — and adding one here would
    /// mean inventing a *new* ambient path from one singleton into another,
    /// which is the opposite of what retiring these singletons is for.
    /// Until `RenderingFlutterBinding` is re-homed to receive its painting
    /// service from the owning runtime instead of constructing its own,
    /// every call to this method returns a brand new `PaintingBinding` whose
    /// `ImageCache` and `SystemFontsNotifier` start and end empty/unused —
    /// nothing else in the process observes whatever you do to THOSE two
    /// fields through this handle. Do not use this to configure or read the
    /// "real" image cache; there isn't one reachable from here today.
    ///
    /// **`font_system()` is the one exception.** Unlike the image cache,
    /// `PaintingBinding::font_system()` does not read this struct's own
    /// fields at all — it reaches the process-wide shared `FONT_SYSTEM`
    /// (`flui-painting`'s `text_layout/layout.rs`), the exact same one every
    /// other `PaintingBinding` value (including the real one on
    /// `AppRuntime`) resolves to. So `Self::painting().font_system()...` or
    /// `Self::painting().register_font(...)` through this "throwaway" value
    /// DOES mutate real, shared, process-wide font state, visible to every
    /// other consumer — the isolation this doc comment describes is
    /// specific to the image cache and font-change notifier, not to
    /// everything reachable through the returned value.
    pub fn painting() -> PaintingBinding {
        PaintingBinding::new()
    }

    // ========================================================================
    // Memory Management
    // ========================================================================

    /// Currently a no-op: does NOT clear any real image cache.
    ///
    /// See [`Self::painting`]'s doc comment for why. This method
    /// deliberately does NOT call `Self::painting().handle_memory_pressure()`:
    /// that inner method's own `tracing::info!("Memory pressure: clearing
    /// image cache")` is legitimate for a real, owned `PaintingBinding`
    /// value, but firing it here — against a throwaway value whose
    /// `ImageCache` starts and ends empty — would be exactly the false "it
    /// worked" signal this method is trying not to emit. This becomes
    /// meaningful again once `RenderingFlutterBinding` is re-homed to
    /// receive its painting service from the owning runtime. Traced at
    /// `trace`, not `info`, so the no-op itself does not read as a real
    /// memory-pressure response in a production log.
    pub fn handle_memory_pressure(&self) {
        tracing::trace!(
            "RenderingFlutterBinding::handle_memory_pressure: inert no-op on a \
             throwaway PaintingBinding -- see the doc comment on this method \
             and on Self::painting"
        );
    }

    /// Test-only: upgrades `scheduler` and reports whether it succeeded —
    /// the direct pin for [`Self::standalone_scheduler`]'s whole reason to
    /// exist (a standalone binding's weak must stay upgradeable for its
    /// entire lifetime, unlike a bare `new_with_pipeline` call handed a
    /// temporary that has already dropped).
    #[cfg(test)]
    fn scheduler_is_alive(&self) -> bool {
        self.scheduler.upgrade().is_some()
    }
}

// ============================================================================
// RendererBinding Implementation
// ============================================================================

impl RendererBinding for RenderingFlutterBinding {
    // ---- formerly PipelineManifold ----

    fn request_visual_update(&self) {
        // Upgrade-or-skip: if the owning realm's scheduler is already gone,
        // there is no frame left to schedule a callback into.
        if let Some(scheduler) = self.scheduler.upgrade() {
            scheduler.schedule_frame(Box::new(|_timing| {
                // Visual update frame callback
            }));
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

    // `RenderingFlutterBinding` is no longer singleton-backed — each test
    // below constructs its own, independent instance instead of sharing a
    // process-wide one, so there is nothing left for a test lock to
    // serialize (the retired semantics-binding test lock guarded exactly
    // this shared-singleton hazard; see AGENTS.md's "Testing quirks").

    /// Red before the fix: `RenderingFlutterBinding::new()` used to pass a
    /// bare `&Scheduler::new()` temporary into `new_with_pipeline`, which
    /// only stores the *downgraded* `WeakScheduler` — nothing else held a
    /// strong reference to that freshly constructed `Scheduler`, so it
    /// dropped at the end of `new()`'s constructing statement and
    /// `request_visual_update`'s upgrade silently, permanently failed from
    /// the moment construction returned (this test would have failed both
    /// assertions below against that shape). Green now: `new()` keeps its
    /// own `Scheduler` alive in `standalone_scheduler` for exactly as long
    /// as the binding lives, so the weak stays upgradeable and
    /// `request_visual_update` genuinely schedules a frame — observable via
    /// `is_frame_scheduled()` flipping true. Nothing ever pumps this
    /// scheduler (there is no realm behind a standalone binding), so this
    /// pins that the SCHEDULING attempt succeeds, not that a frame executes.
    #[test]
    fn standalone_binding_keeps_its_scheduler_alive_so_request_visual_update_actually_schedules() {
        let binding = RenderingFlutterBinding::new();
        assert!(
            binding.scheduler_is_alive(),
            "a standalone binding's own scheduler must outlive construction, \
             not die the moment new() returns"
        );

        binding.request_visual_update();

        let scheduler = binding
            .scheduler
            .upgrade()
            .expect("standalone_scheduler keeps this upgradeable for the binding's whole life");
        assert!(
            scheduler.is_frame_scheduled(),
            "request_visual_update must genuinely schedule a frame on a live \
             standalone scheduler, not silently no-op against an already-dead weak"
        );
    }

    #[test]
    fn test_semantics_enabled() {
        let binding = RenderingFlutterBinding::new();
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
        let binding = RenderingFlutterBinding::new();

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

        let owner = PipelineCell::new(PipelineOwner::new());
        let root_id = owner.with_mut(|o| {
            let id = o.insert(Box::new(RenderColoredBox::red(40.0, 40.0))
                as Box<
                    dyn flui_rendering::traits::RenderObject<flui_rendering::protocol::BoxProtocol>,
                >);
            o.set_root_id(Some(id));
            o.set_root_constraints(Some(BoxConstraints::tight(Size::new(px(100.0), px(100.0)))));
            id
        });
        // Bound to a local, not passed as a bare temporary: `new_with_pipeline`
        // only stores a downgraded `WeakScheduler`, so a temporary here would
        // drop at the end of THIS statement and leave the binding holding a
        // permanently-dead weak (the exact bug `standalone_scheduler` fixes
        // for `Self::new()` — this test's binding takes the explicit-scheduler
        // path instead, so it must keep its own scheduler alive the ordinary
        // way, via a local binding that outlives the statement).
        let scheduler = flui_scheduler::Scheduler::new();
        let binding = RenderingFlutterBinding::new_with_pipeline(owner, &scheduler);

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

        let owner = PipelineCell::new(PipelineOwner::new());
        owner.with_mut(|o| {
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
        });
        // See the sibling `draw_frame_returns_layer_tree_and_defers_when_gated`
        // test's comment: bound to a local so it outlives this statement,
        // not passed as a bare temporary.
        let scheduler = flui_scheduler::Scheduler::new();
        let binding = RenderingFlutterBinding::new_with_pipeline(owner, &scheduler);
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
        let binding = RenderingFlutterBinding::new();

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

        let binding = RenderingFlutterBinding::new();

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

    /// A pointer proving Send/Sync only by never actually crossing a
    /// thread — see the test below for why one is needed.
    ///
    /// SAFETY: `RendererBinding::add_semantics_enabled_listener` demands
    /// `Arc<dyn Fn(bool) + Send + Sync>` because *some* bindings fan out to
    /// genuinely cross-thread platform-accessibility callers. This test's
    /// `RenderingFlutterBinding`, though, is owner-thread-confined
    /// (`!Send`/`!Sync`, transitively via `PipelineCell`) and every closure
    /// built from a `BindingPtr` is invoked synchronously, inline, from
    /// `set_semantics_enabled` on the same thread and within the lifetime
    /// of the local `binding` these pointers are taken from — never stored
    /// past that call, never actually sent anywhere. The `unsafe impl`
    /// therefore satisfies the trait's *type-level* requirement without
    /// violating what it protects against in any binding that is genuinely
    /// shared across threads.
    struct BindingPtr(*const RenderingFlutterBinding);
    #[allow(unsafe_code)]
    unsafe impl Send for BindingPtr {}
    #[allow(unsafe_code)]
    unsafe impl Sync for BindingPtr {}
    impl BindingPtr {
        // SAFETY: see the struct's doc — valid only for the duration of the
        // local `binding` this test constructs it from.
        #[allow(unsafe_code)]
        unsafe fn get(&self) -> &RenderingFlutterBinding {
            unsafe { &*self.0 }
        }
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
    /// Runs directly on the test thread rather than a spawned one:
    /// `RenderingFlutterBinding` is now owner-thread-confined (`!Send`,
    /// transitively via `PipelineCell`), so it can no longer cross an
    /// `Arc` into `std::thread::spawn`, and the reentrant listener's own
    /// closure can no longer capture `binding` directly — it would make the
    /// closure itself `!Send`, which `add_semantics_enabled_listener`'s
    /// signature forbids (see `BindingPtr`'s doc). The deadlock this test
    /// guards against is lock reentrancy on `semantics_listeners`, not a
    /// cross-thread race, so same-thread execution exercises the identical
    /// hazard.
    ///
    /// The former `recv_timeout`-bounded hang-into-panic conversion is
    /// restored below by a watchdog thread instead: it never touches
    /// `binding` (only a `Send`-safe `Arc<AtomicBool>` completion flag),
    /// so it does not reopen the same-thread-confinement problem this test
    /// exists to accommodate. `cargo nextest` has no per-test timeout
    /// configured in this workspace, so an actual regression here — the
    /// exact deadlock this test guards against — would otherwise hang the
    /// whole run rather than fail it.
    #[test]
    fn semantics_listener_that_registers_another_listener_does_not_deadlock() {
        use std::sync::atomic::AtomicUsize;
        use std::time::Duration;

        // Watchdog: a real regression here is a lock-reentrancy deadlock on
        // the *main test thread*, which a panic on any OTHER thread cannot
        // unblock — only terminating the process can. `completed` is the
        // only thing the watchdog thread touches; `binding` itself never
        // crosses a thread boundary.
        let completed = Arc::new(AtomicBool::new(false));
        let completed_watchdog = Arc::clone(&completed);
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(5));
            if !completed_watchdog.load(Ordering::SeqCst) {
                eprintln!(
                    "semantics_listener_that_registers_another_listener_does_not_deadlock: \
                     watchdog fired after 5s -- the test thread is hung, which IS the \
                     lock-reentrancy deadlock this test exists to catch. Hard-exiting: a \
                     panic on this thread cannot unblock the hung test thread, and no \
                     nextest terminate policy would otherwise stop this run."
                );
                std::process::exit(101);
            }
        });

        let binding = RenderingFlutterBinding::new();
        // `&raw const` takes the pointer directly, without building an
        // intermediate `&RenderingFlutterBinding` reference just to decay it.
        let binding_ptr = BindingPtr(&raw const binding);

        let late_calls = Arc::new(AtomicUsize::new(0));
        let late_calls_for_listener = Arc::clone(&late_calls);
        let late_listener: Arc<dyn Fn(bool) + Send + Sync> = Arc::new(move |_enabled| {
            late_calls_for_listener.fetch_add(1, Ordering::Relaxed);
        });

        let late_listener_for_reentrant = late_listener.clone();
        let reentrant_listener: Arc<dyn Fn(bool) + Send + Sync> = Arc::new(move |_enabled| {
            // SAFETY: see `BindingPtr`'s doc — invoked synchronously, inline,
            // on the same thread as (and within the lifetime of) `binding`.
            #[allow(unsafe_code)]
            unsafe { binding_ptr.get() }
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

        // Past the only two calls that could actually hang -- disarm the
        // watchdog now, before the assertions below, so an ordinary
        // assertion failure (a fast, expected test failure) is never
        // mistaken for the slow hang it exists to catch.
        completed.store(true, Ordering::SeqCst);

        binding.remove_semantics_enabled_listener(&late_listener);
        binding.remove_semantics_enabled_listener(&reentrant_listener);

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
