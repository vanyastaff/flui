//! # `flui_testing`
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
//!   `PipelineCell`, so `pump_frame` also drains the build inbox
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
//! use flui_testing::HeadlessBinding;
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

pub mod a11y;
pub mod bootstrap;
pub mod fonts;
pub mod log_capture;
pub mod replay;

pub use a11y::{A11yNode, A11yQuery, A11yQueryError, A11yTree, NotTreeBound};
pub use bootstrap::{BuildCapabilities, MountOptions, MountOwners, Mounted};
pub use fonts::pin_font_faces;
pub use log_capture::{CapturedLog, CapturedRecord, capture, disarm_interest_cache};
pub use replay::{GestureRecorder, PointerPhase, PointerScript, ScriptedPointer};

use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::sync::Arc;
use std::time::Duration;

use flui_animation::{AnimationController, Vsync};
use flui_foundation::PresentationId;
use flui_interaction::ManualClock;
use flui_interaction::arena::GestureArena;
use flui_interaction::routing::MouseTracker;
use flui_interaction::{
    GestureBinding, HitTestResult, InteractionDispatchError, InteractionDispatchHandle,
    InteractionLane, PointerEvent,
};
// `flui-rendering` re-exports `flui-layer` wholesale, so naming the composited
// tree costs no extra dependency edge.
use flui_rendering::layer::LayerTree;
use flui_rendering::pipeline::PipelineCell;
use flui_scheduler::{
    BoxedTask, ClockSource, DemandKind, FrameClock, LocalPostFrameLane, TaskToken, UpdateScheduler,
};
use flui_types::geometry::{Offset, Pixels};
use flui_view::{BuildOwner, ElementId, ElementTree, View};

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
/// holds a [`PipelineCell`] clone for render-object attachment), so the
/// per-frame step checks it out via `with_mut`, runs the frame, and lets the
/// checkout end — mirroring the production frame path.
#[derive(Debug)]
struct TreeBinding {
    build_owner: BuildOwner,
    tree: ElementTree,
    pipeline_owner: PipelineCell,
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
    /// Owns the frame-driven async task driver. Binding-local — its own
    /// fresh `UpdateScheduler` value, never shared with any other binding — so
    /// headless tests stay isolated and parallel-safe; the *driver step* is
    /// the same `drive_async_tasks` method a production frame drive calls on
    /// its own, likewise dedicated, `UpdateScheduler`.
    scheduler: UpdateScheduler,
    /// Owner-affine post-frame callback storage, active across every owner entry.
    local_post_frame: LocalPostFrameLane,
    /// Owner-affine interaction callback storage, active across every owner entry.
    interaction_lane: InteractionLane,
    /// The most recently committed composited [`LayerTree`], retained across a
    /// later frame that has no paint work.
    ///
    /// On screen this value is the frame's whole point — the production draw-frame
    /// path hands it to the compositor. Headlessly there is nothing downstream to hand
    /// it to, so the pipeline's return value used to be discarded, leaving no way
    /// to ask what a frame actually composited. Keeping it makes the headless
    /// frame observable in the same terms as the on-screen one; see
    /// [`layer_tree`](Self::layer_tree).
    ///
    /// `None` before the first painted frame and for a gesture-only binding.
    last_layer_tree: Option<LayerTree>,
    /// Whether the immediately preceding frame produced a fresh layer tree.
    last_frame_painted: bool,
    /// Number of frames that produced a fresh layer tree.
    painted_frame_count: u64,
    /// The deterministic multi-presentation clock registry (issue #556)
    /// — additive to, and independent of, this binding's own single
    /// [`clock`](Self)/[`vsync`](Self::vsync) pair `pump_frame` drives.
    /// Empty until [`install_presentation_clock`](Self::install_presentation_clock)
    /// registers an id; see that method's own doc.
    presentation_clocks: HashMap<PresentationId, PresentationClockEntry>,
}

/// One presentation's own independent clock + controller registry, keyed by
/// [`PresentationId`] in [`HeadlessBinding::presentation_clocks`] — the piece
/// that makes [`HeadlessBinding::pump_presentation`]/[`pump_all`](HeadlessBinding::pump_all)
/// genuinely per-presentation rather than a second view onto the binding's
/// single default clock.
#[derive(Debug)]
struct PresentationClockEntry {
    /// This presentation's own implicit-animation controller registry.
    vsync: Vsync,
    /// This presentation's own produce-gate state machine, reading
    /// `virtual_clock` through a [`ClockSource::Manual`].
    clock: FrameClock,
    /// The same virtual clock `clock`'s [`ClockSource::Manual`] wraps, kept
    /// as its own handle so [`HeadlessBinding::pump_presentation`] can read
    /// [`ManualClock::elapsed`] directly (the seconds
    /// [`Vsync::tick_all`](flui_animation::Vsync::tick_all) wants) without
    /// reaching back through `clock`'s `ClockSource`.
    virtual_clock: ManualClock,
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
        let scheduler = UpdateScheduler::new();
        let local_post_frame = scheduler.new_local_post_frame_lane();
        let interaction_lane = InteractionLane::try_new()?;
        Ok(Self {
            clock,
            gestures,
            vsync: Vsync::new(),
            tree: None,
            scheduler,
            local_post_frame,
            interaction_lane,
            last_layer_tree: None,
            last_frame_painted: false,
            painted_frame_count: 0,
            presentation_clocks: HashMap::new(),
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
    /// Naming any OTHER binding's `UpdateScheduler` here — a production realm's, say —
    /// would leave every headless post-frame callback undrained: nothing in this
    /// process drives frames for a scheduler this binding does not itself own and
    /// pump.
    pub fn install_build_capabilities(&self, build_owner: &mut flui_view::BuildOwner) {
        build_owner.set_async_driver(self.scheduler.async_driver().clone());
        build_owner.set_post_frame_handle(flui_scheduler::PostFrameHandle::new(&self.scheduler));
        build_owner.set_local_post_frame_handle(self.local_post_frame.local_handle());
        build_owner.set_interaction_dispatch_handle(self.interaction_dispatch_handle());
    }

    /// Enter this binding's owner scope for initial mount/build lifecycle work.
    ///
    /// Harnesses call this around the first `mount_root` + `build_scope`, so a
    /// lifecycle callback runs with the same active interaction lane as it does
    /// during [`pump_frame`](Self::pump_frame). The local post-frame lane needs
    /// no activation: `LocalPostFrameHandle` addresses its lane directly (a
    /// `Weak` pointer, minted once by `install_build_capabilities`), so
    /// scheduling through it works regardless of what is "entered" here.
    pub fn enter_owner_scope<R>(&self, callback: impl FnOnce() -> R) -> R {
        self.interaction_lane.enter(callback)
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
    pub fn scheduler(&self) -> &UpdateScheduler {
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
        pipeline_owner: PipelineCell,
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
        pipeline_owner: PipelineCell,
    ) {
        self.bind_tree_with_committed_layer_tree(build_owner, tree, pipeline_owner, None);
    }

    /// Attach a bootstrapped tree and retain the layer tree produced by its
    /// bootstrap frame.
    pub fn bind_tree_with_committed_layer_tree(
        &mut self,
        build_owner: BuildOwner,
        tree: ElementTree,
        pipeline_owner: PipelineCell,
        committed_layer_tree: Option<LayerTree>,
    ) {
        self.bind_tree_with_capabilities(
            build_owner,
            tree,
            pipeline_owner,
            committed_layer_tree,
            bootstrap::BuildCapabilities::Installed,
        );
    }

    /// [`bind_tree_with_committed_layer_tree`](Self::bind_tree_with_committed_layer_tree)
    /// under an explicit capability policy.
    ///
    /// The public entry point installs the full set, which is right for a
    /// caller handing over owners it configured itself: it cannot know what the
    /// binding's own handles are, so the binding supplies them. A bootstrap
    /// that was asked to *withhold* a capability must not go through that door,
    /// or the withholding would last only until the bind and every rebuild
    /// after `init_state` would see a handle the caller believes is absent.
    pub(crate) fn bind_tree_with_capabilities(
        &mut self,
        build_owner: BuildOwner,
        tree: ElementTree,
        pipeline_owner: PipelineCell,
        committed_layer_tree: Option<LayerTree>,
        capabilities: bootstrap::BuildCapabilities,
    ) {
        // Widgets spawn into the driver this binding's frame step
        // polls — the binding-local one, never some OTHER binding's or
        // realm's `UpdateScheduler`. Idempotent: installing it again is a no-op if
        // the caller already did. The async driver goes in under either policy:
        // withholding it would change *which* capability is under test.
        let mut build_owner = build_owner;
        build_owner.set_async_driver(self.scheduler.async_driver().clone());
        if capabilities == bootstrap::BuildCapabilities::Installed {
            // The post-frame capability must name THIS binding's
            // scheduler — the one `pump_frame`'s `drive_frame` drains — never
            // some other binding's or realm's `UpdateScheduler`, which nothing drives
            // headlessly.
            build_owner
                .set_post_frame_handle(flui_scheduler::PostFrameHandle::new(&self.scheduler));
            build_owner.set_local_post_frame_handle(self.local_post_frame.local_handle());
            build_owner.set_interaction_dispatch_handle(self.interaction_dispatch_handle());
        }
        self.tree = Some(TreeBinding {
            build_owner,
            tree,
            pipeline_owner,
        });
        self.last_frame_painted = committed_layer_tree.is_some();
        self.last_layer_tree = committed_layer_tree;
        if self.last_frame_painted {
            self.painted_frame_count = self.painted_frame_count.saturating_add(1);
        }
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
    /// its controller here and is driven by `pump_frame`. `flui-testing` cannot
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

    /// The shared render owner this binding drives, when tree-bound.
    ///
    /// `None` for a gesture-only binding. The cell is shared with the element
    /// tree, so a caller reads committed geometry through it without taking
    /// the owner away from the frame loop.
    #[must_use]
    pub fn pipeline_owner(&self) -> Option<&PipelineCell> {
        self.tree.as_ref().map(|tree| &tree.pipeline_owner)
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
    /// `flui-testing` cannot host that scope itself — it has no `flui-view`
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

    /// The most recently committed composited layer tree.
    ///
    /// This is the headless counterpart of the value `UiRealm::draw_frame`
    /// hands the compositor — the same tree, from the same pipeline step, simply
    /// kept instead of dropped. It answers "what did this frame actually
    /// composite", which the render tree alone cannot: layers are created by
    /// *paint*, so a widget that forces a clip, a transform, or an opacity layer
    /// is visible here and nowhere else.
    ///
    /// A frame with no paint work preserves the previous committed tree. Use
    /// [`Self::did_paint_last_frame`] when the distinction matters.
    #[must_use]
    pub fn layer_tree(&self) -> Option<&LayerTree> {
        self.last_layer_tree.as_ref()
    }

    /// Whether the immediately preceding frame produced a fresh layer tree.
    #[must_use]
    pub fn did_paint_last_frame(&self) -> bool {
        self.last_frame_painted
    }

    /// Turns the semantics phase on, so subsequent frames assemble an
    /// accessibility tree.
    ///
    /// Semantics is off by default here for the same reason it is in
    /// production: assembly costs a tree walk that nothing should pay for until
    /// an assistive technology (or a test) asks. Call this **before** the
    /// [`pump_frame`](Self::pump_frame) whose tree you intend to query — the
    /// phase is a no-op while disabled, so enabling it afterwards leaves
    /// [`a11y_tree`](Self::a11y_tree) returning `None` until the next frame.
    ///
    /// Enabling lazily creates the `SemanticsOwner`, matching
    /// `PipelineOwner::set_semantics_enabled`.
    ///
    /// # Errors
    ///
    /// [`NotTreeBound`] if the binding drives no tree. This is a `Result`
    /// rather than a panic so the failure cannot be silently ignored: a no-op
    /// `enable_semantics` would surface far away, as
    /// [`a11y_tree`](Self::a11y_tree) returning `None`, and read as "this UI
    /// has no semantics" instead of "the call did nothing".
    pub fn enable_semantics(&mut self) -> Result<(), NotTreeBound> {
        self.tree
            .as_ref()
            .ok_or(NotTreeBound)?
            .pipeline_owner
            .with_mut(|owner| owner.set_semantics_enabled(true));
        Ok(())
    }

    /// Whether the semantics phase is assembling a tree.
    ///
    /// `false` for a binding with no tree bound, which has no pipeline to ask.
    #[must_use]
    pub fn semantics_enabled(&self) -> bool {
        self.tree.as_ref().is_some_and(|tree_binding| {
            tree_binding
                .pipeline_owner
                .with(flui_rendering::pipeline::PipelineOwner::semantics_enabled)
        })
    }

    /// The accessibility tree as an assistive technology would receive it.
    ///
    /// `None` until [`enable_semantics`](Self::enable_semantics) has been called
    /// *and* a frame has run — there is no tree to translate before then, and
    /// returning an empty one would let a query assert "no buttons" against a
    /// tree that was never built.
    ///
    /// The snapshot is translated by `flui_semantics::tree_to_update`, the same
    /// function a platform adapter calls, so an assertion here is an assertion
    /// about what a screen reader is told. See [`a11y`] for the
    /// query surface.
    #[must_use]
    pub fn a11y_tree(&self) -> Option<A11yTree> {
        let update = self.tree.as_ref()?.pipeline_owner.with(|owner| {
            owner
                .semantics_owner()
                .and_then(|semantics| semantics.to_accesskit_tree_update(None))
        })?;
        Some(A11yTree::new(update))
    }

    /// Number of frames that have produced a fresh layer tree.
    #[must_use]
    pub fn painted_frame_count(&self) -> u64 {
        self.painted_frame_count
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
            interaction_lane,
            ..
        } = self;
        interaction_lane.enter(|| {
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
        });
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
    /// 7. **Re-hit-test every stationary pointing device** (tree-bound only)
    ///    against the tree this frame just laid out — Flutter's implicit
    ///    `MouseTracker.updateAllDevices` postframe recheck
    ///    (`rendering/mouse_tracker.dart`, called from
    ///    `RendererBinding._handlePersistentFrameCallback`'s
    ///    `_scheduleMouseTrackerUpdate`, still inside the persistent phase,
    ///    ahead of the post-frame callback queue). This is what lets a
    ///    region that appears, moves, or disappears under a **motionless**
    ///    pointer emit enter/exit with no new pointer motion: the mechanism
    ///    production already wires
    ///    (`UiRealm::render_frame_entered`,
    ///    `crates/flui-app/src/app/ui_realm.rs`, driven from inside
    ///    the scheduler's frame closure —
    ///    `crates/flui-app/src/app/runner.rs`) right after layout/paint and
    ///    still inside that same closure, mirrored here against this
    ///    binding's own tree-bound `PipelineOwner` rather than a
    ///    caller-supplied hit-test closure — `pump_frame`'s tree-bound
    ///    branch already owns the same `PipelineCell`
    ///    production's `hit_test_in_view` wraps, so no new parameter is
    ///    needed on this already-widely-called method. Step 7 therefore
    ///    runs inside the same `drive_frame` pipeline closure as step 6,
    ///    not after it returns — see the inline comment at the call site
    ///    for why that placement is load-bearing, not cosmetic.
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
    /// Steps 5–7 run only when the binding is tree-bound
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
            last_layer_tree,
            last_frame_painted,
            painted_frame_count,
            ..
        } = self;
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
            // The desktop / android / wasm runners call the SAME `UpdateScheduler::drive_frame`
            // on the production realm's own owned scheduler; this binding calls it on
            // its binding-local scheduler. A post-frame callback therefore observes THIS
            // frame's committed layout in both, which is what `HeroController` needs.
            //
            // `drive_async_tasks` is no longer called here: the scheduler owns that
            // step now. It still runs before `build_scope`, in
            // `handle_begin_frame`'s mid-frame slot.
            //
            // `UpdateScheduler` is `Arc`-backed and `Clone`, so the handle taken here shares
            // the callback queues with `self.scheduler` — cloning it merely releases
            // the borrow on `self` for the pipeline closure.
            let scheduler = scheduler.clone();
            let vsync_time = flui_scheduler::Instant::now();
            // No `FrameClock` is wired into the headless driver (it
            // doesn't exist yet — see `UpdateScheduler::drive_frame`'s
            // doc); a deadline far in the future means Idle-priority
            // work is never deferred here, matching this binding's
            // behavior before `drive_frame` took a deadline.
            let idle_deadline = flui_scheduler::IdleDeadline::far_future(vsync_time);
            scheduler.drive_frame_with_lane(
                vsync_time,
                idle_deadline,
                || {
                    let painted_layer_tree = Self::run_pipeline(tree);
                    *last_frame_painted = painted_layer_tree.is_some();
                    if let Some(layer_tree) = painted_layer_tree {
                        *last_layer_tree = Some(layer_tree);
                        *painted_frame_count = painted_frame_count.saturating_add(1);
                    }

                    // 7. Re-hit-test every stationary device against the
                    //    tree layout/paint that just committed above,
                    //    still inside this closure's `PersistentCallbacks`
                    //    slot — i.e. BEFORE `end_frame` drains post-frame
                    //    callbacks below, not after `drive_frame` returns.
                    //    Placement matters: production
                    //    (`UiRealm::render_frame_entered`,
                    //    `crates/flui-app/src/app/ui_realm.rs`, invoked from
                    //    `crates/flui-app/src/app/runner.rs`) calls
                    //    `update_all_devices` from inside the SAME
                    //    `drive_frame` pipeline closure it runs its own
                    //    layout/paint step in, so any post-frame work an
                    //    enter/exit callback queues (e.g. a rebuild
                    //    handle) lands in THIS frame's post-frame phase —
                    //    matching the oracle, where
                    //    `_scheduleMouseTrackerUpdate` posts
                    //    `updateAllDevices` from
                    //    `_handlePersistentFrameCallback`, still inside
                    //    the persistent phase, ahead of the post-frame
                    //    queue. Running this after `drive_frame` returns
                    //    would defer that queued work to a LATER pump
                    //    instead. Unconditional and every frame; a
                    //    gesture-only binding has no tree to hit-test, so
                    //    this is a no-op there.
                    if let Some(tree_binding) = tree.as_ref() {
                        let pipeline_owner = &tree_binding.pipeline_owner;
                        gestures.mouse_tracker().update_all_devices(|position| {
                            let mut result = HitTestResult::new();
                            pipeline_owner.with(|owner| owner.hit_test(position, &mut result));
                            result
                        });
                    }
                },
                local_post_frame,
            );
        });
    }

    /// The pipeline step: build → layout (with the build-during-layout fixpoint)
    /// → paint, plus the lazy-sliver service pass. Runs inside
    /// [`UpdateScheduler::drive_frame`]'s persistent slot.
    ///
    /// Returns the composited [`LayerTree`] this frame produced — `None` for a
    /// gesture-only binding, and `None` when nothing was dirty enough to
    /// repaint. `pump_frame` stores it in
    /// [`last_layer_tree`](Self::last_layer_tree).
    fn run_pipeline(tree: &mut Option<TreeBinding>) -> Option<LayerTree> {
        let tree_binding = tree.as_mut()?;

        // Drain the build inbox, filled by the vsync tick and the async-driver
        // poll that ran before this closure.
        tree_binding.build_owner.build_scope(&mut tree_binding.tree);

        // `run_frame_with_layout_builders` is the shared
        // layout<->build fixpoint — it settles every build-during-layout node
        // before paint, then delegates to `PipelineOwner::run_frame`. It is a
        // plain `run_frame` while the registry is empty. `UiRealm::draw_frame`
        // calls the SAME helper: a builder that settles headlessly but not on
        // screen would be a silent correctness bug, so neither path may
        // hand-roll the loop.
        //
        // The owner is threaded by cell, not by value: the helper checks it
        // out per layout pass and lets the checkout end before running the
        // builders, whose `build_scope` mounts render objects through this
        // same cell.
        let result = tree_binding
            .build_owner
            .run_frame_with_layout_builders(&mut tree_binding.tree, &tree_binding.pipeline_owner);
        // A headless frame over an already-mounted, rooted tree must succeed;
        // a pipeline error here is a regression, surfaced loudly (the harness
        // and production frame path expect the same).
        let layer_tree = result.expect("headless pump_frame: pipeline run_frame should succeed");

        // Service lazy-sliver child requests. Layout may have emitted build
        // requests for absent children and retain-band signals for eviction.
        // Drain both buffers, call each registered ChildManager to build/evict,
        // run a second build_scope for newly-built child subtrees, mark slivers
        // needing re-layout, and finalize evicted elements. This is a no-op when
        // no lazy slivers are mounted.
        tree_binding
            .build_owner
            .service_child_requests(&mut tree_binding.tree, &tree_binding.pipeline_owner);

        layer_tree
    }
}

/// The deterministic multi-presentation clock (issue #556).
///
/// [`HeadlessBinding::pump_frame`] above is the FLUI-native equivalent of
/// Flutter's `WidgetTester.pump` — and, like Flutter's, it is single-view:
/// one virtual clock, one `Vsync`. This is the multi-presentation version:
/// each [`PresentationId`] a caller registers via
/// [`install_presentation_clock`](HeadlessBinding::install_presentation_clock)
/// gets its OWN [`FrameClock`] over its OWN [`ClockSource::Manual`] clock and
/// its OWN [`Vsync`] registry, so a test can pump presentation A three times
/// while B sits untouched, or drive A at a scripted 144 Hz cadence and B at
/// 60 Hz in one interleaved script — a table `pump_frame`'s single clock
/// cannot produce. `pump_frame` itself is unmigrated and keeps its exact
/// meaning; this is purely additive.
///
/// This is not a public pacing mode: the manual source replaces the timing
/// INPUT a presentation's clock reads, never the produce policy —
/// `poll`/`DemandMask`/gating are the exact same code path
/// [`ClockSource::Platform`] runs.
impl HeadlessBinding {
    /// Register a fresh, independent [`FrameClock`] + [`Vsync`] pair for
    /// `id`. Returns the `Vsync` clone a caller wraps a `VsyncScope` around
    /// for that presentation's own widget subtree (or registers a bare
    /// [`AnimationController`] into directly).
    ///
    /// Re-registering an already-installed `id` replaces its pair with a
    /// fresh one — any controller registered on the old `Vsync` handle is
    /// simply no longer reachable from this binding (the same "no
    /// accumulate-across-installs" shape [`HeadlessBinding::new`] gives the
    /// binding's own default clock).
    pub fn install_presentation_clock(&mut self, id: PresentationId) -> Vsync {
        let virtual_clock = ManualClock::new();
        let clock = FrameClock::with_source(ClockSource::Manual(virtual_clock.clone()));
        let vsync = Vsync::new();
        self.presentation_clocks.insert(
            id,
            PresentationClockEntry {
                vsync: vsync.clone(),
                clock,
                virtual_clock,
            },
        );
        vsync
    }

    /// `id`'s own registered `Vsync` clone, if
    /// [`install_presentation_clock`](Self::install_presentation_clock) has
    /// registered it.
    #[must_use]
    pub fn presentation_vsync(&self, id: PresentationId) -> Option<Vsync> {
        self.presentation_clocks
            .get(&id)
            .map(|entry| entry.vsync.clone())
    }

    /// Mark direct demand on `id`'s own clock — for scripting a produce with
    /// no controller involved (a `Host`/`Dirty` demand a real embedder or
    /// widget layer would otherwise supply).
    ///
    /// # Panics
    ///
    /// Panics if `id` was never registered via
    /// [`install_presentation_clock`](Self::install_presentation_clock) —
    /// this is a test harness, where a typo'd or never-installed id must
    /// fail loudly rather than silently do nothing (a vacuously "passing"
    /// assertion downstream is worse than a panic here).
    pub fn mark_presentation_demand(&self, id: PresentationId, kind: DemandKind) {
        let entry = self.presentation_clocks.get(&id).unwrap_or_else(|| {
            panic!(
                "mark_presentation_demand: no clock installed for {id:?} -- call \
                 install_presentation_clock first"
            )
        });
        entry.clock.mark_demand(kind);
    }

    /// Configure a minimum interval between produces on `id`'s own clock —
    /// scripting a throttle (e.g. a target frame rate lower than the
    /// pumped cadence) for a test that needs `poll`'s capacity gate to be
    /// load-bearing in its own right, not just the demand mask. `None`
    /// (the default) imposes no throttle.
    ///
    /// # Panics
    ///
    /// Panics if `id` was never registered — see
    /// [`mark_presentation_demand`](Self::mark_presentation_demand)'s doc
    /// for the rationale.
    pub fn set_presentation_min_produce_interval(
        &self,
        id: PresentationId,
        interval: Option<Duration>,
    ) {
        let entry = self.presentation_clocks.get(&id).unwrap_or_else(|| {
            panic!(
                "set_presentation_min_produce_interval: no clock installed for {id:?} -- call \
                 install_presentation_clock first"
            )
        });
        entry.clock.set_min_produce_interval(interval);
    }

    /// How many frames `id`'s own clock has granted a produce for, total.
    ///
    /// # Panics
    ///
    /// Panics if `id` was never registered — see
    /// [`mark_presentation_demand`](Self::mark_presentation_demand)'s doc
    /// for why a missing id fails loudly here rather than returning `0`
    /// indistinguishably from "installed but never produced".
    #[must_use]
    pub fn presentation_produced_count(&self, id: PresentationId) -> u64 {
        self.presentation_clocks
            .get(&id)
            .unwrap_or_else(|| {
                panic!(
                    "presentation_produced_count: no clock installed for {id:?} -- call \
                     install_presentation_clock first"
                )
            })
            .clock
            .produced_count()
    }

    /// Advance exactly `id`'s own clock and controller registry by `dt`,
    /// deterministically — ticks its `Vsync` (marking `Animation` demand if
    /// a controller was still running at the START of this tick), then
    /// polls its `FrameClock`.
    ///
    /// Demand is sampled BEFORE `tick_all`, not after: the tick that
    /// carries a controller across its completion threshold must still be
    /// treated as real animation work, even though `has_running()` already
    /// reports `false` by the time that same tick returns. This matches the
    /// oracle's own contract — `.flutter/packages/flutter/lib/src/scheduler/
    /// ticker.dart`'s `_tick` invokes `_onTick` unconditionally (deciding
    /// whether to *reschedule* only afterward), and `.flutter/packages/
    /// flutter/lib/src/animation/animation_controller.dart`'s own `_tick`
    /// clamps the value to the endpoint, flips the status to `Completed`,
    /// calls `stop()`, and ONLY THEN calls `notifyListeners()`/
    /// `_checkStatusChanged()` — the completing tick still delivers the
    /// final value and fires the status listener. Sampling `has_running()`
    /// after `tick_all` would silently drop the one pump that carries that
    /// final value/status, because by then the controller has already
    /// stopped.
    ///
    /// No wall-clock read reaches this call: every timestamp
    /// [`FrameClock::poll`] sees traces back to this `advance`, on THIS
    /// presentation's own clock only — a sibling `id`'s clock, mask, and
    /// `Vsync` are untouched.
    ///
    /// # Panics
    ///
    /// Panics if `id` was never registered — see
    /// [`mark_presentation_demand`](Self::mark_presentation_demand)'s doc
    /// for the rationale (a test harness must fail loudly on a wiring bug,
    /// not silently pump nothing).
    pub fn pump_presentation(&self, id: PresentationId, dt: Duration) {
        let entry = self.presentation_clocks.get(&id).unwrap_or_else(|| {
            panic!("pump_presentation: no clock installed for {id:?} -- call install_presentation_clock first")
        });
        entry.clock.advance(dt);
        let now = entry.clock.now();
        let now_secs = entry.virtual_clock.elapsed().as_secs_f64();
        let was_running = entry.vsync.has_running();
        entry.vsync.tick_all(now_secs);
        if was_running {
            entry.clock.mark_demand(DemandKind::Animation);
        }
        let _ = entry.clock.poll(now);
    }

    /// [`pump_presentation`](Self::pump_presentation) for every currently
    /// registered id, each advanced by the SAME `dt` — still fully
    /// independent: each clock advances and polls purely against its own
    /// state, sharing no timeline or mask with any other. A no-op if
    /// nothing is registered (there is no id list to fail loudly about).
    ///
    /// **The iteration order is defined and stable: ascending `PresentationId`
    /// order, not insertion or hash order.** This registry is keyed in a
    /// `HashMap`, whose iteration order is randomized per process (and
    /// varies between `HeadlessBinding` instances within one process); on a
    /// binding whose whole purpose is a deterministic, reproducible test
    /// clock, letting `pump_all` fan out in hash order would be a
    /// self-inflicted source of nondeterminism — two presentations'
    /// controllers publishing into shared listener state (a test's own
    /// tracking `Vec`, say) would then see a different callback interleaving
    /// on every run.
    pub fn pump_all(&self, dt: Duration) {
        let mut ids: Vec<PresentationId> = self.presentation_clocks.keys().copied().collect();
        ids.sort_unstable();
        for id in ids {
            self.pump_presentation(id, dt);
        }
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

#[cfg(test)]
mod committed_layer_tree_tests {
    use flui_rendering::layer::LayerTree;
    use flui_rendering::pipeline::{PipelineCell, PipelineOwner};
    use flui_view::{BuildOwner, ElementTree};

    use super::HeadlessBinding;

    #[test]
    fn gesture_only_binding_has_no_committed_output() {
        let binding = HeadlessBinding::new();
        assert!(binding.layer_tree().is_none());
        assert!(!binding.did_paint_last_frame());
        assert_eq!(binding.painted_frame_count(), 0);
    }

    #[test]
    fn rebinding_without_a_bootstrap_frame_clears_previous_output() {
        let mut binding = HeadlessBinding::new();
        binding.bind_tree_with_committed_layer_tree(
            BuildOwner::new(),
            ElementTree::new(),
            PipelineCell::new(PipelineOwner::new()),
            Some(LayerTree::new()),
        );
        assert!(binding.layer_tree().is_some());
        assert!(binding.did_paint_last_frame());
        assert_eq!(binding.painted_frame_count(), 1);

        binding.bind_tree(
            BuildOwner::new(),
            ElementTree::new(),
            PipelineCell::new(PipelineOwner::new()),
        );

        assert!(binding.layer_tree().is_none());
        assert!(!binding.did_paint_last_frame());
        assert_eq!(binding.painted_frame_count(), 1);
    }
}
