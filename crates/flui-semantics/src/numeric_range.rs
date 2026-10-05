//! Validated numeric metadata exposed to native range controls.

use thiserror::Error;

/// A finite numeric value and its inclusive range.
///
/// `step` describes keyboard and increment/decrement proposals. It does not
/// quantize a value requested by assistive technology. A zero-span range is
/// valid and remains inert for adjustment proposals.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NumericRange {
    value: f64,
    min: f64,
    max: f64,
    step: f64,
}

/// Why a numeric range could not be admitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum NumericRangeError {
    /// A value, bound, or step was non-finite.
    #[error("numeric range values must be finite")]
    NonFinite,
    /// The minimum was greater than the maximum.
    #[error("numeric range minimum exceeds its maximum")]
    ReversedBounds,
    /// The current value lies outside the inclusive bounds.
    #[error("numeric range value lies outside its bounds")]
    ValueOutOfRange,
    /// The adjustment step was zero or negative.
    #[error("numeric range step must be positive")]
    NonPositiveStep,
}

impl NumericRange {
    /// Admits finite metadata with `min <= value <= max` and `step > 0`.
    ///
    /// # Errors
    /// Returns the violated admission rule; no invalid range is constructed.
    pub fn new(value: f64, min: f64, max: f64, step: f64) -> Result<Self, NumericRangeError> {
        if ![value, min, max, step].into_iter().all(f64::is_finite) {
            return Err(NumericRangeError::NonFinite);
        }
        if min > max {
            return Err(NumericRangeError::ReversedBounds);
        }
        if value < min || value > max {
            return Err(NumericRangeError::ValueOutOfRange);
        }
        if step <= 0.0 {
            return Err(NumericRangeError::NonPositiveStep);
        }
        Ok(Self {
            value,
            min,
            max,
            step,
        })
    }

    /// Current admitted value.
    #[must_use]
    pub const fn value(self) -> f64 {
        self.value
    }
    /// Inclusive minimum.
    #[must_use]
    pub const fn min(self) -> f64 {
        self.min
    }
    /// Inclusive maximum.
    #[must_use]
    pub const fn max(self) -> f64 {
        self.max
    }
    /// Positive increment/decrement step.
    #[must_use]
    pub const fn step(self) -> f64 {
        self.step
    }
    /// Whether an exact value is finite and inside the inclusive range.
    #[must_use]
    pub fn contains(self, value: f64) -> bool {
        value.is_finite() && value >= self.min && value <= self.max
    }
}
