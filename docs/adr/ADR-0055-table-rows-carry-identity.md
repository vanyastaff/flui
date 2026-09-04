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
- Still open, and tracked in #544: the baseline/`textBaseline` pairing as a type-level invariant
  rather than the recorded degradation it is today.

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

**Why repair rather than reject.** Issue #544 asks for "a fallible constructor or validated builder
for caller-controlled structural errors", and the spec designed one (D1/D4): a `TableRows`
collection with a fallible `push`, and a `RenderTable` owning `Vec<Option<RenderId>>` sized
`columns * rows`. This is a deliberate override of that ask, not an oversight, and the reason is
the issue's own next criterion.

**A fallible constructor here becomes a panic.** `Table::new` and `DataTable::new` are called
inside `build`, which returns a view and cannot propagate a `Result`. Every real call site would
therefore write `.expect(...)`, so a `Result` does not remove the panic — it relocates it from the
library to the caller and makes it unconditional in release, where today's `debug_assert!` at
least compiled out. Issue #544's fifth acceptance criterion is "No caller input causes a caught
internal layout panic"; a fallible constructor in a `build` method satisfies the first criterion by
violating the fifth. Repair satisfies both. This is the argument, not the migration cost — the
project explicitly allows breaking changes, so 31 call sites would not on its own be a reason.

The render-side half was dropped for an unrelated reason: it fights this crate's own recorded rule
that render objects do not own child-adoption bookkeeping. `RenderTable`'s children are attached by
the element tree, and a shadow copy of that list can desync from the tree that actually owns it —
a worse failure than the one it prevents.

**What repair costs.** A padded row is a caller mistake the developer now sees only in a log line,
where a panic would have been impossible to miss. That is the real trade, and it is why the warning
names the expected and found lengths rather than saying something went wrong.

Repair with a warning is the rule this codebase already follows for caller configuration:
`RenderViewport::set_anchor` clamps an out-of-range anchor rather than asserting, and `RenderTable`
already degrades a baseline alignment with no text baseline, both with a recorded rationale and a
green test. Flutter asserts on all of these; the divergence is deliberate, and the warning is what
keeps it from being silent.

Pinned by `table_pads_a_short_row_and_drops_a_long_rows_extra_cells` (the padded cell holds its
slot instead of shifting the grid, and every row still contributes its height) and
`data_table_squares_up_a_row_that_does_not_match_its_columns` (red with the repair removed: an
index-out-of-bounds panic).

## Amendment (2026-09-04): `TableCellVerticalAlignment::IntrinsicHeight`

The variant Flutter has and FLUI did not. A cell with it is measured with `Top`/`Middle`/`Bottom`,
so its own content contributes to how tall the row becomes, and is then re-laid-out tight to the
settled row height with `Fill` (`rendering/table.dart:1401-1405` and `:1437-1441`). The contrast
with `Fill` is which pass sees it: a row whose cells are all `Fill` has zero height because none
of them is measured, while a row whose cells are all `IntrinsicHeight` is as tall as its tallest
cell and every cell in it ends up that tall.

Four sites in `RenderTable`, one of which was not a `match`: `compute_dry_baseline` tested
`alignment == Baseline`, which silently absorbs any new variant. It happened to want the same
answer this variant needs, by luck rather than by design, so it is now a `match` and the next
variant added to this enum is a compile error there instead of a silent default.

`TableCellVerticalAlignment` is deliberately not `#[non_exhaustive]`, so adding a variant is a
breaking change for an external matcher. That is accepted: the enum is small, closed in concept,
and exhaustive matching on it is what just caught three of the four sites that needed updating.

Pinned by the ported oracle case
(`default_vertical_alignment_intrinsic_height_makes_each_row_as_tall_as_its_tallest_cell`, from
`widgets/table_test.dart`'s "Set defaultVerticalAlignment to intrinsic height and check their
heights", including its third assertion that rows differ from each other) and by two harness tests
that put `IntrinsicHeight` and `Fill` side by side on the same two cells: 90 tall versus zero.
