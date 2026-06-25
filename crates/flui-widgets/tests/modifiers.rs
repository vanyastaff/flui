//! Layout tests for the modifier widgets — [`Offstage`] changes layout
//! (zero-size while hidden), while [`RepaintBoundary`]/[`IgnorePointer`]/
//! [`AbsorbPointer`] are layout pass-throughs.

mod common;

use common::{lay_out, loose, size};
use flui_widgets::{IgnorePointer, Offstage, RepaintBoundary, SizedBox};

#[test]
fn offstage_hidden_takes_zero_space() {
    let laid = lay_out(
        Offstage::new()
            .offstage(true)
            .child(SizedBox::square(100.0)),
        loose(1000.0),
    );
    assert_eq!(laid.size(laid.root()), size(0.0, 0.0));
}

#[test]
fn offstage_visible_lays_out_normally() {
    let laid = lay_out(
        Offstage::new()
            .offstage(false)
            .child(SizedBox::square(100.0)),
        loose(1000.0),
    );
    assert_eq!(laid.size(laid.root()), size(100.0, 100.0));
}

#[test]
fn repaint_boundary_is_a_layout_passthrough() {
    let laid = lay_out(
        RepaintBoundary::new().child(SizedBox::new(80.0, 40.0)),
        loose(1000.0),
    );
    assert_eq!(laid.size(laid.root()), size(80.0, 40.0));
}

#[test]
fn ignore_pointer_is_a_layout_passthrough() {
    let laid = lay_out(
        IgnorePointer::new().child(SizedBox::square(64.0)),
        loose(1000.0),
    );
    assert_eq!(laid.size(laid.root()), size(64.0, 64.0));
}
