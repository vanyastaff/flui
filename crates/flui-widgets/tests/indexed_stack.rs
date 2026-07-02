//! `IndexedStack` widget parity — widget surface over `RenderIndexedStack`.

mod common;

use common::{lay_out, loose, offset, size};
use flui_widgets::row;
use flui_widgets::{IndexedStack, Positioned, SizedBox, StackFit};

#[test]
fn indexed_stack_sizes_like_stack_and_mounts_indexed_render_object() {
    let laid = lay_out(
        IndexedStack::new(row![SizedBox::new(40.0, 30.0), SizedBox::new(80.0, 60.0)])
            .index(Some(1)),
        loose(200.0),
    );

    let root = laid.root();
    assert_eq!(laid.find_by_render_type("RenderIndexedStack"), root);
    assert_eq!(laid.size(root), size(80.0, 60.0));

    let first = laid.child(root, 0);
    let second = laid.child(root, 1);
    assert_eq!(laid.size(first), size(40.0, 30.0));
    assert_eq!(laid.size(second), size(80.0, 60.0));
    assert_eq!(laid.offset(first), offset(0.0, 0.0));
    assert_eq!(laid.offset(second), offset(0.0, 0.0));
}

#[test]
fn indexed_stack_none_still_lays_out_children() {
    let laid = lay_out(
        IndexedStack::new(row![SizedBox::new(40.0, 30.0), SizedBox::new(80.0, 60.0)]).index(None),
        loose(200.0),
    );

    let root = laid.root();
    assert_eq!(laid.size(root), size(80.0, 60.0));
    assert_eq!(laid.size(laid.child(root, 0)), size(40.0, 30.0));
    assert_eq!(laid.size(laid.child(root, 1)), size(80.0, 60.0));
}

#[test]
fn indexed_stack_positioned_child_keeps_stack_parent_data() {
    let laid = lay_out(
        IndexedStack::new(row![
            Positioned::new(SizedBox::new(30.0, 20.0))
                .left(12.0)
                .top(8.0),
        ])
        .fit(StackFit::Expand),
        common::tight(100.0, 80.0),
    );

    let child = laid.only_child(laid.root());
    assert_eq!(laid.size(child), size(30.0, 20.0));
    assert_eq!(laid.offset(child), offset(12.0, 8.0));
}
