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
//!   `Scrollable`'s drag `on_end` notes this explicitly.
//! - Parent-physics chaining (`ScrollPhysics.parent`) — not yet wired.

use std::sync::Arc;

use flui_animation::simulation::{
    BouncingScrollSimulation, BoundedFrictionSimulation, FrictionSimulation, Simulation,
    SimulationBounds, SpringDescription, SpringSimulation, Tolerance,
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
    /// Device pixels per logical pixel where the scroll is presented. A
    /// ballistic simulation rests within half a device pixel. `1.0` unless
    /// the caller knows the presentation's ratio
    /// ([`with_device_pixel_ratio`](Self::with_device_pixel_ratio)).
    pub device_pixel_ratio: f64,
}

impl ScrollMetrics {
    /// Builds a metrics snapshot at a device pixel ratio of `1.0` from its
    /// four extent fields — for a test
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
            device_pixel_ratio: 1.0,
        }
    }

    /// This snapshot, presented at `device_pixel_ratio` device pixels per
    /// logical pixel.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_widgets::ScrollMetrics;
    ///
    /// let metrics = ScrollMetrics::new(0.0, 0.0, 500.0, 300.0).with_device_pixel_ratio(2.0);
    /// assert_eq!(metrics.device_pixel_ratio, 2.0);
    /// ```
    #[must_use]
    pub fn with_device_pixel_ratio(self, device_pixel_ratio: f64) -> Self {
        Self {
            device_pixel_ratio,
            ..self
        }
    }

    /// The rest tolerance for a ballistic run under these metrics (ADR-0155): half a
    /// device pixel. `None` for a ratio that is not finite and positive.
    pub(crate) fn ballistic_tolerance(&self) -> Option<Tolerance> {
        Tolerance::for_device_pixel_ratio(self.device_pixel_ratio).ok()
    }

    /// How far off a target, in logical pixels, still counts as on it:
    /// [`rest_slop`] at these metrics' ratio.
    pub(crate) fn rest_slop(&self) -> f64 {
        rest_slop(self.device_pixel_ratio)
    }

    /// The scroll range, or `None` when the extents are unordered or not
    /// finite.
    pub(crate) fn bounds(&self) -> Option<SimulationBounds> {
        SimulationBounds::new(self.min_scroll_extent, self.max_scroll_extent).ok()
    }

    /// The extents as `(min, max)` when `min <= max`, an infinite end
    /// included; `None` for unordered extents or a NaN one, which nothing
    /// can be clamped to.
    pub(crate) fn ordered_extents(&self) -> Option<(f64, f64)> {
        (self.min_scroll_extent <= self.max_scroll_extent)
            .then_some((self.min_scroll_extent, self.max_scroll_extent))
    }

    /// The extent `pixels` lies past, if it lies outside the range.
    fn edge_past(&self) -> Option<f64> {
        if self.pixels < self.min_scroll_extent {
            Some(self.min_scroll_extent)
        } else if self.pixels > self.max_scroll_extent {
            Some(self.max_scroll_extent)
        } else {
            None
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
    /// `PageController::page` and `PageSnap` (`nested/snap.rs`) so the
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
    ///
    /// Extents that are unordered or NaN clamp nothing: the page is read
    /// from `pixels` as it is.
    #[must_use]
    pub fn page(&self, viewport_fraction: f64) -> f64 {
        let clamped = self
            .ordered_extents()
            .map_or(self.pixels, |(min, max)| self.pixels.clamp(min, max));
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

/// Half a device pixel at `device_pixel_ratio`, in logical pixels: the slop a
/// ballistic run rests within (ADR-0155), so whatever came to rest a hair off
/// a target counts as on it. Half a logical pixel for a ratio that is not
/// finite and positive.
pub(crate) fn rest_slop(device_pixel_ratio: f64) -> f64 {
    if device_pixel_ratio.is_finite() && device_pixel_ratio > 0.0 {
        0.5 / device_pixel_ratio
    } else {
        0.5
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
            device_pixel_ratio: 1.0,
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
    /// the drag `on_end` in `Scrollable`.
    fn create_ballistic_simulation(
        &self,
        metrics: &ScrollMetrics,
        velocity_px_per_sec: f64,
    ) -> Option<Box<dyn Simulation>>;

    /// Remaining logical-pixel velocity when this ballistic motion reaches a
    /// hard content boundary, for transfer to an enclosing scrollable.
    ///
    /// The default keeps the motion local. Physics that overscrolls or springs
    /// back should retain that default; a hard-boundary strategy can opt in.
    fn boundary_velocity(
        &self,
        _metrics: &ScrollMetrics,
        _velocity_px_per_sec: f64,
    ) -> Option<f64> {
        None
    }
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
/// post-fling coast is bounded so the final position lands within range. A
/// release from outside the range (the content shrank under a run, or the
/// offset was written past an edge) springs back to the nearest extent on a
/// critically damped spring, whatever its velocity.
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
    /// `proposed_pixels` clamped to the extents; unchanged when the extents
    /// are unordered or NaN, which bound nothing.
    fn apply_boundary_conditions(&self, metrics: &ScrollMetrics, proposed_pixels: f64) -> f64 {
        metrics
            .ordered_extents()
            .map_or(proposed_pixels, |(min, max)| {
                proposed_pixels.clamp(min, max)
            })
    }

    fn create_ballistic_simulation(
        &self,
        metrics: &ScrollMetrics,
        velocity_px_per_sec: f64,
    ) -> Option<Box<dyn Simulation>> {
        // Unordered extents or an invalid ratio: no run.
        let bounds = metrics.bounds()?;
        let tolerance = metrics.ballistic_tolerance()?;
        // Past an edge: back to it at any velocity, critically damped.
        if let Some(edge) = metrics.edge_past() {
            let spring = SpringDescription::with_damping_ratio(1.0, 500.0, 1.0)
                .expect("BUG: the clamping return spring is in the admitted domain");
            let simulation =
                SpringSimulation::new(spring, metrics.pixels, edge, velocity_px_per_sec, tolerance)
                    .ok()?;
            return Some(Box::new(simulation));
        }
        // Skip fling below the configured threshold.
        if velocity_px_per_sec.abs() < self.min_fling_velocity_px_per_sec {
            return None;
        }
        // A NaN position or an invalid drag: no fling.
        let simulation = BoundedFrictionSimulation::new(
            self.fling_drag_coefficient,
            metrics.pixels,
            velocity_px_per_sec,
            bounds,
            tolerance,
        )
        .ok()?;
        Some(Box::new(simulation))
    }

    fn boundary_velocity(&self, metrics: &ScrollMetrics, velocity_px_per_sec: f64) -> Option<f64> {
        if velocity_px_per_sec.abs() < self.min_fling_velocity_px_per_sec
            || metrics.pixels < metrics.min_scroll_extent
            || metrics.pixels > metrics.max_scroll_extent
        {
            return None;
        }
        metrics.bounds()?;
        let simulation = FrictionSimulation::new(
            self.fling_drag_coefficient,
            metrics.pixels,
            velocity_px_per_sec,
            metrics.ballistic_tolerance()?,
        )
        .ok()?;
        let edge = if velocity_px_per_sec > 0.0 {
            metrics.max_scroll_extent
        } else {
            metrics.min_scroll_extent
        };
        let arrival = simulation.time_at_x(edge);
        let remaining = simulation.dx(arrival);
        (arrival.is_finite() && remaining.is_finite() && remaining != 0.0).then_some(remaining)
    }
}

// ---------------------------------------------------------------------------
// BouncingScrollPhysics — iOS-style overscroll + spring-back
// ---------------------------------------------------------------------------

/// Resists outward motion past the content edge, then springs back to the
/// boundary on release. Motion toward the edge is unrestricted.
///
/// During a drag, positions past `[min, max]` are allowed but dampened: by
/// the `overscroll_spring_coefficient` (default 0.52) at the edge, stiffening
/// as the overscroll grows toward the viewport's length, which a pull never
/// reaches however long. On release past an edge,
/// a spring returns the position to the nearest edge; a fling inside the
/// range that friction would carry past an edge overscrolls on the same
/// spring and returns ([`BouncingScrollSimulation`]).
#[derive(Debug, Clone, Copy)]
pub struct BouncingScrollPhysics {
    /// Resistance applied when dragging past the edge, default 0.52: the
    /// share of a pull at the edge that moves the content, falling with the
    /// square of the viewport share left as the overscroll grows.
    /// Range `(0, 1)`: smaller = stiffer.
    pub overscroll_spring_coefficient: f64,
    /// Spring configuration used for the snap-back animation. The default is
    /// `SpringDescription::with_damping_ratio(1.0, 500.0, 0.75)`.
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
            spring: SpringDescription::with_damping_ratio(1.0, 500.0, 0.75)
                .expect("BUG: default scroll spring is in the admitted domain"),
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

impl BouncingScrollPhysics {
    /// How much farther out a pull of `pull` pixels carries content that is
    /// `overscroll` pixels past an edge of a `viewport`-long viewport.
    ///
    /// The resistance per pixel is `coefficient · (1 − overscroll /
    /// viewport)²`: the coefficient at the edge, stiffening toward nothing
    /// as the overscroll nears the viewport's length, which it never
    /// reaches. The resistance is integrated over the pull, so a pull split
    /// over many events lands where one event carrying it all would. A
    /// viewport that is not finite resists at the coefficient alone.
    fn rubber_band(&self, overscroll: f64, pull: f64, viewport: f64) -> f64 {
        let coefficient = self.overscroll_spring_coefficient;
        if !viewport.is_finite() {
            return pull * coefficient;
        }
        let viewport = viewport.max(1.0);
        // The share of the viewport the overscroll has left to take.
        let slack = (1.0 - overscroll / viewport).max(0.0);
        if slack == 0.0 {
            return 0.0;
        }
        // `d(slack)/d(pull) = −coefficient · slack² / viewport`, solved.
        let rest = slack / (1.0 + coefficient * pull * slack / viewport);
        viewport * (slack - rest)
    }
}

impl ScrollPhysics for BouncingScrollPhysics {
    fn apply_boundary_conditions(&self, metrics: &ScrollMetrics, proposed_pixels: f64) -> f64 {
        let Some((min, max)) = metrics.ordered_extents() else {
            return proposed_pixels;
        };
        let (edge, anchor) = if proposed_pixels < min {
            (min, metrics.pixels.min(min))
        } else if proposed_pixels > max {
            (max, metrics.pixels.max(max))
        } else {
            return proposed_pixels;
        };
        // Resist only the additional motion away from the edge. Reapplying
        // resistance to the total overshoot reverses small outward drags.
        let outward = proposed_pixels - anchor;
        if (proposed_pixels < min && outward < 0.0) || (proposed_pixels > max && outward > 0.0) {
            let farther = self.rubber_band(
                (anchor - edge).abs(),
                outward.abs(),
                metrics.viewport_dimension,
            );
            anchor + farther.copysign(outward)
        } else {
            proposed_pixels
        }
    }

    fn create_ballistic_simulation(
        &self,
        metrics: &ScrollMetrics,
        velocity_px_per_sec: f64,
    ) -> Option<Box<dyn Simulation>> {
        let bounds = metrics.bounds()?;
        let tolerance = metrics.ballistic_tolerance()?;
        // If the position is past an edge, spring back regardless of velocity.
        if let Some(edge) = metrics.edge_past() {
            let spring = SpringSimulation::new(
                self.spring,
                metrics.pixels,
                edge,
                velocity_px_per_sec,
                tolerance,
            )
            .ok()?;
            return Some(Box::new(spring));
        }
        // Within bounds: fling if velocity is above the threshold. A fling
        // the friction would carry past an edge overscrolls and springs back.
        if velocity_px_per_sec.abs() < self.min_fling_velocity_px_per_sec {
            return None;
        }
        let simulation = BouncingScrollSimulation::new(
            self.spring,
            self.fling_drag_coefficient,
            metrics.pixels,
            velocity_px_per_sec,
            bounds,
            tolerance,
        )
        .ok()?;
        Some(Box::new(simulation))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
