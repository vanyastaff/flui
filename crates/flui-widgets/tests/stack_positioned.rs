//! `Positioned` parent-data parity — a `Positioned` child contributes a
//! `StackParentData` (edge insets + explicit size) that `RenderStack` reads to
//! place and stretch the child. Proves the ParentDataView seam for the Stack
//! family end-to-end (offsets and resolved sizes).

mod common;

use common::{lay_out, offset, size, tight};
use flui_types::Alignment;
use flui_widgets::row;
use flui_widgets::{Positioned, SizedBox, Stack, StackFit};

#[test]
fn positioned_places_child_at_explicit_edges() {
    // A 200×200 stack with a 50×50 child pinned 10 from the left, 20 from top.
    let laid = lay_out(
        Stack::new(row![
            Positioned::new(SizedBox::new(50.0, 50.0))
                .left(10.0)
                .top(20.0),
        ]),
        tight(200.0, 200.0),
    );

    let root = laid.root();
    assert_eq!(laid.size(root), size(200.0, 200.0));

    let child = laid.only_child(root);
    assert_eq!(laid.size(child), size(50.0, 50.0));
    assert_eq!(laid.offset(child), offset(10.0, 20.0));
}

#[test]
fn positioned_with_both_edges_stretches_the_child() {
    // Pinning both `left` and `right` stretches the child across the axis:
    // 200 − 10 − 30 = 160 wide, placed at x = 10. `top` alone leaves the
    // height at the child's intrinsic 40.
    let laid = lay_out(
        Stack::new(row![
            Positioned::new(SizedBox::new(50.0, 40.0))
                .left(10.0)
                .right(30.0)
                .top(15.0),
        ]),
        tight(200.0, 200.0),
    );

    let root = laid.root();
    let child = laid.only_child(root);
    assert_eq!(laid.size(child), size(160.0, 40.0));
    assert_eq!(laid.offset(child), offset(10.0, 15.0));
}

// ============================================================================
// Stack itself: non-positioned children -- sizing to the largest, and
// alignment. Every test above uses `Positioned` children exclusively;
// `Stack`'s own default (`Alignment::TOP_LEFT`, `StackFit::Loose`) and
// explicit-alignment/fit behavior with plain non-positioned children were
// never directly exercised.
// ============================================================================

#[test]
fn stack_sizes_to_the_largest_child_and_top_left_aligns_by_default() {
    let laid = lay_out(
        Stack::new(row![SizedBox::new(60.0, 40.0), SizedBox::new(100.0, 80.0)]),
        common::loose(1000.0),
    );

    let root = laid.root();
    assert_eq!(
        laid.size(root),
        size(100.0, 80.0),
        "stack sizes to the largest non-positioned child on each axis",
    );

    // Alignment::TOP_LEFT is (-1, -1): every non-positioned child's offset
    // factor is 0 regardless of its own size, so both sit at the origin.
    assert_eq!(laid.offset(laid.child(root, 0)), offset(0.0, 0.0));
    assert_eq!(laid.offset(laid.child(root, 1)), offset(0.0, 0.0));
}

#[test]
fn stack_center_alignment_centers_each_non_positioned_child() {
    let laid = lay_out(
        Stack::new(row![SizedBox::new(60.0, 40.0), SizedBox::new(100.0, 80.0)])
            .alignment(Alignment::CENTER),
        common::loose(1000.0),
    );

    let root = laid.root();
    assert_eq!(laid.size(root), size(100.0, 80.0));
    // Smaller child centered in the 100x80 stack: ((100-60)/2, (80-40)/2).
    assert_eq!(laid.offset(laid.child(root, 0)), offset(20.0, 20.0));
    // The largest child exactly fills the stack, so it sits at the origin.
    assert_eq!(laid.offset(laid.child(root, 1)), offset(0.0, 0.0));
}

#[test]
fn stack_fit_expand_forces_non_positioned_children_to_fill_the_stack() {
    // `StackFit::Expand` tight-constrains non-positioned children to the
    // stack's biggest available size, overriding SizedBox's own configured
    // dimensions entirely.
    let laid = lay_out(
        Stack::new(row![SizedBox::new(60.0, 40.0)]).fit(StackFit::Expand),
        tight(200.0, 150.0),
    );

    let root = laid.root();
    assert_eq!(laid.size(root), size(200.0, 150.0));
    let child = laid.only_child(root);
    assert_eq!(
        laid.size(child),
        size(200.0, 150.0),
        "StackFit::Expand must force the child to fill the stack, not its own 60x40",
    );
    assert_eq!(laid.offset(child), offset(0.0, 0.0));
}
