//! Fractional offset for relative positioning.
//!
//! Similar to Flutter's `FractionalOffset` and `Alignment`.

use super::{Offset, Point, Size};

/// An offset that's expressed as a fraction of a [Size].
///
/// `FractionalOffset(0.0, 0.0)` represents the top left of the rectangle.
/// `FractionalOffset(1.0, 1.0)` represents the bottom right of the rectangle.
/// `FractionalOffset(0.5, 0.5)` represents the center of the rectangle.
///
/// Values can be outside the 0.0-1.0 range to position outside the bounds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FractionalOffset {
    /// The horizontal component, where 0.0 is left and 1.0 is right.
    pub dx: f32,

    /// The vertical component, where 0.0 is top and 1.0 is bottom.
    pub dy: f32,
}

impl FractionalOffset {
    /// Creates a fractional offset.
    pub const fn new(dx: f32, dy: f32) -> Self {
        Self { dx, dy }
    }

    /// The top left corner (0.0, 0.0).
    pub const TOP_LEFT: Self = Self::new(0.0, 0.0);

    /// The center of the top edge (0.5, 0.0).
    pub const TOP_CENTER: Self = Self::new(0.5, 0.0);

    /// The top right corner (1.0, 0.0).
    pub const TOP_RIGHT: Self = Self::new(1.0, 0.0);

    /// The center of the left edge (0.0, 0.5).
    pub const CENTER_LEFT: Self = Self::new(0.0, 0.5);

    /// The center (0.5, 0.5).
    pub const CENTER: Self = Self::new(0.5, 0.5);

    /// The center of the right edge (1.0, 0.5).
    pub const CENTER_RIGHT: Self = Self::new(1.0, 0.5);

    /// The bottom left corner (0.0, 1.0).
    pub const BOTTOM_LEFT: Self = Self::new(0.0, 1.0);

    /// The center of the bottom edge (0.5, 1.0).
    pub const BOTTOM_CENTER: Self = Self::new(0.5, 1.0);

    /// The bottom right corner (1.0, 1.0).
    pub const BOTTOM_RIGHT: Self = Self::new(1.0, 1.0);

    /// Convert the fractional offset to a [Point] within the given size.
    ///
    /// # Example
    /// ```
    /// # use nebula_ui::types::core::{FractionalOffset, Size, Point};
    /// let offset = FractionalOffset::CENTER;
    /// let size = Size::new(100.0, 200.0);
    /// let point = offset.resolve(size);
    ///
    /// assert_eq!(point, Point::new(50.0, 100.0));
    /// ```
    pub fn resolve(&self, size: impl Into<Size>) -> Point {
        let size = size.into();
        Point::new(self.dx * size.width, self.dy * size.height)
    }

    /// Convert the fractional offset to an [Offset] within the given size.
    pub fn to_offset(&self, size: impl Into<Size>) -> Offset {
        let size = size.into();
        Offset::new(self.dx * size.width, self.dy * size.height)
    }

    /// Linearly interpolate between two fractional offsets.
    ///
    /// # Example
    /// ```
    /// # use nebula_ui::types::core::FractionalOffset;
    /// let start = FractionalOffset::TOP_LEFT;
    /// let end = FractionalOffset::BOTTOM_RIGHT;
    /// let mid = FractionalOffset::lerp(start, end, 0.5);
    ///
    /// assert_eq!(mid, FractionalOffset::CENTER);
    /// ```
    pub fn lerp(a: impl Into<FractionalOffset>, b: impl Into<FractionalOffset>, t: f32) -> Self {
        let a = a.into();
        let b = b.into();
        Self::new(a.dx + (b.dx - a.dx) * t, a.dy + (b.dy - a.dy) * t)
    }

    /// Get the inverse of this fractional offset.
    ///
    /// # Example
    /// ```
    /// # use nebula_ui::types::core::FractionalOffset;
    /// let offset = FractionalOffset::new(0.2, 0.8);
    /// let inverse = offset.inverse();
    ///
    /// assert_eq!(inverse, FractionalOffset::new(0.8, 0.2));
    /// ```
    pub fn inverse(&self) -> Self {
        Self::new(1.0 - self.dx, 1.0 - self.dy)
    }
}

// Conversions from tuples
impl From<(f32, f32)> for FractionalOffset {
    fn from((dx, dy): (f32, f32)) -> Self {
        Self::new(dx, dy)
    }
}

impl From<[f32; 2]> for FractionalOffset {
    fn from([dx, dy]: [f32; 2]) -> Self {
        Self::new(dx, dy)
    }
}

// Display implementation
impl std::fmt::Display for FractionalOffset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "FractionalOffset({:.2}, {:.2})", self.dx, self.dy)
    }
}

// Default to center
impl Default for FractionalOffset {
    fn default() -> Self {
        Self::CENTER
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fractional_offset_creation() {
        let offset = FractionalOffset::new(0.5, 0.5);
        assert_eq!(offset.dx, 0.5);
        assert_eq!(offset.dy, 0.5);
    }

    #[test]
    fn test_fractional_offset_constants() {
        assert_eq!(FractionalOffset::TOP_LEFT, FractionalOffset::new(0.0, 0.0));
        assert_eq!(
            FractionalOffset::TOP_CENTER,
            FractionalOffset::new(0.5, 0.0)
        );
        assert_eq!(
            FractionalOffset::TOP_RIGHT,
            FractionalOffset::new(1.0, 0.0)
        );
        assert_eq!(
            FractionalOffset::CENTER_LEFT,
            FractionalOffset::new(0.0, 0.5)
        );
        assert_eq!(FractionalOffset::CENTER, FractionalOffset::new(0.5, 0.5));
        assert_eq!(
            FractionalOffset::CENTER_RIGHT,
            FractionalOffset::new(1.0, 0.5)
        );
        assert_eq!(
            FractionalOffset::BOTTOM_LEFT,
            FractionalOffset::new(0.0, 1.0)
        );
        assert_eq!(
            FractionalOffset::BOTTOM_CENTER,
            FractionalOffset::new(0.5, 1.0)
        );
        assert_eq!(
            FractionalOffset::BOTTOM_RIGHT,
            FractionalOffset::new(1.0, 1.0)
        );
    }

    #[test]
    fn test_fractional_offset_resolve() {
        let offset = FractionalOffset::CENTER;
        let size = Size::new(100.0, 200.0);
        let point = offset.resolve(size);

        assert_eq!(point, Point::new(50.0, 100.0));

        // Test top-left
        let offset = FractionalOffset::TOP_LEFT;
        let point = offset.resolve(size);
        assert_eq!(point, Point::new(0.0, 0.0));

        // Test bottom-right
        let offset = FractionalOffset::BOTTOM_RIGHT;
        let point = offset.resolve(size);
        assert_eq!(point, Point::new(100.0, 200.0));
    }

    #[test]
    fn test_fractional_offset_to_offset() {
        let fractional = FractionalOffset::new(0.25, 0.75);
        let size = Size::new(100.0, 200.0);
        let offset = fractional.to_offset(size);

        assert_eq!(offset, Offset::new(25.0, 150.0));
    }

    #[test]
    fn test_fractional_offset_lerp() {
        let start = FractionalOffset::TOP_LEFT;
        let end = FractionalOffset::BOTTOM_RIGHT;
        let mid = FractionalOffset::lerp(start, end, 0.5);

        assert_eq!(mid, FractionalOffset::CENTER);

        // Test quarter point
        let quarter = FractionalOffset::lerp(start, end, 0.25);
        assert_eq!(quarter, FractionalOffset::new(0.25, 0.25));
    }

    #[test]
    fn test_fractional_offset_inverse() {
        let offset = FractionalOffset::new(0.2, 0.8);
        let inverse = offset.inverse();

        assert!((inverse.dx - 0.8).abs() < 1e-6);
        assert!((inverse.dy - 0.2).abs() < 1e-6);

        // Test that double inverse returns original
        let double_inverse = inverse.inverse();
        assert!((double_inverse.dx - offset.dx).abs() < 1e-6);
        assert!((double_inverse.dy - offset.dy).abs() < 1e-6);
    }

    #[test]
    fn test_fractional_offset_conversions() {
        // From tuple
        let offset: FractionalOffset = (0.3, 0.7).into();
        assert_eq!(offset, FractionalOffset::new(0.3, 0.7));

        // From array
        let offset: FractionalOffset = [0.3, 0.7].into();
        assert_eq!(offset, FractionalOffset::new(0.3, 0.7));
    }

    #[test]
    fn test_fractional_offset_default() {
        let offset = FractionalOffset::default();
        assert_eq!(offset, FractionalOffset::CENTER);
    }

    #[test]
    fn test_fractional_offset_display() {
        let offset = FractionalOffset::new(0.5, 0.75);
        assert_eq!(format!("{}", offset), "FractionalOffset(0.50, 0.75)");
    }

    #[test]
    fn test_fractional_offset_outside_bounds() {
        // Fractional offsets can be outside 0.0-1.0 range
        let offset = FractionalOffset::new(-0.5, 1.5);
        let size = Size::new(100.0, 200.0);
        let point = offset.resolve(size);

        assert_eq!(point, Point::new(-50.0, 300.0));
    }
}
