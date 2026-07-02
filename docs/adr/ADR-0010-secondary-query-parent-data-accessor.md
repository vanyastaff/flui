# ADR-0010: Secondary child queries during layout see parent data via an erased per-child accessor

- **Status:** Proposed (awaiting approval)
- **Date:** 2026-06-30
- **Deciders:** chief-architect; consult api-design-lead (accessor naming/semver), qa-lead (harness)
- **Supersedes / relates to:** `docs/research/2026-06-30-secondary-query-layout-gap.md`, `docs/research/2026-06-30-renderflex-parity-audit.md`
- **Gate:** ARCH-GATE (this doc) → then per-slice DEV-GATEs

## Context

Flutter render objects routinely run **secondary queries on children during a layout-or-measure pass** — `getDryLayout`, intrinsic dimensions, baseline — and those queries must see each child's **parent data** (flex factor/fit, stack positioning), in *every* execution context (real frame, dry pass, headless test harness).

FLUI's three query contexts under-wire this:

| Context | Struct (`context/intrinsics.rs`) | Parent-data access today | Result |
|---|---|---|---|
| Dry layout | `BoxDryLayoutCtx` (:174) | **none** | flex/stack `compute_dry_layout` → `Size::ZERO` |
| Intrinsics | `BoxIntrinsicsCtx` (:105) | narrow `child_flex(i) -> i32` | **test-only**: `child_flex_from_seeds` returns `0` in production (`subtree_arena.rs:589`) → flex main-axis intrinsics ignore flex factors **in production** |
| Dry baseline | `BoxDryBaselineCtx` (:38) | none | container dry baseline → `None` |

Contrast: the **real-layout** path (`BoxLayoutContext<Arity, PD>`, `context/layout.rs:126`) is generic over `PD` and exposes typed `child_parent_data(i) -> Option<&PD>` — flex's `perform_layout` already reads `ctx.child_parent_data(i).map(|pd| (pd.flex, pd.fit))` (flex.rs:319). The query contexts simply never got the equivalent channel.

**Confirmed feasible (no new data plumbing):**
- Production: each child node carries its own parent data — `RenderNode::parent_data() -> Option<&dyn ParentData>` (`storage/node.rs:556`); the query walk already holds every child node in `QuerySlot` (`query.rs:138`). The layout walk reads exactly this at `subtree_arena.rs:781`.
- Test: the harness already attaches presets via `ParentDataSeed` (`testing/parent_data.rs`) and `TreeNode::with_flex_parent_data` / `with_stack_parent_data`; `box_intrinsic_dimension` already clones `self.parent_data_seeds` and threads them (`query.rs:49-61`). `box_dry_layout` / `box_dry_baseline` simply do not.
- `ParentData` is `DowncastSync + DynClone` (`parent_data/base.rs:52`).

## Decision

### D1 — Type-erased per-child parent-data accessor, uniform across all three query contexts (slice-backed)

Give `BoxDryLayoutCtx`, `BoxIntrinsicsCtx`, and `BoxDryBaselineCtx` a **type-erased** parent-data channel, backed by a borrowed slice the driver fills per node:

```rust
// on each of the three query contexts
pub fn child_parent_data(&self, index: usize) -> Option<&dyn ParentData>;
pub fn child_parent_data_as<T: ParentData>(&self, index: usize) -> Option<&T>; // downcast_ref helper
```

Backing field (replacing the bespoke `flex` closure on `BoxIntrinsicsCtx`): `child_parent_data: &'a [Option<&'a dyn ParentData>]`. The parent downcasts to the type it itself declared as `Self::ParentData` — e.g. flex calls `ctx.child_parent_data_as::<FlexParentData>(i)`. `child_flex(i)` survives as a thin convenience over that downcast (keeps the 85 intrinsic overriders and flex.rs:233 untouched).

**Rationale & blast radius (the deciding factor):**

| | (a) Generic `BoxXCtx<PD>` | **(b) Erased accessor — chosen** |
|---|---|---|
| `RenderBox` signature ripple | **~122 override sites** (85 intrinsic + 20 dry + 17 baseline) gain `<Self::ParentData>` | **0** — `compute_*` signatures unchanged |
| `proxy_queries.rs` ambassador delegation | rewritten | untouched |
| Downcast still required? | **Yes** — driver holds `dyn` nodes; the blanket bridge must downcast `&dyn ParentData -> T::ParentData` anyway. Generic *relocates* the downcast, it doesn't remove it | Yes, once at the container call site (a type the container declared) |
| Files actually changed | trait + every overrider + proxies + bridges + driver + harness | 3 ctx structs, 3 raw bridges, 1 driver file, 2 PipelineOwner methods |
| Consistency | matches real-layout `BoxLayoutContext<PD>` | matches the **existing** `child_flex` precedent **and** the existing `child_parent_data_dyn` erased convention in `box_protocol.rs:801` |

Option (a) pays a ~122-signature ripple **and still downcasts internally**; the type-safety win is marginal because the downcast target is the container's own associated type (it cannot reasonably get it wrong, and `child_parent_data_as::<T>` makes mismatch loud in debug). Choose (b).

Accepted asymmetry: typed parent data in `perform_layout` (hot path, every render object), erased in the three query contexts (touched only by multi-child containers). Each side optimizes its own cost; the asymmetry is bounded and named.

### D2 — One context-agnostic sizing routine per container, shared by `perform_layout` and `compute_dry_layout`

Mirror Flutter's `_computeSizes(constraints, ChildLayouter)` + `ChildLayoutHelper.{layoutChild, dryLayoutChild}`. Each multi-child container gets a **private, ctx-free** sizing method that takes a measurement closure and the already-extracted parent-data, computes geometry, and **does not position** (positioning is real-layout-only):

```rust
// private to flex.rs (stack.rs / wrap.rs get their own analogues)
fn compute_sizes(
    &self,
    constraints: BoxConstraints,
    flex_factors: &[Option<i32>],
    flex_fits: &[FlexFit],
    mut measure: impl FnMut(usize, BoxConstraints) -> Size,
) -> FlexSizes /* { size: Size, child_sizes: Vec<Option<Size>>, .. } */;
```

- `perform_layout`: extract flex/fit via typed `ctx.child_parent_data(i)` → `compute_sizes(.., |i,c| ctx.layout_child(i,c))` → then position.
- `compute_dry_layout`: extract via `ctx.child_parent_data_as::<FlexParentData>(i)` → `compute_sizes(.., |i,c| ctx.child_dry_layout(i,c))` → return `.size`.

The shared routine is decoupled from the ctx type — it never sees the erased channel. There is **no** cross-container helper in flui-rendering (their algorithms differ); the shared abstraction is only the closure signature `FnMut(usize, BoxConstraints) -> Size`. **One fact, one place:** the flex sizing math lives once, in `compute_sizes`.

### D3 — Driver + harness wiring via a per-node parent-data slice (unifies production + test)

In `query.rs`, each `*_query_impl` builds a per-node `Vec<Option<Box<dyn ParentData>>>` for the current node's children **before** constructing the recursive child closure, then passes a borrowed `&[Option<&dyn ParentData>]` into `*_raw`:

- Production source: `slots.get(child_id).node.parent_data()` cloned via `dyn_clone::clone_box`.
- Test source (cfg `test`/`testing`): overlay `parent_data_seeds.get(child_id).map(ParentDataSeed::to_box)`.

Cloning (not borrowing) sidesteps the aliasing that forced the original `child_flex` to read a *separate* seed map: the owned Vec coexists with the `&mut slots` the recursion needs. This **deletes `child_flex_from_seeds`** and its test-only restriction, closing the production intrinsic gap as a side effect.

Harness wiring is then minimal: `PipelineOwner::box_dry_layout` and `box_dry_baseline` must clone+thread `self.parent_data_seeds` exactly as `box_intrinsic_dimension` already does (`query.rs:49-61`). The seed-attachment API already exists; no new harness surface for flex/stack.

## Consequences

**Positive**
- Closes the systemic gap for flex/stack/wrap dry layout and (bonus) **fixes production flex intrinsics**, with a change set confined to ~6 files plus the per-container slices.
- `child_parent_data` becomes the single parent-data idiom across all secondary-query contexts; `child_flex` is demoted to a documented convenience.
- `compute_sizes` makes dry/real divergence structurally hard (one routine, two measurers) — the recurring "MVP-as-parity" failure mode is designed out.

**Negative / accepted**
- Erased downcast at container call sites (mitigated by `child_parent_data_as::<T>` with a debug-assert on type mismatch).
- Typed-vs-erased asymmetry between `perform_layout` and the query contexts (named above).
- Per-node parent-data clone on a cold dry/intrinsic miss (small POD structs; the walk is already memoized per `(node, constraints)`).

**Risks (call-outs for the builder)**
- **Dry cache key (`peek_dry_layout`, query.rs:287)** keys on `constraints` only, not parent data. Correct *iff* a flex-factor/fit mutation routes through `mark_needs_layout` → layout-cache clear. Verify this invariant holds for parent-data writes before relying on it; document it at the cache site.
- **Cycle / re-entrancy:** the new parent-data reads happen before the `node.take()` recursion closure exists and touch only sibling slots/owned clones — they add no new re-entrancy surface. Keep the read strictly pre-closure.
- **`compute_distance_to_actual_baseline` has no child channel** (`actual_baseline_raw(&self, baseline)`), unlike `dry_baseline_raw`. Flex's *reported* container baseline cannot be implemented without one. → **Second decision required** (D-B below).
- **IntrinsicWidth/Height un-gating** depends on the *layout-time* child-intrinsic callback (`box_intrinsic_cb`, `subtree_arena.rs:952`) being reachable from a harness layout run — it "returns 0.0 in Direct-storage (test) contexts" (intrinsic_width.rs:224). That is a **separate harness wiring** from parent data. → **Second decision (D-C)**.

## Ordered implementation plan

Dependency order: context API → driver wiring → harness wiring → shared helper → flex dry (**milestone**) → stack/wrap dry → flex baseline → intrinsic forcing.

1. **Context API (D1).** `context/intrinsics.rs`: add the `child_parent_data` slice field + `child_parent_data` / `child_parent_data_as` methods to `BoxDryLayoutCtx`, `BoxIntrinsicsCtx`, `BoxDryBaselineCtx`; replace `BoxIntrinsicsCtx`'s `flex` closure with the slice and reimplement `child_flex` over it; update the three `::new` constructors and the `test_support::leaf_*` helpers (pass `&[]`). *Verifies:* crate compiles; existing leaf intrinsic/dry unit tests still pass (slice empty).
2. **Raw bridges (D1).** `traits/render_object.rs`: change `intrinsic_raw` (swap `child_flex` → `child_parent_data: &[Option<&dyn ParentData>]`), `dry_layout_raw` (add the slice), `dry_baseline_raw` (add the slice). `traits/render_box.rs` blanket impls (567/591/608): construct each ctx with the slice. *Verifies:* compiles; no behavior change yet.
3. **Driver wiring (D3).** `pipeline/owner/query.rs`: in `dry_layout_query_impl` / `intrinsic_query_impl` / `dry_baseline_query_impl`, build the per-node `Vec<Option<Box<dyn ParentData>>>` from slots (+ seeds under cfg) before the recursion closure, pass the borrowed slice to `*_raw`; thread `parent_data_seeds` into `dry_layout_query` / `dry_baseline_query` (cfg-gated like intrinsic); delete `child_flex_from_seeds` (`subtree_arena.rs`). *Verifies:* a unit test that a child with `FlexParentData{flex:2}` is visible via `child_parent_data` inside a dry-layout `compute_*`.
4. **Harness wiring (D3).** `pipeline/owner/query.rs`: `box_dry_layout` + `box_dry_baseline` clone+thread `self.parent_data_seeds` (mirror `box_intrinsic_dimension`, lines 49-61). Confirm `testing/` needs nothing new for flex/stack. *Verifies:* harness `dry_layout`/`dry_baseline` now reach seeded parent data.
5. **Shared sizing helper (D2).** `flui-objects/src/layout/flex.rs`: extract `RenderFlex::compute_sizes(constraints, flex_factors, flex_fits, measure)` from `perform_layout` (sizing only, lines ~309-477; leave positioning ~479-547 in `perform_layout`); rewire `perform_layout` to call it. *Verifies:* all existing flex harness tests stay green (pure refactor, behavior-preserving).
6. **★ MILESTONE — flex `compute_dry_layout` (D2+D1).** `flex.rs`: implement `compute_dry_layout` = extract flex/fit via `ctx.child_parent_data_as::<FlexParentData>` → `compute_sizes(.., |i,c| ctx.child_dry_layout(i,c))` → return `.size`. *Verifies (red→green):* `harness_flex_dry_layout` — Row, 50×30 child, `dry_layout(tight 500×300)` == `(500,300)` and `== layout(..).size`; cross-check `.flutter/flutter-master/.../flex.dart:1079` (`_computeSizes(..).axisSize`). This is the first end-to-end proof.
7. **Stack then wrap `compute_dry_layout`.** `stack.rs` (reads `StackParentData` via `child_parent_data_as` — seed variant already exists), then `wrap.rs`. **Check during wrap:** `WrapParentData` is layout *output*, not input — wrap dry likely needs no child parent data; only add a `ParentDataSeed::Wrap` variant if a real input dependency surfaces. *Verifies:* `harness_stack_dry_layout`, `harness_wrap_dry_layout` vs `.flutter/flutter-master/.../stack.dart` / `wrap.dart`.
8. **Flex baseline** (after D-B is decided). Implement `compute_dry_baseline` for flex (child channel already exists) and the container `compute_distance_to_actual_baseline` (needs D-B's new child channel): horizontal→highest, vertical→first child baseline (`.flutter/flutter-master/.../flex.dart:806`). *Verifies:* `harness_flex_baseline` returns `Some(..)` matching the oracle, not `None`.
9. **IntrinsicWidth/Height default forcing** (after D-C is decided). Un-gate the no-step case once the layout-time child-intrinsic channel is reachable in the harness. *Verifies:* `harness_intrinsic_width_forces_child` with no `step_width`.

Steps 1-6 are one shippable slice (the milestone). 7 is a second slice. 8 and 9 are gated on their second decisions and should not block 1-7.

## Second decisions to flag (do not let them block the milestone)

- **D-A (minor, → api-design-lead):** accessor naming — `child_parent_data` + `child_parent_data_as::<T>` (recommended, reads well at call sites) vs `child_parent_data_dyn` to match the existing protocol-layer suffix. Both are new `pub` surface on `flui-rendering` query contexts → additive, semver-minor.
- **D-B (→ chief-architect + api-design-lead, before step 8):** give `actual_baseline_raw` a child-query channel mirroring `dry_baseline_raw`'s `child_query` closure (ripples 5 `compute_distance_to_actual_baseline` overriders) vs a narrower default helper. Recommend mirroring `dry_baseline_raw` for symmetry; record as a follow-up ADR-let.
- **D-C (→ qa-lead + chief-architect, before step 9):** wire the layout-time child-intrinsic callback into the harness `LayoutRun` so `ctx.child_max_intrinsic_*` returns real values during a harness `perform_layout`. Independent of parent data; likely its own small ADR + harness change.

## Maintainer-grade pre-code gate

**Verdict: ACCEPTABLE.** The design reshapes a weak narrow primitive (`child_flex`, test-only) into one general, production-correct parent-data accessor; reuses existing infra (`ParentDataSeed`, `DowncastSync`, `dyn_clone`, the `child_parent_data_dyn` convention) rather than inventing; keeps one home for flex sizing math (`compute_sizes`); and confines new `pub` surface to additive methods. No new dependency cycles; flui-objects → flui-rendering direction preserved. Forward view: the same `child_parent_data` + `compute_sizes` shape extends to every future multi-child container (table, flow) without further context changes.

The `.flutter/` oracle (`.flutter/flutter-master/`) was restored this session and is available for the DoD cross-checks in steps 6-8.
