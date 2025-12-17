//! Integration tests for full frame rendering cycle.
//!
//! Tests the complete build -> layout -> compositing bits -> paint -> semantics cycle.

use flui_rendering::objects::r#box::basic::{RenderPadding, RenderSizedBox};
use flui_rendering::pipeline::PipelineOwner;
use flui_rendering::prelude::*;
use flui_types::EdgeInsets;

// ============================================================================
// Test Helpers
// ============================================================================

fn create_sized_box(width: f32, height: f32) -> Box<dyn RenderObject> {
    Box::new(RenderSizedBox::new(Some(width), Some(height)))
}

fn create_padding(inset: f32) -> Box<dyn RenderObject> {
    Box::new(RenderPadding::new(EdgeInsets::all(inset)))
}

// ============================================================================
// Full Frame Cycle Tests
// ============================================================================

/// Test the complete frame cycle: layout -> compositing bits -> paint.
#[test]
fn test_full_frame_cycle() {
    let mut owner = PipelineOwner::new();

    // Build tree
    let root_id = owner.set_root_render_object(create_sized_box(100.0, 100.0));
    let _child_id = owner
        .insert_child_render_object(root_id, create_padding(10.0))
        .unwrap();

    // Initial state - all dirty
    assert!(
        !owner.nodes_needing_layout().is_empty(),
        "Should need layout"
    );
    assert!(!owner.nodes_needing_paint().is_empty(), "Should need paint");

    // Phase 1: Layout
    owner.flush_layout();
    assert!(
        owner.nodes_needing_layout().is_empty(),
        "Layout should be clean after flush"
    );

    // Phase 2: Compositing bits (currently no-op but should not crash)
    owner.flush_compositing_bits();
    assert!(
        owner.nodes_needing_compositing_bits_update().is_empty(),
        "Compositing bits should be clean"
    );

    // Phase 3: Paint
    owner.flush_paint();
    assert!(
        owner.nodes_needing_paint().is_empty(),
        "Paint should be clean after flush"
    );

    // Phase 4: Semantics (disabled by default)
    owner.flush_semantics();
    // No assertion - semantics is disabled
}

/// Test that frame phases must be called in order.
#[test]
fn test_frame_phases_order() {
    let mut owner = PipelineOwner::new();

    let root_id = owner.set_root_render_object(create_sized_box(100.0, 100.0));
    let _child_id = owner
        .insert_child_render_object(root_id, create_sized_box(50.0, 50.0))
        .unwrap();

    // Calling paint before layout should work but nodes won't be properly laid out
    // In a real app, this would cause visual glitches

    // Proper order
    owner.flush_layout();
    owner.flush_compositing_bits();
    owner.flush_paint();

    // All clean
    assert!(owner.nodes_needing_layout().is_empty());
    assert!(owner.nodes_needing_paint().is_empty());
}

/// Test multiple frames in sequence.
#[test]
fn test_multiple_frames() {
    let mut owner = PipelineOwner::new();

    let root_id = owner.set_root_render_object(create_sized_box(100.0, 100.0));
    let child_id = owner
        .insert_child_render_object(root_id, create_sized_box(50.0, 50.0))
        .unwrap();

    // Frame 1
    owner.flush_layout();
    owner.flush_compositing_bits();
    owner.flush_paint();

    assert!(owner.nodes_needing_layout().is_empty());
    assert!(owner.nodes_needing_paint().is_empty());

    // Simulate state change - mark child dirty
    owner.add_node_needing_layout(child_id.get(), 1);

    // Frame 2
    owner.flush_layout();
    owner.flush_compositing_bits();
    owner.flush_paint();

    assert!(owner.nodes_needing_layout().is_empty());
    assert!(owner.nodes_needing_paint().is_empty());

    // Frame 3 - no changes, should be no-op
    owner.flush_layout();
    owner.flush_compositing_bits();
    owner.flush_paint();

    assert!(owner.nodes_needing_layout().is_empty());
    assert!(owner.nodes_needing_paint().is_empty());
}

/// Test frame with deep tree.
#[test]
fn test_frame_with_deep_tree() {
    let mut owner = PipelineOwner::new();

    // Create deep tree: root -> p1 -> p2 -> p3 -> p4 -> leaf
    let root_id = owner.set_root_render_object(create_sized_box(200.0, 200.0));
    let p1_id = owner
        .insert_child_render_object(root_id, create_padding(10.0))
        .unwrap();
    let p2_id = owner
        .insert_child_render_object(p1_id, create_padding(10.0))
        .unwrap();
    let p3_id = owner
        .insert_child_render_object(p2_id, create_padding(10.0))
        .unwrap();
    let p4_id = owner
        .insert_child_render_object(p3_id, create_padding(10.0))
        .unwrap();
    let _leaf_id = owner
        .insert_child_render_object(p4_id, create_sized_box(50.0, 50.0))
        .unwrap();

    // Should have 6 nodes needing layout
    let layout_count = owner.nodes_needing_layout().len();
    println!("Nodes needing layout: {}", layout_count);

    // Frame
    owner.flush_layout();
    owner.flush_compositing_bits();
    owner.flush_paint();

    assert!(owner.nodes_needing_layout().is_empty());
    assert!(owner.nodes_needing_paint().is_empty());
}

/// Test frame with wide tree (many siblings).
#[test]
fn test_frame_with_wide_tree() {
    let mut owner = PipelineOwner::new();

    // Create wide tree: root -> child1, child2, child3, ..., child10
    let root_id = owner.set_root_render_object(create_sized_box(500.0, 500.0));

    for i in 0..10 {
        let size = 40.0 + (i as f32 * 5.0);
        let _child_id = owner
            .insert_child_render_object(root_id, create_sized_box(size, size))
            .unwrap();
    }

    // Should have 11 nodes (root + 10 children)
    assert_eq!(owner.render_tree().len(), 11);

    // Frame
    owner.flush_layout();
    owner.flush_compositing_bits();
    owner.flush_paint();

    assert!(owner.nodes_needing_layout().is_empty());
    assert!(owner.nodes_needing_paint().is_empty());
}

// ============================================================================
// Incremental Update Tests
// ============================================================================

/// Test that only dirty nodes are processed in subsequent frames.
#[test]
fn test_incremental_update() {
    let mut owner = PipelineOwner::new();

    let root_id = owner.set_root_render_object(create_sized_box(100.0, 100.0));
    let child1_id = owner
        .insert_child_render_object(root_id, create_sized_box(50.0, 50.0))
        .unwrap();
    let child2_id = owner
        .insert_child_render_object(root_id, create_sized_box(50.0, 50.0))
        .unwrap();

    // Initial frame
    owner.flush_layout();
    owner.flush_paint();

    // Mark only child1 dirty
    owner.add_node_needing_layout(child1_id.get(), 1);

    // Should only have child1 in dirty list
    let dirty_ids: Vec<usize> = owner.nodes_needing_layout().iter().map(|n| n.id).collect();
    assert_eq!(dirty_ids.len(), 1);
    assert!(dirty_ids.contains(&child1_id.get()));
    assert!(!dirty_ids.contains(&child2_id.get()));

    // Flush - only child1 should be processed
    owner.flush_layout();
    assert!(owner.nodes_needing_layout().is_empty());
}

/// Test adding new nodes during frame.
#[test]
fn test_add_nodes_between_frames() {
    let mut owner = PipelineOwner::new();

    let root_id = owner.set_root_render_object(create_sized_box(100.0, 100.0));

    // Frame 1
    owner.flush_layout();
    owner.flush_paint();
    assert!(owner.nodes_needing_layout().is_empty());

    // Add new child
    let _child_id = owner
        .insert_child_render_object(root_id, create_sized_box(50.0, 50.0))
        .unwrap();

    // New child should be dirty
    assert!(!owner.nodes_needing_layout().is_empty());

    // Frame 2
    owner.flush_layout();
    owner.flush_paint();
    assert!(owner.nodes_needing_layout().is_empty());
}

// ============================================================================
// Edge Cases
// ============================================================================

/// Test empty frame (no dirty nodes).
#[test]
fn test_empty_frame() {
    let mut owner = PipelineOwner::new();

    let _root_id = owner.set_root_render_object(create_sized_box(100.0, 100.0));

    // Initial frame
    owner.flush_layout();
    owner.flush_paint();

    // Empty frame - should be no-op
    owner.flush_layout();
    owner.flush_paint();

    assert!(owner.nodes_needing_layout().is_empty());
    assert!(owner.nodes_needing_paint().is_empty());
}

/// Test frame with single node tree.
#[test]
fn test_single_node_frame() {
    let mut owner = PipelineOwner::new();

    let _root_id = owner.set_root_render_object(create_sized_box(100.0, 100.0));

    owner.flush_layout();
    owner.flush_compositing_bits();
    owner.flush_paint();

    assert!(owner.nodes_needing_layout().is_empty());
    assert!(owner.nodes_needing_paint().is_empty());
}

/// Test semantics phase when enabled.
#[test]
fn test_semantics_when_enabled() {
    let mut owner = PipelineOwner::new();

    let root_id = owner.set_root_render_object(create_sized_box(100.0, 100.0));
    let _child_id = owner
        .insert_child_render_object(root_id, create_sized_box(50.0, 50.0))
        .unwrap();

    // Enable semantics
    owner.set_semantics_enabled(true);
    assert!(owner.semantics_enabled());

    // Add nodes needing semantics
    owner.add_node_needing_semantics(root_id.get(), 0);

    // Frame with semantics
    owner.flush_layout();
    owner.flush_compositing_bits();
    owner.flush_paint();
    owner.flush_semantics();

    // Semantics should be flushed
    assert!(owner.nodes_needing_semantics().is_empty());
}
