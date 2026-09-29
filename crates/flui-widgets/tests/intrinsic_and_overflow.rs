//! View-tree integration coverage for the B4 layout widgets: each widget builds
//! its render object and the layout flows end-to-end through the real pipeline
//! (the Proxy-storage path that wires the box→box intrinsic-during-layout
//! callback — the render-object harness exercises the math; these prove the
//! widget→render wiring and the live pipeline path).

use crate::common::{lay_out, loose, tight};
use flui_widgets::{IntrinsicWidth, OverflowBox, RotatedBox, SizedBox};

#[test]
fn rotated_box_quarter_turn_swaps_child_axes() {
    // A 30-wide, 10-tall child rotated one quarter turn occupies a 10×30 box.
    let laid = lay_out(
        RotatedBox::new(1).child(SizedBox::new(30.0, 10.0)),
        loose(200.0),
    );
    let size = laid.size(laid.current_root());
    assert!(
        (size.width - 10.0).abs() < 1e-3 && (size.height - 30.0).abs() < 1e-3,
        "one quarter turn swaps 30×10 → 10×30, got {}×{}",
        size.width,
        size.height,
    );
}

#[test]
fn intrinsic_width_with_step_rounds_child_width_up() {
    // The child's intrinsic width is 30; a 40px step rounds it UP to 40, and the
    // box sizes itself to that stepped intrinsic width. This proves the box→box
    // intrinsic query runs through the live pipeline (without it the width would
    // stay 30).
    let laid = lay_out(
        IntrinsicWidth::new()
            .with_step_width(40.0)
            .child(SizedBox::new(30.0, 20.0)),
        loose(200.0),
    );
    let width = laid.size(laid.current_root()).width;
    assert!(
        (width - 40.0).abs() < 1e-3,
        "intrinsic width 30 stepped to the nearest 40 is 40, got {width}",
    );
}

#[test]
fn overflow_box_lets_child_exceed_the_parent_box() {
    // The parent is tight 50×50; OverflowBox imposes looser child bounds so an
    // 80×80 child lays out at its full size while the box itself stays 50×50.
    let laid = lay_out(
        OverflowBox::new()
            .with_max_width(100.0)
            .with_max_height(100.0)
            .child(SizedBox::new(80.0, 80.0)),
        tight(50.0, 50.0),
    );
    let root = laid.current_root();
    let box_size = laid.size(root);
    let child_size = laid.size(laid.only_child(root));
    assert!(
        (box_size.width - 50.0).abs() < 1e-3 && (box_size.height - 50.0).abs() < 1e-3,
        "the overflow box keeps the parent's tight 50×50, got {}×{}",
        box_size.width,
        box_size.height,
    );
    assert!(
        (child_size.width - 80.0).abs() < 1e-3 && (child_size.height - 80.0).abs() < 1e-3,
        "the child overflows to its own 80×80, got {}×{}",
        child_size.width,
        child_size.height,
    );
}
