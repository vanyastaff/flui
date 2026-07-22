//! Removal is the dispose site — dirty entries die WITH the subtree.
//!
//! A `Drop` impl has no `&PipelineOwner`, so it cannot evict
//! dirty-queue entries; `PipelineOwner::remove_render_object` is the
//! one place removal and owner-side disposal stay in lockstep.
//! Without it, every removal left stale queue entries for the next
//! phase to warn about — and any future retained state would have
//! dangled outright.

use flui_objects::RenderColoredBox;
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
    owner.add_node_needing_paint(child, 1);
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
        .insert_child_render_object(parent, Box::new(RenderColoredBox::blue(10.0, 10.0)))
        .expect("kept child");
    let drop_me = owner
        .insert_child_render_object(parent, Box::new(RenderColoredBox::green(10.0, 10.0)))
        .expect("dropped child");
    owner.set_root_id(Some(parent));

    owner.clear_all_dirty_nodes();
    owner.add_node_needing_paint(keep, 1);
    owner.add_node_needing_paint(drop_me, 1);
    assert_eq!(owner.dirty_node_count(), 2);

    let removed = owner.remove_render_object(drop_me);
    assert_eq!(removed, 1);
    assert_eq!(
        owner.dirty_node_count(),
        1,
        "only the removed subtree's entries are evicted — the sibling's \
         pending paint survives",
    );
    assert!(owner.render_tree().get(keep).is_some());
}
