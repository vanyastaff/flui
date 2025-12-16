//! Text scaling for accessibility.
//!
//! This module provides the [`TextScaler`] trait and implementations for
//! scaling text sizes to support accessibility features like system-wide
//! text scaling.
//!
//! # Examples
//!
//! ```
//! use flui_types::typography::{TextScaler, LinearTextScaler, NoScaling};
//!
//! // Linear scaling (default)
//! let scaler = LinearTextScaler::new(1.5);
//! assert_eq!(scaler.scale(16.0), 24.0);
//!
//! // No scaling (opt-out)
//! let no_scale = NoScaling;
//! assert_eq!(no_scale.scale(16.0), 16.0);
//! ```

use std::fmt::Debug;

/// Trait for scaling text sizes.
///
/// This trait enables accessibility features like system-wide text scaling.
/// Implementations can provide linear scaling, non-linear scaling for large text,
/// or no scaling at all.
///
/// # Thread Safety
///
/// All implementations must be `Send + Sync` for use across threads.
///
/// # Examples
///
/// ```
/// use flui_types::typography::{TextScaler, LinearTextScaler};
///
/// fn layout_text(font_size: f64, scaler: &dyn TextScaler) -> f64 {
///     scaler.scale(font_size)
/// }
///
/// let scaler = LinearTextScaler::new(1.5);
/// assert_eq!(layout_text(16.0, &scaler), 24.0);
/// ```
pub trait TextScaler: Debug + Send + Sync {
    /// Scales a font size.
    ///
    /// # Arguments
    ///
    /// * `font_size` - The original font size in logical pixels.
    ///
    /// # Returns
    ///
    /// The scaled font size.
    fn scale(&self, font_size: f64) -> f64;

    /// Returns the base text scale factor.
    ///
    /// For linear scaling, this is the multiplier applied to all sizes.
    /// For non-linear scaling, this represents the "typical" scale factor.
    fn text_scale_factor(&self) -> f64;

    /// Returns true if this scaler applies no scaling (factor = 1.0).
    #[inline]
    fn is_identity(&self) -> bool {
        (self.text_scale_factor() - 1.0).abs() < f64::EPSILON
    }

    /// Clones this scaler into a boxed trait object.
    fn clone_box(&self) -> Box<dyn TextScaler>;
}

impl Clone for Box<dyn TextScaler> {
    fn clone(&self) -> Self {
        self.clone_box()
    }
}

/// Linear text scaler that multiplies font sizes by a constant factor.
///
/// This is the most common scaling strategy, where all text sizes are
/// scaled by the same factor.
///
/// # Examples
///
/// ```
/// use flui_types::typography::{TextScaler, LinearTextScaler};
///
/// let scaler = LinearTextScaler::new(1.5);
/// assert_eq!(scaler.scale(10.0), 15.0);
/// assert_eq!(scaler.scale(20.0), 30.0);
/// assert_eq!(scaler.text_scale_factor(), 1.5);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LinearTextScaler {
    factor: f64,
}

impl LinearTextScaler {
    /// Creates a new linear text scaler with the given factor.
    ///
    /// # Arguments
    ///
    /// * `factor` - The scale factor (1.0 = no scaling, 2.0 = double size).
    ///
    /// # Panics
    ///
    /// Panics if `factor` is negative or NaN.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::typography::LinearTextScaler;
    ///
    /// let scaler = LinearTextScaler::new(1.5);
    /// ```
    #[must_use]
    pub fn new(factor: f64) -> Self {
        assert!(
            factor >= 0.0 && !factor.is_nan(),
            "Scale factor must be non-negative and not NaN"
        );
        Self { factor }
    }

    /// Creates a linear text scaler with no scaling (factor = 1.0).
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::typography::{TextScaler, LinearTextScaler};
    ///
    /// let scaler = LinearTextScaler::identity();
    /// assert!(scaler.is_identity());
    /// assert_eq!(scaler.scale(16.0), 16.0);
    /// ```
    #[must_use]
    pub const fn identity() -> Self {
        Self { factor: 1.0 }
    }

    /// Returns the scale factor.
    #[inline]
    #[must_use]
    pub const fn factor(&self) -> f64 {
        self.factor
    }
}

impl Default for LinearTextScaler {
    fn default() -> Self {
        Self::identity()
    }
}

impl TextScaler for LinearTextScaler {
    #[inline]
    fn scale(&self, font_size: f64) -> f64 {
        font_size * self.factor
    }

    #[inline]
    fn text_scale_factor(&self) -> f64 {
        self.factor
    }

    fn clone_box(&self) -> Box<dyn TextScaler> {
        Box::new(*self)
    }
}

/// No-op text scaler that returns font sizes unchanged.
///
/// Use this when you want to opt out of system text scaling for
/// specific text elements (e.g., fixed-size icons or badges).
///
/// # Examples
///
/// ```
/// use flui_types::typography::{TextScaler, NoScaling};
///
/// let scaler = NoScaling;
/// assert_eq!(scaler.scale(16.0), 16.0);
/// assert_eq!(scaler.scale(100.0), 100.0);
/// assert!(scaler.is_identity());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct NoScaling;

impl TextScaler for NoScaling {
    #[inline]
    fn scale(&self, font_size: f64) -> f64 {
        font_size
    }

    #[inline]
    fn text_scale_factor(&self) -> f64 {
        1.0
    }

    fn clone_box(&self) -> Box<dyn TextScaler> {
        Box::new(*self)
    }
}

/// Clamped text scaler that limits scaling for large text.
///
/// This scaler applies a different (usually smaller) scale factor
/// to text larger than a threshold. This is useful for accessibility
/// where very large text shouldn't scale as aggressively.
///
/// # Examples
///
/// ```
/// use flui_types::typography::{TextScaler, ClampedTextScaler};
///
/// // Scale small text by 2x, but large text (>24) only by 1.5x
/// let scaler = ClampedTextScaler::new(2.0, 1.5, 24.0);
///
/// assert_eq!(scaler.scale(16.0), 32.0);  // 16 * 2.0
/// assert_eq!(scaler.scale(32.0), 48.0);  // 32 * 1.5
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClampedTextScaler {
    /// Scale factor for small text.
    small_factor: f64,
    /// Scale factor for large text.
    large_factor: f64,
    /// Threshold size above which large_factor is used.
    threshold: f64,
}

impl ClampedTextScaler {
    /// Creates a new clamped text scaler.
    ///
    /// # Arguments
    ///
    /// * `small_factor` - Scale factor for text smaller than threshold.
    /// * `large_factor` - Scale factor for text at or larger than threshold.
    /// * `threshold` - Font size threshold for switching scale factors.
    ///
    /// # Panics
    ///
    /// Panics if any factor is negative or NaN, or if threshold is not positive.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::typography::ClampedTextScaler;
    ///
    /// let scaler = ClampedTextScaler::new(2.0, 1.5, 24.0);
    /// ```
    #[must_use]
    pub fn new(small_factor: f64, large_factor: f64, threshold: f64) -> Self {
        assert!(
            small_factor >= 0.0 && !small_factor.is_nan(),
            "small_factor must be non-negative and not NaN"
        );
        assert!(
            large_factor >= 0.0 && !large_factor.is_nan(),
            "large_factor must be non-negative and not NaN"
        );
        assert!(
            threshold > 0.0 && !threshold.is_nan(),
            "threshold must be positive and not NaN"
        );
        Self {
            small_factor,
            large_factor,
            threshold,
        }
    }

    /// Returns the small text scale factor.
    #[inline]
    #[must_use]
    pub const fn small_factor(&self) -> f64 {
        self.small_factor
    }

    /// Returns the large text scale factor.
    #[inline]
    #[must_use]
    pub const fn large_factor(&self) -> f64 {
        self.large_factor
    }

    /// Returns the threshold size.
    #[inline]
    #[must_use]
    pub const fn threshold(&self) -> f64 {
        self.threshold
    }
}

impl TextScaler for ClampedTextScaler {
    fn scale(&self, font_size: f64) -> f64 {
        if font_size < self.threshold {
            font_size * self.small_factor
        } else {
            font_size * self.large_factor
        }
    }

    fn text_scale_factor(&self) -> f64 {
        // Return the small factor as the "typical" scale
        self.small_factor
    }

    fn clone_box(&self) -> Box<dyn TextScaler> {
        Box::new(*self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_linear_text_scaler() {
        let scaler = LinearTextScaler::new(1.5);
        assert_eq!(scaler.scale(10.0), 15.0);
        assert_eq!(scaler.scale(20.0), 30.0);
        assert_eq!(scaler.text_scale_factor(), 1.5);
        assert!(!scaler.is_identity());
    }

    #[test]
    fn test_linear_text_scaler_identity() {
        let scaler = LinearTextScaler::identity();
        assert_eq!(scaler.scale(16.0), 16.0);
        assert!(scaler.is_identity());
    }

    #[test]
    fn test_linear_text_scaler_default() {
        let scaler = LinearTextScaler::default();
        assert!(scaler.is_identity());
    }

    #[test]
    fn test_no_scaling() {
        let scaler = NoScaling;
        assert_eq!(scaler.scale(16.0), 16.0);
        assert_eq!(scaler.scale(100.0), 100.0);
        assert_eq!(scaler.text_scale_factor(), 1.0);
        assert!(scaler.is_identity());
    }

    #[test]
    fn test_clamped_text_scaler() {
        let scaler = ClampedTextScaler::new(2.0, 1.5, 24.0);

        // Small text scales by 2x
        assert_eq!(scaler.scale(10.0), 20.0);
        assert_eq!(scaler.scale(20.0), 40.0);

        // Large text scales by 1.5x
        assert_eq!(scaler.scale(24.0), 36.0);
        assert_eq!(scaler.scale(32.0), 48.0);

        assert_eq!(scaler.text_scale_factor(), 2.0);
    }

    #[test]
    fn test_clamped_text_scaler_at_threshold() {
        let scaler = ClampedTextScaler::new(2.0, 1.5, 24.0);

        // At exactly threshold, use large factor
        assert_eq!(scaler.scale(24.0), 36.0);

        // Just below threshold, use small factor
        assert_eq!(scaler.scale(23.9), 47.8);
    }

    #[test]
    fn test_text_scaler_clone_box() {
        let scaler: Box<dyn TextScaler> = Box::new(LinearTextScaler::new(1.5));
        let cloned = scaler.clone();
        assert_eq!(cloned.scale(10.0), 15.0);
    }

    #[test]
    #[should_panic(expected = "Scale factor must be non-negative")]
    fn test_linear_text_scaler_negative_factor() {
        let _ = LinearTextScaler::new(-1.0);
    }

    #[test]
    #[should_panic(expected = "threshold must be positive")]
    fn test_clamped_text_scaler_zero_threshold() {
        let _ = ClampedTextScaler::new(1.5, 1.2, 0.0);
    }
}
