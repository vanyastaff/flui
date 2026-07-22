//! Tolerance values for physics simulations
//!
//! This module provides types for defining tolerances used to determine
//! when a simulation has reached a stable state.

/// Structure that specifies maximum allowable magnitudes for distances,
/// durations, and velocity differences to be considered equal.
///
/// Simulations use these thresholds to decide when they are done: a value
/// whose magnitude is strictly below the corresponding epsilon is treated as
/// zero. Distances are in logical pixels, velocities in logical pixels per
/// second, times in seconds. Mirrors Flutter's `Tolerance`.
#[derive(Copy, Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Tolerance {
    // PORT-CHECK-OK-SP3: pre-existing parallel definition; consolidation tracked
    /// The minimum distance between samples to consider them different
    ///
    /// Default: 0.001 (1/1000th of a pixel)
    pub distance: f32,

    /// The minimum velocity to consider the simulation still moving
    ///
    /// Default: 0.001 (pixels per second)
    pub velocity: f32,

    /// The minimum time difference to consider significant
    ///
    /// Default: 0.001 (1 millisecond)
    pub time: f32,
}

impl Tolerance {
    /// The default tolerance.
    ///
    /// All three values are `0.001`, matching Flutter's `Tolerance()` default
    /// (`packages/flutter/lib/src/physics/tolerance.dart`, `_epsilonDefault = 1e-3`).
    ///
    /// A previous version used `velocity: 0.01`, which was 10× Flutter's value
    /// and caused simulations to report done prematurely (velocity threshold too
    /// high). Corrected to `0.001` for parity.
    pub const DEFAULT: Self = Self {
        distance: 0.001,
        velocity: 0.001,
        time: 0.001,
    };

    /// Zero tolerance (requires exact match)
    ///
    /// Useful for testing or scenarios requiring precise comparisons.
    pub const ZERO: Self = Self {
        distance: 0.0,
        velocity: 0.0,
        time: 0.0,
    };

    /// Relaxed tolerance for less precise simulations
    pub const RELAXED: Self = Self {
        distance: 0.1,
        velocity: 0.5,
        time: 0.01,
    };

    /// Creates a tolerance from explicit distance (logical pixels), velocity
    /// (logical pixels per second), and time (seconds) epsilons.
    #[must_use]
    #[inline]
    pub const fn new(distance: f32, velocity: f32, time: f32) -> Self {
        Self {
            distance,
            velocity,
            time,
        }
    }

    /// Returns whether all three epsilons are finite (not NaN or infinite).
    #[must_use]
    #[inline]
    pub fn is_finite(&self) -> bool {
        self.distance.is_finite() && self.velocity.is_finite() && self.time.is_finite()
    }

    /// Returns whether all three epsilons are finite and non-negative.
    #[must_use]
    #[inline]
    pub fn is_valid(&self) -> bool {
        self.is_finite() && self.distance >= 0.0 && self.velocity >= 0.0 && self.time >= 0.0
    }

    /// Returns whether `distance` is negligible: its magnitude is strictly
    /// below the distance epsilon.
    #[must_use]
    #[inline]
    pub fn is_distance_within(&self, distance: f32) -> bool {
        distance.abs() < self.distance
    }

    /// Returns whether `velocity` is negligible: its magnitude is strictly
    /// below the velocity epsilon.
    #[must_use]
    #[inline]
    pub fn is_velocity_within(&self, velocity: f32) -> bool {
        velocity.abs() < self.velocity
    }

    /// Returns whether `time` is negligible: its magnitude is strictly below
    /// the time epsilon.
    #[must_use]
    #[inline]
    pub fn is_time_within(&self, time: f32) -> bool {
        time.abs() < self.time
    }

    /// Returns a copy with all three epsilons multiplied by `factor`.
    ///
    /// Use a factor greater than `1.0` to relax the tolerance, or less than
    /// `1.0` to tighten it.
    #[must_use]
    #[inline]
    pub fn scale(self, factor: f32) -> Self {
        Self {
            distance: self.distance * factor,
            velocity: self.velocity * factor,
            time: self.time * factor,
        }
    }

    /// Creates a tolerance from distance and velocity epsilons, using the
    /// default time epsilon (`0.001` seconds).
    #[must_use]
    #[inline]
    pub const fn from_distance_velocity(distance: f32, velocity: f32) -> Self {
        Self {
            distance,
            velocity,
            time: Self::DEFAULT.time,
        }
    }
}

impl Default for Tolerance {
    #[inline]
    fn default() -> Self {
        Self::DEFAULT
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Default values — parity with Flutter's `Tolerance()` default
    //
    // Flutter: `_epsilonDefault = 1e-3` for all three fields.
    // Source: packages/flutter/lib/src/physics/tolerance.dart, line 15.
    //
    // A previous FLUI version had `velocity: 0.01` (10× too large), causing
    // simulations to stop prematurely.  These tests guard against regression.
    // -----------------------------------------------------------------------

    #[test]
    fn tolerance_default_distance_matches_flutter() {
        assert_eq!(
            Tolerance::DEFAULT.distance,
            1e-3,
            "distance default must match Flutter's 1e-3"
        );
    }

    #[test]
    fn tolerance_default_velocity_matches_flutter() {
        assert_eq!(
            Tolerance::DEFAULT.velocity,
            1e-3,
            "velocity default must match Flutter's 1e-3 (was incorrectly 0.01)"
        );
    }

    #[test]
    fn tolerance_default_time_matches_flutter() {
        assert_eq!(
            Tolerance::DEFAULT.time,
            1e-3,
            "time default must match Flutter's 1e-3"
        );
    }

    #[test]
    fn tolerance_default_via_default_trait() {
        let t = Tolerance::default();
        assert_eq!(t, Tolerance::DEFAULT);
    }
}
