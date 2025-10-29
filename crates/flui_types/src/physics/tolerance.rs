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
/// # Memory Safety
/// - Stack-allocated `Copy` type with no heap allocations
/// - All fields are plain `f32` values
///
/// # Type Safety
/// - All constructors are const-evaluable
/// - Validation methods prevent invalid states
/// - `#[must_use]` on all pure methods
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
///
/// // Validate tolerance values
/// assert!(tolerance.is_valid());
/// assert!(tolerance.is_finite());
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
    #[inline]
    #[must_use]
    pub const fn new(distance: f32, velocity: f32, time: f32) -> Self {
        Self {
            distance,
            velocity,
            time,
        }
    }

    /// Checks if all tolerance values are finite (not NaN or infinite)
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::physics::Tolerance;
    ///
    /// let valid = Tolerance::new(0.01, 0.1, 0.01);
    /// assert!(valid.is_finite());
    ///
    /// let invalid = Tolerance::new(f32::NAN, 0.1, 0.01);
    /// assert!(!invalid.is_finite());
    /// ```
    #[inline]
    #[must_use]
    pub fn is_finite(&self) -> bool {
        self.distance.is_finite() && self.velocity.is_finite() && self.time.is_finite()
    }

    /// Checks if all tolerance values are valid (finite and non-negative)
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::physics::Tolerance;
    ///
    /// let valid = Tolerance::new(0.01, 0.1, 0.01);
    /// assert!(valid.is_valid());
    ///
    /// let invalid = Tolerance::new(-0.01, 0.1, 0.01);
    /// assert!(!invalid.is_valid());
    /// ```
    #[inline]
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.is_finite() && self.distance >= 0.0 && self.velocity >= 0.0 && self.time >= 0.0
    }

    /// Checks if a distance is within this tolerance
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::physics::Tolerance;
    ///
    /// let tolerance = Tolerance::default();
    /// assert!(tolerance.is_distance_within(0.0005));
    /// assert!(!tolerance.is_distance_within(0.002));
    /// ```
    #[inline]
    #[must_use]
    pub fn is_distance_within(&self, distance: f32) -> bool {
        distance.abs() < self.distance
    }

    /// Checks if a velocity is within this tolerance
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::physics::Tolerance;
    ///
    /// let tolerance = Tolerance::default();
    /// assert!(tolerance.is_velocity_within(0.005));
    /// assert!(!tolerance.is_velocity_within(0.02));
    /// ```
    #[inline]
    #[must_use]
    pub fn is_velocity_within(&self, velocity: f32) -> bool {
        velocity.abs() < self.velocity
    }

    /// Checks if a time difference is within this tolerance
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::physics::Tolerance;
    ///
    /// let tolerance = Tolerance::default();
    /// assert!(tolerance.is_time_within(0.0005));
    /// assert!(!tolerance.is_time_within(0.002));
    /// ```
    #[inline]
    #[must_use]
    pub fn is_time_within(&self, time: f32) -> bool {
        time.abs() < self.time
    }

    /// Scales all tolerance values by a factor
    ///
    /// Useful for adjusting tolerances based on rendering scale or DPI.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::physics::Tolerance;
    ///
    /// let base = Tolerance::default();
    /// let scaled = base.scale(2.0);
    /// assert_eq!(scaled.distance, 0.002);
    /// assert_eq!(scaled.velocity, 0.02);
    /// ```
    #[inline]
    #[must_use]
    pub fn scale(self, factor: f32) -> Self {
        Self {
            distance: self.distance * factor,
            velocity: self.velocity * factor,
            time: self.time * factor,
        }
    }

    /// Creates a tolerance with only distance and velocity (time set to default)
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::physics::Tolerance;
    ///
    /// let tolerance = Tolerance::from_distance_velocity(0.01, 0.1);
    /// assert_eq!(tolerance.distance, 0.01);
    /// assert_eq!(tolerance.velocity, 0.1);
    /// ```
    #[inline]
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

    #[test]
    fn test_tolerance_zero() {
        let tolerance = Tolerance::ZERO;
        assert_eq!(tolerance.distance, 0.0);
        assert_eq!(tolerance.velocity, 0.0);
        assert_eq!(tolerance.time, 0.0);
    }

    #[test]
    fn test_tolerance_relaxed() {
        let tolerance = Tolerance::RELAXED;
        assert_eq!(tolerance.distance, 0.1);
        assert_eq!(tolerance.velocity, 0.5);
        assert_eq!(tolerance.time, 0.01);
    }

    #[test]
    fn test_tolerance_is_finite() {
        let finite = Tolerance::new(0.01, 0.1, 0.01);
        assert!(finite.is_finite());

        let infinite = Tolerance::new(f32::INFINITY, 0.1, 0.01);
        assert!(!infinite.is_finite());

        let nan = Tolerance::new(f32::NAN, 0.1, 0.01);
        assert!(!nan.is_finite());
    }

    #[test]
    fn test_tolerance_is_valid() {
        let valid = Tolerance::new(0.01, 0.1, 0.01);
        assert!(valid.is_valid());

        let negative = Tolerance::new(-0.01, 0.1, 0.01);
        assert!(!negative.is_valid());

        let infinite = Tolerance::new(f32::INFINITY, 0.1, 0.01);
        assert!(!infinite.is_valid());
    }

    #[test]
    fn test_tolerance_is_distance_within() {
        let tolerance = Tolerance::default();
        assert!(tolerance.is_distance_within(0.0005));
        assert!(!tolerance.is_distance_within(0.002));
        assert!(tolerance.is_distance_within(-0.0005));
    }

    #[test]
    fn test_tolerance_is_velocity_within() {
        let tolerance = Tolerance::default();
        assert!(tolerance.is_velocity_within(0.005));
        assert!(!tolerance.is_velocity_within(0.02));
        assert!(tolerance.is_velocity_within(-0.005));
    }

    #[test]
    fn test_tolerance_is_time_within() {
        let tolerance = Tolerance::default();
        assert!(tolerance.is_time_within(0.0005));
        assert!(!tolerance.is_time_within(0.002));
        assert!(tolerance.is_time_within(-0.0005));
    }

    #[test]
    fn test_tolerance_scale() {
        let base = Tolerance::new(0.01, 0.1, 0.01);
        let scaled = base.scale(2.0);
        assert_eq!(scaled.distance, 0.02);
        assert_eq!(scaled.velocity, 0.2);
        assert_eq!(scaled.time, 0.02);
    }

    #[test]
    fn test_tolerance_from_distance_velocity() {
        let tolerance = Tolerance::from_distance_velocity(0.01, 0.1);
        assert_eq!(tolerance.distance, 0.01);
        assert_eq!(tolerance.velocity, 0.1);
        assert_eq!(tolerance.time, Tolerance::DEFAULT.time);
    }
}
