//! RenderShiftedBox trait - single child with custom offset.
//!
//! This module provides the `RenderShiftedBox` trait which corresponds to
//! Flutter's `RenderShiftedBox` class - an abstract class for one-child-layout
//! render boxes that provide control over the child's position.

use flui_types::Offset;

use super::{BoxHitTestResult, SingleChildRenderBox, TextBaseline};
use crate::constraints::BoxConstraints;
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
///
/// # Default Implementations
///
/// All methods have default implementations that delegate to the child
/// (if present) with appropriate offset adjustments:
///
/// - Intrinsic measurements: delegate directly to child
/// - Baseline calculations: delegate to child and add vertical offset
/// - Paint: paint child at the child offset
/// - Hit testing: test child with position adjusted by offset
pub trait RenderShiftedBox: SingleChildRenderBox {
    /// Returns the offset at which the child is positioned.
    ///
    /// This is typically stored in the child's `BoxParentData.offset` field
    /// and set during `perform_layout()`.
    fn child_offset(&self) -> Offset;

    // ===== Intrinsic Dimension Methods =====

    /// Computes the minimum intrinsic width by delegating to child.
    ///
    /// Returns 0.0 if there is no child.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderShiftedBox.computeMinIntrinsicWidth`.
    fn shifted_compute_min_intrinsic_width(&self, height: f32) -> f32 {
        self.child()
            .map(|c| c.get_min_intrinsic_width(height))
            .unwrap_or(0.0)
    }

    /// Computes the maximum intrinsic width by delegating to child.
    ///
    /// Returns 0.0 if there is no child.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderShiftedBox.computeMaxIntrinsicWidth`.
    fn shifted_compute_max_intrinsic_width(&self, height: f32) -> f32 {
        self.child()
            .map(|c| c.get_max_intrinsic_width(height))
            .unwrap_or(0.0)
    }

    /// Computes the minimum intrinsic height by delegating to child.
    ///
    /// Returns 0.0 if there is no child.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderShiftedBox.computeMinIntrinsicHeight`.
    fn shifted_compute_min_intrinsic_height(&self, width: f32) -> f32 {
        self.child()
            .map(|c| c.get_min_intrinsic_height(width))
            .unwrap_or(0.0)
    }

    /// Computes the maximum intrinsic height by delegating to child.
    ///
    /// Returns 0.0 if there is no child.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderShiftedBox.computeMaxIntrinsicHeight`.
    fn shifted_compute_max_intrinsic_height(&self, width: f32) -> f32 {
        self.child()
            .map(|c| c.get_max_intrinsic_height(width))
            .unwrap_or(0.0)
    }

    // ===== Baseline Methods =====

    /// Computes the distance to the actual baseline.
    ///
    /// Delegates to child and adds the child's vertical offset (dy).
    /// This is important because the child is shifted from the parent's origin.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderShiftedBox.computeDistanceToActualBaseline`.
    fn shifted_compute_distance_to_actual_baseline(&self, baseline: TextBaseline) -> Option<f32> {
        if let Some(child) = self.child() {
            if let Some(result) = child.get_distance_to_actual_baseline(baseline) {
                // Add the child's vertical offset
                return Some(result + self.child_offset().dy);
            }
        }
        None
    }

    /// Computes the dry baseline (without performing layout).
    ///
    /// Delegates to child. Subclasses that apply transforms should override
    /// to add appropriate offsets.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderShiftedBox.computeDryBaseline`.
    #[allow(unused_variables)]
    fn shifted_compute_dry_baseline(
        &self,
        constraints: BoxConstraints,
        baseline: TextBaseline,
    ) -> Option<f32> {
        self.child()
            .and_then(|c| c.get_dry_baseline(constraints, baseline))
    }

    // ===== Paint Methods =====

    /// Paints the child at its offset.
    ///
    /// This is the standard paint implementation for shifted boxes.
    /// The child is painted at `offset + child_offset()`.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderShiftedBox.paint`.
    fn shifted_paint(&self, context: &mut PaintingContext, offset: Offset) {
        if let Some(child) = self.child() {
            context.paint_child(child, offset + self.child_offset());
        }
    }

    // ===== Hit Testing Methods =====

    /// Hit tests children, adjusting for child offset.
    ///
    /// Transforms the position by subtracting the child offset before testing the child.
    /// This ensures that hit testing correctly accounts for the child's position.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderShiftedBox.hitTestChildren`.
    fn shifted_hit_test_children(&self, result: &mut BoxHitTestResult, position: Offset) -> bool {
        if let Some(child) = self.child() {
            let child_offset = self.child_offset();
            // Transform position to child's coordinate space
            let transformed_position = position - child_offset;
            child.hit_test(result, transformed_position)
        } else {
            false
        }
    }
}
