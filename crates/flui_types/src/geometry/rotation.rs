//! Rotation types

/// Quarter turns for rotation (0°, 90°, 180°, 270°).
///
/// Used for rotating UI elements by multiples of 90 degrees.
///
/// # Examples
///
/// ```
/// use flui_types::geometry::QuarterTurns;
///
/// let rotation = QuarterTurns::One;  // 90° clockwise
/// assert!(rotation.swaps_dimensions());
///
/// let from_int = QuarterTurns::from_int(5);  // Wraps to 1 (90°)
/// assert_eq!(from_int, QuarterTurns::One);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum QuarterTurns {
    /// No rotation (0°)
    #[default]
    Zero = 0,
    /// 90° clockwise
    One = 1,
    /// 180° rotation
    Two = 2,
    /// 270° clockwise (90° counter-clockwise)
    Three = 3,
}

impl QuarterTurns {
    /// Create from integer (modulo 4).
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::QuarterTurns;
    ///
    /// assert_eq!(QuarterTurns::from_int(0), QuarterTurns::Zero);
    /// assert_eq!(QuarterTurns::from_int(5), QuarterTurns::One);  // 5 % 4 = 1
    /// assert_eq!(QuarterTurns::from_int(-1), QuarterTurns::Three);  // Wraps correctly
    /// ```
    #[must_use]
    pub fn from_int(turns: i32) -> Self {
        match turns.rem_euclid(4) {
            0 => QuarterTurns::Zero,
            1 => QuarterTurns::One,
            2 => QuarterTurns::Two,
            3 => QuarterTurns::Three,
            _ => unreachable!(),
        }
    }

    /// Get as integer value (0-3).
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::QuarterTurns;
    ///
    /// assert_eq!(QuarterTurns::Two.as_int(), 2);
    /// ```
    #[inline]
    #[must_use]
    pub const fn as_int(self) -> i32 {
        self as i32
    }

    /// Check if this rotation swaps width and height dimensions.
    ///
    /// Returns `true` for 90° and 270° rotations.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::QuarterTurns;
    ///
    /// assert!(!QuarterTurns::Zero.swaps_dimensions());
    /// assert!(QuarterTurns::One.swaps_dimensions());   // 90°
    /// assert!(!QuarterTurns::Two.swaps_dimensions());  // 180°
    /// assert!(QuarterTurns::Three.swaps_dimensions()); // 270°
    /// ```
    #[inline]
    #[must_use]
    pub const fn swaps_dimensions(self) -> bool {
        matches!(self, QuarterTurns::One | QuarterTurns::Three)
    }

    /// Get the angle in degrees.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::QuarterTurns;
    ///
    /// assert_eq!(QuarterTurns::One.degrees(), 90.0);
    /// assert_eq!(QuarterTurns::Two.degrees(), 180.0);
    /// ```
    #[inline]
    #[must_use]
    pub const fn degrees(self) -> f32 {
        (self as i32 * 90) as f32
    }

    /// Get the angle in radians.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::geometry::QuarterTurns;
    ///
    /// let radians = QuarterTurns::One.radians();
    /// assert!((radians - std::f32::consts::FRAC_PI_2).abs() < 0.0001);
    /// ```
    #[inline]
    #[must_use]
    pub fn radians(self) -> f32 {
        self.degrees().to_radians()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quarter_turns_from_int() {
        assert_eq!(QuarterTurns::from_int(0), QuarterTurns::Zero);
        assert_eq!(QuarterTurns::from_int(1), QuarterTurns::One);
        assert_eq!(QuarterTurns::from_int(2), QuarterTurns::Two);
        assert_eq!(QuarterTurns::from_int(3), QuarterTurns::Three);
        assert_eq!(QuarterTurns::from_int(4), QuarterTurns::Zero);
        assert_eq!(QuarterTurns::from_int(5), QuarterTurns::One);
        assert_eq!(QuarterTurns::from_int(-1), QuarterTurns::Three);
    }

    #[test]
    fn test_quarter_turns_as_int() {
        assert_eq!(QuarterTurns::Zero.as_int(), 0);
        assert_eq!(QuarterTurns::One.as_int(), 1);
        assert_eq!(QuarterTurns::Two.as_int(), 2);
        assert_eq!(QuarterTurns::Three.as_int(), 3);
    }

    #[test]
    fn test_quarter_turns_swaps_dimensions() {
        assert!(!QuarterTurns::Zero.swaps_dimensions());
        assert!(QuarterTurns::One.swaps_dimensions());
        assert!(!QuarterTurns::Two.swaps_dimensions());
        assert!(QuarterTurns::Three.swaps_dimensions());
    }

    #[test]
    fn test_quarter_turns_degrees() {
        assert_eq!(QuarterTurns::Zero.degrees(), 0.0);
        assert_eq!(QuarterTurns::One.degrees(), 90.0);
        assert_eq!(QuarterTurns::Two.degrees(), 180.0);
        assert_eq!(QuarterTurns::Three.degrees(), 270.0);
    }

    #[test]
    fn test_quarter_turns_default() {
        assert_eq!(QuarterTurns::default(), QuarterTurns::Zero);
    }
}
