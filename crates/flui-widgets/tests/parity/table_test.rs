//! ## Test parity notes
//!
//! Two Flutter oracles are represented in this file:
//!
//! ### `packages/flutter/test/rendering/table_test.dart` (pre-existing)
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
//!
//! ### `packages/flutter/test/widgets/table_test.dart` (tag `3.44.0`, Business.1 slice)
//!
//! 27 `testWidgets(` cases total. Every one is accounted for below —
//! ported, cited as already covered, or out-of-scope with a stated reason.
//! Column-width algorithm real-vs-stub verdict (`crates/flui-objects/src/layout/table.rs`):
//! `Fixed`/`Flex`/`Fraction`/`Max`/`Min` are real closed-form math; `Intrinsic`
//! is real too — it queries each cell's `child_min_intrinsic_width`/
//! `child_max_intrinsic_width` through `BoxIntrinsicsCtx`, not a stub. No
//! `todo!`/`unimplemented!`/fake-zero shortcuts found in the Table stack.
//!
//! **Ported** (12 new tests below, this oracle):
//! - `'Table widget - control test'` (LTR half) —
//!   [`equal_flex_columns_size_every_cell_the_same_across_rows_and_columns`].
//! - `'Table widget - column offset (LTR)'` —
//!   [`fixed_column_widths_place_each_cell_at_its_exact_offset`].
//! - `'Table border - smoke test'` (LTR half) —
//!   [`table_border_paints_without_crashing`].
//! - `'Table widget - changing table dimensions'` —
//!   [`changing_row_and_column_count_reuses_and_discards_cells_by_flat_position`]
//!   (documents a real divergence — see its doc comment).
//! - `'Really small deficit double precision error'` —
//!   [`many_equal_flex_columns_converge_without_hanging`].
//! - `'Calculating flex columns with small width deficit'` —
//!   [`mixed_flex_factors_under_a_tight_deficit_do_not_panic`].
//! - `'Table widget - repump test'` —
//!   [`repump_with_different_content_recomputes_equal_flex_columns`].
//! - `'Table widget - intrinsic sizing test'` —
//!   [`intrinsic_column_width_sizes_to_the_widest_cells_content`].
//! - `'Table widget - intrinsic sizing test, resizing'` —
//!   [`intrinsic_column_width_shrinks_when_content_shrinks`].
//! - `'Table widget - intrinsic sizing test, changing column widths'` —
//!   [`switching_default_column_width_to_intrinsic_across_a_rebuild_resizes_columns`].
//! - `'Table widget - Default textBaseline is null'` —
//!   [`baseline_alignment_without_a_text_baseline_degrades_to_top_instead_of_asserting`]
//!   (documents a real, already-flagged divergence).
//! - `'Table widget requires all TableRows to have same number of children'` —
//!   [`irregular_row_lengths_trip_the_debug_assert`] (documents a divergence).
//! - `'Does not crash if a child RenderObject is replaced by another RenderObject
//!   of a different type'` and `'Can replace child with a different RenderObject
//!   type'` — one combined test —
//!   [`a_single_cells_render_object_type_can_be_swapped_without_disturbing_siblings`].
//!
//! **Cited-already-covered** (no new code needed):
//! - `'Table widget - empty'` — the empty-table sizing contract is already
//!   proven by this file's `empty_table_fills_a_tight_surface` and
//!   `empty_table_measures_zero_inside_a_loose_surface` (ported from the
//!   *rendering* oracle, same behavior at the widget layer).
//!
//! **Out of scope** (with reason — none silently narrowed or faked green):
//! - `'Table widget - column offset (RTL)'`, the RTL half of `'Table widget -
//!   control test'`, and the RTL half of `'Table border - smoke test'` — FLUI's
//!   `RenderTable` is documented LTR-only (`flui-objects/src/layout/table.rs`
//!   module doc: "FLUI has not yet plumbed `TextDirection` into layout");
//!   `Table` has no `text_direction` parameter to express RTL at all.
//! - `'Table widget calculate depth'` — no harness accessor exposes
//!   `RenderObject`/`Element` depth for a mounted node; a general tree-depth
//!   invariant, not specific to Table's column-width/row-sizing algorithms
//!   this slice targets.
//! - `'Table widget can be detached and re-attached'` — no generic
//!   GlobalKey-wrapping combinator for an arbitrary `View` was found exposed
//!   to tests within this slice's budget (only specific widgets thread a key
//!   field internally); reparenting-with-GlobalKey needs its own follow-up.
//! - `'Table widget - moving test'` and `'Table widget - keyed rows'` — real
//!   gap: `TableRow` (`flui-widgets/src/layout/table.rs`) has no `key` field
//!   at all, so per-row keyed reconciliation (Flutter's bespoke
//!   `_TableElement` diffing) cannot be expressed — filed to Cross.H.
//! - `'Table widget - global key reparenting'` — the oracle calls
//!   `table.row(0).length` / `table.column(2)` on `RenderTable`; FLUI's
//!   `RenderTable` has no `row(usize)`/`column(usize)` accessor API. Adding
//!   one is a production API-surface change, out of scope for a test port.
//! - `'Table widget diagnostics'` — Flutter's `toStringDeep()` output is a
//!   Dart-specific ASCII tree with hash-code placeholders; no FLUI
//!   equivalent producing an identical string format was confirmed within
//!   budget, and diagnostics formatting is not this slice's target.
//! - `'Do not crash if a child that has not been laid out in a previous
//!   build is removed'` — needs a build-only partial pump (Flutter's `phase:
//!   EnginePhase.build`, skipping layout); FLUI's test harness `pump`/
//!   `pump_widget` always drive a full build+layout+paint frame, with no
//!   build-only primitive.
//! - `'TableRow with no children throws an error message'` — real gap:
//!   `TableRow::new(vec![])` is silently accepted (no validation anywhere in
//!   `Table`/`TableRow`/`RenderTable`); a table whose first row is empty
//!   just resolves `column_count() == 0` and short-circuits to `Size::ZERO`
//!   instead of erroring, diverging from the oracle's explicit rejection.
//!   Filed to Cross.H (distinct from the irregular-row-lengths gap above:
//!   this is "zero children in every row", not "rows disagree in length").
//! - `'Set defaultVerticalAlignment to intrinsic height and check their
//!   heights'` — real gap, already flagged in-code:
//!   `flui-objects/src/layout/table.rs`'s module doc lists
//!   `TableCellVerticalAlignment::IntrinsicHeight` under "Deferred"; the
//!   variant does not exist in `flui_types::layout::TableCellVerticalAlignment`
//!   at all, so the oracle case cannot even be expressed in FLUI's current
//!   API. Filed to Cross.H (pre-existing gap, re-confirmed here).
//! - `'Table has correct roles in semantics'` and `'Table reuse the
//!   semantics nodes for cell wrappers'` — semantics-role assignment for
//!   `Table`/`TableCell` (and, for the second case, focus-driven semantics
//!   node reuse on a text field inside a cell) is outside this column-width-
//!   focused slice's budget to verify; not investigated here.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use flui_types::geometry::px;
use flui_types::layout::{TableCellVerticalAlignment, TableColumnWidth};
// `prelude::*` covers `SizedBox`/`Table`/`TableRow`/`Text`/`ViewExt` and the
// `StatefulView`/`ViewState`/`BuildContext`/`IntoView` authoring spine needed
// by the render-object-type-swap test below.
use flui_widgets::prelude::*;

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

// =============================================================================
// `packages/flutter/test/widgets/table_test.dart` (tag `3.44.0`) ports
// =============================================================================

/// A 3×3 table of short `Text` cells with no column-width overrides (every
/// column defaults to `Flex(1.0)`) sizes every cell to the same width AND
/// height, across both rows and columns.
///
/// Flutter parity: `'Table widget - control test'` (LTR half only — the
/// oracle also runs an RTL half, out of scope: see the module doc's RTL
/// note). `boxA.size == boxD.size == boxG.size == boxB.size` — three equal
/// flex columns give every column the same width, and every row's single
/// line of text gives every row the same height.
#[test]
fn equal_flex_columns_size_every_cell_the_same_across_rows_and_columns() {
    let laid = harness::pump_widget(
        Table::new(vec![
            TableRow::new(vec![
                Text::new("AAAAAA").boxed(),
                Text::new("B").boxed(),
                Text::new("C").boxed(),
            ]),
            TableRow::new(vec![
                Text::new("D").boxed(),
                Text::new("EEE").boxed(),
                Text::new("F").boxed(),
            ]),
            TableRow::new(vec![
                Text::new("G").boxed(),
                Text::new("H").boxed(),
                Text::new("III").boxed(),
            ]),
        ]),
        harness::screen(),
    );

    let box_a = laid.find_text("AAAAAA").expect("cell A must be mounted");
    let box_d = laid.find_text("D").expect("cell D must be mounted");
    let box_g = laid.find_text("G").expect("cell G must be mounted");
    let box_b = laid.find_text("B").expect("cell B must be mounted");

    assert_eq!(
        laid.size(box_a),
        laid.size(box_d),
        "column 0 must be the same width/height in every row"
    );
    assert_eq!(
        laid.size(box_a),
        laid.size(box_g),
        "column 0 must be the same width/height in every row"
    );
    assert_eq!(
        laid.size(box_a),
        laid.size(box_b),
        "three equal-Flex(1.0) columns are equally wide, and every row has \
         one line of text, so column 0 and column 1 measure the same"
    );
}

/// Three `FixedColumnWidth` columns place every cell at an exact width and
/// left offset, independent of the table's `defaultColumnWidth`.
///
/// Flutter parity: `'Table widget - column offset (LTR)'` (RTL half out of
/// scope — see module doc). Exact assertions preserved: column widths
/// 100/110/125, table width 335, and each column's left offset is the sum
/// of the widths of the columns before it.
#[test]
fn fixed_column_widths_place_each_cell_at_its_exact_offset() {
    // The oracle wraps the table in `Center` for LOOSE constraints so it
    // sizes to its natural (335px) width rather than being forced to fill
    // the 800px tight test surface.
    let laid = harness::pump_widget(
        Center::new().child(
            Table::new(vec![
                TableRow::new(vec![
                    Text::new("A1").boxed(),
                    Text::new("B1").boxed(),
                    Text::new("C1").boxed(),
                ]),
                TableRow::new(vec![
                    Text::new("A2").boxed(),
                    Text::new("B2").boxed(),
                    Text::new("C2").boxed(),
                ]),
                TableRow::new(vec![
                    Text::new("A3").boxed(),
                    Text::new("B3").boxed(),
                    Text::new("C3").boxed(),
                ]),
            ])
            .column_widths(HashMap::from([
                (0, TableColumnWidth::Fixed(100.0)),
                (1, TableColumnWidth::Fixed(110.0)),
                (2, TableColumnWidth::Fixed(125.0)),
            ]))
            .default_column_width(TableColumnWidth::Fixed(333.0)),
        ),
        harness::screen(),
    );

    let table_id = laid.find_by_render_type("RenderTable");
    assert_eq!(
        laid.size(table_id).width,
        px(335.0),
        "table width must be the sum of the three explicit column widths, \
         not the (unused) default of 333"
    );

    for row in ["1", "2", "3"] {
        let a = laid
            .find_text(&format!("A{row}"))
            .unwrap_or_else(|| panic!("cell A{row} must be mounted"));
        let b = laid
            .find_text(&format!("B{row}"))
            .unwrap_or_else(|| panic!("cell B{row} must be mounted"));
        let c = laid
            .find_text(&format!("C{row}"))
            .unwrap_or_else(|| panic!("cell C{row} must be mounted"));

        assert_eq!(laid.size(a).width, px(100.0), "column 0 width, row {row}");
        assert_eq!(laid.size(b).width, px(110.0), "column 1 width, row {row}");
        assert_eq!(laid.size(c).width, px(125.0), "column 2 width, row {row}");

        assert_eq!(
            laid.offset(a).dx,
            px(0.0),
            "column 0 left offset, row {row}"
        );
        assert_eq!(
            laid.offset(b).dx,
            px(100.0),
            "column 1 left offset = column 0's width, row {row}"
        );
        assert_eq!(
            laid.offset(c).dx,
            px(210.0),
            "column 2 left offset = column 0 + column 1 widths, row {row}"
        );
    }
}

/// A bordered table lays out and paints without panicking.
///
/// Flutter parity: `'Table border - smoke test'` (LTR half only — see
/// module doc's RTL note). The oracle's own assertion is only
/// `tester.takeException()` staying unset — this port's equivalent is
/// simply that pumping (which paints through `RenderTable::paint`, the
/// border branch) does not panic.
#[test]
fn table_border_paints_without_crashing() {
    let border = flui_types::styling::TableBorder::all(flui_types::styling::BorderSide::new(
        flui_types::Color::BLACK,
        px(1.0),
        flui_types::styling::BorderStyle::Solid,
    ));
    let laid = harness::pump_widget(
        Table::new(vec![
            TableRow::new(vec![
                Text::new("AAAAAA").boxed(),
                Text::new("B").boxed(),
                Text::new("C").boxed(),
            ]),
            TableRow::new(vec![
                Text::new("D").boxed(),
                Text::new("EEE").boxed(),
                Text::new("F").boxed(),
            ]),
            TableRow::new(vec![
                Text::new("G").boxed(),
                Text::new("H").boxed(),
                Text::new("III").boxed(),
            ]),
        ])
        .border(border),
        harness::screen(),
    );

    // Reaching here (mount + layout + paint) without a panic is the whole
    // assertion — matches the oracle's `tester.takeException()` check.
    let table_id = laid.find_by_render_type("RenderTable");
    assert!(laid.size(table_id).width.get() > 0.0);
}

/// Shrinking a table from 3 rows × 3 columns to 2 rows × 4 columns reuses
/// the cell at flat index 0 (`A` → `a`) but does NOT preserve the cell that
/// used to be at flat index 6 (`G`) — a different element now occupies that
/// slot (`g`), because FLUI's flat multi-child element matches purely by
/// index, not Flutter's per-row `_TableElement` diffing.
///
/// Flutter parity: `'Table widget - changing table dimensions'` —
/// `expect(boxA1, equals(boxA2)); expect(boxG1, isNot(equals(boxG2)));`.
/// Flutter's real result comes from ROW-scoped matching: row 0 and row 1
/// both survive (their own cells are reused position-by-position within the
/// row), but row 2 (`G`,`H`,`I`) has no counterpart row in the 2-row tree,
/// so it is entirely disposed; `g` is actually a reuse of the OLD row 1's
/// third slot (which held `F`), not row 2's first slot (`G`) — they were
/// never the same element in Flutter either. `RenderTable::compute_dry_baseline`
/// row-major-flat-list-in-Vec: FLUI's `Table` (`flui-widgets/src/layout/table.rs`
/// module doc) reconciles as ONE flat, position-indexed child list instead —
/// documented and out of scope for this slice ("Reconciliation uses the same
/// flat multi-child element ... not Flutter's bespoke per-row keyed
/// `_TableElement` diffing"). Under flat-index matching, flat index 6 in the
/// OLD 3×3 tree (`G`) and flat index 6 in the NEW 2×4 tree (`g`) are the SAME
/// slot, so FLUI reuses that element too — contradicting the oracle's
/// `isNot(equals())`. Kept `#[ignore]`d with the oracle's real expectation
/// rather than silently dropped or rewritten to match FLUI's divergent
/// behavior. Un-ignore when Table grows row-scoped (not flat-index) child
/// reconciliation. Filed to Cross.H.
#[test]
#[ignore = "documented divergence: FLUI's Table reconciles by flat child \
            index, not Flutter's row-scoped _TableElement diffing — see \
            doc comment; filed to Cross.H"]
fn changing_row_and_column_count_reuses_and_discards_cells_by_flat_position() {
    let mut laid = harness::pump_widget(
        Table::new(vec![
            TableRow::new(vec![
                Text::new("A").boxed(),
                Text::new("B").boxed(),
                Text::new("C").boxed(),
            ]),
            TableRow::new(vec![
                Text::new("D").boxed(),
                Text::new("E").boxed(),
                Text::new("F").boxed(),
            ]),
            TableRow::new(vec![
                Text::new("G").boxed(),
                Text::new("H").boxed(),
                Text::new("I").boxed(),
            ]),
        ]),
        harness::screen(),
    );
    let box_a1 = laid.find_text("A").expect("A must be mounted");
    let box_g1 = laid.find_text("G").expect("G must be mounted");

    laid.pump_widget(Table::new(vec![
        TableRow::new(vec![
            Text::new("a").boxed(),
            Text::new("b").boxed(),
            Text::new("c").boxed(),
            Text::new("d").boxed(),
        ]),
        TableRow::new(vec![
            Text::new("e").boxed(),
            Text::new("f").boxed(),
            Text::new("g").boxed(),
            Text::new("h").boxed(),
        ]),
    ]));
    let box_a2 = laid.find_text("a").expect("a must be mounted");
    let box_g2 = laid.find_text("g").expect("g must be mounted");

    assert_eq!(
        box_a1, box_a2,
        "flat index 0 is reused across the rebuild (A -> a, same element)"
    );
    assert_ne!(
        box_g1, box_g2,
        "the oracle's real expectation: the element that held G is not the \
         same element that now holds g (row-scoped matching in Flutter)"
    );
}

/// Many equal-`Flex` columns (6 columns, 12 identical 16×16 cells) under the
/// default 800×600 test surface must converge and return, not hang.
///
/// Flutter parity: `'Really small deficit double precision error'` —
/// regression for flutter/flutter#27083. The oracle's own comment: "If the
/// above bug is present this test will never terminate." This port's
/// equivalent assertion is that `pump_widget` returns at all, plus a sanity
/// check that the resulting table size is finite and non-negative (a hang
/// or a NaN/negative-width result would both indicate the shrink/grow loop
/// broke down).
#[test]
fn many_equal_flex_columns_converge_without_hanging() {
    let cell = || SizedBox::new(16.0, 16.0).boxed();
    let laid = harness::pump_widget(
        Table::new(vec![
            TableRow::new((0..6).map(|_| cell()).collect()),
            TableRow::new((0..6).map(|_| cell()).collect()),
        ]),
        harness::screen(),
    );

    let table_id = laid.find_by_render_type("RenderTable");
    let size = laid.size(table_id);
    assert!(
        size.width.get().is_finite() && size.width.get() >= 0.0,
        "table width must converge to a finite, non-negative value, got {size:?}"
    );
    assert!(
        size.height.get().is_finite() && size.height.get() >= 0.0,
        "table height must converge to a finite, non-negative value, got {size:?}"
    );
}

/// A mix of one default-flex column and six `Flex(0.123)` columns, laid out
/// under a tight 600×800 surface much narrower than the columns' combined
/// ideal width, must not panic the grow/shrink debug assertions.
///
/// Flutter parity: `'Calculating flex columns with small width deficit'` —
/// the oracle's own comment: "If the error is present, pumpWidget() will
/// fail due to an unsatisfied assertion during the layout phase." This port
/// additionally asserts every resulting column width is non-negative and
/// that a row's widths sum to the 600px surface width — the concrete
/// invariant `grow_and_shrink_column_widths`'s own `debug_assert!(widths[x]
/// >= 0.0, ...)` protects.
#[test]
fn mixed_flex_factors_under_a_tight_deficit_do_not_panic() {
    let cell = || SizedBox::new(1.0, 1.0).boxed();
    let mut column_widths = HashMap::new();
    column_widths.insert(0, TableColumnWidth::Flex(1.0));
    for x in 1..7 {
        column_widths.insert(x, TableColumnWidth::Flex(0.123));
    }

    let laid = harness::pump_widget(
        Table::new(vec![
            TableRow::new((0..7).map(|_| cell()).collect()),
            TableRow::new((0..7).map(|_| cell()).collect()),
        ])
        .column_widths(column_widths),
        harness::screen_of(600.0, 800.0),
    );

    let table_id = laid.find_by_render_type("RenderTable");
    let mut total_width = px(0.0);
    for x in 0..7 {
        let cell_id = laid.child(table_id, x);
        let width = laid.size(cell_id).width;
        assert!(
            width.get() >= 0.0,
            "column {x} width must not go negative under the shrink pass, got {width:?}"
        );
        total_width += width;
    }
    assert!(
        (total_width.get() - 600.0).abs() < 1e-1,
        "row 0's column widths must sum to the 600px surface width, got {total_width:?}"
    );
}

/// Re-pumping a `Table` with the same shape (3×3) but different text content
/// still recomputes equal-flex columns correctly through `update_render_object`
/// — not just `create_render_object`.
///
/// Flutter parity: `'Table widget - repump test'` — `boxA.size ==
/// boxD.size == boxG.size == boxB.size` after the second `pumpWidget` call.
#[test]
fn repump_with_different_content_recomputes_equal_flex_columns() {
    let mut laid = harness::pump_widget(
        Table::new(vec![
            TableRow::new(vec![
                Text::new("AAAAAA").boxed(),
                Text::new("B").boxed(),
                Text::new("C").boxed(),
            ]),
            TableRow::new(vec![
                Text::new("D").boxed(),
                Text::new("EEE").boxed(),
                Text::new("F").boxed(),
            ]),
            TableRow::new(vec![
                Text::new("G").boxed(),
                Text::new("H").boxed(),
                Text::new("III").boxed(),
            ]),
        ]),
        harness::screen(),
    );

    laid.pump_widget(Table::new(vec![
        TableRow::new(vec![
            Text::new("AAA").boxed(),
            Text::new("B").boxed(),
            Text::new("C").boxed(),
        ]),
        TableRow::new(vec![
            Text::new("D").boxed(),
            Text::new("E").boxed(),
            Text::new("FFFFFF").boxed(),
        ]),
        TableRow::new(vec![
            Text::new("G").boxed(),
            Text::new("H").boxed(),
            Text::new("III").boxed(),
        ]),
    ]));

    let box_a = laid.find_text("AAA").expect("AAA must be mounted");
    let box_d = laid.find_text("D").expect("D must be mounted");
    let box_g = laid.find_text("G").expect("G must be mounted");
    let box_b = laid.find_text("B").expect("B must be mounted");

    assert_eq!(laid.size(box_a), laid.size(box_d));
    assert_eq!(laid.size(box_a), laid.size(box_g));
    assert_eq!(laid.size(box_a), laid.size(box_b));
}

/// `IntrinsicColumnWidth` sizes every column to its widest cell's real
/// content width — a longer string in column 0 (`AAA`) makes it wider than
/// column 1 (`B`), proving the column-width algorithm actually measures
/// cells rather than defaulting to a stub value.
///
/// Flutter parity: `'Table widget - intrinsic sizing test'` —
/// `expect(boxA.size.width, greaterThan(boxB.size.width)); expect(boxA.size.height,
/// equals(boxB.size.height));`. FLUI's text stack (cosmic-text) has no
/// Ahem-equivalent deterministic font, so — like `text_test.rs`'s ported
/// cases — this asserts the same relative *direction* Flutter asserts, not
/// exact pixel widths.
#[test]
fn intrinsic_column_width_sizes_to_the_widest_cells_content() {
    let laid = harness::pump_widget(
        Table::new(vec![
            TableRow::new(vec![
                Text::new("AAA").boxed(),
                Text::new("B").boxed(),
                Text::new("C").boxed(),
            ]),
            TableRow::new(vec![
                Text::new("D").boxed(),
                Text::new("E").boxed(),
                Text::new("FFFFFF").boxed(),
            ]),
            TableRow::new(vec![
                Text::new("G").boxed(),
                Text::new("H").boxed(),
                Text::new("III").boxed(),
            ]),
        ])
        .default_column_width(TableColumnWidth::Intrinsic { flex: None }),
        harness::screen(),
    );

    let box_a = laid.find_text("AAA").expect("AAA must be mounted");
    let box_d = laid.find_text("D").expect("D must be mounted");
    let box_g = laid.find_text("G").expect("G must be mounted");
    let box_b = laid.find_text("B").expect("B must be mounted");

    assert_eq!(
        laid.size(box_a),
        laid.size(box_d),
        "column 0 has one width for every row"
    );
    assert_eq!(
        laid.size(box_a),
        laid.size(box_g),
        "column 0 has one width for every row"
    );
    assert!(
        laid.size(box_a).width.get() > laid.size(box_b).width.get(),
        "column 0 ('AAA'/'D'/'G') must be wider than column 1 ('B'/'E'/'H') \
         under real IntrinsicColumnWidth measurement: col0={:?} col1={:?}",
        laid.size(box_a),
        laid.size(box_b),
    );
    assert_eq!(
        laid.size(box_a).height,
        laid.size(box_b).height,
        "IntrinsicColumnWidth only affects width; row height stays uniform \
         for single-line cells of the same style"
    );
}

/// Shrinking column 0's content (`AAAAAA` → `A`) across a rebuild shrinks its
/// `IntrinsicColumnWidth`-sized column — proving the column width is
/// recomputed live, not cached from the first layout.
///
/// Flutter parity: `'Table widget - intrinsic sizing test, resizing'` —
/// `expect(boxA.size.width, lessThan(boxB.size.width));` after the shrink
/// (column 0 becomes narrower than column 1, reversing the first pump's
/// relationship).
#[test]
fn intrinsic_column_width_shrinks_when_content_shrinks() {
    let table = |col0: &str| {
        Table::new(vec![
            TableRow::new(vec![
                Text::new(col0.to_string()).boxed(),
                Text::new("B").boxed(),
                Text::new("C").boxed(),
            ]),
            TableRow::new(vec![
                Text::new("D").boxed(),
                Text::new("EEE").boxed(),
                Text::new("F").boxed(),
            ]),
            TableRow::new(vec![
                Text::new("G").boxed(),
                Text::new("H").boxed(),
                Text::new("III").boxed(),
            ]),
        ])
        .default_column_width(TableColumnWidth::Intrinsic { flex: None })
    };

    let mut laid = harness::pump_widget(table("AAAAAA"), harness::screen());
    laid.pump_widget(table("A"));

    let box_a = laid.find_text("A").expect("A must be mounted");
    let box_d = laid.find_text("D").expect("D must be mounted");
    let box_g = laid.find_text("G").expect("G must be mounted");
    let box_b = laid.find_text("B").expect("B must be mounted");

    assert_eq!(laid.size(box_a), laid.size(box_d));
    assert_eq!(laid.size(box_a), laid.size(box_g));
    assert!(
        laid.size(box_a).width.get() < laid.size(box_b).width.get(),
        "after shrinking column 0's content to a single 'A', column 0 must \
         be narrower than column 1 ('B'/'EEE'/'H'): col0={:?} col1={:?}",
        laid.size(box_a),
        laid.size(box_b),
    );
    assert_eq!(laid.size(box_a).height, laid.size(box_b).height);
}

/// Switching `defaultColumnWidth` from `Flex(1.0)` (equal columns) to
/// `IntrinsicColumnWidth` across a rebuild re-derives column widths from
/// content — column 0 (`AAA`/`D`/`G`) becomes wider than column 1
/// (`B`/`E`/`H`) once intrinsic sizing takes over.
///
/// Flutter parity: `'Table widget - intrinsic sizing test, changing column
/// widths'`.
#[test]
fn switching_default_column_width_to_intrinsic_across_a_rebuild_resizes_columns() {
    let rows = || {
        vec![
            TableRow::new(vec![
                Text::new("AAA").boxed(),
                Text::new("B").boxed(),
                Text::new("C").boxed(),
            ]),
            TableRow::new(vec![
                Text::new("D").boxed(),
                Text::new("E").boxed(),
                Text::new("FFFFFF").boxed(),
            ]),
            TableRow::new(vec![
                Text::new("G").boxed(),
                Text::new("H").boxed(),
                Text::new("III").boxed(),
            ]),
        ]
    };

    let mut laid = harness::pump_widget(Table::new(rows()), harness::screen());
    laid.pump_widget(
        Table::new(rows()).default_column_width(TableColumnWidth::Intrinsic { flex: None }),
    );

    let box_a = laid.find_text("AAA").expect("AAA must be mounted");
    let box_d = laid.find_text("D").expect("D must be mounted");
    let box_g = laid.find_text("G").expect("G must be mounted");
    let box_b = laid.find_text("B").expect("B must be mounted");

    assert_eq!(laid.size(box_a), laid.size(box_d));
    assert_eq!(laid.size(box_a), laid.size(box_g));
    assert!(
        laid.size(box_a).width.get() > laid.size(box_b).width.get(),
        "after switching to IntrinsicColumnWidth, column 0 must be wider \
         than column 1: col0={:?} col1={:?}",
        laid.size(box_a),
        laid.size(box_b),
    );
    assert_eq!(laid.size(box_a).height, laid.size(box_b).height);
}

/// `TableCellVerticalAlignment::Baseline` with no `text_baseline` set does
/// NOT panic — it degrades every baseline-aligned cell to a top-anchored
/// contribution instead. A documented, deliberate FLUI divergence from the
/// oracle's construction-time assertion error.
///
/// Flutter parity: `'Table widget - Default textBaseline is null'` —
/// `expect(() => Table(defaultVerticalAlignment:
/// TableCellVerticalAlignment.baseline), throwsA(isAssertionError...));`.
/// FLUI's `RenderTable::perform_layout` (`flui-objects/src/layout/table.rs`)
/// explicitly generalizes the oracle's `childBaseline == null` fallback to
/// also cover an unset `text_baseline`, with its own rationale comment:
/// "library code must not panic on a config gap." This port asserts the
/// real, graceful behavior (both cells land at `dy = 0`, i.e. top-anchored)
/// rather than pinning the oracle's panic — the divergence is intentional
/// and already documented in the production code, not a silently-introduced
/// gap.
#[test]
fn baseline_alignment_without_a_text_baseline_degrades_to_top_instead_of_asserting() {
    let laid = harness::pump_widget(
        Table::new(vec![TableRow::new(vec![
            SizedBox::new(10.0, 10.0).boxed(),
            SizedBox::new(10.0, 30.0).boxed(),
        ])])
        .default_vertical_alignment(TableCellVerticalAlignment::Baseline),
        harness::screen(),
    );

    let table_id = laid.find_by_render_type("RenderTable");
    let cell0 = laid.child(table_id, 0);
    let cell1 = laid.child(table_id, 1);

    assert_eq!(
        laid.offset(cell0).dy,
        px(0.0),
        "with no text_baseline, a Baseline-aligned cell must degrade to \
         top-anchored (dy=0), not panic"
    );
    assert_eq!(
        laid.offset(cell1).dy,
        px(0.0),
        "same degradation applies to every Baseline-aligned cell in the row"
    );
}

/// Rows with a different cell count than the first row trip FLUI's
/// `debug_assert!` — a real panic, but via a different mechanism and
/// message than the oracle's typed `FlutterError`.
///
/// Flutter parity: `'Table widget requires all TableRows to have same
/// number of children'` — `error!.toStringDeep()` contains `'Table contains
/// irregular row lengths.'`. FLUI's `Table::create_render_object`
/// (`flui-widgets/src/layout/table.rs`) instead uses
/// `debug_assert!(..., "every Table row must have the same number of cells
/// as the first row")` — real in debug/test builds (this assertion is what
/// this test exercises), but a `debug_assert!` compiles out entirely in
/// release builds, unlike Flutter's `FlutterError` which is a real,
/// always-on `Result`-shaped error. That gap (release-mode silent
/// acceptance of a malformed table, rather than a graceful error) is a
/// genuine production robustness gap, distinct from this test's job of
/// documenting today's debug-mode behavior; worth a Cross.H filing.
#[test]
#[should_panic(expected = "every Table row must have the same number of cells as the first row")]
fn irregular_row_lengths_trip_the_debug_assert() {
    let _ = harness::pump_widget(
        Table::new(vec![
            TableRow::new(vec![Text::new("Some Text").boxed()]),
            TableRow::new(vec![]),
        ]),
        harness::screen(),
    );
}

// ── Render-object-type-swap regression (flutter/flutter#31473, #69395) ──────

/// One cell in a `Table` swaps between `SizedBox` and `Text` render objects
/// (a `RenderConstrainedBox` ↔ `RenderParagraph` type change at the same
/// flat child-list slot) across a rebuild — the table must not panic, and
/// only the toggled cell's render-object type changes; its siblings are
/// unaffected.
///
/// Flutter parity: `'Does not crash if a child RenderObject is replaced by
/// another RenderObject of a different type'` (regression for
/// flutter/flutter#31473) and `'Can replace child with a different
/// RenderObject type'` (regression for flutter/flutter#69395) — combined
/// into one test since both exercise the same underlying mechanism (a
/// `Table` cell's concrete render-object type changing in place).
#[test]
fn a_single_cells_render_object_type_can_be_swapped_without_disturbing_siblings() {
    #[derive(Clone, StatefulView)]
    struct TypeSwapCell {
        show_text: Arc<AtomicBool>,
    }

    struct TypeSwapCellState {
        show_text: Arc<AtomicBool>,
    }

    impl StatefulView for TypeSwapCell {
        type State = TypeSwapCellState;

        fn create_state(&self) -> Self::State {
            TypeSwapCellState {
                show_text: Arc::clone(&self.show_text),
            }
        }
    }

    impl ViewState<TypeSwapCell> for TypeSwapCellState {
        fn build(&self, _view: &TypeSwapCell, _ctx: &dyn BuildContext) -> impl IntoView {
            if self.show_text.load(Ordering::Relaxed) {
                Text::new("CRASHHH").boxed()
            } else {
                SizedBox::new(1.0, 1.0).boxed()
            }
        }
    }

    let flags: Vec<Arc<AtomicBool>> = (0..6).map(|_| Arc::new(AtomicBool::new(false))).collect();
    let cells = |flags: &[Arc<AtomicBool>]| -> Vec<_> {
        flags
            .iter()
            .map(|flag| {
                TypeSwapCell {
                    show_text: Arc::clone(flag),
                }
                .boxed()
            })
            .collect()
    };

    let table = Table::new(vec![
        TableRow::new(cells(&flags[0..3])),
        TableRow::new(cells(&flags[3..6])),
    ]);
    let mut laid = harness::pump_widget(table, harness::screen());

    assert_eq!(
        laid.find_all_by_render_type("RenderConstrainedBox").len(),
        6,
        "all six cells must start as SizedBox / RenderConstrainedBox"
    );
    assert!(laid.find_text("CRASHHH").is_none());

    // Toggle only the LAST cell — the others must be unaffected.
    flags[5].store(true, Ordering::Relaxed);
    laid.pump();

    assert_eq!(
        laid.find_all_by_render_type("RenderConstrainedBox").len(),
        5,
        "exactly one cell must have swapped away from RenderConstrainedBox"
    );
    assert!(
        laid.find_text("CRASHHH").is_some(),
        "the toggled cell must now be a RenderParagraph showing 'CRASHHH'"
    );
}
