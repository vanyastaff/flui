//! Velocity types for gesture tracking
//!
//! This module provides types for tracking and estimating velocity
//! of pointer movements.

use crate::geometry::{Offset, Pixels};
use std::time::Duration;

/// A velocity in two dimensions
///
/// Similar to Flutter's `Velocity`. Describes the speed of movement
/// in pixels per second along the x and y axes.
///
/// # Memory Safety
/// - Stack-allocated `Copy` type with no heap allocations
/// - All calculations use safe floating-point math
///
/// # Type Safety
/// - `#[must_use]` on all pure methods
/// - Validation methods prevent invalid states
///
/// # Performance
/// - `#[inline]` on hot-path methods
/// - Zero-cost abstractions for velocity calculations
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
///
/// // Validate velocity
/// assert!(velocity.is_finite());
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Velocity {
    /// The number of pixels per second of velocity in the x and y directions
    pub pixels_per_second: Offset<Pixels>,
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
    #[inline]
    #[must_use]
    pub const fn new(pixels_per_second: Offset<Pixels>) -> Self {
        Self { pixels_per_second }
    }

    /// Creates a velocity from x and y components
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::gestures::Velocity;
    ///
    /// let velocity = Velocity::from_components(100.0, 50.0);
    /// assert_eq!(velocity.pixels_per_second.dx, 100.0);
    /// assert_eq!(velocity.pixels_per_second.dy, 50.0);
    /// ```
    #[inline]
    #[must_use]
    pub fn from_components(dx: f32, dy: f32) -> Self {
        Self::new(Offset::new(Pixels(dx), Pixels(dy)))
    }

    /// Creates a velocity from magnitude and direction
    ///
    /// # Arguments
    ///
    /// * `magnitude` - The speed in pixels per second
    /// * `direction` - The direction in radians from the positive x-axis
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::gestures::Velocity;
    ///
    /// let velocity = Velocity::from_direction(100.0, 0.0);
    /// assert!((velocity.pixels_per_second.dx - 100.0).abs() < 0.01);
    /// assert!(velocity.pixels_per_second.dy.abs() < 0.01);
    /// ```
    #[inline]
    #[must_use]
    pub fn from_direction(magnitude: f32, direction: f32) -> Self {
        Self::new(Offset::from_direction(direction, magnitude))
    }

    /// Create a velocity from an offset traveled over a duration.
    ///
    /// This is the inverse of `distance_over_duration()`. Useful for calculating
    /// velocity from gesture tracking data.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::gestures::Velocity;
    /// use flui_types::Offset;
    /// use std::time::Duration;
    ///
    /// // Moved 100px right and 50px down in 100ms
    /// let velocity = Velocity::from_offset_over_duration(
    ///     Offset::new(100.0, 50.0),
    ///     Duration::from_millis(100)
    /// );
    ///
    /// // Velocity should be 1000px/s right, 500px/s down
    /// assert!((velocity.dx() - 1000.0).abs() < 0.1);
    /// assert!((velocity.dy() - 500.0).abs() < 0.1);
    /// ```
    #[inline]
    #[must_use]
    pub fn from_offset_over_duration(offset: Offset<Pixels>, duration: Duration) -> Self {
        let seconds = duration.as_secs_f32();
        if seconds == 0.0 {
            return Self::ZERO;
        }
        Self::new(offset / seconds)
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
    #[inline]
    #[must_use]
    pub fn magnitude(&self) -> f32 {
        self.pixels_per_second.distance().0
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
    #[inline]
    #[must_use]
    pub fn direction(&self) -> f32 {
        self.pixels_per_second.direction()
    }

    /// Returns whether this velocity is zero
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::gestures::Velocity;
    /// use flui_types::Offset;
    ///
    /// assert!(Velocity::ZERO.is_zero());
    /// assert!(!Velocity::new(Offset::new(1.0, 0.0)).is_zero());
    /// ```
    #[inline]
    #[must_use]
    pub fn is_zero(&self) -> bool {
        self.pixels_per_second.is_zero()
    }

    /// Returns whether all components are finite
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::gestures::Velocity;
    /// use flui_types::Offset;
    ///
    /// let valid = Velocity::new(Offset::new(100.0, 50.0));
    /// assert!(valid.is_finite());
    ///
    /// let invalid = Velocity::new(Offset::new(f32::NAN, 50.0));
    /// assert!(!invalid.is_finite());
    /// ```
    #[inline]
    #[must_use]
    pub fn is_finite(&self) -> bool {
        self.pixels_per_second.dx.is_finite() && self.pixels_per_second.dy.is_finite()
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
    #[must_use]
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

    /// Negates the velocity (reverses direction)
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::gestures::Velocity;
    /// use flui_types::Offset;
    ///
    /// let velocity = Velocity::new(Offset::new(100.0, -50.0));
    /// let negated = velocity.negate();
    /// assert_eq!(negated.pixels_per_second, Offset::new(-100.0, 50.0));
    /// ```
    #[inline]
    #[must_use]
    pub fn negate(&self) -> Self {
        Self::new(-self.pixels_per_second)
    }

    /// Scales the velocity by a factor
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::gestures::Velocity;
    /// use flui_types::Offset;
    ///
    /// let velocity = Velocity::new(Offset::new(100.0, 50.0));
    /// let scaled = velocity.scale(0.5);
    /// assert_eq!(scaled.pixels_per_second, Offset::new(50.0, 25.0));
    /// ```
    #[inline]
    #[must_use]
    pub fn scale(&self, factor: f32) -> Self {
        Self::new(self.pixels_per_second * factor)
    }

    /// Calculates the distance traveled over a duration
    ///
    /// Useful for predictive rendering and animation.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::gestures::Velocity;
    /// use flui_types::Offset;
    /// use std::time::Duration;
    ///
    /// let velocity = Velocity::new(Offset::new(100.0, 0.0));
    /// let distance = velocity.distance_over_duration(Duration::from_secs(1));
    /// assert_eq!(distance, Offset::new(100.0, 0.0));
    /// ```
    #[must_use]
    pub fn distance_over_duration(&self, duration: Duration) -> Offset<Pixels> {
        let seconds = duration.as_secs_f32();
        self.pixels_per_second * seconds
    }

    /// Returns the horizontal (x) component of velocity
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::gestures::Velocity;
    /// use flui_types::Offset;
    ///
    /// let velocity = Velocity::new(Offset::new(100.0, 50.0));
    /// assert_eq!(velocity.dx(), 100.0);
    /// ```
    #[inline]
    #[must_use]
    pub fn dx(&self) -> f32 {
        self.pixels_per_second.dx.0
    }

    /// Returns the vertical (y) component of velocity
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::gestures::Velocity;
    /// use flui_types::Offset;
    ///
    /// let velocity = Velocity::new(Offset::new(100.0, 50.0));
    /// assert_eq!(velocity.dy(), 50.0);
    /// ```
    #[inline]
    #[must_use]
    pub fn dy(&self) -> f32 {
        self.pixels_per_second.dy.0
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
    /// The duration over which the velocity was estimated
    pub duration: Duration,

    /// The offset at which the velocity was estimated
    pub offset: Offset<Pixels>,

    /// The velocity in pixels per second
    pub pixels_per_second: Offset<Pixels>,

    /// A value between 0.0 and 1.0 indicating confidence in the estimate
    ///
    /// A value of 1.0 indicates high confidence, 0.0 indicates low confidence.
    pub confidence: f32,
}

impl VelocityEstimate {
    /// Creates a new velocity estimate
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
    ///     0.95,
    /// );
    /// ```
    #[inline]
    #[must_use]
    pub const fn new(
        offset: Offset<Pixels>,
        pixels_per_second: Offset<Pixels>,
        duration: Duration,
        confidence: f32,
    ) -> Self {
        Self {
            duration,
            offset,
            pixels_per_second,
            confidence,
        }
    }

    /// Returns the velocity
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::gestures::VelocityEstimate;
    /// use flui_types::Offset;
    /// use std::time::Duration;
    ///
    /// let estimate = VelocityEstimate::new(
    ///     Offset::ZERO,
    ///     Offset::new(200.0, -100.0),
    ///     Duration::from_millis(16),
    ///     0.95,
    /// );
    /// let velocity = estimate.velocity();
    /// assert_eq!(velocity.pixels_per_second, Offset::new(200.0, -100.0));
    /// ```
    #[inline]
    #[must_use]
    pub fn velocity(&self) -> Velocity {
        Velocity::new(self.pixels_per_second)
    }

    /// Returns whether this estimate is reliable
    ///
    /// An estimate is considered reliable if the confidence is above 0.5.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::gestures::VelocityEstimate;
    /// use flui_types::Offset;
    /// use std::time::Duration;
    ///
    /// let reliable = VelocityEstimate::new(
    ///     Offset::ZERO,
    ///     Offset::ZERO,
    ///     Duration::from_millis(16),
    ///     0.8,
    /// );
    /// assert!(reliable.is_reliable());
    /// ```
    #[inline]
    #[must_use]
    pub fn is_reliable(&self) -> bool {
        self.confidence > 0.5
    }

    /// Returns whether all values are finite
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::gestures::VelocityEstimate;
    /// use flui_types::Offset;
    /// use std::time::Duration;
    ///
    /// let valid = VelocityEstimate::new(
    ///     Offset::new(100.0, 50.0),
    ///     Offset::new(200.0, -100.0),
    ///     Duration::from_millis(16),
    ///     0.95,
    /// );
    /// assert!(valid.is_finite());
    /// ```
    #[inline]
    #[must_use]
    pub fn is_finite(&self) -> bool {
        self.offset.dx.is_finite()
            && self.offset.dy.is_finite()
            && self.pixels_per_second.dx.is_finite()
            && self.pixels_per_second.dy.is_finite()
            && self.confidence.is_finite()
    }

    /// Returns whether the estimate is valid
    ///
    /// An estimate is valid if all values are finite and confidence is in [0, 1].
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::gestures::VelocityEstimate;
    /// use flui_types::Offset;
    /// use std::time::Duration;
    ///
    /// let valid = VelocityEstimate::new(
    ///     Offset::new(100.0, 50.0),
    ///     Offset::new(200.0, -100.0),
    ///     Duration::from_millis(16),
    ///     0.95,
    /// );
    /// assert!(valid.is_valid());
    ///
    /// let invalid = VelocityEstimate::new(
    ///     Offset::ZERO,
    ///     Offset::ZERO,
    ///     Duration::from_millis(16),
    ///     1.5, // Invalid confidence
    /// );
    /// assert!(!invalid.is_valid());
    /// ```
    #[inline]
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.is_finite() && self.confidence >= 0.0 && self.confidence <= 1.0
    }

    /// Returns the magnitude of the velocity estimate
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::gestures::VelocityEstimate;
    /// use flui_types::Offset;
    /// use std::time::Duration;
    ///
    /// let estimate = VelocityEstimate::new(
    ///     Offset::ZERO,
    ///     Offset::new(3.0, 4.0),
    ///     Duration::from_millis(16),
    ///     0.95,
    /// );
    /// assert_eq!(estimate.magnitude(), 5.0);
    /// ```
    #[inline]
    #[must_use]
    pub fn magnitude(&self) -> f32 {
        self.velocity().magnitude()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::units::px;

    #[test]
    fn test_velocity_zero() {
        let velocity = Velocity::ZERO;
        assert_eq!(velocity.pixels_per_second, Offset::ZERO);
        assert_eq!(velocity.magnitude(), 0.0);
    }

    #[test]
    fn test_velocity_new() {
        let velocity = Velocity::new(Offset::new(px(100.0), px(-50.0)));
        assert_eq!(velocity.pixels_per_second.dx, px(100.0));
        assert_eq!(velocity.pixels_per_second.dy, px(-50.0));
    }

    #[test]
    fn test_velocity_magnitude() {
        let velocity = Velocity::new(Offset::new(px(3.0), px(4.0)));
        assert_eq!(velocity.magnitude(), 5.0);

        let velocity2 = Velocity::new(Offset::new(px(100.0), px(0.0)));
        assert_eq!(velocity2.magnitude(), 100.0);
    }

    #[test]
    fn test_velocity_direction() {
        use std::f32::consts::PI;

        let velocity_right = Velocity::new(Offset::new(px(1.0), px(0.0)));
        assert!((velocity_right.direction() - 0.0).abs() < 0.01);

        let velocity_up = Velocity::new(Offset::new(px(0.0), px(1.0)));
        assert!((velocity_up.direction() - PI / 2.0).abs() < 0.01);

        let velocity_left = Velocity::new(Offset::new(px(-1.0), px(0.0)));
        assert!((velocity_left.direction() - PI).abs() < 0.01);
    }

    #[test]
    fn test_velocity_clamp_magnitude() {
        let velocity = Velocity::new(Offset::new(px(100.0), px(0.0)));

        // Clamp to smaller magnitude
        let clamped = velocity.clamp_magnitude(0.0, 50.0);
        assert_eq!(clamped.magnitude(), 50.0);
        assert_eq!(clamped.pixels_per_second.dx, px(50.0));
        assert_eq!(clamped.pixels_per_second.dy, px(0.0));

        // Already within range
        let unclamped = velocity.clamp_magnitude(0.0, 200.0);
        assert_eq!(unclamped.magnitude(), 100.0);

        // Clamp to minimum
        let clamped_min = Velocity::new(Offset::new(px(10.0), px(0.0))).clamp_magnitude(50.0, 100.0);
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
            Offset::new(px(100.0), px(50.0)),
            Offset::new(px(200.0), px(-100.0)),
            Duration::from_millis(16),
            0.95,
        );

        assert_eq!(estimate.offset, Offset::new(px(100.0), px(50.0)));
        assert_eq!(estimate.pixels_per_second, Offset::new(px(200.0), px(-100.0)));
        assert_eq!(estimate.duration, Duration::from_millis(16));
        assert_eq!(estimate.confidence, 0.95);
    }

    #[test]
    fn test_velocity_estimate_velocity() {
        let estimate = VelocityEstimate::new(
            Offset::new(px(100.0), px(50.0)),
            Offset::new(px(200.0), px(-100.0)),
            Duration::from_millis(16),
            0.95,
        );

        let velocity = estimate.velocity();
        assert_eq!(velocity.pixels_per_second, Offset::new(px(200.0), px(-100.0)));
    }

    #[test]
    fn test_velocity_estimate_reliable() {
        let reliable =
            VelocityEstimate::new(Offset::ZERO, Offset::ZERO, Duration::from_millis(16), 0.8);
        assert!(reliable.is_reliable());

        let unreliable =
            VelocityEstimate::new(Offset::ZERO, Offset::ZERO, Duration::from_millis(16), 0.3);
        assert!(!unreliable.is_reliable());
    }
}
