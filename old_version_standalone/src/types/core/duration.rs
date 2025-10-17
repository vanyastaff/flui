//! Duration type for animations and timing
//!
//! This module provides a Duration type wrapper with Flutter-like API.

use std::time::Duration as StdDuration;

/// A span of time, such as for animations.
///
/// This is a wrapper around std::time::Duration with additional
/// Flutter-like convenience methods.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Duration(pub StdDuration);

impl Duration {
    /// Create a new duration from the given number of microseconds.
    pub const fn from_microseconds(microseconds: u64) -> Self {
        Duration(StdDuration::from_micros(microseconds))
    }

    /// Create a new duration from the given number of milliseconds.
    pub const fn from_milliseconds(milliseconds: u64) -> Self {
        Duration(StdDuration::from_millis(milliseconds))
    }

    /// Create a new duration from the given number of seconds.
    pub const fn from_seconds(seconds: u64) -> Self {
        Duration(StdDuration::from_secs(seconds))
    }

    /// Create a new duration from the given number of minutes.
    pub const fn from_minutes(minutes: u64) -> Self {
        Duration(StdDuration::from_secs(minutes * 60))
    }

    /// Create a new duration from the given number of hours.
    pub const fn from_hours(hours: u64) -> Self {
        Duration(StdDuration::from_secs(hours * 3600))
    }

    /// Create a new duration from the given number of days.
    pub const fn from_days(days: u64) -> Self {
        Duration(StdDuration::from_secs(days * 86400))
    }

    /// A duration of zero time.
    pub const ZERO: Self = Duration(StdDuration::ZERO);

    /// The maximum representable duration.
    pub const MAX: Self = Duration(StdDuration::MAX);

    /// Get the duration in microseconds.
    pub fn as_microseconds(&self) -> u128 {
        self.0.as_micros()
    }

    /// Get the duration in milliseconds.
    pub fn as_milliseconds(&self) -> u128 {
        self.0.as_millis()
    }

    /// Get the duration in seconds.
    pub fn as_seconds(&self) -> u64 {
        self.0.as_secs()
    }

    /// Get the duration as a floating-point number of seconds.
    pub fn as_seconds_f64(&self) -> f64 {
        self.0.as_secs_f64()
    }

    /// Get the duration as a floating-point number of seconds (f32).
    pub fn as_seconds_f32(&self) -> f32 {
        self.0.as_secs_f32()
    }

    /// Check if this duration is zero.
    pub fn is_zero(&self) -> bool {
        self.0.is_zero()
    }

    /// Get the underlying std::time::Duration.
    pub fn as_std(&self) -> StdDuration {
        self.0
    }

    /// Multiply the duration by a scalar.
    pub fn multiply(&self, factor: f64) -> Self {
        Duration(self.0.mul_f64(factor))
    }

    /// Divide the duration by a scalar.
    pub fn divide(&self, divisor: f64) -> Self {
        Duration(self.0.div_f64(divisor))
    }

    /// Add two durations together.
    pub fn add(&self, other: Duration) -> Self {
        Duration(self.0 + other.0)
    }

    /// Subtract a duration from this duration.
    ///
    /// Returns None if the result would be negative.
    pub fn subtract(&self, other: Duration) -> Option<Self> {
        self.0.checked_sub(other.0).map(Duration)
    }

    /// Compare this duration with another and return the minimum.
    pub fn min(&self, other: Duration) -> Self {
        Duration(self.0.min(other.0))
    }

    /// Compare this duration with another and return the maximum.
    pub fn max(&self, other: Duration) -> Self {
        Duration(self.0.max(other.0))
    }

    /// Clamp this duration between a minimum and maximum.
    pub fn clamp(&self, min: Duration, max: Duration) -> Self {
        Duration(self.0.clamp(min.0, max.0))
    }
}

impl Default for Duration {
    fn default() -> Self {
        Duration::ZERO
    }
}

impl From<StdDuration> for Duration {
    fn from(duration: StdDuration) -> Self {
        Duration(duration)
    }
}

impl From<Duration> for StdDuration {
    fn from(duration: Duration) -> Self {
        duration.0
    }
}

impl std::ops::Add for Duration {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Duration(self.0 + rhs.0)
    }
}

impl std::ops::Sub for Duration {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        self.subtract(rhs)
            .unwrap_or(Duration::ZERO)
    }
}

impl std::ops::Mul<f64> for Duration {
    type Output = Self;

    fn mul(self, rhs: f64) -> Self::Output {
        self.multiply(rhs)
    }
}

impl std::ops::Div<f64> for Duration {
    type Output = Self;

    fn div(self, rhs: f64) -> Self::Output {
        self.divide(rhs)
    }
}

/// Common animation durations (similar to Material Design).
pub struct AnimationDurations;

impl AnimationDurations {
    /// Extra short duration: 75ms.
    pub const EXTRA_SHORT: Duration = Duration::from_milliseconds(75);

    /// Short duration: 150ms.
    pub const SHORT: Duration = Duration::from_milliseconds(150);

    /// Medium duration: 300ms.
    pub const MEDIUM: Duration = Duration::from_milliseconds(300);

    /// Long duration: 500ms.
    pub const LONG: Duration = Duration::from_milliseconds(500);

    /// Extra long duration: 700ms.
    pub const EXTRA_LONG: Duration = Duration::from_milliseconds(700);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_duration_creation() {
        let ms = Duration::from_milliseconds(1000);
        assert_eq!(ms.as_milliseconds(), 1000);

        let secs = Duration::from_seconds(1);
        assert_eq!(secs.as_seconds(), 1);

        let mins = Duration::from_minutes(1);
        assert_eq!(mins.as_seconds(), 60);

        let hours = Duration::from_hours(1);
        assert_eq!(hours.as_seconds(), 3600);

        let days = Duration::from_days(1);
        assert_eq!(days.as_seconds(), 86400);
    }

    #[test]
    fn test_duration_zero() {
        assert!(Duration::ZERO.is_zero());
        assert_eq!(Duration::ZERO.as_seconds(), 0);
    }

    #[test]
    fn test_duration_conversions() {
        let duration = Duration::from_seconds(5);
        assert_eq!(duration.as_seconds_f64(), 5.0);
        assert_eq!(duration.as_milliseconds(), 5000);
        assert_eq!(duration.as_microseconds(), 5_000_000);
    }

    #[test]
    fn test_duration_arithmetic() {
        let a = Duration::from_seconds(10);
        let b = Duration::from_seconds(5);

        let sum = a + b;
        assert_eq!(sum.as_seconds(), 15);

        let diff = a - b;
        assert_eq!(diff.as_seconds(), 5);

        let doubled = a * 2.0;
        assert_eq!(doubled.as_seconds(), 20);

        let halved = a / 2.0;
        assert_eq!(halved.as_seconds(), 5);
    }

    #[test]
    fn test_duration_subtract_overflow() {
        let a = Duration::from_seconds(5);
        let b = Duration::from_seconds(10);

        let result = a - b;
        assert_eq!(result, Duration::ZERO); // Should not panic, returns ZERO
    }

    #[test]
    fn test_duration_min_max() {
        let a = Duration::from_seconds(10);
        let b = Duration::from_seconds(5);

        assert_eq!(a.min(b).as_seconds(), 5);
        assert_eq!(a.max(b).as_seconds(), 10);
    }

    #[test]
    fn test_duration_clamp() {
        let duration = Duration::from_seconds(15);
        let min = Duration::from_seconds(5);
        let max = Duration::from_seconds(10);

        let clamped = duration.clamp(min, max);
        assert_eq!(clamped.as_seconds(), 10);

        let too_small = Duration::from_seconds(3);
        let clamped_min = too_small.clamp(min, max);
        assert_eq!(clamped_min.as_seconds(), 5);
    }

    #[test]
    fn test_duration_std_conversion() {
        let duration = Duration::from_seconds(5);
        let std_duration: StdDuration = duration.into();
        assert_eq!(std_duration.as_secs(), 5);

        let back: Duration = std_duration.into();
        assert_eq!(back.as_seconds(), 5);
    }

    #[test]
    fn test_animation_durations() {
        assert_eq!(AnimationDurations::EXTRA_SHORT.as_milliseconds(), 75);
        assert_eq!(AnimationDurations::SHORT.as_milliseconds(), 150);
        assert_eq!(AnimationDurations::MEDIUM.as_milliseconds(), 300);
        assert_eq!(AnimationDurations::LONG.as_milliseconds(), 500);
        assert_eq!(AnimationDurations::EXTRA_LONG.as_milliseconds(), 700);
    }

    #[test]
    fn test_duration_comparison() {
        let a = Duration::from_seconds(5);
        let b = Duration::from_seconds(10);

        assert!(a < b);
        assert!(b > a);
        assert!(a <= a);
        assert!(a >= a);
        assert_eq!(a, a);
        assert_ne!(a, b);
    }

    #[test]
    fn test_duration_multiply_fractional() {
        let duration = Duration::from_seconds(10);
        let half = duration * 0.5;
        assert_eq!(half.as_seconds(), 5);

        let one_and_half = duration * 1.5;
        assert_eq!(one_and_half.as_seconds(), 15);
    }
}
