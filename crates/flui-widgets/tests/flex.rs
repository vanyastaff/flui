//! Layout parity tests for the flex family — exercise contract C2 on both the
//! dynamic `Vec<BoxedView>` path (`Column`) and the static tuple path
//! (`row!` macro), asserting main/cross-axis sizing and child positions.

use crate::common::{lay_out, loose, offset, size, tight};
use flui_objects::FlexDirection;
use flui_view::ViewExt;
use flui_widgets::row;
use flui_widgets::{
    Column, CrossAxisAlignment, Flex, MainAxisAlignment, MainAxisSize, Row, SizedBox,
};

#[test]
fn column_shrink_wraps_and_stacks_children_dynamic_path() {
    // Dynamic Vec<BoxedView> path: two boxes stacked vertically, shrink-wrapped.
    // main (vertical) = 30 + 40 = 70; cross (horizontal) = max(50, 80) = 80.
    let laid = lay_out(
        Column::new(vec![
            SizedBox::new(50.0, 30.0).boxed(),
            SizedBox::new(80.0, 40.0).boxed(),
        ])
        .main_axis_size(MainAxisSize::Min)
        .cross_axis_alignment(CrossAxisAlignment::Start),
        loose(1000.0),
    );

    let root = laid.root();
    assert_eq!(laid.size(root), size(80.0, 70.0));
    assert_eq!(laid.offset(laid.child(root, 0)), offset(0.0, 0.0));
    assert_eq!(laid.offset(laid.child(root, 1)), offset(0.0, 30.0));
    assert_eq!(laid.size(laid.child(root, 0)), size(50.0, 30.0));
    assert_eq!(laid.size(laid.child(root, 1)), size(80.0, 40.0));
}

#[test]
fn row_lays_children_horizontally_static_tuple_path() {
    // Static tuple path via row!: monomorphic per-position children.
    // main (horizontal) = 40 + 60 = 100; cross (vertical) = max(20, 30) = 30.
    let laid = lay_out(
        Row::new(row![SizedBox::new(40.0, 20.0), SizedBox::new(60.0, 30.0)])
            .main_axis_size(MainAxisSize::Min)
            .cross_axis_alignment(CrossAxisAlignment::Start),
        loose(1000.0),
    );

    let root = laid.root();
    assert_eq!(laid.size(root), size(100.0, 30.0));
    assert_eq!(laid.offset(laid.child(root, 0)), offset(0.0, 0.0));
    assert_eq!(laid.offset(laid.child(root, 1)), offset(40.0, 0.0));
}

#[test]
fn column_center_cross_alignment_centers_each_child() {
    // Default-ish cross alignment Center: narrower child is horizontally centered.
    let laid = lay_out(
        Column::new(vec![
            SizedBox::new(40.0, 20.0).boxed(),
            SizedBox::new(80.0, 20.0).boxed(),
        ])
        .main_axis_size(MainAxisSize::Min)
        .cross_axis_alignment(CrossAxisAlignment::Center),
        loose(1000.0),
    );

    let root = laid.root();
    assert_eq!(laid.size(root), size(80.0, 40.0));
    // Narrow child centered in the 80-wide cross axis: (80-40)/2 = 20.
    assert_eq!(laid.offset(laid.child(root, 0)), offset(20.0, 0.0));
    assert_eq!(laid.offset(laid.child(root, 1)), offset(0.0, 20.0));
}

// ============================================================================
// Flex — the generic direction-configurable widget itself (only Row/Column,
// its fixed-direction convenience wrappers, were exercised above).
// ============================================================================

#[test]
fn flex_with_horizontal_direction_behaves_like_row() {
    let laid = lay_out(
        Flex::new(
            FlexDirection::Horizontal,
            row![SizedBox::new(40.0, 20.0), SizedBox::new(60.0, 30.0)],
        )
        .main_axis_size(MainAxisSize::Min)
        .cross_axis_alignment(CrossAxisAlignment::Start),
        loose(1000.0),
    );

    let root = laid.root();
    assert_eq!(laid.size(root), size(100.0, 30.0));
    assert_eq!(laid.offset(laid.child(root, 0)), offset(0.0, 0.0));
    assert_eq!(laid.offset(laid.child(root, 1)), offset(40.0, 0.0));
}

#[test]
fn flex_with_vertical_direction_behaves_like_column() {
    let laid = lay_out(
        Flex::new(
            FlexDirection::Vertical,
            vec![
                SizedBox::new(50.0, 30.0).boxed(),
                SizedBox::new(80.0, 40.0).boxed(),
            ],
        )
        .main_axis_size(MainAxisSize::Min)
        .cross_axis_alignment(CrossAxisAlignment::Start),
        loose(1000.0),
    );

    let root = laid.root();
    assert_eq!(laid.size(root), size(80.0, 70.0));
    assert_eq!(laid.offset(laid.child(root, 0)), offset(0.0, 0.0));
    assert_eq!(laid.offset(laid.child(root, 1)), offset(0.0, 30.0));
}

// ============================================================================
// MainAxisAlignment — never exercised in the tests above (each forces
// MainAxisSize::Min so the main axis exactly fits the children, leaving no
// free space for alignment to distribute).
// ============================================================================

#[test]
fn main_axis_alignment_end_packs_children_against_the_trailing_edge() {
    // Main axis tight at 1000 (MainAxisSize::Max, the default, has no extra
    // effect here since the constraint is already tight) leaves 1000 - 100 =
    // 900px of free space; `End` packs both children flush against it.
    let laid = lay_out(
        Row::new(row![SizedBox::new(40.0, 20.0), SizedBox::new(60.0, 30.0)])
            .main_axis_alignment(MainAxisAlignment::End),
        tight(1000.0, 30.0),
    );

    let root = laid.root();
    assert_eq!(laid.size(root), size(1000.0, 30.0));
    // Default CrossAxisAlignment::Center within a tight 30px cross axis:
    // child0 (h=20) offset by (30-20)/2 = 5; child1 (h=30) offset by 0.
    assert_eq!(laid.offset(laid.child(root, 0)), offset(900.0, 5.0));
    assert_eq!(laid.offset(laid.child(root, 1)), offset(940.0, 0.0));
}

#[test]
fn main_axis_alignment_space_between_puts_all_free_space_between_children() {
    // 1000 - 100 = 900px free space, one gap (2 children), none before the
    // first or after the last child.
    let laid = lay_out(
        Row::new(row![SizedBox::new(40.0, 20.0), SizedBox::new(60.0, 30.0)])
            .main_axis_alignment(MainAxisAlignment::SpaceBetween),
        tight(1000.0, 30.0),
    );

    let root = laid.root();
    assert_eq!(laid.offset(laid.child(root, 0)), offset(0.0, 5.0));
    assert_eq!(laid.offset(laid.child(root, 1)), offset(940.0, 0.0));
}
