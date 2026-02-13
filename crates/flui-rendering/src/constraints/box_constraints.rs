//! Box layout constraints following Flutter's proven model.
//!
//! Provides rectangular constraints for 2D box-based layout with
//! comprehensive query and transformation operations.

use super::Constraints;
use flui_types::{EdgeInsets, Pixels, Size};
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
/// # Flutter Equivalence
///
/// Maps directly to Flutter's `BoxConstraints` class with identical semantics.
#[derive(Clone, Copy, PartialEq)]
pub struct BoxConstraints {
    /// Minimum width that satisfies the constraints.
    pub min_width: Pixels,
    /// Maximum width that satisfies the constraints (may be infinite).
    pub max_width: Pixels,
    /// Minimum height that satisfies the constraints.
    pub min_height: Pixels,
    /// Maximum height that satisfies the constraints (may be infinite).
    pub max_height: Pixels,
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
        min_width: Pixels::ZERO,
        max_width: Pixels::INFINITY,
        min_height: Pixels::ZERO,
        max_height: Pixels::INFINITY,
    };

    /// Zero-sized constraints (tight at zero).
    pub const ZERO: Self = Self {
        min_width: Pixels::ZERO,
        max_width: Pixels::ZERO,
        min_height: Pixels::ZERO,
        max_height: Pixels::ZERO,
    };

    /// Creates new box constraints with explicit bounds.
    #[inline]
    #[must_use]
    pub const fn new(
        min_width: Pixels,
        max_width: Pixels,
        min_height: Pixels,
        max_height: Pixels,
    ) -> Self {
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
            min_width: Pixels::ZERO,
            max_width: size.width,
            min_height: Pixels::ZERO,
            max_height: size.height,
        }
    }

    /// Creates expand constraints that force maximum size (fill parent).
    #[inline]
    #[must_use]
    pub const fn expand() -> Self {
        Self {
            min_width: Pixels::INFINITY,
            max_width: Pixels::INFINITY,
            min_height: Pixels::INFINITY,
            max_height: Pixels::INFINITY,
        }
    }

    /// Creates constraints with optional tight dimensions.
    ///
    /// Tight dimensions use the given value for both min and max.
    /// Loose dimensions allow any size.
    #[inline]
    #[must_use]
    pub const fn tight_for(width: Option<Pixels>, height: Option<Pixels>) -> Self {
        Self {
            min_width: match width {
                Some(w) => w,
                None => Pixels::ZERO,
            },
            max_width: match width {
                Some(w) => w,
                None => Pixels::INFINITY,
            },
            min_height: match height {
                Some(h) => h,
                None => Pixels::ZERO,
            },
            max_height: match height {
                Some(h) => h,
                None => Pixels::INFINITY,
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
            min_width: round_pixels_to_hundredths(self.min_width),
            max_width: round_pixels_to_hundredths(self.max_width),
            min_height: round_pixels_to_hundredths(self.min_height),
            max_height: round_pixels_to_hundredths(self.max_height),
        }
    }

    /// Checks if constraints are already normalized.
    ///
    /// More efficient than comparing with `normalize()` as it checks
    /// each field individually.
    #[inline]
    #[must_use]
    pub fn is_normalized_for_cache(&self) -> bool {
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
            self.constrain_width(Pixels::INFINITY),
            self.constrain_height(Pixels::INFINITY),
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
    pub fn constrain_width(&self, width: Pixels) -> Pixels {
        width.clamp(self.min_width, self.max_width)
    }

    /// Constrains height to be within bounds.
    #[inline]
    #[must_use]
    pub fn constrain_height(&self, height: Pixels) -> Pixels {
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
        let horizontal = Pixels(insets.left + insets.right);
        let vertical = Pixels(insets.top + insets.bottom);

        Self {
            min_width: (self.min_width - horizontal).max(Pixels::ZERO),
            max_width: (self.max_width - horizontal).max(Pixels::ZERO),
            min_height: (self.min_height - vertical).max(Pixels::ZERO),
            max_height: (self.max_height - vertical).max(Pixels::ZERO),
        }
    }

    /// Inflates constraints by edge insets.
    ///
    /// Adds space for insets. Preserves infinity.
    #[inline]
    #[must_use]
    pub fn inflate(&self, insets: EdgeInsets) -> Self {
        let horizontal = Pixels(insets.left + insets.right);
        let vertical = Pixels(insets.top + insets.bottom);

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
            min_width: Pixels::ZERO,
            max_width: self.max_width,
            min_height: Pixels::ZERO,
            max_height: self.max_height,
        }
    }

    /// Tightens constraints to specific dimensions.
    ///
    /// Sets both min and max to the given value for specified dimensions.
    #[inline]
    #[must_use]
    pub fn tighten(&self, width: Option<Pixels>, height: Option<Pixels>) -> Self {
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
    pub const fn with_min_width(mut self, min_width: Pixels) -> Self {
        self.min_width = min_width;
        self
    }

    /// Sets maximum width.
    #[inline]
    #[must_use]
    pub const fn with_max_width(mut self, max_width: Pixels) -> Self {
        self.max_width = max_width;
        self
    }

    /// Sets minimum height.
    #[inline]
    #[must_use]
    pub const fn with_min_height(mut self, min_height: Pixels) -> Self {
        self.min_height = min_height;
        self
    }

    /// Sets maximum height.
    #[inline]
    #[must_use]
    pub const fn with_max_height(mut self, max_height: Pixels) -> Self {
        self.max_height = max_height;
        self
    }

    /// Sets tight width (min == max).
    #[inline]
    #[must_use]
    pub const fn with_tight_width(mut self, width: Pixels) -> Self {
        self.min_width = width;
        self.max_width = width;
        self
    }

    /// Sets tight height (min == max).
    #[inline]
    #[must_use]
    pub const fn with_tight_height(mut self, height: Pixels) -> Self {
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

    /// Returns maximum possible area as raw f32.
    #[inline]
    #[must_use]
    pub fn max_area(&self) -> f32 {
        self.max_width.get() * self.max_height.get()
    }

    /// Returns minimum required area as raw f32.
    #[inline]
    #[must_use]
    pub fn min_area(&self) -> f32 {
        self.min_width.get() * self.min_height.get()
    }

    /// Returns maximum diagonal length as raw f32.
    #[inline]
    #[must_use]
    pub fn max_diagonal(&self) -> f32 {
        let w = self.max_width.get();
        let h = self.max_height.get();
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
    pub const fn width_range(&self) -> (Pixels, Pixels) {
        (self.min_width, self.max_width)
    }

    /// Returns height range as tuple.
    #[inline]
    #[must_use]
    pub const fn height_range(&self) -> (Pixels, Pixels) {
        (self.min_height, self.max_height)
    }

    /// Maps a function over all constraint values.
    #[inline]
    #[must_use]
    pub fn map<F>(&self, f: F) -> Self
    where
        F: Fn(Pixels) -> Pixels,
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
// NORMALIZATION HELPERS
// ============================================================================

/// Rounds a Pixels value to hundredths precision.
#[inline]
fn round_pixels_to_hundredths(value: Pixels) -> Pixels {
    if value.is_finite() {
        Pixels((value.get() * 100.0).round() / 100.0)
    } else {
        value
    }
}

/// Checks if a Pixels value is already normalized.
#[inline]
fn is_pixels_normalized(value: Pixels) -> bool {
    if !value.is_finite() {
        true
    } else {
        value == round_pixels_to_hundredths(value)
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
            && self.max_width >= self.min_width
            && self.min_height <= self.max_height
            && self.max_height >= self.min_height
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
                self.min_width.get(),
                self.min_height.get()
            )
        } else {
            write!(
                f,
                "BoxConstraints(w: {}..{}, h: {}..{})",
                self.min_width.get(),
                self.max_width.get(),
                self.min_height.get(),
                self.max_height.get()
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

impl From<BoxConstraints> for (Pixels, Pixels, Pixels, Pixels) {
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
    use flui_types::geometry::px;
    use std::collections::HashSet;

    #[test]
    fn test_hash_equality() {
        let c1 = BoxConstraints::tight(Size::new(px(100.0), px(100.0)));
        let c2 = BoxConstraints::tight(Size::new(px(100.0), px(100.0)));
        let c3 = BoxConstraints::tight(Size::new(px(200.0), px(200.0)));

        assert_eq!(c1, c2);
        assert_ne!(c1, c3);

        let mut set = HashSet::new();
        set.insert(c1);
        assert!(set.contains(&c2));
        assert!(!set.contains(&c3));
    }

    #[test]
    fn test_normalization() {
        let c = BoxConstraints::new(
            px(10.123_456),
            px(100.987_654),
            px(20.555_555),
            px(200.444_44),
        );
        let normalized = c.normalize();

        assert_eq!(normalized.min_width, px(10.12));
        assert_eq!(normalized.max_width, px(100.99));
        assert_eq!(normalized.min_height, px(20.56));
        assert_eq!(normalized.max_height, px(200.44));

        // Infinity preserved
        let inf = BoxConstraints::UNCONSTRAINED.normalize();
        assert!(inf.max_width.is_infinite());
    }

    #[test]
    fn test_is_normalized() {
        let normalized = BoxConstraints::new(px(10.12), px(100.99), px(20.56), px(200.44));
        assert!(normalized.is_normalized_for_cache());

        let unnormalized = BoxConstraints::new(px(10.123_456), px(100.0), px(20.0), px(200.0));
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
        let c = BoxConstraints::new(px(10.0), px(100.0), px(20.0), px(200.0));

        assert!(c.is_satisfied_by(Size::new(px(50.0), px(50.0))));
        assert!(!c.is_satisfied_by(Size::new(px(5.0), px(50.0))));
        assert!(!c.is_satisfied_by(Size::new(px(50.0), px(5.0))));
        assert!(!c.is_satisfied_by(Size::new(px(150.0), px(50.0))));

        let constrained = c.constrain(Size::new(px(150.0), px(250.0)));
        assert_eq!(constrained, Size::new(px(100.0), px(200.0)));
    }

    #[test]
    fn test_set_operations() {
        let a = BoxConstraints::new(px(0.0), px(100.0), px(0.0), px(100.0));
        let b = BoxConstraints::new(px(50.0), px(150.0), px(50.0), px(150.0));

        let inter = a.intersection(&b).unwrap();
        assert_eq!(inter.min_width, px(50.0));
        assert_eq!(inter.max_width, px(100.0));

        let uni = a.union(&b);
        assert_eq!(uni.min_width, px(0.0));
        assert_eq!(uni.max_width, px(150.0));

        let outer = BoxConstraints::new(px(0.0), px(200.0), px(0.0), px(200.0));
        let inner = BoxConstraints::new(px(50.0), px(150.0), px(50.0), px(150.0));
        assert!(outer.contains(&inner));
        assert!(!inner.contains(&outer));
        assert!(outer.overlaps(&inner));
    }

    #[test]
    fn test_builder_pattern() {
        let c = BoxConstraints::UNCONSTRAINED
            .with_min_width(px(10.0))
            .with_max_width(px(100.0))
            .with_tight_height(px(50.0));

        assert_eq!(c.min_width, px(10.0));
        assert_eq!(c.max_width, px(100.0));
        assert!(c.has_tight_height());
    }
}
