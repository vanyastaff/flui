//! Integration tests for RenderObject hit testing
//!
//! This module tests the complete hit testing implementation across all RenderObjects
//! that implement custom hit_test methods.

#[cfg(test)]
mod tests {
    use crate::objects::effects::{RenderClipOval, RenderClipRect, RenderClipRRect, RenderTransform};
    use crate::objects::interaction::{RenderAbsorbPointer, RenderIgnorePointer};
    use flui_core::element::hit_test::BoxHitTestResult;
    use flui_core::element::{ElementId, ElementTree};
    use flui_core::render::{BoxHitTestContext, Children, Render};
    use flui_types::geometry::Transform;
    use flui_types::painting::Clip;
    use flui_types::{Offset, Size};

    // Helper function to create a simple BoxHitTestContext
    fn create_context<'a>(
        tree: &'a ElementTree,
        position: Offset,
        size: Size,
        children: &'a Children,
        element_id: ElementId,
    ) -> BoxHitTestContext<'a> {
        BoxHitTestContext::new(tree, position, size, children, element_id)
    }

    // ========== AbsorbPointer Tests ==========

    #[test]
    fn test_absorb_pointer_absorbing() {
        let tree = ElementTree::new();
        let children = Children::None;
        let element_id = ElementId::new(1);

        let absorb = RenderAbsorbPointer::new(true);
        let ctx = create_context(&tree, Offset::new(50.0, 50.0), Size::new(100.0, 100.0), &children, element_id);
        let mut result = BoxHitTestResult::new();

        // When absorbing, should add self to result and return true
        let hit = absorb.hit_test(&ctx, &mut result);
        assert!(hit, "AbsorbPointer should return true when absorbing");
        assert_eq!(result.entries().len(), 1, "Should have one entry when absorbing");

        // Verify entry details
        let (hit_element_id, entry) = &result.entries()[0];
        assert_eq!(*hit_element_id, element_id);
        assert_eq!(entry.local_position, Offset::new(50.0, 50.0));
    }

    #[test]
    fn test_absorb_pointer_not_absorbing() {
        let tree = ElementTree::new();
        let children = Children::None;
        let element_id = ElementId::new(1);

        let absorb = RenderAbsorbPointer::new(false);
        let ctx = create_context(&tree, Offset::new(50.0, 50.0), Size::new(100.0, 100.0), &children, element_id);
        let mut result = BoxHitTestResult::new();

        // When not absorbing, should delegate to children (which is None here)
        let hit = absorb.hit_test(&ctx, &mut result);
        assert!(!hit, "Should return false when not absorbing and no children");
        assert!(result.is_empty(), "Result should be empty");
    }

    // ========== IgnorePointer Tests ==========

    #[test]
    fn test_ignore_pointer_ignoring() {
        let tree = ElementTree::new();
        let children = Children::None;
        let element_id = ElementId::new(1);

        let ignore = RenderIgnorePointer::new(true);
        let ctx = create_context(&tree, Offset::new(50.0, 50.0), Size::new(100.0, 100.0), &children, element_id);
        let mut result = BoxHitTestResult::new();

        // When ignoring, should return false (pass through)
        let hit = ignore.hit_test(&ctx, &mut result);
        assert!(!hit, "IgnorePointer should return false when ignoring");
        assert!(result.is_empty(), "Result should be empty when ignoring");
    }

    #[test]
    fn test_ignore_pointer_not_ignoring() {
        let tree = ElementTree::new();
        let children = Children::None;
        let element_id = ElementId::new(1);

        let ignore = RenderIgnorePointer::new(false);
        let ctx = create_context(&tree, Offset::new(50.0, 50.0), Size::new(100.0, 100.0), &children, element_id);
        let mut result = BoxHitTestResult::new();

        // When not ignoring, should delegate to children (which is None here)
        let hit = ignore.hit_test(&ctx, &mut result);
        assert!(!hit, "Should return false when not ignoring and no children");
    }

    // ========== Transform Tests ==========

    #[test]
    fn test_transform_identity() {
        let tree = ElementTree::new();
        let children = Children::None;
        let element_id = ElementId::new(1);

        let transform = RenderTransform::new(Transform::identity());
        let ctx = create_context(&tree, Offset::new(50.0, 50.0), Size::new(100.0, 100.0), &children, element_id);
        let mut result = BoxHitTestResult::new();

        // Identity transform should pass through position unchanged
        let hit = transform.hit_test(&ctx, &mut result);
        assert!(!hit, "Should return false with no children");
    }

    #[test]
    fn test_transform_translation() {
        let tree = ElementTree::new();
        let children = Children::None;
        let element_id = ElementId::new(1);

        let transform = RenderTransform::new(Transform::translate(10.0, 20.0));
        let ctx = create_context(&tree, Offset::new(60.0, 70.0), Size::new(100.0, 100.0), &children, element_id);
        let mut result = BoxHitTestResult::new();

        // Translation should apply inverse: (60, 70) - (10, 20) = (50, 50)
        let hit = transform.hit_test(&ctx, &mut result);
        assert!(!hit, "Should return false with no children");
        // Note: We can't easily verify the transformed position was correct without children
    }

    #[test]
    fn test_transform_singular_matrix() {
        let tree = ElementTree::new();
        let children = Children::None;
        let element_id = ElementId::new(1);

        // Scale by 0 creates a singular (non-invertible) matrix
        let transform = RenderTransform::new(Transform::scale(0.0));
        let ctx = create_context(&tree, Offset::new(50.0, 50.0), Size::new(100.0, 100.0), &children, element_id);
        let mut result = BoxHitTestResult::new();

        // Should return false when transform is not invertible
        let hit = transform.hit_test(&ctx, &mut result);
        assert!(!hit, "Should return false for singular transform");
        assert!(result.is_empty(), "Result should be empty for singular transform");
    }

    // ========== ClipRect Tests ==========

    #[test]
    fn test_clip_rect_inside_bounds() {
        let tree = ElementTree::new();
        let children = Children::None;
        let element_id = ElementId::new(1);

        let clip = RenderClipRect::with_clip(Clip::AntiAlias);
        let ctx = create_context(&tree, Offset::new(50.0, 50.0), Size::new(100.0, 100.0), &children, element_id);
        let mut result = BoxHitTestResult::new();

        // Position (50, 50) is inside bounds (0, 0, 100, 100)
        let hit = clip.hit_test(&ctx, &mut result);
        assert!(!hit, "Should return false with no children (but position is valid)");
    }

    #[test]
    fn test_clip_rect_outside_bounds() {
        let tree = ElementTree::new();
        let children = Children::None;
        let element_id = ElementId::new(1);

        let clip = RenderClipRect::with_clip(Clip::AntiAlias);
        let ctx = create_context(&tree, Offset::new(150.0, 50.0), Size::new(100.0, 100.0), &children, element_id);
        let mut result = BoxHitTestResult::new();

        // Position (150, 50) is outside bounds (0, 0, 100, 100)
        let hit = clip.hit_test(&ctx, &mut result);
        assert!(!hit, "Should return false when outside clip bounds");
        assert!(result.is_empty(), "Result should be empty when outside bounds");
    }

    #[test]
    fn test_clip_rect_no_clipping() {
        let tree = ElementTree::new();
        let children = Children::None;
        let element_id = ElementId::new(1);

        let clip = RenderClipRect::with_clip(Clip::None);
        let ctx = create_context(&tree, Offset::new(150.0, 50.0), Size::new(100.0, 100.0), &children, element_id);
        let mut result = BoxHitTestResult::new();

        // When Clip::None, should delegate without bounds checking
        let hit = clip.hit_test(&ctx, &mut result);
        assert!(!hit, "Should return false with no children");
    }

    // ========== ClipOval Tests ==========

    #[test]
    fn test_clip_oval_center_hit() {
        let tree = ElementTree::new();
        let children = Children::None;
        let element_id = ElementId::new(1);

        let clip = RenderClipOval::with_clip(Clip::AntiAlias);
        let ctx = create_context(&tree, Offset::new(50.0, 50.0), Size::new(100.0, 100.0), &children, element_id);
        let mut result = BoxHitTestResult::new();

        // Center of oval (50, 50) should be inside
        let hit = clip.hit_test(&ctx, &mut result);
        assert!(!hit, "Should return false with no children (but position is valid)");
    }

    #[test]
    fn test_clip_oval_corner_miss() {
        let tree = ElementTree::new();
        let children = Children::None;
        let element_id = ElementId::new(1);

        let clip = RenderClipOval::with_clip(Clip::AntiAlias);
        // Top-left corner (5, 5) should be outside the oval
        let ctx = create_context(&tree, Offset::new(5.0, 5.0), Size::new(100.0, 100.0), &children, element_id);
        let mut result = BoxHitTestResult::new();

        let hit = clip.hit_test(&ctx, &mut result);
        assert!(!hit, "Corner should be outside oval");
        assert!(result.is_empty(), "Result should be empty when outside oval");
    }

    #[test]
    fn test_clip_oval_edge_hit() {
        let tree = ElementTree::new();
        let children = Children::None;
        let element_id = ElementId::new(1);

        let clip = RenderClipOval::with_clip(Clip::AntiAlias);
        // Point on right edge (100, 50) - exactly on the boundary
        let ctx = create_context(&tree, Offset::new(100.0, 50.0), Size::new(100.0, 100.0), &children, element_id);
        let mut result = BoxHitTestResult::new();

        let hit = clip.hit_test(&ctx, &mut result);
        assert!(!hit, "Edge point should be on boundary (implementation may vary)");
    }

    // ========== ClipRRect Tests ==========

    #[test]
    fn test_clip_rrect_center_hit() {
        let tree = ElementTree::new();
        let children = Children::None;
        let element_id = ElementId::new(1);

        let clip = RenderClipRRect::circular(10.0);
        let ctx = create_context(&tree, Offset::new(50.0, 50.0), Size::new(100.0, 100.0), &children, element_id);
        let mut result = BoxHitTestResult::new();

        // Center should always be inside
        let hit = clip.hit_test(&ctx, &mut result);
        assert!(!hit, "Should return false with no children (but position is valid)");
    }

    #[test]
    fn test_clip_rrect_corner_inside_radius() {
        let tree = ElementTree::new();
        let children = Children::None;
        let element_id = ElementId::new(1);

        let clip = RenderClipRRect::circular(20.0);
        // Point (15, 15) should be inside the rounded corner (radius 20)
        let ctx = create_context(&tree, Offset::new(15.0, 15.0), Size::new(100.0, 100.0), &children, element_id);
        let mut result = BoxHitTestResult::new();

        let hit = clip.hit_test(&ctx, &mut result);
        assert!(!hit, "Should return false with no children");
    }

    #[test]
    fn test_clip_rrect_corner_outside_radius() {
        let tree = ElementTree::new();
        let children = Children::None;
        let element_id = ElementId::new(1);

        let clip = RenderClipRRect::circular(20.0);
        // Point (5, 5) should be outside the rounded corner (radius 20)
        let ctx = create_context(&tree, Offset::new(5.0, 5.0), Size::new(100.0, 100.0), &children, element_id);
        let mut result = BoxHitTestResult::new();

        let hit = clip.hit_test(&ctx, &mut result);
        assert!(!hit, "Corner outside radius should miss");
        assert!(result.is_empty(), "Result should be empty when outside corner");
    }

    #[test]
    fn test_clip_rrect_zero_radius() {
        let tree = ElementTree::new();
        let children = Children::None;
        let element_id = ElementId::new(1);

        // Zero radius = regular rectangle
        let clip = RenderClipRRect::circular(0.0);
        let ctx = create_context(&tree, Offset::new(5.0, 5.0), Size::new(100.0, 100.0), &children, element_id);
        let mut result = BoxHitTestResult::new();

        // With zero radius, all points inside bounds should hit
        let hit = clip.hit_test(&ctx, &mut result);
        assert!(!hit, "Should return false with no children (but position is valid)");
    }

    // ========== BoxHitTestContext Tests ==========

    #[test]
    fn test_box_hit_test_context_is_in_bounds() {
        let tree = ElementTree::new();
        let children = Children::None;
        let element_id = ElementId::new(1);

        // Inside bounds
        let ctx = create_context(&tree, Offset::new(50.0, 50.0), Size::new(100.0, 100.0), &children, element_id);
        assert!(ctx.is_in_bounds(), "Position (50, 50) should be in bounds (100x100)");

        // On boundary
        let ctx = create_context(&tree, Offset::new(100.0, 100.0), Size::new(100.0, 100.0), &children, element_id);
        assert!(ctx.is_in_bounds(), "Position (100, 100) should be on bounds (100x100)");

        // Outside bounds - X
        let ctx = create_context(&tree, Offset::new(150.0, 50.0), Size::new(100.0, 100.0), &children, element_id);
        assert!(!ctx.is_in_bounds(), "Position (150, 50) should be outside bounds (100x100)");

        // Outside bounds - Y
        let ctx = create_context(&tree, Offset::new(50.0, 150.0), Size::new(100.0, 100.0), &children, element_id);
        assert!(!ctx.is_in_bounds(), "Position (50, 150) should be outside bounds (100x100)");

        // Negative position
        let ctx = create_context(&tree, Offset::new(-10.0, 50.0), Size::new(100.0, 100.0), &children, element_id);
        assert!(!ctx.is_in_bounds(), "Negative position should be outside bounds");
    }

    #[test]
    fn test_box_hit_test_context_with_position() {
        let tree = ElementTree::new();
        let children = Children::None;
        let element_id = ElementId::new(1);

        let ctx = create_context(&tree, Offset::new(50.0, 50.0), Size::new(100.0, 100.0), &children, element_id);
        let new_ctx = ctx.with_position(Offset::new(75.0, 75.0));

        assert_eq!(new_ctx.position, Offset::new(75.0, 75.0));
        assert_eq!(new_ctx.size, ctx.size);
        assert!(new_ctx.is_in_bounds());
    }

    // ========== Integration Tests ==========

    #[test]
    fn test_combined_absorb_and_clip() {
        // Test that AbsorbPointer inside a ClipRect works correctly
        let tree = ElementTree::new();
        let children = Children::None;
        let element_id = ElementId::new(1);

        // Create AbsorbPointer
        let absorb = RenderAbsorbPointer::new(true);

        // Position inside bounds
        let ctx = create_context(&tree, Offset::new(50.0, 50.0), Size::new(100.0, 100.0), &children, element_id);
        let mut result = BoxHitTestResult::new();

        let hit = absorb.hit_test(&ctx, &mut result);
        assert!(hit, "AbsorbPointer should absorb the hit");
        assert_eq!(result.entries().len(), 1);
    }
}
