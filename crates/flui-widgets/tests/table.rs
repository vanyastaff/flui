//! `Table` widget smoke coverage over `RenderTable`.

use std::collections::HashMap;

use crate::common::{lay_out, offset, size, tight};
use flui_objects::TableColumnWidth;
use flui_view::ViewExt;
use flui_widgets::{SizedBox, Table, TableRow};

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
