//! Flex parent-data parity — the first coverage of the `ParentDataView` seam.
//!
//! `Expanded`/`Flexible` contribute a `FlexParentData` (flex factor + fit) to
//! their child's render node, which the parent `RenderFlex` reads to allocate
//! the main axis. This proves the seam end-to-end: the configured flex reaches
//! the render tree and drives both child **sizes** and **offsets**.

use crate::common::{lay_out, offset, size, tight};
use flui_objects::VerticalDirection;
use flui_widgets::row;
use flui_widgets::{Column, CrossAxisAlignment, Expanded, Row, SizedBox};

pub(crate) fn vertical_direction_updates_mounted_flex_children() {
    for vertical in [false, true] {
        let content = |direction| {
            let children = vec![SizedBox::new(20.0, 10.0), SizedBox::new(30.0, 20.0)];
            flui_widgets::Flex::new(
                if vertical {
                    flui_objects::FlexDirection::Vertical
                } else {
                    flui_objects::FlexDirection::Horizontal
                },
                children,
            )
            .cross_axis_alignment(CrossAxisAlignment::Start)
            .vertical_direction(direction)
        };
        let mut laid = lay_out(content(VerticalDirection::Down), tight(100.0, 100.0));
        let root = laid.root();
        let first = laid.child(root, 0);
        let second = laid.child(root, 1);
        for direction in [VerticalDirection::Up, VerticalDirection::Down] {
            laid.pump_widget(content(direction));
            assert_eq!(laid.root(), root);
            assert_eq!(laid.child(root, 0), first);
            assert_eq!(laid.child(root, 1), second);
            let up = direction == VerticalDirection::Up;
            assert_eq!(laid.offset(first), offset(0.0, if up { 90.0 } else { 0.0 }));
            assert_eq!(
                laid.offset(second),
                if vertical {
                    offset(0.0, if up { 70.0 } else { 10.0 })
                } else {
                    offset(20.0, if up { 80.0 } else { 0.0 })
                }
            );
        }
    }
    // The convenience producers share the same configuration path.
    let row = lay_out(
        Row::new(row![SizedBox::new(20.0, 10.0)])
            .cross_axis_alignment(CrossAxisAlignment::Start)
            .vertical_direction(VerticalDirection::Up),
        tight(100.0, 100.0),
    );
    assert_eq!(row.offset(row.child(row.root(), 0)), offset(0.0, 90.0));
    let column = lay_out(
        Column::new(row![SizedBox::new(20.0, 10.0)]).vertical_direction(VerticalDirection::Up),
        tight(100.0, 100.0),
    );
    assert_eq!(column.offset(column.child(column.root(), 0)).dy, 90.0);
}

pub(crate) fn two_expandeds_split_main_axis_by_flex_factor() {
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
