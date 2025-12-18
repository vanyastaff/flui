//! Direction types for scrollable content.
//!
//! Defines how content grows and flows in scrollable areas.

use std::fmt;

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

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

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
