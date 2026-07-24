//! # `flui_binding`
//!
//! A deterministic, **non-singleton** headless frame driver for FLUI.
//!
//! [`HeadlessBinding`] owns a virtual [`ManualClock`] and a clock-bound
//! [`GestureArena`], and advances time one frame at a time via
//! [`HeadlessBinding::pump_frame`]. It is the FLUI-native equivalent of Flutter's
//! `TestWidgetsFlutterBinding.pump(dt)`: every deadline-driven gesture (long-press,
//! and the press-delay of double-tap) is driven off a single virtual timeline, so
//! tests are deterministic with **no wall-clock `thread::sleep`**.
//!
//! Unlike Flutter's `WidgetsFlutterBinding` (and FLUI's `RenderingFlutterBinding`),
//! this binding is an ordinary instantiable value, not a process global — many can
//! exist at once, so test suites run in parallel without contending on shared
//! singleton state.
//!
//! ## Scope — full frame driver
//!
//! [`pump_frame`](HeadlessBinding::pump_frame) advances the virtual clock, fires
//! due gesture deadlines, ticks registered animation controllers, then (when the
//! binding is tree-bound) rebuilds the element tree and runs the render pipeline
//! frame. The order is load-bearing — everything that can dirty the tree runs
//! before the rebuild — and mirrors Flutter's `TestWidgetsFlutterBinding.pump`.
//!
//! A binding has two flavors, sharing one `pump_frame`:
//!
//! - **Gesture-only** ([`new`](HeadlessBinding::new)): clock + arena, no tree.
//!   `pump_frame` advances time, polls deadlines, and ticks any registered
//!   controller — useful for driving a bare controller (or a deadline recognizer)
//!   deterministically with no `ElementTree` in play.
//! - **Tree-bound** ([`with_tree`](HeadlessBinding::with_tree)): additionally owns
//!   an already-mounted `BuildOwner` + `ElementTree` + shared
//!   `Arc<RwLock<PipelineOwner>>`, so `pump_frame` also drains the build inbox
//!   (`BuildOwner::build_scope`) and lays out / paints / composites
//!   (`PipelineOwner::run_frame`). The binding does **not** mount or root the
//!   tree — that bootstrap (root discovery, `set_root_constraints`) is
//!   embedder/harness policy; `with_tree` receives owners already mounted, rooted,
//!   and laid out.
//!
//! ### Restart-aware controllers
//!
//! A registered [`AnimationController`] is
//! ticked via `tick_at(seconds_since_this_run_started)`. Because a controller
//! re-zeros its run epoch on every fresh `forward()`/`reverse()`/…, the binding
//! watches the controller's
//! [`run_generation`](flui_animation::AnimationController::run_generation) and
//! re-anchors its per-run `t = 0` whenever a new run begins — so a controller that
//! runs twice (forward to completion, then reverse) ticks the second run from its
//! own start instead of snapping to the target on the first frame.
//!
//! ## Example
//!
//! ```
//! use std::sync::Arc;
//! use std::sync::atomic::{AtomicBool, Ordering};
//! use std::time::Duration;
//!
//! use flui_binding::HeadlessBinding;
//! use flui_interaction::settings::GestureSettings;
//! use flui_interaction::{GestureRecognizer, LongPressGestureRecognizer, PointerId};
//! use flui_types::Offset;
//! use flui_types::geometry::px;
//!
//! let mut binding = HeadlessBinding::new();
//!
//! let fired = Arc::new(AtomicBool::new(false));
//! let in_callback = Arc::clone(&fired);
//! let recognizer = LongPressGestureRecognizer::with_settings(
//!     binding.arena().clone(),
//!     GestureSettings::touch_defaults().with_long_press_timeout(Duration::from_millis(500)),
//! )
//! .with_on_long_press_start(move |_details| in_callback.store(true, Ordering::SeqCst));
//!
//! recognizer.add_pointer(PointerId::new(1).unwrap(), Offset::new(px(10.0), px(10.0)));
//!
//! // 300ms of virtual time — the 500ms deadline has not elapsed.
//! binding.pump_frame(Duration::from_millis(300));
//! assert!(!fired.load(Ordering::SeqCst));
//!
//! // Crossing 500ms fires the deadline inside the frame, deterministically.
//! binding.pump_frame(Duration::from_millis(300));
//! assert!(fired.load(Ordering::SeqCst));
//! ```

// Ship bar (wave 3): every public item is documented; keep it that way.
#![deny(missing_docs)]

use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::sync::Arc;
use std::time::Duration;

use flui_animation::{AnimationController, Vsync};
use flui_interaction::ManualClock;
use flui_interaction::arena::GestureArena;
use flui_interaction::routing::MouseTracker;
use flui_interaction::{
    GestureBinding, HitTestResult, InteractionDispatchError, InteractionDispatchHandle,
    InteractionLane, PointerEvent,
};
use flui_rendering::pipeline::PipelineOwner;
use flui_scheduler::{BoxedTask, LocalPostFrameLane, Scheduler, TaskToken};
use flui_types::geometry::{Offset, Pixels};
use flui_view::{BuildOwner, ElementId, ElementTree, View};
use parking_lot::RwLock;

fn preserve_first_pointer_panic(
    first: &mut Option<Box<dyn std::any::Any + Send>>,
    candidate: Option<Box<dyn std::any::Any + Send>>,
    phase: &'static str,
) {
    let Some(candidate) = candidate else {
        return;
    };
    if first.is_none() {
        *first = Some(candidate);
    } else {
        tracing::error!(
            phase,
            "pointer phase panicked after an earlier phase; only the first panic is resumed"
        );
        // Panic payloads are arbitrary user values. A secondary payload with a
        // panicking destructor must not replace the first failure or abort its
        // unwind, so this exceptional value is deliberately leaked.
        std::mem::forget(candidate);
    }
}

/// The mounted tree triple a tree-bound [`HeadlessBinding`] drives each frame.
///
/// `build_owner`'s dirty heap + external inbox feed `build_scope`; `tree` is the
/// element tree it rebuilds; `pipeline_owner` is the **shared** render owner the
/// frame lays out / paints / composites. The owner is shared (the element tree
/// holds an `Arc` clone for render-object attachment), so the per-frame step
/// takes it out under the write lock, runs the frame by value, and restores it —
/// mirroring the production frame path.
#[derive(Debug)]
struct TreeBinding {
    build_owner: BuildOwner,
    tree: ElementTree,
    pipeline_owner: Arc<RwLock<PipelineOwner>>,
}

/// A deterministic, non-singleton headless frame driver.
///
/// Owns the single virtual time authority ([`ManualClock`]) and one complete
/// clock-bound [`GestureBinding`]; optionally also owns a mounted tree triple
/// (via [`with_tree`](Self::with_tree)) and drives a restart-aware
/// animation-controller registry ([`Vsync`]). Drive it with
/// [`pump_frame`](Self::pump_frame).
///
/// # Thread ownership
///
/// A `HeadlessBinding` must be created, used, and dropped on one owner thread.
/// It is intentionally `!Send + !Sync`: owner-local post-frame callbacks may
/// capture `Rc`/`Cell`/`RefCell`. Frame, input, and tree-update entry points
/// activate the binding's local callback lane for their full dynamic extent;
/// embedders performing lifecycle work through raw owner access must wrap it in
/// [`enter_owner_scope`](Self::enter_owner_scope). Cross-thread test work must
/// communicate through the existing Send-safe scheduler capabilities, never
/// move or share the binding itself.
#[derive(Debug)]
pub struct HeadlessBinding {
    /// The single virtual time authority. Every time-based read flows from here.
    clock: ManualClock,
    /// The canonical input owner. Its arena, pointer routes, coalescing queues,
    /// resamplers, and mouse tracker all observe this binding's virtual clock
    /// and owner lane.
    gestures: GestureBinding,
    /// The controller registry ticked each frame on the virtual timeline,
    /// restart-aware. Shared (`Arc`-backed): a `VsyncScope` hands the same
    /// registry to a widget subtree so an implicitly-animated widget registers
    /// its controller here. See [`vsync`](Self::vsync) / [`adopt_vsync`](Self::adopt_vsync).
    vsync: Vsync,
    /// The mounted tree this binding rebuilds + renders each frame. `None` for a
    /// gesture-only binding ([`new`](Self::new)); `Some` once tree-bound.
    tree: Option<TreeBinding>,
    /// Owns the frame-driven async task driver. Binding-local, not
    /// the `Scheduler::instance()` singleton, so headless tests stay isolated and
    /// parallel-safe; the *driver step* is the same `drive_async_tasks` method
    /// `AppBinding::draw_frame` calls.
    scheduler: Scheduler,
    /// Owner-affine post-frame callback storage, active across every owner entry.
    local_post_frame: LocalPostFrameLane,
    /// Owner-affine interaction callback storage, active across every owner entry.
    interaction_lane: InteractionLane,
}

impl HeadlessBinding {
    /// Create a headless binding with a fresh virtual clock and a clock-bound,
    /// binding-owned input pipeline.
    ///
    /// `GestureBinding::with_clock` makes its arena and recognizers observe the
    /// same virtual timeline. The headless runtime therefore exercises the
    /// production routing/coalescing/mouse-tracking owner instead of maintaining
    /// a harness-only arena lifecycle.
    #[must_use]
    pub fn new() -> Self {
        Self::try_new().expect("BUG: interaction lane identity exhausted")
    }

    /// Try to create a headless binding with a fresh owner-local interaction lane.
    ///
    /// # Errors
    ///
    /// Returns [`InteractionDispatchError::IdentifierExhausted`] if the private
    /// interaction lane identity space has no unused value remaining.
    pub fn try_new() -> Result<Self, InteractionDispatchError> {
        let clock = ManualClock::new();
        let gestures = GestureBinding::with_clock(Arc::new(clock.clone()));
        let scheduler = Scheduler::new();
        let local_post_frame = scheduler.local_post_frame_lane();
        let interaction_lane = InteractionLane::try_new()?;
        Ok(Self {
            clock,
            gestures,
            vsync: Vsync::new(),
            tree: None,
            scheduler,
            local_post_frame,
            interaction_lane,
        })
    }

    /// Install this binding's build-time capabilities on `build_owner`.
    ///
    /// The **one** place a headless caller wires the two capabilities a view can
    /// acquire from its `BuildContext`, both naming *this* binding's scheduler:
    /// the async driver and the post-frame handle.
    ///
    /// Must run **before** the root is mounted: a `ViewState::init_state` during
    /// that first `build_scope` already asks for them. `bind_tree` re-installs for
    /// owners bound afterwards.
    ///
    /// Naming the `Scheduler::instance()` singleton here would leave every headless
    /// post-frame callback undrained — nothing drives the singleton's frames in a
    /// headless process.
    pub fn install_build_capabilities(&self, build_owner: &mut flui_view::BuildOwner) {
        build_owner.set_async_driver(self.scheduler.async_driver().clone());
        build_owner.set_post_frame_handle(self.local_post_frame.post_frame_handle());
        build_owner.set_interaction_dispatch_handle(self.interaction_dispatch_handle());
    }

    /// Enter this binding's owner scope for initial mount/build lifecycle work.
    ///
    /// Harnesses call this around the first `mount_root` + `build_scope`, so a
    /// lifecycle callback receives the same active local post-frame lane as it
    /// does during [`pump_frame`](Self::pump_frame).
    pub fn enter_owner_scope<R>(&self, callback: impl FnOnce() -> R) -> R {
        self.local_post_frame
            .enter(|| self.interaction_lane.enter(callback))
    }

    /// The Send-safe interaction dispatch handle for this binding's owner lane.
    #[must_use]
    pub fn interaction_dispatch_handle(&self) -> InteractionDispatchHandle {
        self.interaction_lane.dispatch_handle()
    }

    /// The binding's scheduler, which owns the frame-driven async task driver.
    ///
    /// Binding-local: two `HeadlessBinding`s never share a task set, so async
    /// tests stay parallel-safe.
    #[must_use]
    pub fn scheduler(&self) -> &Scheduler {
        &self.scheduler
    }

    /// Queue `future` for polling in this binding's next
    /// [`pump_frame`](Self::pump_frame).
    ///
    /// The headless test helper: spawn a future (or a channel
    /// receiver a test completes between frames), pump, and observe that the
    /// frame saw it. Dropping the returned token cancels the task.
    #[must_use = "dropping the TaskToken immediately cancels the task"]
    pub fn spawn_local(&self, future: BoxedTask) -> TaskToken {
        self.scheduler.spawn_local(future)
    }

    /// Create a tree-bound binding from already-bootstrapped owners.
    ///
    /// The binding takes ownership of `build_owner` and `tree` and shares
    /// `pipeline_owner` (the element tree holds an `Arc` clone for render-object
    /// attachment). The three must already be **mounted, rooted, and laid out** —
    /// `with_tree` does no bootstrap (root discovery, `set_root_constraints` are
    /// embedder/harness policy). From here, [`pump_frame`](Self::pump_frame)
    /// drives the full per-frame loop: deadlines → controllers → `build_scope` →
    /// `run_frame`.
    ///
    /// The clock and arena are fresh (as in [`new`](Self::new)); gesture and
    /// controller registration work identically on a tree-bound binding.
    #[must_use]
    pub fn with_tree(
        build_owner: BuildOwner,
        tree: ElementTree,
        pipeline_owner: Arc<RwLock<PipelineOwner>>,
    ) -> Self {
        let mut binding = Self::new();
        binding.bind_tree(build_owner, tree, pipeline_owner);
        binding
    }

    /// Attach an already-bootstrapped tree to this binding.
    ///
    /// Use this (rather than [`with_tree`](Self::with_tree)) when the tree must be
    /// mounted *before* it is attached — a `FutureBuilder`/`StreamBuilder`
    /// subscribes in `init_state`, which runs during the mount `build_scope`, so
    /// the build capabilities and their owner-local lane have to be installed
    /// and active before mounting:
    ///
    /// ```rust,ignore
    /// let mut binding = HeadlessBinding::new();
    /// binding.install_build_capabilities(&mut build_owner);
    /// binding.enter_owner_scope(|| {
    ///     // …mount + build_scope…
    /// });
    /// binding.bind_tree(build_owner, tree, pipeline_owner);
    /// ```
    pub fn bind_tree(
        &mut self,
        build_owner: BuildOwner,
        tree: ElementTree,
        pipeline_owner: Arc<RwLock<PipelineOwner>>,
    ) {
        // Widgets spawn into the driver this binding's frame step
        // polls — the binding-local one, never `Scheduler::instance()`. Idempotent:
        // installing it again is a no-op if the caller already did.
        let mut build_owner = build_owner;
        build_owner.set_async_driver(self.scheduler.async_driver().clone());
        // The post-frame capability must name THIS binding's
        // scheduler — the one `pump_frame`'s `drive_frame` drains — never the
        // `Scheduler::instance()` singleton, which nothing drives headlessly.
        build_owner.set_post_frame_handle(self.local_post_frame.post_frame_handle());
        build_owner.set_interaction_dispatch_handle(self.interaction_dispatch_handle());
        self.tree = Some(TreeBinding {
            build_owner,
            tree,
            pipeline_owner,
        });
    }

    /// Register `controller` with this binding's [`Vsync`] so each
    /// [`pump_frame`](Self::pump_frame) advances it on the virtual timeline.
    ///
    /// The controller is `Clone` (`Arc`-backed); register a clone and keep your
    /// own handle to drive it (`forward()`, `reverse()`, …). The registry is
    /// restart-aware: it re-anchors a controller's run on every fresh
    /// `forward`/`reverse`, so a controller run multiple times stays in sync
    /// without any binding-side run lifecycle. Convenience for a test that owns
    /// the controller directly; an implicitly-animated widget instead registers
    /// through a `VsyncScope` over [`vsync`](Self::vsync).
    pub fn register_controller(&mut self, controller: AnimationController) {
        self.vsync.register(controller);
    }

    /// The controller registry this binding ticks each frame.
    ///
    /// Wrap a widget subtree in a `VsyncScope` over `binding.vsync().clone()`
    /// (in `flui-widgets`) so every implicitly-animated widget below registers
    /// its controller here and is driven by `pump_frame`. `flui-binding` cannot
    /// host that scope itself — it has no `flui-widgets` dependency — so the
    /// wiring lives one layer up, exactly as the gesture arena does.
    #[must_use]
    pub fn vsync(&self) -> &Vsync {
        &self.vsync
    }

    /// Replace this binding's registry with a pre-existing shared `Vsync`.
    ///
    /// Use when a `VsyncScope` was placed in the tree *before* the binding was
    /// built (the scope needs the registry handle to hand to descendants, and
    /// the binding must drive that same registry). Call before any controller is
    /// registered, so no registration is stranded on the discarded registry.
    pub fn adopt_vsync(&mut self, vsync: Vsync) {
        self.vsync = vsync;
    }

    /// Mutable access to the bound `BuildOwner`, for an embedder/harness that
    /// schedules a specific element's rebuild (e.g. a root `setState`) before
    /// calling [`pump_frame`](Self::pump_frame).
    ///
    /// # Panics
    ///
    /// Panics if the binding is not tree-bound (constructed via
    /// [`new`](Self::new) rather than [`with_tree`](Self::with_tree)).
    pub fn build_owner_mut(&mut self) -> &mut BuildOwner {
        &mut self
            .tree
            .as_mut()
            .expect("build_owner_mut requires a tree-bound binding (built via with_tree)")
            .build_owner
    }

    /// Mutable access to the bound `ElementTree`, for an embedder/harness that
    /// marks a specific element dirty before calling
    /// [`pump_frame`](Self::pump_frame).
    ///
    /// # Panics
    ///
    /// Panics if the binding is not tree-bound (see
    /// [`build_owner_mut`](Self::build_owner_mut)).
    pub fn tree_mut(&mut self) -> &mut ElementTree {
        &mut self
            .tree
            .as_mut()
            .expect("tree_mut requires a tree-bound binding (built via with_tree)")
            .tree
    }

    /// The shared, clock-bound gesture arena.
    ///
    /// Add a deadline-driven recognizer to the same virtual timeline the frame
    /// driver polls by constructing it against `binding.arena().clone()` (the
    /// arena's entries are `Arc`-backed, so the clone shares them).
    ///
    /// To hand this arena to a whole widget subtree, wrap it in a
    /// `GestureArenaScope` (in `flui-widgets`): every `GestureDetector` below
    /// reads the scope's arena ambiently and competes in / is polled against it.
    /// `flui-binding` cannot host that scope itself — it has no `flui-view`
    /// dependency — so the wiring lives one layer up.
    #[must_use]
    pub fn arena(&self) -> &GestureArena {
        self.gestures.arena()
    }

    /// The complete owner-local input pipeline driven by this binding.
    #[must_use]
    pub fn gestures(&self) -> &GestureBinding {
        &self.gestures
    }

    /// Per-device mouse-region state owned by this binding's input pipeline.
    #[must_use]
    pub fn mouse_tracker(&self) -> &MouseTracker {
        self.gestures.mouse_tracker()
    }

    /// The virtual clock this binding advances each frame.
    ///
    /// Exposed for inspection (`now()` / `elapsed()`). Prefer
    /// [`pump_frame`](Self::pump_frame) to move time forward, so the per-frame
    /// ordering below is honored.
    #[must_use]
    pub fn clock(&self) -> &ManualClock {
        &self.clock
    }

    /// Route a pointer event through the complete binding-owned input pipeline.
    ///
    /// `hit_test` returns the canonical data-only path at the requested
    /// position. `GestureBinding` owns Down-route capture, contact routing,
    /// move coalescing, arena close/sweep, resampling, and mouse tracking; this
    /// headless binding does not reproduce any of those protocols.
    ///
    /// For deterministic test ergonomics, a queued move is flushed at the end
    /// of this input transaction. Production leaves that queue for the next
    /// frame; both paths execute the same canonical queue and dispatch code.
    ///
    /// A routing panic is resumed only after deferred arena resolution has had
    /// its required event-boundary chance to run. If both panic, routing wins
    /// deterministically and the later panic is traced.
    pub fn dispatch_pointer(
        &self,
        event: &PointerEvent,
        hit_test: impl FnOnce(Offset<Pixels>) -> HitTestResult,
    ) {
        self.local_post_frame.enter(|| {
            self.interaction_lane.enter(|| {
                let route_panic = catch_unwind(AssertUnwindSafe(|| {
                    self.gestures.handle_pointer_event(event, hit_test);
                    self.gestures.flush_pending_moves();
                }))
                .err();
                let deferred_panic = catch_unwind(AssertUnwindSafe(|| {
                    self.gestures.drain_deferred_arena_resolutions();
                }))
                .err();

                let mut first_panic = None;
                preserve_first_pointer_panic(&mut first_panic, route_panic, "pointer routing");
                preserve_first_pointer_panic(
                    &mut first_panic,
                    deferred_panic,
                    "deferred arena resolution",
                );
                if let Some(payload) = first_panic {
                    resume_unwind(payload);
                }
            });
        });
    }

    /// Replace the element rooted at `root_id` with `new_root` and schedule it
    /// for rebuild.
    ///
    /// Calls [`ElementTree::update`] using a split borrow over the owned
    /// internal tree-binding struct — `build_owner` and `tree` are separate
    /// fields so the compiler accepts both borrows simultaneously — then pushes
    /// `root_id` onto the dirty heap via `ElementOwner::schedule_build_for` so
    /// the next [`pump_frame`](Self::pump_frame) picks it up.
    ///
    /// This is the headless equivalent of Flutter's `WidgetTester.pumpWidget`
    /// (second call / root swap): replace the mounted root widget's configuration
    /// without tearing down and re-mounting the full tree.
    ///
    /// # Panics
    ///
    /// Panics if the binding is not tree-bound (built via
    /// [`with_tree`](Self::with_tree)).
    pub fn swap_root_view(&mut self, root_id: ElementId, new_root: &dyn View) {
        let Self {
            tree,
            local_post_frame,
            interaction_lane,
            ..
        } = self;
        local_post_frame.enter(|| interaction_lane.enter(|| {
            let Some(tree_binding) = tree.as_mut() else {
                panic!(
                    "swap_root_view requires a tree-bound binding (built via HeadlessBinding::with_tree)"
                );
            };
            // Split borrow: `build_owner` and `tree` are distinct fields of
            // `TreeBinding`, so the borrow checker accepts simultaneous borrows of
            // each through the single `&mut TreeBinding`.
            let mut owner = tree_binding.build_owner.element_owner_mut();
            tree_binding.tree.update(root_id, new_root, &mut owner);
            // Guarantee the element is in the dirty heap even if `dispatch_view_update`
            // only set the internal atomic flag (not the owner's dirty heap).
            owner.schedule_build_for(root_id, 0, flui_view::RebuildReason::RootChange);
        }));
    }

    /// Advance one deterministic frame by `dt`.
    ///
    /// # Ordering
    ///
    /// The steps mirror Flutter's `TestWidgetsFlutterBinding.pump(dt)`
    /// (`fakeAsync.elapse(dt)` → `handleBeginFrame` → `handleDrawFrame`), and the
    /// order is load-bearing:
    ///
    /// 1. **Advance the virtual clock.** Everything time-based reads from here, so
    ///    the new instant must be visible before anything observes it — the
    ///    analogue of `fakeAsync.elapse(dt)`.
    /// 2. **Drain deferred arena defaults.** This is the frame-boundary fallback
    ///    for a lone member queued at a previous event boundary that unwound.
    /// 3. **Fire gesture deadlines** at the new time. Flutter fires due `Timer`s
    ///    inside `elapse`, *ahead* of `handleBeginFrame`; a deadline (e.g. a
    ///    long-press) that has now elapsed resolves here, before any later frame
    ///    work — so the deadline poll is the first thing after the clock moves.
    ///
    /// 4. **Tick registered animation controllers** on the virtual timeline. A
    ///    controller's `tick_at` notifies its listeners, which mark the dependent
    ///    `AnimatedView` dirty into the `BuildOwner`'s external inbox.
    /// 5. **Rebuild the tree** (tree-bound only): `BuildOwner::build_scope` drains
    ///    that inbox at its start and reconciles.
    /// 6. **Run the pipeline frame** (tree-bound only): `PipelineOwner::run_frame`
    ///    lays out, paints, and composites.
    ///
    /// # The load-bearing invariant
    ///
    /// **Everything that can dirty the tree runs before `build_scope`.** A gesture
    /// deferred/default or deadline callback (steps 2–3) may `setState` or
    /// start a controller; a controller tick (step 4) routes through
    /// `notify_listeners` → the
    /// `AnimatedView`'s mark-dirty callback → the `BuildOwner`'s external inbox,
    /// which `build_scope` (step 5) drains at its very start. If step 4 ran *after*
    /// step 5, a tick's inbox entry would miss this frame's drain and rebuild only
    /// next frame — a one-frame animation lag. The order is what makes an
    /// animation visible **same-frame**.
    ///
    /// Steps 5–6 run only when the binding is tree-bound
    /// ([`with_tree`](Self::with_tree)); a gesture-only binding stops after step 4,
    /// so a bare controller can still be driven deterministically.
    pub fn pump_frame(&mut self, dt: Duration) {
        let Self {
            clock,
            gestures,
            vsync,
            tree,
            scheduler,
            local_post_frame,
            interaction_lane,
        } = self;
        local_post_frame.enter(|| {
            interaction_lane.enter(|| {
                // 1. Advance the virtual clock. Every subsequent read sees the new instant.
                clock.advance(dt);

                // 2. Settle a lone default winner left by an earlier event boundary.
                gestures.drain_deferred_arena_resolutions();

                // 3. Dispatch the frame-coalesced pointer batch, then fire
                //    gesture deadlines at the NEW time. A long-press deadline that has
                //    now elapsed fires here, inside the frame.
                gestures.flush_pending_moves();
                gestures.tick_deadlines();

                // 4. Tick the registered controllers on the virtual timeline. The
                //    registry is restart-aware: it re-anchors each controller's run on a
                //    `run_generation` bump and ticks only running controllers with the
                //    raw seconds elapsed since that run's anchor.
                let now_secs = clock.elapsed().as_secs_f64();
                vsync.tick_all(now_secs);

                // 5-8. THE shared frame ordering:
                //
                //      begin (transient + microtasks + ONE async-driver poll)
                //   -> handle_draw_frame (persistent callbacks)
                //   -> the pipeline, below, in the persistent slot
                //   -> end_frame (post-frame callbacks, timing, notify)
                //   -> Idle
                //
                // The desktop / android / wasm runners call the SAME `Scheduler::drive_frame`
                // on the `Scheduler::instance()` singleton; this binding calls it on its
                // binding-local scheduler. A post-frame callback therefore observes THIS
                // frame's committed layout in both, which is what `HeroController` needs.
                //
                // `drive_async_tasks` is no longer called here: the scheduler owns that
                // step now. It still runs before `build_scope`, in
                // `handle_begin_frame`'s mid-frame slot.
                //
                // `Scheduler` is `Arc`-backed and `Clone`, so the handle taken here shares
                // the callback queues with `self.scheduler` — cloning it merely releases
                // the borrow on `self` for the pipeline closure.
                let scheduler = scheduler.clone();
                let vsync_time = flui_scheduler::Instant::now();
                scheduler.drive_frame(vsync_time, || Self::run_pipeline(tree));
            });
        });
    }

    /// The pipeline step: build → layout (with the build-during-layout fixpoint)
    /// → paint, plus the lazy-sliver service pass. Runs inside
    /// [`Scheduler::drive_frame`]'s persistent slot.
    fn run_pipeline(tree: &mut Option<TreeBinding>) {
        let Some(tree_binding) = tree.as_mut() else {
            return;
        };

        // Drain the build inbox, filled by the vsync tick and the async-driver
        // poll that ran before this closure.
        tree_binding.build_owner.build_scope(&mut tree_binding.tree);

        // `run_frame_with_layout_builders` is the shared
        // layout<->build fixpoint — it settles every build-during-layout node
        // before paint, then delegates to `PipelineOwner::run_frame`. It is a
        // plain `run_frame` while the registry is empty. `AppBinding::draw_frame`
        // calls the SAME helper: a builder that settles headlessly but not on
        // screen would be a silent correctness bug, so neither path may
        // hand-roll the loop.
        //
        // The owner is threaded by lock, not by value: the helper takes it out
        // per layout pass and restores it before running the builders, whose
        // `build_scope` mounts render objects through this same lock.
        let result = tree_binding
            .build_owner
            .run_frame_with_layout_builders(&mut tree_binding.tree, &tree_binding.pipeline_owner);
        // A headless frame over an already-mounted, rooted tree must succeed;
        // a pipeline error here is a regression, surfaced loudly (the harness
        // and production frame path expect the same).
        result.expect("headless pump_frame: pipeline run_frame should succeed");

        // Service lazy-sliver child requests. Layout may have emitted build
        // requests for absent children and retain-band signals for eviction.
        // Drain both buffers, call each registered ChildManager to build/evict,
        // run a second build_scope for newly-built child subtrees, mark slivers
        // needing re-layout, and finalize evicted elements. This is a no-op when
        // no lazy slivers are mounted.
        tree_binding
            .build_owner
            .service_child_requests(&mut tree_binding.tree, &tree_binding.pipeline_owner);
    }
}

impl Default for HeadlessBinding {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod auto_trait_tests {
    use static_assertions::assert_not_impl_any;

    use super::HeadlessBinding;

    assert_not_impl_any!(HeadlessBinding: Send, Sync);
}
