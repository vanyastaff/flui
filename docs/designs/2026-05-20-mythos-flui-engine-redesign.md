---
title: "Mythos design verdict — flui-engine redesign"
status: design
date: 2026-05-20
author: Claude Mythos
applies-to: crates/flui-engine
---

# Mythos Design Verdict

## What `flui-engine` should be

A **GPU command renderer** that consumes a finished `Scene<Layer>` from `flui-layer` and lowers each layer variant to wgpu draw calls. Three concerns, one crate: own the wgpu device/queue/surface; record GPU work via a single batched `WgpuPainter`; walk the layer tree and route each variant through a closed `LayerRender` dispatch.

Nothing else. No scene-graph types. No parallel `Compositor` with its own `TransformStack`. No platform-specific feature files for capabilities wgpu already exposes through `Adapter::get_info()` and `Adapter::features()`. No `Box<dyn Backend>` plugin boundary for backends that have never existed.

## What it must not become

A second copy of `flui-layer::Scene` (which is what `wgpu/scene.rs` is today — 1,820 LOC defining `Scene`/`SceneBuilder`/`Layer`/`Primitive`/`LayerBatch`/`PrimitiveBatch`, zero external callers, only referenced by `wgpu/compositor.rs` which is itself unreferenced). A second `TransformStack` whose state is reimplemented inside `WgpuPainter::save`/`restore`. A `Compositor::begin_layer`/`end_layer` abstraction over `LayerBatch` instances that no production code ever constructs. A `flui-engine::wgpu::vulkan`/`dx12`/`metal` documentation crate disguised as code, 2,182 LOC across three files that no caller imports.

A `Painter` trait with 30+ methods, six default impls printing `tracing::warn!("Painter::draw_path: not implemented")`, and exactly one production implementation (`WgpuPainter`) duplicating every method of `CommandRenderer` minus the transform parameter. A `commands` shim module that re-exports `crate::commands` items unchanged because the `LayerRender` trait was glued to `super::commands` instead of `crate`. An `ExternalTextureRegistry`, `TextureCache`, `PathCache`, `MultiDrawBatcher` set of "future GPU subsystems" with zero consumers.

## Main state owner

`Renderer` owns: `wgpu::Instance`, `wgpu::Adapter`, `Arc<wgpu::Device>`, `Arc<wgpu::Queue>`, `Option<wgpu::Surface<'static>>`, `wgpu::SurfaceConfiguration`, `GpuCapabilities`, `WgpuPainter`, optional `OffscreenRenderer`, `DamageTracker`, `OcclusionTracker`. One mutable owner, lives the lifetime of the application window, dropped at shutdown.

`OffscreenRenderer` today is `Arc<parking_lot::Mutex<OffscreenRenderer>>` shared between `Renderer` and the per-frame `Backend`. The lock guards data that has exactly one mutator (the frame thread) and is never read from another thread. **The `Arc<Mutex<>>` shape is deleted.** The `OffscreenRenderer` moves inside `Renderer` as a plain field; the `Backend` borrows it via `&mut OffscreenRenderer` for the duration of one frame, exactly the same way `WgpuPainter` is borrowed.

`TexturePool` today is `Arc<Mutex<TexturePoolInner>>` with a `pool: Arc<Mutex<TexturePoolInner>>` and a `PooledTexture` that holds another `Arc<Mutex<TexturePoolInner>>` to release-on-drop. The pool is per-frame, single-mutator. The `Arc<Mutex<>>` exists only because `PooledTexture::drop` needs to call back into the pool — and that callback can be replaced by an explicit `pool.release(texture)` call inside the `OffscreenRenderer` workflow, or by storing only the texture descriptor and reacquiring on next-frame. **Lock deletion candidate; the call sites are small.**

## Main trust boundary

**Two closed dispatch sites, both compile-time exhaustive.**

1. `LayerRender::render(&self, renderer: &mut R)` and `cleanup(&self, renderer: &mut R)` on the closed `Layer` enum from `flui-layer`. 19 variants, 19 arms, generic over `R: CommandRenderer + ?Sized`. Static dispatch, no vtable, no `Box<dyn Layer>`. This is the canonical Rust-native shape and stays. Adding a 20th layer variant is a coordinated change in `flui-layer` + `flui-engine`.

2. `dispatch_command(&DrawCommand, &mut R)` at `commands.rs`. ~30 variants, ~30 arms, generic over `R: CommandRenderer + ?Sized`. Same static-dispatch shape, same closed boundary.

There is no `Box<dyn Backend>` plugin boundary. The current `pub trait Painter` (30+ methods, 6 default-`tracing::warn!`-printing impls, 1 production impl) is **deleted**. `WgpuPainter` becomes a concrete struct with concrete methods. The `Backend` struct that today wraps `WgpuPainter` and implements `CommandRenderer` stays — it is the visitor for `DrawCommand` and has one production impl + one test mock.

## Main async risk

Zero on the hot path. `Renderer::new` and `Renderer::new_offscreen` are `async` (wgpu's `request_adapter` and `request_device` are async at the wgpu boundary; this is unavoidable at the platform edge). `Renderer::render_scene` is **sync**. The layer walk in `render_layer_recursive` is sync. The `dispatch_command` visitor is sync. The `WgpuPainter` batched recording is sync. Submission via `queue.submit(...)` is sync. Surface acquisition via `surface.get_current_texture()` is sync (the GPU's "is the next backbuffer ready" check is not awaited).

There is **no** `tokio::spawn`, no background frame producer, no async channel between the frame loop and the engine. Backpressure is implicit: `present_mode = Fifo` (vsync) or `Mailbox` (triple-buffered low-latency) — both handled inside wgpu's surface acquisition, not at the engine level.

## Main simplification principle

**Every module in `crates/flui-engine/src/wgpu/` must justify its existence with a production caller — not a re-export, not a doc comment, not "future GPU lifecycle".**

A non-exhaustive list of indirection that does not justify itself today:

- **`wgpu/scene.rs` (1,820 LOC)** — defines `Scene`, `SceneBuilder`, `Layer`, `Primitive`, `LayerBatch`, `PrimitiveBatch`, `PrimitiveType`, `BlendMode`. **All re-exported from `wgpu::mod`. Zero external callers.** Only used internally by `wgpu/compositor.rs` (which is itself dead). The crate-root re-export `pub use scene::{Scene, SceneBuilder};` actively **collides with** `flui_layer::{Scene, SceneBuilder}` which is also re-exported from `flui-engine::lib.rs`. Two `Scene` types in one crate's public API is hostile. Delete `wgpu/scene.rs` entirely.

- **`wgpu/compositor.rs` (365 LOC)** — defines `Compositor`, `TransformStack`, `RenderContext`. Each is a thin stack over `Vec<T>`. Zero external callers. The `Compositor::begin_layer(batch: &LayerBatch)` API consumes the dead `LayerBatch` type from `wgpu/scene.rs`. `WgpuPainter` already maintains a `save`/`restore` transform stack internally. **Delete the whole file.**

- **`wgpu/vulkan.rs` (826 LOC), `wgpu/dx12.rs` (769 LOC), `wgpu/metal.rs` (587 LOC)** — three platform-specific files defining `VulkanFeatures`, `Dx12Features`, `MetalFxUpscaler`, `EdrConfig`, etc. **Zero callers** outside their own doc comments. wgpu already exposes adapter/feature introspection via `wgpu::Adapter::get_info()`, `wgpu::Adapter::features()`, `wgpu::Adapter::limits()`. The three files are wishlists for capability detection that wgpu already does. Total 2,182 LOC. **Delete all three.**

- **`wgpu/external_texture_registry.rs` (315 LOC)** — `ExternalTextureRegistry`, `ExternalTextureEntry`. Zero external callers. Forward-looking infrastructure for platform-view embedding with no consumer. Recoded if/when `PlatformViewLayer` actually grows a wgpu lowering. Delete.

- **`wgpu/texture_cache.rs` (1,000 LOC)** — `TextureCache`. Zero external callers. Distinct from `texture_pool.rs` (which IS used by `OffscreenRenderer` and `WgpuPainter`). Delete.

- **`wgpu/path_cache.rs` (336 LOC)** — `PathCache`. Zero external callers. Delete.

- **`wgpu/multi_draw.rs` (304 LOC)** — `MultiDrawBatcher`, `DrawCommand`, `DrawIndexedIndirectArgs`, `MultiDrawStats`, `PipelineId`. Note the `DrawCommand` here **collides with** `flui_painting::DrawCommand` — two different types named `DrawCommand` cross the engine. Zero external callers for the `MultiDrawBatcher`. Delete.

- **`wgpu/commands.rs` (6 LOC)** — a re-export shim: `pub use crate::{commands::{dispatch_command, dispatch_commands}, traits::CommandRenderer};`. Exists only so `wgpu/layer_render.rs` could write `super::commands::CommandRenderer` instead of `crate::traits::CommandRenderer`. **Delete the shim and fix the import.**

- **`pub trait Painter` (380 LOC in `traits.rs`)** — 30+ methods, 6 default impls that print `tracing::warn!("Painter::draw_path: not implemented")`, 1 production impl (`WgpuPainter`). The trait was designed for "multiple backends without changing high-level rendering code" (its docstring). No second backend exists, no second backend is planned in any document in the repo, and the `LayerRender<R: CommandRenderer>` boundary already provides the abstraction `flui-rendering` needs. **Delete the trait.** `WgpuPainter` becomes a concrete struct.

- **`utils/text.rs` (802 LOC, `VectorTextRenderer`)** — vector text rendering using ttf-parser + lyon. Zero external callers. The `cosmic-text` + `glyphon` stack handles text in `WgpuPainter` directly. The vector-text experiment was "slower than raster text but supports arbitrary transformations" (file docstring). Has 30+ test functions. Delete the whole `utils/` directory — or document a real consumer (none today).

- **`#![allow(dead_code, missing_debug_implementations)]` at `lib.rs:4`** — global suppression of dead-code warnings. Removed once the deletions land. Per-module `#[allow(dead_code)]` markers on `effects`, `instancing`, `pipeline`, `shader_compiler` go away once their consumers are clarified.

- **`Arc<parking_lot::Mutex<OffscreenRenderer>>`** in `Renderer`, `Backend`, `Backend::offscreen()`, `Backend::with_offscreen()`, `Backend::render_shader_mask`, `Renderer::handle_backdrop_filter`. The lock guards data that has one mutator (the render thread) and is never accessed concurrently. **Replace with direct ownership in `Renderer` and a `&mut OffscreenRenderer` parameter to `Backend` workflows.** Mythos refactor target.

- **`Arc<Mutex<TexturePoolInner>>`** in `TexturePool` + `PooledTexture` for the release-on-drop pattern. Single-mutator data. **Replace with explicit `pool.release(texture)` or with descriptor-only acquisition.**

- **`Arc::clone(&device)` / `Arc::clone(&queue)`** in `backend.rs:121-122`, `renderer.rs:260-269, 636-650`, `painter.rs:866`, `offscreen.rs:93, 279, 659-660, 1051-1053`. The `Arc<wgpu::Device>` and `Arc<wgpu::Queue>` are passed by clone into every subsystem (`WgpuPainter`, `OffscreenRenderer`, `TexturePool`). Most clone sites are setup-phase (acceptable). The per-frame clones in `Renderer::render_scene` (lines 636-637) and `Backend::render_shader_mask` (lines 408-409) are not — they touch the hot path. **Replace with `&Arc<wgpu::Device>` references** where the borrow scope allows, or pass `&wgpu::Device` directly.

- **`#[allow(missing_debug_implementations)]` everywhere**, including on multiple structs. wgpu's resources don't implement `Debug`; the crate-level `#![allow(missing_debug_implementations)]` already covers this. Per-struct allows are noise; remove them.

This is not architecture. It is the visible cost of a year of "I'll wire this up later" decisions that never connected. Mythos cleanup recovers the ~6,000 LOC of dead code and the ~8 cross-trait/cross-module name collisions before the engine grows another platform.

---

## 1. Problem Definition

**Responsibility.** Own the wgpu pipeline (device/queue/surface). Consume `flui_layer::Scene` per frame. Walk the layer tree depth-first, dispatching each layer through `LayerRender` static dispatch into a `Backend` that implements `CommandRenderer`. The `Backend` calls into a single `WgpuPainter` that batches GPU work and submits it. Provide damage tracking (skip empty frames), occlusion culling (skip occluded subtrees), and the special mid-frame flush flow for `BackdropFilterLayer` (which needs the surface's pixels mid-frame to apply blur).

**Non-responsibility.**
- Scene graph construction (lives in `flui-layer` / `flui-painting`).
- Widget / element tree mutation (lives in `flui-view` / `flui-rendering`).
- Display-list recording (`Canvas` API; lives in `flui-painting`).
- Platform window creation, event loop, focus/keyboard input (lives in `flui-platform`, threaded via `flui-app`).
- Cross-backend abstraction (no Skia, no Vello, no software path — the doc-string promise is removed).

**Callers.** Two crates consume `flui-engine`:
- `flui-app` (`binding.rs`, `direct.rs`, `runner.rs`) — uses `flui_engine::wgpu::Renderer` and `flui_engine::RenderError`.
- `flui-rendering` — does NOT use `flui-engine` directly per grep; the only mention is `OffscreenRenderer` referenced in doc comments. Pipeline goes `flui-rendering::PipelineOwner::paint` → produces `Scene` → `flui-app` forwards `Scene` → `flui-engine::Renderer::render_scene`.

External-public API surface today is much wider than what the two callers use. The Mythos cut trims the public API to what is actually consumed.

**Lifecycle.** Single `Renderer` per window, constructed once at app start, owns wgpu resources for the lifetime of the window. `render_scene(&Scene)` is called once per frame from `flui-app`'s main loop. `resize(w, h)` is called on window resize. `mark_dirty(rect)` / `mark_full_repaint()` are called when state changes invalidate the surface.

**Key invariants.**
1. **Single owner of wgpu resources.** `Renderer` owns `wgpu::Device`, `wgpu::Queue`, `wgpu::Surface`. No `Arc<RwLock<Renderer>>` anywhere, no `Arc<Mutex<>>` on subsystems that `Renderer` owns. The `Arc<wgpu::Device>` / `Arc<wgpu::Queue>` shared with `WgpuPainter` and (today) `OffscreenRenderer` is the only `Arc` shape allowed — and only because wgpu's own API uses `Arc` for those handles (cheap; reference-counted, not lock-protected).
2. **Sync hot path.** No `async fn` in `render_scene`, `render_layer_recursive`, `handle_backdrop_filter`, `Backend::*`, `WgpuPainter::*`, `LayerRender::*`. Async only at `Renderer::new` (wgpu device init).
3. **Closed dispatch.** `Layer` is a 19-variant closed enum (owned by `flui-layer`). `DrawCommand` is a 30-variant closed enum (owned by `flui-painting`). Both are matched exhaustively in `flui-engine`.
4. **One `CommandRenderer` per frame.** The `Backend` struct is constructed at frame start, holds `WgpuPainter` plus `&mut OffscreenRenderer` (post-refactor), and drops at frame end. No `Arc<RwLock<Backend>>` shape.
5. **No surprise dependency cycles.** `flui-engine` depends on `flui-layer`, `flui-painting`, `flui-foundation`, `flui-types`. It does **not** depend on `flui-rendering` (would be a cycle). `flui-app` is downstream and pulls the engine into the frame loop.

**Failure modes — normal, not exceptional.**
- Surface lost / outdated — `wgpu::SurfaceError::{Lost, Outdated}` is recoverable; `Renderer::reconfigure_surface()` handles it, then the frame retries. Logged at `info` level, not `error`.
- Frame timeout — `wgpu::SurfaceError::Timeout` is transient; the frame is skipped, the next frame proceeds.
- OOM — `wgpu::SurfaceError::OutOfMemory` is fatal at the engine level (`RenderError::OutOfMemory` flagged `is_fatal() = true`); the caller decides whether to retry on a smaller surface or terminate.
- Shader compilation failure — currently propagates as `RenderError::ShaderError`. Mythos plan keeps this; the failure surface is at `Renderer::new` (setup-phase), not per-frame.
- BackdropFilter with no `OffscreenRenderer` and no `COPY_SRC` surface support — falls back to rendering children without the filter (logged at `warn`). Not an error; documented degradation.

---

## 2. Architecture Overview

```text
flui-app (frame loop)
  │  renderer.render_scene(&scene)
  ▼
Renderer                          ◄── single owner of wgpu::Instance / Adapter /
  │  Device / Queue / Surface / SurfaceConfig + WgpuPainter + OffscreenRenderer
  │
  │  damage_tracker.has_damage()? → skip frame
  │  surface.get_current_texture() → SurfaceTexture
  │  device.create_command_encoder(Clear)
  │  queue.submit(clear_encoder)
  │
  │  scene.root() => layer_id
  ▼
render_layer_recursive(tree, layer_id, &mut Backend, &OffscreenRenderer)
  │  depth-first walk; LayerRender::render → children → LayerRender::cleanup
  │
  ▼
Backend                           ◄── per-frame; holds WgpuPainter + borrowed
  │  CommandRenderer impl          OffscreenRenderer + cached offscreen WgpuPainter
  │
  │  with_transform: save → translate/rotate/scale → draw → restore
  │
  ▼
WgpuPainter (concrete)            ◄── batched GPU recording; instancing,
  │  rect / circle / arc / path / text /     tessellation, text via glyphon
  │  gradient / clip / save_layer / restore_layer
  ▼
queue.submit(encoder)
output.present()
```

No `Painter` trait. No `wgpu::Compositor`. No `wgpu::Scene`. No `Box<dyn Backend>`. No `Arc<Mutex<OffscreenRenderer>>`. No `vulkan.rs` / `dx12.rs` / `metal.rs` capability stubs.

**What goes away from current code (≈ -6,200 LOC):**

| Path | LOC | Reason |
|---|---|---|
| `wgpu/scene.rs` | 1,820 | dead scene-graph parallel to `flui_layer::Scene` |
| `wgpu/compositor.rs` | 365 | dead `Compositor`/`TransformStack` over dead `LayerBatch` |
| `wgpu/vulkan.rs` | 826 | platform capability stub; zero callers |
| `wgpu/dx12.rs` | 769 | platform capability stub; zero callers |
| `wgpu/metal.rs` | 587 | platform capability stub; zero callers |
| `wgpu/external_texture_registry.rs` | 315 | dead PlatformView infra |
| `wgpu/texture_cache.rs` | 1,000 | dead (texture_pool is the real cache) |
| `wgpu/path_cache.rs` | 336 | dead |
| `wgpu/multi_draw.rs` | 304 | dead; collides with `flui_painting::DrawCommand` name |
| `wgpu/commands.rs` (shim) | 6 | re-export shim, delete + fix imports |
| `utils/text.rs` (`VectorTextRenderer`) | 802 | dead vector-text experiment |
| `utils/mod.rs` (single re-export) | 7 | deleted with utils/ |
| `pub trait Painter` in `traits.rs` | ~380 | 1 production impl; delete |

**What earns its place (and what gets restructured):**

- `lib.rs` — re-exports, prelude. Trimmed (no more `Scene`/`SceneBuilder` re-exports from `wgpu` since those types disappear).
- `error.rs` — `RenderError` / `RenderResult`. Already clean. Drop `PainterError(String)` variant once the trait is deleted.
- `traits.rs` → renamed/split. `CommandRenderer` stays in `command_renderer.rs` (or merged into `dispatch.rs` next to `dispatch_command`). `Painter` trait deleted.
- `commands.rs` — `dispatch_command` / `dispatch_commands`. Stays at crate root.
- `wgpu/mod.rs` — module list + re-exports. Trimmed.
- `wgpu/renderer.rs` (977 LOC) — `Renderer`, `GpuCapabilities`. Stays after `OffscreenRenderer` ownership refactor.
- `wgpu/backend.rs` (1,199 LOC) — `Backend` + `CommandRenderer` impl. Big but each method is a small bridge into `WgpuPainter`. Trimmed via `with_transform` macro (potential follow-up).
- `wgpu/painter.rs` (3,772 LOC) — `WgpuPainter`. Largest file by far. Splits into directory: `painter/{batch, segment, layer, gradient, text, render}.rs`. Internal cleanup.
- `wgpu/layer_render.rs` (1,191 LOC) — `LayerRender<R>` trait + per-variant impls. The superellipse path cache (thread_local + RefCell) stays as-is — single-threaded hot-path cache, canonical Rust pattern, not a refusal-trigger violation. The 521 LOC of MockRenderer test fixture extracts to `tests/layer_render.rs`.
- `wgpu/offscreen.rs` (1,525 LOC) — `OffscreenRenderer`, blur/morph/mask pipelines. Stays after Arc<Mutex> removal. Splits into directory: `offscreen/{mask, blur, morph}.rs`.
- `wgpu/tessellator.rs` (1,320 LOC) — `Tessellator`. Stays.
- `wgpu/text.rs` (436 LOC) — `TextRenderer` (glyphon-based). Stays.
- `wgpu/text_renderer.rs` (297 LOC) — `TextRenderingSystem`. Investigate vs `text.rs`; possible duplicate.
- `wgpu/pipelines.rs` (372 LOC) — `PipelineBuilder`, `PipelineCache`. Stays; consumed by `painter.rs`.
- `wgpu/pipeline.rs` (316 LOC) — pipeline keys, single-pipeline definitions. Stays after `#[allow(dead_code)]` audit.
- `wgpu/effects.rs` (543 LOC) — gradient instance types, shadow params, blur params. Used by `painter.rs`. Stays; remove `#[allow(dead_code)]`.
- `wgpu/instancing.rs` (701 LOC) — `RectInstance`, `CircleInstance`, etc. Used by `painter.rs`. Stays; remove `#[allow(dead_code)]`.
- `wgpu/shader_compiler.rs` (608 LOC) — `ShaderCache`, `ShaderType`. Used by `offscreen.rs`. Stays; remove `#[allow(dead_code)]`.
- `wgpu/buffers.rs`, `wgpu/buffer_pool.rs`, `wgpu/texture_pool.rs` — GPU buffer/texture pools. Used; stay (after `Arc<Mutex<TexturePoolInner>>` refactor on the last one).
- `wgpu/atlas.rs`, `wgpu/font_loader.rs` — used; stay.
- `wgpu/vertex.rs` — vertex types; used; stays.
- `wgpu/debug.rs` — `DebugBackend` (debug-builds only). Used; stays.
- `wgpu/occlusion.rs` — `OcclusionTracker`. Used by `Renderer`; stays.
- `wgpu/shaders/` — shader source files; stays.

---

## 3. Core Types

```rust
// ───────────────────────────────────────────────────────────────
// Renderer — single owner; concrete type
// ───────────────────────────────────────────────────────────────

#[allow(missing_debug_implementations)]   // wgpu types don't impl Debug
pub struct Renderer {
    // ── wgpu plumbing (setup-phase, immutable after init) ──
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    device: Arc<wgpu::Device>,           // Arc per wgpu convention; not lock-protected
    queue: Arc<wgpu::Queue>,             // Arc per wgpu convention
    capabilities: GpuCapabilities,

    // ── surface (optional for offscreen renderer) ──
    surface: Option<wgpu::Surface<'static>>,
    config: Option<wgpu::SurfaceConfiguration>,
    supports_copy_src: bool,

    // ── per-frame state owned directly, no locks ──
    painter: WgpuPainter,                // owned; was Option<…> + take()/restore dance
    offscreen: Option<OffscreenRenderer>,// owned; was Arc<Mutex<OffscreenRenderer>>
    damage_tracker: flui_layer::DamageTracker,
    occlusion: OcclusionTracker,
}

// ───────────────────────────────────────────────────────────────
// Backend — per-frame visitor; concrete type
// ───────────────────────────────────────────────────────────────

#[allow(missing_debug_implementations)]
pub struct Backend<'a> {
    painter: &'a mut WgpuPainter,
    offscreen: Option<&'a mut OffscreenRenderer>,
    offscreen_painter_cache: Option<WgpuPainter>,  // cached cross-frame; resized on demand
}

impl<'a> Backend<'a> {
    pub fn new(painter: &'a mut WgpuPainter) -> Self { … }
    pub fn with_offscreen(
        painter: &'a mut WgpuPainter,
        offscreen: &'a mut OffscreenRenderer,
    ) -> Self { … }
    // … CommandRenderer impl below
}

impl<'a> CommandRenderer for Backend<'a> { … }   // ~30 methods, each a small bridge

// ───────────────────────────────────────────────────────────────
// WgpuPainter — concrete; no longer behind Painter trait
// ───────────────────────────────────────────────────────────────

#[allow(missing_debug_implementations)]
pub struct WgpuPainter {
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    surface_format: wgpu::TextureFormat,
    size: (u32, u32),
    // ── batched recording state ──
    draw_order: Vec<DrawItem>,
    current_segment: DrawSegment,
    // ── save/restore stack ──
    transform_stack: Vec<Transform>,
    clip_stack: Vec<ClipRect>,
    opacity_stack: Vec<f32>,
    // ── shared subsystems (cheap Arcs from wgpu) ──
    pipelines: PipelineCache,
    tessellator: Tessellator,
    text: TextRenderer,
}

impl WgpuPainter { /* ~40 methods, all concrete */ }

// ───────────────────────────────────────────────────────────────
// OffscreenRenderer — single mutable owner (now on Renderer)
// ───────────────────────────────────────────────────────────────

#[allow(missing_debug_implementations)]
pub struct OffscreenRenderer {
    texture_pool: TexturePool,           // was Arc<TexturePool>; now owned
    shader_cache: ShaderCache,           // was Arc<ShaderCache>; now owned
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    surface_format: wgpu::TextureFormat,
    pipelines: HashMap<ShaderType, wgpu::RenderPipeline>,  // was Arc<wgpu::RenderPipeline>
    bind_group_layout: wgpu::BindGroupLayout,
    blur_bind_group_layout: wgpu::BindGroupLayout,
    blur_pipelines: Option<BlurPipelines>,
    morph_pipelines: Option<MorphPipelines>,
}

// ───────────────────────────────────────────────────────────────
// TexturePool — single owner; explicit release
// ───────────────────────────────────────────────────────────────

pub struct TexturePool {
    device: Arc<wgpu::Device>,
    available: Vec<TextureSlot>,         // direct ownership; no inner Arc<Mutex>
    max_pool_size: usize,
    next_id: u64,
}

pub struct PooledTexture {
    id: u64,
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    descriptor: TextureDesc,
    // No back-reference to pool; release is explicit:
    //   pool.release(pooled)
    // Drop just drops the wgpu::Texture (which releases GPU memory if not pooled).
}

impl TexturePool {
    pub fn acquire(&mut self, w: u32, h: u32, format: wgpu::TextureFormat) -> PooledTexture { … }
    pub fn release(&mut self, texture: PooledTexture) { … }
}

// ───────────────────────────────────────────────────────────────
// CommandRenderer — closed visitor trait (kept; 1 prod impl + 1 test mock)
// ───────────────────────────────────────────────────────────────

pub trait CommandRenderer {
    // ~30 methods on Rect/RRect/Circle/Oval/Line/Path/Arc/DRRect/Points/Text/Image/Texture
    // + render_shadow / render_shader_mask / render_gradient / render_color / render_paint
    // + clip_rect / clip_rrect / clip_path / save_layer / restore_layer
    // + push_clip_rect / push_clip_rrect / push_clip_path / pop_clip
    // + push_offset / push_transform / pop_transform / push_opacity / pop_opacity
    // + push_color_filter / pop_color_filter / push_image_filter / pop_image_filter
    // + add_performance_overlay
    // + viewport_bounds
}

// ───────────────────────────────────────────────────────────────
// LayerRender<R> — closed extension trait per Layer variant
// ───────────────────────────────────────────────────────────────

pub trait LayerRender<R: CommandRenderer + ?Sized> {
    fn render(&self, renderer: &mut R);
    fn cleanup(&self, renderer: &mut R);
}

impl<R: CommandRenderer + ?Sized> LayerRender<R> for flui_layer::Layer { /* 19-arm match */ }
impl<R: CommandRenderer + ?Sized> LayerRender<R> for flui_layer::CanvasLayer { … }
// … 18 more impls

// ───────────────────────────────────────────────────────────────
// Errors — already mostly correct; one variant deleted
// ───────────────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum RenderError {
    #[error("Surface was lost")]               SurfaceLost,
    #[error("Surface is outdated")]            SurfaceOutdated,
    #[error("Surface acquisition timed out")]  Timeout,
    #[error("Out of GPU memory")]              OutOfMemory,
    #[error("Failed to create resource: {0}")] ResourceCreation(String),
    #[error("Failed to create surface: {0}")]  SurfaceCreation(#[source] Box<dyn Error + Send + Sync>),
    #[error("No suitable GPU adapter found")]  NoAdapter,
    #[error("Failed to create GPU device: {0}")] DeviceCreation(#[source] Box<dyn Error + Send + Sync>),
    #[error("Shader error: {0}")]              ShaderError(String),
    #[error("Pipeline error: {0}")]            PipelineError(String),
    #[error("Invalid state: {0}")]             InvalidState(String),
    #[error("Renderer not initialized")]       NotInitialized,
    // PainterError(String) — DELETED (Painter trait gone)
}

pub type RenderResult<T> = Result<T, RenderError>;
```

---

## 4. State Machine

The engine has one explicit frame phase machine:

```text
Renderer::new() async
  │
  ▼
Initialized              ◄── wgpu::Device / Queue / Surface / Painter / Offscreen ready
  │
  │ renderer.resize(w, h)             ┐
  │ renderer.mark_dirty(rect)         │  in-place mutations; same state
  │ renderer.mark_full_repaint()      │
  │ renderer.reconfigure_surface()    ┘
  │
  │ renderer.render_scene(&scene)
  ▼
RenderingFrame           ◄── transient (one call); not a held state
  │
  │ damage check → skip if no damage → return Ok(())
  │
  │ surface.get_current_texture()
  │   │
  │   ├─ Ok(output) ──────────────────────────────────────┐
  │   ├─ Err(Lost | Outdated) → reconfigure_surface() → retry
  │   ├─ Err(Timeout) → return Err(Timeout)
  │   ├─ Err(OutOfMemory) → return Err(OutOfMemory)
  │   └─ Err(other) → return Err(SurfaceLost)              │
  │                                                        │
  │  Clear pass → submit                                   │
  │  scene.root() {                                        │
  │    Backend::new(&mut painter, &mut offscreen)          │
  │    render_layer_recursive(...) {                       │
  │      LayerRender::render → children → cleanup          │
  │    }                                                   │
  │    Final flush → submit                                │
  │  }                                                     │
  │  output.present()                                      │
  │  damage_tracker.reset()                                │
  │                                                        │
  ▼                                                        ▼
Initialized (next frame)                              return Ok(())
```

No phase typestate — the state is implicit in `Renderer`'s method set. The render_scene call is atomic from the caller's view (it either succeeds and presents, or returns an error). There is no "between frames, paused" state worth modelling as a separate type.

A separate sub-state machine inside `WgpuPainter::save_layer` / `restore_layer` is implicit in the `DrawItem` enum (`Segment` / `OffscreenTexture` / `OpacityLayer`). That's already correctly typed as a closed enum; not a Mythos target.

---

## 5. Public API

After the cut, the public surface is:

```rust
// === Crate root re-exports ===
pub use commands::{dispatch_command, dispatch_commands};
pub use error::{RenderError, RenderResult};
pub use traits::CommandRenderer;
// PROD: pub use flui_layer::{CanvasLayer, Layer, LayerId, LayerTree, …}; for caller convenience

// === wgpu backend ===
#[cfg(feature = "wgpu-backend")]
pub use wgpu::{
    Backend,
    FontLoader,
    GpuCapabilities,
    LayerRender,
    OffscreenRenderer,
    Renderer,
    WgpuPainter,
};
#[cfg(all(feature = "wgpu-backend", debug_assertions))]
pub use wgpu::DebugBackend;
```

**What disappears from today's public surface:**

| Today | Disposition |
|---|---|
| `Painter` trait | Deleted (1 prod impl, no second backend planned) |
| `Scene`, `SceneBuilder` (from `wgpu::scene`) | Deleted (collides with `flui_layer::Scene`; dead code) |
| `Compositor`, `RenderContext`, `TransformStack` | Deleted (dead; `WgpuPainter` already maintains stack) |
| `PipelineManager`, `MaskedRenderResult` | Internal to `OffscreenRenderer`; not re-exported |
| `BufferManager`, `DynamicBuffer`, `BufferPool`, `BufferPoolStats` | Internal to `WgpuPainter`; not re-exported |
| `TextureAtlas`, `AtlasEntry`, `AtlasRect` | Internal to `WgpuPainter`; not re-exported |
| `MultiDrawBatcher`, `DrawCommand`, `DrawIndexedIndirectArgs`, `MultiDrawStats`, `PipelineId` | Deleted (dead, name collision) |
| `OffscreenRenderer`, `PipelineManager` | Stays as `OffscreenRenderer`; `PipelineManager` was an alias, dropped |
| `PipelineBuilder`, `PipelineCache` | Internal; not re-exported |
| `ShaderCache`, `ShaderType` | Internal; not re-exported |
| `TextRenderingSystem`, `TextRun` | Investigated for duplication with `TextRenderer`; one kept, the other deleted |
| `TexturePool`, `PooledTexture`, `GpuTexture`, `PoolStats`, `TextureDesc` | Internal; not re-exported |
| `ExternalTextureEntry`, `ExternalTextureRegistry` | Deleted (dead) |
| `Tessellator` | Internal; not re-exported |
| `ImageInstance`, `PathVertex`, `RectInstance`, `RectVertex`, `Vertex` | Internal; not re-exported |

Trim from ~50 re-exports to ~10. External callers (`flui-app` only) use `Renderer` + `RenderError`. Everything else is internal scaffolding that today leaks into the public surface "in case someone needs it."

---

## 6. Internal Modules

```text
crates/flui-engine/src/
  lib.rs                   — re-exports, prelude (trimmed)
  error.rs                 — RenderError + RenderResult (PainterError variant removed)
  commands.rs              — dispatch_command + dispatch_commands
  traits.rs                — CommandRenderer trait ONLY (Painter trait deleted)
                               (rename candidate: command_renderer.rs)
  wgpu/
    mod.rs                 — module list + re-exports (trimmed)
    renderer.rs            — Renderer + GpuCapabilities (post-Arc-Mutex refactor)
    backend.rs             — Backend<'a> + CommandRenderer impl (lifetime-borrowed)
    painter/               — WgpuPainter (split from 3,772 LOC)
      mod.rs               — pub struct + pub API
      batch.rs             — DrawSegment + ScissorRegion + TessellatedBatch
      layer.rs             — SavedLayer / PendingOpacityLayer / save_layer / restore_layer
      gradient.rs          — gradient_rect / radial_gradient_rect / sweep_gradient_rect
      text.rs              — text drawing methods on WgpuPainter
      render.rs            — render() entry point + per-segment GPU submission
    layer_render.rs        — LayerRender<R> trait + 19 impls (production)
    offscreen/             — OffscreenRenderer (split from 1,525 LOC)
      mod.rs               — pub struct + pub API
      mask.rs              — render_masked
      blur.rs              — render_blur (Dual Kawase)
      morph.rs             — dilate / erode pipelines
    tessellator.rs         — Tessellator + IntoLyonPath
    text.rs                — TextRenderer (glyphon)
    text_renderer.rs       — INVESTIGATE: TextRenderingSystem / TextRun
                               (duplicate with text.rs? Either merge or delete)
    pipelines.rs           — PipelineBuilder + PipelineCache
    pipeline.rs            — PipelineKey + single-pipeline descriptors
    effects.rs             — GradientStop + Linear/Radial/Sweep instances
                               + ShadowParams / BlurParams / BlurIntensity
    instancing.rs          — InstanceBatch + Rect/Circle/Arc/Shadow/Gradient/TextureInstance
    shader_compiler.rs     — ShaderCache + ShaderType (used by offscreen)
    buffers.rs             — BufferManager + DynamicBuffer
    buffer_pool.rs         — BufferPool + BufferPoolStats
    texture_pool.rs        — TexturePool + PooledTexture (post-Arc-Mutex refactor)
    atlas.rs               — TextureAtlas (used by WgpuPainter)
    font_loader.rs         — FontLoader (used by text.rs)
    vertex.rs              — Vertex types
    debug.rs               — DebugBackend (debug-builds only)
    occlusion.rs           — OcclusionTracker (used by Renderer)
    shaders/               — WGSL shader sources
      mod.rs
      *.wgsl
  tests/
    layer_render.rs        — extracted MockRenderer + LayerRender tests (~521 LOC)
    backend.rs             — Backend / CommandRenderer integration tests (if extracted)
```

**Deleted:**

```text
  utils/                   — DELETED entirely (VectorTextRenderer dead)
  wgpu/scene.rs            — DELETED (1,820 LOC; collides with flui_layer::Scene)
  wgpu/compositor.rs       — DELETED (365 LOC; dead Compositor/TransformStack)
  wgpu/commands.rs         — DELETED (6 LOC re-export shim)
  wgpu/vulkan.rs           — DELETED (826 LOC; zero callers)
  wgpu/dx12.rs             — DELETED (769 LOC; zero callers)
  wgpu/metal.rs            — DELETED (587 LOC; zero callers)
  wgpu/external_texture_registry.rs — DELETED (315 LOC)
  wgpu/texture_cache.rs    — DELETED (1,000 LOC; texture_pool is the real cache)
  wgpu/path_cache.rs       — DELETED (336 LOC; zero callers)
  wgpu/multi_draw.rs       — DELETED (304 LOC; dead + name collision)
```

**Note on `painter/` split.** The `painter.rs` file at 3,772 LOC is the largest file in the workspace. Splitting it is mechanical but careful — moving a method into a sibling file means re-opening `impl WgpuPainter` blocks. The split is **proposed**, not mandatory: Mythos Step 10 evaluates whether the split lands in this chain or is deferred via an "Outstanding refactor" entry. If deferred, the split is in `ARCHITECTURE.md` `## Outstanding refactors` with the named concrete blocker "no semantic change, just LOC; defer for review-clarity if chain is already large".

---

## 7. Async & Failure Semantics

**Task ownership.** Zero on the hot path. `Renderer::new` and `Renderer::new_offscreen` are async; their futures are owned by the caller (`flui-app`) and completed before `Renderer` is used. The renderer itself has no tasks, no `tokio::spawn`, no `JoinHandle`.

**Cancellation.** Not applicable. Calling `render_scene` is atomic from the caller's view (success and present, or `Err(_)` and no present). Mid-frame cancellation does not exist as a concept; if the application drops the renderer mid-frame, the wgpu encoder is dropped (unsubmitted; safe), the surface texture is dropped (returned to the pool unpresented), and the next frame starts fresh.

**Retry.** `SurfaceError::{Lost, Outdated}` is auto-retried once inside `render_scene` via `reconfigure_surface()`. If the second attempt also fails, the error propagates to the caller. `SurfaceError::Timeout` is reported but not retried; the caller decides whether to skip the frame or escalate.

**Idempotency.** `render_scene(&scene)` is idempotent if called with the same scene and no intervening state mutation — the GPU work submitted is identical. `mark_dirty(rect)` and `mark_full_repaint()` are idempotent (union semantics). `resize(w, h)` is idempotent if called with the same `(w, h)`.

**Backpressure.** Implicit, handled inside wgpu's `Surface::get_current_texture()`. `present_mode = Fifo` (vsync) blocks the next frame until the previous one is displayed (60Hz target). `present_mode = Mailbox` allows the GPU to drop a frame if the renderer outpaces the display. The engine itself has no buffer / queue / channel.

**Shutdown.** `Renderer::drop` releases `wgpu::Surface` (returns surface texture to pool), drops `wgpu::Device` + `wgpu::Queue` (wgpu's `Arc` ref count decrements; the device actually releases when all Arc clones are dropped — which the `WgpuPainter` and `OffscreenRenderer` may still hold transiently). The cleanup order is: surface → painter → offscreen → device/queue Arc-decrement. No explicit `shutdown` method needed; `Drop` is sufficient.

**Partial failure recovery.** A `BackdropFilterLayer` with no offscreen renderer or no `COPY_SRC` support degrades to no-filter (logged at `warn!`). A panicking `LayerRender::render` impl propagates up; there is **no** `catch_unwind` wrapping the layer walk (unlike `flui-layer`'s `Scene::fire_composition_callbacks`) because a layer-rendering panic indicates a bug in the engine itself, not a third-party callback. Mythos plan considers adding `catch_unwind` around the whole `render_scene` body as a single panic boundary — but the existing tests prove the per-layer renderers don't panic in production. **Deferred** unless a real-world panic surfaces.

**Two-phase commits.** Not needed; the GPU command encoder is the unit of atomicity. Either `queue.submit(encoder)` runs to completion (one frame submitted) or it doesn't (no frame submitted; surface texture dropped unpresented). There is no "partially submitted frame" state.

---

## 8. Security Model

`flui-engine` is a GPU-API binding library, not a service. Trust boundaries:

**Trusted inputs.**
- `flui_layer::Scene` — produced by `flui-rendering`'s paint phase. The `Layer` enum is closed; all 19 variants are crate-defined. The `Scene` is `Send`, moved into `render_scene` by reference (`&Scene`). Layer payload data (clip rects, transforms, picture display lists) is trusted because it transits the same workspace and is validated at the painting / layer boundaries.
- `DrawCommand` — produced by `flui-painting::Canvas` recording. Closed 30-variant enum; validated at recording time.
- `Path` (from `flui-types::painting::Path`) — validated at construction; the engine assumes finite coordinates.
- `Image` (from `flui-types::painting::Image`) — texture bytes already uploaded to GPU memory by the painting crate; engine just dispatches draw calls referencing them.

**Untrusted inputs.**
- GPU shader source code — currently all WGSL is crate-internal (`shaders/*.wgsl`). No runtime shader compilation from user input. If a future custom-shader feature lands, validate at the boundary; today this is not a concern.
- Window handle (`raw_window_handle::HasWindowHandle`) — caller-provided. The single `unsafe` block in `Renderer::new` calls `instance.create_surface_unsafe(SurfaceTargetUnsafe::from_window(window))` — the safety contract is "the handle must remain valid for the surface's lifetime." Documented at the call site; honoured by `flui-app` which owns the winit window.

**Capabilities.** The engine mediates GPU access. It does not store credentials, secrets, or persistent user data. GPU buffers and textures are wiped on drop (wgpu handles this).

**Secret handling.** Not applicable. `Renderer::Debug` is intentionally not derived (wgpu types don't implement Debug; the suppression is `#[allow(missing_debug_implementations)]`). No layer or paint configuration is logged at info level; tracing spans use IDs and sizes, not content.

**Logging rules.** Use `tracing` (already wired; the crate is a `tracing` dep). Spans on `Renderer::render_scene`, `Backend::render_shader_mask`, `Renderer::handle_backdrop_filter`. No `println!`/`eprintln!` anywhere in the engine.

**Serialization.** `Renderer` is not `Serialize`/`Deserialize`. `RenderError` is `Debug` + `Display` for log output; not serialized to network.

**Plugin/user input rules.** No plugin surface — `CommandRenderer` is a `pub trait` but the only production impl is `Backend`, the only test impl is `MockRenderer` (in `tests/layer_render.rs`). External crates may implement `CommandRenderer` (e.g. to capture commands for testing) but cannot extend the engine's GPU pipeline because `WgpuPainter` is concrete. The trust boundary is the closed `Layer` and `DrawCommand` enums; external impls of `CommandRenderer` see the same vocabulary the engine sees.

---

## 9. Data-Oriented Notes

**Hot data.** Touched every frame:
- `Layer` enum payload (per node in scene walk) — ~64 bytes typical, ~200 bytes worst case. Closed enum, in-place storage on `LayerNode`. No heap indirection in the variant dispatch.
- `DrawCommand` (per recorded paint op) — closed enum, ~80 bytes typical. Stored in `DisplayList::commands: Vec<DrawCommand>` on `flui-painting`'s side; the engine iterates by reference.
- `DrawSegment::rect_batch` / `circle_batch` / `arc_batch` / etc. on `WgpuPainter` — `Vec<RectInstance>` style instance arrays. Per-frame; capacity preallocated to 1024 (rect/circle/arc) and 512 (gradients) in `DrawSegment::new`.
- `TessellatedBatch` vec for path-tessellated geometry — variable size; bounded by complex-path count per frame.
- Transform/clip/opacity stacks on `WgpuPainter` — typical depth 4-8; `Vec<Transform>` etc. with small allocations.
- `OcclusionTracker::opaque_regions` — `Vec<Rect>` of registered opaque regions for the current frame; bounded by leaf-opaque-layer count.
- `DamageTracker::regions` — `Vec<Rect<Pixels>>` of dirty rectangles; bounded by per-frame dirty count.

**Cold data.**
- `GpuCapabilities` — read once at setup; rarely accessed per-frame.
- `wgpu::SurfaceConfiguration` — read on resize; static otherwise.
- `OffscreenRenderer::pipelines` HashMap — populated on first use of each pipeline; read once per shader-mask / blur invocation.
- `ShaderCache` — same; populated on demand.
- `TexturePool::available` slots — read on `acquire`; written on `release`.

**Allocation strategy.**
- `DrawSegment` preallocates instance batches to 1024 / 512 capacity in `new()`. Each frame's `WgpuPainter` reuses the same allocations (segments are cleared, not deallocated, after submit). This is correct.
- `Vec<DrawItem>` for `draw_order` is variable per frame; resets between frames.
- `OffscreenRenderer::texture_pool` reuses textures across frames via the pool. Today via `Arc<Mutex<>>`; post-refactor via direct `&mut TexturePool`.
- No `Arc::clone` per draw call. Today there are per-frame `Arc::clone(&device)` at `renderer.rs:636-637` (building `RenderContext`) and `backend.rs:121-122` (caching offscreen painter); both are eliminable. Post-refactor, `Arc<wgpu::Device>` is cloned exactly twice (once for `WgpuPainter::new`, once for `OffscreenRenderer::new`) at `Renderer::new` time; never per-frame.

**Forbidden allocations.**
- No `Box<dyn CommandRenderer>` storage. The visitor is generic; `Backend` is concrete.
- No `HashMap` lookup on the per-layer dispatch (`LayerRender` is a `match`).
- No `Arc<Layer>` clones — `Layer` is owned by `LayerNode`, accessed by `&Layer`.
- No per-frame `Arc::clone` on `wgpu::Device` / `wgpu::Queue` (the per-frame clones in `renderer.rs` and `backend.rs` are removed in Mythos Step 5).

**Cache locality.**
- `DrawSegment` instance batches are contiguous `Vec<T>`; iteration order matches GPU consumption order.
- `LayerNode` in `Slab<LayerNode>` is contiguous (per `flui-layer`); depth-first walk has good cache behaviour for typical UI trees (depth ~5-15, fan-out ~1-8).
- `OcclusionTracker::opaque_regions` is a `Vec<Rect>`; `is_occluded` is O(n) scan with small n (~16 typical). Fine.

**Where `Arc`/`Mutex`/`HashMap`/`Box`/`dyn Trait` are acceptable.**
- `Arc<wgpu::Device>` / `Arc<wgpu::Queue>` — wgpu's own API uses Arc-shaped handles; cheap ref-count clones; not lock-protected. Allowed.
- `HashMap<ShaderType, wgpu::RenderPipeline>` in `OffscreenRenderer` — setup-phase; populated on first use of each pipeline; not on the hot path.
- `HashMap<PipelineKey, wgpu::RenderPipeline>` in `PipelineCache` — same.

**Where they are forbidden.**
- `Arc<Mutex<Renderer>>` — never. Single mutable owner.
- `Arc<Mutex<OffscreenRenderer>>` — **today: yes**. Post-Mythos: no.
- `Arc<Mutex<TexturePoolInner>>` — **today: yes**. Post-Mythos: no.
- `Box<dyn Backend>` — never. Backend is concrete.
- `Box<dyn Painter>` — never. Painter trait is deleted.
- `RwLock<Box<dyn CommandRenderer>>` — never.
- `Arc::clone` inside `render_scene`, `render_layer_recursive`, `Backend::render_*`, `WgpuPainter::*` — never.

---

## 10. Error Model

```rust
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum RenderError {
    // ── Surface lifecycle ──
    #[error("Surface was lost")]
    SurfaceLost,

    #[error("Surface is outdated")]
    SurfaceOutdated,

    #[error("Surface acquisition timed out")]
    Timeout,

    // ── GPU resource exhaustion ──
    #[error("Out of GPU memory")]
    OutOfMemory,

    #[error("Failed to create resource: {0}")]
    ResourceCreation(String),

    // ── Initialization ──
    #[error("Failed to create surface: {0}")]
    SurfaceCreation(#[source] Box<dyn Error + Send + Sync>),

    #[error("No suitable GPU adapter found")]
    NoAdapter,

    #[error("Failed to create GPU device: {0}")]
    DeviceCreation(#[source] Box<dyn Error + Send + Sync>),

    // ── Rendering ──
    #[error("Shader error: {0}")]
    ShaderError(String),

    #[error("Pipeline error: {0}")]
    PipelineError(String),

    // ── State / lifecycle ──
    #[error("Invalid state: {0}")]
    InvalidState(String),

    #[error("Renderer not initialized")]
    NotInitialized,
    // Note: PainterError(String) — DELETED in Mythos Step 7 with the Painter trait.
}

pub type RenderResult<T> = Result<T, RenderError>;
```

**Retryable** — `SurfaceLost`, `SurfaceOutdated`, `Timeout`. Auto-retried (`SurfaceLost`/`SurfaceOutdated`) or surfaced for caller decision (`Timeout`).

**Terminal for this frame** — `OutOfMemory` (`is_fatal() = true`). Caller decides whether to reduce surface size and retry or terminate the app.

**User-facing** — `NoAdapter`, `DeviceCreation`, `SurfaceCreation` — setup-phase failures, caller surfaces them as "this device cannot run the application" UI.

**Internal only** — `ResourceCreation`, `ShaderError`, `PipelineError`, `InvalidState`, `NotInitialized`. These indicate engine bugs or wgpu issues, not user error.

**Security-sensitive** — none. The engine does not handle credentials.

`anyhow::Error` is **not** returned from this crate's public API after Mythos cleanup. Today the `Renderer::new` and `Renderer::new_offscreen` use `anyhow::Result<Self>`; Mythos plan migrates these to `RenderResult<Self>` for consistency.

**Today's panic paths** — `wgpu/compositor.rs:48: panic!("Cannot pop identity transform from stack")`. The whole `compositor.rs` is being deleted, so this panic disappears with the file.

---

## 11. Tests Required

Each test must prove a design guarantee.

**Invariants on `Renderer`.**
- `Renderer::new(&window).await.unwrap()` produces a renderer with `surface.is_some()`.
- `Renderer::new_offscreen().await.unwrap()` produces a renderer with `surface.is_none()`.
- `Renderer::resize(w, h)` updates `surface_config` and resets `damage_tracker` to full repaint.
- `Renderer::render_scene(&empty_scene)` returns `Ok(())` and does not present.
- `Renderer::render_scene(&scene_with_content)` after `mark_full_repaint()` presents a frame and resets damage.
- `Renderer::render_scene` after `SurfaceLost` auto-reconfigures and retries.

**Invariants on `Backend`.**
- `Backend::new(&mut painter)` constructs a renderer; `CommandRenderer::viewport_bounds()` matches `painter.viewport_bounds()`.
- `Backend::render_rect` with identity transform calls `painter.rect` directly (no save/restore).
- `Backend::render_rect` with non-identity transform wraps in save/translate/draw/restore.
- `Backend::render_shader_mask` with no offscreen renderer falls back to rendering children without mask (logged at warn).
- `Backend::push_clip_rect` increments `painter.save_count`; `pop_clip` decrements it.

**Invariants on `WgpuPainter`.**
- `painter.save()` + `painter.translate(...)` + `painter.restore()` leaves transform stack unchanged.
- `painter.rect(...)` appends to `current_segment.rect_batch`.
- `painter.queue_offscreen_result(tex, bounds)` ends the current segment and appends an `OffscreenTexture` `DrawItem`.
- `painter.save_layer(Some(bounds), &paint)` starts a new layer; `restore_layer()` ends it.
- `painter.render(&view, &mut encoder)` submits draw calls in `draw_order` order.

**Invariants on `LayerRender<R>` (already exercised by 16 unit tests in `layer_render.rs:655-1191`).**
- Each `Layer` variant calls the expected `CommandRenderer` methods in render order.
- Each variant's `cleanup` undoes its `render` (push/pop balance).
- Identity transforms / zero offsets / opaque opacity / no-clip layers are no-ops (render and cleanup are empty).
- `Layer::BackdropFilter::render` is a no-op (handled at `Renderer` level).
- The 19-variant match in `impl LayerRender for Layer` dispatches correctly to each concrete impl.

**Phase invariants.** None (the engine has no phase typestate; the implicit state machine is enforced by method visibility).

**Cancellation.** Not applicable.

**Retry / idempotency.**
- `render_scene(&scene)` is idempotent given the same scene and no intervening state.
- `mark_dirty(rect)` followed by `mark_dirty(same_rect)` is a no-op (union semantics).
- `reconfigure_surface()` is idempotent.

**Authorization.** Not applicable.

**Malformed input.**
- `render_scene(&scene)` with `scene.has_content() == false` returns `Ok(())` without presenting.
- `render_scene(&scene)` with `scene.root() == None` short-circuits the layer walk.
- `resize(0, 0)` is a no-op.

**Concurrency.**
- `Renderer: !Send`? **TBD** — `wgpu::Surface<'static>` and `Arc<wgpu::Device>` are `Send` per wgpu's design. The renderer should be `Send` for cross-thread frame production. **Compile-test** via `fn assert_send<T: Send>()`.
- No loom test needed; the engine has no concurrent state after the `Arc<Mutex<OffscreenRenderer>>` removal.

**Property tests.** Deferred (mirrors `flui-rendering` / `flui-layer` deferral). Possible classes:
- For any `Scene` with N layers of mixed types, `render_scene` returns `Ok(())` (if surface available).
- For any sequence of `(save, translate, save, scale, restore, restore)` calls, the transform stack is empty at the end.

**Loom tests.** Not applicable (no concurrent state).

**Miri tests.** Run `cargo +nightly miri test -p flui-engine` to verify the `RefCell<HashMap<SuperellipseKey, Path>>` thread-local cache and the `Arc<wgpu::Device>` reference patterns. The `unsafe { instance.create_surface_unsafe(...) }` block in `Renderer::new` interacts with platform GPU drivers; miri can't run that (no GPU). Test it via integration tests on real platforms instead.

**Integration tests.**
- End-to-end offscreen render: build a small scene, call `Renderer::new_offscreen().await`, `render_scene(&scene)`, verify the frame produces non-zero pixels in the expected region. **Note:** requires GPU; skipped in CI without `--features enable-wgpu-tests`.
- Damage tracking: `render_scene(empty)` after `mark_dirty()` skips frame; verify via mock counter.
- Occlusion culling: build a scene with one opaque rect covering a smaller subtree; verify the inner subtree is not rendered (mock backend records dispatch calls).
- Resize flow: resize triggers `mark_full_repaint`; next `render_scene` renders.

---

## 12. Rejected Designs

For each rejected design: what it was, why it was tempting, why it is wrong here.

### `Box<dyn Backend>` / `Box<dyn Painter>` plugin trait for multiple GPU backends

**What:** Keep `Painter` trait + add a `Backend` trait, then store both behind `Box<dyn Painter>` / `Box<dyn Backend>` to enable Skia / Vello / software backends.

**Why tempting:** The crate's docstring says "Future: `skia-backend`, `vello-backend`, `software-backend`" and the trait machinery exists. Removing it feels like burning a bridge.

**Why wrong:** No second backend exists today. No second backend is in any plan in the repo. The trait's 30+ methods with 6 default `tracing::warn!("not implemented")` impls have already accumulated. If a second backend lands in three years, that backend will need to (a) provide an alternative `LayerRender` impl, which `LayerRender<R: CommandRenderer>` already supports via generic dispatch, OR (b) provide a different `Renderer` type at the call-site boundary in `flui-app`. Either path is cheaper than dragging a fake-abstraction `Painter` trait through every iteration. The boundary is already at `CommandRenderer` (closed visitor, 1 prod impl + 1 test mock); that's the boundary worth keeping.

### `Arc<RwLock<Renderer>>` shared across the frame producer + render thread

**What:** Wrap `Renderer` in `Arc<RwLock<>>` so the application can build display lists on a worker thread while the render thread submits frames.

**Why tempting:** Concurrent frame production sounds modern. Could allow CPU-side work (layout, paint) to overlap with GPU submission.

**Why wrong:** wgpu's `Surface` is `Send` but not `Sync`; multiple threads cannot present from the same surface simultaneously. The frame production / GPU submission split is a `Scene` value-move (built on one thread, sent to the render thread), exactly the shape the `flui-layer` Mythos chain established. The `Arc<RwLock<>>` would add lock contention to the hot path with no benefit. Today's single-threaded `render_scene` is correct.

### Keep `wgpu/scene.rs` as "the engine's internal scene representation, distinct from `flui-layer::Scene`"

**What:** Argue that `wgpu/scene.rs`'s `Scene` / `Layer` / `Primitive` is an intermediate IR — a flat list of primitives the engine reorders for batching — while `flui-layer::Scene` is the source tree. Keep both.

**Why tempting:** The 1,820 LOC includes batching logic (`batch_primitives`, `batch_with_context`). Sounds reusable.

**Why wrong:** The wgpu Scene type has zero production callers. The `WgpuPainter` already does instance batching internally (`DrawSegment::{rect_batch, circle_batch, …}`). Adding a second batching layer above would mean either (a) two batching algorithms competing or (b) the upper one rewriting the lower's output. Neither shape exists today. The current code is "an internal IR that nobody emits into and nobody renders from." Delete.

### Keep `Compositor` / `TransformStack` / `RenderContext` from `wgpu/compositor.rs` as "future hooks"

**What:** Keep the `Compositor` struct's `begin_layer` / `end_layer` API for "future compositor frameworks."

**Why tempting:** Stacks of transforms / opacity / blend modes are a recognised compositor pattern.

**Why wrong:** `WgpuPainter` already maintains `transform_stack`, `clip_stack`, `opacity_stack` via `save`/`restore`. The `Compositor` struct duplicates the data. `WgpuPainter::save()` and `WgpuPainter::restore()` are how `Backend::with_transform` already drives the transform stack. The `Compositor` struct exists in parallel and is consumed by zero production code. The `LayerBatch` type that `Compositor` consumes is itself dead. If a future "compositor framework" lands, it will sit above the engine in `flui-rendering` (operating on `flui_layer::Scene`) — not below `WgpuPainter` duplicating the painter's own stacks.

### Keep `vulkan.rs` / `dx12.rs` / `metal.rs` for "platform-specific GPU features"

**What:** Keep the three platform-specific files (`VulkanFeatures` / `Dx12Features` / `MetalFxUpscaler` / `EdrConfig` / `AutoHdrConfig` / `PipelineCacheConfig`). Argument: documentation of what each platform supports; reference for when we wire HDR or pipeline caching.

**Why tempting:** Three years of "we should support Vulkan extensions properly" documented in code form. Hard to delete.

**Why wrong:** wgpu's own `wgpu::Adapter::features()` returns a `wgpu::Features` bitset that lists exactly what is available on this adapter. `wgpu::Adapter::limits()` returns limits. `wgpu::Adapter::get_info()` returns vendor + backend type. The three platform files duplicate this introspection in hand-written Rust, then never call any of it. The "documentation value" is non-zero but lives in dead code; replace with a single `docs/GPU_CAPABILITIES.md` that points readers at wgpu's docs. Deleting the three files saves 2,182 LOC and removes a "I'll wire this up later" footprint that's been festering since the engine's first commit.

### Keep `pub trait Painter` for "backend-agnostic high-level drawing APIs"

**What:** Keep the `Painter` trait so third parties can implement custom painters (e.g., a printer driver that takes `Painter` calls and produces PDF).

**Why tempting:** The trait has 30+ methods with sensible defaults; the abstraction sounds clean.

**Why wrong:** Six of the default impls print `tracing::warn!("Painter::draw_path: not implemented")`. That's not an abstraction; that's a noop trait pretending to be a contract. The single production impl (`WgpuPainter`) has all 30+ methods specialised; deleting the trait means `WgpuPainter` becomes the concrete type, callers (`Backend`) call `painter.rect(...)` instead of `<WgpuPainter as Painter>::rect(painter, ...)`. Zero ergonomic loss, one less indirection. The "PDF painter" use case is hypothetical; if it lands, build a `PdfPainter` type with a `pdf` method on `Renderer` — don't carry a trait for it now.

### Keep `wgpu/texture_cache.rs` (1,000 LOC) distinct from `wgpu/texture_pool.rs` (523 LOC)

**What:** Two separate texture management modules. Texture cache is "by content hash, persistent" while pool is "by descriptor, recycled per frame."

**Why tempting:** Different concerns deserve different abstractions.

**Why wrong:** `texture_cache.rs` has zero callers. Its 1,000 LOC implements a hash-based content cache for textures that the engine never asks for (the engine takes `Image` from `flui-painting`, which has its own caching). `texture_pool.rs` is the real consumer-facing texture lifecycle, used by `OffscreenRenderer` and `WgpuPainter`. Delete the dead one.

### Keep `wgpu/multi_draw.rs` for "future indirect-draw batching"

**What:** The module defines `MultiDrawBatcher`, `DrawIndexedIndirectArgs`, and a `DrawCommand` type used by indirect-draw paths. Argument: GPUs support `vkCmdDrawIndexedIndirect` / `glMultiDrawElementsIndirect`; we'll wire it up eventually.

**Why tempting:** Indirect draws are a real GPU optimisation. Once you have many small batches that share a pipeline, indirect-draw with a buffer of arguments is cheaper than thousands of individual draws.

**Why wrong:** Zero callers. The local `DrawCommand` type also collides with `flui_painting::DrawCommand` (different namespace, but using both in the same crate is hostile). If indirect-draw becomes needed, it's a localised optimisation to a single rendering pass; rebuilding from scratch with a clear performance target is faster than reviving 304 LOC of generic infrastructure.

### Keep `pub mod effects` warning at `wgpu/mod.rs`

**What:** The mod.rs has `#[allow(dead_code)] pub mod effects;`. The allow was added because clippy / `cargo check` flagged unused items.

**Why tempting:** Just a comment; doesn't hurt anything.

**Why wrong:** The `#[allow(dead_code)]` is the canary telling us we don't know which items in `effects.rs` are actually consumed. The Mythos audit confirmed `GradientStop`, `LinearGradientInstance`, `RadialGradientInstance`, `SweepGradientInstance` are used by `painter.rs`. `ShadowInstance`, `BlurParams`, `BlurIntensity`, `LinearGradientBuilder`, `ShadowParams` are not used by `painter.rs` directly — confirm and either remove the unused half or document their consumers. **Audit, don't allow.**

### Use `enum_dispatch` for `LayerRender` instead of macro-collapsed hand-written impls

**What:** Replace the 19 `impl LayerRender for FooLayer { … }` blocks with a single `enum_dispatch` annotation that auto-generates the 19-arm enum match.

**Why tempting:** Eliminates 600+ LOC of mechanical boilerplate in the `impl LayerRender for Layer` arm-by-arm dispatch.

**Why wrong:** Same reasoning as `flui-layer`'s rejection. New proc-macro dep for a small win. The current code is hand-readable; the macro form would be opaque. If the boilerplate grows, a local `macro_rules!` matches the style established in `flui-layer/src/layer/dispatch.rs`. Don't add a dep.

### Wrap `render_scene` in `catch_unwind` to recover from layer panics

**What:** Wrap the whole `render_scene` body in `std::panic::catch_unwind(AssertUnwindSafe(|| ...))` so a panicking `LayerRender::render` impl doesn't take down the frame.

**Why tempting:** Defence-in-depth. The `flui-layer` chain's `Scene::fire_composition_callbacks` wraps composition callbacks in `catch_unwind`; this is the same idea.

**Why wrong:** Composition callbacks in `flui-layer` are third-party closures (user-registered). Layer rendering in `flui-engine` is crate-internal — every `LayerRender` impl is in this crate, tested, has 16 unit tests. If a layer panics, it's an engine bug; the right response is to fix the bug, not mask it. The `catch_unwind` wrapper would introduce `AssertUnwindSafe` ceremony around `Backend` mutable state with no proof of benefit. Deferred unless a real-world panic surfaces. **Tracked in `## Outstanding refactors` as forward-looking.**

---

## 13. Implementation Plan

Ordered. Each step lands as a reviewable commit. Each step compiles, `cargo test -p flui-engine --lib` green, `bash scripts/port-check.sh` green or no commit.

### Step 1 — Delete dead module `utils/` + `VectorTextRenderer`

- Delete `crates/flui-engine/src/utils/text.rs` (802 LOC) and `crates/flui-engine/src/utils/mod.rs` (7 LOC).
- Delete the `pub mod utils;` declaration in `lib.rs`.
- Verify zero external callers (`grep -r "VectorTextRenderer\|utils::text" crates/` returns nothing post-delete).

**Verifies:** `cargo build --workspace` clean.

### Step 2 — Delete dead modules: `wgpu/scene.rs` + `wgpu/compositor.rs`

- Delete `crates/flui-engine/src/wgpu/scene.rs` (1,820 LOC) and `crates/flui-engine/src/wgpu/compositor.rs` (365 LOC).
- Remove from `wgpu/mod.rs`: `mod scene;`, `mod compositor;`, `pub use scene::{Scene, SceneBuilder}`, `pub use compositor::{Compositor, RenderContext, TransformStack}`.
- Verify zero external callers (only internal cross-reference: `compositor.rs` consumed scene.rs types; both gone simultaneously).

**Verifies:** `cargo build --workspace` clean. The crate's `Scene` re-export now resolves only to `flui_layer::Scene`; no collision.

### Step 3 — Delete platform stubs: `wgpu/vulkan.rs` + `wgpu/dx12.rs` + `wgpu/metal.rs`

- Delete `crates/flui-engine/src/wgpu/vulkan.rs` (826 LOC), `crates/flui-engine/src/wgpu/dx12.rs` (769 LOC), `crates/flui-engine/src/wgpu/metal.rs` (587 LOC).
- Remove from `wgpu/mod.rs`: `#[cfg(...)] pub mod vulkan;`, `#[cfg(...)] pub mod dx12;`, `#[cfg(...)] pub mod metal;`.
- Verify zero external callers.

**Verifies:** Per-platform `cargo build --target <platform>` clean.

### Step 4 — Delete dead caches/registries: `wgpu/{external_texture_registry, texture_cache, path_cache, multi_draw, commands}`

- Delete `wgpu/external_texture_registry.rs` (315 LOC), `wgpu/texture_cache.rs` (1,000 LOC), `wgpu/path_cache.rs` (336 LOC), `wgpu/multi_draw.rs` (304 LOC), `wgpu/commands.rs` (6 LOC re-export shim).
- Remove their `mod` declarations and re-exports from `wgpu/mod.rs`.
- Fix `wgpu/layer_render.rs:17` import: change `use super::commands::{CommandRenderer, dispatch_commands};` → `use crate::{commands::dispatch_commands, traits::CommandRenderer};`.

**Verifies:** `cargo build --workspace` clean.

### Step 5 — Delete `Painter` trait

- Delete `pub trait Painter` from `crates/flui-engine/src/traits.rs` (~380 LOC).
- Remove `pub use traits::Painter` from `lib.rs` and `wgpu/mod.rs`.
- Rename `traits.rs` → `command_renderer.rs` for clarity.
- Update `wgpu/painter.rs`: remove `impl Painter for WgpuPainter` block (~30 method impls); methods stay as inherent impls on `WgpuPainter` (they already exist that way; the trait impl wrapped them).
- Update `wgpu/backend.rs`: remove `use crate::traits::{CommandRenderer, Painter}` → `use crate::traits::CommandRenderer`.
- Delete `RenderError::PainterError(String)` variant + `RenderError::painter()` constructor.

**Verifies:** `cargo build --workspace` clean; `cargo test -p flui-engine` green.

### Step 6 — Replace `Arc<parking_lot::Mutex<OffscreenRenderer>>` with direct ownership

- Modify `crates/flui-engine/src/wgpu/renderer.rs`:
  - `offscreen: Option<Arc<parking_lot::Mutex<OffscreenRenderer>>>` → `offscreen: Option<OffscreenRenderer>`.
  - Remove `Arc::new(parking_lot::Mutex::new(offscreen))` wrap at construction.
  - `Renderer::handle_backdrop_filter`: replace `offscreen_arc.lock()` calls with `&mut self.offscreen.as_mut().unwrap()`.
- Modify `crates/flui-engine/src/wgpu/backend.rs`:
  - `Backend::offscreen: Option<Arc<parking_lot::Mutex<OffscreenRenderer>>>` → `offscreen: Option<&'a mut OffscreenRenderer>` (lifetime `'a` introduced; `Backend<'a>` becomes generic).
  - `Backend::new` and `Backend::with_offscreen` take `&'a mut WgpuPainter` + `&'a mut OffscreenRenderer`.
  - `Backend::offscreen()` and `Backend::render_shader_mask`: replace `.lock()` calls with `&mut self.offscreen`.
- Update callers in `Renderer::render_scene`: pass `&mut self.painter`, `&mut self.offscreen` to `Backend::new` / `with_offscreen`.
- Verify the borrow checker accepts the new lifetimes; if rectifying any borrow-checker conflict requires splitting `Renderer::render_layer_recursive` arguments, that split lands in this step.

**Verifies:** `cargo build --workspace` clean; `cargo test -p flui-engine` green.

### Step 7 — Eliminate per-frame `Arc::clone(&device)` / `Arc::clone(&queue)`

- Modify `crates/flui-engine/src/wgpu/renderer.rs`:
  - `RenderContext` (inside `render_scene`): replace `device: Arc::clone(&self.device)` with `device: &self.device`. Adjust struct definition to hold `&'frame wgpu::Device` / `&'frame wgpu::Queue` references with a lifetime tied to the frame.
- Modify `crates/flui-engine/src/wgpu/backend.rs`:
  - `Backend::get_or_create_offscreen_painter`: take `device: &Arc<wgpu::Device>` parameter (already `&Arc<>`); the inner `Arc::clone(device)` calls at `backend.rs:121-122` to `WgpuPainter::with_shared_device` stay because `WgpuPainter` does need to own its `Arc<wgpu::Device>` for lifetime independence. **Acceptable per-frame trade-off**: cloning when the cached painter is being initialised on first use; the cached painter is reused across frames, so the cost amortises to "twice per app lifetime."
- Modify `crates/flui-engine/src/wgpu/painter.rs:866`: same — initialisation-time clone, acceptable.
- Modify `crates/flui-engine/src/wgpu/offscreen.rs:93, 279, 659-660, 1051-1053`: setup-phase clones; document via comment that these are setup-time, not per-frame.

**Verifies:** `bash scripts/port-check.sh` Trigger 5 stays clean for `flui-engine` paths (no per-frame `Arc::clone` in production).

### Step 8 — Replace `Arc<Mutex<TexturePoolInner>>` with explicit release

- Modify `crates/flui-engine/src/wgpu/texture_pool.rs`:
  - `TexturePool::pool: Arc<Mutex<TexturePoolInner>>` → `TexturePool::available: Vec<TextureSlot>` (direct ownership).
  - `PooledTexture` no longer holds `inner: Arc<Mutex<TexturePoolInner>>`; drop becomes a no-op (or drops the `wgpu::Texture` directly).
  - Add `TexturePool::release(&mut self, texture: PooledTexture)` for explicit return.
  - Acquisition: `TexturePool::acquire(&mut self, w, h, format) -> PooledTexture`.
- Update callers in `OffscreenRenderer`:
  - `texture_pool.acquire` already takes `&self`; change to `&mut self`.
  - Wherever a `PooledTexture` is dropped, replace with `self.texture_pool.release(tex)`.
- Verify the `OffscreenRenderer::with_caches` constructor signature (today: `texture_pool: Arc<TexturePool>`); replace `Arc` with owned `TexturePool`.

**Verifies:** `cargo test -p flui-engine` green; verify no `Arc::clone(&texture_pool)` calls remain.

### Step 9 — Migrate `Renderer::new` / `new_offscreen` return type from `anyhow::Result` to `RenderResult`

- Modify `crates/flui-engine/src/wgpu/renderer.rs`:
  - `pub async fn new<W>(window: &W) -> Result<Self>` → `pub async fn new<W>(window: &W) -> RenderResult<Self>`.
  - `pub async fn new_offscreen() -> Result<Self>` → `pub async fn new_offscreen() -> RenderResult<Self>`.
  - Convert wgpu errors to `RenderError` via existing `surface_creation` / `device_creation` constructors.
  - Convert `request_adapter` failure (`anyhow::anyhow!(...)`) to `RenderError::NoAdapter`.
- Update callers in `flui-app/src/app/{binding, direct, runner}.rs`: replace `anyhow::Error` handling with `RenderError`.
- Verify no `anyhow::Result` remains in `flui-engine`'s public API.

**Verifies:** `cargo build --workspace` clean.

### Step 10 — Audit `#[allow(dead_code)]` markers; remove or justify

- For each `#[allow(dead_code)]` (today on `wgpu::effects`, `wgpu::instancing`, `wgpu::pipeline`, `wgpu::shader_compiler`, plus the global `#![allow(dead_code)]` at `lib.rs:4`):
  - Run `cargo check --workspace` after removing the allow.
  - For items that fail the check: either delete (if truly unused) or document the consumer (if used in a feature-gated path or test path).
- Specifically:
  - `effects.rs`: `ShadowInstance`, `ShadowParams`, `BlurParams`, `BlurIntensity`, `LinearGradientBuilder` — find their consumers; if none, delete.
  - `instancing.rs`: verify every `*Instance` type is consumed by `painter.rs`.
  - `pipeline.rs`: verify `PipelineKey` etc. are consumed.
  - `shader_compiler.rs`: verify `ShaderCache` is consumed by `offscreen.rs`.
- Remove the global `#![allow(dead_code)]` at `lib.rs:4` once per-module allows are resolved.

**Verifies:** `cargo build --workspace` clean without dead-code suppressions.

### Step 11 — Investigate `text_renderer.rs` vs `text.rs` duplication

- Read `crates/flui-engine/src/wgpu/text.rs` (436 LOC) and `crates/flui-engine/src/wgpu/text_renderer.rs` (297 LOC).
- Determine: are they duplicates? Are they two-stage text (text.rs = recording, text_renderer.rs = rendering)? Is one feature-gated?
- If duplicate: delete one, document the choice in ARCHITECTURE.md.
- If complementary: rename for clarity (`text/{recording, rendering}.rs` directory split) or document why both exist.

**Verifies:** `cargo test -p flui-engine` green; text functionality unchanged.

### Step 12 — `crates/flui-engine/ARCHITECTURE.md`

Create the per-crate template instance:
- `## wgpu / Vulkan / Metal mapping` (N/A for "Flutter source mapping"; the engine has no Flutter parity. Document the wgpu API surface the engine consumes.).
- `## Mapping decisions` — Accepted trade-offs for: deletion of `Painter` trait (vs keep for future backends); deletion of `wgpu/scene.rs` (vs keep as internal IR); deletion of platform capability files (vs keep as documentation); `OffscreenRenderer` direct ownership (vs `Arc<Mutex<>>`); `TexturePool` explicit release (vs `Arc<Mutex<>>`); closed `LayerRender<R>` static dispatch (vs `Box<dyn Backend>` plugin trait).
- `## Thread safety` — table: `Renderer` Send (not Sync); `WgpuPainter` Send; `OffscreenRenderer` Send; `Arc<wgpu::Device>` / `Arc<wgpu::Queue>` Send+Sync (wgpu convention); no other locks anywhere.
- `## Friction log` — anything not yet refactored. Candidates: `painter.rs` 3,772 LOC un-split (if Step 13's split is deferred); `text_renderer.rs` vs `text.rs` if both kept; the `unsafe { instance.create_surface_unsafe(...) }` block at `renderer.rs:189-192`.
- `## Outstanding refactors` — `painter/` directory split (if deferred); `catch_unwind` around `render_scene` if real panics emerge; second backend abstraction if Skia/Vello/software actually lands.

Flip `docs/PORT.md` `## Index` row for `flui-engine`: "Not yet templated" → "Templated 2026-05-20 (Mythos chain)".

**Verifies:** `crates/flui-engine/ARCHITECTURE.md` exists; `docs/PORT.md` row flipped.

### Step 13 — Extend `scripts/port-check.sh`

- Trigger 1 (`RwLock<Box<dyn ...>>`): add `crates/flui-engine/src` to the path scope; extend the regex to also match `wgpu::Device|wgpu::Queue|wgpu::Surface` storage shapes if a future refactor wraps them in `RwLock`. **Today: zero current violations.**
- Trigger 2 (`Box<dyn>` wrapped in interior-mutability): add `crates/flui-engine/src` to path scope.
- Trigger 3 (`async fn` on hot path): add `crates/flui-engine/src` to path scope; extend verb set to include `submit|present|render_scene|render_layer_recursive`. **Whitelist:** `wgpu/renderer.rs` `new` and `new_offscreen` are async at the edge — but they don't match the verb set, so no whitelist needed.
- Trigger 5 (`Arc::clone` in per-frame loop): the `flui-layer` chain already extended this to `crates/flui-engine/src/wgpu/layer_render.rs`. Extend to also include `crates/flui-engine/src/wgpu/renderer.rs::render_scene` and `crates/flui-engine/src/wgpu/backend.rs::render_*` methods. **Today: per-frame Arc::clone sites are eliminated in Mythos Step 7; check stays clean.**
- New Trigger 7 (forward-looking) — `Arc<Mutex<>>` or `Arc<RwLock<>>` on a struct field in `crates/flui-engine/src/wgpu/`. Forward-looking: catches regressions where the `OffscreenRenderer` / `TexturePool` shape returns. Pattern: `(Arc<(parking_lot::)?(Mutex|RwLock)<\s*\w*Renderer|Arc<(parking_lot::)?(Mutex|RwLock)<\s*\w*Pool)`. Constrained to `crates/flui-engine/src/wgpu/`.

Run `bash scripts/port-check.sh -v` and verify all triggers stay clean.

**Verifies:** `bash scripts/port-check.sh -v` exits 0; seven (six original + one new) "ok" lines.

### Step 14 — Final verification + clippy + workspace tests

- Run `cargo test --workspace`. All tests pass.
- Run `cargo clippy --workspace -- -D warnings`. No new warnings.
- Run `bash scripts/port-check.sh -v`. All triggers clean.
- Run `cargo build --workspace --all-features`. Clean.
- Address any drift surfaced (e.g., unused imports after deletions, dead-code lints uncovered by removing `#![allow(dead_code)]`).
- Flip `docs/plans/2026-05-20-NNN-feat-flui-engine-mythos-redesign-plan.md` `status:` to `completed`.

**Verifies:** workspace fully green; PR mergeable.

---

## Self-check

- **Did I start from data, not traits?** Yes. `Renderer` is concrete. `WgpuPainter` becomes concrete. `Backend` is concrete. The only traits that survive are `CommandRenderer` (closed visitor for `DrawCommand`, 1 prod impl + 1 test mock) and `LayerRender<R>` (closed extension over the `Layer` enum, 1 production R type + 1 test R type).
- **Did every module earn its existence?** 11 modules slated for deletion (≥ 6,000 LOC). 5 modules kept-but-restructured (Arc-Mutex removal in OffscreenRenderer, TexturePool, Renderer, Backend; potential split of painter.rs and offscreen.rs).
- **Did I identify the state owner?** Yes. `Renderer` owns everything; per-frame `Backend<'a>` borrows mutably.
- **Did I define cancellation behavior?** Yes. Not applicable per design; render_scene is atomic from caller's view.
- **Did I define trust boundaries?** Yes. Closed `Layer` enum (`flui-layer`-owned) and closed `DrawCommand` enum (`flui-painting`-owned), both matched exhaustively. `CommandRenderer` trait is a closed visitor with 1 prod impl.
- **Did I avoid fake extensibility?** Yes. `Painter` trait (1 prod impl, 6 default `not implemented` warnings) is deleted. The "multi-backend" docstring promise is dropped from the crate-level docs.
- **Did I avoid Quick Win architecture?** The plan executes 14 steps including dead-code deletion (six dead modules, two dead caches, one dead trait), the `Arc<Mutex<OffscreenRenderer>>` removal (touches `Backend` + `Renderer` + lifetimes), the `Arc<Mutex<TexturePoolInner>>` removal (touches every `PooledTexture` consumer), the `anyhow::Result` → `RenderResult` migration (ripples into `flui-app`), and methodology extension. Breaking ripples in `flui-app` land in-band per the no-quick-wins rule.
- **Did I encode invariants in types where possible?** Yes. `Backend<'a>` borrows lifetimes-tied, enforcing single-frame ownership. `Renderer` direct ownership of `OffscreenRenderer` enforces single-mutator at compile time. The closed `Layer` / `DrawCommand` enums enforce exhaustive dispatch at the match site.
- **Did I reject bad alternatives?** Ten rejected designs documented in §12.
- **Could a Rust developer implement this design without guessing?** Yes, given §13 (14-step plan) + §3 (concrete type sketches) + §6 (module layout).
