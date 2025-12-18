//! Box layout constraints following Flutter's proven model.
//!
//! Provides rectangular constraints for 2D box-based layout with
//! comprehensive query and transformation operations.

use super::Constraints;
use flui_types::{EdgeInsets, Size};
use std::fmt;
use std::hash::{Hash, Hasher};

/// Immutable layout constraints for rectangular (box) layout.
///
/// A size satisfies BoxConstraints if and only if:
/// - `min_width <= width <= max_width`
/// - `min_height <= height <= max_height`
///
/// # Cache Support
///
/// Implements `Hash` and `Eq` for use as cache keys. Use `normalize()` before
/// caching to ensure consistent floating-point comparisons:
///
/// ```ignore
/// let key = constraints.normalize();
/// layout_cache.insert(key, computed_size);
/// ```
///
/// # Normalization
///
/// The `normalize()` method rounds floating-point values to 0.01 precision
/// (2 decimal places) to avoid cache thrashing from rounding errors while
/// maintaining sufficient accuracy for layout calculations.
///
/// # SIMD Operations
///
/// When the `simd` feature is enabled, batch operations are available:
///
/// ```ignore
/// #[cfg(feature = "simd")]
/// {
///     let constrained = constraints.batch_constrain(&sizes);
///     let valid = constraints.batch_is_satisfied_by(&sizes);
/// }
/// ```
///
/// # Flutter Equivalence
///
/// Maps directly to Flutter's `BoxConstraints` class with identical semantics.
#[derive(Clone, Copy, PartialEq)]
pub struct BoxConstraints {
    /// Minimum width that satisfies the constraints.
    pub min_width: f32,
    /// Maximum width that satisfies the constraints (may be infinite).
    pub max_width: f32,
    /// Minimum height that satisfies the constraints.
    pub min_height: f32,
    /// Maximum height that satisfies the constraints (may be infinite).
    pub max_height: f32,
}

// ============================================================================
// HASH + EQ FOR CACHING
// ============================================================================

impl Hash for BoxConstraints {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // Hash as bit patterns (NaN-safe)
        self.min_width.to_bits().hash(state);
        self.max_width.to_bits().hash(state);
        self.min_height.to_bits().hash(state);
        self.max_height.to_bits().hash(state);
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
        max_width: f32::INFINITY,
        min_height: 0.0,
        max_height: f32::INFINITY,
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
    pub const fn new(min_width: f32, max_width: f32, min_height: f32, max_height: f32) -> Self {
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
            min_width: f32::INFINITY,
            max_width: f32::INFINITY,
            min_height: f32::INFINITY,
            max_height: f32::INFINITY,
        }
    }

    /// Creates constraints with optional tight dimensions.
    ///
    /// Tight dimensions use the given value for both min and max.
    /// Loose dimensions allow any size.
    #[inline]
    #[must_use]
    pub const fn tight_for(width: Option<f32>, height: Option<f32>) -> Self {
        Self {
            min_width: match width {
                Some(w) => w,
                None => 0.0,
            },
            max_width: match width {
                Some(w) => w,
                None => f32::INFINITY,
            },
            min_height: match height {
                Some(h) => h,
                None => 0.0,
            },
            max_height: match height {
                Some(h) => h,
                None => f32::INFINITY,
            },
        }
    }

    // ============================================================================
    // NORMALIZATION FOR CACHING
    // ============================================================================

    /// Normalizes constraints for use as cache keys.
    ///
    /// Rounds finite values to 0.01 precision (2 decimal places).
    /// Infinite values are preserved unchanged.
    #[inline]
    #[must_use]
    pub fn normalize(&self) -> Self {
        Self {
            min_width: round_to_hundredths(self.min_width),
            max_width: round_to_hundredths(self.max_width),
            min_height: round_to_hundredths(self.min_height),
            max_height: round_to_hundredths(self.max_height),
        }
    }

    /// Checks if constraints are already normalized.
    ///
    /// More efficient than comparing with `normalize()` as it checks
    /// each field individually.
    #[inline]
    #[must_use]
    pub fn is_normalized_for_cache(&self) -> bool {
        is_normalized(self.min_width)
            && is_normalized(self.max_width)
            && is_normalized(self.min_height)
            && is_normalized(self.max_height)
    }

    // ============================================================================
    // CONSTRAINT QUERIES
    // ============================================================================

    /// Returns whether width is tight (min == max).
    #[inline]
    #[must_use]
    pub const fn has_tight_width(&self) -> bool {
        self.min_width >= self.max_width
    }

    /// Returns whether height is tight (min == max).
    #[inline]
    #[must_use]
    pub const fn has_tight_height(&self) -> bool {
        self.min_height >= self.max_height
    }

    /// Returns whether width has an upper bound.
    #[inline]
    #[must_use]
    pub fn has_bounded_width(&self) -> bool {
        self.max_width < f32::INFINITY
    }

    /// Returns whether height has an upper bound.
    #[inline]
    #[must_use]
    pub fn has_bounded_height(&self) -> bool {
        self.max_height < f32::INFINITY
    }

    /// Returns whether width constraint is infinite.
    #[inline]
    #[must_use]
    pub fn has_infinite_width(&self) -> bool {
        self.min_width >= f32::INFINITY
    }

    /// Returns whether height constraint is infinite.
    #[inline]
    #[must_use]
    pub fn has_infinite_height(&self) -> bool {
        self.min_height >= f32::INFINITY
    }

    /// Returns whether width is loose (min == 0).
    #[inline]
    #[must_use]
    pub const fn has_loose_width(&self) -> bool {
        self.min_width <= 0.0
    }

    /// Returns whether height is loose (min == 0).
    #[inline]
    #[must_use]
    pub const fn has_loose_height(&self) -> bool {
        self.min_height <= 0.0
    }

    /// Returns whether constraints are loose in both dimensions.
    #[inline]
    #[must_use]
    pub const fn is_loose(&self) -> bool {
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
            self.constrain_width(f32::INFINITY),
            self.constrain_height(f32::INFINITY),
        )
    }

    /// Returns the smallest size that satisfies the constraints.
    #[inline]
    #[must_use]
    pub const fn smallest(&self) -> Size {
        Size::new(self.min_width, self.min_height)
    }

    /// Constrains width to be within bounds.
    #[inline]
    #[must_use]
    pub fn constrain_width(&self, width: f32) -> f32 {
        width.clamp(self.min_width, self.max_width)
    }

    /// Constrains height to be within bounds.
    #[inline]
    #[must_use]
    pub fn constrain_height(&self, height: f32) -> f32 {
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
    pub const fn loosen(&self) -> Self {
        Self {
            min_width: 0.0,
            max_width: self.max_width,
            min_height: 0.0,
            max_height: self.max_height,
        }
    }

    /// Tightens constraints to specific dimensions.
    ///
    /// Sets both min and max to the given value for specified dimensions.
    #[inline]
    #[must_use]
    pub fn tighten(&self, width: Option<f32>, height: Option<f32>) -> Self {
        Self {
            min_width: width.unwrap_or(self.min_width),
            max_width: width.unwrap_or(self.max_width),
            min_height: height.unwrap_or(self.min_height),
            max_height: height.unwrap_or(self.max_height),
        }
    }

    /// Enforces these constraints on another set of constraints.
    ///
    /// Constrains the other constraints to fit within these bounds.
    #[inline]
    #[must_use]
    pub fn enforce(&self, other: &Self) -> Self {
        Self {
            min_width: self.constrain_width(other.min_width),
            max_width: self.constrain_width(other.max_width),
            min_height: self.constrain_height(other.min_height),
            max_height: self.constrain_height(other.max_height),
        }
    }

    // ============================================================================
    // BUILDER PATTERN
    // ============================================================================

    /// Sets minimum width.
    #[inline]
    #[must_use]
    pub const fn with_min_width(mut self, min_width: f32) -> Self {
        self.min_width = min_width;
        self
    }

    /// Sets maximum width.
    #[inline]
    #[must_use]
    pub const fn with_max_width(mut self, max_width: f32) -> Self {
        self.max_width = max_width;
        self
    }

    /// Sets minimum height.
    #[inline]
    #[must_use]
    pub const fn with_min_height(mut self, min_height: f32) -> Self {
        self.min_height = min_height;
        self
    }

    /// Sets maximum height.
    #[inline]
    #[must_use]
    pub const fn with_max_height(mut self, max_height: f32) -> Self {
        self.max_height = max_height;
        self
    }

    /// Sets tight width (min == max).
    #[inline]
    #[must_use]
    pub const fn with_tight_width(mut self, width: f32) -> Self {
        self.min_width = width;
        self.max_width = width;
        self
    }

    /// Sets tight height (min == max).
    #[inline]
    #[must_use]
    pub const fn with_tight_height(mut self, height: f32) -> Self {
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

    /// Returns maximum possible area.
    #[inline]
    #[must_use]
    pub fn max_area(&self) -> f32 {
        self.max_width * self.max_height
    }

    /// Returns minimum required area.
    #[inline]
    #[must_use]
    pub const fn min_area(&self) -> f32 {
        self.min_width * self.min_height
    }

    /// Returns maximum diagonal length.
    #[inline]
    #[must_use]
    pub fn max_diagonal(&self) -> f32 {
        (self.max_width * self.max_width + self.max_height * self.max_height).sqrt()
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
    pub const fn width_range(&self) -> (f32, f32) {
        (self.min_width, self.max_width)
    }

    /// Returns height range as tuple.
    #[inline]
    #[must_use]
    pub const fn height_range(&self) -> (f32, f32) {
        (self.min_height, self.max_height)
    }

    /// Maps a function over all constraint values.
    #[inline]
    #[must_use]
    pub fn map<F>(&self, f: F) -> Self
    where
        F: Fn(f32) -> f32,
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
        self.map(|v| v.round())
    }

    /// Floors all constraint values.
    #[inline]
    #[must_use]
    pub fn floor(&self) -> Self {
        self.map(|v| v.floor())
    }

    /// Ceils all constraint values.
    #[inline]
    #[must_use]
    pub fn ceil(&self) -> Self {
        self.map(|v| v.ceil())
    }
}

// ============================================================================
// SIMD BATCH OPERATIONS
// ============================================================================

#[cfg(feature = "simd")]
impl BoxConstraints {
    /// Constrains multiple sizes at once using SIMD.
    ///
    /// Processes sizes in batches of 4 for improved performance.
    ///
    /// # Performance
    ///
    /// Approximately 2-10x faster than constraining individually,
    /// depending on batch size.
    pub fn batch_constrain(&self, sizes: &[Size]) -> Vec<Size> {
        use wide::f32x4;

        let mut result = Vec::with_capacity(sizes.len());
        let chunks = sizes.chunks_exact(4);
        let remainder = chunks.remainder();

        // SIMD process 4 sizes at once
        for chunk in chunks {
            let widths = f32x4::new([
                chunk[0].width,
                chunk[1].width,
                chunk[2].width,
                chunk[3].width,
            ]);
            let heights = f32x4::new([
                chunk[0].height,
                chunk[1].height,
                chunk[2].height,
                chunk[3].height,
            ]);

            let min_w = f32x4::splat(self.min_width);
            let max_w = f32x4::splat(self.max_width);
            let min_h = f32x4::splat(self.min_height);
            let max_h = f32x4::splat(self.max_height);

            let clamped_w = widths.max(min_w).min(max_w);
            let clamped_h = heights.max(min_h).min(max_h);

            let w_arr = clamped_w.to_array();
            let h_arr = clamped_h.to_array();

            for i in 0..4 {
                result.push(Size::new(w_arr[i], h_arr[i]));
            }
        }

        // Handle remaining sizes
        for size in remainder {
            result.push(self.constrain(*size));
        }

        result
    }

    /// Checks if multiple sizes satisfy constraints using SIMD.
    ///
    /// Returns a vector of booleans indicating satisfaction.
    pub fn batch_is_satisfied_by(&self, sizes: &[Size]) -> Vec<bool> {
        use wide::f32x4;

        let mut result = Vec::with_capacity(sizes.len());
        let chunks = sizes.chunks_exact(4);
        let remainder = chunks.remainder();

        let min_w = f32x4::splat(self.min_width);
        let max_w = f32x4::splat(self.max_width);
        let min_h = f32x4::splat(self.min_height);
        let max_h = f32x4::splat(self.max_height);

        for chunk in chunks {
            let widths = f32x4::new([
                chunk[0].width,
                chunk[1].width,
                chunk[2].width,
                chunk[3].width,
            ]);
            let heights = f32x4::new([
                chunk[0].height,
                chunk[1].height,
                chunk[2].height,
                chunk[3].height,
            ]);

            let w_valid = widths.cmp_ge(min_w) & widths.cmp_le(max_w);
            let h_valid = heights.cmp_ge(min_h) & heights.cmp_le(max_h);
            let valid = w_valid & h_valid;

            for i in 0..4 {
                result.push(valid.extract(i) != 0);
            }
        }

        for size in remainder {
            result.push(self.is_satisfied_by(*size));
        }

        result
    }
}

// ============================================================================
// NORMALIZATION HELPERS
// ============================================================================

/// Rounds value to hundredths precision.
#[inline]
fn round_to_hundredths(value: f32) -> f32 {
    if value.is_finite() {
        (value * 100.0).round() / 100.0
    } else {
        value
    }
}

/// Checks if value is already normalized.
#[inline]
fn is_normalized(value: f32) -> bool {
    if !value.is_finite() {
        true
    } else {
        value == round_to_hundredths(value)
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
        write!(f, "{:?}", self)
    }
}

// ============================================================================
// OPERATOR OVERLOADS
// ============================================================================

impl std::ops::Mul<f32> for BoxConstraints {
    type Output = Self;

    fn mul(self, scale: f32) -> Self {
        self.map(|v| v * scale)
    }
}

impl std::ops::Div<f32> for BoxConstraints {
    type Output = Self;

    fn div(self, scale: f32) -> Self {
        self.map(|v| v / scale)
    }
}

impl std::ops::MulAssign<f32> for BoxConstraints {
    fn mul_assign(&mut self, scale: f32) {
        *self = *self * scale;
    }
}

impl std::ops::DivAssign<f32> for BoxConstraints {
    fn div_assign(&mut self, scale: f32) {
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

impl From<BoxConstraints> for (f32, f32, f32, f32) {
    fn from(c: BoxConstraints) -> Self {
        (c.min_width, c.max_width, c.min_height, c.max_height)
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn test_hash_equality() {
        let c1 = BoxConstraints::tight(Size::new(100.0, 100.0));
        let c2 = BoxConstraints::tight(Size::new(100.0, 100.0));
        let c3 = BoxConstraints::tight(Size::new(200.0, 200.0));

        assert_eq!(c1, c2);
        assert_ne!(c1, c3);

        let mut set = HashSet::new();
        set.insert(c1);
        assert!(set.contains(&c2));
        assert!(!set.contains(&c3));
    }

    #[test]
    fn test_normalization() {
        let c = BoxConstraints::new(10.123456, 100.987654, 20.555555, 200.444444);
        let normalized = c.normalize();

        assert_eq!(normalized.min_width, 10.12);
        assert_eq!(normalized.max_width, 100.99);
        assert_eq!(normalized.min_height, 20.56);
        assert_eq!(normalized.max_height, 200.44);

        // Infinity preserved
        let inf = BoxConstraints::UNCONSTRAINED.normalize();
        assert!(inf.max_width.is_infinite());
    }

    #[test]
    fn test_is_normalized() {
        let normalized = BoxConstraints::new(10.12, 100.99, 20.56, 200.44);
        assert!(normalized.is_normalized_for_cache());

        let unnormalized = BoxConstraints::new(10.123456, 100.0, 20.0, 200.0);
        assert!(!unnormalized.is_normalized_for_cache());
    }

    #[test]
    fn test_constants() {
        assert!(BoxConstraints::ZERO.is_tight());
        assert_eq!(BoxConstraints::ZERO.biggest(), Size::ZERO);

        assert!(!BoxConstraints::UNCONSTRAINED.has_bounded_width());
        assert!(!BoxConstraints::UNCONSTRAINED.has_bounded_height());
    }

    #[test]
    fn test_size_operations() {
        let c = BoxConstraints::new(10.0, 100.0, 20.0, 200.0);

        assert!(c.is_satisfied_by(Size::new(50.0, 50.0)));
        assert!(!c.is_satisfied_by(Size::new(5.0, 50.0)));
        assert!(!c.is_satisfied_by(Size::new(50.0, 5.0)));
        assert!(!c.is_satisfied_by(Size::new(150.0, 50.0)));

        let constrained = c.constrain(Size::new(150.0, 250.0));
        assert_eq!(constrained, Size::new(100.0, 200.0));
    }

    #[test]
    fn test_set_operations() {
        let a = BoxConstraints::new(0.0, 100.0, 0.0, 100.0);
        let b = BoxConstraints::new(50.0, 150.0, 50.0, 150.0);

        let inter = a.intersection(&b).unwrap();
        assert_eq!(inter.min_width, 50.0);
        assert_eq!(inter.max_width, 100.0);

        let uni = a.union(&b);
        assert_eq!(uni.min_width, 0.0);
        assert_eq!(uni.max_width, 150.0);

        let outer = BoxConstraints::new(0.0, 200.0, 0.0, 200.0);
        let inner = BoxConstraints::new(50.0, 150.0, 50.0, 150.0);
        assert!(outer.contains(&inner));
        assert!(!inner.contains(&outer));
        assert!(outer.overlaps(&inner));
    }

    #[test]
    fn test_builder_pattern() {
        let c = BoxConstraints::UNCONSTRAINED
            .with_min_width(10.0)
            .with_max_width(100.0)
            .with_tight_height(50.0);

        assert_eq!(c.min_width, 10.0);
        assert_eq!(c.max_width, 100.0);
        assert!(c.has_tight_height());
    }

    #[cfg(feature = "simd")]
    #[test]
    fn test_batch_operations() {
        let c = BoxConstraints::loose(Size::new(100.0, 100.0));
        let sizes = vec![
            Size::new(50.0, 50.0),
            Size::new(150.0, 150.0),
            Size::new(75.0, 75.0),
            Size::new(200.0, 200.0),
        ];

        let constrained = c.batch_constrain(&sizes);
        assert_eq!(constrained[0], Size::new(50.0, 50.0));
        assert_eq!(constrained[1], Size::new(100.0, 100.0));
        assert_eq!(constrained[3], Size::new(100.0, 100.0));

        let valid = c.batch_is_satisfied_by(&sizes);
        assert_eq!(valid, vec![true, false, true, false]);
    }
}
