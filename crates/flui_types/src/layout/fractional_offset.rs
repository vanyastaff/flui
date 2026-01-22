//! FractionalOffset - alignment using 0.0-1.0 coordinates
//!
//! Similar to Flutter's `FractionalOffset`. Unlike `Alignment` which uses
//! -1.0 to 1.0 coordinates, `FractionalOffset` uses 0.0 to 1.0 where
//! (0.0, 0.0) is the top-left corner.

use crate::{Offset, Size};

/// An offset expressed as a fraction of a Size.
///
/// `FractionalOffset` uses a coordinate system where (0.0, 0.0) represents
/// the top-left corner and (1.0, 1.0) represents the bottom-right corner.
///
/// This is different from `Alignment` which uses (-1.0, -1.0) for top-left
/// and (1.0, 1.0) for bottom-right.
///
/// # Examples
///
/// ```
/// use flui_types::layout::FractionalOffset;
/// use flui_types::Size;
///
/// // Top-left corner
/// let top_left = FractionalOffset::TOP_LEFT;
/// assert_eq!(top_left.dx, 0.0);
/// assert_eq!(top_left.dy, 0.0);
///
/// // Center
/// let center = FractionalOffset::CENTER;
/// assert_eq!(center.dx, 0.5);
/// assert_eq!(center.dy, 0.5);
///
/// // Calculate offset within a container
/// let container = Size::new(200.0, 100.0);
/// let offset = center.along_size(container);
/// assert_eq!(offset.dx, 100.0);
/// assert_eq!(offset.dy, 50.0);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct FractionalOffset {
    /// The distance fraction in the horizontal direction.
    ///
    /// A value of 0.0 corresponds to the left edge, 1.0 to the right edge.
    pub dx: f32,

    /// The distance fraction in the vertical direction.
    ///
    /// A value of 0.0 corresponds to the top edge, 1.0 to the bottom edge.
    pub dy: f32,
}

impl FractionalOffset {
    /// The top-left corner (0.0, 0.0).
    pub const TOP_LEFT: Self = Self { dx: 0.0, dy: 0.0 };

    /// The top-center (0.5, 0.0).
    pub const TOP_CENTER: Self = Self { dx: 0.5, dy: 0.0 };

    /// The top-right corner (1.0, 0.0).
    pub const TOP_RIGHT: Self = Self { dx: 1.0, dy: 0.0 };

    /// The center-left (0.0, 0.5).
    pub const CENTER_LEFT: Self = Self { dx: 0.0, dy: 0.5 };

    /// The center (0.5, 0.5).
    pub const CENTER: Self = Self { dx: 0.5, dy: 0.5 };

    /// The center-right (1.0, 0.5).
    pub const CENTER_RIGHT: Self = Self { dx: 1.0, dy: 0.5 };

    /// The bottom-left corner (0.0, 1.0).
    pub const BOTTOM_LEFT: Self = Self { dx: 0.0, dy: 1.0 };

    /// The bottom-center (0.5, 1.0).
    pub const BOTTOM_CENTER: Self = Self { dx: 0.5, dy: 1.0 };

    /// The bottom-right corner (1.0, 1.0).
    pub const BOTTOM_RIGHT: Self = Self { dx: 1.0, dy: 1.0 };

    /// Creates a FractionalOffset from dx and dy values.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::layout::FractionalOffset;
    ///
    /// let offset = FractionalOffset::new(0.25, 0.75);
    /// assert_eq!(offset.dx, 0.25);
    /// assert_eq!(offset.dy, 0.75);
    /// ```
    #[inline]
    #[must_use]
    pub const fn new(dx: f32, dy: f32) -> Self {
        Self { dx, dy }
    }

    /// Creates a FractionalOffset from an Alignment.
    ///
    /// Converts from Alignment coordinates (-1.0 to 1.0) to
    /// FractionalOffset coordinates (0.0 to 1.0).
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::layout::{Alignment, FractionalOffset};
    ///
    /// let alignment = Alignment::CENTER; // (0.0, 0.0) in Alignment coords
    /// let fractional = FractionalOffset::from_alignment(alignment);
    /// assert_eq!(fractional.dx, 0.5);
    /// assert_eq!(fractional.dy, 0.5);
    /// ```
    #[inline]
    #[must_use]
    pub fn from_alignment(alignment: crate::layout::Alignment) -> Self {
        Self {
            dx: (alignment.x + 1.0) / 2.0,
            dy: (alignment.y + 1.0) / 2.0,
        }
    }

    /// Converts this FractionalOffset to an Alignment.
    ///
    /// Converts from FractionalOffset coordinates (0.0 to 1.0) to
    /// Alignment coordinates (-1.0 to 1.0).
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::layout::{Alignment, FractionalOffset};
    ///
    /// let fractional = FractionalOffset::CENTER; // (0.5, 0.5)
    /// let alignment = fractional.to_alignment();
    /// assert_eq!(alignment.x, 0.0);
    /// assert_eq!(alignment.y, 0.0);
    /// ```
    #[inline]
    #[must_use]
    pub fn to_alignment(&self) -> crate::layout::Alignment {
        crate::layout::Alignment::new(self.dx * 2.0 - 1.0, self.dy * 2.0 - 1.0)
    }

    /// Returns the offset in pixels for a given container size.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::layout::FractionalOffset;
    /// use flui_types::Size;
    ///
    /// let offset = FractionalOffset::new(0.25, 0.5);
    /// let container = Size::new(200.0, 100.0);
    /// let pixels = offset.along_size(container);
    ///
    /// assert_eq!(pixels.dx, 50.0);  // 0.25 * 200
    /// assert_eq!(pixels.dy, 50.0);  // 0.5 * 100
    /// ```
    #[inline]
    #[must_use]
    pub fn along_size(&self, size: Size<f32>) -> Offset<f32> {
        Offset::new(self.dx * size.width, self.dy * size.height)
    }

    /// Returns the offset in pixels for positioning a child within a parent.
    ///
    /// This calculates where to position the top-left corner of a child
    /// of `child_size` within a parent of `parent_size` such that the
    /// fractional offset point on the child aligns with the fractional
    /// offset point on the parent.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::layout::FractionalOffset;
    /// use flui_types::Size;
    ///
    /// let center = FractionalOffset::CENTER;
    /// let parent = Size::new(200.0, 200.0);
    /// let child = Size::new(50.0, 50.0);
    /// let offset = center.along_offset(parent, child);
    ///
    /// // Child should be centered: (200-50)/2 = 75
    /// assert_eq!(offset.dx, 75.0);
    /// assert_eq!(offset.dy, 75.0);
    /// ```
    #[inline]
    #[must_use]
    pub fn along_offset(&self, parent_size: Size<f32>, child_size: Size<f32>) -> Offset<f32> {
        let parent_offset = self.along_size(parent_size);
        let child_offset = self.along_size(child_size);
        Offset::new(
            parent_offset.dx - child_offset.dx,
            parent_offset.dy - child_offset.dy,
        )
    }

    /// Linearly interpolates between two FractionalOffsets.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::layout::FractionalOffset;
    ///
    /// let a = FractionalOffset::TOP_LEFT;
    /// let b = FractionalOffset::BOTTOM_RIGHT;
    /// let mid = FractionalOffset::lerp(a, b, 0.5);
    ///
    /// assert_eq!(mid.dx, 0.5);
    /// assert_eq!(mid.dy, 0.5);
    /// ```
    #[inline]
    #[must_use]
    pub fn lerp(a: Self, b: Self, t: f32) -> Self {
        Self {
            dx: a.dx + (b.dx - a.dx) * t,
            dy: a.dy + (b.dy - a.dy) * t,
        }
    }

    /// Returns whether both components are finite.
    #[inline]
    #[must_use]
    pub fn is_finite(&self) -> bool {
        self.dx.is_finite() && self.dy.is_finite()
    }

    /// Returns the negation of this offset.
    #[inline]
    #[must_use]
    pub fn negate(&self) -> Self {
        Self {
            dx: -self.dx,
            dy: -self.dy,
        }
    }
}

impl std::ops::Add for FractionalOffset {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self {
            dx: self.dx + other.dx,
            dy: self.dy + other.dy,
        }
    }
}

impl std::ops::Sub for FractionalOffset {
    type Output = Self;

    fn sub(self, other: Self) -> Self {
        Self {
            dx: self.dx - other.dx,
            dy: self.dy - other.dy,
        }
    }
}

impl std::ops::Mul<f32> for FractionalOffset {
    type Output = Self;

    fn mul(self, factor: f32) -> Self {
        Self {
            dx: self.dx * factor,
            dy: self.dy * factor,
        }
    }
}

impl std::ops::Div<f32> for FractionalOffset {
    type Output = Self;

    fn div(self, divisor: f32) -> Self {
        Self {
            dx: self.dx / divisor,
            dy: self.dy / divisor,
        }
    }
}

impl std::ops::Neg for FractionalOffset {
    type Output = Self;

    fn neg(self) -> Self {
        self.negate()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::Alignment;

    #[test]
    fn test_constants() {
        assert_eq!(FractionalOffset::TOP_LEFT.dx, 0.0);
        assert_eq!(FractionalOffset::TOP_LEFT.dy, 0.0);

        assert_eq!(FractionalOffset::CENTER.dx, 0.5);
        assert_eq!(FractionalOffset::CENTER.dy, 0.5);

        assert_eq!(FractionalOffset::BOTTOM_RIGHT.dx, 1.0);
        assert_eq!(FractionalOffset::BOTTOM_RIGHT.dy, 1.0);
    }

    #[test]
    fn test_from_alignment() {
        let top_left = FractionalOffset::from_alignment(Alignment::TOP_LEFT);
        assert_eq!(top_left.dx, 0.0);
        assert_eq!(top_left.dy, 0.0);

        let center = FractionalOffset::from_alignment(Alignment::CENTER);
        assert_eq!(center.dx, 0.5);
        assert_eq!(center.dy, 0.5);

        let bottom_right = FractionalOffset::from_alignment(Alignment::BOTTOM_RIGHT);
        assert_eq!(bottom_right.dx, 1.0);
        assert_eq!(bottom_right.dy, 1.0);
    }

    #[test]
    fn test_to_alignment() {
        let alignment = FractionalOffset::CENTER.to_alignment();
        assert_eq!(alignment.x, 0.0);
        assert_eq!(alignment.y, 0.0);

        let alignment = FractionalOffset::TOP_LEFT.to_alignment();
        assert_eq!(alignment.x, -1.0);
        assert_eq!(alignment.y, -1.0);
    }

    #[test]
    fn test_along_size() {
        let offset = FractionalOffset::new(0.25, 0.5);
        let size = Size::new(200.0, 100.0);
        let pixels = offset.along_size(size);

        assert_eq!(pixels.dx, 50.0);
        assert_eq!(pixels.dy, 50.0);
    }

    #[test]
    fn test_along_offset() {
        let center = FractionalOffset::CENTER;
        let parent = Size::new(200.0, 200.0);
        let child = Size::new(50.0, 50.0);
        let offset = center.along_offset(parent, child);

        assert_eq!(offset.dx, 75.0);
        assert_eq!(offset.dy, 75.0);
    }

    #[test]
    fn test_lerp() {
        let a = FractionalOffset::new(0.0, 0.0);
        let b = FractionalOffset::new(1.0, 1.0);
        let mid = FractionalOffset::lerp(a, b, 0.5);

        assert_eq!(mid.dx, 0.5);
        assert_eq!(mid.dy, 0.5);
    }

    #[test]
    fn test_operators() {
        let a = FractionalOffset::new(0.2, 0.3);
        let b = FractionalOffset::new(0.1, 0.2);

        let sum = a + b;
        assert!((sum.dx - 0.3).abs() < 0.001);
        assert!((sum.dy - 0.5).abs() < 0.001);

        let diff = a - b;
        assert!((diff.dx - 0.1).abs() < 0.001);
        assert!((diff.dy - 0.1).abs() < 0.001);

        let scaled = a * 2.0;
        assert!((scaled.dx - 0.4).abs() < 0.001);
        assert!((scaled.dy - 0.6).abs() < 0.001);
    }
}
