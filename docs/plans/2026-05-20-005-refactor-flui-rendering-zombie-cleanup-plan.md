---
date: 2026-05-20
type: refactor
status: active
origin: docs/brainstorms/flui-rendering-zombie-cleanup-requirements.md
depth: lightweight
target_crate: flui-rendering
target_branch: naughty-jackson-324931
---

# refactor: flui-rendering Phase 1 zombie cleanup

## Summary

Five atomic commits that remove confirmed-zombie items from `flui-rendering` and synchronise `CLAUDE.md` crate status with `Cargo.toml`: legacy commented `impl RenderObject for RenderView`, sealed-zero-impl `IntrinsicProtocol` + `BaselineProtocol`, the unreachable `RenderState::mark_needs_*` propagation impl bulk (trait shape preserved at `pub(crate)` on cost-prudence grounds), the `CLAUDE.md` ↔ `Cargo.toml` crate-status divergence, and a final audit-doc Step 1 acknowledgement.

## Problem Frame

The Mythos audit ([docs/research/2026-05-20-mythos-audit-render-paint-layer-engine.md](../research/2026-05-20-mythos-audit-render-paint-layer-engine.md)) plus two `ce-doc-review` rounds against the origin brainstorm ([docs/brainstorms/flui-rendering-zombie-cleanup-requirements.md](../brainstorms/flui-rendering-zombie-cleanup-requirements.md)) reduced the actionable Step 1 cleanup to four atomic deletions, plus an audit-doc acknowledgement. Each item has zero hidden-use risk in the workspace — verified by grep across `crates/` for each deleted symbol and by `Get-ChildItem` on the actual file structure. The production dirty-marking path is `PipelineOwner::add_node_needing_layout / add_node_needing_paint` invoked from `flui-view` and `flui-hot-reload`, not `RenderState::mark_needs_*` or `AtomicRenderFlags::set_needs_*`, so the deleted propagation methods and their `MockTree` tests exercise an unreachable code path. The trait shape itself is preserved (downgraded to `pub(crate)`) on the cost argument: 40 lines is cheaper to keep than to re-create if a future viewport-invalidation hook adopts the same tree-decoupling shape — not as an audit endorsement of the shape.

## Requirements

Carries forward from origin (see [origin: docs/brainstorms/flui-rendering-zombie-cleanup-requirements.md](../brainstorms/flui-rendering-zombie-cleanup-requirements.md)). R1-R4 are functional cleanup requirements; R5-R8 are verification gates that apply across all units. U5 (audit-doc Step 1 acknowledgement) is a plan-level addition realising an explicit Success Criterion from the origin, not a net-new requirement.

**Functional requirements (origin):**
- **R1**: Delete commented-out `impl RenderObject for RenderView` block.
- **R2**: Delete `IntrinsicProtocol` + `BaselineProtocol` traits + all re-exports.
- **R3**: Delete `RenderState::mark_needs_*` impl bulk + `MockTree` + four propagation tests; keep `RenderDirtyPropagation` trait declaration at `pub(crate)`; preserve five non-propagation tests.
- **R4**: Fix `CLAUDE.md` crate-status divergence (both directions: `flui-rendering`/`flui-view` disabled→active; `flui-build` active→disabled).

**Verification gates (origin, apply across U1-U4):**
- **R5**: `cargo build --workspace` passes after each commit.
- **R6**: `cargo test --workspace` passes after R3-bearing commit and stays passing.
- **R7**: `cargo clippy --workspace --all-targets -- -D warnings` passes after the final commit (justfile-aligned form).
- **R8**: Post-cleanup grep returns zero non-deletion-diff hits for the deleted symbols.

## Output Structure

No new directory hierarchy created. All edits modify existing files. Per-unit `**Files:**` sections are authoritative.

## Key Technical Decisions

- **Commit-scope tag = `refactor(rendering):`** for U1-U3 (no `flui-` prefix) per recent flui-rendering precedent (commits `4d05efc5`, `dc0fa1ad`, `d0e53c63`). U4 + U5 use `docs(claude-md):` and `docs(plans):` respectively per `1b4ddecf` precedent.
- **Five commits, not four** per user choice. U5 separates audit-doc Step 1 acknowledgement from R4's code-status doc fix — cleaner audit trail, avoids mixing two unrelated doc files in one commit.
- **`pub(crate)` re-export preserved** in `crates/flui-rendering/src/storage/state/mod.rs`, downgraded from `pub`. Remove the re-export entirely only if `cargo clippy` flags it as unused after the visibility narrowing. Resolves origin Deferred-to-Planning #1.
- **`missing_docs` lint compliance:** keep a brief `///` rustdoc one-liner above the `// PRESERVED_FOR:` line comment so the workspace `missing_docs = "warn"` lint at [Cargo.toml:181](../../Cargo.toml) stays satisfied after the `pub(crate)` downgrade. Avoids `#[allow(missing_docs)]` which would silently mask other doc gaps if added later.
- **Verification surface includes `bash scripts/port-check.sh -v`** after each commit per institutional precedent (every recent Mythos cleanup commit ran it; reviewer-enforced gate). 7 triggers ok.
- **Verification surface includes `cargo build -p flui-hot-reload --features app-plugin --all-targets`** after U3 to catch ABI-shape regression on the widget-pipeline path. Resolves origin Deferred-to-Planning #2.
- **Clippy command aligns to justfile** `--all-targets` only (not `--all-features` as origin brainstorm wrote). Justfile is the canonical verification path (`just clippy` → `cargo clippy --workspace --all-targets -- -D warnings`).
- **Commit message trailer required:** `Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>` per `CLAUDE.md` directive and observed precedent on every Mythos chain commit.
- **Line-number policy: symbol-based discovery.** Implementer uses `rg` on symbol names at edit time; cited positions are illustrative only.
- **`crates.md` NOT touched.** [docs/crates.md](../../docs/crates.md) already lists `flui-rendering`/`flui-view` as Active and `flui-build` as Disabled; only `CLAUDE.md` is divergent. Editing `crates.md` is out of scope.

## Implementation Units

### U1. Delete legacy commented `impl RenderObject for RenderView`

**Goal:** Remove the 190+ line pre-U2-refactor commented impl block. Pure text deletion — no code paths affected.

**Requirements:** R1 (from origin).

**Dependencies:** None — leaf change.

**Files:**
- [crates/flui-rendering/src/view/render_view.rs](../../crates/flui-rendering/src/view/render_view.rs) — delete the comment block (lines ~524-730, delimited by the banner `// === RenderObject Implementation (Legacy - commented out) ===` and the matching closing `// }` near end of file).

**Approach:**
- Locate the banner via `rg '// === RenderObject Implementation \(Legacy - commented out\) ==='`.
- Delete from the banner line through the last `// }` of the commented impl.
- Verify no other commented-impl markers remain via `rg '// impl RenderObject for RenderView' crates/`.

**Test scenarios:** none — pure comment deletion, no behavioural change.

**Verification:**
- `cargo check -p flui-rendering` passes.
- `cargo build --workspace` passes.
- `bash scripts/port-check.sh -v` reports 7 triggers ok.
- Post-edit `rg '// impl RenderObject for RenderView' crates/` returns zero hits.

---

### U2. Delete `IntrinsicProtocol` + `BaselineProtocol` sealed traits

**Goal:** Remove two sealed-zero-impl protocol traits and all their re-exports from the crate root, the protocol module, and the prelude.

**Requirements:** R2 (from origin).

**Dependencies:** None — leaf change. May land before or after U1.

**Files:**
- [crates/flui-rendering/src/protocol/protocol.rs](../../crates/flui-rendering/src/protocol/protocol.rs) — delete the `pub trait IntrinsicProtocol: Protocol { ... }` and `pub trait BaselineProtocol: Protocol { ... }` declarations.
- [crates/flui-rendering/src/protocol/mod.rs](../../crates/flui-rendering/src/protocol/mod.rs) — remove `IntrinsicProtocol` and `BaselineProtocol` from the module-level `pub use protocol::{...}` block AND from the `pub mod prelude { pub use super::{...} }` block.
- [crates/flui-rendering/src/lib.rs](../../crates/flui-rendering/src/lib.rs) — remove the crate-root re-exports of `BaselineProtocol` (around line 158) and `IntrinsicProtocol` (around line 168).

**Approach:**
- Symbol-based discovery: `rg 'IntrinsicProtocol|BaselineProtocol' crates/flui-rendering/src/` enumerates every reference.
- Delete the trait declarations.
- Delete every `pub use` line that names either trait.
- Workspace-wide grep `rg 'IntrinsicProtocol|BaselineProtocol' crates/ --glob '!**/target/**'` returns only deletion-diff hits.

**Test scenarios:** none — sealed traits with zero implementers, zero callers. No tests reference them.

**Verification:**
- `cargo check -p flui-rendering` passes.
- `cargo build --workspace` passes.
- `bash scripts/port-check.sh -v` reports 7 triggers ok.
- Post-edit `rg 'IntrinsicProtocol|BaselineProtocol' crates/` returns zero hits.
- Post-edit `rg 'IntrinsicProtocol|BaselineProtocol' docs/` may return hits (audit + brainstorm + this plan reference them in prose); those are out of scope for the grep gate.

---

### U3. Delete `RenderState` propagation impl bulk + tests; preserve trait at `pub(crate)`

**Goal:** Remove the unreachable propagation methods on `RenderState<P>` and their test infrastructure. Preserve the trait declaration shape at `pub(crate)` visibility with an explicit preservation marker.

**Requirements:** R3 (from origin), Deferred-to-Planning #1, #2.

**Dependencies:** None — leaf change. May land before or after U1, U2.

**Files:**
- [crates/flui-rendering/src/storage/state/propagation.rs](../../crates/flui-rendering/src/storage/state/propagation.rs)
- [crates/flui-rendering/src/storage/state/tests.rs](../../crates/flui-rendering/src/storage/state/tests.rs)
- [crates/flui-rendering/src/storage/state/mod.rs](../../crates/flui-rendering/src/storage/state/mod.rs)

**Approach:**

In `state/propagation.rs`:
- Delete the entire `impl<P: Protocol> RenderState<P>` block (~430 LOC, lines ~84-516). This carries five methods: `mark_needs_layout`, `mark_parent_needs_layout`, `mark_needs_paint`, `mark_needs_compositing`, and `mark_needs_compositing_bits_update`.
- Keep the `RenderDirtyPropagation` trait declaration itself (lines ~39-78).
- Downgrade the trait's `pub` keyword to `pub(crate)`.
- Replace the trait's existing rustdoc with the new combined doc-comment block:

  ```rust
  /// Tree operations needed by boundary-aware dirty propagation (preserved as cost-cheap option).
  // PRESERVED_FOR: future viewport-invalidation hook (audit Step 4 item 13 contemplates pinning down the production dirty-marking path; this trait shape may or may not be adopted at that time — kept as cost-cheap option, not as an endorsed design).
  pub(crate) trait RenderDirtyPropagation { /* unchanged body */ }
  ```

  Format: brief `///` rustdoc on the first line satisfies the workspace `missing_docs = "warn"` lint; the `// PRESERVED_FOR:` marker on the next line preserves the bounded rationale in-source. This is directional formatting guidance — the implementer may adjust prose so long as both pieces (one-line rustdoc + `PRESERVED_FOR` marker) survive.

In `state/tests.rs`:
- Delete the `MockTree` struct and its `impl RenderDirtyPropagation for MockTree` block.
- Delete the four propagation tests: `test_mark_needs_layout_propagates_to_parent`, `test_mark_needs_layout_stops_at_relayout_boundary`, `test_mark_needs_layout_early_return`, `test_mark_parent_needs_layout_ignores_boundary`.
- Preserve the five non-propagation tests: `render_state_box_fits_budget`, `render_state_sliver_fits_budget`, `test_geometry_write_once`, `test_atomic_offset`, `test_boundary_flags`.
- Remove imports that become unused (likely candidates: `HashMap`, `Arc`, `Mutex` brought in solely for MockTree). Verify by running `cargo build -p flui-rendering` first; the compiler flags unused imports after MockTree removal.

In `state/mod.rs`:
- Change `pub use propagation::RenderDirtyPropagation` (around line 152) to `pub(crate) use propagation::RenderDirtyPropagation`. Keep the `#[allow(unused_imports)]` attribute (if present at line 151) conservatively — remove only if `cargo clippy` explicitly flags it.

**In-commit edge-case handling (per user-memory directive "no defer-with-excuse"):** if any of the above edits surface compilation breakage in code outside the stated files (e.g., an unanticipated in-crate caller of `RenderState::mark_needs_*`), fix the breakage in this commit. Do not defer to a follow-up.

**Patterns to follow:**
- Existing `#[allow(dead_code)]` placeholder marker pattern in [crates/flui-rendering/src/view/render_view.rs](../../crates/flui-rendering/src/view/render_view.rs) lines 65-87 — line comments explaining intent placed adjacent to the declaration.
- Existing rustdoc + section banner conventions in [crates/flui-rendering/src/storage/state/propagation.rs](../../crates/flui-rendering/src/storage/state/propagation.rs) lines 1-9 (module-level `//!` + banner format).

**Test scenarios:**
- **Test expectation: none on new behaviour** — this unit deletes tests and methods, it does not add behaviour. The five preserved tests in `tests.rs` continue to enforce existing semantics:
  - `render_state_box_fits_budget` — `RenderState<BoxProtocol>` ≤128 bytes (Mythos Step 14 memory-budget guard).
  - `render_state_sliver_fits_budget` — `RenderState<SliverProtocol>` ≤192 bytes.
  - `test_geometry_write_once` — `OnceCell<ProtocolGeometry>` panics on second set.
  - `test_atomic_offset` — atomic offset round-trip.
  - `test_boundary_flags` — repaint/relayout boundary accessors.
- After U3 these five tests must still compile and pass.

**Verification:**
- `cargo check -p flui-rendering` passes.
- `cargo test -p flui-rendering --lib` passes; the five preserved tests still run.
- `cargo test -p flui-rendering --tests` passes.
- `cargo build --workspace` passes.
- `cargo build -p flui-hot-reload --features app-plugin --all-targets` passes (Deferred-to-Planning #2 verification — catches widget-pipeline ABI regression).
- `bash scripts/port-check.sh -v` reports 7 triggers ok.
- Post-edit greps return zero hits across `crates/flui-rendering/` (excluding deletion-diff):
  - `rg 'MockTree' crates/flui-rendering/` — scope-restricted because [crates/flui-tree/src/iter/cursor.rs:762](../../crates/flui-tree/src/iter/cursor.rs) defines an unrelated `MockTree` for tree cursor iteration tests; a workspace-wide grep would false-positive.
  - `rg 'RenderState::mark_needs_layout|RenderState::mark_needs_paint|RenderState::mark_parent_needs_layout|RenderState::mark_needs_compositing' crates/` — workspace-wide; these symbols are unique to `flui-rendering`.
  - `rg 'use flui_rendering::.*RenderDirtyPropagation' crates/` — path-qualified external import patterns, workspace-wide.

---

### U4. Fix `CLAUDE.md` ↔ `Cargo.toml` crate-status divergence

**Goal:** Reconcile `CLAUDE.md`'s "Active crates" and "Temporarily disabled" subsections with the ground-truth state in `Cargo.toml`. Both directions: promote `flui-rendering` + `flui-view`, demote `flui-build`.

**Requirements:** R4 (from origin).

**Dependencies:** None — independent doc edit. Land last among code-cleanup commits for clean log shape.

**Files:**
- [CLAUDE.md](../../CLAUDE.md)

**Approach:**
- **Move `flui-rendering` and `flui-view`** out of the "Temporarily disabled until integration complete" list at `CLAUDE.md` around line 35 into the appropriate "Active crates" subsections at lines 29-31 (`flui-rendering` belongs under "Framework"; `flui-view` belongs under "Framework").
- **Move `flui-build`** out of the "Active crates: Tools" subsection around line 32 into the "Temporarily disabled" list. Update prose mentioning `flui-build` as live.
- Verify the final "Temporarily disabled" list matches `Cargo.toml:46-50` exactly: `flui-animation`, `flui-reactivity`, `flui-devtools`, `flui-cli`, `flui-build` (alphabetical or as ordered in Cargo.toml; match Cargo.toml ordering for consistency).
- `flui-engine` and `flui-layer` are already in the Active section — verify, do not duplicate.

**Test scenarios:** none — pure doc fix.

**Verification:**
- After editing, the lists in `CLAUDE.md` match `Cargo.toml:21-22` (Active: `flui-rendering`, `flui-view`) and `Cargo.toml:46-50` (Disabled: `flui-animation`, `flui-reactivity`, `flui-devtools`, `flui-cli`, `flui-build`).
- `cargo build --workspace` still passes (no impact on build, just doc consistency).
- `bash scripts/port-check.sh -v` reports 7 triggers ok.
- Cross-check with `docs/crates.md` — that file is already correct and should not need editing. If divergence found, raise to user; do not silently edit `crates.md`.

---

### U5. Mark audit doc Step 1 partially complete + final cleanup verification

**Goal:** Update the Mythos audit document to record that Step 1 (Safe deletions) is partially complete with this batch, and note that ClipContext consolidation moved to a separate brainstorm.

**Requirements:** Origin Success Criteria — "The audit document is updated to mark Step 1 (Safe deletions) partially complete with links to the commits, and to note that `ClipContext` consolidation has moved to a separate brainstorm."

**Dependencies:** U1, U2, U3, U4 must have landed (need their commit hashes for the audit-doc references).

**Files:**
- [docs/research/2026-05-20-mythos-audit-render-paint-layer-engine.md](../research/2026-05-20-mythos-audit-render-paint-layer-engine.md)

**Approach:**
- Add a short status annotation to the audit doc's "Restructuring Plan → Step 1 — Safe deletions" subsection noting:
  - Items 1, 2, 3 (commented impl, IntrinsicProtocol + BaselineProtocol, RenderDirtyPropagation impl bulk) — completed; reference commits U1, U2, U3 hashes (filled in at edit time after the commits land).
  - Item "delete duplicate ClipContext" — deferred to a separate brainstorm because round-1 ce-doc-review uncovered the missed `CanvasContext` production impl and trait-shape incompatibility; cleanup will land as part of the ClipContext consolidation work.
  - `CLAUDE.md` fix — completed in U4.
- Format the annotation as a short status block at the top of the "Step 1" section, not as a rewrite. Preserve all existing recommendation text.

**Test scenarios:** none — pure doc update.

**Verification:**
- `cargo build --workspace` passes (no code impact).
- The audit doc's Step 1 section clearly indicates partial completion with commit references and explicit ClipContext deferral note.

---

## Verification

The implementer runs the following after each commit. All must pass:

- `cargo check -p flui-rendering` (clean)
- `cargo test -p flui-rendering --lib` (passing count noted in commit body)
- `cargo test -p flui-rendering --tests` (passing count noted in commit body)
- `cargo build --workspace` (clean)
- `bash scripts/port-check.sh -v` (7 triggers ok; institutional gate)

After U3 specifically (Deferred-to-Planning #2):
- `cargo build -p flui-hot-reload --features app-plugin --all-targets` (clean)

After U4 (final commit before U5):
- `cargo clippy --workspace --all-targets -- -D warnings` (clean) — runs full clippy gate matching `just clippy`.

Post-cleanup (after U5):
- Workspace-wide grep audit returns zero non-deletion-diff hits across `crates/` for each of: `IntrinsicProtocol`, `BaselineProtocol`, `RenderState::mark_needs_layout`, `RenderState::mark_needs_paint`, `RenderState::mark_parent_needs_layout`, `RenderState::mark_needs_compositing`, `MockTree`, `// impl RenderObject for RenderView`.
- `RenderDirtyPropagation` symbol grep should return only the preserved declaration at `crates/flui-rendering/src/storage/state/propagation.rs` and the `pub(crate)` re-export in `state/mod.rs`. External path-qualified import patterns (`use flui_rendering::.*RenderDirtyPropagation`) should be zero outside `crates/flui-rendering/`.
- `CLAUDE.md` "Temporarily disabled" list equals `Cargo.toml:46-50` set.
- Audit doc Step 1 carries the completion annotation referencing the U1-U4 commit hashes.

## Scope Boundaries

- **In scope:** the five units U1-U5 above, executed atomically in order.

### Deferred to Follow-Up Work

- **`scripts/port-check.sh:147` `state.rs` glob cleanup** — research surfaced that `--glob '!**/state.rs'` in `port-check.sh` becomes a stale exclusion after R3 lands (the `state.rs` god-module no longer exists; `state/` is a directory). Not a blocker; can be cleaned up in a separate trivial doc-tools commit. Out of scope for this batch.
- **Pre-built external plugin `.dll/.so/.dylib` rebuilds.** External downstream consumers of `flui-rendering` (e.g., custom `app-plugin` builds out of tree) must rebuild against the new crate revision. This is a known one-off cost called out in the origin brainstorm Dependencies; no in-batch action.

### Outside this batch's scope

These items belong to separate brainstorms / plans — do not pull them in:

- **`ClipContext` consolidation** (originally R4 in draft 1 of the brainstorm). Reshaped into a separate brainstorm because round-1 ce-doc-review uncovered the missed `CanvasContext` production impl and incompatible trait signatures. Includes the `flui-painting/ARCHITECTURE.md` doc-lie fix.
- **`SUPERELLIPSE_CACHE` bounding** (audit Priority #4).
- **`SceneBuilder` missing methods** (audit Priority #2).
- **`PictureLayer` hint fields** (audit Priority #3).
- **`RendererBinding` redesign** (audit Priority #5).
- **Delegate trait visibility narrowing** (CustomPainter, FlowDelegate, MultiChildLayoutDelegate, SingleChildLayoutDelegate).
- **Lyon tessellation feature-flag move** (audit Step 16).
- **`pipeline.rs` / `pipelines.rs` consolidation** (audit Step 10).
- **`Arc<Mutex<OffscreenRenderer>>` ownership review** (audit Step 12).
- **`RenderObject` roadmap** (audit Priority #6 — 88% Flutter parity gap).
- **Production integration test for dirty-marking path** (audit Step 4 item 13) — separately scoped follow-up.

Each gets its own brainstorm / plan iteration. If breakage in any of these surfaces mid-edit on the current cleanup, fix in-commit (per user-memory directive); do not defer to a follow-up unless the breakage is genuinely orthogonal.

## Risks & Dependencies

- **R-A1 (Low):** `cargo clippy` flags newly unused imports in `state/tests.rs` after MockTree removal. **Mitigation:** clippy/build flags them; implementer removes the listed imports in the same U3 commit. Pre-cleanup compiler check before final commit catches this. Per memory directive, do not defer to a follow-up.
- **R-A2 (Low):** `cargo clippy` flags the preserved re-export at `state/mod.rs:152` as unused after `pub(crate)` downgrade. **Mitigation:** per Deferred-to-Planning #1 decision, remove the re-export entirely if clippy flags it. In-commit fix. The `#[allow(unused_imports)]` attribute may also need removal.
- **R-A3 (Low):** A previously-unidentified in-crate caller of `RenderDirtyPropagation` surfaces after `pub(crate)` downgrade. **Mitigation:** grep already verified zero callers; if one surfaces (e.g., a doc-test), fix in-commit. Per memory directive, no defer.
- **R-A4 (Medium):** External downstream `app-plugin` consumers built against the prior crate revision fail to link after rebuild. **Mitigation:** explicitly out of scope (origin Dependencies notes this); the brainstorm's r3 dropped the in-tree `desktop_scene` smoke test as ineffective. `cargo build -p flui-hot-reload --features app-plugin --all-targets` in U3 verification catches the in-tree case. External rebuild is a known one-off cost.
- **R-A5 (Low):** `bash scripts/port-check.sh -v` trigger count changes from 7 to a different number after the cleanup. **Mitigation:** observe and accept; the cleanup may move counts (deleting commented impl + dead methods may reduce trigger hits). If the count changes, document the new baseline in the U1 commit body and continue.

**Dependencies:** None outside the worktree branch `naughty-jackson-324931`. Land all five commits on that branch. Open PR (or merge directly per project workflow) after U5 lands.

## Outstanding Questions

### Deferred to Implementation

- [Affects U3][Technical] Whether the `#[allow(unused_imports)]` attribute at `crates/flui-rendering/src/storage/state/mod.rs:151` survives the `pub(crate)` downgrade. Default: keep conservatively; `cargo clippy` decides at edit time.
- [Affects U5][Technical] Exact prose of the audit-doc Step 1 annotation — implementer writes the short status block reading the existing audit doc; format follows the document's existing tone (terse, technical).
