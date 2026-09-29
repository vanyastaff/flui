use flui_objects::RenderColoredBox;
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
