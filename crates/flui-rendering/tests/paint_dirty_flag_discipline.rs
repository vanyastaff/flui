//! Paint dirty-flag discipline.
//!
//! Validates the Core.0 exit criterion: "a RepaintBoundary-isolated repaint
//! clears `needs_paint` only on painted nodes." The paint walk
//! (`paint_subtree`) clears `needs_paint` on each node it visits; nodes not
//! reached by the root descent retain their flag until the residue scan at
//! the end of `run_paint` emits a warning + force-clears.
//!
//! Refs:
//!   * crates/flui-objects/src/proxy/repaint_boundary.rs
//!   * crates/flui-rendering/src/pipeline/owner/mod.rs — `run_paint`,
//!     `paint_subtree`

use flui_foundation::LayerId;
use flui_layer::{Layer, LayerTree};
use flui_objects::{RenderColoredBox, RenderPadding, RenderRepaintBoundary};
use flui_rendering::{constraints::BoxConstraints, pipeline::PipelineOwner, traits::RenderObject};

// ============================================================================
// Test 1 — RepaintBoundary bootstrap sets IS_REPAINT_BOUNDARY flag true
// ============================================================================

// ============================================================================
// Test 2 — paint clears needs_paint on all painted nodes
// ============================================================================

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
    owner.set_root_constraints(Some(BoxConstraints::new(0.0, 200.0, 0.0, 200.0)));

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
    owner.mark_needs_paint(leaf_id);

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

fn layer_tree_has_picture(tree: &LayerTree) -> bool {
    fn walk(tree: &LayerTree, id: LayerId) -> bool {
        let Some(node) = tree.get(id) else {
            return false;
        };
        matches!(node.layer(), Layer::Picture(_))
            || node.children().iter().any(|&child| walk(tree, child))
    }
    walk(tree, tree.root())
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
    owner.set_root_constraints(Some(BoxConstraints::new(0.0, 200.0, 0.0, 200.0)));

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

    let mut owner = owner.into_idle();
    owner
        .render_tree()
        .get(child_id)
        .expect("child")
        .mark_layout_flag();
    owner.mark_needs_paint(root_id);

    let owner = owner.into_layout();
    let owner = owner.into_compositing();
    let mut owner = owner.into_paint();
    owner.run_paint().expect("repaint with stale child layout");

    assert!(
        !owner.layer_tree().is_some_and(layer_tree_has_picture),
        "needs_layout node must not paint stale geometry",
    );
}
