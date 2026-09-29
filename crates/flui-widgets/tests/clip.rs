//! Layout tests for the clip widgets — clipping affects painting only, so
//! layout is a pass-through (the child's size).

use crate::common::{lay_out, loose, size};
use flui_widgets::{ClipRect, SizedBox};

#[test]
fn clip_rect_is_a_layout_passthrough() {
    let laid = lay_out(
        ClipRect::new().child(SizedBox::new(120.0, 80.0)),
        loose(1000.0),
    );
    assert_eq!(laid.size(laid.root()), size(120.0, 80.0));
}
