//! The baseline a line of text is aligned on.

/// A horizontal line used for aligning text.
///
/// Baseline-aligned layout (e.g. `CrossAxisAlignment::Baseline`) lines up
/// children along the selected baseline kind rather than their box edges.
#[derive(Default, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(flui_foundation::inspect::Inspect)]
#[expect(
    clippy::exhaustive_enums,
    reason = "each baseline has its own measurement and cache"
)]
pub enum TextBaseline {
    /// The alphabetic baseline, the one Latin, Cyrillic and Greek glyphs sit on.
    #[default]
    Alphabetic,

    /// Ideographic baseline (used for CJK scripts).
    ///
    /// This is the baseline used for Chinese, Japanese, and Korean scripts.
    /// In these scripts, the baseline is typically at the bottom of the
    /// character box.
    Ideographic,
}
