//! Velocity types for gesture tracking
//!
//! This module provides types for tracking and estimating velocity
//! of pointer movements.

use crate::geometry::Offset;
use std::time::Duration;

/// A velocity in two dimensions
///
/// Similar to Flutter's `Velocity`. Describes the speed of movement
/// in pixels per second along the x and y axes.
///
/// # Examples
///
/// ```
/// use flui_types::gestures::Velocity;
/// use flui_types::Offset;
///
/// let velocity = Velocity::new(Offset::new(100.0, 50.0));
/// assert_eq!(velocity.pixels_per_second, Offset::new(100.0, 50.0));
///
/// // Get magnitude (speed)
/// let speed = velocity.magnitude();
/// assert!((speed - 111.80).abs() < 0.1);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Velocity {
    /// The number of pixels per second of velocity in the x and y directions
    pub pixels_per_second: Offset,
}

impl Velocity {
    /// A velocity that is zero in both dimensions
    pub const ZERO: Self = Self {
        pixels_per_second: Offset::ZERO,
    };

    /// Creates a new velocity
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::gestures::Velocity;
    /// use flui_types::Offset;
    ///
    /// let velocity = Velocity::new(Offset::new(100.0, -50.0));
    /// assert_eq!(velocity.pixels_per_second.dx, 100.0);
    /// assert_eq!(velocity.pixels_per_second.dy, -50.0);
    /// ```
    pub const fn new(pixels_per_second: Offset) -> Self {
        Self { pixels_per_second }
    }

    /// Returns the magnitude (speed) of the velocity
    ///
    /// This is the Euclidean distance in pixels per second.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::gestures::Velocity;
    /// use flui_types::Offset;
    ///
    /// let velocity = Velocity::new(Offset::new(3.0, 4.0));
    /// assert_eq!(velocity.magnitude(), 5.0);
    /// ```
    pub fn magnitude(&self) -> f32 {
        self.pixels_per_second.distance()
    }

    /// Returns the direction of the velocity in radians
    ///
    /// Returns the angle from the positive x-axis, in the range [-π, π].
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::gestures::Velocity;
    /// use flui_types::Offset;
    /// use std::f32::consts::PI;
    ///
    /// let velocity = Velocity::new(Offset::new(1.0, 0.0));
    /// assert!((velocity.direction() - 0.0).abs() < 0.01);
    ///
    /// let velocity_up = Velocity::new(Offset::new(0.0, 1.0));
    /// assert!((velocity_up.direction() - PI / 2.0).abs() < 0.01);
    /// ```
    pub fn direction(&self) -> f32 {
        self.pixels_per_second.direction()
    }

    /// Clamps the magnitude of the velocity
    ///
    /// If the magnitude exceeds `max`, scales the velocity to have magnitude `max`.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::gestures::Velocity;
    /// use flui_types::Offset;
    ///
    /// let velocity = Velocity::new(Offset::new(100.0, 0.0));
    /// let clamped = velocity.clamp_magnitude(0.0, 50.0);
    /// assert_eq!(clamped.magnitude(), 50.0);
    /// ```
    pub fn clamp_magnitude(&self, min: f32, max: f32) -> Self {
        let magnitude = self.magnitude();
        if magnitude == 0.0 {
            return *self;
        }

        let clamped_magnitude = magnitude.clamp(min, max);
        if clamped_magnitude == magnitude {
            return *self;
        }

        let scale = clamped_magnitude / magnitude;
        Self::new(self.pixels_per_second * scale)
    }
}

impl Default for Velocity {
    fn default() -> Self {
        Self::ZERO
    }
}

/// An estimate of the velocity of a pointer
///
/// Similar to Flutter's `VelocityEstimate`. Includes position, velocity,
/// and confidence information.
///
/// # Examples
///
/// ```
/// use flui_types::gestures::VelocityEstimate;
/// use flui_types::Offset;
/// use std::time::Duration;
///
/// let estimate = VelocityEstimate::new(
///     Offset::new(100.0, 50.0),
///     Offset::new(200.0, -100.0),
///     Duration::from_millis(16),
///     1.0,
/// );
///
/// assert_eq!(estimate.pixels_per_second, Offset::new(200.0, -100.0));
/// assert_eq!(estimate.confidence, 1.0);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct VelocityEstimate {
    /// The offset at which the velocity was estimated
    pub offset: Offset,

    /// The velocity in pixels per second
    pub pixels_per_second: Offset,

    /// The duration over which the velocity was estimated
    pub duration: Duration,

    /// A value between 0.0 and 1.0 indicating confidence in the estimate
    ///
    /// A value of 1.0 indicates high confidence, 0.0 indicates low confidence.
    pub confidence: f32,
}

impl VelocityEstimate {
    /// Creates a new velocity estimate
    pub const fn new(
        offset: Offset,
        pixels_per_second: Offset,
        duration: Duration,
        confidence: f32,
    ) -> Self {
        Self {
            offset,
            pixels_per_second,
            duration,
            confidence,
        }
    }

    /// Returns the velocity
    pub fn velocity(&self) -> Velocity {
        Velocity::new(self.pixels_per_second)
    }

    /// Returns whether this estimate is reliable
    ///
    /// An estimate is considered reliable if the confidence is above 0.5.
    pub fn is_reliable(&self) -> bool {
        self.confidence > 0.5
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_velocity_zero() {
        let velocity = Velocity::ZERO;
        assert_eq!(velocity.pixels_per_second, Offset::ZERO);
        assert_eq!(velocity.magnitude(), 0.0);
    }

    #[test]
    fn test_velocity_new() {
        let velocity = Velocity::new(Offset::new(100.0, -50.0));
        assert_eq!(velocity.pixels_per_second.dx, 100.0);
        assert_eq!(velocity.pixels_per_second.dy, -50.0);
    }

    #[test]
    fn test_velocity_magnitude() {
        let velocity = Velocity::new(Offset::new(3.0, 4.0));
        assert_eq!(velocity.magnitude(), 5.0);

        let velocity2 = Velocity::new(Offset::new(100.0, 0.0));
        assert_eq!(velocity2.magnitude(), 100.0);
    }

    #[test]
    fn test_velocity_direction() {
        use std::f32::consts::PI;

        let velocity_right = Velocity::new(Offset::new(1.0, 0.0));
        assert!((velocity_right.direction() - 0.0).abs() < 0.01);

        let velocity_up = Velocity::new(Offset::new(0.0, 1.0));
        assert!((velocity_up.direction() - PI / 2.0).abs() < 0.01);

        let velocity_left = Velocity::new(Offset::new(-1.0, 0.0));
        assert!((velocity_left.direction() - PI).abs() < 0.01);
    }

    #[test]
    fn test_velocity_clamp_magnitude() {
        let velocity = Velocity::new(Offset::new(100.0, 0.0));

        // Clamp to smaller magnitude
        let clamped = velocity.clamp_magnitude(0.0, 50.0);
        assert_eq!(clamped.magnitude(), 50.0);
        assert_eq!(clamped.pixels_per_second.dx, 50.0);
        assert_eq!(clamped.pixels_per_second.dy, 0.0);

        // Already within range
        let unclamped = velocity.clamp_magnitude(0.0, 200.0);
        assert_eq!(unclamped.magnitude(), 100.0);

        // Clamp to minimum
        let clamped_min = Velocity::new(Offset::new(10.0, 0.0)).clamp_magnitude(50.0, 100.0);
        assert_eq!(clamped_min.magnitude(), 50.0);
    }

    #[test]
    fn test_velocity_clamp_zero() {
        let velocity = Velocity::ZERO;
        let clamped = velocity.clamp_magnitude(10.0, 100.0);
        assert_eq!(clamped.magnitude(), 0.0);
    }

    #[test]
    fn test_velocity_default() {
        let velocity = Velocity::default();
        assert_eq!(velocity, Velocity::ZERO);
    }

    #[test]
    fn test_velocity_estimate_new() {
        let estimate = VelocityEstimate::new(
            Offset::new(100.0, 50.0),
            Offset::new(200.0, -100.0),
            Duration::from_millis(16),
            0.95,
        );

        assert_eq!(estimate.offset, Offset::new(100.0, 50.0));
        assert_eq!(estimate.pixels_per_second, Offset::new(200.0, -100.0));
        assert_eq!(estimate.duration, Duration::from_millis(16));
        assert_eq!(estimate.confidence, 0.95);
    }

    #[test]
    fn test_velocity_estimate_velocity() {
        let estimate = VelocityEstimate::new(
            Offset::new(100.0, 50.0),
            Offset::new(200.0, -100.0),
            Duration::from_millis(16),
            0.95,
        );

        let velocity = estimate.velocity();
        assert_eq!(velocity.pixels_per_second, Offset::new(200.0, -100.0));
    }

    #[test]
    fn test_velocity_estimate_reliable() {
        let reliable = VelocityEstimate::new(
            Offset::ZERO,
            Offset::ZERO,
            Duration::from_millis(16),
            0.8,
        );
        assert!(reliable.is_reliable());

        let unreliable = VelocityEstimate::new(
            Offset::ZERO,
            Offset::ZERO,
            Duration::from_millis(16),
            0.3,
        );
        assert!(!unreliable.is_reliable());
    }
}
