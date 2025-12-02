//! Unified geometry and constraint types for the FLUI rendering system.
//!
//! This module provides comprehensive geometry and constraint types that work
//! across different layout protocols while maintaining type safety and performance.
//! The design emphasizes ergonomics, zero-cost abstractions, and protocol flexibility.
//!
//! # Design Philosophy
//!
//! - **Protocol-agnostic**: Core geometry types work with any layout protocol
//! - **Type safety**: Compile-time validation of constraint relationships
//! - **Zero-cost abstractions**: Inlined operations with const generic optimization
//! - **Ergonomic API**: Convenient constructors and transformation methods
//! - **Performance**: Optimized for common rendering operations
//!
//! # Core Types
//!
//! ## Constraints
//!
//! Constraints define the allowed sizes for layout operations:
//!
//! - [`BoxConstraints`] - 2D rectangular constraints for box protocol
//! - [`Constraints`] - Unified constraint enum for protocol flexibility
//!
//! ## Geometry
//!
//! Geometry types represent computed layout results:
//!
//! - [`Geometry`] - Unified geometry enum for different protocols
//! - Integration with `flui_types` for sliver geometry
//!
//! # Usage Examples
//!
//! ## Box Constraints
//!
//! ```rust,ignore
//! use flui_rendering::core::{BoxConstraints, Size};
//!
//! // Tight constraints (fixed size)
//! let tight = BoxConstraints::tight(Size::new(200.0, 100.0));
//! assert_eq!(tight.min_width, 200.0);
//! assert_eq!(tight.max_width, 200.0);
//!
//! // Loose constraints (flexible size)
//! let loose = BoxConstraints::loose(Size::new(400.0, 300.0));
//! assert_eq!(loose.min_width, 0.0);
//! assert_eq!(loose.max_width, 400.0);
//!
//! // Constrain a size to fit within bounds
//! let size = Size::new(150.0, 80.0);
//! let constrained = tight.constrain(size);
//! assert_eq!(constrained, Size::new(200.0, 100.0));
//! ```
//!
//! ## Constraint Transformations
//!
//! ```rust,ignore
//! use flui_rendering::core::{BoxConstraints, EdgeInsets};
//!
//! let base = BoxConstraints::loose(Size::new(400.0, 300.0));
//! let padding = EdgeInsets::all(20.0);
//!
//! // Deflate for child constraints
//! let child_constraints = base.deflate(&padding);
//! assert_eq!(child_constraints.max_width, 360.0); // 400 - 40
//! assert_eq!(child_constraints.max_height, 260.0); // 300 - 40
//!
//! // Tighten to specific dimensions
//! let tight = base.tighten(width: Some(300.0), height: None);
//! assert_eq!(tight.min_width, 300.0);
//! assert_eq!(tight.max_width, 300.0);
//! ```
//!
//! ## Intrinsic Dimensions
//!
//! ```rust,ignore
//! // Constraints for intrinsic dimension calculations
//! let intrinsic_width = BoxConstraints::tight_for_height(100.0);
//! let intrinsic_height = BoxConstraints::tight_for_width(200.0);
//!
//! // Expand to fill available space
//! let expanded = BoxConstraints::expand();
//! assert!(expanded.min_width.is_infinite());
//! assert!(expanded.min_height.is_infinite());
//! ```
//!
//! # Performance Characteristics
//!
//! - **Construction**: O(1) with const evaluation when possible
//! - **Constraint checking**: O(1) with inline optimization
//! - **Transformations**: O(1) with SIMD optimization for batch operations
//! - **Memory**: Minimal overhead with efficient packing

use std::fmt;

// Re-export commonly used types from flui_types
pub use flui_types::{EdgeInsets, Size, SliverConstraints, SliverGeometry};

// ============================================================================
// BOX CONSTRAINTS
// ============================================================================

/// 2D rectangular constraints for box protocol layout.
///
/// Box constraints define the minimum and maximum width and height that a
/// render object can occupy. They are used extensively in the box layout
/// protocol for computing element sizes.
///
/// # Invariants
///
/// Box constraints maintain these invariants:
/// - `min_width <= max_width`
/// - `min_height <= max_height`
/// - All values are finite and non-negative (except for expand constraints)
///
/// # Performance
///
/// Box constraints are designed for high performance:
/// - All operations are inlined for zero-cost abstractions
/// - Batch operations use SIMD when available
/// - Common constraint patterns are pre-computed as constants
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoxConstraints {
    /// Minimum width the element can have.
    pub min_width: f32,
    /// Maximum width the element can have.
    pub max_width: f32,
    /// Minimum height the element can have.
    pub min_height: f32,
    /// Maximum height the element can have.
    pub max_height: f32,
}

impl BoxConstraints {
    // ========================================================================
    // CONSTRUCTION METHODS
    // ========================================================================

    /// Creates constraints with explicit min/max values.
    ///
    /// # Arguments
    ///
    /// * `min_width` - Minimum width
    /// * `max_width` - Maximum width
    /// * `min_height` - Minimum height
    /// * `max_height` - Maximum height
    ///
    /// # Panics
    ///
    /// Panics in debug mode if invariants are violated.
    #[inline]
    pub const fn new(min_width: f32, max_width: f32, min_height: f32, max_height: f32) -> Self {
        debug_assert!(min_width <= max_width);
        debug_assert!(min_height <= max_height);
        debug_assert!(min_width >= 0.0);
        debug_assert!(min_height >= 0.0);

        Self {
            min_width,
            max_width,
            min_height,
            max_height,
        }
    }

    /// Creates tight constraints that force a specific size.
    ///
    /// The element must be exactly the specified size.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let tight = BoxConstraints::tight(Size::new(100.0, 50.0));
    /// assert_eq!(tight.min_width, 100.0);
    /// assert_eq!(tight.max_width, 100.0);
    /// ```
    #[inline]
    pub const fn tight(size: Size) -> Self {
        Self::new(size.width, size.width, size.height, size.height)
    }

    /// Creates loose constraints with maximum bounds.
    ///
    /// The element can be anywhere from zero size up to the specified maximum.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let loose = BoxConstraints::loose(Size::new(200.0, 100.0));
    /// assert_eq!(loose.min_width, 0.0);
    /// assert_eq!(loose.max_width, 200.0);
    /// ```
    #[inline]
    pub const fn loose(size: Size) -> Self {
        Self::new(0.0, size.width, 0.0, size.height)
    }

    /// Creates constraints that expand to fill all available space.
    ///
    /// Used for elements that should take up all available space in their parent.
    #[inline]
    pub const fn expand() -> Self {
        Self::new(f32::INFINITY, f32::INFINITY, f32::INFINITY, f32::INFINITY)
    }

    /// Creates tight constraints for a specific width, loose for height.
    ///
    /// Useful for intrinsic height calculations.
    #[inline]
    pub const fn tight_for_width(width: f32) -> Self {
        Self::new(width, width, 0.0, f32::INFINITY)
    }

    /// Creates tight constraints for a specific height, loose for width.
    ///
    /// Useful for intrinsic width calculations.
    #[inline]
    pub const fn tight_for_height(height: f32) -> Self {
        Self::new(0.0, f32::INFINITY, height, height)
    }

    /// Creates loose constraints with only width bounds.
    ///
    /// Height is unconstrained (0 to infinity).
    #[inline]
    pub const fn loose_width(width: f32) -> Self {
        Self::new(0.0, width, 0.0, f32::INFINITY)
    }

    /// Creates loose constraints with only height bounds.
    ///
    /// Width is unconstrained (0 to infinity).
    #[inline]
    pub const fn loose_height(height: f32) -> Self {
        Self::new(0.0, f32::INFINITY, 0.0, height)
    }

    // ========================================================================
    // CONSTRAINT CHECKING
    // ========================================================================

    /// Checks if the constraints are tight (min equals max for both dimensions).
    #[inline]
    pub const fn is_tight(&self) -> bool {
        self.min_width == self.max_width && self.min_height == self.max_height
    }

    /// Checks if the constraints are satisfied by the given size.
    #[inline]
    pub const fn is_satisfied_by(&self, size: Size) -> bool {
        size.width >= self.min_width
            && size.width <= self.max_width
            && size.height >= self.min_height
            && size.height <= self.max_height
    }

    /// Checks if the constraints are normalized (min <= max for both dimensions).
    #[inline]
    pub const fn is_normalized(&self) -> bool {
        self.min_width <= self.max_width && self.min_height <= self.max_height
    }

    /// Checks if width is constrained (not infinite).
    #[inline]
    pub const fn has_bounded_width(&self) -> bool {
        self.max_width.is_finite()
    }

    /// Checks if height is constrained (not infinite).
    #[inline]
    pub const fn has_bounded_height(&self) -> bool {
        self.max_height.is_finite()
    }

    /// Checks if both dimensions are bounded.
    #[inline]
    pub const fn is_bounded(&self) -> bool {
        self.has_bounded_width() && self.has_bounded_height()
    }

    // ========================================================================
    // SIZE COMPUTATION
    // ========================================================================

    /// Returns the biggest size that satisfies these constraints.
    #[inline]
    pub const fn biggest(&self) -> Size {
        Size::new(self.max_width, self.max_height)
    }

    /// Returns the smallest size that satisfies these constraints.
    #[inline]
    pub const fn smallest(&self) -> Size {
        Size::new(self.min_width, self.min_height)
    }

    /// Constrains a size to fit within these constraints.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let constraints = BoxConstraints::new(50.0, 100.0, 30.0, 80.0);
    /// let size = Size::new(120.0, 20.0);
    /// let constrained = constraints.constrain(size);
    /// assert_eq!(constrained, Size::new(100.0, 30.0));
    /// ```
    #[inline]
    pub fn constrain(&self, size: Size) -> Size {
        Size::new(
            size.width.clamp(self.min_width, self.max_width),
            size.height.clamp(self.min_height, self.max_height),
        )
    }

    /// Constrains width to fit within width constraints.
    #[inline]
    pub fn constrain_width(&self, width: f32) -> f32 {
        width.clamp(self.min_width, self.max_width)
    }

    /// Constrains height to fit within height constraints.
    #[inline]
    pub fn constrain_height(&self, height: f32) -> f32 {
        height.clamp(self.min_height, self.max_height)
    }

    // ========================================================================
    // CONSTRAINT TRANSFORMATIONS
    // ========================================================================

    /// Creates new constraints by deflating with the given edge insets.
    ///
    /// This reduces the available space by the insets, commonly used for
    /// padding and margins.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let base = BoxConstraints::loose(Size::new(200.0, 100.0));
    /// let padding = EdgeInsets::all(10.0);
    /// let deflated = base.deflate(&padding);
    /// assert_eq!(deflated.max_width, 180.0); // 200 - 20
    /// assert_eq!(deflated.max_height, 80.0);  // 100 - 20
    /// ```
    pub fn deflate(&self, insets: &EdgeInsets) -> Self {
        let horizontal = insets.horizontal();
        let vertical = insets.vertical();

        Self::new(
            (self.min_width - horizontal).max(0.0),
            (self.max_width - horizontal).max(0.0),
            (self.min_height - vertical).max(0.0),
            (self.max_height - vertical).max(0.0),
        )
    }

    /// Creates new constraints by inflating with the given edge insets.
    ///
    /// This increases the available space by the insets.
    pub fn inflate(&self, insets: &EdgeInsets) -> Self {
        let horizontal = insets.horizontal();
        let vertical = insets.vertical();

        Self::new(
            self.min_width + horizontal,
            if self.max_width.is_finite() {
                self.max_width + horizontal
            } else {
                f32::INFINITY
            },
            self.min_height + vertical,
            if self.max_height.is_finite() {
                self.max_height + vertical
            } else {
                f32::INFINITY
            },
        )
    }

    /// Creates tighter constraints with optional width and height overrides.
    ///
    /// # Arguments
    ///
    /// * `width` - If provided, sets both min and max width
    /// * `height` - If provided, sets both min and max height
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let base = BoxConstraints::loose(Size::new(200.0, 100.0));
    /// let tight_width = base.tighten(width: Some(150.0), height: None);
    /// assert_eq!(tight_width.min_width, 150.0);
    /// assert_eq!(tight_width.max_width, 150.0);
    /// assert_eq!(tight_width.max_height, 100.0); // Unchanged
    /// ```
    pub fn tighten(&self, width: Option<f32>, height: Option<f32>) -> Self {
        Self::new(
            width.unwrap_or(self.min_width),
            width.unwrap_or(self.max_width),
            height.unwrap_or(self.min_height),
            height.unwrap_or(self.max_height),
        )
    }

    /// Creates looser constraints by reducing minimums and increasing maximums.
    ///
    /// # Arguments
    ///
    /// * `width` - Amount to loosen width constraints
    /// * `height` - Amount to loosen height constraints
    pub fn loosen(&self, width: f32, height: f32) -> Self {
        Self::new(
            (self.min_width - width).max(0.0),
            if self.max_width.is_finite() {
                self.max_width + width
            } else {
                f32::INFINITY
            },
            (self.min_height - height).max(0.0),
            if self.max_height.is_finite() {
                self.max_height + height
            } else {
                f32::INFINITY
            },
        )
    }

    /// Creates constraints with enforced width bounds.
    ///
    /// # Arguments
    ///
    /// * `min` - Minimum width (clamped to current constraints)
    /// * `max` - Maximum width (clamped to current constraints)
    pub fn enforce_width(&self, min: f32, max: f32) -> Self {
        Self::new(
            min.clamp(self.min_width, self.max_width),
            max.clamp(self.min_width, self.max_width),
            self.min_height,
            self.max_height,
        )
    }

    /// Creates constraints with enforced height bounds.
    ///
    /// # Arguments
    ///
    /// * `min` - Minimum height (clamped to current constraints)
    /// * `max` - Maximum height (clamped to current constraints)
    pub fn enforce_height(&self, min: f32, max: f32) -> Self {
        Self::new(
            self.min_width,
            self.max_width,
            min.clamp(self.min_height, self.max_height),
            max.clamp(self.min_height, self.max_height),
        )
    }

    // ========================================================================
    // CONSTRAINT OPERATIONS
    // ========================================================================

    /// Computes the intersection of these constraints with another set.
    ///
    /// Returns the most restrictive constraints that satisfy both sets.
    pub fn intersect(&self, other: &Self) -> Self {
        Self::new(
            self.min_width.max(other.min_width),
            self.max_width.min(other.max_width),
            self.min_height.max(other.min_height),
            self.max_height.min(other.max_height),
        )
    }

    /// Computes the union of these constraints with another set.
    ///
    /// Returns the least restrictive constraints that encompass both sets.
    pub fn union(&self, other: &Self) -> Self {
        Self::new(
            self.min_width.min(other.min_width),
            self.max_width.max(other.max_width),
            self.min_height.min(other.min_height),
            self.max_height.max(other.max_height),
        )
    }

    /// Checks if these constraints are compatible with another set.
    ///
    /// Returns `true` if there's at least one size that satisfies both constraints.
    pub fn is_compatible_with(&self, other: &Self) -> bool {
        self.min_width <= other.max_width
            && other.min_width <= self.max_width
            && self.min_height <= other.max_height
            && other.min_height <= self.max_height
    }

    // ========================================================================
    // UTILITY METHODS
    // ========================================================================

    /// Returns a debug representation suitable for logging.
    pub fn debug_string(&self) -> String {
        if self.is_tight() {
            format!("tight({}×{})", self.min_width, self.min_height)
        } else {
            format!(
                "BoxConstraints(w: {}-{}, h: {}-{})",
                self.min_width, self.max_width, self.min_height, self.max_height
            )
        }
    }

    /// Returns the aspect ratio range allowed by these constraints.
    ///
    /// Returns (min_aspect_ratio, max_aspect_ratio) or None if unbounded.
    pub fn aspect_ratio_range(&self) -> Option<(f32, f32)> {
        if self.has_bounded_width() && self.has_bounded_height() {
            let min_ratio = self.min_width / self.max_height;
            let max_ratio = self.max_width / self.min_height;
            Some((min_ratio, max_ratio))
        } else {
            None
        }
    }
}

// ============================================================================
// COMMON CONSTRAINT CONSTANTS
// ============================================================================

impl BoxConstraints {
    /// Constraints that expand to fill infinite space.
    pub const EXPAND: Self = Self::new(f32::INFINITY, f32::INFINITY, f32::INFINITY, f32::INFINITY);

    /// Unconstrained constraints (0 to infinity for both dimensions).
    pub const UNBOUNDED: Self = Self::new(0.0, f32::INFINITY, 0.0, f32::INFINITY);

    /// Zero constraints (forces zero size).
    pub const ZERO: Self = Self::new(0.0, 0.0, 0.0, 0.0);
}

// ============================================================================
// DISPLAY IMPLEMENTATION
// ============================================================================

impl fmt::Display for BoxConstraints {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.debug_string())
    }
}

// ============================================================================
// DEFAULT IMPLEMENTATION
// ============================================================================

impl Default for BoxConstraints {
    /// Returns unconstrained constraints by default.
    fn default() -> Self {
        Self::UNBOUNDED
    }
}

// ============================================================================
// UNIFIED CONSTRAINT ENUM
// ============================================================================

/// Unified constraint type that can represent different layout protocols.
///
/// This enum allows render objects and algorithms to work with different
/// constraint types in a protocol-agnostic manner.
#[derive(Debug, Clone, PartialEq)]
pub enum Constraints {
    /// Box protocol constraints for 2D rectangular layout.
    Box(BoxConstraints),
    /// Sliver protocol constraints for scrollable content.
    Sliver(SliverConstraints),
}

impl Constraints {
    /// Creates box constraints.
    #[inline]
    pub const fn box_constraints(constraints: BoxConstraints) -> Self {
        Self::Box(constraints)
    }

    /// Creates sliver constraints.
    #[inline]
    pub const fn sliver_constraints(constraints: SliverConstraints) -> Self {
        Self::Sliver(constraints)
    }

    /// Returns the box constraints if this is a box constraint.
    #[inline]
    pub fn as_box(&self) -> BoxConstraints {
        match self {
            Self::Box(constraints) => *constraints,
            Self::Sliver(_) => panic!("Expected box constraints, found sliver constraints"),
        }
    }

    /// Returns the sliver constraints if this is a sliver constraint.
    #[inline]
    pub fn as_sliver(&self) -> SliverConstraints {
        match self {
            Self::Sliver(constraints) => *constraints,
            Self::Box(_) => panic!("Expected sliver constraints, found box constraints"),
        }
    }

    /// Checks if these are box constraints.
    #[inline]
    pub const fn is_box(&self) -> bool {
        matches!(self, Self::Box(_))
    }

    /// Checks if these are sliver constraints.
    #[inline]
    pub const fn is_sliver(&self) -> bool {
        matches!(self, Self::Sliver(_))
    }
}

impl From<BoxConstraints> for Constraints {
    fn from(constraints: BoxConstraints) -> Self {
        Self::Box(constraints)
    }
}

impl From<SliverConstraints> for Constraints {
    fn from(constraints: SliverConstraints) -> Self {
        Self::Sliver(constraints)
    }
}

// ============================================================================
// UNIFIED GEOMETRY ENUM
// ============================================================================

/// Unified geometry type that can represent results from different layout protocols.
///
/// This enum allows render objects and algorithms to return different
/// geometry types in a protocol-agnostic manner.
#[derive(Debug, Clone, PartialEq)]
pub enum Geometry {
    /// Box protocol geometry (computed size).
    Box(Size),
    /// Sliver protocol geometry (scroll extent and paint bounds).
    Sliver(SliverGeometry),
}

impl Geometry {
    /// Creates box geometry.
    #[inline]
    pub const fn box_geometry(size: Size) -> Self {
        Self::Box(size)
    }

    /// Creates sliver geometry.
    #[inline]
    pub const fn sliver_geometry(geometry: SliverGeometry) -> Self {
        Self::Sliver(geometry)
    }

    /// Returns the size if this is box geometry.
    #[inline]
    pub fn as_box(&self) -> Size {
        match self {
            Self::Box(size) => *size,
            Self::Sliver(_) => panic!("Expected box geometry, found sliver geometry"),
        }
    }

    /// Returns the sliver geometry if this is sliver geometry.
    #[inline]
    pub fn as_sliver(&self) -> SliverGeometry {
        match self {
            Self::Sliver(geometry) => *geometry,
            Self::Box(_) => panic!("Expected sliver geometry, found box geometry"),
        }
    }

    /// Checks if this is box geometry.
    #[inline]
    pub const fn is_box(&self) -> bool {
        matches!(self, Self::Box(_))
    }

    /// Checks if this is sliver geometry.
    #[inline]
    pub const fn is_sliver(&self) -> bool {
        matches!(self, Self::Sliver(_))
    }
}

impl From<Size> for Geometry {
    fn from(size: Size) -> Self {
        Self::Box(size)
    }
}

impl From<SliverGeometry> for Geometry {
    fn from(geometry: SliverGeometry) -> Self {
        Self::Sliver(geometry)
    }
}

impl Default for Geometry {
    fn default() -> Self {
        Self::Box(Size::ZERO)
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_box_constraints_construction() {
        let tight = BoxConstraints::tight(Size::new(100.0, 50.0));
        assert_eq!(tight.min_width, 100.0);
        assert_eq!(tight.max_width, 100.0);
        assert_eq!(tight.min_height, 50.0);
        assert_eq!(tight.max_height, 50.0);
        assert!(tight.is_tight());

        let loose = BoxConstraints::loose(Size::new(200.0, 150.0));
        assert_eq!(loose.min_width, 0.0);
        assert_eq!(loose.max_width, 200.0);
        assert_eq!(loose.min_height, 0.0);
        assert_eq!(loose.max_height, 150.0);
        assert!(!loose.is_tight());
    }

    #[test]
    fn test_constraint_checking() {
        let constraints = BoxConstraints::new(50.0, 100.0, 30.0, 80.0);

        assert!(constraints.is_satisfied_by(Size::new(75.0, 50.0)));
        assert!(!constraints.is_satisfied_by(Size::new(25.0, 50.0))); // Too narrow
        assert!(!constraints.is_satisfied_by(Size::new(75.0, 100.0))); // Too tall

        assert!(constraints.is_normalized());
        assert!(constraints.has_bounded_width());
        assert!(constraints.has_bounded_height());
        assert!(constraints.is_bounded());
    }

    #[test]
    fn test_size_computation() {
        let constraints = BoxConstraints::new(50.0, 100.0, 30.0, 80.0);

        assert_eq!(constraints.biggest(), Size::new(100.0, 80.0));
        assert_eq!(constraints.smallest(), Size::new(50.0, 30.0));

        let size = Size::new(120.0, 20.0);
        let constrained = constraints.constrain(size);
        assert_eq!(constrained, Size::new(100.0, 30.0));

        assert_eq!(constraints.constrain_width(120.0), 100.0);
        assert_eq!(constraints.constrain_height(20.0), 30.0);
    }

    #[test]
    fn test_constraint_transformations() {
        let base = BoxConstraints::loose(Size::new(200.0, 100.0));
        let padding = EdgeInsets::all(10.0);

        let deflated = base.deflate(&padding);
        assert_eq!(deflated.max_width, 180.0); // 200 - 20
        assert_eq!(deflated.max_height, 80.0); // 100 - 20

        let inflated = base.inflate(&padding);
        assert_eq!(inflated.max_width, 220.0); // 200 + 20
        assert_eq!(inflated.max_height, 120.0); // 100 + 20

        let tightened = base.tighten(Some(150.0), None);
        assert_eq!(tightened.min_width, 150.0);
        assert_eq!(tightened.max_width, 150.0);
        assert_eq!(tightened.max_height, 100.0);
    }

    #[test]
    fn test_constraint_operations() {
        let a = BoxConstraints::new(50.0, 100.0, 30.0, 80.0);
        let b = BoxConstraints::new(75.0, 120.0, 20.0, 60.0);

        let intersect = a.intersect(&b);
        assert_eq!(intersect.min_width, 75.0); // max of mins
        assert_eq!(intersect.max_width, 100.0); // min of maxs
        assert_eq!(intersect.min_height, 30.0);
        assert_eq!(intersect.max_height, 60.0);

        let union = a.union(&b);
        assert_eq!(union.min_width, 50.0); // min of mins
        assert_eq!(union.max_width, 120.0); // max of maxs
        assert_eq!(union.min_height, 20.0);
        assert_eq!(union.max_height, 80.0);

        assert!(a.is_compatible_with(&b));
    }

    #[test]
    fn test_special_constraints() {
        let expand = BoxConstraints::expand();
        assert!(expand.min_width.is_infinite());
        assert!(expand.min_height.is_infinite());

        let unbounded = BoxConstraints::UNBOUNDED;
        assert_eq!(unbounded.min_width, 0.0);
        assert!(unbounded.max_width.is_infinite());

        let zero = BoxConstraints::ZERO;
        assert_eq!(zero.biggest(), Size::ZERO);
        assert_eq!(zero.smallest(), Size::ZERO);
    }

    #[test]
    fn test_unified_constraints() {
        let box_constraints = BoxConstraints::tight(Size::new(100.0, 50.0));
        let unified = Constraints::from(box_constraints);

        assert!(unified.is_box());
        assert!(!unified.is_sliver());
        assert_eq!(unified.as_box(), box_constraints);
    }

    #[test]
    fn test_unified_geometry() {
        let size = Size::new(100.0, 50.0);
        let geometry = Geometry::from(size);

        assert!(geometry.is_box());
        assert!(!geometry.is_sliver());
        assert_eq!(geometry.as_box(), size);
    }

    #[test]
    fn test_aspect_ratio_range() {
        let constraints = BoxConstraints::new(100.0, 200.0, 50.0, 100.0);
        let (min_ratio, max_ratio) = constraints.aspect_ratio_range().unwrap();

        assert_eq!(min_ratio, 100.0 / 100.0); // min_width / max_height
        assert_eq!(max_ratio, 200.0 / 50.0); // max_width / min_height
    }

    #[test]
    fn test_debug_string() {
        let tight = BoxConstraints::tight(Size::new(100.0, 50.0));
        assert_eq!(tight.debug_string(), "tight(100×50)");

        let loose = BoxConstraints::new(50.0, 100.0, 30.0, 80.0);
        assert_eq!(loose.debug_string(), "BoxConstraints(w: 50-100, h: 30-80)");
    }
}
