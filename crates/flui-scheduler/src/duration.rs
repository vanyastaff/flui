//! Type-safe duration wrappers using the Newtype pattern
//!
//! This module provides zero-cost newtype wrappers for durations,
//! preventing accidental mixing of milliseconds, seconds, and other units.
//!
//! ## Available Types
//!
//! - [`Milliseconds`] - Time in milliseconds (f64)
//! - [`Seconds`] - Time in seconds (f64)
//! - [`Microseconds`] - Time in microseconds (i64, integer precision)
//! - [`Percentage`] - Percentage value (0.0 to 100.0)
//! - [`FrameDuration`] - Frame budget with FPS-based calculations
//!
//! ## Newtype Pattern
//!
//! The newtype pattern wraps a primitive type in a struct to create
//! a distinct type. This prevents accidental mixing of semantically
//! different values that have the same underlying representation.
//!
//! ```rust
//! use flui_scheduler::duration::{Milliseconds, Seconds, FrameDuration};
//!
//! // Type-safe - can't accidentally mix units
//! let ms = Milliseconds::new(16.67);
//! let secs = Seconds::new(0.01667);
//!
//! // Easy conversion
//! let ms_from_secs: Milliseconds = secs.into();
//!
//! // FrameDuration for budget calculations
//! let budget = FrameDuration::from_fps(60);
//! assert!(budget.as_ms().value() - 16.67 < 0.01);
//! ```

use std::fmt;
use std::ops::{Add, AddAssign, Div, DivAssign, Mul, MulAssign, Sub, SubAssign};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

// =============================================================================
// Milliseconds Newtype
// =============================================================================

/// Type-safe milliseconds wrapper
///
/// Prevents accidental mixing with other duration units.
///
/// # Examples
///
/// ```
/// use flui_scheduler::duration::Milliseconds;
///
/// let ms = Milliseconds::new(16.67);
/// assert_eq!(ms.value(), 16.67);
///
/// // Arithmetic operations
/// let total = ms + Milliseconds::new(10.0);
/// assert!((total.value() - 26.67).abs() < 0.01);
///
/// // Convert to std::time::Duration
/// let std_duration: std::time::Duration = ms.into();
/// ```
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Milliseconds(f64);

// Note: We intentionally don't implement Eq/Hash for f64-based types
// because floating-point equality is problematic (NaN != NaN, -0.0 == 0.0)

impl Milliseconds {
    /// Zero milliseconds
    pub const ZERO: Self = Self(0.0);

    /// One millisecond
    pub const ONE: Self = Self(1.0);

    /// Create a new milliseconds value
    #[inline]
    pub const fn new(ms: f64) -> Self {
        Self(ms)
    }

    /// Get the raw value
    #[inline]
    pub const fn value(self) -> f64 {
        self.0
    }

    /// Convert to seconds
    #[inline]
    pub fn to_seconds(self) -> Seconds {
        Seconds::new(self.0 / 1000.0)
    }

    /// Convert to microseconds
    #[inline]
    pub fn to_micros(self) -> Microseconds {
        Microseconds::new((self.0 * 1000.0) as i64)
    }

    /// Check if this duration is zero
    #[inline]
    pub fn is_zero(self) -> bool {
        self.0 == 0.0
    }

    /// Get the maximum of two durations
    #[inline]
    pub fn max(self, other: Self) -> Self {
        Self(self.0.max(other.0))
    }

    /// Get the minimum of two durations
    #[inline]
    pub fn min(self, other: Self) -> Self {
        Self(self.0.min(other.0))
    }

    /// Clamp the duration to a range
    #[inline]
    pub fn clamp(self, min: Self, max: Self) -> Self {
        Self(self.0.clamp(min.0, max.0))
    }

    /// Saturating subtraction (returns ZERO if result would be negative)
    #[inline]
    pub fn saturating_sub(self, other: Self) -> Self {
        Self((self.0 - other.0).max(0.0))
    }
}

impl fmt::Display for Milliseconds {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:.2}ms", self.0)
    }
}

impl Add for Milliseconds {
    type Output = Self;

    #[inline]
    fn add(self, rhs: Self) -> Self::Output {
        Self(self.0 + rhs.0)
    }
}

impl Sub for Milliseconds {
    type Output = Self;

    #[inline]
    fn sub(self, rhs: Self) -> Self::Output {
        Self(self.0 - rhs.0)
    }
}

impl AddAssign for Milliseconds {
    #[inline]
    fn add_assign(&mut self, rhs: Self) {
        self.0 += rhs.0;
    }
}

impl SubAssign for Milliseconds {
    #[inline]
    fn sub_assign(&mut self, rhs: Self) {
        self.0 -= rhs.0;
    }
}

impl Mul<f64> for Milliseconds {
    type Output = Self;

    #[inline]
    fn mul(self, rhs: f64) -> Self::Output {
        Self(self.0 * rhs)
    }
}

impl MulAssign<f64> for Milliseconds {
    #[inline]
    fn mul_assign(&mut self, rhs: f64) {
        self.0 *= rhs;
    }
}

impl Div<f64> for Milliseconds {
    type Output = Self;

    #[inline]
    fn div(self, rhs: f64) -> Self::Output {
        Self(self.0 / rhs)
    }
}

impl DivAssign<f64> for Milliseconds {
    #[inline]
    fn div_assign(&mut self, rhs: f64) {
        self.0 /= rhs;
    }
}

impl From<Seconds> for Milliseconds {
    #[inline]
    fn from(secs: Seconds) -> Self {
        Self(secs.value() * 1000.0)
    }
}

impl From<std::time::Duration> for Milliseconds {
    #[inline]
    fn from(duration: std::time::Duration) -> Self {
        Self(duration.as_secs_f64() * 1000.0)
    }
}

impl From<Milliseconds> for std::time::Duration {
    #[inline]
    fn from(ms: Milliseconds) -> Self {
        std::time::Duration::from_secs_f64(ms.0 / 1000.0)
    }
}

impl From<f64> for Milliseconds {
    #[inline]
    fn from(value: f64) -> Self {
        Self(value)
    }
}

impl From<Microseconds> for Milliseconds {
    #[inline]
    fn from(us: Microseconds) -> Self {
        us.to_ms()
    }
}

// =============================================================================
// Seconds Newtype
// =============================================================================

/// Type-safe seconds wrapper
///
/// # Examples
///
/// ```
/// use flui_scheduler::duration::Seconds;
///
/// let secs = Seconds::new(1.5);
/// let ms = secs.to_ms();
/// assert_eq!(ms.value(), 1500.0);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Seconds(f64);

impl Seconds {
    /// Zero seconds
    pub const ZERO: Self = Self(0.0);

    /// One second
    pub const ONE: Self = Self(1.0);

    /// Create a new seconds value
    #[inline]
    pub const fn new(secs: f64) -> Self {
        Self(secs)
    }

    /// Get the raw value
    #[inline]
    pub const fn value(self) -> f64 {
        self.0
    }

    /// Convert to milliseconds
    #[inline]
    pub fn to_ms(self) -> Milliseconds {
        Milliseconds::new(self.0 * 1000.0)
    }

    /// Check if this duration is zero
    #[inline]
    pub fn is_zero(self) -> bool {
        self.0 == 0.0
    }
}

impl fmt::Display for Seconds {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:.3}s", self.0)
    }
}

impl Add for Seconds {
    type Output = Self;

    #[inline]
    fn add(self, rhs: Self) -> Self::Output {
        Self(self.0 + rhs.0)
    }
}

impl Sub for Seconds {
    type Output = Self;

    #[inline]
    fn sub(self, rhs: Self) -> Self::Output {
        Self(self.0 - rhs.0)
    }
}

impl AddAssign for Seconds {
    #[inline]
    fn add_assign(&mut self, rhs: Self) {
        self.0 += rhs.0;
    }
}

impl SubAssign for Seconds {
    #[inline]
    fn sub_assign(&mut self, rhs: Self) {
        self.0 -= rhs.0;
    }
}

impl Mul<f64> for Seconds {
    type Output = Self;

    #[inline]
    fn mul(self, rhs: f64) -> Self::Output {
        Self(self.0 * rhs)
    }
}

impl Div<f64> for Seconds {
    type Output = Self;

    #[inline]
    fn div(self, rhs: f64) -> Self::Output {
        Self(self.0 / rhs)
    }
}

impl From<Milliseconds> for Seconds {
    #[inline]
    fn from(ms: Milliseconds) -> Self {
        Self(ms.value() / 1000.0)
    }
}

impl From<std::time::Duration> for Seconds {
    #[inline]
    fn from(duration: std::time::Duration) -> Self {
        Self(duration.as_secs_f64())
    }
}

impl From<Seconds> for std::time::Duration {
    #[inline]
    fn from(secs: Seconds) -> Self {
        std::time::Duration::from_secs_f64(secs.0)
    }
}

impl From<f64> for Seconds {
    #[inline]
    fn from(value: f64) -> Self {
        Self(value)
    }
}

// =============================================================================
// Microseconds Newtype
// =============================================================================

/// Type-safe microseconds wrapper (integer precision)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Microseconds(i64);

impl Microseconds {
    /// Zero microseconds
    pub const ZERO: Self = Self(0);

    /// One microsecond
    pub const ONE: Self = Self(1);

    /// Create a new microseconds value
    #[inline]
    pub const fn new(us: i64) -> Self {
        Self(us)
    }

    /// Get the raw value
    #[inline]
    pub const fn value(self) -> i64 {
        self.0
    }

    /// Convert to milliseconds
    #[inline]
    pub fn to_ms(self) -> Milliseconds {
        Milliseconds::new(self.0 as f64 / 1000.0)
    }

    /// Try to convert to std::time::Duration
    ///
    /// Returns `None` if the value is negative, as `std::time::Duration` cannot
    /// represent negative durations.
    #[inline]
    pub fn try_to_std_duration(self) -> Option<std::time::Duration> {
        if self.0 >= 0 {
            Some(std::time::Duration::from_micros(self.0 as u64))
        } else {
            None
        }
    }

    /// Convert to std::time::Duration
    ///
    /// # Panics
    ///
    /// Panics if the value is negative, as `std::time::Duration` cannot
    /// represent negative durations. Prefer [`try_to_std_duration`](Self::try_to_std_duration)
    /// for fallible conversion.
    #[inline]
    pub fn to_std_duration(self) -> std::time::Duration {
        self.try_to_std_duration().unwrap_or_else(|| {
            panic!(
                "Cannot convert negative Microseconds ({}) to Duration",
                self.0
            )
        })
    }
}

impl fmt::Display for Microseconds {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}μs", self.0)
    }
}

impl From<i64> for Microseconds {
    #[inline]
    fn from(value: i64) -> Self {
        Self(value)
    }
}

impl From<std::time::Duration> for Microseconds {
    #[inline]
    fn from(duration: std::time::Duration) -> Self {
        Self(duration.as_micros() as i64)
    }
}

impl From<Microseconds> for std::time::Duration {
    /// # Panics
    ///
    /// Panics if the `Microseconds` value is negative.
    #[inline]
    fn from(us: Microseconds) -> Self {
        us.to_std_duration()
    }
}

// =============================================================================
// FrameDuration - Frame budget calculations
// =============================================================================

/// Frame duration with budget-related calculations
///
/// This type is specifically designed for frame timing calculations,
/// providing convenient methods for FPS-based budgets.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct FrameDuration {
    /// Target duration in milliseconds
    target_ms: Milliseconds,
}

impl FrameDuration {
    /// Frame duration for 30 FPS (~33.33ms)
    pub const FPS_30: Self = Self {
        target_ms: Milliseconds::new(33.333),
    };

    /// Frame duration for 60 FPS (~16.67ms) - standard target
    pub const FPS_60: Self = Self {
        target_ms: Milliseconds::new(16.667),
    };

    /// Frame duration for 120 FPS (~8.33ms) - high refresh rate
    pub const FPS_120: Self = Self {
        target_ms: Milliseconds::new(8.333),
    };

    /// Frame duration for 144 FPS (~6.94ms) - gaming monitors
    pub const FPS_144: Self = Self {
        target_ms: Milliseconds::new(6.944),
    };

    /// Create a frame duration from target FPS
    ///
    /// # Panics
    ///
    /// Panics if `fps` is 0.
    #[inline]
    pub fn from_fps(fps: u32) -> Self {
        assert!(fps > 0, "FPS must be greater than 0");
        Self {
            target_ms: Milliseconds::new(1000.0 / fps as f64),
        }
    }

    /// Get target duration in milliseconds
    #[inline]
    pub fn as_ms(self) -> Milliseconds {
        self.target_ms
    }

    /// Get target duration in seconds
    #[inline]
    pub fn as_seconds(self) -> Seconds {
        self.target_ms.to_seconds()
    }

    /// Get the FPS this duration represents
    #[inline]
    pub fn fps(self) -> f64 {
        1000.0 / self.target_ms.value()
    }

    /// Calculate remaining budget given elapsed time
    #[inline]
    pub fn remaining(self, elapsed: Milliseconds) -> Milliseconds {
        self.target_ms.saturating_sub(elapsed)
    }

    /// Check if elapsed time exceeds the budget
    #[inline]
    pub fn is_over_budget(self, elapsed: Milliseconds) -> bool {
        elapsed > self.target_ms
    }

    /// Calculate budget utilization (0.0 to 1.0+)
    #[inline]
    pub fn utilization(self, elapsed: Milliseconds) -> f64 {
        elapsed.value() / self.target_ms.value()
    }

    /// Check if deadline is near (>80% budget used)
    #[inline]
    pub fn is_deadline_near(self, elapsed: Milliseconds) -> bool {
        self.utilization(elapsed) >= 0.8
    }

    /// Check if elapsed time indicates a janky frame.
    ///
    /// A frame is considered janky if it exceeds the target frame duration.
    /// For 60 FPS, any frame taking longer than ~16.67ms is janky.
    #[inline]
    pub fn is_janky(self, elapsed: Milliseconds) -> bool {
        elapsed.value() > self.target_ms.value()
    }
}

impl Default for FrameDuration {
    fn default() -> Self {
        Self::FPS_60
    }
}

impl fmt::Display for FrameDuration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({:.1} FPS)", self.target_ms, self.fps())
    }
}

// =============================================================================
// Percentage Newtype
// =============================================================================

/// Type-safe percentage wrapper (0.0 to 100.0)
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Percentage(f64);

impl Percentage {
    /// Zero percent
    pub const ZERO: Self = Self(0.0);

    /// 100 percent
    pub const HUNDRED: Self = Self(100.0);

    /// Create a new percentage value
    #[inline]
    pub const fn new(percent: f64) -> Self {
        Self(percent)
    }

    /// Create from a ratio (0.0 to 1.0)
    #[inline]
    pub fn from_ratio(ratio: f64) -> Self {
        Self(ratio * 100.0)
    }

    /// Get the raw percentage value (0.0 to 100.0)
    #[inline]
    pub const fn value(self) -> f64 {
        self.0
    }

    /// Get as a ratio (0.0 to 1.0)
    #[inline]
    pub fn as_ratio(self) -> f64 {
        self.0 / 100.0
    }

    /// Clamp to valid range (0-100)
    #[inline]
    pub fn clamped(self) -> Self {
        Self(self.0.clamp(0.0, 100.0))
    }
}

impl fmt::Display for Percentage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:.1}%", self.0)
    }
}

impl From<f64> for Percentage {
    #[inline]
    fn from(value: f64) -> Self {
        Self(value)
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_milliseconds_conversion() {
        let ms = Milliseconds::new(1000.0);
        let secs = ms.to_seconds();
        assert_eq!(secs.value(), 1.0);

        let back: Milliseconds = secs.into();
        assert_eq!(back.value(), 1000.0);
    }

    #[test]
    fn test_milliseconds_arithmetic() {
        let a = Milliseconds::new(10.0);
        let b = Milliseconds::new(5.0);

        assert_eq!((a + b).value(), 15.0);
        assert_eq!((a - b).value(), 5.0);
        assert_eq!((a * 2.0).value(), 20.0);
        assert_eq!((a / 2.0).value(), 5.0);
    }

    #[test]
    fn test_saturating_sub() {
        let a = Milliseconds::new(5.0);
        let b = Milliseconds::new(10.0);

        assert_eq!(a.saturating_sub(b), Milliseconds::ZERO);
        assert_eq!(b.saturating_sub(a).value(), 5.0);
    }

    #[test]
    fn test_frame_duration() {
        let budget = FrameDuration::from_fps(60);

        assert!((budget.as_ms().value() - 16.667).abs() < 0.001);
        assert!((budget.fps() - 60.0).abs() < 0.1);

        let elapsed = Milliseconds::new(10.0);
        assert!(!budget.is_over_budget(elapsed));
        assert_eq!(
            budget.remaining(elapsed).value(),
            budget.as_ms().value() - 10.0
        );

        let over = Milliseconds::new(20.0);
        assert!(budget.is_over_budget(over));
        assert_eq!(budget.remaining(over), Milliseconds::ZERO);
    }

    #[test]
    fn test_frame_duration_constants() {
        assert!((FrameDuration::FPS_30.fps() - 30.0).abs() < 0.1);
        assert!((FrameDuration::FPS_60.fps() - 60.0).abs() < 0.1);
        assert!((FrameDuration::FPS_120.fps() - 120.0).abs() < 0.1);
        assert!((FrameDuration::FPS_144.fps() - 144.0).abs() < 0.1);
    }

    #[test]
    fn test_percentage() {
        let p = Percentage::from_ratio(0.5);
        assert_eq!(p.value(), 50.0);
        assert_eq!(p.as_ratio(), 0.5);

        let clamped = Percentage::new(150.0).clamped();
        assert_eq!(clamped.value(), 100.0);
    }

    #[test]
    fn test_display() {
        assert_eq!(format!("{}", Milliseconds::new(16.67)), "16.67ms");
        assert_eq!(format!("{}", Seconds::new(1.234)), "1.234s");
        assert_eq!(format!("{}", Microseconds::new(1000)), "1000μs");
        assert_eq!(format!("{}", Percentage::new(75.5)), "75.5%");
    }

    #[test]
    fn test_microseconds_to_std_duration() {
        let us = Microseconds::new(1000);
        let duration = us.to_std_duration();
        assert_eq!(duration.as_micros(), 1000);

        // Zero should work
        let zero = Microseconds::ZERO.to_std_duration();
        assert_eq!(zero.as_micros(), 0);
    }

    #[test]
    fn test_try_to_std_duration() {
        let positive = Microseconds::new(1000);
        assert_eq!(
            positive.try_to_std_duration(),
            Some(std::time::Duration::from_micros(1000))
        );

        let zero = Microseconds::ZERO;
        assert_eq!(zero.try_to_std_duration(), Some(std::time::Duration::ZERO));

        let negative = Microseconds::new(-100);
        assert_eq!(negative.try_to_std_duration(), None);
    }

    #[test]
    #[should_panic(expected = "Cannot convert negative Microseconds")]
    fn test_negative_microseconds_panics() {
        let negative = Microseconds::new(-100);
        let _ = negative.to_std_duration();
    }
}
