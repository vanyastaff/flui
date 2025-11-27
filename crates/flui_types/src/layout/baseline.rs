//! Baseline types for text alignment

/// Baseline type for text alignment.
///
/// Similar to Flutter's `TextBaseline`.
///
/// # Examples
///
/// ```
/// use flui_types::layout::TextBaseline;
///
/// let alphabetic = TextBaseline::Alphabetic;
/// let ideographic = TextBaseline::Ideographic;
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum TextBaseline {
    /// Alphabetic baseline (most common for Latin scripts).
    ///
    /// This is the baseline used for most Western scripts including Latin,
    /// Greek, and Cyrillic alphabets.
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
    pub const fn is_alphabetic(&self) -> bool {
        matches!(self, Self::Alphabetic)
    }

    /// Returns true if this is the ideographic baseline.
    pub const fn is_ideographic(&self) -> bool {
        matches!(self, Self::Ideographic)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_text_baseline_types() {
        assert_ne!(TextBaseline::Alphabetic, TextBaseline::Ideographic);
        assert!(TextBaseline::Alphabetic.is_alphabetic());
        assert!(!TextBaseline::Alphabetic.is_ideographic());
        assert!(TextBaseline::Ideographic.is_ideographic());
        assert!(!TextBaseline::Ideographic.is_alphabetic());
    }

    #[test]
    fn test_text_baseline_default() {
        assert_eq!(TextBaseline::default(), TextBaseline::Alphabetic);
    }
}
