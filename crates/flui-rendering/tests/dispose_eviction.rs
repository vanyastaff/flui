//! Removal is the dispose site — dirty entries die WITH the subtree.
//!
//! A `Drop` impl has no `&PipelineOwner`, so it cannot evict
//! dirty-queue entries; `PipelineOwner::remove_render_object` is the
//! one place removal and owner-side disposal stay in lockstep.
//! Without it, every removal left stale queue entries for the next
//! phase to warn about — and any future retained state would have
//! dangled outright.

use flui_objects::{RenderColoredBox, RenderRepaintBoundary};
use flui_rendering::pipeline::PipelineOwner;

type BoxedRenderObject =
    Box<dyn flui_rendering::traits::RenderObject<flui_rendering::protocol::BoxProtocol>>;

#[test]
fn removing_a_subtree_evicts_its_dirty_entries() {
    let mut owner = PipelineOwner::new();
    let parent = owner.insert(Box::new(RenderColoredBox::red(10.0, 10.0)) as BoxedRenderObject);
    let child = owner
        .insert_child_render_object(parent, Box::new(RenderColoredBox::blue(10.0, 10.0)))
        .expect("child insert");
    owner.set_root_id(Some(parent));

    // insert already queued both; pile on explicit marks too.
    owner.mark_needs_layout(child);
    owner.mark_needs_paint(child);
    assert!(owner.has_dirty_nodes());

    let removed = owner.remove_render_object(parent);
    assert_eq!(removed, 2, "parent + child must both be removed");
    assert!(
        !owner.has_dirty_nodes(),
        "every dirty entry of the removed subtree must be evicted — a \
         freed slot's queue entry would otherwise reach the next phase",
    );
    assert!(
        owner.root_id().is_none(),
        "removing the root clears root_id",
    );
    assert!(
        owner.render_tree().get(parent).is_none(),
        "stale parent id must not resolve (generation bumped)",
    );
}

#[test]
fn removing_one_subtree_keeps_sibling_entries() {
    let mut owner = PipelineOwner::new();
    let parent = owner.insert(Box::new(RenderColoredBox::red(10.0, 10.0)) as BoxedRenderObject);
    let keep = owner
        .insert_child_render_object(parent, Box::new(RenderRepaintBoundary::new()))
        .expect("kept child");
    let drop_me = owner
        .insert_child_render_object(parent, Box::new(RenderRepaintBoundary::new()))
        .expect("dropped child");
    owner.set_root_id(Some(parent));
    owner.set_semantics_enabled(true);

    owner.clear_all_dirty_nodes();
    let parent_node = owner.render_tree().get(parent).expect("parent");
    parent_node.clear_needs_paint();
    parent_node.clear_needs_compositing_bits_update();
    match owner.render_tree_mut().get_mut(parent).expect("parent") {
        flui_rendering::storage::RenderNode::Box(entry) => entry.state().clear_needs_layout(),
        flui_rendering::storage::RenderNode::Sliver(entry) => entry.state().clear_needs_layout(),
    }
    owner
        .render_tree()
        .get(keep)
        .expect("kept boundary")
        .set_was_repaint_boundary(true);
    owner
        .render_tree()
        .get(keep)
        .expect("kept boundary")
        .clear_needs_paint();
    owner
        .render_tree()
        .get(drop_me)
        .expect("dropped boundary")
        .set_was_repaint_boundary(true);
    owner
        .render_tree()
        .get(drop_me)
        .expect("dropped boundary")
        .clear_needs_paint();
    owner.mark_needs_paint(keep);
    owner.mark_needs_paint(drop_me);
    assert_eq!(owner.dirty_node_count(), 2);

    let removed = owner.remove_render_object(drop_me);
    assert_eq!(removed, 1);
    assert_eq!(owner.nodes_needing_paint().len(), 1);
    assert_eq!(owner.nodes_needing_paint()[0].id, keep);
    assert_eq!(owner.nodes_needing_layout()[0].id, parent);
    assert_eq!(owner.nodes_needing_compositing_bits_update()[0].id, parent);
    assert_eq!(owner.nodes_needing_semantics()[0].id, parent);
    let parent_node = owner.render_tree().get(parent).expect("surviving parent");
    assert!(parent_node.needs_layout());
    assert!(parent_node.needs_compositing_bits_update());
    assert!(
        !parent_node.needs_paint(),
        "layout owns the parent's eventual paint; no immediate parent paint mark"
    );
    assert!(owner.render_tree().get(keep).is_some());
}
