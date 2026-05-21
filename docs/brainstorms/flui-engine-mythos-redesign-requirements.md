---
date: 2026-05-20
topic: flui-engine-mythos-redesign
origin: docs/designs/2026-05-20-mythos-flui-engine-redesign.md
---

# flui-engine Mythos Redesign — Requirements

## Summary

Apply the Mythos architectural lens + the full 14-step refactor methodology established by the `flui-rendering` chain (PR #77 on `main`, merged commit `03774584`) and the `flui-layer` chain (PR #78 on `main`, merged commit `a78cdd69`) to `crates/flui-engine/` (~25,119 LOC). The crate is the wgpu rendering backend that consumes `Scene` from `flui-layer` and submits draw calls to the GPU. Phase 1 investigation surfaced ≥ 6,000 LOC of dead modules (a parallel scene graph in `wgpu/scene.rs`, a dead `Compositor`/`TransformStack` in `wgpu/compositor.rs`, three platform-capability files for vulkan/dx12/metal with zero callers, `ExternalTextureRegistry` / `TextureCache` / `PathCache` / `MultiDrawBatcher` / `VectorTextRenderer` infrastructure with zero callers), a fake `Painter` trait (380 LOC, 1 production impl, 6 default impls printing `tracing::warn!("not implemented")`), one `Arc<parking_lot::Mutex<OffscreenRenderer>>` shared across `Renderer` and `Backend` with single-mutator data, one `Arc<Mutex<TexturePoolInner>>` for the release-on-drop pattern that can be made explicit, several per-frame `Arc::clone(&device)` / `Arc::clone(&queue)` calls in the hot path, an `anyhow::Result` return type in `Renderer::new` / `new_offscreen` that should migrate to `RenderResult`, and a 3,772 LOC god module (`wgpu/painter.rs`) with mixed batch / layer / gradient / text concerns.

The design verdict at `docs/designs/2026-05-20-mythos-flui-engine-redesign.md` resolves these into a 14-step implementation plan; this brainstorm encodes the user-story / requirements layer that drives that plan.

---

## Problem Frame

Two Mythos chains have merged into `main` (the `flui-rendering` redesign at commit `03774584` and the `flui-layer` redesign at commit `a78cdd69`). Today the methodology covers three crates: `flui-foundation` (grafted), `flui-rendering` (templated 2026-05-20 with `PipelineOwner<Phase>` typestate + structured error model), and `flui-layer` (templated 2026-05-20 with closed `Layer` enum + `Vec<CompositionCallback>` fold-in + 39 unsafe deletions). The next crate that earns the same treatment is `flui-engine`, because it (a) consumes `Scene` from `flui-layer` and is the inbound GPU lowering boundary, (b) carries the highest dead-code-to-live-code ratio in the workspace (six dead modules, ~6,000 LOC, plus a global `#![allow(dead_code)]` at the crate root), (c) has not previously been audited and accumulated three years of "I'll wire this up later" decisions that never connected, and (d) sits on the critical hot path — friction here ripples into every frame's GPU work.

Without the Mythos pass, the crate carries:

- A 1,820 LOC `wgpu/scene.rs` that defines its own `Scene`, `SceneBuilder`, `Layer`, `Primitive`, `LayerBatch`, `PrimitiveBatch`, `PrimitiveType`, `BlendMode` — all re-exported from `wgpu::mod` and the crate root, **colliding by name with `flui_layer::Scene` and `flui_layer::SceneBuilder` which are also re-exported from `flui-engine::lib.rs`**. Two `Scene` types from one crate's public API.
- A 365 LOC `wgpu/compositor.rs` that defines `Compositor` / `TransformStack` / `RenderContext` over the dead `LayerBatch` type, duplicating the save/restore stack that `WgpuPainter` already maintains.
- Three platform-capability files (`wgpu/vulkan.rs` 826 LOC, `wgpu/dx12.rs` 769 LOC, `wgpu/metal.rs` 587 LOC) that define `VulkanFeatures`, `Dx12Features`, `MetalFxUpscaler`, `EdrConfig`, `PipelineCacheConfig`, etc. — all of which wgpu already provides via `Adapter::get_info()` / `features()` / `limits()`. **Zero external callers** outside their own doc-comments.
- 1,651 LOC of unused caches and registries: `wgpu/external_texture_registry.rs` (315 LOC), `wgpu/texture_cache.rs` (1,000 LOC — distinct from the actually-used `wgpu/texture_pool.rs`), `wgpu/path_cache.rs` (336 LOC). Zero external callers.
- 304 LOC `wgpu/multi_draw.rs` with a `DrawCommand` type that **name-collides with `flui_painting::DrawCommand`**. Zero external callers.
- 6 LOC `wgpu/commands.rs` re-export shim that only exists so `wgpu/layer_render.rs` could write `super::commands::CommandRenderer` instead of `crate::traits::CommandRenderer`.
- 802 LOC `utils/text.rs` (`VectorTextRenderer`) — a ttf-parser + lyon vector text experiment with 30+ test functions and zero production callers.
- `pub trait Painter` (~380 LOC in `traits.rs`) with 30+ methods, 6 default impls printing `tracing::warn!("Painter::draw_path: not implemented")`, **1 production impl** (`WgpuPainter`). The trait's docstring promises "multiple backends without changing the high-level rendering code"; no second backend exists or is planned in any document.
- `Arc<parking_lot::Mutex<OffscreenRenderer>>` in `Renderer.offscreen` and `Backend.offscreen`, accessed via `offscreen.lock()` in `Backend::render_shader_mask` and `Renderer::handle_backdrop_filter`. The lock guards data that has exactly one mutator (the render thread).
- `Arc<Mutex<TexturePoolInner>>` in `TexturePool.pool` + `PooledTexture.inner` for the release-on-drop pattern.
- Per-frame `Arc::clone(&device)` / `Arc::clone(&queue)` calls in `Renderer::render_scene` (`renderer.rs:636-637` building `RenderContext`), `Backend::render_shader_mask` (`backend.rs:408-409` after offscreen lock acquisition), and `Backend::get_or_create_offscreen_painter` (`backend.rs:121-122`). All on the hot path.
- `anyhow::Result<Self>` return type on `Renderer::new` and `Renderer::new_offscreen` — inconsistent with the rest of the public API which uses `RenderResult<T>`.
- A 3,772 LOC god module `wgpu/painter.rs` (the largest .rs file in the workspace) mixing `DrawSegment` recording, layer save/restore, gradient generation, text rendering integration, and per-frame submission.
- A 1,525 LOC `wgpu/offscreen.rs` mixing `OffscreenRenderer` with mask, blur, and morphological filter pipelines.
- A 1,199 LOC `wgpu/backend.rs` mixing `Backend` struct, `CommandRenderer` impl, transform decomposition, and offscreen painter caching.
- A 1,191 LOC `wgpu/layer_render.rs` mixing the `LayerRender<R>` trait, the 19-arm enum dispatch, the per-variant impls, and ~521 LOC of `MockRenderer` test fixture.
- `#![allow(dead_code, missing_debug_implementations)]` at `lib.rs:4` — global dead-code suppression.
- Per-module `#[allow(dead_code)]` on `wgpu::effects`, `wgpu::instancing`, `wgpu::pipeline`, `wgpu::shader_compiler`. Either these have legitimate consumers (audit) or items inside them are dead (delete the dead items).
- Two `text` modules (`wgpu/text.rs` 436 LOC and `wgpu/text_renderer.rs` 297 LOC) — possible duplicate, requires investigation.

The shape is what Mythos was designed to catch: a crate that grew by accretion, where every "future feature" left footprint in the form of stubs, traits with one impl, registries with zero callers, and platform-specific files that wgpu already supersedes. The cost shape is recurring: every new feature on top of `flui-engine` (e.g. when `flui-rendering` adds custom render callbacks, when `flui-app` wires the build pipeline) will inherit and possibly extend the same maintenance debt unless cleaned first.

The Mythos pass front-loads the cleanup.

---

## Actors

- **A1. Solo maintainer (`vanyastaff`)** — runs the Mythos refactor by hand following the 14-step plan; primary author of the resulting commit chain and PR. Mythos rules are non-negotiable per `~/.claude/projects/.../memory/no-quick-wins-vanyastaff.md`: "execute full migration including breaking ripples." Maintainer is the consumer of the resulting `crates/flui-engine/ARCHITECTURE.md` template instance.

- **A2. Implementation agent (Claude Code / `/aif-implement` / `implement-coordinator`)** — consumes the resulting `crates/flui-engine/ARCHITECTURE.md` `## Outstanding refactors` section when picking up follow-up work (e.g. `painter.rs` directory split if deferred from this chain, `text_renderer.rs` vs `text.rs` final reconciliation if deferred, `catch_unwind` boundary if real panics emerge). Not a primary author of the Mythos pass itself; downstream reader.

- **A3. Downstream crates as consumers** — `flui-app` (the only direct consumer of `flui-engine::wgpu::Renderer` per grep). The migration from `anyhow::Result` to `RenderResult<T>` on `Renderer::new` / `new_offscreen` ripples into `flui-app/src/app/binding.rs`, `flui-app/src/app/direct.rs`, `flui-app/src/app/runner.rs`. No other crate imports `flui-engine` symbols today.

---

## Key Flows

- **F1. Author the Mythos design verdict for `flui-engine`**
  - **Trigger:** the next crate in line after `flui-rendering` and `flui-layer` needs the Mythos lens applied.
  - **Actors:** A1.
  - **Steps:** Investigate the current shape (Phase 1) → identify dead modules / fake abstractions / lock-or-Arc-clone violations → write the 13-section design verdict at `docs/designs/<date>-mythos-flui-engine-redesign.md` matching the `flui-rendering` and `flui-layer` template → publish.
  - **Outcome:** A reviewable design verdict exists that the implementation chain can be sourced from. The verdict is the source of truth for the rest of the chain.
  - **Covered by:** R1, R2, R3.

- **F2. Execute the 14-step Mythos refactor chain**
  - **Trigger:** the verdict is published and the implementation plan is approved.
  - **Actors:** A1; agent A2 may pick up individual Outstanding refactors after the chain.
  - **Steps:** Branch off `main` → execute each Mythos step as a commit → after each step, `cargo check --workspace`, `cargo test -p flui-engine --lib`, `bash scripts/port-check.sh` (extended) all green or no commit → land breaking ripples in `flui-app` in-band per the no-quick-wins memo.
  - **Outcome:** All 14 steps committed, all gates green, the PR is mergeable into `main` without remaining Mythos-blocked violations except those explicitly logged as concrete-blocker-with-named-dependency in `## Outstanding refactors`.
  - **Covered by:** R4 through R18.

- **F3. Extend `scripts/port-check.sh` to cover `crates/flui-engine/src/`**
  - **Trigger:** the refactor lands and the methodology should now refuse the same patterns on next introduction in `flui-engine`.
  - **Actors:** A1.
  - **Steps:** Add `crates/flui-engine/src/` to Triggers 1, 2, 3 path scopes. Trigger 5 (per-frame `Arc::clone`) was already extended to `flui-engine/src/wgpu/layer_render.rs` by the `flui-layer` chain U13; further extend to `crates/flui-engine/src/wgpu/{renderer.rs, backend.rs}`. Add a new Trigger 7 (forward-looking) for `Arc<(parking_lot::)?(Mutex|RwLock)<wgpu::*>` or for `Arc<(parking_lot::)?(Mutex|RwLock)<*Renderer|*Pool>` shape in `crates/flui-engine/src/wgpu/` to catch regressions of the `Arc<Mutex<OffscreenRenderer>>` / `Arc<Mutex<TexturePoolInner>>` patterns deleted in U6/U8.
  - **Outcome:** Future re-introductions of any of the seven refusal-trigger patterns inside `flui-engine` are caught at port-check time, not at next-quarter cleanup time.
  - **Covered by:** R19, R20.

- **F4. Templated `ARCHITECTURE.md` for `flui-engine`**
  - **Trigger:** the chain lands and the methodology requires a per-crate template instance for the touched crate.
  - **Actors:** A1.
  - **Steps:** Create `crates/flui-engine/ARCHITECTURE.md` following the five-section template in `docs/PORT.md`. For `flui-engine` the `## Flutter source mapping` section is N/A (the engine has no Flutter parity; it's a wgpu-native GPU lowering layer), replaced with a `## wgpu / Vulkan / Metal mapping` section documenting which wgpu APIs each module touches and citing wgpu docs / Vulkan-Metal spec where relevant. → graft `## Mapping decisions` (Accepted trade-offs for: closed `LayerRender<R>` static dispatch vs `Box<dyn Backend>`, direct ownership of `OffscreenRenderer` vs `Arc<Mutex<>>`, explicit `TexturePool::release` vs `Arc<Mutex<>>` drop, `Painter` trait deletion vs retain for future, dead-module deletion vs retain as "internal IR"), `## Thread safety` (post-refactor: zero `Mutex<>` / `RwLock<>` in production code; `Arc<wgpu::Device>` / `Arc<wgpu::Queue>` per wgpu convention; `Renderer: Send` for cross-thread frame production), `## Friction log` (anything not yet refactored — `painter.rs` 3,772 LOC un-split if Step deferred; the `unsafe { instance.create_surface_unsafe(...) }` block at `renderer.rs:189`; `#[allow(missing_debug_implementations)]` necessary because wgpu doesn't impl Debug), `## Outstanding refactors` (painter/ split if deferred, `catch_unwind` boundary, second-backend abstraction if Skia/Vello actually lands, named-resource debug introspection) → update `docs/PORT.md` `## Index` to flip `flui-engine` from "Not yet templated" to "Templated 2026-05-20".
  - **Outcome:** The per-crate template instance for `flui-engine` exists; the methodology's coverage advances by one active crate to a total of four.
  - **Covered by:** R21, R22.

---

## Requirements

### Design verdict authorship

- **R1.** The design verdict at `docs/designs/2026-05-20-mythos-flui-engine-redesign.md` follows the 13-section structure established by the precedent verdicts: Problem Definition, Architecture Overview, Core Types, State Machine, Public API, Internal Modules, Async/Failure Semantics, Security Model, Data-Oriented Notes, Error Model, Tests Required, Rejected Designs, Implementation Plan. Sections that fold to "N/A" for `flui-engine` (e.g. no phase typestate state machine because the engine has only one `Initialized` state and atomic `render_scene` calls) are present with a one-sentence justification, not omitted.

- **R2.** The verdict names the main state owner (`Renderer` owns wgpu::Device/Queue/Surface/Painter/OffscreenRenderer), the main trust boundary (`Layer` enum closed match + `DrawCommand` enum closed match, both via static-dispatch `LayerRender<R>` and `dispatch_command`), the main async risk (zero hot-path; `Renderer::new` async at the wgpu init edge), and the main simplification principle (every module must justify its existence with a production caller, not a re-export, not a doc comment, not "future GPU lifecycle").

- **R3.** Rejected designs in §12 of the verdict cover at least ten alternatives explicitly considered and discarded: `Box<dyn Backend>` plugin trait for multiple backends, `Arc<RwLock<Renderer>>` shared across frame producer + render thread, `wgpu/scene.rs` as "engine's internal IR", `Compositor`/`TransformStack` as "future hooks", platform-capability files as "documentation", `pub trait Painter` for "backend-agnostic high-level APIs", `wgpu/texture_cache.rs` as separate from `wgpu/texture_pool.rs`, `wgpu/multi_draw.rs` for "future indirect-draw batching", `#[allow(dead_code)] pub mod effects` to silence warnings, `enum_dispatch` crate for `LayerRender`, `catch_unwind` around `render_scene` for layer panics. The rejection of each names the temptation and the concrete reason it is wrong for FLUI.

### Refactor scope — dead surface deletion

- **R4.** `crates/flui-engine/src/utils/text.rs` (802 LOC, `VectorTextRenderer`) and `crates/flui-engine/src/utils/mod.rs` (7 LOC) are deleted. The `pub mod utils;` declaration in `lib.rs` is removed. Verified by `grep -r "VectorTextRenderer\|utils::text\|utils/text" crates/` returning zero matches outside the deletion target itself at the start of the chain, then zero matches anywhere after the chain.

- **R5.** `crates/flui-engine/src/wgpu/scene.rs` (1,820 LOC defining `Scene`, `SceneBuilder`, `Layer`, `Primitive`, `LayerBatch`, `PrimitiveBatch`, `PrimitiveType`, `BlendMode`) and `crates/flui-engine/src/wgpu/compositor.rs` (365 LOC defining `Compositor`, `TransformStack`, `RenderContext`) are deleted. The corresponding `mod` declarations and `pub use` re-exports in `wgpu/mod.rs` are removed. Post-deletion, the only `Scene` re-exported from `flui-engine` is `flui_layer::Scene` — no name collision.

- **R6.** The three platform-capability files are deleted: `crates/flui-engine/src/wgpu/vulkan.rs` (826 LOC), `crates/flui-engine/src/wgpu/dx12.rs` (769 LOC), `crates/flui-engine/src/wgpu/metal.rs` (587 LOC). Their `#[cfg(...)] pub mod` declarations in `wgpu/mod.rs` are removed.

- **R7.** Dead infrastructure modules are deleted: `wgpu/external_texture_registry.rs` (315 LOC), `wgpu/texture_cache.rs` (1,000 LOC), `wgpu/path_cache.rs` (336 LOC), `wgpu/multi_draw.rs` (304 LOC), `wgpu/commands.rs` (6 LOC re-export shim). Their `mod` declarations and `pub use` re-exports in `wgpu/mod.rs` are removed. The `wgpu/layer_render.rs:17` import is fixed from `use super::commands::{CommandRenderer, dispatch_commands};` to `use crate::{commands::dispatch_commands, traits::CommandRenderer};`.

- **R8.** The `pub trait Painter` (~380 LOC in `traits.rs`) is deleted. The `traits.rs` file is renamed to `command_renderer.rs` (or stays but trimmed to ~380 LOC of just the `CommandRenderer` trait). `wgpu/painter.rs` removes its `impl Painter for WgpuPainter` block (~30 method impls); the methods stay as inherent impls on `WgpuPainter`. `wgpu/backend.rs` removes `use crate::traits::Painter`; only `use crate::traits::CommandRenderer` remains. `RenderError::PainterError(String)` variant and `RenderError::painter()` constructor are deleted. The crate-level `pub use traits::Painter` re-export is removed from `lib.rs` and `wgpu/mod.rs`.

### Refactor scope — lock / Arc-clone removal

- **R9.** `Arc<parking_lot::Mutex<OffscreenRenderer>>` is replaced with direct ownership:
  - `Renderer::offscreen: Option<Arc<parking_lot::Mutex<OffscreenRenderer>>>` → `offscreen: Option<OffscreenRenderer>`.
  - `Backend::offscreen: Option<Arc<parking_lot::Mutex<OffscreenRenderer>>>` → `offscreen: Option<&'a mut OffscreenRenderer>` (with lifetime `'a` introduced on `Backend<'a>`).
  - `Backend::new` and `Backend::with_offscreen` take `&'a mut WgpuPainter` + `&'a mut OffscreenRenderer`.
  - Callers in `Renderer::render_scene` pass `&mut self.painter`, `&mut self.offscreen` to `Backend` construction.
  - All `offscreen.lock()` calls inside `Backend::render_shader_mask` and `Renderer::handle_backdrop_filter` are replaced with direct `&mut` access.

- **R10.** `Arc<Mutex<TexturePoolInner>>` is replaced with direct ownership + explicit release:
  - `TexturePool::pool: Arc<Mutex<TexturePoolInner>>` → `TexturePool::available: Vec<TextureSlot>`.
  - `PooledTexture` no longer holds `inner: Arc<Mutex<TexturePoolInner>>`; its Drop becomes a no-op (or drops `wgpu::Texture` directly).
  - New method: `TexturePool::release(&mut self, texture: PooledTexture)`.
  - Acquisition: `TexturePool::acquire(&mut self, w, h, format) -> PooledTexture` (was `&self`; changed to `&mut self`).
  - `OffscreenRenderer::texture_pool: Arc<TexturePool>` → `texture_pool: TexturePool` (no Arc).
  - Every consumer of `PooledTexture` either explicitly returns it via `pool.release(tex)` or accepts that drop discards the texture (acceptable for one-frame textures).

- **R11.** Per-frame `Arc::clone(&device)` / `Arc::clone(&queue)` calls in `Renderer::render_scene` (`renderer.rs:636-637` constructing `RenderContext`) are eliminated. The `RenderContext` struct is changed from owning `Arc<wgpu::Device>` / `Arc<wgpu::Queue>` to borrowing `&'frame wgpu::Device` / `&'frame wgpu::Queue` references with a lifetime tied to the frame. The setup-phase `Arc::clone` calls in `Renderer::new`, `Renderer::new_offscreen`, `WgpuPainter::with_shared_device`, `OffscreenRenderer::new` are retained as acceptable (setup-only, run once per renderer lifetime).

### Refactor scope — error model + dead-code audit

- **R12.** `Renderer::new<W>(window: &W) -> Result<Self>` and `Renderer::new_offscreen() -> Result<Self>` migrate from `anyhow::Result<Self>` to `RenderResult<Self>`. wgpu errors are converted to `RenderError` via the existing `surface_creation` and `device_creation` constructors. `request_adapter` failure becomes `RenderError::NoAdapter`. The `anyhow` crate stays in `Cargo.toml` (transitive via wgpu) but is no longer used in the public API of `flui-engine`.

- **R13.** Every `#[allow(dead_code)]` marker is audited: the global `#![allow(dead_code)]` at `lib.rs:4` and per-module allows on `wgpu::effects`, `wgpu::instancing`, `wgpu::pipeline`, `wgpu::shader_compiler`. For each: either delete the dead items (if truly unused) or document the consumer (if used in a feature-gated path). The global `#![allow(dead_code)]` is removed; per-module allows are removed where their items are consumed. Items in `effects.rs` like `ShadowInstance`, `ShadowParams`, `BlurParams`, `BlurIntensity`, `LinearGradientBuilder` are either confirmed consumed by `painter.rs` or deleted. Items in `instancing.rs` like every `*Instance` type are verified consumed.

- **R14.** `wgpu/text.rs` (436 LOC) vs `wgpu/text_renderer.rs` (297 LOC) duplication is investigated. Either:
  - they are duplicates → one is deleted, the choice documented in ARCHITECTURE.md;
  - they are complementary (e.g. `text.rs` = recording, `text_renderer.rs` = rendering) → renamed for clarity or directory-split (`text/{recording, rendering}.rs`).

### Methodology extension

- **R15.** `scripts/port-check.sh` is extended:
  - Trigger 1 (`RwLock<Box<dyn ...>>`) adds `crates/flui-engine/src` to its path scope; the regex is extended to also catch `RwLock<wgpu::Device|wgpu::Queue|wgpu::Surface|OffscreenRenderer|WgpuPainter|TexturePool>` shapes (forward-looking; no current violations).
  - Trigger 2 (`Box<dyn>` wrapped in interior-mutability) adds `crates/flui-engine/src` to its path scope.
  - Trigger 3 (`async fn` on `build|layout|paint|perform_layout|composite|render|fire_composition_callbacks`) adds `crates/flui-engine/src` to its path scope; the verb set is extended to also include `submit|present|render_scene|render_layer_recursive|handle_backdrop_filter` so engine-level async violations are caught at the same trigger.
  - Trigger 5 (`Arc::clone` in per-frame paint/composite loop) — was already extended to `crates/flui-engine/src/wgpu/layer_render.rs` by the `flui-layer` chain U13. Further extends to `crates/flui-engine/src/wgpu/{renderer.rs, backend.rs}` (production paths), excluding the setup-phase `Renderer::new` / `new_offscreen` / `WgpuPainter::with_shared_device` / `OffscreenRenderer::new` paths via file/function exclusion.
  - New Trigger 7 (forward-looking) — `Arc<(parking_lot::)?(Mutex|RwLock)<*Renderer|*Pool|wgpu::*>` shape on a struct field in `crates/flui-engine/src/wgpu/`. Catches regressions of the `Arc<Mutex<OffscreenRenderer>>` (R9) and `Arc<Mutex<TexturePoolInner>>` (R10) patterns deleted in this chain.

- **R16.** After the refactor chain lands, `bash scripts/port-check.sh -v` exits 0 and reports each trigger as "ok" (seven triggers total: 1, 2, 3, 4, 5, 6, 7).

### Per-crate `ARCHITECTURE.md` instance

- **R17.** `crates/flui-engine/ARCHITECTURE.md` is created following the `docs/PORT.md` template specification (five fixed sections). Because `flui-engine` has no Flutter parity (it's a wgpu-native GPU lowering layer, not a port of any Flutter rendering component), the `## Flutter source mapping` section is replaced with a `## wgpu / Vulkan / Metal mapping` section that documents which wgpu APIs each module touches and cites wgpu / Vulkan-Metal spec references. The other four sections (Mapping decisions, Thread safety, Friction log, Outstanding refactors) follow the template directly.

- **R18.** `docs/PORT.md` `## Index` table flips `flui-engine` from "Not yet templated" to "Templated 2026-05-20" in the same commit that ships `crates/flui-engine/ARCHITECTURE.md`. The Index's "External references" section is unchanged.

### Mythos rules (non-negotiable, sourced from no-quick-wins memo)

- **R19.** Breaking ripples in `flui-app` are executed in-band, not deferred. The `Renderer::new` / `new_offscreen` migration from `anyhow::Result` to `RenderResult` (R12) lands together with caller updates in `flui-app/src/app/{binding, direct, runner}.rs`. The only legitimate deferrals are concrete-blocker-with-named-dependency (e.g. external dependency needed, CI infra change, derive-macro feature) explicitly named in `## Outstanding refactors`. "Mechanical busywork" and "would touch flui-app" are NOT legitimate deferrals.

- **R20.** No new `unsafe` block is introduced. The chain is expected to **net-zero** unsafe blocks (the single `unsafe { instance.create_surface_unsafe(...) }` in `Renderer::new` is required by wgpu's API contract and stays; no other unsafe is added). If a refactor would require new unsafe (e.g. `transmute` for vertex buffer casts), it is documented in the `## Outstanding refactors` with a local safety invariant + unit test commitment.

- **R21.** The hot path (`Renderer::render_scene`, `render_layer_recursive`, `Backend::render_*`, `WgpuPainter::*`, `LayerRender::*`, `dispatch_command`) remains synchronous after the chain. No `async fn` may be introduced on any rendering method. `Renderer::new` and `new_offscreen` stay async (wgpu's `request_adapter` + `request_device` are async at the wgpu boundary; acceptable per the strategy clause "sync hot path, async at edges").

---

## Acceptance Examples

- **AE1. Covers R4.** Given `crates/flui-engine/src/utils/text.rs` defines `VectorTextRenderer` (802 LOC) and `crates/flui-engine/src/utils/mod.rs` re-exports it (7 LOC), and the type has zero external callers in the workspace (verified by `grep -r "VectorTextRenderer\|utils::text" crates/`), when the Mythos chain lands, then `crates/flui-engine/src/utils/` does not exist, `lib.rs` has no `pub mod utils;`, and a fresh `grep -r "VectorTextRenderer\|utils::text" crates/` returns zero matches.

- **AE2. Covers R5.** Given `crates/flui-engine/src/wgpu/scene.rs` (1,820 LOC) defines `Scene`, `SceneBuilder`, `Layer`, `Primitive`, `LayerBatch`, `PrimitiveBatch`, and `crates/flui-engine/src/wgpu/compositor.rs` (365 LOC) defines `Compositor`, `TransformStack`, `RenderContext` consuming the dead types, and both are re-exported from `wgpu::mod` colliding with `flui_layer::Scene` re-export, when the Mythos chain lands, then both files do not exist, the `wgpu::mod` exports `pub use scene::{Scene, SceneBuilder}` and `pub use compositor::{Compositor, RenderContext, TransformStack}` are gone, the only `Scene` re-exported from `flui-engine::lib.rs` is `flui_layer::Scene`, and `cargo build --workspace` is clean.

- **AE3. Covers R6.** Given `crates/flui-engine/src/wgpu/vulkan.rs` (826 LOC), `wgpu/dx12.rs` (769 LOC), `wgpu/metal.rs` (587 LOC) — three platform-capability files with zero external callers (verified by `grep -r "wgpu::vulkan::\|wgpu::dx12::\|wgpu::metal::" crates/` returning only doc-comment matches), when the Mythos chain lands, then all three files do not exist, the `wgpu/mod.rs` `#[cfg(...)] pub mod vulkan;` / `dx12;` / `metal;` declarations are removed, and `cargo build --target x86_64-pc-windows-msvc` / `cargo build --target x86_64-apple-darwin` / `cargo build --target x86_64-unknown-linux-gnu` are all clean.

- **AE4. Covers R7.** Given dead infrastructure modules `wgpu/external_texture_registry.rs` (315 LOC), `wgpu/texture_cache.rs` (1,000 LOC), `wgpu/path_cache.rs` (336 LOC), `wgpu/multi_draw.rs` (304 LOC), `wgpu/commands.rs` (6 LOC re-export shim) all have zero external callers, when the Mythos chain lands, then all five files do not exist, their `mod` declarations and `pub use` re-exports are removed from `wgpu/mod.rs`, the `wgpu/layer_render.rs:17` import is updated to `use crate::{commands::dispatch_commands, traits::CommandRenderer};`, and `cargo build --workspace` is clean.

- **AE5. Covers R8.** Given `pub trait Painter` (~380 LOC in `traits.rs`) has 30+ methods including 6 default impls printing `tracing::warn!("Painter::draw_path: not implemented")` and exactly 1 production impl (`WgpuPainter`), when the Mythos chain lands, then the trait does not exist, `WgpuPainter`'s methods are inherent (not trait impls), `Backend` calls `painter.rect(...)` directly without `<WgpuPainter as Painter>::rect(painter, ...)` ceremony, `RenderError::PainterError(String)` is deleted, and `cargo test -p flui-engine` is green.

- **AE6. Covers R9.** Given `Renderer::offscreen: Option<Arc<parking_lot::Mutex<OffscreenRenderer>>>` and `Backend::offscreen: Option<Arc<parking_lot::Mutex<OffscreenRenderer>>>` both wrap the same `OffscreenRenderer` in an Arc<Mutex<>> that has exactly one mutator (the render thread), when the Mythos chain lands, then `Renderer::offscreen: Option<OffscreenRenderer>` (direct ownership), `Backend::offscreen: Option<&'a mut OffscreenRenderer>` (borrowed), `Backend<'a>` is generic over a frame lifetime, `Backend::render_shader_mask` and `Renderer::handle_backdrop_filter` use `&mut` access instead of `.lock()`, and `cargo test -p flui-engine` is green.

- **AE7. Covers R10.** Given `TexturePool::pool: Arc<Mutex<TexturePoolInner>>` and `PooledTexture::inner: Arc<Mutex<TexturePoolInner>>` form a release-on-drop pattern with single-mutator data, when the Mythos chain lands, then `TexturePool::available: Vec<TextureSlot>` (direct ownership), `PooledTexture` has no inner Arc field, `TexturePool::acquire(&mut self, ...)` and `TexturePool::release(&mut self, ...)` are explicit, every consumer of `PooledTexture` either calls `pool.release(tex)` or accepts drop semantics, and `cargo test -p flui-engine` is green.

- **AE8. Covers R11.** Given `Renderer::render_scene` constructs `RenderContext { device: Arc::clone(&self.device), queue: Arc::clone(&self.queue), ... }` per frame (lines 636-637), when the Mythos chain lands, then `RenderContext` borrows `&'frame wgpu::Device` / `&'frame wgpu::Queue`, the per-frame `Arc::clone` calls in `Renderer::render_scene` are eliminated, and `bash scripts/port-check.sh -v` Trigger 5 stays clean for `crates/flui-engine/src/wgpu/{renderer.rs, backend.rs}`.

- **AE9. Covers R12.** Given `Renderer::new<W>(window: &W) -> anyhow::Result<Self>` and `Renderer::new_offscreen() -> anyhow::Result<Self>` return `anyhow::Result` while the rest of the engine's public API uses `RenderResult<T>`, when the Mythos chain lands, then both methods return `RenderResult<Self>`, wgpu errors are converted via `RenderError::surface_creation` / `device_creation`, `flui-app/src/app/{binding, direct, runner}.rs` callers handle `RenderError` directly (no `anyhow::Error` conversion), and `cargo build --workspace` is clean.

- **AE10. Covers R13.** Given `#![allow(dead_code, missing_debug_implementations)]` at `lib.rs:4` and per-module `#[allow(dead_code)]` markers on `wgpu::effects`, `wgpu::instancing`, `wgpu::pipeline`, `wgpu::shader_compiler` suppress dead-code warnings globally, when the Mythos chain lands, then the global `#![allow(dead_code)]` is removed (only `#![allow(missing_debug_implementations)]` remains because wgpu types don't implement Debug), per-module dead-code allows are removed where their items are consumed (or items deleted where unused), and `cargo clippy --workspace -- -D warnings` is clean.

- **AE11. Covers R15, R16.** Given `scripts/port-check.sh` currently scopes Triggers 1, 2, 3 to `crates/flui-rendering/src` + `crates/flui-view/src` + `crates/flui-painting/src` + `crates/flui-layer/src`, Trigger 5 to `crates/flui-rendering/src/objects` + `crates/flui-engine/src/wgpu/layer_render.rs`, and Triggers 4 and 6 are scoped per their existing rules, when the Mythos chain lands, then Triggers 1, 2, 3 also scope `crates/flui-engine/src`, Trigger 3 verb set adds `submit|present|render_scene|render_layer_recursive|handle_backdrop_filter`, Trigger 5 scope adds `crates/flui-engine/src/wgpu/{renderer.rs, backend.rs}` (excluding setup-phase paths), a new Trigger 7 scans `crates/flui-engine/src/wgpu/` for `Arc<(parking_lot::)?(Mutex|RwLock)<*Renderer|*Pool|wgpu::*>` shapes, and `bash scripts/port-check.sh -v` exits 0 with seven "ok" lines.

- **AE12. Covers R17, R18.** Given `docs/PORT.md` `## Index` lists `flui-engine` as "Not yet templated" today, when the Mythos chain lands, then `crates/flui-engine/ARCHITECTURE.md` exists with the five fixed template sections populated (with `## wgpu / Vulkan / Metal mapping` in place of `## Flutter source mapping`), `docs/PORT.md` `## Index` lists `flui-engine` as "Templated 2026-05-20", and the `## Mapping decisions` section includes "Accepted trade-offs" entries for: `LayerRender<R>` static dispatch (vs `Box<dyn Backend>`), direct `OffscreenRenderer` ownership (vs `Arc<Mutex<>>`), explicit `TexturePool::release` (vs `Arc<Mutex<>>` Drop), `Painter` trait deletion (vs retain for future), dead-module deletion (vs retain as "internal IR"), `RenderResult` migration (vs `anyhow::Result`).

- **AE13. Covers R19 (Mythos rules).** Given the chain migrates `Renderer::new` / `new_offscreen` from `anyhow::Result` to `RenderResult` and forces callers in `flui-app/src/app/binding.rs`, `flui-app/src/app/direct.rs`, `flui-app/src/app/runner.rs` to migrate their error handling, when the chain lands, then those caller-side updates are commits inside the same PR (not a follow-up), and no "TODO: migrate callers" comment exists anywhere.

---

## Success Criteria

- The Mythos refactor chain merges as a feature branch off `main` in a single PR with 14 reviewable commits, each commit passing `cargo check --workspace`, `cargo test -p flui-engine --lib`, and `bash scripts/port-check.sh` (extended). No commit lands with broken tests or red CI.

- Net unsafe delta for `flui-engine`: **0** (the single existing `unsafe { instance.create_surface_unsafe(...) }` block in `Renderer::new` is required by wgpu's API contract and stays; no new unsafe is added). Net LOC delta in the touched .rs files: targeted reduction of **≥ 6,000 LOC** of production code (utils/ 809 LOC + wgpu/scene 1,820 + wgpu/compositor 365 + wgpu/vulkan 826 + wgpu/dx12 769 + wgpu/metal 587 + wgpu/external_texture_registry 315 + wgpu/texture_cache 1,000 + wgpu/path_cache 336 + wgpu/multi_draw 304 + wgpu/commands 6 + Painter trait ~380 — total ~7,517 LOC eligible; not all may be removable in a single chain, but the budget is ≥ 6,000).

- `crates/flui-engine/ARCHITECTURE.md` exists and matches the template. `docs/PORT.md` `## Index` shows `flui-engine` as "Templated 2026-05-20".

- `scripts/port-check.sh` covers `crates/flui-engine/src/` (Triggers 1, 2, 3, 5, 7). Running `bash scripts/port-check.sh -v` exits 0 and prints seven "ok" lines.

- The PR description follows the shape of PR #77 / PR #78 (Track A / Track B sections, key decisions, testing summary, quick-wins-track callouts listing any temptations the maintainer caught and rejected during the chain).

- A2 (a downstream agent) can pick up any entry from `crates/flui-engine/ARCHITECTURE.md` `## Outstanding refactors` and produce a follow-up PR without a fresh brainstorm or out-of-band clarification.

- All `flui-app` integration tests pass after the `anyhow::Result` → `RenderResult` migration.

---

## Scope Boundaries

- **Out of scope: `painter/` directory split (mechanical LOC redistribution).** The verdict mentions splitting `wgpu/painter.rs` (3,772 LOC) into `painter/{batch, segment, layer, gradient, text, render}.rs`. If chain bandwidth permits, the split lands in U10 (mirroring `flui-layer` U10 compositor split). If not, the split is deferred to an Outstanding refactor with the named blocker "no semantic change; defer for review-clarity if chain is already large."

- **Out of scope: `offscreen/` directory split.** Similar reasoning. `wgpu/offscreen.rs` (1,525 LOC) could split into `offscreen/{mask, blur, morph}.rs`. Same defer-or-include decision at chain time.

- **Out of scope: property tests, miri gate, loom tests for `flui-engine`.** Same shape as `flui-rendering`'s / `flui-layer`'s carry-over (requires `proptest`/`loom` dev-deps and CI infra changes that exceed the chain's scope).

- **Out of scope: Implementing the missing `BackdropFilter`, `ImageFilter::ColorAdjust`, `ImageFilter::Compose` GPU paths.** The verdict notes these have fallback paths (render without filter, logged at warn). Production-quality implementation is a separate feature plan, not a Mythos chain.

- **Out of scope: Second rendering backend (Skia, Vello, software).** Deleting the `Painter` trait does not preclude future backends; it just removes the empty-promise scaffolding. When/if a real backend is needed, it's a new feature plan.

- **Out of scope: HDR / WCG / EDR rendering.** The deleted `wgpu/metal.rs` defined `EdrConfig`; the verdict's deletion does not remove HDR capability from wgpu itself (`wgpu::Adapter::features()` still reports it). HDR rendering is a separate feature plan that would re-implement only what's needed, not 587 LOC of stubs.

- **Out of scope: Re-enabling `flui-devtools`, `flui-cli`.** Disabled crates are not in the chain's blast radius.

- **Out of scope: Workspace-wide `Arc<RwLock<>>` audit of non-`flui-engine` crates.** This chain only touches `flui-engine` (locks, dead code, refactors) and `flui-app` (R12 caller migration). A workspace-wide audit is a separate brainstorm.

- **Out of scope: Documenting per-shader-stage GPU lowering for every WGSL file in `wgpu/shaders/`.** The verdict notes "per-variant GPU lowering documentation in flui-engine" as an Outstanding refactor that lives in this crate's docs, not in this chain.

- **Out of scope: Custom-render-callback integration.** `docs/plans/2026-03-31-custom-render-callback-design.md` exists as a precedent plan. Integration with `flui-engine`'s `WgpuPainter` is a separate feature.

---

## Key Decisions

- **Delete dead modules over keep-as-internal-IR.** Six modules with zero production callers (`utils/`, `wgpu/scene.rs`, `wgpu/compositor.rs`, `wgpu/vulkan.rs`, `wgpu/dx12.rs`, `wgpu/metal.rs`, plus four dead infrastructure modules) are deleted. The "internal IR for batching" argument for `wgpu/scene.rs` is rejected because `WgpuPainter` already does internal batching via `DrawSegment`; a second IR above is duplication.

- **Delete `Painter` trait over keep for second backend.** The trait has 1 production impl and 6 default `not implemented` warnings. No second backend exists or is planned. When a real second backend lands, the abstraction will be rebuilt against the actual second impl, not retrofitted to a hypothetical one.

- **Direct ownership of `OffscreenRenderer` over `Arc<Mutex<>>`.** Single-mutator data. The lock guards against concurrent access that never happens. Direct ownership + lifetime-borrowed `Backend<'a>` enforces single-mutator at compile time.

- **Explicit `TexturePool::release` over `Arc<Mutex<>>` Drop.** Drop-based release requires an inner `Arc<Mutex<TexturePoolInner>>` on every `PooledTexture`. Explicit `pool.release(tex)` removes the back-reference; consumers call it when they know they're done. Drop becomes a no-op (or drops the wgpu::Texture, releasing GPU memory if not pooled).

- **Borrow references over per-frame Arc::clone.** `RenderContext` (per-frame state struct) holds `&wgpu::Device` / `&wgpu::Queue` references with a frame lifetime, not `Arc<wgpu::Device>` clones. Setup-phase Arc::clone (Renderer::new, OffscreenRenderer::new) stays.

- **Migrate to `RenderResult<T>` over keep `anyhow::Result`.** Consistency with the rest of the engine's public API. `RenderError` is structured and `thiserror`-derived; `anyhow::Error` is opaque and loses type-level information.

- **Static dispatch `LayerRender<R>` over `enum_dispatch` macro.** Same reasoning as `flui-layer`'s rejection. Output identical; no new proc-macro dep. Hand-readable.

- **Audit `#[allow(dead_code)]` instead of trusting it.** Each suppression is a canary that an item lost its consumer. The chain removes the global allow at `lib.rs:4` and per-module allows; survivors must have documented consumers.

- **Keep the single `unsafe { instance.create_surface_unsafe(...) }` block.** wgpu's API requires it; the safety invariant (window handle lifetime) is documented at the call site and honoured by `flui-app`. Net unsafe delta is **zero**.

- **`Renderer: Send` (not `Sync`) over `Sync`-everywhere.** Frame production happens on one thread, GPU submission on the render thread; `Send` allows cross-thread Renderer move. `Sync` would invite `Arc<Renderer>` ceremony at every caller with no concrete need.

- **Land breaking ripples in-band over deferred.** Per the no-quick-wins memo. The `anyhow::Result` → `RenderResult` migration in `flui-app/src/app/{binding, direct, runner}.rs` lands in the same chain (R12 + R19). No "follow-up PR for migrating callers."

- **`painter/` and `offscreen/` directory splits are chain-bandwidth-dependent.** If the chain has room, they land in U10. If not, they go to Outstanding refactors with concrete blocker "no semantic change; mechanical LOC redistribution; defer for review-clarity."

---

## Open Questions

### Resolved during planning

- "Does any external caller actually use `Painter` trait outside `flui-engine`?" — resolved: zero. The only `impl Painter` is `WgpuPainter` inside `flui-engine`. No other crate has `impl Painter for ...` anywhere.

- "Does `wgpu/scene.rs` have any production caller in the workspace?" — resolved: zero. Only `wgpu/compositor.rs` (itself dead) references its types.

- "How many `unsafe impl` blocks does `flui-engine` carry?" — resolved: zero. (Unlike `flui-layer` which had 39, the engine has none.) Net unsafe delta is zero.

- "How many `Arc::clone(&device)` / `Arc::clone(&queue)` sites are on the per-frame hot path?" — resolved: two confirmed per-frame sites (`renderer.rs:636-637`, `backend.rs:408-409`); plus initialisation-time sites that stay (`backend.rs:121-122` for the offscreen painter cache amortises across frames; `renderer.rs:260-269` is setup-phase).

- "Are there any genuine `Box<dyn ...>` storage sites in the crate?" — resolved: only `Box<dyn Error + Send + Sync>` in `RenderError::SurfaceCreation` and `RenderError::DeviceCreation` for cross-backend error wrapping. These are appropriate for error types and stay.

### Deferred to implementation

- **Whether `traits.rs` should be renamed to `command_renderer.rs` after the `Painter` trait deletion.** Resolution at U5 step time. Renaming is mechanical; if it touches imports across many files, defer to a small cleanup commit at U14. Recommend: rename in U5 (single commit), since the file's content shrinks to just `CommandRenderer` and the new name is clearer.

- **Whether the new Trigger 7 should be added in U13 or in a separate housekeeping commit.** Resolution at U13. Recommend: add in U13 so the trigger lands at the same time as the refactor that motivates it (R9 + R10 deletions of the `Arc<Mutex<>>` patterns).

- **Whether `painter/` and `offscreen/` directory splits land in this chain (U10) or are deferred.** Resolution at U10. Recommend: defer both to Outstanding refactors with concrete blocker "no semantic change; mechanical LOC redistribution; in scope for a follow-up chain." The current chain is already touching 14 LOC-heavy changes; mechanical splits without semantic restructuring can land in a thin housekeeping PR.

- **Whether `text.rs` vs `text_renderer.rs` is duplication or two-stage architecture.** Resolution at U11. Recommend: read both files end-to-end at U11; if duplicate, delete one; if complementary, document the split in ARCHITECTURE.md and rename for clarity.

- **Whether the `unsafe { instance.create_surface_unsafe(...) }` block needs an explicit SAFETY comment block.** Resolution at U14 cleanup. Recommend: add a SAFETY comment naming the window-handle-lifetime invariant; mirrors `flui-rendering`'s pattern of documenting unsafe locally at every block.

- **Whether to add `catch_unwind` around `render_scene` in this chain or defer.** Resolution: defer. The verdict's §12 rejected design notes "deferred unless a real-world panic surfaces"; track in `## Outstanding refactors` as forward-looking.

---

## Related Work

- `docs/designs/2026-05-20-mythos-flui-rendering-redesign.md` — the precedent verdict (rendering chain). The 13-section structure, rejected-designs format, and 14-step implementation plan template are sourced from this document.

- `docs/designs/2026-05-20-mythos-flui-layer-redesign.md` — the precedent verdict (layer chain). The "ARCHITECTURE.md template-N/A for Flutter source mapping" pattern is established here for crates without Flutter parity.

- `docs/brainstorms/flui-layer-mythos-redesign-requirements.md` — the precedent brainstorm (layer chain). The R-ID / AE / Scope Boundaries / Key Decisions structure is mirrored here.

- `docs/plans/2026-05-20-002-feat-flui-layer-mythos-redesign-plan.md` — the precedent implementation plan (layer chain). The U1-U14 plan structure with dependency graph + per-unit acceptance tests is mirrored in this chain's plan.

- `docs/plans/2026-05-19-001-feat-flutter-port-methodology-plan.md` — the methodology plan that established `docs/PORT.md`, the per-crate `ARCHITECTURE.md` template, and `scripts/port-check.sh`. Source for the U12/U13 patterns.

- `crates/flui-rendering/ARCHITECTURE.md` — the exemplar per-crate template instance for a Flutter-parity crate.

- `crates/flui-layer/ARCHITECTURE.md` — the exemplar per-crate template instance for a Mythos-cleaned crate. The `## Mapping decisions` "Accepted trade-offs" format used in this chain mirrors layer's.

- `~/.claude/projects/.../memory/no-quick-wins-vanyastaff.md` — the no-deferred-ripples rule. R19/AE13 codifies this for the chain.

- Reference commits on `main` (exemplars for the chain steps):
  - `907a7787` — full delete + rewire (analog: U1-U7 dead-code deletions).
  - `4d05efc5` — god-module split (analog: U10/U11 painter/ / offscreen/ splits, **if** they land in this chain).
  - `dc0fa1ad` — `catch_unwind` plumbing (analog: NOT applied here; deferred per §12 rejected design).
  - `d0e53c63` — extension-trait split (analog: U5 `Painter` trait deletion).
  - `6edae9fd` — disjoint-borrow `unsafe` primitive (NOT applicable; `Renderer` mutation is single-owner; no disjoint-borrow primitive needed).
  - `702e8751` through `5dda0350` — the `flui-layer` chain (15 commits, PR #78). The U-numbering, commit format, and "Mythos Step N" message convention used in this chain mirror layer's.

External references this chain builds on (referenced in `docs/PORT.md`):

- `wgpu` docs (used as the GPU API reference for the engine; cited in `## wgpu / Vulkan / Metal mapping` of ARCHITECTURE.md).
- Vulkan + Metal specs (cited where engine code follows their semantics, e.g. swapchain present modes).
- GPUI's platform abstraction patterns (cited in CLAUDE.md `Reference Sources`; influenced the Renderer/Backend split shape).
