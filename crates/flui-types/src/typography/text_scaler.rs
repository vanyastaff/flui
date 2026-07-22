//! Text scaling for accessibility.
//!
//! This module provides the [`TextScaler`] trait and implementations for
//! scaling text sizes to support accessibility features like system-wide
//! text scaling.
//!
//! # Examples
//!
//! ```
//! use flui_types::typography::{LinearTextScaler, NoScaling, TextScaler};
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
/// Implementations can provide linear scaling, non-linear scaling for large
/// text, or no scaling at all.
///
/// # Thread Safety
///
/// All implementations must be `Send + Sync` for use across threads.
///
/// # Examples
///
/// ```
/// use flui_types::typography::{LinearTextScaler, TextScaler};
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

    /// Returns true if this scaler is the identity (scale factor ~1.0).
    #[inline]
    fn is_identity(&self) -> bool {
        (self.text_scale_factor() - 1.0).abs() < f64::EPSILON
    }

    /// Clones this scaler into a boxed trait object.
    fn clone_box(&self) -> Box<dyn TextScaler>;
}

impl Clone for Box<dyn TextScaler> {
    #[inline]
    fn clone(&self) -> Self {
        self.clone_box()
    }
}

/// A [`TextScaler`] that multiplies every font size by a constant factor.
///
/// This is the default scaling model, equivalent to Flutter's
/// `TextScaler.linear`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LinearTextScaler {
    factor: f64,
}

impl LinearTextScaler {
    /// Creates a linear scaler with the given scale factor.
    ///
    /// # Panics
    ///
    /// Panics if `factor` is negative or NaN.
    #[must_use]
    #[inline]
    pub fn new(factor: f64) -> Self {
        assert!(
            factor >= 0.0 && !factor.is_nan(),
            "Scale factor must be non-negative and not NaN"
        );
        Self { factor }
    }

    /// Creates the identity scaler (factor 1.0, no scaling).
    #[must_use]
    #[inline]
    pub const fn identity() -> Self {
        Self { factor: 1.0 }
    }

    /// Returns the scale factor.
    #[must_use]
    #[inline]
    pub const fn factor(&self) -> f64 {
        self.factor
    }
}

impl Default for LinearTextScaler {
    #[inline]
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

    #[inline]
    fn clone_box(&self) -> Box<dyn TextScaler> {
        Box::new(*self)
    }
}

/// A [`TextScaler`] that never scales: every font size is returned unchanged.
///
/// Use this to opt out of system text scaling, equivalent to Flutter's
/// `TextScaler.noScaling`.
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

    #[inline]
    fn clone_box(&self) -> Box<dyn TextScaler> {
        Box::new(*self)
    }
}

/// A [`TextScaler`] that applies different factors to small and large text.
///
/// Font sizes below `threshold` are multiplied by `small_factor`; sizes at
/// or above it by `large_factor`. This supports non-linear accessibility
/// scaling where already-large text is scaled less aggressively.
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
    /// Creates a clamped scaler with the given small/large factors and size threshold.
    ///
    /// # Panics
    ///
    /// Panics if either factor is negative or NaN, or if `threshold` is
    /// not strictly positive.
    #[must_use]
    #[inline]
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

    /// Returns the scale factor applied to sizes below the threshold.
    #[must_use]
    #[inline]
    pub const fn small_factor(&self) -> f64 {
        self.small_factor
    }

    /// Returns the scale factor applied to sizes at or above the threshold.
    #[must_use]
    #[inline]
    pub const fn large_factor(&self) -> f64 {
        self.large_factor
    }

    /// Returns the font size threshold that separates small from large text.
    #[must_use]
    #[inline]
    pub const fn threshold(&self) -> f64 {
        self.threshold
    }
}

impl TextScaler for ClampedTextScaler {
    #[inline]
    fn scale(&self, font_size: f64) -> f64 {
        if font_size < self.threshold {
            font_size * self.small_factor
        } else {
            font_size * self.large_factor
        }
    }

    #[inline]
    fn text_scale_factor(&self) -> f64 {
        // Return the small factor as the "typical" scale
        self.small_factor
    }

    #[inline]
    fn clone_box(&self) -> Box<dyn TextScaler> {
        Box::new(*self)
    }
}
