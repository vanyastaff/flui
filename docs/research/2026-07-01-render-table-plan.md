# RenderTable + Table — plan (oracle-verified)

Core.2 catalog closure (Medium priority, self-contained). Unlike Flow, this needs **no new pipeline primitive** — Table is layout-time positioned, confirmed below. The real risk is entirely in porting a genuinely tricky multi-pass float algorithm, plus cleaning up a small pre-existing type-debt item that sits directly in this feature's path.

## Headline: no new primitive; the risk is the column-width algorithm's exact pass order, plus one pre-existing type fix

`RenderTable` positions children **during layout** (`childParentData.offset = Offset(positions[x], rowTop + …)`, oracle `table.dart:1399,1425-1441`) and paints them with a plain `context.paintChild(child, childParentData.offset + offset)` (`table.dart:1508-1514`) — no delegate, no paint-time transform, no replay trick. `hitTestChildren` (`table.dart:1453-1473`) iterates the flat child list in reverse and uses `addWithPaintOffset`. This is **exactly** `RenderStack`'s shape (`crates/flui-objects/src/layout/stack.rs`), not `RenderFlow`'s: `hit_test` is `ctx.hit_test_child_at_layout_offset(i)` in reverse index order, confirmed available at `crates/flui-rendering/src/context/hit_test.rs:169`. No `PaintCx::with_transform`, no `FlowPaintingContext`-style replay.

The real risk is two-fold:

1. **`_computeColumnWidths`** (`table.dart:1070-1236`) — a 4-pass float algorithm (ideal widths → flex growth → underflow growth → maxWidth-driven two-round shrink) that must be reused, byte-for-byte in formula, by `perform_layout`, `compute_dry_layout`, **and** (nested) `compute_min_intrinsic_height`. FLUI already has everything needed to write this ONCE, generic over two closures: `BoxLayoutContext::child_min_intrinsic_width`/`child_max_intrinsic_width` (`crates/flui-rendering/src/context/layout.rs:402-420`), `BoxDryLayoutCtx`'s equivalents (`context/intrinsics.rs:399-404`), and `BoxIntrinsicsCtx`'s (`context/intrinsics.rs:270-275`) all share the identical `(index, f32) -> f32` shape — so no chicken-and-egg problem exists for the `IntrinsicColumnWidth` branch (querying a cell's intrinsic width from within `perform_layout`, before any cell is really laid out, is already supported). This mirrors `RenderStack`'s `compute_size`/`measure`-closure pattern (`stack.rs:350-398`).
2. **Pre-existing type debt sitting directly in scope.** FLUI already has TWO independently-defined `TableCellVerticalAlignment` enums — `flui_types::layout::table::TableCellVerticalAlignment` (`crates/flui-types/src/layout/table.rs:42-60`, 5 variants) and `flui_rendering::parent_data::table_text::TableCellVerticalAlignment` (`crates/flui-rendering/src/parent_data/table_text.rs:33-50`, 5 variants) — both tagged `// PORT-CHECK-OK-SP3: pre-existing parallel definition; consolidation tracked`. Worse, the already-existing `TableCellParentData.vertical_alignment` field (`table_text.rs:29`) is **non-optional** (defaults `Top`), where Flutter's is `TableCellVerticalAlignment?` (defaults `null` = "defer to `RenderTable.defaultVerticalAlignment`", `table.dart:20`). Building on the type as-is silently breaks "an unset cell follows a later change to the table's default" — this must be fixed as part of this feature, not deferred.

## 1. Existing FLUI scaffolding already in place (surprisingly much)

- `TableColumnWidth` already exists, `crates/flui-types/src/layout/table.rs:6-38` — closed 4-variant enum (`Fixed(f32)`, `Flex(f32)`, `Intrinsic`, `Fraction(f32)`) vs. oracle's 6 types (+ `MaxColumnWidth`/`MinColumnWidth` combinators; oracle's `IntrinsicColumnWidth` also carries an optional flex). Zero consumers today (grep-confirmed) — safe to extend later; first slice keeps the enum's existing shape (see §6).
- `TableCellParentData` already exists, `table_text.rs:14-116` (`offset`, `x: usize`, `y: usize`, `vertical_alignment`). Zero consumers today. **This is the parent data RenderTable should use directly — no new parent-data type needed** (§2).
- `flui-rendering/src/testing/parent_data.rs:24-58`'s `ParentDataSeed` enum has no `Table` variant yet — small additive harness infra needed (mirrors `Stack`/`Flex`'s `with_stack_parent_data`/`with_flex_parent_data`, `flui-rendering/src/testing/tree.rs:134-141`).
- No `TableBorder` type anywhere in FLUI — needs fresh authoring, but Canvas already has every primitive it needs: `draw_line`/`draw_path`/`draw_rect`/`draw_drrect` (`crates/flui-painting/src/canvas/drawing.rs`) and `Paint::stroke`/`Paint::fill` (`crates/flui-types/src/painting/paint.rs:109-146`) — `table_border.dart`'s algorithm ports almost mechanically, unlike Flow's missing transform primitive.
- Row decorations: `flui_painting::paint_box_decoration(canvas, rect, &BoxDecoration<Pixels>)` already exists as a **pure stateless fn** (`crates/flui-painting/src/decoration.rs:26`, already consumed by `RenderDecoratedBox`, `crates/flui-objects/src/proxy/decorated_box.rs:116`) — no `BoxPainter`/`ImageConfiguration` caching to port at all; FLUI's decoration model is already simpler than Flutter's here.
- Baseline: `ctx.child_distance_to_actual_baseline(index, TextBaseline)` already exists (`context/layout.rs:364-374`) and is the exact primitive `RenderFlex`'s `CrossAxisAlignment::Baseline` already uses (`crates/flui-objects/src/layout/flex.rs:653`) — same call, applied per-cell-in-a-row instead of per-flex-child.
- Child-list mutation (oracle's `columns`/`rows` setters with child-migration, `setChild`, `setFlatChildren`, `addRow`, `setChildren`, `attach`/`detach`/`visitChildren`, `table.dart:397-961`) is **Flutter-element** machinery, not render-object machinery, in FLUI: `RenderStack`/`RenderFlow` never call `adopt_child`/`drop_child` themselves — that's owned by the generic `ElementKind::render_variable` multi-child element. `RenderTable` only needs a plain `column_count: usize` field; `row_count` is derived as `ctx.child_count() / column_count`. This removes ~250 oracle lines from scope — a different, already-existing layer owns it, not a cut.
- RTL column ordering (oracle's `textDirection` switch, `table.dart:1344-1359`): FLUI has an established, on-point precedent for deferring exactly this — `RenderWrap` documents "FLUI has not yet plumbed `TextDirection` into layout … always interpreted as LTR" (`crates/flui-objects/src/layout/wrap.rs:17-19`), and `RenderFlex` takes no `TextDirection` parameter at all. Table follows the same precedent.

## 2. Parent-data decision: reuse `TableCellParentData`, after one field-type fix

`RenderTable::ParentData = TableCellParentData`. Required fix (small, do first, zero consumers today so free/non-breaking): change `vertical_alignment: TableCellVerticalAlignment` → `vertical_alignment: Option<TableCellVerticalAlignment>` (default `None`), matching oracle exactly, and resolve the duplicate-enum debt by re-pointing this field at `flui_types::layout::TableCellVerticalAlignment` (the type the public `Table`/`TableCell` widget API should expose anyway, consistent with `TableColumnWidth` already living in `flui_types`), retiring `flui_rendering::parent_data::table_text::TableCellVerticalAlignment` (delete or alias) — closing the tracked `PORT-CHECK-OK-SP3` debt as part of this feature.

`x`/`y` are **not needed** for RenderTable's own layout logic (`row = index / column_count`, `col = index % column_count` is always derivable from the flat index FLUI already threads through `BoxLayoutContext<Variable, _>`) — RenderTable still **writes** them via `ctx.child_parent_data_mut(i)` purely for API/diagnostics parity with the oracle's public getters (`table.dart:1376-1377`), a deliberate reuse decision, not an oversight (mirrors `RenderFlow`'s own explicit parent-data note in its module doc). No `container: ContainerParentDataMixin` field needed — confirmed vestigial/unread by FLUI's Vec-based Variable arity, and `TableCellParentData` already correctly omits it.

## 3. `RenderTable` (`crates/flui-objects/src/layout/table.rs`)

`type Arity = Variable; type ParentData = TableCellParentData;`

Fields: `column_count: usize`, `column_widths: HashMap<usize, TableColumnWidth>`, `default_column_width: TableColumnWidth` (default `Flex(1.0)`), `default_vertical_alignment: TableCellVerticalAlignment` (default `Top`), `text_baseline: Option<TextBaseline>`, `border: Option<TableBorder>`, `row_decorations: Vec<Option<BoxDecoration<Pixels>>>`, plus cached-for-paint/hit-test geometry rebuilt every layout: `row_tops: Vec<f32>` (len `row_count+1`), `column_lefts: Vec<f32>`, `table_width: f32`, `baseline_distance: Option<f32>`. No `rows` field — always `ctx.child_count() / column_count`.

**`compute_column_widths`** — the core private helper, generic over child-intrinsic-width closures so it's usable from `perform_layout`/`compute_dry_layout`/the nested `compute_min_intrinsic_height` call (mirrors `RenderStack::compute_size`'s `measure` closure, `stack.rs:350-398`): exact 4-pass port of `table.dart:1070-1236` —
- **Pass 1** (`L1082-1120`): per column, resolve `max_intrinsic_width`/`min_intrinsic_width` by `TableColumnWidth` variant (`Fixed`→value/value; `Flex`→0/0 +flex=value; `Fraction`→`value*max_width` if finite else 0 — note oracle does *not* clamp the fraction to 0..1 despite FLUI's own doc comment claiming it does; follow FLUI's documented clamp behavior and flag the divergence; `Intrinsic`→ loop all cells in the column via `child_min/max_intrinsic_width(idx, f32::INFINITY)`, taking the max — the one variant that touches children). Accumulate `table_width`, `unflexed_table_width`, `total_flex`.
- **Pass 2** (`L1124-1153`): if `total_flex > 0`, grow flexed columns toward `target_width` (`max_width` if finite else `min_width`): `flexed_width = remaining_width * flex[x] / total_flex`, only grows. Else grow all columns equally toward `min_width` (mutually exclusive branches, oracle's own comment).
- **Pass 3** (`L1168-1234`): if `table_width > max_width`, shrink in two rounds — proportional shrink of flexed columns toward their floors (re-accumulating `total_flex` as columns hit floor, oracle's `newTotalFlex`), then equal-delta shrink of remaining non-floored columns. Use a local `const EPSILON: f32 = 1e-6;` (mirrors `wrap.rs:33-36`'s `PRECISION_TOLERANCE` convention, the more directly-analogous sibling file, over `sliver_geometry.rs`'s `flui_foundation::EPSILON_F32`).

`perform_layout`: resolve `column_lefts`/`table_width` from `widths` (LTR only, §6); then per row, a **measure pass** (resolve `vertical_alignment.unwrap_or(default)`, branch baseline/top/middle/bottom/fill exactly per `table.dart:1378-1410`, writing `x`/`y` via `ctx.child_parent_data_mut`) followed by a **position pass** (`table.dart:1418-1444`; `Fill` gets a second tight-height layout call). `compute_dry_layout`: same column/row-height pass without positioning (`L1290-1327`); returns `Size::ZERO` when any cell resolves to `Baseline` (oracle asserts this is unsupported for dry layout, `L1305-1312`).

Intrinsics: `compute_min/max_intrinsic_width` (`L969-997`) do **not** call `compute_column_widths` — just sum each column's own min/max against `container_width = INFINITY`. `compute_min_intrinsic_height` (`L999-1021`) **does** need the full column-width pass, then sums per-row `max(child.getMaxIntrinsicHeight(widths[x]))` — note MAX even inside the MIN function, oracle's own quirk, preserve exactly; `compute_max_intrinsic_height` is literally `compute_min_intrinsic_height` (`L1023-1026`).

`paint`: row decorations (via `paint_box_decoration` per row rect) → `ctx.paint_children()` (row-major order == paint order) → border (`table.dart:1475-1526` order, exactly). `hit_test`: `if !ctx.is_within_own_size() { return false }`; reverse-index loop calling `ctx.hit_test_child_at_layout_offset(i)` — directly `RenderStack`-shaped, confirming the task's "should be simple" expectation, no replay needed.

## 4. `TableBorder` (new type, `crates/flui-types/src/styling/table_border.rs`)

6 `BorderSide<Pixels>` fields (top/right/bottom/left/horizontal_inside/vertical_inside), `TableBorder::all`/`::symmetric`/`Default`. New paint fn `paint_table_border(canvas, rect, rows: &[f32], columns: &[f32], border)` (`flui-painting`, mirrors `paint_border`'s placement): interior vertical lines via one `Path`+`draw_path(&Paint::stroke(...))` call (`table_border.dart:296-311`), then horizontal (`L314-329`), then outer border reusing the existing 4-filled-edge-rect logic already in `decoration.rs:251-`+ (uniform/non-uniform branches) — **`border_radius` deferred** (§6).

## 5. `Table`/`TableRow`/`TableCell` widgets (`crates/flui-widgets/src/layout/table.rs`)

Placement: `layout/`, registered next to `flow`/`list_body`. `Table { rows: Vec<TableRow>, column_widths, default_column_width, border, default_vertical_alignment, text_baseline }`, `TableRow { decoration: Option<BoxDecoration<Pixels>>, cells: Vec<BoxedView> }`. `visit_child_views` flattens rows row-major — the exact order `RenderTable`'s flat child list expects. Reconciliation uses the **same generic flat-`ViewSeq` `ElementKind::render_variable`** machinery `Stack`/`Flow` already use (`generic_render_view_element!`), not Flutter's bespoke `_TableElement` per-row keyed diffing — the plan's one deliberate architecture-level cut, documented in §6. `TableCell` is a `ParentDataView<ParentData = TableCellParentData>` mirroring `Positioned` exactly (`crates/flui-widgets/src/stack/positioned.rs:100-129`): sets only `vertical_alignment = Some(...)`; `x`/`y`/`offset` are inert defaults since `RenderTable` overwrites them unconditionally during layout.

## 6. Catalog guard (mandatory)

Add `"RenderTable"` to `RENDER_OBJECT_TYPES` + doc-table row + `harness_table_*` tests in `crates/flui-objects/tests/render_object_harness.rs`.

## 7. Tests

Unit: column-width pass-by-pass (Fixed-only, Flex-only, Fraction finite/infinite, Intrinsic-queries-real-cells, the adversarial 1px/flex-1000 vs 1000px/flex-1 shrink case straight from oracle's own doc comment); the `Option<>` fix's actual behavior (an unset cell follows a later `default_vertical_alignment` change; an explicit `Some(Top)` does not); dry-layout-returns-ZERO-on-baseline. Harness (real per-cell geometry, matching Flow/CustomPaint rigor — not just overall size): 2×2 grid with Fixed+Flex column mix asserting each cell's exact offset/size; paint-order via `display_commands()` (decoration → children → border); border line positions; hit-test per cell + miss outside bounds; baseline row alignment. Parity: column-width formulas vs. hand-computed Flutter-equivalent values for each sizing-mode combination.

## 8. Risk ranking

- **HIGH** — `compute_column_widths`'s exact 4-pass port, especially the two shrink-convergence loops' epsilon/`total_flex`-reaccumulation.
- **HIGH→MED** (de-risked by confirmed infra) — sharing one `compute_column_widths` across `perform_layout`/`compute_dry_layout`/nested `compute_min_intrinsic_height`: all three context types (`BoxLayoutContext:402-420`, `BoxDryLayoutCtx:399-404`, `BoxIntrinsicsCtx:270-275`) already expose matching `child_min/max_intrinsic_width(index, extent)` — no missing primitive, just closure-wiring transcription risk.
- **MED** — the `TableCellVerticalAlignment` duplicate-enum consolidation + `Option<>` fix — small diff, easy to skip, and skipping it silently reintroduces a real correctness bug.
- **MED** — `TableBorder`'s paint-order/uniform-vs-non-uniform branch split — mechanical but fresh transcription, not an availability gap.
- **MED** — baseline row alignment's two-pass measure/position split interacting with `Fill`'s second-layout-pass rule.
- **LOW** — hit_test (confirmed `RenderStack`-shaped); widget scaffolding (direct `Stack`/`Positioned` clone); catalog guard; row-decoration painting (reuses existing stateless fn verbatim).

## 9. Deferred, documented

- `MaxColumnWidth`/`MinColumnWidth` combinators and `IntrinsicColumnWidth`'s optional flex — `TableColumnWidth`'s existing 4-variant enum has zero consumers today; extending it later is non-breaking.
- `TableCellVerticalAlignment::IntrinsicHeight` (oracle's 6th variant) — a Fill/Top hybrid; the 5 variants FLUI's enum already has cover the common cases.
- RTL column ordering — follows `RenderWrap`'s and `RenderFlex`'s established, documented LTR-only precedent.
- `TableBorder.border_radius` — ship zero-radius uniform + non-uniform paths only.
- Flutter's `_TableElement` per-row keyed reconciliation (`TableRow.key`) — FLUI's `Table` uses the same generic flat-`ViewSeq` reconciliation as `Stack`/`Flow`.
- Semantics (`assembleSemanticsNode` etc.) — no semantics tree in FLUI yet, consistent catalog-wide.
- `ImageConfiguration`/per-row `BoxPainter` caching — N/A, FLUI's decoration painting is already stateless.
- RenderTable's own child-adoption API surface — owned by FLUI's generic multi-child element layer already.

### Critical Files for Implementation
- crates/flui-objects/src/layout/table.rs (new — RenderTable)
- crates/flui-rendering/src/parent_data/table_text.rs (existing TableCellParentData — needs the Option<> fix + enum consolidation)
- crates/flui-types/src/layout/table.rs (existing TableColumnWidth/TableCellVerticalAlignment enums)
- crates/flui-objects/src/layout/stack.rs (closest Variable-arity + generic-closure precedent)
- crates/flui-widgets/src/layout/table.rs (new — Table/TableRow/TableCell widgets)
