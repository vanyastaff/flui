//! Direction types for scrollable content.
//!
//! Defines how content grows and flows in scrollable areas.

use std::fmt;

use crate::constraints::AxisDirection;

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
    pub const fn apply_to(self, value: f64) -> f64 {
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
    /// Forward growth keeps the axis direction, while reverse growth uses its
    /// opposite.
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
    pub const fn multiplier(self) -> f64 {
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
/// Reverse growth flips the scroll direction; forward growth preserves it.
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
