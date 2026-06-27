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
//! On `on_pan_end`, a [`ScrollPhysics`] ballistic simulation is started via
//! `AnimationController::animate_with`. The fling controller is registered
//! with the ambient [`VsyncScope`] in `init_state` so the binding ticks it
//! each frame deterministically; a value listener on the controller pushes the
//! current pixel position into the [`ScrollController`] each tick.
//!
//! `on_pan_start` halts any in-flight fling via `stop()`, so grabbing a
//! scrolling list feels physically correct.
//!
//! # Flutter parity
//!
//! Corresponds to `widgets/scrollable.dart` `Scrollable`. FLUI merges
//! `ScrollPosition` into `ScrollController` (v1 restriction: one position per
//! controller).

use std::sync::Arc;
use std::time::Duration;

use flui_animation::{Animation, AnimationController, Scheduler, Vsync, VsyncRegistration};
use flui_foundation::{Listenable, ListenerId};
use flui_rendering::hit_testing::HitTestBehavior;
use flui_types::layout::Axis;
use flui_view::prelude::StatefulView;
use flui_view::{BuildContext, BuildContextExt, Child, IntoView, ViewState};

use crate::animated::VsyncScope;
use crate::scroll::{ClampingScrollPhysics, ScrollController, SharedScrollPhysics};
use crate::{AnimatedBuilder, GestureDetector, SingleChildScrollView};

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
}

impl std::fmt::Debug for Scrollable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Scrollable")
            .field("scroll_direction", &self.scroll_direction)
            .field("controller", &self.controller)
            .field("physics", &self.physics)
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
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
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
    /// state so the fling listener (registered in `init_state`) can reach it
    /// without re-capturing on every `build`. Updated in `did_update_view`.
    ///
    /// Note: if the caller replaces the controller with a different object
    /// (rather than mutating the same `Arc`-backed handle), the listener will
    /// continue driving the old one until the widget is disposed. Full
    /// hot-swap is deferred — the common case is one stable controller.
    scroll_controller: ScrollController,
    /// The ballistic simulation driver. Bounds span `(NEG_INFINITY, INFINITY)`
    /// so pixel-space simulation positions are not clamped to `[0, 1]`.
    ///
    /// Created once in `create_state`; registered with the ambient
    /// `VsyncScope` in `init_state`; disposed in `dispose`.
    fling_controller: AnimationController,
    /// Value-listener ID on `fling_controller` that pushes pixels into
    /// `scroll_controller` each tick. Registered in `init_state`, removed in
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

impl ViewState<Scrollable> for ScrollableState {
    fn init_state(&mut self, ctx: &dyn BuildContext) {
        // Attach the value listener that pushes the fling simulation's current
        // pixel position into the scroll controller each tick. The listener
        // holds `Arc`-backed clones, so it survives across widget rebuilds.
        let fling_ref = self.fling_controller.clone();
        let scroll_ref = self.scroll_controller.clone();
        let listener_id = self.fling_controller.add_listener(Arc::new(move || {
            scroll_ref.set_pixels(fling_ref.value());
        }));
        self.fling_listener_id = Some(listener_id);

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

    fn build(&self, view: &Scrollable, _ctx: &dyn BuildContext) -> impl IntoView {
        let scroll_controller = view.controller.clone();
        let physics = view.physics.clone();
        let scroll_direction = view.scroll_direction;
        let child = view.child.clone();
        let fling_controller = self.fling_controller.clone();

        AnimatedBuilder::new(scroll_controller.as_listenable(), move || {
            let pixels = scroll_controller.pixels();

            // Clones for the gesture callbacks; each closure needs its own
            // `Arc`-counted handle (no refcount bump at call time).
            let fling_stop = fling_controller.clone();
            let ctrl_update = scroll_controller.clone();
            let phys_update = physics.clone();
            let fling_start = fling_controller.clone();
            let phys_fling = physics.clone();
            let ctrl_fling = scroll_controller.clone();

            let mut scroll_view = SingleChildScrollView::new()
                .scroll_direction(scroll_direction)
                .offset(pixels);
            if let Some(content) = child.clone().into_inner() {
                scroll_view = scroll_view.child(content);
            }

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
                    let clamped = phys_update.apply_boundary_conditions(
                        proposed,
                        ctrl_update.min_scroll_extent(),
                        ctrl_update.max_scroll_extent(),
                    );
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

                    if let Some(sim) = phys_fling.create_ballistic_simulation(
                        fling_velocity_px_per_sec,
                        ctrl_fling.pixels(),
                        ctrl_fling.min_scroll_extent(),
                        ctrl_fling.max_scroll_extent(),
                    ) {
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
        // Track the current controller so the fling listener stays in sync if
        // a parent rebuild hands us a new configuration.
        self.scroll_controller = new_view.controller.clone();
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
        self.fling_controller.dispose();
    }
}
