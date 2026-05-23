#[derive(Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum QuarterTurns {
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

    #[must_use]
    pub const fn as_int(self) -> i32 {
        self as i32
    }

    #[must_use]
    pub const fn swaps_dimensions(self) -> bool {
        matches!(self, QuarterTurns::One | QuarterTurns::Three)
    }

    #[must_use]
    pub const fn degrees(self) -> f32 {
        (self as i32 * 90) as f32
    }

    #[must_use]
    pub fn radians(self) -> f32 {
        self.degrees().to_radians()
    }

    #[must_use]
    pub fn to_radians(self) -> crate::Radians {
        crate::radians(self.radians())
    }
}

// Conversions
impl From<QuarterTurns> for f32 {
    #[inline]
    fn from(turns: QuarterTurns) -> Self {
        turns.radians()
    }
}

impl From<QuarterTurns> for crate::Radians {
    #[inline]
    fn from(turns: QuarterTurns) -> Self {
        turns.to_radians()
    }
}
