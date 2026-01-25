//! Axis and direction types for layout systems
//!
//! This module contains types for representing axes, directions, and orientation,
//! similar to Flutter's axis system.

use crate::geometry::Pixels;
use crate::Size;

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
    /// use flui_types::Axis;
    ///
    /// assert_eq!(Axis::Horizontal.opposite(), Axis::Vertical);
    /// assert_eq!(Axis::Vertical.opposite(), Axis::Horizontal);
    /// ```
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
    /// use flui_types::Axis;
    ///
    /// assert!(Axis::Horizontal.is_horizontal());
    /// assert!(!Axis::Vertical.is_horizontal());
    /// ```
    pub const fn is_horizontal(self) -> bool {
        matches!(self, Axis::Horizontal)
    }

    /// Check if this is the vertical axis.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::Axis;
    ///
    /// assert!(Axis::Vertical.is_vertical());
    /// assert!(!Axis::Horizontal.is_vertical());
    /// ```
    pub const fn is_vertical(self) -> bool {
        matches!(self, Axis::Vertical)
    }

    /// Get the size component along this axis.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::{Axis, Size};
    ///
    /// let size = Size::new(100.0, 50.0);
    /// assert_eq!(Axis::Horizontal.select_size(size), 100.0);
    /// assert_eq!(Axis::Vertical.select_size(size), 50.0);
    /// ```
    #[inline]
    #[must_use]
    pub const fn select_size(self, size: Size<Pixels>) -> f32 {
        match self {
            Axis::Horizontal => size.width.0,
            Axis::Vertical => size.height.0,
        }
    }

    /// Create a size with the given value on this axis and zero on the other.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::{Axis, Size};
    ///
    /// assert_eq!(Axis::Horizontal.make_size(100.0), Size::new(100.0, 0.0));
    /// assert_eq!(Axis::Vertical.make_size(100.0), Size::new(0.0, 100.0));
    /// ```
    #[inline]
    #[must_use]
    pub const fn make_size(self, value: f32) -> Size<Pixels> {
        match self {
            Axis::Horizontal => Size::new(Pixels(value), Pixels(0.0)),
            Axis::Vertical => Size::new(Pixels(0.0), Pixels(value)),
        }
    }

    /// Create a size with the given main and cross values.
    ///
    /// The main value is along this axis, cross value is along the opposite axis.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::{Axis, Size};
    ///
    /// assert_eq!(Axis::Horizontal.make_size_with_cross(100.0, 50.0), Size::new(100.0, 50.0));
    /// assert_eq!(Axis::Vertical.make_size_with_cross(100.0, 50.0), Size::new(50.0, 100.0));
    /// ```
    #[inline]
    #[must_use]
    pub const fn make_size_with_cross(self, main: f32, cross: f32) -> Size<Pixels> {
        match self {
            Axis::Horizontal => Size::new(Pixels(main), Pixels(cross)),
            Axis::Vertical => Size::new(Pixels(cross), Pixels(main)),
        }
    }

    /// Flip a size based on this axis.
    ///
    /// Horizontal axis returns the size unchanged, vertical axis swaps width and height.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::{Axis, Size};
    ///
    /// let size = Size::new(100.0, 50.0);
    /// assert_eq!(Axis::Horizontal.flip_size(size), Size::new(100.0, 50.0));
    /// assert_eq!(Axis::Vertical.flip_size(size), Size::new(50.0, 100.0));
    /// ```
    #[inline]
    #[must_use]
    pub const fn flip_size(self, size: Size<Pixels>) -> Size<Pixels> {
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
    /// use flui_types::{Axis, Size};
    ///
    /// let size = Size::new(100.0, 50.0);
    /// assert_eq!(Axis::Horizontal.main_size(size), 100.0);
    /// assert_eq!(Axis::Vertical.main_size(size), 50.0);
    /// ```
    #[inline]
    #[must_use]
    pub const fn main_size(self, size: Size<Pixels>) -> f32 {
        self.select_size(size)
    }

    /// Get the cross size component (along the opposite axis).
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::{Axis, Size};
    ///
    /// let size = Size::new(100.0, 50.0);
    /// assert_eq!(Axis::Horizontal.cross_size(size), 50.0);
    /// assert_eq!(Axis::Vertical.cross_size(size), 100.0);
    /// ```
    #[inline]
    #[must_use]
    pub const fn cross_size(self, size: Size<Pixels>) -> f32 {
        self.opposite().select_size(size)
    }
}

/// A direction along either the horizontal or vertical axis.
///
/// Similar to Flutter's `AxisDirection`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Default)]
pub enum AxisDirection {
    /// From left to right.
    #[default]
    LeftToRight,

    /// From right to left.
    RightToLeft,

    /// From top to bottom.
    TopToBottom,

    /// From bottom to top.
    BottomToTop,
}

impl AxisDirection {
    /// Get the axis for this direction.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::{Axis, layout::AxisDirection};
    ///
    /// assert_eq!(AxisDirection::LeftToRight.axis(), Axis::Horizontal);
    /// assert_eq!(AxisDirection::TopToBottom.axis(), Axis::Vertical);
    /// ```
    pub const fn axis(self) -> Axis {
        match self {
            AxisDirection::LeftToRight | AxisDirection::RightToLeft => Axis::Horizontal,
            AxisDirection::TopToBottom | AxisDirection::BottomToTop => Axis::Vertical,
        }
    }

    /// Get the opposite direction.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::layout::AxisDirection;
    ///
    /// assert_eq!(AxisDirection::LeftToRight.opposite(), AxisDirection::RightToLeft);
    /// assert_eq!(AxisDirection::TopToBottom.opposite(), AxisDirection::BottomToTop);
    /// ```
    pub const fn opposite(self) -> Self {
        match self {
            AxisDirection::LeftToRight => AxisDirection::RightToLeft,
            AxisDirection::RightToLeft => AxisDirection::LeftToRight,
            AxisDirection::TopToBottom => AxisDirection::BottomToTop,
            AxisDirection::BottomToTop => AxisDirection::TopToBottom,
        }
    }

    /// Check if this direction is positive (left-to-right or top-to-bottom).
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::layout::AxisDirection;
    ///
    /// assert!(AxisDirection::LeftToRight.is_positive());
    /// assert!(!AxisDirection::RightToLeft.is_positive());
    /// ```
    pub const fn is_positive(self) -> bool {
        matches!(
            self,
            AxisDirection::LeftToRight | AxisDirection::TopToBottom
        )
    }

    /// Check if this direction is negative (right-to-left or bottom-to-top).
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::layout::AxisDirection;
    ///
    /// assert!(AxisDirection::RightToLeft.is_negative());
    /// assert!(!AxisDirection::LeftToRight.is_negative());
    /// ```
    pub const fn is_negative(self) -> bool {
        !self.is_positive()
    }

    /// Check if this direction is reversed relative to the natural reading direction.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::layout::AxisDirection;
    ///
    /// assert!(AxisDirection::RightToLeft.is_reversed());
    /// assert!(!AxisDirection::LeftToRight.is_reversed());
    /// ```
    pub const fn is_reversed(self) -> bool {
        matches!(
            self,
            AxisDirection::RightToLeft | AxisDirection::BottomToTop
        )
    }

    /// Convert to a sign multiplier (1.0 for positive, -1.0 for negative).
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::layout::AxisDirection;
    ///
    /// assert_eq!(AxisDirection::LeftToRight.sign(), 1.0);
    /// assert_eq!(AxisDirection::RightToLeft.sign(), -1.0);
    /// ```
    #[inline]
    #[must_use]
    pub const fn sign(self) -> f32 {
        if self.is_positive() {
            1.0
        } else {
            -1.0
        }
    }

    /// Create from an axis and whether it's reversed.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::{Axis, layout::AxisDirection};
    ///
    /// assert_eq!(AxisDirection::from_axis(Axis::Horizontal, false), AxisDirection::LeftToRight);
    /// assert_eq!(AxisDirection::from_axis(Axis::Horizontal, true), AxisDirection::RightToLeft);
    /// ```
    pub const fn from_axis(axis: Axis, reversed: bool) -> Self {
        match (axis, reversed) {
            (Axis::Horizontal, false) => AxisDirection::LeftToRight,
            (Axis::Horizontal, true) => AxisDirection::RightToLeft,
            (Axis::Vertical, false) => AxisDirection::TopToBottom,
            (Axis::Vertical, true) => AxisDirection::BottomToTop,
        }
    }

    /// Get the perpendicular direction
    ///
    /// Flips to the cross axis while maintaining direction sign.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::layout::AxisDirection;
    ///
    /// assert_eq!(AxisDirection::TopToBottom.flip(), AxisDirection::LeftToRight);
    /// assert_eq!(AxisDirection::BottomToTop.flip(), AxisDirection::RightToLeft);
    /// ```
    pub const fn flip(self) -> Self {
        match self {
            AxisDirection::LeftToRight => AxisDirection::TopToBottom,
            AxisDirection::RightToLeft => AxisDirection::BottomToTop,
            AxisDirection::TopToBottom => AxisDirection::LeftToRight,
            AxisDirection::BottomToTop => AxisDirection::RightToLeft,
        }
    }
}

/// Whether in portrait or landscape orientation.
///
/// Similar to Flutter's `Orientation`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Orientation {
    /// Portrait orientation (height > width).
    #[default]
    Portrait,

    /// Landscape orientation (width >= height).
    Landscape,
}

impl Orientation {
    /// Determine orientation from a size.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::{layout::Orientation, Size};
    ///
    /// assert_eq!(Orientation::from_size(Size::new(100.0, 200.0)), Orientation::Portrait);
    /// assert_eq!(Orientation::from_size(Size::new(200.0, 100.0)), Orientation::Landscape);
    /// ```
    #[inline]
    #[must_use]
    pub fn from_size(size: Size<Pixels>) -> Self {
        if size.height > size.width {
            Orientation::Portrait
        } else {
            Orientation::Landscape
        }
    }

    /// Get the main axis for this orientation.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::{Axis, layout::Orientation};
    ///
    /// assert_eq!(Orientation::Portrait.main_axis(), Axis::Vertical);
    /// assert_eq!(Orientation::Landscape.main_axis(), Axis::Horizontal);
    /// ```
    pub const fn main_axis(self) -> Axis {
        match self {
            Orientation::Portrait => Axis::Vertical,
            Orientation::Landscape => Axis::Horizontal,
        }
    }

    /// Get the cross axis for this orientation.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::{Axis, layout::Orientation};
    ///
    /// assert_eq!(Orientation::Portrait.cross_axis(), Axis::Horizontal);
    /// assert_eq!(Orientation::Landscape.cross_axis(), Axis::Vertical);
    /// ```
    pub const fn cross_axis(self) -> Axis {
        self.main_axis().opposite()
    }

    /// Check if this is portrait orientation.
    pub const fn is_portrait(self) -> bool {
        matches!(self, Orientation::Portrait)
    }

    /// Check if this is landscape orientation.
    pub const fn is_landscape(self) -> bool {
        matches!(self, Orientation::Landscape)
    }
}

/// The direction in which boxes flow vertically.
///
/// Similar to Flutter's `VerticalDirection`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum VerticalDirection {
    /// Boxes flow from top to bottom.
    #[default]
    Down,

    /// Boxes flow from bottom to top.
    Up,
}

impl VerticalDirection {
    /// Get the axis direction for this vertical direction.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::layout::{AxisDirection, VerticalDirection};
    ///
    /// assert_eq!(VerticalDirection::Down.to_axis_direction(), AxisDirection::TopToBottom);
    /// assert_eq!(VerticalDirection::Up.to_axis_direction(), AxisDirection::BottomToTop);
    /// ```
    pub const fn to_axis_direction(self) -> AxisDirection {
        match self {
            VerticalDirection::Down => AxisDirection::TopToBottom,
            VerticalDirection::Up => AxisDirection::BottomToTop,
        }
    }

    /// Check if this direction is down (top to bottom).
    pub const fn is_down(self) -> bool {
        matches!(self, VerticalDirection::Down)
    }

    /// Check if this direction is up (bottom to top).
    pub const fn is_up(self) -> bool {
        matches!(self, VerticalDirection::Up)
    }

    /// Get the opposite direction.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::layout::VerticalDirection;
    ///
    /// assert_eq!(VerticalDirection::Down.opposite(), VerticalDirection::Up);
    /// assert_eq!(VerticalDirection::Up.opposite(), VerticalDirection::Down);
    /// ```
    pub const fn opposite(self) -> Self {
        match self {
            VerticalDirection::Down => VerticalDirection::Up,
            VerticalDirection::Up => VerticalDirection::Down,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
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
    fn test_axis_flip_size() {
        let size = Size::new(100.0, 50.0);
        assert_eq!(Axis::Horizontal.flip_size(size), Size::new(100.0, 50.0));
        assert_eq!(Axis::Vertical.flip_size(size), Size::new(50.0, 100.0));
    }

    #[test]
    fn test_axis_direction_operations() {
        let ltr = AxisDirection::LeftToRight;
        let rtl = AxisDirection::RightToLeft;
        let ttb = AxisDirection::TopToBottom;
        let btt = AxisDirection::BottomToTop;

        assert_eq!(ltr.axis(), Axis::Horizontal);
        assert_eq!(rtl.axis(), Axis::Horizontal);
        assert_eq!(ttb.axis(), Axis::Vertical);
        assert_eq!(btt.axis(), Axis::Vertical);

        assert!(ltr.is_positive());
        assert!(!rtl.is_positive());
        assert!(ttb.is_positive());
        assert!(!btt.is_positive());

        assert!(!ltr.is_negative());
        assert!(rtl.is_negative());
        assert!(!ttb.is_negative());
        assert!(btt.is_negative());

        assert!(!ltr.is_reversed());
        assert!(rtl.is_reversed());
        assert!(!ttb.is_reversed());
        assert!(btt.is_reversed());

        assert_eq!(ltr.sign(), 1.0);
        assert_eq!(rtl.sign(), -1.0);
        assert_eq!(ttb.sign(), 1.0);
        assert_eq!(btt.sign(), -1.0);

        assert_eq!(ltr.opposite(), rtl);
        assert_eq!(rtl.opposite(), ltr);
        assert_eq!(ttb.opposite(), btt);
        assert_eq!(btt.opposite(), ttb);
    }

    #[test]
    fn test_axis_direction_from_axis() {
        assert_eq!(
            AxisDirection::from_axis(Axis::Horizontal, false),
            AxisDirection::LeftToRight
        );
        assert_eq!(
            AxisDirection::from_axis(Axis::Horizontal, true),
            AxisDirection::RightToLeft
        );
        assert_eq!(
            AxisDirection::from_axis(Axis::Vertical, false),
            AxisDirection::TopToBottom
        );
        assert_eq!(
            AxisDirection::from_axis(Axis::Vertical, true),
            AxisDirection::BottomToTop
        );
    }

    #[test]
    fn test_orientation_operations() {
        let portrait = Orientation::Portrait;
        let landscape = Orientation::Landscape;

        assert!(portrait.is_portrait());
        assert!(!portrait.is_landscape());
        assert!(landscape.is_landscape());
        assert!(!landscape.is_portrait());

        assert_eq!(portrait.main_axis(), Axis::Vertical);
        assert_eq!(portrait.cross_axis(), Axis::Horizontal);
        assert_eq!(landscape.main_axis(), Axis::Horizontal);
        assert_eq!(landscape.cross_axis(), Axis::Vertical);
    }

    #[test]
    fn test_orientation_from_size() {
        assert_eq!(
            Orientation::from_size(Size::new(100.0, 200.0)),
            Orientation::Portrait
        );
        assert_eq!(
            Orientation::from_size(Size::new(200.0, 100.0)),
            Orientation::Landscape
        );
        // Tie goes to landscape
        assert_eq!(
            Orientation::from_size(Size::new(100.0, 100.0)),
            Orientation::Landscape
        );
    }

    #[test]
    fn test_vertical_direction_operations() {
        let down = VerticalDirection::Down;
        let up = VerticalDirection::Up;

        assert!(down.is_down());
        assert!(!down.is_up());
        assert!(up.is_up());
        assert!(!up.is_down());

        assert_eq!(down.to_axis_direction(), AxisDirection::TopToBottom);
        assert_eq!(up.to_axis_direction(), AxisDirection::BottomToTop);

        assert_eq!(down.opposite(), up);
        assert_eq!(up.opposite(), down);
    }
}
