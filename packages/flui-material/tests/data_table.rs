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
use flui_sdk::widgets::Text;

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
    let render_table = laid.try_find_by_render_type("RenderTable").unwrap();
    let heading_cell = laid.child(render_table, 0);

    assert_eq!(
        laid.size(heading_cell).height,
        96.0,
        "the widget-level override must win over both the theme and the M3 default"
    );
}

// =============================================================================
// Selection dispatch
// =============================================================================

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
    let render_table = laid.try_find_by_render_type("RenderTable").unwrap();

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
