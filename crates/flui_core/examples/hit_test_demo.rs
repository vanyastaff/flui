//! Hit Testing Demo
//!
//! Demonstrates the hit testing system for pointer events.
//!
//! This example shows how hit testing works by creating a simple element tree
//! and testing positions against it.

use flui_core::{
    element::ElementTree, render::RenderNode, view::IntoElement, BoxedLayer, ElementId,
    RenderElement,
};
use flui_types::{constraints::BoxConstraints, Offset, Size};

fn main() {
    println!("=== Hit Testing Demo ===\n");

    // Create a simple tree structure with manually configured sizes
    let mut tree = ElementTree::new();

    // Root element: 400x400 box at (0,0)
    let root_render = create_box_render("Root", Size::new(400.0, 400.0));
    let mut root_elem = RenderElement::new(root_render);
    root_elem.set_offset(Offset::ZERO);
    // Set size manually (normally done by layout)
    root_elem.render_state().write().set_size(Size::new(400.0, 400.0));

    // Child element: 200x200 box at offset (50,50) from root
    let child_render = create_box_render("Child", Size::new(200.0, 200.0));
    let mut child_elem = RenderElement::new(child_render);
    child_elem.set_offset(Offset::new(50.0, 50.0));
    child_elem
        .render_state()
        .write()
        .set_size(Size::new(200.0, 200.0));

    // Insert into tree (child first, then root with child reference)
    let child_id = tree.insert(child_elem.into_element());

    // IMPORTANT: Need to manually update root's children list
    // This would normally be done by the build pipeline
    // For now, we'll just insert root without children for simplicity
    let root_id = tree.insert(root_elem.into_element());

    println!("Tree structure:");
    println!("  Root: bounds (0,0 - 400x400)");
    println!("  Child: offset (50,50), bounds (50,50 - 250x250)\n");
    println!("NOTE: This is a simplified demo. Child is not connected to root in this example.\n");

    // Test 1: Hit test root directly
    println!("=== Test 1: Root Element ===");
    test_hit_position(&tree, root_id, Offset::new(100.0, 100.0));
    test_hit_position(&tree, root_id, Offset::new(20.0, 20.0));
    test_hit_position(&tree, root_id, Offset::new(500.0, 500.0));

    // Test 2: Hit test child element
    println!("\n=== Test 2: Child Element ===");
    // Remember child is at offset (50,50), so position (100,100) is inside it
    // But since child offset is relative to parent, we test in global coords
    test_hit_position(&tree, child_id, Offset::new(100.0, 100.0));
    test_hit_position(&tree, child_id, Offset::new(20.0, 20.0));

    println!("\n✅ Hit testing demo complete!");
}

/// Test hit testing at a specific position
fn test_hit_position(tree: &ElementTree, element_id: ElementId, position: Offset) {
    println!("Testing position {:?}:", position);

    let result = tree.hit_test(element_id, position);

    if result.is_empty() {
        println!("  ❌ No elements hit");
    } else {
        println!("  ✅ Hit {} element(s):", result.entries().len());
        for entry in result.iter() {
            println!(
                "     Element {:?} at local position {:?}",
                entry.element_id, entry.local_position
            );
        }
    }
}

/// Create a simple box render object for testing
fn create_box_render(name: &str, size: Size) -> RenderNode {
    use flui_core::render::LeafRender;

    /// Simple box render for testing
    #[derive(Debug)]
    struct TestBox {
        _name: String,
        size: Size,
    }

    impl LeafRender for TestBox {
        type Metadata = ();

        fn layout(&mut self, _constraints: BoxConstraints) -> Size {
            self.size
        }

        fn paint(&self, _offset: Offset) -> BoxedLayer {
            // Return an empty container layer
            use flui_engine::layer::ContainerLayer;
            Box::new(ContainerLayer::new())
        }

        fn metadata(&self) -> Option<&dyn std::any::Any> {
            None
        }
    }

    RenderNode::leaf(Box::new(TestBox {
        _name: name.to_string(),
        size,
    }))
}
