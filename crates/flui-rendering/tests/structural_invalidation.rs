use flui_objects::RenderColoredBox;
use flui_rendering::{pipeline::PipelineOwner, testing::raw_tree};

fn clear_node_dirty_flags(owner: &mut PipelineOwner, id: flui_foundation::RenderId) {
    assert!(owner.render_tree().contains(id), "inserted render node");
    raw_tree::clear_dirty_flags(owner, id);
}

pub(crate) fn pure_reorder_marks_layout_only() {
    let mut owner = PipelineOwner::new(flui_rendering::TextContextHandle::standalone());
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

    assert_eq!(owner.queued_for_layout().len(), 1);
    assert_eq!(owner.queued_for_layout()[0], parent);
    assert_eq!(owner.queued_for_compositing_bits(), []);
    assert_eq!(owner.queued_for_semantics(), []);
    assert_eq!(owner.queued_for_paint(), []);
}
