//! Direction types for scrolling and layout growth.
//!
//! This module provides types that describe directions for scroll views
//! and layout growth.

/// Direction in which content grows in a scrollable area.
///
/// # Flutter Equivalence
///
/// Corresponds to Flutter's `GrowthDirection` enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum GrowthDirection {
    /// Content grows in the forward direction.
    ///
    /// For vertical scrolling, this is downward.
    /// For horizontal scrolling, this is to the right (in LTR) or left (in RTL).
    #[default]
    Forward,

    /// Content grows in the reverse direction.
    ///
    /// For vertical scrolling, this is upward.
    /// For horizontal scrolling, this is to the left (in LTR) or right (in RTL).
    Reverse,
}

impl GrowthDirection {
    /// Returns whether this growth direction is forward.
    #[inline]
    #[must_use]
    pub const fn is_forward(self) -> bool {
        matches!(self, GrowthDirection::Forward)
    }

    /// Returns whether this growth direction is reverse.
    #[inline]
    #[must_use]
    pub const fn is_reverse(self) -> bool {
        matches!(self, GrowthDirection::Reverse)
    }

    /// Returns the opposite direction.
    #[inline]
    #[must_use]
    pub const fn flip(self) -> Self {
        match self {
            GrowthDirection::Forward => GrowthDirection::Reverse,
            GrowthDirection::Reverse => GrowthDirection::Forward,
        }
    }

    /// Apply growth direction to a value.
    ///
    /// Returns the value as-is for Forward, negated for Reverse.
    /// Useful for converting logical offsets to physical offsets.
    #[inline]
    #[must_use]
    pub const fn apply_to(self, value: f32) -> f32 {
        match self {
            GrowthDirection::Forward => value,
            GrowthDirection::Reverse => -value,
        }
    }
}

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
    fn test_growth_direction_apply_to() {
        assert_eq!(GrowthDirection::Forward.apply_to(10.0), 10.0);
        assert_eq!(GrowthDirection::Reverse.apply_to(10.0), -10.0);
    }
}
