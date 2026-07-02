//! [`Table`], [`TableRow`], and [`TableCell`] — grid layout over `RenderTable`.

use std::collections::HashMap;

use flui_objects::RenderTable;
use flui_rendering::parent_data::TableCellParentData;
use flui_rendering::protocol::BoxProtocol;
use flui_types::Pixels;
use flui_types::layout::{TableCellVerticalAlignment, TableColumnWidth};
use flui_types::styling::{BoxDecoration, TableBorder};
use flui_types::typography::TextBaseline;
use flui_view::{
    BoxedView, IntoView, ParentDataView, RenderView, View, ViewExt, impl_parent_data_view,
    impl_render_view,
};

/// One row of a [`Table`]: an optional background decoration plus its cells.
///
/// Every row must contribute exactly as many cells as the table has columns
/// — [`Table`] derives its column count from the first row and
/// debug-asserts every other row matches it (Flutter parity: `Table`
/// requires every `TableRow.children` to have the same length).
#[derive(Clone, Debug)]
pub struct TableRow {
    decoration: Option<BoxDecoration<Pixels>>,
    cells: Vec<BoxedView>,
}

impl TableRow {
    /// A row of `cells` with no background decoration.
    pub fn new(cells: Vec<BoxedView>) -> Self {
        Self {
            decoration: None,
            cells,
        }
    }

    /// Builder: paint `decoration` behind this row's cells.
    #[must_use]
    pub fn decoration(mut self, decoration: BoxDecoration<Pixels>) -> Self {
        self.decoration = Some(decoration);
        self
    }
}

/// Lays out `rows` in a grid: `RenderTable` resolves each column's width
/// (fixed/flex/fraction/intrinsic) and sizes each row to its tallest cell.
///
/// Flutter parity: `widgets/table.dart` `Table` over `RenderTable`. Defaults
/// match Flutter: `default_column_width = Flex(1.0)`,
/// `default_vertical_alignment = Top`, no border, no explicit text baseline.
///
/// Reconciliation uses the same flat multi-child element
/// (`ElementKind::render_variable`) `Stack`/`Flow` already use — not
/// Flutter's bespoke per-row keyed `_TableElement` diffing (deliberately out
/// of scope for this slice; see the render-table research plan). Wrap a cell
/// in [`TableCell`] to override its vertical alignment.
#[derive(Clone, Debug)]
pub struct Table {
    rows: Vec<TableRow>,
    column_widths: HashMap<usize, TableColumnWidth>,
    default_column_width: TableColumnWidth,
    default_vertical_alignment: TableCellVerticalAlignment,
    text_baseline: Option<TextBaseline>,
    border: Option<TableBorder>,
}

impl Table {
    /// A table of `rows`, with Flutter's default column width, alignment, no
    /// border, and no explicit text baseline.
    pub fn new(rows: Vec<TableRow>) -> Self {
        Self {
            rows,
            column_widths: HashMap::new(),
            default_column_width: TableColumnWidth::Flex(1.0),
            default_vertical_alignment: TableCellVerticalAlignment::Top,
            text_baseline: None,
            border: None,
        }
    }

    /// Builder: set per-column width overrides.
    #[must_use]
    pub fn column_widths(mut self, column_widths: HashMap<usize, TableColumnWidth>) -> Self {
        self.column_widths = column_widths;
        self
    }

    /// Builder: set the width used by columns with no explicit override.
    #[must_use]
    pub fn default_column_width(mut self, width: TableColumnWidth) -> Self {
        self.default_column_width = width;
        self
    }

    /// Builder: set the vertical alignment used by cells with no explicit
    /// [`TableCell`] override.
    #[must_use]
    pub fn default_vertical_alignment(mut self, alignment: TableCellVerticalAlignment) -> Self {
        self.default_vertical_alignment = alignment;
        self
    }

    /// Builder: set the text baseline used by `TableCellVerticalAlignment::Baseline` cells.
    #[must_use]
    pub fn text_baseline(mut self, baseline: TextBaseline) -> Self {
        self.text_baseline = Some(baseline);
        self
    }

    /// Builder: set the table border.
    #[must_use]
    pub fn border(mut self, border: TableBorder) -> Self {
        self.border = Some(border);
        self
    }

    /// The number of columns — the first row's cell count (`0` with no rows).
    fn column_count(&self) -> usize {
        self.rows.first().map_or(0, |row| row.cells.len())
    }

    /// One [`Option<BoxDecoration>`] per row, in row order — the shape
    /// `RenderTable::row_decorations` expects.
    fn row_decorations(&self) -> Vec<Option<BoxDecoration<Pixels>>> {
        self.rows.iter().map(|row| row.decoration.clone()).collect()
    }
}

impl RenderView for Table {
    type Protocol = BoxProtocol;
    type RenderObject = RenderTable;

    fn create_render_object(&self) -> Self::RenderObject {
        debug_assert!(
            self.rows
                .iter()
                .all(|row| row.cells.len() == self.column_count()),
            "every Table row must have the same number of cells as the first row",
        );
        RenderTable::new(self.column_count())
            .with_column_widths(self.column_widths.clone())
            .with_default_column_width(self.default_column_width.clone())
            .with_default_vertical_alignment(self.default_vertical_alignment)
            .with_text_baseline(self.text_baseline)
            .with_border(self.border)
            .with_row_decorations(self.row_decorations())
    }

    fn update_render_object(&self, render_object: &mut Self::RenderObject) {
        debug_assert!(
            self.rows
                .iter()
                .all(|row| row.cells.len() == self.column_count()),
            "every Table row must have the same number of cells as the first row",
        );
        render_object.set_column_count(self.column_count());
        render_object.set_column_widths(self.column_widths.clone());
        render_object.set_default_column_width(self.default_column_width.clone());
        render_object.set_default_vertical_alignment(self.default_vertical_alignment);
        render_object.set_text_baseline(self.text_baseline);
        render_object.set_border(self.border);
        render_object.set_row_decorations(self.row_decorations());
    }

    fn has_children(&self) -> bool {
        self.rows.iter().any(|row| !row.cells.is_empty())
    }

    fn visit_child_views(&self, visitor: &mut dyn FnMut(&dyn View)) {
        // Row-major flattening — the exact order `RenderTable`'s flat
        // child list expects (`row = index / column_count`, `col = index %
        // column_count`).
        for row in &self.rows {
            for cell in &row.cells {
                visitor(cell);
            }
        }
    }
}

impl_render_view!(Table);

/// Overrides a cell's vertical alignment within its [`Table`] row.
///
/// A [`ParentDataView`] contributing a [`TableCellParentData`] to its child's
/// render node — mirrors [`Positioned`](crate::Positioned) exactly. Only
/// `vertical_alignment` is set; `x`/`y`/`offset` are inert defaults since
/// `RenderTable` overwrites them unconditionally during layout.
///
/// Flutter parity: `widgets/table.dart` `TableCell`.
#[derive(Clone, Debug)]
pub struct TableCell {
    vertical_alignment: TableCellVerticalAlignment,
    child: BoxedView,
}

impl TableCell {
    /// Wraps `child`, overriding its vertical alignment to `vertical_alignment`.
    pub fn new(vertical_alignment: TableCellVerticalAlignment, child: impl IntoView) -> Self {
        Self {
            vertical_alignment,
            child: child.into_view().boxed(),
        }
    }
}

impl ParentDataView for TableCell {
    type ParentData = TableCellParentData;

    fn child(&self) -> &dyn View {
        &self.child
    }

    fn create_parent_data(&self) -> Self::ParentData {
        TableCellParentData::zero().with_alignment(self.vertical_alignment)
    }
}

impl_parent_data_view!(TableCell);

#[cfg(test)]
mod tests {
    use flui_types::Color;
    use flui_types::typography::TextBaseline;
    use flui_view::RenderView;

    use super::*;
    use crate::SizedBox;

    fn row(cells: usize) -> TableRow {
        TableRow::new((0..cells).map(|_| SizedBox::shrink().boxed()).collect())
    }

    #[test]
    fn column_count_is_the_first_rows_cell_count() {
        assert_eq!(Table::new(vec![row(3), row(3)]).column_count(), 3);
    }

    #[test]
    fn column_count_is_zero_with_no_rows() {
        assert_eq!(Table::new(Vec::new()).column_count(), 0);
    }

    #[test]
    fn row_decorations_collects_each_rows_decoration_in_order() {
        let decorated =
            row(1).decoration(BoxDecoration::new().set_color(Some(Color::rgb(1, 2, 3))));
        let table = Table::new(vec![row(1), decorated]);

        let decorations = table.row_decorations();
        assert_eq!(decorations.len(), 2);
        assert!(decorations[0].is_none(), "first row has no decoration");
        assert_eq!(
            decorations[1].as_ref().and_then(|d| d.color),
            Some(Color::rgb(1, 2, 3)),
        );
    }

    #[test]
    fn create_render_object_installs_the_configured_border() {
        let border = TableBorder::all(flui_types::styling::BorderSide::new(
            Color::BLACK,
            flui_types::geometry::px(1.0),
            flui_types::styling::BorderStyle::Solid,
        ));
        let render_object = Table::new(vec![row(1)])
            .border(border)
            .create_render_object();
        assert_eq!(render_object.border(), Some(&border));
    }

    #[test]
    fn update_render_object_replaces_the_border() {
        let mut render_object = Table::new(vec![row(1)]).create_render_object();
        assert_eq!(render_object.border(), None);

        let border = TableBorder::all(flui_types::styling::BorderSide::new(
            Color::BLACK,
            flui_types::geometry::px(2.0),
            flui_types::styling::BorderStyle::Solid,
        ));
        Table::new(vec![row(1)])
            .border(border)
            .update_render_object(&mut render_object);
        assert_eq!(render_object.border(), Some(&border));
    }

    #[test]
    fn debug_reports_builder_overrides() {
        let table = Table::new(vec![row(1)])
            .default_column_width(TableColumnWidth::Fixed(40.0))
            .default_vertical_alignment(TableCellVerticalAlignment::Bottom)
            .text_baseline(TextBaseline::Alphabetic);
        let debug = format!("{table:?}");
        assert!(
            debug.contains("Fixed") && debug.contains("Bottom") && debug.contains("Alphabetic"),
            "Debug output must reflect the overridden builder values, got: {debug}",
        );
    }

    #[test]
    fn has_children_is_false_when_every_row_is_empty() {
        assert!(!Table::new(vec![TableRow::new(Vec::new())]).has_children());
        assert!(!Table::new(Vec::new()).has_children());
        assert!(Table::new(vec![row(1)]).has_children());
    }
}
