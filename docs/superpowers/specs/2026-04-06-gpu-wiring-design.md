# GPU Wiring — Full Draw Call Submission

**Date:** 2026-04-06
**Status:** Approved
**Scope:** Wire all 6 batchers to actual GPU rendering in FrameEncoder::finish()
**Depends on:** flui-engine redesign spec (2026-04-06-flui-engine-redesign.md)

## Summary

Connect the new modular engine architecture to actual GPU rendering. Currently FrameEncoder::finish() only clears the screen. This spec covers: uploading batcher data to GPU buffers, adapting all shaders to new vertex layouts, executing draw calls for all primitive types, integrating glyphon text rendering, adding headless render-to-texture tests, and creating a visual demo example.

## Design Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Scope | Full wiring (all 6 batchers) | Complete rendering pipeline in one pass |
| Shader approach | Adapt existing WGSL from src/wgpu/shaders/ | 18 shaders already exist, minimal adaptation needed |
| Text integration | Glyphon inside main render pass | One pass, glyphon renders after shapes/effects |
| Draw order (v1) | By type (rects → circles → paths → images → effects → text) | Simple, works for 90% UI. Per-layer interleaving is a follow-up |
| Compositing (v1) | SaveLayer/RestoreLayer with opacity only | ShaderMask/BackdropFilter fallback with warning |
| Verification | Headless pixel readback tests + visual winit example | CI automation + manual visual check |

## Architecture

### GPU Infrastructure Additions

**GpuDevice** gains static shared buffers:
- `unit_quad_vbo: wgpu::Buffer` — 4 vertices `[f32; 2]`, 32 bytes, never changes
- `unit_quad_ibo: wgpu::Buffer` — 6 indices `u16`, 12 bytes, never changes
- `text_system: parking_lot::Mutex<TextSystem>` — glyphon font system + atlas + renderer

**RenderSurface** gains viewport uniform:
- `viewport_buffer: wgpu::Buffer` — FrameUniforms (16 bytes), updated on resize()
- `viewport_bind_group: wgpu::BindGroup` — bound at group(0) for all pipelines

### Shader Adaptation

All shaders use unified Viewport uniform at `@group(0) @binding(0)`:
```wgsl
struct Viewport {
    size: vec2<f32>,
    _padding: vec2<f32>,
}
@group(0) @binding(0) var<uniform> viewport: Viewport;
```

Shader-to-pipeline mapping:

| Pipeline | Shader | Instance Layout Adaptation |
|---|---|---|
| RectInstanced | rect_instanced.wgsl | Verify existing — should already match RectInstance |
| CircleInstanced | circle_instanced.wgsl | Adapt to CircleInstance (center[2], radius[2], color[4], transform[4]) |
| ArcInstanced | arc_instanced.wgsl | Adapt to ArcInstance layout |
| PathFill | fill.wgsl | Non-instanced, PathVertex (position[2] + color[4]) |
| PathStroke | shape.wgsl | Same as PathFill |
| Image | texture_instanced.wgsl | Adapt to ImageQuadInstance, add texture sampler at group(1) |
| LinearGradient | gradients/linear.wgsl | Adapt uniform bindings |
| RadialGradient | gradients/radial.wgsl | Adapt uniform bindings |
| Shadow | effects/shadow.wgsl | Adapt to ShadowInstance layout |
| BlurDownsample | effects/blur_downsample.wgsl | Offscreen texture I/O |
| BlurUpsample | effects/blur_upsample.wgsl | Offscreen texture I/O |
| Compositing | New simple textured quad | For offscreen composite |

### FrameEncoder::finish() Full Flow

```
finish(self)
|
+-- 1. PREPARE PHASE (before render pass)
|   +-- Upload viewport uniform via queue.write_buffer()
|   +-- For each non-empty batcher:
|   |     Allocate GPU buffer from BufferPool
|   |     queue.write_buffer(buffer, 0, bytemuck::cast_slice(&instances))
|   +-- TextSystem::prepare(text_runs, device, queue) -> glyphon atlas upload
|   +-- Allocate offscreen textures for compositing (if needed)
|
+-- 2. RENDER PHASE (single render pass)
|   +-- set_viewport()
|   +-- bind viewport uniform (group 0)
|   +-- // Shapes (instanced)
|   +-- if rects: set_pipeline(Rect), set_vbo(0, quad), set_ibo(quad), set_vbo(1, rects), draw_indexed(6, N)
|   +-- if circles: set_pipeline(Circle), set_vbo(1, circles), draw_indexed(6, N)
|   +-- if arcs: set_pipeline(Arc), set_vbo(1, arcs), draw_indexed(6, N)
|   +-- // Paths (indexed, non-instanced)
|   +-- if paths: set_pipeline(PathFill), set_vbo(0, path_verts), set_ibo(path_indices),
|   |           for each draw_range: draw_indexed(count, 1)
|   +-- // Images (instanced per texture group)
|   +-- for each texture_group:
|   |     set_pipeline(Image), bind_texture(group 1), set_vbo(1, instances), draw_indexed(6, N)
|   +-- // Effects
|   +-- if linear_gradients: set_pipeline(LinearGradient), draw
|   +-- if radial_gradients: set_pipeline(RadialGradient), draw
|   +-- if shadows: set_pipeline(Shadow), set_vbo(1, shadows), draw_indexed(6, N)
|   +-- // Text (glyphon, last before compositing)
|   +-- TextSystem::render(&mut render_pass)
|   +-- // Scissor rects applied via set_scissor_rect() interleaved with draws
|
+-- 3. SUBMIT
|   +-- encoder.finish() -> CommandBuffer
|   +-- queue.submit()
|   +-- surface_texture.present()
|
+-- 4. CLEANUP
    +-- Return allocated buffers to BufferPool
    +-- TextSystem::trim(current_frame)
```

### Text System

**TextSystem** wraps glyphon:
- `FontSystem` — cosmic-text font loading (system + embedded Roboto fallback)
- `SwashCache` — glyph rasterization cache
- `TextAtlas` — GPU glyph atlas
- `TextRenderer` — glyphon draw submission
- `Viewport` — glyphon viewport for coordinate conversion

Flow:
1. TextBatcher collects PreparedTextRun (cache_key + position + color)
2. In prepare phase: create glyphon Buffers, shape text, call `text_renderer.prepare()`
3. In render phase: call `text_renderer.render(&mut render_pass)`

Lives in `GpuDevice` behind `Mutex<TextSystem>` (shared across windows).

### Compositing (v1)

SaveLayer/RestoreLayer:
1. PushRenderTarget: allocate offscreen texture from TexturePool, end current render pass, begin new pass targeting offscreen texture
2. Draw into offscreen texture
3. PopRenderTarget: end offscreen pass, resume main pass, draw offscreen texture as textured quad with opacity

ShaderMask, BackdropFilter, ColorFilter, ImageFilter: render without effect, log warning. Full implementation deferred to v2.

### Headless Render-to-Texture

```rust
impl GpuDevice {
    pub fn create_render_texture(&self, w: u32, h: u32) -> (wgpu::Texture, wgpu::TextureView);
}

pub struct HeadlessFrameEncoder { /* renders to texture, not surface */ }

pub fn read_texture_to_rgba(gpu: &GpuDevice, texture: &wgpu::Texture, w: u32, h: u32) -> Vec<u8>;
```

Texture created with `RENDER_ATTACHMENT | COPY_SRC`. Readback via staging buffer with `COPY_DST | MAP_READ`.

### Visual Example

`examples/render_demo.rs`: winit window with GpuDevice + RenderSurface, renders a scene with:
- Grid of 100 colored rects (10x10)
- 20 circles with varying sizes
- Text labels ("Hello FLUI", fps counter)
- Linear gradient background rect
- A few shadows

Event loop: render on RedrawRequested, resize on Resized.

## Pipeline Updates

### PipelineRegistry

Add `get()` method (if missing) and update each `create_*_pipeline()` to use the correct shader with matching vertex buffer layouts.

### Per-pipeline vertex buffer layout

| Pipeline | Buffer 0 (vertex) | Buffer 1 (instance) |
|---|---|---|
| RectInstanced | unit_quad [f32;2] step=Vertex | RectInstance 64B step=Instance |
| CircleInstanced | unit_quad [f32;2] step=Vertex | CircleInstance 48B step=Instance |
| ArcInstanced | unit_quad [f32;2] step=Vertex | ArcInstance 48B step=Instance |
| PathFill | PathVertex [f32;6] step=Vertex | (none) |
| PathStroke | PathVertex [f32;6] step=Vertex | (none) |
| Image | unit_quad [f32;2] step=Vertex | ImageQuadInstance 64B step=Instance |
| LinearGradient | unit_quad [f32;2] step=Vertex | (gradient data via uniform/storage) |
| RadialGradient | unit_quad [f32;2] step=Vertex | (gradient data via uniform/storage) |
| Shadow | unit_quad [f32;2] step=Vertex | ShadowInstance 48B step=Instance |

## Testing

### Headless GPU Tests (feature-gated: enable-wgpu-tests)

- `headless_render_rects_not_blank` — 100 rects, readback, assert not all white
- `headless_render_circles` — circles, verify non-blank
- `headless_render_text` — text, verify non-blank
- `headless_render_mixed_scene` — rects + text + gradients, verify non-blank
- `headless_render_with_clip` — clip rect, verify pixels outside clip are blank
- `headless_resize_and_render` — resize, re-render, verify dimensions correct

### Criterion Benchmarks (extend existing)

- `bench_finish_1000_rects` — full pipeline: scene → dispatch → upload → draw → submit
- `bench_finish_mixed_ui` — realistic scene end-to-end
- `bench_buffer_upload_10k_instances` — measure write_buffer throughput

## Out of Scope

- Per-layer draw call interleaving (v2 — current v1 draws by type)
- ShaderMask / BackdropFilter GPU effects (v2 — fallback with warning)
- Sweep gradients (v2)
- Path → lyon conversion for DrawPath command (existing dispatch logs debug)
- Rich text spans / DrawTextSpan (logs warning)
- External texture registry wiring (video/camera)

## Files Modified

| File | Change |
|---|---|
| `context/gpu_device.rs` | Add unit_quad buffers, TextSystem, create_render_texture() |
| `context/render_surface.rs` | Add viewport_buffer, viewport_bind_group, update resize() |
| `frame/encoder.rs` | Full finish() implementation with prepare/render/submit/cleanup |
| `text/system.rs` | Full TextSystem with glyphon integration |
| `pipelines/shape_pipeline.rs` | Correct vertex layouts for circle, arc |
| `pipelines/path_pipeline.rs` | PathVertex layout |
| `pipelines/image_pipeline.rs` | ImageQuadInstance layout + texture bind group |
| `pipelines/gradient_pipeline.rs` | Gradient data bindings |
| `pipelines/shadow_pipeline.rs` | ShadowInstance layout |
| `pipelines/registry.rs` | Ensure get() method works |
| `shaders/circle_instanced.wgsl` | Adapt instance inputs |
| `shaders/arc_instanced.wgsl` | Adapt instance inputs |
| `shaders/fill.wgsl` | Adapt vertex inputs |
| `shaders/texture_instanced.wgsl` | Adapt + texture sampler |
| `shaders/gradients/linear.wgsl` | Adapt uniform bindings |
| `shaders/gradients/radial.wgsl` | Adapt uniform bindings |
| `shaders/effects/shadow.wgsl` | Adapt instance inputs |

### New Files

| File | Purpose |
|---|---|
| `context/headless_render.rs` | HeadlessFrameEncoder + read_texture_to_rgba |
| `examples/render_demo.rs` | Visual winit demo |
