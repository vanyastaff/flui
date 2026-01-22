//! Corner enumeration for rectangle corners.
//!
//! This module provides the [`Corner`] enum for referring to specific corners
//! of a rectangle. Used throughout FLUI for corner-based operations like
//! positioning, alignment, and corner-specific styling.

use super::Axis;

/// The four corners of a rectangle.
///
/// Used for corner-based positioning, alignment, and operations.
/// Follows CSS conventions with clockwise ordering starting from top-left.
///
/// # Examples
///
/// ```
/// use flui_types::geometry::{Corner, Axis};
///
/// let corner = Corner::TopLeft;
/// assert_eq!(corner.opposite(), Corner::BottomRight);
/// assert_eq!(corner.other_side_along(Axis::Horizontal), Corner::TopRight);
/// ```
#[derive(Copy, Clone, PartialEq, Eq, Debug, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Corner {
    /// Top-left corner (0, 0 in standard coordinate system).
    TopLeft,
    /// Top-right corner.
    TopRight,
    /// Bottom-left corner.
    BottomLeft,
    /// Bottom-right corner.
    BottomRight,
}

impl Corner {
    /// Returns the diagonally opposite corner.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::Corner;
    ///
    /// assert_eq!(Corner::TopLeft.opposite(), Corner::BottomRight);
    /// assert_eq!(Corner::TopRight.opposite(), Corner::BottomLeft);
    /// assert_eq!(Corner::BottomLeft.opposite(), Corner::TopRight);
    /// assert_eq!(Corner::BottomRight.opposite(), Corner::TopLeft);
    /// ```
    #[inline]
    #[must_use]
    pub const fn opposite(self) -> Self {
        match self {
            Corner::TopLeft => Corner::BottomRight,
            Corner::TopRight => Corner::BottomLeft,
            Corner::BottomLeft => Corner::TopRight,
            Corner::BottomRight => Corner::TopLeft,
        }
    }

    /// Returns the corner on the other side along the given axis.
    ///
    /// - Along `Horizontal` axis: moves left ↔ right
    /// - Along `Vertical` axis: moves top ↔ bottom
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::{Corner, Axis};
    ///
    /// // Horizontal movement
    /// assert_eq!(
    ///     Corner::TopLeft.other_side_along(Axis::Horizontal),
    ///     Corner::TopRight
    /// );
    /// assert_eq!(
    ///     Corner::BottomLeft.other_side_along(Axis::Horizontal),
    ///     Corner::BottomRight
    /// );
    ///
    /// // Vertical movement
    /// assert_eq!(
    ///     Corner::TopLeft.other_side_along(Axis::Vertical),
    ///     Corner::BottomLeft
    /// );
    /// assert_eq!(
    ///     Corner::TopRight.other_side_along(Axis::Vertical),
    ///     Corner::BottomRight
    /// );
    /// ```
    #[inline]
    #[must_use]
    pub const fn other_side_along(self, axis: Axis) -> Self {
        match axis {
            Axis::Horizontal => match self {
                Corner::TopLeft => Corner::TopRight,
                Corner::TopRight => Corner::TopLeft,
                Corner::BottomLeft => Corner::BottomRight,
                Corner::BottomRight => Corner::BottomLeft,
            },
            Axis::Vertical => match self {
                Corner::TopLeft => Corner::BottomLeft,
                Corner::TopRight => Corner::BottomRight,
                Corner::BottomLeft => Corner::TopLeft,
                Corner::BottomRight => Corner::TopRight,
            },
        }
    }

    /// Returns true if this is a top corner (TopLeft or TopRight).
    #[inline]
    #[must_use]
    pub const fn is_top(self) -> bool {
        matches!(self, Corner::TopLeft | Corner::TopRight)
    }

    /// Returns true if this is a bottom corner (BottomLeft or BottomRight).
    #[inline]
    #[must_use]
    pub const fn is_bottom(self) -> bool {
        matches!(self, Corner::BottomLeft | Corner::BottomRight)
    }

    /// Returns true if this is a left corner (TopLeft or BottomLeft).
    #[inline]
    #[must_use]
    pub const fn is_left(self) -> bool {
        matches!(self, Corner::TopLeft | Corner::BottomLeft)
    }

    /// Returns true if this is a right corner (TopRight or BottomRight).
    #[inline]
    #[must_use]
    pub const fn is_right(self) -> bool {
        matches!(self, Corner::TopRight | Corner::BottomRight)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_opposite() {
        assert_eq!(Corner::TopLeft.opposite(), Corner::BottomRight);
        assert_eq!(Corner::TopRight.opposite(), Corner::BottomLeft);
        assert_eq!(Corner::BottomLeft.opposite(), Corner::TopRight);
        assert_eq!(Corner::BottomRight.opposite(), Corner::TopLeft);

        // Test symmetry
        assert_eq!(Corner::TopLeft.opposite().opposite(), Corner::TopLeft);
    }

    #[test]
    fn test_other_side_along() {
        // Horizontal axis
        assert_eq!(
            Corner::TopLeft.other_side_along(Axis::Horizontal),
            Corner::TopRight
        );
        assert_eq!(
            Corner::TopRight.other_side_along(Axis::Horizontal),
            Corner::TopLeft
        );
        assert_eq!(
            Corner::BottomLeft.other_side_along(Axis::Horizontal),
            Corner::BottomRight
        );
        assert_eq!(
            Corner::BottomRight.other_side_along(Axis::Horizontal),
            Corner::BottomLeft
        );

        // Vertical axis
        assert_eq!(
            Corner::TopLeft.other_side_along(Axis::Vertical),
            Corner::BottomLeft
        );
        assert_eq!(
            Corner::TopRight.other_side_along(Axis::Vertical),
            Corner::BottomRight
        );
        assert_eq!(
            Corner::BottomLeft.other_side_along(Axis::Vertical),
            Corner::TopLeft
        );
        assert_eq!(
            Corner::BottomRight.other_side_along(Axis::Vertical),
            Corner::TopRight
        );
    }

    #[test]
    fn test_is_predicates() {
        assert!(Corner::TopLeft.is_top());
        assert!(Corner::TopRight.is_top());
        assert!(!Corner::BottomLeft.is_top());
        assert!(!Corner::BottomRight.is_top());

        assert!(Corner::BottomLeft.is_bottom());
        assert!(Corner::BottomRight.is_bottom());
        assert!(!Corner::TopLeft.is_bottom());
        assert!(!Corner::TopRight.is_bottom());

        assert!(Corner::TopLeft.is_left());
        assert!(Corner::BottomLeft.is_left());
        assert!(!Corner::TopRight.is_left());
        assert!(!Corner::BottomRight.is_left());

        assert!(Corner::TopRight.is_right());
        assert!(Corner::BottomRight.is_right());
        assert!(!Corner::TopLeft.is_right());
        assert!(!Corner::BottomLeft.is_right());
    }
}
