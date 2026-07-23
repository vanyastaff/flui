//! `CustomSingleChildLayout` widget smoke coverage over
//! `RenderCustomSingleChildLayoutBox`.

use std::any::Any;
use std::sync::Arc;

use crate::common::{lay_out, loose, offset, size};
use flui_rendering::constraints::BoxConstraints;
use flui_types::{Offset, Size};
use flui_widgets::{CustomSingleChildLayout, SingleChildLayoutDelegate, SizedBox};

#[derive(Debug)]
struct FixedDelegate {
    size: Size,
    child_constraints: BoxConstraints,
    offset: Offset,
}

impl SingleChildLayoutDelegate for FixedDelegate {
    fn get_size(&self, _constraints: BoxConstraints) -> Size {
        self.size
    }

    fn get_constraints_for_child(&self, _constraints: BoxConstraints) -> BoxConstraints {
        self.child_constraints
    }

    fn get_position_for_child(&self, _size: Size, _child_size: Size) -> Offset {
        self.offset
    }

    fn should_relayout(&self, _old_delegate: &dyn SingleChildLayoutDelegate) -> bool {
        true
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

fn fixed_delegate() -> Arc<dyn SingleChildLayoutDelegate> {
    Arc::new(FixedDelegate {
        size: size(120.0, 80.0),
        child_constraints: BoxConstraints::tight(size(30.0, 20.0)),
        offset: offset(70.0, 50.0),
    })
}

#[test]
fn custom_single_child_layout_mounts_render_object_and_positions_child() {
    let laid = lay_out(
        CustomSingleChildLayout::new(fixed_delegate()).child(SizedBox::new(10.0, 10.0)),
        loose(200.0),
    );

    let root = laid.root();
    assert_eq!(
        laid.find_by_render_type("RenderCustomSingleChildLayoutBox"),
        root
    );
    assert_eq!(laid.size(root), size(120.0, 80.0));

    let child = laid.only_child(root);
    assert_eq!(laid.size(child), size(30.0, 20.0));
    assert_eq!(laid.offset(child), offset(70.0, 50.0));
}
