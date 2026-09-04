//! [`Table`], [`TableRow`], and [`TableCell`] — grid layout over `RenderTable`.

use std::collections::HashMap;

use flui_foundation::ViewKey;
use flui_objects::RenderTable;

use crate::SizedBox;
use flui_rendering::parent_data::TableCellParentData;
use flui_rendering::protocol::BoxProtocol;
use flui_types::Pixels;
use flui_types::layout::{TableCellVerticalAlignment, TableColumnWidth};
use flui_types::styling::{BoxDecoration, TableBorder};
use flui_types::typography::TextBaseline;
use flui_view::{
    BoxedView, BuildContext, IntoView, ParentDataView, RenderView, StatelessView, View, ViewExt,
    impl_parent_data_view, impl_render_view,
};

/// One row of a [`Table`]: an optional background decoration plus its cells.
///
/// Every row must contribute exactly as many cells as the table has columns
/// — [`Table`] derives its column count from the first row and
/// debug-asserts every other row matches it (Flutter parity: `Table`
/// requires every `TableRow.children` to have the same length).
#[derive(Clone)]
pub struct TableRow {
    decoration: Option<BoxDecoration<Pixels>>,
    cells: Vec<BoxedView>,
    key: Option<Box<dyn ViewKey>>,
}

impl std::fmt::Debug for TableRow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TableRow")
            .field("cells", &self.cells.len())
            .field("keyed", &self.key.is_some())
            .finish_non_exhaustive()
    }
}

impl TableRow {
    /// A row of `cells` with no background decoration.
    pub fn new(cells: Vec<BoxedView>) -> Self {
        Self {
            decoration: None,
            cells,
            key: None,
        }
    }

    /// Builder: give this row an identity that survives reordering.
    ///
    /// A keyed row keeps its cells' elements — and so their state — when the
    /// rows around it move, are inserted, or are removed. Unkeyed rows are
    /// matched to each other in order, among the unkeyed rows only, which is
    /// what lets a keyed row move past them without disturbing them
    /// (Flutter's `_TableElement.update`).
    ///
    /// # Panics
    ///
    /// Panics if `key` is a `GlobalKey`: a row is not an element, so there is
    /// nothing for a global key to address. Key the cell instead.
    #[must_use]
    pub fn key(mut self, key: impl ViewKey + 'static) -> Self {
        assert!(
            !key.is_global_key(),
            "TableRow::key takes a local key: a row is not an element, so a \
             GlobalKey has nothing to address here — key the cell instead",
        );
        self.key = Some(Box::new(key));
        self
    }

    /// Builder: paint `decoration` behind this row's cells.
    #[must_use]
    pub fn decoration(mut self, decoration: BoxDecoration<Pixels>) -> Self {
        self.decoration = Some(decoration);
        self
    }
}

/// One cell, carrying the identity of the row it belongs to.
///
/// `RenderTable`'s children are one flat row-major list, so the element tree
/// reconciles them as one flat list too. Giving each cell a key derived from
/// `(row identity, column index)` makes that flat reconcile behave exactly as
/// Flutter's row-scoped `_TableElement.update` does: a cell is matched to the
/// cell that held the same position in the same row, wherever that row has
/// moved to, and a row with no counterpart is disposed whole.
///
/// The wrapper owns no render object, so the cell's own render node still
/// attaches directly to `RenderTable`, and a `TableCell` inside it is still
/// the nearest parent-data ancestor of that node.
#[derive(Clone)]
struct KeyedCell {
    key: CellKey,
    child: BoxedView,
}

/// Which of a table's rows a cell belongs to, and which cell of that row it
/// is — as identity, not as coordinates.
///
/// A part is the corresponding key when there is one and a position
/// otherwise, and the two are never confused: a keyed row is matched to the
/// row with an equal key wherever it has moved to, an unkeyed row to the
/// unkeyed row with the same ordinal *among the unkeyed rows* (which is what
/// lets a keyed row move past them without disturbing them), a keyed cell to
/// the cell with an equal key anywhere in its row, and a keyless cell to the
/// cell in the same column.
#[derive(Clone)]
enum KeyPart {
    /// The key its owner carries. Compared with `key_eq`, never by hash: the
    /// reconciler treats hashes as buckets and disambiguates semantically,
    /// and so must this.
    Keyed(Box<dyn ViewKey>),
    /// The owner's position among its keyless peers.
    Position(usize),
}

impl KeyPart {
    fn part_eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Keyed(a), Self::Keyed(b)) => a.key_eq(&**b),
            (Self::Position(a), Self::Position(b)) => a == b,
            _ => false,
        }
    }

    fn part_hash(&self) -> u64 {
        match self {
            // The discriminant keeps a key's hash out of the position space.
            Self::Keyed(key) => key.key_hash() ^ 0x9e37_79b9_7f4a_7c15,
            Self::Position(index) => *index as u64,
        }
    }
}

/// A cell's identity: its row's, then its own.
#[derive(Clone)]
struct CellKey {
    row: KeyPart,
    cell: KeyPart,
}

impl ViewKey for CellKey {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn key_eq(&self, other: &dyn ViewKey) -> bool {
        // PORT-CHECK-OK-DOWNCAST: `key_eq` takes `&dyn ViewKey` and is
        // contractually a comparison against this key's own type — every key
        // in the workspace implements it this way (see `SaltedKey`).
        let same_kind = other.as_any().downcast_ref::<Self>(); // PORT-CHECK-OK-DOWNCAST: see above
        same_kind
            .is_some_and(|other| self.row.part_eq(&other.row) && self.cell.part_eq(&other.cell))
    }

    fn key_hash(&self) -> u64 {
        // Rotating the row's hash keeps `(row a, cell b)` apart from
        // `(row b, cell a)`.
        self.row.part_hash().rotate_left(32) ^ self.cell.part_hash()
    }

    fn clone_key(&self) -> Box<dyn ViewKey> {
        Box::new(self.clone())
    }

    fn debug_fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "CellKey(row: {:?}, cell: {:?})", self.row, self.cell)
    }

    // `is_global_key` keeps the trait default `false`: this wrapper must
    // never register a cell's own GlobalKey, which stays on the cell.
}

impl std::fmt::Debug for KeyPart {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Keyed(key) => write!(f, "{:?}", &**key),
            Self::Position(index) => write!(f, "#{index}"),
        }
    }
}

impl KeyedCell {
    fn new(key: CellKey, child: BoxedView) -> Self {
        Self { key, child }
    }
}

impl std::fmt::Debug for KeyedCell {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeyedCell")
            .field("key", &self.key.row)
            .field("cell", &self.key.cell)
            .finish_non_exhaustive()
    }
}

impl View for KeyedCell {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateless(self)
    }

    fn key(&self) -> Option<&dyn ViewKey> {
        Some(&self.key)
    }
}

impl StatelessView for KeyedCell {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        self.child.clone()
    }
}

/// Lays out `rows` in a grid: `RenderTable` resolves each column's width
/// (fixed/flex/fraction/intrinsic) and sizes each row to its tallest cell.
///
/// Flutter parity: `widgets/table.dart` `Table` over `RenderTable`. Defaults
/// match Flutter: `default_column_width = Flex(1.0)`,
/// `default_vertical_alignment = Top`, no border, no explicit text baseline.
///
/// Rows carry identity. `RenderTable`'s children are one flat row-major list,
/// so the element tree reconciles them as one flat list — but each cell is
/// keyed by its row's identity and its column, which makes that reconcile
/// behave as Flutter's row-scoped `_TableElement.update` does: a keyed row
/// keeps its cells wherever it moves to, unkeyed rows are matched to each
/// other in order, and a row with no counterpart is disposed whole. Wrap a
/// cell in [`TableCell`] to override its vertical alignment.
#[derive(Clone, Debug)]
pub struct Table {
    rows: Vec<TableRow>,
    /// `rows`' cells in row-major order, each carrying its row's identity.
    /// Built once with the table rather than per visit, so a rebuild costs
    /// no more cloning than the rows themselves already do.
    cells: Vec<KeyedCell>,
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
        let rows = square_up(rows);
        Self {
            cells: identified_cells(&rows),
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

/// `rows`' cells in row-major order, each keyed by its row's identity.
///
/// A keyed row is identified by its key's hash; an unkeyed row by its ordinal
/// among the unkeyed rows, so inserting or removing a keyed row does not
/// renumber the unkeyed ones (Flutter matches its unkeyed rows by sequence
/// among themselves for the same reason).
/// Makes `rows` rectangular against the first row's cell count, padding a
/// short row with empty cells and dropping a long row's extras.
///
/// A table whose rows disagree used to be a `debug_assert!` here and another
/// one in the render object, which means a release build carried the ragged
/// grid all the way down: the render object floor-divides to get its row
/// count, so a trailing partial row was laid out by nothing and hit-tested by
/// nothing while still being handed to paint. Squaring up at construction
/// removes that state instead of asserting about it.
///
/// Repairing rather than rejecting is this library's rule for caller
/// configuration — the same rule `RenderViewport::set_anchor` follows for an
/// out-of-range anchor and `RenderTable` follows for a baseline alignment with
/// no text baseline. Flutter asserts instead; the divergence is deliberate,
/// and the warning is what keeps it from being silent.
fn square_up(mut rows: Vec<TableRow>) -> Vec<TableRow> {
    let Some(columns) = rows.first().map(|row| row.cells.len()) else {
        return rows;
    };
    let ragged = rows.iter().any(|row| row.cells.len() != columns);
    if !ragged {
        return rows;
    }

    let found: Vec<usize> = rows.iter().map(|row| row.cells.len()).collect();
    tracing::warn!(
        expected = columns,
        ?found,
        "Table: the rows do not all have the first row's cell count; short \
         rows are padded with empty cells and extra cells are dropped. Give \
         every row the same number of cells."
    );

    for row in &mut rows {
        row.cells.truncate(columns);
        while row.cells.len() < columns {
            row.cells.push(SizedBox::shrink().boxed());
        }
    }
    rows
}

fn identified_cells(rows: &[TableRow]) -> Vec<KeyedCell> {
    let mut unkeyed_ordinal = 0usize;
    let mut cells = Vec::new();
    for row in rows {
        let row_part = if let Some(key) = &row.key {
            KeyPart::Keyed(key.clone_key())
        } else {
            let ordinal = unkeyed_ordinal;
            unkeyed_ordinal += 1;
            KeyPart::Position(ordinal)
        };
        for (column, cell) in row.cells.iter().enumerate() {
            // A cell that carries its own key keeps it as its identity, so it
            // is matched to the cell with an equal key anywhere in its row —
            // the identity it had before this wrapper existed. A keyless cell
            // is matched by column, as its position in the row.
            let cell_part = cell.0.key().map_or(KeyPart::Position(column), |key| {
                KeyPart::Keyed(key.clone_key())
            });
            cells.push(KeyedCell::new(
                CellKey {
                    row: row_part.clone(),
                    cell: cell_part,
                },
                cell.clone(),
            ));
        }
    }
    cells
}

impl RenderView for Table {
    type Protocol = BoxProtocol;
    type RenderObject = RenderTable;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
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

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        // No shape check here: `Table::new` squared the rows up, so the grid
        // this hands the render object always divides evenly.
        let mut impact = flui_rendering::RenderUpdateImpact::NONE;
        impact |= render_object.set_column_count(self.column_count());
        impact |= render_object.set_column_widths(self.column_widths.clone());
        impact |= render_object.set_default_column_width(self.default_column_width.clone());
        impact |= render_object.set_default_vertical_alignment(self.default_vertical_alignment);
        impact |= render_object.set_text_baseline(self.text_baseline);
        impact |= render_object.set_border(self.border);
        impact |= render_object.set_row_decorations(self.row_decorations());
        impact
    }

    fn has_children(&self) -> bool {
        self.rows.iter().any(|row| !row.cells.is_empty())
    }

    fn visit_child_views(&self, visitor: &mut dyn FnMut(&dyn View)) {
        // Row-major, the exact order `RenderTable`'s flat child list expects
        // (`row = index / column_count`, `col = index % column_count`), each
        // cell carrying its row's identity so the flat reconcile preserves
        // rows the way Flutter's row-scoped one does.
        for cell in &self.cells {
            visitor(cell);
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

    fn apply_parent_data(
        &self,
        parent_data: &mut Self::ParentData,
    ) -> flui_rendering::RenderUpdateImpact {
        let alignment = Some(self.vertical_alignment);
        if parent_data.vertical_alignment == alignment {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        parent_data.vertical_alignment = alignment;
        flui_rendering::RenderUpdateImpact::LAYOUT
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
    fn table_cell_parent_data_reports_exact_impact_and_preserves_layout_fields() {
        let mut data = TableCellParentData::new(4, 6, TableCellVerticalAlignment::Top);
        data.offset = flui_types::Offset::new(
            flui_types::geometry::px(8.0),
            flui_types::geometry::px(13.0),
        );
        let unchanged = TableCell::new(TableCellVerticalAlignment::Top, SizedBox::shrink());
        assert_eq!(
            unchanged.apply_parent_data(&mut data),
            flui_rendering::RenderUpdateImpact::NONE
        );
        let changed = TableCell::new(TableCellVerticalAlignment::Bottom, SizedBox::shrink());
        assert_eq!(
            changed.apply_parent_data(&mut data),
            flui_rendering::RenderUpdateImpact::LAYOUT
        );
        assert_eq!(data.x, 4);
        assert_eq!(data.y, 6);
        assert_eq!(
            data.offset,
            flui_types::Offset::new(
                flui_types::geometry::px(8.0),
                flui_types::geometry::px(13.0)
            )
        );
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
            .create_render_object(&flui_view::RenderObjectContext::detached());
        assert_eq!(render_object.border(), Some(&border));
    }

    #[test]
    fn update_render_object_replaces_the_border() {
        let mut render_object = Table::new(vec![row(1)])
            .create_render_object(&flui_view::RenderObjectContext::detached());
        assert_eq!(render_object.border(), None);

        let border = TableBorder::all(flui_types::styling::BorderSide::new(
            Color::BLACK,
            flui_types::geometry::px(2.0),
            flui_types::styling::BorderStyle::Solid,
        ));
        let impact = Table::new(vec![row(1)])
            .border(border)
            .update_render_object(
                &flui_view::RenderObjectContext::detached(),
                &mut render_object,
            );
        assert_eq!(impact, flui_rendering::RenderUpdateImpact::LAYOUT);
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
