//! Baseline types for text alignment

#[derive(Default, Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum TextBaseline {
    #[default]
    Alphabetic,

    /// Ideographic baseline (used for CJK scripts).
    ///
    /// This is the baseline used for Chinese, Japanese, and Korean scripts.
    /// In these scripts, the baseline is typically at the bottom of the character box.
    Ideographic,
}

impl TextBaseline {
    /// Returns true if this is the alphabetic baseline.
    #[inline]
    pub const fn is_alphabetic(&self) -> bool {
        matches!(self, Self::Alphabetic)
    }

    /// Returns true if this is the ideographic baseline.
    #[inline]
    pub const fn is_ideographic(&self) -> bool {
        matches!(self, Self::Ideographic)
    }
}
