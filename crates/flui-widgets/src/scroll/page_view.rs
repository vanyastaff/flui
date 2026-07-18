//! [`PageView`] — a scrollable list that works page by page, plus its
//! [`PageController`] and [`PageScrollPhysics`].
//!
//! # Flutter parity
//!
//! Mirrors `widgets/page_view.dart` (tag `3.44.0`): `PageView` composes a
//! [`Scrollable`] with [`PageScrollPhysics`] and a [`PageController`],
//! `viewport_builder`-ing a [`Viewport`] over a single
//! [`SliverFillViewport`] — the same "one sliver whose children each fill a
//! `viewport_fraction`-sized page" shape the oracle uses (`SliverFillViewport`
//! over `SliverChildDelegate` there; a plain eager child list here, see
//! [`PageView`]'s own docs for that divergence).
//!
//! # Deferred / documented divergences from the oracle (v1)
//!
//! - **Eager children.** `SliverFillViewport` (`flui-widgets`) has no lazy
//!   child delegate yet — every page attaches up front, not
//!   `PageView.builder`'s on-demand construction.
//! - **`on_page_changed` is listener-based**, not `NotificationListener<
//!   ScrollNotification>` — FLUI has no scroll-notification bubbling yet.
//!   Fires when `round(page)` changes, same as the oracle's
//!   `_lastReportedPage` tracking.
//! - **`pageSnapping: false`, `reverse`, `padEnds`, `allowImplicitScrolling`,
//!   `PageStorage` restoration, and `viewport_fraction > 1.0` centering** are
//!   not modeled — [`PageScrollPhysics`] is always applied (page snapping is
//!   the only supported mode) and [`DimensionChangePolicy::KeepFractionalPage`]
//!   already documents the `viewport_fraction > 1.0` gap it inherits.
//! - **`PageController::animateToPage`/`nextPage`/`previousPage`** (animated,
//!   ticker-driven transitions) are not ported — [`PageController::jump_to_page`]
//!   is the only programmatic navigation, matching `jumpToPage`.

use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use flui_animation::simulation::{ScrollSpringSimulation, Simulation, SpringDescription};
use flui_foundation::{Listenable, ListenerId};
use flui_rendering::view::{
    CacheExtentStyle, DimensionChangePolicy, ScrollPosition, ViewportOffset,
};
use flui_types::layout::{Axis, AxisDirection};
use flui_view::prelude::StatefulView;
use flui_view::seq::ViewSeq;
use flui_view::{BoxedView, BuildContext, IntoView, ViewExt, ViewState};

use crate::scroll::{
    ClampingScrollPhysics, ScrollController, ScrollMetrics, ScrollPhysics, Scrollable,
    SharedScrollPhysics, SliverFillViewport, Viewport,
};

// ============================================================================
// PageScrollPhysics
// ============================================================================

/// Scroll physics that snap a [`PageView`] to page boundaries after a drag or
/// fling.
///
/// # Flutter parity
///
/// Mirrors `PageScrollPhysics` (`widgets/page_view.dart`, tag `3.44.0`):
/// `_getTargetPixels`' velocity-vs-tolerance ±half-page bias, rounded to the
/// nearest whole page, sprung to via [`ScrollSpringSimulation`]. FLUI's
/// `ScrollPhysics` trait has no `parent`-chaining (see `scroll_physics.rs`'s
/// module docs), so where the oracle defers out-of-range handling to
/// `super.createBallisticSimulation` (its `parent` physics), this instead
/// owns [`boundary`](Self::boundary) directly and delegates to it — the same
/// effective behavior (Flutter's real usage always resolves `parent` to a
/// platform physics anyway), without adding general trait-level chaining out
/// of this PR's scope.
#[derive(Debug, Clone)]
pub struct PageScrollPhysics {
    /// Fraction of the viewport one logical page occupies. Must match the
    /// [`PageController`] driving the same [`PageView`] — this is how
    /// `_getPage`/`_getPixels` in the oracle special-case `_PagePosition`
    /// (which knows its own `viewportFraction`); FLUI's `ScrollMetrics`
    /// snapshot carries no such field, so this physics must be told
    /// separately.
    pub viewport_fraction: f32,
    /// Boundary-clamping and out-of-range ballistic physics this delegates
    /// to. Defaults to [`ClampingScrollPhysics`] (Flutter's platform default
    /// on most targets).
    pub boundary: SharedScrollPhysics,
    /// Spring configuration for the page-to-page snap. Flutter's base
    /// `ScrollPhysics.spring` default (`SpringDescription.withDampingRatio(
    /// mass: 0.5, stiffness: 100.0, ratio: 1.1)`) — `PageScrollPhysics` does
    /// not override `spring` in the oracle, so this is NOT
    /// `BouncingScrollPhysics`'s bouncier tuning.
    pub spring: SpringDescription,
    /// Below this absolute velocity (logical px/s), `_getTargetPixels`
    /// applies no directional bias — the drag settles to the nearest page
    /// rather than committing to next/previous. Mirrors `toleranceFor`'s
    /// velocity term (`1.0 / (0.050 * devicePixelRatio)`) evaluated at
    /// `devicePixelRatio == 1.0` — FLUI's `ScrollMetrics` carries no
    /// device-pixel-ratio field (documented divergence, consistent with the
    /// fixed velocity thresholds `ClampingScrollPhysics`/
    /// `BouncingScrollPhysics` already use).
    pub velocity_tolerance_px_per_sec: f32,
}

impl PageScrollPhysics {
    /// Page-snapping physics for a [`PageView`] whose pages occupy
    /// `viewport_fraction` of the viewport.
    ///
    /// # Panics
    ///
    /// Panics when `viewport_fraction <= 0.0`.
    #[must_use]
    pub fn new(viewport_fraction: f32) -> Self {
        assert!(
            viewport_fraction > 0.0,
            "PageScrollPhysics viewport_fraction must be > 0.0 (got {viewport_fraction})"
        );
        Self {
            viewport_fraction,
            boundary: Arc::new(ClampingScrollPhysics::new()),
            spring: SpringDescription::with_damping_ratio(0.5, 100.0, 1.1),
            velocity_tolerance_px_per_sec: 20.0,
        }
    }
}

impl ScrollPhysics for PageScrollPhysics {
    fn apply_boundary_conditions(&self, metrics: &ScrollMetrics, proposed_pixels: f32) -> f32 {
        self.boundary
            .apply_boundary_conditions(metrics, proposed_pixels)
    }

    fn create_ballistic_simulation(
        &self,
        metrics: &ScrollMetrics,
        velocity_px_per_sec: f32,
    ) -> Option<Box<dyn Simulation>> {
        // Out of range and not heading back in: defer entirely to the
        // boundary physics, mirroring `super.createBallisticSimulation`
        // (the oracle's `parent` chain).
        if (velocity_px_per_sec <= 0.0 && metrics.pixels <= metrics.min_scroll_extent)
            || (velocity_px_per_sec >= 0.0 && metrics.pixels >= metrics.max_scroll_extent)
        {
            return self
                .boundary
                .create_ballistic_simulation(metrics, velocity_px_per_sec);
        }

        let mut page = metrics.page(self.viewport_fraction);
        if velocity_px_per_sec < -self.velocity_tolerance_px_per_sec {
            page -= 0.5;
        } else if velocity_px_per_sec > self.velocity_tolerance_px_per_sec {
            page += 0.5;
        }
        let target = metrics.pixels_from_page(self.viewport_fraction, page.round());

        if (target - metrics.pixels).abs() > f32::EPSILON {
            Some(Box::new(ScrollSpringSimulation::new(
                self.spring,
                metrics.pixels,
                target,
                velocity_px_per_sec,
            )))
        } else {
            None
        }
    }
}

// ============================================================================
// PageController
// ============================================================================

/// Controls which page is visible in a [`PageView`].
///
/// # Flutter parity
///
/// Mirrors `PageController` (`widgets/page_view.dart`, tag `3.44.0`).
/// `initial_page` and `viewport_fraction` are fixed at construction (Flutter
/// declares both `final`) rather than mutable builder fields — retargeting
/// either after construction has no oracle behavior to port (Flutter's own
/// `viewportFraction` setter, used internally by `attach`, is not part of the
/// public `PageController` API this port targets).
#[derive(Clone, Debug)]
pub struct PageController {
    scroll: ScrollController,
    initial_page: usize,
    viewport_fraction: f32,
}

impl Default for PageController {
    fn default() -> Self {
        Self::new()
    }
}

impl PageController {
    /// A controller starting at page `0` with `viewport_fraction: 1.0` —
    /// Flutter's `PageController()` defaults.
    #[must_use]
    pub fn new() -> Self {
        Self::with_params(0, 1.0)
    }

    /// A controller starting at `initial_page`, with pages occupying
    /// `viewport_fraction` of the viewport.
    ///
    /// # Panics
    ///
    /// Panics when `viewport_fraction <= 0.0` (Flutter asserts the same at
    /// `PageController` construction).
    #[must_use]
    pub fn with_params(initial_page: usize, viewport_fraction: f32) -> Self {
        assert!(
            viewport_fraction > 0.0,
            "PageController viewport_fraction must be > 0.0 (got {viewport_fraction})"
        );
        let scroll = ScrollController::new();
        scroll
            .position()
            .set_dimension_policy(DimensionChangePolicy::KeepFractionalPage {
                viewport_fraction,
                initial_page: Some(initial_page as f32),
            });
        Self {
            scroll,
            initial_page,
            viewport_fraction,
        }
    }

    /// The page shown when the controlled [`PageView`] is first laid out.
    #[must_use]
    pub fn initial_page(&self) -> usize {
        self.initial_page
    }

    /// The fraction of the viewport each page occupies.
    #[must_use]
    pub fn viewport_fraction(&self) -> f32 {
        self.viewport_fraction
    }

    /// The current fractional page, or `None` before the controlled
    /// [`PageView`] has completed its first layout.
    ///
    /// # Flutter parity
    ///
    /// Uses the guarded [`ScrollMetrics::page`] formula (`PageMetrics.page`),
    /// not the internal recompute `apply_viewport_dimension` drives — see
    /// that method's docs. FLUI's `ScrollPosition` always "has pixels" (no
    /// `hasPixels == false` state to mirror Flutter's pre-attach `null`), so
    /// [`ScrollPosition::has_applied_viewport_dimension`] is the substitute
    /// "not yet answerable" signal instead.
    #[must_use]
    pub fn page(&self) -> Option<f32> {
        let position = self.scroll.position();
        if !position.has_applied_viewport_dimension() {
            return None;
        }
        let metrics = ScrollMetrics::from(&position);
        Some(metrics.page(self.viewport_fraction))
    }

    /// Jumps to `page` without animation.
    ///
    /// # Flutter parity
    ///
    /// Mirrors `jumpToPage`'s three-way branch (`widgets/page_view.dart`, tag
    /// `3.44.0`):
    /// - **Currently collapsed** (a real dimension was established at least
    ///   once, but the viewport currently reads `0.0`) — overwrites the
    ///   cached page directly (`_cachedPage = page`), so a page jump
    ///   requested while temporarily hidden takes effect the moment the
    ///   viewport regains a real dimension.
    /// - **Never established** — updates the pending startup page (mirrors
    ///   `_pageToUseOnStartup`), same as before any layout has run.
    /// - **Real, established dimension** — jumps directly, unclamped,
    ///   matching `jumpTo`'s "without checking if the new value is in range"
    ///   contract.
    pub fn jump_to_page(&self, page: usize) {
        let page_f = page as f32;
        let mut position = self.scroll.position();
        if position.set_cached_page_while_collapsed(page_f) {
            return;
        }
        if position.has_applied_viewport_dimension() {
            let metrics = ScrollMetrics::from(&position);
            let pixels = metrics.pixels_from_page(self.viewport_fraction, page_f);
            position.jump_to(pixels);
        } else {
            position.set_dimension_policy(DimensionChangePolicy::KeepFractionalPage {
                viewport_fraction: self.viewport_fraction,
                initial_page: Some(page_f),
            });
        }
    }

    /// The shared [`ScrollPosition`] backing this controller.
    #[must_use]
    pub fn position(&self) -> ScrollPosition {
        self.scroll.position()
    }

    /// The underlying [`ScrollController`], for wiring into a [`Scrollable`].
    #[must_use]
    pub fn scroll_controller(&self) -> ScrollController {
        self.scroll.clone()
    }

    /// An `Arc<dyn Listenable>` pointing at the same shared position.
    #[must_use]
    pub fn as_listenable(&self) -> Arc<dyn Listenable> {
        self.scroll.as_listenable()
    }
}

// ============================================================================
// PageView (configuration)
// ============================================================================

/// A callback fired when the displayed page changes.
type OnPageChanged = Arc<dyn Fn(usize) + Send + Sync>;

/// A scrollable list that works page by page.
///
/// Each child fills [`PageController::viewport_fraction`] of the viewport
/// along [`PageView::scroll_direction`]. `PageView` is pure composition over
/// [`Scrollable`] + [`PageScrollPhysics`] + [`Viewport`] +
/// [`SliverFillViewport`] — no new render objects.
///
/// # Flutter parity
///
/// Mirrors `PageView` (`widgets/page_view.dart`, tag `3.44.0`). See the
/// module docs for the documented v1 divergences (eager children,
/// listener-based `on_page_changed`, no `pageSnapping: false`/`reverse`/
/// `padEnds`).
#[derive(Clone, StatefulView)]
pub struct PageView {
    controller: PageController,
    scroll_direction: Axis,
    on_page_changed: Option<OnPageChanged>,
    cache_extent: Option<(f32, CacheExtentStyle)>,
    children: Vec<BoxedView>,
}

impl PageView {
    /// A horizontally-scrolling page view over `children` (Flutter's default
    /// `scrollDirection: Axis.horizontal`), with a fresh default
    /// [`PageController`].
    pub fn new(children: impl ViewSeq) -> Self {
        Self {
            controller: PageController::new(),
            scroll_direction: Axis::Horizontal,
            on_page_changed: None,
            cache_extent: None,
            children: children.into_boxed_vec(),
        }
    }

    /// Attach a [`PageController`] (position + page navigation). Multiple
    /// clones of the same controller share state.
    #[must_use]
    pub fn controller(mut self, controller: PageController) -> Self {
        self.controller = controller;
        self
    }

    /// The scroll axis (default [`Axis::Horizontal`]).
    #[must_use]
    pub fn scroll_direction(mut self, axis: Axis) -> Self {
        self.scroll_direction = axis;
        self
    }

    /// Called whenever the page in the center of the viewport changes —
    /// fires when `round(page)` differs from the last reported page. See the
    /// module docs for how this diverges from Flutter's
    /// `NotificationListener<ScrollNotification>`-based implementation.
    #[must_use]
    pub fn on_page_changed(mut self, callback: impl Fn(usize) + Send + Sync + 'static) -> Self {
        self.on_page_changed = Some(Arc::new(callback));
        self
    }

    /// Set how far beyond the visible page(s) to keep neighboring pages laid
    /// out and painted ([`Viewport::cache_extent`] passthrough).
    #[must_use]
    pub fn cache_extent(mut self, cache_extent: f32, style: CacheExtentStyle) -> Self {
        self.cache_extent = Some((cache_extent, style));
        self
    }
}

impl std::fmt::Debug for PageView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PageView")
            .field("controller", &self.controller)
            .field("scroll_direction", &self.scroll_direction)
            .field("has_on_page_changed", &self.on_page_changed.is_some())
            .field("cache_extent", &self.cache_extent)
            .field("children", &self.children.len())
            .finish_non_exhaustive()
    }
}

// ============================================================================
// State
// ============================================================================

/// Persistent state for [`PageView`].
///
/// Owns the `round(page)`-change listener registered on the controller's
/// position in [`init_state`](ViewState::init_state) and removed in
/// [`dispose`](ViewState::dispose).
pub struct PageViewState {
    controller: PageController,
    on_page_changed: Option<OnPageChanged>,
    last_reported_page: Arc<AtomicI64>,
    page_listener_id: Option<ListenerId>,
}

impl std::fmt::Debug for PageViewState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PageViewState")
            .field("controller", &self.controller)
            .field("has_on_page_changed", &self.on_page_changed.is_some())
            .field(
                "last_reported_page",
                &self.last_reported_page.load(Ordering::Relaxed),
            )
            .finish_non_exhaustive()
    }
}

impl StatefulView for PageView {
    type State = PageViewState;

    fn create_state(&self) -> Self::State {
        PageViewState {
            controller: self.controller.clone(),
            on_page_changed: self.on_page_changed.clone(),
            last_reported_page: Arc::new(AtomicI64::new(self.controller.initial_page() as i64)),
            page_listener_id: None,
        }
    }
}

impl ViewState<PageView> for PageViewState {
    fn init_state(&mut self, _ctx: &dyn BuildContext) {
        let Some(on_page_changed) = self.on_page_changed.clone() else {
            return;
        };
        let position = self.controller.position();
        let viewport_fraction = self.controller.viewport_fraction();
        let last_reported = Arc::clone(&self.last_reported_page);

        let listener_id = self
            .controller
            .as_listenable()
            .add_listener(Arc::new(move || {
                if !position.has_applied_viewport_dimension() {
                    return;
                }
                let metrics = ScrollMetrics::from(&position);
                // The guarded `page` formula clamps its numerator to >= 0.0
                // before dividing, so `round()` never yields a negative value —
                // the `.max(0.0)` here is belt-and-suspenders against a
                // negative-zero rounding artifact, not a real overflow guard.
                let current_page = metrics.page(viewport_fraction).round().max(0.0) as i64;
                if current_page != last_reported.load(Ordering::SeqCst) {
                    last_reported.store(current_page, Ordering::SeqCst);
                    on_page_changed(current_page as usize);
                }
            }));
        self.page_listener_id = Some(listener_id);
    }

    fn build(&self, view: &PageView, _ctx: &dyn BuildContext) -> impl IntoView {
        let controller = self.controller.clone();
        let viewport_fraction = controller.viewport_fraction();
        let scroll_direction = view.scroll_direction;
        let axis_direction = match scroll_direction {
            Axis::Horizontal => AxisDirection::LeftToRight,
            Axis::Vertical => AxisDirection::TopToBottom,
        };
        let children = view.children.clone();
        let cache_extent = view.cache_extent;
        let physics: SharedScrollPhysics = Arc::new(PageScrollPhysics::new(viewport_fraction));

        Scrollable::new()
            .controller(controller.scroll_controller())
            .physics(physics)
            .scroll_direction(scroll_direction)
            .viewport_builder(Rc::new(move |position: ScrollPosition| {
                let sliver = SliverFillViewport::new(viewport_fraction, children.clone());
                let mut viewport = Viewport::new((sliver,))
                    .axis_direction(axis_direction)
                    .position(position);
                if let Some((extent, style)) = cache_extent {
                    viewport = viewport.cache_extent(extent, style);
                }
                viewport.boxed()
            }))
            .boxed()
    }

    fn did_update_view(&mut self, _old_view: &PageView, new_view: &PageView) {
        // Track the current controller/callback so a parent rebuild handing
        // a new configuration is reflected — same limitation as
        // `ScrollableState::did_update_view`: a controller SWAP (not a
        // mutation of the same shared controller) does not re-register the
        // page-changed listener, which stays bound to whatever controller
        // `init_state` saw.
        self.controller = new_view.controller.clone();
        self.on_page_changed.clone_from(&new_view.on_page_changed);
    }

    fn dispose(&mut self) {
        if let Some(id) = self.page_listener_id.take() {
            self.controller.as_listenable().remove_listener(id);
        }
    }
}

// ============================================================================
// Tests
// ============================================================================
//
// `PageScrollPhysics::create_ballistic_simulation` is tested here at the pure
// function level (metrics + velocity in, a `Simulation` out) rather than only
// through a gesture-driven `PageView` in the `parity` integration corpus.
// Reason: this crate's headless test harness dispatches pointer events with
// real `Instant::now()` timestamps advanced by only a minimal OS-timer tick
// (`advance_gesture_clock`), so a synthetic drag's *measured* release
// velocity is enormous — `Scrollable::on_pan_end` clamps it to Flutter's
// `kMaxFlingVelocity` (±8,000 px/s), still far above
// `PageScrollPhysics::velocity_tolerance_px_per_sec` (20.0). A gesture-driven
// settle test therefore can't isolate "distance-only rounding" from
// "velocity-biased" behavior deterministically — exactly the halfway-vs-fling
// distinction these tests exist to pin. The `parity` corpus still proves the
// full gesture → physics → spring → settle pipeline wires up correctly (see
// `a_full_drag_and_release_settles_the_page_view_through_the_real_gesture_and_spring_pipeline`
// there) — it just doesn't isolate the halfway threshold, which needs exact
// control over velocity that only a direct physics call can provide.
#[cfg(test)]
mod tests {
    use super::*;

    /// A 300px-per-page metrics snapshot (`viewport_fraction: 1.0`) at
    /// `pixels`, with `min_scroll_extent: 0.0` and a generous
    /// `max_scroll_extent` so boundary short-circuiting never triggers.
    fn metrics_at(pixels: f32) -> ScrollMetrics {
        ScrollMetrics::new(pixels, 0.0, 3000.0, 300.0)
    }

    /// A spring simulation's value 2 simulated seconds in — long enough for
    /// `PageScrollPhysics`'s default spring (`with_damping_ratio(0.5, 100.0,
    /// 1.1)`, overdamped) to settle within a fraction of a pixel of its
    /// target. Mirrors the settle-tolerance pattern
    /// `bouncing_physics_top_overscroll_springs_back_to_min_extent`
    /// (`tests/parity/scrollable_test.rs`) uses via repeated `pump_for` — here
    /// evaluated directly against the `Simulation`, with no gesture-harness
    /// timing noise to isolate.
    fn settled_x(sim: &dyn Simulation) -> f32 {
        sim.x(2.0)
    }

    /// Oracle: `PageScrollPhysics._getTargetPixels`/`createBallisticSimulation`
    /// (`widgets/page_view.dart`, tag `3.44.0`) — at velocity `0.0` (within
    /// `velocity_tolerance_px_per_sec`), a position more than half a page past
    /// the current page boundary settles FORWARD to the next page.
    /// Cross-checked against `'Page changes at halfway point'`
    /// (`test/widgets/page_view_test.dart`), whose 800px-wide viewport crosses
    /// its halfway mark at a -420px drag (`-380` short of half, `-420` past
    /// it) — the same "more than half" threshold this test pins at a 300px
    /// page (`pixels: 160.0`, page `0.533`).
    #[test]
    fn settles_forward_past_the_halfway_point_at_zero_velocity() {
        let physics = PageScrollPhysics::new(1.0);
        let metrics = metrics_at(160.0); // page = 160 / 300 = 0.533 (> 0.5)
        let sim = physics
            .create_ballistic_simulation(&metrics, 0.0)
            .expect("more than half a page off-boundary must produce a settle simulation");
        assert!(
            (settled_x(&sim) - 300.0).abs() < 1.0,
            "a position past the halfway point at zero velocity must settle FORWARD to \
             the next page (300.0), got {:.2}",
            settled_x(&sim)
        );
    }

    /// The converse of the above: less than half a page past the current
    /// boundary, at zero velocity, settles BACKWARD to the current page —
    /// the same halfway threshold, approached from below.
    #[test]
    fn settles_backward_below_the_halfway_point_at_zero_velocity() {
        let physics = PageScrollPhysics::new(1.0);
        let metrics = metrics_at(100.0); // page = 100 / 300 = 0.333 (< 0.5)
        let sim = physics
            .create_ballistic_simulation(&metrics, 0.0)
            .expect("a nonzero off-boundary position must produce a settle simulation");
        assert!(
            (settled_x(&sim) - 0.0).abs() < 1.0,
            "a position below the halfway point at zero velocity must settle BACK to \
             the current page (0.0), got {:.2}",
            settled_x(&sim)
        );
    }

    /// Oracle: `_getTargetPixels`'s velocity bias — `velocity >
    /// tolerance.velocity` adds `0.5` to the page BEFORE rounding, so a fling
    /// above the tolerance commits to the NEXT page even from very close to
    /// the current page's start (`page: 0.1`, nowhere near the 0.5 halfway
    /// mark this same physics uses at rest — see the two tests above).
    #[test]
    fn fling_velocity_beyond_tolerance_advances_regardless_of_distance() {
        let physics = PageScrollPhysics::new(1.0);
        let metrics = metrics_at(30.0); // page = 30 / 300 = 0.1 (far from 0.5)
        let velocity = physics.velocity_tolerance_px_per_sec + 10.0; // above tolerance
        let sim = physics
            .create_ballistic_simulation(&metrics, velocity)
            .expect("a velocity above tolerance must produce a settle simulation");
        assert!(
            (settled_x(&sim) - 300.0).abs() < 1.0,
            "velocity above tolerance must advance to the next page (300.0) despite \
             sitting at only page 0.1, got {:.2}",
            settled_x(&sim)
        );
    }

    /// Symmetric to the above: a BACKWARD fling above tolerance commits back
    /// to the current (lower) page even from close to the NEXT page's start.
    #[test]
    fn backward_fling_velocity_beyond_tolerance_retreats_regardless_of_distance() {
        let physics = PageScrollPhysics::new(1.0);
        let metrics = metrics_at(320.0); // page = 320 / 300 = 1.067 (just past page 1)
        let velocity = -(physics.velocity_tolerance_px_per_sec + 10.0); // above tolerance, backward
        let sim = physics
            .create_ballistic_simulation(&metrics, velocity)
            .expect("a velocity above tolerance must produce a settle simulation");
        assert!(
            (settled_x(&sim) - 300.0).abs() < 1.0,
            "a backward fling above tolerance must retreat to page 1 (300.0) despite \
             sitting at page 1.067, got {:.2}",
            settled_x(&sim)
        );
    }

    /// Oracle: out-of-range and not headed back in defers entirely to the
    /// boundary physics (`super.createBallisticSimulation`'s `parent` chain).
    /// `ClampingScrollPhysics` (the default boundary) returns `None` below its
    /// own fling threshold, so a slow drag that overshot `max_scroll_extent`
    /// produces no page-snap simulation at all.
    #[test]
    fn out_of_range_heading_further_out_of_range_defers_to_the_boundary_physics() {
        let physics = PageScrollPhysics::new(1.0);
        let metrics = ScrollMetrics::new(3100.0, 0.0, 3000.0, 300.0); // past max_scroll_extent
        let sim = physics.create_ballistic_simulation(&metrics, 10.0); // below fling threshold, heading further out
        assert!(
            sim.is_none(),
            "out-of-range with no boundary fling must defer to ClampingScrollPhysics, \
             which returns None below its threshold"
        );
    }

    /// `PageController::with_params` rejects a non-positive `viewport_fraction`
    /// — mirrors `PageController`'s constructor assert (`assert(viewportFraction
    /// > 0.0)`).
    #[test]
    #[should_panic(expected = "viewport_fraction must be > 0.0")]
    fn page_controller_rejects_a_non_positive_viewport_fraction() {
        let _ = PageController::with_params(0, 0.0);
    }

    /// `PageController::page` returns `None` before any layout has committed
    /// a real viewport dimension — FLUI's substitute for Flutter's
    /// `'PageController cannot return page while unattached'` assertion
    /// (`test/widgets/page_view_test.dart`): a documented divergence returning
    /// `Option::None` instead of panicking.
    #[test]
    fn page_returns_none_before_any_layout() {
        let controller = PageController::with_params(2, 1.0);
        assert_eq!(
            controller.page(),
            None,
            "page() must return None before apply_viewport_dimension has ever run"
        );
    }
}
