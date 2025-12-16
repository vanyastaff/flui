//! RenderShiftedBox trait - single child with custom offset.
//!
//! This module provides the `RenderShiftedBox` trait which corresponds to
//! Flutter's `RenderShiftedBox` class - an abstract class for one-child-layout
//! render boxes that provide control over the child's position.
//!
//! # Flutter Architecture
//!
//! In Flutter, `RenderShiftedBox`:
//! - Extends `RenderBox` with `RenderObjectWithChildMixin<RenderBox>`
//! - Reads child offset from `child.parentData.offset` (BoxParentData)
//! - Parent sets offset during `performLayout()` via `child.parentData.offset = ...`
//!
//! # Key Points
//!
//! - Offset is stored in CHILD's parentData, NOT in parent
//! - Parent writes offset, child stores it
//! - Paint and hitTest read offset from child.parentData

use ambassador::delegatable_trait;
use flui_types::Offset;

use super::{BoxHitTestResult, RenderBox, SingleChildRenderBox, TextBaseline};
use crate::constraints::BoxConstraints;
use crate::parent_data::BoxParentData;
use crate::pipeline::PaintingContext;

// ============================================================================
// Helper function for setting child offset
// ============================================================================

/// Sets the offset in a child's BoxParentData.
///
/// This is a free function because trait methods require `self`.
/// Call this during `performLayout()` to position the child.
///
/// # Flutter Equivalence
///
/// In Flutter this is done via:
/// ```dart
/// final childParentData = child!.parentData! as BoxParentData;
/// childParentData.offset = Offset(padding.left, padding.top);
/// ```
///
/// # Example
///
/// ```ignore
/// fn perform_layout(&mut self, constraints: BoxConstraints) -> Size {
///     if let Some(child) = self.child_mut() {
///         let child_size = child.perform_layout(inner_constraints);
///         set_child_offset(child, Offset::new(padding.left, padding.top));
///     }
///     // ...
/// }
/// ```
pub fn set_child_offset(child: &mut dyn RenderBox, offset: Offset) {
    if let Some(pd) = child.parent_data_mut() {
        if let Some(bpd) = pd.as_any_mut().downcast_mut::<BoxParentData>() {
            bpd.offset = offset;
        }
    }
}

/// Gets the offset from a child's BoxParentData.
///
/// Returns `Offset::ZERO` if the child has no BoxParentData.
pub fn get_child_offset(child: &dyn RenderBox) -> Offset {
    child
        .parent_data()
        .and_then(|pd| pd.as_any().downcast_ref::<BoxParentData>())
        .map(|bpd| bpd.offset)
        .unwrap_or(Offset::ZERO)
}

// ============================================================================
// RenderShiftedBox Trait
// ============================================================================

/// Trait for render boxes that position their child at a custom offset.
///
/// RenderShiftedBox is used for render objects that:
/// - Apply padding or margins
/// - Position a child within a larger area
/// - Need to adjust hit testing by an offset
///
/// # Flutter Equivalence
///
/// This corresponds to `RenderShiftedBox` in Flutter's `rendering/shifted_box.dart`.
///
/// # Offset Storage (IMPORTANT)
///
/// Following Flutter's architecture:
/// - Offset is stored in `child.parentData.offset` (BoxParentData)
/// - Parent sets offset during `performLayout()`
/// - `paint()` and `hitTestChildren()` read from child.parentData
///
/// ```dart
/// // Flutter's RenderShiftedBox.paint:
/// void paint(PaintingContext context, Offset offset) {
///   if (child != null) {
///     final childParentData = child!.parentData! as BoxParentData;
///     context.paintChild(child!, childParentData.offset + offset);
///   }
/// }
/// ```
///
/// # Default Implementations
///
/// All methods have default implementations that delegate to the child
/// with appropriate offset adjustments read from child's parentData.
#[delegatable_trait]
pub trait RenderShiftedBox: SingleChildRenderBox {
    // ========================================================================
    // Child Offset Access
    // ========================================================================

    /// Returns the child's offset from the child's parentData.
    ///
    /// This reads `child.parentData.offset` (BoxParentData).
    /// Returns `Offset::ZERO` if no child or parentData is not BoxParentData.
    ///
    /// # Flutter Equivalence
    ///
    /// In Flutter this is accessed via:
    /// ```dart
    /// final childParentData = child!.parentData! as BoxParentData;
    /// final offset = childParentData.offset;
    /// ```
    fn child_parent_data_offset(&self) -> Offset {
        self.child()
            .and_then(|child| {
                child
                    .parent_data()
                    .and_then(|pd| pd.as_any().downcast_ref::<BoxParentData>())
                    .map(|bpd| bpd.offset)
            })
            .unwrap_or(Offset::ZERO)
    }

    // ========================================================================
    // Intrinsic Dimension Methods
    // ========================================================================

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

    // ========================================================================
    // Baseline Methods
    // ========================================================================

    /// Computes the distance to the actual baseline.
    ///
    /// Delegates to child and adds the child's vertical offset (dy) from parentData.
    /// This is important because the child is shifted from the parent's origin.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderShiftedBox.computeDistanceToActualBaseline`:
    /// ```dart
    /// double? computeDistanceToActualBaseline(TextBaseline baseline) {
    ///   double? result;
    ///   if (child != null) {
    ///     result = child!.getDistanceToActualBaseline(baseline);
    ///     final childParentData = child!.parentData! as BoxParentData;
    ///     if (result != null) {
    ///       result += childParentData.offset.dy;
    ///     }
    ///   }
    ///   return result;
    /// }
    /// ```
    fn shifted_compute_distance_to_actual_baseline(&self, baseline: TextBaseline) -> Option<f32> {
        if let Some(child) = self.child() {
            if let Some(result) = child.get_distance_to_actual_baseline(baseline) {
                // Add the child's vertical offset from parentData
                return Some(result + self.child_parent_data_offset().dy);
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

    // ========================================================================
    // Paint Methods
    // ========================================================================

    /// Paints the child at its offset from parentData.
    ///
    /// This is the standard paint implementation for shifted boxes.
    /// The child is painted at `offset + child.parentData.offset`.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderShiftedBox.paint`:
    /// ```dart
    /// void paint(PaintingContext context, Offset offset) {
    ///   if (child != null) {
    ///     final childParentData = child!.parentData! as BoxParentData;
    ///     context.paintChild(child!, childParentData.offset + offset);
    ///   }
    /// }
    /// ```
    fn shifted_paint(&self, context: &mut PaintingContext, offset: Offset) {
        if let Some(child) = self.child() {
            let child_offset = self.child_parent_data_offset();
            context.paint_child(child, offset + child_offset);
        }
    }

    // ========================================================================
    // Hit Testing Methods
    // ========================================================================

    /// Hit tests children, adjusting for child offset from parentData.
    ///
    /// Uses `result.addWithPaintOffset` to transform the position by
    /// the child's offset before testing the child.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to Flutter's `RenderShiftedBox.hitTestChildren`:
    /// ```dart
    /// bool hitTestChildren(BoxHitTestResult result, {required Offset position}) {
    ///   if (child != null) {
    ///     final childParentData = child!.parentData! as BoxParentData;
    ///     return result.addWithPaintOffset(
    ///       offset: childParentData.offset,
    ///       position: position,
    ///       hitTest: (result, transformed) => child!.hitTest(result, position: transformed),
    ///     );
    ///   }
    ///   return false;
    /// }
    /// ```
    fn shifted_hit_test_children(&self, result: &mut BoxHitTestResult, position: Offset) -> bool {
        if let Some(child) = self.child() {
            let child_offset = self.child_parent_data_offset();
            result.add_with_paint_offset(Some(child_offset), position, |result, transformed| {
                child.hit_test(result, transformed)
            })
        } else {
            false
        }
    }
}
