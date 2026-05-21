//! `DisplayListStats` -- structured counts of different command types
//! in a `DisplayList`.
//!
//! Mythos chain U5 extracted these from the 2,434-LOC
//! `display_list.rs` god module. Stats are computed on demand via
//! `DisplayListExt::stats()` (one iteration over commands).

/// Detailed statistics about a [`crate::display_list::DisplayList`]'s
/// contents.
///
/// Provides counts of different command types to help analyze
/// rendering complexity and optimize performance.
///
/// # Field Categories
///
/// - **Total**: All commands.
/// - **By Category**: `draw`, `clip`, `effect`, `layer`.
/// - **By Content Type**: `shapes`, `images`, `text` (subsets of
///   `draw`).
/// - **Other**: `hit_regions`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DisplayListStats {
    /// Total number of commands.
    pub total: usize,
    /// Number of drawing commands.
    pub draw: usize,
    /// Number of clipping commands.
    pub clip: usize,
    /// Number of effect commands.
    pub effect: usize,
    /// Number of layer commands.
    pub layer: usize,
    /// Number of shape commands (subset of draw).
    pub shapes: usize,
    /// Number of image/texture commands (subset of draw).
    pub images: usize,
    /// Number of text commands (subset of draw).
    pub text: usize,
    /// Number of hit regions.
    pub hit_regions: usize,
}

impl DisplayListStats {
    /// Creates a new statistics object with all counts set to zero.
    pub const fn zero() -> Self {
        Self {
            total: 0,
            draw: 0,
            clip: 0,
            effect: 0,
            layer: 0,
            shapes: 0,
            images: 0,
            text: 0,
            hit_regions: 0,
        }
    }

    /// Creates a new statistics object with the specified counts.
    #[allow(clippy::too_many_arguments)]
    pub const fn new(
        total: usize,
        draw: usize,
        clip: usize,
        effect: usize,
        layer: usize,
        shapes: usize,
        images: usize,
        text: usize,
        hit_regions: usize,
    ) -> Self {
        Self {
            total,
            draw,
            clip,
            effect,
            layer,
            shapes,
            images,
            text,
            hit_regions,
        }
    }
}

impl std::fmt::Display for DisplayListStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "DisplayList: {} commands ({} shapes, {} images, {} text, {} clips, {} effects, {} layers), {} hit regions",
            self.total,
            self.shapes,
            self.images,
            self.text,
            self.clip,
            self.effect,
            self.layer,
            self.hit_regions
        )
    }
}
