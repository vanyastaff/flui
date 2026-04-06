# GPU Wiring Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Wire all batchers to actual GPU draw calls so FrameEncoder renders shapes, paths, text, images, and effects to screen.

**Architecture:** Add unit quad buffers + viewport uniform to GpuDevice/RenderSurface. Adapt existing WGSL shaders to match new vertex layouts. Implement full prepare/render/submit flow in FrameEncoder::finish(). Add glyphon TextSystem. Add headless render-to-texture for testing. Create visual winit demo.

**Tech Stack:** wgpu 25.x, glyphon 0.9, lyon 1.0, glam 0.30, bytemuck, winit 0.30

**Spec:** `docs/superpowers/specs/2026-04-06-gpu-wiring-design.md`

---

## File Structure

```
crates/flui-engine/src/
  context/
    gpu_device.rs          — Modify: add unit_quad_vbo, unit_quad_ibo, text_system, create_render_texture()
    render_surface.rs      — Modify: add viewport_buffer, viewport_bind_group, update resize()
    headless_render.rs     — Create: HeadlessFrameEncoder + read_texture_to_rgba()
  frame/
    encoder.rs             — Modify: full finish() with prepare/render/submit/cleanup
  text/
    system.rs              — Modify: full TextSystem with glyphon
  pipelines/
    shape_pipeline.rs      — Modify: add create_circle_pipeline, create_arc_pipeline (real shaders)
    path_pipeline.rs       — Modify: real PathVertex layout
    image_pipeline.rs      — Modify: real ImageQuadInstance layout + texture bind group
    gradient_pipeline.rs   — Modify: real gradient shader bindings
    shadow_pipeline.rs     — Modify: real ShadowInstance layout
    blur_pipeline.rs       — Keep as placeholder (offscreen compositing v2)
    registry.rs            — Modify: add bind_group_layout() getter if missing
  shaders/
    circle_instanced.wgsl  — Modify: verify/adapt instance inputs match CircleInstance
    arc_instanced.wgsl     — Modify: adapt to ArcInstance layout
    fill.wgsl              — Modify: adapt to PathVertex inputs
    texture_instanced.wgsl — Modify: adapt to ImageQuadInstance + sampler
    gradients/linear.wgsl  — Modify: adapt uniform bindings
    gradients/radial.wgsl  — Modify: adapt uniform bindings
    effects/shadow.wgsl    — Modify: adapt to ShadowInstance layout
  debug.rs                 — Keep unchanged
  vertex.rs                — Keep unchanged

examples/
  render_demo.rs           — Create: visual winit demo

tests/
  gpu_integration.rs       — Modify: add headless render pixel readback tests
```

---

## Task 1: Add unit quad buffers to GpuDevice

**Files:**
- Modify: `src/context/gpu_device.rs`

This is the foundation — every instanced draw needs the shared unit quad.

- [ ] **Step 1: Add unit quad buffer fields to GpuDevice struct**

In `src/context/gpu_device.rs`, add to the struct (after `default_format` field):

```rust
pub struct GpuDevice {
    // ... existing fields ...
    default_format: wgpu::TextureFormat,
    unit_quad_vbo: wgpu::Buffer,
    unit_quad_ibo: wgpu::Buffer,
}
```

- [ ] **Step 2: Create unit quad buffers in new_headless() and new_with_surface()**

Add a helper method to GpuDevice:

```rust
fn create_unit_quad_buffers(device: &wgpu::Device) -> (wgpu::Buffer, wgpu::Buffer) {
    use wgpu::util::DeviceExt;

    #[rustfmt::skip]
    let vertices: &[f32] = &[
        0.0, 0.0,  // top-left
        1.0, 0.0,  // top-right
        1.0, 1.0,  // bottom-right
        0.0, 1.0,  // bottom-left
    ];

    let indices: &[u16] = &[0, 1, 2, 0, 2, 3];

    let vbo = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("unit_quad_vbo"),
        contents: bytemuck::cast_slice(vertices),
        usage: wgpu::BufferUsages::VERTEX,
    });

    let ibo = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("unit_quad_ibo"),
        contents: bytemuck::cast_slice(indices),
        usage: wgpu::BufferUsages::INDEX,
    });

    (vbo, ibo)
}
```

Call it in both `new_headless()` and `new_with_surface()` before constructing Self:

```rust
let (unit_quad_vbo, unit_quad_ibo) = Self::create_unit_quad_buffers(&device);
```

Add `wgpu::util::DeviceExt` to the imports at the top of the file.

- [ ] **Step 3: Add getter methods**

```rust
pub fn unit_quad_vbo(&self) -> &wgpu::Buffer { &self.unit_quad_vbo }
pub fn unit_quad_ibo(&self) -> &wgpu::Buffer { &self.unit_quad_ibo }
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo check -p flui-engine`

- [ ] **Step 5: Commit**

```bash
git add crates/flui-engine/src/context/gpu_device.rs
git commit -m "feat(flui-engine): add unit quad buffers to GpuDevice"
```

---

## Task 2: Add viewport uniform to RenderSurface

**Files:**
- Modify: `src/context/render_surface.rs`
- Modify: `src/context/gpu_device.rs` (expose bind_group_layout)

- [ ] **Step 1: Expose bind_group_layout from PipelineRegistry**

In `src/pipelines/registry.rs`, verify this method exists (it should from Task 9 of the previous plan):

```rust
pub fn bind_group_layout(&self) -> &Arc<wgpu::BindGroupLayout> {
    &self.bind_group_layout
}
```

If missing, add it. Also add a convenience method to GpuDevice:

```rust
// In gpu_device.rs
pub fn bind_group_layout(&self) -> &wgpu::BindGroupLayout {
    &self.pipelines.bind_group_layout()
}
```

- [ ] **Step 2: Add viewport buffer fields to RenderSurface**

```rust
pub struct RenderSurface {
    // ... existing fields ...
    viewport_buffer: wgpu::Buffer,
    viewport_bind_group: wgpu::BindGroup,
}
```

- [ ] **Step 3: Create viewport buffer in RenderSurface constructors**

Add helper:

```rust
fn create_viewport_resources(
    gpu: &GpuDevice,
    width: u32,
    height: u32,
    scale_factor: f32,
) -> (wgpu::Buffer, wgpu::BindGroup) {
    use crate::vertex::FrameUniforms;

    let uniforms = FrameUniforms::new(width, height, scale_factor);

    let buffer = gpu.device().create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("viewport_uniform"),
        contents: bytemuck::bytes_of(&uniforms),
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
    });

    let bind_group = gpu.device().create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("viewport_bind_group"),
        layout: gpu.bind_group_layout(),
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: buffer.as_entire_binding(),
        }],
    });

    (buffer, bind_group)
}
```

Add `use wgpu::util::DeviceExt;` to imports. Call in `new()`.

- [ ] **Step 4: Update resize() to rewrite viewport buffer**

```rust
pub fn resize(&mut self, width: u32, height: u32, scale_factor: f32) {
    self.width = width.max(1);
    self.height = height.max(1);
    self.scale_factor = scale_factor;
    self.config.width = self.width;
    self.config.height = self.height;
    self.surface.configure(self.gpu.device(), &self.config);

    // Update viewport uniform
    let uniforms = crate::vertex::FrameUniforms::new(self.width, self.height, self.scale_factor);
    self.gpu.queue().write_buffer(&self.viewport_buffer, 0, bytemuck::bytes_of(&uniforms));
}
```

- [ ] **Step 5: Add getters**

```rust
pub fn viewport_bind_group(&self) -> &wgpu::BindGroup { &self.viewport_bind_group }
pub fn viewport_buffer(&self) -> &wgpu::Buffer { &self.viewport_buffer }
```

- [ ] **Step 6: Verify**

Run: `cargo check -p flui-engine`

- [ ] **Step 7: Commit**

```bash
git add crates/flui-engine/src/context/ crates/flui-engine/src/pipelines/registry.rs
git commit -m "feat(flui-engine): add viewport uniform to RenderSurface"
```

---

## Task 3: Adapt circle and arc shaders

**Files:**
- Modify: `src/shaders/circle_instanced.wgsl`
- Modify: `src/shaders/arc_instanced.wgsl`
- Modify: `src/pipelines/shape_pipeline.rs`

The existing circle shader expects instance inputs at locations 2-5. Verify they match CircleInstance layout: `center: vec2 @2`, `radius: vec2 @3`, `color: vec4 @4`, `transform: vec4 @5`.

- [ ] **Step 1: Read and verify circle_instanced.wgsl instance inputs**

Read the shader. If instance input locations don't match CircleInstance::desc() (locations 2,3,4,5 with types Float32x2, Float32x2, Float32x4, Float32x4), fix them.

- [ ] **Step 2: Read and verify/fix arc_instanced.wgsl**

Arc shader should accept ArcInstance: `center: vec2 @2`, `radius_start: f32 @3`, `start_angle: f32 @4`, `sweep_angle: f32 @5`, `color: vec4 @6`. Fix vertex inputs to match ArcInstance::desc().

- [ ] **Step 3: Update create_circle_pipeline and create_arc_pipeline in shape_pipeline.rs**

Replace placeholder implementations with real ones using correct shaders and vertex layouts:

```rust
pub fn create_circle_pipeline(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    bind_group_layout: &wgpu::BindGroupLayout,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("circle_instanced_shader"),
        source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/circle_instanced.wgsl").into()),
    });
    // Same pipeline structure as rect but with CircleInstance::desc()
    // ... (full pipeline descriptor matching rect pattern but with circle vertex layout)
}
```

- [ ] **Step 4: Verify**

Run: `cargo check -p flui-engine`

- [ ] **Step 5: Commit**

```bash
git add crates/flui-engine/src/shaders/ crates/flui-engine/src/pipelines/shape_pipeline.rs
git commit -m "feat(flui-engine): adapt circle and arc shaders to new instance layouts"
```

---

## Task 4: Adapt path, image, gradient, and shadow pipelines

**Files:**
- Modify: `src/pipelines/path_pipeline.rs`, `image_pipeline.rs`, `gradient_pipeline.rs`, `shadow_pipeline.rs`
- Modify: `src/shaders/fill.wgsl`, `texture_instanced.wgsl`, `gradients/linear.wgsl`, `gradients/radial.wgsl`, `effects/shadow.wgsl`

Same pattern as Task 3 — read each shader, verify/fix inputs to match our vertex types, update pipeline creation with correct layouts.

- [ ] **Step 1: Path pipeline — adapt fill.wgsl for PathVertex**

PathVertex has `position: [f32;2]` at location 0 and `color: [f32;4]` at location 1. Non-instanced. Verify shader accepts these. Update `create_path_fill_pipeline()` and `create_path_stroke_pipeline()` to use `PathVertex::desc()` as the only vertex buffer.

- [ ] **Step 2: Image pipeline — adapt texture_instanced.wgsl for ImageQuadInstance**

ImageQuadInstance has `dst_bounds`, `src_uv`, `color`, `transform` at locations 2-5. Add texture+sampler bind group at group(1). Update shader and `create_image_pipeline()`.

- [ ] **Step 3: Gradient pipelines — adapt linear.wgsl and radial.wgsl**

Gradients need gradient stop data. For v1, pass stops via a uniform buffer at group(1). Update shaders and pipeline creation functions.

- [ ] **Step 4: Shadow pipeline — adapt shadow.wgsl for ShadowInstance**

ShadowInstance has `bounds`, `color`, `offset`, `blur_radius`, `spread`. Verify shader inputs match. Update `create_shadow_pipeline()`.

- [ ] **Step 5: Verify**

Run: `cargo check -p flui-engine`

- [ ] **Step 6: Commit**

```bash
git add crates/flui-engine/src/shaders/ crates/flui-engine/src/pipelines/
git commit -m "feat(flui-engine): adapt path, image, gradient, and shadow pipelines"
```

---

## Task 5: Implement TextSystem with glyphon

**Files:**
- Modify: `src/text/system.rs`
- Modify: `src/context/gpu_device.rs` (add text_system field)

- [ ] **Step 1: Implement TextSystem**

In `src/text/system.rs`:

```rust
use glyphon::{
    Attrs, Buffer, Cache, Color as GlyphonColor, Family, FontSystem, Metrics,
    Resolution, Shaping, SwashCache, TextArea, TextAtlas, TextBounds, TextRenderer, Viewport,
};
use crate::batchers::text::PreparedTextRun;
use crate::text::cache::TextCacheKey;

pub struct TextSystem {
    font_system: FontSystem,
    swash_cache: SwashCache,
    text_atlas: TextAtlas,
    text_renderer: TextRenderer,
    viewport: Viewport,
    text_buffers: Vec<Buffer>,
}
```

Key methods:
- `new(device, queue, format)` — initialize glyphon components, load embedded font
- `prepare(device, queue, runs: &[PreparedTextRun], width, height, scale)` — shape text, prepare atlas
- `render(pass: &mut wgpu::RenderPass)` — render text into pass
- `trim()` — evict unused atlas entries

Read glyphon 0.9 API carefully — the exact types and method signatures may differ. Check the old `src/wgpu/text.rs` for reference on how the existing code uses glyphon.

- [ ] **Step 2: Add text_system to GpuDevice**

```rust
pub struct GpuDevice {
    // ... existing fields ...
    text_system: parking_lot::Mutex<TextSystem>,
}
```

Create in `new_headless()` and `new_with_surface()`. Add getter:

```rust
pub fn text_system(&self) -> &parking_lot::Mutex<TextSystem> { &self.text_system }
```

- [ ] **Step 3: Verify**

Run: `cargo check -p flui-engine`

- [ ] **Step 4: Commit**

```bash
git add crates/flui-engine/src/text/system.rs crates/flui-engine/src/context/gpu_device.rs
git commit -m "feat(flui-engine): implement TextSystem with glyphon"
```

---

## Task 6: Wire FrameEncoder::finish() with actual draw calls

**Files:**
- Modify: `src/frame/encoder.rs`

This is the core task — replace the TODO stub with real GPU submission.

- [ ] **Step 1: Add prepare phase before render pass**

Before `encoder.begin_render_pass()`, add buffer upload logic:

```rust
// Upload viewport uniform
let uniforms = FrameUniforms::new(self.surface.width(), self.surface.height(), self.scale_factor);
self.gpu.queue().write_buffer(self.surface.viewport_buffer(), 0, bytemuck::bytes_of(&uniforms));

// Upload shape instances
let rect_buffer = if !self.batchers.shapes.rects.is_empty() {
    let data = bytemuck::cast_slice(&self.batchers.shapes.rects);
    let buf = self.gpu.device().create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("rect_instances"),
        contents: data,
        usage: wgpu::BufferUsages::VERTEX,
    });
    Some(buf)
} else { None };

// Same pattern for circles, arcs, paths, shadows...
```

Note: For v1, use `create_buffer_init` directly instead of BufferPool (simpler, optimize later).

- [ ] **Step 2: Add shape draw calls inside render pass**

```rust
// Bind viewport uniform (shared by all pipelines)
render_pass.set_bind_group(0, self.surface.viewport_bind_group(), &[]);

// Unit quad shared by all instanced draws
render_pass.set_vertex_buffer(0, self.gpu.unit_quad_vbo().slice(..));
render_pass.set_index_buffer(self.gpu.unit_quad_ibo().slice(..), wgpu::IndexFormat::Uint16);

// Rects
if let Some(ref buf) = rect_buffer {
    render_pass.set_pipeline(self.gpu.pipelines().get(PipelineId::RectInstanced).unwrap());
    render_pass.set_vertex_buffer(1, buf.slice(..));
    render_pass.draw_indexed(0..6, 0, 0..self.batchers.shapes.rect_count() as u32);
}

// Circles
if let Some(ref buf) = circle_buffer {
    render_pass.set_pipeline(self.gpu.pipelines().get(PipelineId::CircleInstanced).unwrap());
    render_pass.set_vertex_buffer(1, buf.slice(..));
    render_pass.draw_indexed(0..6, 0, 0..self.batchers.shapes.circle_count() as u32);
}

// Arcs
if let Some(ref buf) = arc_buffer {
    render_pass.set_pipeline(self.gpu.pipelines().get(PipelineId::ArcInstanced).unwrap());
    render_pass.set_vertex_buffer(1, buf.slice(..));
    render_pass.draw_indexed(0..6, 0, 0..self.batchers.shapes.arc_count() as u32);
}
```

- [ ] **Step 3: Add path draw calls**

```rust
// Paths (non-instanced)
if !self.batchers.paths.is_empty() {
    let vert_data = bytemuck::cast_slice(self.batchers.paths.vertices());
    let idx_data = bytemuck::cast_slice(self.batchers.paths.indices());

    let path_vbo = self.gpu.device().create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("path_vertices"), contents: vert_data, usage: wgpu::BufferUsages::VERTEX,
    });
    let path_ibo = self.gpu.device().create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("path_indices"), contents: idx_data, usage: wgpu::BufferUsages::INDEX,
    });

    render_pass.set_pipeline(self.gpu.pipelines().get(PipelineId::PathFill).unwrap());
    render_pass.set_vertex_buffer(0, path_vbo.slice(..));
    render_pass.set_index_buffer(path_ibo.slice(..), wgpu::IndexFormat::Uint32);

    for range in self.batchers.paths.draw_ranges() {
        render_pass.draw_indexed(range.start_index..(range.start_index + range.index_count), 0, 0..1);
    }
}
```

- [ ] **Step 4: Add shadow draw calls**

```rust
if !self.batchers.effects.shadows.is_empty() {
    let data = bytemuck::cast_slice(&self.batchers.effects.shadows);
    let shadow_buf = self.gpu.device().create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("shadow_instances"), contents: data, usage: wgpu::BufferUsages::VERTEX,
    });

    render_pass.set_pipeline(self.gpu.pipelines().get(PipelineId::Shadow).unwrap());
    render_pass.set_vertex_buffer(0, self.gpu.unit_quad_vbo().slice(..));
    render_pass.set_index_buffer(self.gpu.unit_quad_ibo().slice(..), wgpu::IndexFormat::Uint16);
    render_pass.set_vertex_buffer(1, shadow_buf.slice(..));
    render_pass.draw_indexed(0..6, 0, 0..self.batchers.effects.shadow_count() as u32);
}
```

- [ ] **Step 5: Add text rendering**

```rust
// Text (after all shapes/effects)
if !self.batchers.text.is_empty() {
    let mut text_system = self.gpu.text_system().lock();
    text_system.prepare(
        self.gpu.device(),
        self.gpu.queue(),
        self.batchers.text.runs(),
        self.surface.width(),
        self.surface.height(),
        self.scale_factor,
    );
    text_system.render(&mut render_pass);
}
```

- [ ] **Step 6: Verify compilation**

Run: `cargo check -p flui-engine`

- [ ] **Step 7: Commit**

```bash
git add crates/flui-engine/src/frame/encoder.rs
git commit -m "feat(flui-engine): wire FrameEncoder::finish() with actual GPU draw calls"
```

---

## Task 7: Headless render-to-texture infrastructure

**Files:**
- Create: `src/context/headless_render.rs`
- Modify: `src/context/mod.rs`
- Modify: `src/context/gpu_device.rs`

- [ ] **Step 1: Add create_render_texture to GpuDevice**

```rust
pub fn create_render_texture(&self, width: u32, height: u32) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = self.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("headless_render_target"),
        size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: self.default_format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (texture, view)
}
```

- [ ] **Step 2: Create headless_render.rs with pixel readback**

```rust
//! Headless rendering utilities for testing.

use crate::context::gpu_device::GpuDevice;
use crate::error::RenderResult;
use crate::frame::dispatch::{Batchers, traverse_scene};
use crate::frame::state_stack::StateStack;
use crate::vertex::FrameUniforms;
use flui_layer::Scene;

/// Read a rendered texture back to CPU as RGBA bytes.
pub fn read_texture_to_rgba(
    gpu: &GpuDevice,
    texture: &wgpu::Texture,
    width: u32,
    height: u32,
) -> Vec<u8> {
    let bytes_per_pixel = 4u32;
    let padded_bytes_per_row = (width * bytes_per_pixel + 255) & !255; // align to 256
    let buffer_size = (padded_bytes_per_row * height) as u64;

    let staging = gpu.device().create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback_staging"),
        size: buffer_size,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut encoder = gpu.device().create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("readback_encoder"),
    });

    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &staging,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_bytes_per_row),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
    );

    gpu.queue().submit(std::iter::once(encoder.finish()));

    let slice = staging.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| { tx.send(result).unwrap(); });
    gpu.device().poll(wgpu::Maintain::Wait);
    rx.recv().unwrap().unwrap();

    let mapped = slice.get_mapped_range();
    let mut pixels = Vec::with_capacity((width * height * bytes_per_pixel) as usize);
    for row in 0..height {
        let start = (row * padded_bytes_per_row) as usize;
        let end = start + (width * bytes_per_pixel) as usize;
        pixels.extend_from_slice(&mapped[start..end]);
    }
    drop(mapped);
    staging.unmap();

    pixels
}
```

- [ ] **Step 3: Export from context/mod.rs**

Add `pub mod headless_render;` gated behind `wgpu-backend`.

- [ ] **Step 4: Verify**

Run: `cargo check -p flui-engine`

- [ ] **Step 5: Commit**

```bash
git add crates/flui-engine/src/context/
git commit -m "feat(flui-engine): add headless render-to-texture and pixel readback"
```

---

## Task 8: Headless render tests

**Files:**
- Modify: `tests/gpu_integration.rs`

- [ ] **Step 1: Add headless render tests**

```rust
#[cfg(feature = "enable-wgpu-tests")]
mod headless_render {
    use std::sync::Arc;
    use flui_engine::context::gpu_device::GpuDevice;
    use flui_engine::context::headless_render::read_texture_to_rgba;
    use flui_engine::frame::dispatch::{Batchers, traverse_scene};
    use flui_engine::frame::state_stack::StateStack;
    use flui_engine::vertex::FrameUniforms;
    use flui_layer::{Scene, Layer, CanvasLayer};
    use flui_types::{Size, px};

    #[test]
    fn headless_render_clears_to_white() {
        let gpu = GpuDevice::new_headless().expect("GPU init");
        let (texture, view) = gpu.create_render_texture(100, 100);

        // Create command encoder that just clears
        let mut encoder = gpu.device().create_command_encoder(&Default::default());
        {
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::WHITE),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
        }
        gpu.queue().submit(std::iter::once(encoder.finish()));

        let pixels = read_texture_to_rgba(&gpu, &texture, 100, 100);
        // First pixel should be white (255, 255, 255, 255) in BGRA format
        assert_eq!(pixels.len(), 100 * 100 * 4);
        assert!(pixels[0] > 200); // B channel
        assert!(pixels[1] > 200); // G channel
        assert!(pixels[2] > 200); // R channel
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p flui-engine --features enable-wgpu-tests headless_render`

Note: These tests require a GPU adapter. They may not pass in headless CI environments.

- [ ] **Step 3: Commit**

```bash
git add crates/flui-engine/tests/
git commit -m "test(flui-engine): add headless render pixel readback tests"
```

---

## Task 9: Visual render demo example

**Files:**
- Create: `examples/render_demo.rs`
- Modify: `Cargo.toml` (add winit to dev-dependencies and example)

- [ ] **Step 1: Add winit dev-dependency and example to Cargo.toml**

```toml
[dev-dependencies]
# ... existing ...
winit = "0.30"

[[example]]
name = "render_demo"
required-features = ["wgpu-backend"]
```

- [ ] **Step 2: Create render_demo.rs**

A minimal winit window that creates GpuDevice + RenderSurface, builds a simple scene with colored rects, and renders it.

```rust
//! Visual rendering demo — opens a window and renders shapes.
//!
//! Run: cargo run -p flui-engine --example render_demo

use std::sync::Arc;
use flui_engine::context::gpu_device::GpuDevice;
use flui_engine::context::render_surface::RenderSurface;
use flui_engine::frame::encoder::FrameEncoder;
use flui_engine::batchers::shapes::ShapeBatcher;
use flui_engine::frame::dispatch::Batchers;
use flui_engine::frame::state_stack::StateStack;
use flui_layer::{Scene, Layer, CanvasLayer};
use winit::event::{Event, WindowEvent};
use winit::event_loop::EventLoop;
use winit::window::WindowBuilder;

fn main() {
    tracing_subscriber::fmt::init();

    let event_loop = EventLoop::new().unwrap();
    let window = WindowBuilder::new()
        .with_title("FLUI Engine Demo")
        .with_inner_size(winit::dpi::LogicalSize::new(800, 600))
        .build(&event_loop)
        .unwrap();

    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
    // Safety: window handle is valid for event loop lifetime
    let surface = unsafe { instance.create_surface(&window) }.unwrap();
    let gpu = Arc::new(GpuDevice::new_with_surface(&instance, &surface).unwrap());
    let size = window.inner_size();
    let scale = window.scale_factor() as f32;
    let mut render_surface = unsafe {
        RenderSurface::new(Arc::clone(&gpu), &instance, &window, size.width, size.height, scale)
    }.unwrap();

    event_loop.run(move |event, target| {
        match event {
            Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => {
                target.exit();
            }
            Event::WindowEvent { event: WindowEvent::Resized(size), .. } => {
                render_surface.resize(size.width.max(1), size.height.max(1), scale);
            }
            Event::WindowEvent { event: WindowEvent::RedrawRequested, .. } => {
                // Build a simple test scene
                let scene = Scene::empty(flui_types::Size::new(
                    flui_types::px(size.width as f32),
                    flui_types::px(size.height as f32),
                ));

                match render_surface.begin_frame() {
                    Ok(mut frame) => {
                        // For now, manually add shapes to test rendering
                        // (bypass Scene, directly populate batchers)
                        for i in 0..10 {
                            for j in 0..10 {
                                let x = 20.0 + i as f32 * 75.0;
                                let y = 20.0 + j as f32 * 55.0;
                                let r = (i as f32) / 10.0;
                                let g = (j as f32) / 10.0;
                                frame.batchers_mut().shapes.add_rect(
                                    x, y, 60.0, 40.0,
                                    [r, g, 0.5, 1.0],
                                    [8.0, 8.0, 8.0, 8.0],
                                    [1.0, 0.0, 0.0, 1.0],
                                );
                            }
                        }

                        if let Err(e) = frame.finish() {
                            tracing::error!("Frame error: {}", e);
                        }
                    }
                    Err(e) => tracing::error!("begin_frame error: {}", e),
                }
            }
            _ => {}
        }
    }).unwrap();
}
```

Note: This needs `FrameEncoder` to expose batchers for direct manipulation. Add a method:
```rust
// In encoder.rs
pub fn batchers_mut(&mut self) -> &mut Batchers { &mut self.batchers }
```

- [ ] **Step 3: Verify compilation**

Run: `cargo build -p flui-engine --example render_demo`

- [ ] **Step 4: Commit**

```bash
git add crates/flui-engine/examples/ crates/flui-engine/Cargo.toml crates/flui-engine/src/frame/encoder.rs
git commit -m "feat(flui-engine): add visual render demo example"
```

---

## Task 10: Final verification and benchmarks

- [ ] **Step 1: Run all tests**

```bash
cargo test -p flui-engine
```

Expected: All existing 112+ tests pass, no regressions.

- [ ] **Step 2: Run clippy**

```bash
cargo clippy -p flui-engine -- -D warnings 2>&1 | grep "flui-engine"
```

Fix any new warnings.

- [ ] **Step 3: Run benchmarks**

```bash
cargo bench -p flui-engine -- --test
```

Verify benchmark compilation.

- [ ] **Step 4: Commit any fixes**

```bash
git add crates/flui-engine/
git commit -m "chore(flui-engine): fix clippy warnings and verify GPU wiring"
```
