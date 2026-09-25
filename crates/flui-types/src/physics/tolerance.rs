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

    /// Flutter's `Tolerance()` uses 1e-3 for all three fields
    /// (`physics/tolerance.dart`); an earlier velocity of 0.01 stopped
    /// simulations early.
    #[test]
    fn constants_match_flutter_defaults() {
        assert_eq!(Tolerance::DEFAULT, Tolerance::new(1e-3, 1e-3, 1e-3));
        assert_eq!(Tolerance::default(), Tolerance::DEFAULT);
        assert_eq!(Tolerance::ZERO, Tolerance::new(0.0, 0.0, 0.0));
        assert_eq!(Tolerance::RELAXED, Tolerance::new(0.1, 0.5, 0.01));
        assert_eq!(
            Tolerance::from_distance_velocity(2.0, 3.0),
            Tolerance::new(2.0, 3.0, 1e-3)
        );
    }

    /// Each field on its own can make a tolerance non-finite or invalid.
    #[test]
    fn finiteness_and_validity_per_field() {
        let t = Tolerance::new(1.0, 2.0, 3.0);
        assert!(t.is_finite() && t.is_valid());
        assert!(Tolerance::ZERO.is_valid());
        for broken in [
            Tolerance {
                distance: f32::NAN,
                ..t
            },
            Tolerance {
                velocity: f32::INFINITY,
                ..t
            },
            Tolerance {
                time: f32::NAN,
                ..t
            },
        ] {
            assert!(!broken.is_finite() && !broken.is_valid(), "{broken:?}");
        }
        for negative in [
            Tolerance {
                distance: -1.0,
                ..t
            },
            Tolerance {
                velocity: -1.0,
                ..t
            },
            Tolerance { time: -1.0, ..t },
        ] {
            assert!(negative.is_finite() && !negative.is_valid(), "{negative:?}");
        }
    }

    /// Strictly inside, on magnitude: the bound itself is outside.
    #[test]
    fn within_checks_are_strict_on_magnitude() {
        let t = Tolerance::new(1.0, 2.0, 3.0);
        let checks: [fn(&Tolerance, f32) -> bool; 3] = [
            Tolerance::is_distance_within,
            Tolerance::is_velocity_within,
            Tolerance::is_time_within,
        ];
        for (within, bound) in checks.into_iter().zip([1.0, 2.0, 3.0]) {
            assert!(within(&t, bound * 0.5) && within(&t, -bound * 0.5));
            assert!(!within(&t, bound) && !within(&t, -bound));
        }
    }

    #[test]
    fn scale_scales_every_field() {
        assert_eq!(
            Tolerance::new(1.0, 2.0, 3.0).scale(2.0),
            Tolerance::new(2.0, 4.0, 6.0)
        );
    }
}
