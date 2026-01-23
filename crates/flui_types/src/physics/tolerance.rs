//! Tolerance values for physics simulations
//!
//! This module provides types for defining tolerances used to determine
//! when a simulation has reached a stable state.

#[derive(Copy, Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Tolerance {
    /// The minimum distance between samples to consider them different
    ///
    /// Default: 0.001 (1/1000th of a pixel)
    pub distance: f32,

    /// The minimum velocity to consider the simulation still moving
    ///
    /// Default: 0.01 (pixels per second)
    pub velocity: f32,

    /// The minimum time difference to consider significant
    ///
    /// Default: 0.001 (1 millisecond)
    pub time: f32,
}

impl Tolerance {
    /// The default tolerance
    pub const DEFAULT: Self = Self {
        distance: 0.001,
        velocity: 0.01,
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
    pub const fn new(distance: f32, velocity: f32, time: f32) -> Self {
        Self {
            distance,
            velocity,
            time,
        }
    }

    #[must_use]
    pub fn is_finite(&self) -> bool {
        self.distance.is_finite() && self.velocity.is_finite() && self.time.is_finite()
    }

    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.is_finite() && self.distance >= 0.0 && self.velocity >= 0.0 && self.time >= 0.0
    }

    #[must_use]
    pub fn is_distance_within(&self, distance: f32) -> bool {
        distance.abs() < self.distance
    }

    #[must_use]
    pub fn is_velocity_within(&self, velocity: f32) -> bool {
        velocity.abs() < self.velocity
    }

    #[must_use]
    pub fn is_time_within(&self, time: f32) -> bool {
        time.abs() < self.time
    }

    #[must_use]
    pub fn scale(self, factor: f32) -> Self {
        Self {
            distance: self.distance * factor,
            velocity: self.velocity * factor,
            time: self.time * factor,
        }
    }

    #[must_use]
    pub const fn from_distance_velocity(distance: f32, velocity: f32) -> Self {
        Self {
            distance,
            velocity,
            time: Self::DEFAULT.time,
        }
    }
}

impl Default for Tolerance {
    fn default() -> Self {
        Self::DEFAULT
    }
}
