# ADR-0012: Containers report their own baseline by eager-recording during layout, not via a new child channel (closes D-B)

*`RenderFlex` (and other multi-child containers) report their own actual baseline to a baseline-aligned parent by computing it during `perform_layout` — using the child-baseline channel the layout context **already** exposes — and serving it from a `&self` field. No new child-query channel is added to `compute_distance_to_actual_baseline` / `actual_baseline_raw`.*

---

- **Status:** Accepted (chief-architect ARCH-GATE: ACCEPTABLE; cross-checked against `.flutter/flutter-master/` oracle + the ADR-0010/0011 infra)
- **Date:** 2026-06-30
- **Deciders:** chief-architect; consult api-design-lead (confirming the *non-*change to the public `compute_distance_to_actual_baseline` signature), qa-lead (nested-baseline harness test)
- **Supersedes / relates to:** **Resolves ADR-0010's flagged second-decision D-B** (`docs/adr/ADR-0010-secondary-query-parent-data-accessor.md`, line 102 risk + line 124 D-B) and unblocks its **step 8** (flex baseline, line 116). Sibling of ADR-0011 (which closed D-C the same way ADR-0011 closed it — by reusing existing machinery rather than adding a channel). Builds on ADR-0010's landed parent-data accessor.
- **Gate:** ARCH-GATE (this doc) → then per-slice DEV-GATEs.

---

## Context

A container's **own** baseline is derived from its children's baselines. Flutter's `RenderFlex.computeDistanceToActualBaseline` dispatches on axis:

```dart
// flex.dart:806-812
@override
double? computeDistanceToActualBaseline(TextBaseline baseline) {
  return switch (_direction) {
    Axis.horizontal => defaultComputeDistanceToHighestActualBaseline(baseline),
    Axis.vertical   => defaultComputeDistanceToFirstActualBaseline(baseline),
  };
}
```

and the two `RenderBoxContainerDefaultsMixin` helpers walk the children, each reading the **committed** child baseline plus the child's **laid-out offset**:

```dart
// box.dart:3318-3330  — FIRST child (child-LIST order) that has a baseline
double? defaultComputeDistanceToFirstActualBaseline(TextBaseline baseline) {
  ChildType? child = firstChild;
  while (child != null) {
    final childParentData = child.parentData! as ParentDataType;
    final double? result = child.getDistanceToActualBaseline(baseline);
    if (result != null) { return result + childParentData.offset.dy; }
    child = childParentData.nextSibling;
  }
  return null;
}

// box.dart:3336-3348  — HIGHEST baseline = MIN over children of (child baseline + offset.dy)
double? defaultComputeDistanceToHighestActualBaseline(TextBaseline baseline) {
  BaselineOffset minBaseline = BaselineOffset.noBaseline;
  ChildType? child = firstChild;
  while (child != null) {
    final childParentData = child.parentData! as ParentDataType;
    final BaselineOffset candidate =
        BaselineOffset(child.getDistanceToActualBaseline(baseline)) + childParentData.offset.dy;
    minBaseline = minBaseline.minOf(candidate);
    child = childParentData.nextSibling;
  }
  return minBaseline.offset;
}
```

In Flutter the *value* is produced **lazily on demand and memoized** — `getDistanceToActualBaseline` runs `computeDistanceToActualBaseline` through the layout cache (`box.dart:2509-2520`), reading each child's *stored* `parentData.offset.dy`. Rule #1 protects the observable baseline **value** (highest / first, per the formulas above), not Flutter's caching strategy.

### The gap in FLUI (verified)

`RenderFlex` has **no** `compute_distance_to_actual_baseline` override, so it inherits the default:

```rust
// traits/render_box.rs:281-283 — the default, no child channel
fn compute_distance_to_actual_baseline(&self, _baseline: TextBaseline) -> Option<f32> { None }
```

So a flex reports **no** baseline to its parent. This breaks nested baseline alignment: a `Row(CrossAxisAlignment::Baseline)` (or a `RenderBaseline`) containing a flex cannot align to that flex's contents. Note flex *already consumes* child baselines for its own cross-axis `Baseline` alignment (`flui-objects/src/layout/flex.rs:538-553, 563-571` via `ctx.child_distance_to_actual_baseline`); the missing half is flex **reporting** its baseline upward.

### The channel flex needs already exists — the post-layout baseline path is mechanism (2), and it is read-only

During `perform_layout`, the borrowed-arena walk (mechanism (2) in ADR-0011's taxonomy) wires an actual-baseline callback for **every** box node:

```rust
// pipeline/owner/subtree_arena.rs:902-912  (wired into the erased ctx at :998)
let baseline_cb_owned = move |child_id, baseline| {
    arena_for_cb.get(child_id).and_then(|child_ptr| {
        let child_node: &RenderNode = unsafe { &*child_ptr.0 }; // SHARED reborrow, distinct laid-out node
        child_node.as_box().and_then(|entry| entry.render_object().actual_baseline_raw(baseline))
    })
};
```

`ErasedBoxLayoutCtx::child_distance_to_actual_baseline` routes through this callback (`protocol/box_protocol.rs:1148-1155`), and `BoxLayoutContext::child_distance_to_actual_baseline` exposes it to every container's `perform_layout` (`context/layout.rs:364-369`, `protocol/box_protocol.rs:534-545`). So **flex can already read its children's committed baselines inside its own `perform_layout`** — and it knows each child's offset there, because it computed and assigned it via `ctx.position_child(i, offset)` (`flex.rs:574-575`).

Two properties of this path matter for the decision:

1. **It is `&self`/read-only.** `actual_baseline_raw(&self, baseline)` takes `&self` (`traits/render_box.rs:631-633`, default `traits/render_object.rs:301-303`); the callback does a **shared** reborrow of a *distinct* laid-out child. Unlike `box_intrinsic_query_borrowed` (which takes `&mut` to write the intrinsic cache and therefore needs the `is_in_flight` gate — ADR-0011), the baseline read writes nothing and aliases nothing. **No `is_in_flight`, no new `unsafe`.**
2. **It is only reachable during a layout walk.** There is no standalone `box_actual_baseline` query (contrast `box_dry_baseline`, `query.rs:98`); `actual_baseline_raw` is called *only* from the layout-time callback above. A container is therefore always **already laid out** (children committed, offsets assigned) before any parent queries its baseline — the ordering the record-then-serve approach relies on is structurally guaranteed.

### FLUI already settled how containers report post-layout baselines — eager record, not on-demand

Every FLUI container that already reports a baseline does so by **computing it during `perform_layout` and storing it in a `&self` field**, then serving that field from `compute_distance_to_actual_baseline` — the exact opposite of Flutter's lazy-memoized model, and an intentional Rust-native divergence (FLUI stores layout outputs in the object, not in child parent-data):

| Container | Records during layout | Serves from | Source |
|---|---|---|---|
| `RenderBaseline` | own baseline is the fixed `baseline_offset` field; child offset in `child_offset` field | `Some(self.baseline_offset.get())` | `baseline.rs:96, 103-109` |
| Align / Center / Padding (`AligningShiftedBox`) | `record_child_baselines(ctx)` caches **both** kinds into `child_baselines: [Option<f32>; 2]` after positioning | `self.inner.actual_baseline(baseline)` | `shifted_box.rs:124-159`; `align.rs:184, 192-194`; `center.rs:243-245` |

`compute_distance_to_actual_baseline` in FLUI is `(&self, baseline) -> Option<f32>` with **no ctx**, precisely because the answer is expected to have been recorded during layout. Flex is the one multi-child container that never got this treatment. Closing D-B is making flex do what its siblings already do.

---

## Decision

**We adopt the established eager-record convention for flex (and future multi-child containers). We reject adding a child-query channel to `compute_distance_to_actual_baseline` / `actual_baseline_raw`.** D-B needs **zero new infrastructure** — no ctx, no raw-bridge change, no driver change, no trait-signature change. This reverses ADR-0010's tentative D-B lean ("mirror `dry_baseline_raw`'s `child_query`, ripples 5 overriders") now that the eager-record convention and the existing layout-time channel are confirmed.

### D-B1 — `compute_distance_to_actual_baseline` stays `(&self, baseline)`; no channel is added

The trait method keeps its current signature (`traits/render_box.rs:281`). The four existing overriders (`paragraph.rs:215`, `baseline.rs:103`, `align.rs:192`, `center.rs:243`) are untouched. `actual_baseline_raw` (`render_box.rs:631`, `render_object.rs:301`) and its layout-time callback (`subtree_arena.rs:902`) are untouched. This is the decisive property: the change is confined to **one file** (`flui-objects/src/layout/flex.rs`) and adds **no** new `pub` surface to `flui-rendering`.

### D-B2 — `RenderFlex` records its reported baseline (both kinds) during `perform_layout` and overrides `compute_distance_to_actual_baseline`

Add a field and populate it in the positioning loop that already runs (`flex.rs:555-578`), where both the per-child baseline (`ctx.child_distance_to_actual_baseline(i, kind)`) and the per-child offset (the `offset` passed to `ctx.position_child(i, offset)`) are in hand:

```rust
// RenderFlex, new field — index by TextBaseline (Alphabetic = 0, Ideographic = 1),
// mirroring AligningShiftedBox::child_baselines (shifted_box.rs:138-141).
reported_baseline: [Option<f32>; 2],
```

Compute per Flutter's formulas, in **child-list order** (`flex.rs`'s positioning loop is index order = list order):

- **Horizontal → highest** (`box.dart:3336-3348`): `min` over all children of `child_baseline_i + offset.dy_i`, where for a horizontal flex `offset.dy_i` is the *cross-axis* offset (`cross_offset`).
- **Vertical → first** (`box.dart:3318-3330`): the **first** child in list order with a baseline: `child_baseline_i + offset.dy_i`, where for a vertical flex `offset.dy_i` is the *main-axis* offset.

Both kinds (`Alphabetic`, `Ideographic`) are recorded because the querying parent chooses the kind — this is distinct from `self.text_baseline`, which only selects the kind for flex's *own cross-axis* `Baseline` alignment (`flex.rs:108`, `:539`). Then:

```rust
fn compute_distance_to_actual_baseline(&self, baseline: TextBaseline) -> Option<f32> {
    self.reported_baseline[baseline_index(baseline)]
}
```

**Nesting composes with no extra plumbing.** When an outer flex lays out an inner flex via `ctx.layout_child`, the inner flex records *its* baseline; when the outer flex later reads `ctx.child_distance_to_actual_baseline(inner_idx, kind)`, the layout-time callback calls `inner_flex.actual_baseline_raw(kind)` → the recorded field. Arbitrary depth works through the existing `baseline_cb` + eager-record, with only `&self` reborrows of distinct nodes.

**Cost, and how to bound it.** The eager model pays even when no parent ever queries the baseline (Flutter's lazy model does not). Per-layout cost is `O(children × kinds)` baseline reads, each `O(1)` for leaf children (a paragraph reads cached painter state) and recursive only for nested-container children (rare). Bound it: the **vertical/first** case early-stops at the first child with a baseline; the **horizontal/highest** case is genuinely all-children but reuses the reads flex may already do for cross-axis `Baseline` alignment (`flex.rs:541-549`) — fold both into one pass. This matches the always-on cost `AligningShiftedBox::record_child_baselines` already accepts for single-child boxes; for the niche where it is unwanted, a future lazy variant is possible but is over-engineering now.

### D-B3 — The dry half (`RenderFlex::compute_dry_baseline`) needs **zero new infrastructure** and can land **independently, NOW**

The dry baseline of a flex is a **separate** method from the post-layout one and is already fully expressible with the *existing* `BoxDryBaselineCtx`, which after ADR-0010/0011 exposes everything Flutter's `computeDryBaseline` uses:

| Flutter (`flex.dart:936-952`, helpers `:954-1025`, `:1027+`) | FLUI ctx accessor | Landed by |
|---|---|---|
| `ChildLayoutHelper.dryLayoutChild` | `ctx.child_dry_layout(i, c)` | pre-existing (`DryBaselineChildRequest::DryLayout`) |
| `child.getDryBaseline(cc, baseline)` | `ctx.child_dry_baseline(i, c, kind)` | pre-existing |
| `_getFlex(child)` (parent data) | `ctx.child_parent_data_as::<FlexParentData>(i)` | ADR-0010 |
| flex-child constraint intrinsics, if needed | `ctx.child_intrinsic(i, dim, extent)` | ADR-0011 |

Proof the two-kind dry-baseline path already works end-to-end: `RenderBaseline::compute_dry_baseline` (`baseline.rs:111-131`) already issues a **cross-kind** pair of `ctx.child_dry_baseline` calls (`requested` + `own`). So flex's dry baseline is a pure flex-side addition; it does **not** depend on this ADR's post-layout decision and **may land as a cheap, independent slice ahead of, alongside, or after D-B2.**

One constraint governs *how* it lands: dry baseline needs each child's **dry offset**, and per ADR-0010 D2 positioning lives only in `perform_layout` while `compute_sizes` is sizing-only. Flutter itself **duplicates** the positioning simulation in `_computeDryDistanceToHighestBaseline` / `_computeDryDistanceToFirstBaseline` (`flex.dart:954-1025`). FLUI should **not** duplicate: extract the offset/positioning math into a shared, ctx-free `compute_offsets(sizes, …)` (or have `compute_sizes` also surface the baseline-aligned offset, matching Flutter's `sizes.baselineOffset`, `flex.dart:944`) so `perform_layout`, `compute_distance_to_actual_baseline` recording, and `compute_dry_baseline` share one home (one fact, one place). This extraction is the only real work in the dry slice; it is flex-internal, still zero `flui-rendering` change.

---

## Consequences

**Positive**

- Closes D-B (flex reports its baseline; nested baseline alignment works) with a change confined to **one file** and **zero** new `pub` surface, `unsafe`, re-entrancy surface, or `is_in_flight` obligation.
- Flex joins the settled FLUI convention (`RenderBaseline`, `AligningShiftedBox`) instead of introducing a second, parallel way to report container baselines — one idiom, consistently applied.
- The dry half (D-B3) is decoupled and independently shippable, so it need not contend with the concurrent sliver work in `flui-objects`.
- No signature churn on a widely-implemented trait method; api-design-lead sign-off is trivial (the public contract is unchanged).

**Negative / Trade-offs**

- The eager model recomputes flex's baseline every layout even when unqueried (bounded above; matches the existing single-child convention). Accepted as an intentional divergence from Flutter's lazy-memoized model — the observable value is identical (rule #1 is satisfied on behavior).
- Flex now records **both** baseline kinds, a small always-on read cost folded into the existing positioning pass.
- The dry slice must extract shared positioning math to avoid the duplication Flutter tolerates — a small, well-scoped refactor, but it is the reason the dry half is *its own* slice rather than a one-liner.

**Follow-ups**

- ADR-0010 step 8 (flex baseline) is unblocked: post-layout via D-B2, dry via D-B3.
- Items #14/#7 (RenderBaseline cross-kind dry baseline) already use the two-kind dry-baseline path (`baseline.rs:127-130`); they are not blocked by this ADR and gain nothing further from it — noted to prevent false coupling.

---

## Alternatives Considered

| Option | Why rejected |
|---|---|
| **B — add a child-query channel to `actual_baseline_raw` / `compute_distance_to_actual_baseline`** (ADR-0010's tentative D-B lean: mirror `dry_baseline_raw`'s `child_query`) | Changes a widely-implemented trait method's signature (ripples the 4 existing overriders + the raw bridge + the driver), adds new `pub` surface, and would require the driver to *reconstruct child offsets post-layout* — which FLUI does not store in a child-channel-readable place (each container stashes offsets in its own fields, e.g. `RenderBaseline.child_offset`). It duplicates a capability the layout ctx already provides (`child_distance_to_actual_baseline`) and diverges from the convention Align/Center/RenderBaseline already established. Strictly more infrastructure for a strictly worse outcome. |
| **C — port Flutter's lazy-memoized model** (compute on first query, cache in the layout cache; give `actual_baseline_raw` `&mut` + an `is_in_flight`-gated child walk) | Imports the `&mut` aliasing hazard and `is_in_flight` obligation that ADR-0011 deliberately confined to the intrinsic borrowed-arena path — for a read that is currently `&self` and hazard-free. The eager model already yields the identical observable value. Reserve as a future optimization only if baseline recompute ever shows up in a profile. |
| **D — implement post-layout D-B2 and dry D-B3 as one atomic change** | Unlike ADR-0011's IntrinsicWidth fix (where real and dry were two halves of one behavior and had to stay in lockstep to preserve `dry==committed`), post-layout and dry baseline are independent methods with independent tests. Bundling them needlessly couples the cheap post-layout fix to the dry slice's positioning-extraction refactor, and increases collision risk with the concurrent `flui-objects` sliver work. Keep them separate. |

---

## Ordered implementation plan

All work is in `flui-objects/src/layout/flex.rs` (box layout — no overlap with the concurrent sliver edits; the implementer should still rebase onto the latest `flex.rs`). Zero `flui-rendering` changes.

**Slice A — post-layout container baseline (D-B1, D-B2):**

1. Add `reported_baseline: [Option<f32>; 2]` to `RenderFlex` (default `[None; 2]`); add a private `baseline_index(TextBaseline) -> usize` helper (mirror `shifted_box.rs:155-158`).
2. In `perform_layout`'s positioning loop (`flex.rs:555-578`), accumulate the reported baseline for both kinds from `ctx.child_distance_to_actual_baseline(i, kind)` + the child's assigned `offset.dy`: horizontal → `min` over all children (`box.dart:3336-3348`); vertical → first child in list order with a baseline (`box.dart:3318-3330`). Reuse the reads flex already does for cross-axis `Baseline` alignment. Reset to `[None; 2]` on the zero-child path.
3. Override `compute_distance_to_actual_baseline(&self, baseline)` to return `self.reported_baseline[baseline_index(baseline)]` (`flex.dart:806-812`).
4. **★ MILESTONE — nested-baseline harness proof.** `render_object_harness.rs`: add `harness_flex_reports_highest_baseline`. Tree: an outer `RenderBaseline::new(Alphabetic, 100.px)` wrapping a `RenderFlex::row()` (`CrossAxisAlignment::Start`) of two baseline-bearing children with **distinct** baselines (recommended self-contained shape: two `RenderBaseline` children over fixed-size leaves with `baseline_offset` 10 and 30, since `RenderBaseline` already reports its baseline — `baseline.rs:103-109`). Lay out; via `Probe::offset` assert the flex is shifted so its reported baseline (highest = `min(10, 30) = 10`) sits at 100 — i.e. `offset(flex).dy == 100 - 10`. This is **red before step 3** (flex → `None` → `RenderBaseline` falls back to child height, `baseline.rs:92-94`) and **green after**. Add a `RenderFlex::column()` analogue asserting the *first*-child baseline. Cross-check the expected values against `box.dart:3318-3348`.

**Slice B — dry container baseline (D-B3), independent, may land before or after Slice A:**

5. Extract flex positioning-offset math into a shared ctx-free helper (`compute_offsets(sizes, …)`), or extend `compute_sizes` to also yield the baseline-aligned offset (`flex.dart:944` `sizes.baselineOffset`); rewire `perform_layout` to it (behavior-preserving; existing flex harness tests stay green).
6. Implement `RenderFlex::compute_dry_baseline` using `ctx.child_dry_layout` + `ctx.child_dry_baseline` + `ctx.child_parent_data_as::<FlexParentData>` and the shared offset helper: baseline-aligned → the shared baseline offset; else highest/first per `flex.dart:947-951`.
7. Harness: `harness_flex_dry_baseline` asserting `run.dry_baseline(flex, c, kind)` equals the post-layout reported baseline from Slice A's tree (dry == committed), cross-checked against `flex.dart:936-1025`.

Slice A is the shippable milestone (steps 1-4). Slice B is a second, independent slice (steps 5-7).

---

## Maintainer-grade pre-code gate

**Verdict: ACCEPTABLE.** The design reuses the child-baseline channel the layout context already exposes (`context/layout.rs:364`) and the eager-record convention Align/Center/RenderBaseline already established (`shifted_box.rs:124-159`), rather than inventing a parallel on-demand channel; it adds **no** new `pub` surface, **no** `unsafe`, **no** re-entrancy surface, and **no** trait-signature change (the one real aliasing hazard — the `&mut` intrinsic borrowed-arena path — is deliberately not touched, and the baseline read stays `&self`); it keeps the flex sizing/positioning math in one home by extracting the offset helper before the dry slice reads it (one fact, one place); and it adds no dependency cycle (`flui-objects → flui-rendering` direction preserved). Forward view (2 years / 3 extensions): the same record-both-kinds-during-layout shape serves every future multi-child container that must report a baseline (table, flow, grid) with no context or trait surgery; the dry-baseline half rides the existing `BoxDryBaselineCtx` accessors for the same set. The `.flutter/flutter-master/` oracle is available for the milestone DoD cross-checks (`box.dart:3318-3348`, `flex.dart:806-812, 936-1025`).
