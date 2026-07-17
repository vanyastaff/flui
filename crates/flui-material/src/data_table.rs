//! [`DataTable`] — a tabular data display with an optional leading
//! selection checkbox column.
//!
//! # Flutter parity
//!
//! `material/data_table.dart`'s `DataColumn`/`DataRow`/`DataCell`/`DataTable`
//! and `material/data_table_theme.dart`'s `DataTableThemeData` (oracle tag
//! `3.44.0`). Verified constants (`data_table.dart`'s private statics,
//! `:815-828`, and the `kMinInteractiveDimension` default sourced from
//! `constants.dart`):
//!
//! | Token | Value | Oracle |
//! |---|---|---|
//! | `_headingRowHeight` | `56.0` | `data_table.dart` |
//! | `_horizontalMargin` | `24.0` | `data_table.dart` |
//! | `_columnSpacing` | `56.0` | `data_table.dart` |
//! | `_dividerThickness` | `1.0` | `data_table.dart` |
//! | data row min/max height | `kMinInteractiveDimension` = `48.0` | `constants.dart` |
//! | heading text style | `TextTheme.titleSmall` | `_buildHeadingCell` |
//! | data text style | `TextTheme.bodyMedium` | `_buildDataCell` |
//! | selected row color | `colorScheme.primary` @ `8%` opacity | `build`'s `defaultRowColor` |
//! | `Checkbox.width` (checkbox column formula) | `18.0` | `checkbox.dart` |
//!
//! Two corrections against this task's initial brief, both verified at the
//! tag rather than assumed: the data row height default is
//! `kMinInteractiveDimension` (`48.0`), not `52.0`; the heading text style is
//! `TextTheme.titleSmall`, not `labelLarge` — `data_table.dart` at `3.44.0`
//! has no `_DataTableDefaultsM3` token-class layer at all (unlike the button
//! family), so every default above is a bare literal or a direct
//! `TextTheme`/`ColorScheme` read, confirmed by grepping the oracle file
//! directly rather than assuming an M3-tokens indirection exists.
//!
//! # Layout: genuine per-column intrinsic sizing, not a fallback
//!
//! Flutter's `DataTable` lays out over a custom `Table`/`RenderTable` with
//! per-column `IntrinsicColumnWidth` sizing. FLUI already has the same
//! machinery — [`flui_widgets::Table`] over [`flui_objects::RenderTable`],
//! including
//! [`TableColumnWidth::Intrinsic`] with the oracle's own 4-pass
//! grow/shrink algorithm (`rendering/table.dart:1070-1236`, ported in
//! `flui-objects/src/layout/table.rs`). V1 therefore uses the SAME default
//! heuristic as the oracle (`_initOnlyTextColumn`/`build`'s column-width
//! selection, `data_table.dart:1148-1155`): the single non-numeric column
//! (if there is exactly one) gets `Intrinsic { flex: Some(1.0) }`, every
//! other column gets `Intrinsic { flex: None }`, and [`DataColumn::column_width`]
//! overrides either with any [`TableColumnWidth`] (the oracle's
//! `DataColumn.columnWidth`). No fixed/flex compromise was needed — the
//! "layout reality check" this task opened with does not hold for this
//! codebase.
//!
//! # Composition: per-cell `InkWell`, not a row-spanning `TableRowInkWell`
//!
//! The oracle wraps each selectable row's non-`onTap` cells in
//! `TableRowInkWell`, a row-rect-spanning ink responder that walks up to the
//! nearest `RenderTable` (`data_table.dart`'s `TableRowInkWell.getRectCallback`)
//! to paint one splash across the whole row. FLUI's `RenderTable` exposes no
//! such row-rect query yet. V1 instead wraps each selectable cell
//! individually in [`crate::InkWell`] bound to the SAME toggle callback —
//! tapping any cell (or the checkbox's own padding) still fires
//! [`DataRow::on_select_changed`] with the same next value, but the overlay
//! fill is clipped to each cell's own bounds rather than spanning the row. A
//! named divergence, not a silent one.
//!
//! # Selection: tristate heading checkbox
//!
//! The heading checkbox (shown when [`DataTable::show_checkbox_column`] is
//! `true` and at least one row carries [`DataRow::on_select_changed`])
//! mirrors the oracle's `_handleSelectAll` exactly: checked when every
//! selectable row is selected, unchecked when none are, and
//! indeterminate (`None`, tristate) when some but not all are.
//! [`DataTable::on_select_all`] overrides the fan-out; otherwise every
//! selectable row whose `selected` differs from the new value is toggled.
//!
//! # Deferred (named, not silently dropped)
//!
//! - **Sorting** — `sortColumnIndex`/`sortAscending`/`DataColumn.onSort` and
//!   the animated sort-arrow indicator. No `DataColumn` sort surface ships in
//!   V1.
//! - **`PaginatedDataTable`** — a distinct oracle widget, out of scope.
//! - **Editable cells** — `DataCell.showEditIcon`/`placeholder`, and
//!   `DataCell.onDoubleTap`/`onLongPress`/`onTapDown`/`onTapCancel` (only
//!   [`DataCell::on_tap`] ships).
//! - **`DataRow.onLongPress`/`onHover`/`mouseCursor`/`color`** — no per-row
//!   override surface beyond `selected`/`on_select_changed` yet; a row's
//!   background/overlay always resolves through the table-level
//!   [`DataTable::data_row_color`] cascade.
//! - **`DataColumn.tooltip`/`onSort`/`mouseCursor`/`headingRowAlignment`** —
//!   tied to the deferred sort feature.
//! - **`DataTable.border`/`clipBehavior`** — the table always composes as
//!   the oracle's `Clip.none` default with no `TableBorder`.
//! - **Sticky headers** — `DataTable` (unlike a future scrolling container)
//!   never scrolled independently of its heading row in the oracle either;
//!   this is a property of whatever scrolls a `DataTable`, not of the widget
//!   itself, so there is nothing to port here.
//! - **Dense/`VisualDensity`** — no consumer wired to this substrate yet,
//!   matching every other V1 selection control in this crate.

use std::collections::HashMap;
use std::rc::Rc;

use flui_rendering::constraints::BoxConstraints;
use flui_types::geometry::px;
use flui_types::styling::{Border, BorderSide, BorderStyle, BoxDecoration, Color};
use flui_types::typography::TextStyle;
use flui_types::{Alignment, EdgeInsets, Pixels};
use flui_view::prelude::*;
use flui_widgets::{
    Center, Container, DefaultTextStyle, Padding, Semantics, SemanticsRole, Table, TableCell,
    TableCellVerticalAlignment, TableColumnWidth, TableRow, WidgetState, WidgetStateProperty,
    WidgetStates,
};

use crate::checkbox::{CHECKBOX_EDGE_SIZE, Checkbox};
use crate::color_scheme::ColorScheme;
use crate::ink_well::InkWell;
use crate::material::Material;
use crate::theme::Theme;
use crate::theme_data::ThemeData;

/// `data_table.dart`'s `_headingRowHeight` (oracle tag `3.44.0`).
const DEFAULT_HEADING_ROW_HEIGHT: f32 = 56.0;
/// `data_table.dart`'s `_horizontalMargin` (oracle tag `3.44.0`).
const DEFAULT_HORIZONTAL_MARGIN: f32 = 24.0;
/// `data_table.dart`'s `_columnSpacing` (oracle tag `3.44.0`).
const DEFAULT_COLUMN_SPACING: f32 = 56.0;
/// `data_table.dart`'s `_dividerThickness` (oracle tag `3.44.0`).
const DEFAULT_DIVIDER_THICKNESS: f32 = 1.0;
/// `kMinInteractiveDimension` (`constants.dart`, `48.0`, oracle tag
/// `3.44.0`) — the data row min/max height default. Verified at the tag;
/// NOT `52.0`.
const DEFAULT_DATA_ROW_HEIGHT: f32 = 48.0;
/// The selected-row default color's opacity: `colorScheme.primary.withOpacity(0.08)`
/// (`data_table.dart`'s `defaultRowColor`, oracle tag `3.44.0`).
const SELECTED_ROW_OPACITY: f32 = 0.08;

/// A row-selection toggle: fires with the row's next `selected` value.
/// `Rc`-based (owner-local, per ADR-0027) — matches [`InkWell`]'s own
/// callback shape.
type RowSelectCallback = Rc<dyn Fn(bool)>;
/// A cell tap handler.
type CellTapCallback = Rc<dyn Fn()>;
/// A resolved, per-state row/overlay color cascade.
type RowColorProperty = WidgetStateProperty<Option<Color>>;

/// Column configuration for a [`DataTable`].
///
/// Flutter parity: `DataColumn` (`data_table.dart`, oracle tag `3.44.0`),
/// narrowed to [`label`](Self::new) and [`numeric`](Self::numeric) — see the
/// module docs for the deferred `tooltip`/`onSort`/`mouseCursor`/
/// `headingRowAlignment` fields.
#[derive(Clone, Debug)]
pub struct DataColumn {
    label: BoxedView,
    numeric: bool,
    column_width: Option<TableColumnWidth>,
}

impl DataColumn {
    /// A column headed by `label`, non-numeric, with the default
    /// intrinsic-width sizing (see the module docs).
    pub fn new(label: impl IntoView) -> Self {
        Self {
            label: label.into_view().boxed(),
            numeric: false,
            column_width: None,
        }
    }

    /// Marks this column's cell contents as numeric: right-aligned instead
    /// of left-aligned, and excluded from the "only text column gets flex"
    /// heuristic. Flutter parity: `DataColumn.numeric`.
    #[must_use]
    pub fn numeric(mut self, numeric: bool) -> Self {
        self.numeric = numeric;
        self
    }

    /// Overrides this column's width, bypassing the default intrinsic-width
    /// heuristic. Flutter parity: `DataColumn.columnWidth`.
    #[must_use]
    pub fn column_width(mut self, width: TableColumnWidth) -> Self {
        self.column_width = Some(width);
        self
    }
}

/// One cell's data within a [`DataRow`].
///
/// Flutter parity: `DataCell` (`data_table.dart`, oracle tag `3.44.0`),
/// narrowed to [`child`](Self::new) and [`on_tap`](Self::on_tap) — see the
/// module docs for the deferred `placeholder`/`showEditIcon`/
/// `onDoubleTap`/`onLongPress`/`onTapDown`/`onTapCancel` fields.
#[derive(Clone)]
pub struct DataCell {
    child: BoxedView,
    on_tap: Option<CellTapCallback>,
}

impl std::fmt::Debug for DataCell {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DataCell")
            .field("has_on_tap", &self.on_tap.is_some())
            .finish_non_exhaustive()
    }
}

impl DataCell {
    /// A cell displaying `child`, with no tap handler — a tap instead falls
    /// through to the owning [`DataRow`]'s selection toggle, if any.
    pub fn new(child: impl IntoView) -> Self {
        Self {
            child: child.into_view().boxed(),
            on_tap: None,
        }
    }

    /// Sets a tap handler for this specific cell. When present, it overrides
    /// the row's own selection-toggle tap for this cell only. Flutter
    /// parity: `DataCell.onTap`.
    #[must_use]
    pub fn on_tap(mut self, callback: impl Fn() + 'static) -> Self {
        self.on_tap = Some(Rc::new(callback));
        self
    }
}

/// Row configuration and cell data for a [`DataTable`].
///
/// Flutter parity: `DataRow` (`data_table.dart`, oracle tag `3.44.0`),
/// narrowed to [`cells`](Self::new), [`selected`](Self::selected), and
/// [`on_select_changed`](Self::on_select_changed) — see the module docs for
/// the deferred `onLongPress`/`onHover`/`color`/`mouseCursor` fields.
#[derive(Clone)]
pub struct DataRow {
    cells: Vec<DataCell>,
    selected: bool,
    on_select_changed: Option<RowSelectCallback>,
}

impl std::fmt::Debug for DataRow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DataRow")
            .field("cell_count", &self.cells.len())
            .field("selected", &self.selected)
            .field("is_selectable", &self.on_select_changed.is_some())
            .finish_non_exhaustive()
    }
}

impl DataRow {
    /// A row of `cells`, unselected, with no selection handler (not
    /// selectable — no checkbox is shown for this row even when the table
    /// displays a checkbox column).
    pub fn new(cells: Vec<DataCell>) -> Self {
        Self {
            cells,
            selected: false,
            on_select_changed: None,
        }
    }

    /// Marks this row as currently selected. Flutter parity: `DataRow.selected`.
    #[must_use]
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    /// Sets the selection-change handler, fired with the row's next
    /// `selected` value on a checkbox toggle or a row tap. Presence of a
    /// handler is what makes this row selectable — see the module docs'
    /// "Selection" section. Flutter parity: `DataRow.onSelectChanged`.
    #[must_use]
    pub fn on_select_changed(mut self, callback: impl Fn(bool) + 'static) -> Self {
        self.on_select_changed = Some(Rc::new(callback));
        self
    }
}

/// A Material Design data table: a heading row of [`DataColumn`] labels
/// above [`DataRow`]s of [`DataCell`]s, with an optional leading selection
/// checkbox column.
///
/// See the module docs for the M3 default token table, the layout strategy,
/// and the named deferrals.
///
/// ```rust
/// use flui_material::{DataCell, DataColumn, DataRow, DataTable, Theme, ThemeData};
/// use flui_widgets::Text;
///
/// let table = DataTable::new(
///     vec![DataColumn::new(Text::new("Name")), DataColumn::new(Text::new("Age")).numeric(true)],
///     vec![DataRow::new(vec![
///         DataCell::new(Text::new("Ada")),
///         DataCell::new(Text::new("36")),
///     ])],
/// );
/// let _themed = Theme::new(ThemeData::light(), table);
/// ```
#[derive(Clone, StatelessView)]
pub struct DataTable {
    columns: Vec<DataColumn>,
    rows: Vec<DataRow>,
    on_select_all: Option<RowSelectCallback>,
    show_checkbox_column: bool,
    show_bottom_border: bool,
    decoration: Option<BoxDecoration<Pixels>>,
    data_row_color: Option<RowColorProperty>,
    data_row_min_height: Option<f32>,
    data_row_max_height: Option<f32>,
    data_text_style: Option<TextStyle>,
    heading_row_color: Option<RowColorProperty>,
    heading_row_height: Option<f32>,
    heading_text_style: Option<TextStyle>,
    horizontal_margin: Option<f32>,
    column_spacing: Option<f32>,
    divider_thickness: Option<f32>,
    checkbox_horizontal_margin: Option<f32>,
}

impl std::fmt::Debug for DataTable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DataTable")
            .field("column_count", &self.columns.len())
            .field("row_count", &self.rows.len())
            .field("show_checkbox_column", &self.show_checkbox_column)
            .finish_non_exhaustive()
    }
}

impl DataTable {
    /// A table of `columns` headings above `rows` of data, with every style
    /// override falling through to the M3 defaults (see the module docs'
    /// token table). Flutter parity: `DataTable.new`.
    ///
    /// # Panics (debug only)
    ///
    /// Debug-asserts every row's cell count matches `columns.len()` on
    /// [`build`](StatelessView::build) — Flutter parity: `DataTable`'s own
    /// constructor assert.
    pub fn new(columns: Vec<DataColumn>, rows: Vec<DataRow>) -> Self {
        Self {
            columns,
            rows,
            on_select_all: None,
            show_checkbox_column: true,
            show_bottom_border: false,
            decoration: None,
            data_row_color: None,
            data_row_min_height: None,
            data_row_max_height: None,
            data_text_style: None,
            heading_row_color: None,
            heading_row_height: None,
            heading_text_style: None,
            horizontal_margin: None,
            column_spacing: None,
            divider_thickness: None,
            checkbox_horizontal_margin: None,
        }
    }

    /// Overrides the heading checkbox's "select/clear all" fan-out; without
    /// it, every selectable row's own handler is called directly. Flutter
    /// parity: `DataTable.onSelectAll`.
    #[must_use]
    pub fn on_select_all(mut self, callback: impl Fn(bool) + 'static) -> Self {
        self.on_select_all = Some(Rc::new(callback));
        self
    }

    /// Whether a leading checkbox column is displayed when at least one row
    /// is selectable. Defaults to `true`. Flutter parity:
    /// `DataTable.showCheckboxColumn`.
    #[must_use]
    pub fn show_checkbox_column(mut self, show: bool) -> Self {
        self.show_checkbox_column = show;
        self
    }

    /// Whether every row (including the heading row) paints a bottom
    /// divider instead of the default top divider on rows after the first.
    /// Defaults to `false`. Flutter parity: `DataTable.showBottomBorder`.
    #[must_use]
    pub fn show_bottom_border(mut self, show: bool) -> Self {
        self.show_bottom_border = show;
        self
    }

    /// Overrides the table's background/border decoration. Flutter parity:
    /// `DataTable.decoration`.
    #[must_use]
    pub fn decoration(mut self, decoration: BoxDecoration<Pixels>) -> Self {
        self.decoration = Some(decoration);
        self
    }

    /// Overrides the data rows' background color, per state. Flutter
    /// parity: `DataTable.dataRowColor`.
    #[must_use]
    pub fn data_row_color(mut self, color: RowColorProperty) -> Self {
        self.data_row_color = Some(color);
        self
    }

    /// Overrides each data row's minimum height. Flutter parity:
    /// `DataTable.dataRowMinHeight`.
    #[must_use]
    pub fn data_row_min_height(mut self, height: f32) -> Self {
        self.data_row_min_height = Some(height);
        self
    }

    /// Overrides each data row's maximum height. Flutter parity:
    /// `DataTable.dataRowMaxHeight`.
    #[must_use]
    pub fn data_row_max_height(mut self, height: f32) -> Self {
        self.data_row_max_height = Some(height);
        self
    }

    /// Overrides the data cells' text style. Flutter parity:
    /// `DataTable.dataTextStyle`.
    #[must_use]
    pub fn data_text_style(mut self, style: TextStyle) -> Self {
        self.data_text_style = Some(style);
        self
    }

    /// Overrides the heading row's background color, per state. Flutter
    /// parity: `DataTable.headingRowColor`.
    #[must_use]
    pub fn heading_row_color(mut self, color: RowColorProperty) -> Self {
        self.heading_row_color = Some(color);
        self
    }

    /// Overrides the heading row's height. Flutter parity:
    /// `DataTable.headingRowHeight`.
    #[must_use]
    pub fn heading_row_height(mut self, height: f32) -> Self {
        self.heading_row_height = Some(height);
        self
    }

    /// Overrides the heading cells' text style. Flutter parity:
    /// `DataTable.headingTextStyle`.
    #[must_use]
    pub fn heading_text_style(mut self, style: TextStyle) -> Self {
        self.heading_text_style = Some(style);
        self
    }

    /// Overrides the margin between the table's edges and the first/last
    /// column's content. Flutter parity: `DataTable.horizontalMargin`.
    #[must_use]
    pub fn horizontal_margin(mut self, margin: f32) -> Self {
        self.horizontal_margin = Some(margin);
        self
    }

    /// Overrides the margin between adjacent data columns. Flutter parity:
    /// `DataTable.columnSpacing`.
    #[must_use]
    pub fn column_spacing(mut self, spacing: f32) -> Self {
        self.column_spacing = Some(spacing);
        self
    }

    /// Overrides the divider thickness painted between rows. Flutter
    /// parity: `DataTable.dividerThickness`.
    #[must_use]
    pub fn divider_thickness(mut self, thickness: f32) -> Self {
        self.divider_thickness = Some(thickness);
        self
    }

    /// Overrides the margin around the leading selection checkbox. Flutter
    /// parity: `DataTable.checkboxHorizontalMargin`.
    #[must_use]
    pub fn checkbox_horizontal_margin(mut self, margin: f32) -> Self {
        self.checkbox_horizontal_margin = Some(margin);
        self
    }
}

// =============================================================================
// Style resolution — widget -> theme -> M3 default, pure and unit-testable
// (mirrors `crate::divider::resolve_style`).
// =============================================================================

/// [`DataTable`]'s theme-resolved geometry/color/text-style — see
/// [`resolve_style`]'s doc comment for the cascade.
struct ResolvedDataTableStyle {
    decoration: Option<BoxDecoration<Pixels>>,
    /// The widget/theme `dataRowColor` override cascade — `None` when
    /// neither tier set one (distinct from "resolves to no color for these
    /// states", which is `Some` wrapping a property that itself returns
    /// `None`). Used BOTH for each row's background (with the M3 selected-row
    /// default layered under it) and for the row `InkWell`'s overlay color
    /// (WITHOUT that default — see the module docs' overlay note).
    data_row_color: Option<RowColorProperty>,
    /// The M3 default row-color resolver: `Selected` -> `primary@8%`, else
    /// no color. Always present (unlike `data_row_color`).
    default_row_color: RowColorProperty,
    data_row_min_height: f32,
    data_row_max_height: f32,
    data_text_style: TextStyle,
    /// The heading row's resolved background color — pre-resolved against
    /// the empty state set (the oracle always resolves `headingRowColor`
    /// against `<WidgetState>{}`, `data_table.dart`'s `build`).
    heading_row_color: Option<Color>,
    heading_row_height: f32,
    heading_text_style: TextStyle,
    horizontal_margin: f32,
    column_spacing: f32,
    divider_thickness: f32,
    checkbox_margin_start: f32,
    checkbox_margin_end: f32,
}

/// Resolves the M3 `DataTable` defaults through the widget -> theme ->
/// default cascade, per field. Flutter parity: `this.horizontalMargin ??
/// dataTableTheme.horizontalMargin ?? theme.dataTableTheme.horizontalMargin
/// ?? _horizontalMargin` (and the sibling per-field cascades),
/// `data_table.dart`'s `build`, oracle tag `3.44.0`.
fn resolve_style(widget: &DataTable, theme: &ThemeData) -> ResolvedDataTableStyle {
    let table_theme = theme.data_table_theme.as_ref();

    let horizontal_margin = widget
        .horizontal_margin
        .or_else(|| table_theme.and_then(|t| t.horizontal_margin))
        .unwrap_or(DEFAULT_HORIZONTAL_MARGIN);
    let checkbox_horizontal_margin = widget
        .checkbox_horizontal_margin
        .or_else(|| table_theme.and_then(|t| t.checkbox_horizontal_margin));
    let checkbox_margin_start = checkbox_horizontal_margin.unwrap_or(horizontal_margin);
    let checkbox_margin_end = checkbox_horizontal_margin.unwrap_or(horizontal_margin / 2.0);
    let column_spacing = widget
        .column_spacing
        .or_else(|| table_theme.and_then(|t| t.column_spacing))
        .unwrap_or(DEFAULT_COLUMN_SPACING);
    let divider_thickness = widget
        .divider_thickness
        .or_else(|| table_theme.and_then(|t| t.divider_thickness))
        .unwrap_or(DEFAULT_DIVIDER_THICKNESS);
    let heading_row_height = widget
        .heading_row_height
        .or_else(|| table_theme.and_then(|t| t.heading_row_height))
        .unwrap_or(DEFAULT_HEADING_ROW_HEIGHT);
    let data_row_min_height = widget
        .data_row_min_height
        .or_else(|| table_theme.and_then(|t| t.data_row_min_height))
        .unwrap_or(DEFAULT_DATA_ROW_HEIGHT);
    let data_row_max_height = widget
        .data_row_max_height
        .or_else(|| table_theme.and_then(|t| t.data_row_max_height))
        .unwrap_or(DEFAULT_DATA_ROW_HEIGHT);
    let heading_text_style = widget
        .heading_text_style
        .clone()
        .or_else(|| table_theme.and_then(|t| t.heading_text_style.clone()))
        .unwrap_or_else(|| theme.text_theme.title_small.clone().unwrap_or_default());
    let data_text_style = widget
        .data_text_style
        .clone()
        .or_else(|| table_theme.and_then(|t| t.data_text_style.clone()))
        .unwrap_or_else(|| theme.text_theme.body_medium.clone().unwrap_or_default());
    let decoration = widget
        .decoration
        .clone()
        .or_else(|| table_theme.and_then(|t| t.decoration.clone()));

    let data_row_color = widget
        .data_row_color
        .clone()
        .or_else(|| table_theme.and_then(|t| t.data_row_color.clone()));
    let heading_row_color_property = widget
        .heading_row_color
        .clone()
        .or_else(|| table_theme.and_then(|t| t.heading_row_color.clone()));

    let default_row_color = default_row_color(theme.color_scheme.primary);
    let heading_row_color = heading_row_color_property
        .as_ref()
        .and_then(|property| property.resolve(&WidgetStates::NONE))
        .or_else(|| default_row_color.resolve(&WidgetStates::NONE));

    ResolvedDataTableStyle {
        decoration,
        data_row_color,
        default_row_color,
        data_row_min_height,
        data_row_max_height,
        data_text_style,
        heading_row_color,
        heading_row_height,
        heading_text_style,
        horizontal_margin,
        column_spacing,
        divider_thickness,
        checkbox_margin_start,
        checkbox_margin_end,
    }
}

/// The M3 default row-color resolver: selected rows tint `primary` at
/// [`SELECTED_ROW_OPACITY`], every other state resolves to no color.
/// Flutter parity: `build`'s `defaultRowColor` (`data_table.dart`, oracle
/// tag `3.44.0`).
fn default_row_color(primary: Color) -> RowColorProperty {
    WidgetStateProperty::resolve_with(move |states: &WidgetStates| {
        if states.contains_state(WidgetState::Selected) {
            Some(primary.with_opacity(SELECTED_ROW_OPACITY))
        } else {
            None
        }
    })
}

/// The active [`WidgetStates`] for a data row: `Selected` when the row is
/// selected, `Disabled` when at least one row in the table is selectable but
/// this one is not. Flutter parity: `build`'s per-row `states` set
/// (`data_table.dart`, oracle tag `3.44.0`).
fn row_states(selected: bool, is_disabled: bool) -> WidgetStates {
    let mut states = WidgetStates::NONE;
    if selected {
        states = states.with_state(WidgetState::Selected);
    }
    if is_disabled {
        states = states.with_state(WidgetState::Disabled);
    }
    states
}

// =============================================================================
// Column-width / layout resolution — pure and unit-testable.
// =============================================================================

/// The index of `columns`' only non-numeric column, or `None` when there
/// are zero or more than one. Flutter parity: `DataTable._initOnlyTextColumn`
/// (`data_table.dart`, oracle tag `3.44.0`).
fn only_text_column(columns: &[DataColumn]) -> Option<usize> {
    let mut result = None;
    for (index, column) in columns.iter().enumerate() {
        if !column.numeric {
            if result.is_some() {
                return None;
            }
            result = Some(index);
        }
    }
    result
}

/// The [`TableColumnWidth`] for `column`: its own override if set, else the
/// oracle's default heuristic — `Intrinsic { flex: Some(1.0) }` for the sole
/// non-numeric column, `Intrinsic { flex: None }` otherwise. Flutter parity:
/// `build`'s column-width selection (`data_table.dart:1148-1155`, oracle tag
/// `3.44.0`).
fn column_table_width(
    column: &DataColumn,
    index: usize,
    only_text_column: Option<usize>,
) -> TableColumnWidth {
    if let Some(width) = column.column_width.clone() {
        width
    } else if Some(index) == only_text_column {
        TableColumnWidth::Intrinsic { flex: Some(1.0) }
    } else {
        TableColumnWidth::Intrinsic { flex: None }
    }
}

/// The checkbox column's fixed width: margin + [`CHECKBOX_EDGE_SIZE`] +
/// margin. Flutter parity: `build`'s `tableColumns[0] = FixedColumnWidth(...)`
/// (`data_table.dart`, oracle tag `3.44.0`).
fn checkbox_column_width(margin_start: f32, margin_end: f32) -> f32 {
    margin_start + CHECKBOX_EDGE_SIZE + margin_end
}

/// A data column's cell padding at `data_column_index` (0-based, excluding
/// any leading checkbox column). Flutter parity: `build`'s `paddingStart`/
/// `paddingEnd` `switch` (`data_table.dart`, oracle tag `3.44.0`).
fn cell_padding(
    data_column_index: usize,
    column_count: usize,
    display_checkbox_column: bool,
    checkbox_margin_is_set: bool,
    horizontal_margin: f32,
    column_spacing: f32,
) -> EdgeInsets {
    let start = if data_column_index == 0 {
        if display_checkbox_column && !checkbox_margin_is_set {
            horizontal_margin / 2.0
        } else {
            horizontal_margin
        }
    } else {
        column_spacing / 2.0
    };
    let end = if data_column_index == column_count - 1 {
        horizontal_margin
    } else {
        column_spacing / 2.0
    };
    EdgeInsets::new(px(0.0), px(end), px(0.0), px(start))
}

/// The row divider's border side. Flutter parity: `Divider.createBorderSide`
/// (reusing [`crate::divider`]'s own established M3 default color,
/// `ColorScheme.outlineVariant`), invoked from `build`'s `borderSide`
/// (`data_table.dart`, oracle tag `3.44.0`).
fn row_border_side(color_scheme: &ColorScheme, thickness: f32) -> BorderSide<Pixels> {
    BorderSide::new(
        color_scheme.outline_variant,
        px(thickness),
        BorderStyle::Solid,
    )
}

/// A [`TableRow`]'s background/border decoration. Flutter parity: `build`'s
/// `TableRow` construction — `Border(bottom: side)` when
/// [`DataTable::show_bottom_border`] is set, else `Border(top: side)` for
/// every row EXCEPT `row_index == 0` (the heading row never gets a top
/// border), else no border at all (`data_table.dart`, oracle tag `3.44.0`).
fn row_decoration(
    row_index: usize,
    show_bottom_border: bool,
    color: Option<Color>,
    border_side: BorderSide<Pixels>,
) -> BoxDecoration<Pixels> {
    let border = if show_bottom_border {
        Some(Border::new(None, None, Some(border_side), None))
    } else if row_index == 0 {
        None
    } else {
        Some(Border::new(Some(border_side), None, None, None))
    };
    BoxDecoration::new().set_color(color).set_border(border)
}

/// Whether every selectable row is selected (`all_checked`) and whether some
/// but not all are (`some_checked`, driving the heading checkbox's
/// indeterminate tristate). Flutter parity: `build`'s `allChecked`/
/// `someChecked` (`data_table.dart`, oracle tag `3.44.0`).
fn selection_summary(rows: &[DataRow], display_checkbox_column: bool) -> (bool, bool) {
    if !display_checkbox_column {
        return (false, false);
    }
    let mut selectable_count = 0usize;
    let mut checked_count = 0usize;
    for row in rows {
        if row.on_select_changed.is_some() {
            selectable_count += 1;
            if row.selected {
                checked_count += 1;
            }
        }
    }
    let all_checked = selectable_count > 0 && checked_count == selectable_count;
    let any_checked = checked_count > 0;
    let some_checked = any_checked && !all_checked;
    (all_checked, some_checked)
}

// =============================================================================
// Cell construction
// =============================================================================

/// Wraps `content` in an [`InkWell`] bound to `on_tap`, applying `overlay`
/// only when the widget/theme cascade set one — an unset cascade leaves
/// `InkWell`'s own "paints nothing extra" default in place rather than
/// forcing a color. See [`ResolvedDataTableStyle::data_row_color`]'s doc
/// comment.
fn wrap_selectable(
    content: BoxedView,
    on_tap: impl Fn() + 'static,
    overlay: Option<RowColorProperty>,
) -> BoxedView {
    let mut ink_well = InkWell::new(content).on_tap(on_tap);
    if let Some(overlay) = overlay {
        ink_well = ink_well.overlay_color(overlay);
    }
    ink_well.boxed()
}

/// The heading checkbox cell: a tristate [`Checkbox`] centered in its
/// margin, wrapped to fill the checkbox column's cell. Flutter parity:
/// `_buildCheckbox` called with `tristate: true` from `build`
/// (`data_table.dart`, oracle tag `3.44.0`).
fn header_checkbox_cell(
    checked: Option<bool>,
    on_change: impl Fn(Option<bool>) + 'static,
    margin_start: f32,
    margin_end: f32,
) -> BoxedView {
    let checkbox = Checkbox::new(checked).tristate(true).on_changed(on_change);
    let content = Semantics::new().container(true).child(
        Padding::new(EdgeInsets::new(
            px(0.0),
            px(margin_end),
            px(0.0),
            px(margin_start),
        ))
        .child(Center::new().child(checkbox)),
    );
    TableCell::new(TableCellVerticalAlignment::Fill, content).boxed()
}

/// A data row's checkbox cell: a non-tristate [`Checkbox`], with the whole
/// cell also tap-toggling the row when selectable. Flutter parity:
/// `_buildCheckbox` called per row from `build` (`data_table.dart`, oracle
/// tag `3.44.0`).
fn row_checkbox_cell(
    selected: bool,
    on_select_changed: Option<RowSelectCallback>,
    margin_start: f32,
    margin_end: f32,
    overlay: Option<RowColorProperty>,
) -> BoxedView {
    let mut checkbox = Checkbox::new(Some(selected)).tristate(false);
    if let Some(handler) = on_select_changed.clone() {
        checkbox = checkbox.on_changed(move |next| handler(next.unwrap_or(false)));
    }
    let content: BoxedView = Semantics::new()
        .container(true)
        .child(
            Padding::new(EdgeInsets::new(
                px(0.0),
                px(margin_end),
                px(0.0),
                px(margin_start),
            ))
            .child(Center::new().child(checkbox)),
        )
        .boxed();

    let wrapped = match on_select_changed {
        Some(handler) => wrap_selectable(content, move || handler(!selected), overlay),
        None => content,
    };
    TableCell::new(TableCellVerticalAlignment::Fill, wrapped).boxed()
}

/// A heading cell: the column's label, right-aligned when
/// [`DataColumn::numeric`], at the resolved heading text style and row
/// height. Flutter parity: `_buildHeadingCell` (sorting arrow omitted — see
/// the module docs). `data_table.dart`, oracle tag `3.44.0`.
fn header_cell(
    label: BoxedView,
    numeric: bool,
    padding: EdgeInsets,
    text_style: TextStyle,
    height: f32,
) -> BoxedView {
    let alignment = if numeric {
        Alignment::CENTER_RIGHT
    } else {
        Alignment::CENTER_LEFT
    };
    Semantics::new()
        .role(SemanticsRole::ColumnHeader)
        .child(
            Container::new()
                .padding(padding)
                .height(height)
                .alignment(alignment)
                .child(DefaultTextStyle::new(text_style, label)),
        )
        .boxed()
}

/// A data cell: the cell's child, right-aligned when [`DataColumn::numeric`],
/// at the resolved data text style and row height range, wrapped in an
/// [`InkWell`] when the cell or its row is tappable. Flutter parity:
/// `_buildDataCell` (edit-icon/placeholder omitted — see the module docs).
/// `data_table.dart`, oracle tag `3.44.0`.
#[allow(clippy::too_many_arguments)] // mirrors the oracle's own per-cell parameter list; a patch struct would only relocate this
fn data_cell(
    cell: &DataCell,
    numeric: bool,
    padding: EdgeInsets,
    text_style: TextStyle,
    min_height: f32,
    max_height: f32,
    row_toggle: Option<CellTapCallback>,
    overlay: Option<RowColorProperty>,
) -> BoxedView {
    let alignment = if numeric {
        Alignment::CENTER_RIGHT
    } else {
        Alignment::CENTER_LEFT
    };
    let constraints = BoxConstraints::new(
        Pixels::ZERO,
        Pixels::INFINITY,
        px(min_height),
        px(max_height),
    );
    let content: BoxedView = Container::new()
        .padding(padding)
        .constraints(constraints)
        .alignment(alignment)
        .child(DefaultTextStyle::new(text_style, cell.child.clone()))
        .boxed();

    if let Some(on_tap) = cell.on_tap.clone() {
        wrap_selectable(content, move || on_tap(), overlay)
    } else if let Some(toggle) = row_toggle {
        wrap_selectable(content, move || toggle(), overlay)
    } else {
        content
    }
}

impl StatelessView for DataTable {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        debug_assert!(
            self.rows
                .iter()
                .all(|row| row.cells.len() == self.columns.len()),
            "every DataRow must have as many cells ({}) as DataTable has columns ({})",
            self.rows.first().map_or(0, |row| row.cells.len()),
            self.columns.len(),
        );

        let theme = Theme::of(ctx);
        let style = resolve_style(self, &theme);

        let any_row_selectable = self.rows.iter().any(|row| row.on_select_changed.is_some());
        let display_checkbox_column = self.show_checkbox_column && any_row_selectable;
        let (all_checked, some_checked) = selection_summary(&self.rows, display_checkbox_column);
        let only_text = only_text_column(&self.columns);
        let column_count = self.columns.len() + usize::from(display_checkbox_column);
        let border_side = row_border_side(&theme.color_scheme, style.divider_thickness);

        let mut column_widths: HashMap<usize, TableColumnWidth> = HashMap::new();
        if display_checkbox_column {
            column_widths.insert(
                0,
                TableColumnWidth::Fixed(checkbox_column_width(
                    style.checkbox_margin_start,
                    style.checkbox_margin_end,
                )),
            );
        }
        for (index, column) in self.columns.iter().enumerate() {
            let display_index = index + usize::from(display_checkbox_column);
            column_widths.insert(display_index, column_table_width(column, index, only_text));
        }

        // ---- heading row ----
        let mut heading_cells: Vec<BoxedView> = Vec::with_capacity(column_count);
        if display_checkbox_column {
            let checked = if some_checked {
                None
            } else {
                Some(all_checked)
            };
            let rows_snapshot: Vec<(bool, Option<RowSelectCallback>)> = self
                .rows
                .iter()
                .map(|row| (row.selected, row.on_select_changed.clone()))
                .collect();
            let on_select_all = self.on_select_all.clone();
            heading_cells.push(header_checkbox_cell(
                checked,
                move |next: Option<bool>| {
                    let effective = some_checked || next.unwrap_or(false);
                    if let Some(handler) = &on_select_all {
                        handler(effective);
                    } else {
                        for (selected, handler) in &rows_snapshot {
                            if let Some(handler) = handler
                                && *selected != effective
                            {
                                handler(effective);
                            }
                        }
                    }
                },
                style.checkbox_margin_start,
                style.checkbox_margin_end,
            ));
        }
        for (index, column) in self.columns.iter().enumerate() {
            let padding = cell_padding(
                index,
                self.columns.len(),
                display_checkbox_column,
                self.checkbox_horizontal_margin.is_some(),
                style.horizontal_margin,
                style.column_spacing,
            );
            heading_cells.push(header_cell(
                column.label.clone(),
                column.numeric,
                padding,
                style.heading_text_style.clone(),
                style.heading_row_height,
            ));
        }
        let heading_decoration = row_decoration(
            0,
            self.show_bottom_border,
            style.heading_row_color,
            border_side,
        );
        let mut table_rows = vec![TableRow::new(heading_cells).decoration(heading_decoration)];

        // ---- data rows ----
        for (row_index, row) in self.rows.iter().enumerate() {
            let is_disabled = any_row_selectable && row.on_select_changed.is_none();
            let states = row_states(row.selected, is_disabled);
            let row_color = style
                .data_row_color
                .as_ref()
                .and_then(|property| property.resolve(&states))
                .or_else(|| style.default_row_color.resolve(&states));
            let overlay = style.data_row_color.clone();

            let mut cells: Vec<BoxedView> = Vec::with_capacity(column_count);
            if display_checkbox_column {
                cells.push(row_checkbox_cell(
                    row.selected,
                    row.on_select_changed.clone(),
                    style.checkbox_margin_start,
                    style.checkbox_margin_end,
                    overlay.clone(),
                ));
            }
            let row_toggle: Option<CellTapCallback> =
                row.on_select_changed.clone().map(|handler| {
                    let selected = row.selected;
                    Rc::new(move || handler(!selected)) as CellTapCallback
                });
            for (col_index, column) in self.columns.iter().enumerate() {
                let padding = cell_padding(
                    col_index,
                    self.columns.len(),
                    display_checkbox_column,
                    self.checkbox_horizontal_margin.is_some(),
                    style.horizontal_margin,
                    style.column_spacing,
                );
                cells.push(data_cell(
                    &row.cells[col_index],
                    column.numeric,
                    padding,
                    style.data_text_style.clone(),
                    style.data_row_min_height,
                    style.data_row_max_height,
                    row_toggle.clone(),
                    overlay.clone(),
                ));
            }

            let decoration = row_decoration(
                row_index + 1,
                self.show_bottom_border,
                row_color,
                border_side,
            );
            table_rows.push(TableRow::new(cells).decoration(decoration));
        }

        let table = Table::new(table_rows)
            .column_widths(column_widths)
            .default_vertical_alignment(TableCellVerticalAlignment::Middle);

        let material = Material::new(Color::TRANSPARENT).child(table);
        let mut container = Container::new();
        if let Some(decoration) = style.decoration.clone() {
            container = container.decoration(decoration);
        }
        container.child(material)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme_data::DataTableThemeData;

    fn text_column(label: &str) -> DataColumn {
        DataColumn::new(flui_widgets::Text::new(label.to_string()))
    }

    fn text_cell(label: &str) -> DataCell {
        DataCell::new(flui_widgets::Text::new(label.to_string()))
    }

    // ---- M3 default constants, pinned against the oracle -------------------

    #[test]
    fn default_constants_match_the_oracle() {
        assert_eq!(DEFAULT_HEADING_ROW_HEIGHT, 56.0);
        assert_eq!(DEFAULT_HORIZONTAL_MARGIN, 24.0);
        assert_eq!(DEFAULT_COLUMN_SPACING, 56.0);
        assert_eq!(DEFAULT_DIVIDER_THICKNESS, 1.0);
        // kMinInteractiveDimension, NOT 52.0 — verified at the oracle tag.
        assert_eq!(DEFAULT_DATA_ROW_HEIGHT, 48.0);
        assert_eq!(SELECTED_ROW_OPACITY, 0.08);
        assert_eq!(CHECKBOX_EDGE_SIZE, 18.0);
    }

    #[test]
    fn resolve_style_defaults_to_the_m3_token_table() {
        let theme = ThemeData::light();
        let table = DataTable::new(vec![text_column("Name")], Vec::new());
        let style = resolve_style(&table, &theme);

        assert_eq!(style.heading_row_height, DEFAULT_HEADING_ROW_HEIGHT);
        assert_eq!(style.horizontal_margin, DEFAULT_HORIZONTAL_MARGIN);
        assert_eq!(style.column_spacing, DEFAULT_COLUMN_SPACING);
        assert_eq!(style.divider_thickness, DEFAULT_DIVIDER_THICKNESS);
        assert_eq!(style.data_row_min_height, DEFAULT_DATA_ROW_HEIGHT);
        assert_eq!(style.data_row_max_height, DEFAULT_DATA_ROW_HEIGHT);
        assert_eq!(
            style.heading_text_style,
            theme.text_theme.title_small.unwrap()
        );
        assert_eq!(style.data_text_style, theme.text_theme.body_medium.unwrap());
        assert!(style.decoration.is_none());
        assert!(style.heading_row_color.is_none());
        // No checkbox override: start falls to horizontal_margin, end to half.
        assert_eq!(style.checkbox_margin_start, DEFAULT_HORIZONTAL_MARGIN);
        assert_eq!(style.checkbox_margin_end, DEFAULT_HORIZONTAL_MARGIN / 2.0);
    }

    #[test]
    fn resolve_style_theme_tier_beats_the_default_when_no_widget_override_is_set() {
        let mut theme = ThemeData::light();
        theme.data_table_theme = Some(DataTableThemeData {
            heading_row_height: Some(64.0),
            horizontal_margin: Some(32.0),
            ..Default::default()
        });
        let table = DataTable::new(vec![text_column("Name")], Vec::new());
        let style = resolve_style(&table, &theme);

        assert_eq!(style.heading_row_height, 64.0);
        assert_eq!(style.horizontal_margin, 32.0);
        // Fields the theme left unset independently fall to the M3 default.
        assert_eq!(style.column_spacing, DEFAULT_COLUMN_SPACING);
    }

    #[test]
    fn resolve_style_widget_override_wins_over_the_theme() {
        let mut theme = ThemeData::light();
        theme.data_table_theme = Some(DataTableThemeData {
            heading_row_height: Some(64.0),
            ..Default::default()
        });
        let table = DataTable::new(vec![text_column("Name")], Vec::new()).heading_row_height(72.0);
        let style = resolve_style(&table, &theme);

        assert_eq!(style.heading_row_height, 72.0);
    }

    #[test]
    fn resolve_style_selected_row_default_is_primary_at_8_percent() {
        let theme = ThemeData::light();
        let table = DataTable::new(vec![text_column("Name")], Vec::new());
        let style = resolve_style(&table, &theme);

        let selected = row_states(true, false);
        let unselected = row_states(false, false);
        assert_eq!(
            style.default_row_color.resolve(&selected),
            Some(
                theme
                    .color_scheme
                    .primary
                    .with_opacity(SELECTED_ROW_OPACITY)
            )
        );
        assert_eq!(style.default_row_color.resolve(&unselected), None);
    }

    // ---- only_text_column ----------------------------------------------------

    #[test]
    fn only_text_column_finds_the_sole_non_numeric_column() {
        let columns = vec![
            text_column("Name"),
            text_column("Age").numeric(true),
            text_column("Score").numeric(true),
        ];
        assert_eq!(only_text_column(&columns), Some(0));
    }

    #[test]
    fn only_text_column_is_none_with_multiple_non_numeric_columns() {
        let columns = vec![text_column("Name"), text_column("City")];
        assert_eq!(only_text_column(&columns), None);
    }

    #[test]
    fn only_text_column_is_none_when_every_column_is_numeric() {
        let columns = vec![
            text_column("A").numeric(true),
            text_column("B").numeric(true),
        ];
        assert_eq!(only_text_column(&columns), None);
    }

    #[test]
    fn column_table_width_gives_the_only_text_column_flex_one() {
        let column = text_column("Name");
        assert_eq!(
            column_table_width(&column, 0, Some(0)),
            TableColumnWidth::Intrinsic { flex: Some(1.0) }
        );
    }

    #[test]
    fn column_table_width_gives_other_columns_no_flex() {
        let column = text_column("Age").numeric(true);
        assert_eq!(
            column_table_width(&column, 1, Some(0)),
            TableColumnWidth::Intrinsic { flex: None }
        );
    }

    #[test]
    fn column_table_width_honors_an_explicit_override() {
        let column = text_column("Name").column_width(TableColumnWidth::Fixed(120.0));
        assert_eq!(
            column_table_width(&column, 0, Some(0)),
            TableColumnWidth::Fixed(120.0)
        );
    }

    // ---- checkbox_column_width -------------------------------------------

    #[test]
    fn checkbox_column_width_sums_margins_and_the_checkbox_edge() {
        assert_eq!(checkbox_column_width(24.0, 12.0), 24.0 + 18.0 + 12.0);
    }

    // ---- cell_padding -------------------------------------------------------

    #[test]
    fn cell_padding_first_column_without_checkbox_gets_full_horizontal_margin_start() {
        let padding = cell_padding(0, 2, false, false, 24.0, 56.0);
        assert_eq!(padding.left, px(24.0));
    }

    #[test]
    fn cell_padding_first_column_with_checkbox_and_no_checkbox_margin_gets_half_margin_start() {
        let padding = cell_padding(0, 2, true, false, 24.0, 56.0);
        assert_eq!(padding.left, px(12.0));
    }

    #[test]
    fn cell_padding_first_column_with_explicit_checkbox_margin_gets_full_margin_start() {
        let padding = cell_padding(0, 2, true, true, 24.0, 56.0);
        assert_eq!(padding.left, px(24.0));
    }

    #[test]
    fn cell_padding_middle_column_gets_half_column_spacing_on_both_sides() {
        let padding = cell_padding(1, 3, false, false, 24.0, 56.0);
        assert_eq!(padding.left, px(28.0));
        assert_eq!(padding.right, px(28.0));
    }

    #[test]
    fn cell_padding_last_column_gets_full_horizontal_margin_end() {
        let padding = cell_padding(1, 2, false, false, 24.0, 56.0);
        assert_eq!(padding.right, px(24.0));
    }

    // ---- row_decoration -------------------------------------------------------

    #[test]
    fn row_decoration_heading_row_has_no_border_by_default() {
        let side = BorderSide::new(Color::BLACK, px(1.0), BorderStyle::Solid);
        let decoration = row_decoration(0, false, None, side);
        assert!(decoration.border.is_none());
    }

    #[test]
    fn row_decoration_data_rows_get_a_top_border_by_default() {
        let side = BorderSide::new(Color::BLACK, px(1.0), BorderStyle::Solid);
        let decoration = row_decoration(1, false, None, side);
        let border = decoration.border.expect("data rows must carry a border");
        assert_eq!(border.top, Some(side));
        assert!(border.bottom.is_none());
    }

    #[test]
    fn row_decoration_show_bottom_border_puts_a_border_on_every_row_including_the_heading() {
        let side = BorderSide::new(Color::BLACK, px(1.0), BorderStyle::Solid);
        let heading = row_decoration(0, true, None, side);
        let data_row = row_decoration(1, true, None, side);

        assert_eq!(heading.border.unwrap().bottom, Some(side));
        assert_eq!(data_row.border.unwrap().bottom, Some(side));
    }

    #[test]
    fn row_decoration_carries_the_resolved_color() {
        let side = BorderSide::new(Color::BLACK, px(1.0), BorderStyle::Solid);
        let decoration = row_decoration(1, false, Some(Color::rgb(1, 2, 3)), side);
        assert_eq!(decoration.color, Some(Color::rgb(1, 2, 3)));
    }

    // ---- selection_summary (tristate) ------------------------------------

    #[test]
    fn selection_summary_is_false_false_when_the_checkbox_column_is_hidden() {
        let rows = vec![DataRow::new(vec![text_cell("a")]).selected(true)];
        assert_eq!(selection_summary(&rows, false), (false, false));
    }

    #[test]
    fn selection_summary_all_checked_when_every_selectable_row_is_selected() {
        let rows = vec![
            DataRow::new(vec![text_cell("a")])
                .selected(true)
                .on_select_changed(|_| {}),
            DataRow::new(vec![text_cell("b")])
                .selected(true)
                .on_select_changed(|_| {}),
        ];
        assert_eq!(selection_summary(&rows, true), (true, false));
    }

    #[test]
    fn selection_summary_none_checked_when_no_selectable_row_is_selected() {
        let rows = vec![
            DataRow::new(vec![text_cell("a")]).on_select_changed(|_| {}),
            DataRow::new(vec![text_cell("b")]).on_select_changed(|_| {}),
        ];
        assert_eq!(selection_summary(&rows, true), (false, false));
    }

    #[test]
    fn selection_summary_is_tristate_some_checked_when_only_some_rows_are_selected() {
        // Mutation-honest: a broken `checked_count == selectable_count` (e.g.
        // `>=`) or a dropped `!all_checked` guard collapses this to
        // `(true, ..)` or `(.., false)` — this must observe exactly
        // `(false, true)`.
        let rows = vec![
            DataRow::new(vec![text_cell("a")])
                .selected(true)
                .on_select_changed(|_| {}),
            DataRow::new(vec![text_cell("b")]).on_select_changed(|_| {}),
        ];
        assert_eq!(selection_summary(&rows, true), (false, true));
    }

    #[test]
    fn selection_summary_ignores_non_selectable_rows() {
        // A selected row with no handler must not count toward `all_checked`
        // — the oracle's `rowsWithCheckbox` filters to `onSelectChanged != null`.
        let rows = vec![
            DataRow::new(vec![text_cell("a")])
                .selected(true)
                .on_select_changed(|_| {}),
            DataRow::new(vec![text_cell("b")]).selected(false), // not selectable
        ];
        assert_eq!(selection_summary(&rows, true), (true, false));
    }

    // ---- DataTable builder plumbing ---------------------------------------

    #[test]
    fn new_leaves_every_override_unset_and_defaults_show_checkbox_column_true() {
        let table = DataTable::new(vec![text_column("Name")], Vec::new());
        assert!(table.show_checkbox_column);
        assert!(!table.show_bottom_border);
        assert!(table.heading_row_height.is_none());
        assert!(table.decoration.is_none());
    }

    #[test]
    fn builder_overrides_are_stored_verbatim() {
        let table = DataTable::new(vec![text_column("Name")], Vec::new())
            .show_checkbox_column(false)
            .show_bottom_border(true)
            .heading_row_height(64.0)
            .horizontal_margin(32.0)
            .column_spacing(40.0)
            .divider_thickness(2.0)
            .checkbox_horizontal_margin(16.0)
            .data_row_min_height(50.0)
            .data_row_max_height(60.0);

        assert!(!table.show_checkbox_column);
        assert!(table.show_bottom_border);
        assert_eq!(table.heading_row_height, Some(64.0));
        assert_eq!(table.horizontal_margin, Some(32.0));
        assert_eq!(table.column_spacing, Some(40.0));
        assert_eq!(table.divider_thickness, Some(2.0));
        assert_eq!(table.checkbox_horizontal_margin, Some(16.0));
        assert_eq!(table.data_row_min_height, Some(50.0));
        assert_eq!(table.data_row_max_height, Some(60.0));
    }
}
