[← V-7 ownership design](2026-06-09-v7-ownership-unification-design.md) · [Architectural contracts](2026-05-22-architectural-contracts.md) · [Plan](2026-06-08-beat-flutter-plan.md)

# Wave 2 = KTD-9 Element-Storage Migration (working setState, the quality way)

> **Supersedes** the V-7 bootstrap-swap framing. **Source:** chief-architect design + harsh-critic adversarial workflow (2026-06-09), grounded in the post-Core-Contracts-Phase-0-3 code. **Adversarial verdict: needs-revision** — fixes folded into the plan below. User mandate: **no quick-win, no MVP.**

## Why setState is a no-op today (settled ground truth)

- `ElementKind` closed enum exists (`element/kind.rs`, 8 variants, `#[non_exhaustive]`) but has **zero construction sites** — pure shape, a migration gap.
- The `Slab<ElementNode>` holds **only the root** in production (`len()==1`). Every deeper element is owned hierarchically as `Option/Vec<Box<dyn ElementBase>>` in `child_storage.rs` — **not** addressable by `ElementId`.
- `ElementBase::visit_children` is a **no-op** on the unified production element ("children managed internally"); `RootRenderElement.child_element` is a `Box`, its `visit_children` a stub.
- `Element::set_state` flips only a local `AtomicBool` (no `schedule_build_for` heap insert) and is a method on the concrete `Element<…>` — **unreachable through `dyn ElementBase`**.
- `build_scope` reaches elements only via `tree.get_mut(ElementId)` → can only ever reach the root.
- The keyed reconciler (Phase 2) operates on the **box-vec**, not slab ids (its own header says so).
- `architectural-contracts.md:54`'s "already true of the code" is **false** (predates the phases).

**The fix is the KTD-9 id-based storage** the codebase has been naming as the prerequisite: make every element slab-resident and `ElementId`-addressable, so `schedule_build_for` reaches deep dirty elements, `build_scope` rebuilds + reconciles them, and `setState` is reachable through `dyn`.

## Approach (C1+C4+C6+C9 end state)

One `Slab<ElementNode>` is the single source of truth; children are `ElementId` **edges** (Single=`Option<ElementId>`, Optional=`Option<ElementId>`, Variable=`Vec<ElementId>`), not a second `Box` ownership graph. `dyn` survives only at the sanctioned C9 points (the Slab slot + the `Vec<BoxedView>` fallback). `setState` → `owner.schedule_build_for(self_id, depth)` (AtomicBool flip **and** depth-heap insert in one act — the locked sentence) → wake chain. `build_scope` (already depth-shallow-first, dedup) reaches every dirty element; `perform_build` reconciles children by id+key via the re-pointed keyed reconciler; unmount via `tree.remove` (soft-remove + GlobalKey retake). Production bootstraps through `attach_root_widget` (slab-resident root); `AppBinding.root_element`/`mount_root`/`rebuild_root` are **deleted**. This is Flutter's `BuildOwner.buildScope → Element.rebuild → updateChild(ren)` exactly.

## Revised unit sequence (adversarial fixes folded in)

- **U0a — ADR: ElementKind enum vs struct-with-`dyn`Box** *(owner decision, before any irreversible storage commit)*. Current `ElementNode` is a struct holding `Box<dyn ElementBase>`; the contract specifies the closed enum (0 construction sites). U1 commits the storage shape irreversibly — the ADR that governs C4 must be ratified **first**. Evidence: 0 enum sites, the C9 sanctioned-dyn registry, Flutter's own dynamic Element dispatch.
- **U0b — Borrow-architecture design** *(FATAL blocker for U1)*. `build_scope` holds `&mut node` while child storage needs the slab → the recursive-reborrow UB PR #144/#145 fixed. Define `ElementChildStorage` over a **pre-acquired subtree handle** (`get_subtree_mut`+`SubtreeBorrows`+`NodePtr`, distinct-slot Unique tags, Miri-clean), NOT a fresh `&mut ElementTree`. Cite the PR #145 pattern as the required shape.
- **U0c — Lock-order invariant + deadlock-safe setState**. Global order `widgets.write()` **before** `pipeline_owner.write()`, never reverse. `setState`/wake chain flip an AtomicBool + enqueue on a **side-queue**; never re-enter `widgets.write()` from a build/wake callback (mirror `finalize_tree` `mem::take` snapshot + V-21 observer-snapshot). Debug re-entrancy guard.
- **U0d — Full-tree-dirty animation bench** (bench-fidelity discipline). Memo prunes **nothing** on animated views (config changes every frame); slab `get_mut` + per-iter `ElementOwner` handle rebuild may regress the 60fps full-rebuild path vs the box deref. Measure before U1; no perf claim without the number.
- **U1+U2 (ATOMIC) — id-based storage + id-based keyed reconciler**. `ElementChildStorage` impls store `ElementId` edges over the U0b subtree handle; `reconcile_children` re-pointed to `&mut Vec<ElementId>` + slab, matching by `node.key()/key_hash()`; inflate=`tree.insert`, reuse=`tree.get_mut().update`, remove=`tree.remove`. **Must land as one commit** (mixed box-vec/id storage is silently inconsistent + leaks on unmount). Verify = structural invariant: every `visit_children`-reachable node is slab-resident, `visit_children` non-empty where `len>0`, the `unified.rs:186` no-op stub is killed here.
- **U3 — real tree-bound `BuildContext`** on the build path (replace `new_minimal` at `behavior.rs:242/365/452`) so `ctx.set_state`/`mark_needs_build`/`depend_on_inherited` reach the owner + real ancestors (C5, co-locked).
- **U4 — setState reaches the heap** (id-routed via ctx/StateHandle → `schedule_build_for`; no signals).
- **U5 — slab-resident root + delete the parallel root path** (`RootRenderElement.child_id`; delete `AppBinding.root_element`+`mount_root`+`rebuild_root`; resolve the duplicate `RootElementImpl` vs `RootRenderElement`). Not deferred — leaving it is a permanent third-tree false-green.
- **U6 — wake chain** (`set_on_need_frame(request_redraw)`; confirm `on_build_scheduled`).
- **U7 — GATE: end-to-end setState round-trip** driving `AppBinding::render_frame` (production paint path) against a real/headless renderer, asserting the **scene/render-tree changed** on a 3-level-deep stateful element. **Must fail on current main first** (proves detection power). The only test that detects the production no-op.
- **Plus units** (from adversarial must-fix): GlobalKey cross-parent Active→Active reparent (`from_parent: Some`, state transfer — newly enabled by U1, currently unimplemented → Hero/Reorderable state loss); per-arity slab-index-reuse contracts (Optional clear / Single replace must null the id on soft-remove-returns-None; generation check or assert-on-stale on `ElementId` resolution — Slab reuses freed indices); mid-build dirty semantics (deeper-coalesce / shallower-defer / re-entrant-idempotent; `InheritedBehavior:802` mid-build schedule must not double-borrow the `ElementOwner` split-borrow).

## Two strategic decisions (owner)

1. **C4 ADR (U0a): does the slab store the closed `ElementKind` enum, or stay struct-with-`dyn`Box?** Hard to reverse once children migrate. Locked-contract territory.
2. **Greenlight the multi-PR KTD-9 migration program?** This is the framework spine — foundational, UB/deadlock/state-loss-sensitive, multiple PRs. Not a single session.

## Status

Design vetted (chief-architect + harsh-critic). Verdict needs-revision → the U0 prerequisites (ADR + borrow-arch + lock-order + bench) are now front-loaded. Implementation is **blocked on the two owner decisions** above. This is the correct outcome of designing before coding a locked-contract spine change.
