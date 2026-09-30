//! The order a flex lays its children out in along the vertical axis.

use flui_rendering::constraints::AxisDirection;

/// The direction in which boxes flow vertically.
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
    /// use flui_rendering::constraints::AxisDirection;
    /// use flui_objects::VerticalDirection;
    ///
    /// assert_eq!(
    ///     VerticalDirection::Down.to_axis_direction(),
    ///     AxisDirection::TopToBottom
    /// );
    /// assert_eq!(
    ///     VerticalDirection::Up.to_axis_direction(),
    ///     AxisDirection::BottomToTop
    /// );
    /// ```
    #[inline]
    pub const fn to_axis_direction(self) -> AxisDirection {
        match self {
            VerticalDirection::Down => AxisDirection::TopToBottom,
            VerticalDirection::Up => AxisDirection::BottomToTop,
        }
    }

    /// Check if this direction is down (top to bottom).
    #[inline]
    pub const fn is_down(self) -> bool {
        matches!(self, VerticalDirection::Down)
    }

    /// Check if this direction is up (bottom to top).
    #[inline]
    pub const fn is_up(self) -> bool {
        matches!(self, VerticalDirection::Up)
    }

    /// Get the opposite direction.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_objects::VerticalDirection;
    ///
    /// assert_eq!(VerticalDirection::Down.opposite(), VerticalDirection::Up);
    /// assert_eq!(VerticalDirection::Up.opposite(), VerticalDirection::Down);
    /// ```
    #[inline]
    pub const fn opposite(self) -> Self {
        match self {
            VerticalDirection::Down => VerticalDirection::Up,
            VerticalDirection::Up => VerticalDirection::Down,
        }
    }
}
