//! Scroll-physics strategies — the `ScrollPhysics` trait plus two standard
//! implementations: `ClampingScrollPhysics` (Android-style hard clamp) and
//! `BouncingScrollPhysics` (iOS-style overscroll + spring-back).
//!
//! # Contract
//!
//! - `apply_boundary_conditions` returns the _allowed_ position rather than the
//!   rejected overshoot, which is the simpler contract for purely-eager
//!   callbacks; it takes a `ScrollMetrics` snapshot.
//! - `create_ballistic_simulation` returns `Option<Box<dyn Simulation>>` for the
//!   fling/spring-back animation.
//!
//! # Deferred (v1)
//!
//! - `BouncingScrollPhysics.create_ballistic_simulation` creates the spring
//!   simulation but the caller is responsible for driving (ticking) it —
//!   `Scrollable.on_pan_end` notes this explicitly.
//! - Parent-physics chaining (`ScrollPhysics.parent`) — not yet wired.

use std::sync::Arc;

use flui_animation::simulation::{
    BoundedFrictionSimulation, ScrollSpringSimulation, Simulation, SpringDescription,
};
use flui_rendering::view::ScrollPosition;

// ---------------------------------------------------------------------------
// ScrollMetrics
// ---------------------------------------------------------------------------

/// A point-in-time snapshot of a scroll position's extents — the value a
/// [`ScrollPhysics`] method reads to decide a boundary/ballistic outcome.
///
/// This is a snapshot **value object**, not a live view: it is read once at
/// the call site and passed by reference into the physics call, so the
/// physics implementation always sees a self-consistent set of fields even
/// if the underlying [`ScrollPosition`] is mutated concurrently afterward.
///
/// It is a plain `Copy` struct rather than a live interface, since
/// `ScrollPhysics` never needs to observe further extent changes mid-call: the
/// given metrics are only valid during the physics method call.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollMetrics {
    /// Current scroll offset in logical pixels.
    pub pixels: f64,
    /// The smallest in-range value for `pixels`.
    pub min_scroll_extent: f64,
    /// The largest in-range value for `pixels`.
    pub max_scroll_extent: f64,
    /// The viewport's length along the scroll axis.
    pub viewport_dimension: f64,
}

impl ScrollMetrics {
    /// Builds a metrics snapshot directly from its four fields — for a test
    /// fixture or a caller assembling metrics from values that don't come
    /// from a live [`ScrollPosition`] (e.g. a hypothetical "what if" probe).
    /// Prefer [`ScrollMetrics::from`] when a [`ScrollPosition`] is at hand.
    ///
    /// All four fields are required: a caller that only cares about
    /// `pixels`/`min_scroll_extent`/`max_scroll_extent` still must pass an
    /// explicit `viewport_dimension` (even if `0.0`) rather than have it
    /// silently defaulted — a physics implementation that reads
    /// `viewport_dimension` (e.g. a future page-snapping physics) must not
    /// get `0.0` from every fixture that forgot to set it.
    #[must_use]
    pub fn new(
        pixels: f64,
        min_scroll_extent: f64,
        max_scroll_extent: f64,
        viewport_dimension: f64,
    ) -> Self {
        Self {
            pixels,
            min_scroll_extent,
            max_scroll_extent,
            viewport_dimension,
        }
    }

    /// The current fractional "page" at `viewport_fraction`, defensively
    /// guarded to be callable at any time (including before real content
    /// dimensions exist).
    ///
    /// Computed as `max(0.0, clamp(pixels, min, max)) / max(1.0,
    /// viewport_dimension * viewport_fraction)`. This is the *public*, defensively-guarded
    /// formula — distinct from the internal recompute
    /// `ScrollPosition::apply_viewport_dimension` drives — used by both
    /// `PageController::page` and `PageScrollPhysics` (`page_view.rs`) so the
    /// two agree on exactly what "the current page" means.
    ///
    /// This snapshot alone never special-cases a collapsed
    /// (`viewport_dimension == 0.0`) viewport's cached page
    /// (`DimensionChangePolicy::KeepFractionalPage`'s private `cached_page`,
    /// which `ScrollMetrics` has no access to) — it always divides
    /// `pixels`/`viewport_dimension` as written above. `PageController::page`
    /// (`page_view.rs`) is the one that consults the cached page first via
    /// `ScrollPosition::cached_page`, falling back to this formula only when
    /// the viewport isn't currently collapsed.
    #[must_use]
    pub fn page(&self, viewport_fraction: f64) -> f64 {
        let clamped = self
            .pixels
            .clamp(self.min_scroll_extent, self.max_scroll_extent);
        clamped.max(0.0) / (self.viewport_dimension * viewport_fraction).max(1.0)
    }

    /// The inverse of [`page`](Self::page): the pixel offset for `page` at
    /// `viewport_fraction`.
    ///
    /// Computed as `page * viewport_dimension * viewport_fraction`. Unlike
    /// [`page`](Self::page), this has no `max(1.0, ...)` guard — it is a
    /// forward computation, not a division, so there is no zero-denominator
    /// hazard to guard against.
    #[must_use]
    pub fn pixels_from_page(&self, viewport_fraction: f64, page: f64) -> f64 {
        page * self.viewport_dimension * viewport_fraction
    }
}

impl From<&ScrollPosition> for ScrollMetrics {
    /// Snapshots `position`'s four extent fields in a single lock
    /// acquisition (via [`ScrollPosition::extents_snapshot`]) rather than
    /// four separate reads that could observe a torn state if another
    /// thread mutated the position in between.
    fn from(position: &ScrollPosition) -> Self {
        let snapshot = position.extents_snapshot();
        Self {
            pixels: snapshot.pixels,
            min_scroll_extent: snapshot.min_scroll_extent,
            max_scroll_extent: snapshot.max_scroll_extent,
            viewport_dimension: snapshot.viewport_dimension,
        }
    }
}

// ---------------------------------------------------------------------------
// Trait
// ---------------------------------------------------------------------------

/// Decides what happens at the scroll edges and after a fling gesture ends.
///
/// Implement this trait to provide custom boundary clamping and ballistic
/// (fling / spring-back) behaviour for a [`Scrollable`](super::Scrollable).
///
/// `apply_boundary_conditions` returns the _allowed position_ (not the rejected
/// overshoot), which is ergonomically simpler for a callback-driven update
/// model.
pub trait ScrollPhysics: Send + Sync + std::fmt::Debug {
    /// Return the position the scroller should move to given a `proposed_pixels`
    /// offset in the context of `metrics`.
    ///
    /// For clamping physics this clips `proposed_pixels` to
    /// `[metrics.min_scroll_extent, metrics.max_scroll_extent]`. For bouncing
    /// physics a position past the edge is partially allowed with increasing
    /// resistance.
    ///
    /// The return value is the _accepted_ position, not the rejected overshoot.
    fn apply_boundary_conditions(&self, metrics: &ScrollMetrics, proposed_pixels: f64) -> f64;

    /// Create a `Simulation` that coasts the viewport to rest after the user
    /// lifts their finger.
    ///
    /// Returns `None` when the velocity is below the minimum fling threshold
    /// (or when the scroll is already at rest at a valid position), so the
    /// caller can skip animation entirely.
    ///
    /// The simulation positions are in logical pixels, matching
    /// `ScrollController.pixels()`. The caller is responsible for advancing
    /// (ticking) the simulation; see the `DEFERRED` note in
    /// `Scrollable::on_pan_end`.
    fn create_ballistic_simulation(
        &self,
        metrics: &ScrollMetrics,
        velocity_px_per_sec: f64,
    ) -> Option<Box<dyn Simulation>>;
}

/// Shared, type-erased physics handle.
///
/// Cloning the `Arc` is cheap; the physics object itself is stateless.
pub type SharedScrollPhysics = Arc<dyn ScrollPhysics>;

// ---------------------------------------------------------------------------
// ClampingScrollPhysics — Android-style hard clamp
// ---------------------------------------------------------------------------

/// Hard-clamps scroll position to `[min, max]`.
///
/// Scroll cannot go past the content edge; the boundary snaps instantly and
/// post-fling coast is bounded so the final position lands within range.
#[derive(Debug, Clone, Copy)]
pub struct ClampingScrollPhysics {
    /// Below this absolute velocity (logical px / s) no fling is started.
    ///
    /// Default is ~50 px/s; 0 px/s disables the threshold (always
    /// fling). Kept as a field rather than a constant so callers can tune it
    /// without a full custom implementation.
    pub min_fling_velocity_px_per_sec: f64,
    /// Friction drag coefficient for the fling deceleration. Must be in `(0,
    /// 1)`. The default is approximately `0.135` in
    /// `BoundedFrictionSimulation`.
    pub fling_drag_coefficient: f64,
}

impl ClampingScrollPhysics {
    /// Default Android-matching physics (drag ≈ 0.135, min-fling ≈ 50 px/s).
    #[must_use]
    pub fn new() -> Self {
        Self {
            min_fling_velocity_px_per_sec: 50.0,
            fling_drag_coefficient: 0.135,
        }
    }
}

impl Default for ClampingScrollPhysics {
    /// Returns the same values as [`ClampingScrollPhysics::new`] — notably
    /// `fling_drag_coefficient = 0.135`, which `#[derive(Default)]` cannot
    /// provide (it would zero-initialize every field, and a drag coefficient of
    /// 0 is invalid for `BoundedFrictionSimulation`).
    fn default() -> Self {
        Self::new()
    }
}

impl ScrollPhysics for ClampingScrollPhysics {
    fn apply_boundary_conditions(&self, metrics: &ScrollMetrics, proposed_pixels: f64) -> f64 {
        proposed_pixels.clamp(metrics.min_scroll_extent, metrics.max_scroll_extent)
    }

    fn create_ballistic_simulation(
        &self,
        metrics: &ScrollMetrics,
        velocity_px_per_sec: f64,
    ) -> Option<Box<dyn Simulation>> {
        // Skip fling below the configured threshold.
        if velocity_px_per_sec.abs() < self.min_fling_velocity_px_per_sec {
            return None;
        }
        // If the position is already out of bounds (possible if the caller
        // skipped boundary conditions), do not attempt a physics fling — let
        // the caller snap it into range first.
        if metrics.pixels < metrics.min_scroll_extent || metrics.pixels > metrics.max_scroll_extent
        {
            return None;
        }
        Some(Box::new(BoundedFrictionSimulation::new(
            self.fling_drag_coefficient,
            metrics.pixels,
            velocity_px_per_sec,
            metrics.min_scroll_extent,
            metrics.max_scroll_extent,
        )))
    }
}

// ---------------------------------------------------------------------------
// BouncingScrollPhysics — iOS-style overscroll + spring-back
// ---------------------------------------------------------------------------

/// Resists outward motion past the content edge, then springs back to the
/// boundary on release. Motion toward the edge is unrestricted.
///
/// During a drag, positions past `[min, max]` are allowed but dampened by the
/// `overscroll_spring_coefficient` (default 0.52). On release, a
/// `ScrollSpringSimulation` returns the position to the nearest valid edge.
#[derive(Debug, Clone, Copy)]
pub struct BouncingScrollPhysics {
    /// Resistance applied when dragging past the edge, default 0.52.
    /// Range `(0, 1)`: smaller = stiffer.
    pub overscroll_spring_coefficient: f64,
    /// Spring configuration used for the snap-back animation. The default is
    /// `SpringDescription::with_damping_ratio(1.0, 500.0, 0.75)` (the "bouncy"
    /// preset).
    pub spring: SpringDescription,
    /// Below this absolute velocity (px/s) no fling is started.
    pub min_fling_velocity_px_per_sec: f64,
    /// Friction drag coefficient for in-bounds fling deceleration.
    pub fling_drag_coefficient: f64,
}

impl BouncingScrollPhysics {
    /// Default iOS-matching physics.
    #[must_use]
    pub fn new() -> Self {
        Self {
            overscroll_spring_coefficient: 0.52,
            spring: SpringDescription::with_damping_ratio(1.0, 500.0, 0.75),
            min_fling_velocity_px_per_sec: 50.0,
            fling_drag_coefficient: 0.135,
        }
    }
}

impl Default for BouncingScrollPhysics {
    fn default() -> Self {
        Self::new()
    }
}

impl ScrollPhysics for BouncingScrollPhysics {
    fn apply_boundary_conditions(&self, metrics: &ScrollMetrics, proposed_pixels: f64) -> f64 {
        let anchor = if proposed_pixels < metrics.min_scroll_extent {
            metrics.pixels.min(metrics.min_scroll_extent)
        } else if proposed_pixels > metrics.max_scroll_extent {
            metrics.pixels.max(metrics.max_scroll_extent)
        } else {
            return proposed_pixels;
        };
        // Resist only the additional motion away from the edge. Reapplying
        // resistance to the total overshoot reverses small outward drags.
        if (proposed_pixels < metrics.min_scroll_extent && proposed_pixels < anchor)
            || (proposed_pixels > metrics.max_scroll_extent && proposed_pixels > anchor)
        {
            anchor + (proposed_pixels - anchor) * self.overscroll_spring_coefficient
        } else {
            proposed_pixels
        }
    }

    fn create_ballistic_simulation(
        &self,
        metrics: &ScrollMetrics,
        velocity_px_per_sec: f64,
    ) -> Option<Box<dyn Simulation>> {
        // If the position is past an edge, spring back regardless of velocity.
        if metrics.pixels < metrics.min_scroll_extent {
            return Some(Box::new(ScrollSpringSimulation::new(
                self.spring,
                metrics.pixels,
                metrics.min_scroll_extent,
                velocity_px_per_sec,
            )));
        }
        if metrics.pixels > metrics.max_scroll_extent {
            return Some(Box::new(ScrollSpringSimulation::new(
                self.spring,
                metrics.pixels,
                metrics.max_scroll_extent,
                velocity_px_per_sec,
            )));
        }
        // Within bounds: fling if velocity is above the threshold.
        if velocity_px_per_sec.abs() < self.min_fling_velocity_px_per_sec {
            return None;
        }
        Some(Box::new(BoundedFrictionSimulation::new(
            self.fling_drag_coefficient,
            metrics.pixels,
            velocity_px_per_sec,
            metrics.min_scroll_extent,
            metrics.max_scroll_extent,
        )))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
