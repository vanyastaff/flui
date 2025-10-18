//! Tolerance values for physics simulations
//!
//! This module provides types for defining tolerances used to determine
//! when a simulation has reached a stable state.

/// Tolerance values for determining when a simulation is done
///
/// Similar to Flutter's `Tolerance`. Defines the maximum acceptable
/// difference for position, velocity, and time when determining if a
/// simulation has completed.
///
/// # Examples
///
/// ```
/// use flui_types::physics::Tolerance;
///
/// let tolerance = Tolerance::default();
/// assert_eq!(tolerance.distance, 0.001);
/// assert_eq!(tolerance.velocity, 0.01);
/// assert_eq!(tolerance.time, 0.001);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
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

    /// Creates a new tolerance with the given values
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::physics::Tolerance;
    ///
    /// let tolerance = Tolerance::new(0.01, 0.1, 0.01);
    /// assert_eq!(tolerance.distance, 0.01);
    /// assert_eq!(tolerance.velocity, 0.1);
    /// assert_eq!(tolerance.time, 0.01);
    /// ```
    pub const fn new(distance: f32, velocity: f32, time: f32) -> Self {
        Self {
            distance,
            velocity,
            time,
        }
    }
}

impl Default for Tolerance {
    fn default() -> Self {
        Self::DEFAULT
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tolerance_default() {
        let tolerance = Tolerance::default();
        assert_eq!(tolerance.distance, 0.001);
        assert_eq!(tolerance.velocity, 0.01);
        assert_eq!(tolerance.time, 0.001);
    }

    #[test]
    fn test_tolerance_default_const() {
        let tolerance = Tolerance::DEFAULT;
        assert_eq!(tolerance.distance, 0.001);
        assert_eq!(tolerance.velocity, 0.01);
        assert_eq!(tolerance.time, 0.001);
    }

    #[test]
    fn test_tolerance_new() {
        let tolerance = Tolerance::new(0.01, 0.1, 0.01);
        assert_eq!(tolerance.distance, 0.01);
        assert_eq!(tolerance.velocity, 0.1);
        assert_eq!(tolerance.time, 0.01);
    }

    #[test]
    fn test_tolerance_copy() {
        let t1 = Tolerance::new(0.01, 0.1, 0.01);
        let t2 = t1;
        assert_eq!(t1, t2);
    }
}
