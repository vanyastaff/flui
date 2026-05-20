//! `TextBaseline` -- the baseline to use for aligning text.
//!
//! Mythos chain U7 extracted this standalone enum from the 990-LOC
//! `text_painter.rs` god module.

/// The baseline to use for aligning text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum TextBaseline {
    /// The alphabetic baseline (bottom of letters like 'x').
    #[default]
    Alphabetic,
    /// The ideographic baseline (bottom of CJK characters).
    Ideographic,
}
