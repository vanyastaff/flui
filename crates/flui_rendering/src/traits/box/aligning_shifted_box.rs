//! RenderAligningShiftedBox trait - shifted box with alignment support.

use flui_types::{Alignment, Offset, Size};

use super::RenderShiftedBox;

/// Trait for shifted boxes that use alignment to position the child.
///
/// # Flutter Equivalence
///
/// This corresponds to `RenderAligningShiftedBox` in Flutter.
///
/// # Usage
///
/// Use when you need to:
/// - Align a child within a larger parent area
/// - Apply width/height factors to size relative to child
/// - Support various alignment configurations
pub trait RenderAligningShiftedBox: RenderShiftedBox {
    /// Returns the alignment used to position the child.
    fn alignment(&self) -> Alignment;

    /// Returns the width factor, if any.
    ///
    /// When set, the width is `child_width * width_factor`.
    fn width_factor(&self) -> Option<f32>;

    /// Returns the height factor, if any.
    ///
    /// When set, the height is `child_height * height_factor`.
    fn height_factor(&self) -> Option<f32>;

    /// Computes the child offset based on alignment.
    ///
    /// Call this after laying out the child to compute its offset.
    fn compute_aligned_offset(&self, parent_size: Size, child_size: Size) -> Offset {
        self.alignment().along_offset(Offset::new(
            parent_size.width - child_size.width,
            parent_size.height - child_size.height,
        ))
    }
}
