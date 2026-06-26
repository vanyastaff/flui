//! Layout tests for the clip widgets — clipping affects painting only, so
//! layout is a pass-through (the child's size).

mod common;

use common::{lay_out, loose, size};
use flui_widgets::{ClipOval, ClipRRect, ClipRect, SizedBox};

#[test]
fn clip_rect_is_a_layout_passthrough() {
    let laid = lay_out(
        ClipRect::new().child(SizedBox::new(120.0, 80.0)),
        loose(1000.0),
    );
    assert_eq!(laid.size(laid.root()), size(120.0, 80.0));
}

#[test]
fn clip_oval_is_a_layout_passthrough() {
    let laid = lay_out(ClipOval::new().child(SizedBox::square(96.0)), loose(1000.0));
    assert_eq!(laid.size(laid.root()), size(96.0, 96.0));
}

#[test]
fn clip_rrect_is_a_layout_passthrough() {
    // The rounded corners are a paint-time effect; layout still passes the
    // child's size straight through.
    let laid = lay_out(
        ClipRRect::circular(12.0).child(SizedBox::new(120.0, 80.0)),
        loose(1000.0),
    );
    assert_eq!(laid.size(laid.root()), size(120.0, 80.0));
}
