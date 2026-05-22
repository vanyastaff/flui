---
title: "refactor: flui-painting × flui-view — Cycle 5 audit execution"
type: refactor
status: active
date: 2026-05-22
origin: docs/brainstorms/flui-painting-view-cycle5-requirements.md
---

# refactor: flui-painting × flui-view — Cycle 5 audit execution

## Summary

Cycle 5 of the audit-execute series closes the flui-painting × flui-view audit across **8 waves / 15 implementation units**: genuine-zombie deletion, two half-impl wire-ups (root bootstrap, error-view), keyed child reconciliation, hot-path performance, and hygiene. Planning research revealed that audit finding V-7 is mis-scoped — `ElementTree` is vestigial, so V-7 is deferred to a dedicated Cycle 6 element-ownership unification rather than executed as a mechanical trait impl.

---

## Problem Frame

The completed audit ([docs/research/2026-05-22-flui-painting-view-audit.md](../research/2026-05-22-flui-painting-view-audit.md)) and its requirements doc ([docs/brainstorms/flui-painting-view-cycle5-requirements.md](../brainstorms/flui-painting-view-cycle5-requirements.md)) define a 45-finding refactor cycle. The requirements doc already settled the product decisions (the three-bucket classification policy, the V-1 reframe, the wire-vs-gate calls). What remained for planning: dependency-order the work into waves, resolve the four `Deferred to Planning` questions, and verify the audit's blast-radius estimates against live code — the Cycle 4 receipts record that audit estimates can be off by an order of magnitude.

That verification surfaced one structural problem the audit and the requirements doc both missed: the flui-view element tree has **two coexisting ownership models**, and the finding that depends most on that model (V-7) was mis-scoped. This plan reshapes V-7 accordingly and sequences the rest.

---

## Requirements

This plan executes origin requirements **R1–R12 and R14–R20** ([origin](../brainstorms/flui-painting-view-cycle5-requirements.md)) — R13 is reshaped into a deferral (below) and R11 was already origin-deferred. Each implementation unit cites the origin R-IDs it advances. Grouped:

**Classification policy** — R1 (three-bucket classification), R2 (no feature-gate-default-off for port-targets), R3 (`// PORT-TARGET:` ledger).

**Finding execution** — R4 (genuine-zombie deletion), R5 (`canvas::sugar` deleted not gated), R6 (port-targets kept compiled), R7 (`AnimationBehavior` rename), R8 (wire `RootRenderView`), R9 (wire `ErrorView`), R10 (delete flat inherited registry), R12 (keyed reconciliation), R14 (`attach_root_widget` → `Result`), R15 (hot-path performance), R16 (P2/P3 hygiene), R17 (cross-crate `Color`).

**Reshaped** — **R13 (V-7 `ElementTree` implements flui-tree traits) is deferred** — see Key Technical Decisions and Scope Boundaries. **R11** (faithful per-element inherited map) was already origin-deferred and stays deferred.

**Execution contract** — R18 (atomic-commit-per-finding), R19 (per-wave verification gate), R20 (origin scope = all 45 findings; origin defers V-20 and V-23 to Cycle 6, and this plan additionally defers V-7 — see Key Technical Decisions and Scope Boundaries).

**Origin acceptance examples:** AE1 (covers R10), AE2 (covers R9), AE3 (covers R8), AE4 (covers R12), AE5 (covers R3/R6) — carried into unit test scenarios.

---

## Scope Boundaries

- No new abstractions and no Flutter redesign — Cycle 5 is port-fidelity work only (origin Scope Boundaries).
- The faithful per-element persistent inherited map (real O(1) inherited lookup) is not built — Cycle 5 executes origin R10 (delete the broken flat registry, keep the correct O(depth) walk); the O(1) map itself is origin R11, which stays deferred.
- Creating the `flui-widgets` crate is out of scope — port-targets are kept ready, not wired.
- Wave sequencing within this plan is fixed here; the store-by-id vs store-by-value question is resolved (see Key Technical Decisions).
- Cycle 6 target selection is out of scope.

### Deferred to Follow-Up Work

- **V-7 — `ElementTree` implements `TreeRead`/`TreeNav`/`TreeWrite` (origin R13): deferred to Cycle 6.** Planning research found `ElementTree` is vestigial — `ElementTree::insert` (non-root) has zero production callers (test-only, `tree/element_tree.rs:661`); production inserts only the root via `mount_root_with_pipeline_owner`. The live element tree is the recursive `ElementCore.children` → `VariableChildStorage` → `Vec<Box<dyn ElementBase>>` box nesting. Implementing the flui-tree traits faithfully requires unifying the two ownership models onto `ElementTree` as the single by-id owner (`ElementNode` gains a children index; the arity child-storages migrate to `Vec<ElementId>`). That is a foundational refactor — the audit's "+200 LOC mechanical" estimate is an order of magnitude low, exactly the failure mode the Cycle 4 receipts warn about. It is entangled with V-1, V-2, and V-13 (shared root cause) and belongs in its own cycle, joining the audit's own deferrals below.
- **V-20 (`ElementBase` sub-trait split) and V-23 (`WidgetsBindingInner` per-field locks): deferred to Cycle 6** — the audit marks both "future wave."
- **Recommended Cycle 6:** the element-ownership unification (faithful V-7) + V-20 + V-23 — a flui-view core architectural cycle. The unification also unblocks the real fixes for V-1 (per-element inherited map) and V-13 (real `BuildContext` threading), whose Cycle 5 treatments are deliberately partial.

---

## Context & Research

### Relevant Code and Patterns

- **flui-tree trait surface** (`crates/flui-tree/src/traits/{read,nav,write}.rs`) — `TreeRead` (3 required methods), `TreeNav` (5 required; no `root()`), `TreeWrite` (3 required; `remove` cascades free via an iterative post-order drain). `RenderTree` (`crates/flui-rendering/src/storage/tree.rs`) and `LayerTree` (`crates/flui-layer/src/tree/tree_traits.rs`) are the canonical implementors — relevant as the **Cycle 6** V-7 model, not Cycle 5.
- **Element ownership** — `ElementTree` (`crates/flui-view/src/tree/element_tree.rs`): `Slab<ElementNode>`, production-populated with the root only. `ElementCore<V,A>` (`crates/flui-view/src/element/generic.rs`): holds `children: A::Storage`; the arity storages (`crates/flui-view/src/element/child_storage.rs`) own child elements as `Vec<Box<dyn ElementBase>>`. The live tree is the box nesting.
- **Keyed reconciliation** — `reconcile_children` (`crates/flui-view/src/tree/reconciliation.rs`) implements Flutter's 5-phase keyed O(N) algorithm but operates on `ElementTree` + `ElementId` and has zero production callers; `VariableChildStorage::update_with_views` uses an index loop.
- **flui-app ↔ flui-view seam** — `flui-app` calls `WidgetsBinding::attach_root_widget(view)` once (`crates/flui-app/src/app/binding.rs:182`) and never touches `ElementTree` or `RootRenderView`. Wiring the root bootstrap is intra-`flui-view`.
- **`scripts/port-check.sh`** — 7 institutional refusal triggers; mandatory per-wave gate. Triggers 1/2/3/6 cover the Cycle 5 crates.
- **Cargo profiles** — neither `[profile.dev]` nor `[profile.release]` sets `panic = "abort"` (`Cargo.toml`); `std::panic::catch_unwind` is viable for the ErrorView wire-up.

### Institutional Learnings

- No `docs/solutions/` store exists; institutional knowledge lives in audit/brainstorm/receipt triads under `docs/`.
- **Cycle 4 wave-execution template** (`docs/research/2026-05-22-cycle4-wave2-receipts.md`, `-design.md`): per-wave gate = build + clippy `-D warnings` + per-crate `test --lib` + `cargo doc --no-deps` (zero *new* warnings) + `scripts/port-check.sh -v`; commit-by-commit receipt ledger; architectural `rg` grep-gates per deletion. Known flake: `flui-app --lib` needs `--test-threads=1` (pre-existing singleton-state flake, not introduced by any cycle).
- **Meta-learning — audit estimates can be 10× off** (Cycle 4 receipts): the audit's R-7 estimate was ~50 LOC predicted vs ~1,500 actual. Verify blast radius against live code before committing. This learning is what surfaced the V-7 reshape; it also flags the V-2 ("±200-400") and P-7 ("±200 cross-cutting") estimates as soft.

### External References

None — FLUI is a Flutter port; the reference is the in-repo `.flutter/` source. Parity anchors: `attachRootWidget` → `RenderObjectToWidgetAdapter` (`.flutter/flutter-master/packages/flutter/lib/src/widgets/binding.dart`); `ComponentElement.performRebuild` dual try/catch → `ErrorWidget.builder` (`.flutter/flutter-master/packages/flutter/lib/src/widgets/framework.dart:5810-5859`); `Element.updateChildren` keyed reconciliation (`framework.dart:4125`).

---

## Key Technical Decisions

- **V-7 deferred to Cycle 6 (deviation from origin R13).** `ElementTree` is not the live element tree, so the audit's mechanical trait-impl framing is not executable. Deferring it to a dedicated element-ownership-unification cycle is correct sizing, not defer-with-excuse: the work is genuinely foundational and entangled, and jamming a broken half-version into a 45-finding cycle would violate port fidelity. The plan documents the unification shape so Cycle 6 inherits a design seed.
- **V-2 reconciliation lands store-by-value.** `reconcile_children` is adapted to operate on the live `VariableChildStorage` box vec (`&mut Vec<Box<dyn ElementBase>>`), keyed-matching old child boxes to new views. Store-by-id is the faithful long-term shape but requires the deferred unification; store-by-value delivers the keyed-reconciliation value (Hero/Reorderable/GlobalKey) in Cycle 5 without blocking on V-7. This resolves the origin `Deferred to Planning` question on storage model. The accepted cost: the Cycle 6 unification will re-adapt `reconcile_children` to `Vec<ElementId>`. That rework is judged worth paying — it unblocks Hero/Reorderable/GlobalKey a full cycle earlier, and because the 5-phase algorithm's structure is preserved across both adaptations (only the data-structure binding changes), the re-adaptation is mechanical, not a redesign.
- **`ErrorView` wire-up uses `std::panic::catch_unwind` + `AssertUnwindSafe`.** No build profile sets `panic = "abort"`, so the catch is viable. `AssertUnwindSafe` is the boundary tool (it is safe code — compatible with both crates' `forbid(unsafe_code)`). On a caught panic the element being built is in an indeterminate state, so the unit replaces the whole element/subtree with the error view rather than reading partial state — mirroring Flutter's `ComponentElement.performRebuild`.
- **V-1 and V-13 get their separable Cycle 5 treatment only.** V-1 deletes the dead flat registry and keeps the O(depth) walk (origin R10) — the walk's deeper limitation (it traverses `ElementTree`, which the dummy-context issue leaves under-populated) is part of the deferred unification, not Cycle 5. V-13 caches one dummy `ElementBuildContext` in `BuildOwner` to kill the per-build allocation (audit option 2) — the real fix (threading a live context) also belongs to the unification.
- **`canvas::sugar` is deleted, not feature-gated** (origin R5) — invented ergonomics with no Flutter analogue.
- **Wave order** is severity- and dependency-driven, matching the Cycle 4 precedent: P0 correctness first, then feature wire-ups, then subtractive cleanup, then performance, then hygiene. Only one inter-wave dependency exists (U6 builds on U4).

---

## Open Questions

### Resolved During Planning

- **Store-by-id vs store-by-value for `VariableChildStorage` (origin Deferred-to-Planning):** store-by-value for Cycle 5 — see Key Technical Decisions.
- **`catch_unwind` unwind-safety and `panic` profile (origin Deferred-to-Planning):** resolved — no `panic = "abort"` profile; `AssertUnwindSafe` boundary; whole-subtree replacement.
- **Does anything besides the audit-named tests read the flat inherited registry (origin Deferred-to-Planning)?** Resolved — audit Appendix A.2 grep shows `register_inherited`/`unregister_inherited`/`inherited_element` have only definition sites and test callers. Safe to delete.
- **`ElementTree` `TreeWrite` cascade semantics (origin Deferred-to-Planning):** subsumed — V-7 deferred, so this question moves to the Cycle 6 unification.

### Deferred to Implementation

- Exact mechanics of adapting `reconcile_children` to the box-vec model (U5) — the 5-phase algorithm is sound; the data-structure adaptation is execution-discovery.
- Exact element-tree state guaranteed consistent after a caught build panic (U7) — the unit's invariant is whole-subtree replacement; the precise teardown of the panicked subtree is execution-discovery.
- Final LOC and the flui-engine ripple extent of `Arc<Paint>` interning (U10) — the audit's "±200 cross-cutting" estimate is soft; verify against live code before sizing the wave.
- Window-size sourcing for the root `RenderView` configuration when wiring `RootRenderView` (U6).
- Whether `flui-view`'s `ErrorView` already ships a no-builder default constructor (U7) — if not, U7 must add one.

---

## High-Level Technical Design

> *This illustrates the intended approach and is directional guidance for review, not implementation specification.*

**The element-ownership split (why V-7 is deferred).** Production today maintains two parallel structures:

```
ElementTree  (tree/element_tree.rs)          ElementCore<V,A>  (element/generic.rs) — the LIVE tree
  Slab<ElementNode>                            children: A::Storage
    └─ root ElementNode  ◄── only the root       └─ VariableChildStorage (element/child_storage.rs)
       (ElementTree::insert for non-root              └─ Vec<Box<dyn ElementBase>>   ◄── child elements
        = test-only, zero prod callers)                     └─ each Element owns its own ElementCore … (recurses)
```

`ElementTree` is the intended-future by-id tree; the box nesting is what production actually runs. V-1 (inherited lookup walks `ElementTree`), V-2 (`reconcile_children` operates on `ElementTree`), V-7 (trait impls on `ElementTree`), and V-13 (dummy `ElementTree` per build) are all symptoms of this split. Cycle 5 delivers each finding's part that does **not** require closing the split; closing it is Cycle 6.

**Wave dependency graph.** Waves are independent except where noted — they may land as parallel branches or sequentially.

| Wave | Units | Depends on | Crates |
|---|---|---|---|
| 1 — P0 correctness | U1–U4 | — | painting, view |
| 2 — Keyed reconciliation | U5 | — | view |
| 3 — Wire-ups | U6, U7 | U6 → U4 | view |
| 4 — Zombie removal | U8 | — | painting, view |
| 5 — Keep + ledger | U9 | — | painting, view |
| 6 — Hot-path performance | U10–U12 | — | painting, view, engine |
| 7 — Hygiene | U13, U14 | — | painting, view |
| 8 — Cross-crate | U15 | — | app |

U6 depends on U4 because routing the root bootstrap builds on the `Result`-returning `attach_root_widget`. All other waves are mutually independent; the order is the recommended landing sequence (correctness → features → cleanup → perf → hygiene).

---

## Implementation Units

Each unit advances one or more origin requirements. Within multi-finding units, **each finding lands as its own atomic commit** (origin R18) — the unit is the plan-level work cluster, not the commit boundary. Per-finding fix mechanics live in the audit; units cite `(audit X-N)` rather than restating them.

## Phase 1 — P0 correctness & parallel-type drift

### U1. Delete the `tessellation` module

**Goal:** Remove the 537-LOC `tessellation` module, the `tessellation` feature, and the `lyon` dependency from flui-painting — the engine owns the canonical tessellator.

**Requirements:** origin R4.

**Dependencies:** None.

**Files:**
- Delete: `crates/flui-painting/src/tessellation.rs`, `crates/flui-painting/tests/tessellation_integration.rs`, `crates/flui-painting/examples/simple_tessellation.rs` (if present)
- Modify: `crates/flui-painting/Cargo.toml` (drop `lyon` dep + `tessellation` feature), `crates/flui-painting/src/lib.rs` (drop `pub mod tessellation`), `crates/flui-painting/docs/{ARCHITECTURE,PERFORMANCE}.md`

**Approach:**
- Delete per audit P-1 fix shape. Default features become `["text"]`.
- Grep gate: `rg "flui_painting::tessellat" crates/` returns zero hits after.

**Test scenarios:**
- Test expectation: none — pure deletion of zero-consumer code. Workspace test suite must stay green.

**Verification:** `cargo build --workspace` and the per-wave gate pass; `lyon` no longer in flui-painting's dependency tree.

---

### U2. Delete the `hit_region` parallel `PointerEvent` surface

**Goal:** Remove flui-painting's `display_list::hit_region` module — a parallel `PointerEvent` type and a hit-region routing that routes nowhere (`flui-interaction` owns the canonical `PointerEvent`).

**Requirements:** origin R4.

**Dependencies:** None.

**Files:**
- Delete: `crates/flui-painting/src/display_list/hit_region.rs`
- Modify: `crates/flui-painting/src/display_list/mod.rs` (drop `hit_regions` field), `crates/flui-painting/src/canvas/mod.rs` (drop `add_hit_region`), `crates/flui-painting/src/display_list/stats.rs` + `sealed.rs` (drop `DisplayListStats::hit_regions`), `crates/flui-painting/src/lib.rs` (drop re-exports)
- Test: `crates/flui-painting/tests/canvas_unit.rs` (drop the hit-region test)

**Approach:**
- Delete per audit P-2; the `DisplayListStats::hit_regions` field becomes dead and is removed in the same wave (audit P-19); the cross-crate `PointerEvent` collision resolves (audit V-8).
- Grep gate: `rg "HitRegion|add_hit_region" crates/` returns zero hits after.

**Test scenarios:**
- Test expectation: none — pure deletion. Workspace test suite must stay green.

**Verification:** per-wave gate passes; `DisplayList` no longer carries a `hit_regions` field.

---

### U3. Delete the dead flat inherited-element registry

**Goal:** Remove `BuildOwner::inherited_elements` and its `register_inherited` / `unregister_inherited` / `inherited_element` methods (origin R10). The flat `HashMap<TypeId, ElementId>` is never populated by production and is not a faithful port of Flutter's per-element `_inheritedElements` map — wiring it would mis-scope nested same-type `InheritedView`s.

**Requirements:** origin R10.

**Dependencies:** None.

**Files:**
- Modify: `crates/flui-view/src/owner/build_owner.rs` (drop the field, the three methods, the `Debug` field entry)
- Test: `crates/flui-view/tests/inherited_dependency.rs`, `crates/flui-view/tests/build_owner_tests.rs` (drop the registry-population test setup)

**Approach:**
- The O(depth) `walk_ancestors_for_inherited` path in `ElementBuildContext::depend_on_inherited` is correct (nearest-ancestor scoping) and stays untouched.
- The inherited-dependency *system* (the ancestor walk + `InheritedBehavior::record_dependent` + `on_view_updated` notify) is complete and correct — deleting the lookup cache does not change `InheritedView` behavior.

**Test scenarios:**
- Covers AE1. Regression — existing inherited-dependency tests must stay green after the registry is removed, proving `depend_on` still resolves via the walk.
- Edge case — a test with a `Theme`-style `InheritedView` nested inside another of the same view type: a descendant's `depend_on` resolves to the *nearest* ancestor (the walk is correct here; a flat registry would not be).

**Verification:** per-wave gate passes; `rg "register_inherited|inherited_elements" crates/` returns zero hits after.

---

### U4. Convert `attach_root_widget` double-attach panic to `Result`

**Goal:** Replace the `assert!`-panic on double-attach in `WidgetsBinding::attach_root_widget` with a `Result<(), AttachError>` return (origin R14, Constitution Principle 6).

**Requirements:** origin R14.

**Dependencies:** None.

**Files:**
- Modify: `crates/flui-view/src/binding.rs` (the `attach_root_widget` body + a `thiserror`-derived `AttachError` enum)
- Test: `crates/flui-view/src/binding.rs` tests (update the `#[should_panic]` test to assert `Err(AttachError::AlreadyAttached)`)

**Approach:**
- Per audit V-12. `AttachError` is a `#[derive(Debug, thiserror::Error)]` `#[non_exhaustive]` enum.
- The flui-app call site (`crates/flui-app/src/app/binding.rs:182`) currently ignores the return; making the method `Result`-returning is a small, source-compatible ripple — flui-app may `let _ =` or surface the error. Confirm flui-app still builds.

**Test scenarios:**
- Happy path — first `attach_root_widget` on a fresh binding returns `Ok(())` and mounts the root.
- Error path — a second `attach_root_widget` without an intervening `detach` returns `Err(AttachError::AlreadyAttached)` and does not mutate state.

**Verification:** per-wave gate passes including `cargo build -p flui-app`.

---

## Phase 2 — Keyed child reconciliation

### U5. Hoist keyed reconciliation into the production child-update path

**Goal:** Replace the index-based loop in `VariableChildStorage::update_with_views` with the keyed 5-phase reconciliation algorithm, so keyed widget moves preserve element state (origin R12) — unblocking `Hero`, `Reorderable`, and `GlobalKey` reparenting.

**Requirements:** origin R12.

**Dependencies:** None.

**Files:**
- Modify: `crates/flui-view/src/tree/reconciliation.rs` (adapt `reconcile_children` to operate on the box-vec model; resolve `ReconcileAction`), `crates/flui-view/src/element/child_storage.rs` (`update_with_views` calls the adapted algorithm), `crates/flui-view/src/view/view.rs` (`View::can_update` default consults keys)
- Test: `crates/flui-view/tests/reconciliation_tests.rs` (extend), `crates/flui-view/src/tree/reconciliation.rs` unit tests

**Approach:**
- Adapt `reconcile_children` to operate on `&mut Vec<Box<dyn ElementBase>>` (the live `VariableChildStorage` model) rather than `ElementTree` + `ElementId` — store-by-value, per Key Technical Decisions. The 5-phase keyed algorithm (match-start / match-end / build-key-map / process-middle / cleanup) is sound; only the data-structure binding changes.
- `View::can_update` default updates to compare keys as well as `view_type_id` (audit V-25 / Drift A) — required for keyed matching to function.
- `ReconcileAction` (audit V-11): the adapted algorithm applies changes directly and does not need the enum — delete it rather than carry a `#[allow(dead_code)]` placeholder.
- Old-vs-new matching uses `View::key()`; un-keyed children fall back to positional matching (Flutter parity).

**Execution note:** Start with a failing test for keyed reorder state-preservation, then adapt the algorithm.

**Technical design:** *(directional)* the algorithm keeps its current phase structure; `tree.insert/update/remove` calls become box-vec operations (`Vec::push`, in-place `update`, `drain`/`swap`), and the key map is `HashMap<KeyHash, usize>` over the old box vec's indices.

**Patterns to follow:** the existing 5-phase structure in `reconcile_children`; Flutter `Element.updateChildren` (`framework.dart:4125`).

**Test scenarios:**
- Covers AE4. Happy path — a `Variable`-arity element whose children carry keys, child list reordered between builds: children are matched by key and element state is preserved (not rebuilt by index).
- Happy path — un-keyed children: positional matching behaves as the old index loop did.
- Edge case — empty new list (all children removed), empty old list (all children created), single-element lists.
- Edge case — new list is a strict reorder of the old (no adds/removes); a prepend; an append; a middle insert + middle remove combined.
- Error path — duplicate keys in the new view list: defined, non-panicking behavior (document the resolution — e.g. first-wins).
- Integration — a child with a `GlobalKey` moved to a new slot retains its associated element/state across the rebuild.

**Verification:** per-wave gate passes; `reconcile_children` has a production caller; keyed reorder tests pass.

---

## Phase 3 — Wire forward-looking half-impls to their consumers

### U6. Route the root-widget bootstrap through `RootRenderView`

**Goal:** Wire `WidgetsBinding::attach_root_widget` to bootstrap through `RootRenderView` / `RootRenderElement` and remove the parallel direct-mount path (origin R8), mirroring Flutter's `attachRootWidget` → `RenderObjectToWidgetAdapter` bootstrap.

**Requirements:** origin R8.

**Dependencies:** U4 (builds on the `Result`-returning `attach_root_widget`).

**Files:**
- Modify: `crates/flui-view/src/binding.rs` (`attach_root_widget` wraps the user view in `RootRenderView` before mounting), `crates/flui-view/src/view/root.rs` (if `RootRenderView::new` needs an ergonomic constructor for the binding path)
- Test: `crates/flui-view/src/binding.rs` tests

**Approach:**
- `attach_root_widget` constructs `RootRenderView::new(user_view, width, height)` and mounts that via `mount_root_with_pipeline_owner`. `RootRenderView<V>` already implements `View`, so it slots into the existing `&dyn View` parameter without a signature change. `RootRenderElement` already implements `ElementBase` / `RenderObjectElement` / `RenderTreeRootElement` — only the binding's call into it is missing.
- Remove the direct-mount path so only one bootstrap path exists.
- The flui-app call site is unchanged — wiring is intra-`flui-view`.
- Open: window-size sourcing for the root `RenderView` configuration (see Deferred to Implementation).

**Test scenarios:**
- Covers AE3. Happy path — `attach_root_widget` on a fresh binding creates and mounts the root through `RootRenderView`; a grep of the crate finds no second direct-mount path.
- Integration — the mounted root element produces a working render-tree root (`PipelineOwner` wired) as before the change.
- Edge case — root view with zero children vs. a child subtree both bootstrap correctly.

**Verification:** per-wave gate passes including `cargo build -p flui-app`; only one root-mount path remains.

---

### U7. Wire `ErrorView` to a build-panic catch

**Goal:** Make `Element::perform_build` catch a panicking user `build()` and substitute the registered error view, instead of unwinding the frame (origin R9) — mirroring Flutter's `ComponentElement.performRebuild`.

**Requirements:** origin R9.

**Dependencies:** None.

**Files:**
- Modify: `crates/flui-view/src/element/behavior.rs` — the user-`build()` call in `StatelessBehavior::perform_build` and the `state.build(...)` call in `StatefulBehavior::perform_build`; `crates/flui-view/src/view/error.rs` (construct the error view from a caught panic)
- Test: `crates/flui-view/tests/` (new error-view integration test)

**Approach:**
- Two sites carry a user `build()`: the `core.view().build(&ctx)` expression in `StatelessBehavior::perform_build` and the `state.build(...)` expression in `StatefulBehavior::perform_build`. Wrap **only that single build expression** in `std::panic::catch_unwind(AssertUnwindSafe(...))`, capturing the returned child view.
- The `catch_unwind` must not extend over `behavior_commons::finish_single_child_build` / `core.update_or_create_child` — a panic caught mid-child-update would leave `core` half-mutated. On `Err`, replace the captured child view with the error view *before* the child-update helper runs, so the helper is never entered with a panicked `core`.
- `ProxyBehavior` / `InheritedBehavior` build via `build_proxy_style` (a non-panicking child accessor, not a user `build()`) — they need no catch.
- On `Err`, build a `FlutterError` from the panic payload and construct the error view via the registered `ErrorViewBuilder`.
- If no `ErrorViewBuilder` is registered, fall back to a built-in default error view (Flutter parity: `ErrorWidget.builder` defaults to a built-in). Confirm `flui-view`'s `ErrorView` ships a no-builder default constructor; add one if it does not (see Open Questions).

**Execution note:** Start with a failing integration test: a view whose `build()` panics, asserting the error view renders.

**Test scenarios:**
- Covers AE2. Happy path — with a registered error-view builder, a view whose `build()` panics: `perform_build` catches it and the element renders the error view; the frame does not unwind.
- Edge case — panic in a nested child's `build()`: only that subtree is replaced; siblings are unaffected.
- Edge case — no builder registered: a default error view renders, still no unwind.
- Error path — confirm the caught panic does not leave dangling render-tree or dirty-list state.

**Verification:** per-wave gate passes; a build-panic test proves the error view substitutes without aborting the process.

---

## Phase 4 — Genuine zombie removal

### U8. Delete remaining genuine-zombie surface

**Goal:** Delete the genuine zombies and parallel-type drift not already handled in Wave 1 (origin R4, R5).

**Requirements:** origin R4, R5.

**Dependencies:** None.

**Files:**
- Delete: `crates/flui-painting/src/canvas/sugar/` (whole directory), `crates/flui-painting/src/text_layout/fallback.rs`, `crates/flui-view/src/element/notification.rs` (the parallel-dispatch types only)
- Modify: `crates/flui-painting/Cargo.toml` (drop `canvas-sugar` feature; make `text` non-optional), `crates/flui-painting/src/canvas/{mod,composition}.rs` (drop `sugar` module + `Canvas::record`/`build`), `crates/flui-painting/src/text_layout/mod.rs` (drop the `#[cfg(not(feature = "text"))]` arms), `crates/flui-painting/src/lib.rs` (drop `Picture` alias + re-exports), `crates/flui-view/src/binding.rs` (drop `SharedWidgetsBinding` + `create_shared_binding`), `crates/flui-view/src/lib.rs` + `element/mod.rs` (re-export trims)

**Approach:** one atomic commit per finding —
- `canvas::sugar` (audit P-4, P-14, P-15) — deleted outright, not feature-gated (origin R5): invented ergonomics, no Flutter analogue.
- `text_layout::fallback` parallel `TextLayout` (audit P-3) — deleted; the `text` feature becomes non-optional. The `--no-default-features` build path changes shape, but no workspace consumer disables defaults — note in the changelog.
- `Picture` type alias (audit P-9) — deleted; `DisplayList` is the canonical name.
- `NotificationNode` / `NotificationHandler` / `BoxedNotification` / `NotificationCallback` (audit V-9) — deleted; the unified `ElementBase::on_notification` protocol is the live path. The `Notification` trait and the typed notification structs are kept.
- `SharedWidgetsBinding` + `create_shared_binding` (audit V-10) — deleted; `#[deprecated]` since 0.2.0, zero callers.

**Test scenarios:**
- Test expectation: none — pure deletion of zero-consumer code. Workspace test suite must stay green.
- Per-deletion grep gates: `rg "draw_pill|canvas::sugar" crates/`, `rg "fallback::TextLayout" crates/`, `rg "flui_painting::Picture" crates/`, `rg "NotificationNode|NotificationHandler" crates/`, `rg "SharedWidgetsBinding" crates/` each return zero hits.

**Verification:** per-wave gate passes; all grep gates clean.

---

## Phase 5 — Forward-looking port-targets: keep + ledger

### U9. Ledger and de-collide the forward-looking port modules

**Goal:** Keep the forward-looking Flutter port-targets compiled in the default build, mark each with a `// PORT-TARGET:` ledger comment naming the consumer it waits on (origin R3, R6), and rename the colliding `AnimationBehavior` struct (origin R7).

**Requirements:** origin R3, R6, R7.

**Dependencies:** None.

**Files:**
- Modify: `crates/flui-view/src/view/animated.rs` + `crates/flui-view/src/element/behavior.rs` (ledger comment; rename the `AnimationBehavior` struct), `crates/flui-view/src/view/parent_data.rs` (ledger comment), `crates/flui-painting/src/text_painter/mod.rs` + `crates/flui-painting/src/text_layout/layout.rs` (ledger comment), `crates/flui-view/src/lib.rs` (re-export rename)

**Approach:**
- Each port-target module gets a module-head `// PORT-TARGET: <consumer>, <crate>` comment — e.g. `ParentDataView` → `flui-widgets::Flexible`/`Positioned`; `AnimatedView` → `flui-widgets` animated widgets + `flui-animation`; `TextPainter` → `flui-widgets::RichText`/`TextField`. None is feature-gated (origin R2 — gating default-off would exile correct code from `just ci`).
- Rename flui-view's `AnimationBehavior` struct to remove the collision with `flui-animation::AnimationBehavior` (enum). `AnimatedBehavior` matches the `<ViewKind>Behavior` convention (`StatelessBehavior`, `InheritedBehavior`, …); final name is the implementer's call.

**Test scenarios:**
- Covers AE5. The ledger comment is present at each port-target module head and names a concrete future consumer.
- Test expectation for the rename: none behavioral — a pure rename; existing tests referencing the type update and stay green.

**Verification:** per-wave gate passes; `rg "AnimationBehavior" crates/flui-view` finds only the renamed symbol; no name collision with `flui-animation`.

---

## Phase 6 — Hot-path performance

### U10. Intern `Paint` to remove the per-draw clone

**Goal:** Eliminate the per-`draw_*`-call `Paint` clone (~80–200 bytes, plus `Box<Shader>` heap alloc) by interning `Paint` behind `Arc` at recording time (origin R15, audit P-7).

**Requirements:** origin R15.

**Dependencies:** None.

**Files:**
- Modify: `crates/flui-painting/src/display_list/command.rs` (the 29 `DrawCommand` variants carry `Arc<Paint>`), `crates/flui-painting/src/canvas/drawing.rs` + `crates/flui-painting/src/canvas/mod.rs` (interning pool), `crates/flui-engine/src/wgpu/` (dispatch reads through `Arc::as_ref`)
- Test: `crates/flui-painting/tests/` + a benchmark

**Approach:**
- `DrawCommand` variants carry `paint: Arc<Paint>`; `Canvas` maintains an interning pool so identical paints share storage (first use costs a clone, repeats become an `Arc` refcount bump).
- This is the largest performance unit and the only one crossing into `flui-engine` (dispatch updates to read `Arc::as_ref`). The audit's "±200 LOC cross-cutting" estimate is soft — verify against live code and re-size the wave if it diverges (Cycle 4 meta-learning).
- **Split trigger:** if the `flui-engine` changes exceed read-only `Arc::as_ref` / deref substitutions at existing dispatch call sites — i.e., any engine-side data structure or API shape changes — stop and split into U10a (flui-painting `Arc<Paint>` interning) and U10b (flui-engine dispatch update), landing U10a in Wave 6 and U10b as a follow-up. Expect the engine touch to span the `render_*` call sites in `crates/flui-engine/src/commands.rs` plus the `Option<Paint>` variants becoming `Option<Arc<Paint>>`.

**Execution note:** Bench the per-frame paint-clone cost before and after.

**Test scenarios:**
- Happy path — recording two `draw_*` calls with identical `Paint` produces two `DrawCommand`s sharing one `Arc<Paint>`.
- Happy path — distinct paints get distinct `Arc`s; the rendered output is unchanged.
- Integration — the engine dispatch path renders an interned `DrawCommand` identically to the pre-change `Paint`-by-value path.
- Benchmark — a 100-draw frame shows reduced allocation vs. the pre-change baseline.

**Verification:** per-wave gate passes including `cargo build -p flui-engine` and `cargo test -p flui-engine`; benchmark shows no regression and a measured allocation reduction.

---

### U11. Painting hot-path cleanups

**Goal:** Land the bounded painting performance findings (origin R15).

**Requirements:** origin R15.

**Dependencies:** None.

**Files:**
- Modify: `crates/flui-painting/src/canvas/composition.rs` (P-11), `crates/flui-painting/src/display_list/{command,command_ops}.rs` (P-13), `crates/flui-painting/src/canvas/drawing.rs` (P-6), `crates/flui-painting/src/canvas/{state,clipping,mod}.rs` (P-8)

**Approach:** one atomic commit per finding —
- `append_display_list_at_offset` (audit P-11): apply the offset as a paint-time transform hint instead of an O(N) clone-and-rewrite — bench before/after.
- `DrawCommand::kind()` (audit P-13): `kind()` is already a compact or-pattern match (`command_ops.rs`), not the 29-arm match the audit describes — gate this finding on a before-benchmark; keep the `#[repr(u8)]`-discriminant change only if `kind()` filtering shows as a measured hot spot, otherwise drop P-13 as already-adequate.
- `draw_polyline` (audit P-6): `windows(2)` idiom.
- `ClipShape` (audit P-8): replace the data-carrying enum with a `usize` clip-depth counter — the variant payloads are stored but never read.

**Test scenarios:**
- Happy path (P-11) — appending a display list at a non-zero offset produces the same rendered geometry as before; benchmark shows the O(N) bake is gone.
- Happy path (P-13) — `kind()` returns the correct `CommandKind` for a sample of all variant categories.
- Happy path (P-6) — `draw_polyline` over N points records N−1 line segments; N<2 records nothing.
- Edge case (P-8) — save/restore nesting reports the correct depth via the counter; `save_count` semantics unchanged.

**Verification:** per-wave gate passes; P-11 benchmark shows no regression.

---

### U12. View hot-path cleanups

**Goal:** Land the bounded flui-view performance findings (origin R15).

**Requirements:** origin R15.

**Dependencies:** None.

**Files:**
- Modify: `crates/flui-view/src/binding.rs` (`collect_all_elements`, V-16), `crates/flui-view/src/owner/build_owner.rs` (cached dummy context, V-13), `crates/flui-view/src/context/element_build_context.rs` (V-13)

**Approach:** one atomic commit per finding —
- `collect_all_elements` (audit V-16): replace the recursive `Vec::extend` (O(N²) worst case) with an iterative stack walk into a single `Vec::with_capacity(tree.len())` — O(N).
- Dummy `ElementBuildContext` (audit V-13, option 2): cache one dummy context in `BuildOwner` and reuse it, instead of allocating two `Arc<RwLock<…>>` per stateless build. This is the cheap, separable part of V-13; the real fix (threading a live context) is part of the deferred element-ownership unification.

**Test scenarios:**
- Happy path (V-16) — `collect_all_elements` over a known tree returns every `(ElementId, depth)` pair with correct depths.
- Edge case (V-16) — a deep unbalanced chain and a wide shallow tree both traverse correctly; result ordering is stable.
- Happy path (V-13) — repeated stateless builds reuse one cached dummy context; no per-build `Arc` allocation.

**Verification:** per-wave gate passes; `collect_all_elements` is allocation-bounded.

---

## Phase 7 — Hygiene & API discipline

### U13. API-discipline and doc-cadence hygiene

**Goal:** Land the P2/P3 hygiene findings (origin R16) — `#[non_exhaustive]`, visibility trims, dead-accessor removal, deadlock-safe event dispatch, and doc-cadence markers.

**Requirements:** origin R16.

**Dependencies:** None.

**Files:**
- Modify: `crates/flui-view/src/element/lifecycle.rs` (V-17), `crates/flui-view/src/element/render_object_element.rs` (V-22), `crates/flui-painting/src/binding.rs` (P-10), `crates/flui-view/src/owner/build_owner.rs` (V-15), `crates/flui-view/src/binding.rs` (V-21), plus doc files for P-12/P-16/P-17/P-18/V-24

**Approach:** one atomic commit per finding —
- `#[non_exhaustive]` on `Lifecycle` (audit V-17) and `RenderSlot` (audit V-22).
- `SystemFontsNotifier` demoted to `pub(crate)` (audit P-10) — no platform trigger exists yet.
- Delete the `#[allow(dead_code)]` `DirtyElement::depth` / `InactiveElement::depth` accessors (audit V-15) — `Ord` uses direct field access.
- Snapshot-then-fire on the sync `WidgetsBinding::handle_*` event handlers (audit V-21) — clone the observer `Vec` before iterating so an observer callback cannot deadlock on the binding lock. This is the one behavioral item in the unit (a deadlock-safety fix).
- Doc-cadence markers and doc fixes (audit P-12, P-16, P-17, P-18, V-24): `// REMOVE_BY:` / `// REVIEW_BY:` markers on outstanding-refactor lists and the predictive-back-gesture surface; the re-export-shape note for `Paint`/`Path`.
- Audit P-20 (`#![forbid(unsafe_code)]`) and V-18 (`Box<dyn Fn>` over `Arc`) are audit no-action findings — confirmed correct as-is, no change.

**Test scenarios:**
- Edge case — `#[non_exhaustive]` additions may surface match-exhaustiveness errors in the test harness; update those matches.
- Happy path (V-21) — an event handler whose observer callback re-enters the binding does not deadlock.
- Test expectation for the doc/visibility/accessor-deletion items: none behavioral — workspace test suite must stay green.

**Verification:** per-wave gate passes; `cargo doc --no-deps` introduces zero new warnings.

---

### U14. Wire `did_change_dependencies` to inherited updates

**Goal:** Wire `InheritedBehavior::on_view_updated` to invoke the typed `did_change_dependencies` lifecycle hook on each dependent's state (origin R16 — audit V-19) — Flutter parity. V-19 is a behavioral correctness/parity wire-up, not a cosmetic hygiene item; it sits in the hygiene wave only for sequencing, and is kept as its own unit for that reason.

**Requirements:** origin R16.

**Dependencies:** None.

**Files:**
- Modify: `crates/flui-view/src/element/behavior.rs` (`InheritedBehavior::on_view_updated`), `crates/flui-view/src/view/stateful.rs` (the `ViewState::did_change_dependencies` hook)
- Test: `crates/flui-view/tests/inherited_dependency.rs`

**Approach:**
- When `update_should_notify` is true, in addition to scheduling each dependent's rebuild, invoke the dependent state's typed `did_change_dependencies` hook via the `state_as_any` accessor + `TypeId` check.
- This is a real behavioral addition (dependents gain a typed callback) — kept as its own unit, not folded into U13's hygiene bundle.

**Test scenarios:**
- Integration — updating an `InheritedView` with `update_should_notify == true` calls `did_change_dependencies` on each dependent's state exactly once, before the dependent rebuilds.
- Edge case — `update_should_notify == false`: `did_change_dependencies` is not called.
- Edge case — a dependent that does not override `did_change_dependencies` (empty default impl) is unaffected.

**Verification:** per-wave gate passes; the inherited-dependency test asserts the typed hook fires.

---

## Phase 8 — Cross-crate parallel-type cleanup

### U15. Delete the parallel `Color` in flui-app

**Goal:** Delete `flui-app`'s parallel `Color` / `ColorScheme` and migrate to the canonical `flui_types::Color` (origin R17, audit V-14).

**Requirements:** origin R17.

**Dependencies:** None.

**Files:**
- Modify/Delete: `crates/flui-app/src/theme/colors.rs` (delete the parallel `Color`; rebuild `ColorScheme` on `flui_types::Color` if `ColorScheme` is retained), `crates/flui-app/src/theme/mod.rs` (re-export trims)

**Approach:**
- Delete the `f32`-channel `Color` struct (zero in-workspace consumers — audit-verified). `ColorScheme` (semantic tokens) is a reasonable flui-app concept; if retained, it is built from `flui_types::Color` values, not the parallel struct.

**Test scenarios:**
- Test expectation: none — deletion of zero-consumer code. `cargo build -p flui-app` and its test suite must stay green.
- Grep gate: `rg "flui_app::theme::.*Color" crates/` returns zero hits.

**Verification:** per-wave gate passes including `cargo build -p flui-app`.

---

## System-Wide Impact

- **Interaction graph:** U6 changes the root-mount path inside `flui-view`; the `flui-app` entry point (`attach_root_widget`) is preserved. U7 inserts a panic boundary into the build phase. U14 adds a dependent-notification callback to inherited updates.
- **Error propagation:** U4 converts a panic to `Result<(), AttachError>`; U7 converts user-`build()` panics into a rendered error view instead of a process abort.
- **State lifecycle risks:** U7 — a caught build panic leaves the element under construction in an indeterminate state; the unit's invariant is whole-subtree replacement. U5 — keyed reconciliation must not leak or double-unmount child elements during reorder.
- **API surface parity:** U4 (`attach_root_widget` signature), U9 (`AnimationBehavior` rename), U8 (`Picture`, `SharedWidgetsBinding`, sugar removals) change public surfaces — all verified zero-external-consumer or single-internal-caller.
- **Cross-crate ripple:** U10 (`Arc<Paint>`) reaches into `flui-engine` dispatch; U4 lightly touches `flui-app`; U15 is in `flui-app`. The per-wave gate builds and tests the affected crate explicitly.
- **Integration coverage:** U5 (keyed reorder state preservation), U6 (root render-tree wiring), U7 (panic boundary), U10 (engine dispatch of interned paint), U14 (inherited-update callback) need integration tests — unit tests with mocks will not prove them.
- **Unchanged invariants:** the three-tree architecture, the inherited-dependency *system* (walk + `record_dependent` + notify), the arity child-storage model, and the dependency DAG are explicitly unchanged. The element-ownership split is documented but **not** resolved this cycle.

---

## Risks & Dependencies

| Risk | Mitigation |
|------|------------|
| Audit blast-radius estimates are soft (Cycle 4: 10× miss) — V-2 ("±200-400"), P-7 ("±200 cross-cutting") | Treat as hypotheses; verify against live code at wave start; re-size or split the wave if it diverges. V-7 has already been reshaped on this basis. |
| `Arc<Paint>` (U10) crosses into `flui-engine` — scope creep beyond painting × view | Keep the engine change mechanical (`Arc::as_ref` at dispatch); the per-wave gate builds and tests `flui-engine`; if the ripple is larger than mechanical, split U10 into its own follow-up. |
| `catch_unwind` (U7) — element-tree state after a caught panic | Whole-subtree replacement invariant; no `panic = "abort"` profile exists; integration test asserts no dangling render-tree / dirty-list state. |
| Keyed reconciliation (U5) on the box-vec model could double-unmount or leak elements on reorder | Test scenarios cover reorder/prepend/append/insert+remove and `GlobalKey` move; start test-first. |
| V-7 deferral leaves V-1/V-13 with partial fixes | Documented explicitly in Key Technical Decisions and Scope Boundaries; the partial fixes are net-positive (dead code removed, allocation reduced) and do not regress behavior. |
| `flui-app --lib` singleton-state test flake (pre-existing) | Run `flui-app` lib tests with `--test-threads=1` per the Cycle 4 receipts. |
| `#[non_exhaustive]` (U13) surfaces match-exhaustiveness breaks in the test harness | Expected; fix the matches in the same commit. |
| Cycle 6 is pre-loaded with six deferred findings (V-7, V-1's real fix, V-2's faithful store-by-id shape, V-13's real fix, V-20, V-23) but is not yet scheduled — the origin names three competing Cycle 6 directions | The element-ownership unification is the **recommended and expected** immediate next cycle. If Cycle 6 is routed elsewhere, the two-ownership-model split and store-by-value reconciliation persist as a tracked known-incomplete state; recording the V-7 reshape in the audit + requirements doc (see Documentation / Operational Notes) ensures a re-audit re-surfaces it rather than silently re-flagging. |

---

## Documentation / Operational Notes

- Each wave lands as one PR (one squash-merge on `main`), with one atomic commit per finding on the branch, conventional-commit format, finding IDs in the subject, and the `Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>` trailer — matching the PR #108–#117 precedent. Suggested branch naming: `feat/painting-view-cycle5-wave<N>`.
- Per-wave verification gate (origin R19): `cargo build --workspace`; `cargo clippy --workspace --all-targets -- -D warnings`; `cargo test -p <touched-crate> --lib`; `cargo doc -p <crate> --no-deps` (zero *new* warnings); `bash scripts/port-check.sh -v` (7 triggers); the per-finding `rg` grep-gates listed in the deletion units.
- Produce a Cycle 5 receipts document mirroring `docs/research/2026-05-22-cycle4-wave2-receipts.md` (commit-by-commit ledger + grep gates) as waves land.
- Update the audit document's status and the requirements doc to record that V-7 was reshaped to a Cycle 6 deferral.
- `text` becoming a non-optional feature (U8) is a packaging change — note it in the changelog.

---

## Sources & References

- **Origin document:** [docs/brainstorms/flui-painting-view-cycle5-requirements.md](../brainstorms/flui-painting-view-cycle5-requirements.md)
- **Audit:** [docs/research/2026-05-22-flui-painting-view-audit.md](../research/2026-05-22-flui-painting-view-audit.md) — per-finding evidence and fix shapes
- **Wave-execution templates:** [docs/research/2026-05-22-cycle4-wave2-receipts.md](../research/2026-05-22-cycle4-wave2-receipts.md), [docs/research/2026-05-22-cycle4-wave2-design.md](../research/2026-05-22-cycle4-wave2-design.md)
- Related code: `crates/flui-tree/src/traits/`, `crates/flui-rendering/src/storage/tree.rs` (V-7 Cycle 6 model), `crates/flui-view/src/tree/element_tree.rs`, `crates/flui-view/src/element/child_storage.rs`
- Flutter parity anchors: `.flutter/flutter-master/packages/flutter/lib/src/widgets/binding.dart` (root bootstrap), `.flutter/flutter-master/packages/flutter/lib/src/widgets/framework.dart` (`performRebuild` error catch, `updateChildren` reconciliation)
- Predecessor PRs: #108–#117 (Cycle 4)
