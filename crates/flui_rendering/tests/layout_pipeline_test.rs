//! Integration tests for the layout pipeline.
//!
//! These tests verify the full layout flow:
//! 1. User implements RenderBox
//! 2. Wrapped in BoxWrapper for RenderObject
//! 3. Stored in RenderTree
//! 4. PipelineOwner.flush_layout() performs layout

use flui_rendering::constraints::BoxConstraints;
use flui_rendering::prelude::*;
use flui_types::Size;

// ============================================================================
// Test RenderBox Implementations
// ============================================================================

/// A simple colored box that takes a fixed size.
#[derive(Debug)]
struct ColoredBox {
    preferred_size: Size,
    actual_size: Size,
}

impl ColoredBox {
    fn new(width: f32, height: f32) -> Self {
        Self {
            preferred_size: Size::new(width, height),
            actual_size: Size::ZERO,
        }
    }
}

impl RenderBox for ColoredBox {
    type Arity = Leaf;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>) {
        // Constrain our preferred size to parent's constraints
        self.actual_size = ctx.constrain(self.preferred_size);
        ctx.complete_with_size(self.actual_size);
    }

    fn size(&self) -> Size {
        self.actual_size
    }

    fn set_size(&mut self, size: Size) {
        self.actual_size = size;
    }

    fn paint(&mut self, _ctx: &mut BoxPaintContext<'_, Leaf, BoxParentData>) {
        // Would paint a colored rectangle here
    }

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Leaf, BoxParentData>) -> bool {
        ctx.is_within_size(self.actual_size.width, self.actual_size.height)
    }
}

/// A simple sized box that applies size constraints to its child.
#[derive(Debug)]
struct SizedBox {
    width: Option<f32>,
    height: Option<f32>,
    size: Size,
}

impl SizedBox {
    fn new(width: Option<f32>, height: Option<f32>) -> Self {
        Self {
            width,
            height,
            size: Size::ZERO,
        }
    }
}

impl RenderBox for SizedBox {
    type Arity = Single;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Single, BoxParentData>) {
        // Apply our width/height constraints
        let constraints = ctx.constraints();

        let child_constraints = BoxConstraints::new(
            self.width.unwrap_or(constraints.min_width),
            self.width.unwrap_or(constraints.max_width),
            self.height.unwrap_or(constraints.min_height),
            self.height.unwrap_or(constraints.max_height),
        );

        // Layout child with our constraints
        let child_size = ctx.layout_child(0, child_constraints);

        // Position child at origin
        ctx.position_child(0, Offset::ZERO);

        // Our size is the child's size (or our explicit size)
        self.size = Size::new(
            self.width.unwrap_or(child_size.width),
            self.height.unwrap_or(child_size.height),
        );

        ctx.complete_with_size(self.size);
    }

    fn size(&self) -> Size {
        self.size
    }

    fn set_size(&mut self, size: Size) {
        self.size = size;
    }

    fn paint(&mut self, _ctx: &mut BoxPaintContext<'_, Single, BoxParentData>) {
        // Paint child
    }

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Single, BoxParentData>) -> bool {
        ctx.is_within_size(self.size.width, self.size.height)
    }
}

// ============================================================================
// Integration Tests
// ============================================================================

#[test]
fn test_colored_box_layout() {
    // Create a ColoredBox
    let colored_box = ColoredBox::new(100.0, 50.0);

    // Wrap it for RenderTree storage
    let wrapper = BoxWrapper::new(colored_box);

    // Create a PipelineOwner and insert the render object
    let mut pipeline = PipelineOwner::new();
    let root_id = pipeline.set_root_render_object(Box::new(wrapper));

    // Verify node was inserted
    assert!(pipeline.render_tree().contains(root_id));
    assert_eq!(pipeline.render_tree().len(), 1);

    // Set constraints on the root
    if let Some(node) = pipeline.render_tree().get(root_id) {
        let mut render_object = node.render_object_mut();
        render_object.set_cached_constraints(BoxConstraints::tight(Size::new(200.0, 100.0)));
    }

    // Flush layout
    pipeline.flush_layout();

    // Verify layout was performed (no longer needs layout)
    if let Some(node) = pipeline.render_tree().get(root_id) {
        let render_object = node.render_object();
        assert!(!render_object.needs_layout());

        // Check paint bounds
        // Tight constraints (200x100) force the size to exactly 200x100
        // The ColoredBox preferred (100x50) gets constrained to tight bounds
        let bounds = render_object.paint_bounds();
        assert_eq!(bounds.width(), 200.0); // tight constraints force 200
        assert_eq!(bounds.height(), 100.0); // tight constraints force 100
    }
}

#[test]
fn test_box_wrapper_creation_and_access() {
    let colored_box = ColoredBox::new(150.0, 75.0);
    let wrapper = BoxWrapper::new(colored_box);

    // Access inner
    assert_eq!(wrapper.inner().preferred_size, Size::new(150.0, 75.0));

    // Check RenderObject implementation
    assert!(wrapper.needs_layout());
    assert!(wrapper.needs_paint());
    assert_eq!(wrapper.depth(), 0);
}

#[test]
fn test_pipeline_owner_with_wrapper() {
    let mut pipeline = PipelineOwner::new();

    // Insert wrapped render object
    let colored_box = ColoredBox::new(200.0, 100.0);
    let wrapper = BoxWrapper::new(colored_box);
    let id = pipeline.set_root_render_object(Box::new(wrapper));

    // Verify it's in the dirty lists
    assert!(pipeline.has_dirty_nodes());

    // Flush layout
    pipeline.flush_layout();

    // Check node state
    if let Some(node) = pipeline.render_tree().get(id) {
        assert!(!node.render_object().needs_layout());
    }
}

#[test]
fn test_multiple_render_objects() {
    let mut pipeline = PipelineOwner::new();

    // Create root
    let root = ColoredBox::new(300.0, 200.0);
    let root_wrapper = BoxWrapper::new(root);
    let root_id = pipeline.set_root_render_object(Box::new(root_wrapper));

    // Create child (will be sibling in tree for now)
    let child = ColoredBox::new(100.0, 50.0);
    let child_wrapper = BoxWrapper::new(child);
    let child_id = pipeline.insert_child_render_object(root_id, Box::new(child_wrapper));

    assert!(child_id.is_some());
    let child_id = child_id.unwrap();

    // Verify tree structure
    assert_eq!(pipeline.render_tree().len(), 2);
    assert_eq!(pipeline.render_tree().parent(child_id), Some(root_id));
    assert!(pipeline.render_tree().children(root_id).contains(&child_id));

    // Flush layout
    pipeline.flush_layout();

    // Both should no longer need layout
    assert!(!pipeline
        .render_tree()
        .get(root_id)
        .unwrap()
        .render_object()
        .needs_layout());
    assert!(!pipeline
        .render_tree()
        .get(child_id)
        .unwrap()
        .render_object()
        .needs_layout());
}

#[test]
fn test_constraints_propagation() {
    let colored_box = ColoredBox::new(500.0, 500.0); // Larger than constraints
    let mut wrapper = BoxWrapper::new(colored_box);

    // Layout with tight constraints that are smaller
    let constraints = BoxConstraints::tight(Size::new(100.0, 100.0));
    wrapper.layout(constraints, true);

    // Size should be constrained
    assert_eq!(wrapper.inner().actual_size, Size::new(100.0, 100.0));
    assert!(!wrapper.needs_layout());
}

#[test]
fn test_layout_context_helpers() {
    let colored_box = ColoredBox::new(50.0, 50.0);
    let mut wrapper = BoxWrapper::new(colored_box);

    // Layout with loose constraints
    let constraints = BoxConstraints::loose(Size::new(200.0, 200.0));
    wrapper.layout(constraints, true);

    // Size should be the preferred size (within loose constraints)
    assert_eq!(wrapper.inner().actual_size, Size::new(50.0, 50.0));
}

#[test]
fn test_paint_offset_propagation() {
    use flui_rendering::objects::RenderCenter;
    use flui_rendering::objects::RenderColoredBox;
    use flui_types::Offset;

    let mut pipeline = PipelineOwner::new();

    // Create Center widget with a ColoredBox child
    // The Center should position the child at the center offset
    let center = RenderCenter::new();
    let center_wrapper = BoxWrapper::new(center);
    let root_id = pipeline.set_root_render_object(Box::new(center_wrapper));

    // Add colored box child (50x50)
    let colored_box = RenderColoredBox::red(50.0, 50.0);
    let child_wrapper = BoxWrapper::new(colored_box);
    let child_id = pipeline.insert_child_render_object(root_id, Box::new(child_wrapper));
    assert!(child_id.is_some());
    let child_id = child_id.unwrap();

    // Set constraints on the root (200x200)
    if let Some(node) = pipeline.render_tree().get(root_id) {
        let mut render_object = node.render_object_mut();
        render_object.set_cached_constraints(BoxConstraints::tight(Size::new(200.0, 200.0)));
    }

    // Flush layout
    pipeline.flush_layout();

    // Verify layout was performed
    assert!(!pipeline
        .render_tree()
        .get(root_id)
        .unwrap()
        .render_object()
        .needs_layout());
    assert!(!pipeline
        .render_tree()
        .get(child_id)
        .unwrap()
        .render_object()
        .needs_layout());

    // Check that Center positioned the child correctly
    // Center should put 50x50 child in center of 200x200 = offset (75, 75)
    if let Some(node) = pipeline.render_tree().get(root_id) {
        let render_object = node.render_object();
        let child_offset = render_object.child_offset(0);
        // With 200x200 parent and 50x50 child, center offset should be (75, 75)
        assert_eq!(child_offset.dx, 75.0);
        assert_eq!(child_offset.dy, 75.0);
    }

    // Now flush paint and verify layer tree is created
    pipeline.flush_paint();

    let layer_tree = pipeline.layer_tree();
    assert!(layer_tree.is_some());
    let layer_tree = layer_tree.unwrap();

    // Layer tree should have at least root offset layer + picture layer
    assert!(layer_tree.len() >= 1);
}

#[test]
fn test_repaint_boundary_creates_offset_layer() {
    use flui_rendering::objects::RenderColoredBox;
    use flui_rendering::wrapper::BoxWrapper;

    let mut pipeline = PipelineOwner::new();

    // Create a root colored box
    let root_box = RenderColoredBox::blue(200.0, 200.0);
    let root_wrapper = BoxWrapper::new(root_box);
    let root_id = pipeline.set_root_render_object(Box::new(root_wrapper));

    // Create a child with repaint boundary
    let child_box = RenderColoredBox::red(50.0, 50.0);
    let child_wrapper = BoxWrapper::with_repaint_boundary(child_box);
    let child_id = pipeline.insert_child_render_object(root_id, Box::new(child_wrapper));
    assert!(child_id.is_some());

    // Set constraints on root
    if let Some(node) = pipeline.render_tree().get(root_id) {
        let mut render_object = node.render_object_mut();
        render_object.set_cached_constraints(BoxConstraints::tight(Size::new(200.0, 200.0)));
    }

    // Flush layout and paint
    pipeline.flush_layout();
    pipeline.flush_paint();

    // Verify layer tree was created
    let layer_tree = pipeline.layer_tree();
    assert!(layer_tree.is_some());

    // The repaint boundary child should have created its own OffsetLayer
    // Layer tree should have: root offset + picture + child offset layer + child picture
    let layer_tree = layer_tree.unwrap();
    assert!(
        layer_tree.len() >= 2,
        "Expected at least 2 layers, got {}",
        layer_tree.len()
    );
}
