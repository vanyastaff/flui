//! `Spacer` -- proportional empty space in a `Row`/`Column`.
//!
//! `Spacer` composes to `Expanded::new(SizedBox::shrink()).flex(flex)`
//! (a `StatelessView`, so it contributes no render node of its own); these
//! tests prove that composition reaches the render tree with the same
//! flex-splitting and offset behavior `tests/flex_parent_data.rs` already
//! proves for `Expanded` directly.

mod common;

use common::{lay_out, offset, size, tight};
use flui_widgets::row;
use flui_widgets::{Row, SizedBox, Spacer};

#[test]
fn spacer_default_flex_factor_fills_the_remaining_main_axis() {
    // Row 300 wide: a fixed 100-wide box followed by a default Spacer
    // (flex = 1). The Spacer must fill the remaining 200.
    let laid = lay_out(
        Row::new(row![SizedBox::new(100.0, 50.0), Spacer::new(),]),
        tight(300.0, 50.0),
    );

    let root = laid.root();
    // RenderFlex + the fixed box + the Spacer's SizedBox::shrink() = 3 render
    // nodes. Neither Spacer nor its inner Expanded contributes a render node
    // of its own.
    assert_eq!(laid.render_node_count(), 3);

    let fixed = laid.child(root, 0);
    let spacer = laid.child(root, 1);

    assert_eq!(laid.size(fixed), size(100.0, 50.0));
    assert_eq!(laid.offset(fixed), offset(0.0, 0.0));

    assert_eq!(laid.size(spacer), size(200.0, 0.0));
    // Default CrossAxisAlignment::Center vertically centers the Spacer's
    // zero-height box within the 50px row: (50 - 0) / 2 = 25.
    assert_eq!(laid.offset(spacer), offset(100.0, 25.0));
}

#[test]
fn spacer_flex_factor_splits_the_main_axis_proportionally() {
    // Two Spacers at flex 1 and 2 split a 300-wide row 100 / 200, matching
    // `two_expandeds_split_main_axis_by_flex_factor` for plain `Expanded`.
    let laid = lay_out(
        Row::new(row![Spacer::new(), Spacer::new().flex(2),]),
        tight(300.0, 50.0),
    );

    let root = laid.root();
    let first = laid.child(root, 0);
    let second = laid.child(root, 1);

    assert_eq!(laid.size(first), size(100.0, 0.0));
    assert_eq!(laid.offset(first), offset(0.0, 25.0));

    assert_eq!(laid.size(second), size(200.0, 0.0));
    assert_eq!(laid.offset(second), offset(100.0, 25.0));
}

#[test]
fn two_equal_spacers_center_a_fixed_child() {
    // Row 300 wide with a 50-wide fixed child flanked by two default Spacers:
    // remaining space (300 - 50 = 250) splits evenly (125 / 125) around it.
    let laid = lay_out(
        Row::new(row![
            Spacer::new(),
            SizedBox::new(50.0, 50.0),
            Spacer::new(),
        ]),
        tight(300.0, 50.0),
    );

    let root = laid.root();
    let leading_spacer = laid.child(root, 0);
    let fixed = laid.child(root, 1);
    let trailing_spacer = laid.child(root, 2);

    assert_eq!(laid.size(leading_spacer), size(125.0, 0.0));
    assert_eq!(laid.offset(leading_spacer), offset(0.0, 25.0));

    assert_eq!(laid.size(fixed), size(50.0, 50.0));
    assert_eq!(laid.offset(fixed), offset(125.0, 0.0));

    assert_eq!(laid.size(trailing_spacer), size(125.0, 0.0));
    assert_eq!(laid.offset(trailing_spacer), offset(175.0, 25.0));
}
