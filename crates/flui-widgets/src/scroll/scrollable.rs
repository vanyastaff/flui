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
//! # Deferred (v1)
//!
//! - Fling ballistic simulation after `on_pan_end`. The simulation is
//!   constructed by `ScrollPhysics::create_ballistic_simulation`, but driving
//!   (ticking) it via `AnimationController::animate_with` + `VsyncScope`
//!   requires wiring the vsync registration in `init_state`, which is deferred.
//! - Axis-constrained drag (only free pan is wired today).
//! - `update_dimensions` auto-called from layout results. In v1 the caller
//!   must call `ScrollController::update_dimensions` externally once the
//!   viewport size is known.
//!
//! # Flutter parity
//!
//! Corresponds to `widgets/scrollable.dart` `Scrollable`. FLUI merges
//! `ScrollPosition` into `ScrollController` (v1 restriction: one position per
//! controller).

use flui_rendering::hit_testing::HitTestBehavior;
use flui_types::layout::Axis;
// `StatelessView` must come from `prelude` — it re-exports the derive macro via
// `flui_macros`; the direct `flui_view::StatelessView` path has only the trait.
use flui_view::prelude::StatelessView;
use flui_view::{BuildContext, Child, IntoView};

use crate::scroll::{ClampingScrollPhysics, ScrollController, SharedScrollPhysics};
use crate::{AnimatedBuilder, GestureDetector, SingleChildScrollView};

/// Detects pan gestures on its child and maps them to scroll-offset changes
/// on the given [`ScrollController`].
///
/// Rebuild is driven reactively: the controller implements [`Listenable`], so
/// every call to [`ScrollController::set_pixels`] schedules a rebuild of only
/// the inner [`AnimatedBuilder`] subtree — the outer `Scrollable` element does
/// not rebuild on each frame.
///
/// # Example
///
/// ```rust,ignore
/// let controller = ScrollController::new();
/// controller.update_dimensions(400.0, 0.0, 1000.0);
///
/// Scrollable::new()
///     .controller(controller.clone())
///     .child(MyTallContent::new())
/// ```
///
/// [`Listenable`]: flui_foundation::Listenable
#[derive(Clone, StatelessView)]
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
            physics: std::sync::Arc::new(ClampingScrollPhysics::default()),
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

impl StatelessView for Scrollable {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        let controller = self.controller.clone();
        let physics = self.physics.clone();
        let scroll_direction = self.scroll_direction;
        // Snapshot the child from view config; the closure re-reads it on
        // every notification so a parent rebuild with a new child is reflected.
        let child = self.child.clone();

        AnimatedBuilder::new(self.controller.as_listenable(), move || {
            let pixels = controller.pixels();
            // Clone handles for the update closure (captured by value, called
            // on each pan event — no lock held across user code).
            let ctrl_update = controller.clone();
            let phys_update = physics.clone();

            // Build the inner scroll view at the current pixel offset.
            let mut scroll_view = SingleChildScrollView::new()
                .scroll_direction(scroll_direction)
                .offset(pixels);
            if let Some(content) = child.clone().into_inner() {
                scroll_view = scroll_view.child(content);
            }

            // Flutter parity: Scrollable uses HitTestBehavior.opaque so the
            // gesture area fires regardless of whether the child content is
            // itself hittable (e.g. an empty SizedBox).
            GestureDetector::new()
                .behavior(HitTestBehavior::Opaque)
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
                    let new_pixels = phys_update.apply_boundary_conditions(
                        proposed,
                        ctrl_update.min_scroll_extent(),
                        ctrl_update.max_scroll_extent(),
                    );
                    ctrl_update.set_pixels(new_pixels);
                })
                .on_pan_end(|_details| {
                    // DEFERRED (v1): fling ballistic simulation.
                    //
                    // Would use:
                    //   let sim = phys.create_ballistic_simulation(
                    //       -details.primary_velocity, controller.pixels(),
                    //       controller.min_scroll_extent(),
                    //       controller.max_scroll_extent());
                    //   AnimationController::animate_with(sim) — requires
                    //   VsyncScope wiring in init_state.
                })
                .child(scroll_view)
        })
    }
}
