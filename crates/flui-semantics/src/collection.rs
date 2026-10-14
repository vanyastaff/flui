//! Where a node sits in a set, a tree or a table (ADR-0206).
//!
//! A screen reader announces "item 12 of 100" from a node's position and its
//! container's size, "row 3, column 2" from a cell's place in a table, and a
//! tree item's depth from its level. Every one of these is a fact about a
//! node's *place*, not about the node, so they travel as one group
//! ([`Collection`]) and merge as one.
//!
//! **A position counts from zero, a level, a row and a column from one.** A
//! position is zero-based as AccessKit's is (the adapters add the one ARIA
//! starts at). A level, a row and a column are the numbers ARIA's authors write,
//! `1` for the first, held as [`NonZeroU32`] and [`NonZeroUsize`] so that a
//! zero-based first row cannot be passed by mistake; the translation subtracts
//! the one AccessKit does not have.

use std::num::{NonZeroU32, NonZeroUsize};

/// How many members a set has.
///
/// A set that is virtualized publishes only some of its members, and a set
/// whose end has not been reached has no size to give. [`Self::Unknown`] says
/// so; it is never the number of members that happen to be built.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SetSize {
    /// The set has this many members, built or not.
    Known(usize),
    /// The number of members is not known.
    Unknown,
}

impl SetSize {
    /// The number of members, if it is known.
    #[must_use]
    pub const fn known(self) -> Option<usize> {
        match self {
            Self::Known(count) => Some(count),
            Self::Unknown => None,
        }
    }
}

/// The extent of a table, a grid or a tree grid.
///
/// Rows and columns are counted as the table has them, not as it has built
/// them: a virtualized table of ten thousand rows reports
/// `SetSize::Known(10_000)` however few are on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TableShape {
    rows: SetSize,
    columns: SetSize,
}

impl TableShape {
    /// A table with `rows` rows and `columns` columns.
    #[must_use]
    pub const fn new(rows: SetSize, columns: SetSize) -> Self {
        Self { rows, columns }
    }

    /// The number of rows.
    #[must_use]
    pub const fn rows(self) -> SetSize {
        self.rows
    }

    /// The number of columns.
    #[must_use]
    pub const fn columns(self) -> SetSize {
        self.columns
    }
}

/// Where a cell, a row or a header sits in its table, and how much of it the
/// cell covers.
///
/// A row and a column are the cell's *true* place in the table, counted from
/// one as `aria-rowindex` and `aria-colindex` are, and not its place among
/// the cells that happen to be built: a virtualized table that has built rows
/// 4000 to 4010 gives them rows 4000 to 4010.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CellPosition {
    row: NonZeroUsize,
    column: NonZeroUsize,
    row_span: NonZeroUsize,
    column_span: NonZeroUsize,
}

impl CellPosition {
    /// A cell of one row and one column at `row`, `column` (both one-based).
    #[must_use]
    pub const fn new(row: NonZeroUsize, column: NonZeroUsize) -> Self {
        Self {
            row,
            column,
            row_span: NonZeroUsize::MIN,
            column_span: NonZeroUsize::MIN,
        }
    }

    /// This cell covering `rows` rows and `columns` columns.
    #[must_use]
    pub const fn spanning(self, rows: NonZeroUsize, columns: NonZeroUsize) -> Self {
        Self {
            row_span: rows,
            column_span: columns,
            ..self
        }
    }

    /// The one-based row of the cell's first row.
    #[must_use]
    pub const fn row(self) -> NonZeroUsize {
        self.row
    }

    /// The one-based column of the cell's first column.
    #[must_use]
    pub const fn column(self) -> NonZeroUsize {
        self.column
    }

    /// How many rows the cell covers.
    #[must_use]
    pub const fn row_span(self) -> NonZeroUsize {
        self.row_span
    }

    /// How many columns the cell covers.
    #[must_use]
    pub const fn column_span(self) -> NonZeroUsize {
        self.column_span
    }
}

/// The order a column or a row is sorted in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SortDirection {
    /// From smallest to largest.
    Ascending,
    /// From largest to smallest.
    Descending,
    /// Sorted by a rule that is neither.
    Other,
}

/// A node's place among its siblings and in a table (ADR-0206).
///
/// Position and size are two halves of one announcement, but they live on two
/// nodes: AccessKit reads a set's size from the *container*
/// (`size_of_set_from_container`), and the position from the item. A node can
/// be both, a tree item that holds a nested list, so a node carries either or
/// both.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct Collection {
    /// This node's zero-based place among the members of its set.
    pub(crate) position_in_set: Option<usize>,
    /// How many members the set this node contains has.
    pub(crate) set_size: Option<SetSize>,
    /// The depth of a tree item, a list item or a row; `1` is the top.
    pub(crate) level: Option<NonZeroU32>,
    /// The extent of the table this node is.
    pub(crate) table: Option<TableShape>,
    /// This node's place in its table.
    pub(crate) cell: Option<CellPosition>,
    /// How this header's column or row is sorted.
    pub(crate) sort: Option<SortDirection>,
    /// Whether the container allows more than one selected member.
    pub(crate) multiselectable: Option<bool>,
}

impl Collection {
    /// Whether two groups describe two places that cannot be one node's:
    /// both declare an entry's position, level, table, cell or sort, or give
    /// the set it contains different sizes or selection rules. Two groups that
    /// declare the *same* size or selection rule describe one set, as two
    /// slivers composed into one list do, and merge.
    pub(crate) fn conflicts(&self, other: &Self) -> bool {
        let Self {
            position_in_set,
            set_size,
            level,
            table,
            cell,
            sort,
            multiselectable,
        } = self;
        let both = |a: bool, b: bool| a && b;
        both(position_in_set.is_some(), other.position_in_set.is_some())
            || both(level.is_some(), other.level.is_some())
            || both(table.is_some(), other.table.is_some())
            || both(cell.is_some(), other.cell.is_some())
            || both(sort.is_some(), other.sort.is_some())
            || both(set_size.is_some(), other.set_size.is_some()) && set_size != &other.set_size
            || both(multiselectable.is_some(), other.multiselectable.is_some())
                && multiselectable != &other.multiselectable
    }

    /// Fills what this group leaves undeclared from `other`; a declared field
    /// is never replaced.
    pub(crate) fn absorb(&mut self, other: &Self) {
        let Self {
            position_in_set,
            set_size,
            level,
            table,
            cell,
            sort,
            multiselectable,
        } = other;
        self.position_in_set = self.position_in_set.or(*position_in_set);
        self.set_size = self.set_size.or(*set_size);
        self.level = self.level.or(*level);
        self.table = self.table.or(*table);
        self.cell = self.cell.or(*cell);
        self.sort = self.sort.or(*sort);
        self.multiselectable = self.multiselectable.or(*multiselectable);
    }
}
