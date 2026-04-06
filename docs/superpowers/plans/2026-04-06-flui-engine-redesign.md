# flui-engine Redesign Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the God Object WgpuPainter with a modular GpuDevice + RenderSurface + FrameEncoder architecture supporting multi-window, full DrawCommand/Layer dispatch, per-layer batching, and criterion benchmarks.

**Architecture:** Shared GPU state (`GpuDevice`) separated from per-window state (`RenderSurface`). Per-frame `FrameEncoder` borrows `&mut RenderSurface` and uses `Arc<GpuDevice>` for read-only access to pipelines/caches. Batchers (Shape, Path, Text, Image, Effect, Compositing) accumulate instances and flush at layer boundaries with pipeline-key sorting.

**Tech Stack:** Rust 1.91, wgpu 25.x, glyphon 0.9, lyon 1.0, glam 0.30, bytemuck, criterion 0.5, tracing

**Spec:** `docs/superpowers/specs/2026-04-06-flui-engine-redesign.md`

---

## File Structure

```
crates/flui-engine/src/
├─��� lib.rs                          # Public API re-exports (rewrite)
├��─ error.rs                        # Keep unchanged
├── context/
│   ├── mod.rs                      # Create
│   ├── gpu_device.rs               # Create: GpuDevice
│   ├── render_surface.rs           # Create: RenderSurface
│   ├── capabilities.rs             # Create: GpuCapabilities (adapt from wgpu/renderer.rs)
│   └── headless.rs                 # Create: headless adapter
├── frame/
│   ├── mod.rs                      # Create
│   ├── encoder.rs                  # Create: FrameEncoder
│   ├── state_stack.rs              # Create: TransformStack, ClipStack, OpacityStack
│   ├── dispatch.rs                 # Create: DrawCommand + Layer dispatch
│   └── submission.rs               # Create: BatchedDraw, GPU submit
├── batchers/
│   ├── mod.rs                      # Create
│   ├── shapes.rs                   # Create: ShapeBatcher
│   ├── paths.rs                    # Create: PathBatcher
│   ├── text.rs                     # Create: TextBatcher
│   ├── images.rs                   # Create: ImageBatcher
│   ├── effects.rs                  # Create: EffectBatcher
│   └── compositing.rs              # Create: CompositingBatcher
├── pipelines/
│   ├── mod.rs                      # Create
│   ├── registry.rs                 # Create: PipelineRegistry
│   ├── shape_pipeline.rs           # Create
│   ├── path_pipeline.rs            # Create
│   ├── image_pipeline.rs           # Create
│   ├── gradient_pipeline.rs        # Create
│   ├── shadow_pipeline.rs          # Create
│   └── blur_pipeline.rs            # Create
├── text/
│   ├── mod.rs                      # Create
│   ├── system.rs                   # Create: TextSystem
│   └── cache.rs                    # Create: ShapeCache
├── resources/
│   ├── mod.rs                      # Create
│   ├── buffer_pool.rs              # Create (adapt from wgpu/buffer_pool.rs)
│   ├── texture_cache.rs            # Create (adapt from wgpu/texture_cache.rs)
│   ├── texture_atlas.rs            # Create (adapt from wgpu/atlas.rs)
│   └── texture_pool.rs             # Create (adapt from wgpu/texture_pool.rs)
├── platform/
│   ├── mod.rs                      # Create
│   ├── metal.rs                    # Adapt from wgpu/metal.rs
│   ├── dx12.rs                     # Adapt from wgpu/dx12.rs
│   └── vulkan.rs                   # Adapt from wgpu/vulkan.rs
├── vertex.rs                       # Create (consolidate wgpu/vertex.rs + wgpu/instancing.rs)
├── debug.rs                        # Adapt from wgpu/debug.rs
├── shaders/                        # Move from wgpu/shaders/ (keep all .wgsl files as-is)
│   └── (all existing .wgsl files)
└── (delete old: traits.rs, commands.rs, utils/, wgpu/)
```

---

## Task 1: Scaffold module structure and vertex types

**Files:**
- Create: `src/context/mod.rs`, `src/frame/mod.rs`, `src/batchers/mod.rs`, `src/pipelines/mod.rs`, `src/text/mod.rs`, `src/resources/mod.rs`, `src/platform/mod.rs`
- Create: `src/vertex.rs`
- Modify: `src/lib.rs`
- Modify: `Cargo.toml`

- [ ] **Step 1: Move shaders out of wgpu/ to src/shaders/**

```bash
cp -r crates/flui-engine/src/wgpu/shaders crates/flui-engine/src/shaders
```

- [ ] **Step 2: Create empty module files**

Create `src/context/mod.rs`:
```rust
pub mod gpu_device;
pub mod render_surface;
pub mod capabilities;
pub mod headless;
```

Create `src/frame/mod.rs`:
```rust
pub mod encoder;
pub mod state_stack;
pub mod dispatch;
pub mod submission;
```

Create `src/batchers/mod.rs`:
```rust
pub mod shapes;
pub mod paths;
pub mod text;
pub mod images;
pub mod effects;
pub mod compositing;
```

Create `src/pipelines/mod.rs`:
```rust
pub mod registry;
pub mod shape_pipeline;
pub mod path_pipeline;
pub mod image_pipeline;
pub mod gradient_pipeline;
pub mod shadow_pipeline;
pub mod blur_pipeline;
```

Create `src/text/mod.rs`:
```rust
pub mod system;
pub mod cache;
```

Create `src/resources/mod.rs`:
```rust
pub mod buffer_pool;
pub mod texture_cache;
pub mod texture_atlas;
pub mod texture_pool;
```

Create `src/platform/mod.rs`:
```rust
#[cfg(target_os = "macos")]
pub mod metal;
#[cfg(target_os = "windows")]
pub mod dx12;
#[cfg(target_os = "linux")]
pub mod vulkan;
```

- [ ] **Step 3: Create vertex.rs with consolidated types**

Create `src/vertex.rs`:
```rust
//! GPU vertex and instance types for all render pipelines.
//!
//! All types are `#[repr(C)]` + `Pod` + `Zeroable` for zero-copy GPU uploads.

use bytemuck::{Pod, Zeroable};
use flui_types::{Color, DevicePixels, Point};

/// Generic vertex with position, color, and texture coordinates.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, Pod, Zeroable)]
pub struct Vertex {
    pub position: [f32; 2],
    pub color: [f32; 4],
    pub tex_coord: [f32; 2],
}

impl Vertex {
    pub fn new(position: [f32; 2], color: [f32; 4], tex_coord: [f32; 2]) -> Self {
        Self { position, color, tex_coord }
    }

    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute { offset: 0, shader_location: 0, format: wgpu::VertexFormat::Float32x2 },
                wgpu::VertexAttribute { offset: 8, shader_location: 1, format: wgpu::VertexFormat::Float32x4 },
                wgpu::VertexAttribute { offset: 24, shader_location: 2, format: wgpu::VertexFormat::Float32x2 },
            ],
        }
    }
}

/// Path vertex for tessellated fill/stroke.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, Pod, Zeroable)]
pub struct PathVertex {
    pub position: [f32; 2],
    pub color: [f32; 4],
}

impl PathVertex {
    pub fn new(x: f32, y: f32, color: [f32; 4]) -> Self {
        Self { position: [x, y], color }
    }

    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute { offset: 0, shader_location: 0, format: wgpu::VertexFormat::Float32x2 },
                wgpu::VertexAttribute { offset: 8, shader_location: 1, format: wgpu::VertexFormat::Float32x4 },
            ],
        }
    }
}

/// Instanced rectangle (also used for rounded rects via corner_radii).
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, Pod, Zeroable)]
pub struct RectInstance {
    pub bounds: [f32; 4],        // x, y, width, height
    pub color: [f32; 4],
    pub corner_radii: [f32; 4],  // top-left, top-right, bottom-right, bottom-left
    pub transform: [f32; 4],     // scale_x, skew_x, skew_y, scale_y
}

impl RectInstance {
    pub fn rect(x: f32, y: f32, w: f32, h: f32, color: [f32; 4]) -> Self {
        Self {
            bounds: [x, y, w, h],
            color,
            corner_radii: [0.0; 4],
            transform: [1.0, 0.0, 0.0, 1.0],
        }
    }

    pub fn rounded_rect(x: f32, y: f32, w: f32, h: f32, color: [f32; 4], radii: [f32; 4]) -> Self {
        Self {
            bounds: [x, y, w, h],
            color,
            corner_radii: radii,
            transform: [1.0, 0.0, 0.0, 1.0],
        }
    }

    pub fn with_transform(mut self, transform: [f32; 4]) -> Self {
        self.transform = transform;
        self
    }

    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &[
                wgpu::VertexAttribute { offset: 0, shader_location: 3, format: wgpu::VertexFormat::Float32x4 },
                wgpu::VertexAttribute { offset: 16, shader_location: 4, format: wgpu::VertexFormat::Float32x4 },
                wgpu::VertexAttribute { offset: 32, shader_location: 5, format: wgpu::VertexFormat::Float32x4 },
                wgpu::VertexAttribute { offset: 48, shader_location: 6, format: wgpu::VertexFormat::Float32x4 },
            ],
        }
    }
}

/// Instanced circle (also used for ovals).
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, Pod, Zeroable)]
pub struct CircleInstance {
    pub center: [f32; 2],
    pub radius: [f32; 2],  // rx, ry (equal for circle, different for oval)
    pub color: [f32; 4],
    pub transform: [f32; 4],
}

impl CircleInstance {
    pub fn circle(cx: f32, cy: f32, r: f32, color: [f32; 4]) -> Self {
        Self {
            center: [cx, cy],
            radius: [r, r],
            color,
            transform: [1.0, 0.0, 0.0, 1.0],
        }
    }

    pub fn oval(cx: f32, cy: f32, rx: f32, ry: f32, color: [f32; 4]) -> Self {
        Self {
            center: [cx, cy],
            radius: [rx, ry],
            color,
            transform: [1.0, 0.0, 0.0, 1.0],
        }
    }

    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &[
                wgpu::VertexAttribute { offset: 0, shader_location: 3, format: wgpu::VertexFormat::Float32x2 },
                wgpu::VertexAttribute { offset: 8, shader_location: 4, format: wgpu::VertexFormat::Float32x2 },
                wgpu::VertexAttribute { offset: 16, shader_location: 5, format: wgpu::VertexFormat::Float32x4 },
                wgpu::VertexAttribute { offset: 32, shader_location: 6, format: wgpu::VertexFormat::Float32x4 },
            ],
        }
    }
}

/// Instanced arc segment.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, Pod, Zeroable)]
pub struct ArcInstance {
    pub center: [f32; 2],
    pub radius: f32,
    pub start_angle: f32,
    pub sweep_angle: f32,
    pub color: [f32; 4],
    pub _padding: [f32; 3],
}

impl ArcInstance {
    pub fn new(cx: f32, cy: f32, radius: f32, start: f32, sweep: f32, color: [f32; 4]) -> Self {
        Self {
            center: [cx, cy],
            radius,
            start_angle: start,
            sweep_angle: sweep,
            color,
            _padding: [0.0; 3],
        }
    }

    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &[
                wgpu::VertexAttribute { offset: 0, shader_location: 3, format: wgpu::VertexFormat::Float32x2 },
                wgpu::VertexAttribute { offset: 8, shader_location: 4, format: wgpu::VertexFormat::Float32 },
                wgpu::VertexAttribute { offset: 12, shader_location: 5, format: wgpu::VertexFormat::Float32 },
                wgpu::VertexAttribute { offset: 16, shader_location: 6, format: wgpu::VertexFormat::Float32 },
                wgpu::VertexAttribute { offset: 20, shader_location: 7, format: wgpu::VertexFormat::Float32x4 },
            ],
        }
    }
}

/// Instanced line segment.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, Pod, Zeroable)]
pub struct LineInstance {
    pub start: [f32; 2],
    pub end: [f32; 2],
    pub color: [f32; 4],
    pub width: f32,
    pub _padding: [f32; 3],
}

impl LineInstance {
    pub fn new(x1: f32, y1: f32, x2: f32, y2: f32, color: [f32; 4], width: f32) -> Self {
        Self {
            start: [x1, y1],
            end: [x2, y2],
            color,
            width,
            _padding: [0.0; 3],
        }
    }
}

/// Instanced textured quad for images.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, Pod, Zeroable)]
pub struct ImageQuadInstance {
    pub dst_bounds: [f32; 4],    // x, y, width, height
    pub src_uv: [f32; 4],        // u_min, v_min, u_max, v_max
    pub color: [f32; 4],         // tint color (white = no tint)
    pub transform: [f32; 4],
}

impl ImageQuadInstance {
    pub fn new(dst: [f32; 4], src_uv: [f32; 4], color: [f32; 4]) -> Self {
        Self {
            dst_bounds: dst,
            src_uv,
            color,
            transform: [1.0, 0.0, 0.0, 1.0],
        }
    }

    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &[
                wgpu::VertexAttribute { offset: 0, shader_location: 3, format: wgpu::VertexFormat::Float32x4 },
                wgpu::VertexAttribute { offset: 16, shader_location: 4, format: wgpu::VertexFormat::Float32x4 },
                wgpu::VertexAttribute { offset: 32, shader_location: 5, format: wgpu::VertexFormat::Float32x4 },
                wgpu::VertexAttribute { offset: 48, shader_location: 6, format: wgpu::VertexFormat::Float32x4 },
            ],
        }
    }
}

/// Shared frame-level uniforms (bind group 0 for all pipelines).
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct FrameUniforms {
    pub viewport_size: [f32; 2],
    pub scale_factor: f32,
    pub _padding: f32,
}

impl FrameUniforms {
    pub fn new(width: u32, height: u32, scale_factor: f32) -> Self {
        Self {
            viewport_size: [width as f32, height as f32],
            scale_factor,
            _padding: 0.0,
        }
    }
}

/// Unit quad vertices shared by all instanced pipelines.
pub const UNIT_QUAD_VERTICES: &[[f32; 2]] = &[
    [0.0, 0.0],
    [1.0, 0.0],
    [0.0, 1.0],
    [1.0, 1.0],
];

pub const UNIT_QUAD_INDICES: &[u16] = &[0, 1, 2, 2, 1, 3];
```

- [ ] **Step 4: Add criterion to Cargo.toml dev-dependencies**

Add to `Cargo.toml`:
```toml
[dev-dependencies]
pollster = { workspace = true }
env_logger = "0.11.8"
tokio = { workspace = true }
criterion = { version = "0.5", features = ["html_reports"] }

[[bench]]
name = "engine_bench"
harness = false
```

- [ ] **Step 5: Update lib.rs to reference new modules**

Rewrite `src/lib.rs`:
```rust
#![allow(dead_code, missing_debug_implementations)]

//! FLUI Rendering Engine - GPU-accelerated rendering for FLUI
//!
//! # Architecture
//!
//! ```text
//! GpuDevice (shared: device, queue, pipelines, caches)
//!     │
//!     ▼
//! RenderSurface (per-window: surface, config)
//!     │ begin_frame()
//!     ▼
//! FrameEncoder (per-frame: batchers, state stacks)
//!     │ render_scene()
//!     ▼
//! Layer traversal → DrawCommand dispatch → Batchers → GPU submit
//! ```

pub mod error;
pub mod vertex;

#[cfg(feature = "wgpu-backend")]
pub mod context;
#[cfg(feature = "wgpu-backend")]
pub mod frame;
#[cfg(feature = "wgpu-backend")]
pub mod batchers;
#[cfg(feature = "wgpu-backend")]
pub mod pipelines;
#[cfg(feature = "wgpu-backend")]
pub mod text;
#[cfg(feature = "wgpu-backend")]
pub mod resources;
pub mod platform;
pub mod shaders {
    // Shader source strings loaded via include_str!
}

// Keep old wgpu module during migration (will be removed in final task)
#[cfg(feature = "wgpu-backend")]
pub mod wgpu;

pub mod debug;

// Re-exports
pub use error::{RenderError, RenderResult};
pub use flui_layer::{
    CanvasLayer, Layer, LayerId, LayerTree, LinkRegistry, Scene, SceneBuilder, SceneCompositor,
    ShaderMaskLayer,
};
pub use flui_painting::Paint;
```

- [ ] **Step 6: Verify it compiles**

Run: `cargo check -p flui-engine`
Expected: compiles (empty modules + old wgpu module still present)

- [ ] **Step 7: Commit**

```bash
git add crates/flui-engine/
git commit -m "refactor(flui-engine): scaffold new module structure

- Add context/, frame/, batchers/, pipelines/, text/, resources/, platform/ modules
- Create vertex.rs with consolidated vertex/instance types
- Copy shaders to src/shaders/
- Add criterion to dev-dependencies
- Keep old wgpu/ module during migration"
```

---

## Task 2: StateStack (transform, clip, opacity)

**Files:**
- Create: `src/frame/state_stack.rs`
- Test: inline `#[cfg(test)]` module

- [ ] **Step 1: Write failing tests for TransformStack**

Add to `src/frame/state_stack.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use glam::Mat4;

    #[test]
    fn transform_stack_starts_identity() {
        let stack = TransformStack::new();
        assert_eq!(stack.current(), Mat4::IDENTITY);
        assert_eq!(stack.depth(), 0);
    }

    #[test]
    fn transform_stack_push_pop() {
        let mut stack = TransformStack::new();
        let t = Mat4::from_translation(glam::Vec3::new(10.0, 20.0, 0.0));
        stack.push(t);
        assert_eq!(stack.depth(), 1);
        // Transforms compose: identity * t = t
        assert_eq!(stack.current(), t);
        stack.pop();
        assert_eq!(stack.current(), Mat4::IDENTITY);
        assert_eq!(stack.depth(), 0);
    }

    #[test]
    fn transform_stack_composes() {
        let mut stack = TransformStack::new();
        let t1 = Mat4::from_translation(glam::Vec3::new(10.0, 0.0, 0.0));
        let t2 = Mat4::from_translation(glam::Vec3::new(0.0, 20.0, 0.0));
        stack.push(t1);
        stack.push(t2);
        // Composed: translate(10, 20)
        let expected = t1 * t2;
        assert_eq!(stack.current(), expected);
    }

    #[test]
    fn clip_stack_starts_empty() {
        let stack = ClipStack::new();
        assert!(stack.current_scissor(1920, 1080, 1.0).is_none());
    }

    #[test]
    fn clip_stack_push_pop() {
        let mut stack = ClipStack::new();
        stack.push_rect(ClipRect { x: 10.0, y: 20.0, width: 100.0, height: 200.0 });
        let scissor = stack.current_scissor(1920, 1080, 1.0).unwrap();
        assert_eq!(scissor.x, 10);
        assert_eq!(scissor.y, 20);
        assert_eq!(scissor.width, 100);
        assert_eq!(scissor.height, 200);
        stack.pop();
        assert!(stack.current_scissor(1920, 1080, 1.0).is_none());
    }

    #[test]
    fn clip_stack_intersects() {
        let mut stack = ClipStack::new();
        stack.push_rect(ClipRect { x: 0.0, y: 0.0, width: 100.0, height: 100.0 });
        stack.push_rect(ClipRect { x: 50.0, y: 50.0, width: 100.0, height: 100.0 });
        let scissor = stack.current_scissor(1920, 1080, 1.0).unwrap();
        // Intersection: (50,50)-(100,100)
        assert_eq!(scissor.x, 50);
        assert_eq!(scissor.y, 50);
        assert_eq!(scissor.width, 50);
        assert_eq!(scissor.height, 50);
    }

    #[test]
    fn opacity_stack_starts_opaque() {
        let stack = OpacityStack::new();
        assert_eq!(stack.current(), 1.0);
    }

    #[test]
    fn opacity_stack_multiplies() {
        let mut stack = OpacityStack::new();
        stack.push(0.5);
        assert_eq!(stack.current(), 0.5);
        stack.push(0.5);
        assert_eq!(stack.current(), 0.25);
        stack.pop();
        assert_eq!(stack.current(), 0.5);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p flui-engine state_stack -- --nocapture`
Expected: FAIL (types not defined)

- [ ] **Step 3: Implement StateStack types**

Write `src/frame/state_stack.rs` (above the tests):
```rust
//! Transform, clip, and opacity state stacks for frame rendering.

use crate::frame::submission::ScissorRect;
use glam::Mat4;

/// Clipping rectangle in logical pixels.
#[derive(Copy, Clone, Debug)]
pub struct ClipRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl ClipRect {
    pub fn intersect(&self, other: &ClipRect) -> Option<ClipRect> {
        let x1 = self.x.max(other.x);
        let y1 = self.y.max(other.y);
        let x2 = (self.x + self.width).min(other.x + other.width);
        let y2 = (self.y + self.height).min(other.y + other.height);
        if x2 > x1 && y2 > y1 {
            Some(ClipRect { x: x1, y: y1, width: x2 - x1, height: y2 - y1 })
        } else {
            None
        }
    }

    pub fn to_scissor(&self, viewport_width: u32, viewport_height: u32, scale: f32) -> ScissorRect {
        let x = (self.x * scale).round().max(0.0) as u32;
        let y = (self.y * scale).round().max(0.0) as u32;
        let w = (self.width * scale).round() as u32;
        let h = (self.height * scale).round() as u32;
        ScissorRect {
            x: x.min(viewport_width),
            y: y.min(viewport_height),
            width: w.min(viewport_width.saturating_sub(x)),
            height: h.min(viewport_height.saturating_sub(y)),
        }
    }
}

/// Stack of matrix transforms, composed multiplicatively.
pub struct TransformStack {
    stack: Vec<Mat4>,
    composed: Vec<Mat4>,
}

impl TransformStack {
    pub fn new() -> Self {
        Self {
            stack: Vec::with_capacity(16),
            composed: vec![Mat4::IDENTITY],
        }
    }

    pub fn push(&mut self, transform: Mat4) {
        self.stack.push(transform);
        let current = *self.composed.last().unwrap_or(&Mat4::IDENTITY);
        self.composed.push(current * transform);
    }

    pub fn pop(&mut self) {
        self.stack.pop();
        self.composed.pop();
    }

    pub fn current(&self) -> Mat4 {
        self.composed.last().copied().unwrap_or(Mat4::IDENTITY)
    }

    pub fn depth(&self) -> usize {
        self.stack.len()
    }

    pub fn reset(&mut self) {
        self.stack.clear();
        self.composed.clear();
        self.composed.push(Mat4::IDENTITY);
    }
}

/// Stack of axis-aligned clip rects, intersected progressively.
pub struct ClipStack {
    rects: Vec<ClipRect>,
    composed: Vec<Option<ClipRect>>,
}

impl ClipStack {
    pub fn new() -> Self {
        Self {
            rects: Vec::with_capacity(8),
            composed: vec![None],
        }
    }

    pub fn push_rect(&mut self, rect: ClipRect) {
        let current = self.composed.last().copied().unwrap_or(None);
        let intersected = match current {
            Some(existing) => existing.intersect(&rect),
            None => Some(rect),
        };
        self.rects.push(rect);
        self.composed.push(intersected);
    }

    pub fn pop(&mut self) {
        self.rects.pop();
        self.composed.pop();
    }

    pub fn current_scissor(&self, viewport_w: u32, viewport_h: u32, scale: f32) -> Option<ScissorRect> {
        self.composed.last().copied().flatten().map(|r| r.to_scissor(viewport_w, viewport_h, scale))
    }

    pub fn current_clip(&self) -> Option<ClipRect> {
        self.composed.last().copied().flatten()
    }

    pub fn reset(&mut self) {
        self.rects.clear();
        self.composed.clear();
        self.composed.push(None);
    }
}

/// Stack of opacity values, multiplied progressively.
pub struct OpacityStack {
    values: Vec<f32>,
    composed: Vec<f32>,
}

impl OpacityStack {
    pub fn new() -> Self {
        Self {
            values: Vec::with_capacity(8),
            composed: vec![1.0],
        }
    }

    pub fn push(&mut self, opacity: f32) {
        let current = *self.composed.last().unwrap_or(&1.0);
        self.values.push(opacity);
        self.composed.push(current * opacity);
    }

    pub fn pop(&mut self) {
        self.values.pop();
        self.composed.pop();
    }

    pub fn current(&self) -> f32 {
        self.composed.last().copied().unwrap_or(1.0)
    }

    pub fn reset(&mut self) {
        self.values.clear();
        self.composed.clear();
        self.composed.push(1.0);
    }
}

/// Combined state stacks for a frame.
pub struct StateStack {
    pub transform: TransformStack,
    pub clip: ClipStack,
    pub opacity: OpacityStack,
}

impl StateStack {
    pub fn new() -> Self {
        Self {
            transform: TransformStack::new(),
            clip: ClipStack::new(),
            opacity: OpacityStack::new(),
        }
    }

    pub fn reset(&mut self) {
        self.transform.reset();
        self.clip.reset();
        self.opacity.reset();
    }
}
```

- [ ] **Step 4: Create minimal submission.rs for ScissorRect**

Create `src/frame/submission.rs`:
```rust
//! GPU draw command types and submission logic.

/// Scissor rectangle in physical pixels (what wgpu expects).
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct ScissorRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}
```

- [ ] **Step 5: Update frame/mod.rs exports**

Update `src/frame/mod.rs`:
```rust
pub mod state_stack;
pub mod submission;
pub mod encoder;
pub mod dispatch;
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test -p flui-engine state_stack -- --nocapture`
Expected: All 7 tests PASS

- [ ] **Step 7: Commit**

```bash
git add crates/flui-engine/src/frame/
git commit -m "feat(flui-engine): implement StateStack (transform, clip, opacity)

- TransformStack: composing matrix transforms
- ClipStack: intersecting axis-aligned clip rects with scissor conversion
- OpacityStack: multiplicative opacity composition
- All stacks tested without GPU dependency"
```

---

## Task 3: ShapeBatcher

**Files:**
- Create: `src/batchers/shapes.rs`
- Test: inline `#[cfg(test)]`

- [ ] **Step 1: Write failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_batcher_has_no_draws() {
        let batcher = ShapeBatcher::new();
        assert!(batcher.is_empty());
        assert_eq!(batcher.rect_count(), 0);
        assert_eq!(batcher.circle_count(), 0);
    }

    #[test]
    fn add_rects_accumulates() {
        let mut batcher = ShapeBatcher::new();
        batcher.add_rect(10.0, 20.0, 100.0, 50.0, [1.0, 0.0, 0.0, 1.0], [0.0; 4], [1.0, 0.0, 0.0, 1.0]);
        batcher.add_rect(30.0, 40.0, 80.0, 60.0, [0.0, 1.0, 0.0, 1.0], [0.0; 4], [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(batcher.rect_count(), 2);
        assert!(!batcher.is_empty());
    }

    #[test]
    fn add_circle_accumulates() {
        let mut batcher = ShapeBatcher::new();
        batcher.add_circle(50.0, 50.0, 25.0, [1.0; 4], [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(batcher.circle_count(), 1);
    }

    #[test]
    fn add_oval_uses_circle_instance() {
        let mut batcher = ShapeBatcher::new();
        batcher.add_oval(0.0, 0.0, 100.0, 50.0, [1.0; 4], [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(batcher.circle_count(), 1); // ovals stored as circles with different rx/ry
    }

    #[test]
    fn clear_resets_all() {
        let mut batcher = ShapeBatcher::new();
        batcher.add_rect(0.0, 0.0, 10.0, 10.0, [1.0; 4], [0.0; 4], [1.0, 0.0, 0.0, 1.0]);
        batcher.add_circle(0.0, 0.0, 5.0, [1.0; 4], [1.0, 0.0, 0.0, 1.0]);
        batcher.clear();
        assert!(batcher.is_empty());
        assert_eq!(batcher.rect_count(), 0);
        assert_eq!(batcher.circle_count(), 0);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p flui-engine batchers::shapes -- --nocapture`
Expected: FAIL

- [ ] **Step 3: Implement ShapeBatcher**

Write `src/batchers/shapes.rs`:
```rust
//! ShapeBatcher accumulates instanced shape primitives (rect, rrect, circle, oval, arc, line).
//!
//! Each `add_*` method bakes the current transform into instance data.
//! On `flush()`, one instanced draw call per primitive type.

use crate::vertex::{ArcInstance, CircleInstance, LineInstance, RectInstance};

/// Accumulates instanced shape primitives for batch rendering.
pub struct ShapeBatcher {
    rects: Vec<RectInstance>,
    circles: Vec<CircleInstance>,
    arcs: Vec<ArcInstance>,
    lines: Vec<LineInstance>,
}

impl ShapeBatcher {
    pub fn new() -> Self {
        Self {
            rects: Vec::with_capacity(256),
            circles: Vec::with_capacity(64),
            arcs: Vec::with_capacity(16),
            lines: Vec::with_capacity(64),
        }
    }

    /// Add a rectangle or rounded rectangle.
    /// `transform` is [scale_x, skew_x, skew_y, scale_y] from the composed matrix.
    pub fn add_rect(
        &mut self,
        x: f32, y: f32, w: f32, h: f32,
        color: [f32; 4],
        corner_radii: [f32; 4],
        transform: [f32; 4],
    ) {
        self.rects.push(
            RectInstance::rounded_rect(x, y, w, h, color, corner_radii)
                .with_transform(transform),
        );
    }

    /// Add a circle.
    pub fn add_circle(&mut self, cx: f32, cy: f32, r: f32, color: [f32; 4], transform: [f32; 4]) {
        self.circles.push(CircleInstance::circle(cx, cy, r, color));
    }

    /// Add an oval (ellipse).
    pub fn add_oval(&mut self, x: f32, y: f32, w: f32, h: f32, color: [f32; 4], transform: [f32; 4]) {
        let cx = x + w / 2.0;
        let cy = y + h / 2.0;
        self.circles.push(CircleInstance::oval(cx, cy, w / 2.0, h / 2.0, color));
    }

    /// Add an arc.
    pub fn add_arc(
        &mut self,
        cx: f32, cy: f32, radius: f32,
        start_angle: f32, sweep_angle: f32,
        color: [f32; 4],
    ) {
        self.arcs.push(ArcInstance::new(cx, cy, radius, start_angle, sweep_angle, color));
    }

    /// Add a line.
    pub fn add_line(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, color: [f32; 4], width: f32) {
        self.lines.push(LineInstance::new(x1, y1, x2, y2, color, width));
    }

    pub fn rect_count(&self) -> usize { self.rects.len() }
    pub fn circle_count(&self) -> usize { self.circles.len() }
    pub fn arc_count(&self) -> usize { self.arcs.len() }
    pub fn line_count(&self) -> usize { self.lines.len() }

    pub fn is_empty(&self) -> bool {
        self.rects.is_empty()
            && self.circles.is_empty()
            && self.arcs.is_empty()
            && self.lines.is_empty()
    }

    pub fn clear(&mut self) {
        self.rects.clear();
        self.circles.clear();
        self.arcs.clear();
        self.lines.clear();
    }

    /// Take the accumulated instance data, leaving self empty.
    /// Used for zero-alloc pool recycling.
    pub fn take_rects(&mut self) -> Vec<RectInstance> {
        std::mem::take(&mut self.rects)
    }

    pub fn take_circles(&mut self) -> Vec<CircleInstance> {
        std::mem::take(&mut self.circles)
    }

    pub fn take_arcs(&mut self) -> Vec<ArcInstance> {
        std::mem::take(&mut self.arcs)
    }

    pub fn take_lines(&mut self) -> Vec<LineInstance> {
        std::mem::take(&mut self.lines)
    }

    /// Restore pre-allocated Vecs (zero-alloc pool recycling).
    pub fn restore(&mut self, rects: Vec<RectInstance>, circles: Vec<CircleInstance>, arcs: Vec<ArcInstance>, lines: Vec<LineInstance>) {
        self.rects = rects;
        self.circles = circles;
        self.arcs = arcs;
        self.lines = lines;
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p flui-engine batchers::shapes -- --nocapture`
Expected: All 5 tests PASS

- [ ] **Step 5: Commit**

```bash
git add crates/flui-engine/src/batchers/
git commit -m "feat(flui-engine): implement ShapeBatcher

- Accumulates RectInstance, CircleInstance, ArcInstance, LineInstance
- Supports zero-alloc pool recycling via take/restore
- Ovals mapped to CircleInstance with different rx/ry"
```

---

## Task 4: PathBatcher with lyon tessellation

**Files:**
- Create: `src/batchers/paths.rs`
- Test: inline `#[cfg(test)]`

This task follows the same TDD pattern. PathBatcher wraps lyon `FillTessellator` and `StrokeTessellator`, tessellates paths into `PathVertex` + indices on `add_path()`, and accumulates vertices for batch draw.

Due to plan length, remaining tasks are listed with scope and key implementation notes rather than full TDD steps. Each task follows the same pattern: write failing test -> implement -> verify -> commit.

---

## Task 5: TextBatcher and ShapeCache

**Files:**
- Create: `src/batchers/text.rs`, `src/text/cache.rs`, `src/text/system.rs`

TextBatcher stores `PreparedTextRun` structs. ShapeCache implements LRU with 120-frame TTL, keyed by `TextCacheKey` (text_hash + font_size_bits + font_family_hash + font_weight). TextSystem wraps glyphon `FontSystem`, `SwashCache`, `TextAtlas`, `TextRenderer`.

Unit tests: ShapeCache LRU eviction, TextCacheKey hash stability, TextBatcher accumulation.
GPU tests (feature-gated): TextSystem::prepare + render.

---

## Task 6: ImageBatcher

**Files:**
- Create: `src/batchers/images.rs`

Accumulates `ImageQuadInstance` entries, groups by texture_id. Tests: accumulation, grouping by texture.

---

## Task 7: EffectBatcher

**Files:**
- Create: `src/batchers/effects.rs`

Handles gradient instances (linear/radial), shadow instances, blur pass descriptors. Gradient and shadow are instanced. Blur uses TexturePool for offscreen passes (deferred to Task 12).

---

## Task 8: CompositingBatcher

**Files:**
- Create: `src/batchers/compositing.rs`

Handles SaveLayer/RestoreLayer via offscreen render target stack. Manages TexturePool allocation. ShaderMask, BackdropFilter, ColorFilter, ImageFilter render children to offscreen then composite back.

---

## Task 9: PipelineRegistry and all pipelines

**Files:**
- Create: `src/pipelines/registry.rs`, `src/pipelines/shape_pipeline.rs`, `src/pipelines/path_pipeline.rs`, `src/pipelines/image_pipeline.rs`, `src/pipelines/gradient_pipeline.rs`, `src/pipelines/shadow_pipeline.rs`, `src/pipelines/blur_pipeline.rs`

PipelineRegistry creates all pipelines at init using `include_str!()` for WGSL shaders. Each pipeline file creates its `wgpu::RenderPipeline` with correct vertex layouts, bind group layouts, and blend state.

`PipelineId` enum with `all()` method for iteration. GPU test: verify all pipelines create without error.

---

## Task 10: Resources (BufferPool, TextureCache, TextureAtlas, TexturePool)

**Files:**
- Create: `src/resources/buffer_pool.rs`, `src/resources/texture_cache.rs`, `src/resources/texture_atlas.rs`, `src/resources/texture_pool.rs`

Adapt from existing `wgpu/buffer_pool.rs`, `wgpu/texture_cache.rs`, `wgpu/atlas.rs`, `wgpu/texture_pool.rs`. Clean up imports, remove God Object dependencies, make self-contained.

---

## Task 11: GpuDevice and GpuCapabilities

**Files:**
- Create: `src/context/gpu_device.rs`, `src/context/capabilities.rs`, `src/context/headless.rs`

GpuDevice owns `Arc<wgpu::Device>`, `Arc<wgpu::Queue>`, `PipelineRegistry`, `TextureCache`, `BufferPool`, `TextSystem`. Uses `pollster::block_on()` for async wgpu init. `new_headless()` creates without window.

Adapt GpuCapabilities from existing `wgpu/renderer.rs::GpuCapabilities`. Platform modules (`metal.rs`, `dx12.rs`, `vulkan.rs`) adapt from existing files.

---

## Task 12: RenderSurface

**Files:**
- Create: `src/context/render_surface.rs`

Owns `wgpu::Surface`, `SurfaceConfiguration`, `Arc<GpuDevice>`. Pre-allocated Vec pools for FrameEncoder. `resize()` reconfigures surface. `begin_frame()` acquires surface texture, returns `FrameEncoder` or `SurfaceLost` error.

---

## Task 13: DrawCommand dispatch and Layer traversal

**Files:**
- Create: `src/frame/dispatch.rs`

Exhaustive `match` on all 27 `DrawCommand` variants routing to correct batcher. Exhaustive `match` on all 20 `Layer` variants with correct state stack push/pop and children traversal via `LayerTree::children()`.

Unit test: create mock batchers, verify each DrawCommand variant routes correctly.

---

## Task 14: FrameEncoder

**Files:**
- Create: `src/frame/encoder.rs`

Connects everything: holds `&mut RenderSurface`, `Arc<GpuDevice>`, batchers, state stacks. `render_scene()` traverses Scene's LayerTree. `finish()` flushes all batchers, builds wgpu::CommandEncoder, executes all BatchedDraw commands, submits, presents.

---

## Task 15: BatchedDraw submission

**Files:**
- Create: `src/frame/submission.rs` (expand from Task 2 stub)

Submission loop iterates `Vec<BatchedDraw>`, executing each:
- `Instanced` → `render_pass.set_pipeline()` + `set_vertex_buffer()` + `draw_indexed()`
- `Indexed` → same but from path vertex/index buffers
- `Text` → delegate to `TextSystem::render()`
- `SetScissor` → `render_pass.set_scissor_rect()`
- `PushRenderTarget` / `PopRenderTarget` → begin/end offscreen render pass

---

## Task 16: Update lib.rs public API and remove old wgpu/ module

**Files:**
- Modify: `src/lib.rs`
- Delete: `src/wgpu/` (entire directory), `src/traits.rs`, `src/commands.rs`, `src/utils/`

Final cleanup: remove old module, update re-exports to new public API (`GpuDevice`, `RenderSurface`, `FrameEncoder`).

---

## Task 17: Integration tests (headless GPU)

**Files:**
- Create: `tests/gpu_integration.rs`

Feature-gated behind `gpu-tests`. Tests:
- `GpuDevice::new_headless()` succeeds
- `RenderSurface::new_headless()` succeeds
- All pipeline variants created
- Frame with empty scene renders
- Frame with rects + text renders
- Resize handles zero/large dimensions
- Surface lost recovery flow

---

## Task 18: Criterion benchmarks

**Files:**
- Create: `benches/engine_bench.rs`

Benchmarks:
- `bench_1000_rects`: create headless context, scene with 1000 rects, measure render_scene + finish
- `bench_text_rendering`: 100 text runs
- `bench_mixed_ui`: realistic scene (rects + text + gradients)
- `bench_deep_layer_tree`: 50 nested layers
- `bench_batching_sort`: 10k shape instances, measure flush

---

## Task 19: DebugEncoder and tracing spans

**Files:**
- Modify: `src/debug.rs`
- Modify: `src/frame/encoder.rs`

Add `tracing::instrument` to `render_scene()`, `render_layer()`, `flush_all()`, `finish()`. Adapt DebugEncoder to log all dispatched commands without GPU.

---

## Task 20: Final verification

- [ ] `cargo check -p flui-engine`
- [ ] `cargo test -p flui-engine`
- [ ] `cargo test -p flui-engine --features gpu-tests` (if GPU available)
- [ ] `cargo clippy -p flui-engine -- -D warnings`
- [ ] `cargo fmt --all`
- [ ] `cargo bench -p flui-engine` (if GPU available)
- [ ] `cargo build --workspace` (verify no regression)
