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

use std::sync::Arc;
use std::time::Duration;

use flui_animation::{AnimationController, Vsync};
use flui_interaction::PointerEvent;
use flui_interaction::arena::{GestureArena, run_pointer_lifecycle};
use flui_interaction::{ManualClock, MonotonicClock};
use flui_rendering::pipeline::PipelineOwner;
use flui_view::{BuildOwner, ElementId, ElementTree, View};
use parking_lot::RwLock;

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
/// Owns the single virtual time authority ([`ManualClock`]) and a clock-bound
/// [`GestureArena`] whose deadline checks read that clock; optionally also owns a
/// mounted tree triple (via [`with_tree`](Self::with_tree)) and drives a
/// restart-aware animation-controller registry ([`Vsync`]). Drive it with
/// [`pump_frame`](Self::pump_frame).
#[derive(Debug)]
pub struct HeadlessBinding {
    /// The single virtual time authority. Every time-based read flows from here.
    clock: ManualClock,
    /// The shared, clock-bound arena. Deadline-driven recognizers added to it (via
    /// [`arena`](Self::arena)) resolve against the virtual clock.
    arena: GestureArena,
    /// The controller registry ticked each frame on the virtual timeline,
    /// restart-aware. Shared (`Arc`-backed): a `VsyncScope` hands the same
    /// registry to a widget subtree so an implicitly-animated widget registers
    /// its controller here. See [`vsync`](Self::vsync) / [`adopt_vsync`](Self::adopt_vsync).
    vsync: Vsync,
    /// The mounted tree this binding rebuilds + renders each frame. `None` for a
    /// gesture-only binding ([`new`](Self::new)); `Some` once tree-bound.
    tree: Option<TreeBinding>,
}

impl HeadlessBinding {
    /// Create a headless binding with a fresh virtual clock and a clock-bound,
    /// binding-owned gesture arena.
    ///
    /// The arena is built via
    /// `GestureArena::binding_driven(Arc::new(clock.clone()))`, so the arena and
    /// the binding observe the *same* virtual timeline (the clock's elapsed
    /// counter is `Arc`-backed and shared across clones) AND the recognizers
    /// below never self-sweep — this binding runs the close/sweep lifecycle in
    /// [`dispatch_pointer`](Self::dispatch_pointer).
    #[must_use]
    pub fn new() -> Self {
        let clock = ManualClock::new();
        let arena =
            GestureArena::binding_driven(Arc::new(clock.clone()) as Arc<dyn MonotonicClock>);
        Self {
            clock,
            arena,
            vsync: Vsync::new(),
            tree: None,
        }
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
        binding.tree = Some(TreeBinding {
            build_owner,
            tree,
            pipeline_owner,
        });
        binding
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
        &self.arena
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

    /// Route a pointer event to the hit-test path, then run the arena's
    /// close/sweep lifecycle — Flutter's `GestureBinding.handleEvent` order.
    ///
    /// `route` delivers the event to the framework (hit-test + dispatch, which
    /// drives every hit `Listener`'s `add_pointer` / `handle_event`); the closure
    /// keeps this binding rendering-agnostic, since `flui-binding` cannot name
    /// `HitTestResult`. The route runs **first**, then the arena is closed on
    /// `Down` and swept on `Up` / `Cancel`. The route-before-sweep order is
    /// load-bearing: it lets a double-tap's first-up `hold` run before the sweep,
    /// so the sweep observes the hold and defers — and lets every overlapping
    /// detector add its recognizers before the single `close`.
    pub fn dispatch_pointer(&self, event: &PointerEvent, route: impl FnOnce(&PointerEvent)) {
        route(event);
        run_pointer_lifecycle(&self.arena, event);
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
        let Some(tree_binding) = self.tree.as_mut() else {
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
        owner.schedule_build_for(root_id, 0);
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
    /// 2. **Fire gesture deadlines** at the new time. Flutter fires due `Timer`s
    ///    inside `elapse`, *ahead* of `handleBeginFrame`; a deadline (e.g. a
    ///    long-press) that has now elapsed resolves here, before any later frame
    ///    work — so the deadline poll is the first thing after the clock moves.
    ///
    /// 3. **Tick registered animation controllers** on the virtual timeline. A
    ///    controller's `tick_at` notifies its listeners, which mark the dependent
    ///    `AnimatedView` dirty into the `BuildOwner`'s external inbox.
    /// 4. **Rebuild the tree** (tree-bound only): `BuildOwner::build_scope` drains
    ///    that inbox at its start and reconciles.
    /// 5. **Run the pipeline frame** (tree-bound only): `PipelineOwner::run_frame`
    ///    lays out, paints, and composites.
    ///
    /// # The load-bearing invariant
    ///
    /// **Everything that can dirty the tree runs before `build_scope`.** A gesture
    /// deadline callback (step 2) may `setState` or start a controller; a
    /// controller tick (step 3) routes through `notify_listeners` → the
    /// `AnimatedView`'s mark-dirty callback → the `BuildOwner`'s external inbox,
    /// which `build_scope` (step 4) drains at its very start. If step 3 ran *after*
    /// step 4, a tick's inbox entry would miss this frame's drain and rebuild only
    /// next frame — a one-frame animation lag. The order is what makes an
    /// animation visible **same-frame**.
    ///
    /// Steps 4–5 run only when the binding is tree-bound
    /// ([`with_tree`](Self::with_tree)); a gesture-only binding stops after step 3,
    /// so a bare controller can still be driven deterministically.
    pub fn pump_frame(&mut self, dt: Duration) {
        // 1. Advance the virtual clock. Every subsequent read sees the new instant.
        self.clock.advance(dt);

        // 2. Fire gesture deadlines at the NEW time. A long-press deadline that has
        //    now elapsed fires here, inside the frame.
        self.arena.poll_deadlines();

        // 3. Tick the registered controllers on the virtual timeline. The
        //    registry is restart-aware: it re-anchors each controller's run on a
        //    `run_generation` bump and ticks only running controllers with the
        //    raw seconds elapsed since that run's anchor.
        let now_secs = self.clock.elapsed().as_secs_f64();
        self.vsync.tick_all(now_secs);

        // 4 + 5. Tree-bound only: drain the build inbox (filled by steps 2-3) and
        //        run the render pipeline frame. The pipeline owner is shared via
        //        `Arc<RwLock<…>>`, so take it out under the write lock, run the
        //        frame by value, and restore — the production frame path.
        if let Some(tree_binding) = self.tree.as_mut() {
            tree_binding.build_owner.build_scope(&mut tree_binding.tree);

            let mut guard = tree_binding.pipeline_owner.write();
            let owner = std::mem::take(&mut *guard);
            let (owner, result) = owner.run_frame();
            // A headless frame over an already-mounted, rooted tree must succeed;
            // a pipeline error here is a regression, surfaced loudly (the harness
            // and production frame path expect the same).
            result.expect("headless pump_frame: pipeline run_frame should succeed");
            *guard = owner;
            // Guard dropped here so the pipeline write-lock is free for the
            // service calls below.
            drop(guard);

            // 6. Service lazy-sliver child requests. Layout may have emitted
            //    build requests for absent children and retain-band signals for
            //    eviction. Drain both buffers, call each registered ChildManager
            //    to build/evict, run a second build_scope for newly-built child
            //    subtrees, mark slivers needing re-layout, and finalize evicted
            //    elements. This is a no-op when no lazy slivers are mounted.
            tree_binding
                .build_owner
                .service_child_requests(&mut tree_binding.tree, &tree_binding.pipeline_owner);
        }
    }
}

impl Default for HeadlessBinding {
    fn default() -> Self {
        Self::new()
    }
}
