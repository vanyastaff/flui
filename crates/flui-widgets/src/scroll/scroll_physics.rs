//! Scroll-physics strategies — `ScrollPhysics` trait plus the two standard
//! implementations Flutter ships: `ClampingScrollPhysics` (Android-style hard
//! clamp) and `BouncingScrollPhysics` (iOS-style overscroll + spring-back).
//!
//! # Flutter parity
//!
//! Mirrors `widgets/scroll_physics.dart` `ScrollPhysics`:
//! - `apply_boundary_conditions` ↔ `applyBoundaryConditions` (returns the
//!   _allowed_ position rather than the rejected overshoot, which is the
//!   simpler contract for FLUI's purely-eager callbacks).
//! - `create_ballistic_simulation` ↔ `createBallisticSimulation` (returns
//!   `Option<Box<dyn Simulation>>` for the fling/spring-back animation).
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

// ---------------------------------------------------------------------------
// Trait
// ---------------------------------------------------------------------------

/// Decides what happens at the scroll edges and after a fling gesture ends.
///
/// Implement this trait to provide custom boundary clamping and ballistic
/// (fling / spring-back) behaviour for a [`Scrollable`](super::Scrollable).
///
/// # Flutter parity
///
/// Corresponds to `ScrollPhysics` in `widgets/scroll_physics.dart`. The FLUI
/// contract is slightly different: `apply_boundary_conditions` returns the
/// _allowed position_ (not the rejected overshoot), which is ergonomically
/// simpler for a callback-driven update model.
pub trait ScrollPhysics: Send + Sync + std::fmt::Debug {
    /// Return the position the scroller should move to given a `proposed_pixels`
    /// offset in the context of the current scroll extents.
    ///
    /// For clamping physics this clips `proposed_pixels` to
    /// `[min_scroll_extent, max_scroll_extent]`. For bouncing physics a
    /// position past the edge is partially allowed with increasing resistance.
    ///
    /// # Flutter parity
    ///
    /// Corresponds to `applyBoundaryConditions`: the sign difference is that
    /// Flutter returns the _rejected_ overshoot; FLUI returns the _accepted_
    /// position. The net visual result is identical.
    fn apply_boundary_conditions(
        &self,
        proposed_pixels: f32,
        min_scroll_extent: f32,
        max_scroll_extent: f32,
    ) -> f32;

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
    ///
    /// # Flutter parity
    ///
    /// Corresponds to `createBallisticSimulation`.
    fn create_ballistic_simulation(
        &self,
        velocity_px_per_sec: f32,
        current_pixels: f32,
        min_scroll_extent: f32,
        max_scroll_extent: f32,
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
///
/// # Flutter parity
///
/// Mirrors `ClampingScrollPhysics` from `widgets/scroll_physics.dart`.
#[derive(Debug, Clone, Copy, Default)]
pub struct ClampingScrollPhysics {
    /// Below this absolute velocity (logical px / s) no fling is started.
    ///
    /// Flutter default is ~50 px/s; 0 px/s disables the threshold (always
    /// fling). Kept as a field rather than a constant so callers can tune it
    /// without a full custom implementation.
    pub min_fling_velocity_px_per_sec: f32,
    /// Friction drag coefficient for the fling deceleration. Must be in `(0,
    /// 1)`. Flutter uses a value corresponding to approximately `0.135` in
    /// `BoundedFrictionSimulation`.
    pub fling_drag_coefficient: f32,
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

impl ScrollPhysics for ClampingScrollPhysics {
    fn apply_boundary_conditions(
        &self,
        proposed_pixels: f32,
        min_scroll_extent: f32,
        max_scroll_extent: f32,
    ) -> f32 {
        proposed_pixels.clamp(min_scroll_extent, max_scroll_extent)
    }

    fn create_ballistic_simulation(
        &self,
        velocity_px_per_sec: f32,
        current_pixels: f32,
        min_scroll_extent: f32,
        max_scroll_extent: f32,
    ) -> Option<Box<dyn Simulation>> {
        // Skip fling below the configured threshold.
        if velocity_px_per_sec.abs() < self.min_fling_velocity_px_per_sec {
            return None;
        }
        // If the position is already out of bounds (possible if the caller
        // skipped boundary conditions), do not attempt a physics fling — let
        // the caller snap it into range first.
        if current_pixels < min_scroll_extent || current_pixels > max_scroll_extent {
            return None;
        }
        Some(Box::new(BoundedFrictionSimulation::new(
            self.fling_drag_coefficient,
            current_pixels,
            velocity_px_per_sec,
            min_scroll_extent,
            max_scroll_extent,
        )))
    }
}

// ---------------------------------------------------------------------------
// BouncingScrollPhysics — iOS-style overscroll + spring-back
// ---------------------------------------------------------------------------

/// Allows the scroll position to move past the content edge with increasing
/// resistance, then springs back to the boundary on release.
///
/// During a drag, positions past `[min, max]` are allowed but dampened by the
/// `overscroll_spring_coefficient` (Flutter uses 0.52). On release, a
/// `ScrollSpringSimulation` returns the position to the nearest valid edge.
///
/// # Flutter parity
///
/// Mirrors `BouncingScrollPhysics` from `widgets/scroll_physics.dart`.
#[derive(Debug, Clone, Copy)]
pub struct BouncingScrollPhysics {
    /// Resistance applied when dragging past the edge. Flutter hard-codes
    /// 0.52 in `applyPhysicsToUserOffset`. Range `(0, 1)`: smaller = stiffer.
    pub overscroll_spring_coefficient: f32,
    /// Spring configuration used for the snap-back animation.
    ///
    /// Flutter's `ScrollSpringSimulation` uses
    /// `SpringDescription.withDampingRatio(1.0, 500.0, 0.75)` (the "bouncy"
    /// preset). The FLUI default mirrors this.
    pub spring: SpringDescription,
    /// Below this absolute velocity (px/s) no fling is started. Flutter's
    /// bouncing physics also skips a fling for low velocities.
    pub min_fling_velocity_px_per_sec: f32,
    /// Friction drag coefficient for in-bounds fling deceleration.
    pub fling_drag_coefficient: f32,
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
    fn apply_boundary_conditions(
        &self,
        proposed_pixels: f32,
        min_scroll_extent: f32,
        max_scroll_extent: f32,
    ) -> f32 {
        if proposed_pixels < min_scroll_extent {
            // Allow overscroll past the top/left, but dampen it.
            let overscroll = proposed_pixels - min_scroll_extent;
            min_scroll_extent + overscroll * self.overscroll_spring_coefficient
        } else if proposed_pixels > max_scroll_extent {
            // Allow overscroll past the bottom/right, but dampen it.
            let overscroll = proposed_pixels - max_scroll_extent;
            max_scroll_extent + overscroll * self.overscroll_spring_coefficient
        } else {
            proposed_pixels
        }
    }

    fn create_ballistic_simulation(
        &self,
        velocity_px_per_sec: f32,
        current_pixels: f32,
        min_scroll_extent: f32,
        max_scroll_extent: f32,
    ) -> Option<Box<dyn Simulation>> {
        // If the position is past an edge, spring back regardless of velocity.
        if current_pixels < min_scroll_extent {
            return Some(Box::new(ScrollSpringSimulation::new(
                self.spring,
                current_pixels,
                min_scroll_extent,
                velocity_px_per_sec,
            )));
        }
        if current_pixels > max_scroll_extent {
            return Some(Box::new(ScrollSpringSimulation::new(
                self.spring,
                current_pixels,
                max_scroll_extent,
                velocity_px_per_sec,
            )));
        }
        // Within bounds: fling if velocity is above the threshold.
        if velocity_px_per_sec.abs() < self.min_fling_velocity_px_per_sec {
            return None;
        }
        Some(Box::new(BoundedFrictionSimulation::new(
            self.fling_drag_coefficient,
            current_pixels,
            velocity_px_per_sec,
            min_scroll_extent,
            max_scroll_extent,
        )))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    #![allow(clippy::float_cmp)] // unit tests assert exact clamping/pass-through values, not computed floats
    use super::*;

    // ClampingScrollPhysics ---------------------------------------------------

    #[test]
    fn clamping_apply_boundary_clamps_to_min() {
        let physics = ClampingScrollPhysics::new();
        let allowed = physics.apply_boundary_conditions(-50.0, 0.0, 400.0);
        assert_eq!(allowed, 0.0, "position below min is clamped to min");
    }

    #[test]
    fn clamping_apply_boundary_clamps_to_max() {
        let physics = ClampingScrollPhysics::new();
        let allowed = physics.apply_boundary_conditions(500.0, 0.0, 400.0);
        assert_eq!(allowed, 400.0, "position above max is clamped to max");
    }

    #[test]
    fn clamping_apply_boundary_passes_through_in_bounds() {
        let physics = ClampingScrollPhysics::new();
        let allowed = physics.apply_boundary_conditions(200.0, 0.0, 400.0);
        assert_eq!(
            allowed, 200.0,
            "in-bounds position passes through unchanged"
        );
    }

    #[test]
    fn clamping_ballistic_returns_none_below_fling_threshold() {
        let physics = ClampingScrollPhysics::new();
        let sim = physics.create_ballistic_simulation(10.0, 100.0, 0.0, 400.0);
        assert!(
            sim.is_none(),
            "velocity below min_fling_velocity should produce no simulation"
        );
    }

    #[test]
    fn clamping_ballistic_returns_simulation_above_fling_threshold() {
        let physics = ClampingScrollPhysics::new();
        let sim = physics.create_ballistic_simulation(300.0, 100.0, 0.0, 400.0);
        assert!(
            sim.is_some(),
            "velocity above min_fling_velocity should produce a bounded friction simulation"
        );
    }

    // BouncingScrollPhysics ---------------------------------------------------

    #[test]
    fn bouncing_apply_boundary_allows_overscroll_past_min_with_resistance() {
        let physics = BouncingScrollPhysics::new();
        // Propose a position 100 px past the top.
        let allowed = physics.apply_boundary_conditions(-100.0, 0.0, 400.0);
        // Resistance: -100 * 0.52 = -52 → allowed = 0 + (-52) = -52.
        assert!(
            allowed > -100.0 && allowed < 0.0,
            "overscroll past min should be partially allowed with damping, got {allowed}"
        );
        let expected = -100.0_f32 * 0.52;
        assert!(
            (allowed - expected).abs() < 0.001,
            "bouncing resistance at -100 should be {expected}, got {allowed}"
        );
    }

    #[test]
    fn bouncing_apply_boundary_allows_overscroll_past_max_with_resistance() {
        let physics = BouncingScrollPhysics::new();
        // Propose 80 px past the bottom (max = 400).
        let allowed = physics.apply_boundary_conditions(480.0, 0.0, 400.0);
        // Resistance: 80 * 0.52 = 41.6 → allowed = 400 + 41.6 = 441.6.
        let expected = 400.0 + 80.0_f32 * 0.52;
        assert!(
            (allowed - expected).abs() < 0.001,
            "bouncing resistance at 480 (max=400) should be {expected}, got {allowed}"
        );
    }

    #[test]
    fn bouncing_ballistic_springs_back_when_overscrolled_past_min() {
        let physics = BouncingScrollPhysics::new();
        // Position is already below min; spring back regardless of velocity.
        let sim = physics.create_ballistic_simulation(0.0, -50.0, 0.0, 400.0);
        assert!(
            sim.is_some(),
            "overscroll past min should produce a spring-back simulation even at zero velocity"
        );
    }

    #[test]
    fn bouncing_ballistic_springs_back_when_overscrolled_past_max() {
        let physics = BouncingScrollPhysics::new();
        let sim = physics.create_ballistic_simulation(0.0, 450.0, 0.0, 400.0);
        assert!(
            sim.is_some(),
            "overscroll past max should produce a spring-back simulation even at zero velocity"
        );
    }

    #[test]
    fn bouncing_ballistic_returns_none_below_fling_threshold_when_in_bounds() {
        let physics = BouncingScrollPhysics::new();
        let sim = physics.create_ballistic_simulation(10.0, 200.0, 0.0, 400.0);
        assert!(
            sim.is_none(),
            "in-bounds position with sub-threshold velocity should produce no simulation"
        );
    }
}
