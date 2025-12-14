//! RenderShiftedBox trait - single child with custom offset.

use flui_types::Offset;

use super::{BoxHitTestResult, SingleChildRenderBox};
use crate::pipeline::PaintingContext;

/// Trait for render boxes that position their child at a custom offset.
///
/// RenderShiftedBox is used for render objects that:
/// - Apply padding or margins
/// - Position a child within a larger area
/// - Need to adjust hit testing by an offset
///
/// # Flutter Equivalence
///
/// This corresponds to `RenderShiftedBox` in Flutter.
///
/// # Key Difference from ProxyBox
///
/// - ProxyBox: size equals child size, no offset
/// - ShiftedBox: size may differ from child, child is at an offset
pub trait RenderShiftedBox: SingleChildRenderBox {
    /// Returns the offset at which the child is positioned.
    fn child_offset(&self) -> Offset;

    /// Paints the child at its offset.
    fn shifted_paint(&self, context: &mut PaintingContext, offset: Offset) {
        if let Some(child) = self.child() {
            context.paint_child(child, offset + self.child_offset());
        }
    }

    /// Hit tests children, adjusting for child offset.
    fn shifted_hit_test_children(&self, result: &mut BoxHitTestResult, position: Offset) -> bool {
        let child_offset = self.child_offset();
        let child_position = position - child_offset;

        self.child()
            .map(|c| c.hit_test(result, child_position))
            .unwrap_or(false)
    }
}
