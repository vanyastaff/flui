# flui-engine Full Redesign

**Date:** 2026-04-06
**Status:** Approved (revised after review)
**Scope:** Full feature-complete redesign of `flui-engine` crate

## Summary

Complete architectural redesign of flui-engine to replace the current God Object architecture (WgpuPainter, 2281 lines, 80-method CommandRenderer trait) with a modular system built around two core concepts: **GpuDevice** (shared GPU state across windows) and **RenderSurface** (per-window state) with **FrameEncoder** (per-frame recording and submission).

## Design Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Scope | Full feature-complete redesign | Current architecture has fundamental issues that targeted refactoring cannot fix |
| Backend abstraction | wgpu-primary with escape hatches | Public types wgpu-agnostic, internals optimized for wgpu. Constitution requires backend readiness without over-engineering |
| Core architecture | GpuDevice (shared) + RenderSurface (per-window) + FrameEncoder (per-frame) | Solves lifetime, multi-window, and responsibility decomposition |
| CommandRenderer trait | Remove entirely | Premature abstraction for single-backend. FrameEncoder dispatches via direct `match` on DrawCommand |
| Text rendering | Glyphon (wraps cosmic-text) | Already in deps, manages atlas internally, proven in Iced ecosystem |
| Shader pipeline | Static pipelines, compile at init | Known primitive set, `include_str!()` for WGSL, zero runtime compilation, predictable startup |
| Batching strategy | Hybrid per-layer | Sort within layers (safe for z-order), flush at layer boundaries. Expected 6-12 draw calls per frame |
| Testing | Unit tests (no GPU) + headless GPU tests + criterion benchmarks + tracing profiling | Comprehensive coverage with fast feedback loop |

## Architecture

### Core Concepts

**GpuDevice** (shared, long-lived, one per application):
- Owns shared GPU handles: `Arc<wgpu::Device>`, `Arc<wgpu::Queue>`
- Owns shared caches: `PipelineRegistry`, `TextureCache`, `BufferPool`, `TextSystem`
- Owns `GpuCapabilities` (detected features)
- Shareable across multiple windows via `Arc<GpuDevice>`
- `new()` blocks internally via `pollster::block_on()` for wgpu async init

**RenderSurface** (per-window, long-lived):
- Owns `wgpu::Surface` and `wgpu::SurfaceConfiguration`
- Holds `Arc<GpuDevice>` reference to shared state
- Provides `begin_frame()` to create a FrameEncoder
- Handles surface reconfiguration on resize and surface lost events
- One `RenderSurface` per window enables multi-window rendering

**FrameEncoder** (per-frame, create -> record -> finish -> drop):
- Borrows `&mut RenderSurface` (only the per-window surface)
- Holds `Arc<GpuDevice>` for read-only access to shared caches/pipelines
- Contains per-primitive batchers: `ShapeBatcher`, `PathBatcher`, `TextBatcher`, `ImageBatcher`, `EffectBatcher`
- Contains `StateStack` for transform/clip/opacity
- Uses pre-allocated `Vec`s from `RenderSurface` (zero per-frame allocation)
- `finish()` uploads buffers and submits GPU command buffer

### Multi-Window Support

```rust
// Application creates one shared GpuDevice
let gpu = Arc::new(GpuDevice::new()?);

// Each window gets its own RenderSurface
let mut surface_1 = RenderSurface::new(Arc::clone(&gpu), &window_1)?;
let mut surface_2 = RenderSurface::new(Arc::clone(&gpu), &window_2)?;

// Render independently (can even be on different threads)
let mut frame_1 = surface_1.begin_frame()?;
frame_1.render_scene(&scene_1)?;
frame_1.finish()?;

let mut frame_2 = surface_2.begin_frame()?;
frame_2.render_scene(&scene_2)?;
frame_2.finish()?;
```

### Data Flow

```
Scene (from flui-layer)
  |
  v
FrameEncoder::render_scene()
  |
  v  (depth-first layer traversal)
  |
  +-- Layer dispatch (see "Layer Dispatch Table" below)
  |     Clip layers     --> state.push_clip() + render children + state.pop_clip()
  |     Transform layers --> state.push_transform() + render children + state.pop_transform()
  |     Effect layers    --> state.push_*() + render children + flush_layer() + state.pop_*()
  |     Leaf layers      --> dispatch DisplayList commands (see "DrawCommand Dispatch Table")
  |
  v  (at each compositing layer boundary)
  |
  flush_layer() -- batchers emit BatchedDraw, sorted by pipeline key
  |
  v
FrameEncoder::finish()
  |
  v
Upload buffers --> Build wgpu::CommandBuffer --> queue.submit() --> present()
```

### Public API

```rust
/// Shared GPU state (one per application)
pub struct GpuDevice { /* ... */ }

impl GpuDevice {
    /// Create with auto-detected backend. Blocks on wgpu async init via pollster.
    /// # Safety
    /// Caller must ensure the adapter/device outlive all surfaces created from this device.
    pub fn new() -> RenderResult<Self>;

    /// Create without a window (headless adapter) for testing/CI.
    pub fn new_headless() -> RenderResult<Self>;

    pub fn capabilities(&self) -> &GpuCapabilities;
}

/// Per-window rendering surface
pub struct RenderSurface { /* ... */ }

impl RenderSurface {
    /// Create a surface for a window.
    /// Requires both HasWindowHandle + HasDisplayHandle (wgpu 25.x).
    /// Contains unsafe surface creation with SAFETY comment.
    pub fn new(
        gpu: Arc<GpuDevice>,
        window: &(impl HasWindowHandle + HasDisplayHandle),
    ) -> RenderResult<Self>;

    /// Create headless surface for testing (renders to texture).
    pub fn new_headless(gpu: Arc<GpuDevice>, width: u32, height: u32) -> RenderResult<Self>;

    /// Resize surface. Takes physical pixel dimensions + scale factor.
    /// Also handles surface reconfiguration after SurfaceLost.
    pub fn resize(&mut self, width_px: u32, height_px: u32, scale_factor: f32);

    /// Begin recording a new frame.
    /// Returns Err(SurfaceLost) if surface needs reconfiguration -- caller should
    /// call resize() and retry.
    pub fn begin_frame(&mut self) -> RenderResult<FrameEncoder<'_>>;
}

/// Per-frame command encoder
pub struct FrameEncoder<'surface> { /* ... */ }

impl<'surface> FrameEncoder<'surface> {
    pub fn render_scene(&mut self, scene: &Scene) -> RenderResult<()>;

    /// Submit all recorded GPU work and present.
    /// On SurfaceLost: frame is dropped, caller should call
    /// RenderSurface::resize() before next begin_frame().
    pub fn finish(self) -> RenderResult<()>;
}
```

## Coordinate Spaces

All public API uses **physical pixels** (`DevicePixels`) for GPU-facing dimensions:
- `RenderSurface::resize(width_px, height_px, scale_factor)` -- physical pixels + scale factor
- `FrameUniforms::viewport_size` -- physical pixels
- `BatchedDraw::SetScissor` -- physical pixels (wgpu requires physical for `set_scissor_rect`)

The `scale_factor` is passed to shaders via `FrameUniforms` so shaders can convert logical coordinates to physical pixels. Scene geometry from flui-layer arrives in logical pixels (`Pixels`) and is converted to `DevicePixels` during batcher `add_*()` calls using the current scale factor.

## Error Recovery

### Surface Lost

When `wgpu::SurfaceTexture` acquisition fails with `SurfaceError::Lost` or `SurfaceError::Outdated`:

1. `begin_frame()` returns `Err(RenderError::SurfaceLost)` or `Err(RenderError::SurfaceOutdated)`
2. Caller calls `surface.resize(current_width, current_height, current_scale_factor)`
3. `resize()` internally calls `surface.configure(&device, &config)` to reconfigure
4. Caller retries `begin_frame()` on the next frame

If error occurs mid-frame (during `finish()`):
1. `finish()` returns `Err(RenderError::SurfaceLost)`
2. All in-flight FrameEncoder state is dropped (batchers cleared, commands discarded)
3. Caller proceeds as above (resize + retry)

### Device Lost

wgpu device lost is fatal for the current GpuDevice. Recovery requires:
1. Drop all RenderSurfaces
2. Drop GpuDevice
3. Create new GpuDevice + new RenderSurfaces

This is a rare event (GPU driver crash, hardware removal). The spec does not add automatic recovery -- the application layer (flui-app) handles this.

### Timeout

`SurfaceError::Timeout` is non-fatal. `begin_frame()` returns `Err(RenderError::Timeout)`. Caller skips the frame and retries next cycle.

## Module Structure

```
crates/flui-engine/src/
+-- lib.rs                    # Public API, re-exports
+-- error.rs                  # RenderError, RenderResult (unchanged)
|
+-- context/
|   +-- mod.rs
|   +-- gpu_device.rs         # GpuDevice (shared GPU state)
|   +-- render_surface.rs     # RenderSurface (per-window)
|   +-- capabilities.rs       # GpuCapabilities
|   +-- headless.rs           # Headless adapter for tests
|
+-- frame/
|   +-- mod.rs
|   +-- encoder.rs            # FrameEncoder
|   +-- state_stack.rs        # TransformStack, ClipStack, OpacityStack
|   +-- submission.rs         # BatchedDraw, GPU command buffer submit
|   +-- dispatch.rs           # DrawCommand match dispatch + Layer traversal
|
+-- batchers/
|   +-- mod.rs
|   +-- shapes.rs             # ShapeBatcher (rect, rrect, circle, arc, oval, line)
|   +-- paths.rs              # PathBatcher (lyon tessellation)
|   +-- text.rs               # TextBatcher (glyphon text runs)
|   +-- images.rs             # ImageBatcher (textures, atlas entries)
|   +-- effects.rs            # EffectBatcher (gradients, shadows, blur)
|   +-- compositing.rs        # CompositingBatcher (SaveLayer/RestoreLayer, offscreen targets)
|
+-- pipelines/
|   +-- mod.rs
|   +-- registry.rs           # PipelineRegistry
|   +-- shape_pipeline.rs     # Instanced rect/circle/arc
|   +-- path_pipeline.rs      # Tessellated paths
|   +-- text_pipeline.rs      # Textured glyph quads
|   +-- image_pipeline.rs     # Textured image quads
|   +-- gradient_pipeline.rs  # Linear/radial gradients
|   +-- shadow_pipeline.rs    # Shadow rendering
|   +-- blur_pipeline.rs      # Dual kawase blur
|   +-- compositing_pipeline.rs # Offscreen render target compositing
|
+-- text/
|   +-- mod.rs
|   +-- system.rs             # TextSystem (glyphon FontSystem + SwashCache)
|   +-- atlas.rs              # GlyphAtlas
|   +-- cache.rs              # ShapeCache (LRU shaped text buffers)
|
+-- resources/
|   +-- mod.rs
|   +-- buffer_pool.rs        # BufferPool (reusable vertex/index/uniform)
|   +-- texture_cache.rs      # TextureCache (loaded images)
|   +-- texture_atlas.rs      # TextureAtlas (shelf-packing)
|   +-- texture_pool.rs       # TexturePool (offscreen render targets)
|   +-- external_textures.rs  # ExternalTextureRegistry (video/camera, deferred to v2)
|
+-- shaders/                  # WGSL files (existing, kept in engine per constitution boundary rules)
|   +-- rect_instanced.wgsl
|   +-- circle_instanced.wgsl
|   +-- arc_instanced.wgsl
|   +-- fill.wgsl
|   +-- shape.wgsl
|   +-- texture_instanced.wgsl
|   +-- gradients/
|   |   +-- linear.wgsl
|   |   +-- radial.wgsl
|   +-- effects/
|   |   +-- shadow.wgsl
|   |   +-- blur_downsample.wgsl
|   |   +-- blur_upsample.wgsl
|   +-- common/
|       +-- sdf.wgsl
|
+-- platform/
|   +-- mod.rs
|   +-- metal.rs              # Metal capabilities + workarounds
|   +-- dx12.rs               # DX12 capabilities + workarounds
|   +-- vulkan.rs             # Vulkan capabilities + workarounds
|
+-- vertex.rs                 # All vertex/instance types consolidated
+-- debug.rs                  # DebugEncoder (traces all commands without GPU)
```

### Files Removed

- `traits.rs` -- CommandRenderer and Painter traits (FrameEncoder is the only consumer)
- `commands.rs` -- dispatch logic moves into frame/dispatch.rs as direct `match`
- `wgpu/painter.rs` -- God Object split into context/ + frame/ + batchers/
- `wgpu/backend.rs` -- replaced by FrameEncoder
- `wgpu/compositor.rs` -- stacks moved to frame/state_stack.rs
- `wgpu/text_renderer.rs` -- stub replaced by text/system.rs
- `wgpu/instancing.rs` -- moved to vertex.rs + batchers/
- `wgpu/buffer_pool.rs` -- moved to resources/buffer_pool.rs
- `utils/text.rs` -- vector text removed (glyphon-only)

### Files Retained / Adapted

- `error.rs` -- unchanged
- `shaders/` -- WGSL files kept, same structure
- `vertex.rs` -- consolidated from vertex.rs + instancing.rs
- `debug.rs` -- adapted as DebugEncoder for new API

## DrawCommand Dispatch Table

Complete mapping of every `DrawCommand` variant to its target batcher or state operation:

| DrawCommand | Target | Notes |
|---|---|---|
| **Primitives** | | |
| DrawRect | ShapeBatcher | Instanced, corner_radii = 0 |
| DrawRRect | ShapeBatcher | Instanced, with corner_radii |
| DrawCircle | ShapeBatcher | Instanced via CircleInstance |
| DrawOval | ShapeBatcher | Instanced, mapped to ellipse variant of CircleInstance |
| DrawArc | ShapeBatcher | Instanced via ArcInstance |
| DrawLine | ShapeBatcher | Instanced via LineInstance |
| DrawDRRect | PathBatcher | Tessellated (outer - inner path), too complex for instancing |
| DrawPath | PathBatcher | Tessellated via lyon fill/stroke |
| DrawPoints | PathBatcher | Tessellated as point/line primitives |
| DrawVertices | PathBatcher | Pre-tessellated vertices, direct upload |
| DrawColor | ShapeBatcher | Full-viewport rect with blend mode |
| **Text** | | |
| DrawText | TextBatcher | Glyphon shaped text run |
| DrawTextSpan | TextBatcher | Rich text spans (deferred to v2, logs warning) |
| **Images** | | |
| DrawImage | ImageBatcher | Instanced textured quad |
| DrawTexture | ImageBatcher | External texture quad |
| DrawAtlas | ImageBatcher | Sprite atlas, multiple instanced quads |
| DrawImageRepeat | ImageBatcher | Tiled texture via repeated instances |
| DrawImageNineSlice | ImageBatcher | 9 instanced quads with UV mapping |
| DrawImageFiltered | ImageBatcher | Image + effect pass (filter applied post-draw) |
| **Effects** | | |
| DrawGradient | EffectBatcher | Linear/radial gradient on rect |
| DrawGradientRRect | EffectBatcher | Linear/radial gradient on rounded rect |
| DrawShadow | EffectBatcher | Shadow instance |
| ShaderMask | CompositingBatcher | Render child to offscreen, apply shader mask, composite |
| BackdropFilter | CompositingBatcher | Read back framebuffer region, apply filter, composite |
| **Clipping** | | |
| ClipRect | StateStack | push_clip(rect) |
| ClipRRect | StateStack | push_clip(rrect) -- uses scissor for AABB, stencil for rounded |
| ClipPath | StateStack | push_clip(path) -- stencil buffer |
| **Layer Operations** | | |
| SaveLayer | CompositingBatcher | Allocate offscreen render target from TexturePool, redirect rendering |
| RestoreLayer | CompositingBatcher | Composite offscreen target back to main target with blend/opacity |

## Layer Dispatch Table

Complete mapping of every `Layer` variant to its handling in `render_scene()`:

| Layer Variant | Handling | Notes |
|---|---|---|
| **Leaf Layers** | | |
| Canvas(CanvasLayer) | Dispatch all commands from display_list | Primary content layer |
| Picture(PictureLayer) | Dispatch all commands from picture's display_list | Cached immutable content (repaint boundary) |
| Texture(TextureLayer) | ImageBatcher -- external GPU texture quad | Video frames, camera preview |
| PlatformView(PlatformViewLayer) | Skip (rendered by platform, engine leaves hole) | Native views embedded in UI |
| PerformanceOverlay(PerformanceOverlayLayer) | Dedicated overlay rendering (frame times, GPU stats) | Debug only |
| **Clip Layers** | | |
| ClipRect(ClipRectLayer) | state.push_clip(rect) + render children + flush + state.pop_clip() | Hardware scissor |
| ClipRRect(ClipRRectLayer) | state.push_clip(rrect) + render children + flush + state.pop_clip() | Scissor AABB + stencil for rounded corners |
| ClipPath(ClipPathLayer) | state.push_clip(path) + render children + flush + state.pop_clip() | Stencil buffer |
| ClipSuperellipse(ClipSuperellipseLayer) | Fallback to ClipRRect (proper squircle deferred to v2) | |
| **Transform Layers** | | |
| Offset(OffsetLayer) | state.push_transform(translate) + render children + state.pop_transform() | Translation-only optimization |
| Transform(TransformLayer) | state.push_transform(matrix) + render children + state.pop_transform() | Full matrix transform |
| **Effect Layers** | | |
| Opacity(OpacityLayer) | state.push_opacity(value) + render children + flush + state.pop_opacity() | Multiplicative alpha |
| ColorFilter(ColorFilterLayer) | CompositingBatcher -- render to offscreen, apply color matrix, composite | GPU shader post-process |
| ImageFilter(ImageFilterLayer) | CompositingBatcher -- render to offscreen, apply blur/dilate/erode, composite | GPU multi-pass |
| ShaderMask(ShaderMaskLayer) | CompositingBatcher -- render to offscreen, apply shader mask, composite | |
| BackdropFilter(BackdropFilterLayer) | CompositingBatcher -- read framebuffer, apply filter, composite | Frosted glass effect |
| **Linking Layers** | | |
| Leader(LeaderLayer) | Register anchor position in link registry + render children | No visual effect |
| Follower(FollowerLayer) | Look up leader position, apply offset transform + render children | Positioned relative to leader |
| **Annotation Layers** | | |
| AnnotatedRegion(AnnotatedRegionLayer) | Skip (metadata only, consumed by platform layer) | No visual effect |

## Batching System

### Batcher Trait

```rust
/// Batchers accumulate draw data during recording, then emit BatchedDraw
/// commands on flush. Batchers receive &GpuDevice (immutable shared state)
/// for buffer allocation, NOT &mut RenderSurface.
pub(crate) trait Batcher {
    fn flush(&mut self, gpu: &GpuDevice, draws: &mut Vec<BatchedDraw>);
    fn is_empty(&self) -> bool;
    fn clear(&mut self);
}
```

Note: Batchers take `&GpuDevice` (shared, immutable) not `&GpuContext` or `&mut RenderSurface`. This avoids borrow checker conflicts because FrameEncoder borrows `&mut RenderSurface` while batchers only need read access to device/queue/pipelines.

### ShapeBatcher

Accumulates instanced primitives. Each `add_*()` bakes current transform from StateStack into instance data. On `flush()`, one instanced draw call per primitive type.

Primitive types: `RectInstance` (rect + rrect via corner_radii), `CircleInstance` (circle + oval), `ArcInstance`, `LineInstance`.

### PathBatcher

Tessellates paths immediately on `add_path()` via lyon. Vertices and indices accumulate. On `flush()`, one draw call if pipeline unchanged, otherwise split by pipeline key. Also handles `DrawDRRect` (tessellated as outer - inner), `DrawPoints`, and `DrawVertices` (pre-tessellated).

### TextBatcher

Stores `PreparedTextRun` (cache key + position + color). On `flush()`, delegates to `TextSystem::prepare()` then stores a `BatchedDraw::Text { pass_data_index }` referencing the prepared state in TextSystem.

### ImageBatcher

Accumulates `TextureInstance` entries. Groups by texture_id to minimize bind group switches. On `flush()`, one instanced draw call per unique texture.

### EffectBatcher

Accumulates gradient, shadow, and blur instances. Gradients are instanced (one draw per type). Blur uses multi-pass: downsample chain then upsample chain via offscreen textures from `TexturePool`.

### CompositingBatcher

Handles `SaveLayer`/`RestoreLayer`, `ShaderMask`, `BackdropFilter`, `ColorFilter`, `ImageFilter`. Manages offscreen render targets via `TexturePool`:

1. On `SaveLayer`: allocate offscreen texture from pool, push as current render target
2. Subsequent draw commands render to the offscreen texture
3. On `RestoreLayer`: composite offscreen texture back to parent target with blend mode + opacity
4. `ShaderMask`/`BackdropFilter`/`ColorFilter`/`ImageFilter`: similar pattern with additional shader pass between render and composite

### BatchedDraw

```rust
pub(crate) enum BatchedDraw {
    Instanced {
        pipeline: PipelineId,
        vertex_buffer: BufferSlice,
        instance_buffer: BufferSlice,
        instance_count: u32,
    },
    Indexed {
        pipeline: PipelineId,
        vertex_buffer: BufferSlice,
        index_buffer: BufferSlice,
        index_count: u32,
    },
    Text {
        /// Index into TextSystem's prepared pass data for this flush.
        /// TextSystem::render() uses this to draw the correct text runs.
        pass_index: u32,
    },
    SetScissor(ScissorRect),   // physical pixels, u32 coords for wgpu
    ClearScissor,
    PushRenderTarget {
        /// Offscreen texture from TexturePool
        texture_index: u32,
    },
    PopRenderTarget {
        /// Composite offscreen back to parent with given pipeline
        pipeline: PipelineId,
        blend_mode: BlendMode,
        opacity: f32,
    },
}

/// Scissor rect in physical pixels (what wgpu expects)
pub(crate) struct ScissorRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}
```

### Per-Layer Flush

A "layer boundary" is defined as: after all children of a compositing layer (ClipRect, Opacity, Transform, etc.) have been rendered. At that point, all batchers flush accumulated instances into `Vec<BatchedDraw>`, sorted by pipeline key within the layer. This preserves painter's order between layers (layers render in tree order) while minimizing draw calls within a layer (same-pipeline instances merged). Expected: 6-12 draw calls per frame for typical UI.

Note: Leaf CanvasLayers do NOT trigger a flush on their own -- only compositing layer boundaries do. Multiple consecutive CanvasLayers within the same parent accumulate into the same batch set.

### Zero-Allocation Hot Path

To meet the "zero allocations after init" target, `Vec`s used by batchers and the command buffer are pre-allocated in `RenderSurface` and lent to `FrameEncoder` via `std::mem::take()` / swap:

```rust
impl RenderSurface {
    fn begin_frame(&mut self) -> FrameEncoder<'_> {
        FrameEncoder {
            surface: self,
            gpu: Arc::clone(&self.gpu),
            shapes: ShapeBatcher::reuse(&mut self.shape_pool),  // takes pre-allocated Vecs
            commands: std::mem::take(&mut self.command_pool),     // reuse Vec capacity
            // ...
        }
    }
}

impl FrameEncoder<'_> {
    fn finish(self) {
        // ... submit to GPU ...
        // Return Vecs back to RenderSurface for reuse
        self.surface.command_pool = self.commands;  // capacity preserved
        self.surface.shape_pool = self.shapes.into_pool();
    }
}
```

## Text System

### Components

- **TextSystem** -- owns glyphon FontSystem, SwashCache, TextAtlas, TextRenderer. Lives in `GpuDevice` (shared across windows).
- **ShapeCache** -- LRU cache of shaped text buffers, keyed by (text_hash, font_size, font_family, font_weight)
- **GlyphAtlas** -- managed by glyphon internally, GPU texture for rasterized glyphs

### Flow

1. `TextBatcher::add_run()` -- shapes text via TextSystem, stores cache key
2. ShapeCache hit rate ~90%+ for UI (same labels rendered every frame)
3. `flush()` -- TextSystem::prepare() rasterizes new glyphs, uploads to atlas
4. TextSystem::render() draws textured quads into the render pass, using pass_index from BatchedDraw::Text

### Cache Eviction

LRU with 120-frame TTL (~2 seconds at 60fps). Max 1024 entries default.

### Font Loading

System fonts loaded via cosmic-text. Embedded NotoSans-Regular as fallback for headless/CI.

### Out of Scope (v1)

- Rich text spans (mixed styles in one run) -- DrawTextSpan logs warning, renders plain text
- Text selection / cursor positioning (flui-interaction responsibility)
- Bidirectional text testing
- Custom font loading API
- External texture registry (video/camera) -- deferred to v2

## Pipeline Registry

### Static Compilation

All render pipelines created at GpuDevice init. Shaders loaded via `include_str!()`. Expected init time: ~5-15ms.

### Pipeline Types

| PipelineId | Shader | Use |
|---|---|---|
| RectInstanced | rect_instanced.wgsl | Rects and rounded rects |
| CircleInstanced | circle_instanced.wgsl | Circles and ovals via SDF |
| ArcInstanced | arc_instanced.wgsl | Arc segments |
| PathFill | fill.wgsl | Tessellated path fills |
| PathStroke | shape.wgsl | Tessellated path strokes |
| Image | texture_instanced.wgsl | Textured image quads |
| LinearGradient | gradients/linear.wgsl | Linear gradients |
| RadialGradient | gradients/radial.wgsl | Radial gradients |
| Shadow | effects/shadow.wgsl | Drop shadows |
| BlurDownsample | effects/blur_downsample.wgsl | Blur downsample pass |
| BlurUpsample | effects/blur_upsample.wgsl | Blur upsample pass |
| Compositing | (simple textured quad) | Offscreen target composite |

### Shared Uniforms

```rust
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct FrameUniforms {
    viewport_size: [f32; 2],    // physical pixels
    scale_factor: f32,           // logical-to-physical multiplier
    _padding: f32,
}
```

One uniform buffer per frame, shared across all pipelines via bind group 0.

## Testing Strategy

### Unit Tests (no GPU, fast)

Test all logic that does not require GPU:
- **Batchers**: verify merging, instance counts, flush behavior
- **StateStack**: push/pop transform, clip, opacity composition
- **ShapeCache**: LRU eviction, cache hit/miss
- **Vertex types**: bytemuck layout, construction helpers
- **TextCacheKey**: hash stability, equality
- **Dispatch**: verify all DrawCommand variants are handled (exhaustive match)
- **Layer traversal**: verify all Layer variants are handled

### Integration Tests (headless GPU, feature-gated)

```toml
[features]
gpu-tests = []

[dev-dependencies]
criterion = { version = "0.5", features = ["html_reports"] }
```

Tests behind `#[cfg(feature = "gpu-tests")]`:
- Frame renders without error
- All pipeline variants create successfully
- Resize to zero/large dimensions
- Scene with mixed primitives renders
- Text rendering produces non-empty output
- Surface lost recovery (resize + retry)
- Multi-surface from shared GpuDevice

### Criterion Benchmarks

```
benches/engine_bench.rs
```

Benchmark functions:
- `bench_1000_rects` -- instanced rectangle rendering throughput
- `bench_text_rendering` -- 100 text runs
- `bench_mixed_ui` -- realistic UI scene (rects + text + images + gradients)
- `bench_deep_layer_tree` -- 50 nested layers
- `bench_batching_sort` -- sort 10k draw items

### Profiling

Tracing spans on every phase (`render_scene`, `render_layer`, `flush_batchers`, `gpu_submit`).

```bash
# Criterion
cargo bench -p flui-engine

# perf (Linux)
cargo build --release -p flui-engine --example bench_scene
perf record --call-graph=dwarf ./target/release/examples/bench_scene
perf report

# cargo-asm for hot functions
cargo asm -p flui-engine "ShapeBatcher::flush"
cargo asm -p flui-engine "FrameEncoder::dispatch_command"

# flamegraph
cargo flamegraph --bin bench_scene -p flui-engine
```

## Performance Targets

| Metric | Target | Notes |
|---|---|---|
| Draw calls per frame | 6-12 | Typical UI with mixed primitives |
| 1000 rects | < 1ms | Instanced rendering |
| Frame budget | < 16ms | 60fps target (constitution requirement) |
| Text cache hit rate | > 90% | For stable UI (labels, buttons) |
| Init time | < 50ms | Pipeline compilation + font loading |
| Buffer reuse | > 80% | After first few frames |
| Zero allocations | hot path | Pre-allocated Vecs swapped between RenderSurface and FrameEncoder |

## Constitution Compliance

- **I. Flutter as Reference**: Adapted layer tree composition from Flutter, not copied
- **II. Strict DAG**: flui-engine depends on flui-types, flui-foundation, flui-painting, flui-layer (downward only)
- **III. Zero Unsafe in Widget Layer**: unsafe only in `RenderSurface::new()` for `create_surface_unsafe` (wgpu 25.x requires this). Each unsafe block has `// SAFETY:` comment per constitution rules
- **IV. Composition Over Inheritance**: Batcher trait + concrete types, no dyn dispatch in hot path
- **V. Declarative API**: Scene is immutable input, engine is imperative consumer
- **Performance**: On-demand rendering, 60fps target, zero hot-path allocations via Vec pooling
- **Rendering Pipeline**: flui-painting records, flui-engine consumes and renders (boundary rules respected)
- **Text**: cosmic-text/glyphon as specified
- **Shaders**: WGSL stored in flui-engine crate. Note: constitution section "Rendering Backend" mentions `flui-painting/shaders/` but the "Rendering Pipeline" boundary rules section clarifies that flui-engine "owns all GPU state (buffers, textures, shaders, pipelines)". Shaders are GPU-specific and belong in flui-engine. A constitution PATCH amendment (clarification) is recommended to align the shader location reference.
- **Backend abstraction**: Constitution mentions `trait PaintBackend` in flui-painting. This redesign removes `CommandRenderer` from flui-engine but does not modify flui-painting. If `PaintBackend` exists in flui-painting, it remains untouched. If it does not exist yet, its creation is out of scope for this spec. A constitution review is recommended to clarify whether `PaintBackend` is a current requirement or a future goal.
- **Testing**: Unit + integration + benchmarks with criterion
