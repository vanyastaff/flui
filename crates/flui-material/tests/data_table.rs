//! `DataTable` widget-level integration coverage — mounts a real `DataTable`
//! through the full render pipeline (`tests/common/mod.rs`, the same harness
//! `tests/card.rs`/`tests/checkbox.rs` use) and proves geometry, selection
//! dispatch, and the theme cascade actually reach a mounted tree, not just
//! `data_table.rs`'s own pure-function unit tests (`resolve_style`,
//! `selection_summary`, `row_decoration`) computed in isolation.

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{LaidOut, loose};
use flui_foundation::RenderId;
use flui_material::{
    DataCell, DataColumn, DataRow, DataTable, DataTableThemeData, Theme, ThemeData,
};
use flui_widgets::{TableColumnWidth, Text};

fn text_column(label: &str) -> DataColumn {
    DataColumn::new(Text::new(label.to_string()))
}

fn text_cell(label: &str) -> DataCell {
    DataCell::new(Text::new(label.to_string()))
}

fn themed(theme: ThemeData, table: DataTable) -> Theme {
    Theme::new(theme, table)
}

/// The pixel center of a mounted render node, in root-relative coordinates —
/// a reliable dispatch target regardless of how many proxy layers sit
/// between the `RenderTable` cell and its interactive leaf.
fn center_of(laid: &LaidOut, id: RenderId) -> (f32, f32) {
    let origin = laid.absolute_offset(id);
    let size = laid.size(id);
    (
        origin.dx.get() + size.width.get() / 2.0,
        origin.dy.get() + size.height.get() / 2.0,
    )
}

/// The `RenderParagraph` among `paragraphs` whose absolute x-offset falls
/// inside `cell`'s horizontal bounds — used instead of assuming a fixed
/// index into `find_all_by_render_type`'s traversal order, so a test does
/// not silently pass because it picked the wrong glyph.
fn paragraph_in(laid: &LaidOut, cell: RenderId, paragraphs: &[RenderId]) -> RenderId {
    let cell_x0 = laid.absolute_offset(cell).dx.get();
    let cell_x1 = cell_x0 + laid.size(cell).width.get();
    *paragraphs
        .iter()
        .find(|&&p| {
            let x = laid.absolute_offset(p).dx.get();
            x >= cell_x0 - 0.01 && x <= cell_x1 + 0.01
        })
        .expect("a RenderParagraph should be mounted inside this cell's horizontal bounds")
}

// =============================================================================
// Mount + geometry
// =============================================================================

/// A mounted `DataTable` composes exactly one `RenderTable`, with one flat
/// child per (row, column) cell in row-major order — heading row first.
#[test]
fn mounting_composes_a_render_table_with_row_major_children() {
    let table = DataTable::new(
        vec![text_column("Name"), text_column("Role")],
        vec![DataRow::new(vec![text_cell("Ada"), text_cell("Engineer")])],
    );
    let laid = common::lay_out(themed(ThemeData::light(), table), loose(600.0));

    let render_table = laid
        .find_by_render_type("RenderTable")
        .expect("DataTable must mount exactly one RenderTable");
    // 2 columns x (1 heading row + 1 data row) = 4 flat children, no
    // checkbox column since no row is selectable.
    assert_eq!(laid.children(render_table).len(), 4);
}

/// The heading row is `56.0` tall and each data row is `48.0` tall
/// (`kMinInteractiveDimension`) by default — verified at the oracle tag,
/// NOT `52.0` (see `data_table.rs`'s module docs for the correction).
#[test]
fn default_row_heights_match_the_verified_m3_token_table() {
    let table = DataTable::new(
        vec![text_column("Name")],
        vec![DataRow::new(vec![text_cell("Ada")])],
    );
    let laid = common::lay_out(themed(ThemeData::light(), table), loose(400.0));
    let render_table = laid.find_by_render_type("RenderTable").unwrap();

    let heading_cell = laid.child(render_table, 0);
    let data_cell = laid.child(render_table, 1);

    assert_eq!(
        laid.size(heading_cell).height.get(),
        56.0,
        "heading row height must default to 56.0"
    );
    assert_eq!(
        laid.size(data_cell).height.get(),
        48.0,
        "data row height must default to kMinInteractiveDimension (48.0), not 52.0"
    );
}

/// A `DataTableThemeData.heading_row_height` override reaches the MOUNTED
/// tree, not just `resolve_style` computed in isolation.
#[test]
fn heading_row_height_theme_override_reaches_the_mounted_tree() {
    let mut theme = ThemeData::light();
    theme.data_table_theme = Some(DataTableThemeData {
        heading_row_height: Some(80.0),
        ..Default::default()
    });
    let table = DataTable::new(
        vec![text_column("Name")],
        vec![DataRow::new(vec![text_cell("Ada")])],
    );
    let laid = common::lay_out(themed(theme, table), loose(400.0));
    let render_table = laid.find_by_render_type("RenderTable").unwrap();
    let heading_cell = laid.child(render_table, 0);

    assert_eq!(laid.size(heading_cell).height.get(), 80.0);
}

/// A widget-level override beats the theme tier, which beats the M3 default
/// — the full triple, proven on a mounted tree (the theme-vs-default and
/// widget-vs-theme halves are already unit-tested in isolation against
/// `resolve_style`; this closes the loop end to end).
#[test]
fn widget_override_beats_theme_beats_default_on_a_mounted_tree() {
    let mut theme = ThemeData::light();
    theme.data_table_theme = Some(DataTableThemeData {
        heading_row_height: Some(80.0),
        ..Default::default()
    });
    let table = DataTable::new(
        vec![text_column("Name")],
        vec![DataRow::new(vec![text_cell("Ada")])],
    )
    .heading_row_height(96.0);
    let laid = common::lay_out(themed(theme, table), loose(400.0));
    let render_table = laid.find_by_render_type("RenderTable").unwrap();
    let heading_cell = laid.child(render_table, 0);

    assert_eq!(
        laid.size(heading_cell).height.get(),
        96.0,
        "the widget-level override must win over both the theme and the M3 default"
    );
}

/// Numeric columns right-align their content; non-numeric columns
/// left-align — both under an identical fixed column width, so the ONLY
/// variable is `DataColumn::numeric`.
#[test]
fn numeric_columns_right_align_their_cell_content() {
    let table = DataTable::new(
        vec![
            text_column("A").column_width(TableColumnWidth::Fixed(160.0)),
            text_column("N")
                .numeric(true)
                .column_width(TableColumnWidth::Fixed(160.0)),
        ],
        vec![DataRow::new(vec![text_cell("1"), text_cell("2")])],
    );
    let laid = common::lay_out(themed(ThemeData::light(), table), loose(600.0));
    let render_table = laid.find_by_render_type("RenderTable").unwrap();

    // Row-major flat children: [heading A, heading N, data A, data N].
    let data_a = laid.child(render_table, 2);
    let data_n = laid.child(render_table, 3);
    let paragraphs = laid.find_all_by_render_type("RenderParagraph");

    let glyph_a = paragraph_in(&laid, data_a, &paragraphs);
    let glyph_n = paragraph_in(&laid, data_n, &paragraphs);

    let local_x_a = laid.absolute_offset(glyph_a).dx.get() - laid.absolute_offset(data_a).dx.get();
    let local_x_n = laid.absolute_offset(glyph_n).dx.get() - laid.absolute_offset(data_n).dx.get();

    assert!(
        local_x_a < local_x_n,
        "the numeric column's glyph ({local_x_n}) must sit further right within its \
         160px column than the non-numeric column's glyph ({local_x_a})"
    );
    assert!(
        local_x_n > 80.0,
        "a right-aligned glyph in a 160px column should sit past the column's midpoint, \
         got local x {local_x_n}"
    );
}

// =============================================================================
// Selection dispatch
// =============================================================================

/// Tapping a selectable row's checkbox cell fires `on_select_changed` with
/// the row's next (flipped) value.
#[test]
fn row_checkbox_tap_fires_on_select_changed_with_the_next_value() {
    let observed = Rc::new(RefCell::new(None));
    let recorder = Rc::clone(&observed);
    let table = DataTable::new(
        vec![text_column("Name")],
        vec![
            DataRow::new(vec![text_cell("Ada")])
                .selected(false)
                .on_select_changed(move |next| *recorder.borrow_mut() = Some(next)),
        ],
    );
    let laid = common::lay_out(themed(ThemeData::light(), table), loose(400.0));
    let render_table = laid.find_by_render_type("RenderTable").unwrap();

    // Checkbox column now leads: [heading checkbox, heading Name, row checkbox, row Name].
    let row_checkbox = laid.child(render_table, 2);
    let (x, y) = center_of(&laid, row_checkbox);
    laid.dispatch_pointer_down(x, y);
    laid.dispatch_pointer_up(x, y);

    assert_eq!(
        *observed.borrow(),
        Some(true),
        "tapping an unselected, selectable row's checkbox must fire on_select_changed(true)"
    );
}

/// The heading checkbox selects every selectable row when none are
/// currently checked.
#[test]
fn heading_checkbox_selects_all_when_none_are_checked() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let make_row = |label: &str| {
        let recorder = Rc::clone(&log);
        let label = label.to_string();
        DataRow::new(vec![text_cell(&label)]).on_select_changed(move |next| {
            recorder.borrow_mut().push((label.clone(), next));
        })
    };
    let table = DataTable::new(
        vec![text_column("Name")],
        vec![make_row("Ada"), make_row("Grace")],
    );
    let laid = common::lay_out(themed(ThemeData::light(), table), loose(400.0));
    let render_table = laid.find_by_render_type("RenderTable").unwrap();

    let heading_checkbox = laid.child(render_table, 0);
    let (x, y) = center_of(&laid, heading_checkbox);
    laid.dispatch_pointer_down(x, y);
    laid.dispatch_pointer_up(x, y);

    let calls = log.borrow();
    assert_eq!(calls.len(), 2, "both selectable rows must be toggled");
    assert!(
        calls.iter().all(|(_, next)| *next),
        "every row must be selected true"
    );
}

/// The heading checkbox clears every selectable row when all are currently
/// checked.
#[test]
fn heading_checkbox_clears_all_when_every_row_is_checked() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let make_row = |label: &str| {
        let recorder = Rc::clone(&log);
        let label = label.to_string();
        DataRow::new(vec![text_cell(&label)])
            .selected(true)
            .on_select_changed(move |next| {
                recorder.borrow_mut().push((label.clone(), next));
            })
    };
    let table = DataTable::new(
        vec![text_column("Name")],
        vec![make_row("Ada"), make_row("Grace")],
    );
    let laid = common::lay_out(themed(ThemeData::light(), table), loose(400.0));
    let render_table = laid.find_by_render_type("RenderTable").unwrap();

    let heading_checkbox = laid.child(render_table, 0);
    let (x, y) = center_of(&laid, heading_checkbox);
    laid.dispatch_pointer_down(x, y);
    laid.dispatch_pointer_up(x, y);

    let calls = log.borrow();
    assert_eq!(calls.len(), 2, "both selected rows must be toggled");
    assert!(
        calls.iter().all(|(_, next)| !*next),
        "every row must be cleared false"
    );
}

/// Tristate quirk (Flutter parity: `_handleSelectAll`'s `someChecked ||
/// (checked ?? false)`): tapping the heading checkbox while it is in the
/// INDETERMINATE state (some, not all, rows checked) always SELECTS all
/// rows — it never clears them, even though the checkbox's own naive
/// tap-cycle would suggest otherwise. A broken `someChecked ||` (e.g.
/// dropped, or `&&`) flips this assertion.
#[test]
fn heading_checkbox_tap_selects_all_from_the_indeterminate_state() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let selected_recorder = Rc::clone(&log);
    let unselected_recorder = Rc::clone(&log);
    let table = DataTable::new(
        vec![text_column("Name")],
        vec![
            DataRow::new(vec![text_cell("Ada")])
                .selected(true)
                .on_select_changed(move |next| selected_recorder.borrow_mut().push(("Ada", next))),
            DataRow::new(vec![text_cell("Grace")])
                .selected(false)
                .on_select_changed(move |next| {
                    unselected_recorder.borrow_mut().push(("Grace", next));
                }),
        ],
    );
    let laid = common::lay_out(themed(ThemeData::light(), table), loose(400.0));
    let render_table = laid.find_by_render_type("RenderTable").unwrap();

    let heading_checkbox = laid.child(render_table, 0);
    let (x, y) = center_of(&laid, heading_checkbox);
    laid.dispatch_pointer_down(x, y);
    laid.dispatch_pointer_up(x, y);

    let calls = log.borrow();
    assert!(
        calls.iter().all(|(_, next)| *next),
        "tapping the indeterminate heading checkbox must select every row, got {calls:?}"
    );
}

/// A row with no `on_select_changed` handler is not selectable: its
/// checkbox cell swallows a tap with no observable effect, even though the
/// table shows a checkbox column (because another row IS selectable).
#[test]
fn a_row_with_no_handler_swallows_a_checkbox_tap() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let recorder = Rc::clone(&log);
    let table = DataTable::new(
        vec![text_column("Name")],
        vec![
            DataRow::new(vec![text_cell("Ada")])
                .on_select_changed(move |next| recorder.borrow_mut().push(next)),
            DataRow::new(vec![text_cell("Unselectable")]), // no on_select_changed
        ],
    );
    let laid = common::lay_out(themed(ThemeData::light(), table), loose(400.0));
    let render_table = laid.find_by_render_type("RenderTable").unwrap();

    // Row-major: [heading checkbox, heading Name, Ada checkbox, Ada Name,
    // Unselectable checkbox, Unselectable Name] — index 4 is the disabled row's cell.
    let disabled_row_checkbox = laid.child(render_table, 4);
    let (x, y) = center_of(&laid, disabled_row_checkbox);
    laid.dispatch_pointer_down(x, y);
    laid.dispatch_pointer_up(x, y);

    assert!(
        log.borrow().is_empty(),
        "a non-selectable row's checkbox tap must fire no handler at all"
    );
}
