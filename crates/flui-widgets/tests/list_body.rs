//! `ListBody` widget parity over `RenderListBody`.

use crate::common::{lay_out, offset, size};
use flui_rendering::constraints::BoxConstraints;
use flui_types::{geometry::px, layout::Axis};
use flui_widgets::row;
use flui_widgets::{ListBody, SizedBox};

fn vertical_constraints(width: f32) -> BoxConstraints {
    BoxConstraints::new(px(0.0), px(width), px(0.0), px(f32::INFINITY))
}

#[test]
fn list_body_vertical_stretches_children_and_sums_height() {
    let laid = lay_out(
        ListBody::new(row![SizedBox::new(20.0, 10.0), SizedBox::new(30.0, 20.0)]),
        vertical_constraints(100.0),
    );

    let root = laid.root();
    assert_eq!(laid.find_by_render_type("RenderListBody"), root);
    assert_eq!(laid.size(root), size(100.0, 30.0));

    let first = laid.child(root, 0);
    let second = laid.child(root, 1);
    assert_eq!(laid.size(first), size(100.0, 10.0));
    assert_eq!(laid.size(second), size(100.0, 20.0));
    assert_eq!(laid.offset(first), offset(0.0, 0.0));
    assert_eq!(laid.offset(second), offset(0.0, 10.0));
}

#[test]
fn list_body_reverse_vertical_places_first_child_last() {
    let laid = lay_out(
        ListBody::new(row![SizedBox::new(20.0, 10.0), SizedBox::new(30.0, 20.0)]).reverse(true),
        vertical_constraints(100.0),
    );

    let root = laid.root();
    assert_eq!(laid.size(root), size(100.0, 30.0));
    assert_eq!(laid.offset(laid.child(root, 0)), offset(0.0, 20.0));
    assert_eq!(laid.offset(laid.child(root, 1)), offset(0.0, 0.0));
}

#[test]
fn list_body_horizontal_uses_unbounded_width_and_bounded_height() {
    let constraints = BoxConstraints::new(px(0.0), px(f32::INFINITY), px(0.0), px(50.0));
    let laid = lay_out(
        ListBody::new(row![SizedBox::new(20.0, 10.0), SizedBox::new(30.0, 20.0)])
            .main_axis(Axis::Horizontal),
        constraints,
    );

    let root = laid.root();
    assert_eq!(laid.size(root), size(50.0, 50.0));
    assert_eq!(laid.size(laid.child(root, 0)), size(20.0, 50.0));
    assert_eq!(laid.offset(laid.child(root, 1)), offset(20.0, 0.0));
}
