//! `DataTable` widget-level integration coverage — mounts a real `DataTable`
//! through the full render pipeline (`tests/common/mod.rs`, the same harness
//! `tests/card.rs`/`tests/checkbox.rs` use) and proves geometry, selection
//! dispatch, and the theme cascade actually reach a mounted tree, not just
//! `data_table.rs`'s own pure-function unit tests (`resolve_style`,
//! `selection_summary`, `row_decoration`) computed in isolation.

use crate::common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{LaidOut, loose};
use flui_material::{
    DataCell, DataColumn, DataRow, DataTable, DataTableThemeData, Theme, ThemeData,
};
use flui_sdk::foundation::RenderId;
use flui_sdk::widgets::{TableColumnWidth, Text};

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
fn center_of(laid: &LaidOut, id: RenderId) -> (f64, f64) {
    let origin = laid.absolute_offset(id);
    let size = laid.size(id);
    (origin.dx + size.width / 2.0, origin.dy + size.height / 2.0)
}

// =============================================================================
// Mount + geometry
// =============================================================================

/// A widget-level override beats the theme tier, which beats the M3 default
/// — the full triple, proven on a mounted tree (the theme-vs-default and
/// widget-vs-theme halves are already unit-tested in isolation against
/// `resolve_style`; this closes the loop end to end).
pub fn widget_override_beats_theme_beats_default_on_a_mounted_tree() {
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
    let render_table = laid
        .try_find_by_render_type("RenderTable")
        .expect("RenderTable is mounted");
    let heading_cell = laid.child(render_table, 0);

    assert_eq!(
        laid.size(heading_cell).height,
        96.0,
        "the widget-level override must win over both the theme and the M3 default"
    );
}

fn table_cell_insets(theme_margin: Option<f64>, widget_margin: Option<f64>) -> [f64; 3] {
    let mut theme = ThemeData::light();
    theme.data_table_theme = Some(DataTableThemeData {
        checkbox_horizontal_margin: theme_margin,
        ..Default::default()
    });
    let mut table = DataTable::new(
        vec![text_column("Heading").column_width(TableColumnWidth::Fixed(140.0))],
        vec![DataRow::new(vec![text_cell("Entry")]).on_select_changed(|_, _| {})],
    );
    if let Some(margin) = widget_margin {
        table = table.checkbox_horizontal_margin(margin);
    }
    let laid = common::lay_out(themed(theme, table), loose(400.0));
    let render_table = laid.find_by_render_type("RenderTable");
    let heading = laid.find_text("Heading").expect("heading is laid out");
    let entry = laid.find_text("Entry").expect("entry is laid out");
    let heading_cell = laid.child(render_table, 1);
    let data_cell = laid.child(render_table, 3);
    [
        laid.absolute_offset(heading).dx - laid.absolute_offset(heading_cell).dx,
        laid.absolute_offset(entry).dx - laid.absolute_offset(data_cell).dx,
        laid.size(laid.child(render_table, 0)).width,
    ]
}

pub fn themed_checkbox_margin_matches_the_same_widget_margin() {
    let themed = table_cell_insets(Some(13.0), None);
    let overridden = table_cell_insets(None, Some(13.0));
    assert_eq!(
        themed, overridden,
        "both cascade tiers must lay out the same cells"
    );
    assert_eq!(themed, [24.0, 24.0, 44.0]);
}

pub fn checkbox_margin_override_beats_the_theme_without_changing_default_spacing() {
    assert_eq!(table_cell_insets(Some(13.0), Some(7.0)), [24.0, 24.0, 32.0]);
    assert_eq!(table_cell_insets(None, None), [12.0, 12.0, 54.0]);
}

// =============================================================================
// Selection dispatch
// =============================================================================

/// Tristate quirk (`some_checked || checked.unwrap_or(false)`): tapping the heading checkbox while it is in the
/// INDETERMINATE state (some, not all, rows checked) always SELECTS all
/// rows — it never clears them, even though the checkbox's own naive
/// tap-cycle would suggest otherwise. A broken `someChecked ||` (e.g.
/// dropped, or `&&`) flips this assertion.
pub fn heading_checkbox_tap_selects_all_from_the_indeterminate_state() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let selected_recorder = Rc::clone(&log);
    let unselected_recorder = Rc::clone(&log);
    let table = DataTable::new(
        vec![text_column("Name")],
        vec![
            DataRow::new(vec![text_cell("Ada")])
                .selected(true)
                .on_select_changed(move |_cx, next| {
                    selected_recorder.borrow_mut().push(("Ada", next));
                }),
            DataRow::new(vec![text_cell("Grace")])
                .selected(false)
                .on_select_changed(move |_cx, next| {
                    unselected_recorder.borrow_mut().push(("Grace", next));
                }),
        ],
    );
    let laid = common::lay_out(themed(ThemeData::light(), table), loose(400.0));
    let render_table = laid
        .try_find_by_render_type("RenderTable")
        .expect("RenderTable is mounted");

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
