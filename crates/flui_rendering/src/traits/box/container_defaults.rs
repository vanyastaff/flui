//! RenderBoxContainerDefaultsMixin - default implementations for box containers.

use super::{BoxHitTestResult, MultiChildRenderBox, RenderBox, TextBaseline};
use flui_types::Offset;

/// Trait providing default implementations for multi-child box containers.
///
/// # Flutter Equivalence
///
/// This corresponds to `RenderBoxContainerDefaultsMixin` in Flutter.
///
/// ```dart
/// mixin RenderBoxContainerDefaultsMixin<ChildType extends RenderObject,
///     ParentDataType extends ContainerBoxParentData<ChildType>>
///     on ContainerRenderObjectMixin<ChildType, ParentDataType> {
///   // Default implementations for baselines, hit testing, and painting
/// }
/// ```
///
/// # Purpose
///
/// This mixin provides sensible default implementations for common operations
/// on multi-child containers:
///
/// - **Baseline computation**: Finding the first or highest baseline among children
/// - **Hit testing**: Testing children in reverse paint order (front to back)
/// - **Painting**: Painting all children at their offsets
///
/// # Usage
///
/// Implementors can use these default methods or override them with custom behavior.
pub trait RenderBoxContainerDefaultsMixin: MultiChildRenderBox {
    // ========================================================================
    // Child Offset Access
    // ========================================================================

    /// Returns the offset of a child from its parent data.
    ///
    /// This method must be implemented by the container to provide access
    /// to the child's offset stored in its parent data.
    ///
    /// # Arguments
    ///
    /// * `child` - The child to get the offset for
    ///
    /// # Returns
    ///
    /// The offset at which the child should be painted
    fn get_child_offset(&self, child: &dyn RenderBox) -> Offset;
    // ========================================================================
    // Baseline Computation
    // ========================================================================

    /// Computes the distance to the first baseline of the first child with a baseline.
    ///
    /// Returns the distance from the top of this render box to the first baseline
    /// of the given type found among the children, or `None` if no child has
    /// a baseline of that type.
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// double? defaultComputeDistanceToFirstActualBaseline(TextBaseline baseline) {
    ///   for (final child in childrenInPaintOrder) {
    ///     final result = child.getDistanceToActualBaseline(baseline);
    ///     if (result != null) {
    ///       return result + parentData.offset.dy;
    ///     }
    ///   }
    ///   return null;
    /// }
    /// ```
    fn default_compute_distance_to_first_actual_baseline(
        &self,
        baseline: TextBaseline,
    ) -> Option<f32> {
        for child in self.children() {
            if let Some(child_baseline) = child.get_distance_to_baseline(baseline, true) {
                let child_offset = self.get_child_offset(child);
                return Some(child_baseline + child_offset.dy);
            }
        }
        None
    }

    /// Computes the distance to the highest (closest to top) baseline among all children.
    ///
    /// Returns the minimum distance from the top of this render box to any baseline
    /// of the given type found among the children, or `None` if no child has
    /// a baseline of that type.
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// double? defaultComputeDistanceToHighestActualBaseline(TextBaseline baseline) {
    ///   double? result;
    ///   for (final child in childrenInPaintOrder) {
    ///     final candidate = child.getDistanceToActualBaseline(baseline);
    ///     if (candidate != null) {
    ///       final adjusted = candidate + parentData.offset.dy;
    ///       result = result == null ? adjusted : math.min(result, adjusted);
    ///     }
    ///   }
    ///   return result;
    /// }
    /// ```
    fn default_compute_distance_to_highest_actual_baseline(
        &self,
        baseline: TextBaseline,
    ) -> Option<f32> {
        let mut result: Option<f32> = None;

        for child in self.children() {
            if let Some(child_baseline) = child.get_distance_to_baseline(baseline, true) {
                let child_offset = self.get_child_offset(child);
                let adjusted = child_baseline + child_offset.dy;
                result = Some(result.map_or(adjusted, |r| r.min(adjusted)));
            }
        }

        result
    }

    // ========================================================================
    // Hit Testing
    // ========================================================================

    /// Default hit testing implementation for box containers.
    ///
    /// Tests children in reverse order (last to first, which is typically
    /// front to back in paint order). Returns true if any child was hit.
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// bool defaultHitTestChildren(BoxHitTestResult result, {required Offset position}) {
    ///   for (final child in childrenInHitTestOrder) {
    ///     final parentData = child.parentData as ContainerBoxParentData<ChildType>;
    ///     final isHit = result.addWithPaintOffset(
    ///       offset: parentData.offset,
    ///       position: position,
    ///       hitTest: (result, transformed) => child.hitTest(result, position: transformed),
    ///     );
    ///     if (isHit) return true;
    ///   }
    ///   return false;
    /// }
    /// ```
    ///
    /// # Arguments
    ///
    /// * `result` - The hit test result to add entries to
    /// * `position` - The position to test in local coordinates
    ///
    /// # Returns
    ///
    /// `true` if any child was hit, `false` otherwise
    fn default_hit_test_children(&self, result: &mut BoxHitTestResult, position: Offset) -> bool {
        // Collect children into a vec so we can iterate in reverse
        let children: Vec<_> = self.children().collect();

        // Test in reverse order (front to back)
        for child in children.into_iter().rev() {
            let child_offset = self.get_child_offset(child);
            let transformed_position = position - child_offset;

            if child.hit_test(result, transformed_position) {
                return true;
            }
        }

        false
    }

    // ========================================================================
    // Painting
    // ========================================================================

    /// Default painting implementation for box containers.
    ///
    /// Paints all children at their respective offsets from parent data.
    ///
    /// # Arguments
    ///
    /// * `paint_fn` - A function that paints a single child at a given offset.
    ///   This is typically `context.paint_child(child, offset)`.
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// void defaultPaint(PaintingContext context, Offset offset) {
    ///   for (final child in childrenInPaintOrder) {
    ///     final parentData = child.parentData as ContainerBoxParentData<ChildType>;
    ///     context.paintChild(child, parentData.offset + offset);
    ///   }
    /// }
    /// ```
    fn default_paint<F>(&self, base_offset: Offset, mut paint_fn: F)
    where
        F: FnMut(&dyn RenderBox, Offset),
    {
        for child in self.children() {
            let child_offset = self.get_child_offset(child);
            paint_fn(child, base_offset + child_offset);
        }
    }

    /// Returns an iterator over children in paint order.
    ///
    /// By default, this is the same as `children()` (first to last).
    /// Override this if your container paints children in a different order.
    fn children_in_paint_order(&self) -> Box<dyn Iterator<Item = &dyn RenderBox> + '_> {
        self.children()
    }

    /// Returns an iterator over children in hit test order.
    ///
    /// By default, this is reverse paint order (last to first).
    /// Override this if your container hit tests in a different order.
    fn children_in_hit_test_order(&self) -> Vec<&dyn RenderBox> {
        let mut children: Vec<_> = self.children().collect();
        children.reverse();
        children
    }
}

#[cfg(test)]
mod tests {
    // Tests would be added when implementing concrete types
}
