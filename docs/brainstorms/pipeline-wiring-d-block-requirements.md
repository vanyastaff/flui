---
date: 2026-05-23
topic: pipeline-wiring-d-block
rev: 2
revised: 2026-05-23
---

# Pipeline Wiring D-Block — Core.0 Closure

> **Rev 2 (2026-05-23).** Multi-agent pre-plan-write review (ce-architecture-strategist, ce-feasibility-reviewer, ce-scope-guardian-reviewer, ce-adversarial-document-reviewer) surfaced 4 P0 plan-breakers and ~12 P1/P2 substantive findings. Architecture forks resolved in companion decision memo at [`docs/research/2026-05-23-d-block-architecture-decision-memo.md`](../research/2026-05-23-d-block-architecture-decision-memo.md) (D1–D12 binding decisions). This rev integrates findings: corrected false "zero external consumers" premise (3 production callers exist); rewrote R3 from "wire boundary discipline" to "author greenfield propagation logic"; added R19–R27 for memo decisions; split PR-B and PR-C per file-fence analysis; corrected trigger numbering (#9 already taken by FR-036 from PR #134).

## Summary

Close Core.0's D-block by wiring the layout, compositing, and paint phase orchestration to Flutter-shape parity. Ships as **seven PRs**. Six on the serial critical path: PR-C-1 (`flui-log` merge) → PR-C-2 (`flui-geometry` split + Constitution amendment) → PR-B-gate (layer/semantics Wave 3+4 lifecycle + dirty-bit propagation) → PR-A1 (D-1 layout pipeline) → PR-A2 (D-3+D-4 compositing+paint pipeline) → PR-C-3 (refusal triggers #8/#10/#11/#12/#13 — install LAST per close-first PR #134 precedent). PR-B-followup (layer/semantics Waves 1+2+5+6+7) ships parallel, can land any time post-PR-B-gate, does NOT gate D-block critical path.

---

## Problem Frame

Three pipeline phases in `crates/flui-rendering/src/pipeline/owner.rs` are silent no-ops today:

- **D-1** (`layout_node_with_children`, ~line 855): walks the tree depth-first checking `needs_layout()` and recursing, but never invokes `RenderEntry::layout(constraints)`. The only `entry.layout()` callsite in the file is inside a `#[test]` block at ~line 1790.
- **D-3** (`run_compositing`, ~line 922): logs `"compositing-bits update is a no-op until..."`, clears the dirty list, returns `Ok(())`. Flutter's `flushCompositingBits` walks dirty nodes and sets each layer's compositing-needs flag via `_updateSubtreeCompositingBits` — FLUI does none of it.
- **D-4** (`run_paint`, ~line 1010): the flag-clear loop iterates the dirty list clearing `needs_paint`; the paint walk descends only from `root_id`. A node-needing-paint not reachable from `root_id` in that descent gets its flag **cleared without being painted** — stale until something else dirties it.

All three are SP-1 (silent class) per [`docs/research/2026-05-22-architecture-correction-plan.md`](../research/2026-05-22-architecture-correction-plan.md) §D-1/D-3/D-4. ROADMAP calls D-1/D-3/D-4 collectively "the single most important new work in Core.0."

**Pipeline has 3 production consumers (rev 2 correction).** `PipelineOwner::run_frame` is invoked from:
- `crates/flui-app/src/bindings/renderer_binding.rs:438`
- `crates/flui-app/src/app/binding.rs:406`
- `crates/flui-hot-reload/src/pipeline.rs:161`

Rev 1 erroneously claimed "zero external consumers." Re-layout panics will hit `AppBinding::draw_frame` on frame 2. Hot-reload's force-paint contract interacts with R8 paint-clear discipline. Test scope expanded accordingly (R18).

Core.1 vertical slice cannot proceed until the pipeline actually orchestrates work. D-block is the gate.

---

## Process meta

All PRs follow CLAUDE.md §Engineering Standards & Subagent Dispatch: atomic-commit-per-unit shape mirroring PR #122/#130/#132/#133/#134 ritual (5–15 conventional-commit-formatted commits per PR, each with `Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>` trailer). Each PR's exit gates: `cargo build --workspace` + `cargo clippy --workspace --all-targets -- -D warnings` + `cargo test -p <touched-crate> --lib` + `bash scripts/port-check.sh -v` all exit 0. **Test scope (rev 2)** expanded to include `cargo test -p flui-app` and `cargo test -p flui-hot-reload` because `run_frame` paths transitively exercise the new walk. (Rev 1 R17/R18 dropped from R-ID list — process discipline lives in CLAUDE.md, not feature requirements.)

---

## Requirements

**PR-A1 — D-1 layout phase wiring**

- R1. `layout_node_with_children` invokes `RenderEntry::layout(constraints)` per-node; per-node `perform_layout` drives child recursion via the typed `BoxLayoutContext`'s child slice (see R19 for borrow architecture).
- R2. `RenderEntry::layout` short-circuits cache-hit path: if `state.constraints() == Some(&new_constraints) && !state.needs_layout()`, return cached geometry without invoking `perform_layout_raw`. Mirrors Flutter `if (!_needsLayout && constraints == _constraints) return;` at `.flutter/.../object.dart:2845`.
- R3. **Author boundary-aware `mark_needs_layout` propagation** as a new `PipelineOwner::mark_needs_layout(id)` method. Walks parent chain via `RenderEntry::links().parent()`, marks each ancestor `needs_layout`, stops at first ancestor with `IS_RELAYOUT_BOUNDARY` flag set OR at root, calls `add_node_needing_layout` for the boundary. Greenfield — propagation logic does not exist today (`crates/flui-rendering/src/storage/state/propagation.rs` is empty stub deleted in Cycle 4 R-5).
- R4. Layout integration-test gate: a `Padding → Center → ColoredBox` 3-level tree lays out with correct constraints and computed sizes (matches ROADMAP Core.0 exit verbatim).
- R5. Layout integration-test gate: sibling subtrees marked independently re-layout independently — dirtying sibling A does not re-layout sibling B's subtree.
- R6. Layout integration-test gate: a relayout-boundary-isolated re-layout uses cached constraints without parent re-propagation.

**PR-A1 — D-1 prep + bridge (rev 2 additions per memo)**

- R19. **Borrow architecture (memo D1)**: pipeline uses `RenderTree::get_parent_and_children_mut(parent_id)` disjoint-borrow primitive at `crates/flui-rendering/src/storage/tree.rs:215-301`. `BoxLayoutCtx` carries `&mut [&mut RenderEntry<BoxProtocol>]` for children. `LayoutChildCallback` type at `box_protocol.rs:187` removed or repurposed — replaced by direct child-slice access.
- R20. **Re-entry safety prep commit (memo D2, P0 before R1)**: `RenderState::set_constraints` and `RenderState::set_geometry` drop `OnceCell` panic-on-already-set; replaced with `Option<T>` + `set_constraints_replace` / `set_geometry_replace` (`&mut self`, straight assignment). Mirrors Flutter `_constraints = constraints` at `.flutter/.../object.dart:2865`. **Without this commit `RenderEntry::layout` panics on frame 2** — gates entire D-1 work.
- R21. **`mark_needs_layout` caller migration (memo D3)**: `crates/flui-view/src/element/behavior_commons.rs:230-253` migrates from direct `owner.add_node_needing_layout(render_id, depth)` to `owner.mark_needs_layout(render_id)`. `crates/flui-hot-reload/src/pipeline.rs:156` keeps `add_node_needing_paint` direct (force-paint contract), updates any layout-side mark.
- R22. **Protocol-erasure dispatch (memo D4)**: `RenderNode::layout_erased(constraints: ErasedConstraints) -> RenderResult<ErasedGeometry>` dispatches via enum match. `ErasedConstraints` = `enum { Box(BoxConstraints), Sliver(SliverConstraints) }`; mismatch returns `RenderError::ProtocolMismatch`.
- R23. **`RenderObject` → `RenderBox` bridge (memo D5)**: blanket impl at `traits/render_box.rs:380-395` rewritten. `perform_layout_raw` signature changes to accept `&mut dyn LayoutCtxErased`; blanket impl downcasts to `BoxLayoutContext`, invokes `T::perform_layout(ctx)`. Replaces current no-op stub that returns `*self.size()`.
- R24. **Layout cycle detection (memo D6)**: `currently_laying_out: FxHashSet<RenderId>` on `PipelineOwner<Layout>`; RAII drop-guard for unwind safety; cycle re-entry returns new `RenderError::LayoutCycle(RenderId)` variant. Replaces existing `LAYOUT_DEPTH_LIMIT` recursion guard at `owner.rs:807-810` (wrong place after walk rewrite).
- R25. **Dirty-queue dedup (memo D7)**: `add_node_needing_layout` / `_paint` / `_compositing` / `_semantics` add two-layer dedup: (a) check `state.needs_*()` flag, no-op if already set (Flutter `markNeedsLayout` shape); (b) mid-layout marks defer to a `mid_layout_marks: Vec<DirtyNode>` side queue drained at next outer iteration of `run_layout`.
- R26. **Bootstrap `compute_relayout_boundary`**: call inside `RenderEntry::layout` after `set_constraints_replace` with `parent_uses_size=false, sized_by_parent=false, has_parent=parent.is_some()`. Currently TEST-ONLY callers (`crates/flui-rendering/src/storage/state/geometry.rs:179-197`). Bootstrap required for R3 propagation to terminate at boundaries (otherwise every node has `is_relayout_boundary == false` and propagation runs to root).

**PR-A2 — D-3+D-4 compositing + paint phase wiring**

- R7. `run_compositing` walks dirty nodes (sorted by depth shallow-first, already correct at `owner.rs:945`) and propagates via Flutter's `_updateSubtreeCompositingBits` algorithm against the already-existing `RenderObject::is_repaint_boundary()` and `RenderObject::always_needs_compositing()` accessors at `traits/render_object.rs:199, 229`. **No trait-surface expansion required** (rev 2 correction — stale TODO comment at `owner.rs:958-960` predates these accessors). Two new flag bits added to `RenderFlags`: `NEEDS_COMPOSITING`, `NEEDS_COMPOSITING_BITS_UPDATE`.
- R8. **Paint flag-clear with explicit retention rule (memo D8)**: `run_paint` walks paint set from `root_id`, maintains `painted: FxHashSet<RenderId>` of nodes actually visited. Post-walk, iterate `dirty.needs_paint`: painted nodes clear their flag normally; unreached nodes log `tracing::warn!` AND clear the flag (preserves R-15 invariant from PR #109). Dirty entries drop for both classes — set of painted nodes drives clear-pass.
- R9. Compositing integration-test gate: a layer subtree marked dirty triggers compositing-bits propagation (matches ROADMAP Core.0 exit verbatim).
- R10. Paint integration-test gate: a `RepaintBoundary`-isolated repaint clears `needs_paint` only on nodes inside the boundary subtree (matches ROADMAP Core.0 exit verbatim).
- R11. **Verification check (rev 2 restatement)**: confirm `dirty.needs_compositing` and `dirty.needs_paint` sort orderings remain symmetric with `dirty.needs_layout` after rewrite. Research verified all four queues already sort-by-depth correctly (`needs_paint` deep-first per Flutter parity at `owner.rs:1049`, others shallow-first at `owner.rs:763/945/1373`). No net-new sort code; verification gate ensures no regression.
- R26b. **Repaint-boundary flag bootstrap**: every node's `IS_REPAINT_BOUNDARY` flag is set at insert from the `RenderObject::is_repaint_boundary()` hardcoded answer. Without this, R10 cannot pass — no production code currently sets the flag.

**PR-B-gate — layer/semantics lifecycle + dirty-bit propagation (memo D11)**

- R12a. Execute **Wave 3 + Wave 4 only** of [`plans/2026-05-22-004-feat-layer-semantics-repair-plan.md`](../plans/2026-05-22-004-feat-layer-semantics-repair-plan.md) (U8–U13, ~8–10 atomic commits). Wave 3 lands `disposed: AtomicBool` + Drop guards mirroring PR #84's `ChangeNotifier::dispose`; `needs_add_to_scene` dirty-bit propagation matching Flutter `updateSubtreeNeedsAddToScene`. Wave 4 lands slab-tree hygiene pair (auto-detach on `add_child` + cascade-by-default `remove`) mirrored across `flui-layer` and `flui-semantics`. **Gates PR-A2 only** — D-3 reads layer state via `_updateSubtreeCompositingBits`; clean lifecycle prevents stale-layer panics mid-batch.

**PR-B-followup — layer/semantics rest (memo D11)**

- R12b. Execute Wave 1 + Wave 2 + Wave 5 + Wave 6 + Wave 7 of the layer/semantics repair plan (U1–U7 + U14–U24, ~14 atomic commits). Cross.H foundation hardening — zero-consumer deletions, Layer enum boxing, semantics platform routing, type-system cleanups, sync/alloc reduction. **Does not gate D-block** — can land any time, parallel.

**PR-C-1 — `flui-log` merge (memo D9)**

- R14. `flui-log` merges into `flui-foundation::log` module; 7 production callsites across 5 files in 3 consuming crates updated mechanically (`flui_log::` → `flui_foundation::log::`). `flui-log` removed from `[workspace.members]`. Touchpoints: `crates/flui-log/*`, `crates/flui-foundation/src/log/` (new), `crates/flui-app/{Cargo.toml,src/lib.rs,src/app/{direct,runner}.rs}`, `crates/flui-cli/{Cargo.toml,src/main.rs}`, `crates/flui-view/{Cargo.toml,src/lib.rs}`, root `Cargo.toml`.

**PR-C-2 — `flui-geometry` split + Constitution amendment (memo D9)**

- R15. `flui-geometry` splits out of `flui-types` into its own crate at `crates/flui-geometry/`. 25 files from `crates/flui-types/src/geometry/` migrate; `flui-types` adds dep on `flui-geometry` and re-exports the geometry surface for backward compat during transition.
- R16. Constitution version bumped (2.3.0 → 2.4.0); layer table in `.specify/memory/constitution.md` amended to match [`docs/FOUNDATIONS.md`](../FOUNDATIONS.md) Part IV (`flui-log` removed, `flui-geometry` added, Edition 2024 / `rust-version = 1.94` line corrected). `CLAUDE.md` version reference updated.

**PR-C-3 — refusal triggers install (memo D10, LAST PR in sequence)**

- R13. Refusal triggers **#8 / #10 / #11 / #12 / #13** (five new — #9 already taken by FR-036 sanctioned-dyn-boundary registry from PR #134 per `scripts/port-check.sh:308`) written into [`docs/PORT.md`](../PORT.md); mechanically-detectable ones (#8, #10, #12, #13) become `scripts/port-check.sh` gates extending current set to 13 total triggers. Trigger #8 (SP-1 stubbed-but-called) ships with zero violations — D-block PRs (A1+A2) close existing SP-1 stubs before this PR opens (close-first precedent from PR #134 / FR-036). `// STUB-OK: <reason> (issue #N)` marker discipline documented in PORT.md trigger #8 prose for any remaining production stubs.

**Cross-cutting**

- R26c. **PR file fences (memo D9)**: strict per-PR touchpoint enumeration prevents merge conflict. Full table in companion memo §D9. Key fences: PR-A1 touches `flui-rendering` + `flui-view/element/behavior_commons.rs` + `flui-hot-reload/pipeline.rs` only; PR-A2 touches `flui-rendering` only; PR-B-gate + PR-B-followup touch `flui-layer` + `flui-semantics` + (Wave 2) `flui-types/Alignment` only; PR-C-1/2/3 enumerated independently.
- R27. **Test infrastructure (memo D12)**: new directory `crates/flui-rendering/tests/pipeline/` with `common/mod.rs` fixture builders (`make_three_level_box`, `assert_geometry`, `assert_offset`, `make_two_repaint_boundary_subtrees`, `mid_layout_compositing_dirty_scenario`) + per-phase test files (`layout_pipeline_test.rs`, `compositing_pipeline_test.rs`, `paint_pipeline_test.rs`). Mirror Cycle 5 convention for `crates/flui-view/tests/`. Delete `crates/flui-rendering/tests/layout_pipeline_test.rs.disabled` orphan in PR-A1.

---

## Sequencing

```
Merge order (strict serial — file fences per R26c):

  PR-C-1 ──► PR-C-2 ──► PR-B-gate ──► PR-A1 ──► PR-A2 ──► PR-C-3
   log     geometry    Wave 3+4     D-1 wir.   D-3+D-4   trig #8
   merge    split      lifecycle    + memo     + tests   install
                       + tests      D1-D7+D12             (close-
                                                          first)

  PR-B-followup ───► parallel; lands any time post-PR-B-gate
  (Wave 1+2+5+6+7)   independent of D-block critical path
```

PR-C-1 lands first because its touchpoints (Cargo.toml workspace edits, lib.rs re-exports) overlap nothing else and clear the deck. PR-C-2 (`flui-geometry` split) lands second to land workspace-shape changes before D-block opens. PR-B-gate gates PR-A2 only (per memo D11). PR-A1 + PR-A2 are sequential within Track A (D-1 prep enables D-3+D-4 testing on real layouts). PR-C-3 (trigger #8 install) lands LAST per close-first precedent — D-block PRs close SP-1 violations first.

---

## Acceptance Examples

- AE1. **Covers R1, R4, R20, R23.** Given a tree `Padding(EdgeInsets::all(8.0)) → Center → ColoredBox(Size::new(80, 40))` with root `BoxConstraints::tight(Size::new(200, 100))`, when `run_layout` executes, `RenderEntry::layout` is invoked on the root; per-node `perform_layout` recurses through the typed `BoxLayoutContext` child slice; final computed sizes match Flutter parity — `Padding` 200×100, `Center` content area 184×84 (after 8-inset), `ColoredBox` 80×40 positioned at center. **No panic on `RenderState::set_geometry` second call** (validates R20).
- AE2. **Covers R3, R5, R21.** Given two `RepaintBoundary`-isolated subtrees `A` and `B` under a common root, when `mark_needs_layout` is called on a node inside `A` via the new `PipelineOwner::mark_needs_layout(id)` path (R21 migration target), the dirty mark propagates up to `A`'s relayout boundary and stops there; `B`'s subtree is untouched on the next `run_layout` pass.
- AE3. **Covers R2, R6, R20.** Given a node `N` that is its own relayout boundary with cached constraints `C` from its last layout pass, when `N` is re-marked dirty and `run_layout` executes, `RenderEntry::layout` is invoked with `C` (no parent re-propagation); cache-hit short-circuit returns cached geometry without re-running `perform_layout_raw`; resulting subtree sizes are correct.
- AE4. **Covers R7, R9, R26b.** Given a tree where a descendant marks `needs_compositing`, when `run_compositing` executes, the dirty queue is sorted shallow-first; the `_updateSubtreeCompositingBits` walk reads `RenderObject::is_repaint_boundary()` + `always_needs_compositing()` (already-existing accessors); `IS_REPAINT_BOUNDARY` flag set at insert per R26b makes the walk terminate correctly at boundaries.
- AE5. **Covers R8, R10.** Given a tree where node `X` inside a `RepaintBoundary`-isolated subtree is marked `needs_paint`, when `run_paint` executes, only nodes inside that boundary subtree are visited; `painted: FxHashSet` tracks them; clear-pass clears `needs_paint` on painted nodes; unreached dirty entries log `tracing::warn!` and clear (R-15 invariant preserved).
- AE6. **Covers R25.** Given a node `N` whose `perform_layout` calls `mark_needs_layout(M)` on a sibling during layout (mid-layout dirty mark), `M`'s entry goes to `mid_layout_marks` side queue; current `run_layout` iteration completes without re-entering `M`; next outer iteration drains `mid_layout_marks` into `dirty.needs_layout` and processes `M`.
- AE7. **Covers R12a + R7 interaction.** Given a `LayerNode` disposed via `LayerTree::remove` while `_updateSubtreeCompositingBits` walk is mid-traversal, the walk skips the disposed slot via `disposed: AtomicBool` guard (from PR-B-gate Wave 3) without panic; `tracing::warn!` logged with `render_id` + `layer_id`.
- AE8. **Covers R20.** Given the AE1 tree, when `run_layout` is invoked a second consecutive frame (e.g., from `AppBinding::draw_frame` two frames in a row), no `RenderState::set_constraints` or `set_geometry` panic fires; second frame's layout completes successfully (validates `OnceCell` → `Option` migration).
- AE9. **Covers R24.** Given an adversarial tree `A → B` where `A::perform_layout` calls `layout_child(B)` and `B::perform_layout` calls `layout_child(A)` (synthetic cycle), `run_layout` returns `Err(RenderError::LayoutCycle(A))` without stack overflow; `currently_laying_out` set is empty post-error (RAII drop-guard works).

---

## Success Criteria

- **Human outcome**: the render pipeline orchestrates work end-to-end. A `Padding → Center → ColoredBox` tree (and Flex/Variable, Leaf, Opacity equivalents already implemented under `crates/flui-rendering/src/objects/`) lay out and paint correctly without test-only callsites. `AppBinding::draw_frame` runs frame-after-frame without panic. Core.1 vertical-slice work can begin building widgets that actually render.
- **Downstream-agent handoff**: every R has at least one integration test exercising it; the four ROADMAP-Core.0-named gate tests (R4 / R5 / R9 / R10) pass; `port-check.sh -v` reports 13 triggers green after R13; `cargo clippy --workspace --all-targets -- -D warnings` exits 0 on each PR; no `unimplemented!()` / `todo!()` introduced in non-test code (grep gate); `cargo test -p flui-app -p flui-hot-reload` passes (run_frame consumers).
- **Parity-scoreboard delta**: `rendering` machine row moves from "machine ~90%" to "machine spec-complete" per ROADMAP scoreboard.

---

## Scope Boundaries

- Intrinsic dimension protocol (`getMinIntrinsicWidth` / `getMaxIntrinsicWidth` / etc.) — deferred to Core.2 render-object catalog
- `parent_uses_size` optimization + dynamic `is_relayout_boundary` recomputation (Flutter `.flutter/.../object.dart:2845` recomputes per layout based on `parent_uses_size` param) — deferred to Core.2; rev 2 acknowledges this is a Flutter-parity divergence
- Mythos Cycle 4 closure (rendering × engine remaining findings) — separate parallel workstream
- Widget → render-object mapping checklist (`docs/research/widget-renderobject-map.md`) — separate Core.0 deliverable; Core.2 entry gate
- Physics audit of `crates/flui-types/src/physics/` against Flutter — separate Core.0 deliverable
- `RasterBackend` seam in `flui-engine` + `Scene` / `DrawCommand` contract freeze — separate parallel Core.0 deliverables
- Speckit ceremony (`specs/006-pipeline-wiring/`) — companion decision memo + this requirements doc + architecture-correction-plan serve as research substrate; atomic-commit-per-unit shape preserves traceability without spec-file overhead
- Flutter widget-test corpus port — Core.1 deliverable (parity oracle infrastructure)
- Re-enable `flui-animation`, create `flui-widgets` skeleton — Core.1 deliverables
- Sliver protocol `RenderObject` → `RenderSliver` bridge (memo D5 Box analog) — Core.2 sliver work (no slivers in D-block test surface)
- Async layout / cross-thread layout — out of scope indefinitely

### Deferred to Follow-Up Work

- Dead `ProtocolRenderObject::mark_needs_layout` trait method at `crates/flui-rendering/src/protocol/protocol.rs:133` with zero production impls — SP-2 candidate, defer to next Cross.H cleanup
- `TextPainter::mark_needs_layout` self-defined method (10 sites) at `crates/flui-painting/src/text_painter/mod.rs:277-349` — unrelated to render pipeline; possible renaming for clarity in TextPainter cycle

---

## Key Decisions

- **Flutter-shape parity over narrow gate-test-only inside D-1.** Boundary discipline (R3) + cached constraints (R2) + cycle detection (R24) land inside PR-A1, not deferred. Companion memo D1–D7 provide binding architecture; "NO quick wins" discipline applies to memo-resolved scope.
- **D-1 split into 10–12 atomic commits inside one PR-A1**, not single mega-diff. Per-commit breakdown: R20 prep (OnceCell→Option) → R21 caller migration → R3 propagation authoring + R26 bootstrap → R19 borrow-arch wiring → R22 protocol-erasure dispatch → R23 RenderBox bridge → R24 cycle detection → R25 dedup → R1 walk rewrite → R2 cache-hit → R4+R5+R6+AE8+AE9 tests → R27 test fixture infra.
- **Borrow architecture via `get_parent_and_children_mut` disjoint-borrow primitive** (memo D1) — not interior mutability (forbidden by PORT.md triggers #1+#2), not callback closures (cannot capture `&mut PipelineOwner` as `Fn`), not worklist refactor (breaks parent-uses-child-size contract).
- **`RenderState::set_constraints` + `set_geometry` `OnceCell` → `Option` migration is P0 prep** (memo D2) — without this, R1 wiring panics on frame 2 of any production binding (3 callers exist per Problem Frame correction). Mandatory C0 commit before walk rewrite.
- **`mark_needs_layout` propagation is greenfield authoring** (memo D3) — `crates/flui-rendering/src/storage/state/propagation.rs` is empty stub deleted in Cycle 4 R-5; current production callers use bare flag set + direct `add_node_needing_layout`. R3 + R21 + R26 together author the Flutter-parity propagation walk plus 2-caller migration plus bootstrap.
- **Paint flag-clear retention rule = purge-warn** (memo D8) — preserves R-15 invariant from PR #109. Per-frame orphan tracking would be Cross.H scope, deferred.
- **D-3 does NOT require trait-surface expansion** (rev 2 correction, scope review confirmed) — `is_repaint_boundary()` + `always_needs_compositing()` already exist on `RenderObject<P>` trait at `traits/render_object.rs:199, 229`. PR-A2 implements `_updateSubtreeCompositingBits` against existing accessors; stale TODO at `owner.rs:958-960` deleted as part of the walk implementation.
- **PR sequencing: 7 PRs in strict serial merge order** (PR-C-1 → PR-C-2 → PR-B-gate → PR-A1 → PR-A2 → PR-C-3, plus PR-B-followup parallel). File fences enforce non-overlap (memo D9).
- **Layer/semantics repair plan splits into PR-B-gate (Waves 3+4) + PR-B-followup (rest)** (memo D11) — only Wave 3 lifecycle + Wave 4 slab-tree hygiene gate D-3; other waves are Cross.H foundation hardening that is independent of D-block.
- **Trigger #8 install in PR-C-3, last in sequence** (memo D10) — close-first-install-after precedent from PR #134 / FR-036. D-block PRs close SP-1 violations first; PR-C-3 ships gate with zero violations + `// STUB-OK` discipline for any remaining stubs.
- **PR-C splits into PR-C-1 (`flui-log` merge) + PR-C-2 (`flui-geometry` split + Constitution) + PR-C-3 (refusal triggers)** (memo D9) — separates Cargo.toml conflict surfaces; allows trigger install to land LAST.
- **No speckit ceremony for D-block.** Companion decision memo + this requirements doc + architecture-correction-plan §D-1/D-3/D-4 + ROADMAP Core.0 exit criteria provide the spec substrate. Speckit `spec → plan → tasks` would duplicate without adding decision capture.

---

## Dependencies / Assumptions

- **Memory `symmetric-defects-and-bounds`**: symmetric code shares defects. R11 verification check ensures `needs_compositing` / `needs_paint` sort orderings remain symmetric with `needs_layout` post-edit.
- **Memory `no-quick-wins-vanyastaff`**: full consolidation including breaking ripples; defer-with-excuse pattern forbidden. Sets Flutter-shape-parity scope of D-1 + drives memo D1–D12 resolutions.
- **Memory `agent-dispatch-standards`**: any subagent dispatched for this work must frame Senior Rust + Flutter-port philosophy + 2026 quality bar + atomic-commit-per-unit shape; orchestrator catches sloppy work pre-review.
- **Verified 2026-05-23 (this brainstorm rev 2)**: pipeline `run_frame` has **3 production consumers** (`crates/flui-app/src/bindings/renderer_binding.rs:438`, `crates/flui-app/src/app/binding.rs:406`, `crates/flui-hot-reload/src/pipeline.rs:161`). Rev 1's "zero external consumers" claim was false. D-block test scope expanded to include `cargo test -p flui-app -p flui-hot-reload`.
- **Verified 2026-05-23 (this brainstorm)**: `is_relayout_boundary` storage flag, `RenderError::relayout_boundary` / `RenderError::layout_depth_exceeded` error variants, `LayoutContext::layout_child` / `layout_and_position_child` / `layout_all_children` helpers, `compute_relayout_boundary` function, and `get_parent_and_children_mut` disjoint-borrow primitive ALL exist. D-1 walk rewrite has scaffolding in place; orchestration wire-in only, not greenfield API.
- **Verified 2026-05-23 (this brainstorm)**: `RenderState::set_constraints` + `set_geometry` panic-on-already-set (`crates/flui-rendering/src/storage/state/constraints.rs:38`, `state/geometry.rs:71`). R20 prep commit mandatory.
- **Verified 2026-05-23 (this brainstorm)**: `is_repaint_boundary()` + `always_needs_compositing()` already exist on `RenderObject<P>` trait (`traits/render_object.rs:199, 229`). D-3 needs no trait expansion.
- **Verified 2026-05-23 (this brainstorm)**: blanket impl `<T: RenderBox> RenderObject<BoxProtocol> for T` at `traits/render_box.rs:380-395` has no-op `perform_layout_raw` returning `*self.size()` — Size::ZERO for fresh objects. R23 bridge mandatory or AE1 returns zero sizes.
- **Verified 2026-05-23 (this brainstorm)**: `dirty.needs_compositing`, `dirty.needs_paint`, `dirty.needs_semantics` queues exist symmetric with `dirty.needs_layout`. All four already sort-by-depth correctly (`owner.rs:763/945/1049/1373`). R11 is verification, not new sort code.
- **Verified 2026-05-23 (this brainstorm)**: `mark_needs_layout` callers — 2 production sites (`crates/flui-view/src/element/behavior_commons.rs:244`, `crates/flui-hot-reload/src/pipeline.rs:156`) both currently use direct `add_node_needing_layout` (no propagation). R21 migration scope confirmed.
- **Verified 2026-05-23 (this brainstorm)**: `scripts/port-check.sh:308` confirms trigger #9 already installed as FR-036 (sanctioned-dyn-boundary registry, 32-trait allowlist) from PR #134. R13 numbering corrected to #8/#10/#11/#12/#13.
- **Plan exists**: [`plans/2026-05-22-004-feat-layer-semantics-repair-plan.md`](../plans/2026-05-22-004-feat-layer-semantics-repair-plan.md) — 24 commits / 7 waves / +550 LOC. R12a executes Wave 3+4 only (gates D-3+D-4). R12b executes rest (parallel, independent).
- **Companion decision memo**: [`docs/research/2026-05-23-d-block-architecture-decision-memo.md`](../research/2026-05-23-d-block-architecture-decision-memo.md) carries D1–D12 binding architecture decisions with concrete API signatures and code sketches. Plan-write inherits memo decisions; deviations require explicit Key Decision entries.

---

## Outstanding Questions

### Resolve Before Planning

(none — all P0 forks resolved in companion decision memo; rev 2 incorporates resolutions.)

### Deferred to Planning

- **[Affects R23][Technical]** Exact shape of `LayoutCtxErased` trait — generic-over-protocol enum vs separate type per protocol vs trait-object-friendly wrapper. Investigate during planning; pick the option that ripples least through existing `RenderObject<P>` impls.
- **[Affects R7][Needs research]** Flutter's `_updateSubtreeCompositingBits` algorithm at `.flutter/.../object.dart:3226-3258` has subtle ordering rules around `needs_compositing` clear-pass; port-faithful implementation needs reading Flutter source + adjacent helpers during planning.
- **[Affects R13][Technical]** Exact regex / file-list for each new refusal trigger #8/#10/#11/#12/#13; depends on architecture-correction-plan §2 specifications. Investigate during planning of PR-C-3.
- **[Affects R14][Needs research]** `flui-log` callers — full grep + migration list verified (7 sites, 5 files, 3 crates per ce-repo-research-analyst 2026-05-23). Confirm prelude-glob ambiguity check during planning.
- **[Affects R15][Needs research]** `flui-geometry` member set — 25 files identified per ce-repo-research-analyst. Confirm exact partition (which 25 stay in `flui-types`, which exit) and re-export shape during planning.
- **[Affects R22][Technical]** Final shape of `ErasedConstraints` / `ErasedGeometry` enums — naming, location (in `flui-rendering::storage` or a new `flui-rendering::protocol::erased` module), `From`/`TryFrom` conversion helpers. Investigate during planning.
