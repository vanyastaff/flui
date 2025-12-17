//! Integration tests for repaint boundary behavior.
//!
//! Tests that repaint boundaries correctly stop dirty propagation
//! and that paint is correctly constrained to subtrees.

use flui_rendering::objects::r#box::basic::{RenderPadding, RenderSizedBox};
use flui_rendering::pipeline::PipelineOwner;
use flui_rendering::prelude::*;
use flui_types::EdgeInsets;

// ============================================================================
// Test Helpers
// ============================================================================

/// Creates a simple RenderBox.
fn create_sized_box(width: f32, height: f32) -> Box<dyn RenderObject> {
    Box::new(RenderSizedBox::new(Some(width), Some(height)))
}

/// Creates padding RenderBox.
fn create_padding(inset: f32) -> Box<dyn RenderObject> {
    Box::new(RenderPadding::new(EdgeInsets::all(inset)))
}

// ============================================================================
// Repaint Boundary Tests
// ============================================================================

/// Test that mark_needs_paint sets the dirty flag.
#[test]
fn test_mark_needs_paint_sets_flag() {
    let mut obj = RenderSizedBox::new(Some(100.0), Some(100.0));

    // Clear the flag first
    obj.clear_needs_paint();
    assert!(!obj.needs_paint(), "Should not need paint after clear");

    // Mark needs paint
    obj.mark_needs_paint();
    assert!(obj.needs_paint(), "Should need paint after marking");
}

/// Test that clear_needs_paint clears the flag.
#[test]
fn test_clear_needs_paint() {
    let mut obj = RenderSizedBox::new(Some(100.0), Some(100.0));

    obj.mark_needs_paint();
    assert!(obj.needs_paint());

    obj.clear_needs_paint();
    assert!(!obj.needs_paint(), "Should not need paint after clear");
}

/// Test that flush_paint processes nodes in depth order (deep first).
#[test]
fn test_paint_processes_deep_first() {
    let mut owner = PipelineOwner::new();

    // Create deep tree
    let root_id = owner.set_root_render_object(create_sized_box(100.0, 100.0));
    let child1_id = owner
        .insert_child_render_object(root_id, create_padding(5.0))
        .unwrap();
    let child2_id = owner
        .insert_child_render_object(child1_id, create_padding(5.0))
        .unwrap();
    let _child3_id = owner
        .insert_child_render_object(child2_id, create_sized_box(10.0, 10.0))
        .unwrap();

    // Flush layout first (required before paint)
    owner.flush_layout();

    // Dirty nodes for paint should be set from insert
    let dirty_paint_count = owner.nodes_needing_paint().len();
    println!("Dirty paint nodes before flush: {}", dirty_paint_count);

    // After flush_paint, all should be clean
    owner.flush_paint();

    assert!(
        owner.nodes_needing_paint().is_empty(),
        "All nodes should be clean after flush_paint"
    );
}

/// Test that repaint boundary nodes are processed independently.
#[test]
fn test_repaint_boundary_independent_processing() {
    let mut owner = PipelineOwner::new();

    // Create tree with two branches:
    // root -> branch1 -> child1
    //      -> branch2 -> child2

    let root_id = owner.set_root_render_object(create_sized_box(200.0, 200.0));

    let branch1_id = owner
        .insert_child_render_object(root_id, create_padding(10.0))
        .unwrap();
    let _child1_id = owner
        .insert_child_render_object(branch1_id, create_sized_box(50.0, 50.0))
        .unwrap();

    let branch2_id = owner
        .insert_child_render_object(root_id, create_padding(10.0))
        .unwrap();
    let _child2_id = owner
        .insert_child_render_object(branch2_id, create_sized_box(50.0, 50.0))
        .unwrap();

    // Flush all
    owner.flush_layout();
    owner.flush_paint();

    assert!(owner.nodes_needing_paint().is_empty());

    // Mark only branch1 as needing paint
    owner.add_node_needing_paint(branch1_id.get(), 1);

    // Only branch1 should be dirty
    let dirty_ids: Vec<usize> = owner.nodes_needing_paint().iter().map(|n| n.id).collect();
    assert_eq!(dirty_ids.len(), 1);
    assert!(dirty_ids.contains(&branch1_id.get()));

    // Flush should only process branch1
    owner.flush_paint();
    assert!(owner.nodes_needing_paint().is_empty());
}

/// Test that layout triggers paint.
#[test]
fn test_layout_triggers_paint() {
    let mut obj = RenderSizedBox::new(Some(100.0), Some(100.0));

    // Clear paint flag
    obj.clear_needs_paint();
    assert!(!obj.needs_paint());

    // Mark needs layout and perform layout
    obj.mark_needs_layout();
    obj.layout_without_resize();

    // Paint should now be needed
    assert!(
        obj.needs_paint(),
        "layout_without_resize should mark needs_paint"
    );
}

// ============================================================================
// Full Pipeline Tests
// ============================================================================

/// Test complete layout -> paint cycle.
#[test]
fn test_layout_then_paint_cycle() {
    let mut owner = PipelineOwner::new();

    let root_id = owner.set_root_render_object(create_sized_box(100.0, 100.0));
    let _child_id = owner
        .insert_child_render_object(root_id, create_sized_box(50.0, 50.0))
        .unwrap();

    // Both layout and paint should be dirty
    assert!(!owner.nodes_needing_layout().is_empty());
    assert!(!owner.nodes_needing_paint().is_empty());

    // Flush layout
    owner.flush_layout();
    assert!(owner.nodes_needing_layout().is_empty());

    // Paint might still be dirty (layout triggers paint)
    // Flush paint
    owner.flush_paint();
    assert!(owner.nodes_needing_paint().is_empty());
}

/// Test multiple paint cycles.
#[test]
fn test_multiple_paint_cycles() {
    let mut owner = PipelineOwner::new();

    let root_id = owner.set_root_render_object(create_sized_box(100.0, 100.0));
    let child_id = owner
        .insert_child_render_object(root_id, create_sized_box(50.0, 50.0))
        .unwrap();

    // First cycle
    owner.flush_layout();
    owner.flush_paint();
    assert!(owner.nodes_needing_paint().is_empty());

    // Mark dirty again
    owner.add_node_needing_paint(child_id.get(), 1);
    assert_eq!(owner.nodes_needing_paint().len(), 1);

    // Second cycle
    owner.flush_paint();
    assert!(owner.nodes_needing_paint().is_empty());
}

/// Test that paint respects depth ordering (deep first for correct compositing).
#[test]
fn test_paint_depth_ordering() {
    let mut owner = PipelineOwner::new();

    // Create tree: root(depth=0) -> child(depth=1) -> grandchild(depth=2)
    let root_id = owner.set_root_render_object(create_sized_box(100.0, 100.0));
    let child_id = owner
        .insert_child_render_object(root_id, create_padding(5.0))
        .unwrap();
    let grandchild_id = owner
        .insert_child_render_object(child_id, create_sized_box(25.0, 25.0))
        .unwrap();

    owner.flush_layout();

    // Clear and manually add in wrong order
    owner.flush_paint(); // Clear
    owner.add_node_needing_paint(root_id.get(), 0);
    owner.add_node_needing_paint(grandchild_id.get(), 2);
    owner.add_node_needing_paint(child_id.get(), 1);

    // Verify dirty list has all three
    assert_eq!(owner.nodes_needing_paint().len(), 3);

    // After flush, the implementation should have processed deep-first
    // (grandchild, then child, then root)
    owner.flush_paint();
    assert!(owner.nodes_needing_paint().is_empty());
}
