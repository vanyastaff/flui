//! Layout test for [`Baseline`] — positions a child by a baseline distance.

mod common;

use common::{lay_out, loose, offset, size};
use flui_widgets::prelude::TextBaseline;
use flui_widgets::{Baseline, SizedBox};

#[test]
fn baseline_positions_child_by_baseline_distance() {
    // A SizedBox has no text baseline, so RenderBaseline falls back to the
    // child's height (30) as its baseline. With baseline = 40, the child's
    // bottom sits at y=40, so the child (50×30) is offset to (0, 10) and the
    // box is 50 wide × 40 tall (baseline distance).
    let laid = lay_out(
        Baseline::new(40.0, TextBaseline::Alphabetic).child(SizedBox::new(50.0, 30.0)),
        loose(1000.0),
    );
    assert_eq!(laid.size(laid.root()), size(50.0, 40.0));
    assert_eq!(laid.offset(laid.only_child(laid.root())), offset(0.0, 10.0));
}
