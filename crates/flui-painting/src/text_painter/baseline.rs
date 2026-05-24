//! `TextBaseline` -- the baseline to use for aligning text.
//!
//! Mythos chain U7 extracted this standalone enum from the 990-LOC
//! `text_painter.rs` god module.
//!
//! # Divergence from `flui_types::layout::TextBaseline`
//!
//! `flui-types` defines a sibling [`TextBaseline`] enum with the same
//! `Alphabetic` + `Ideographic` variants (and matching default), so a
//! `pub use` re-export would tighten the workspace's type
//! vocabulary. We deliberately keep the local definition for now
//! because the painting/typography surface relies on `Copy + Eq +
//! Hash` (the enum is passed by value across hot paths and is used
//! as a map key in downstream cache layers), while the
//! `flui-types` definition only derives `Clone + PartialEq`. Replacing
//! this enum is a workspace-level decision: either widen the
//! `flui-types` derives or take a small breaking change on the
//! painting API.
//!
//! Tracked in `crates/flui-painting/ARCHITECTURE.md ## Outstanding
//! refactors`.
///
/// The baseline to use for aligning text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum TextBaseline {
    // PORT-CHECK-OK-SP3: pre-existing parallel definition; consolidation tracked
    /// The alphabetic baseline (bottom of letters like 'x').
    #[default]
    Alphabetic,
    /// The ideographic baseline (bottom of CJK characters).
    Ideographic,
}
