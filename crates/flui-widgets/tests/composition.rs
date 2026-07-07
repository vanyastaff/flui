//! Deeper layout-parity tests: multi-level constraint propagation, the full
//! `Container` composition stack, `Stack` alignment, and flex main-axis
//! distribution. Each asserts computed geometry that would be wrong if a layer
//! mis-propagated constraints or mis-placed a child.

use crate::common::{lay_out, loose, offset, size, tight};
use flui_geometry::{EdgeInsets, px};
use flui_types::Alignment;
use flui_types::Color;
use flui_view::ViewExt;
use flui_widgets::row;
use flui_widgets::{
    Align, Center, Container, MainAxisAlignment, MainAxisSize, Padding, Row, SizedBox, Stack,
};

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
fn container_color_and_padding_compose_around_child() {
    // color + padding(8) around a 50×50 box, no forced size → 66×66.
    let laid = lay_out(
        Container::new()
            .color(Color::rgb(10, 20, 30))
            .padding(EdgeInsets::all(px(8.0)))
            .child(SizedBox::square(50.0)),
        loose(1000.0),
    );
    assert_eq!(laid.size(laid.root()), size(66.0, 66.0));
}

#[test]
fn container_margin_adds_space_outside_the_forced_size() {
    // width/height 100×50 + margin(10): the box is 100×50, the margin pads it
    // to 120×70.
    let laid = lay_out(
        Container::new()
            .width(100.0)
            .height(50.0)
            .margin(EdgeInsets::all(px(10.0)))
            .child(SizedBox::shrink()),
        loose(1000.0),
    );
    assert_eq!(laid.size(laid.root()), size(120.0, 70.0));
}

#[test]
fn stack_center_alignment_centers_each_child() {
    // Loose fit, CENTER alignment: stack = largest child (80×80); the smaller
    // 40×40 child is centered at ((80-40)/2, (80-40)/2) = (20,20).
    let laid = lay_out(
        Stack::new(vec![
            SizedBox::square(80.0).boxed(),
            SizedBox::square(40.0).boxed(),
        ])
        .alignment(Alignment::CENTER),
        loose(1000.0),
    );
    let root = laid.root();
    assert_eq!(laid.size(root), size(80.0, 80.0));
    assert_eq!(laid.offset(laid.child(root, 0)), offset(0.0, 0.0));
    assert_eq!(laid.offset(laid.child(root, 1)), offset(20.0, 20.0));
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
    assert_eq!(laid.offset(laid.child(root, 0)).dx, px(0.0));
    // last child at 200 - 60 = 140.
    assert_eq!(laid.offset(laid.child(root, 1)).dx, px(140.0));
}

#[test]
fn align_inside_constrained_parent_positions_correctly() {
    // A Center forcing 100×100, an Align(BOTTOM_RIGHT) child of 30×30 → (70,70).
    let laid = lay_out(
        Center::new().child(
            SizedBox::square(100.0)
                .child(Align::new(Alignment::BOTTOM_RIGHT).child(SizedBox::square(30.0))),
        ),
        tight(300.0, 300.0),
    );
    // Center fills 300, child SizedBox 100 centered at (100,100).
    let sized = laid.only_child(laid.root());
    assert_eq!(laid.size(sized), size(100.0, 100.0));
    assert_eq!(laid.offset(sized), offset(100.0, 100.0));
    // Align fills the 100×100, its 30×30 child at bottom-right (70,70).
    let align = laid.only_child(sized);
    let inner = laid.only_child(align);
    assert_eq!(laid.offset(inner), offset(70.0, 70.0));
}
