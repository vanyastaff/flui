---
date: 2026-05-23
topic: d-block-architecture-decisions
status: decision-memo
---

# D-Block Architecture Decision Memo

**Purpose.** Resolve the architecture forks that pre-plan-write multi-agent review surfaced as P0 blockers in [`pipeline-wiring-d-block-requirements.md`](../brainstorms/pipeline-wiring-d-block-requirements.md). Each decision below is binding for plan-write; deviations require an explicit Key Decision entry with rationale.

**Origin reviews.** ce-architecture-strategist, ce-feasibility-reviewer, ce-scope-guardian-reviewer, ce-adversarial-document-reviewer, all 2026-05-23.

**Status post-decision.** Brainstorm doc requires revision (false premise, missing requirements, scope corrections). Plan-write proceeds only after brainstorm rev2 lands.

---

## D1 — Borrow architecture for parent→child layout recursion

**Fork.** `LayoutChildCallback = &'ctx (dyn Fn(RenderId, BoxConstraints) -> Size + Send + Sync)` at `crates/flui-rendering/src/protocol/box_protocol.rs:187-188` cannot capture `&mut PipelineOwner` (`Fn`, not `FnMut`; interior mutability forbidden by PORT.md triggers #1+#2).

**Decision.** Use `RenderTree::get_parent_and_children_mut(parent_id)` disjoint-borrow primitive at `crates/flui-rendering/src/storage/tree.rs:215-301`. Parent's `perform_layout` receives a context constructed from the parent + child mut slice borrowed disjointly from the slab. **Callback type changes** from `&dyn Fn(RenderId, BoxConstraints) -> Size` to a `BoxLayoutCtx` field carrying `&mut [&mut RenderEntry<BoxProtocol>]` for children, indexed by per-parent child position.

**Rationale.** Cycle 4 PR #109 net `-1037 LOC` deliberately removed interior-mutability shapes from this area; restoring them via `RefCell`/`Mutex` re-introduces the U2 anti-pattern PORT.md forbids. The disjoint-borrow primitive was built for exactly this case (Cycle 4 storage hygiene). Bonus: synchronous, single-threaded, no closure capture gymnastics.

**API shape sketch.**

```rust
// pipeline/owner.rs
impl PipelineOwner<Layout> {
    fn layout_dirty_root(&mut self, id: RenderId, constraints: BoxConstraints) -> RenderResult<()> {
        // 1. Cycle guard (see D6)
        if !self.currently_laying_out.insert(id) {
            return Err(RenderError::LayoutCycle(id));
        }
        // 2. Disjoint borrow parent + its direct children
        let (parent, children) = self.render_tree.get_parent_and_children_mut(id)?;
        // 3. Construct typed context with mut child slice
        let mut ctx = BoxLayoutCtx::new(constraints, children);
        // 4. Invoke RenderObject → RenderBox bridge (see D5)
        let size = parent.as_box_mut().unwrap().layout_with_ctx(&mut ctx)?;
        // 5. Cycle release + state update
        self.currently_laying_out.remove(&id);
        parent.state_mut().set_geometry_replace(size.into());
        Ok(())
    }
}
```

**Out of scope.** Async layout. Cross-thread layout. Both deferred indefinitely.

---

## D2 — `RenderState::set_constraints` + `set_geometry` re-entry safety

**Fork.** `OnceCell`-shaped `state.constraints` (`crates/flui-rendering/src/storage/state/constraints.rs:38`) and `state.geometry` (`crates/flui-rendering/src/storage/state/geometry.rs:71`) PANIC on second set. R1 wiring makes pipeline panic on frame 2 of any production binding.

**Decision.** Replace `OnceCell<T>` with `Option<T>` in both `RenderState::constraints` and `RenderState::geometry`. Methods take `&mut self` (already required by `RenderEntry::layout`'s `&mut self`). Drop panic-on-already-set. Add `set_constraints_replace(&mut self, c: ProtocolConstraints<P>)` and `set_geometry_replace(&mut self, g: ProtocolGeometry<P>)` that perform straight assignment.

**Rationale.** Flutter `.flutter/flutter-master/packages/flutter/lib/src/rendering/object.dart:2865` does `_constraints = constraints` unconditionally (after the early-out cache-hit check). The OnceCell shape was a defensive premature optimization that never fired in production because `RenderEntry::layout` had zero production callers. Removing it costs nothing and enables re-layout.

**Migration.** `entry.rs:281-283` becomes:

```rust
// storage/entry.rs — RenderEntry::layout after perform_layout_raw success path
if let Some(cached) = self.state.constraints() {
    if cached == &constraints && !self.state.needs_layout() {
        return Ok(self.state.geometry().cloned().unwrap_or_default());
    }
}
let geometry = catch_unwind(AssertUnwindSafe(|| self.render_object.perform_layout_raw(constraints.clone())))?;
self.state.set_geometry_replace(geometry.clone());
self.state.set_constraints_replace(constraints);
self.state.clear_needs_layout();
Ok(geometry)
```

The cache-hit short-circuit at the top is the Flutter `if (!_needsLayout && constraints == _constraints) return;` pattern.

**Out of scope.** Atomic re-layout flag dance. Single-threaded layout is the contract; atomics live elsewhere.

---

## D3 — `mark_needs_layout` propagation is greenfield, not wiring

**Fork.** Brainstorm R3 framed as "ensure propagation stops at boundary" assumes propagation EXISTS. Reality: `crates/flui-rendering/src/storage/state/propagation.rs` is empty stub deleted in Cycle 4 R-5. `mark_needs_layout` on `AtomicRenderFlags` at `flags.rs:631` is a bare flag set. Two production callers (`flui-view/src/element/behavior_commons.rs:244`, `flui-hot-reload/src/pipeline.rs:156`) call `add_node_needing_layout` direct, no walk.

**Decision.** Author boundary-aware propagation on `PipelineOwner`, NOT on `RenderEntry`. Method signature:

```rust
// pipeline/owner.rs — phase-agnostic accessor (works in Idle or Layout phase)
impl<Phase> PipelineOwner<Phase> {
    /// Mark a render object as needing layout, propagating to the nearest
    /// relayout boundary (Flutter `markNeedsLayout` shape).
    /// Mirrors `.flutter/.../object.dart:2658-2700`.
    pub fn mark_needs_layout(&mut self, id: RenderId) {
        let mut current = id;
        loop {
            let Some(entry) = self.render_tree.get_mut(current) else { return };
            if entry.state().needs_layout() { return; } // idempotent
            entry.state().mark_needs_layout();
            if entry.state().is_relayout_boundary() || entry.links().parent().is_none() {
                let depth = self.render_tree.depth(current).unwrap_or(0);
                self.dirty.needs_layout.push(DirtyNode::new(current, depth));
                return;
            }
            current = entry.links().parent().unwrap();
        }
    }
}
```

**Migration (PR-A1 prep commits).**
- `crates/flui-view/src/element/behavior_commons.rs:230-253` — replace direct `add_node_needing_layout` call with `owner.mark_needs_layout(render_id)`. Net code reduction.
- `crates/flui-hot-reload/src/pipeline.rs:156` — keep `add_node_needing_paint` direct (force-paint contract per hot-reload doc); switch any layout-side mark to `mark_needs_layout`.

**Bootstrap dependency.** Every node's `IS_RELAYOUT_BOUNDARY` flag must be SET before its first `mark_needs_layout`. `compute_relayout_boundary` at `crates/flui-rendering/src/storage/state/geometry.rs:179-197` currently has TEST-ONLY callers. Decision: call `compute_relayout_boundary(parent_uses_size = false, sized_by_parent = false, has_parent = parent.is_some())` inside `RenderEntry::layout` after `set_constraints_replace`. **`parent_uses_size` parameter stays `false` for now** — full Flutter dynamic recomputation deferred to Core.2 per existing scope boundary.

**Out of scope.** `parent_uses_size` dynamic re-classification per layout pass (Flutter parity drift, acknowledged in brainstorm as Core.2 work; we document the divergence here).

---

## D4 — `RenderNode::layout` protocol-erasure dispatch

**Fork.** Pipeline's `render_tree.get_mut(id)` returns `&mut RenderNode` (protocol-erased enum). `RenderEntry::layout(constraints)` is generic over protocol `P`. Pipeline doesn't know which protocol the node uses at the call site.

**Decision.** Add `RenderNode::layout_erased(&mut self, constraints: ErasedConstraints) -> RenderResult<ErasedGeometry>` that dispatches via enum match, calling the protocol-typed `RenderEntry::layout` internally. `ErasedConstraints` = enum over `BoxConstraints` and `SliverConstraints`; `ErasedGeometry` = enum over `Size` and `SliverGeometry`. Mismatch (e.g., handing Box constraints to a Sliver node) returns `RenderError::ProtocolMismatch`.

**Rationale.** Existing `RenderNode` already pattern-matches per-protocol (e.g., `is_relayout_boundary` at `storage/node.rs:304-307`). Adding a `layout_erased` method is symmetric and idiomatic.

**API shape sketch.**

```rust
// storage/node.rs
pub enum ErasedConstraints {
    Box(BoxConstraints),
    Sliver(SliverConstraints),
}
pub enum ErasedGeometry {
    Box(Size),
    Sliver(SliverGeometry),
}

impl RenderNode {
    pub fn layout_erased(&mut self, constraints: ErasedConstraints) -> RenderResult<ErasedGeometry> {
        match (self, constraints) {
            (Self::Box(entry), ErasedConstraints::Box(c)) => entry.layout(c).map(ErasedGeometry::Box),
            (Self::Sliver(entry), ErasedConstraints::Sliver(c)) => entry.layout(c).map(ErasedGeometry::Sliver),
            _ => Err(RenderError::ProtocolMismatch),
        }
    }
}
```

Pipeline-level walks operate on `RenderNode::layout_erased`; typed dispatch happens inside.

---

## D5 — `RenderObject<BoxProtocol> for T: RenderBox` blanket-impl bridge

**Fork.** Blanket impl at `crates/flui-rendering/src/traits/render_box.rs:380-395` has `perform_layout_raw` = **no-op returning `*self.size()`** (Size::ZERO for fresh objects). Real layout flows through `RenderBox::perform_layout(BoxLayoutContext)` via the typed surface. R1 wires the wrong function — AE1 demonstrably returns Size::ZERO.

**Decision.** Rewrite the blanket impl's `perform_layout_raw` to construct the typed context using D1's disjoint-borrow story and invoke `RenderBox::perform_layout`. Bridge function:

```rust
// traits/render_box.rs — blanket impl
impl<T: RenderBox> RenderObject<BoxProtocol> for T {
    fn perform_layout_raw(&mut self, constraints: BoxConstraints) -> Size {
        // The pipeline (D1) constructs BoxLayoutCtx with disjoint child borrows
        // and passes it through via a TLS or threaded-context mechanism.
        // BoxLayoutContext construction happens INSIDE the pipeline's
        // layout_dirty_root, NOT inside this trait method — so this method
        // signature must change OR the bridge moves up one level.
        // ...
    }
}
```

**The bridge requires `perform_layout_raw` signature to change** — it cannot construct `BoxLayoutCtx` without the children slice. Two viable shapes:

- **Shape A**: `perform_layout_raw(&mut self, ctx: &mut dyn LayoutCtxErased) -> ErasedGeometry`. Blanket impl downcasts `ctx` to `BoxLayoutContext`, calls `T::perform_layout(ctx)`. Trait surface change ripples to every `RenderObject<P>` impl.
- **Shape B**: keep `perform_layout_raw` as-is for non-Box protocols; add separate `perform_layout_box(&mut self, ctx: &mut BoxLayoutContext) -> Size` for the box bridge. `RenderEntry<BoxProtocol>::layout` calls the latter; other protocols call the former. Less rippling but bifurcates the trait.

**Decision.** Shape A (single dispatch surface, ripples bounded since `RenderObject<P>` impls are mostly inside the blanket). `LayoutCtxErased` is a trait-object-friendly enum-shaped wrapper that the blanket impl downcasts.

**Out of scope.** Sliver bridge — analogous shape, lands as part of Core.2 sliver work (no slivers in D-block test surface).

---

## D6 — Layout cycle detection

**Fork.** `LAYOUT_DEPTH_LIMIT = 1024` at `crates/flui-rendering/src/pipeline/owner.rs:811` lives in `layout_node_with_children` recursion. New walk shape (D1) has no `layout_node_with_children` recursion — depth counter never increments. Stack overflow on cycle violates Constitution Principle 6 (typed errors only).

**Decision.** Per-node cycle detection on `PipelineOwner<Layout>` via `currently_laying_out: rustc_hash::FxHashSet<RenderId>`. Populated on entry to `layout_dirty_root` (D1), removed on exit (RAII guard for unwind safety). Cycle re-entry → `RenderError::LayoutCycle(RenderId)`.

```rust
// error.rs — new variant
pub enum RenderError {
    // ... existing ...
    LayoutCycle(RenderId),
}

// pipeline/owner.rs — drop-guard
struct CycleGuard<'a> { set: &'a mut FxHashSet<RenderId>, id: RenderId }
impl Drop for CycleGuard<'_> { fn drop(&mut self) { self.set.remove(&self.id); } }
```

**Rationale.** Flutter uses `_debugDoingThisLayout` per-object boolean. FxHashSet on pipeline matches that shape adapted to typed errors. Drop guard ensures unwind safety.

**Replace `LAYOUT_DEPTH_LIMIT`** with this mechanism. Old depth-limit code at `owner.rs:807-810` deleted.

---

## D7 — Dirty-queue dedup

**Fork.** `add_node_needing_layout(id, depth)` at `crates/flui-rendering/src/pipeline/owner.rs:605` is raw push, no dedup. Mid-layout self-mark → infinite loop in the `while !empty` loop.

**Decision.** Two-layer dedup:

1. **Flag-gated no-op at `mark_needs_layout` site (D3)**: if `entry.state().needs_layout()` already true, return early. Flutter's `markNeedsLayout` shape.
2. **Mid-layout deferral via `currently_laying_out` set (D6)**: if `add_node_needing_layout` is called for an ID currently in the layout set, the push is deferred — entry goes into a `mid_layout_marks: Vec<DirtyNode>` side queue that's drained into `dirty.needs_layout` at the next `run_layout` outer iteration.

```rust
impl<Phase> PipelineOwner<Phase> {
    pub fn add_node_needing_layout(&mut self, id: RenderId, depth: usize) {
        if self.currently_laying_out.contains(&id) {
            self.mid_layout_marks.push(DirtyNode::new(id, depth));
            return;
        }
        if let Some(entry) = self.render_tree.get(id) {
            if entry.state().needs_layout() { return; } // already marked
        }
        self.dirty.needs_layout.push(DirtyNode::new(id, depth));
    }
}
```

Symmetric for `add_node_needing_paint` / `add_node_needing_compositing_bits_update` / `add_node_needing_semantics`.

**Rationale.** Without dedup, adversarial Task 4's "perform_layout marks self dirty" → infinite recursion. With dedup at both layers, the Flutter pattern is preserved AND mid-frame re-marks defer correctly.

---

## D8 — Paint flag clear retention

**Fork.** Brainstorm R8 "clear `needs_paint` only on nodes it actually paints" loses retention story for un-painted dirty entries. Three options: retain / purge-warn / migrate-to-orphan.

**Decision.** Purge-warn (preserve current R-15 invariant from PR #109). Specifically:

- During paint walk, maintain `painted: FxHashSet<RenderId>` of nodes actually visited.
- Post-walk, iterate `dirty.needs_paint`; for each entry NOT in `painted`, log `tracing::warn!(target = "flui-rendering", render_id = ?id, "paint dropped — node not reachable from root")` AND clear the flag (current R-15 behavior) AND drop the dirty entry.
- For entries IN `painted`, clear the flag (paint completed) AND drop the dirty entry.

**Rationale.** Adversarial Task 8 showed naive "leave unpainted flags set" leaks orphan-subtree state forever. Retain-with-warn is the only safe choice for D-block scope. Per-frame orphan tracking (option C) is Cross.H work, not D-block.

R8 wording in revised brainstorm: "`run_paint` walks the paint set from `root_id`; the clear pass distinguishes painted vs unreached. Painted nodes clear their flag normally; unreached nodes log a warn and clear (preserves R-15)."

---

## D9 — PR file fences (sequencing safety)

**Fork.** Adversarial Task 9 + scope Task 4: PR-B and PR-A1/PR-A2 sequencing assumes independence but file overlap not enumerated.

**Decision.** Strict per-PR file fences:

| PR | Allowed touchpoints | Forbidden |
|---|---|---|
| **PR-A1** (D-1) | `crates/flui-rendering/src/pipeline/owner.rs`, `crates/flui-rendering/src/storage/entry.rs`, `crates/flui-rendering/src/storage/state/{constraints,geometry,flags}.rs`, `crates/flui-rendering/src/storage/node.rs`, `crates/flui-rendering/src/protocol/box_protocol.rs`, `crates/flui-rendering/src/traits/render_box.rs`, `crates/flui-rendering/src/error.rs`, `crates/flui-view/src/element/behavior_commons.rs`, `crates/flui-hot-reload/src/pipeline.rs`, `crates/flui-rendering/tests/pipeline/` (new dir), `crates/flui-rendering/tests/common/mod.rs` (new) | `crates/flui-layer/*`, `crates/flui-semantics/*`, `crates/flui-types/*`, `crates/flui-log/*`, `crates/flui-geometry/*`, `scripts/port-check.sh`, `docs/PORT.md`, `.specify/memory/constitution.md` |
| **PR-A2** (D-3+D-4) | `crates/flui-rendering/src/pipeline/owner.rs`, `crates/flui-rendering/src/storage/flags.rs` (new flag bits for `NEEDS_COMPOSITING_BITS_UPDATE`), `crates/flui-rendering/src/storage/state/flags.rs`, `crates/flui-rendering/tests/pipeline/` | Same as PR-A1 plus anything outside `flui-rendering` |
| **PR-B-gate** (Wave 3+4) | `crates/flui-layer/src/tree/layer_tree.rs`, `crates/flui-layer/src/layer/mod.rs` (lifecycle), `crates/flui-semantics/src/tree/semantics_tree.rs` (cascade-remove symmetry) | `crates/flui-rendering/*`, anything in `flui-types`/`flui-log`/`flui-geometry` |
| **PR-B-followup** (Waves 1+2+5+6+7) | Per the layer/semantics repair plan as written | `crates/flui-rendering/*` (D-block fences); other constraints per plan |
| **PR-C-1** (`flui-log` merge) | `crates/flui-log/*`, `crates/flui-foundation/src/log/` (new), `crates/flui-app/{Cargo.toml,src/lib.rs,src/app/{direct,runner}.rs}`, `crates/flui-cli/{Cargo.toml,src/main.rs}`, `crates/flui-view/{Cargo.toml,src/lib.rs}`, root `Cargo.toml` | Anything in `flui-rendering`/`flui-layer`/`flui-semantics` |
| **PR-C-2** (`flui-geometry` split + Constitution amendment) | `crates/flui-geometry/` (new crate), `crates/flui-types/{Cargo.toml,src/lib.rs,src/geometry/}`, all geometry consumers (mechanical re-export migration), `.specify/memory/constitution.md`, `CLAUDE.md`, root `Cargo.toml` | Anything in pipeline/storage |
| **PR-C-3** (refusal triggers #8/#10/#11/#12/#13) | `scripts/port-check.sh`, `docs/PORT.md` | Anything else |

**Rationale.** Adversarial Task 9 showed `flui-view` `behavior_commons.rs` migration AND `flui-types` re-exports could collide. The fences enforce non-overlap. PR-C splits resolve Cargo.toml conflict between flui-log merge and flui-geometry split (scope Task 4).

**Merge ordering**: PR-C-1, PR-C-2, PR-B-gate, PR-B-followup, PR-A1, PR-A2, PR-C-3 (trigger #8 install last, after D-block closes SP-1 violations per D10).

---

## D10 — Trigger #8 install timing

**Fork.** Brainstorm offered concurrent-with-allowlist vs post-D-block. Confirmed by ce-architecture review: PR #134 (FR-036 / trigger #9) followed close-first-install-after — install ships with zero markers.

**Decision.** Trigger #8 (SP-1 stubbed-but-called) installs in **PR-C-3, the LAST PR in the sequence**, after PR-A1 + PR-A2 close the D-1/D-3/D-4 SP-1 stub violations. PORT.md prose entry for trigger #8 can land in PR-C-2 as informational; the `port-check.sh` gate flips ON only in PR-C-3.

**`// STUB-OK: <reason> (issue #N)` markers** — allowed for remaining production stubs (e.g., `run_semantics` body if still no-op post-D-block). Each marker requires a tracking issue. Discipline lives in `docs/PORT.md` trigger #8 prose.

---

## D11 — PR-B scope split

**Fork.** Scope-guardian Task 1: 24-commit layer/semantics plan over-scope for "gating D-3"; only Wave 3 + Wave 4 (U8-U13) gate D-3.

**Decision.** Split as in D9 file fences:

- **PR-B-gate**: Waves 3 + 4 only (U8-U13). ~8-10 atomic commits. Gates PR-A2.
- **PR-B-followup**: Waves 1 + 2 + 5 + 6 + 7 (U1-U7 + U14-U24). ~14 atomic commits. Independent of D-block, can land any time.

Layer/semantics repair plan document gets updated with a "Phase split for D-block" section pointing to this memo.

---

## D12 — D-block test infrastructure

**Fork.** Feasibility Task 8: no 3-level tree fixture exists; `crates/flui-rendering/tests/` has only one `.disabled` file.

**Decision.** New directory `crates/flui-rendering/tests/pipeline/` with:

- `common/mod.rs` — fixture builders:
  - `make_three_level_box(padding_inset: EdgeInsets, colored_size: Size) -> (PipelineOwner<Idle>, RenderId)`
  - `assert_geometry(tree: &RenderTree, id: RenderId, expected: Size)`
  - `assert_offset(tree: &RenderTree, id: RenderId, expected: Offset)`
  - `make_two_repaint_boundary_subtrees() -> ...` (for AE2 / AE5)
  - `mid_layout_compositing_dirty_scenario() -> ...` (for AE6 — see brainstorm rev2)
- `layout_pipeline_test.rs` — AE1 + AE2 + AE3 (covers D-1)
- `compositing_pipeline_test.rs` — AE4 + AE6 (covers D-3)
- `paint_pipeline_test.rs` — AE5 + AE7 (covers D-4)

**Rationale.** Per Cycle 5 convention for `crates/flui-view/tests/`. Builders are seed for `crates/flui-widgets/tests/parity/` once `flui-widgets` exists (Core.1). Delete the `.disabled` orphan file in PR-A1.

---

## Summary table — brainstorm rev2 actions

| Decision | Brainstorm rev2 impact |
|---|---|
| D1 | New R: borrow architecture via `get_parent_and_children_mut`; deferred-question §R2 resolved. |
| D2 | New R-relayout-safety (P0 prep commit before C1): drop OnceCell panic on `set_constraints`/`set_geometry`. R2 rewritten as cache-hit short-circuit. |
| D3 | R3 rewritten as "author boundary-aware `mark_needs_layout`"; new R-migration for flui-view + flui-hot-reload caller updates; new R-bootstrap for `compute_relayout_boundary` call inside `RenderEntry::layout`. C0 prep commit becomes migration commit, not doc-only. |
| D4 | New R: `RenderNode::layout_erased` + `ErasedConstraints` / `ErasedGeometry` enums. |
| D5 | New R: rewrite `RenderObject<P>::perform_layout_raw` signature to accept `&mut dyn LayoutCtxErased`; blanket impl bridges to `RenderBox::perform_layout`. |
| D6 | New R-cycle: `currently_laying_out: FxHashSet<RenderId>` + `RenderError::LayoutCycle(id)` variant; delete old `LAYOUT_DEPTH_LIMIT`. |
| D7 | New R-dedup: flag-gated no-op + `mid_layout_marks` side queue. Applies to all four dirty queues symmetrically. |
| D8 | R8 rewritten with explicit retention rule: purge-warn (R-15 invariant preserved). |
| D9 | New §"PR file fences" section in brainstorm; PR-C splits into PR-C-1/PR-C-2/PR-C-3 with explicit merge ordering. |
| D10 | R13 trigger numbering corrected (#8/#10/#11/#12/#13); trigger #8 install in PR-C-3 LAST. |
| D11 | R12 updated: PR-B splits into PR-B-gate (Waves 3+4) + PR-B-followup (rest). |
| D12 | New R-test-infra: `crates/flui-rendering/tests/pipeline/` + `tests/common/mod.rs`; delete `.disabled` orphan. |

**Brainstorm rev2 also corrects two false premises:**

1. **Strike "zero external consumers"** — `run_frame` has 3 production callers (`crates/flui-app/src/bindings/renderer_binding.rs:438`, `crates/flui-app/src/app/binding.rs:406`, `crates/flui-hot-reload/src/pipeline.rs:161`). Test scope expands to `cargo test -p flui-app -p flui-hot-reload`.
2. **Strike "symmetric defect sweep" requirement (R11)** — verified all four dirty queues already sort-by-depth correctly. R11 becomes verification, not new work.

**Drop R17 + R18** — process-meta, belongs in CLAUDE.md reference not R-IDs.

**Add new AEs:**
- AE6: mid-layout-dirty → next-frame-compositing (covers D7 deferral)
- AE7: layer disposed mid-compositing → walk skips, no panic (covers D11 PR-B-gate + D-3 interaction)
- AE8: frame-2 re-layout of same tree → no panic (covers D2)
- AE9: layout cycle A→B→A → `RenderError::LayoutCycle`, no stack overflow (covers D6)

---

## Files referenced

- `crates/flui-rendering/src/pipeline/owner.rs` (lines 605, 754, 807-810, 855-896, 922, 945, 958-960, 1049, 1095-1114, 1321, 1373)
- `crates/flui-rendering/src/storage/entry.rs:252-286`
- `crates/flui-rendering/src/storage/state/constraints.rs:38-52`
- `crates/flui-rendering/src/storage/state/geometry.rs:71, 179-197`
- `crates/flui-rendering/src/storage/state/flags.rs:124-153`
- `crates/flui-rendering/src/storage/state/propagation.rs` (empty stub — propagation deleted Cycle 4 R-5)
- `crates/flui-rendering/src/storage/tree.rs:215-301` (disjoint-borrow primitives)
- `crates/flui-rendering/src/storage/node.rs:296-307`
- `crates/flui-rendering/src/storage/flags.rs:631, 766, 778`
- `crates/flui-rendering/src/protocol/box_protocol.rs:187-188, 243, 280-306`
- `crates/flui-rendering/src/traits/render_box.rs:372-395`
- `crates/flui-rendering/src/traits/render_object.rs:199, 229`
- `crates/flui-rendering/src/error.rs:191-298`
- `crates/flui-rendering/src/context/layout.rs:95-172`
- `crates/flui-view/src/element/behavior_commons.rs:230-253`
- `crates/flui-view/src/view/root.rs:188-207`
- `crates/flui-hot-reload/src/pipeline.rs:155-161`
- `crates/flui-app/src/bindings/renderer_binding.rs:438`
- `crates/flui-app/src/app/binding.rs:406`
- `crates/flui-layer/src/tree/layer_tree.rs:180-300`
- `crates/flui-layer/src/layer/mod.rs:288-314`
- `crates/flui-rendering/tests/layout_pipeline_test.rs.disabled` (DELETE in PR-A1)
- `scripts/port-check.sh:285-473` (trigger #9 / FR-036 already installed)
- `docs/PORT.md` (Refusal triggers section, lines 18-87)
- `docs/research/2026-05-22-architecture-correction-plan.md` §D-1/D-3/D-4 + §2 SP-1..SP-8
- `docs/plans/2026-05-22-004-feat-layer-semantics-repair-plan.md` (PR-B target)
- `docs/plans/2026-05-22-005-feat-view-element-core-contracts-plan.md` (plan precedent)
- `.flutter/flutter-master/packages/flutter/lib/src/rendering/object.dart:2658-2700, 2738-2766, 2845, 3226-3258`
