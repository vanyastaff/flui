//! Integration tests for RenderTree dirty marking behavior.
//!
//! These tests verify that when RenderObjects are added to the tree,
//! they are properly connected to PipelineOwner and marked dirty.
//!
//! In Flutter:
//! 1. When a child is adopted, it gets attached to the same PipelineOwner as parent
//! 2. On attach, if needs_layout is true, the node is added to _nodesNeedingLayout
//! 3. flush_layout then processes all dirty nodes

use std::sync::Arc;

use parking_lot::RwLock;

use flui_rendering::objects::r#box::basic::{RenderPadding, RenderSizedBox};
use flui_rendering::pipeline::PipelineOwner;
use flui_rendering::traits::RenderObject;
use flui_rendering::view::{RenderView, ViewConfiguration};
use flui_types::{EdgeInsets, Size};

// ============================================================================
// Test: Children in RenderTree should be tracked in dirty lists
// ============================================================================

/// Test that inserting a child using insert_child_render_object adds it to dirty list.
///
/// This test verifies the Flutter-like behavior:
/// 1. Create PipelineOwner with RenderTree
/// 2. Insert root RenderView using set_root_render_object
/// 3. Insert child RenderPadding using insert_child_render_object
/// 4. Verify child is in nodes_needing_layout list
#[test]
fn test_insert_child_adds_to_dirty_list() {
    let mut owner = PipelineOwner::new();

    // Create and insert root using the new method
    let config = ViewConfiguration::from_size(Size::new(800.0, 600.0), 1.0);
    let render_view = RenderView::with_configuration(config);
    let root_id = owner.set_root_render_object(Box::new(render_view));

    // Insert child using the new method that tracks dirty state
    let padding = RenderPadding::new(EdgeInsets::all(10.0));
    let child_id = owner.insert_child_render_object(root_id, Box::new(padding));

    assert!(child_id.is_some(), "Child should be inserted");

    // Check dirty list - now it should contain the child
    let dirty_count = owner.nodes_needing_layout().len();

    println!("Dirty nodes count: {}", dirty_count);
    println!("Dirty nodes: {:?}", owner.nodes_needing_layout());

    // With the new insert_child_render_object method, the child should be in dirty list
    assert!(
        dirty_count >= 2,
        "Should have at least root and child in dirty list, got {}",
        dirty_count
    );
}

/// Test that children inherit PipelineOwner on attach.
///
/// In Flutter, when a RenderObject is attached:
/// 1. Its owner is set
/// 2. If needs_layout, it's added to owner's dirty list
/// 3. All children are recursively attached
#[test]
fn test_attach_propagates_to_children() {
    let owner = Arc::new(RwLock::new(PipelineOwner::new()));

    // Create a tree manually: RenderView -> RenderPadding -> RenderSizedBox
    let config = ViewConfiguration::from_size(Size::new(800.0, 600.0), 1.0);

    // Create sized box (innermost)
    let sized_box = RenderSizedBox::fixed(100.0, 50.0);

    // Create padding with child
    let padding = RenderPadding::with_child(EdgeInsets::all(10.0), Box::new(sized_box));

    // Create render view with child
    let mut render_view = RenderView::with_child(config, Box::new(padding));

    // Before attach - nothing should be in owner's dirty list
    assert!(
        owner.read().nodes_needing_layout().is_empty(),
        "No dirty nodes before attach"
    );

    // Attach the tree to owner
    // This should recursively attach children and add them to dirty list
    {
        let owner_guard = owner.read();
        render_view.attach(&owner_guard);
    }

    // After attach - nodes should be in dirty list
    // NOTE: This uses the "old" architecture where children are inside RenderObjects
    // The dirty marking happens via BaseRenderObject.attach() which calls schedule_layout_with_owner()

    // Check if anything was added to dirty list
    let dirty_count = owner.read().nodes_needing_layout().len();
    println!("Dirty nodes after attach: {}", dirty_count);
    println!("Dirty nodes: {:?}", owner.read().nodes_needing_layout());

    // In the old architecture, this might work because:
    // - RenderView stores child internally
    // - attach() calls child.attach() recursively
    // - But BaseRenderObject uses Arc<RwLock<PipelineOwner>>, not raw pointer
}

/// Test the full pipeline: insert, flush_layout using new methods
#[test]
fn test_full_layout_pipeline_with_tree() {
    let mut owner = PipelineOwner::new();

    // Setup RenderTree with root using new method
    let config = ViewConfiguration::from_size(Size::new(800.0, 600.0), 1.0);
    let render_view = RenderView::with_configuration(config);
    let root_id = owner.set_root_render_object(Box::new(render_view));

    // Add children using new method that tracks dirty state
    let padding = RenderPadding::new(EdgeInsets::all(10.0));
    let padding_id = owner.insert_child_render_object(root_id, Box::new(padding));
    assert!(padding_id.is_some());

    let sized_box = RenderSizedBox::fixed(100.0, 50.0);
    let sized_box_id = owner.insert_child_render_object(padding_id.unwrap(), Box::new(sized_box));
    assert!(sized_box_id.is_some());

    // Get tree structure info
    let tree = owner.render_tree();
    println!("Tree has {} nodes", tree.len());
    println!("Root: {:?}", owner.root_id());

    // Dirty list should be populated automatically by insert methods
    let dirty_before = owner.nodes_needing_layout().len();
    println!("Dirty nodes before flush: {}", dirty_before);
    println!("Dirty nodes: {:?}", owner.nodes_needing_layout());

    // All 3 nodes should be in dirty list
    assert!(
        dirty_before >= 3,
        "All 3 nodes should be in dirty list, got {}",
        dirty_before
    );

    // Now flush layout
    owner.flush_layout();

    let dirty_after = owner.nodes_needing_layout().len();
    println!("Dirty nodes after flush: {}", dirty_after);

    assert_eq!(dirty_after, 0, "All nodes should be processed");
}

/// Test that shows the architectural disconnect between RenderTree and dirty tracking.
///
/// The issue: RenderTree stores nodes but doesn't connect them to PipelineOwner.
/// When we insert_child, the child's BaseRenderObject doesn't know about the owner,
/// so it can't add itself to the dirty list.
#[test]
fn test_architectural_disconnect() {
    let mut owner = PipelineOwner::new();

    // Insert a RenderPadding directly
    let padding = RenderPadding::new(EdgeInsets::all(10.0));

    // The padding has needs_layout = true by default (from BaseRenderObject::new())
    // But when we insert it into RenderTree, it doesn't get connected to PipelineOwner

    let padding_id = owner.render_tree_mut().insert(Box::new(padding));

    // The RenderTree stores the node, but:
    // 1. The padding's BaseRenderObject.owner is None
    // 2. So mark_needs_layout() doesn't add to PipelineOwner's dirty list
    // 3. flush_layout() won't process it

    // Verify the node exists in tree
    assert!(owner.render_tree().get(padding_id).is_some());

    // But dirty list is empty because no attach happened
    assert!(
        owner.nodes_needing_layout().is_empty(),
        "Dirty list should be empty - nodes aren't connected to owner"
    );

    // This is the core issue we need to fix:
    // Option 1: RenderTree.insert() should attach nodes to owner
    // Option 2: Separate attach step after insertion
    // Option 3: Store owner reference in RenderTree and pass during insert
}
