//! Tolerance values for physics simulations
//!
//! This module provides types for defining tolerances used to determine
//! when a simulation has reached a stable state.

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

    #[must_use]
    #[inline]
    pub const fn new(distance: f32, velocity: f32, time: f32) -> Self {
        Self {
            distance,
            velocity,
            time,
        }
    }

    #[must_use]
    #[inline]
    pub fn is_finite(&self) -> bool {
        self.distance.is_finite() && self.velocity.is_finite() && self.time.is_finite()
    }

    #[must_use]
    #[inline]
    pub fn is_valid(&self) -> bool {
        self.is_finite() && self.distance >= 0.0 && self.velocity >= 0.0 && self.time >= 0.0
    }

    #[must_use]
    #[inline]
    pub fn is_distance_within(&self, distance: f32) -> bool {
        distance.abs() < self.distance
    }

    #[must_use]
    #[inline]
    pub fn is_velocity_within(&self, velocity: f32) -> bool {
        velocity.abs() < self.velocity
    }

    #[must_use]
    #[inline]
    pub fn is_time_within(&self, time: f32) -> bool {
        time.abs() < self.time
    }

    #[must_use]
    #[inline]
    pub fn scale(self, factor: f32) -> Self {
        Self {
            distance: self.distance * factor,
            velocity: self.velocity * factor,
            time: self.time * factor,
        }
    }

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
