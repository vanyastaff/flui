//! Box constraints for 2D cartesian layout.
//!
//! BoxConstraints describe the rectangular space available for a render object
//! during layout. They specify minimum and maximum dimensions that the object
//! must satisfy.
//!
//! # Flutter Equivalence
//!
//! This corresponds to Flutter's `BoxConstraints` from `rendering/box.dart`.

use super::Constraints;
use flui_types::{EdgeInsets, Size};
use std::fmt;

/// Immutable layout constraints for RenderBox layout.
///
/// A Size respects BoxConstraints if and only if:
/// - `min_width <= width <= max_width`
/// - `min_height <= height <= max_height`
///
/// The constraints themselves must satisfy:
/// - `0.0 <= min_width <= max_width <= f32::INFINITY`
/// - `0.0 <= min_height <= max_height <= f32::INFINITY`
///
/// # Terminology
///
/// - **Tight**: min == max (only one size satisfies)
/// - **Loose**: min == 0 (any size up to max satisfies)
/// - **Bounded**: max < infinity
/// - **Unbounded**: max == infinity
/// - **Expanding**: min == max == infinity
///
/// # Examples
///
/// ```
/// use flui_rendering::constraints::{BoxConstraints, Constraints};
/// use flui_types::Size;
///
/// // Tight constraints force exact size
/// let tight = BoxConstraints::tight(Size::new(100.0, 50.0));
/// assert!(tight.is_tight());
///
/// // Loose constraints allow any size up to max
/// let loose = BoxConstraints::loose(Size::new(200.0, 100.0));
/// assert_eq!(loose.min_width, 0.0);
/// ```
#[derive(Clone, Copy, PartialEq)]
pub struct BoxConstraints {
    /// Minimum width that satisfies the constraints.
    pub min_width: f32,
    /// Maximum width that satisfies the constraints. May be `f32::INFINITY`.
    pub max_width: f32,
    /// Minimum height that satisfies the constraints.
    pub min_height: f32,
    /// Maximum height that satisfies the constraints. May be `f32::INFINITY`.
    pub max_height: f32,
}

impl BoxConstraints {
    /// Unconstrained - widget can be any size.
    pub const UNCONSTRAINED: Self = Self {
        min_width: 0.0,
        max_width: f32::INFINITY,
        min_height: 0.0,
        max_height: f32::INFINITY,
    };

    /// Creates new box constraints with the given bounds.
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
    pub fn tight(size: Size) -> Self {
        Self {
            min_width: size.width,
            max_width: size.width,
            min_height: size.height,
            max_height: size.height,
        }
    }

    /// Creates loose constraints allowing any size from zero to the given size.
    #[inline]
    #[must_use]
    pub fn loose(size: Size) -> Self {
        Self {
            min_width: 0.0,
            max_width: size.width,
            min_height: 0.0,
            max_height: size.height,
        }
    }

    /// Creates constraints with optional tight width and/or height.
    #[inline]
    #[must_use]
    pub fn tight_for(width: Option<f32>, height: Option<f32>) -> Self {
        Self {
            min_width: width.unwrap_or(0.0),
            max_width: width.unwrap_or(f32::INFINITY),
            min_height: height.unwrap_or(0.0),
            max_height: height.unwrap_or(f32::INFINITY),
        }
    }

    /// Creates constraints with tight width/height if the values are finite.
    #[inline]
    #[must_use]
    pub fn tight_for_finite(width: f32, height: f32) -> Self {
        Self {
            min_width: if width.is_finite() { width } else { 0.0 },
            max_width: if width.is_finite() {
                width
            } else {
                f32::INFINITY
            },
            min_height: if height.is_finite() { height } else { 0.0 },
            max_height: if height.is_finite() {
                height
            } else {
                f32::INFINITY
            },
        }
    }

    /// Creates expand constraints that force infinite size (fill parent).
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

    /// Creates expand constraints with optional fixed dimensions.
    #[inline]
    #[must_use]
    pub fn expand_with(width: Option<f32>, height: Option<f32>) -> Self {
        Self {
            min_width: width.unwrap_or(f32::INFINITY),
            max_width: width.unwrap_or(f32::INFINITY),
            min_height: height.unwrap_or(f32::INFINITY),
            max_height: height.unwrap_or(f32::INFINITY),
        }
    }

    /// Creates a copy with the given fields replaced.
    #[inline]
    #[must_use]
    pub fn copy_with(
        &self,
        min_width: Option<f32>,
        max_width: Option<f32>,
        min_height: Option<f32>,
        max_height: Option<f32>,
    ) -> Self {
        Self {
            min_width: min_width.unwrap_or(self.min_width),
            max_width: max_width.unwrap_or(self.max_width),
            min_height: min_height.unwrap_or(self.min_height),
            max_height: max_height.unwrap_or(self.max_height),
        }
    }

    // ===== Constraint queries =====

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

    /// Returns whether there is an upper bound on width.
    #[inline]
    #[must_use]
    pub fn has_bounded_width(&self) -> bool {
        self.max_width < f32::INFINITY
    }

    /// Returns whether there is an upper bound on height.
    #[inline]
    #[must_use]
    pub fn has_bounded_height(&self) -> bool {
        self.max_height < f32::INFINITY
    }

    /// Returns whether the width constraint is infinite.
    #[inline]
    #[must_use]
    pub fn has_infinite_width(&self) -> bool {
        self.min_width >= f32::INFINITY
    }

    /// Returns whether the height constraint is infinite.
    #[inline]
    #[must_use]
    pub fn has_infinite_height(&self) -> bool {
        self.min_height >= f32::INFINITY
    }

    // ===== Size operations =====

    /// Returns the biggest size that satisfies the constraints.
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
    pub fn smallest(&self) -> Size {
        Size::new(self.constrain_width(0.0), self.constrain_height(0.0))
    }

    /// Constrains the given width to be within min/max bounds.
    #[inline]
    #[must_use]
    pub fn constrain_width(&self, width: f32) -> f32 {
        width.clamp(self.min_width, self.max_width)
    }

    /// Constrains the given height to be within min/max bounds.
    #[inline]
    #[must_use]
    pub fn constrain_height(&self, height: f32) -> f32 {
        height.clamp(self.min_height, self.max_height)
    }

    /// Constrains the given size to be within the constraints.
    #[inline]
    #[must_use]
    pub fn constrain(&self, size: Size) -> Size {
        Size::new(
            self.constrain_width(size.width),
            self.constrain_height(size.height),
        )
    }

    /// Constrains width and height separately.
    #[inline]
    #[must_use]
    pub fn constrain_dimensions(&self, width: f32, height: f32) -> Size {
        Size::new(self.constrain_width(width), self.constrain_height(height))
    }

    /// Returns a size that preserves aspect ratio while satisfying constraints.
    #[must_use]
    pub fn constrain_size_and_attempt_to_preserve_aspect_ratio(&self, size: Size) -> Size {
        if self.is_tight() {
            return self.smallest();
        }

        if size.is_empty() {
            return self.constrain(size);
        }

        let mut width = size.width;
        let mut height = size.height;
        let aspect_ratio = width / height;

        if width > self.max_width {
            width = self.max_width;
            height = width / aspect_ratio;
        }

        if height > self.max_height {
            height = self.max_height;
            width = height * aspect_ratio;
        }

        if width < self.min_width {
            width = self.min_width;
            height = width / aspect_ratio;
        }

        if height < self.min_height {
            height = self.min_height;
            width = height * aspect_ratio;
        }

        Size::new(self.constrain_width(width), self.constrain_height(height))
    }

    /// Returns whether the given size satisfies the constraints.
    #[inline]
    #[must_use]
    pub fn is_satisfied_by(&self, size: Size) -> bool {
        size.width >= self.min_width
            && size.width <= self.max_width
            && size.height >= self.min_height
            && size.height <= self.max_height
    }

    // ===== Constraint transformations =====

    /// Returns constraints with width and height swapped.
    #[inline]
    #[must_use]
    pub const fn flipped(&self) -> Self {
        Self {
            min_width: self.min_height,
            max_width: self.max_height,
            min_height: self.min_width,
            max_height: self.max_width,
        }
    }

    /// Returns constraints with only width constraints (height unconstrained).
    #[inline]
    #[must_use]
    pub const fn width_constraints(&self) -> Self {
        Self {
            min_width: self.min_width,
            max_width: self.max_width,
            min_height: 0.0,
            max_height: f32::INFINITY,
        }
    }

    /// Returns constraints with only height constraints (width unconstrained).
    #[inline]
    #[must_use]
    pub const fn height_constraints(&self) -> Self {
        Self {
            min_width: 0.0,
            max_width: f32::INFINITY,
            min_height: self.min_height,
            max_height: self.max_height,
        }
    }

    /// Returns constraints deflated by the given edges (for padding).
    #[must_use]
    pub fn deflate(&self, edges: EdgeInsets) -> Self {
        let horizontal = edges.left + edges.right;
        let vertical = edges.top + edges.bottom;
        let deflated_min_width = (self.min_width - horizontal).max(0.0);
        let deflated_min_height = (self.min_height - vertical).max(0.0);
        Self {
            min_width: deflated_min_width,
            max_width: (self.max_width - horizontal).max(deflated_min_width),
            min_height: deflated_min_height,
            max_height: (self.max_height - vertical).max(deflated_min_height),
        }
    }

    /// Returns constraints with minimums set to zero.
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

    /// Returns constraints tightened to the given width/height.
    #[must_use]
    pub fn tighten(&self, width: Option<f32>, height: Option<f32>) -> Self {
        Self {
            min_width: width
                .map(|w| w.clamp(self.min_width, self.max_width))
                .unwrap_or(self.min_width),
            max_width: width
                .map(|w| w.clamp(self.min_width, self.max_width))
                .unwrap_or(self.max_width),
            min_height: height
                .map(|h| h.clamp(self.min_height, self.max_height))
                .unwrap_or(self.min_height),
            max_height: height
                .map(|h| h.clamp(self.min_height, self.max_height))
                .unwrap_or(self.max_height),
        }
    }

    /// Returns constraints that respect both self and other.
    #[must_use]
    pub fn enforce(&self, constraints: BoxConstraints) -> Self {
        Self {
            min_width: self
                .min_width
                .clamp(constraints.min_width, constraints.max_width),
            max_width: self
                .max_width
                .clamp(constraints.min_width, constraints.max_width),
            min_height: self
                .min_height
                .clamp(constraints.min_height, constraints.max_height),
            max_height: self
                .max_height
                .clamp(constraints.min_height, constraints.max_height),
        }
    }

    // ===== Arithmetic operations =====

    /// Scales all constraint values by the given factor.
    #[inline]
    #[must_use]
    pub fn scale(&self, factor: f32) -> Self {
        Self {
            min_width: self.min_width * factor,
            max_width: self.max_width * factor,
            min_height: self.min_height * factor,
            max_height: self.max_height * factor,
        }
    }

    /// Divides all constraint values by the given factor.
    #[inline]
    #[must_use]
    pub fn divide(&self, factor: f32) -> Self {
        Self {
            min_width: self.min_width / factor,
            max_width: self.max_width / factor,
            min_height: self.min_height / factor,
            max_height: self.max_height / factor,
        }
    }

    /// Linearly interpolates between two constraints.
    #[inline]
    #[must_use]
    pub fn lerp(&self, other: &Self, t: f32) -> Self {
        Self {
            min_width: self.min_width + (other.min_width - self.min_width) * t,
            max_width: self.max_width + (other.max_width - self.max_width) * t,
            min_height: self.min_height + (other.min_height - self.min_height) * t,
            max_height: self.max_height + (other.max_height - self.max_height) * t,
        }
    }

    // ===== Normalization =====

    /// Returns a normalized copy of these constraints.
    ///
    /// If `min > max`, the values are swapped. Negative values are clamped to 0.
    /// NaN values are replaced with 0.
    ///
    /// # Flutter Equivalence
    ///
    /// Corresponds to Flutter's `BoxConstraints.normalize()`.
    #[inline]
    #[must_use]
    pub fn normalize(&self) -> Self {
        let min_width = if self.min_width.is_nan() {
            0.0
        } else {
            self.min_width.max(0.0)
        };
        let max_width = if self.max_width.is_nan() {
            0.0
        } else {
            self.max_width.max(0.0)
        };
        let min_height = if self.min_height.is_nan() {
            0.0
        } else {
            self.min_height.max(0.0)
        };
        let max_height = if self.max_height.is_nan() {
            0.0
        } else {
            self.max_height.max(0.0)
        };

        Self {
            min_width: min_width.min(max_width),
            max_width: max_width.max(min_width),
            min_height: min_height.min(max_height),
            max_height: max_height.max(min_height),
        }
    }

    // ===== Validation =====

    /// Asserts that the constraints are normalized (debug only).
    #[inline]
    pub fn assert_is_normalized(&self) {
        debug_assert!(!self.min_width.is_nan(), "min_width cannot be NaN");
        debug_assert!(!self.max_width.is_nan(), "max_width cannot be NaN");
        debug_assert!(!self.min_height.is_nan(), "min_height cannot be NaN");
        debug_assert!(!self.max_height.is_nan(), "max_height cannot be NaN");
        debug_assert!(
            self.min_width >= 0.0,
            "min_width ({}) cannot be negative",
            self.min_width
        );
        debug_assert!(
            self.min_height >= 0.0,
            "min_height ({}) cannot be negative",
            self.min_height
        );
        debug_assert!(
            self.min_width <= self.max_width,
            "min_width ({}) > max_width ({})",
            self.min_width,
            self.max_width
        );
        debug_assert!(
            self.min_height <= self.max_height,
            "min_height ({}) > max_height ({})",
            self.min_height,
            self.max_height
        );
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
        } else if self.min_width == 0.0 && self.min_height == 0.0 {
            write!(
                f,
                "BoxConstraints(loose: {}×{})",
                self.max_width, self.max_height
            )
        } else {
            f.debug_struct("BoxConstraints")
                .field("min_width", &self.min_width)
                .field("max_width", &self.max_width)
                .field("min_height", &self.min_height)
                .field("max_height", &self.max_height)
                .finish()
        }
    }
}

impl fmt::Display for BoxConstraints {
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
                "BoxConstraints({} ≤ w ≤ {}, {} ≤ h ≤ {})",
                self.min_width, self.max_width, self.min_height, self.max_height
            )
        }
    }
}

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

    #[cfg(debug_assertions)]
    fn debug_assert_is_valid(&self, _is_applied_constraint: bool) -> bool {
        self.assert_is_normalized();
        true
    }
}

// ============================================================================
// Rust-specific Enhancements: Operators
// ============================================================================

impl std::ops::Mul<f32> for BoxConstraints {
    type Output = Self;

    /// Scales all constraint values by the given factor.
    ///
    /// # Example
    ///
    /// ```
    /// use flui_rendering::constraints::BoxConstraints;
    ///
    /// let c = BoxConstraints::new(10.0, 100.0, 20.0, 200.0);
    /// let scaled = c * 2.0;
    /// assert_eq!(scaled.min_width, 20.0);
    /// assert_eq!(scaled.max_width, 200.0);
    /// ```
    #[inline]
    fn mul(self, rhs: f32) -> Self::Output {
        self.scale(rhs)
    }
}

impl std::ops::Mul<BoxConstraints> for f32 {
    type Output = BoxConstraints;

    #[inline]
    fn mul(self, rhs: BoxConstraints) -> Self::Output {
        rhs.scale(self)
    }
}

impl std::ops::Div<f32> for BoxConstraints {
    type Output = Self;

    /// Divides all constraint values by the given factor.
    ///
    /// # Example
    ///
    /// ```
    /// use flui_rendering::constraints::BoxConstraints;
    ///
    /// let c = BoxConstraints::new(20.0, 200.0, 40.0, 400.0);
    /// let divided = c / 2.0;
    /// assert_eq!(divided.min_width, 10.0);
    /// assert_eq!(divided.max_width, 100.0);
    /// ```
    #[inline]
    fn div(self, rhs: f32) -> Self::Output {
        self.divide(rhs)
    }
}

impl std::ops::Rem<f32> for BoxConstraints {
    type Output = Self;

    /// Computes remainder of all constraint values divided by the given factor.
    #[inline]
    fn rem(self, rhs: f32) -> Self::Output {
        Self {
            min_width: self.min_width % rhs,
            max_width: self.max_width % rhs,
            min_height: self.min_height % rhs,
            max_height: self.max_height % rhs,
        }
    }
}

impl std::ops::Neg for BoxConstraints {
    type Output = Self;

    /// Negates all constraint values (rarely useful, but mirrors Flutter's `~/`).
    #[inline]
    fn neg(self) -> Self::Output {
        Self {
            min_width: -self.min_width,
            max_width: -self.max_width,
            min_height: -self.min_height,
            max_height: -self.max_height,
        }
    }
}

// ============================================================================
// Rust-specific Enhancements: From traits
// ============================================================================

impl From<Size> for BoxConstraints {
    /// Creates tight constraints from a Size.
    ///
    /// # Example
    ///
    /// ```
    /// use flui_rendering::constraints::{BoxConstraints, Constraints};
    /// use flui_types::Size;
    ///
    /// let c: BoxConstraints = Size::new(100.0, 50.0).into();
    /// assert!(c.is_tight());
    /// ```
    #[inline]
    fn from(size: Size) -> Self {
        Self::tight(size)
    }
}

impl From<(f32, f32)> for BoxConstraints {
    /// Creates tight constraints from a (width, height) tuple.
    #[inline]
    fn from((width, height): (f32, f32)) -> Self {
        Self::tight(Size::new(width, height))
    }
}

impl From<[f32; 4]> for BoxConstraints {
    /// Creates constraints from [min_width, max_width, min_height, max_height].
    #[inline]
    fn from([min_width, max_width, min_height, max_height]: [f32; 4]) -> Self {
        Self::new(min_width, max_width, min_height, max_height)
    }
}

impl From<BoxConstraints> for Size {
    /// Returns the biggest size that satisfies the constraints.
    #[inline]
    fn from(c: BoxConstraints) -> Self {
        c.biggest()
    }
}

// ============================================================================
// Rust-specific Enhancements: Additional methods
// ============================================================================

impl BoxConstraints {
    /// Returns whether both width and height are bounded (max < infinity).
    ///
    /// # Rust Enhancement
    ///
    /// Convenience method combining `has_bounded_width()` and `has_bounded_height()`.
    #[inline]
    #[must_use]
    pub fn is_bounded(&self) -> bool {
        self.has_bounded_width() && self.has_bounded_height()
    }

    /// Returns whether the constraints are unbounded in any dimension.
    #[inline]
    #[must_use]
    pub fn is_unbounded(&self) -> bool {
        !self.has_bounded_width() || !self.has_bounded_height()
    }

    /// Returns whether both dimensions are expanding (min == infinity).
    #[inline]
    #[must_use]
    pub fn is_expanding(&self) -> bool {
        self.has_infinite_width() && self.has_infinite_height()
    }

    /// Returns the aspect ratio of the biggest size (width / height).
    ///
    /// Returns `None` if height would be zero or infinite.
    #[inline]
    #[must_use]
    pub fn aspect_ratio(&self) -> Option<f32> {
        let biggest = self.biggest();
        if biggest.height > 0.0 && biggest.height.is_finite() && biggest.width.is_finite() {
            Some(biggest.width / biggest.height)
        } else {
            None
        }
    }

    /// Returns constraints inflated by the given edges (opposite of deflate).
    #[must_use]
    pub fn inflate(&self, edges: EdgeInsets) -> Self {
        let horizontal = edges.left + edges.right;
        let vertical = edges.top + edges.bottom;
        Self {
            min_width: self.min_width + horizontal,
            max_width: self.max_width + horizontal,
            min_height: self.min_height + vertical,
            max_height: self.max_height + vertical,
        }
    }

    /// Returns the center point of the biggest size.
    #[inline]
    #[must_use]
    pub fn center(&self) -> flui_types::Offset {
        let biggest = self.biggest();
        flui_types::Offset::new(biggest.width / 2.0, biggest.height / 2.0)
    }

    /// Checks if these constraints are approximately equal to other constraints.
    ///
    /// Uses epsilon comparison for floating-point tolerance.
    #[inline]
    #[must_use]
    pub fn approx_eq(&self, other: &Self, epsilon: f32) -> bool {
        (self.min_width - other.min_width).abs() <= epsilon
            && (self.max_width - other.max_width).abs() <= epsilon
            && (self.min_height - other.min_height).abs() <= epsilon
            && (self.max_height - other.max_height).abs() <= epsilon
    }

    // ===== Builder pattern methods =====

    /// Returns a copy with min_width set to the given value.
    #[inline]
    #[must_use]
    pub const fn with_min_width(self, min_width: f32) -> Self {
        Self { min_width, ..self }
    }

    /// Returns a copy with max_width set to the given value.
    #[inline]
    #[must_use]
    pub const fn with_max_width(self, max_width: f32) -> Self {
        Self { max_width, ..self }
    }

    /// Returns a copy with min_height set to the given value.
    #[inline]
    #[must_use]
    pub const fn with_min_height(self, min_height: f32) -> Self {
        Self { min_height, ..self }
    }

    /// Returns a copy with max_height set to the given value.
    #[inline]
    #[must_use]
    pub const fn with_max_height(self, max_height: f32) -> Self {
        Self { max_height, ..self }
    }

    /// Returns a copy with tight width (min_width == max_width).
    #[inline]
    #[must_use]
    pub const fn with_tight_width(self, width: f32) -> Self {
        Self {
            min_width: width,
            max_width: width,
            ..self
        }
    }

    /// Returns a copy with tight height (min_height == max_height).
    #[inline]
    #[must_use]
    pub const fn with_tight_height(self, height: f32) -> Self {
        Self {
            min_height: height,
            max_height: height,
            ..self
        }
    }

    // ===== Set operations =====

    /// Returns constraints that are the intersection of self and other.
    ///
    /// The result satisfies both constraints (most restrictive).
    /// Returns `None` if the intersection is empty (no valid size).
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

    /// Returns constraints that are the union of self and other.
    ///
    /// The result satisfies either constraint (least restrictive).
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

    // ===== Additional utility methods =====

    /// Returns the area of the biggest size.
    #[inline]
    #[must_use]
    pub fn max_area(&self) -> f32 {
        let biggest = self.biggest();
        biggest.width * biggest.height
    }

    /// Returns the area of the smallest size.
    #[inline]
    #[must_use]
    pub fn min_area(&self) -> f32 {
        let smallest = self.smallest();
        smallest.width * smallest.height
    }

    /// Returns the diagonal length of the biggest size.
    #[inline]
    #[must_use]
    pub fn max_diagonal(&self) -> f32 {
        let biggest = self.biggest();
        (biggest.width * biggest.width + biggest.height * biggest.height).sqrt()
    }

    /// Returns whether this constraint fully contains another.
    ///
    /// True if any size satisfying `other` also satisfies `self`.
    #[inline]
    #[must_use]
    pub fn contains(&self, other: &Self) -> bool {
        self.min_width <= other.min_width
            && self.max_width >= other.max_width
            && self.min_height <= other.min_height
            && self.max_height >= other.max_height
    }

    /// Returns whether this constraint overlaps with another.
    ///
    /// True if there exists at least one size satisfying both constraints.
    #[inline]
    #[must_use]
    pub fn overlaps(&self, other: &Self) -> bool {
        self.intersection(other).is_some()
    }

    /// Clamps these constraints to fit within outer constraints.
    #[must_use]
    pub fn clamp_to(&self, outer: &Self) -> Self {
        Self {
            min_width: self.min_width.clamp(outer.min_width, outer.max_width),
            max_width: self.max_width.clamp(outer.min_width, outer.max_width),
            min_height: self.min_height.clamp(outer.min_height, outer.max_height),
            max_height: self.max_height.clamp(outer.min_height, outer.max_height),
        }
    }

    /// Returns constraints with width range expanded by delta on each side.
    #[inline]
    #[must_use]
    pub fn expand_width(&self, delta: f32) -> Self {
        Self {
            min_width: (self.min_width - delta).max(0.0),
            max_width: self.max_width + delta,
            ..*self
        }
    }

    /// Returns constraints with height range expanded by delta on each side.
    #[inline]
    #[must_use]
    pub fn expand_height(&self, delta: f32) -> Self {
        Self {
            min_height: (self.min_height - delta).max(0.0),
            max_height: self.max_height + delta,
            ..*self
        }
    }

    /// Returns the width range as (min, max) tuple.
    #[inline]
    #[must_use]
    pub const fn width_range(&self) -> (f32, f32) {
        (self.min_width, self.max_width)
    }

    /// Returns the height range as (min, max) tuple.
    #[inline]
    #[must_use]
    pub const fn height_range(&self) -> (f32, f32) {
        (self.min_height, self.max_height)
    }

    /// Returns whether the width is loose (min_width == 0).
    #[inline]
    #[must_use]
    pub fn has_loose_width(&self) -> bool {
        self.min_width == 0.0
    }

    /// Returns whether the height is loose (min_height == 0).
    #[inline]
    #[must_use]
    pub fn has_loose_height(&self) -> bool {
        self.min_height == 0.0
    }

    /// Returns whether these constraints are fully loose (min == 0 for both).
    #[inline]
    #[must_use]
    pub fn is_loose(&self) -> bool {
        self.has_loose_width() && self.has_loose_height()
    }

    /// Maps all constraint values through a function.
    #[inline]
    #[must_use]
    pub fn map<F: Fn(f32) -> f32>(&self, f: F) -> Self {
        Self {
            min_width: f(self.min_width),
            max_width: f(self.max_width),
            min_height: f(self.min_height),
            max_height: f(self.max_height),
        }
    }

    /// Returns constraints rounded to the nearest integer values.
    #[inline]
    #[must_use]
    pub fn round(&self) -> Self {
        self.map(|v| v.round())
    }

    /// Returns constraints with values rounded down.
    #[inline]
    #[must_use]
    pub fn floor(&self) -> Self {
        self.map(|v| v.floor())
    }

    /// Returns constraints with values rounded up.
    #[inline]
    #[must_use]
    pub fn ceil(&self) -> Self {
        self.map(|v| v.ceil())
    }
}

// ============================================================================
// Rust-specific Enhancements: Assignment Operators
// ============================================================================

impl std::ops::MulAssign<f32> for BoxConstraints {
    #[inline]
    fn mul_assign(&mut self, rhs: f32) {
        self.min_width *= rhs;
        self.max_width *= rhs;
        self.min_height *= rhs;
        self.max_height *= rhs;
    }
}

impl std::ops::DivAssign<f32> for BoxConstraints {
    #[inline]
    fn div_assign(&mut self, rhs: f32) {
        self.min_width /= rhs;
        self.max_width /= rhs;
        self.min_height /= rhs;
        self.max_height /= rhs;
    }
}

impl std::ops::Add for BoxConstraints {
    type Output = Self;

    /// Adds constraint values element-wise (union-like behavior for maximums).
    #[inline]
    fn add(self, rhs: Self) -> Self::Output {
        Self {
            min_width: self.min_width + rhs.min_width,
            max_width: self.max_width + rhs.max_width,
            min_height: self.min_height + rhs.min_height,
            max_height: self.max_height + rhs.max_height,
        }
    }
}

impl std::ops::Sub for BoxConstraints {
    type Output = Self;

    /// Subtracts constraint values element-wise.
    #[inline]
    fn sub(self, rhs: Self) -> Self::Output {
        Self {
            min_width: self.min_width - rhs.min_width,
            max_width: self.max_width - rhs.max_width,
            min_height: self.min_height - rhs.min_height,
            max_height: self.max_height - rhs.max_height,
        }
    }
}

impl std::ops::AddAssign for BoxConstraints {
    #[inline]
    fn add_assign(&mut self, rhs: Self) {
        self.min_width += rhs.min_width;
        self.max_width += rhs.max_width;
        self.min_height += rhs.min_height;
        self.max_height += rhs.max_height;
    }
}

impl std::ops::SubAssign for BoxConstraints {
    #[inline]
    fn sub_assign(&mut self, rhs: Self) {
        self.min_width -= rhs.min_width;
        self.max_width -= rhs.max_width;
        self.min_height -= rhs.min_height;
        self.max_height -= rhs.max_height;
    }
}

// ============================================================================
// Rust-specific Enhancements: Additional From traits
// ============================================================================

impl From<(Size, Size)> for BoxConstraints {
    /// Creates constraints from (min_size, max_size) tuple.
    #[inline]
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
    /// Converts to (min_width, max_width, min_height, max_height) tuple.
    #[inline]
    fn from(c: BoxConstraints) -> Self {
        (c.min_width, c.max_width, c.min_height, c.max_height)
    }
}

impl From<BoxConstraints> for [f32; 4] {
    /// Converts to [min_width, max_width, min_height, max_height] array.
    #[inline]
    fn from(c: BoxConstraints) -> Self {
        [c.min_width, c.max_width, c.min_height, c.max_height]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tight() {
        let c = BoxConstraints::tight(Size::new(100.0, 50.0));
        assert!(c.is_tight());
        assert!(c.has_tight_width());
        assert!(c.has_tight_height());
        assert_eq!(c.biggest(), Size::new(100.0, 50.0));
        assert_eq!(c.smallest(), Size::new(100.0, 50.0));
    }

    #[test]
    fn test_loose() {
        let c = BoxConstraints::loose(Size::new(100.0, 50.0));
        assert!(!c.is_tight());
        assert_eq!(c.min_width, 0.0);
        assert_eq!(c.min_height, 0.0);
        assert_eq!(c.max_width, 100.0);
        assert_eq!(c.max_height, 50.0);
    }

    #[test]
    fn test_constrain() {
        let c = BoxConstraints::new(50.0, 150.0, 30.0, 100.0);

        assert_eq!(c.constrain(Size::new(40.0, 20.0)), Size::new(50.0, 30.0));
        assert_eq!(
            c.constrain(Size::new(200.0, 150.0)),
            Size::new(150.0, 100.0)
        );
        assert_eq!(c.constrain(Size::new(100.0, 50.0)), Size::new(100.0, 50.0));
    }

    #[test]
    fn test_flipped() {
        let c = BoxConstraints::new(50.0, 150.0, 30.0, 100.0);
        let flipped = c.flipped();

        assert_eq!(flipped.min_width, 30.0);
        assert_eq!(flipped.max_width, 100.0);
        assert_eq!(flipped.min_height, 50.0);
        assert_eq!(flipped.max_height, 150.0);
    }

    #[test]
    fn test_loosen() {
        let c = BoxConstraints::new(50.0, 150.0, 30.0, 100.0);
        let loosened = c.loosen();

        assert_eq!(loosened.min_width, 0.0);
        assert_eq!(loosened.min_height, 0.0);
        assert_eq!(loosened.max_width, 150.0);
        assert_eq!(loosened.max_height, 100.0);
    }

    #[test]
    fn test_enforce() {
        let inner = BoxConstraints::new(0.0, 200.0, 0.0, 200.0);
        let outer = BoxConstraints::new(50.0, 150.0, 50.0, 150.0);
        let enforced = inner.enforce(outer);

        assert_eq!(enforced.min_width, 50.0);
        assert_eq!(enforced.max_width, 150.0);
        assert_eq!(enforced.min_height, 50.0);
        assert_eq!(enforced.max_height, 150.0);
    }

    #[test]
    fn test_lerp() {
        let a = BoxConstraints::new(0.0, 100.0, 0.0, 100.0);
        let b = BoxConstraints::new(100.0, 200.0, 100.0, 200.0);
        let mid = a.lerp(&b, 0.5);

        assert_eq!(mid.min_width, 50.0);
        assert_eq!(mid.max_width, 150.0);
    }

    #[test]
    fn test_is_normalized() {
        assert!(BoxConstraints::new(50.0, 150.0, 30.0, 100.0).is_normalized());
        assert!(!BoxConstraints::new(150.0, 50.0, 30.0, 100.0).is_normalized());
        assert!(!BoxConstraints::new(-10.0, 50.0, 30.0, 100.0).is_normalized());
    }

    #[test]
    fn test_aspect_ratio_preservation() {
        let c = BoxConstraints::new(0.0, 200.0, 0.0, 100.0);
        let size = Size::new(400.0, 200.0); // 2:1 aspect ratio
        let result = c.constrain_size_and_attempt_to_preserve_aspect_ratio(size);

        // Should maintain 2:1 ratio: 200x100
        assert_eq!(result, Size::new(200.0, 100.0));
    }

    // ===== Flutter API completeness tests =====

    #[test]
    fn test_width_constraints() {
        let c = BoxConstraints::new(50.0, 150.0, 30.0, 100.0);
        let width_only = c.width_constraints();

        assert_eq!(width_only.min_width, 50.0);
        assert_eq!(width_only.max_width, 150.0);
        assert_eq!(width_only.min_height, 0.0);
        assert_eq!(width_only.max_height, f32::INFINITY);
    }

    #[test]
    fn test_height_constraints() {
        let c = BoxConstraints::new(50.0, 150.0, 30.0, 100.0);
        let height_only = c.height_constraints();

        assert_eq!(height_only.min_width, 0.0);
        assert_eq!(height_only.max_width, f32::INFINITY);
        assert_eq!(height_only.min_height, 30.0);
        assert_eq!(height_only.max_height, 100.0);
    }

    #[test]
    fn test_normalize() {
        // Normal case - already normalized
        let c = BoxConstraints::new(50.0, 150.0, 30.0, 100.0);
        let n = c.normalize();
        assert_eq!(n, c);

        // min > max - should swap
        let c = BoxConstraints::new(150.0, 50.0, 100.0, 30.0);
        let n = c.normalize();
        assert_eq!(n.min_width, 50.0);
        assert_eq!(n.max_width, 150.0);
        assert_eq!(n.min_height, 30.0);
        assert_eq!(n.max_height, 100.0);

        // Negative values - should clamp to 0
        let c = BoxConstraints::new(-10.0, 50.0, -20.0, 100.0);
        let n = c.normalize();
        assert_eq!(n.min_width, 0.0);
        assert_eq!(n.max_width, 50.0);
        assert_eq!(n.min_height, 0.0);
        assert_eq!(n.max_height, 100.0);

        // NaN values - should become 0
        let c = BoxConstraints::new(f32::NAN, 50.0, 30.0, f32::NAN);
        let n = c.normalize();
        assert_eq!(n.min_width, 0.0);
        assert_eq!(n.max_width, 50.0);
        assert_eq!(n.min_height, 0.0);
        assert_eq!(n.max_height, 30.0);
    }

    // ===== Rust operator tests =====

    #[test]
    fn test_mul_operator() {
        let c = BoxConstraints::new(10.0, 100.0, 20.0, 200.0);
        let scaled = c * 2.0;
        assert_eq!(scaled.min_width, 20.0);
        assert_eq!(scaled.max_width, 200.0);
        assert_eq!(scaled.min_height, 40.0);
        assert_eq!(scaled.max_height, 400.0);

        // Also test f32 * BoxConstraints
        let scaled2 = 2.0 * c;
        assert_eq!(scaled2, scaled);
    }

    #[test]
    fn test_div_operator() {
        let c = BoxConstraints::new(20.0, 200.0, 40.0, 400.0);
        let divided = c / 2.0;
        assert_eq!(divided.min_width, 10.0);
        assert_eq!(divided.max_width, 100.0);
        assert_eq!(divided.min_height, 20.0);
        assert_eq!(divided.max_height, 200.0);
    }

    #[test]
    fn test_rem_operator() {
        let c = BoxConstraints::new(15.0, 105.0, 25.0, 205.0);
        let rem = c % 10.0;
        assert_eq!(rem.min_width, 5.0);
        assert_eq!(rem.max_width, 5.0);
        assert_eq!(rem.min_height, 5.0);
        assert_eq!(rem.max_height, 5.0);
    }

    #[test]
    fn test_neg_operator() {
        let c = BoxConstraints::new(10.0, 100.0, 20.0, 200.0);
        let neg = -c;
        assert_eq!(neg.min_width, -10.0);
        assert_eq!(neg.max_width, -100.0);
        assert_eq!(neg.min_height, -20.0);
        assert_eq!(neg.max_height, -200.0);
    }

    // ===== From trait tests =====

    #[test]
    fn test_from_size() {
        let c: BoxConstraints = Size::new(100.0, 50.0).into();
        assert!(c.is_tight());
        assert_eq!(c.min_width, 100.0);
        assert_eq!(c.max_width, 100.0);
        assert_eq!(c.min_height, 50.0);
        assert_eq!(c.max_height, 50.0);
    }

    #[test]
    fn test_from_tuple() {
        let c: BoxConstraints = (100.0, 50.0).into();
        assert!(c.is_tight());
        assert_eq!(c.min_width, 100.0);
        assert_eq!(c.max_width, 100.0);
    }

    #[test]
    fn test_from_array() {
        let c: BoxConstraints = [10.0, 100.0, 20.0, 200.0].into();
        assert_eq!(c.min_width, 10.0);
        assert_eq!(c.max_width, 100.0);
        assert_eq!(c.min_height, 20.0);
        assert_eq!(c.max_height, 200.0);
    }

    #[test]
    fn test_into_size() {
        let c = BoxConstraints::new(50.0, 100.0, 30.0, 80.0);
        let size: Size = c.into();
        assert_eq!(size, c.biggest());
    }

    // ===== Additional Rust enhancement tests =====

    #[test]
    fn test_is_bounded() {
        let bounded = BoxConstraints::new(0.0, 100.0, 0.0, 100.0);
        assert!(bounded.is_bounded());

        let unbounded_w = BoxConstraints::new(0.0, f32::INFINITY, 0.0, 100.0);
        assert!(!unbounded_w.is_bounded());

        let unbounded_h = BoxConstraints::new(0.0, 100.0, 0.0, f32::INFINITY);
        assert!(!unbounded_h.is_bounded());
    }

    #[test]
    fn test_is_unbounded() {
        let bounded = BoxConstraints::new(0.0, 100.0, 0.0, 100.0);
        assert!(!bounded.is_unbounded());

        let unbounded = BoxConstraints::UNCONSTRAINED;
        assert!(unbounded.is_unbounded());
    }

    #[test]
    fn test_is_expanding() {
        let expanding = BoxConstraints::expand();
        assert!(expanding.is_expanding());

        let partial = BoxConstraints::expand_with(Some(100.0), None);
        assert!(!partial.is_expanding()); // width is finite
    }

    #[test]
    fn test_aspect_ratio_method() {
        let c = BoxConstraints::loose(Size::new(200.0, 100.0));
        assert_eq!(c.aspect_ratio(), Some(2.0));

        let zero_height = BoxConstraints::loose(Size::new(100.0, 0.0));
        assert_eq!(zero_height.aspect_ratio(), None);

        let infinite = BoxConstraints::UNCONSTRAINED;
        assert_eq!(infinite.aspect_ratio(), None);
    }

    #[test]
    fn test_inflate() {
        let c = BoxConstraints::new(50.0, 100.0, 30.0, 80.0);
        let edges = EdgeInsets::all(10.0);
        let inflated = c.inflate(edges);

        assert_eq!(inflated.min_width, 70.0); // 50 + 20
        assert_eq!(inflated.max_width, 120.0); // 100 + 20
        assert_eq!(inflated.min_height, 50.0); // 30 + 20
        assert_eq!(inflated.max_height, 100.0); // 80 + 20
    }

    #[test]
    fn test_inflate_deflate_inverse() {
        let c = BoxConstraints::new(50.0, 100.0, 30.0, 80.0);
        let edges = EdgeInsets::all(10.0);

        // inflate then deflate should give original (approximately)
        let inflated = c.inflate(edges);
        let deflated = inflated.deflate(edges);

        assert!(c.approx_eq(&deflated, 0.001));
    }

    #[test]
    fn test_center() {
        let c = BoxConstraints::loose(Size::new(200.0, 100.0));
        let center = c.center();
        assert_eq!(center.dx, 100.0);
        assert_eq!(center.dy, 50.0);
    }

    #[test]
    fn test_approx_eq() {
        let a = BoxConstraints::new(100.0, 200.0, 50.0, 150.0);
        let b = BoxConstraints::new(100.001, 199.999, 50.001, 149.999);

        assert!(a.approx_eq(&b, 0.01));
        assert!(!a.approx_eq(&b, 0.0001));
    }

    // ===== Builder pattern tests =====

    #[test]
    fn test_with_min_width() {
        let c = BoxConstraints::loose(Size::new(100.0, 100.0));
        let c = c.with_min_width(50.0);
        assert_eq!(c.min_width, 50.0);
        assert_eq!(c.max_width, 100.0);
    }

    #[test]
    fn test_with_max_width() {
        let c = BoxConstraints::new(0.0, 100.0, 0.0, 100.0);
        let c = c.with_max_width(200.0);
        assert_eq!(c.max_width, 200.0);
    }

    #[test]
    fn test_with_tight_width() {
        let c = BoxConstraints::UNCONSTRAINED.with_tight_width(100.0);
        assert!(c.has_tight_width());
        assert_eq!(c.min_width, 100.0);
        assert_eq!(c.max_width, 100.0);
    }

    #[test]
    fn test_with_tight_height() {
        let c = BoxConstraints::UNCONSTRAINED.with_tight_height(50.0);
        assert!(c.has_tight_height());
        assert_eq!(c.min_height, 50.0);
        assert_eq!(c.max_height, 50.0);
    }

    #[test]
    fn test_builder_chaining() {
        let c = BoxConstraints::UNCONSTRAINED
            .with_min_width(10.0)
            .with_max_width(100.0)
            .with_min_height(20.0)
            .with_max_height(200.0);

        assert_eq!(c.min_width, 10.0);
        assert_eq!(c.max_width, 100.0);
        assert_eq!(c.min_height, 20.0);
        assert_eq!(c.max_height, 200.0);
    }

    // ===== Set operations tests =====

    #[test]
    fn test_intersection() {
        let a = BoxConstraints::new(0.0, 100.0, 0.0, 100.0);
        let b = BoxConstraints::new(50.0, 150.0, 50.0, 150.0);

        let inter = a.intersection(&b).unwrap();
        assert_eq!(inter.min_width, 50.0);
        assert_eq!(inter.max_width, 100.0);
        assert_eq!(inter.min_height, 50.0);
        assert_eq!(inter.max_height, 100.0);
    }

    #[test]
    fn test_intersection_empty() {
        let a = BoxConstraints::new(0.0, 50.0, 0.0, 50.0);
        let b = BoxConstraints::new(100.0, 200.0, 100.0, 200.0);

        assert!(a.intersection(&b).is_none());
    }

    #[test]
    fn test_union() {
        let a = BoxConstraints::new(20.0, 80.0, 20.0, 80.0);
        let b = BoxConstraints::new(50.0, 150.0, 50.0, 150.0);

        let uni = a.union(&b);
        assert_eq!(uni.min_width, 20.0);
        assert_eq!(uni.max_width, 150.0);
        assert_eq!(uni.min_height, 20.0);
        assert_eq!(uni.max_height, 150.0);
    }

    #[test]
    fn test_contains() {
        let outer = BoxConstraints::new(0.0, 200.0, 0.0, 200.0);
        let inner = BoxConstraints::new(50.0, 150.0, 50.0, 150.0);

        assert!(outer.contains(&inner));
        assert!(!inner.contains(&outer));
    }

    #[test]
    fn test_overlaps() {
        let a = BoxConstraints::new(0.0, 100.0, 0.0, 100.0);
        let b = BoxConstraints::new(50.0, 150.0, 50.0, 150.0);
        let c = BoxConstraints::new(200.0, 300.0, 200.0, 300.0);

        assert!(a.overlaps(&b));
        assert!(!a.overlaps(&c));
    }

    // ===== Utility methods tests =====

    #[test]
    fn test_max_area() {
        let c = BoxConstraints::loose(Size::new(100.0, 50.0));
        assert_eq!(c.max_area(), 5000.0);
    }

    #[test]
    fn test_min_area() {
        let c = BoxConstraints::new(10.0, 100.0, 20.0, 200.0);
        assert_eq!(c.min_area(), 200.0); // 10 * 20
    }

    #[test]
    fn test_max_diagonal() {
        let c = BoxConstraints::loose(Size::new(3.0, 4.0));
        assert!((c.max_diagonal() - 5.0).abs() < 0.001); // 3-4-5 triangle
    }

    #[test]
    fn test_clamp_to() {
        let inner = BoxConstraints::new(0.0, 200.0, 0.0, 200.0);
        let outer = BoxConstraints::new(50.0, 150.0, 50.0, 150.0);

        let clamped = inner.clamp_to(&outer);
        assert_eq!(clamped.min_width, 50.0);
        assert_eq!(clamped.max_width, 150.0);
    }

    #[test]
    fn test_expand_width() {
        let c = BoxConstraints::new(50.0, 100.0, 30.0, 80.0);
        let expanded = c.expand_width(10.0);

        assert_eq!(expanded.min_width, 40.0);
        assert_eq!(expanded.max_width, 110.0);
        assert_eq!(expanded.min_height, 30.0); // unchanged
    }

    #[test]
    fn test_expand_height() {
        let c = BoxConstraints::new(50.0, 100.0, 30.0, 80.0);
        let expanded = c.expand_height(10.0);

        assert_eq!(expanded.min_height, 20.0);
        assert_eq!(expanded.max_height, 90.0);
        assert_eq!(expanded.min_width, 50.0); // unchanged
    }

    #[test]
    fn test_width_height_range() {
        let c = BoxConstraints::new(10.0, 100.0, 20.0, 200.0);
        assert_eq!(c.width_range(), (10.0, 100.0));
        assert_eq!(c.height_range(), (20.0, 200.0));
    }

    #[test]
    fn test_is_loose() {
        let loose = BoxConstraints::loose(Size::new(100.0, 100.0));
        assert!(loose.is_loose());
        assert!(loose.has_loose_width());
        assert!(loose.has_loose_height());

        let not_loose = BoxConstraints::new(10.0, 100.0, 0.0, 100.0);
        assert!(!not_loose.is_loose());
        assert!(!not_loose.has_loose_width());
        assert!(not_loose.has_loose_height());
    }

    #[test]
    fn test_map() {
        let c = BoxConstraints::new(10.0, 100.0, 20.0, 200.0);
        let doubled = c.map(|v| v * 2.0);

        assert_eq!(doubled.min_width, 20.0);
        assert_eq!(doubled.max_width, 200.0);
        assert_eq!(doubled.min_height, 40.0);
        assert_eq!(doubled.max_height, 400.0);
    }

    #[test]
    fn test_round_floor_ceil() {
        let c = BoxConstraints::new(10.3, 100.7, 20.5, 200.1);

        let rounded = c.round();
        assert_eq!(rounded.min_width, 10.0);
        assert_eq!(rounded.max_width, 101.0);

        let floored = c.floor();
        assert_eq!(floored.min_width, 10.0);
        assert_eq!(floored.max_width, 100.0);

        let ceiled = c.ceil();
        assert_eq!(ceiled.min_width, 11.0);
        assert_eq!(ceiled.max_width, 101.0);
    }

    // ===== Assignment operators tests =====

    #[test]
    fn test_mul_assign() {
        let mut c = BoxConstraints::new(10.0, 100.0, 20.0, 200.0);
        c *= 2.0;
        assert_eq!(c.min_width, 20.0);
        assert_eq!(c.max_width, 200.0);
    }

    #[test]
    fn test_div_assign() {
        let mut c = BoxConstraints::new(20.0, 200.0, 40.0, 400.0);
        c /= 2.0;
        assert_eq!(c.min_width, 10.0);
        assert_eq!(c.max_width, 100.0);
    }

    #[test]
    fn test_add_operator() {
        let a = BoxConstraints::new(10.0, 100.0, 20.0, 200.0);
        let b = BoxConstraints::new(5.0, 50.0, 10.0, 100.0);
        let sum = a + b;

        assert_eq!(sum.min_width, 15.0);
        assert_eq!(sum.max_width, 150.0);
    }

    #[test]
    fn test_sub_operator() {
        let a = BoxConstraints::new(10.0, 100.0, 20.0, 200.0);
        let b = BoxConstraints::new(5.0, 50.0, 10.0, 100.0);
        let diff = a - b;

        assert_eq!(diff.min_width, 5.0);
        assert_eq!(diff.max_width, 50.0);
    }

    #[test]
    fn test_add_assign() {
        let mut c = BoxConstraints::new(10.0, 100.0, 20.0, 200.0);
        c += BoxConstraints::new(5.0, 50.0, 10.0, 100.0);

        assert_eq!(c.min_width, 15.0);
        assert_eq!(c.max_width, 150.0);
    }

    #[test]
    fn test_sub_assign() {
        let mut c = BoxConstraints::new(10.0, 100.0, 20.0, 200.0);
        c -= BoxConstraints::new(5.0, 50.0, 10.0, 100.0);

        assert_eq!(c.min_width, 5.0);
        assert_eq!(c.max_width, 50.0);
    }

    // ===== Additional From trait tests =====

    #[test]
    fn test_from_size_tuple() {
        let min = Size::new(10.0, 20.0);
        let max = Size::new(100.0, 200.0);
        let c: BoxConstraints = (min, max).into();

        assert_eq!(c.min_width, 10.0);
        assert_eq!(c.max_width, 100.0);
        assert_eq!(c.min_height, 20.0);
        assert_eq!(c.max_height, 200.0);
    }

    #[test]
    fn test_into_tuple() {
        let c = BoxConstraints::new(10.0, 100.0, 20.0, 200.0);
        let tuple: (f32, f32, f32, f32) = c.into();
        assert_eq!(tuple, (10.0, 100.0, 20.0, 200.0));
    }

    #[test]
    fn test_into_array() {
        let c = BoxConstraints::new(10.0, 100.0, 20.0, 200.0);
        let arr: [f32; 4] = c.into();
        assert_eq!(arr, [10.0, 100.0, 20.0, 200.0]);
    }
}
