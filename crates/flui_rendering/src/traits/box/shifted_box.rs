//! Shifted box trait for custom child positioning

use crate::traits::r#box::SingleChildRenderBox;
use crate::traits::{BoxHitTestResult, PaintingContext};
use flui_types::Offset;

/// Trait for render boxes that position their child at a custom offset
///
/// RenderShiftedBox is used for render objects that:
/// 1. Can change the size relationship between parent and child
/// 2. Position the child at a specific offset within the parent
///
/// # Examples
///
/// - **RenderPadding**: Parent is larger, child offset by padding
/// - **RenderAlign**: Child positioned based on alignment
/// - **RenderBaseline**: Child positioned relative to baseline
///
/// # Ambassador Support
///
/// ```ignore
/// use ambassador::Delegate;
///
/// #[derive(Delegate)]
/// #[delegate(SingleChildRenderBox, target = "shifted")]
/// struct RenderPadding {
///     shifted: ShiftedBox,
///     padding: EdgeInsets,
/// }
///
/// impl RenderShiftedBox for RenderPadding {
///     fn child_offset(&self) -> Offset {
///         *self.shifted.offset()
///     }
/// }
/// ```
///
/// # Required Implementation
///
/// You must implement `child_offset()` to specify where the child is positioned.
pub trait RenderShiftedBox: SingleChildRenderBox {
    // Note: child() and child_mut() are inherited from SingleChildRenderBox
    // No need to duplicate them here to avoid ambiguity

    // ============================================================
    // SPECIFIC to RenderShiftedBox
    // ============================================================

    /// Returns the offset at which the child is positioned
    ///
    /// This offset is relative to the parent's top-left corner.
    fn child_offset(&self) -> Offset;

    // ============================================================
    // DUPLICATED from RenderBox (for delegation) with custom behavior
    // ============================================================

    /// Hit testing accounts for child offset
    fn hit_test_children(&self, result: &mut dyn BoxHitTestResult, position: Offset) -> bool {
        if let Some(child) = self.child() {
            let child_position = position - self.child_offset();
            child.hit_test(result, child_position)
        } else {
            false
        }
    }

    /// Painting uses child offset
    fn paint(&self, context: &mut dyn PaintingContext, offset: Offset) {
        if let Some(child) = self.child() {
            context.paint_child(child, offset + self.child_offset());
        }
    }
}
