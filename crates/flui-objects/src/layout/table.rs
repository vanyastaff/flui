//! `RenderTable` — variable-arity render object that lays out children in a
//! row-major grid, with per-column width resolution and per-row height sized
//! to the tallest cell.
//!
//! # Flutter equivalence
//!
//! Behavior-faithful port of Flutter's
//! [`RenderTable`](https://api.flutter.dev/flutter/rendering/RenderTable-class.html)
//! (`packages/flutter/lib/src/rendering/table.dart`).
//!
//! # Rust-native differences (documented, not silent)
//!
//! - **No null cells / column span.** Every row-major flat-child-list slot is
//!   a real, present child — `column_count` children per row, always.
//!   Flutter's `RenderBox?` sparse-cell support is out of scope for this
//!   slice; the widget layer (`Table`/`TableRow`) is responsible for keeping
//!   every row exactly `column_count` cells long.
//! - **LTR column ordering only.** Mirrors `RenderWrap`'s and `RenderFlex`'s
//!   documented precedent — FLUI has not yet plumbed `TextDirection` into
//!   layout.
//! - **`TableColumnWidth::Fraction` clamps to `0.0..=1.0`.** The oracle's
//!   `FractionColumnWidth` does NOT clamp (`table.dart`'s
//!   `FractionColumnWidth.minIntrinsicWidth`/`maxIntrinsicWidth` multiply the
//!   raw value unclamped) — this port instead honors
//!   `TableColumnWidth::Fraction`'s own FLUI doc contract ("Values are
//!   clamped to the 0.0-1.0 range"), a deliberate, flagged divergence from
//!   the oracle rather than a bug.
//! - **`MaxColumnWidth`/`MinColumnWidth`** are supported via
//!   [`TableColumnWidth::Max`]/[`TableColumnWidth::Min`]
//!   (`flui_types::layout::table`): each folds both operands' widths (by
//!   max/min) and flex factors, faithfully to the oracle
//!   (`table.dart:235-340`), and nests recursively.
//! - **`IntrinsicColumnWidth`'s optional flex** is supported via
//!   [`TableColumnWidth::Intrinsic`]`{ flex }`: the intrinsic width is the
//!   column's floor and a `Some(flex)` also claims leftover space in the grow
//!   pass, faithfully to the oracle (`table.dart:94`).
//! - **`compute_dry_baseline`** reports the first row's baseline without
//!   committing layout — the dry mirror of
//!   [`compute_distance_to_actual_baseline`](RenderTable::compute_distance_to_actual_baseline):
//!   dry column widths, then the max child dry-baseline over row 0's
//!   `Baseline`-aligned cells (driven by the table's `text_baseline`).
//! - **Deferred**: `TableCellVerticalAlignment::IntrinsicHeight` —
//!   `TableCellVerticalAlignment` keeps its existing FLUI shape (see
//!   `flui_types::layout::table`).

use std::collections::HashMap;

use flui_painting::{paint_box_decoration, paint_table_border};
use flui_tree::Variable;
use flui_types::{
    Offset, Pixels, Rect, Size,
    layout::{TableCellVerticalAlignment, TableColumnWidth},
    styling::{BoxDecoration, TableBorder},
};

use flui_rendering::{
    constraints::BoxConstraints,
    context::{
        BoxDryBaselineCtx, BoxDryLayoutCtx, BoxHitTestContext, BoxIntrinsicsCtx, BoxLayoutContext,
    },
    parent_data::TableCellParentData,
    traits::{RenderBox, TextBaseline},
};

/// Discriminant for [`RenderTable::column_extent`]'s per-cell probe: whether
/// an `Intrinsic` column should measure each cell's minimum or maximum
/// intrinsic width. `Fixed`/`Flex`/`Fraction` columns never touch a cell, so
/// this only matters for the `Intrinsic` variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WidthQuery {
    /// Probe each cell's minimum intrinsic width.
    Min,
    /// Probe each cell's maximum intrinsic width.
    Max,
}

/// Folds two operands' flex factors for a `Max`/`Min` column-width combinator.
///
/// A `None` operand contributes no flex, so the other operand's flex passes
/// through unchanged; when both carry flex, `fold` (`f32::max` for `Max`,
/// `f32::min` for `Min`) picks between them. Mirrors the oracle's
/// `MaxColumnWidth.flex`/`MinColumnWidth.flex` (`table.dart:266-276`/`:318-328`).
fn combine_flex(a: Option<f32>, b: Option<f32>, fold: impl Fn(f32, f32) -> f32) -> Option<f32> {
    match (a, b) {
        (Some(a), Some(b)) => Some(fold(a, b)),
        (a, b) => a.or(b),
    }
}

/// Local convergence epsilon for the two-round shrink in
/// [`RenderTable::compute_column_widths`] — mirrors `wrap.rs`'s
/// `PRECISION_TOLERANCE` convention (the more directly-analogous sibling
/// file) over `flui_foundation::EPSILON_F32`.
const EPSILON: f32 = 1e-6;

/// Lays out children in a row-major grid: `column_count` cells per row, with
/// per-column width resolved by [`TableColumnWidth`] and each row's height
/// sized to its tallest cell.
///
/// `row_count` is always derived as `child_count / column_count` — there is
/// no `rows` field, mirroring how `RenderStack`/`RenderFlow` never own their
/// own child-adoption bookkeeping (that machinery lives in FLUI's generic
/// `ElementKind::render_variable` multi-child element).
#[derive(Debug, Clone)]
pub struct RenderTable {
    column_count: usize,
    column_widths: HashMap<usize, TableColumnWidth>,
    default_column_width: TableColumnWidth,
    default_vertical_alignment: TableCellVerticalAlignment,
    text_baseline: Option<TextBaseline>,
    border: Option<TableBorder>,
    row_decorations: Vec<Option<BoxDecoration<Pixels>>>,

    // Cached geometry, rebuilt on every layout — read by `paint`/`hit_test`.
    /// Row top offsets, length `row_count + 1` (the last entry is the
    /// table's total content height).
    row_tops: Vec<Pixels>,
    /// Column left offsets, length `column_count`.
    column_lefts: Vec<Pixels>,
    /// Total table width (sum of resolved column widths).
    table_width: Pixels,
    /// The first row's baseline distance, if any cell in it resolved to
    /// `TableCellVerticalAlignment::Baseline` with a real baseline value.
    baseline_distance: Option<Pixels>,
}

impl RenderTable {
    /// Creates a table with `column_count` columns and Flutter's defaults:
    /// `default_column_width = Flex(1.0)`, `default_vertical_alignment =
    /// Top`, no border, no row decorations, no explicit text baseline.
    pub fn new(column_count: usize) -> Self {
        Self {
            column_count,
            column_widths: HashMap::new(),
            default_column_width: TableColumnWidth::Flex(1.0),
            default_vertical_alignment: TableCellVerticalAlignment::Top,
            text_baseline: None,
            border: None,
            row_decorations: Vec::new(),
            row_tops: vec![Pixels::ZERO],
            column_lefts: Vec::new(),
            table_width: Pixels::ZERO,
            baseline_distance: None,
        }
    }

    /// Builder: set per-column width overrides.
    #[must_use]
    pub fn with_column_widths(mut self, column_widths: HashMap<usize, TableColumnWidth>) -> Self {
        self.column_widths = column_widths;
        self
    }

    /// Builder: set the width used by columns with no explicit override.
    // Not `const`: `TableColumnWidth` now owns `Box`ed combinator variants, so
    // reassigning the field runs a destructor a const context cannot evaluate.
    #[must_use]
    pub fn with_default_column_width(mut self, width: TableColumnWidth) -> Self {
        self.default_column_width = width;
        self
    }

    /// Builder: set the vertical alignment used by cells with no explicit
    /// [`TableCellParentData::vertical_alignment`].
    #[must_use]
    pub const fn with_default_vertical_alignment(
        mut self,
        alignment: TableCellVerticalAlignment,
    ) -> Self {
        self.default_vertical_alignment = alignment;
        self
    }

    /// Builder: set the text baseline used by
    /// `TableCellVerticalAlignment::Baseline` cells.
    #[must_use]
    pub const fn with_text_baseline(mut self, baseline: Option<TextBaseline>) -> Self {
        self.text_baseline = baseline;
        self
    }

    /// Builder: set the table border.
    #[must_use]
    pub const fn with_border(mut self, border: Option<TableBorder>) -> Self {
        self.border = border;
        self
    }

    /// Builder: set per-row background decorations, indexed by row.
    #[must_use]
    pub fn with_row_decorations(
        mut self,
        row_decorations: Vec<Option<BoxDecoration<Pixels>>>,
    ) -> Self {
        self.row_decorations = row_decorations;
        self
    }

    /// The number of columns.
    #[inline]
    pub const fn column_count(&self) -> usize {
        self.column_count
    }

    /// The vertical alignment used by cells with no explicit override.
    #[inline]
    pub const fn default_vertical_alignment(&self) -> TableCellVerticalAlignment {
        self.default_vertical_alignment
    }

    /// The table border, if any.
    #[inline]
    pub fn border(&self) -> Option<&TableBorder> {
        self.border.as_ref()
    }

    /// The first row's baseline distance from the last layout, if any.
    #[inline]
    pub fn baseline_distance(&self) -> Option<Pixels> {
        self.baseline_distance
    }

    /// Updates the column count; returns `true` if the value changed.
    pub fn set_column_count(&mut self, column_count: usize) -> bool {
        if self.column_count == column_count {
            return false;
        }
        self.column_count = column_count;
        true
    }

    /// Updates the per-column width overrides; returns `true` if changed.
    pub fn set_column_widths(&mut self, column_widths: HashMap<usize, TableColumnWidth>) -> bool {
        if self.column_widths == column_widths {
            return false;
        }
        self.column_widths = column_widths;
        true
    }

    /// Updates the default column width; returns `true` if changed.
    pub fn set_default_column_width(&mut self, width: TableColumnWidth) -> bool {
        if self.default_column_width == width {
            return false;
        }
        self.default_column_width = width;
        true
    }

    /// Updates the default vertical alignment; returns `true` if changed.
    pub fn set_default_vertical_alignment(
        &mut self,
        alignment: TableCellVerticalAlignment,
    ) -> bool {
        if self.default_vertical_alignment == alignment {
            return false;
        }
        self.default_vertical_alignment = alignment;
        true
    }

    /// Updates the text baseline; returns `true` if changed.
    pub fn set_text_baseline(&mut self, baseline: Option<TextBaseline>) -> bool {
        if self.text_baseline == baseline {
            return false;
        }
        self.text_baseline = baseline;
        true
    }

    /// Updates the table border; returns `true` if changed.
    pub fn set_border(&mut self, border: Option<TableBorder>) -> bool {
        if self.border == border {
            return false;
        }
        self.border = border;
        true
    }

    /// Updates the per-row background decorations; returns `true` if changed.
    pub fn set_row_decorations(
        &mut self,
        row_decorations: Vec<Option<BoxDecoration<Pixels>>>,
    ) -> bool {
        if self.row_decorations == row_decorations {
            return false;
        }
        self.row_decorations = row_decorations;
        true
    }

    /// The `TableColumnWidth` in effect for column `x` (an explicit override,
    /// or [`Self::default_column_width`]).
    fn column_width_for(&self, x: usize) -> TableColumnWidth {
        self.column_widths
            .get(&x)
            .cloned()
            .unwrap_or_else(|| self.default_column_width.clone())
    }

    /// Resolves ONE column's ideal-or-min width (per `query_kind`) and — only
    /// meaningful when it came from the ideal/max query — its flex factor.
    ///
    /// `Fixed`/`Flex`/`Fraction` never touch a cell (their formula is a pure
    /// function of `container_width`); only `Intrinsic` probes cells, via
    /// `query(index, extent, query_kind)`, taking the max across the column
    /// exactly like the oracle's `IntrinsicColumnWidth.minIntrinsicWidth`/
    /// `maxIntrinsicWidth` (`table.dart:106-121`).
    fn column_extent(
        &self,
        x: usize,
        row_count: usize,
        container_width: Pixels,
        query_kind: WidthQuery,
        query: &mut impl FnMut(usize, f32, WidthQuery) -> f32,
    ) -> (Pixels, Option<f32>) {
        let spec = self.column_width_for(x);
        self.extent_for_spec(&spec, x, row_count, container_width, query_kind, query)
    }

    /// Resolves one column-width `spec` (which may be a nested
    /// [`Max`](TableColumnWidth::Max)/[`Min`](TableColumnWidth::Min)
    /// combinator) to a width for `query_kind` and its flex factor.
    ///
    /// The combinators evaluate BOTH operands against the same cells and
    /// `container_width` and fold the results — width by `max`/`min`, flex by
    /// [`combine_flex`] — exactly like the oracle's `MaxColumnWidth`/
    /// `MinColumnWidth` (`table.dart:235-340`). Recursion depth equals the
    /// nesting depth of the spec (typically 1); each leaf is O(rows) only for
    /// `Intrinsic`, O(1) otherwise.
    fn extent_for_spec(
        &self,
        spec: &TableColumnWidth,
        x: usize,
        row_count: usize,
        container_width: Pixels,
        query_kind: WidthQuery,
        query: &mut impl FnMut(usize, f32, WidthQuery) -> f32,
    ) -> (Pixels, Option<f32>) {
        match spec {
            TableColumnWidth::Fixed(value) => (Pixels::new(*value), None),
            TableColumnWidth::Flex(flex) => (Pixels::ZERO, Some(*flex)),
            TableColumnWidth::Fraction(fraction) => {
                // Divergence from the oracle: see the module doc's
                // "Fraction clamps" note.
                let fraction = fraction.clamp(0.0, 1.0);
                let width = if container_width.is_finite() {
                    Pixels::new(fraction * container_width.get())
                } else {
                    Pixels::ZERO
                };
                (width, None)
            }
            TableColumnWidth::Intrinsic { flex } => {
                let mut extent = Pixels::ZERO;
                for y in 0..row_count {
                    let idx = x + y * self.column_count;
                    extent = extent.max(Pixels::new(query(idx, f32::INFINITY, query_kind)));
                }
                // The intrinsic width is the column's floor; `flex` (if any)
                // lets it also claim leftover space in the grow pass, exactly
                // like the oracle's `IntrinsicColumnWidth.flex`.
                (extent, *flex)
            }
            TableColumnWidth::Max(a, b) => {
                let (wa, fa) =
                    self.extent_for_spec(a, x, row_count, container_width, query_kind, query);
                let (wb, fb) =
                    self.extent_for_spec(b, x, row_count, container_width, query_kind, query);
                (wa.max(wb), combine_flex(fa, fb, f32::max))
            }
            TableColumnWidth::Min(a, b) => {
                let (wa, fa) =
                    self.extent_for_spec(a, x, row_count, container_width, query_kind, query);
                let (wb, fb) =
                    self.extent_for_spec(b, x, row_count, container_width, query_kind, query);
                (wa.min(wb), combine_flex(fa, fb, f32::min))
            }
        }
    }

    /// The 4-pass column-width algorithm (`table.dart:1070-1236`), generic
    /// over a single intrinsic-width-query closure so `perform_layout`,
    /// `compute_dry_layout`, and the nested call inside
    /// `compute_min_intrinsic_height` all share ONE implementation (mirrors
    /// `RenderStack::compute_size`'s `measure`-closure pattern, `stack.rs`).
    ///
    /// A single combined closure (rather than two separate
    /// `FnMut`-per-query-kind closures) is deliberate: two closures that each
    /// independently capture the same `&mut ctx` cannot both be alive as
    /// function arguments at once (a real borrow-checker constraint, not a
    /// style choice) — one closure discriminated by [`WidthQuery`] sidesteps
    /// it entirely.
    ///
    /// Pass 1 (`L1082-1120`): ideal widths + min widths + flex.
    /// Pass 2 (`L1124-1153`): grow flexed columns toward the target width, or
    /// grow all columns equally toward `min_width_constraint` if none are
    /// flexed (mutually exclusive branches, oracle's own comment).
    /// Pass 3 (`L1168-1234`): two-round shrink when the table exceeds
    /// `max_width_constraint` — proportional shrink of flexed columns toward
    /// their floors (re-accumulating `total_flex` as columns hit floor), then
    /// equal-delta shrink of the remaining non-floored columns.
    fn compute_column_widths(
        &self,
        row_count: usize,
        min_width_constraint: Pixels,
        max_width_constraint: Pixels,
        mut query: impl FnMut(usize, f32, WidthQuery) -> f32,
    ) -> Vec<Pixels> {
        let column_count = self.column_count;
        if column_count == 0 {
            return Vec::new();
        }

        // ---- Pass 1 (`L1082-1120`): ideal widths, min widths, flex ---------
        let mut widths = vec![0.0f32; column_count];
        let mut min_widths = vec![0.0f32; column_count];
        let mut flexes: Vec<Option<f32>> = vec![None; column_count];

        for x in 0..column_count {
            let (ideal, flex) = self.column_extent(
                x,
                row_count,
                max_width_constraint,
                WidthQuery::Max,
                &mut query,
            );
            let (min_w, _) = self.column_extent(
                x,
                row_count,
                max_width_constraint,
                WidthQuery::Min,
                &mut query,
            );
            widths[x] = ideal.get();
            min_widths[x] = min_w.get();
            flexes[x] = flex;
        }

        Self::grow_and_shrink_column_widths(
            &mut widths,
            &min_widths,
            &mut flexes,
            min_width_constraint.get(),
            max_width_constraint.get(),
        );

        widths.into_iter().map(Pixels::new).collect()
    }

    /// Passes 2 and 3 of the column-width algorithm (`table.dart:1124-1234`),
    /// factored out of [`Self::compute_column_widths`]'s cell-touching Pass 1
    /// so the grow/shrink arithmetic can be exercised directly against
    /// hand-picked `widths`/`min_widths`/`flexes` — including the oracle's
    /// own adversarial doc-comment scenario (`table.dart:1170-1179`): a
    /// low-ideal/high-flex column paired with a high-ideal/low-flex column
    /// under a tiny `max_width_constraint` must shrink toward each column's
    /// floor without ever going negative.
    ///
    /// Pass 2: grows flexed columns toward the target width (`max_width_constraint`
    /// if finite, else `min_width_constraint`), or — absent any flex — grows
    /// all columns equally toward `min_width_constraint` (mutually exclusive
    /// branches, oracle's own comment).
    /// Pass 3: if the table exceeds `max_width_constraint`, shrinks in two
    /// rounds — proportional shrink of flexed columns toward their floors
    /// (re-accumulating `total_flex` as columns hit floor), then equal-delta
    /// shrink of the remaining non-floored columns.
    fn grow_and_shrink_column_widths(
        widths: &mut [f32],
        min_widths: &[f32],
        flexes: &mut [Option<f32>],
        min_width_constraint: f32,
        max_width_constraint: f32,
    ) {
        let column_count = widths.len();
        debug_assert_eq!(min_widths.len(), column_count);
        debug_assert_eq!(flexes.len(), column_count);
        if column_count == 0 {
            return;
        }

        let mut table_width: f32 = widths.iter().sum();
        let unflexed_table_width: f32 = widths
            .iter()
            .zip(flexes.iter())
            .filter_map(|(w, f)| f.is_none().then_some(*w))
            .sum();
        let mut total_flex: f32 = flexes.iter().flatten().sum();

        // ---- Pass 2: grow toward the target width ---------------------------
        if total_flex > 0.0 {
            // This can only grow the table, but it WILL grow the table at
            // least as big as the target width.
            let target_width = if max_width_constraint.is_finite() {
                max_width_constraint
            } else {
                min_width_constraint
            };
            if table_width < target_width {
                let remaining_width = target_width - unflexed_table_width;
                for x in 0..column_count {
                    if let Some(flex) = flexes[x] {
                        let flexed_width = remaining_width * flex / total_flex;
                        if widths[x] < flexed_width {
                            table_width += flexed_width - widths[x];
                            widths[x] = flexed_width;
                        }
                    }
                }
            }
        } else if table_width < min_width_constraint {
            // Steps 2 and 3 are mutually exclusive.
            let delta = (min_width_constraint - table_width) / column_count as f32;
            for w in widths.iter_mut() {
                *w += delta;
            }
            table_width = min_width_constraint;
        }

        // ---- Pass 3: shrink to fit the max width constraint ------------------
        if table_width > max_width_constraint {
            let mut deficit = table_width - max_width_constraint;
            let mut available_columns = column_count;

            // Round 1: proportionally shrink flexed columns toward their
            // floors, re-accumulating `total_flex` as columns hit floor.
            while deficit > EPSILON && total_flex > EPSILON {
                let mut new_total_flex = 0.0f32;
                for x in 0..column_count {
                    if let Some(flex) = flexes[x] {
                        let new_width = widths[x] - deficit * flex / total_flex;
                        if new_width <= min_widths[x] {
                            deficit -= widths[x] - min_widths[x];
                            widths[x] = min_widths[x];
                            flexes[x] = None;
                            available_columns -= 1;
                        } else {
                            deficit -= widths[x] - new_width;
                            widths[x] = new_width;
                            new_total_flex += flex;
                        }
                        debug_assert!(widths[x] >= 0.0, "column {x} width went negative");
                    }
                }
                total_flex = new_total_flex;
            }

            // Round 2: equal-delta shrink of whatever isn't at its floor yet.
            while deficit > EPSILON && available_columns > 0 {
                let delta = deficit / available_columns as f32;
                let mut new_available_columns = 0;
                for x in 0..column_count {
                    let available_delta = widths[x] - min_widths[x];
                    if available_delta > 0.0 {
                        if available_delta <= delta {
                            deficit -= widths[x] - min_widths[x];
                            widths[x] = min_widths[x];
                        } else {
                            deficit -= delta;
                            widths[x] -= delta;
                            new_available_columns += 1;
                        }
                    }
                }
                available_columns = new_available_columns;
            }
        }
    }
}

impl flui_foundation::Diagnosticable for RenderTable {
    fn debug_fill_properties(&self, builder: &mut flui_foundation::DiagnosticsBuilder) {
        builder.add_int("column_count", self.column_count as i64, None);
        builder.add_enum(
            "default_vertical_alignment",
            self.default_vertical_alignment,
        );
        builder.add_enum("default_column_width", self.default_column_width.clone());
        builder.add(
            "border",
            match &self.border {
                Some(_) => "set".to_string(),
                None => "none".to_string(),
            },
        );
    }
}

impl RenderBox for RenderTable {
    type Arity = Variable;
    type ParentData = TableCellParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut BoxLayoutContext<'_, Variable, TableCellParentData>,
    ) -> Size {
        let constraints = *ctx.constraints();
        let child_count = ctx.child_count();
        let column_count = self.column_count;

        if column_count == 0 || child_count == 0 {
            self.row_tops = vec![Pixels::ZERO];
            self.column_lefts = Vec::new();
            self.table_width = Pixels::ZERO;
            self.baseline_distance = None;
            return constraints.constrain(Size::ZERO);
        }

        let row_count = child_count / column_count;
        debug_assert_eq!(
            row_count * column_count,
            child_count,
            "RenderTable requires child_count ({child_count}) to be an exact \
             multiple of column_count ({column_count}) — every row must \
             contribute exactly column_count cells"
        );

        let widths = self.compute_column_widths(
            row_count,
            constraints.min_width,
            constraints.max_width,
            |i, h, kind| match kind {
                WidthQuery::Min => ctx.child_min_intrinsic_width(i, h),
                WidthQuery::Max => ctx.child_max_intrinsic_width(i, h),
            },
        );

        // Column positions (LTR only — see module doc).
        let mut column_lefts = vec![Pixels::ZERO; column_count];
        for x in 1..column_count {
            column_lefts[x] = column_lefts[x - 1] + widths[x - 1];
        }
        let table_width = column_lefts[column_count - 1] + widths[column_count - 1];

        let mut row_tops = Vec::with_capacity(row_count + 1);
        self.baseline_distance = None;
        let mut row_top = Pixels::ZERO;

        for y in 0..row_count {
            row_tops.push(row_top);

            // Resolve each cell's effective alignment once, and stamp `x`/`y`
            // onto its parent data for API/diagnostics parity with the
            // oracle's public getters (`table.dart:1376-1377`) — RenderTable's
            // own layout logic never reads them back.
            let mut alignments = Vec::with_capacity(column_count);
            for x in 0..column_count {
                let idx = x + y * column_count;
                let alignment = ctx
                    .child_parent_data(idx)
                    .and_then(|pd| pd.vertical_alignment)
                    .unwrap_or(self.default_vertical_alignment);
                alignments.push(alignment);
                if let Some(pd) = ctx.child_parent_data_mut(idx) {
                    pd.x = x;
                    pd.y = y;
                }
            }

            // ---- Measure pass (table.dart:1399-1421) ------------------------
            let mut row_height = Pixels::ZERO;
            let mut have_baseline = false;
            let mut before_baseline = Pixels::ZERO;
            let mut after_baseline = Pixels::ZERO;
            let mut baselines = vec![Pixels::ZERO; column_count];
            let mut cell_sizes = vec![Size::ZERO; column_count];

            for x in 0..column_count {
                let idx = x + y * column_count;
                match alignments[x] {
                    TableCellVerticalAlignment::Baseline => {
                        let cc = BoxConstraints::tight_for(Some(widths[x]), None);
                        let size = ctx.layout_child(idx, cc);
                        cell_sizes[x] = size;
                        // A cell missing an actual baseline (or the table
                        // missing an explicit `text_baseline`) degrades to a
                        // top-anchored contribution — the oracle's own
                        // `childBaseline == null` branch, generalized to also
                        // cover an unset `text_baseline` instead of asserting
                        // (library code must not panic on a config gap).
                        let baseline = self
                            .text_baseline
                            .and_then(|kind| ctx.child_distance_to_actual_baseline(idx, kind));
                        match baseline {
                            Some(distance) => {
                                let distance = Pixels::new(distance);
                                before_baseline = before_baseline.max(distance);
                                after_baseline = after_baseline.max(size.height - distance);
                                baselines[x] = distance;
                                have_baseline = true;
                            }
                            None => {
                                row_height = row_height.max(size.height);
                            }
                        }
                    }
                    TableCellVerticalAlignment::Top
                    | TableCellVerticalAlignment::Middle
                    | TableCellVerticalAlignment::Bottom => {
                        let cc = BoxConstraints::tight_for(Some(widths[x]), None);
                        let size = ctx.layout_child(idx, cc);
                        cell_sizes[x] = size;
                        row_height = row_height.max(size.height);
                    }
                    TableCellVerticalAlignment::Fill => {
                        // Not measured here — laid out in the position pass
                        // once `row_height` is final.
                    }
                }
            }

            if have_baseline {
                if y == 0 {
                    self.baseline_distance = Some(before_baseline);
                }
                row_height = row_height.max(before_baseline + after_baseline);
            }

            // ---- Position pass (table.dart:1418-1444) ------------------------
            for x in 0..column_count {
                let idx = x + y * column_count;
                let offset = match alignments[x] {
                    TableCellVerticalAlignment::Baseline => {
                        Offset::new(column_lefts[x], row_top + before_baseline - baselines[x])
                    }
                    TableCellVerticalAlignment::Top => Offset::new(column_lefts[x], row_top),
                    TableCellVerticalAlignment::Middle => Offset::new(
                        column_lefts[x],
                        row_top + (row_height - cell_sizes[x].height) / 2.0,
                    ),
                    TableCellVerticalAlignment::Bottom => {
                        Offset::new(column_lefts[x], row_top + row_height - cell_sizes[x].height)
                    }
                    TableCellVerticalAlignment::Fill => {
                        // Second, tight-height layout call (oracle's own
                        // second pass for `fill`).
                        let cc = BoxConstraints::tight_for(Some(widths[x]), Some(row_height));
                        ctx.layout_child(idx, cc);
                        Offset::new(column_lefts[x], row_top)
                    }
                };
                ctx.position_child(idx, offset);
            }

            row_top += row_height;
        }
        row_tops.push(row_top);

        self.row_tops = row_tops;
        self.column_lefts = column_lefts;
        self.table_width = table_width;

        constraints.constrain(Size::new(table_width, row_top))
    }

    fn compute_dry_layout(
        &self,
        constraints: BoxConstraints,
        ctx: &mut BoxDryLayoutCtx<'_>,
    ) -> Size {
        let column_count = self.column_count;
        let child_count = ctx.child_count();
        if column_count == 0 || child_count == 0 {
            return constraints.constrain(Size::ZERO);
        }
        let row_count = child_count / column_count;

        let widths = self.compute_column_widths(
            row_count,
            constraints.min_width,
            constraints.max_width,
            |i, h, kind| match kind {
                WidthQuery::Min => ctx.child_min_intrinsic_width(i, h),
                WidthQuery::Max => ctx.child_max_intrinsic_width(i, h),
            },
        );
        let table_width = widths.iter().copied().fold(Pixels::ZERO, |a, b| a + b);

        let mut row_top = Pixels::ZERO;
        for y in 0..row_count {
            let mut row_height = Pixels::ZERO;
            for (x, &width) in widths.iter().enumerate() {
                let idx = x + y * column_count;
                let alignment = ctx
                    .child_parent_data_as::<TableCellParentData>(idx)
                    .and_then(|pd| pd.vertical_alignment)
                    .unwrap_or(self.default_vertical_alignment);
                match alignment {
                    TableCellVerticalAlignment::Baseline => {
                        // Oracle asserts this combination unsupported for dry
                        // layout (`table.dart:1305-1312`) — baseline metrics
                        // require a real layout pass.
                        return Size::ZERO;
                    }
                    TableCellVerticalAlignment::Top
                    | TableCellVerticalAlignment::Middle
                    | TableCellVerticalAlignment::Bottom => {
                        let cc = BoxConstraints::tight_for(Some(width), None);
                        let size = ctx.child_dry_layout(idx, cc);
                        row_height = row_height.max(size.height);
                    }
                    TableCellVerticalAlignment::Fill => {}
                }
            }
            row_top += row_height;
        }

        constraints.constrain(Size::new(table_width, row_top))
    }

    fn compute_min_intrinsic_width(&self, _height: f32, ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        let column_count = self.column_count;
        let child_count = ctx.child_count();
        if column_count == 0 || child_count == 0 {
            return 0.0;
        }
        let row_count = child_count / column_count;
        let mut query = |i: usize, h: f32, kind: WidthQuery| match kind {
            WidthQuery::Min => ctx.child_min_intrinsic_width(i, h),
            WidthQuery::Max => ctx.child_max_intrinsic_width(i, h),
        };
        let mut total = 0.0f32;
        for x in 0..column_count {
            let (min_w, _) =
                self.column_extent(x, row_count, Pixels::INFINITY, WidthQuery::Min, &mut query);
            total += min_w.get();
        }
        total
    }

    fn compute_max_intrinsic_width(&self, _height: f32, ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        let column_count = self.column_count;
        let child_count = ctx.child_count();
        if column_count == 0 || child_count == 0 {
            return 0.0;
        }
        let row_count = child_count / column_count;
        let mut query = |i: usize, h: f32, kind: WidthQuery| match kind {
            WidthQuery::Min => ctx.child_min_intrinsic_width(i, h),
            WidthQuery::Max => ctx.child_max_intrinsic_width(i, h),
        };
        let mut total = 0.0f32;
        for x in 0..column_count {
            let (max_w, _) =
                self.column_extent(x, row_count, Pixels::INFINITY, WidthQuery::Max, &mut query);
            total += max_w.get();
        }
        total
    }

    fn compute_min_intrinsic_height(&self, width: f32, ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        let column_count = self.column_count;
        let child_count = ctx.child_count();
        if column_count == 0 || child_count == 0 {
            return 0.0;
        }
        let row_count = child_count / column_count;

        // Mirrors `BoxConstraints.tightForFinite(width: width)` — only the
        // width bound feeds `compute_column_widths` (it never reads height).
        let requested_width = Pixels::new(width);
        let (min_width, max_width) = if requested_width.is_finite() {
            (requested_width, requested_width)
        } else {
            (Pixels::ZERO, Pixels::INFINITY)
        };

        let widths =
            self.compute_column_widths(row_count, min_width, max_width, |i, h, kind| match kind {
                WidthQuery::Min => ctx.child_min_intrinsic_width(i, h),
                WidthQuery::Max => ctx.child_max_intrinsic_width(i, h),
            });

        // Winner of the 2016 world's most expensive intrinsic dimension
        // function award (the oracle's own doc comment, `table.dart:998`) —
        // note MAX even inside the MIN function, preserved exactly.
        let mut total = 0.0f32;
        for y in 0..row_count {
            let mut row_height = 0.0f32;
            for (x, &width) in widths.iter().enumerate() {
                let idx = x + y * column_count;
                row_height = row_height.max(ctx.child_max_intrinsic_height(idx, width.get()));
            }
            total += row_height;
        }
        total
    }

    fn compute_max_intrinsic_height(&self, width: f32, ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        // Oracle's own quirk (`table.dart:1023-1026`): `computeMaxIntrinsicHeight`
        // literally returns `getMinIntrinsicHeight(width)` — verified against
        // the oracle, not a transcription typo.
        self.compute_min_intrinsic_height(width, ctx)
    }

    fn compute_distance_to_actual_baseline(&self, _baseline: TextBaseline) -> Option<f32> {
        self.baseline_distance.map(Pixels::get)
    }

    /// Dry equivalent of [`Self::compute_distance_to_actual_baseline`]: the
    /// first row's baseline, computed without committing layout.
    ///
    /// Mirrors the live measure pass (`perform_layout`, the `y == 0` branch
    /// that stores `baseline_distance`): resolve dry column widths, then take
    /// the max child dry-baseline among row 0's `Baseline`-aligned cells. Like
    /// [`Self::compute_distance_to_actual_baseline`], the table's baseline is
    /// driven by its own [`text_baseline`](Self::with_text_baseline) (not the
    /// requested `_baseline`); without one, every baseline cell degrades to a
    /// height contribution and the table reports no baseline. Returns the same
    /// value the committed layout stores, so dry and live agree.
    fn compute_dry_baseline(
        &self,
        constraints: BoxConstraints,
        _baseline: TextBaseline,
        ctx: &mut BoxDryBaselineCtx<'_>,
    ) -> Option<f32> {
        let column_count = self.column_count;
        let child_count = ctx.child_count();
        if column_count == 0 || child_count == 0 {
            return None;
        }
        let row_count = child_count / column_count;
        if row_count == 0 {
            return None;
        }
        // No table text baseline → every `Baseline` cell degrades (the live
        // `childBaseline == null` branch), so the table reports no baseline.
        let text_baseline = self.text_baseline?;

        // Same dry column-width resolution the live path uses — the dry ctx
        // exposes the identical intrinsic-width probes.
        let widths = self.compute_column_widths(
            row_count,
            constraints.min_width,
            constraints.max_width,
            |i, h, kind| match kind {
                WidthQuery::Min => ctx.child_min_intrinsic_width(i, h),
                WidthQuery::Max => ctx.child_max_intrinsic_width(i, h),
            },
        );

        // First row (indices `0..column_count`): the table baseline is the max
        // dry-baseline over its `Baseline`-aligned cells (the live path's
        // `before_baseline`), or `None` if none report a baseline.
        // Row 0's cells are the flat children `0..column_count`, so the
        // column index doubles as the row-0 child index.
        let mut before_baseline: Option<f32> = None;
        for (cell, &width) in widths.iter().enumerate() {
            let alignment = ctx
                .child_parent_data_as::<TableCellParentData>(cell)
                .and_then(|pd| pd.vertical_alignment)
                .unwrap_or(self.default_vertical_alignment);
            if alignment == TableCellVerticalAlignment::Baseline {
                let cell_constraints = BoxConstraints::tight_for(Some(width), None);
                if let Some(distance) =
                    ctx.child_dry_baseline(cell, cell_constraints, text_baseline)
                {
                    before_baseline = Some(before_baseline.map_or(distance, |b| b.max(distance)));
                }
            }
        }
        before_baseline
    }

    fn paint(&self, ctx: &mut flui_rendering::context::PaintCx<'_, Variable>) {
        let row_count = self.row_tops.len().saturating_sub(1);

        // 1. Row decorations (table.dart:1478-1494).
        for y in 0..row_count {
            if let Some(Some(decoration)) = self.row_decorations.get(y) {
                let rect = Rect::from_ltrb(
                    Pixels::ZERO,
                    self.row_tops[y],
                    self.table_width,
                    self.row_tops[y + 1],
                );
                paint_box_decoration(ctx.canvas(), rect, decoration);
            }
        }

        // 2. Children, row-major order == paint order.
        ctx.paint_children();

        // 3. Table border, on top of everything (table.dart:1508-1525).
        if let Some(border) = &self.border {
            let table_height = self.row_tops.last().copied().unwrap_or(Pixels::ZERO);
            let rect = Rect::from_ltrb(Pixels::ZERO, Pixels::ZERO, self.table_width, table_height);
            let interior_rows: &[Pixels] = if self.row_tops.len() > 2 {
                &self.row_tops[1..self.row_tops.len() - 1]
            } else {
                &[]
            };
            let interior_columns: &[Pixels] = if self.column_lefts.len() > 1 {
                &self.column_lefts[1..]
            } else {
                &[]
            };
            paint_table_border(ctx.canvas(), rect, interior_rows, interior_columns, border);
        }
    }

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Variable, TableCellParentData>) -> bool {
        if !ctx.is_within_own_size() {
            return false;
        }
        let row_count = self.row_tops.len().saturating_sub(1);
        let child_count = row_count * self.column_count;
        for i in (0..child_count).rev() {
            if ctx.hit_test_child_at_layout_offset(i) {
                return true;
            }
        }
        false
    }
}

// =============================================================================
// Tests — compute_column_widths, pass by pass
// =============================================================================

#[cfg(test)]
mod tests {
    use flui_types::geometry::px;

    use super::*;

    /// A table whose `column_widths` map assigns one `TableColumnWidth` per
    /// column index `0..widths.len()`.
    fn table_with(widths: &[TableColumnWidth]) -> RenderTable {
        let column_widths = widths
            .iter()
            .enumerate()
            .map(|(x, w)| (x, w.clone()))
            .collect::<HashMap<_, _>>();
        RenderTable::new(widths.len()).with_column_widths(column_widths)
    }

    /// A query closure that panics if called — proves `Fixed`/`Flex`/
    /// `Fraction` columns never touch a cell (only `Intrinsic` may).
    fn deny_query() -> impl FnMut(usize, f32, WidthQuery) -> f32 {
        |index, extent, kind| {
            panic!(
                "non-Intrinsic column queried child {index} ({kind:?} @ {extent}) — \
                 Fixed/Flex/Fraction must never touch a cell"
            )
        }
    }

    // ---- Pass 1: Fixed-only ------------------------------------------------

    #[test]
    fn fixed_only_columns_use_their_exact_value_untouched_by_generous_constraints() {
        let table = table_with(&[
            TableColumnWidth::Fixed(50.0),
            TableColumnWidth::Fixed(100.0),
            TableColumnWidth::Fixed(30.0),
        ]);
        let widths = table.compute_column_widths(1, Pixels::ZERO, Pixels::INFINITY, deny_query());
        assert_eq!(widths, vec![px(50.0), px(100.0), px(30.0)]);
    }

    #[test]
    fn fixed_only_columns_grow_equally_when_min_width_constraint_forces_it() {
        // No flex present -> pass 2's "else" branch: grow every column
        // equally toward `min_width_constraint` (table.dart:1155-1160).
        let table = table_with(&[TableColumnWidth::Fixed(10.0), TableColumnWidth::Fixed(20.0)]);
        let widths = table.compute_column_widths(1, px(60.0), Pixels::INFINITY, deny_query());
        assert_eq!(widths, vec![px(25.0), px(35.0)]);
    }

    // ---- Pass 1/2: Flex-only ------------------------------------------------

    #[test]
    fn flex_only_columns_share_the_target_width_proportionally() {
        let table = table_with(&[TableColumnWidth::Flex(1.0), TableColumnWidth::Flex(2.0)]);
        let widths = table.compute_column_widths(1, Pixels::ZERO, px(300.0), deny_query());
        assert_eq!(widths, vec![px(100.0), px(200.0)]);
    }

    // ---- Pass 1: Fraction, finite vs. infinite container -------------------

    #[test]
    fn fraction_column_resolves_against_a_finite_container_width() {
        let table = table_with(&[TableColumnWidth::Fraction(0.25)]);
        let widths = table.compute_column_widths(1, Pixels::ZERO, px(400.0), deny_query());
        assert_eq!(widths, vec![px(100.0)]);
    }

    #[test]
    fn fraction_column_is_zero_against_an_infinite_container_width() {
        let table = table_with(&[TableColumnWidth::Fraction(0.25)]);
        let widths = table.compute_column_widths(1, Pixels::ZERO, Pixels::INFINITY, deny_query());
        assert_eq!(widths, vec![px(0.0)]);
    }

    #[test]
    fn fraction_value_above_one_is_clamped_a_documented_divergence_from_the_oracle() {
        // The oracle's `FractionColumnWidth` does NOT clamp (see the module
        // doc's "Fraction clamps" note) — FLUI's `TableColumnWidth::Fraction`
        // doc contract promises a 0.0..=1.0 clamp, so 1.5 must behave as 1.0,
        // not produce a 150px column from a 100px container.
        let table = table_with(&[TableColumnWidth::Fraction(1.5)]);
        let widths = table.compute_column_widths(1, Pixels::ZERO, px(100.0), deny_query());
        assert_eq!(widths, vec![px(100.0)]);
    }

    // ---- Max/Min combinators (oracle table.dart:235-340) -------------------

    #[test]
    fn max_combinator_takes_the_larger_of_its_two_specs() {
        // Fixed(100) vs Fraction(0.1): at container 400 the fraction is 40 < 100
        // (Fixed wins); at container 2000 the fraction is 200 > 100 (it wins).
        let table = table_with(&[TableColumnWidth::max(
            TableColumnWidth::Fixed(100.0),
            TableColumnWidth::Fraction(0.1),
        )]);
        let narrow = table.compute_column_widths(1, Pixels::ZERO, px(400.0), deny_query());
        assert_eq!(
            narrow,
            vec![px(100.0)],
            "fixed floor wins when the fraction is smaller"
        );
        let wide = table.compute_column_widths(1, Pixels::ZERO, px(2000.0), deny_query());
        assert_eq!(
            wide,
            vec![px(200.0)],
            "fraction wins when it exceeds the fixed floor"
        );
    }

    #[test]
    fn min_combinator_takes_the_smaller_of_its_two_specs() {
        // Fixed(100) vs Fraction(0.1): at container 400 the fraction is 40 (it
        // wins); at container 2000 the fraction is 200 > 100 (the fixed ceiling
        // wins).
        let table = table_with(&[TableColumnWidth::min(
            TableColumnWidth::Fixed(100.0),
            TableColumnWidth::Fraction(0.1),
        )]);
        let narrow = table.compute_column_widths(1, Pixels::ZERO, px(400.0), deny_query());
        assert_eq!(
            narrow,
            vec![px(40.0)],
            "fraction wins when below the fixed ceiling"
        );
        let wide = table.compute_column_widths(1, Pixels::ZERO, px(2000.0), deny_query());
        assert_eq!(wide, vec![px(100.0)], "fixed ceiling caps the column");
    }

    #[test]
    fn combinators_nest_recursively() {
        // Max(Min(Fixed(100), Fraction(0.5)), Fixed(30)) @ container 100:
        // inner Min(100, 50) = 50; outer Max(50, 30) = 50.
        let table = table_with(&[TableColumnWidth::max(
            TableColumnWidth::min(
                TableColumnWidth::Fixed(100.0),
                TableColumnWidth::Fraction(0.5),
            ),
            TableColumnWidth::Fixed(30.0),
        )]);
        let widths = table.compute_column_widths(1, Pixels::ZERO, px(100.0), deny_query());
        assert_eq!(widths, vec![px(50.0)]);
    }

    #[test]
    fn max_combinator_flex_is_the_larger_flex_and_drives_distribution() {
        // Max(Flex(3), Flex(1)) -> width 0, flex max(3,1)=3. Beside a Flex(1),
        // total flex 4 splits 400 as 300 / 100.
        let table = table_with(&[
            TableColumnWidth::max(TableColumnWidth::Flex(3.0), TableColumnWidth::Flex(1.0)),
            TableColumnWidth::Flex(1.0),
        ]);
        let widths = table.compute_column_widths(1, Pixels::ZERO, px(400.0), deny_query());
        assert_eq!(widths, vec![px(300.0), px(100.0)]);
    }

    #[test]
    fn min_combinator_flex_is_the_smaller_flex() {
        // Min(Flex(3), Flex(1)) -> flex min(3,1)=1. Beside a Flex(1), even split.
        let table = table_with(&[
            TableColumnWidth::min(TableColumnWidth::Flex(3.0), TableColumnWidth::Flex(1.0)),
            TableColumnWidth::Flex(1.0),
        ]);
        let widths = table.compute_column_widths(1, Pixels::ZERO, px(400.0), deny_query());
        assert_eq!(widths, vec![px(200.0), px(200.0)]);
    }

    #[test]
    fn combine_flex_passes_through_the_set_operand_and_folds_two() {
        assert_eq!(combine_flex(Some(1.0), None, f32::max), Some(1.0));
        assert_eq!(combine_flex(None, Some(2.0), f32::max), Some(2.0));
        assert_eq!(combine_flex(Some(1.0), Some(2.0), f32::max), Some(2.0));
        assert_eq!(combine_flex(Some(1.0), Some(2.0), f32::min), Some(1.0));
        assert_eq!(combine_flex(None, None, f32::max), None);
    }

    // ---- Pass 1: Intrinsic queries real cells -------------------------------

    #[test]
    fn intrinsic_column_takes_the_max_over_every_cell_in_the_column() {
        let table = table_with(&[TableColumnWidth::Intrinsic { flex: None }]);
        // Column 0, 2 rows -> cells at flat index 0 and 1. Cell 0 reports
        // (min=10, max=30); cell 1 reports (min=25, max=15) — deliberately
        // anti-correlated so "max across cells, per query kind independently"
        // is the only way to get column min=25 (from cell 1) and column
        // ideal=30 (from cell 0).
        let (ideal, flex) = table.column_extent(
            0,
            2,
            Pixels::INFINITY,
            WidthQuery::Max,
            &mut |index, _extent, kind| match (index, kind) {
                (0, WidthQuery::Min) => 10.0,
                (0, WidthQuery::Max) => 30.0,
                (1, WidthQuery::Min) => 25.0,
                (1, WidthQuery::Max) => 15.0,
                _ => unreachable!("only 2 cells in this test"),
            },
        );
        assert_eq!(ideal, px(30.0));
        assert_eq!(flex, None);

        let widths = table.compute_column_widths(
            2,
            Pixels::ZERO,
            Pixels::INFINITY,
            |index, _extent, kind| match (index, kind) {
                (0, WidthQuery::Min) => 10.0,
                (0, WidthQuery::Max) => 30.0,
                (1, WidthQuery::Min) => 25.0,
                (1, WidthQuery::Max) => 15.0,
                _ => unreachable!("only 2 cells in this test"),
            },
        );
        // Ideal (max-query) wins the table's resolved width: 30, not 25.
        assert_eq!(widths, vec![px(30.0)]);
    }

    #[test]
    fn intrinsic_column_with_flex_grows_into_leftover_space() {
        // Column 0 is Intrinsic { flex: 1 } reporting a 30px content width;
        // column 1 is Fixed(50). Target 200 leaves 150 after the fixed column,
        // and the single flexed column claims all of it — its 30px intrinsic
        // width is only a floor (oracle `IntrinsicColumnWidth.flex`).
        let table = table_with(&[
            TableColumnWidth::Intrinsic { flex: Some(1.0) },
            TableColumnWidth::Fixed(50.0),
        ]);
        let widths =
            table.compute_column_widths(1, Pixels::ZERO, px(200.0), |index, _extent, _kind| {
                if index == 0 { 30.0 } else { 0.0 }
            });
        assert_eq!(widths, vec![px(150.0), px(50.0)]);
    }

    #[test]
    fn intrinsic_column_without_flex_keeps_its_content_width() {
        // Same layout but no flex — the intrinsic column stays at its 30px
        // content width and the table is left smaller than the container.
        let table = table_with(&[
            TableColumnWidth::Intrinsic { flex: None },
            TableColumnWidth::Fixed(50.0),
        ]);
        let widths =
            table.compute_column_widths(1, Pixels::ZERO, px(200.0), |index, _extent, _kind| {
                if index == 0 { 30.0 } else { 0.0 }
            });
        assert_eq!(widths, vec![px(30.0), px(50.0)]);
    }

    // ---- Pass 3: the oracle's own adversarial shrink scenario ---------------

    #[test]
    fn adversarial_shrink_converges_to_the_max_width_without_going_negative() {
        // Oracle's own doc comment (table.dart:1170-1179): "a 1px wide column
        // of flex 1000.0 and a 1000px wide column of flex 1.0 ... If the
        // maximum table width is 2px, then just applying the flexes to the
        // deficit would result in a table with one column at -998px and one
        // column at 990px, which is wildly unhelpful." The two-round shrink
        // must instead floor the low-ideal/high-flex column at 0 and push
        // nearly the whole deficit onto the high-ideal/low-flex column.
        let mut widths = [1.0f32, 1000.0f32];
        let min_widths = [0.0f32, 0.0f32];
        let mut flexes = [Some(1000.0f32), Some(1.0f32)];

        RenderTable::grow_and_shrink_column_widths(&mut widths, &min_widths, &mut flexes, 0.0, 2.0);

        for &w in &widths {
            assert!(w >= 0.0, "no column may go negative, got {widths:?}");
            assert!(w.is_finite(), "no column may go non-finite, got {widths:?}");
        }
        let total: f32 = widths.iter().sum();
        assert!(
            (total - 2.0).abs() < 1e-3,
            "shrunk columns must sum to the 2px max width, got {total} from {widths:?}"
        );
        assert!(
            (widths[0] - 0.0).abs() < 1e-3,
            "the high-flex/low-ideal column floors at 0, got {widths:?}"
        );
        assert!(
            (widths[1] - 2.0).abs() < 1e-3,
            "the low-flex/high-ideal column absorbs the deficit, got {widths:?}"
        );
    }

    #[test]
    fn shrink_excludes_a_column_from_round_one_once_it_hits_its_own_floor() {
        // Column 0's floor (min_width = 40) is reached partway through round
        // 1's proportional shrink; it drops out of the flex pool (`flexes[0]
        // = None`) and column 1 (still flexed) absorbs the rest of the
        // deficit within the SAME round-1 loop — this converges before round
        // 2 is ever needed.
        let mut widths = [50.0f32, 50.0f32];
        let min_widths = [40.0f32, 0.0f32];
        let mut flexes = [Some(1.0f32), Some(1.0f32)];

        RenderTable::grow_and_shrink_column_widths(
            &mut widths,
            &min_widths,
            &mut flexes,
            0.0,
            60.0,
        );

        assert!(
            (widths[0] - 40.0).abs() < 1e-3,
            "column 0 must floor at its min_width (40), got {widths:?}"
        );
        let total: f32 = widths.iter().sum();
        assert!(
            (total - 60.0).abs() < 1e-3,
            "shrunk columns must sum to the 60px max width, got {total} from {widths:?}"
        );
    }

    #[test]
    fn shrink_falls_back_to_round_two_when_no_column_is_flexed() {
        // With no flex at all, round 1's `while` guard (`total_flex >
        // EPSILON`) is false from the start, so the ENTIRE deficit must be
        // absorbed by round 2's equal-delta shrink of non-floored columns.
        let mut widths = [50.0f32, 50.0f32];
        let min_widths = [10.0f32, 30.0f32];
        let mut flexes = [None, None];

        RenderTable::grow_and_shrink_column_widths(
            &mut widths,
            &min_widths,
            &mut flexes,
            0.0,
            60.0,
        );

        assert_eq!(widths, [30.0, 30.0]);
    }
}
