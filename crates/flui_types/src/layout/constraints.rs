//! Box constraints for layout calculations
//!
//! This module provides BoxConstraints, Flutter-style layout constraints
//! that define the min/max width and height for a box.

use crate::geometry::{Pixels, Size};
use std::fmt;

/// Box constraints that define min/max width and height
///
/// Similar to Flutter's BoxConstraints. Used throughout the layout system
/// to propagate size constraints from parent to child.
///
/// # Examples
///
/// ```
/// use flui_types::layout::BoxConstraints;
/// use flui_types::geometry::{px, Size};
///
/// // Tight constraints (exact size)
/// let tight = BoxConstraints::tight(Size::new(px(100.0), px(200.0)));
/// assert!(tight.is_tight());
///
/// // Loose constraints (max size, can be smaller)
/// let loose = BoxConstraints::loose(Size::new(px(300.0), px(400.0)));
/// assert!(!loose.is_tight());
///
/// // Constrain a size
/// let size = Size::new(px(150.0), px(250.0));
/// let constrained = loose.constrain(size);
/// assert_eq!(constrained.width, px(150.0)); // Within bounds
/// ```
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct BoxConstraints {
    /// Minimum width
    pub min_width: Pixels,
    /// Maximum width
    pub max_width: Pixels,
    /// Minimum height
    pub min_height: Pixels,
    /// Maximum height
    pub max_height: Pixels,
}

impl BoxConstraints {
    // ===== Constructors =====

    /// Creates new constraints with specified bounds
    #[inline]
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

    /// Creates tight constraints (exact size)
    ///
    /// Both min and max are set to the same value, forcing the exact size.
    #[inline]
    pub const fn tight(size: Size<Pixels>) -> Self {
        Self::new(size.width, size.width, size.height, size.height)
    }

    /// Creates tight constraints for width only
    #[inline]
    pub const fn tight_width(width: Pixels) -> Self {
        Self::new(width, width, Pixels::ZERO, Pixels::MAX)
    }

    /// Creates tight constraints for height only
    #[inline]
    pub const fn tight_height(height: Pixels) -> Self {
        Self::new(Pixels::ZERO, Pixels::MAX, height, height)
    }

    /// Creates loose constraints (max size, min is zero)
    ///
    /// The box can be any size from zero to the specified maximum.
    #[inline]
    pub const fn loose(size: Size<Pixels>) -> Self {
        Self::new(Pixels::ZERO, size.width, Pixels::ZERO, size.height)
    }

    /// Creates unbounded constraints (no limits)
    #[inline]
    pub const fn unbounded() -> Self {
        Self::new(Pixels::ZERO, Pixels::MAX, Pixels::ZERO, Pixels::MAX)
    }

    /// Creates constraints that expand to fill available space
    #[inline]
    pub const fn expand() -> Self {
        Self::new(Pixels::MAX, Pixels::MAX, Pixels::MAX, Pixels::MAX)
    }

    // ===== Queries =====

    /// Returns true if these constraints force an exact size
    #[inline]
    pub const fn is_tight(&self) -> bool {
        self.min_width.0 >= self.max_width.0 && self.min_height.0 >= self.max_height.0
    }

    /// Returns true if the width is bounded (has finite max)
    #[inline]
    pub const fn has_bounded_width(&self) -> bool {
        self.max_width.0 < Pixels::MAX.0
    }

    /// Returns true if the height is bounded (has finite max)
    #[inline]
    pub const fn has_bounded_height(&self) -> bool {
        self.max_height.0 < Pixels::MAX.0
    }

    /// Returns true if both dimensions are bounded
    #[inline]
    pub const fn is_bounded(&self) -> bool {
        self.has_bounded_width() && self.has_bounded_height()
    }

    /// Returns true if constraints have zero size
    #[inline]
    pub const fn is_zero(&self) -> bool {
        self.max_width.0 <= 0.0 && self.max_height.0 <= 0.0
    }

    /// Returns true if the width must be a specific value
    #[inline]
    pub const fn has_tight_width(&self) -> bool {
        self.min_width.0 >= self.max_width.0
    }

    /// Returns true if the height must be a specific value
    #[inline]
    pub const fn has_tight_height(&self) -> bool {
        self.min_height.0 >= self.max_height.0
    }

    /// Returns the biggest size that satisfies these constraints
    #[inline]
    pub const fn biggest(&self) -> Size<Pixels> {
        Size::new(self.max_width, self.max_height)
    }

    /// Returns the smallest size that satisfies these constraints
    #[inline]
    pub const fn smallest(&self) -> Size<Pixels> {
        Size::new(self.min_width, self.min_height)
    }

    // ===== Operations =====

    /// Constrain a size to fit within these constraints
    #[inline]
    pub fn constrain(&self, size: Size<Pixels>) -> Size<Pixels> {
        Size::new(
            self.constrain_width(size.width),
            self.constrain_height(size.height),
        )
    }

    /// Constrain width only
    #[inline]
    pub fn constrain_width(&self, width: Pixels) -> Pixels {
        Pixels(width.0.clamp(self.min_width.0, self.max_width.0))
    }

    /// Constrain height only
    #[inline]
    pub fn constrain_height(&self, height: Pixels) -> Pixels {
        Pixels(height.0.clamp(self.min_height.0, self.max_height.0))
    }

    /// Creates constraints with the width replaced
    #[inline]
    pub const fn with_width(&self, min: Pixels, max: Pixels) -> Self {
        Self::new(min, max, self.min_height, self.max_height)
    }

    /// Creates constraints with the height replaced
    #[inline]
    pub const fn with_height(&self, min: Pixels, max: Pixels) -> Self {
        Self::new(self.min_width, self.max_width, min, max)
    }

    /// Creates constraints with max width tightened
    #[inline]
    pub fn tighten_width(&self, width: Option<Pixels>) -> Self {
        let width = width.unwrap_or(self.max_width);
        Self::new(
            Pixels(self.min_width.0.min(width.0)),
            width,
            self.min_height,
            self.max_height,
        )
    }

    /// Creates constraints with max height tightened
    #[inline]
    pub fn tighten_height(&self, height: Option<Pixels>) -> Self {
        let height = height.unwrap_or(self.max_height);
        Self::new(
            self.min_width,
            self.max_width,
            Pixels(self.min_height.0.min(height.0)),
            height,
        )
    }

    /// Creates constraints with both dimensions tightened
    #[inline]
    pub fn tighten(&self, size: Option<Size<Pixels>>) -> Self {
        if let Some(size) = size {
            Self::tight(size)
        } else {
            *self
        }
    }

    /// Loosens the constraints by removing minimums
    #[inline]
    pub const fn loosen(&self) -> Self {
        Self::new(Pixels::ZERO, self.max_width, Pixels::ZERO, self.max_height)
    }

    /// Enforces the constraints (clamps to valid range)
    ///
    /// Ensures min <= max for both dimensions
    #[inline]
    pub fn enforce(&self) -> Self {
        Self::new(
            self.min_width,
            Pixels(self.max_width.0.max(self.min_width.0)),
            self.min_height,
            Pixels(self.max_height.0.max(self.min_height.0)),
        )
    }

    /// Deflates constraints by Edges (shrinks available space)
    #[inline]
    pub fn deflate(&self, insets: crate::geometry::Edges<Pixels>) -> Self {
        let horizontal = insets.horizontal_total();
        let vertical = insets.vertical_total();

        Self::new(
            Pixels((self.min_width.0 - horizontal.0).max(0.0)),
            Pixels((self.max_width.0 - horizontal.0).max(0.0)),
            Pixels((self.min_height.0 - vertical.0).max(0.0)),
            Pixels((self.max_height.0 - vertical.0).max(0.0)),
        )
    }

    /// Normalizes to valid constraints
    ///
    /// Ensures: 0 <= min <= max <= infinity
    #[inline]
    pub fn normalize(&self) -> Self {
        Self::new(
            Pixels(self.min_width.0.max(0.0)),
            Pixels(self.max_width.0.max(self.min_width.0)),
            Pixels(self.min_height.0.max(0.0)),
            Pixels(self.max_height.0.max(self.min_height.0)),
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
            write!(
                f,
                "BoxConstraints({}x{})",
                self.min_width.0, self.min_height.0
            )
        } else {
            write!(
                f,
                "BoxConstraints({} <= w <= {}, {} <= h <= {})",
                self.min_width.0, self.max_width.0, self.min_height.0, self.max_height.0
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
    use crate::geometry::px;

    #[test]
    #[inline]
    fn test_tight_constraints() {
        let size = Size::new(px(100.0), px(200.0));
        let constraints = BoxConstraints::tight(size);

        assert!(constraints.is_tight());
        assert_eq!(constraints.smallest(), size);
        assert_eq!(constraints.biggest(), size);
    }

    #[test]
    #[inline]
    fn test_loose_constraints() {
        let size = Size::new(px(100.0), px(200.0));
        let constraints = BoxConstraints::loose(size);

        assert!(!constraints.is_tight());
        assert_eq!(constraints.min_width, px(0.0));
        assert_eq!(constraints.max_width, px(100.0));
    }

    #[test]
    #[inline]
    fn test_constrain() {
        let constraints = BoxConstraints::new(px(50.0), px(150.0), px(100.0), px(300.0));

        // Within bounds
        let size1 = Size::new(px(100.0), px(200.0));
        assert_eq!(constraints.constrain(size1), size1);

        // Too small
        let size2 = Size::new(px(10.0), px(50.0));
        assert_eq!(constraints.constrain(size2), Size::new(px(50.0), px(100.0)));

        // Too large
        let size3 = Size::new(px(200.0), px(400.0));
        assert_eq!(
            constraints.constrain(size3),
            Size::new(px(150.0), px(300.0))
        );
    }

    #[test]
    #[inline]
    fn test_bounded() {
        let bounded = BoxConstraints::loose(Size::new(px(100.0), px(200.0)));
        assert!(bounded.is_bounded());

        let unbounded = BoxConstraints::unbounded();
        assert!(!unbounded.is_bounded());
    }

    #[test]
    #[inline]
    fn test_loosen() {
        let tight = BoxConstraints::tight(Size::new(px(100.0), px(200.0)));
        let loose = tight.loosen();

        assert_eq!(loose.min_width, px(0.0));
        assert_eq!(loose.min_height, px(0.0));
        assert_eq!(loose.max_width, px(100.0));
        assert_eq!(loose.max_height, px(200.0));
    }

    #[test]
    #[inline]
    fn test_normalize() {
        // Invalid constraints (min > max)
        let invalid = BoxConstraints::new(
            px(100.0),
            px(50.0), // min > max
            px(200.0),
            px(100.0), // min > max
        );

        let normalized = invalid.normalize();
        assert!(normalized.min_width.0 <= normalized.max_width.0);
        assert!(normalized.min_height.0 <= normalized.max_height.0);
    }
}
