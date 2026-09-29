//! Deeper layout-parity tests: multi-level constraint propagation, the full
//! `Container` composition stack, `Stack` alignment, and flex main-axis
//! distribution. Each asserts computed geometry that would be wrong if a layer
//! mis-propagated constraints or mis-placed a child.

use crate::common::{lay_out, loose, offset, size, tight};
use flui_widgets::row;
use flui_widgets::{MainAxisAlignment, MainAxisSize, Padding, Row, SizedBox};

#[test]
fn nested_padding_accumulates_insets_through_levels() {
    // Padding(10) → Padding(5) → SizedBox(100): inner 110, outer 130.
    let laid = lay_out(
        Padding::all(10.0).child(Padding::all(5.0).child(SizedBox::square(100.0))),
        loose(1000.0),
    );
    assert_eq!(laid.size(laid.root()), size(130.0, 130.0));

    let inner_padding = laid.only_child(laid.root());
    assert_eq!(laid.size(inner_padding), size(110.0, 110.0));
    // Inner padding sits at (10,10) inside the outer padding.
    assert_eq!(laid.offset(inner_padding), offset(10.0, 10.0));

    let inner_box = laid.only_child(inner_padding);
    assert_eq!(laid.size(inner_box), size(100.0, 100.0));
    assert_eq!(laid.offset(inner_box), offset(5.0, 5.0));
}

#[test]
fn row_space_between_pushes_children_to_the_edges() {
    // main=Max → 200 wide; SpaceBetween puts the first child at the left edge
    // and the last at the right edge.
    let laid = lay_out(
        Row::new(row![SizedBox::new(40.0, 20.0), SizedBox::new(60.0, 20.0)])
            .main_axis_alignment(MainAxisAlignment::SpaceBetween)
            .main_axis_size(MainAxisSize::Max),
        tight(200.0, 50.0),
    );
    let root = laid.root();
    assert_eq!(laid.size(root), size(200.0, 50.0));
    assert_eq!(laid.offset(laid.child(root, 0)).dx, 0.0);
    // last child at 200 - 60 = 140.
    assert_eq!(laid.offset(laid.child(root, 1)).dx, 140.0);
}
