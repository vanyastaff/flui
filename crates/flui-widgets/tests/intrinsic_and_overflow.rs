//! View-tree integration coverage for the B4 layout widgets: each widget builds
//! its render object and the layout flows end-to-end through the real pipeline
//! (the Proxy-storage path that wires the box→box intrinsic-during-layout
//! callback — the render-object harness exercises the math; these prove the
//! widget→render wiring and the live pipeline path).

mod common;

use common::{lay_out, loose, tight};
use flui_geometry::px;
use flui_types::Size;
use flui_widgets::{
    Column, IntrinsicHeight, IntrinsicWidth, OverflowBox, RotatedBox, SizedBox, SizedOverflowBox,
};
use flui_widgets::{MainAxisSize, column};

#[test]
fn rotated_box_quarter_turn_swaps_child_axes() {
    // A 30-wide, 10-tall child rotated one quarter turn occupies a 10×30 box.
    let laid = lay_out(
        RotatedBox::new(1).child(SizedBox::new(30.0, 10.0)),
        loose(200.0),
    );
    let size = laid.size(laid.current_root());
    assert!(
        (size.width.get() - 10.0).abs() < 1e-3 && (size.height.get() - 30.0).abs() < 1e-3,
        "one quarter turn swaps 30×10 → 10×30, got {}×{}",
        size.width.get(),
        size.height.get(),
    );
}

#[test]
fn rotated_box_half_turn_keeps_child_axes() {
    // Two quarter turns (180°) restore the original orientation/extent.
    let laid = lay_out(
        RotatedBox::new(2).child(SizedBox::new(30.0, 10.0)),
        loose(200.0),
    );
    let size = laid.size(laid.current_root());
    assert!(
        (size.width.get() - 30.0).abs() < 1e-3 && (size.height.get() - 10.0).abs() < 1e-3,
        "two quarter turns keep 30×10, got {}×{}",
        size.width.get(),
        size.height.get(),
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
    let width = laid.size(laid.current_root()).width.get();
    assert!(
        (width - 40.0).abs() < 1e-3,
        "intrinsic width 30 stepped to the nearest 40 is 40, got {width}",
    );
}

#[test]
fn intrinsic_height_collapses_a_maxed_column_to_its_intrinsic_height() {
    // A column of a 30-tall and a 50-tall child stacks to an 80px intrinsic
    // height. With `MainAxisSize::Max` the column would otherwise FILL the loose
    // 200px height; IntrinsicHeight tightens it to the 80px intrinsic instead.
    // The result (80, not 200) only holds if the box→box intrinsic query runs
    // through the live pipeline against the multi-child subtree.
    let laid = lay_out(
        IntrinsicHeight::new().child(
            Column::new(column![
                SizedBox::new(20.0, 30.0),
                SizedBox::new(20.0, 50.0)
            ])
            .main_axis_size(MainAxisSize::Max),
        ),
        loose(200.0),
    );
    let height = laid.size(laid.current_root()).height.get();
    assert!(
        (height - 80.0).abs() < 1e-3,
        "intrinsic height tightens the maxed column to its 30+50 stack (80), \
         not the 200px loose fill; got {height}",
    );
}

#[test]
fn overflow_box_lets_child_exceed_the_parent_box() {
    // The parent is tight 50×50; OverflowBox imposes looser child bounds so an
    // 80×80 child lays out at its full size while the box itself stays 50×50.
    let laid = lay_out(
        OverflowBox::new()
            .with_max_width(px(100.0))
            .with_max_height(px(100.0))
            .child(SizedBox::new(80.0, 80.0)),
        tight(50.0, 50.0),
    );
    let root = laid.current_root();
    let box_size = laid.size(root);
    let child_size = laid.size(laid.only_child(root));
    assert!(
        (box_size.width.get() - 50.0).abs() < 1e-3 && (box_size.height.get() - 50.0).abs() < 1e-3,
        "the overflow box keeps the parent's tight 50×50, got {}×{}",
        box_size.width.get(),
        box_size.height.get(),
    );
    assert!(
        (child_size.width.get() - 80.0).abs() < 1e-3
            && (child_size.height.get() - 80.0).abs() < 1e-3,
        "the child overflows to its own 80×80, got {}×{}",
        child_size.width.get(),
        child_size.height.get(),
    );
}

#[test]
fn sized_overflow_box_fixes_its_own_size_while_child_overflows() {
    // The box reports a fixed 40×40 regardless of its 100×100 child.
    let laid = lay_out(
        SizedOverflowBox::new(Size::new(px(40.0), px(40.0))).child(SizedBox::new(100.0, 100.0)),
        loose(200.0),
    );
    let root = laid.current_root();
    let box_size = laid.size(root);
    let child_size = laid.size(laid.only_child(root));
    assert!(
        (box_size.width.get() - 40.0).abs() < 1e-3 && (box_size.height.get() - 40.0).abs() < 1e-3,
        "the sized overflow box reports its requested 40×40, got {}×{}",
        box_size.width.get(),
        box_size.height.get(),
    );
    assert!(
        (child_size.width.get() - 100.0).abs() < 1e-3,
        "the child lays out at its own 100px width, overflowing the 40px box, got {}",
        child_size.width.get(),
    );
}
