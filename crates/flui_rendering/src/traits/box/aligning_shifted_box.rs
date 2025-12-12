//! Aligning shifted box trait for alignment-based positioning

use crate::traits::box::RenderShiftedBox;
use crate::geometry::Size;
use flui_types::{Alignment, Offset};

/// Trait for render boxes that position children using alignment
///
/// RenderAligningShiftedBox extends RenderShiftedBox with alignment support.
/// It's used for objects like:
/// - **RenderAlign**: Aligns child within available space
/// - **RenderCenter**: Centers child (alignment = center)
/// - **RenderPositioned**: Positioned with alignment in Stack
///
/// # Alignment Resolution
///
/// The child offset is computed from:
/// 1. Alignment value (e.g., topLeft, center, bottomRight)
/// 2. Child size
/// 3. Parent size
/// 4. Optional width/height factors
///
/// # Ambassador Support
///
/// ```ignore
/// use ambassador::Delegate;
///
/// #[derive(Delegate)]
/// #[delegate(SingleChildRenderBox, target = "aligning")]
/// struct RenderAlign {
///     aligning: AligningBox,
/// }
///
/// impl RenderAligningShiftedBox for RenderAlign {
///     fn alignment(&self) -> Alignment {
///         self.aligning.alignment()
///     }
///
///     fn width_factor(&self) -> Option<f32> {
///         self.aligning.width_factor()
///     }
///
///     fn height_factor(&self) -> Option<f32> {
///         self.aligning.height_factor()
///     }
/// }
///
/// impl RenderShiftedBox for RenderAlign {
///     fn child_offset(&self) -> Offset {
///         *self.aligning.offset()
///     }
/// }
/// ```
#[ambassador::delegatable_trait]
pub trait RenderAligningShiftedBox: RenderShiftedBox {
    /// Returns the alignment to use for positioning the child
    fn alignment(&self) -> Alignment;

    /// Returns the width factor (if any)
    ///
    /// If Some(f), the parent's width will be `child_width * f`.
    /// If None, the parent's width will be as large as possible.
    fn width_factor(&self) -> Option<f32> {
        None
    }

    /// Returns the height factor (if any)
    ///
    /// If Some(f), the parent's height will be `child_height * f`.
    /// If None, the parent's height will be as large as possible.
    fn height_factor(&self) -> Option<f32> {
        None
    }

    /// Resolves alignment to compute child offset
    ///
    /// This helper method computes the offset based on:
    /// - The alignment value
    /// - Child size
    /// - Parent size
    ///
    /// # Formula
    ///
    /// ```text
    /// offset.x = (parent_width - child_width) * alignment.x
    /// offset.y = (parent_height - child_height) * alignment.y
    /// ```
    ///
    /// Where alignment components range from -1.0 (left/top) to 1.0 (right/bottom),
    /// with 0.0 being center.
    fn resolve_alignment(&self, child_size: Size, parent_size: Size) -> Offset {
        let alignment = self.alignment();

        // Alignment ranges from -1.0 to 1.0
        // Convert to range 0.0 to 1.0 for offset calculation
        let x = (parent_size.width - child_size.width) * ((alignment.x + 1.0) / 2.0);
        let y = (parent_size.height - child_size.height) * ((alignment.y + 1.0) / 2.0);

        Offset::new(x, y)
    }
}

// Blanket implementation: RenderAligningShiftedBox -> RenderShiftedBox
impl<T: RenderAligningShiftedBox> RenderShiftedBox for T {
    fn child_offset(&self) -> Offset {
        RenderAligningShiftedBox::child_offset(self)
    }
}
