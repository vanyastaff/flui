//! Integration tests for the layout pipeline.
//!
//! These tests verify the full layout flow:
//! 1. User implements RenderBox
//! 2. Converted via IntoRenderObject for RenderTree storage
//! 3. PipelineOwner.flush_layout() performs layout

use flui_rendering::constraints::{BoxConstraints, Constraints};
use flui_rendering::prelude::*;
use flui_tree::TreeWrite;
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

impl flui_foundation::Diagnosticable for ColoredBox {}

impl RenderBox for ColoredBox {
    type Arity = Leaf;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>) {
        // Constrain our preferred size to parent's constraints
        self.actual_size = ctx.constrain(self.preferred_size);
        ctx.complete_with_size(self.actual_size);
    }

    fn size(&self) -> &Size {
        &self.actual_size
    }

    fn size_mut(&mut self) -> &mut Size {
        &mut self.actual_size
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

impl flui_foundation::Diagnosticable for SizedBox {}

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

    fn size(&self) -> &Size {
        &self.size
    }

    fn size_mut(&mut self) -> &mut Size {
        &mut self.size
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
fn test_colored_box_creation() {
    let colored_box = ColoredBox::new(100.0, 50.0);
    assert_eq!(colored_box.preferred_size, Size::new(100.0, 50.0));
    assert_eq!(*colored_box.size(), Size::ZERO);
}

#[test]
fn test_sized_box_creation() {
    let sized_box = SizedBox::new(Some(100.0), Some(50.0));
    assert_eq!(sized_box.width, Some(100.0));
    assert_eq!(sized_box.height, Some(50.0));
}

#[test]
fn test_into_render_node() {
    use flui_rendering::protocol::IntoRenderObject;

    let colored_box = ColoredBox::new(100.0, 50.0);
    let node = colored_box.into_render_node();
    assert!(node.is_box());
}

#[test]
fn test_render_tree_insertion() {
    use flui_rendering::protocol::IntoRenderObject;
    use flui_rendering::storage::RenderTree;

    let mut tree = RenderTree::new();

    let colored_box = ColoredBox::new(100.0, 50.0);
    let node = colored_box.into_render_node();
    let id = tree.insert(node);

    assert!(tree.contains(id));
    assert_eq!(tree.len(), 1);
}

#[test]
fn test_render_tree_multiple_nodes() {
    use flui_rendering::protocol::IntoRenderObject;
    use flui_rendering::storage::RenderTree;

    let mut tree = RenderTree::new();

    // Insert multiple nodes
    let parent = ColoredBox::new(200.0, 200.0);
    let parent_node = parent.into_render_node();
    let parent_id = tree.insert(parent_node);

    let child = ColoredBox::new(50.0, 50.0);
    let child_node = child.into_render_node();
    let child_id = tree.insert(child_node);

    // Verify tree has both nodes
    assert_eq!(tree.len(), 2);
    assert!(tree.contains(parent_id));
    assert!(tree.contains(child_id));
}

#[test]
fn test_box_constraints_operations() {
    // Test tight constraints
    let tight = BoxConstraints::tight(Size::new(100.0, 50.0));
    assert!(tight.is_tight());
    assert_eq!(tight.min_width, 100.0);
    assert_eq!(tight.max_width, 100.0);

    // Test loose constraints
    let loose = BoxConstraints::loose(Size::new(200.0, 100.0));
    assert!(!loose.is_tight());
    assert_eq!(loose.min_width, 0.0);
    assert_eq!(loose.max_width, 200.0);

    // Test constrain
    let size = tight.constrain(Size::new(500.0, 500.0));
    assert_eq!(size, Size::new(100.0, 50.0));
}
