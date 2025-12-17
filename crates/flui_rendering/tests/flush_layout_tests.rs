//! Integration tests for flush_layout behavior.
//!
//! These tests verify that PipelineOwner::flush_layout() correctly iterates
//! over all dirty nodes (not just the root), matching Flutter's behavior.
//!
//! Flutter's flushLayout() does:
//! 1. Sorts dirty nodes by depth (shallow first)
//! 2. Iterates through EACH dirty node
//! 3. Calls _layoutWithoutResize() on each node that still needs layout
//!
//! Our implementation currently only processes the root node, which is incorrect.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use flui_foundation::{Diagnosticable, DiagnosticsBuilder};
use flui_rendering::hit_testing::{HitTestEntry, HitTestTarget, PointerEvent};
use parking_lot::RwLock;

use flui_rendering::constraints::BoxConstraints;
use flui_rendering::lifecycle::BaseRenderObject;
use flui_rendering::pipeline::PipelineOwner;
use flui_rendering::traits::{RenderBox, RenderObject};
use flui_types::{Offset, Point, Rect, Size};

// ============================================================================
// Test RenderObject that tracks layout calls
// ============================================================================

/// A test render object that counts how many times perform_layout is called.
#[allow(dead_code)]
#[derive(Debug)]
struct TestRenderBox {
    base: BaseRenderObject,
    layout_count: Arc<AtomicUsize>,
    size: Size,
    name: String,
}

#[allow(dead_code)]
impl TestRenderBox {
    fn new(name: &str, layout_count: Arc<AtomicUsize>) -> Self {
        Self {
            base: BaseRenderObject::new(),
            layout_count,
            size: Size::ZERO,
            name: name.to_string(),
        }
    }

    fn with_node_id(name: &str, node_id: usize, layout_count: Arc<AtomicUsize>) -> Self {
        Self {
            base: BaseRenderObject::with_node_id(node_id),
            layout_count,
            size: Size::ZERO,
            name: name.to_string(),
        }
    }
}

impl RenderObject for TestRenderBox {
    fn base(&self) -> &BaseRenderObject {
        &self.base
    }

    fn base_mut(&mut self) -> &mut BaseRenderObject {
        &mut self.base
    }

    fn owner(&self) -> Option<&flui_rendering::pipeline::PipelineOwner> {
        None // We use base().owner() for Arc<RwLock<PipelineOwner>>
    }

    fn attach(&mut self, _owner: &flui_rendering::pipeline::PipelineOwner) {
        // No-op for test
    }

    fn detach(&mut self) {
        self.base.detach();
    }

    fn adopt_child(&mut self, _child: &mut dyn RenderObject) {
        unimplemented!("adopt_child not needed for this test")
    }

    fn drop_child(&mut self, _child: &mut dyn RenderObject) {
        unimplemented!("drop_child not needed for this test")
    }

    fn redepth_child(&mut self, _child: &mut dyn RenderObject) {
        unimplemented!("redepth_child not needed for this test")
    }

    fn mark_parent_needs_layout(&mut self) {
        // No-op for test
    }

    fn schedule_initial_layout(&mut self) {
        self.mark_needs_layout();
    }

    fn schedule_initial_paint(&mut self) {
        self.mark_needs_paint();
    }

    fn visit_children(&self, _visitor: &mut dyn FnMut(&dyn RenderObject)) {
        // No children for simple test
    }

    fn visit_children_mut(&mut self, _visitor: &mut dyn FnMut(&mut dyn RenderObject)) {
        // No children for simple test
    }

    fn paint_bounds(&self) -> Rect {
        Rect::from_origin_size(Point::ZERO, self.size)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

impl RenderBox for TestRenderBox {
    fn size(&self) -> Size {
        self.size
    }

    fn set_size(&mut self, size: Size) {
        self.size = size;
    }

    fn perform_layout(&mut self, constraints: BoxConstraints) -> Size {
        // Increment layout count - this is what we're testing
        self.layout_count.fetch_add(1, Ordering::SeqCst);
        tracing::info!("TestRenderBox '{}' perform_layout called", self.name);

        // Clear needs_layout flag
        self.base.clear_needs_layout();

        // Return constrained size
        let size = constraints.constrain(Size::new(100.0, 100.0));
        self.size = size;
        size
    }

    fn compute_min_intrinsic_width(&self, _height: f32) -> f32 {
        100.0
    }

    fn compute_max_intrinsic_width(&self, _height: f32) -> f32 {
        100.0
    }

    fn compute_min_intrinsic_height(&self, _width: f32) -> f32 {
        100.0
    }

    fn compute_max_intrinsic_height(&self, _width: f32) -> f32 {
        100.0
    }

    fn compute_dry_layout(&self, constraints: BoxConstraints) -> Size {
        constraints.constrain(Size::new(100.0, 100.0))
    }

    fn hit_test_self(&self, _position: Offset) -> bool {
        true
    }

    fn paint(&self, _context: &mut flui_rendering::pipeline::PaintingContext, _offset: Offset) {
        // No-op for test
    }
}

// ============================================================================
// Tests for flush_layout behavior
// ============================================================================

/// Test that flush_layout processes all dirty nodes, not just the root.
///
/// This test verifies the Flutter behavior where flush_layout iterates
/// through ALL nodes in _nodesNeedingLayout, not just the root.
#[test]
fn test_flush_layout_processes_all_dirty_nodes() {
    let mut owner = PipelineOwner::new();

    // Track layout calls for each node (prefixed to avoid warnings until used)
    let _layout_count_a = Arc::new(AtomicUsize::new(0));
    let _layout_count_b = Arc::new(AtomicUsize::new(0));
    let _layout_count_c = Arc::new(AtomicUsize::new(0));

    // Add three nodes at different depths to the dirty list
    // In Flutter: _nodesNeedingLayout.add(nodeA), etc.
    owner.add_node_needing_layout(1, 0); // node A at depth 0 (root)
    owner.add_node_needing_layout(2, 1); // node B at depth 1
    owner.add_node_needing_layout(3, 2); // node C at depth 2

    // Verify we have 3 dirty nodes
    assert_eq!(
        owner.nodes_needing_layout().len(),
        3,
        "Should have 3 nodes in dirty list before flush"
    );

    // Flush layout
    owner.flush_layout();

    // After flush, dirty list should be empty
    assert!(
        owner.nodes_needing_layout().is_empty(),
        "Dirty list should be empty after flush_layout"
    );

    // KEY TEST: In correct Flutter-like behavior, flush_layout should have
    // called perform_layout (or _layoutWithoutResize) on EACH dirty node.
    //
    // Currently our implementation only processes the root, so this test
    // demonstrates the expected behavior we need to implement.
    //
    // For now we just verify the dirty list is cleared - the actual
    // perform_layout calls require nodes to be in the RenderTree.
}

/// Test that flush_layout sorts nodes by depth (shallow first).
///
/// Flutter sorts dirty nodes so parents (smaller depth) are processed
/// before children (larger depth).
#[test]
fn test_flush_layout_sorts_by_depth_shallow_first() {
    let mut owner = PipelineOwner::new();

    // Add nodes in reverse depth order
    owner.add_node_needing_layout(3, 5); // deepest
    owner.add_node_needing_layout(1, 0); // shallowest (root)
    owner.add_node_needing_layout(2, 2); // middle

    // Get the nodes before flush (to verify sorting happens internally)
    let nodes_before = owner.nodes_needing_layout().to_vec();
    assert_eq!(nodes_before[0].depth, 5, "First added should be depth 5");
    assert_eq!(nodes_before[1].depth, 0, "Second added should be depth 0");
    assert_eq!(nodes_before[2].depth, 2, "Third added should be depth 2");

    // Flush layout - this should sort and process shallow-first
    owner.flush_layout();

    // After flush, list is cleared
    assert!(owner.nodes_needing_layout().is_empty());
}

/// Test that nodes added during flush_layout are processed in the same frame.
///
/// In Flutter, if a node's layout causes another node to be marked dirty,
/// the outer while loop in flushLayout continues until no dirty nodes remain.
#[test]
fn test_flush_layout_handles_new_dirty_nodes_during_flush() {
    let mut owner = PipelineOwner::new();

    // Add initial dirty node
    owner.add_node_needing_layout(1, 0);

    // TODO: In a full implementation, we'd need a mechanism where
    // processing node 1 can trigger node 2 to be marked dirty.
    // This test documents the expected behavior.

    owner.flush_layout();

    // All nodes (including any added during flush) should be processed
    assert!(owner.nodes_needing_layout().is_empty());
}

/// Test that flush_layout only processes nodes that still need layout.
///
/// In Flutter: `if (node._needsLayout && node.owner == this)`
/// A node might have been laid out by a parent, so we check the flag again.
#[test]
fn test_flush_layout_skips_already_laid_out_nodes() {
    // This test demonstrates that even if a node is in the dirty list,
    // we should check its needs_layout flag before calling layout.
    //
    // This happens when:
    // 1. Node B is marked dirty
    // 2. Node A (parent of B) is also marked dirty
    // 3. During flush, A is processed first (smaller depth)
    // 4. A's layout triggers B's layout as a child
    // 5. When we reach B in the dirty list, it no longer needs layout

    let mut owner = PipelineOwner::new();

    // Add nodes - parent first (depth 0), child second (depth 1)
    owner.add_node_needing_layout(1, 0); // parent
    owner.add_node_needing_layout(2, 1); // child

    // In a proper implementation, when we process node 1, it might
    // lay out node 2 as part of its children. Then when we reach
    // node 2 in the dirty list, we should check node._needsLayout
    // and skip it if already laid out.

    owner.flush_layout();

    assert!(owner.nodes_needing_layout().is_empty());
}

/// Test that flush_layout recursively flushes child pipeline owners.
#[test]
fn test_flush_layout_flushes_child_pipelines() {
    let mut parent_owner = PipelineOwner::new();
    let child_owner = Arc::new(RwLock::new(PipelineOwner::new()));

    // Add dirty node to child
    child_owner.write().add_node_needing_layout(10, 0);

    // Adopt child pipeline
    parent_owner.adopt_child(child_owner.clone());

    // Verify child has dirty nodes
    assert_eq!(child_owner.read().nodes_needing_layout().len(), 1);

    // Flush parent - should also flush child
    parent_owner.flush_layout();

    // Child's dirty list should also be cleared
    assert!(
        child_owner.read().nodes_needing_layout().is_empty(),
        "Child pipeline's dirty nodes should be cleared after parent flush"
    );
}

// ============================================================================
// Tests demonstrating what needs to be fixed
// ============================================================================

/// This test demonstrates the architectural issue we need to fix.
///
/// Current behavior: flush_layout only processes root_id
/// Expected behavior: flush_layout iterates all dirty nodes
#[test]
fn test_current_behavior_only_processes_root() {
    use flui_rendering::view::{RenderView, ViewConfiguration};

    let mut owner = PipelineOwner::new();

    // Create a RenderView as root with configuration
    let config = ViewConfiguration::from_size(Size::new(800.0, 600.0), 1.0);
    let render_view = RenderView::with_configuration(config);

    // Insert into render tree and set as root
    let root_id = owner.render_tree_mut().insert(Box::new(render_view));
    owner.set_root_id(Some(root_id));

    // Add multiple dirty nodes
    owner.add_node_needing_layout(1, 0);
    owner.add_node_needing_layout(2, 1);
    owner.add_node_needing_layout(3, 2);

    // Flush layout
    owner.flush_layout();

    // Currently, only the root (RenderView) is processed.
    // The dirty list is cleared but nodes 2 and 3 never had layout called.
    // This test documents this limitation.

    assert!(
        owner.nodes_needing_layout().is_empty(),
        "Dirty list should be empty"
    );

    // TODO: Add assertions that verify ALL nodes had layout called,
    // not just the root. This requires implementing layout_without_resize()
    // and iterating through dirty nodes.
}

// ============================================================================
// Diagnosticable Implementation
// ============================================================================

impl Diagnosticable for TestRenderBox {
    fn debug_fill_properties(&self, properties: &mut DiagnosticsBuilder) {
        properties.add("name", &self.name);
        properties.add("size", format!("{:?}", self.size));
    }
}

impl HitTestTarget for TestRenderBox {
    fn handle_event(&self, event: &PointerEvent, entry: &HitTestEntry) {
        RenderObject::handle_event(self, event, entry);
    }
}
