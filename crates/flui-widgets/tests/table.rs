//! `Table` widget smoke coverage over `RenderTable`.

use std::collections::HashMap;

use crate::common::{lay_out, offset, size, tight};
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

/// A key on a cell is its identity inside its row: two keyed cells that swap
/// columns keep their elements, as they would if they were plain siblings.
///
/// Each cell's wrapper carries `(row identity, cell identity)`, and a cell
/// that has its own key contributes that key rather than its column — so the
/// wrapper follows the cell instead of pinning it to a column number.
#[test]
fn keyed_cells_that_swap_columns_keep_their_elements() {
    use flui_foundation::{ValueKey, ViewKey};
    use flui_view::{BuildContext, IntoView, StatelessView, View, element::ElementKind};
    use flui_widgets::Text;

    /// A cell that carries a key of its own — there is no `ViewExt::key`
    /// builder, so tests wrap the view they want to key.
    #[derive(Clone)]
    struct KeyedText {
        key: ValueKey<String>,
        label: String,
    }

    impl View for KeyedText {
        fn create_element(&self) -> ElementKind {
            ElementKind::stateless(self)
        }
        fn key(&self) -> Option<&dyn ViewKey> {
            Some(&self.key)
        }
    }

    impl StatelessView for KeyedText {
        fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
            Text::new(self.label.clone())
        }
    }

    let cell = |label: &str| KeyedText {
        key: ValueKey::new(label.to_string()),
        label: label.to_string(),
    };
    let mut laid = lay_out(
        Table::new(vec![TableRow::new(vec![
            cell("left").boxed(),
            cell("right").boxed(),
        ])]),
        tight(200.0, 100.0),
    );
    let left_before = laid.find_text("left").expect("left is mounted");
    let right_before = laid.find_text("right").expect("right is mounted");

    laid.pump_widget(Table::new(vec![TableRow::new(vec![
        cell("right").boxed(),
        cell("left").boxed(),
    ])]));

    assert_eq!(
        laid.find_text("left"),
        Some(left_before),
        "the cell keyed `left` keeps its element after moving to column 1"
    );
    assert_eq!(
        laid.find_text("right"),
        Some(right_before),
        "and so does the one it swapped with"
    );
}

/// Row identity is compared semantically, not by hash.
///
/// The reconciler treats a key's hash as a bucket and settles equality with
/// `key_eq`; a cell's identity must do the same, or two rows whose keys
/// happen to collide would trade elements.
#[test]
fn rows_whose_keys_collide_by_hash_are_still_distinct() {
    use flui_foundation::ViewKey;
    use flui_widgets::Text;

    /// A key whose hash is a constant, so any two of them collide, but whose
    /// equality is its own value.
    #[derive(Clone, PartialEq, Eq, Debug)]
    struct CollidingKey(&'static str);

    impl ViewKey for CollidingKey {
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
        fn key_eq(&self, other: &dyn ViewKey) -> bool {
            other
                .as_any()
                .downcast_ref::<Self>()
                .is_some_and(|other| self == other)
        }
        fn key_hash(&self) -> u64 {
            0
        }
        fn clone_key(&self) -> Box<dyn ViewKey> {
            Box::new(self.clone())
        }
        fn debug_fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "CollidingKey({})", self.0)
        }
    }

    let row = |label: &str, key: &'static str| {
        TableRow::new(vec![Text::new(label.to_string()).boxed()]).key(CollidingKey(key))
    };
    let mut laid = lay_out(
        Table::new(vec![row("first", "a"), row("second", "b")]),
        tight(200.0, 100.0),
    );
    let first_before = laid.find_text("first").expect("first is mounted");
    let second_before = laid.find_text("second").expect("second is mounted");

    // The two rows swap places. Their keys collide by hash, so only `key_eq`
    // can tell them apart.
    laid.pump_widget(Table::new(vec![row("second", "b"), row("first", "a")]));

    assert_eq!(
        laid.find_text("first"),
        Some(first_before),
        "row `a` keeps its cell's element wherever it moves to"
    );
    assert_eq!(
        laid.find_text("second"),
        Some(second_before),
        "and row `b` keeps its own — a hash collision must not trade them"
    );
}

/// Rows that disagree about their cell count used to be two `debug_assert!`s
/// and nothing else, so a release build carried the ragged grid into
/// `RenderTable` — which floor-divides to get its row count, leaving a
/// trailing partial row that layout skipped, hit-test ignored, and paint was
/// still handed. `Table::new` squares the rows up instead, so that state
/// cannot be constructed.
#[test]
fn table_pads_a_short_row_and_drops_a_long_rows_extra_cells() {
    let laid = lay_out(
        Table::new(vec![
            TableRow::new(vec![
                SizedBox::new(1.0, 10.0).boxed(),
                SizedBox::new(1.0, 10.0).boxed(),
            ]),
            // One cell short: padded, so this row is still a row.
            TableRow::new(vec![SizedBox::new(1.0, 20.0).boxed()]),
            // One cell too many: the extra is dropped.
            TableRow::new(vec![
                SizedBox::new(1.0, 30.0).boxed(),
                SizedBox::new(1.0, 30.0).boxed(),
                SizedBox::new(1.0, 30.0).boxed(),
            ]),
        ]),
        tight(100.0, 200.0),
    );

    let root = laid.root();
    assert_eq!(
        laid.children(root).len(),
        6,
        "three rows of two columns reach the render object as exactly six cells",
    );
    // The padded cell occupies its slot rather than shifting the grid: row 1's
    // real cell stays in column 0, and row 2 starts where row 1 ends — so all
    // three rows contributed their height (10, then 20, then 30) and none was
    // truncated away.
    assert_eq!(laid.offset(laid.child(root, 2)), offset(0.0, 10.0));
    assert_eq!(laid.offset(laid.child(root, 4)), offset(0.0, 30.0));
}
