//! Box layout constraints: a min/max range per axis.
//!
//! Provides rectangular constraints for 2D box-based layout with
//! comprehensive query and transformation operations.

use std::{
    fmt,
    hash::{Hash, Hasher},
};

use flui_foundation::geometry::{EdgeInsets, Size, canonical_bits_f64};

use super::Constraints;

/// Immutable layout constraints for rectangular (box) layout.
///
/// A size satisfies BoxConstraints if and only if:
/// - `min_width <= width <= max_width`
/// - `min_height <= height <= max_height`
///
/// # Cache Support
///
/// Implements `Hash` and `Eq` for use as cache keys. Use `round_for_cache()` before
/// caching to ensure consistent floating-point comparisons:
///
/// ```ignore
/// let key = constraints.round_for_cache();
/// layout_cache.insert(key, computed_size);
/// ```
///
/// # Cache Rounding
///
/// The `round_for_cache()` method rounds floating-point values to 0.01 precision
/// (2 decimal places) to avoid cache thrashing from rounding errors while
/// maintaining sufficient accuracy for layout calculations.
#[derive(Clone, Copy, PartialEq)]
pub struct BoxConstraints {
    /// Minimum width that satisfies the constraints.
    pub min_width: f64,
    /// Maximum width that satisfies the constraints (may be infinite).
    pub max_width: f64,
    /// Minimum height that satisfies the constraints.
    pub min_height: f64,
    /// Maximum height that satisfies the constraints (may be infinite).
    pub max_height: f64,
}

// ============================================================================
// HASH + EQ FOR CACHING
// ============================================================================

impl Hash for BoxConstraints {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // Canonical bits, so constraints equal under `PartialEq` (`0.0` and
        // `-0.0`) hash equal.
        canonical_bits_f64(self.min_width).hash(state);
        canonical_bits_f64(self.max_width).hash(state);
        canonical_bits_f64(self.min_height).hash(state);
        canonical_bits_f64(self.max_height).hash(state);
    }
}

impl Eq for BoxConstraints {}

// ============================================================================
// CONSTRUCTORS
// ============================================================================

impl BoxConstraints {
    /// Unconstrained - allows any size.
    pub const UNCONSTRAINED: Self = Self {
        min_width: 0.0,
        max_width: f64::INFINITY,
        min_height: 0.0,
        max_height: f64::INFINITY,
    };

    /// Zero-sized constraints (tight at zero).
    pub const ZERO: Self = Self {
        min_width: 0.0,
        max_width: 0.0,
        min_height: 0.0,
        max_height: 0.0,
    };

    /// Creates new box constraints with explicit bounds.
    #[inline]
    #[must_use]
    pub const fn new(min_width: f64, max_width: f64, min_height: f64, max_height: f64) -> Self {
        Self {
            min_width,
            max_width,
            min_height,
            max_height,
        }
    }

    /// Creates tight constraints that force exactly the given size.
    #[inline]
    #[must_use]
    pub const fn tight(size: Size) -> Self {
        Self {
            min_width: size.width,
            max_width: size.width,
            min_height: size.height,
            max_height: size.height,
        }
    }

    /// Creates loose constraints allowing from zero to the given size.
    #[inline]
    #[must_use]
    pub const fn loose(size: Size) -> Self {
        Self {
            min_width: 0.0,
            max_width: size.width,
            min_height: 0.0,
            max_height: size.height,
        }
    }

    /// Creates expand constraints that force maximum size (fill parent).
    #[inline]
    #[must_use]
    pub const fn expand() -> Self {
        Self {
            min_width: f64::INFINITY,
            max_width: f64::INFINITY,
            min_height: f64::INFINITY,
            max_height: f64::INFINITY,
        }
    }

    /// Creates constraints with optional tight dimensions.
    ///
    /// Tight dimensions use the given value for both min and max.
    /// Loose dimensions allow any size.
    #[inline]
    #[must_use]
    pub const fn tight_for(width: Option<f64>, height: Option<f64>) -> Self {
        Self {
            min_width: match width {
                Some(w) => w,
                None => 0.0,
            },
            max_width: match width {
                Some(w) => w,
                None => f64::INFINITY,
            },
            min_height: match height {
                Some(h) => h,
                None => 0.0,
            },
            max_height: match height {
                Some(h) => h,
                None => f64::INFINITY,
            },
        }
    }

    /// Creates constraints that tighten each dimension to its value only when
    /// that value is finite, leaving infinite dimensions unconstrained.
    ///
    /// Used by intrinsic-dimension probes that pass `f64::INFINITY` for the
    /// axis they are not constraining.
    #[inline]
    #[must_use]
    pub fn tight_for_finite(width: f64, height: f64) -> Self {
        Self {
            min_width: if width.is_finite() { width } else { 0.0 },
            max_width: if width.is_finite() {
                width
            } else {
                f64::INFINITY
            },
            min_height: if height.is_finite() { height } else { 0.0 },
            max_height: if height.is_finite() {
                height
            } else {
                f64::INFINITY
            },
        }
    }

    // ============================================================================
    // NORMALIZATION FOR CACHING
    // ============================================================================

    /// Rounds constraints for use as cache keys.
    ///
    /// Rounds finite values to 0.01 precision (2 decimal places).
    /// Infinite values are preserved unchanged.
    ///
    // TODO: a real normalize() (min≥0, max≥min) can live here if a caller needs it
    #[inline]
    #[must_use]
    pub fn round_for_cache(&self) -> Self {
        Self {
            min_width: round_pixels_to_hundredths(self.min_width),
            max_width: round_pixels_to_hundredths(self.max_width),
            min_height: round_pixels_to_hundredths(self.min_height),
            max_height: round_pixels_to_hundredths(self.max_height),
        }
    }

    /// Checks if constraints are already rounded for caching.
    ///
    /// More efficient than comparing with `round_for_cache()` as it checks
    /// each field individually.
    #[inline]
    #[must_use]
    pub fn is_rounded_for_cache(&self) -> bool {
        is_pixels_normalized(self.min_width)
            && is_pixels_normalized(self.max_width)
            && is_pixels_normalized(self.min_height)
            && is_pixels_normalized(self.max_height)
    }

    // ============================================================================
    // CONSTRAINT QUERIES
    // ============================================================================

    /// Returns whether width is tight (min == max).
    #[inline]
    #[must_use]
    pub fn has_tight_width(&self) -> bool {
        self.min_width >= self.max_width
    }

    /// Returns whether height is tight (min == max).
    #[inline]
    #[must_use]
    pub fn has_tight_height(&self) -> bool {
        self.min_height >= self.max_height
    }

    /// Returns whether width has an upper bound.
    #[inline]
    #[must_use]
    pub fn has_bounded_width(&self) -> bool {
        self.max_width.is_finite()
    }

    /// Returns whether height has an upper bound.
    #[inline]
    #[must_use]
    pub fn has_bounded_height(&self) -> bool {
        self.max_height.is_finite()
    }

    /// Returns whether width constraint is infinite.
    #[inline]
    #[must_use]
    pub fn has_infinite_width(&self) -> bool {
        self.min_width.is_infinite()
    }

    /// Returns whether height constraint is infinite.
    #[inline]
    #[must_use]
    pub fn has_infinite_height(&self) -> bool {
        self.min_height.is_infinite()
    }

    /// Returns whether width is loose (min == 0).
    #[inline]
    #[must_use]
    pub fn has_loose_width(&self) -> bool {
        self.min_width <= 0.0
    }

    /// Returns whether height is loose (min == 0).
    #[inline]
    #[must_use]
    pub fn has_loose_height(&self) -> bool {
        self.min_height <= 0.0
    }

    /// Returns whether constraints are loose in both dimensions.
    #[inline]
    #[must_use]
    pub fn is_loose(&self) -> bool {
        self.has_loose_width() && self.has_loose_height()
    }

    // ============================================================================
    // SIZE OPERATIONS
    // ============================================================================

    /// Returns the largest size that satisfies the constraints.
    #[inline]
    #[must_use]
    pub fn biggest(&self) -> Size {
        Size::new(
            self.constrain_width(f64::INFINITY),
            self.constrain_height(f64::INFINITY),
        )
    }

    /// Returns the smallest size that satisfies the constraints.
    #[inline]
    #[must_use]
    pub fn smallest(&self) -> Size {
        Size::new(self.min_width, self.min_height)
    }

    /// Constrains width to be within bounds.
    #[inline]
    #[must_use]
    pub fn constrain_width(&self, width: f64) -> f64 {
        width.clamp(self.min_width, self.max_width)
    }

    /// Constrains height to be within bounds.
    #[inline]
    #[must_use]
    pub fn constrain_height(&self, height: f64) -> f64 {
        height.clamp(self.min_height, self.max_height)
    }

    /// Constrains a size to satisfy these constraints.
    #[inline]
    #[must_use]
    pub fn constrain(&self, size: Size) -> Size {
        Size::new(
            self.constrain_width(size.width),
            self.constrain_height(size.height),
        )
    }

    /// Checks if a size satisfies these constraints.
    #[inline]
    #[must_use]
    pub fn is_satisfied_by(&self, size: Size) -> bool {
        size.width >= self.min_width
            && size.width <= self.max_width
            && size.height >= self.min_height
            && size.height <= self.max_height
    }

    /// Constrains a size while attempting to preserve aspect ratio.
    ///
    /// Given a natural size (e.g., image dimensions), this method finds the
    /// largest size that:
    /// 1. Preserves the original aspect ratio
    /// 2. Fits within these constraints
    ///
    /// If the natural size is zero, returns the constrained zero size.
    /// If an axis is unbounded, the natural size is kept on that axis (subject to min constraints).
    #[must_use]
    pub fn constrain_size_and_attempt_to_preserve_aspect_ratio(&self, size: Size) -> Size {
        // Tight constraints fix the size outright.
        if self.is_tight() {
            return self.smallest();
        }

        let mut width = size.width;
        let mut height = size.height;

        // A degenerate aspect source has no ratio to preserve — just constrain.
        if width <= 0.0 || height <= 0.0 {
            return self.constrain(size);
        }

        let aspect_ratio = width / height;
        let min_w = self.min_width;
        let max_w = self.max_width;
        let min_h = self.min_height;
        let max_h = self.max_height;

        // Adjust each out-of-range dimension and bring the OTHER dimension along
        // to keep the ratio. Order (max then min, width then height) matters:
        // clamping a dimension up to its min and then "scaling down" the same
        // dimension would get re-clamped and never grow its partner.
        if width > max_w {
            width = max_w;
            height = width / aspect_ratio;
        }
        if height > max_h {
            height = max_h;
            width = height * aspect_ratio;
        }
        if width < min_w {
            width = min_w;
            height = width / aspect_ratio;
        }
        if height < min_h {
            height = min_h;
            width = height * aspect_ratio;
        }

        Size::new(self.constrain_width(width), self.constrain_height(height))
    }

    // ============================================================================
    // TRANSFORMATION OPERATIONS
    // ============================================================================

    /// Deflates constraints by edge insets.
    ///
    /// Reduces available space by insets (padding, borders, etc.).
    /// Clamps to zero if insets exceed available space.
    #[inline]
    #[must_use]
    pub fn deflate(&self, insets: EdgeInsets) -> Self {
        let horizontal = insets.left + insets.right;
        let vertical = insets.top + insets.bottom;

        Self {
            min_width: (self.min_width - horizontal).max(0.0),
            max_width: (self.max_width - horizontal).max(0.0),
            min_height: (self.min_height - vertical).max(0.0),
            max_height: (self.max_height - vertical).max(0.0),
        }
    }

    /// Inflates constraints by edge insets.
    ///
    /// Adds space for insets. Preserves infinity.
    #[inline]
    #[must_use]
    pub fn inflate(&self, insets: EdgeInsets) -> Self {
        let horizontal = insets.left + insets.right;
        let vertical = insets.top + insets.bottom;

        Self {
            min_width: self.min_width + horizontal,
            max_width: if self.max_width.is_finite() {
                self.max_width + horizontal
            } else {
                self.max_width
            },
            min_height: self.min_height + vertical,
            max_height: if self.max_height.is_finite() {
                self.max_height + vertical
            } else {
                self.max_height
            },
        }
    }

    /// Loosens constraints by removing minimums.
    #[inline]
    #[must_use]
    pub fn loosen(&self) -> Self {
        Self {
            min_width: 0.0,
            max_width: self.max_width,
            min_height: 0.0,
            max_height: self.max_height,
        }
    }

    /// Tightens constraints to specific dimensions.
    ///
    /// Sets both min and max to the given value for specified dimensions,
    /// clamped to existing bounds to maintain invariant (min <= max).
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let constraints = BoxConstraints {
    ///     min_width: 0,
    ///     max_width: 100,
    ///     min_height: 0,
    ///     max_height: 100,
    /// };
    /// let tight = constraints.tighten(Some(500), None);
    /// // Result: min_width=100, max_width=100 (clamped to existing max)
    /// ```
    #[inline]
    #[must_use]
    pub fn tighten(&self, width: Option<f64>, height: Option<f64>) -> Self {
        Self {
            min_width: width.map_or(self.min_width, |w| w.clamp(self.min_width, self.max_width)),
            max_width: width.map_or(self.max_width, |w| w.clamp(self.min_width, self.max_width)),
            min_height: height.map_or(self.min_height, |h| {
                h.clamp(self.min_height, self.max_height)
            }),
            max_height: height.map_or(self.max_height, |h| {
                h.clamp(self.min_height, self.max_height)
            }),
        }
    }

    /// Swaps the width and height axes of these constraints.
    ///
    /// Returns constraints with `min_width ↔ min_height` and `max_width ↔ max_height`
    /// exchanged.  Used by `RenderRotatedBox` to pass rotated constraints to the
    /// child when the quarter-turns count is odd (90° or 270°): the child must
    /// fill the space that appears as the parent's *height* but, from the child's
    /// coordinate frame, is its *width*.
    #[inline]
    #[must_use]
    pub fn flipped(self) -> Self {
        Self {
            min_width: self.min_height,
            max_width: self.max_height,
            min_height: self.min_width,
            max_height: self.max_width,
        }
    }

    /// Returns these constraints clamped to fit within `other`'s bounds, while
    /// staying as close as possible to the originals.
    ///
    /// `a.enforce(b)` clamps **`a`'s own** values into `b`'s `[min, max]` range —
    /// the argument's bounds win — so `additional.enforce(parent)` keeps the
    /// parent's hard limits. Clamping `other`'s values into `self`'s range
    /// (the reverse) would let additional constraints override the parent and
    /// silently oversize children whenever the two ranges did not overlap.
    #[inline]
    #[must_use]
    pub fn enforce(&self, other: &Self) -> Self {
        Self {
            min_width: other.constrain_width(self.min_width),
            max_width: other.constrain_width(self.max_width),
            min_height: other.constrain_height(self.min_height),
            max_height: other.constrain_height(self.max_height),
        }
    }

    // ============================================================================
    // BUILDER PATTERN
    // ============================================================================

    /// Sets minimum width.
    #[inline]
    #[must_use]
    pub const fn with_min_width(mut self, min_width: f64) -> Self {
        self.min_width = min_width;
        self
    }

    /// Sets maximum width.
    #[inline]
    #[must_use]
    pub const fn with_max_width(mut self, max_width: f64) -> Self {
        self.max_width = max_width;
        self
    }

    /// Sets minimum height.
    #[inline]
    #[must_use]
    pub const fn with_min_height(mut self, min_height: f64) -> Self {
        self.min_height = min_height;
        self
    }

    /// Sets maximum height.
    #[inline]
    #[must_use]
    pub const fn with_max_height(mut self, max_height: f64) -> Self {
        self.max_height = max_height;
        self
    }

    /// Sets tight width (min == max).
    #[inline]
    #[must_use]
    pub const fn with_tight_width(mut self, width: f64) -> Self {
        self.min_width = width;
        self.max_width = width;
        self
    }

    /// Sets tight height (min == max).
    #[inline]
    #[must_use]
    pub const fn with_tight_height(mut self, height: f64) -> Self {
        self.min_height = height;
        self.max_height = height;
        self
    }

    // ============================================================================
    // SET OPERATIONS
    // ============================================================================

    /// Computes intersection of two constraint sets.
    ///
    /// Returns constraints that satisfy both inputs, or `None` if
    /// no such constraints exist.
    #[must_use]
    pub fn intersection(&self, other: &Self) -> Option<Self> {
        let min_width = self.min_width.max(other.min_width);
        let max_width = self.max_width.min(other.max_width);
        let min_height = self.min_height.max(other.min_height);
        let max_height = self.max_height.min(other.max_height);

        if min_width <= max_width && min_height <= max_height {
            Some(Self {
                min_width,
                max_width,
                min_height,
                max_height,
            })
        } else {
            None
        }
    }

    /// Computes union of two constraint sets.
    ///
    /// Returns constraints that satisfy at least one input.
    #[inline]
    #[must_use]
    pub fn union(&self, other: &Self) -> Self {
        Self {
            min_width: self.min_width.min(other.min_width),
            max_width: self.max_width.max(other.max_width),
            min_height: self.min_height.min(other.min_height),
            max_height: self.max_height.max(other.max_height),
        }
    }

    /// Checks if these constraints contain another set.
    ///
    /// Returns true if all sizes satisfying `other` also satisfy `self`.
    #[inline]
    #[must_use]
    pub fn contains(&self, other: &Self) -> bool {
        self.min_width <= other.min_width
            && self.max_width >= other.max_width
            && self.min_height <= other.min_height
            && self.max_height >= other.max_height
    }

    /// Checks if constraint sets overlap.
    ///
    /// Returns true if there exists any size satisfying both constraints.
    #[inline]
    #[must_use]
    pub fn overlaps(&self, other: &Self) -> bool {
        self.intersection(other).is_some()
    }

    // ============================================================================
    // UTILITY METHODS
    // ============================================================================

    /// Returns maximum possible area as raw f64.
    #[inline]
    #[must_use]
    pub fn max_area(&self) -> f64 {
        self.max_width * self.max_height
    }

    /// Returns minimum required area as raw f64.
    #[inline]
    #[must_use]
    pub fn min_area(&self) -> f64 {
        self.min_width * self.min_height
    }

    /// Returns maximum diagonal length as raw f64.
    #[inline]
    #[must_use]
    pub fn max_diagonal(&self) -> f64 {
        let w = self.max_width;
        let h = self.max_height;
        (w * w + h * h).sqrt()
    }

    /// Clamps constraints to fit within bounds.
    #[inline]
    #[must_use]
    pub fn clamp_to(&self, bounds: &Self) -> Self {
        Self {
            min_width: self.min_width.clamp(bounds.min_width, bounds.max_width),
            max_width: self.max_width.clamp(bounds.min_width, bounds.max_width),
            min_height: self.min_height.clamp(bounds.min_height, bounds.max_height),
            max_height: self.max_height.clamp(bounds.min_height, bounds.max_height),
        }
    }

    /// Returns width range as tuple.
    #[inline]
    #[must_use]
    pub const fn width_range(&self) -> (f64, f64) {
        (self.min_width, self.max_width)
    }

    /// Returns height range as tuple.
    #[inline]
    #[must_use]
    pub const fn height_range(&self) -> (f64, f64) {
        (self.min_height, self.max_height)
    }

    /// Maps a function over all constraint values.
    #[inline]
    #[must_use]
    pub fn map<F>(&self, f: F) -> Self
    where
        F: Fn(f64) -> f64,
    {
        Self {
            min_width: f(self.min_width),
            max_width: f(self.max_width),
            min_height: f(self.min_height),
            max_height: f(self.max_height),
        }
    }

    /// Rounds all constraint values.
    #[inline]
    #[must_use]
    pub fn round(&self) -> Self {
        self.map(f64::round)
    }

    /// Floors all constraint values.
    #[inline]
    #[must_use]
    pub fn floor(&self) -> Self {
        self.map(f64::floor)
    }

    /// Ceils all constraint values.
    #[inline]
    #[must_use]
    pub fn ceil(&self) -> Self {
        self.map(f64::ceil)
    }
}

// ============================================================================
// NORMALIZATION HELPERS
// ============================================================================

/// Rounds a logical length to hundredths precision.
#[inline]
fn round_pixels_to_hundredths(value: f64) -> f64 {
    if value.is_finite() {
        (value * 100.0).round() / 100.0
    } else {
        value
    }
}

/// Checks if a logical length is already normalized.
#[inline]
fn is_pixels_normalized(value: f64) -> bool {
    if value.is_finite() {
        value == round_pixels_to_hundredths(value)
    } else {
        true
    }
}

// ============================================================================
// TRAIT IMPLEMENTATIONS
// ============================================================================

impl Constraints for BoxConstraints {
    fn is_tight(&self) -> bool {
        self.has_tight_width() && self.has_tight_height()
    }

    fn is_normalized(&self) -> bool {
        self.min_width >= 0.0
            && self.min_height >= 0.0
            && self.min_width <= self.max_width
            && self.min_height <= self.max_height
            && !self.min_width.is_nan()
            && !self.max_width.is_nan()
            && !self.min_height.is_nan()
            && !self.max_height.is_nan()
    }
}

impl Default for BoxConstraints {
    fn default() -> Self {
        Self::UNCONSTRAINED
    }
}

impl fmt::Debug for BoxConstraints {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_tight() {
            write!(
                f,
                "BoxConstraints(tight: {}×{})",
                self.min_width, self.min_height
            )
        } else {
            write!(
                f,
                "BoxConstraints(w: {}..{}, h: {}..{})",
                self.min_width, self.max_width, self.min_height, self.max_height
            )
        }
    }
}

impl fmt::Display for BoxConstraints {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

// ============================================================================
// OPERATOR OVERLOADS
// ============================================================================

impl std::ops::Mul<f64> for BoxConstraints {
    type Output = Self;

    fn mul(self, scale: f64) -> Self {
        self.map(|v| v * scale)
    }
}

impl std::ops::Div<f64> for BoxConstraints {
    type Output = Self;

    fn div(self, scale: f64) -> Self {
        self.map(|v| v / scale)
    }
}

impl std::ops::MulAssign<f64> for BoxConstraints {
    fn mul_assign(&mut self, scale: f64) {
        *self = *self * scale;
    }
}

impl std::ops::DivAssign<f64> for BoxConstraints {
    fn div_assign(&mut self, scale: f64) {
        *self = *self / scale;
    }
}

// ============================================================================
// CONVERSIONS
// ============================================================================

impl From<Size> for BoxConstraints {
    fn from(size: Size) -> Self {
        Self::tight(size)
    }
}

impl From<(Size, Size)> for BoxConstraints {
    fn from((min, max): (Size, Size)) -> Self {
        Self {
            min_width: min.width,
            max_width: max.width,
            min_height: min.height,
            max_height: max.height,
        }
    }
}

impl From<BoxConstraints> for (f64, f64, f64, f64) {
    fn from(c: BoxConstraints) -> Self {
        (c.min_width, c.max_width, c.min_height, c.max_height)
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {}
