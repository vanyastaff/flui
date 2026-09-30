//! The direction a scrollable or a sliver list grows along an axis.

use flui_foundation::geometry::Axis;

/// A direction along either the horizontal or vertical axis.
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
    /// use flui_foundation::geometry::Axis;
    /// use flui_rendering::constraints::AxisDirection;
    ///
    /// assert_eq!(AxisDirection::LeftToRight.axis(), Axis::Horizontal);
    /// assert_eq!(AxisDirection::TopToBottom.axis(), Axis::Vertical);
    /// ```
    #[inline]
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
    /// use flui_rendering::constraints::AxisDirection;
    ///
    /// assert_eq!(
    ///     AxisDirection::LeftToRight.opposite(),
    ///     AxisDirection::RightToLeft
    /// );
    /// assert_eq!(
    ///     AxisDirection::TopToBottom.opposite(),
    ///     AxisDirection::BottomToTop
    /// );
    /// ```
    #[inline]
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
    /// use flui_rendering::constraints::AxisDirection;
    ///
    /// assert!(AxisDirection::LeftToRight.is_positive());
    /// assert!(!AxisDirection::RightToLeft.is_positive());
    /// ```
    #[inline]
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
    /// use flui_rendering::constraints::AxisDirection;
    ///
    /// assert!(AxisDirection::RightToLeft.is_negative());
    /// assert!(!AxisDirection::LeftToRight.is_negative());
    /// ```
    #[inline]
    pub const fn is_negative(self) -> bool {
        !self.is_positive()
    }

    /// Check if this direction is reversed relative to the natural reading
    /// direction.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_rendering::constraints::AxisDirection;
    ///
    /// assert!(AxisDirection::RightToLeft.is_reversed());
    /// assert!(!AxisDirection::LeftToRight.is_reversed());
    /// ```
    #[inline]
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
    /// use flui_rendering::constraints::AxisDirection;
    ///
    /// assert_eq!(AxisDirection::LeftToRight.sign(), 1.0);
    /// assert_eq!(AxisDirection::RightToLeft.sign(), -1.0);
    /// ```
    #[inline]
    #[must_use]
    pub const fn sign(self) -> f64 {
        if self.is_positive() { 1.0 } else { -1.0 }
    }

    /// Create from an axis and whether it's reversed.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_foundation::geometry::Axis;
    /// use flui_rendering::constraints::AxisDirection;
    ///
    /// assert_eq!(
    ///     AxisDirection::from_axis(Axis::Horizontal, false),
    ///     AxisDirection::LeftToRight
    /// );
    /// assert_eq!(
    ///     AxisDirection::from_axis(Axis::Horizontal, true),
    ///     AxisDirection::RightToLeft
    /// );
    /// ```
    #[inline]
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
    /// use flui_rendering::constraints::AxisDirection;
    ///
    /// assert_eq!(
    ///     AxisDirection::TopToBottom.flip(),
    ///     AxisDirection::LeftToRight
    /// );
    /// assert_eq!(
    ///     AxisDirection::BottomToTop.flip(),
    ///     AxisDirection::RightToLeft
    /// );
    /// ```
    #[inline]
    pub const fn flip(self) -> Self {
        match self {
            AxisDirection::LeftToRight => AxisDirection::TopToBottom,
            AxisDirection::RightToLeft => AxisDirection::BottomToTop,
            AxisDirection::TopToBottom => AxisDirection::LeftToRight,
            AxisDirection::BottomToTop => AxisDirection::RightToLeft,
        }
    }
}
