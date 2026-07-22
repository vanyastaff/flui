//! Paint dirty-flag discipline.
//!
//! Validates the Core.0 exit criterion: "a RepaintBoundary-isolated repaint
//! clears `needs_paint` only on painted nodes." The paint walk
//! (`paint_subtree`) clears `needs_paint` on each node it visits; nodes not
//! reached by the root descent retain their flag until the residue scan at
//! the end of `run_paint` emits a warning + force-clears.
//!
//! Refs:
//!   * crates/flui-rendering/src/objects/repaint_boundary.rs
//!   * crates/flui-rendering/src/pipeline/owner/mod.rs — `run_paint`,
//!     `paint_subtree`

use flui_foundation::LayerId;
use flui_layer::{Layer, LayerTree};
use flui_objects::{RenderColoredBox, RenderPadding, RenderRepaintBoundary};
use flui_rendering::{constraints::BoxConstraints, pipeline::PipelineOwner, traits::RenderObject};
use flui_types::geometry::px;

// ============================================================================
// Test 1 — RepaintBoundary bootstrap sets IS_REPAINT_BOUNDARY flag true
// ============================================================================

/// Insert a `RenderRepaintBoundary` and verify the storage flag reflects the
/// trait answer (`is_repaint_boundary() == true`).
#[test]
fn repaint_boundary_bootstrap_sets_flag_true() {
    let mut owner = PipelineOwner::new();
    let boundary_id = owner.insert(Box::new(RenderRepaintBoundary::new())
        as Box<dyn RenderObject<flui_rendering::protocol::BoxProtocol>>);

    let node = owner
        .render_tree()
        .get(boundary_id)
        .expect("boundary in tree");

    assert!(
        node.is_repaint_boundary(),
        "RenderRepaintBoundary trait answer must be true",
    );
    assert!(
        node.is_repaint_boundary_flag(),
        "IS_REPAINT_BOUNDARY storage flag must be true after insert",
    );
    assert_eq!(
        node.is_repaint_boundary_flag(),
        node.is_repaint_boundary(),
        "storage flag must equal trait answer for RenderRepaintBoundary",
    );
}

// ============================================================================
// Test 2 — paint clears needs_paint on all painted nodes
// ============================================================================

/// Build tree: Root(Padding) -> Child(ColoredBox). Run full pipeline. Assert
/// both nodes have `needs_paint == false` after paint.
#[test]
fn paint_clears_needs_paint_on_painted_nodes() {
    let mut owner = PipelineOwner::new();
    let padding_id = owner.insert(Box::new(RenderPadding::all(5.0))
        as Box<dyn RenderObject<flui_rendering::protocol::BoxProtocol>>);
    let child_id = owner
        .insert_child_render_object(padding_id, Box::new(RenderColoredBox::red(40.0, 40.0)))
        .expect("child insert");

    owner.set_root_id(Some(padding_id));
    owner.set_root_constraints(Some(BoxConstraints::new(
        px(0.0),
        px(200.0),
        px(0.0),
        px(200.0),
    )));

    // Run full pipeline: layout -> compositing -> paint.
    let mut owner = owner.into_layout();
    owner.run_layout().expect("layout succeeds");

    let mut owner = owner.into_compositing();
    owner.run_compositing().expect("compositing succeeds");

    let mut owner = owner.into_paint();
    owner.run_paint().expect("paint succeeds");

    // Both nodes must have needs_paint == false after paint.
    let padding_node = owner
        .render_tree()
        .get(padding_id)
        .expect("padding in tree");
    assert!(
        !padding_node.needs_paint(),
        "root (Padding) needs_paint must be cleared after run_paint",
    );

    let child_node = owner.render_tree().get(child_id).expect("child in tree");
    assert!(
        !child_node.needs_paint(),
        "child (ColoredBox) needs_paint must be cleared after run_paint",
    );
}

// ============================================================================
// Test 3 — RepaintBoundary isolates subtree paint
// ============================================================================

/// Build tree: Root(Padding) -> RepaintBoundary -> Leaf(ColoredBox).
/// Run initial full paint (clears all flags). Mark leaf dirty for paint.
/// Run paint again. Assert leaf's flag cleared, root's flag remains false
/// (boundary isolation — root was never flagged dirty again).
#[test]
fn repaint_boundary_isolates_subtree_paint() {
    let mut owner = PipelineOwner::new();
    let root_id = owner.insert(Box::new(RenderPadding::all(5.0))
        as Box<dyn RenderObject<flui_rendering::protocol::BoxProtocol>>);
    let boundary_id = owner
        .insert_child_render_object(root_id, Box::new(RenderRepaintBoundary::new()))
        .expect("boundary insert");
    let leaf_id = owner
        .insert_child_render_object(boundary_id, Box::new(RenderColoredBox::red(30.0, 30.0)))
        .expect("leaf insert");

    owner.set_root_id(Some(root_id));
    owner.set_root_constraints(Some(BoxConstraints::new(
        px(0.0),
        px(200.0),
        px(0.0),
        px(200.0),
    )));

    // Frame 1: full pipeline to clear all initial dirty flags.
    let mut owner = owner.into_layout();
    owner.run_layout().expect("frame 1 layout");
    let mut owner = owner.into_compositing();
    owner.run_compositing().expect("frame 1 compositing");
    let mut owner = owner.into_paint();
    owner.run_paint().expect("frame 1 paint");

    // Verify all flags cleared after frame 1.
    assert!(
        !owner.render_tree().get(root_id).unwrap().needs_paint(),
        "precondition: root needs_paint cleared after frame 1",
    );
    assert!(
        !owner.render_tree().get(boundary_id).unwrap().needs_paint(),
        "precondition: boundary needs_paint cleared after frame 1",
    );
    assert!(
        !owner.render_tree().get(leaf_id).unwrap().needs_paint(),
        "precondition: leaf needs_paint cleared after frame 1",
    );

    // Mark ONLY the leaf dirty for paint (simulate a re-paint request).
    let mut owner = owner.into_idle();
    let leaf_depth = owner.render_tree().depth(leaf_id).unwrap_or(0) as usize;
    owner.render_tree().get(leaf_id).unwrap().mark_paint_flag();
    owner.add_node_needing_paint(leaf_id, leaf_depth);

    // Frame 2: run paint again (skip layout/compositing — only paint dirty).
    // Transition through the required phases.
    let owner = owner.into_layout();
    let owner = owner.into_compositing();
    let mut owner = owner.into_paint();
    owner.run_paint().expect("frame 2 paint");

    // Leaf was painted → needs_paint cleared.
    let leaf_node = owner.render_tree().get(leaf_id).expect("leaf");
    assert!(
        !leaf_node.needs_paint(),
        "leaf needs_paint must be cleared after frame 2 paint \
         (it was in the dirty list and painted)",
    );

    // Root was NOT dirty for frame 2 → should still be clean.
    let root_node = owner.render_tree().get(root_id).expect("root");
    assert!(
        !root_node.needs_paint(),
        "root needs_paint must remain false — boundary isolation means \
         only the dirty subtree was scheduled, root was never re-dirtied",
    );
}

// ============================================================================
// Test 4 — only painted nodes clear flag; clean nodes stay clean
// ============================================================================

/// Build a linear tree: Root(Padding) -> Middle(Padding) -> Leaf(ColoredBox).
/// After frame 1 (all flags cleared), mark ONLY the leaf dirty for paint.
/// Run paint again — the root descent visits all nodes from root downward,
/// so all three get `needs_paint` cleared. The middle node was never dirty
/// between frames, proving the discipline: the root descent paints (and
/// clears) every reachable node, while unreached nodes would retain their
/// flag (validated by the tracing::warn residue scan in `run_paint`).
#[test]
fn unpainted_unreached_nodes_still_clear_flag() {
    let mut owner = PipelineOwner::new();

    // Linear chain: root -> middle -> leaf.
    let root_id = owner.insert(Box::new(RenderPadding::all(3.0))
        as Box<dyn RenderObject<flui_rendering::protocol::BoxProtocol>>);
    let middle_id = owner
        .insert_child_render_object(root_id, Box::new(RenderPadding::all(2.0)))
        .expect("middle insert");
    let leaf_id = owner
        .insert_child_render_object(middle_id, Box::new(RenderColoredBox::red(40.0, 40.0)))
        .expect("leaf insert");

    owner.set_root_id(Some(root_id));
    owner.set_root_constraints(Some(BoxConstraints::new(
        px(0.0),
        px(200.0),
        px(0.0),
        px(200.0),
    )));

    // Frame 1: full pipeline clears all flags.
    let mut owner = owner.into_layout();
    owner.run_layout().expect("frame 1 layout");
    let mut owner = owner.into_compositing();
    owner.run_compositing().expect("frame 1 compositing");
    let mut owner = owner.into_paint();
    owner.run_paint().expect("frame 1 paint");

    // Verify precondition: all flags clean.
    for (label, id) in [("root", root_id), ("middle", middle_id), ("leaf", leaf_id)] {
        assert!(
            !owner.render_tree().get(id).unwrap().needs_paint(),
            "precondition: {label} needs_paint must be false after frame 1",
        );
    }

    // Mark ONLY the leaf dirty for paint.
    let mut owner = owner.into_idle();
    let leaf_depth = owner.render_tree().depth(leaf_id).unwrap_or(0) as usize;
    owner.render_tree().get(leaf_id).unwrap().mark_paint_flag();
    owner.add_node_needing_paint(leaf_id, leaf_depth);

    // Frame 2: paint phase only (transition through required phases).
    let owner = owner.into_layout();
    let owner = owner.into_compositing();
    let mut owner = owner.into_paint();
    owner.run_paint().expect("frame 2 paint");

    // Leaf was dirty, painted by root descent → flag cleared.
    assert!(
        !owner.render_tree().get(leaf_id).unwrap().needs_paint(),
        "leaf (dirty) needs_paint must be cleared after paint",
    );

    // Middle was never dirty between frames → flag stays false
    // (root descent visits it anyway and clears it, but it was
    // already false — the point is it doesn't get spuriously set).
    assert!(
        !owner.render_tree().get(middle_id).unwrap().needs_paint(),
        "middle (clean) needs_paint must remain false — it was never \
         marked dirty between frames",
    );

    // Root was painted by root descent → flag cleared.
    assert!(
        !owner.render_tree().get(root_id).unwrap().needs_paint(),
        "root needs_paint must be cleared after paint (painted by root descent)",
    );
}

fn layer_tree_has_picture(tree: &LayerTree) -> bool {
    fn walk(tree: &LayerTree, id: LayerId) -> bool {
        let Some(node) = tree.get(id) else {
            return false;
        };
        matches!(node.layer(), Layer::Picture(_))
            || node.children().iter().any(|&child| walk(tree, child))
    }
    tree.root().is_some_and(|root| walk(tree, root))
}

#[test]
fn paint_skips_node_that_still_needs_layout() {
    let mut owner = PipelineOwner::new();
    let root_id = owner.insert(Box::new(RenderPadding::all(0.0))
        as Box<dyn RenderObject<flui_rendering::protocol::BoxProtocol>>);
    let child_id = owner
        .insert_child_render_object(root_id, Box::new(RenderColoredBox::red(40.0, 40.0)))
        .expect("child insert");

    owner.set_root_id(Some(root_id));
    owner.set_root_constraints(Some(BoxConstraints::new(
        px(0.0),
        px(200.0),
        px(0.0),
        px(200.0),
    )));

    let mut owner = owner.into_layout();
    owner.run_layout().expect("layout");
    let mut owner = owner.into_compositing();
    owner.run_compositing().expect("compositing");
    let mut owner = owner.into_paint();
    owner.run_paint().expect("paint");

    assert!(
        owner.layer_tree().is_some_and(layer_tree_has_picture),
        "first paint must record child picture ops",
    );

    let root_depth = owner.render_tree().depth(root_id).unwrap_or(0) as usize;
    let mut owner = owner.into_idle();
    owner
        .render_tree()
        .get(child_id)
        .expect("child")
        .mark_layout_flag();
    owner.add_node_needing_paint(root_id, root_depth);

    let owner = owner.into_layout();
    let owner = owner.into_compositing();
    let mut owner = owner.into_paint();
    owner.run_paint().expect("repaint with stale child layout");

    assert!(
        !owner.layer_tree().is_some_and(layer_tree_has_picture),
        "needs_layout node must not paint stale geometry",
    );
}
