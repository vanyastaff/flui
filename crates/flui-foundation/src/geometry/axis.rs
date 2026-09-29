//! The two axes of a 2D coordinate system.

use crate::geometry::Size;

/// The two cardinal directions in two dimensions.
///
/// Similar to Flutter's `Axis`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Axis {
    /// The horizontal axis (left to right).
    #[default]
    Horizontal,

    /// The vertical axis (top to bottom).
    Vertical,
}

impl Axis {
    /// Get the opposite axis.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_foundation::geometry::Axis;
    ///
    /// assert_eq!(Axis::Horizontal.opposite(), Axis::Vertical);
    /// assert_eq!(Axis::Vertical.opposite(), Axis::Horizontal);
    /// ```
    #[inline]
    pub const fn opposite(self) -> Self {
        match self {
            Axis::Horizontal => Axis::Vertical,
            Axis::Vertical => Axis::Horizontal,
        }
    }

    /// Check if this is the horizontal axis.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_foundation::geometry::Axis;
    ///
    /// assert!(Axis::Horizontal.is_horizontal());
    /// assert!(!Axis::Vertical.is_horizontal());
    /// ```
    #[inline]
    pub const fn is_horizontal(self) -> bool {
        matches!(self, Axis::Horizontal)
    }

    /// Check if this is the vertical axis.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_foundation::geometry::Axis;
    ///
    /// assert!(Axis::Vertical.is_vertical());
    /// assert!(!Axis::Horizontal.is_vertical());
    /// ```
    #[inline]
    pub const fn is_vertical(self) -> bool {
        matches!(self, Axis::Vertical)
    }

    /// Get the size component along this axis.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_foundation::geometry::Axis;
    /// use flui_foundation::geometry::Size;
    ///
    /// let size = Size::new(100.0, 50.0);
    /// assert_eq!(Axis::Horizontal.select_size(size), 100.0);
    /// assert_eq!(Axis::Vertical.select_size(size), 50.0);
    /// ```
    #[inline]
    #[must_use]
    pub const fn select_size(self, size: Size<f64>) -> f64 {
        match self {
            Axis::Horizontal => size.width,
            Axis::Vertical => size.height,
        }
    }

    /// Create a size with the given value on this axis and zero on the other.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_foundation::geometry::Axis;
    /// use flui_foundation::geometry::Size;
    ///
    /// assert_eq!(Axis::Horizontal.make_size(100.0), Size::new(100.0, 0.0));
    /// assert_eq!(Axis::Vertical.make_size(100.0), Size::new(0.0, 100.0));
    /// ```
    #[inline]
    #[must_use]
    pub const fn make_size(self, value: f64) -> Size<f64> {
        match self {
            Axis::Horizontal => Size::new(value, 0.0),
            Axis::Vertical => Size::new(0.0, value),
        }
    }

    /// Create a size with the given main and cross values.
    ///
    /// The main value is along this axis, cross value is along the opposite
    /// axis.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_foundation::geometry::Axis;
    /// use flui_foundation::geometry::Size;
    ///
    /// assert_eq!(
    ///     Axis::Horizontal.make_size_with_cross(100.0, 50.0),
    ///     Size::new(100.0, 50.0)
    /// );
    /// assert_eq!(
    ///     Axis::Vertical.make_size_with_cross(100.0, 50.0),
    ///     Size::new(50.0, 100.0)
    /// );
    /// ```
    #[inline]
    #[must_use]
    pub const fn make_size_with_cross(self, main: f64, cross: f64) -> Size<f64> {
        match self {
            Axis::Horizontal => Size::new(main, cross),
            Axis::Vertical => Size::new(cross, main),
        }
    }

    /// Flip a size based on this axis.
    ///
    /// Horizontal axis returns the size unchanged, vertical axis swaps width
    /// and height.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_foundation::geometry::Axis;
    /// use flui_foundation::geometry::Size;
    ///
    /// let size = Size::new(100.0, 50.0);
    /// assert_eq!(Axis::Horizontal.flip_size(size), Size::new(100.0, 50.0));
    /// assert_eq!(Axis::Vertical.flip_size(size), Size::new(50.0, 100.0));
    /// ```
    #[inline]
    #[must_use]
    pub const fn flip_size(self, size: Size<f64>) -> Size<f64> {
        match self {
            Axis::Horizontal => size,
            Axis::Vertical => Size::new(size.height, size.width),
        }
    }

    /// Get the main size component (along this axis).
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_foundation::geometry::Axis;
    /// use flui_foundation::geometry::Size;
    ///
    /// let size = Size::new(100.0, 50.0);
    /// assert_eq!(Axis::Horizontal.main_size(size), 100.0);
    /// assert_eq!(Axis::Vertical.main_size(size), 50.0);
    /// ```
    #[inline]
    #[must_use]
    pub const fn main_size(self, size: Size<f64>) -> f64 {
        self.select_size(size)
    }

    /// Get the cross size component (along the opposite axis).
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_foundation::geometry::Axis;
    /// use flui_foundation::geometry::Size;
    ///
    /// let size = Size::new(100.0, 50.0);
    /// assert_eq!(Axis::Horizontal.cross_size(size), 50.0);
    /// assert_eq!(Axis::Vertical.cross_size(size), 100.0);
    /// ```
    #[inline]
    #[must_use]
    pub const fn cross_size(self, size: Size<f64>) -> f64 {
        self.opposite().select_size(size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[inline]
    fn test_axis_operations() {
        let horizontal = Axis::Horizontal;
        let vertical = Axis::Vertical;

        assert!(horizontal.is_horizontal());
        assert!(!horizontal.is_vertical());
        assert!(vertical.is_vertical());
        assert!(!vertical.is_horizontal());

        assert_eq!(horizontal.opposite(), Axis::Vertical);
        assert_eq!(vertical.opposite(), Axis::Horizontal);
    }

    #[test]
    #[inline]
    fn test_axis_size_operations() {
        let size = Size::new(100.0, 50.0);

        assert_eq!(Axis::Horizontal.select_size(size), 100.0);
        assert_eq!(Axis::Vertical.select_size(size), 50.0);

        assert_eq!(Axis::Horizontal.main_size(size), 100.0);
        assert_eq!(Axis::Vertical.main_size(size), 50.0);

        assert_eq!(Axis::Horizontal.cross_size(size), 50.0);
        assert_eq!(Axis::Vertical.cross_size(size), 100.0);
    }

    #[test]
    #[inline]
    fn test_axis_make_size() {
        assert_eq!(Axis::Horizontal.make_size(100.0), Size::new(100.0, 0.0));
        assert_eq!(Axis::Vertical.make_size(100.0), Size::new(0.0, 100.0));

        assert_eq!(
            Axis::Horizontal.make_size_with_cross(100.0, 50.0),
            Size::new(100.0, 50.0)
        );
        assert_eq!(
            Axis::Vertical.make_size_with_cross(100.0, 50.0),
            Size::new(50.0, 100.0)
        );
    }

    #[test]
    #[inline]
    fn test_axis_flip_size() {
        let size = Size::new(100.0, 50.0);
        assert_eq!(Axis::Horizontal.flip_size(size), Size::new(100.0, 50.0));
        assert_eq!(Axis::Vertical.flip_size(size), Size::new(50.0, 100.0));
    }
}
