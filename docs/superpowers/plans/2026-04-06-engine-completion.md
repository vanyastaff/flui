# Engine Completion Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Complete all remaining GPU rendering features: buffer pooling, image loading, sweep gradients, stencil clipping, SaveLayer/RestoreLayer compositing, ShaderMask/BackdropFilter effects.

**Architecture:** Build bottom-up — buffer pooling first (used by everything), then image loading (standalone), then sweep gradients (follows existing pattern), then stencil clipping (render pass change), then compositing (most complex, uses all the above).

**Tech Stack:** wgpu 25.x, image crate, lyon 1.0, bytemuck, glam 0.30

**Spec:** `docs/superpowers/specs/2026-04-06-engine-completion-design.md`

---

## File Structure

```
crates/flui-engine/src/
  frame/encoder.rs         — Modify: replace create_buffer_init with BufferPool, add compositing render target switching, stencil attachment
  frame/submission.rs      — Modify: add compositing DrawOps
  frame/dispatch.rs        — Modify: stencil clip ops, sweep gradient dispatch
  batchers/effects.rs      — Modify: add SweepGradientInstance
  batchers/compositing.rs  — Keep as-is (already has the ops)
  context/gpu_device.rs    — Modify: add load_image(), texture_id_counter, image_sampler, image_bind_group_layout
  context/render_surface.rs — Modify: add stencil texture
  pipelines/registry.rs    — Modify: add SweepGradient pipeline, stencil pipeline, image_bind_group_layout
  pipelines/gradient_pipeline.rs — Modify: add create_sweep_gradient_pipeline()
  shaders/gradients/sweep.wgsl — Create: sweep gradient shader
  shaders/compositing.wgsl — Create: textured quad with opacity for offscreen composite
  shaders/stencil_write.wgsl — Create: writes to stencil buffer only
```

---

## Task 1: Buffer Pooling in finish()

**Files:**
- Modify: `src/frame/encoder.rs`

Replace all `device.create_buffer_init()` calls in finish() with BufferPool reuse.

- [ ] **Step 1: Read current finish() and identify all create_buffer_init calls**

Read `src/frame/encoder.rs` to find every `create_buffer_init` call. Each needs to be replaced with: (1) pool.get_vertex_buffer(device, size) to get a reusable buffer, (2) queue.write_buffer() to upload data.

- [ ] **Step 2: Add BufferPool reset at start of finish()**

At the beginning of finish(), before any buffer uploads:
```rust
self.gpu.buffer_pool().lock().reset();
```

- [ ] **Step 3: Replace rect buffer allocation**

Replace:
```rust
let rect_buf = self.gpu.device().create_buffer_init(&wgpu::util::BufferInitDescriptor {
    label: Some("rect_instances"),
    contents: bytemuck::cast_slice(self.batchers.shapes.rects()),
    usage: wgpu::BufferUsages::VERTEX,
});
```

With:
```rust
let rect_data = bytemuck::cast_slice(self.batchers.shapes.rects());
let rect_buf = {
    let mut pool = self.gpu.buffer_pool().lock();
    pool.get_vertex_buffer(self.gpu.device(), rect_data.len() as u64).clone()
};
self.gpu.queue().write_buffer(&rect_buf, 0, rect_data);
```

Note: BufferPool::get_vertex_buffer returns `&wgpu::Buffer`. We need to clone the buffer handle or adjust the API. Read BufferPool to see exact return type and adjust accordingly. May need to return the Buffer by index and look it up later, or store references.

- [ ] **Step 4: Replace all other buffer allocations similarly**

Same pattern for: circle_buf, arc_buf, shadow_buf, path vertex/index buffers. Each `create_buffer_init` → `pool.get + queue.write_buffer`.

- [ ] **Step 5: Verify compilation and tests**

Run: `cargo check -p flui-engine && cargo test -p flui-engine`

- [ ] **Step 6: Commit**

```bash
git commit -am "perf(flui-engine): replace per-frame buffer allocation with BufferPool reuse"
```

---

## Task 2: Image Loading Pipeline

**Files:**
- Modify: `src/context/gpu_device.rs`
- Modify: `src/frame/encoder.rs`
- Modify: `src/pipelines/registry.rs`

- [ ] **Step 1: Add image loading to GpuDevice**

Add fields to GpuDevice:
```rust
texture_id_counter: std::sync::atomic::AtomicU64,
image_sampler: wgpu::Sampler,
image_bind_group_layout: Arc<wgpu::BindGroupLayout>,
```

Add method:
```rust
pub fn load_image(&self, data: &[u8]) -> RenderResult<u64> {
    let img = image::load_from_memory(data)
        .map_err(|e| RenderError::resource(format!("image decode: {e}")))?;
    let rgba = img.to_rgba8();
    let (width, height) = rgba.dimensions();

    let texture = self.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("loaded_image"),
        size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });

    self.queue.write_texture(
        wgpu::TexelCopyTextureInfo { texture: &texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
        &rgba,
        wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4 * width), rows_per_image: Some(height) },
        wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
    );

    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let id = self.texture_id_counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;

    let mut cache = self.texture_cache.lock();
    cache.insert(id, texture, view, width, height);

    Ok(id)
}
```

- [ ] **Step 2: Create image sampler and bind group layout in GpuDevice init**

In new_headless() and new_with_surface():
```rust
let image_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
    label: Some("image_sampler"),
    mag_filter: wgpu::FilterMode::Linear,
    min_filter: wgpu::FilterMode::Linear,
    ..Default::default()
});

let image_bind_group_layout = Arc::new(device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
    label: Some("image_bind_group_layout"),
    entries: &[
        wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        },
        wgpu::BindGroupLayoutEntry {
            binding: 1,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        },
    ],
}));
```

Add getters: `image_sampler()`, `image_bind_group_layout()`.

- [ ] **Step 3: Wire image rendering in finish()**

In the DrawGroup loop in finish(), after shadows, add image rendering:

For each texture group in ImageBatcher:
1. Look up texture view from TextureCache by texture_id
2. Create bind group with texture view + sampler at group(1)
3. Set Image pipeline, bind groups (0: viewport, 1: texture), draw instanced

- [ ] **Step 4: Add image to render_demo example**

Load a test image and add it to the demo scene. Use `include_bytes!` for an embedded test image, or generate a simple procedural texture.

- [ ] **Step 5: Verify and commit**

```bash
cargo check -p flui-engine && cargo test -p flui-engine
git commit -am "feat(flui-engine): implement image loading and texture rendering pipeline"
```

---

## Task 3: Sweep Gradients

**Files:**
- Modify: `src/batchers/effects.rs`
- Create: `src/shaders/gradients/sweep.wgsl`
- Modify: `src/pipelines/gradient_pipeline.rs`
- Modify: `src/pipelines/registry.rs`
- Modify: `src/frame/dispatch.rs`
- Modify: `src/frame/encoder.rs`
- Modify: `src/frame/submission.rs`

- [ ] **Step 1: Add SweepGradientInstance to EffectBatcher**

In effects.rs:
```rust
#[derive(Clone, Debug)]
pub struct SweepGradientInstance {
    pub bounds: [f32; 4],
    pub center: [f32; 2],
    pub start_angle: f32,
    pub end_angle: f32,
    pub stops: Vec<GradientStop>,
    pub corner_radii: [f32; 4],
    pub transform: [f32; 4],
}
```

Add field `sweep_gradients: Vec<SweepGradientInstance>` to EffectBatcher. Add methods: `add_sweep_gradient()`, `sweep_gradient_count()`, `sweep_gradients()`.

- [ ] **Step 2: Add SweepGradient to BatcherSnapshot**

In submission.rs, add `sweep_gradients: u32` to BatcherSnapshot. Update `Batchers::snapshot()`.

- [ ] **Step 3: Create sweep gradient shader**

Create `src/shaders/gradients/sweep.wgsl` following linear.wgsl pattern but computing t from atan2:

```wgsl
// Fragment shader:
let dx = frag_local.x - 0.5;  // center of unit quad
let dy = frag_local.y - 0.5;
var angle = atan2(dy, dx);     // [-PI, PI]
angle = angle + 3.14159265;    // [0, 2*PI]
let total_range = uniforms.end_angle - uniforms.start_angle;
var t = (angle - uniforms.start_angle) / total_range;
t = clamp(t % 1.0, 0.0, 1.0);
// ... same stop interpolation as linear
```

- [ ] **Step 4: Add SweepGradient pipeline**

In gradient_pipeline.rs: `create_sweep_gradient_pipeline()` — same pattern as linear/radial but with sweep shader. Add `SweepGradient` to PipelineId enum. Register in PipelineRegistry::new().

- [ ] **Step 5: Wire dispatch and encoder**

In dispatch.rs: change SweepGradient fallback from radial to actual sweep gradient instance.
In encoder.rs: add sweep gradient rendering loop (same pattern as linear/radial).

- [ ] **Step 6: Tests and commit**

Add test for sweep gradient stop interpolation. Verify compilation.

```bash
cargo test -p flui-engine && git commit -am "feat(flui-engine): implement sweep gradient shader and pipeline"
```

---

## Task 4: Stencil-Based Clipping

**Files:**
- Modify: `src/context/render_surface.rs`
- Create: `src/shaders/stencil_write.wgsl`
- Modify: `src/pipelines/registry.rs`
- Modify: `src/frame/encoder.rs`
- Modify: `src/frame/dispatch.rs`
- Modify: `src/frame/state_stack.rs`
- Modify: `src/frame/submission.rs`

- [ ] **Step 1: Add stencil texture to RenderSurface**

Add field `stencil_texture: wgpu::Texture` and `stencil_view: wgpu::TextureView` to RenderSurface. Create in constructor with format `Depth24PlusStencil8` and usage `RENDER_ATTACHMENT`. Recreate on resize().

Add getter: `stencil_view() -> &wgpu::TextureView`.

- [ ] **Step 2: Create stencil write shader**

Create `src/shaders/stencil_write.wgsl`:
```wgsl
struct Viewport { size: vec2<f32>, _padding: vec2<f32> }
@group(0) @binding(0) var<uniform> viewport: Viewport;

@vertex
fn vs_main(@location(0) position: vec2<f32>) -> @builtin(position) vec4<f32> {
    let clip = vec2<f32>(
        position.x / viewport.size.x * 2.0 - 1.0,
        1.0 - position.y / viewport.size.y * 2.0,
    );
    return vec4<f32>(clip, 0.0, 1.0);
}

@fragment
fn fs_main() -> @location(0) vec4<f32> {
    return vec4<f32>(0.0); // no color output, only stencil write
}
```

- [ ] **Step 3: Create stencil write pipeline**

Add `StencilWrite` to PipelineId. Create pipeline with:
- Color writes disabled (`write_mask: ColorWrites::empty()`)
- Depth stencil state: stencil always pass, write stencil, increment on pass
- Vertex layout: PathVertex (position + color, reuse for path tessellation)

- [ ] **Step 4: Add stencil depth to StateStack**

In state_stack.rs, add to StateStack:
```rust
pub stencil_depth: u32,
```

Add reset in `StateStack::reset()`.

- [ ] **Step 5: Add stencil DrawOps**

In submission.rs, add to DrawOp:
```rust
StencilPush { vertices_start: u32, vertices_count: u32, indices_start: u32, indices_count: u32 },
StencilPop { vertices_start: u32, vertices_count: u32, indices_start: u32, indices_count: u32 },
```

- [ ] **Step 6: Wire ClipRRect with stencil in dispatch.rs**

In traverse_layer for ClipRRect:
1. Convert RRect to lyon path (via existing rrect_to_lyon_path)
2. Tessellate path
3. Store tessellated vertices/indices in PathBatcher
4. Emit StencilPush DrawOp with vertex/index ranges
5. Traverse children
6. Emit StencilPop DrawOp
7. Pop clip state

Same for ClipPath.

- [ ] **Step 7: Handle stencil ops in encoder.rs finish()**

Add depth_stencil_attachment to render pass using `stencil_view()`. For StencilPush/StencilPop DrawOps:
- Set stencil write pipeline
- Upload tessellated clip geometry
- Draw to stencil buffer
- Configure stencil test for subsequent draws

- [ ] **Step 8: Tests and commit**

Add unit test: stencil depth push/pop tracking.

```bash
cargo test -p flui-engine && git commit -am "feat(flui-engine): implement stencil-based clipping for ClipRRect and ClipPath"
```

---

## Task 5: SaveLayer/RestoreLayer Compositing

**Files:**
- Create: `src/shaders/compositing.wgsl`
- Modify: `src/frame/encoder.rs`
- Modify: `src/frame/submission.rs`
- Modify: `src/frame/dispatch.rs`

- [ ] **Step 1: Create compositing shader**

Create `src/shaders/compositing.wgsl` — textured quad with uniform opacity:
```wgsl
struct Viewport { size: vec2<f32>, _padding: vec2<f32> }
@group(0) @binding(0) var<uniform> viewport: Viewport;

struct CompositingUniforms { bounds: vec4<f32>, opacity: f32, _padding: vec3<f32> }
@group(1) @binding(0) var<uniform> composite: CompositingUniforms;
@group(1) @binding(1) var t_offscreen: texture_2d<f32>;
@group(1) @binding(2) var s_offscreen: sampler;

@vertex
fn vs_main(@location(0) position: vec2<f32>) -> VertexOutput {
    // Transform unit quad to composite bounds in clip space
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let tex_color = textureSample(t_offscreen, s_offscreen, in.uv);
    return vec4<f32>(tex_color.rgb, tex_color.a * composite.opacity);
}
```

- [ ] **Step 2: Update compositing pipeline with real shader**

In registry.rs, update the Compositing pipeline creation to use compositing.wgsl with a bind group layout for composite uniforms + texture + sampler.

- [ ] **Step 3: Add compositing DrawOps**

In submission.rs, add:
```rust
PushRenderTarget { width: u32, height: u32, clear_color: [f32; 4] },
PopRenderTarget { opacity: f32, bounds: [f32; 4] },
```

- [ ] **Step 4: Wire SaveLayer/RestoreLayer in dispatch.rs**

In traverse_layer, for layers that need offscreen rendering (Opacity with low alpha, ColorFilter, ImageFilter, ShaderMask, BackdropFilter):
- Emit PushRenderTarget DrawOp with bounds
- Traverse children
- Emit PopRenderTarget DrawOp with opacity

For now, wire just the SaveLayer/RestoreLayer commands from DrawCommand dispatch. Opacity layers can continue using multiplicative alpha for simple cases.

- [ ] **Step 5: Handle render target switching in finish()**

In finish(), when encountering PushRenderTarget:
1. End current render pass (drop it)
2. Acquire offscreen texture from TexturePool
3. Create new render pass targeting offscreen texture
4. Push onto render target stack

When encountering PopRenderTarget:
1. End offscreen render pass
2. Pop from render target stack
3. Resume parent render pass (create new render pass targeting parent surface/texture)
4. Draw offscreen texture as textured quad via Compositing pipeline with opacity

This requires restructuring finish() to handle multiple render passes. The current single render pass block must become a loop.

- [ ] **Step 6: Tests and commit**

Add unit test: PushRenderTarget/PopRenderTarget DrawOp emission from layer traversal.

```bash
cargo test -p flui-engine && git commit -am "feat(flui-engine): implement SaveLayer/RestoreLayer offscreen compositing"
```

---

## Task 6: ShaderMask and BackdropFilter

**Files:**
- Modify: `src/frame/encoder.rs`
- Modify: `src/frame/dispatch.rs`
- Modify: `src/frame/submission.rs`

- [ ] **Step 1: Wire ShaderMask dispatch**

ShaderMask = SaveLayer + render child + apply mask shader. In dispatch.rs, for Layer::ShaderMask:
1. Emit PushRenderTarget (render child to offscreen A)
2. Traverse children
3. Emit PopRenderTarget with mask shader parameters

The mask is applied during the Pop phase using a variant of the compositing shader that multiplies by a gradient mask.

For v1: use the compositing pipeline with opacity derived from the shader's gradient. Full shader masking with arbitrary gradients is complex — defer to v2 if needed.

- [ ] **Step 2: Wire BackdropFilter dispatch**

BackdropFilter:
1. Copy current render target region to a temp texture (via command encoder copy_texture_to_texture)
2. Apply blur (downsample + upsample passes)
3. Draw blurred texture back to render target at bounds
4. Render child on top

In dispatch.rs for Layer::BackdropFilter:
1. Emit a BackdropBlur DrawOp with bounds and blur sigma
2. Traverse children

In finish(): handle BackdropBlur by ending the render pass, doing copy + blur passes, resuming render pass, drawing blurred result.

For v1: implement blur-only BackdropFilter. Other filter types (dilate, erode, matrix) log warning.

- [ ] **Step 3: Tests and commit**

```bash
cargo test -p flui-engine && git commit -am "feat(flui-engine): implement ShaderMask and BackdropFilter effects"
```

---

## Task 7: Update render_demo and final verification

**Files:**
- Modify: `examples/render_demo.rs`

- [ ] **Step 1: Add image, gradients, clipping to demo**

Enhance render_demo to showcase all new features:
- Load an embedded test image
- Add sweep gradient
- Add clipped region (rounded rect clip with content inside)
- Add semi-transparent overlay (SaveLayer with opacity)

- [ ] **Step 2: Run all tests**

```bash
cargo test -p flui-engine
```

- [ ] **Step 3: Run benchmarks**

```bash
cargo bench -p flui-engine -- --test
```

- [ ] **Step 4: Build workspace**

```bash
cargo build --workspace
```

- [ ] **Step 5: Commit**

```bash
git commit -am "feat(flui-engine): update render demo with all GPU features"
```
