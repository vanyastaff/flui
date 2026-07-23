//! `CustomMultiChildLayout` widget smoke coverage over
//! `RenderCustomMultiChildLayoutBox` plus `LayoutId` parent-data delivery.

use std::any::Any;
use std::sync::Arc;

use crate::common::{lay_out, loose, offset, size};
use flui_rendering::constraints::BoxConstraints;
use flui_types::{Offset, Size};
use flui_widgets::{
    CustomMultiChildLayout, LayoutId, MultiChildLayoutContext, MultiChildLayoutDelegate, SizedBox,
    row,
};

#[derive(Debug)]
struct TwoSlotDelegate {
    size: Size,
}

impl MultiChildLayoutDelegate for TwoSlotDelegate {
    fn get_size(&self, _constraints: BoxConstraints) -> Size {
        self.size
    }

    fn perform_layout(&self, context: &mut dyn MultiChildLayoutContext, parent_size: Size) {
        if context.has_child("header") {
            context.layout_child(
                "header",
                BoxConstraints::tight(Size::new(parent_size.width, size(20.0, 20.0).height)),
            );
            context.position_child("header", Offset::ZERO);
        }
        if context.has_child("body") {
            context.layout_child("body", BoxConstraints::tight(size(70.0, 30.0)));
            context.position_child("body", offset(10.0, 25.0));
        }
    }

    fn should_relayout(&self, old_delegate: &dyn MultiChildLayoutDelegate) -> bool {
        old_delegate
            .as_any()
            .downcast_ref::<Self>()
            .is_none_or(|old| self.size != old.size)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

fn two_slot_delegate() -> Arc<dyn MultiChildLayoutDelegate> {
    Arc::new(TwoSlotDelegate {
        size: size(120.0, 90.0),
    })
}

#[test]
fn custom_multi_child_layout_mounts_render_object_and_positions_layout_id_children() {
    let laid = lay_out(
        CustomMultiChildLayout::new(
            two_slot_delegate(),
            row![
                LayoutId::new("header", SizedBox::new(10.0, 10.0)),
                LayoutId::new("body", SizedBox::new(10.0, 10.0)),
            ],
        ),
        loose(200.0),
    );

    let root = laid.root();
    assert_eq!(
        laid.find_by_render_type("RenderCustomMultiChildLayoutBox"),
        root
    );
    assert_eq!(laid.render_node_count(), 3);
    assert_eq!(laid.size(root), size(120.0, 90.0));

    let header = laid.child(root, 0);
    let body = laid.child(root, 1);
    assert_eq!(laid.size(header), size(120.0, 20.0));
    assert_eq!(laid.offset(header), offset(0.0, 0.0));
    assert_eq!(laid.size(body), size(70.0, 30.0));
    assert_eq!(laid.offset(body), offset(10.0, 25.0));
}
