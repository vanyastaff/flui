//! Layout tests for the modifier widgets — [`Offstage`] changes layout
//! (zero-size while hidden), while [`RepaintBoundary`]/[`IgnorePointer`]/
//! [`AbsorbPointer`] are layout pass-throughs.

use crate::common::{lay_out, loose, size};
use flui_widgets::{Offstage, SizedBox};

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
