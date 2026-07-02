//! ## Test parity notes
//!
//! Flutter source: `packages/flutter/test/rendering/table_test.dart`
//! - line 17 `'Table control test; tight'`
//! - line 43 `'Table control test; loose'`
//! - line 50 `'Table control test: constrained flex columns'`
//!
//! Widget → render-object mapping:
//! - `Table` → `RenderTable`
//!
//! Divergence: the oracle constructs bare `RenderTable`/`RenderConstrainedBox`
//! trees directly (`rendering_tester.dart`'s `layout()` helper); this port
//! goes through the `Table`/`TableRow`/`SizedBox` widget layer instead,
//! proving the same column-width formulas hold end-to-end through
//! `Table::create_render_object`/`update_render_object`.

use flui_types::geometry::px;
use flui_view::ViewExt;
use flui_widgets::{SizedBox, Table, TableRow};

use crate::harness;

/// A table with zero rows sizes to its incoming tight constraints — the
/// oracle's `rows * columns == 0` early return (`table.dart`'s
/// `performLayout`) constrains `Size.zero` up to the tight bound.
///
/// Flutter parity: `table_test.dart` line 17 `'Table control test; tight'` —
/// `expect(table.size.width, equals(800.0)); expect(table.size.height,
/// equals(600.0));` (Flutter's default test surface is 800x600).
#[test]
fn empty_table_fills_a_tight_surface() {
    let laid = harness::pump_widget(Table::new(vec![]), harness::screen());
    let table_id = laid.find_by_render_type("RenderTable");
    assert_eq!(
        laid.size(table_id),
        crate::common::size(800.0, 600.0),
        "an empty table must fill a tight 800x600 surface, matching \
         `constraints.constrain(Size.zero)` on a tight incoming constraint",
    );
}

/// A table with zero rows measures zero inside a LOOSE surface — the same
/// early return, but `constrain(Size.zero)` on a loose (min=0) constraint
/// stays zero instead of growing to fill it.
///
/// Flutter parity: `table_test.dart` line 43 `'Table control test; loose'` —
/// `expect(table.size, equals(Size.zero));` (table wrapped in a
/// `RenderPositionedBox`, which loosens the constraint it passes down).
#[test]
fn empty_table_measures_zero_inside_a_loose_surface() {
    let laid = harness::pump_widget(Table::new(vec![]), harness::screen_of(0.0, 0.0));
    let table_id = laid.find_by_render_type("RenderTable");
    assert_eq!(laid.size(table_id), crate::common::size(0.0, 0.0));
}

/// 6 equal-flex columns under a 100px-wide tight constraint each resolve to
/// `100.0 / 6`, proving `_computeColumnWidths`' pass 2 (equal-flex growth to
/// the target width) holds through the full `Table` widget wiring, not just
/// the bare `RenderTable` unit tests.
///
/// Flutter parity: `table_test.dart` line 50 `'Table control test:
/// constrained flex columns'` — `const double expectedWidth = 100.0 / 6; for
/// (final child in children) { expect(child.size.width,
/// moreOrLessEquals(expectedWidth)); }`.
#[test]
fn six_equal_flex_columns_share_the_tight_width_equally() {
    let cells: Vec<_> = (0..6).map(|_| SizedBox::new(1.0, 1.0).boxed()).collect();
    let laid = harness::pump_widget(
        Table::new(vec![TableRow::new(cells)]),
        harness::screen_of(100.0, 600.0),
    );
    let table_id = laid.find_by_render_type("RenderTable");

    let expected_width = 100.0 / 6.0;
    for index in 0..6 {
        let cell_id = laid.child(table_id, index);
        let width = laid.size(cell_id).width;
        assert!(
            (width - px(expected_width)).get().abs() < 1e-3,
            "column {index} width must be ~{expected_width:.4}, got {width:?}",
        );
    }
}
