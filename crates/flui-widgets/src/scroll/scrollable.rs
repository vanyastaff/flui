//! [`Scrollable`] — gesture-driven interactive scroll widget.
//!
//! `Scrollable` composes:
//! - A [`GestureDetector`] that translates pan events into offset mutations on
//!   the [`ScrollController`].
//! - An [`AnimatedBuilder`] driven by the controller's [`Listenable`]: every
//!   time `set_pixels` fires `notify_listeners`, the inner subtree rebuilds
//!   with the current scroll offset, giving the illusion of continuous motion.
//! - A [`SingleChildScrollView`] as the layout/paint host, receiving the live
//!   `controller.pixels()` as its programmatic offset.
//!
//! # Fling ballistic simulation
//!
//! On `on_pan_end`, a `ScrollPhysics` ballistic simulation is started via
//! `AnimationController::animate_with`. The fling controller is registered
//! with the ambient [`VsyncScope`] in `init_state` so the binding ticks it
//! each frame deterministically; a value listener on the controller pushes the
//! current pixel position into the [`ScrollController`] each tick.
//!
//! `on_pan_start` halts any in-flight fling via `stop()`, so grabbing a
//! scrolling list feels physically correct.
//!
//! # `animate_to` servicing (ADR-0037)
//!
//! [`ScrollController::animate_to`]/[`jump_to`](ScrollController::jump_to)
//! don't drive the fling controller directly — they queue a command (see
//! `scroll_controller.rs`'s module docs) that this widget's `build` closure
//! services on every rebuild the controller's notify triggers, via
//! [`ScrollController::service_pending_command`]. Reusing the SAME
//! `AnimationController` the ballistic fling above drives means `on_pan_start`
//! cancels a running `animate_to` for free — it stops whichever of the two
//! (fling or curve-driven tween) happens to be active — and `jump_to` queues
//! an explicit cancel for the same reason.
//!
//! # Flutter parity
//!
//! Corresponds to `widgets/scrollable.dart` `Scrollable`. FLUI merges
//! `ScrollPosition` into `ScrollController` (v1 restriction: one position per
//! controller).

use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use flui_animation::{Animation, AnimationController, Scheduler, Vsync, VsyncRegistration};
use flui_foundation::{Listenable, ListenerId};
use flui_rendering::hit_testing::HitTestBehavior;
use flui_rendering::view::ScrollPosition;
use flui_types::layout::Axis;
use flui_view::prelude::StatefulView;
use flui_view::{BoxedView, BuildContext, BuildContextExt, Child, IntoView, ViewExt, ViewState};

use crate::animated::VsyncScope;
use crate::scroll::{ClampingScrollPhysics, ScrollController, ScrollMetrics, SharedScrollPhysics};
use crate::{AnimatedBuilder, GestureDetector, SingleChildScrollView};

/// A caller-supplied composition of the scrollable content, receiving the
/// [`Scrollable`]'s shared [`ScrollPosition`] and returning the view to
/// scroll. See [`Scrollable::viewport_builder`].
///
/// `Rc`, not `Arc + Send + Sync`: `BoxedView` erases to `Box<dyn View>`, and
/// `View` carries no `Send`/`Sync` supertrait (widget trees are built and
/// laid out on one thread), so a closure that captures pre-built view
/// content (e.g. an eager child list) can never satisfy `+ Send + Sync` —
/// same reason `AnimatedBuilder`'s own builder closure
/// (`transitions/animated_builder.rs`) is `Rc<dyn Fn() -> BoxedView>`, not
/// `Arc<... + Send + Sync>`.
pub type ViewportBuilder = Rc<dyn Fn(ScrollPosition) -> BoxedView>;

// ---------------------------------------------------------------------------
// View (configuration)
// ---------------------------------------------------------------------------

/// Detects pan gestures on its child and maps them to scroll-offset changes
/// on the given [`ScrollController`], including a ballistic fling simulation
/// after the user lifts their finger.
///
/// Rebuild is driven reactively: the controller implements [`Listenable`], so
/// every call to [`ScrollController::set_pixels`] schedules a rebuild of only
/// the inner [`AnimatedBuilder`] subtree — the outer `Scrollable` element does
/// not rebuild on each frame.
///
/// A [`VsyncScope`] must be above the `Scrollable` in the tree (or provided
/// by the application's binding) for fling animations to be driven
/// deterministically; without one the fling controller falls back to its own
/// wall-clock scheduler.
///
/// # Example
///
/// ```rust,ignore
/// let controller = ScrollController::new();
/// controller.update_dimensions(400.0, 0.0, 1000.0);
///
/// VsyncScope::new(
///     vsync.clone(),
///     Scrollable::new()
///         .controller(controller.clone())
///         .child(MyTallContent::new()),
/// )
/// ```
///
/// [`Listenable`]: flui_foundation::Listenable
#[derive(Clone, StatefulView)]
pub struct Scrollable {
    /// The shared position + notification hub.
    controller: ScrollController,
    /// The boundary / fling behaviour.
    physics: SharedScrollPhysics,
    /// The axis along which the child scrolls.
    scroll_direction: Axis,
    /// The content to make scrollable.
    child: Child,
    /// Overrides the scrollable content's composition entirely; `None`
    /// keeps the `SingleChildScrollView`-over-`child` fast path. See
    /// [`Scrollable::viewport_builder`].
    viewport_builder: Option<ViewportBuilder>,
}

impl std::fmt::Debug for Scrollable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Scrollable")
            .field("scroll_direction", &self.scroll_direction)
            .field("controller", &self.controller)
            .field("physics", &self.physics)
            .field("has_viewport_builder", &self.viewport_builder.is_some())
            .finish_non_exhaustive()
    }
}

impl Default for Scrollable {
    fn default() -> Self {
        Self {
            controller: ScrollController::new(),
            physics: Arc::new(ClampingScrollPhysics::new()),
            scroll_direction: Axis::Vertical,
            child: Child::empty(),
            viewport_builder: None,
        }
    }
}

impl Scrollable {
    /// A new vertical `Scrollable` with clamping physics and a fresh
    /// `ScrollController`. Call `.controller(...)` to share the position with
    /// a [`Scrollbar`](super::Scrollbar) or to read the offset programmatically.
    pub fn new() -> Self {
        Self::default()
    }

    /// Attach a [`ScrollController`] (position + notification hub). Multiple
    /// clones of the same controller share state, so a `Scrollbar` can listen
    /// to the same controller.
    #[must_use]
    pub fn controller(mut self, controller: ScrollController) -> Self {
        self.controller = controller;
        self
    }

    /// Override the boundary / fling behaviour (default:
    /// [`ClampingScrollPhysics`]).
    #[must_use]
    pub fn physics(mut self, physics: SharedScrollPhysics) -> Self {
        self.physics = physics;
        self
    }

    /// The scroll axis (default [`Axis::Vertical`]).
    #[must_use]
    pub fn scroll_direction(mut self, axis: Axis) -> Self {
        self.scroll_direction = axis;
        self
    }

    /// The scrollable content.
    ///
    /// Ignored when [`Scrollable::viewport_builder`] is set — the builder
    /// closure is responsible for its own content in that case.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }

    /// Override the scrollable content's composition entirely.
    ///
    /// By default (`None`) `Scrollable` composes a [`SingleChildScrollView`]
    /// wrapping [`Scrollable::child`] — the fast path most callers want. Set
    /// this to compose an arbitrary scrollable widget instead — e.g. a
    /// [`Viewport`](super::Viewport) over several slivers, a `ListView`, or a
    /// `CustomScrollView` — when a single child in a `SingleChildScrollView`
    /// isn't the right shape.
    ///
    /// The closure receives this `Scrollable`'s controller's shared
    /// [`ScrollPosition`] and must inject it into whatever it builds
    /// (typically via that widget's own `.position(...)`) so the drag/fling
    /// gesture wiring above still drives it, and `RenderViewport`'s
    /// committed content extents still flush back into the same controller.
    ///
    /// When this is `Some`, [`Scrollable::child`] is ignored.
    #[must_use]
    pub fn viewport_builder(mut self, builder: ViewportBuilder) -> Self {
        self.viewport_builder = Some(builder);
        self
    }
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// Persistent state for [`Scrollable`].
///
/// Owns the ballistic fling [`AnimationController`] and its vsync
/// registration. The fling controller has effectively unbounded value range
/// (`f32::NEG_INFINITY` → `f32::INFINITY`) so pixel-space simulation values
/// are never clamped. A value listener on the controller pushes the live pixel
/// position into the [`ScrollController`] each tick.
pub struct ScrollableState {
    /// The scroll controller from the current view configuration. Kept in
    /// state so the fling listener (installed by
    /// [`install_fling_listener`](ScrollableState::install_fling_listener))
    /// can reach it without re-capturing on every `build`. Updated in
    /// `did_update_view` BEFORE both `install_fling_listener` and
    /// `install_stop_hook` re-run — each always reads whatever this field
    /// currently holds, so a controller SWAP moves both onto the new
    /// controller in the same call.
    scroll_controller: ScrollController,
    /// The ballistic simulation driver. Bounds span `(NEG_INFINITY, INFINITY)`
    /// so pixel-space simulation positions are not clamped to `[0, 1]`.
    ///
    /// Created once in `create_state`; registered with the ambient
    /// `VsyncScope` in `init_state`; disposed in `dispose`.
    fling_controller: AnimationController,
    /// Value-listener ID on `fling_controller` that pushes pixels into
    /// `scroll_controller` each tick. Installed by
    /// [`install_fling_listener`](ScrollableState::install_fling_listener)
    /// (called from `init_state`, and re-run on every `did_update_view` so a
    /// controller swap moves it onto the new controller), removed in
    /// `dispose`.
    fling_listener_id: Option<ListenerId>,
    /// Vsync handle kept for `unregister` in `dispose`.
    vsync: Option<Vsync>,
    /// Registration handle returned by `vsync.register(fling_controller)`.
    vsync_registration: Option<VsyncRegistration>,
}

impl std::fmt::Debug for ScrollableState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScrollableState")
            .field("scroll_controller", &self.scroll_controller)
            .field("fling_registered", &self.vsync_registration.is_some())
            .finish_non_exhaustive()
    }
}

impl StatefulView for Scrollable {
    type State = ScrollableState;

    fn create_state(&self) -> Self::State {
        // Wide-open bounds: pixel values from the ballistic simulation are
        // never clamped — the simulation's own `is_done` terminates the run.
        // `NEG_INFINITY < INFINITY` satisfies `with_bounds`'s lower < upper
        // check; `value.clamp(NEG_INF, INF)` is the identity on finite f32.
        let fling_controller = AnimationController::with_bounds(
            Duration::from_millis(1),
            Arc::new(Scheduler::new()),
            f32::NEG_INFINITY,
            f32::INFINITY,
        )
        .expect("NEG_INFINITY < INFINITY satisfies the bounds invariant");

        ScrollableState {
            scroll_controller: self.controller.clone(),
            fling_controller,
            fling_listener_id: None,
            vsync: None,
            vsync_registration: None,
        }
    }
}

impl ScrollableState {
    /// Installs the binding's post-frame capability on the scroll
    /// controller's shared `ScrollPosition`, so `RenderViewport::
    /// perform_layout`'s committed content extents (`apply_viewport_dimension`/
    /// `apply_content_dimensions`) can flush a coalesced notification after
    /// layout instead of never notifying at all.
    ///
    /// Lifecycle-only (ADR-0021, port-check trigger #22): called from
    /// `init_state`/`did_change_dependencies`, never from `build`. A no-op
    /// when no handle is available yet — `set_flush_handle` is idempotent, so
    /// a later call (e.g. from `did_change_dependencies`) still installs it.
    fn install_flush_handle(&self, ctx: &dyn BuildContext) {
        if let Some(handle) = ctx.post_frame_handle() {
            self.scroll_controller.position().set_flush_handle(handle);
        }
    }

    /// Installs the synchronous `jump_to` cancellation hook (ADR-0037) on
    /// [`ScrollController`], closing over this state's own fling controller —
    /// see `ScrollController`'s `stop_hook` field docs for why `jump_to`
    /// needs a hook called AT jump_to time rather than a merely-queued
    /// command serviced on the next rebuild.
    ///
    /// Idempotent (mirrors `install_flush_handle`): called from `init_state`
    /// AND `did_update_view` (a controller swap must move the hook onto the
    /// NEW controller — see `scroll_controller`'s field doc), always against
    /// whatever `self.scroll_controller` currently is.
    fn install_stop_hook(&self) {
        let fling = self.fling_controller.clone();
        self.scroll_controller.set_stop_hook(Arc::new(move || {
            let _ = fling.stop();
        }));
    }

    /// Installs (or re-installs) the fling value listener that pushes the
    /// ballistic simulation's current pixel position into
    /// `self.scroll_controller` each tick.
    ///
    /// Idempotent, mirroring `install_stop_hook`: called from `init_state`
    /// AND `did_update_view`, always against whatever `self.scroll_controller`
    /// currently is. Removes any previously-installed listener first — this
    /// is what closes the swap-blindness bug: without it, a controller SWAP
    /// left the listener captured in `init_state` pushing ticks into the OLD
    /// controller forever, so an `animate_to`/fling on the NEW controller
    /// drove `fling_controller`'s value but the new controller's own
    /// `pixels()` never moved.
    fn install_fling_listener(&mut self) {
        if let Some(id) = self.fling_listener_id.take() {
            self.fling_controller.remove_listener(id);
        }
        let fling_ref = self.fling_controller.clone();
        let scroll_ref = self.scroll_controller.clone();
        let listener_id = self.fling_controller.add_listener(Arc::new(move || {
            scroll_ref.set_pixels(fling_ref.value());
        }));
        self.fling_listener_id = Some(listener_id);
    }
}

impl ViewState<Scrollable> for ScrollableState {
    fn init_state(&mut self, ctx: &dyn BuildContext) {
        self.install_flush_handle(ctx);
        self.install_stop_hook();
        self.install_fling_listener();

        // Register with the ambient VsyncScope so the binding ticks the fling
        // controller on each virtual frame — the same pattern used by
        // `ImplicitController::register`.
        if let Some(vsync) = ctx.get::<VsyncScope, _>(|scope| scope.vsync().clone()) {
            let registration = vsync.register(self.fling_controller.clone());
            self.vsync = Some(vsync);
            self.vsync_registration = Some(registration);
        }
        // If no VsyncScope is present, the fling controller falls back to its
        // own wall-clock Scheduler ticker — animations still work on a real
        // display, they simply cannot be driven deterministically in tests.
    }

    fn did_change_dependencies(&mut self, ctx: &dyn BuildContext) {
        self.install_flush_handle(ctx);
    }

    fn build(&self, view: &Scrollable, _ctx: &dyn BuildContext) -> impl IntoView {
        let scroll_controller = view.controller.clone();
        let physics = view.physics.clone();
        let scroll_direction = view.scroll_direction;
        let child = view.child.clone();
        let viewport_builder = view.viewport_builder.clone();
        let fling_controller = self.fling_controller.clone();

        AnimatedBuilder::new(scroll_controller.as_listenable(), move || {
            // Service any `animate_to`/`jump_to`-queued command BEFORE
            // building this rebuild's subtree — this closure reruns on every
            // notify the controller fires (ADR-0037's "notify path"),
            // exactly the trigger `ScrollController::animate_to`/`jump_to`
            // fire after queuing a command. See `scroll_controller.rs`'s
            // module docs and `ScrollController::service_pending_command`.
            scroll_controller.service_pending_command(&fling_controller);

            // Clones for the gesture callbacks; each closure needs its own
            // `Arc`-counted handle (no refcount bump at call time).
            let fling_stop = fling_controller.clone();
            let ctrl_update = scroll_controller.clone();
            let phys_update = physics.clone();
            let fling_start = fling_controller.clone();
            let phys_fling = physics.clone();
            let ctrl_fling = scroll_controller.clone();

            // Position mode, not `.offset(pixels)`: the composed viewport's
            // offset IS this controller's shared `ScrollPosition`, so a
            // gesture write is observed directly (no push from this rebuild)
            // and `RenderViewport::perform_layout`'s committed content
            // extents flush back into the same position — see
            // `ScrollPosition`'s docs and `Viewport::position`.
            let scroll_view: BoxedView = if let Some(build_viewport) = &viewport_builder {
                // Custom composition: the closure owns injecting the shared
                // position into whatever it builds.
                build_viewport(scroll_controller.position())
            } else {
                let mut scsv = SingleChildScrollView::new()
                    .scroll_direction(scroll_direction)
                    .position(scroll_controller.position());
                if let Some(content) = child.clone().into_inner() {
                    scsv = scsv.child(content);
                }
                scsv.boxed()
            };

            // Flutter parity: Scrollable uses HitTestBehavior::Opaque so the
            // gesture area fires regardless of whether the child content is
            // itself hittable (e.g. an empty SizedBox).
            GestureDetector::new()
                .behavior(HitTestBehavior::Opaque)
                .on_pan_start(move |_details| {
                    // Grab: halt any in-flight fling so the list stops at the
                    // finger's contact position (Flutter parity — ScrollPosition
                    // calls `activity.cancel()` on `handleDragStart`).
                    let _ = fling_stop.stop();
                })
                .on_pan_update(move |details| {
                    // Flutter convention: a downward finger drag (positive delta
                    // on the scroll axis) moves the viewport toward the START of
                    // the content, so the offset DECREASES.
                    //
                    // `apply_boundary_conditions` enforces the physics limits
                    // (hard clamp or spring resistance) before committing.
                    let raw_delta = match scroll_direction {
                        Axis::Vertical => details.delta.dy.get(),
                        Axis::Horizontal => details.delta.dx.get(),
                    };
                    let proposed = ctrl_update.pixels() - raw_delta;
                    let metrics = ScrollMetrics::from(&ctrl_update.position());
                    let clamped = phys_update.apply_boundary_conditions(&metrics, proposed);
                    ctrl_update.set_pixels(clamped);
                })
                .on_pan_end(move |details| {
                    // Pointer velocity is in "screen coordinates": positive dy =
                    // finger moving down. Scroll offset increases when the finger
                    // moves UP, so we negate to convert to scroll velocity.
                    let fling_velocity_px_per_sec = match scroll_direction {
                        Axis::Vertical => -details.velocity.pixels_per_second.dy.get(),
                        Axis::Horizontal => -details.velocity.pixels_per_second.dx.get(),
                    };
                    // Cap at Flutter's `kMaxFlingVelocity` (8 000 px/s). The LSQ
                    // velocity tracker can produce astronomically large velocities
                    // when pointer samples arrive with sub-millisecond timestamps
                    // (headless test timing); an unbounded velocity drives
                    // `UnderdampedSolution` to `f32::INFINITY` for any t > 0.
                    let fling_velocity_px_per_sec =
                        fling_velocity_px_per_sec.clamp(-8_000.0, 8_000.0);
                    // `clamp` propagates NaN (IEEE 754); NaN can arrive when all
                    // pointer events share the same timestamp (degenerate LSQ —
                    // now guarded in `VelocityTracker::compute_estimate`, but we
                    // keep this as belt-and-suspenders). Treat NaN as zero so
                    // physics still springs back when the position is past a
                    // boundary, even without a measurable fling velocity.
                    let fling_velocity_px_per_sec = if fling_velocity_px_per_sec.is_nan() {
                        0.0
                    } else {
                        fling_velocity_px_per_sec
                    };

                    let metrics = ScrollMetrics::from(&ctrl_fling.position());
                    if let Some(sim) =
                        phys_fling.create_ballistic_simulation(&metrics, fling_velocity_px_per_sec)
                    {
                        // `Box<dyn Simulation>` implements `Simulation` via the
                        // blanket impl in `flui-animation`, so it can be passed
                        // directly as `S: Simulation + 'static`.
                        let _ = fling_start.animate_with(sim);
                    }
                })
                .child(scroll_view)
        })
    }

    fn did_update_view(&mut self, _old_view: &Scrollable, new_view: &Scrollable) {
        // Track the current controller so the fling listener and stop hook
        // stay in sync if a parent rebuild hands us a new configuration —
        // both re-installs below always read `self.scroll_controller` as
        // just updated here.
        self.scroll_controller = new_view.controller.clone();

        // Re-install the fling value listener on the (possibly new)
        // controller. `install_fling_listener` is idempotent (removes any
        // previous listener first), so this is cheap even when the
        // controller didn't actually change. Without this, a controller
        // SWAP would leave the listener pushing ticks into the OLD
        // controller forever: an `animate_to`/fling driven on the NEW
        // controller would move `fling_controller`'s value, but nothing
        // would ever copy it into the new controller's own `ScrollPosition`
        // — its pixels would never move.
        self.install_fling_listener();

        // Re-install the stop hook on the (possibly new) controller —
        // `install_stop_hook` is idempotent (see its doc), so this is cheap
        // even when the controller didn't actually change. Without this, a
        // controller SWAP would leave the hook on the OLD controller only:
        // the new controller's `jump_to` would silently lose the
        // synchronous cancel path (see `ScrollController`'s `stop_hook`
        // field docs for the one-frame gap that reopens).
        self.install_stop_hook();
    }

    fn dispose(&mut self) {
        // Remove the value listener before disposing the controller so the
        // listener closure cannot fire after the state is gone.
        if let Some(id) = self.fling_listener_id.take() {
            self.fling_controller.remove_listener(id);
        }
        // Release the vsync registration so the binding does not hold a
        // reference to the disposed controller.
        if let (Some(vsync), Some(registration)) =
            (self.vsync.take(), self.vsync_registration.take())
        {
            vsync.unregister(registration);
        }
        // Detach the ADR-0037 stop hook and drop any not-yet-serviced
        // pending command — without this, the user-held `ScrollController`
        // would keep an `Arc` closing over this about-to-be-disposed
        // `fling_controller` alive (and reachable via `jump_to`) forever, and
        // a command queued while still attached to THIS widget would
        // otherwise resurface against a DIFFERENT `ScrollableState` if the
        // same controller is later re-attached to a new `Scrollable`.
        self.scroll_controller.clear_stop_hook();
        self.scroll_controller.clear_pending_command();
        self.fling_controller.dispose();
    }
}
