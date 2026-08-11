use flui_objects::{RenderColoredBox, RenderSliverToBoxAdapter};
use flui_rendering::{pipeline::PipelineOwner, storage::RenderNode};

fn clear_node_dirty_flags(owner: &mut PipelineOwner, id: flui_foundation::RenderId) {
    let node = owner.render_tree().get(id).expect("inserted render node");
    node.clear_needs_paint();
    node.clear_needs_compositing_bits_update();
    match owner
        .render_tree_mut()
        .get_mut(id)
        .expect("inserted render node")
    {
        RenderNode::Box(entry) => entry.state().clear_needs_layout(),
        RenderNode::Sliver(entry) => entry.state().clear_needs_layout(),
    }
}

fn assert_exact_parent_membership_work(owner: &PipelineOwner, parent: flui_foundation::RenderId) {
    assert_eq!(
        owner
            .nodes_needing_layout()
            .iter()
            .filter(|entry| entry.id == parent)
            .count(),
        1
    );
    assert_eq!(
        owner
            .nodes_needing_compositing_bits_update()
            .iter()
            .filter(|entry| entry.id == parent)
            .count(),
        1,
    );
    assert_eq!(
        owner
            .nodes_needing_semantics()
            .iter()
            .filter(|entry| entry.id == parent)
            .count(),
        1
    );
    assert!(
        owner
            .nodes_needing_paint()
            .iter()
            .all(|entry| entry.id != parent)
    );
    let parent_node = owner
        .render_tree()
        .get(parent)
        .expect("parent remains mounted");
    assert!(parent_node.needs_layout());
    assert!(parent_node.needs_compositing_bits_update());
    assert!(!parent_node.needs_paint());
}

#[test]
fn box_child_insertion_applies_full_parent_membership_impact() {
    let mut owner = PipelineOwner::new();
    let parent = owner.set_root_render_object(Box::new(RenderColoredBox::red(10.0, 10.0)));
    owner.set_semantics_enabled(true);
    owner.clear_all_dirty_nodes();
    clear_node_dirty_flags(&mut owner, parent);

    let child = owner
        .insert_child_render_object(parent, Box::new(RenderColoredBox::blue(5.0, 5.0)))
        .expect("box child insertion");

    assert_exact_parent_membership_work(&owner, parent);
    assert_eq!(
        owner
            .nodes_needing_layout()
            .iter()
            .filter(|entry| entry.id == child)
            .count(),
        1
    );
}

#[test]
fn sliver_child_insertion_applies_full_parent_membership_impact() {
    let mut owner = PipelineOwner::new();
    let parent = owner.set_root_render_object(Box::new(RenderColoredBox::red(10.0, 10.0)));
    owner.set_semantics_enabled(true);
    owner.clear_all_dirty_nodes();
    clear_node_dirty_flags(&mut owner, parent);

    let child = owner
        .insert_sliver_child_render_object(parent, Box::new(RenderSliverToBoxAdapter::new()))
        .expect("sliver child insertion");

    assert_exact_parent_membership_work(&owner, parent);
    assert_eq!(
        owner
            .nodes_needing_layout()
            .iter()
            .filter(|entry| entry.id == child)
            .count(),
        1
    );
}

#[test]
fn dropping_a_child_applies_full_parent_membership_impact() {
    let mut owner = PipelineOwner::new();
    let parent = owner.set_root_render_object(Box::new(RenderColoredBox::red(10.0, 10.0)));
    owner.set_semantics_enabled(true);
    let child = owner
        .insert_child_render_object(parent, Box::new(RenderColoredBox::blue(5.0, 5.0)))
        .expect("box child insertion");
    owner.clear_all_dirty_nodes();
    clear_node_dirty_flags(&mut owner, parent);
    clear_node_dirty_flags(&mut owner, child);

    owner.drop_render_child(parent, child);

    assert_exact_parent_membership_work(&owner, parent);
    assert_eq!(owner.render_tree().parent(child), None);
}

#[test]
fn pure_reorder_marks_layout_only() {
    let mut owner = PipelineOwner::new();
    let parent = owner.set_root_render_object(Box::new(RenderColoredBox::red(10.0, 10.0)));
    owner.set_semantics_enabled(true);
    let first = owner
        .insert_child_render_object(parent, Box::new(RenderColoredBox::blue(5.0, 5.0)))
        .expect("first child insertion");
    let second = owner
        .insert_child_render_object(parent, Box::new(RenderColoredBox::green(5.0, 5.0)))
        .expect("second child insertion");
    owner.clear_all_dirty_nodes();
    for id in [parent, first, second] {
        clear_node_dirty_flags(&mut owner, id);
    }

    owner.note_render_children_reordered(parent);

    assert_eq!(owner.nodes_needing_layout().len(), 1);
    assert_eq!(owner.nodes_needing_layout()[0].id, parent);
    assert!(owner.nodes_needing_compositing_bits_update().is_empty());
    assert!(owner.nodes_needing_semantics().is_empty());
    assert!(owner.nodes_needing_paint().is_empty());
}
