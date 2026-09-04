# #544 v2 — Table: rectangular by construction, rows identified by salted cell keys

Supersedes spec.md after its adversarial review (2026-09-04). What the review corrected, verified
by me before rewriting:

- **D4 was fiction.** `RenderTable` has had all four intrinsics since its first commit
  (`crates/flui-objects/src/layout/table.rs:823,843,863,901`; `compute_max_intrinsic_height`
  even ports Flutter's own `= getMinIntrinsicHeight` quirk with the citation). My "no intrinsics"
  claim came from a multi-pattern grep truncated by `head`. **D4 is deleted.**
- **D2 was not constructible.** There is no non-render N-child `ElementKind`: every component base
  is `Element<V, Single, …>` (`crates/flui-view/src/element/kind.rs:132,140,148,156,188`), and
  `rg "Variable, (Stateless|Stateful|Proxy)Behavior"` returns 0. The sliver-adaptor "composite"
  precedent I cited is `RenderVariable` with a real render id, not a component whose children
  attach to a distant ancestor. **D2 is replaced by salted cell keys.**
- **D1's baseline error would revert a recorded improvement.**
  `baseline_alignment_without_a_text_baseline_degrades_to_top_instead_of_asserting`
  (`crates/flui-widgets/tests/parity/table_test.rs:777`) is a *green* test pinning FLUI's
  deliberate divergence from Flutter's throw, with the rationale in the render object
  (`layout/table.rs:687-706`: library code must not panic on a config gap). **Reshaped.**
- **The panic the issue asks about is in `DataTable`, not `RenderTable`.**
  `crates/flui-material/src/data_table.rs:1058` indexes `row.cells[col_index]` behind a
  `debug_assert!` that is compiled out in release; `RenderTable::perform_layout` guards zero and
  floor-divides, so no index panic is reachable there (it truncates silently instead).

## Reference floor (Flutter, re-read)
- `_TableElement.update` (`widgets/table.dart:338-393`) partitions old rows into keyed (matched
  globally by key) and unkeyed (matched by **sequence among unkeyed rows only**); `updateChildren`
  runs *within* a row with slots `(columnIndex, rowIndex)`.
- `RenderTable._children` is `List<RenderBox?>` sized `columns * rows` **by construction**
  (`rendering/table.dart:397,423,462`), with `assert(_children.length == rows * columns)` at five
  sites and `setChild(x, y, null)` for a hole.
- `Table`'s constructor asserts row regularity, non-emptiness, unique row keys, and the
  baseline/textBaseline pairing — all debug-only.

## Decisions

### D1 — invalid configurations are unrepresentable, not merely rejected (ALT-4)
- `TableRows`: a collection whose first pushed row fixes the column count; `push(row) -> Result<(),
  TableError>` rejects an empty row and a row of the wrong length at the point the caller supplies
  it (`TableError::{EmptyRow, IrregularRowLength { expected, found }, DuplicateRowKey}`). `Table::new(TableRows)`
  is then **infallible** and no builder method returns `Result`.
- The baseline pairing becomes one atomic builder `baseline_alignment(TextBaseline)` that sets both
  fields; `default_vertical_alignment` takes a type that cannot express `Baseline`. The invariant
  is unrepresentable, the builder stays order-insensitive, and the recorded degradation for a
  render object configured directly (`flui-objects` is a lower layer and stays permissive) is
  preserved — the green parity test keeps passing unchanged.
- The fallible surface also goes where the release panic actually is: `DataTable`/`DataRow` build
  through `TableRows`, so `data_table.rs:1058` cannot index out of bounds.

### D2 — row identity through salted cell keys (ALT-1)
`Table::visit_child_views` keeps the flat row-major list, but wraps each cell in one concrete
keyless-proxy view keyed by `(row_identity, column_index)`, where `row_identity` is the row's key
if it has one and its **ordinal among unkeyed rows** otherwise. The existing flat keyed reconciler
then reproduces `_TableElement.update` exactly, including the unkeyed-sequential rule that a naive
per-row port would get wrong: FLUI's reconciler is `Element.updateChildren`, which **destroys**
keyless middle children (`crates/flui-view/src/tree/id_reconcile.rs:229-238`), so rows-as-children
would lose state on a mixed keyed/unkeyed reorder.
- `TableRow` gains `key: Option<Box<dyn ViewKey>>` and `.key(...)`.
- Prerequisites to verify in the first commit: the wrapper is a single concrete type so
  `can_update_by_id`'s `view_type_id` check (`id_reconcile.rs:434`) does not split on cell type;
  and `apply_ancestor_parent_data` still finds a `TableCell` config first (`element_tree.rs:867`).
- Render-child **order** was never the risk: `synchronize_render_children`
  (`tree/element_tree.rs:1128-1272`, armed at `id_reconcile.rs:393-401`, run once per build drain
  at `owner/build_owner.rs:1495`) is a whole-tree DFS in slot order, with a regression test
  (`crates/flui-widgets/tests/component_child_ordering.rs`).

### D3 — `TableCellVerticalAlignment::IntrinsicHeight` (unchanged, the one decision that survived)
Grouped with top/middle/bottom for row-height measurement, with `Fill` in the offset pass, `None`
in `compute_dry_baseline`. Blast radius is three exhaustive matches in one file
(`flui-objects/src/layout/table.rs:682,734,800`) plus one equality comparison at `:965` that
absorbs a new variant silently — it happens to give Flutter's answer, so make it a `match` so the
next variant cannot pass unnoticed. The enum is not `#[non_exhaustive]`: adding a variant is
semver-breaking for external matchers, which is allowed and recorded.

### D4 — REASSESSED during implementation (2026-09-04): repair at construction, not a new render-side shape

What follows was the plan; it is kept for the reasoning, but it is NOT what shipped. Two things
turned up while building it:

1. **The render object cannot own the shape.** `RenderTable`'s children are attached by the
   element tree, not held by the render object; `crates/flui-objects/src/layout/table.rs`'s own
   module doc records that as deliberate ("render objects never own their own child-adoption
   bookkeeping"). A `Vec<Option<RenderId>>` field would shadow that ownership and could desync
   from it, which is a worse failure than the one it prevents.
2. **The ragged grid was already unreachable in debug and reachable in release, from BOTH ends.**
   `Table::update_render_object` and `RenderTable::perform_layout` each carried a `debug_assert!`
   and nothing else, so a release build carried the ragged rows all the way down: the render
   object floor-divides for its row count, leaving a trailing partial row that layout skipped,
   hit-test ignored, and paint was still handed. `DataTable` was worse still — `row.cells[col_index]`
   panics with an index-out-of-bounds in release, with no assert involved.

What shipped instead squares the rows up where the caller supplies them: `Table::new` pads a short
row and drops a long row's extras against the first row's cell count, `DataTable::new` does the
same against `columns.len()`, and both warn once naming the lengths. `RenderTable` keeps a
release-safe warning in place of its `debug_assert!` and bounds `paint` to the grid it laid out, so
the lower layer stays self-consistent for a direct consumer.

That is repair rather than rejection, which is this library's established rule for caller
configuration — `RenderViewport::set_anchor` clamps an out-of-range anchor, and `RenderTable`
already degrades a baseline alignment with no text baseline, both with a recorded rationale and a
green test. It is also what keeps `Table::new` usable inside a declarative `build`: `TableRows`
with a `Result` would have pushed an `.expect()` through 31 call sites to prevent a mistake the
library can correct deterministically. Flutter asserts; the divergence is deliberate and the
warning is what keeps it from being silent.

The original plan, for the record:

### D4 (superseded) — the grid is rectangular by construction (ALT-2, replaces the deleted intrinsics work)
`RenderTable` takes `columns` + `Vec<Option<RenderId>>` sized `columns * rows`, installed
atomically (Flutter's `setFlatChildren`), instead of inferring `row_count = child_count /
column_count` at six sites (`:621,778,829,849,869,934`). A hole becomes representable — today a
missing cell shifts the whole grid — and the silent truncation of a partial trailing row, which
`paint_children` still paints at a stale offset, becomes impossible. This is an unrecorded
protocol-level divergence from the reference; the ADR records it either way.

## Slices (order fixed: prove the bet first)
- **S1** D2 + un-ignore the pin. Smallest change that tests the architectural bet.
- **S2** D4 (rectangular child list) + the partial-row pins.
- **S3** D1 (`TableRows`, the atomic baseline builder, `DataTable` migration) — the widest breaking
  migration, last, once the shape is settled. Census: 33 `Table::new` + 49 `TableRow::new` sites,
  of which 3 are production, all inside `DataTable::build`.
- **S4** D3.
Record: ADR-0055 "Table rows carry identity and the grid is rectangular by construction".

## Gate list
`crates/flui-widgets/tests/{table.rs,parity/table_test.rs}`; `parity/manifest.toml` in **three**
places when the pin is un-ignored (the target row's `tests`, the `[[targets.pins]]` entry, and
both `[summary]` and `[summary.families."table"]` — `parity_inventory.rs:531-560,891-945` asserts
set equality and recomputes the counts); `crates/flui-material/src/data_table.rs` + its tests;
`crates/flui-objects/tests/render_object_harness.rs` (RenderTable harness, including the intrinsic
pins at ~10936); `crates/flui-rendering/src/testing/{tree,parent_data}.rs`.
