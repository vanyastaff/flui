# flui-engine Architecture

This document is the per-crate template instance for `flui-engine` as defined by [`docs/PORT.md`](../../docs/PORT.md). It records the wgpu / Vulkan / Metal API surfaces the crate consumes, the divergence decisions taken during the Mythos chain (PR opened 2026-05-20, commit chain on `feat/flui-engine-mythos-redesign`), the current thread-safety surface, the known friction not yet refactored, and the planned cleanups that the methodology will pick up next.

The deeper Mythos design verdict lives at [`docs/designs/2026-05-20-mythos-flui-engine-redesign.md`](../../docs/designs/2026-05-20-mythos-flui-engine-redesign.md). The implementation plan lives at [`docs/plans/2026-05-20-003-feat-flui-engine-mythos-redesign-plan.md`](../../docs/plans/2026-05-20-003-feat-flui-engine-mythos-redesign-plan.md). The requirements brainstorm lives at [`docs/brainstorms/flui-engine-mythos-redesign-requirements.md`](../../docs/brainstorms/flui-engine-mythos-redesign-requirements.md).

---

## wgpu / Vulkan / Metal mapping

`flui-engine` is **not** a Flutter port. There is no Dart `Engine` class to point at, and no Skia equivalent because wgpu replaces the Skia layer entirely. The crate is a Rust-native GPU lowering layer that consumes `flui_layer::Scene` from `flui-layer` and submits draw calls via wgpu. The `## Flutter source mapping` section from the per-crate template specification is replaced here with this `## wgpu / Vulkan / Metal mapping` section per the precedent set in [`docs/PORT.md`](../../docs/PORT.md) `## Mapping rules` "Multi-source references" clause.

The relevant external APIs the crate consumes:

| FLUI module | wgpu API surface | Notes |
|---|---|---|
| [`src/wgpu/renderer.rs`](src/wgpu/renderer.rs) `Renderer` | `wgpu::Instance`, `wgpu::Adapter`, `wgpu::Device`, `wgpu::Queue`, `wgpu::Surface<'static>`, `wgpu::SurfaceConfiguration`, `wgpu::TextureFormat`, `wgpu::PresentMode`, `wgpu::SurfaceError`, `wgpu::CommandEncoderDescriptor`, `wgpu::RenderPassDescriptor`, `wgpu::CompositeAlphaMode` | The single owner of all per-window GPU state. Backend selection per platform: Metal (macOS/iOS) / DX12 (Windows) / Vulkan (Linux/Android) / WebGPU+GL (Web). |
| [`src/wgpu/painter.rs`](src/wgpu/painter.rs) `WgpuPainter` | `wgpu::Buffer`, `wgpu::RenderPipeline`, `wgpu::BindGroup`, `wgpu::ShaderModule`, `wgpu::Texture`, `wgpu::TextureView`, `wgpu::Sampler`, `wgpu::ShaderSource::Wgsl`, `wgpu::RenderPassColorAttachment`, `wgpu::LoadOp`, `wgpu::StoreOp` | Batched recording + per-frame submission. Uses instancing (`RectInstance`, `CircleInstance`, `ArcInstance`, `TextureInstance`) for fast axis-aligned primitives; falls back to `lyon`-tessellated paths for arbitrary geometry. |
| [`src/wgpu/backend.rs`](src/wgpu/backend.rs) `Backend` | -- | Visitor over `flui_painting::DrawCommand`; implements `CommandRenderer`; bridges per-command to `WgpuPainter` inherent methods. |
| [`src/wgpu/layer_render.rs`](src/wgpu/layer_render.rs) `LayerRender<R>` | -- | Closed extension trait per `flui_layer::Layer` variant. 19 impls. Static dispatch via generic `R: CommandRenderer + ?Sized`. |
| [`src/commands.rs`](src/commands.rs) `dispatch_command` | -- | Closed visitor over the ~30-variant `flui_painting::DrawCommand` enum. Static dispatch via generic `R: CommandRenderer + ?Sized`. |
| [`src/wgpu/offscreen.rs`](src/wgpu/offscreen.rs) `OffscreenRenderer` | `wgpu::RenderPipeline`, `wgpu::BindGroupLayout`, `wgpu::BindGroup`, `wgpu::Sampler` | Offscreen-texture pipelines for `ShaderMaskLayer` (compose with mask shader) and `BackdropFilterLayer` (Dual-Kawase blur). |
| [`src/wgpu/shader_compiler.rs`](src/wgpu/shader_compiler.rs) `ShaderCache` | `wgpu::ShaderModule`, `wgpu::ShaderSource` | Caches compiled WGSL modules per `ShaderType` enum (Solid/LinearGradient/RadialGradient mask shaders; BlurHorizontal/Vertical/Downsample/Upsample; MorphDilate/Erode). |
| [`src/wgpu/pipelines.rs`](src/wgpu/pipelines.rs) `PipelineCache` + `PipelineBuilder` | `wgpu::RenderPipelineDescriptor`, `wgpu::VertexBufferLayout`, `wgpu::ColorTargetState`, `wgpu::BlendState`, `wgpu::DepthStencilState` | Caches pipelines per `PipelineKey` (paint-style + blend-mode + format). |
| [`src/wgpu/texture_pool.rs`](src/wgpu/texture_pool.rs) `TexturePool` | `wgpu::TextureDescriptor`, `wgpu::TextureUsages` | Per-frame texture reuse for offscreen renders. Currently `Arc<Mutex<TexturePoolInner>>` -- Mythos friction; see [Outstanding refactors](#outstanding-refactors). |
| [`src/wgpu/tessellator.rs`](src/wgpu/tessellator.rs) `Tessellator` | -- | Adapter over `lyon::tessellation::FillTessellator` + `StrokeTessellator`. |
| [`src/wgpu/text.rs`](src/wgpu/text.rs) `TextRenderer` | -- | Adapter over `glyphon` (cosmic-text + glyph atlas + GPU sampling). |
| [`src/wgpu/occlusion.rs`](src/wgpu/occlusion.rs) `OcclusionTracker` | -- | Per-frame opaque-region tracker. Pure CPU; no GPU API surface. |

**Spec references:**
- wgpu API: [wgpu.rs documentation](https://docs.rs/wgpu) (workspace pin 25.x; see [`Cargo.toml`](../../Cargo.toml) `[workspace.dependencies]`).
- Vulkan spec: [Khronos Vulkan 1.4 Specification](https://registry.khronos.org/vulkan/specs/1.4/html/vkspec.html) -- consumed via wgpu's `vulkan` backend on Linux/Android.
- Metal spec: [Apple Metal 4 documentation](https://developer.apple.com/documentation/metal) -- consumed via wgpu's `metal` backend on macOS/iOS.
- DirectX 12: [Microsoft DirectX 12 Agility SDK](https://devblogs.microsoft.com/directx/directx12agility/) -- consumed via wgpu's `dx12` backend on Windows.
- WebGPU: [W3C WebGPU Specification](https://www.w3.org/TR/webgpu/) -- consumed via wgpu's `webgpu` backend on the Web.

---

## Mapping decisions

This section records places where the Rust shape diverges from the patterns the GPU APIs themselves suggest, or where the original `flui-engine` code shape diverged from the Mythos-cleaned shape. Each entry follows the "Accepted trade-offs" format established by [`docs/plans/2026-03-31-custom-render-callback-design.md`](../../docs/plans/2026-03-31-custom-render-callback-design.md).

### 1. Closed `LayerRender<R>` static dispatch, not `Box<dyn Backend>` plugin trait

**Rule:** [`docs/PORT.md`](../../docs/PORT.md) Mapping rule "Compile-time over runtime"; constitution Anti-Patterns ("Prefer generics and enum dispatch over `dyn` trait objects"); strategy clause "Behavior loyal to wgpu/Vulkan/Metal semantics, structure Rust-native."

**Choice:** `LayerRender<R: CommandRenderer + ?Sized>` is a closed extension trait with 19 impls (one per `flui_layer::Layer` variant) ([`src/wgpu/layer_render.rs`](src/wgpu/layer_render.rs)). Dispatch is static via generics; no `Box<dyn Layer>`, no `Box<dyn Backend>`, no vtable on the hot path. `CommandRenderer` itself has exactly **one production impl** (`Backend` in [`src/wgpu/backend.rs`](src/wgpu/backend.rs)) and **one test mock** (`MockRenderer` in `layer_render.rs:683-965`). The trait earns its existence via the test mock and via the static-dispatch generic boundary; a future second backend (Skia/Vello/software) would add a second impl, not displace the trait.

**Alternatives:**
- `Box<dyn Backend>` plugin trait for "multiple rendering backends without changing high-level code" -- rejected. No second backend exists or is planned in any document in the repo. Static dispatch + closed `CommandRenderer` already provides the abstraction `flui-rendering` needs.
- `enum_dispatch` crate to auto-generate the 19-arm `impl LayerRender for Layer` match -- rejected. New proc-macro dep for a small win; output identical; hand-readable match is preferred per the precedent set in `flui-layer` Mythos U4.

**Accepted trade-off:** Adding a 20th `Layer` variant is a coordinated change in `flui-layer` + `flui-engine` (the 19-arm match in `impl LayerRender for Layer` won't compile without the new arm). The Rust borrow-checker provides match-exhaustiveness checks at compile time; the trait object form would lose that guarantee. Mythos verdict §12 rejected designs #1, #9.

### 2. Deletion of `pub trait Painter`, not retain for future second backend

**Rule:** Strategy clause "Every `dyn`, every `Arc`, every `RwLock` must defend its existence in writing." Mythos verdict §12 rejected design #6.

**Choice:** `pub trait Painter` (~420 LOC at `traits.rs:380-780`, 30+ methods, 6 default impls printing `tracing::warn!("Painter::draw_path: not implemented")`) deleted entirely in Mythos U5 (commit `1b376beb`). `WgpuPainter`'s methods became inherent (no trait dispatch). The single existing `impl Painter for WgpuPainter` block (1,519 LOC) became `impl WgpuPainter`. The two `painter.text_styled(...)` call sites in `Backend` were inlined to `painter.text(...)` (the default `text_styled` impl was just `self.text(...)`). The `examples/painting_demo` had 14 `use flui_engine::Painter;` lines that were converted to comments noting the trait was deleted in U5 (function signatures already took the concrete `&mut flui_engine::WgpuPainter` type, so no functional change).

**Alternatives:**
- Retain `Painter` trait "for future Skia/Vello/software backends" -- rejected. No second backend exists or is planned. The trait's six default impls printing `tracing::warn!("not implemented")` proved the abstraction was empty.
- Retain `Painter` for "PDF painter" or other off-screen capture use cases -- rejected. If such a backend lands, it builds a `PdfPainter` type with a clear `Pdf` method on `Renderer`; today's trait carried no useful constraint.

**Accepted trade-off:** Future "we need a software fallback for headless CI" or "we need a Vello backend for production rendering" decisions require building the abstraction against a concrete second impl, not retrofitting to the hypothetical-only one. The cost of rebuilding from scratch when a real consumer arrives is lower than the cost of carrying a fake abstraction through every refactor in between (verdict §12 rejected design #6).

### 3. Deletion of `wgpu/scene.rs` parallel scene-graph + `wgpu/compositor.rs` duplicate save-stack

**Rule:** Mythos audit principle "every module must justify its existence with a production caller -- not a re-export, not a doc comment."

**Choice:** Delete `wgpu/scene.rs` (1,820 LOC defining `Scene`, `SceneBuilder`, `Layer`, `Primitive`, `LayerBatch`, `PrimitiveBatch`, `PrimitiveType`, `BlendMode`) and `wgpu/compositor.rs` (365 LOC defining `Compositor`, `TransformStack`, `RenderContext`) in Mythos U2 (commit `b04636cf`). The two files together formed a parallel scene-graph + compositing stack that had:

- Zero external callers in `crates/`, `examples/`.
- Re-exports from `wgpu/mod.rs` that name-collided with `flui_layer::Scene` and `flui_layer::SceneBuilder` (also re-exported at the engine crate root). Two `Scene` types in one crate's public API.
- An internal-only mutual dependency: `wgpu/compositor.rs` consumed the `LayerBatch` type from `wgpu/scene.rs` and was the dead module's only consumer.
- A `Compositor::begin_layer` / `end_layer` API duplicating `WgpuPainter::save`/`restore`'s transform stack + opacity stack + clip stack (which is the working stack consumed by `Backend::with_transform`).

**Alternatives:**
- Keep `wgpu/scene.rs` as "an intermediate IR -- a flat list of primitives the engine reorders for batching" -- rejected. `WgpuPainter` already does instance batching internally via `DrawSegment::{rect_batch, circle_batch, arc_batch, …}`. Adding a second batching layer above would either (a) leave both alive doing the same work, or (b) rewrite one in terms of the other. Neither shape existed; both layers were dead.
- Keep `wgpu/compositor.rs` as "future compositor framework hooks" -- rejected. The stacks it duplicated are the canonical `WgpuPainter` save/restore stacks. A future compositor framework would sit at a different boundary entirely.

**Accepted trade-off:** Verdict §12 rejected designs #3, #4. The deletion removed 2,185 LOC of dead architecture in one commit.

### 4. Deletion of platform-capability stubs (`vulkan.rs`, `dx12.rs`, `metal.rs`), wgpu's `Adapter::features()` already provides

**Rule:** Strategy clause "Don't re-implement what wgpu already exposes." Mythos verdict §12 rejected design #5.

**Choice:** Delete `wgpu/vulkan.rs` (826 LOC), `wgpu/dx12.rs` (769 LOC), `wgpu/metal.rs` (587 LOC) in Mythos U3 (commit `5c0e5696`). The three files reimplemented adapter introspection (`VulkanFeatures`, `PipelineCacheConfig`, `Dx12Features`, `AutoHdrConfig`, `MetalFxUpscaler`, `EdrConfig`) that wgpu's `Adapter::get_info()` / `Adapter::features()` / `Adapter::limits()` already provide.

The `GpuCapabilities` struct in `wgpu/renderer.rs` is the canonical capability surface; it uses `wgpu::Adapter::features()` directly for `supports_hdr` / `supports_push_constants` / `supports_bc_compression` / `supports_astc_compression` / `supports_etc2_compression` detection.

**Alternatives:**
- Keep the three files as "documentation of what each platform supports" -- rejected. The documentation value lived in dead code; replace with a single `docs/GPU_CAPABILITIES.md` if needed. The 2,182 LOC of stubs that never connected to a real call path is hostile to the next reader.

**Accepted trade-off:** Future HDR / EDR / WCG / MetalFX features will re-implement only what's needed against wgpu's actual capability surface, not 2,182 LOC of stubs. HDR support via `Rgba16Float` surface format is **not lost** -- `GpuCapabilities::supports_hdr` continues to detect it; the deleted `EdrConfig` was a configuration struct with no consumer.

### 5. `RenderResult<T>` consistency over `anyhow::Result<T>` on `Renderer` + `FontLoader`

**Rule:** Strategy clause "Consistent error model in the engine's public API."

**Choice:** Migrate `Renderer::new` / `Renderer::new_offscreen` / `FontLoader::load_file` / `FontLoader::load_directory` from `anyhow::Result<T>` to `RenderResult<T>` (Mythos U6 in commit log; verdict's U9, commit `8e6acb65`). Map wgpu errors to specific `RenderError` variants: `wgpu::SurfaceTargetUnsafe::from_window` and `wgpu::Instance::create_surface_unsafe` failures -> `RenderError::surface_creation`; `wgpu::Instance::request_adapter` failure -> `RenderError::AdapterRequest(#[source] Box<dyn Error + Send + Sync>)` (preserves the wgpu diagnostic payload via `Error::source()`); `wgpu::Adapter::request_device` failure -> `RenderError::device_creation`. filesystem errors in font_loader -> `RenderError::ResourceIo { context, #[source] source: std::io::Error }` (preserves `io::ErrorKind` for caller-side matching).

**Alternatives:**
- Keep `anyhow::Result` on `Renderer::new` "because it's simpler" -- rejected. Inconsistent with `RenderResult<T>` on every other engine API.
- Use the existing `RenderError::ResourceCreation(String)` variant via `format!("font load {path}: {e}")` for font_loader -- rejected during the chain's cleanup pass. The format-into-String shape severs `Error::source()` and loses `io::ErrorKind` discrimination. Added `RenderError::ResourceIo { context, #[source] source: io::Error }` instead.
- Keep `RenderError::NoAdapter` as a sentinel-only variant (discarding the wgpu diagnostic via `.map_err(|_| NoAdapter)`) -- rejected during the chain's cleanup pass. wgpu 25.x's `RequestAdapterError::NotFound` carries `active_backends` / `requested_backends` / `supported_backends` / `no_fallback_backends` / `no_adapter_backends` / `incompatible_surface_backends` operator-diagnostic fields; the original `.map_err(|_| NoAdapter)` flatten dropped them. Added `RenderError::AdapterRequest(#[source] Box<dyn Error + Send + Sync>)` to preserve the diagnostic.

**Accepted trade-off:** Verdict §12 rejected design #8. **No `flui-app` ripple was required** -- `RenderError: Error + Send + Sync` auto-converts to `anyhow::Error` via the blanket `From<E: Error + Send + Sync + 'static>` impl. The original plan's claim of "ripple into flui-app" was incorrect; verified by `cargo build -p flui-app` clean post-migration with zero caller-side changes. The migration is engine-internal.

### 5b. Deletion of global `#![allow(dead_code)]` + per-module audit

**Rule:** Mythos audit principle "every `#[allow(dead_code)]` is either a documented forward-looking consumer or an unaudited zombie."

**Choice:** Remove global `#![allow(dead_code)]` at `lib.rs:4`; only `#![allow(missing_debug_implementations)]` stays (wgpu's resource handles intentionally don't impl `Debug`). Per-module `#[allow(dead_code)]` retained with documentation on `effects`, `instancing`, `pipeline`, `shader_compiler` modules (forward-looking helpers with named eventual consumers in `painter.rs`'s public API; per-item audit tracked in Outstanding refactor "Per-item audit of dead helpers in `effects` / `instancing` / `pipeline` / `shader_compiler` modules"). Per-field allows added on `Renderer::{instance, adapter}` (wgpu keep-alive), `TextureAtlas::format` (queried via wgpu Texture, not field), `BufferManager::device` (Arc<Device> for buffer creation).

**Alternatives:**
- Keep global `#![allow(dead_code)]` "during active development" -- rejected. The global allow hides the per-item dead-code findings that the chain surfaced.
- Delete every per-module allow + every dead item the lint flags -- rejected as scope creep. The chain prioritised module-level cleanup over per-item churn; per-item audit deferred to Outstanding refactor with explicit per-item inventory.

**Accepted trade-off:** 17 dead-code warnings surfaced + addressed via deletion or per-item documentation in Mythos U7 (Step 7 in commit log; verdict's U10+U11, commit `5d51f35e`). Test count: 53 -> 48 (5 tests deleted alongside `text_renderer.rs`).

### 6. The single existing `unsafe` block at `Renderer::new` stays + gets a documented SAFETY comment

**Rule:** Mythos audit principle "every unsafe block must defend its existence in writing."

**Choice:** The single `unsafe { instance.create_surface_unsafe(...) }` block at `Renderer::new` is required by wgpu's API contract (`SurfaceTargetUnsafe::from_window` and `Instance::create_surface_unsafe` are both unsafe). The block was consolidated in Mythos U9 to cover both unsafe calls together with a single SAFETY comment naming the window-handle-lifetime invariant honoured by `flui-app` (which owns the winit window for the application's lifetime).

**Alternatives:**
- Split the two unsafe calls into separate blocks "for granular SAFETY documentation" -- rejected. The invariant is the same: the window handle must outlive the surface. One block, one comment, one rationale.

**Accepted trade-off:** Net unsafe delta for the chain: **0**. No new unsafe added; the existing unsafe was localised and documented.

### Net delta summary

| Mythos step | Net LOC delta | Net unsafe delta | Net `Arc<Mutex<>>` delta |
|---|---|---|---|
| U1 (delete `utils/`) | -809 | 0 | 0 |
| U2 (delete `wgpu/scene.rs` + `wgpu/compositor.rs`) | -2,185 | 0 | 0 |
| U3 (delete platform stubs) | -2,182 | 0 | 0 |
| U4 (delete `wgpu/commands.rs` shim) | -6 | 0 | 0 |
| U5 (delete `Painter` trait) | -492 | 0 | 0 |
| U9 (`anyhow::Result` -> `RenderResult`) | ~+20 | 0 | 0 |
| U10 + U11 (dead_code audit + `text_renderer.rs` deletion) | ~-330 | 0 | 0 |
| **Total** | **~-5,984** | **0** | **0** |

Per-frame `Arc::clone` removal (verdict's U7) and `Arc<Mutex<OffscreenRenderer>>` + `Arc<Mutex<TexturePoolInner>>` removal (verdict's U6 + U8) were **deferred** to follow-up; see [Outstanding refactors](#outstanding-refactors).

---

## Thread safety

`flui-engine` runs on the render thread; wgpu handles its own thread-safety via `Arc<Device>` / `Arc<Queue>` (cheap ref-counted handles, not lock-protected). Per strategy clause "sync hot path, async at edges," neither the layer walk nor the per-command dispatch is multi-threaded; `Renderer::render_scene` is sync. Async only at `Renderer::new` and `Renderer::new_offscreen` (wgpu's `request_adapter` and `request_device` are async at the wgpu boundary).

| Site | Primitive | Category | Notes |
|---|---|---|---|
| `Renderer::instance` ([`src/wgpu/renderer.rs`](src/wgpu/renderer.rs)) | `wgpu::Instance` | Owned, keep-alive | Single mutator. `#[allow(dead_code)]` documents the keep-alive shape (Adapter depends on Instance being alive). |
| `Renderer::adapter` | `wgpu::Adapter` | Owned, keep-alive | Same shape. |
| `Renderer::device` / `Renderer::queue` | `Arc<wgpu::Device>` / `Arc<wgpu::Queue>` | Shared, wgpu convention | wgpu's own API uses `Arc` for these handles (cheap ref-count, not lock-protected). Shared by `WgpuPainter` and `OffscreenRenderer` via setup-phase `Arc::clone` (acceptable; not per-frame). |
| `Renderer::surface` | `Option<wgpu::Surface<'static>>` | Owned, single-mutator | wgpu 25.x's `Surface<'_>: Send + Sync` (verified via `assert_impl_all!` in `wgpu/src/api/surface.rs`). Single-mutator enforced by code convention (only `Renderer::render_scene` calls `surface.get_current_texture`), not by trait bound. |
| `Renderer::painter` | `Option<WgpuPainter>` | Owned, single-mutator | The take/return dance during `render_scene` is the per-frame ownership transfer. |
| `Renderer::offscreen` | `Option<Arc<parking_lot::Mutex<OffscreenRenderer>>>` | **Mythos friction** | The lock is uncontended in production (single-mutator). Removal requires a `Backend<'a>` lifetime refactor; see [Outstanding refactors](#outstanding-refactors). |
| `Backend::offscreen` | `Option<Arc<parking_lot::Mutex<OffscreenRenderer>>>` | **Mythos friction** | Same; symmetric with the above. |
| `Backend::offscreen_painter` | `Option<WgpuPainter>` | Owned, single-mutator | Cross-frame painter cache; resized on demand. No lock. |
| `WgpuPainter::device` / `WgpuPainter::queue` | `Arc<wgpu::Device>` / `Arc<wgpu::Queue>` | Shared, wgpu convention | Same as `Renderer::device`. |
| `WgpuPainter::transform_stack` / `clip_stack` / `opacity_stack` | `Vec<T>` | Owned, single-mutator | Per-frame save/restore stacks. No lock. |
| `OffscreenRenderer::pipelines` ([`src/wgpu/offscreen.rs`](src/wgpu/offscreen.rs)) | `HashMap<ShaderType, Arc<wgpu::RenderPipeline>>` | Setup-phase populated, frame-read | The `Arc<RenderPipeline>` clones at lines 659-660, 1051-1053 are per-effect-frame (small constant; not per-layer). |
| `TexturePool::pool` ([`src/wgpu/texture_pool.rs`](src/wgpu/texture_pool.rs)) | `Arc<Mutex<TexturePoolInner>>` | **Mythos friction** | Single-mutator data behind a lock + back-reference on `PooledTexture` for release-on-drop. Refactor target; see [Outstanding refactors](#outstanding-refactors). |
| `ShaderCache::cache` ([`src/wgpu/shader_compiler.rs`](src/wgpu/shader_compiler.rs)) | `RwLock<HashMap<ShaderType, Arc<CompiledShader>>>` | Setup-phase populated, frame-read | The lock is uncontended; cache is populated lazily on first use of each shader, then read-only. Acceptable per the precedent of `PipelineOwner`'s `Weak<RwLock<>>` in `flui-rendering`. |
| `layer_render.rs` `SUPERELLIPSE_CACHE` | `thread_local! RefCell<HashMap<SuperellipseKey, Path>>` | Thread-local, hot-path | Canonical Rust pattern for single-threaded hot-path caching; not a refusal-trigger violation per `docs/PORT.md` Trigger 2 (the regex matches `Box<dyn Layer>` / `dyn RenderObject` shapes, not `HashMap<SuperellipseKey, Path>`). |

**Unsafe blocks:**

| Site | Block | Safety invariant |
|---|---|---|
| [`src/wgpu/renderer.rs:189-208`](src/wgpu/renderer.rs) | `unsafe { let surface_target = wgpu::SurfaceTargetUnsafe::from_window(window).map_err(...)?; instance.create_surface_unsafe(surface_target).map_err(...)? }` | In this call site the `SurfaceTargetUnsafe::RawHandle` is consumed immediately by `create_surface_unsafe`, so the two unsafe-fn contracts collapse to a single invariant: `window` must outlive the returned `Surface`. `flui-app::App` holds the winit window for the entire application lifetime, so the `Renderer` (and its `Surface<'static>`) cannot outlive the window. |
| [`src/wgpu/buffer_pool.rs:277-298`](src/wgpu/buffer_pool.rs) | 5 unsafe deref blocks (raw pointer disjoint-borrow primitive: `unsafe { &mut *allocations }`, `unsafe { &mut *reuses }`, `unsafe { &*vertex_ptr }`) | Pre-existing unsafe primitive for disjoint borrows over the allocation tracker. Predates this chain. The safety invariants are documented locally in the file. Not modified by the Mythos chain (net unsafe delta for this chain: 0); listed here for inventory completeness. |

No `unsafe impl Send/Sync` anywhere in the crate. Two unsafe surfaces total: one in `renderer.rs` (wgpu surface creation), one in `buffer_pool.rs` (5-block disjoint-borrow primitive, pre-existing). Net unsafe delta for the chain: **0** (chain added zero; consolidated one existing block in `renderer.rs` with a documented SAFETY comment).

**Auto-derived Send/Sync** on (verified against wgpu 25.x trait bounds):
- `Renderer` -- `Send + Sync` today (every field is `Send + Sync`: wgpu `Instance`/`Adapter`/`Device`/`Queue`/`Surface<'static>` are all `Send + Sync` in wgpu 25.x; `Arc<parking_lot::Mutex<OffscreenRenderer>>` provides `Sync` via the Mutex). Single-mutator enforced by code convention, not by trait bound. After U6 (Outstanding refactor) replaces `Arc<Mutex<OffscreenRenderer>>` with owned `Option<OffscreenRenderer>`, `Renderer: Send + Sync` will still hold iff `OffscreenRenderer: Sync`; if not, `Renderer` becomes `Send`-only.
- `WgpuPainter` -- `Send`, not `Sync` (holds `Arc<wgpu::Device>` + `Arc<wgpu::Queue>` which are `Send + Sync`, but internal batch state uses `Vec<T>` mutated through `&mut self`; no interior mutability sync surface).
- `OffscreenRenderer` -- `Send`, not `Sync` (HashMap of `Arc<RenderPipeline>` is `Send`; the struct has no interior-mutability sync primitives).
- `TexturePool` -- `Send + Sync` today (through inner `Arc<Mutex<TexturePoolInner>>`); will become `Send`-only after U8 (Outstanding refactor) replaces the inner lock with direct ownership.

---

## Friction log

Known sites that do not yet match the methodology but are not violations of the current refusal triggers. Each entry names the site and the next planned step.

### `Arc<parking_lot::Mutex<OffscreenRenderer>>` shared between `Renderer` and `Backend`

**Sites:** [`src/wgpu/renderer.rs:147`](src/wgpu/renderer.rs) (`Renderer::offscreen` field), [`src/wgpu/renderer.rs:670`](src/wgpu/renderer.rs) (`Arc::clone(offscreen)` at `Backend::with_offscreen` construction), [`src/wgpu/renderer.rs:898, 925`](src/wgpu/renderer.rs) (`offscreen_arc.lock()` calls in `handle_backdrop_filter`), [`src/wgpu/backend.rs:26`](src/wgpu/backend.rs) (field), [`src/wgpu/backend.rs:45, 57`](src/wgpu/backend.rs) (signatures), [`src/wgpu/backend.rs:399`](src/wgpu/backend.rs) (`self.offscreen.clone()` Arc-clone bind), [`src/wgpu/backend.rs:407, 464`](src/wgpu/backend.rs) (`offscreen_arc.lock()` calls in `render_shader_mask`).

**Violation:** none of the seven refusal triggers; the Mythos verdict §9 lists this as the canonical "Arc<Mutex<>> over single-mutator data" smell, but the lock is uncontended in production (single-mutator). The shape is a known maintenance burden, not a soundness or contention issue today.

**Next planned step:** see [Outstanding refactors](#outstanding-refactors) -- the refactor requires introducing a frame lifetime `Backend<'a>` and restructuring `Renderer::render_scene`'s painter take/return pattern.

### `Arc<Mutex<TexturePoolInner>>` back-reference on `PooledTexture`

**Sites:** [`src/wgpu/texture_pool.rs:71`](src/wgpu/texture_pool.rs) (`TexturePool::pool` field), [`src/wgpu/texture_pool.rs:224`](src/wgpu/texture_pool.rs) (`PooledTexture::inner` back-reference), [`src/wgpu/texture_pool.rs:239`](src/wgpu/texture_pool.rs) (`Arc::new(Mutex::new(...))` at construction), [`src/wgpu/texture_pool.rs:277`](src/wgpu/texture_pool.rs) (`Arc::clone(&self.inner)` at `acquire` return).

**Violation:** none of the seven refusal triggers; the lock is uncontended (single-mutator). The Mythos verdict §9 flagged this as the second canonical Arc<Mutex<>> smell.

**Next planned step:** see [Outstanding refactors](#outstanding-refactors) -- replace back-reference + Drop with explicit `pool.release(texture)` API.

### Per-frame `Arc::clone(&self.device)` / `Arc::clone(&self.queue)` in `Renderer::render_scene`

**Sites:** [`src/wgpu/renderer.rs:636-637`](src/wgpu/renderer.rs) (`RenderContext { device: Arc::clone(&self.device), queue: Arc::clone(&self.queue), … }`).

**Violation:** none today -- Trigger 5's regex doesn't match this site (the path scope was set up only after `flui-engine/src/wgpu/layer_render.rs` and the engine's own per-frame paths were not added to the trigger before this chain's U13). U13 (this chain) extends the scope to catch the regression if reintroduced.

**Next planned step:** see [Outstanding refactors](#outstanding-refactors) -- `RenderContext` becomes `RenderContext<'frame>` with borrowed `&'frame wgpu::Device` / `&'frame wgpu::Queue` references. Tied to the `Backend<'a>` lifetime refactor.

### `painter.rs` is 3,772 LOC -- the largest .rs file in the workspace

**Site:** [`src/wgpu/painter.rs`](src/wgpu/painter.rs).

**Violation:** none of the refusal triggers; the file passes `port-check` and `clippy`. Mythos audit flagged it as a "god module" because it mixes batch recording, save-layer state machines, gradient construction, text rendering integration, and per-frame submission. The verdict proposed a `painter/{batch, segment, layer, gradient, text, render}.rs` directory split; the chain deferred the split because:

- The split is mechanical (no semantic change), but every change requires re-opening `impl WgpuPainter` blocks across multiple files.
- The chain already touched `painter.rs` for the `Painter` trait removal (U5) and the dead-code audit (U10).
- Review clarity favours landing the split in a thin housekeeping PR after the chain merges.

**Next planned step:** see [Outstanding refactors](#outstanding-refactors).

### `offscreen.rs` is 1,525 LOC -- second god module

**Site:** [`src/wgpu/offscreen.rs`](src/wgpu/offscreen.rs).

Same shape as `painter.rs`. Mixes mask, blur, and morphological filter pipelines. Verdict proposed an `offscreen/{mask, blur, morph}.rs` split; deferred for the same review-clarity reason.

### Forward-looking helpers in `effects`, `instancing`, `pipeline`, `shader_compiler` modules

**Sites:** [`src/wgpu/effects.rs`](src/wgpu/effects.rs) (`ShadowParams::elevation_*`, `BlurIntensity`, `LinearGradientBuilder`), [`src/wgpu/instancing.rs`](src/wgpu/instancing.rs) (`RectInstance::rounded_rect` / `with_transform`, `CircleInstance::ellipse`, `TextureInstance::with_rotation`), [`src/wgpu/pipeline.rs`](src/wgpu/pipeline.rs) (various constructor shortcuts), [`src/wgpu/shader_compiler.rs`](src/wgpu/shader_compiler.rs) (`ShaderCache::cached_count`, `ShaderCache::clear`).

**Violation:** none; module-level `#[allow(dead_code)]` retained with documentation. These are forward-looking helpers with named eventual consumers (`painter.rs` and devtools introspection); per-item deletion is bandwidth-dependent.

**Next planned step:** per-item audit + deletion -- see [Outstanding refactors](#outstanding-refactors).

### `wgpu/texture_cache.rs` (1,000 LOC) + `wgpu/external_texture_registry.rs` (315 LOC) + `wgpu/path_cache.rs` (336 LOC) + `wgpu/multi_draw.rs` (304 LOC) -- in-crate-only consumers via `painter.rs` fields

**Sites:** [`src/wgpu/painter.rs:316, 319, 326, 1525`](src/wgpu/painter.rs) (struct fields + one `use super::multi_draw` import).

**Violation:** none of the refusal triggers. The original Mythos verdict (U4) proposed deleting all four modules because no external caller exists. Implementation surfaced `WgpuPainter` fields referencing each: `texture_cache: TextureCache`, `external_texture_registry: ExternalTextureRegistry`, `path_cache: PathCache`, `MultiDrawBatcher` import. Whether these fields are populated-and-queried in production paths or stored-but-never-read is interior to `painter.rs`'s 3,772 LOC; determining that requires a `painter.rs` internal audit that the chain deferred per the verdict's "bandwidth-dependent" clause.

**Next planned step:** see [Outstanding refactors](#outstanding-refactors).

### Doctest examples may use pre-`Pixels`-wrap `Offset::new(f32, f32)` shape

**Sites:** TBD per `cargo test --doc -p flui-engine` after the chain merges. The doctest breakage pattern is the same as the `flui-layer` chain's Friction log entry; engine doctests inherit the same `Offset::new` signature constraint.

**Next planned step:** mechanical sweep of `Offset::new(<f32>, <f32>)` -> `Offset::new(px(<f32>), px(<f32>))` plus an explicit `use flui_types::geometry::px;` in each affected doc example. Out of scope for this Mythos chain.

---

## Outstanding refactors

Concrete cleanups visible from `flui-engine` outward, sized for an `/aif-implement` dispatch. Each entry names a file and what would need to change. Each has a named concrete blocker per the no-quick-wins memo.

### `Arc<parking_lot::Mutex<OffscreenRenderer>>` -> direct ownership + `Backend<'a>` frame lifetime

**Files:** [`src/wgpu/renderer.rs`](src/wgpu/renderer.rs), [`src/wgpu/backend.rs`](src/wgpu/backend.rs).

**Goal:** replace `Option<Arc<parking_lot::Mutex<OffscreenRenderer>>>` on `Renderer` and `Backend` with `Option<OffscreenRenderer>` (direct ownership on `Renderer`) + `Option<&'a mut OffscreenRenderer>` (borrowed on `Backend<'a>` for the frame lifetime).

**Shape:**
1. `Renderer::offscreen: Option<Arc<parking_lot::Mutex<OffscreenRenderer>>>` -> `offscreen: Option<OffscreenRenderer>`.
2. `pub struct Backend` -> `pub struct Backend<'a>` with `painter: &'a mut WgpuPainter`, `offscreen: Option<&'a mut OffscreenRenderer>`.
3. `Renderer::render_scene` restructured to drop the `self.painter.take()` / `self.painter = Some(painter)` pattern (no longer needed; Backend borrows).
4. `Backend::render_shader_mask` uses `&mut self.offscreen.as_mut()` instead of `offscreen_arc.lock()`.
5. `Renderer::handle_backdrop_filter` takes `backend: &mut Backend<'_>` and accesses offscreen via the backend's lifetime.

**Concrete blocker:** the refactor requires breaking the painter take/return pattern in `render_scene` (substantial lifetime gymnastics with the `Backend<'a>` lifetime, since `Backend::render_shader_mask` and `Renderer::handle_backdrop_filter` both need disjoint `&mut painter` + `&mut offscreen` from the same `Backend<'a>`). The borrow-checker resolution is non-trivial (likely needs a `Backend::split_mut(&mut self) -> (&mut WgpuPainter, Option<&mut OffscreenRenderer>)` accessor). Estimated 200-400 LOC of churn for marginal-runtime-benefit (the lock is uncontended in production); review-clarity favours landing this as a separate housekeeping PR with focused review.

**Dependencies:** none external; pure Rust refactor.

### `Arc<Mutex<TexturePoolInner>>` -> direct ownership + explicit `pool.release(texture)`

**Files:** [`src/wgpu/texture_pool.rs`](src/wgpu/texture_pool.rs), [`src/wgpu/offscreen.rs`](src/wgpu/offscreen.rs) (consumer).

**Goal:** remove the back-reference `Arc<Mutex<TexturePoolInner>>` on `PooledTexture` that exists only for release-on-drop. Replace with explicit `pool.release(texture)` at the workflow boundaries.

**Shape:**
1. `TexturePool::pool: Arc<Mutex<TexturePoolInner>>` -> `TexturePool::available: Vec<TextureSlot>` (direct).
2. `PooledTexture` removes the `inner: Arc<Mutex<TexturePoolInner>>` field; Drop becomes a no-op (or drops the `wgpu::Texture` directly, releasing GPU memory if not pooled).
3. New method: `TexturePool::release(&mut self, texture: PooledTexture)`.
4. `TexturePool::acquire(&mut self, ...)` (was `&self`).
5. `OffscreenRenderer::with_caches` parameter changes from `Arc<TexturePool>` to `TexturePool`.
6. Every consumer of `PooledTexture` either calls `pool.release(tex)` after the texture is no longer needed, or accepts that Drop discards the texture (acceptable for one-frame textures; the slot is reused via `release`).

**Concrete blocker:** the refactor touches every `PooledTexture` consumer (currently 4-6 sites across `OffscreenRenderer::render_blur`, `render_masked`, `render_dilate`, `render_erode`). Each consumer needs an explicit `release` call -- mechanical but error-prone if a consumer forgets to release.

**Dependencies:** none.

### Per-frame `Arc::clone(&self.device)` / `Arc::clone(&self.queue)` -> borrowed references

**Files:** [`src/wgpu/renderer.rs:636-637`](src/wgpu/renderer.rs).

**Goal:** eliminate per-frame Arc clones in `RenderContext` construction. The clones are uncontested (single ref-count increment), but the pattern compounds across hundreds of frames per second.

**Shape:**
1. `RenderContext` changes from `device: Arc<wgpu::Device>` -> `device: &'frame wgpu::Device` (with frame lifetime).
2. `Renderer::render_scene` body: `RenderContext { device: &self.device, queue: &self.queue, ... }`.
3. `Renderer::handle_backdrop_filter` signature gains `ctx: &RenderContext<'_>` with the frame lifetime.

**Concrete blocker:** depends on the `Backend<'a>` lifetime refactor above; both share the frame-lifetime boundary. Lands together with the `Arc<Mutex<OffscreenRenderer>>` removal.

**Dependencies:** previous Outstanding refactor.

### `painter/` directory split: `wgpu/painter.rs` (3,772 LOC) -> `painter/{batch, segment, layer, gradient, text, render}.rs`

**Files:** [`src/wgpu/painter.rs`](src/wgpu/painter.rs) -> directory.

**Goal:** drop the largest .rs file in the workspace by extracting cohesive concerns into sibling files. The pattern mirrors `flui-layer`'s U10 `compositor.rs` -> `compositor/{builder, retained}.rs` split.

**Shape:**
- `painter/mod.rs` (~50 LOC) -- `pub struct WgpuPainter` + public API + re-exports.
- `painter/batch.rs` (~600 LOC) -- `DrawSegment`, `TessellatedBatch`, `ScissorRegion`.
- `painter/layer.rs` (~400 LOC) -- `SavedLayer`, `PendingOpacityLayer`, `save_layer`, `restore_layer`.
- `painter/gradient.rs` (~600 LOC) -- gradient construction + dispatch.
- `painter/text.rs` (~400 LOC) -- text rendering methods.
- `painter/render.rs` (~800 LOC) -- `render()` entry point + per-segment GPU submission.

**Concrete blocker:** mechanical LOC redistribution with no semantic change. The split requires careful re-opening of `impl WgpuPainter` blocks across multiple files; verification that internal helpers stay accessible (`pub(super)` where needed). Estimated 1-2 hours of mechanical edits; review-clarity favours a focused housekeeping PR.

**Dependencies:** none.

### `offscreen/` directory split: `wgpu/offscreen.rs` (1,525 LOC) -> `offscreen/{mask, blur, morph}.rs`

**Files:** [`src/wgpu/offscreen.rs`](src/wgpu/offscreen.rs) -> directory.

**Goal:** same shape as `painter/` split. Mixes mask, blur, and morphological filter pipelines.

**Concrete blocker:** same.

**Dependencies:** none.

### Audit `painter.rs` consumers of `texture_cache`, `external_texture_registry`, `path_cache`, `multi_draw`

**Files:** [`src/wgpu/painter.rs`](src/wgpu/painter.rs), [`src/wgpu/texture_cache.rs`](src/wgpu/texture_cache.rs), [`src/wgpu/external_texture_registry.rs`](src/wgpu/external_texture_registry.rs), [`src/wgpu/path_cache.rs`](src/wgpu/path_cache.rs), [`src/wgpu/multi_draw.rs`](src/wgpu/multi_draw.rs).

**Goal:** determine whether `WgpuPainter` fields (`texture_cache: TextureCache`, `external_texture_registry: ExternalTextureRegistry`, `path_cache: PathCache`) and the `multi_draw::MultiDrawBatcher` import are populated-and-queried in production paths or stored-but-never-read.

**Shape:** read each field's use in `painter.rs`. For each:
- If populated + queried via specific call paths -> document the path in ARCHITECTURE.md and leave the module.
- If populated but never queried (zombie field) -> delete the field, then delete the module (if no other consumer).
- If never populated (dead init) -> delete the field, then delete the module.

**Estimated deletion budget:** ~1,955 LOC (the four modules) if all confirmed unused. Substantial LOC win for an audit-only Mythos pass.

**Concrete blocker:** depends on the `painter/` directory split (above) for review-clarity -- auditing 3,772-LOC `painter.rs` for field usage is much easier after the split.

**Dependencies:** `painter/` split first.

### Per-item audit of `effects`, `instancing`, `pipeline`, `shader_compiler` dead helpers

**Files:** [`src/wgpu/effects.rs`](src/wgpu/effects.rs), [`src/wgpu/instancing.rs`](src/wgpu/instancing.rs), [`src/wgpu/pipeline.rs`](src/wgpu/pipeline.rs), [`src/wgpu/shader_compiler.rs`](src/wgpu/shader_compiler.rs).

**Goal:** for each item flagged by removing the module-level `#[allow(dead_code)]`, decide keep-or-delete.

**Item inventory (from `cargo check` output at chain end):**
- `effects.rs`: `ShadowParams::elevation_1` through `elevation_5`, `BlurIntensity::iterations` / `radius`, `LinearGradientBuilder::new` / `add_stop` / `start` / `end` / `build` -- forward-looking shadow/blur/gradient builder helpers.
- `instancing.rs`: `RectInstance::rounded_rect` / `with_transform`, `CircleInstance::ellipse`, `ArcInstance::ellipse`, `TextureInstance::with_rotation` / `with_uv` -- constructor shortcuts.
- `pipeline.rs`: multiple items (TBD per audit).
- `shader_compiler.rs`: `ShaderCache::cached_count` / `clear` -- devtools introspection.

**Concrete blocker:** per-item audit requires reading each function's body + tracking caller search. Estimated 2-3 hours for ~20 items.

**Dependencies:** none.

### `catch_unwind` boundary on `Renderer::render_scene` (forward-looking)

**Files:** [`src/wgpu/renderer.rs`](src/wgpu/renderer.rs).

**Goal:** wrap the whole `render_scene` body in `std::panic::catch_unwind(AssertUnwindSafe(|| ...))` so a panicking `LayerRender::render` impl doesn't take down the frame.

**Concrete blocker:** no real-world panic surfaced today; the 16 `LayerRender` unit tests prove the per-variant render impls don't panic in production. Defensive-in-depth is a forward-looking concern, not a current bug. The `AssertUnwindSafe` ceremony around `Backend` + `WgpuPainter` mutable state requires careful audit that they remain consistent after a panic.

**Dependencies:** observed-real-world-panic before implementing.

### Doc-sweep on `WgpuPainter` inherent methods

**Files:** [`src/wgpu/painter.rs`](src/wgpu/painter.rs) (the `impl WgpuPainter` block at the post-U5 boundary).

**Goal:** the U5 commit (`1b376beb`) deleted the `pub trait Painter` and converted its 46 methods to inherent `pub fn` on `WgpuPainter`. The trait's per-method doc comments were lost with the trait deletion. A blanket `#[allow(missing_docs)]` was applied on the impl block as a transient suppression. The doc-sweep restores per-method docs (copied/adapted from the deleted trait's doc comments preserved in git history at commit `db817c0d^:crates/flui-engine/src/traits.rs`).

**Shape:** 46 doc comments to write (or port from the pre-deletion git revision). After the sweep, remove the `#[allow(missing_docs)]` from the impl block at `painter.rs:2025` area.

**Concrete blocker:** mechanical doc-writing; ~2-3 hours. Could land in the same PR as the `painter/` directory split (Outstanding refactor #4) since both touch the impl block.

**Dependencies:** ideally lands with the `painter/` split for review-clarity, but standalone is also fine.

### Pre-existing follow-ups (filed for awareness, not deferred-with-blocker)

The following pre-existing concerns are tracked outside this Outstanding refactors list because they have no concrete blocker -- they are scheduled fixes, not technical deferrals:

- **Doctest `Pixels`-wrap fix:** every doctest in `src/wgpu/*.rs` currently uses `Offset::new(100.0, 50.0)` which fails to compile because `Offset<Pixels>::new` requires `Pixels`-wrapped arguments. Pre-existed the Mythos chain. Mechanical sweep; 1-2 hours. Should be its own PR; not on the deferred-refactor list because there is no blocker. Tracked separately in the Friction log.

- **Cross-crate Mythos chain continuation** (`flui-app`, `flui-view`, `flui-platform`, `flui-painting`, `flui-interaction`): these are next-crate planning artifacts. Listed in [`docs/PORT.md`](../../docs/PORT.md) `## Index` as "Not yet templated" entries; brainstorms + verdicts + plans live in `docs/brainstorms/`, `docs/designs/`, `docs/plans/` when they are authored. **Out of scope for `flui-engine/ARCHITECTURE.md`'s Outstanding refactors** (which scope to work visible from this crate outward). Pointers stay in `docs/PORT.md`.

---

## Notes

- **Net unsafe delta for this chain: 0.** The single existing `unsafe { instance.create_surface_unsafe(...) }` block in `Renderer::new` is required by wgpu's API contract and stays; the chain consolidated the two unsafe calls into one block with a documented SAFETY comment. Zero new unsafe blocks were added.
- **Net LOC reduction for this chain: ~-5,888 LOC of production code** (per `git show --stat` totals across the 10 substantive commits): U1 -812 (utils/ delete), U2 -2,190 (scene.rs + compositor.rs delete), U3 -2,188 (platform stubs delete), U4 net -1 (commands.rs shim + import fixes), U5 net -429 (Painter trait + RenderError::PainterError + painting_demo imports), U6 net +23 (anyhow -> RenderResult), U7 net -291 (text_renderer.rs delete + shader const aliases + dead-code allow audit). Original target was ≥6,000 LOC; **target missed by ~112 LOC** because R7's promised 1,955 LOC of additional module deletions deferred (the four `wgpu/{texture_cache, external_texture_registry, path_cache, multi_draw}.rs` modules turned out to have in-crate consumers via `painter.rs` fields; deletion deferred to Outstanding refactor #6). Three god modules (`painter.rs`, `offscreen.rs`, plus the historic mid-size files) remain un-split; tracked in Outstanding refactors.
- **`port-check.sh` extended in Mythos U13 of this chain** -- see [`docs/PORT.md`](../../docs/PORT.md) `## Refusal triggers` for the seven trigger inventory after the extension.
- **`Arc<Mutex<>>` shapes for `OffscreenRenderer` and `TexturePoolInner` survived the chain.** Documented in Friction log + Outstanding refactors with concrete blockers. The chain prioritised dead-code deletion (largest LOC wins) over lock-shape refactoring (substantial lifetime gymnastics for marginal runtime benefit).
- **Two test counts** at chain end: `cargo test -p flui-engine --lib` shows 48 passed (down from 53 pre-chain, with 5 tests deleted alongside `text_renderer.rs`); `cargo test -p flui-engine --doc` count TBD per doctest fix Outstanding refactor.
- **`anyhow::Result` is no longer in the engine's public API.** `Renderer::new`, `Renderer::new_offscreen`, `FontLoader::load_file`, `FontLoader::load_directory` all return `RenderResult<T>`. The `anyhow` crate stays in `Cargo.toml` (transitive via wgpu) but is no longer used in any signature; the workspace-wide consistency win.
