//! `Table` widget smoke coverage over `RenderTable`.

mod common;

use std::collections::HashMap;

use common::{lay_out, offset, size, tight};
use flui_types::geometry::px;
use flui_types::layout::{TableCellVerticalAlignment, TableColumnWidth};
use flui_view::ViewExt;
use flui_widgets::{SizedBox, Table, TableCell, TableRow};

#[test]
fn table_mounts_render_table_and_lays_out_a_grid_row_major() {
    // Column 0 fixed at 30; column 1 (default Flex(1.0)) fills the 70px
    // remainder under the tight 100px width.
    let laid = lay_out(
        Table::new(vec![
            TableRow::new(vec![
                SizedBox::new(1.0, 10.0).boxed(),
                SizedBox::new(1.0, 20.0).boxed(),
            ]),
            TableRow::new(vec![
                SizedBox::new(1.0, 5.0).boxed(),
                SizedBox::new(1.0, 15.0).boxed(),
            ]),
        ])
        .column_widths(HashMap::from([(0, TableColumnWidth::Fixed(30.0))])),
        tight(100.0, 200.0),
    );

    let root = laid.root();
    assert_eq!(laid.find_by_render_type("RenderTable"), root);

    let a = laid.child(root, 0); // row 0, col 0
    let b = laid.child(root, 1); // row 0, col 1
    let c = laid.child(root, 2); // row 1, col 0
    let d = laid.child(root, 3); // row 1, col 1

    // Row 0 height = max(10, 20) = 20.
    assert_eq!(laid.size(a), size(30.0, 10.0));
    assert_eq!(laid.offset(a), offset(0.0, 0.0));
    assert_eq!(laid.size(b), size(70.0, 20.0));
    assert_eq!(laid.offset(b), offset(30.0, 0.0));

    // Row 1 (starts at y=20) height = max(5, 15) = 15.
    assert_eq!(laid.size(c), size(30.0, 5.0));
    assert_eq!(laid.offset(c), offset(0.0, 20.0));
    assert_eq!(laid.size(d), size(70.0, 15.0));
    assert_eq!(laid.offset(d), offset(30.0, 20.0));
}

#[test]
fn table_row_major_flattening_matches_children_declaration_order() {
    // Each row's height is its own tallest cell (20px here) — Table does NOT
    // stretch rows to fill the incoming height, so with 3 rows of 20px each,
    // row tops land at 0, 20, 40.
    let laid = lay_out(
        Table::new(vec![
            TableRow::new(vec![SizedBox::new(1.0, 20.0).boxed()]),
            TableRow::new(vec![SizedBox::new(1.0, 20.0).boxed()]),
            TableRow::new(vec![SizedBox::new(1.0, 20.0).boxed()]),
        ]),
        tight(60.0, 200.0),
    );
    let root = laid.root();
    assert_eq!(laid.render_node_count(), 4, "table + 3 cells");
    assert_eq!(laid.offset(laid.child(root, 0)), offset(0.0, 0.0));
    assert_eq!(laid.offset(laid.child(root, 1)), offset(0.0, 20.0));
    assert_eq!(laid.offset(laid.child(root, 2)), offset(0.0, 40.0));
}

#[test]
fn table_cell_overrides_the_tables_default_vertical_alignment() {
    let laid = lay_out(
        Table::new(vec![TableRow::new(vec![
            SizedBox::new(1.0, 10.0).boxed(),
            TableCell::new(TableCellVerticalAlignment::Bottom, SizedBox::new(1.0, 10.0)).boxed(),
            SizedBox::new(1.0, 50.0).boxed(), // spacer: forces row height to 50
        ])]),
        tight(90.0, 60.0),
    );
    let root = laid.root();

    // Row height = 50 (the spacer). The unset cell keeps the table's default
    // (Top): offset dy = 0. The `TableCell`-wrapped cell overrides to Bottom:
    // offset dy = 50 - 10 = 40.
    assert_eq!(laid.offset(laid.child(root, 0)).dy, px(0.0));
    assert_eq!(laid.offset(laid.child(root, 1)).dy, px(40.0));
}
