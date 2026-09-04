# ADR-0055: Table rows carry identity

- **Status:** Accepted (2026-09-04)
- **Date:** 2026-09-04
- **Deciders:** @vanyastaff
- **Scope:** `crates/flui-widgets/src/layout/table.rs` (`Table`, `TableRow`, the private
  `KeyedCell`), `crates/flui-widgets/tests/parity/table_test.rs`.
- **Related:** [ADR-0050](ADR-0050-global-key-identity-and-frame-reservations.md) (key identity),
  [ADR-0052](ADR-0052-lazy-sliver-child-identity-and-recovery.md) (the salted-key precedent);
  issue #544.

## Context

`RenderTable`'s children are one flat, row-major list: the render object derives a cell's position
from its flat index (`row = index / column_count`). The `Table` view therefore flattened its rows
into that one list and the element tree reconciled it **by flat position**, so a rebuild that
changed the column count re-paired cells across row boundaries — the third cell of row 1 and the
first cell of row 2 are the same flat index in a 3-column and a 4-column table. A row inserted or
removed in the middle shifted every element after it.

Flutter solves this in the element: `_TableElement.update` (`widgets/table.dart`) partitions the old
rows into keyed rows (matched globally by key) and unkeyed rows (matched **by sequence among the
unkeyed rows only**), then reconciles cells positionally *within* each matched row.

Porting that shape directly would need a component element with N children, which this element model
does not have: every non-render `ElementKind` is `Element<V, Single, …>`
(`crates/flui-view/src/element/kind.rs`), and `StatelessView::build` returns exactly one view. It
would mean a new element kind, a new multi-child component trait, blanket impls across the
behavior/dispatch surface, and an FR-036 `dyn`-allowlist entry.

## Decision

**Give each cell the identity of its row, and let the existing flat keyed reconciler do the rest.**

`Table` wraps every cell in one concrete render-less view (`KeyedCell`, a `StatelessView` with a
key — the same shape `AnimatedSwitcher`'s `KeyedEntry` already uses) keyed by
`CellIdentity { keyed, row, column }`, where `row` is the row's own key hash when it has one and its
**ordinal among the unkeyed rows** otherwise, and `keyed` keeps the two spaces apart so a hash can
never collide with an ordinal. `TableRow` gains an optional local key.

This reproduces the reference's observable behaviour through machinery that already exists:

- a keyed row keeps its cells' elements wherever it moves to;
- unkeyed rows are matched among themselves, so a keyed row moving past them leaves them alone —
  the reason Flutter matches unkeyed rows by sequence rather than by index;
- a row with no counterpart is disposed whole, and its cells are not re-paired with another row's.

The wrapper owns no render object, so each cell's render node still attaches directly to
`RenderTable` in row-major order, and a `TableCell` inside the wrapper is still the nearest
parent-data ancestor of that node. The wrapped list is built once with the table, not per visit, so
a rebuild costs no more cloning than the rows already did.

**Improvement over the reference:** Flutter needs a bespoke element with its own two-partition
matcher; FLUI needs a key. The behaviour is the same and there is one reconciler to reason about
instead of two.

**Why not per-row keys alone:** FLUI's reconciler is a port of Flutter's *generic*
`Element.updateChildren`, which **destroys** keyless middle children
(`crates/flui-view/src/tree/id_reconcile.rs`). Rows-as-children would therefore lose state on a
mixed keyed/unkeyed reorder — precisely the case keys exist for. Encoding the row's identity into
the cell key is what makes the generic reconciler behave like the bespoke one.

**Row keys are local.** `TableRow::key` rejects a `GlobalKey`: a row is not an element, so a global
key would have nothing to address. Key the cell instead.

## Consequences

- The `#[ignore]`d divergence pin
  (`changing_row_and_column_count_reuses_and_discards_cells_by_flat_position`) is retired: the test
  is un-ignored, renamed for what it now proves, and passes for the reference's own reason. The
  `table` parity family drops to zero pins and zero diverged cases.
- `TableRow`'s `Debug` no longer prints its cells (it prints their count and whether the row is
  keyed) because `BoxedView` has no `Debug`.
- Still open, and tracked in #544: validated construction (empty and irregular rows are debug-only
  assertions today, and `DataTable` indexes its cells behind one), `RenderTable`'s inferred
  `row_count = child_count / column_count` versus the reference's rectangular-by-construction
  `List<RenderBox?>`, and `TableCellVerticalAlignment::IntrinsicHeight`.
