# Engine Completion — Remaining GPU Features

**Date:** 2026-04-06
**Status:** Approved
**Scope:** 6 remaining engine features to complete GPU rendering pipeline
**Depends on:** flui-engine redesign + GPU wiring specs

## Summary

Complete the remaining GPU rendering features: offscreen compositing (SaveLayer/RestoreLayer), shader effects (ShaderMask, BackdropFilter), sweep gradients, image loading pipeline, stencil-based clipping, and buffer pooling optimization.

## 1. SaveLayer/RestoreLayer GPU Compositing

### Architecture

SaveLayer/RestoreLayer enable rendering a subtree to an offscreen texture, then compositing it back with opacity, blend mode, or effects applied.

### Flow

```
SaveLayer(bounds, paint)
  → Flush current batchers (emit DrawOps for pending work)
  → Allocate offscreen texture from TexturePool (matching bounds size)
  → End current render pass
  → Begin new render pass targeting offscreen texture (clear transparent)
  → Push new render target onto a target stack in FrameEncoder

[child draw commands render into offscreen texture]

RestoreLayer
  → Flush batchers
  → End offscreen render pass
  → Pop render target from stack
  → Resume parent render pass (main surface or outer offscreen)
  → Draw offscreen texture as a textured quad using Compositing pipeline
    with paint.opacity and paint.blend_mode
  → Release offscreen texture back to TexturePool
```

### Implementation

- Add `render_target_stack: Vec<RenderTarget>` to FrameEncoder
- `RenderTarget` holds: texture, view, width, height, clear_color
- Modify finish() to handle `DrawOp::PushRenderTarget` / `DrawOp::PopRenderTarget`
- The Compositing pipeline (already created as placeholder) renders a textured quad
- Need a compositing shader: simple textured quad with uniform opacity

### Nesting

SaveLayer can nest. The target stack handles this naturally — each SaveLayer pushes, each RestoreLayer pops. Maximum depth: 8 (log warning if exceeded).

## 2. ShaderMask and BackdropFilter

### ShaderMask

Renders child content to offscreen, then applies a shader mask:

```
ShaderMask(child, shader, bounds, blend_mode)
  → [Same as SaveLayer: render child to offscreen A]
  → Render shader (gradient/pattern) to offscreen B
  → Composite A masked by B back to parent
```

For v1: render child to offscreen, composite with mask shader applied as fragment shader uniform. The mask shader receives the gradient parameters and computes alpha per-pixel.

### BackdropFilter

Reads the current framebuffer content behind the element and applies a filter:

```
BackdropFilter(child, filter, bounds)
  → Copy current render target region (bounds) to texture C
  → Apply filter (blur) to texture C via multi-pass blur pipeline
  → Draw filtered C back to render target at bounds position
  → Render child on top
```

Requires: copy render target to texture (via command encoder), blur pipeline passes (already have blur_downsample/upsample shaders).

### Implementation notes

- ShaderMask needs two offscreen textures (content + mask)
- BackdropFilter needs copy_texture_to_texture for framebuffer capture
- Both use TexturePool for allocation
- Filter types supported: Blur (via existing blur shaders), ColorMatrix (via uniform)

## 3. Sweep Gradients

### Shader

```wgsl
// In fragment shader:
let dx = frag_pos.x - center.x;
let dy = frag_pos.y - center.y;
var t = atan2(dy, dx) / (2.0 * 3.14159265) + 0.5; // normalize to [0, 1]
t = (t - start_angle / (2.0 * PI)) % 1.0;         // apply start angle offset
// ... same gradient stop interpolation as linear/radial
```

### Implementation

- Add `SweepGradientInstance` to EffectBatcher (bounds, center, start_angle, end_angle, stops)
- Add `create_sweep_gradient_pipeline()` to gradient_pipeline.rs
- Add `SweepGradient` variant to PipelineId
- Wire in dispatch.rs (Shader::SweepGradient currently falls back to radial)
- Wire in encoder.rs finish()

## 4. Image Loading Pipeline

### Architecture

```
Image bytes (PNG/JPEG/etc)
  → image crate decode → RGBA pixels
  → wgpu texture upload → TextureCache (keyed by image ID)
  → DrawImage dispatch → ImageBatcher with real texture_id
  → finish(): bind texture from TextureCache, draw instanced quad
```

### GpuDevice additions

```rust
impl GpuDevice {
    /// Load image from raw bytes (PNG, JPEG, etc.) and upload to GPU.
    /// Returns a texture ID for use with DrawImage.
    pub fn load_image(&self, data: &[u8]) -> RenderResult<u64> {
        let img = image::load_from_memory(data)?;
        let rgba = img.to_rgba8();
        let (width, height) = rgba.dimensions();
        // Create wgpu texture, write pixels, store in TextureCache
        // Return unique texture ID
    }

    /// Load image from filesystem path.
    pub fn load_image_file(&self, path: &std::path::Path) -> RenderResult<u64> {
        let data = std::fs::read(path)?;
        self.load_image(&data)
    }
}
```

### Texture binding in finish()

ImageBatcher groups instances by texture_id. For each group:
1. Look up texture in TextureCache
2. Create bind group with texture view + sampler at group(1)
3. Set pipeline (Image), bind group, draw instanced

### Image pipeline bind group layout

Group(0): viewport uniform (shared)
Group(1): texture view + sampler

The image pipeline already has a texture bind group layout from Task 4 of GPU wiring. Wire it to actual textures.

## 5. Stencil-Based Clipping

### Current state

ClipRect uses hardware scissor (AABB only). ClipRRect and ClipPath log debug and fall through — no actual non-rectangular clipping.

### Architecture

Add a depth/stencil attachment to the render pass:

```rust
depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
    view: &stencil_view,
    depth_ops: None,  // not using depth
    stencil_ops: Some(wgpu::Operations {
        load: wgpu::LoadOp::Clear(0),
        store: wgpu::StoreOp::Store,
    }),
})
```

### Stencil clipping flow

```
ClipRRect (push):
  1. Set stencil state: write, always pass, ref = current_depth + 1
  2. Render rounded rect shape to stencil buffer (increments stencil value)
  3. Set stencil state: test, pass if stencil >= current_depth + 1
  4. Increment current_depth

ClipRRect (pop):
  1. Set stencil state: write, always pass, ref = current_depth
  2. Render rounded rect shape to stencil buffer (decrements/resets stencil)
  3. Set stencil state: test, pass if stencil >= current_depth
  4. Decrement current_depth
```

### Implementation

- Create stencil texture in RenderSurface (Depth24PlusStencil8 format)
- Add stencil_depth counter to StateStack
- Add StencilWrite / StencilTest pipeline variants (or dynamic stencil state)
- Modify ClipRRect/ClipPath handling in dispatch.rs to emit stencil ops
- Need a "stencil-only" shader that writes to stencil but not color

### ClipPath

Same as ClipRRect but tessellates the path via lyon and renders the tessellated geometry to stencil buffer.

## 6. Buffer Pooling in finish()

### Current state

`finish()` calls `device.create_buffer_init()` for every instance buffer every frame. This allocates new GPU memory each frame.

### Target

Use BufferPool (already exists in resources/) to reuse buffers:

```rust
// Before:
let rect_buf = self.gpu.device().create_buffer_init(&BufferInitDescriptor { ... });

// After:
let mut pool = self.gpu.buffer_pool().lock();
let rect_buf = pool.get_vertex_buffer(self.gpu.device(), data.len() as u64);
self.gpu.queue().write_buffer(rect_buf, 0, data);
```

### Implementation

- At start of finish(): `pool.reset()` (mark all buffers available)
- Replace all `create_buffer_init` with `pool.get_*_buffer()` + `queue.write_buffer()`
- BufferPool::get_vertex_buffer returns a buffer with size >= requested
- After submit: buffers stay in pool for next frame reuse

### Expected impact

~10-20% CPU reduction on buffer allocation overhead (matching benchmark from old painter.rs analysis).

## Testing

### Unit tests (no GPU)
- Stencil depth push/pop in StateStack
- SweepGradient stop interpolation
- Image texture ID generation

### Integration tests (feature-gated)
- SaveLayer/RestoreLayer with opacity renders correctly
- Stencil clip produces non-rectangular mask
- Image loads and renders
- Buffer pool reuse after multiple frames

### Benchmark additions
- `bench_savelayer_nested` — 10 nested SaveLayer/RestoreLayer
- `bench_finish_with_pool` — compare pooled vs non-pooled buffer allocation

## Out of Scope

- BlendMode GPU implementation (uses wgpu blend state, not custom shader)
- Texture atlas for small images (future optimization)
- Async image loading (future)
- HDR rendering
