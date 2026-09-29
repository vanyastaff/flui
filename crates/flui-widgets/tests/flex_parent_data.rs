//! Flex parent-data parity — the first coverage of the `ParentDataView` seam.
//!
//! `Expanded`/`Flexible` contribute a `FlexParentData` (flex factor + fit) to
//! their child's render node, which the parent `RenderFlex` reads to allocate
//! the main axis. This proves the seam end-to-end: the configured flex reaches
//! the render tree and drives both child **sizes** and **offsets**.

use crate::common::{lay_out, offset, size, tight};
use flui_widgets::row;
use flui_widgets::{Expanded, Row, SizedBox};

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
