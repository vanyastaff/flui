---
date: 2026-05-20
topic: flui-rendering-zombie-cleanup
scope: lightweight
audit_source: docs/research/2026-05-20-mythos-audit-render-paint-layer-engine.md
revision: r3 (post ce-doc-review rounds 1 + 2 — file paths verified against worktree, R9 dropped, R4 widened)
---

# flui-rendering Phase 1 zombie cleanup

## Summary

Atomic four-commit cleanup that removes confirmed-zombie items from `flui-rendering` and synchronises `CLAUDE.md` crate status with `Cargo.toml`: a 190+ line commented `impl RenderObject for RenderView` block, two sealed-zero-impl protocol traits, the unreachable `RenderState::mark_needs_*` propagation impl bulk + its tests + the now-orphan `MockTree` (while keeping the `RenderDirtyPropagation` trait declaration itself at `pub(crate)` visibility on cost-cheap-to-preserve grounds), and one stale crate-status block in `CLAUDE.md`. Pure cleanup — no functional changes on actually-invoked code paths, no public API additions, no replacement abstractions.

## Problem Frame

The Mythos audit ([docs/research/2026-05-20-mythos-audit-render-paint-layer-engine.md](../research/2026-05-20-mythos-audit-render-paint-layer-engine.md)) catalogued a set of zombie abstractions and stale docs across the `flui-{rendering,painting,layer,engine}` stack. Two `ce-doc-review` rounds whittled the scope to four atomic deletions:

- **190+ lines of commented-out `impl RenderObject for RenderView`** survived the U2 refactor as "reference". Pure text deletion; git history preserves the legacy shape.
- **`IntrinsicProtocol` + `BaselineProtocol`** are sealed traits with zero implementers and zero callers. Future intrinsic/baseline work will need a different abstraction shape (extension traits on `Protocol`, not sealed) — confirmed by Flutter cross-reference (Flutter's intrinsic API is wide and unsealed, used by `RenderIntrinsicWidth`, `RenderConstrainedBox`, `RenderAspectRatio`).
- **`RenderDirtyPropagation`'s impl bulk on `RenderState<P>`** plus the four propagation tests and the `MockTree` helper are unreachable in production. Production dirty marking goes through `PipelineOwner::add_node_needing_layout / add_node_needing_paint` called directly from `flui-view` and `flui-hot-reload`; nothing calls `RenderState::mark_needs_*`, and `collect_nodes_needing_layout / collect_nodes_needing_paint` (the matching pipeline-side AtomicRenderFlags-consuming entry points) have zero callers. The bulk goes. The trait declaration itself stays — narrowed to `pub(crate)` — on the cost argument that the 40-line trait shape is cheap to preserve while a future viewport-invalidation hook may or may not adopt it (the audit's Step 4 item 13 documents the path-pinning work but does not endorse this specific trait shape; preservation is a cost-prudence call, not an audit recommendation).
- **`CLAUDE.md`** lists `flui-rendering` and `flui-view` as "Temporarily disabled" though `Cargo.toml:21-22` lists them as Active, AND lists `flui-build` as Active under "Tools" though `Cargo.toml:50` has `# "crates/flui-build",` commented out. Both directions need fixing in the same edit.

Each of these costs review attention, misleads new contributors, and inflates the public API surface.

A fifth item from the original drafting — **`ClipContext` trait consolidation** — has been **deferred** to a separate brainstorm. Round-1 `ce-doc-review` discovered that `flui-rendering::ClipContext` has a production implementer (`CanvasContext` at [crates/flui-rendering/src/context/canvas.rs:695](../../crates/flui-rendering/src/context/canvas.rs)) that the audit's `impl ClipContext for` grep missed due to a `super::` qualification, and that the two traits have incompatible signatures (`canvas` vs `canvas_mut` accessor, `FnOnce(&mut Canvas)` vs `FnOnce(&mut Self)` painter callback, `Rect` vs `Rect<Pixels>` typed-unit divergence). Consolidating is a migration, not a deletion — out of scope for this Lightweight pure-cleanup batch. The follow-up brainstorm will also fix [crates/flui-painting/ARCHITECTURE.md](../../crates/flui-painting/ARCHITECTURE.md) lines 37 and 94, which currently claim `CanvasContext` implements `flui-painting::ClipContext` (it does not).

## Requirements

**Line-number policy:** brainstorm cites paths and symbol names; line numbers are illustrative only. Implementer uses `grep` / `rg` on the symbol name at edit time and ignores any cited line position. The cited counts (e.g. "~430 LOC", "190+ lines") are approximate.

**Deletion sweep — atomic commits**

- R1. Delete the commented-out legacy `impl RenderObject for RenderView` block in [crates/flui-rendering/src/view/render_view.rs](../../crates/flui-rendering/src/view/render_view.rs). The block is delimited by the comment banner `// === RenderObject Implementation (Legacy - commented out) ===` and the matching closing `// }` near end-of-file. Post-edit grep `// impl RenderObject for RenderView` returns zero hits (commit 1).

- R2. Delete `IntrinsicProtocol` and `BaselineProtocol` traits from [crates/flui-rendering/src/protocol/protocol.rs](../../crates/flui-rendering/src/protocol/protocol.rs) plus their re-exports from [crates/flui-rendering/src/lib.rs](../../crates/flui-rendering/src/lib.rs) (the prelude / lib-level re-export block) and [crates/flui-rendering/src/protocol/mod.rs](../../crates/flui-rendering/src/protocol/mod.rs) (the module-level `pub use protocol::{...}` and the prelude block — both reference the trait names). Post-edit grep across `crates/` for `IntrinsicProtocol` and `BaselineProtocol` returns zero hits outside `docs/` (commit 2).

- R3. In the `crates/flui-rendering/src/storage/state/` directory (note: directory, not single file):
  - In [crates/flui-rendering/src/storage/state/propagation.rs](../../crates/flui-rendering/src/storage/state/propagation.rs):
    - **Delete** the entire `impl<P: Protocol> RenderState<P>` block carrying `mark_needs_layout`, `mark_parent_needs_layout`, `mark_needs_paint`, `mark_needs_compositing`, and `mark_needs_compositing_bits_update` (all five methods — the file's own header at the top lists all five; the original drafting listed only four).
    - **Keep** the `RenderDirtyPropagation` trait declaration itself.
    - **Downgrade** the trait's `pub` to `pub(crate)`.
    - **Replace** the trait's doc-comment with: `// PRESERVED_FOR: future viewport-invalidation hook (audit Step 4 item 13 contemplates pinning down the production dirty-marking path; this trait shape may or may not be adopted at that time — kept as cost-cheap option, not as an endorsed design).`
  - In [crates/flui-rendering/src/storage/state/tests.rs](../../crates/flui-rendering/src/storage/state/tests.rs):
    - **Delete** the `MockTree` struct + its `impl RenderDirtyPropagation for MockTree` block.
    - **Delete** the four propagation tests: `test_mark_needs_layout_propagates_to_parent`, `test_mark_needs_layout_stops_at_relayout_boundary`, `test_mark_needs_layout_early_return`, `test_mark_parent_needs_layout_ignores_boundary`.
    - **Delete** any imports that become unused after the above (e.g. `HashMap`, `Arc`, `Mutex` if they were brought in solely for MockTree).
    - **Preserve** the five non-propagation tests: `render_state_box_fits_budget` (Mythos Step 14 memory-budget guard), `render_state_sliver_fits_budget` (memory-budget guard), `test_geometry_write_once` (OnceCell write-once semantics), `test_atomic_offset` (atomic offset round-trip), `test_boundary_flags` (repaint/relayout boundary accessors). These exercise live production semantics and are unrelated to the propagation deletion.
  - In [crates/flui-rendering/src/storage/state/mod.rs](../../crates/flui-rendering/src/storage/state/mod.rs):
    - Change `pub use propagation::RenderDirtyPropagation` to `pub(crate) use propagation::RenderDirtyPropagation` (or remove it entirely if no in-crate consumer needs the re-export). Drop the `#[allow(unused_imports)]` attribute attached to the re-export if removing makes it unnecessary; otherwise keep it.

  Post-edit: `cargo build -p flui-rendering` passes, `cargo test -p flui-rendering` passes, the five preserved tests still run and pass (commit 3).

- R4. Update [CLAUDE.md](../../CLAUDE.md):
  - **Move** `flui-rendering` and `flui-view` out of the "Temporarily disabled" subsection into the appropriate "Active crates" subsections, matching `Cargo.toml:21-22`.
  - **Move** `flui-build` out of the "Active crates: Tools" subsection into the "Temporarily disabled" subsection, matching `Cargo.toml:50` (currently commented out). Update any prose in the Active crates description that names `flui-build` as live.
  - Verify the resulting "Temporarily disabled" list matches `Cargo.toml:46-50` exactly: `flui-animation`, `flui-reactivity`, `flui-devtools`, `flui-cli`, `flui-build`.
  - `flui-engine` and `flui-layer` are already in the Active section — do not duplicate (commit 4).

**Verification**

- R5. `cargo build --workspace` passes after each commit.
- R6. `cargo test --workspace` passes after R3 (commit 3) and stays passing through R4 (commit 4). Within R3, the five preserved tests in `state/tests.rs` (`render_state_box_fits_budget`, `render_state_sliver_fits_budget`, `test_geometry_write_once`, `test_atomic_offset`, `test_boundary_flags`) continue to pass.
- R7. `cargo clippy --workspace --all-targets --all-features -- -D warnings` passes after the final commit.
- R8. Post-cleanup grep across `crates/` (and the workspace root, excluding `docs/` and `target/`) for the following returns zero non-deletion-diff hits:
  - `IntrinsicProtocol`, `BaselineProtocol` (full symbol names)
  - `RenderState::mark_needs_layout`, `RenderState::mark_needs_paint`, `RenderState::mark_parent_needs_layout`, `RenderState::mark_needs_compositing`, `RenderState::mark_needs_compositing_bits_update`
  - `MockTree`
  - `// impl RenderObject for RenderView` (commented-impl marker)
  - Path-qualified imports of the deleted symbols: `use flui_rendering::.*IntrinsicProtocol`, `use flui_rendering::.*BaselineProtocol`, `use flui_rendering::.*MockTree`. The `RenderDirtyPropagation` symbol is preserved as `pub(crate)`, so external-import patterns for it should also return zero hits — verify.

## Success Criteria

- Reviewers reading `view/render_view.rs`, `protocol/protocol.rs`, and `storage/state/propagation.rs` no longer waste attention on dead code paths.
- Workspace builds + tests + clippy stay green after each atomic commit.
- The audit document is updated to mark Step 1 (Safe deletions) partially complete with links to the four commits, and to note that `ClipContext` consolidation has moved to a separate brainstorm.
- `RenderDirtyPropagation` remains in the crate as a `pub(crate)` placeholder; the trait's narrow API surface and tree-decoupling rationale are not lost. The `// PRESERVED_FOR:` doc comment makes the preservation justification explicit and bounded.
- Git history shows four atomic commits — one per finding — enabling per-finding revert if needed.
- `CLAUDE.md` and `Cargo.toml` agree on which crates are active vs disabled.

## Scope Boundaries

- **Out of scope: `ClipContext` consolidation (originally R4 in draft 1).** Reshaped into a separate brainstorm. Round-1 ce-doc-review showed `flui-rendering::ClipContext` has a production implementer (`CanvasContext`) that audit grep missed, and the two traits have incompatible signatures. Consolidation requires migration work (closure-shape change, accessor rename, typed-unit Rect migration, callsite audit), not deletion — incompatible with this Lightweight batch.

- **Out of scope: deleting the `RenderDirtyPropagation` trait shape entirely.** The trait stays as `pub(crate)`. Only the impl bulk on `RenderState<P>` and the propagation tests + MockTree go. Deleting the trait now would force a re-create later if the audit's Step 4 item 13 work materializes the viewport hook; preservation is a cost-prudence call.

- **Out of scope: rewriting the deleted tests onto a different abstraction.** Round-1 ce-doc-review confirmed that `AtomicRenderFlags::set_needs_layout/paint` is also not the production dirty-marking mechanism — the real path is `PipelineOwner::add_node_needing_layout / add_node_needing_paint` invoked from `flui-view` and `flui-hot-reload`. Tests are dropped outright with explicit acknowledgement: they exercised an unreachable propagation code path. A new integration test covering the real path is a follow-up under audit Step 4 item 13, not this batch.

- **Out of scope: a smoke test of `examples/desktop_scene` after the cleanup.** Round-2 ce-doc-review showed `examples/desktop_scene` depends only on `flui-hot-reload`, `flui-layer`, `flui-types` — not on `flui-rendering`. A smoke test of that example cannot detect breakage from these deletions. `cargo build --workspace` (R5) covers in-tree linkage. Out-of-tree pre-built plugins built against a prior crate revision must rebuild after this cleanup — that is a known one-off cost called out in Dependencies / Assumptions.

- **Out of scope: adding `clip_superellipse_and_paint`** to `flui-painting::ClipContext` for Flutter 4-method parity. Belongs to the deferred ClipContext-consolidation brainstorm.

- **Out of scope: introducing replacement abstractions** for the deleted impl bulk. When dirty-propagation hooks are needed, they get a different design (likely tied to a real production caller, not a generic trait bound).

- **Out of scope: the other audit findings.** SUPERELLIPSE_CACHE bounding, SceneBuilder missing methods, PictureLayer hint fields, `RendererBinding` redesign, delegate trait visibility narrowing, Lyon tessellation feature-flag move, `pipeline.rs`/`pipelines.rs` consolidation, `Arc<Mutex<OffscreenRenderer>>` ownership review, and RenderObject roadmap — each gets its own brainstorm / plan iteration.

- **Out of scope: production behavioral changes on invoked code paths.** Every requirement is either a deletion of unreachable code, a visibility narrowing (`pub` → `pub(crate)`) on a trait with no production implementer, or a doc fix. No callable behavior on actually-invoked code paths changes.

## Key Decisions

- **Keep `RenderDirtyPropagation` trait shape; delete only the unused impl bulk on `RenderState<P>`.** Rationale: round-1 ce-doc-review's adversarial F6 finding showed the trait (≈40 lines) is the cheap part while the impl methods (~430 LOC) are the bulk. Deleting the trait now means re-creating it nearly identically if a future viewport-invalidation hook adopts the same tree-decoupling shape — but round-2 ce-doc-review correctly pointed out that the audit itself (Step 4 item 13 and the line-248 recommendation) does **not** endorse this specific trait shape for the viewport hook. So the preservation is justified on the cost-prudence argument alone, not as a design endorsement. Visibility downgrades to `pub(crate)` so the surface area is invisible to crate consumers until a real caller materializes. The `// PRESERVED_FOR:` doc comment makes the bounded rationale explicit.

- **Drop the four propagation tests + MockTree outright; do NOT rewrite them.** Original draft proposed rewriting onto `AtomicRenderFlags` direct calls on the assumption that AtomicRenderFlags is the production dirty path. Round-1 ce-doc-review's adversarial F2 finding showed this assumption is false — the production dirty path is `PipelineOwner::add_node_needing_layout / add_node_needing_paint` invoked from `flui-view` and `flui-hot-reload`, never AtomicRenderFlags. Rewriting onto AtomicRenderFlags would produce tests that pass forever while the real path stays uncovered. The deleted tests exercised an unreachable code path; replacing them with tests of a different unreachable path is worse than dropping them. A new integration test covering the real production path is a follow-up under audit Step 4 item 13.

- **Preserve the five non-propagation tests in `state/tests.rs`.** `render_state_box_fits_budget` and `render_state_sliver_fits_budget` guard the data-oriented-design memory budgets documented in [docs/designs/2026-05-20-mythos-flui-rendering-redesign.md](../../docs/designs/2026-05-20-mythos-flui-rendering-redesign.md) Section 9 — they are not propagation-related and remain load-bearing. `test_geometry_write_once`, `test_atomic_offset`, `test_boundary_flags` cover OnceCell / atomic / boundary semantics that are live production code. Test deletion is propagation-scoped only.

- **Drop R9 smoke test of `examples/desktop_scene` entirely.** Round-2 ce-doc-review's scope-guardian F3 showed that `desktop_scene` does not depend on `flui-rendering` — its plugin macro is `scene_plugin!` (pure `flui-layer`), not `app_plugin!` (which would pull `flui-rendering` behind the `app-plugin` feature). The smoke test as originally proposed is confidence theater. The verification surface that actually matters (in-tree workspace build with all transitive consumers) is already covered by R5's `cargo build --workspace`.

- **Defer `ClipContext` consolidation to a separate brainstorm.** Originally drafted as R4 with the assumption that `flui-rendering::ClipContext` had zero production implementers. Round-1 ce-doc-review uncovered the missed `CanvasContext` impl and the API-shape incompatibility between the two traits. Migration is non-trivial: closure-callback shape change, accessor rename, typed-unit Rect migration, and a sweep of every `clip_*_and_paint` call site. That work does not fit a "pure deletion, no functional changes" framing. A follow-up brainstorm will cover it together with the `flui-painting/ARCHITECTURE.md` doc-lie fix.

- **R4 widens to fix two doc-Cargo divergences in the same commit.** Round-2 ce-doc-review's scope-guardian F4 / adversarial F6 surfaced a second divergence: `CLAUDE.md:32` lists `flui-build` as Active under Tools while `Cargo.toml:50` has it commented out. Both directions get fixed in R4 so `CLAUDE.md` and `Cargo.toml` agree.

- **Four atomic commits, one per finding** (R1 → R2 → R3 → R4 in order). Atomic shape gives per-finding revert boundaries.

- **Land commits on the current worktree branch `naughty-jackson-324931`.** No new branch needed; the worktree already isolates the cleanup work from main.

## Dependencies / Assumptions

- **Production dirty-marking path is `PipelineOwner::add_node_needing_layout / add_node_needing_paint`**, not `AtomicRenderFlags::set_needs_layout/paint`. Verified by reading [crates/flui-view/src/view/root.rs:192-193](../../crates/flui-view/src/view/root.rs), [crates/flui-view/src/element/behavior.rs:437-438](../../crates/flui-view/src/element/behavior.rs), [crates/flui-hot-reload/src/pipeline.rs:151](../../crates/flui-hot-reload/src/pipeline.rs), and [crates/flui-rendering/src/pipeline/owner.rs](../../crates/flui-rendering/src/pipeline/owner.rs) (`run_layout` / `run_paint` consume `self.dirty.needs_layout/paint` populated only by `add_node_needing_*`, never by AtomicRenderFlags). `collect_nodes_needing_layout` / `collect_nodes_needing_paint` (the matching AtomicRenderFlags-consuming entry points) have zero callers.

- **The `storage/state/` path is a directory**, not a single source file. Verified by `Get-ChildItem` on the worktree: `state/` contains `mod.rs`, `constraints.rs`, `flags.rs`, `geometry.rs`, `offset.rs`, `propagation.rs` (≈20 KB, contains the trait + impl bulk), and `tests.rs` (≈10 KB, contains both propagation and non-propagation tests). Earlier drafting of this brainstorm misstated the layout based on an erroneous round-1 reviewer claim; r3 verifies and corrects.

- **No downstream workspace code imports the deleted symbols.** Verified by grep across `crates/`: `IntrinsicProtocol`, `BaselineProtocol`, `RenderState::mark_needs_*`, `MockTree` — only the defining files and the test mocks colocated with them reference these symbols. `flui-app`, `flui-view`, `flui-hot-reload`, and other workspace crates do not depend on them.

- **`RenderDirtyPropagation` is reachable today only via `flui_rendering::storage::state::RenderDirtyPropagation`** because `storage/mod.rs:66` declares `mod state;` (private). The trait's `pub` keyword therefore already does not expose it externally — the `pub(crate)` downgrade in R3 is a defensive hygiene tightening, not an active API retraction. R8's grep on path-qualified imports confirms no external consumer exists.

- **External dynamically-loaded plugins (`flui-hot-reload` consumers using the `app-plugin` feature) built against the prior crate revision must rebuild.** The `app-plugin` feature pulls `flui-rendering` + `flui-view`; pre-built `.dll/.so/.dylib` artifacts compiled against the prior symbol set need a rebuild. In-tree linkage of the `app-plugin` feature is exercised by `cargo build --workspace` only if the workspace default feature set enables it — verify at edit time by running `cargo build -p flui-hot-reload --features app-plugin --all-targets` alongside R5. External rebuild is a known one-off cost; R4's `CLAUDE.md` update may optionally note this.

- **`cargo test --workspace` is the safety net for missed references.** No `[dev-dependencies]` consumers depend on the deleted symbols; tests catch any oversight at workspace build time.

## Outstanding Questions

None blocking. All product decisions resolved across rounds 1 and 2 of ce-doc-review.

### Deferred to Planning

- [Affects R3][Technical] Should the `pub(crate)` downgrade ship together with explicit removal of the `state/mod.rs` re-export, or keep the re-export at `pub(crate)` visibility? Planning verifies whether any in-crate caller benefits from the `crate::storage::state::RenderDirtyPropagation` import path vs `crate::storage::state::propagation::RenderDirtyPropagation`. Default: narrow the re-export to `pub(crate)`; remove only if `cargo clippy` flags it as unused.

- [Affects R3][Needs research] Should `cargo build -p flui-hot-reload --features app-plugin --all-targets` be added to the per-commit verification list to catch ABI-shape regressions on the widget-pipeline path? Today it is implicit; planning may want to make it explicit in the verification script.
