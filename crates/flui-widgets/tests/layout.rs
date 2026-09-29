//! Layout parity tests for the single-child box widgets — each asserts a
//! computed size/offset that would be wrong if the widget mis-wired its render
//! object or failed to attach its child.

use crate::common::{lay_out, loose, offset, size, tight};
use flui_widgets::{Center, Padding, SizedBox};

#[test]
fn padding_wraps_child_plus_insets() {
    // SizedBox 100×100 inside Padding::all(8) → 116×116, child at (8, 8).
    let laid = lay_out(
        Padding::all(8.0).child(SizedBox::square(100.0)),
        loose(1000.0),
    );

    assert_eq!(laid.size(laid.root()), size(116.0, 116.0));
    let child = laid.only_child(laid.root());
    assert_eq!(laid.size(child), size(100.0, 100.0));
    assert_eq!(laid.offset(child), offset(8.0, 8.0));
}

#[test]
fn center_fills_tight_constraints_and_centers_child() {
    // Center fills the tight 200×200, child stays 100×100 at the middle (50,50).
    let laid = lay_out(
        Center::new().child(SizedBox::square(100.0)),
        tight(200.0, 200.0),
    );
    assert_eq!(laid.size(laid.root()), size(200.0, 200.0));
    let child = laid.only_child(laid.root());
    assert_eq!(laid.size(child), size(100.0, 100.0));
    assert_eq!(laid.offset(child), offset(50.0, 50.0));
}

#[test]
fn sized_box_forces_child_dimensions() {
    // SizedBox 120×60 wrapping a Center that would otherwise fill — the box
    // forces the size regardless of the child.
    let laid = lay_out(
        SizedBox::new(120.0, 60.0).child(Center::new().child(SizedBox::square(10.0))),
        loose(1000.0),
    );
    assert_eq!(laid.size(laid.root()), size(120.0, 60.0));
}
