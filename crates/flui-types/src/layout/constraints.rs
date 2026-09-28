//! Box constraints for layout calculations
//!
//! This module provides BoxConstraints, Flutter-style layout constraints
//! that define the min/max width and height for a box.

use std::fmt;

use crate::geometry::Size;

/// Box constraints that define min/max width and height
///
/// Similar to Flutter's BoxConstraints. Used throughout the layout system
/// to propagate size constraints from parent to child.
///
/// # Examples
///
/// ```
/// use flui_types::{
///     geometry::Size,
///     layout::BoxConstraints,
/// };
///
/// // Tight constraints (exact size)
/// let tight = BoxConstraints::tight(Size::new(100.0, 200.0));
/// assert!(tight.is_tight());
///
/// // Loose constraints (max size, can be smaller)
/// let loose = BoxConstraints::loose(Size::new(300.0, 400.0));
/// assert!(!loose.is_tight());
///
/// // Constrain a size
/// let size = Size::new(150.0, 250.0);
/// let constrained = loose.constrain(size);
/// assert_eq!(constrained.width, 150.0); // Within bounds
/// ```
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct BoxConstraints {
    /// Minimum width
    pub min_width: f64,
    /// Maximum width
    pub max_width: f64,
    /// Minimum height
    pub min_height: f64,
    /// Maximum height
    pub max_height: f64,
}

impl BoxConstraints {
    // ===== Constructors =====

    /// Creates new constraints with specified bounds
    #[inline]
    pub const fn new(min_width: f64, max_width: f64, min_height: f64, max_height: f64) -> Self {
        Self {
            min_width,
            max_width,
            min_height,
            max_height,
        }
    }

    /// Creates tight constraints (exact size)
    ///
    /// Both min and max are set to the same value, forcing the exact size.
    #[inline]
    pub const fn tight(size: Size<f64>) -> Self {
        Self::new(size.width, size.width, size.height, size.height)
    }

    /// Creates tight constraints for width only
    #[inline]
    pub const fn tight_width(width: f64) -> Self {
        Self::new(width, width, 0.0, f64::MAX)
    }

    /// Creates tight constraints for height only
    #[inline]
    pub const fn tight_height(height: f64) -> Self {
        Self::new(0.0, f64::MAX, height, height)
    }

    /// Creates loose constraints (max size, min is zero)
    ///
    /// The box can be any size from zero to the specified maximum.
    #[inline]
    pub const fn loose(size: Size<f64>) -> Self {
        Self::new(0.0, size.width, 0.0, size.height)
    }

    /// Creates unbounded constraints (no limits)
    #[inline]
    pub const fn unbounded() -> Self {
        Self::new(0.0, f64::MAX, 0.0, f64::MAX)
    }

    /// Creates constraints that expand to fill available space
    #[inline]
    pub const fn expand() -> Self {
        Self::new(f64::MAX, f64::MAX, f64::MAX, f64::MAX)
    }

    // ===== Queries =====

    /// Returns true if these constraints force an exact size
    #[inline]
    pub const fn is_tight(&self) -> bool {
        self.min_width >= self.max_width && self.min_height >= self.max_height
    }

    /// Returns true if the width is bounded (has finite max)
    #[inline]
    pub const fn has_bounded_width(&self) -> bool {
        self.max_width < f64::MAX
    }

    /// Returns true if the height is bounded (has finite max)
    #[inline]
    pub const fn has_bounded_height(&self) -> bool {
        self.max_height < f64::MAX
    }

    /// Returns true if both dimensions are bounded
    #[inline]
    pub const fn is_bounded(&self) -> bool {
        self.has_bounded_width() && self.has_bounded_height()
    }

    /// Returns true if constraints have zero size
    #[inline]
    pub const fn is_zero(&self) -> bool {
        self.max_width <= 0.0 && self.max_height <= 0.0
    }

    /// Returns true if the width must be a specific value
    #[inline]
    pub const fn has_tight_width(&self) -> bool {
        self.min_width >= self.max_width
    }

    /// Returns true if the height must be a specific value
    #[inline]
    pub const fn has_tight_height(&self) -> bool {
        self.min_height >= self.max_height
    }

    /// Returns the biggest size that satisfies these constraints
    #[inline]
    pub const fn biggest(&self) -> Size<f64> {
        Size::new(self.max_width, self.max_height)
    }

    /// Returns the smallest size that satisfies these constraints
    #[inline]
    pub const fn smallest(&self) -> Size<f64> {
        Size::new(self.min_width, self.min_height)
    }

    // ===== Operations =====

    /// Constrain a size to fit within these constraints
    #[inline]
    pub fn constrain(&self, size: Size<f64>) -> Size<f64> {
        Size::new(
            self.constrain_width(size.width),
            self.constrain_height(size.height),
        )
    }

    /// Constrain width only
    #[inline]
    pub fn constrain_width(&self, width: f64) -> f64 {
        width.clamp(self.min_width, self.max_width)
    }

    /// Constrain height only
    #[inline]
    pub fn constrain_height(&self, height: f64) -> f64 {
        height.clamp(self.min_height, self.max_height)
    }

    /// Creates constraints with the width replaced
    #[inline]
    pub const fn with_width(&self, min: f64, max: f64) -> Self {
        Self::new(min, max, self.min_height, self.max_height)
    }

    /// Creates constraints with the height replaced
    #[inline]
    pub const fn with_height(&self, min: f64, max: f64) -> Self {
        Self::new(self.min_width, self.max_width, min, max)
    }

    /// Creates constraints with max width tightened
    #[inline]
    pub fn tighten_width(&self, width: Option<f64>) -> Self {
        let width = width.unwrap_or(self.max_width);
        Self::new(
            self.min_width.min(width),
            width,
            self.min_height,
            self.max_height,
        )
    }

    /// Creates constraints with max height tightened
    #[inline]
    pub fn tighten_height(&self, height: Option<f64>) -> Self {
        let height = height.unwrap_or(self.max_height);
        Self::new(
            self.min_width,
            self.max_width,
            self.min_height.min(height),
            height,
        )
    }

    /// Creates constraints with both dimensions tightened
    #[inline]
    pub fn tighten(&self, size: Option<Size<f64>>) -> Self {
        if let Some(size) = size {
            Self::tight(size)
        } else {
            *self
        }
    }

    /// Loosens the constraints by removing minimums
    #[inline]
    pub const fn loosen(&self) -> Self {
        Self::new(0.0, self.max_width, 0.0, self.max_height)
    }

    /// Enforces the constraints (clamps to valid range)
    ///
    /// Ensures min <= max for both dimensions
    #[inline]
    pub fn enforce(&self) -> Self {
        Self::new(
            self.min_width,
            self.max_width.max(self.min_width),
            self.min_height,
            self.max_height.max(self.min_height),
        )
    }

    /// Deflates constraints by Edges (shrinks available space)
    #[inline]
    pub fn deflate(&self, insets: crate::geometry::Edges<f64>) -> Self {
        let horizontal = insets.horizontal_total();
        let vertical = insets.vertical_total();

        Self::new(
            (self.min_width - horizontal).max(0.0),
            (self.max_width - horizontal).max(0.0),
            (self.min_height - vertical).max(0.0),
            (self.max_height - vertical).max(0.0),
        )
    }

    /// Normalizes to valid constraints
    ///
    /// Ensures: 0 <= min <= max <= infinity
    #[inline]
    pub fn normalize(&self) -> Self {
        // The maximum is raised to the *normalized* minimum, as in Flutter;
        // raising it to a negative original minimum could leave max < min.
        let min_width = self.min_width.max(0.0);
        let min_height = self.min_height.max(0.0);
        Self::new(
            min_width,
            self.max_width.max(min_width),
            min_height,
            self.max_height.max(min_height),
        )
    }
}

impl Default for BoxConstraints {
    #[inline]
    fn default() -> Self {
        Self::unbounded()
    }
}

impl fmt::Display for BoxConstraints {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_tight() {
            write!(f, "BoxConstraints({}x{})", self.min_width, self.min_height)
        } else {
            write!(
                f,
                "BoxConstraints({} <= w <= {}, {} <= h <= {})",
                self.min_width, self.max_width, self.min_height, self.max_height
            )
        }
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[inline]
    fn test_tight_constraints() {
        let size = Size::new(100.0, 200.0);
        let constraints = BoxConstraints::tight(size);

        assert!(constraints.is_tight());
        assert_eq!(constraints.smallest(), size);
        assert_eq!(constraints.biggest(), size);
    }

    #[test]
    #[inline]
    fn test_loose_constraints() {
        let size = Size::new(100.0, 200.0);
        let constraints = BoxConstraints::loose(size);

        assert!(!constraints.is_tight());
        assert_eq!(constraints.min_width, 0.0);
        assert_eq!(constraints.max_width, 100.0);
    }

    #[test]
    #[inline]
    fn test_constrain() {
        let constraints = BoxConstraints::new(50.0, 150.0, 100.0, 300.0);

        // Within bounds
        let size1 = Size::new(100.0, 200.0);
        assert_eq!(constraints.constrain(size1), size1);

        // Too small
        let size2 = Size::new(10.0, 50.0);
        assert_eq!(constraints.constrain(size2), Size::new(50.0, 100.0));

        // Too large
        let size3 = Size::new(200.0, 400.0);
        assert_eq!(constraints.constrain(size3), Size::new(150.0, 300.0));
    }

    #[test]
    #[inline]
    fn test_bounded() {
        let bounded = BoxConstraints::loose(Size::new(100.0, 200.0));
        assert!(bounded.is_bounded());

        let unbounded = BoxConstraints::unbounded();
        assert!(!unbounded.is_bounded());
    }

    #[test]
    #[inline]
    fn test_loosen() {
        let tight = BoxConstraints::tight(Size::new(100.0, 200.0));
        let loose = tight.loosen();

        assert_eq!(loose.min_width, 0.0);
        assert_eq!(loose.min_height, 0.0);
        assert_eq!(loose.max_width, 100.0);
        assert_eq!(loose.max_height, 200.0);
    }

    fn c(min_w: f64, max_w: f64, min_h: f64, max_h: f64) -> BoxConstraints {
        BoxConstraints::new(min_w, max_w, min_h, max_h)
    }

    /// `normalize` lifts negative minimums to zero and raises each maximum
    /// to at least its minimum; `enforce` only does the latter.
    #[test]
    fn normalize_and_enforce() {
        assert_eq!(
            c(100.0, 50.0, -5.0, -10.0).normalize(),
            c(100.0, 100.0, 0.0, 0.0)
        );
        assert_eq!(
            c(100.0, 50.0, -5.0, -10.0).enforce(),
            c(100.0, 100.0, -5.0, -5.0)
        );
        assert_eq!(c(1.0, 2.0, 3.0, 4.0).normalize(), c(1.0, 2.0, 3.0, 4.0));
    }

    /// Each predicate on one axis at a time, so an `&&` that became `||`
    /// (or the reverse) shows. `f64::MAX` itself counts as unbounded.
    #[test]
    fn per_axis_predicates() {
        let tight_w = BoxConstraints::tight_width(10.0);
        assert!(tight_w.has_tight_width() && !tight_w.has_tight_height() && !tight_w.is_tight());
        let tight_h = BoxConstraints::tight_height(10.0);
        assert!(!tight_h.has_tight_width() && tight_h.has_tight_height() && !tight_h.is_tight());

        let bounded_w = c(0.0, 10.0, 0.0, f64::MAX);
        assert!(
            bounded_w.has_bounded_width()
                && !bounded_w.has_bounded_height()
                && !bounded_w.is_bounded()
        );
        let bounded_h = c(0.0, f64::MAX, 0.0, 10.0);
        assert!(
            !bounded_h.has_bounded_width()
                && bounded_h.has_bounded_height()
                && !bounded_h.is_bounded()
        );

        assert!(c(0.0, 0.0, 0.0, 0.0).is_zero());
        assert!(!c(0.0, 0.0, 0.0, 1.0).is_zero());
        assert!(!c(0.0, 1.0, 0.0, 0.0).is_zero());
    }

    #[test]
    fn tighten() {
        let loose = c(20.0, 100.0, 30.0, 200.0);
        assert_eq!(loose.tighten_width(Some(10.0)), c(10.0, 10.0, 30.0, 200.0));
        assert_eq!(loose.tighten_width(None), loose);
        assert_eq!(loose.tighten_height(Some(50.0)), c(20.0, 100.0, 30.0, 50.0));
        assert_eq!(loose.tighten_height(None), loose);
        assert_eq!(
            loose.tighten(Some(Size::new(7.0, 8.0))),
            c(7.0, 7.0, 8.0, 8.0)
        );
        assert_eq!(loose.tighten(None), loose);
    }

    #[test]
    fn display() {
        assert_eq!(c(7.0, 7.0, 8.0, 8.0).to_string(), "BoxConstraints(7x8)");
        assert_eq!(
            c(1.0, 2.0, 3.0, 4.0).to_string(),
            "BoxConstraints(1 <= w <= 2, 3 <= h <= 4)"
        );
    }

    /// Each bound shrinks by its axis's total inset, floored at zero.
    #[test]
    fn deflate_subtracts_the_insets_per_axis() {
        let insets = crate::geometry::Edges::new(1.0, 2.0, 4.0, 8.0);
        let deflated = c(20.0, 100.0, 30.0, 200.0).deflate(insets);
        assert_eq!(deflated, c(10.0, 90.0, 25.0, 195.0));
        assert_eq!(
            c(5.0, 12.0, 3.0, 7.0).deflate(insets),
            c(0.0, 2.0, 0.0, 2.0)
        );
    }
}
