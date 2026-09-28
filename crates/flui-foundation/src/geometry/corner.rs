//! Corner enumeration for rectangle corners.
//!
//! This module provides the [`Corner`] enum for referring to specific corners
//! of a rectangle. Used throughout FLUI for corner-based operations like
//! positioning, alignment, and corner-specific styling.

use super::Axis;

/// Enumeration of rectangle corners.
///
/// Used for corner-based operations like positioning, alignment, and styling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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

    /// Returns `true` if this is a top corner.
    #[must_use]
    pub const fn is_top(self) -> bool {
        matches!(self, Corner::TopLeft | Corner::TopRight)
    }

    /// Returns `true` if this is a bottom corner.
    #[must_use]
    pub const fn is_bottom(self) -> bool {
        matches!(self, Corner::BottomLeft | Corner::BottomRight)
    }

    /// Returns `true` if this is a left corner.
    #[must_use]
    pub const fn is_left(self) -> bool {
        matches!(self, Corner::TopLeft | Corner::BottomLeft)
    }

    /// Returns `true` if this is a right corner.
    #[must_use]
    pub const fn is_right(self) -> bool {
        matches!(self, Corner::TopRight | Corner::BottomRight)
    }
}
