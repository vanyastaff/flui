//! Reconciliation of the sets and tables the built semantics tree declares: positions and sizes
//! are published only where the tree keeps their collection.

use std::num::NonZeroUsize;

use flui_foundation::RenderId;
use flui_semantics::{
    SemanticsCollectionOrigin, SemanticsConfiguration, SemanticsIndexOrigin, SetSize, TableShape,
};

use super::super::subtree_arena::ensure_stack;
use super::{BuiltSemanticsNode, regions::SemanticsSource};

/// A set an ancestor in the built tree contains, as a member below it sees it.
#[derive(Clone, Copy)]
struct SetScope {
    /// The render node that declared it; a region declares sets of its own.
    host: Option<RenderId>,
    size: SetSize,
    origin: SemanticsCollectionOrigin,
    /// A member below would read this set's total as its own: withhold it.
    omit_total: bool,
}

/// Holds every place a node declares to the sets and tables it actually sits
/// in once the tree is built, so a node the clip dropped imposes no scope.
///
/// AccessKit pairs a position with the size of the nearest included ancestor
/// that declares one (`size_of_set_from_container`) and a cell with the
/// nearest table. So:
///
/// - a position past its own known set's size is not published ("item 8 of
///   3"), nor a cell or row outside its known table ("row 5 of 3");
/// - a member of an unknown or separately authored set must not
///   acquire an unrelated total through that walk ("item 12 of 5"): the totals
///   of the known sets above it are withheld rather than a sentinel invented.
pub(super) fn reconcile_collections(root: &mut BuiltSemanticsNode<'_>) {
    fn visit(
        node: &mut BuiltSemanticsNode<'_>,
        sets: &mut Vec<SetScope>,
        tables: &mut Vec<TableShape>,
    ) {
        ensure_stack(|| {
            // A position is held to its own set's size only: one numbered for
            // another set is that set's, and the total is withheld below.
            let member_of_nearest = node.config.position_in_set().is_some()
                && member_matches(node.config.index_origin(), sets.last());
            hold_places_to_scopes(
                &mut node.config,
                sets.last().copied().filter(|_| member_of_nearest),
                tables.last().copied(),
            );
            let own = node.config.size_of_set().map(|size| SetScope {
                host: match node.source {
                    SemanticsSource::Render(id) => Some(id),
                    SemanticsSource::Region(_) => None,
                },
                size,
                origin: node
                    .config
                    .collection_origin()
                    .unwrap_or(SemanticsCollectionOrigin::Authored),
                omit_total: false,
            });
            let unmatched_member = node.config.position_in_set().is_some() && !member_of_nearest;
            // Only a member that would read an unrelated total withholds it:
            // an unknown set with no positioned member below leaves the totals
            // above it alone, and one with members reaches here through them.
            if unmatched_member {
                for scope in sets.iter_mut() {
                    scope.omit_total |= matches!(scope.size, SetSize::Known(_));
                }
            }
            // A table, a grid or a tree grid is its cells' table whether or
            // not it declares a shape: without one its extent is unbounded,
            // and an outer table's shape does not bound its cells.
            let own_table = node.config.published_table_shape().or_else(|| {
                matches!(
                    node.config.role(),
                    flui_semantics::SemanticsRole::Table
                        | flui_semantics::SemanticsRole::Grid
                        | flui_semantics::SemanticsRole::TreeGrid
                )
                .then(|| TableShape::new(SetSize::Unknown, SetSize::Unknown))
            });
            if let Some(scope) = own {
                sets.push(scope);
            }
            if let Some(table) = own_table {
                tables.push(table);
            }
            for child in &mut node.children {
                visit(child, sets, tables);
            }
            if own_table.is_some() {
                tables.pop();
            }
            if own.is_some()
                && sets
                    .pop()
                    .expect("BUG: the scope pushed for this node is on top")
                    .omit_total
            {
                node.config.omit_collection_total();
            }
        });
    }
    visit(root, &mut Vec::new(), &mut Vec::new());
}

/// Whether a member's position belongs to the nearest set above it: a lazy
/// host's stamp to that host, an authored position to an authored set.
fn member_matches(origin: Option<SemanticsIndexOrigin>, nearest: Option<&SetScope>) -> bool {
    let Some(scope) = nearest.filter(|scope| matches!(scope.size, SetSize::Known(_))) else {
        return false;
    };
    match origin {
        Some(SemanticsIndexOrigin::Collection(host)) => scope.host == Some(host),
        Some(SemanticsIndexOrigin::Authored) | None => {
            scope.origin == SemanticsCollectionOrigin::Authored
        }
    }
}

/// Clear the places `config` declares that its nearest set and table cannot
/// hold. Assembly re-runs on every mark, so a drop is logged at debug level
/// only, where a warning would repeat.
fn hold_places_to_scopes(
    config: &mut SemanticsConfiguration,
    set: Option<SetScope>,
    table: Option<TableShape>,
) {
    if let Some(index) = config.position_in_set()
        && let Some(SetSize::Known(size)) = set.map(|set| set.size)
        && index >= size
    {
        tracing::debug!(
            index,
            size,
            "a semantics position is past its set's size; not published"
        );
        config.clear_position_in_set();
    }
    if let Some(cell) = config.cell_position()
        && !cell_fits(cell, table)
    {
        tracing::debug!(
            ?cell,
            ?table,
            "a semantics cell does not lie inside its table; not published"
        );
        config.clear_cell_position();
    }
    if let Some(row) = config.row_index()
        && !row_fits(row, table)
    {
        tracing::debug!(
            row = row.get(),
            ?table,
            "a semantics row is not one of its table's; not published"
        );
        config.clear_row_index();
    }
}

/// Whether a cell, with its spans, lies inside the table it sits in. A known
/// extent bounds it; an unknown one cannot be exceeded.
fn cell_fits(cell: flui_semantics::CellPosition, table: Option<TableShape>) -> bool {
    let within = |first: NonZeroUsize, span: NonZeroUsize, extent: SetSize| match extent {
        SetSize::Known(count) => first
            .get()
            .checked_add(span.get() - 1)
            .is_some_and(|last| last <= count),
        _ => true,
    };
    table.is_none_or(|table| {
        within(cell.row(), cell.row_span(), table.rows())
            && within(cell.column(), cell.column_span(), table.columns())
    })
}

/// Whether a row is one of the rows of the table it sits in.
fn row_fits(row: NonZeroUsize, table: Option<TableShape>) -> bool {
    table.is_none_or(|table| match table.rows() {
        SetSize::Known(count) => row.get() <= count,
        _ => true,
    })
}

/// Whether `config` declares a set or a table, whose members' places
/// [`reconcile_collections`] holds to it.
pub(super) fn declares_collection_scope(config: &SemanticsConfiguration) -> bool {
    config.size_of_set().is_some() || config.table_shape().is_some()
}

pub(super) fn has_collection_metadata(root: &BuiltSemanticsNode<'_>) -> bool {
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        if declares_collection_scope(&node.config) {
            return true;
        }
        pending.extend(&node.children);
    }
    false
}
