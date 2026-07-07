//! Flex parent-data parity — the first coverage of the `ParentDataView` seam.
//!
//! `Expanded`/`Flexible` contribute a `FlexParentData` (flex factor + fit) to
//! their child's render node, which the parent `RenderFlex` reads to allocate
//! the main axis. This proves the seam end-to-end: the configured flex reaches
//! the render tree and drives both child **sizes** and **offsets**.

use crate::common::{lay_out, offset, size, tight};
use flui_widgets::row;
use flui_widgets::{Expanded, Flexible, Row, SizedBox};

#[test]
fn expanded_takes_remaining_main_axis_after_fixed_sibling() {
    // Row 300 wide: an `Expanded` child followed by a fixed 100-wide box. The
    // Expanded must fill the remaining 200; the fixed box keeps its 100. The
    // Expanded is listed FIRST, so its offset is 0 and the fixed box follows at
    // 200 — the slot order must survive the build queue, not the render-attach
    // order.
    let laid = lay_out(
        Row::new(row![
            Expanded::new(SizedBox::height(50.0)),
            SizedBox::new(100.0, 50.0),
        ]),
        tight(300.0, 50.0),
    );

    let root = laid.root();
    // RenderFlex + the Expanded's box + the fixed box = 3 render nodes. The
    // `Expanded` itself is a ParentDataElement with NO render node of its own.
    assert_eq!(laid.render_node_count(), 3);

    let expanded_child = laid.child(root, 0);
    let fixed = laid.child(root, 1);

    assert_eq!(laid.size(expanded_child), size(200.0, 50.0));
    assert_eq!(laid.offset(expanded_child), offset(0.0, 0.0));

    assert_eq!(laid.size(fixed), size(100.0, 50.0));
    assert_eq!(laid.offset(fixed), offset(200.0, 0.0));
}

#[test]
fn two_expandeds_split_main_axis_by_flex_factor() {
    // Two Expandeds at flex 1 and 2 split a 300-wide row 100 / 200.
    let laid = lay_out(
        Row::new(row![
            Expanded::new(SizedBox::height(50.0)),
            Expanded::new(SizedBox::height(50.0)).flex(2),
        ]),
        tight(300.0, 50.0),
    );

    let root = laid.root();
    let first = laid.child(root, 0);
    let second = laid.child(root, 1);

    assert_eq!(laid.size(first), size(100.0, 50.0));
    assert_eq!(laid.offset(first), offset(0.0, 0.0));

    assert_eq!(laid.size(second), size(200.0, 50.0));
    assert_eq!(laid.offset(second), offset(100.0, 0.0));
}

#[test]
fn flexible_loose_fit_does_not_force_child_to_fill() {
    // A `Flexible` (loose fit) caps the child at its share but lets it stay
    // smaller: a 60-wide box inside a flex-1 Flexible keeps its 60 even though
    // its share of the 300-wide row is larger.
    let laid = lay_out(
        Row::new(row![
            Flexible::new(SizedBox::new(60.0, 50.0)),
            SizedBox::new(100.0, 50.0),
        ]),
        tight(300.0, 50.0),
    );

    let root = laid.root();
    let flexible_child = laid.child(root, 0);
    let fixed = laid.child(root, 1);

    // Loose fit: the child keeps its intrinsic 60, not its 200 share.
    assert_eq!(laid.size(flexible_child), size(60.0, 50.0));
    assert_eq!(laid.size(fixed), size(100.0, 50.0));
}
