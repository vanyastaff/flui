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
key — the same shape `AnimatedSwitcher`'s `KeyedEntry` already uses) whose key is the pair
`(row identity, cell identity)`. Each half is the corresponding key when there is one and a
position otherwise: a row contributes its own key or its **ordinal among the unkeyed rows**, and a
cell contributes its own key or its column. `TableRow` gains an optional local key.

Equality is semantic, never by hash. `CellKey::key_eq` delegates to each half's `ViewKey::key_eq`,
because the reconciler treats a hash as a bucket and settles equality semantically; comparing
hashes here would let two rows whose keys collide trade elements. A key's hash is folded with a
constant so it cannot land in the position space, and the row's half is rotated so `(row a, cell b)`
stays apart from `(row b, cell a)`.

Keeping a cell's own key in its identity is what preserves the behaviour cells had as plain
siblings: two keyed cells that swap columns within a row keep their elements, because the wrapper
follows the cell instead of pinning it to a column number.

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
- Still open, and tracked in #544: `TableCellVerticalAlignment::IntrinsicHeight`, and the
  baseline/`textBaseline` pairing as a type-level invariant rather than the recorded degradation it
  is today.

## Amendment (2026-09-04): a ragged grid is repaired where it is supplied

Row lengths that disagreed were two `debug_assert!`s and nothing else — one in
`Table::update_render_object`, one in `RenderTable::perform_layout` — so a release build carried
the ragged rows all the way down. The render object floor-divides for its row count, which left a
trailing partial row that layout skipped, hit-test ignored, and paint was still handed. `DataTable`
was worse: `row.cells[col_index]` panics with an index-out-of-bounds in release, no assert
involved, which is a public API that panics on caller data.

`Table::new` and `DataTable::new` now square their rows up where the caller supplies them — a short
row is padded with empty cells, a long row's extras are dropped, against the first row's cell count
and `columns.len()` respectively — and each warns once naming the lengths involved. Both
`debug_assert!`s are gone because the state they described can no longer be constructed.
`RenderTable` keeps a release-safe warning in place of its own assertion and bounds `paint` to the
grid its last layout positioned, so a direct consumer of the lower layer gets the same
self-consistency between what is drawn and what is touchable.

**Why repair rather than reject.** An earlier design (recorded in the issue's spec as D1/D4) made
the invalid grid unrepresentable: a `TableRows` collection with a fallible `push`, and a
`RenderTable` owning `Vec<Option<RenderId>>` sized `columns * rows`. Both were dropped during
implementation. The render-side half fights this crate's own recorded rule that render objects do
not own child-adoption bookkeeping, and a shadow copy of the child list can desync from the tree
that actually owns it — a worse failure than the one it prevents. The widget-side half would have
pushed a `Result` through 31 call sites, inside `build` methods that cannot propagate one, to
prevent a mistake the library can correct deterministically.

Repair with a warning is the rule this codebase already follows for caller configuration:
`RenderViewport::set_anchor` clamps an out-of-range anchor rather than asserting, and `RenderTable`
already degrades a baseline alignment with no text baseline, both with a recorded rationale and a
green test. Flutter asserts on all of these; the divergence is deliberate, and the warning is what
keeps it from being silent.

Pinned by `table_pads_a_short_row_and_drops_a_long_rows_extra_cells` (the padded cell holds its
slot instead of shifting the grid, and every row still contributes its height) and
`data_table_squares_up_a_row_that_does_not_match_its_columns` (red with the repair removed: an
index-out-of-bounds panic).
