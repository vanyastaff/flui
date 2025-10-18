//! Direction types for scrolling and layout growth
//!
//! This module provides types that describe directions for scroll views
//! and layout growth.

/// Direction in which content grows in a scrollable area
///
/// Similar to Flutter's `GrowthDirection`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Default)]
pub enum GrowthDirection {
    /// Content grows in the forward direction
    ///
    /// For vertical scrolling, this is downward.
    /// For horizontal scrolling, this is to the right (in LTR) or left (in RTL).
    #[default]
    Forward,

    /// Content grows in the reverse direction
    ///
    /// For vertical scrolling, this is upward.
    /// For horizontal scrolling, this is to the left (in LTR) or right (in RTL).
    Reverse,
}

impl GrowthDirection {
    /// Returns whether this growth direction is forward
    pub const fn is_forward(self) -> bool {
        matches!(self, GrowthDirection::Forward)
    }

    /// Returns whether this growth direction is reverse
    pub const fn is_reverse(self) -> bool {
        matches!(self, GrowthDirection::Reverse)
    }

    /// Returns the opposite direction
    pub const fn flip(self) -> Self {
        match self {
            GrowthDirection::Forward => GrowthDirection::Reverse,
            GrowthDirection::Reverse => GrowthDirection::Forward,
        }
    }
}


/// Direction of a scroll gesture or animation
///
/// Similar to Flutter's `ScrollDirection`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Default)]
pub enum ScrollDirection {
    /// The scroll view is not scrolling
    #[default]
    Idle,

    /// The scroll view is scrolling in the forward direction
    ///
    /// For a vertical scroll view, this means scrolling down.
    /// For a horizontal scroll view, this means scrolling to the right (in LTR).
    Forward,

    /// The scroll view is scrolling in the reverse direction
    ///
    /// For a vertical scroll view, this means scrolling up.
    /// For a horizontal scroll view, this means scrolling to the left (in LTR).
    Reverse,
}

impl ScrollDirection {
    /// Returns whether the scroll is idle
    pub const fn is_idle(self) -> bool {
        matches!(self, ScrollDirection::Idle)
    }

    /// Returns whether the scroll is forward
    pub const fn is_forward(self) -> bool {
        matches!(self, ScrollDirection::Forward)
    }

    /// Returns whether the scroll is reverse
    pub const fn is_reverse(self) -> bool {
        matches!(self, ScrollDirection::Reverse)
    }

    /// Returns the opposite direction, or Idle if already idle
    pub const fn flip(self) -> Self {
        match self {
            ScrollDirection::Idle => ScrollDirection::Idle,
            ScrollDirection::Forward => ScrollDirection::Reverse,
            ScrollDirection::Reverse => ScrollDirection::Forward,
        }
    }
}


/// The direction in which a sliver's content is ordered
///
/// Re-exported from layout module for convenience.
pub use crate::layout::AxisDirection;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_growth_direction_forward() {
        let dir = GrowthDirection::Forward;
        assert!(dir.is_forward());
        assert!(!dir.is_reverse());
        assert_eq!(dir.flip(), GrowthDirection::Reverse);
    }

    #[test]
    fn test_growth_direction_reverse() {
        let dir = GrowthDirection::Reverse;
        assert!(!dir.is_forward());
        assert!(dir.is_reverse());
        assert_eq!(dir.flip(), GrowthDirection::Forward);
    }

    #[test]
    fn test_growth_direction_default() {
        assert_eq!(GrowthDirection::default(), GrowthDirection::Forward);
    }

    #[test]
    fn test_scroll_direction_idle() {
        let dir = ScrollDirection::Idle;
        assert!(dir.is_idle());
        assert!(!dir.is_forward());
        assert!(!dir.is_reverse());
        assert_eq!(dir.flip(), ScrollDirection::Idle);
    }

    #[test]
    fn test_scroll_direction_forward() {
        let dir = ScrollDirection::Forward;
        assert!(!dir.is_idle());
        assert!(dir.is_forward());
        assert!(!dir.is_reverse());
        assert_eq!(dir.flip(), ScrollDirection::Reverse);
    }

    #[test]
    fn test_scroll_direction_reverse() {
        let dir = ScrollDirection::Reverse;
        assert!(!dir.is_idle());
        assert!(!dir.is_forward());
        assert!(dir.is_reverse());
        assert_eq!(dir.flip(), ScrollDirection::Forward);
    }

    #[test]
    fn test_scroll_direction_default() {
        assert_eq!(ScrollDirection::default(), ScrollDirection::Idle);
    }
}
