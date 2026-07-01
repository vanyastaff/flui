# ADR-0011: Dry-layout and dry-baseline contexts get a child-intrinsic query channel (closes D-C)

*Give `BoxDryLayoutCtx` / `BoxDryBaselineCtx` the same child-intrinsic query the real-layout context already has, so dry layout of an intrinsic-consuming proxy matches `perform_layout` instead of approximating it.*

---

- **Status:** Accepted (chief-architect ARCH-GATE: ACCEPTABLE; reviewed by orchestrator against `.flutter/` oracle + ADR-0010 infra)
- **Date:** 2026-06-30
- **Deciders:** chief-architect; consult api-design-lead (query-enum surface / `#[non_exhaustive]`), qa-lead (filling-child harness test)
- **Supersedes / relates to:** ADR-0010 (secondary-query parent-data accessor) — this decision **resolves ADR-0010's flagged second-decision D-C** (line 125) and unblocks its step 9 (line 117). Builds on ADR-0010's landed D1/D3 infra (the erased per-child slice + take-out query driver).
- **Gate:** ARCH-GATE (this doc) → then per-slice DEV-GATEs.

---

## Context

Flutter's `RenderIntrinsicWidth` / `RenderIntrinsicHeight` size their child to the child's **intrinsic** extent. The sizing lives in one helper, `_childConstraints`, which issues a real child-intrinsic query:

```dart
// proxy_box.dart:712-720  (RenderIntrinsicWidth)
BoxConstraints _childConstraints(RenderBox child, BoxConstraints constraints) {
  return constraints.tighten(
    width: constraints.hasTightWidth
        ? null
        : _applyStep(child.getMaxIntrinsicWidth(constraints.maxHeight), _stepWidth),
    height: stepHeight == null
        ? null
        : _applyStep(child.getMaxIntrinsicHeight(constraints.maxWidth), _stepHeight),
  );
}
```

Crucially, `_childConstraints` is shared by **both** measure paths via `_computeSize(layoutChild, constraints)`:

- `performLayout` → `_computeSize(ChildLayoutHelper.layoutChild, …)` (proxy_box.dart:743-745)
- `computeDryLayout` → `_computeSize(ChildLayoutHelper.dryLayoutChild, …)` (proxy_box.dart:730-734)
- `computeDryBaseline` → `child.getDryBaseline(_childConstraints(…), …)` (proxy_box.dart:736-740)

So in Flutter the child-**intrinsic** query runs in the real pass, the dry pass, **and** the dry-baseline pass. Dry layout legitimately querying intrinsics is part of the layout protocol — Prime-Directive rule #1 territory (port the layout protocol 1:1).

### The gap in FLUI (verified)

FLUI has **two** child-intrinsic query mechanisms, and only one of them is reachable from dry contexts:

| Mechanism | Used by | How it holds children | Re-entrancy guard |
|---|---|---|---|
| **(1) take-out slot map** — `intrinsic_query` / `dry_layout_query` / `dry_baseline_query` (`pipeline/owner/query.rs`) | standalone `box_intrinsic_dimension`, `box_dry_layout`, `box_dry_baseline` (and the harness `min_intrinsic_*` / `dry_layout` / `dry_baseline`) | `QuerySlot { node: Option<&mut …> }`; the node is `node.take()`n OUT while its own computation runs | cyclic re-entry finds `None` → `debug_assert!` + degenerate-but-defined. **Safe by construction, no `unsafe`.** |
| **(2) borrowed subtree arena** — `box_intrinsic_query_borrowed` (`subtree_arena.rs:1132`, `unsafe`) | live `perform_layout` via `ctx.child_intrinsic` (`context/layout.rs:386`) | children held by live borrows in the arena pool | needs the `is_in_flight` gate (u21b regression test) to skip an in-flight slot rather than alias it |

**`perform_layout` already resolves child intrinsics — in production *and* in the harness.** `layout_subtree_borrowed_impl` wires `box_intrinsic_cb_ref` into the erased box-layout ctx for **every** box node it lays out (`subtree_arena.rs:1000`), routing `ctx.child_max_intrinsic_*` through mechanism (2). This is proven by the u21b cyclic-intrinsic test and the flex intrinsic harness tests. (ADR-0010's original D-C framing — "wire the layout-time intrinsic callback into the harness" — was investigated and found *already done*; the stale comment in `harness_intrinsic_height_with_child_passes_size_through`, intrinsic_height test at `render_object_harness.rs:2864`, predates that wiring and only asserts width.)

**The dry contexts cannot query intrinsics at all.** `BoxDryLayoutCtx` and `BoxDryBaselineCtx` (`context/intrinsics.rs`) expose only `child_dry_layout` / `child_dry_baseline` (+ the ADR-0010 erased `child_parent_data*`). No child-intrinsic method exists. So the dry paths **approximate** the intrinsic with a `child_dry_layout` at a loose axis:

- `RenderIntrinsicWidth::compute_dry_layout` (`intrinsic_width.rs:313-343`) probes via `ctx.child_dry_layout(0, loose-on-one-axis)`.
- `RenderIntrinsicHeight::compute_dry_layout` (`intrinsic_height.rs:160-179`) does the same.
- Both `compute_dry_baseline` (`intrinsic_width.rs:346-386`, `intrinsic_height.rs:181-198`) repeat the approximation.

That approximation is **wrong for a width/height-filling child** — the exact case these proxies exist for. Example: `RenderIntrinsicWidth` over a `RenderFlex` row with `MainAxisSize::Max`. The row's `getMaxIntrinsicWidth` is the **sum** of its children's intrinsic widths (say 100). A dry layout of that row under a *loose* width returns the loose max (say 500), not 100. So:

- `perform_layout` (mechanism 2): forces child to intrinsic width 100 → size ≈ `(100, H)`.
- `compute_dry_layout` (approximation): forces child to the loose-dry 500 → size ≈ `(500, H)`.

**Dry ≠ committed** for the primary use case — and FLUI enforces the dry==committed invariant (every dry harness test this session asserts `dry_layout == box_geometry`, e.g. `harness_stack_dry_layout_*` at `render_object_harness.rs:1379-1453`). It is also a real production defect: any parent that dry-measures an intrinsic-sizing subtree gets a size the committed layout will not honor.

### Why this blocks the `RenderIntrinsicWidth` "force width by default" fix

Flutter forces the child's width to its intrinsic width **whenever width is not tight**, regardless of `stepWidth` (`_applyStep(x, null) == x`). FLUI's `child_constraints` (`intrinsic_width.rs:149-176`) instead only forces when `step_width.is_some()` — a behavioral bug. But fixing `perform_layout` alone would make the *real* pass force the intrinsic width while the *dry* pass keeps returning the loose approximation → it would **widen** the dry≠committed gap. The fix is not landable until dry can obtain the same intrinsic value the real pass uses. That is what this ADR unblocks.

### Confirmed feasible (no new plumbing)

- The dry driver already **holds the take-out slot map** (`dry_layout_query_impl`, `dry_baseline_query_impl` in `query.rs`). Its child closures already recurse via `dry_layout_query(slots, child_id, …)`. Adding an `intrinsic_query(slots, child_id, …)` dispatch in the same closure is mechanism (1) reused verbatim — the child is a *different* node than the one taken out, so **no new re-entrancy surface and no `is_in_flight`/`unsafe` needed**.
- `parent_data_seeds` are already threaded into `dry_layout_query` / `dry_baseline_query` (ADR-0010 D3, `query.rs:76-88, 104-117`), so the intrinsic sub-query sees the same seeds the enclosing dry query does — flex/stack children whose intrinsics depend on parent data stay correct in the dry pass.
- Both mechanisms memoize into the **same** per-node intrinsic cache, so a child's intrinsic answer is identical whether reached from `perform_layout` (mechanism 2) or from a dry sub-query (mechanism 1). dry==committed by shared cache, not by coincidence.

---

## Decision

**We adopt Option A: add a child-intrinsic query channel to `BoxDryLayoutCtx` and `BoxDryBaselineCtx`, wired by the dry driver through the existing safe take-out `intrinsic_query`.** We reject Option B (documented approximation) as a rule-#1 and dry==committed violation.

### D-C1 — A per-context "child sub-query request" enum; add an `Intrinsic` kind

A context that runs on the borrowed slot map can hold **one** `&mut`-capturing child callback, not several (two closures cannot each hold `&mut slots`). The codebase already solved this for `BoxDryBaselineCtx` with a dispatched request enum (`DryBaselineChildRequest`). We follow that idiom and keep each context's request set **exactly** the sub-query kinds it legitimately issues (ISP — no god-request):

```rust
// context/intrinsics.rs — new, for BoxDryLayoutCtx
#[non_exhaustive]
pub enum DryLayoutChildRequest { DryLayout(BoxConstraints), Intrinsic(IntrinsicDimension, f32) }
#[non_exhaustive]
pub enum DryLayoutChildResponse { DryLayout(Size), Intrinsic(f32) }

// context/intrinsics.rs — extend the EXISTING baseline enums with an Intrinsic kind
#[non_exhaustive]  // add attribute
pub enum DryBaselineChildRequest  { Baseline(BoxConstraints, TextBaseline), DryLayout(BoxConstraints), Intrinsic(IntrinsicDimension, f32) }
#[non_exhaustive]  // add attribute
pub enum DryBaselineChildResponse { Baseline(Option<f32>), DryLayout(Size), Intrinsic(f32) }
```

`#[non_exhaustive]` is added now (OCP): these protocol enums are *demonstrably* growing, so future child-query kinds must be additive rather than a breaking change. All three enums stay `Copy` (`IntrinsicDimension`, `BoxConstraints`, `Size`, `f32` are `Copy`).

### D-C2 — Mirror `BoxIntrinsicsCtx`'s accessor surface on the two dry contexts

Both dry contexts gain the same intrinsic accessors the real path already exposes (`context/layout.rs:386-422`) — one general method plus four named conveniences, identical signatures, so callers write the same code in every context (one idiom, one place):

```rust
impl BoxDryLayoutCtx<'_>  /* and BoxDryBaselineCtx<'_> */ {
    pub fn child_intrinsic(&mut self, index: usize, dim: IntrinsicDimension, extent: f32) -> f32;
    pub fn child_max_intrinsic_width (&mut self, index: usize, height: f32) -> f32;
    pub fn child_min_intrinsic_width (&mut self, index: usize, height: f32) -> f32;
    pub fn child_max_intrinsic_height(&mut self, index: usize, width:  f32) -> f32;
    pub fn child_min_intrinsic_height(&mut self, index: usize, width:  f32) -> f32;
}
```

`BoxDryLayoutCtx`'s backing field changes from `dry: FnMut(usize, BoxConstraints) -> Size` to `query: FnMut(usize, DryLayoutChildRequest) -> DryLayoutChildResponse`; `child_dry_layout` keeps its signature and dispatches `DryLayout`. `BoxDryBaselineCtx`'s callback type is unchanged (already the enum) — only its enum grew and its accessor set widened.

This ripples **only the internal `dry_layout_raw` bridge**, not the ~122 `compute_*` override sites: `compute_dry_layout(&self, constraints, ctx: &mut BoxDryLayoutCtx)` is unchanged; only the ctx's private field + `::new` + the blanket bridge that constructs it move. Proxies do **not** override `*_raw` (they use the blanket impl via `proxy_queries.rs` helpers), so there is no ambassador-delegation fan-out. Blast radius: `context/intrinsics.rs`, `traits/render_object.rs` (default `dry_layout_raw` sig), `traits/render_box.rs` (blanket `dry_layout_raw`), `pipeline/owner/query.rs`.

### D-C3 — Driver wiring: dispatch `Intrinsic` to the safe take-out `intrinsic_query`

In `query.rs`, the dispatched child closure in `dry_layout_query_impl` (and the existing one in `dry_baseline_query_impl`) handles the new arm:

```rust
DryLayoutChildRequest::DryLayout(c)      => dry_layout_query(slots, child_id, c, seeds),
DryLayoutChildRequest::Intrinsic(dim, e) => intrinsic_query(slots, child_id, dim, e, seeds),
```

`intrinsic_query` operates on the **same** `slots` map the dry walk already owns; it takes the child (a different node than the one currently taken out) OUT for its own sub-walk and restores it. This is the identical discipline the existing `DryLayout`/`Baseline` arms use. **No `unsafe`, no `is_in_flight` gate** — that gate belongs to mechanism (2) (`box_intrinsic_query_borrowed`), which holds live borrows; mechanism (1) detects cycles via the taken-out `None`. `box_dry_layout` / `box_dry_baseline` need no change (seeds already threaded).

### D-C4 — `RenderIntrinsicWidth` / `RenderIntrinsicHeight`: one shared `_childConstraints`, correct math, both passes on the intrinsic channel

Extract Flutter's `_childConstraints` as a single `&self` helper parameterized by an intrinsic-query closure, and call it from **all three** compute paths so real, dry, and dry-baseline are structurally identical (mirrors ADR-0010 D2's `compute_sizes` "one routine, N measurers"):

```rust
// intrinsic_width.rs — one home for the sizing math
fn child_constraints(&self, constraints: BoxConstraints,
                     mut intrinsic: impl FnMut(IntrinsicDimension, f32) -> f32) -> BoxConstraints;
```

- `perform_layout`   → `child_constraints(c, |d,e| ctx.child_intrinsic(0, d, e))` then `ctx.layout_child(0, cc)`.
- `compute_dry_layout` → `child_constraints(c, |d,e| ctx.child_intrinsic(0, d, e))` then `ctx.child_dry_layout(0, cc)`.
- `compute_dry_baseline` → `child_constraints(c, |d,e| ctx.child_intrinsic(0, d, e))` then `ctx.child_dry_baseline(0, cc, b)`.

The same fix corrects the math to match proxy_box.dart:712-720 (all inside the one helper):

1. **Always force width when not tight** (drop the `step_width.is_some()` gate) — the core bug. `apply_step(x, None) == x`, so the no-step case forces to the raw intrinsic.
2. **Raw query args**: the width query uses `constraints.maxHeight` (raw, not step-snapped); the height query uses `constraints.maxWidth` (raw, not the computed width). FLUI currently snaps the height arg and reuses the computed width — both diverge.
3. **Step-then-clamp order**: apply the step to the raw intrinsic, *then* clamp to `[min,max]` (Flutter's `tighten` clamps after `_applyStep`). FLUI currently clamps then steps. Cross-check FLUI `BoxConstraints::tighten` semantics; if it does not clamp, clamp explicitly after the step.
4. **Height forced only when `step_height` is set** (IntrinsicWidth) — unchanged from Flutter; IntrinsicHeight always forces height when not tight.

The `#[cfg]`-gated "returns 0.0 in Direct-storage" fallbacks in the doc comments stay accurate for the pipeline-less Direct context but no longer describe the harness/production path.

---

## Consequences

**Positive**

- Dry layout / dry baseline of an intrinsic-consuming proxy now equals `perform_layout` — the dry==committed invariant holds for the case these proxies exist for. Fixes a production correctness defect, not only a test gap.
- Unblocks the `RenderIntrinsicWidth` default-force-width fix (ADR-0010 step 9) with real Flutter parity across all three passes.
- `child_intrinsic` becomes uniform across real-layout, intrinsics, dry-layout, and dry-baseline contexts — the same call reads the same in every context.
- `_childConstraints` lives once per proxy and drives all three passes: the "MVP-as-parity" / dry-drift failure mode is designed out structurally.
- Reuses the safe take-out `intrinsic_query` — **zero new `unsafe`**, no new re-entrancy surface, no `is_in_flight` obligation.

**Negative / Trade-offs**

- `BoxDryLayoutCtx`'s single `dry` callback becomes an enum-dispatched `query` callback; `dry_layout_raw`'s callback type changes (internal bridge only, ~4 files).
- Adding `#[non_exhaustive]` to `DryBaselineChildRequest`/`Response` forces internal matches to add a wildcard arm; that is intended (OCP) but is churn at the match sites in `query.rs`.
- A dry layout of an IntrinsicWidth subtree now issues *two* memoized child sub-queries (one intrinsic, one dry-layout) — exactly as Flutter does; memoization makes this cheaper than Flutter's speculative recomputation (a rule-#2 leapfrog that does not change observable behavior).

**Follow-ups**

- Update the stale `harness_intrinsic_height_with_child_passes_size_through` test (`render_object_harness.rs:2864`) to assert the full `60×40` size and drop the "returns 0" comment.
- ADR-0010's D-B (container `compute_distance_to_actual_baseline` child channel) remains independent and open; not touched here.

---

## Alternatives Considered

| Option | Why rejected |
|---|---|
| **B — accept the dry approximation as a documented divergence**, gate forcing to `perform_layout` | Violates Prime-Directive rule #1 (dry-layout-queries-intrinsics is part of the ported layout protocol) and the enforced dry==committed invariant, for the *exact* case the proxies exist for. It is also a live production defect for any dry-measuring parent, not merely a harness compromise. |
| **C1 — special-case IntrinsicWidth/Height inside the dry driver** | The driver already does the recursion; the only real question is the API the object sees. Special-casing types in the driver breaks the "objects are pure functions + a child channel" contract and does not generalize to other future dry-consuming proxies. It is Option A with worse layering. |
| **C2 — one unified `SecondaryChildRequest { DryLayout, DryBaseline, Intrinsic }` shared by both contexts** | ISP violation: the dry-layout context would carry a `Baseline` variant it never issues, forcing dead/`unreachable!` arms. Per-context enums keep each contract exactly as wide as its real capability. |
| **C3 — make the dry contexts generic to route a typed intrinsic channel** | Same ~122-site ripple ADR-0010 D1 already rejected for parent data; the erased/dispatched shape is the settled convention. |
| **A2 — reuse the `unsafe` borrowed-arena `box_intrinsic_query_borrowed` from the dry driver** | The dry driver is the *safe* take-out world; reaching into the borrowed-arena world would import the `is_in_flight`/aliasing obligations for no benefit — mechanism (1) already answers the query correctly. |

---

## Ordered implementation plan

Sequencing rule: the dry fix and the `perform_layout` math fix are **two halves of one behavior** and must land **atomically** to keep dry==committed green at every commit. Do **not** land the `perform_layout` force-width fix first on its own — a real-pass-only fix with dry still approximating widens the gap (and regresses production dry-measurement). Land the enabling infra first (behavior-preserving), then flip both passes together.

**Slice 1 — infra (behavior-preserving; all existing tests stay green):**

1. **Context API (D-C1, D-C2).** `context/intrinsics.rs`: add `DryLayoutChildRequest`/`Response` (`#[non_exhaustive]`); switch `BoxDryLayoutCtx` to the dispatched `query` field; keep `child_dry_layout`; add the five intrinsic accessors. Extend `DryBaselineChildRequest`/`Response` with `Intrinsic`, add `#[non_exhaustive]`, add the five accessors to `BoxDryBaselineCtx`. Update the `::new` constructors and the `test_support::leaf_dry_layout` / `leaf_dry_baseline` deny closures (panic on any request — a leaf must not query). *Verifies:* crate compiles; existing leaf dry unit tests still pass.
2. **Raw bridge (D-C2).** `traits/render_object.rs` default `dry_layout_raw` + `traits/render_box.rs` blanket `dry_layout_raw`: change the callback type to the enum form and construct `BoxDryLayoutCtx` with it. `dry_baseline_raw` unchanged in shape (enum grew only). *Verifies:* compiles, no behavior change.
3. **Driver wiring (D-C3).** `pipeline/owner/query.rs`: in `dry_layout_query_impl`, make the child closure dispatch `DryLayout → dry_layout_query`, `Intrinsic → intrinsic_query`; in `dry_baseline_query_impl`, add the `Intrinsic → intrinsic_query` arm (+ wildcard for `#[non_exhaustive]`). Thread the in-scope `seeds` into the intrinsic sub-query. *Verifies (new unit test):* a `compute_dry_layout` that calls `ctx.child_max_intrinsic_width(0, …)` receives the child's real intrinsic (e.g. a flex row's summed width), and it equals `run.min_intrinsic_width(child, …)`.

**Slice 2 — behavior fix (the `perform_layout` bug + dry correction, atomic):**

4. **`RenderIntrinsicWidth` (D-C4).** `flui-objects/src/layout/intrinsic_width.rs`: rewrite `child_constraints` to take an `intrinsic` closure and implement proxy_box.dart:712-720 exactly (always-force width, raw args, step-then-clamp). Rewire `perform_layout`, `compute_dry_layout`, `compute_dry_baseline` to call it with `ctx.child_intrinsic`. Delete the `child_dry_layout`-based probe approximation and `width_for_height_query`.
5. **`RenderIntrinsicHeight` (D-C4).** Same shape for `intrinsic_height.rs` (`_childConstraints` at proxy_box.dart:816-819: force height to `child.getMaxIntrinsicHeight(constraints.maxWidth)` when not tight).
6. **★ MILESTONE — filling-child harness proof.** `render_object_harness.rs`: add `harness_intrinsic_width_forces_filling_child` — `RenderIntrinsicWidth::unconstrained()` over a `RenderFlex` row (`MainAxisSize::Max`) of two `50×30` children, under `loose(width 500, height 300)`. Assert `run.dry_layout(root, c) == run.box_geometry(root)` **and** both equal the intrinsic-derived size (`100 × 30`), cross-checked against `.flutter/flutter-master/.../flex.dart` `computeMaxIntrinsicWidth` (sum of children) and proxy_box.dart:723-734. Add the IntrinsicHeight analogue. This test is **red before slice 2, green after** — the anti-cheating proof that dry now equals real for the filling case. Also update the stale `harness_intrinsic_height_with_child_passes_size_through` to assert `60×40`.

Slice 1 is one shippable, behavior-neutral PR. Slice 2 is the second PR and must land as a unit (steps 4-6 together).

---

## Maintainer-grade pre-code gate

**Verdict: ACCEPTABLE.** The design reuses the existing safe take-out `intrinsic_query` (no new `unsafe`, no new re-entrancy surface, no `is_in_flight` obligation — the one real safety hazard is confined to the borrowed-arena path this decision deliberately avoids); mirrors the settled erased/dispatched context convention (`DryBaselineChildRequest`, `child_intrinsic`) rather than inventing; keeps the IntrinsicWidth/Height sizing math in one `_childConstraints` shared by all three passes (one fact, one place); confines new `pub` surface to additive, `#[non_exhaustive]`-guarded enums and accessor methods; and adds no dependency cycle (flui-objects → flui-rendering direction preserved). Forward view (2 years / 3 extensions): the same `child_intrinsic`-on-dry-context shape serves every future dry-consuming proxy (baseline, aspect-ratio, table cell sizing) without further context surgery, and `#[non_exhaustive]` means the next child-query kind is a variant, not a break. The `.flutter/flutter-master/` oracle is available for the step-6 DoD cross-checks.
