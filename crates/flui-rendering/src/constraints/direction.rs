//! Direction types for scrollable content.
//!
//! Defines how content grows and flows in scrollable areas.

use std::fmt;

use flui_types::layout::AxisDirection;

/// Direction in which content grows within a scrollable area.
///
/// Determines whether new content is added at the end (Forward) or
/// beginning (Reverse) of the content sequence.
///
/// # Examples
///
/// ```ignore
/// use flui_rendering::constraints::GrowthDirection;
///
/// let dir = GrowthDirection::Forward;
/// assert_eq!(dir.multiplier(), 1.0);
///
/// let reversed = dir.flip();
/// assert_eq!(reversed.multiplier(), -1.0);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum GrowthDirection {
    /// Content grows in the forward direction (normal reading order).
    #[default]
    Forward,

    /// Content grows in the reverse direction (opposite of reading order).
    Reverse,
}

impl GrowthDirection {
    /// Returns whether this is forward growth.
    #[inline]
    #[must_use]
    pub const fn is_forward(self) -> bool {
        matches!(self, GrowthDirection::Forward)
    }

    /// Returns whether this is reverse growth.
    #[inline]
    #[must_use]
    pub const fn is_reverse(self) -> bool {
        matches!(self, GrowthDirection::Reverse)
    }

    /// Returns the opposite growth direction.
    #[inline]
    #[must_use]
    pub const fn flip(self) -> Self {
        match self {
            GrowthDirection::Forward => GrowthDirection::Reverse,
            GrowthDirection::Reverse => GrowthDirection::Forward,
        }
    }

    /// Applies growth direction to a value.
    ///
    /// Returns the value unchanged for Forward, negated for Reverse.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// assert_eq!(GrowthDirection::Forward.apply_to(10.0), 10.0);
    /// assert_eq!(GrowthDirection::Reverse.apply_to(10.0), -10.0);
    /// ```
    #[inline]
    #[must_use]
    pub const fn apply_to(self, value: f32) -> f32 {
        match self {
            GrowthDirection::Forward => value,
            GrowthDirection::Reverse => -value,
        }
    }

    /// Applies growth direction to an integer value.
    #[inline]
    #[must_use]
    pub const fn apply_to_i32(self, value: i32) -> i32 {
        match self {
            GrowthDirection::Forward => value,
            GrowthDirection::Reverse => -value,
        }
    }

    /// Applies growth direction to an axis direction.
    ///
    /// Mirrors Flutter's `applyGrowthDirectionToAxisDirection`: forward growth
    /// keeps the axis direction, while reverse growth uses its opposite.
    #[inline]
    #[must_use]
    pub const fn apply_to_axis_direction(self, axis_direction: AxisDirection) -> AxisDirection {
        match self {
            GrowthDirection::Forward => axis_direction,
            GrowthDirection::Reverse => axis_direction.opposite(),
        }
    }

    /// Returns the directional multiplier (+1 for Forward, -1 for Reverse).
    ///
    /// Useful for calculations that need to scale by direction.
    #[inline]
    #[must_use]
    pub const fn multiplier(self) -> f32 {
        match self {
            GrowthDirection::Forward => 1.0,
            GrowthDirection::Reverse => -1.0,
        }
    }

    /// Returns the directional multiplier as an integer.
    #[inline]
    #[must_use]
    pub const fn multiplier_i32(self) -> i32 {
        match self {
            GrowthDirection::Forward => 1,
            GrowthDirection::Reverse => -1,
        }
    }
}

impl fmt::Display for GrowthDirection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GrowthDirection::Forward => write!(f, "forward"),
            GrowthDirection::Reverse => write!(f, "reverse"),
        }
    }
}

/// Applies growth direction to the user scroll direction.
///
/// Mirrors Flutter's composition inside `RenderViewport.layoutChildSequence`:
/// reverse growth flips the scroll direction; forward growth preserves it.
#[inline]
#[must_use]
pub fn apply_growth_direction_to_scroll_direction(
    scroll_direction: crate::view::ScrollDirection,
    growth_direction: GrowthDirection,
) -> crate::view::ScrollDirection {
    use crate::view::ScrollDirection;

    match growth_direction {
        GrowthDirection::Forward => scroll_direction,
        GrowthDirection::Reverse => match scroll_direction {
            ScrollDirection::Idle => ScrollDirection::Idle,
            ScrollDirection::Forward => ScrollDirection::Reverse,
            ScrollDirection::Reverse => ScrollDirection::Forward,
        },
    }
}

/// Whether sliver content is laid out in the "right way up" reading direction.
///
/// Mirrors Flutter's `RenderSliverHelpers.rightWayUp`.
#[inline]
#[must_use]
pub const fn right_way_up(
    axis_direction: AxisDirection,
    growth_direction: GrowthDirection,
) -> bool {
    let reversed = axis_direction.is_reversed();
    match growth_direction {
        GrowthDirection::Forward => !reversed,
        GrowthDirection::Reverse => reversed,
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn test_forward() {
        let dir = GrowthDirection::Forward;
        assert!(dir.is_forward());
        assert!(!dir.is_reverse());
        assert_eq!(dir.flip(), GrowthDirection::Reverse);
        assert_eq!(dir.multiplier(), 1.0);
        assert_eq!(dir.multiplier_i32(), 1);
    }

    #[test]
    fn test_reverse() {
        let dir = GrowthDirection::Reverse;
        assert!(!dir.is_forward());
        assert!(dir.is_reverse());
        assert_eq!(dir.flip(), GrowthDirection::Forward);
        assert_eq!(dir.multiplier(), -1.0);
        assert_eq!(dir.multiplier_i32(), -1);
    }

    #[test]
    fn test_default() {
        assert_eq!(GrowthDirection::default(), GrowthDirection::Forward);
    }

    #[test]
    fn test_apply_to() {
        assert_eq!(GrowthDirection::Forward.apply_to(10.0), 10.0);
        assert_eq!(GrowthDirection::Reverse.apply_to(10.0), -10.0);

        assert_eq!(GrowthDirection::Forward.apply_to_i32(10), 10);
        assert_eq!(GrowthDirection::Reverse.apply_to_i32(10), -10);
    }

    #[test]
    fn test_apply_to_axis_direction_all_pairs() {
        use flui_types::layout::AxisDirection::{
            BottomToTop, LeftToRight, RightToLeft, TopToBottom,
        };

        let cases = [
            (TopToBottom, GrowthDirection::Forward, TopToBottom),
            (TopToBottom, GrowthDirection::Reverse, BottomToTop),
            (BottomToTop, GrowthDirection::Forward, BottomToTop),
            (BottomToTop, GrowthDirection::Reverse, TopToBottom),
            (LeftToRight, GrowthDirection::Forward, LeftToRight),
            (LeftToRight, GrowthDirection::Reverse, RightToLeft),
            (RightToLeft, GrowthDirection::Forward, RightToLeft),
            (RightToLeft, GrowthDirection::Reverse, LeftToRight),
        ];

        for (axis, growth, expected) in cases {
            assert_eq!(
                growth.apply_to_axis_direction(axis),
                expected,
                "axis={axis:?}, growth={growth:?}",
            );
        }
    }

    #[test]
    fn test_apply_growth_direction_to_scroll_direction() {
        use crate::view::ScrollDirection;

        assert_eq!(
            apply_growth_direction_to_scroll_direction(
                ScrollDirection::Forward,
                GrowthDirection::Forward
            ),
            ScrollDirection::Forward,
        );
        assert_eq!(
            apply_growth_direction_to_scroll_direction(
                ScrollDirection::Forward,
                GrowthDirection::Reverse
            ),
            ScrollDirection::Reverse,
        );
        assert_eq!(
            apply_growth_direction_to_scroll_direction(
                ScrollDirection::Reverse,
                GrowthDirection::Reverse
            ),
            ScrollDirection::Forward,
        );
        assert_eq!(
            apply_growth_direction_to_scroll_direction(
                ScrollDirection::Idle,
                GrowthDirection::Reverse
            ),
            ScrollDirection::Idle,
        );
    }

    #[test]
    fn test_right_way_up_all_pairs() {
        use flui_types::layout::AxisDirection::{
            BottomToTop, LeftToRight, RightToLeft, TopToBottom,
        };

        let cases = [
            (TopToBottom, GrowthDirection::Forward, true),
            (TopToBottom, GrowthDirection::Reverse, false),
            (BottomToTop, GrowthDirection::Forward, false),
            (BottomToTop, GrowthDirection::Reverse, true),
            (LeftToRight, GrowthDirection::Forward, true),
            (LeftToRight, GrowthDirection::Reverse, false),
            (RightToLeft, GrowthDirection::Forward, false),
            (RightToLeft, GrowthDirection::Reverse, true),
        ];

        for (axis, growth, expected) in cases {
            assert_eq!(
                right_way_up(axis, growth),
                expected,
                "axis={axis:?}, growth={growth:?}",
            );
        }
    }

    #[test]
    fn test_hash_and_eq() {
        let f1 = GrowthDirection::Forward;
        let f2 = GrowthDirection::Forward;
        let r = GrowthDirection::Reverse;

        assert_eq!(f1, f2);
        assert_ne!(f1, r);

        let mut set = HashSet::new();
        set.insert(f1);
        assert!(set.contains(&f2));
        assert!(!set.contains(&r));
    }

    #[test]
    fn test_display() {
        assert_eq!(GrowthDirection::Forward.to_string(), "forward");
        assert_eq!(GrowthDirection::Reverse.to_string(), "reverse");
    }
}
