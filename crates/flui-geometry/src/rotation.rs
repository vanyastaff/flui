/// A rotation expressed as a whole number of 90° clockwise turns.
///
/// Only the four axis-aligned orientations are representable, which makes
/// rotations exact (no floating-point drift) and cheap to compose.
#[derive(Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum QuarterTurns {
    /// No rotation.
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
    /// Creates a rotation from an integer number of quarter turns.
    ///
    /// The value is normalized modulo 4, so negative and out-of-range
    /// counts wrap (e.g. `-1` becomes [`QuarterTurns::Three`]).
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

    /// Returns the number of quarter turns as an integer in `0..=3`.
    #[must_use]
    pub const fn as_int(self) -> i32 {
        self as i32
    }

    /// Returns `true` if this rotation swaps width and height (90° or 270°).
    #[must_use]
    pub const fn swaps_dimensions(self) -> bool {
        matches!(self, QuarterTurns::One | QuarterTurns::Three)
    }

    /// Returns the rotation angle in degrees (0.0, 90.0, 180.0, or 270.0).
    #[must_use]
    pub const fn degrees(self) -> f64 {
        (self as i32 * 90) as f64
    }

    /// Returns the rotation angle in radians as a raw `f64`.
    #[must_use]
    pub fn radians(self) -> f64 {
        self.degrees().to_radians()
    }

    /// Returns the rotation angle in radians (same as [`Self::radians`]).
    #[must_use]
    pub fn to_radians(self) -> f64 {
        self.radians()
    }
}

// Conversions
impl From<QuarterTurns> for f64 {
    #[inline]
    fn from(turns: QuarterTurns) -> Self {
        turns.radians()
    }
}
