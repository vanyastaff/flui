---
date: 2026-05-20
topic: flui-layer-mythos-redesign
origin: docs/designs/2026-05-20-mythos-flui-layer-redesign.md
---

# flui-layer Mythos Redesign — Requirements

## Summary

Apply the Mythos architectural lens + the full 14-step refactor methodology established by the `flui-rendering` chain (PR #77 on `main`, merged commit `03774584`) to `crates/flui-layer/`. The crate is ~12,202 LOC and sits between `flui-rendering` (which emits a `Scene`) and `flui-engine` (which renders it to GPU via wgpu). Phase 1 investigation surfaced one Mythos refusal-trigger violation (`Arc<Mutex<Vec<Box<dyn Fn() + Send + Sync>>>>` for composition callbacks), 39 unjustified `unsafe impl Send + Sync` blocks, three god modules totalling 3,710 LOC (`tree/layer_tree.rs` 1660, `layer/mod.rs` 1075, `compositor.rs` 975), 467 LOC of `LayerHandle<T>` with zero external callers, one 0-impl dead trait (`HasCompositionCallbacks`), a 5-method `LayerTree::push_*` API that duplicates `SceneBuilder::push_*`, and an unused `parallel = ["rayon"]` feature flag. The design verdict at `docs/designs/2026-05-20-mythos-flui-layer-redesign.md` resolves these into a 14-step implementation plan; this brainstorm encodes the user-story / requirements layer that drives that plan.

---

## Problem Frame

The merge of PR #77 landed the Flutter port methodology as three artifacts (the refactored `flui-rendering` exemplar, the per-crate `ARCHITECTURE.md` template, the top-level `docs/PORT.md`). Today the methodology covers `flui-foundation` (grafted) and `flui-rendering` (templated 2026-05-20). The next crate that earns the same treatment is `flui-layer`, because it (a) sits on the critical hot path between rendering and GPU, (b) has the shape that motivated Mythos in the first place (god modules, dead surface, cargo-cult unsafe, copy-pasted Dart class hierarchy), and (c) is the inbound consumer for `flui-engine`'s wgpu backend — friction here ripples directly into engine performance.

Without the Mythos pass, the crate carries:
- 39 `unsafe impl Send + Sync` blocks (each one a soundness-comment maintenance burden with no actual unsafe operation behind it; one block even has the comment "contains only owned, Send types" directly above the unsafe);
- A `LayerHandle<T>` (467 LOC, 17 type aliases, `Arc<AtomicUsize>` ref-count) that no external caller in the workspace instantiates or reads;
- A `CompositionCallbackRegistry` whose `Arc<Mutex<Vec<(Id, Box<dyn Fn() + Send + Sync>)>>>` storage is shaped for cross-thread sharing nobody uses, and whose `HasCompositionCallbacks` trait has zero impls;
- A `LayerTree` documented to be wrapped in `Arc<RwLock<LayerTree>>` for "multi-threaded access" — directly contradicting the strategy clause "sync hot path, async at edges" and the Mythos refusal trigger 1 (the storage type's doc-comment instructing users to wrap it in the forbidden shape);
- A `LayerTree::push_clip_rect`/`push_clip_rrect`/`push_clip_path`/`push_transform`/`push_opacity` API (5 methods) that duplicates the same 5 methods on `SceneBuilder` — two implementations, two test suites, one canonical pattern;
- 57 `is_*` / `as_*` / `as_*_mut` methods on the `Layer` enum (19 variants × 3) of pattern-match boilerplate;
- An unused `rayon` dependency behind a `parallel` feature flag with no `rayon::*` call sites in any source file.

The shape is exactly what Mythos was designed to catch: ports of Dart classes into Rust without paying attention to what Rust gives you for free (auto-derived `Send`/`Sync`, closed enum dispatch, `Drop`, `Option<T>`, `&mut self` single-writer enforcement).

The cost shape is recurring: every new feature on top of `flui-layer` (e.g. when `flui-animation` re-enables and starts pushing more layer types, when `flui-devtools` re-enables and wants to inspect the layer tree) will inherit and possibly extend the same maintenance debt unless cleaned first. The Mythos pass front-loads the cleanup.

---

## Actors

- **A1. Solo maintainer (`vanyastaff`)** — runs the Mythos refactor by hand following the 14-step plan; primary author of the resulting commit chain and PR. Mythos rules are non-negotiable per `~/.claude/projects/.../memory/no-quick-wins-vanyastaff.md`: "execute full migration including breaking ripples." Maintainer is the consumer of the resulting `crates/flui-layer/ARCHITECTURE.md` template instance.

- **A2. Implementation agent (Claude Code / `/aif-implement` / `implement-coordinator`)** — consumes the resulting `crates/flui-layer/ARCHITECTURE.md` `## Outstanding refactors` section when picking up follow-up work (e.g. `SmallVec<[LayerId; 4]>` for `LayerNode::children`, property tests, miri gate). Not a primary author of the Mythos pass itself; downstream reader.

- **A3. Downstream crates as consumers** — `flui-rendering` (paint phase), `flui-engine` (wgpu backend), `flui-app` (frame loop), `flui-hot-reload` (scene preservation). Each must continue to compile and behave identically after the refactor. The `Scene::dispose(self)` → `drop(scene)` ripple lands in `flui-app::direct.rs`, `flui-hot-reload::driver.rs`, and `flui-hot-reload::pipeline.rs`. The `SceneBuilder::pop` panic → `Result` ripple lands in `flui-rendering::context/canvas.rs`. No public-API consumer of `LayerHandle<T>` or `CompositionCallbackRegistry` exists outside `flui-layer` itself, so those ripples are zero.

---

## Key Flows

- **F1. Author the Mythos design verdict for `flui-layer`**
  - **Trigger:** the next crate in line after `flui-rendering` needs the Mythos lens applied.
  - **Actors:** A1.
  - **Steps:** Investigate the current shape (Phase 1) → identify refusal-trigger violations and dead surface → write the 13-section design verdict at `docs/designs/<date>-mythos-flui-layer-redesign.md` matching the `flui-rendering` template → publish.
  - **Outcome:** A reviewable design verdict exists that the implementation chain can be sourced from. The verdict is the source of truth for the rest of the chain.
  - **Covered by:** R1, R2, R3.

- **F2. Execute the 14-step Mythos refactor chain**
  - **Trigger:** the verdict is published and the implementation plan is approved.
  - **Actors:** A1; agent A2 may pick up individual Outstanding refactors after the chain.
  - **Steps:** Branch off `main` → execute each Mythos step as a commit → after each step, `cargo check --workspace`, `cargo test -p flui-layer --lib`, `bash scripts/port-check.sh` (extended) all green or no commit → land breaking ripples in `flui-rendering`/`flui-engine`/`flui-app`/`flui-hot-reload` in-band per the no-quick-wins memo.
  - **Outcome:** All 14 steps committed, all gates green, the PR is mergeable into `main` without remaining Mythos-blocked violations except those explicitly logged as concrete-blocker-with-named-dependency in `## Outstanding refactors`.
  - **Covered by:** R4, R5, R6, R7, R8, R9, R10, R11, R12, R13, R14.

- **F3. Extend `scripts/port-check.sh` to cover `crates/flui-layer/src/`**
  - **Trigger:** the refactor lands and the methodology should now refuse the same patterns on next introduction in `flui-layer`.
  - **Actors:** A1.
  - **Steps:** Add `crates/flui-layer/src/` to the relevant trigger globs (Trigger 1: `RwLock<Box<dyn>>` on storage-shaped types; Trigger 2: `Box<dyn>` wrapped in interior-mutability; Trigger 3: `async fn` on layer methods; Trigger 5: `Arc::clone` in per-frame layer walk inside `flui-engine`) → run `bash scripts/port-check.sh -v` and verify all triggers stay clean post-refactor.
  - **Outcome:** Future re-introductions of any of the six refusal-trigger patterns inside `flui-layer` or in the engine's layer-walk are caught at port-check time, not at next-quarter cleanup time.
  - **Covered by:** R15, R16.

- **F4. Templated `ARCHITECTURE.md` for `flui-layer`**
  - **Trigger:** the chain lands and the methodology requires a per-crate template instance for the touched crate.
  - **Actors:** A1.
  - **Steps:** Create `crates/flui-layer/ARCHITECTURE.md` following the five-section template in `docs/PORT.md` → graft `## Flutter source mapping` (Flutter `layer.dart` class → FLUI file table), `## Mapping decisions` (closed `Layer` enum vs `Box<dyn>`, `Vec<CompositionCallback>` vs `Arc<Mutex<>>`, single-owner `LayerTree` vs `Arc<RwLock<>>`, `LayerHandle<T>` deletion), `## Thread safety` (post-refactor: no locks anywhere in production code; auto-derived Send/Sync everywhere), `## Friction log` (anything not yet refactored), `## Outstanding refactors` (SmallVec for children, property/miri tests, future GPU lifecycle hooks if needed) → update `docs/PORT.md` `## Index` to flip `flui-layer` from "Not yet templated" to "Templated 2026-05-20".
  - **Outcome:** The per-crate template instance for `flui-layer` exists; the methodology's coverage advances by one active crate.
  - **Covered by:** R17, R18.

---

## Requirements

### Design verdict authorship

- **R1.** The design verdict at `docs/designs/2026-05-20-mythos-flui-layer-redesign.md` follows the 13-section structure established by `docs/designs/2026-05-20-mythos-flui-rendering-redesign.md`: Problem Definition, Architecture Overview, Core Types, State Machine, Public API, Internal Modules, Async/Failure Semantics, Security Model, Data-Oriented Notes, Error Model, Tests Required, Rejected Designs, Implementation Plan. Sections that fold to "N/A" for `flui-layer` (e.g. no phase typestate state machine) are present with a one-sentence justification, not omitted.

- **R2.** The verdict names the main state owner (`Scene` owns `LayerTree` owns `Slab<LayerNode>`), the main trust boundary (`Layer` is a closed concrete enum, not a `Box<dyn Layer>` plugin trait — deliberately the opposite of `RenderObject<P>`), the main async risk (zero — no `async fn` anywhere), and the main simplification principle (every indirection must justify its presence in writing).

- **R3.** Rejected designs in §12 of the verdict cover at least eight alternatives explicitly considered and discarded: `Box<dyn Layer>` plugin trait, `Arc<RwLock<LayerTree>>` shared tree, `LayerHandle<T>` retained for future GPU lifecycle, `CompositionCallbackRegistry` as shared `Arc<Mutex<>>`, `enum_dispatch` crate for the `is_*`/`as_*` boilerplate, `Sync` on `Scene` via `Arc<Scene>`, retained `unsafe impl Send + Sync` blocks, retained `tracing` dep with no emission. The rejection of each names the temptation and the concrete reason it is wrong for FLUI.

### Refactor scope — dead surface deletion

- **R4.** `crates/flui-layer/src/handle.rs` (467 LOC, `LayerHandle<T>` + 17 type aliases + `AnyLayerHandle`) is deleted. Verified by `grep -r "LayerHandle\|AnyLayerHandle" crates/` returning zero matches outside `crates/flui-layer/src/handle.rs` (the deletion target itself) at the start of the chain, then zero matches anywhere after the chain.

- **R5.** `crates/flui-layer/src/layer/composition_callback.rs` is deleted (358 LOC: `CompositionCallbackRegistry`, `CompositionCallbackHandle`, `CompositionCallbackId`, `HasCompositionCallbacks`, `CallbackStorage`). The composition-callback storage is folded into `Scene` as a plain `Vec<CompositionCallback>` field where `CompositionCallback(Box<dyn FnOnce() + Send + 'static>)`. `Scene::add_composition_callback` and `Scene::fire_composition_callbacks(&mut self) -> Vec<LayerError>` provide the new surface. `HasCompositionCallbacks` trait (0 impls) is not re-exported.

- **R6.** The 39 `unsafe impl Send + Sync` blocks across 18 layer files (`layer/*.rs`) + `scene.rs` + `handle.rs` are deleted. Auto-derivation suffices because every layer's fields are `Send + Sync` already (verified by `cargo build --workspace` post-deletion). If any compile error surfaces, the offending field is identified and either changed or its layer is documented in the `## Friction log`.

### Refactor scope — duplicate API deletion

- **R7.** `LayerTree::push_clip_rect`, `push_clip_rrect`, `push_clip_path`, `push_transform`, `push_opacity` (5 methods, ~120 LOC of impl + ~720 LOC of tests in `tree/layer_tree.rs`) are deleted. The canonical scene-construction API is `SceneBuilder::push_*` (30 methods). Test scenarios duplicated by `SceneBuilder` tests are deleted; non-duplicate tests (if any) migrate to `tests/scene_builder.rs`.

- **R8.** `LayerNode::get_layer` / `get_layer_mut` (duplicate accessors next to `layer` / `layer_mut`) are deleted. Callers use the canonical `layer` / `layer_mut`.

- **R9.** `Scene::dispose(self)` (which calls `drop(self)`) is deleted. Callers (`flui-app::direct`, `flui-hot-reload::*`) migrate to `drop(scene)` or `let _ = scene;`. Breaking ripple lands in the same chain per the no-quick-wins rule.

### Refactor scope — god module splits

- **R10.** `crates/flui-layer/src/tree/layer_tree.rs` (1660 LOC) splits into:
  - `tree/layer_tree.rs` retained at ~250 LOC of production code (LayerNode struct + LayerTree struct + core methods).
  - `tests/layer_tree.rs` (integration test crate) holds the extracted ~720 LOC test suite. Test names are preserved so the pre/post diff is reviewable as a move, not a rewrite.
  - The deleted 5 `push_*` helpers (R7) account for ~120 LOC + their tests.

- **R11.** `crates/flui-layer/src/layer/mod.rs` (1075 LOC) splits into:
  - `layer/mod.rs` retained at ~300 LOC with `Layer` enum + `bounds()` + `needs_compositing()` + `is_opaque()` (the three semantic methods the engine consumes).
  - `layer/bounds.rs` for the `LayerBounds` trait (extracted from line 916).
  - `layer/dispatch.rs` for the `is_*` / `as_*` / `as_*_mut` boilerplate, generated by a `macro_rules!` or `paste!`-based macro. The external surface stays identical so callers do not change.

- **R12.** `crates/flui-layer/src/compositor.rs` (975 LOC) splits into:
  - `compositor/mod.rs` (~20 LOC) — re-exports.
  - `compositor/builder.rs` (~600 LOC) — `SceneBuilder<'a>` and its `push_*` / `add_*` / `pop` / `build` methods.
  - `compositor/retained.rs` (~150 LOC) — `SceneCompositor` and `CompositorStats` (the retained-layer manager, structurally separate from `SceneBuilder`).
  - `tests/scene_builder.rs` (integration test crate) holds the extracted ~300 LOC test suite.

### Refactor scope — error model

- **R13.** `crates/flui-layer/src/error.rs` is added with `LayerError` (variants: `UnknownLayerId`, `BuilderStackUnderflow`, `OrphanedLeader`, `OrphanedFollower`, `CallbackPoisoned`) and `LayerResult<T> = Result<T, LayerError>`. The variants are `thiserror::Error`-derived.

- **R14.** `SceneBuilder::pop` panic is replaced by `Result<LayerId, LayerError::BuilderStackUnderflow>`. `try_pop` is retained as the panic-free option that returns `Option<LayerId>`. `Scene::fire_composition_callbacks` wraps each callback in `std::panic::catch_unwind(AssertUnwindSafe(|| ...))` and returns `Vec<LayerError>` for the poisoned ones; subsequent callbacks still fire (matching `flui-rendering` Step 12 `Poisoned` shape, commit `dc0fa1ad`).

### Methodology extension

- **R15.** `scripts/port-check.sh` is extended:
  - Trigger 1 (`RwLock<Box<dyn ...>>`) adds `crates/flui-layer/src/` to its path scope.
  - Trigger 2 (`Box<dyn>` wrapped in interior-mutability) adds `crates/flui-layer/src/` to its path scope.
  - Trigger 3 (`async fn` on `build|layout|paint|perform_layout`) adds `crates/flui-layer/src/` to its path scope and extends the verb set to include `composite|render|fire_composition_callbacks` so layer-level async violations are caught.
  - Trigger 5 (`Arc::clone` in per-frame paint loop) extends its scope to `crates/flui-engine/src/wgpu/layer_render.rs` (the per-frame layer walk inside the GPU backend) as a forward-looking guard.

- **R16.** After the refactor chain lands, `bash scripts/port-check.sh -v` exits 0 and reports each trigger as "ok". Any violation that cannot be resolved at chain time is documented in `crates/flui-layer/ARCHITECTURE.md` `## Outstanding refactors` with concrete-blocker language (named external dependency, not "would touch X").

### Per-crate `ARCHITECTURE.md` instance

- **R17.** `crates/flui-layer/ARCHITECTURE.md` is created following the `docs/PORT.md` template specification (five fixed sections: `## Flutter source mapping`, `## Mapping decisions`, `## Thread safety`, `## Friction log`, `## Outstanding refactors`). Optional sections may be added (e.g. `## Exception ledger` if accepted trade-offs accumulate).

- **R18.** `docs/PORT.md` `## Index` table flips `flui-layer` from "Not yet templated" to "Templated 2026-05-20" in the same commit that ships `crates/flui-layer/ARCHITECTURE.md`.

### Mythos rules (non-negotiable, sourced from no-quick-wins memo)

- **R19.** Breaking ripples in adjacent crates (`flui-rendering`, `flui-engine`, `flui-app`, `flui-hot-reload`) are executed in-band, not deferred. The only legitimate deferrals are concrete-blocker-with-named-dependency: external dependency needed (proptest dev-dep, loom dev-dep, miri CI infra, derive-macro feature) explicitly named in `## Outstanding refactors`. "Mechanical busywork" and "would touch flui-app" are NOT legitimate deferrals.

- **R20.** No new `unsafe` block is introduced unless it is a deletion-flavoured primitive with a local safety invariant and a unit test. The Mythos chain for `flui-layer` is expected to **net-delete** 39 `unsafe impl Send + Sync` blocks and add zero new ones, because no disjoint-borrow primitive on `LayerTree` is required (single-owner during build, frozen during render).

- **R21.** The hot path (`SceneBuilder::push_*`, `Layer::bounds`, `Layer::needs_compositing`, `Layer::is_opaque`, `flui-engine`'s layer walk) remains synchronous after the chain. No `async fn` may be introduced on any layer method or scene-construction API.

---

## Acceptance Examples

- **AE1. Covers R4.** Given the `LayerHandle<T>` API has zero external callers in the workspace at the start of the chain (verified by `grep -r "use flui_layer::.*Handle" crates/`), when the Mythos chain lands, then `crates/flui-layer/src/handle.rs` does not exist, `LayerHandle` and the 17 type aliases are not re-exported from `lib.rs`, the prelude does not mention them, and a fresh `grep -r "LayerHandle" crates/` returns zero matches.

- **AE2. Covers R5, R13, R14.** Given `flui-app::binding.rs` and `flui-rendering::context/canvas.rs` may register composition callbacks today (verified by `grep -r "CompositionCallback" crates/`), when the Mythos chain lands, then `composition_callback.rs` does not exist, `Scene::add_composition_callback(FnOnce() + Send + 'static)` is the only registration surface, `Scene::fire_composition_callbacks(&mut self) -> Vec<LayerError>` is the only fire surface, and a callback that panics returns `Err(LayerError::CallbackPoisoned)` for that callback while subsequent callbacks still fire (test fixture: 3 callbacks where the middle one panics, all 3 invoked, returned vec contains exactly one `CallbackPoisoned` entry).

- **AE3. Covers R6, R20.** Given `crates/flui-layer/src/` contains 39 `unsafe impl Send + Sync` blocks at the start of the chain (verified by `rg "^unsafe impl.*\b(Send|Sync)\b" crates/flui-layer/src/ -c`), when the Mythos chain lands, then `rg "^unsafe impl" crates/flui-layer/src/` returns zero matches, `cargo build --workspace` is clean, and `cargo test -p flui-layer` passes. The net-unsafe delta for `flui-layer` is **−39**.

- **AE4. Covers R7, R10.** Given `LayerTree::push_clip_rect`, `push_clip_rrect`, `push_clip_path`, `push_transform`, `push_opacity` exist today (verified by `grep -n "pub fn push_" crates/flui-layer/src/tree/layer_tree.rs`), when the Mythos chain lands, then those 5 methods do not exist on `LayerTree`, callers that used them have migrated to `SceneBuilder::push_*` (zero callers existed in production code at the start, verified by repo grep), and `crates/flui-layer/src/tree/layer_tree.rs` is ≤ 300 LOC of production code (down from 1660 LOC), with the test suite extracted to `tests/layer_tree.rs`.

- **AE5. Covers R11.** Given `layer/mod.rs` is 1075 LOC with 600 LOC of `is_*` / `as_*` / `as_*_mut` boilerplate today, when the Mythos chain lands, then `layer/mod.rs` is ≤ 300 LOC, `layer/dispatch.rs` holds the macro-generated boilerplate, `layer/bounds.rs` holds the `LayerBounds` trait, and the external surface (e.g. `layer.is_clip_rect()`, `layer.as_canvas_mut()`) compiles unchanged for all callers.

- **AE6. Covers R12.** Given `compositor.rs` is 975 LOC mixing `SceneBuilder<'a>` and `SceneCompositor` (different concerns), when the Mythos chain lands, then `compositor/builder.rs` holds `SceneBuilder<'a>`, `compositor/retained.rs` holds `SceneCompositor + CompositorStats`, `compositor/mod.rs` is a thin re-export, and the `crate::SceneBuilder` / `crate::SceneCompositor` external paths still work.

- **AE7. Covers R15, R16.** Given `scripts/port-check.sh` currently scopes Trigger 1, 2 to `crates/flui-rendering/src` + `crates/flui-view/src` and Trigger 3 to `flui-rendering/src` + `flui-view/src` + `flui-painting/src` only, when the Mythos chain lands, then the script's six trigger blocks each include `crates/flui-layer/src` (Triggers 1, 2, 3) or `crates/flui-engine/src/wgpu/layer_render.rs` (Trigger 5), and `bash scripts/port-check.sh -v` reports "ok" for all six triggers post-refactor.

- **AE8. Covers R17, R18.** Given `docs/PORT.md` `## Index` lists `flui-layer` as "Not yet templated" today, when the Mythos chain lands, then `crates/flui-layer/ARCHITECTURE.md` exists with the five fixed template sections populated, `docs/PORT.md` `## Index` lists `flui-layer` as "Templated 2026-05-20", and the `## Mapping decisions` section includes "Accepted trade-offs" entries for: closed `Layer` enum (vs `Box<dyn Layer>`), `Vec<CompositionCallback>` on Scene (vs `Arc<Mutex<Registry>>`), single-owner `LayerTree` (vs `Arc<RwLock<>>`), `LayerHandle<T>` deletion (vs retain for future).

- **AE9. Covers R19 (Mythos rules).** Given the chain renames `Scene::dispose` and forces callers in `flui-app::direct.rs`, `flui-hot-reload::driver.rs`, `flui-hot-reload::pipeline.rs` to migrate to `drop(scene)`, when the chain lands, then those caller-side updates are commits inside the same PR (not a follow-up), and no "TODO: migrate callers of `Scene::dispose`" comment exists anywhere.

---

## Success Criteria

- The Mythos refactor chain merges as a feature branch off `main` in a single PR with 14 reviewable commits, each commit passing `cargo check --workspace`, `cargo test -p flui-layer --lib`, and `bash scripts/port-check.sh` (extended). No commit lands with broken tests or red CI.

- Net unsafe delta for `flui-layer`: **−39**. Net LOC delta for the touched .rs files: targeted reduction of ≥ 3,000 LOC (from the three god modules + handle.rs + composition_callback.rs deletions; some LOC moves to integration test files rather than disappears).

- `crates/flui-layer/ARCHITECTURE.md` exists and matches the template. `docs/PORT.md` `## Index` shows `flui-layer` as "Templated 2026-05-20".

- `scripts/port-check.sh` covers `crates/flui-layer/src/` (Triggers 1, 2, 3) and `crates/flui-engine/src/wgpu/layer_render.rs` (Trigger 5). Running `bash scripts/port-check.sh -v` exits 0 and prints six "ok" lines.

- The PR description follows the shape of PR #77 (Track A / Track B sections if applicable, key decisions, testing summary, quick-wins-track callouts listing any temptations the maintainer caught and rejected during the chain).

- A2 (a downstream agent) can pick up any entry from `crates/flui-layer/ARCHITECTURE.md` `## Outstanding refactors` and produce a follow-up PR without a fresh brainstorm or out-of-band clarification.

---

## Scope Boundaries

- **Out of scope: `SmallVec<[LayerId; 4]>` for `LayerNode::children`.** The verdict mentions this as a post-Mythos optimisation; it is recorded as an Outstanding refactor but not landed in this chain because it requires a small-vec dependency decision separate from the Mythos pass.

- **Out of scope: property tests, miri gate, loom tests for `flui-layer`.** Same shape as `flui-rendering`'s carry-over (the rendering chain deferred these to Outstanding refactors because they require `proptest`/`loom` dev-deps and CI infra changes that exceed the chain's scope).

- **Out of scope: Flutter retained-layer optimisation full implementation.** Today's `SceneCompositor::retained` is a stub with 5 callers, none of which are production code. The Mythos chain extracts it to its own file but does not implement the retained-layer scene-construction shortcut.

- **Out of scope: `flui-engine`'s wgpu backend internal cleanup.** The chain only touches `crates/flui-engine/src/wgpu/layer_render.rs` if Trigger 5's port-check extension flags `Arc::clone` in the layer walk (which the Phase 1 investigation confirmed it does not today — the check is forward-looking).

- **Out of scope: Re-enabling `flui-animation`, `flui-devtools`, `flui-cli`.** Disabled crates are not in the chain's blast radius. They may inherit Mythos-clean shapes when re-enabled in future chains.

- **Out of scope: Cross-crate dependency-graph audit.** This chain only touches the consumers of `flui-layer`'s public API where R19 (in-band breaking ripples) requires it. A workspace-wide audit of `Arc<RwLock<>>` sites in non-`flui-layer` crates is a separate brainstorm.

- **Out of scope: Documenting per-variant GPU lowering in `flui-engine`.** The verdict notes that every `Layer` variant must have a documented wgpu translation; producing that documentation is the engine crate's responsibility and is not in this chain.

- **Out of scope: Building a third-party `Box<dyn Layer>` plugin boundary.** The verdict explicitly rejects this as Rejected Design #1; the chain enforces the closed-enum shape.

---

## Key Decisions

- **Closed `Layer` enum over `Box<dyn Layer>` plugin trait.** The GPU backend cannot lower arbitrary user layers to wgpu draw calls; every variant is a coordinated change. The closed enum gives exhaustive-match compile-time checks; the trait object loses that. This is deliberately the opposite shape from `RenderObject<P>` (which IS a plugin boundary).

- **`Vec<CompositionCallback>` on `Scene` over `Arc<Mutex<Registry>>`.** Today's registry has zero cross-thread consumers; the lock and the `Arc` are pure ceremony. `FnOnce` (not `Fn`) matches the one-shot fire semantics. The fold-in deletes 358 LOC for a ~30 LOC replacement on `Scene`.

- **Single-owner `LayerTree` + `Send` `Scene` value-move over `Arc<RwLock<LayerTree>>`.** The doc-comment recommending the locked shape is incorrect; cross-thread layer construction happens by building a subtree on a worker thread and emitting it as a value the render thread receives.

- **Delete `LayerHandle<T>` over retain-for-future.** No external caller reads `ref_count`. The 467 LOC + 17 type aliases is dead weight. If GPU resource lifecycle management becomes a real need, the right shape is on `flui-engine`'s resource registry, not a wrapper in `flui-layer`. Rebuilding from scratch will be ~50 LOC; the current 467 LOC is hostile to that rebuild.

- **Macro-collapse `is_*`/`as_*`/`as_*_mut` over `enum_dispatch` dep.** Hand-written `macro_rules!` in `layer/dispatch.rs` avoids a new proc-macro dep for a small win. Output is identical to what `enum_dispatch` would generate. External callers cannot tell the difference.

- **Delete the `unsafe impl Send + Sync` blocks (39) over keep-them-just-in-case.** Auto-derivation is correct. The unsafe blocks are cargo-cult from Dart's threading model. If a future layer introduces a `Cell<T>` field, the compile error surfaces immediately at the right level (the offending field, not a copy-pasted unsafe impl).

- **Retain `tracing` dep + add minimal `#[instrument]` spans over delete the dep.** Aligns with `flui-rendering`'s instrumentation convention. Cheap to add now; expensive to retrofit later.

- **Delete `parallel = ["rayon"]` feature + `rayon` dep over retain-for-future.** Feature flag with no implementation is a lie. Rayon brings significant transitive deps. When parallel layer-tree traversal becomes a measured need, re-add with the actual implementation.

- **Delete `Scene::dispose(self)` over keep-for-API-stability.** The method calls `drop(self)`. Rust users know what `drop` does. Callers that read as `scene.dispose()` migrate to `drop(scene)` or `let _ = scene;`. The API stability cost is one breaking change in a chain that already has breaking changes.

- **Replace `SceneBuilder::pop` panic with `Result` over keep panic.** Stack underflow is a programmer error; panicking is "correct" in the sense that nothing else makes sense, but `Result` gives the test surface a way to verify the error path and gives callers a recovery point if they want one. `try_pop` stays for the panic-free path.

- **Land breaking ripples in-band over deferred.** Per the no-quick-wins memo. The chain is 14 steps; ripples land in steps 8 (Scene::dispose), 10 (pop result), and 12 (ARCHITECTURE.md). No "follow-up PR for migrating callers" exists in the plan.

---

## Open Questions

### Resolved during planning

- "Is `Layer` already a concrete enum or `Box<dyn Layer>`?" — resolved by Phase 1 investigation: it is already a concrete `enum Layer { Canvas(...), Picture(...), ... }`. The Mythos pass strengthens this position with `#[non_exhaustive]` and documents it as a closed boundary in `## Mapping decisions`.

- "Does any external caller actually use `LayerHandle<T>`?" — resolved: zero. The handle is internal-only and unused.

- "Does the composition-callback registry have any external impl?" — resolved: zero. `HasCompositionCallbacks` has 0 impls anywhere in the workspace.

- "How many `unsafe impl Send + Sync` blocks need deletion?" — resolved: 39 across `layer/*.rs`, `scene.rs`, `handle.rs`.

- "Are there any genuine `Box<dyn ...Layer>` storage sites in the crate?" — resolved: zero. The only `Box<dyn ...>` storage is `Box<dyn Fn() + Send + Sync>` in the composition-callback registry (folded into `Scene::composition_callbacks: Vec<CompositionCallback>` in R5).

### Deferred to implementation

- **Whether `layer/dispatch.rs` should be hand-`macro_rules!` or use the `paste` crate** — resolution at the macro-write step. The `paste` crate is already an indirect dep via other workspace crates; the choice is cosmetic (identifier-concatenation ergonomics). If `paste` is not available transitively at the chain's step, hand-write the macro.

- **Whether `compositor/retained.rs` should be `pub` from `compositor/mod.rs` or kept `pub(crate)`** — resolution depends on whether `flui-rendering` has any direct dependency on `SceneCompositor::retain`. Phase 1 investigation found no production callers; recommend `pub(crate)` initially and elevate to `pub` if/when an external caller appears.

- **Final structure of the `tests/` directory inside `crates/flui-layer/`** — the test suite extraction lands as `tests/layer_tree.rs` and `tests/scene_builder.rs`; whether `tests/link_registry.rs` is also extracted (it has ~290 LOC of inline tests in `link_registry.rs`) is at the maintainer's discretion. If extracted, it lands in the same chain step (R12 split scope expands).

- **Whether the `parallel = ["rayon"]` feature deletion ripples to any downstream Cargo.toml** — resolution requires `grep -rn "flui-layer" crates/` for feature mentions. Phase 1 investigation found none; recommend deleting the feature and dep in the same step (R13 / Step 11 of the verdict's implementation plan).

- **Whether `SceneBuilder::pop`'s `Result` return type is `LayerResult<LayerId>` or `LayerResult<()>` is preferred** — resolution at the API-design step. The current `pop()` returns `()` and panics on empty; the simplest Result-flavoured shape returns `LayerResult<()>` and stays compatible. If callers need the popped ID, they use `try_pop -> Option<LayerId>`. Recommend `LayerResult<()>` for `pop` and `Option<LayerId>` for `try_pop`.

---

## Related Work

- `docs/designs/2026-05-20-mythos-flui-rendering-redesign.md` — the precedent verdict. The structure, the rejected-designs format, and the 14-step implementation plan template are sourced from this document.
- `docs/plans/2026-05-19-001-feat-flutter-port-methodology-plan.md` — the methodology plan that established `docs/PORT.md`, the per-crate `ARCHITECTURE.md` template, and `scripts/port-check.sh`. Status: completed (merged via PR #77).
- `crates/flui-rendering/ARCHITECTURE.md` — the exemplar per-crate template instance. The `flui-layer/ARCHITECTURE.md` instance R17 produces mirrors its shape.
- `~/.claude/projects/.../memory/no-quick-wins-vanyastaff.md` — the no-deferred-ripples rule. R19 codifies this for the chain.
- Reference commits on `main` (exemplars for the chain steps):
  - `907a7787` — full delete + rewire (analog: Mythos Step 1, `LayerHandle<T>` deletion)
  - `4d05efc5` — god-module split (analog: Mythos Steps 6, 7, 11 — `layer_tree.rs`, `compositor.rs`, `layer/mod.rs` splits)
  - `dc0fa1ad` — `catch_unwind` plumbing (analog: Mythos Step 10, `Scene::fire_composition_callbacks` panic catching)
  - `d0e53c63` — extension-trait split (analog: Mythos Step 11 macro form for `is_*`/`as_*` dispatch)
