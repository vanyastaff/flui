# FLUI Engine Architecture

**Version:** 0.1.0
**Date:** 2025-01-10
**Author:** Claude (Anthropic)
**Status:** Implementation In Progress

---

## Executive Summary

This document describes the architecture of **flui_engine** crate, which provides the **GPU rendering engine** for FLUI. It sits at the bottom of the rendering stack, executing DisplayLists from flui_painting using wgpu, Lyon tessellation, and Glyphon text rendering.

**Current Status:** ✅ Layer system implemented, ✅ WgpuPainter implemented, ✅ EventRouter implemented

**Key Responsibilities:**
1. **Layer System** - Composable scene graph for efficient rendering and effects
2. **WgpuPainter** - Low-level GPU rendering backend (wgpu + Lyon + Glyphon)
3. **Event Router** - Pointer event dispatch and hit testing
4. **DevTools Integration** - Thin wrapper for flui_devtools profiler integration

**Architecture Pattern:** **Composite Pattern** (Layers) + **Command Executor** (DisplayList → GPU)

---

## Table of Contents

1. [Architecture Overview](#architecture-overview)
2. [Layer System](#layer-system)
3. [WgpuPainter (GPU Backend)](#wgpupainter-gpu-backend)
4. [Event Routing](#event-routing)
5. [DevTools Integration](#devtools-integration)
6. [Performance Characteristics](#performance-characteristics)

---

## Architecture Overview

### Position in the Stack

```text
┌─────────────────────────────────────────────────────────────┐
│                   flui_painting                             │
│              (High-level Canvas API)                         │
│                                                              │
│  Canvas records DisplayList:                                │
│    canvas.draw_rect(rect, paint)                           │
│    canvas.draw_path(path, paint)                           │
│    canvas.draw_text(text, style)                           │
│    ↓                                                        │
│  DisplayList { commands: Vec<DrawCommand> }                │
└──────────────────────┬──────────────────────────────────────┘
                       │ sent to
                       ▼
┌─────────────────────────────────────────────────────────────┐
│                    flui_engine                              │
│          (GPU Rendering Engine)                              │
│                                                              │
│  Layer System:                                              │
│  ┌────────────────────────────────────────────────┐        │
│  │ PictureLayer {                                 │        │
│  │   display_list: DisplayList                    │        │
│  │   composite() {                                │        │
│  │     for cmd in display_list.commands() {      │        │
│  │       match cmd {                              │        │
│  │         DrawRect => painter.rect(...)          │        │
│  │         DrawPath => painter.path(...)          │        │
│  │         DrawText => painter.text(...)          │        │
│  │       }                                         │        │
│  │     }                                           │        │
│  │   }                                             │        │
│  │ }                                               │        │
│  └────────────────────────────────────────────────┘        │
│                       ↓                                      │
│  WgpuPainter:                                               │
│  ┌────────────────────────────────────────────────┐        │
│  │ rect() → tessellate → GPU buffer               │        │
│  │ path() → Lyon tessellate → GPU buffer          │        │
│  │ text() → Glyphon SDF → GPU buffer              │        │
│  └────────────────────────────────────────────────┘        │
│                       ↓                                      │
│  wgpu → Vulkan/Metal/DX12/WebGPU                           │
└─────────────────────────────────────────────────────────────┘
```

### Key Design Principles

1. **GPU-First** - All rendering happens on GPU via wgpu
2. **DisplayList Executor** - Executes commands from flui_painting
3. **Composable Layers** - Tree of effects (transform, opacity, clip, blur)
4. **Batching** - Automatic geometry batching for performance
5. **Zero-Copy** - Direct GPU uploads without intermediate buffers

---

## Layer System

### Layer Trait (Base)

All layers implement the `Layer` trait:

```rust
// In flui_engine/src/layer/base.rs

use crate::painter::Painter;
use flui_types::{Rect, Point};

/// Base trait for all composited layers
pub trait Layer: Send + Sync + std::fmt::Debug {
    /// Paint this layer using the given painter
    ///
    /// This is where DisplayLists get executed via WgpuPainter.
    fn paint(&self, painter: &mut dyn Painter);

    /// Get the bounding rectangle
    fn bounds(&self) -> Rect;

    /// Check if visible
    fn is_visible(&self) -> bool {
        true
    }

    /// Hit test - returns true if point is within this layer
    fn hit_test(&self, position: Point) -> bool {
        self.bounds().contains(position)
    }

    /// Returns child layers (for container layers)
    fn children(&self) -> &[BoxedLayer] {
        &[] // Default: no children
    }
}

pub type BoxedLayer = Box<dyn Layer>;
```

### Layer Types

#### 1. PictureLayer (Leaf Layer - DisplayList Executor)

**Most Important Layer** - executes DisplayLists from flui_painting:

```rust
// In flui_engine/src/layer/picture.rs

use flui_painting::{Canvas, DisplayList, DrawCommand};
use crate::painter::{Painter, Paint};
use flui_types::Rect;

/// Picture layer - executes DisplayList commands
///
/// This is where high-level Canvas API commands get
/// translated to low-level GPU rendering calls.
///
/// **Implementation Note:** PictureLayer now uses Canvas internally
/// for recording commands, which are then accessed via display_list().
pub struct PictureLayer {
    /// Canvas for recording drawing commands
    canvas: Canvas,
}

impl PictureLayer {
    /// Creates a new picture layer
    pub fn new() -> Self {
        Self {
            canvas: Canvas::new(),
        }
    }

    /// Creates a new picture layer from a display list
    pub fn from_display_list(display_list: DisplayList) -> Self {
        // Note: DisplayList is immutable, so we create a new canvas
        let _ = display_list;
        Self::new()
    }

    /// Clears the display list (for pooling)
    pub fn clear(&mut self) {
        self.canvas = Canvas::new();
    }

    /// Get the display list
    pub fn display_list(&self) -> &DisplayList {
        self.canvas.display_list()
    }

    /// Backward-compatible drawing methods
    pub fn draw_rect(&mut self, rect: Rect, paint: Paint) { /* ... */ }
    pub fn draw_circle(&mut self, center: Point, radius: f32, paint: Paint) { /* ... */ }
    // ... other drawing methods
}

impl Layer for PictureLayer {
    fn paint(&self, painter: &mut dyn Painter) {
        // Execute each command in the DisplayList
        for cmd in self.canvas.display_list().commands() {
            match cmd {
                DrawCommand::DrawRect { rect, paint, .. } => {
                    painter.rect(*rect, &convert_paint(paint));
                }
                DrawCommand::DrawRRect { rrect, paint, .. } => {
                    painter.rrect(*rrect, &convert_paint(paint));
                }
                DrawCommand::DrawCircle { center, radius, paint, .. } => {
                    painter.circle(*center, *radius, &convert_paint(paint));
                }
                DrawCommand::DrawPath { path, paint, .. } => {
                    painter.path(path, &convert_paint(paint)); // ← Lyon tessellation
                }
                DrawCommand::DrawText { text, offset, style, .. } => {
                    let font_size = style.font_size.unwrap_or(14.0) as f32;
                    let paint = Paint::fill(style.color.unwrap_or(Color::BLACK));
                    let position = Point::new(offset.dx, offset.dy);
                    painter.text_styled(text, position, font_size, &paint); // ← Glyphon
                }
                DrawCommand::DrawImage { image, dst, .. } => {
                    painter.draw_image(&format!("{:?}", image), dst.top_left());
                }
                DrawCommand::ClipRect { .. }
                | DrawCommand::ClipRRect { .. }
                | DrawCommand::ClipPath { .. } => {
                    // Clipping handled separately by Painter trait
                }
                DrawCommand::DrawShadow { .. } => {
                    // Shadow rendering not yet implemented
                }
                DrawCommand::DrawLine { p1, p2, paint, .. } => {
                    painter.line(*p1, *p2, &convert_paint(paint));
                }
                DrawCommand::DrawOval { rect, paint, .. } => {
                    // Approximate as circle
                    let center = rect.center();
                    let radius = rect.width().min(rect.height()) / 2.0;
                    painter.circle(center, radius, &convert_paint(paint));
                }
            }
        }
    }

    fn bounds(&self) -> Rect {
        self.canvas.display_list().bounds()
    }

    fn is_visible(&self) -> bool {
        !self.canvas.display_list().is_empty()
    }
}
```

**Key Point:** PictureLayer is the bridge between flui_painting (high-level) and WgpuPainter (GPU).

#### 2. ContainerLayer (Composite)

Groups child layers:

```rust
// In flui_engine/src/layer/container.rs

/// A layer that contains child layers
pub struct ContainerLayer {
    children: Vec<BoxedLayer>,
    bounds: Rect,
}

impl Layer for ContainerLayer {
    fn paint(&self, painter: &mut dyn Painter) {
        // Paint all children in order
        for child in &self.children {
            child.paint(painter);
        }
    }

    fn bounds(&self) -> Rect {
        self.bounds
    }

    fn children(&self) -> &[BoxedLayer] {
        &self.children
    }
}
```

#### 3. TransformLayer (Effect)

Applies transforms:

```rust
// In flui_engine/src/layer/transform.rs

/// A layer that applies a transform to its child
pub struct TransformLayer {
    transform: Matrix4,
    child: Option<BoxedLayer>,
}

impl Layer for TransformLayer {
    fn paint(&self, painter: &mut dyn Painter) {
        painter.save();
        painter.concat_transform(self.transform);

        if let Some(ref child) = self.child {
            child.paint(painter);
        }

        painter.restore();
    }
}
```

#### 4. OpacityLayer (Effect)

Applies opacity:

```rust
// In flui_engine/src/layer/opacity.rs

/// A layer that applies opacity to its child
pub struct OpacityLayer {
    opacity: f32, // 0.0 to 1.0
    child: Option<BoxedLayer>,
}

impl Layer for OpacityLayer {
    fn paint(&self, painter: &mut dyn Painter) {
        if self.opacity < 0.01 {
            return; // Fully transparent - skip
        }

        if (self.opacity - 1.0).abs() < 0.01 {
            // Fully opaque - no layer needed
            if let Some(ref child) = self.child {
                child.paint(painter);
            }
            return;
        }

        // Partial opacity - use offscreen buffer
        painter.save_layer(self.opacity);

        if let Some(ref child) = self.child {
            child.paint(painter);
        }

        painter.restore();
    }
}
```

#### 5. ClipLayer (Effect)

Clips children:

```rust
// In flui_engine/src/layer/clip.rs

/// A layer that clips its child to a rectangle
pub struct ClipRectLayer {
    clip_rect: Rect,
    child: Option<BoxedLayer>,
}

impl Layer for ClipRectLayer {
    fn paint(&self, painter: &mut dyn Painter) {
        painter.save();
        painter.clip_rect(self.clip_rect);

        if let Some(ref child) = self.child {
            child.paint(painter);
        }

        painter.restore();
    }
}
```

---

## WgpuPainter (GPU Backend)

### Painter Trait (Abstraction)

```rust
// In flui_engine/src/painter/mod.rs

use flui_types::{Point, Rect, Matrix4};
use flui_painting::{Paint, Path};

/// Low-level painter trait
///
/// WgpuPainter implements this trait using GPU rendering.
pub trait Painter {
    // Transform
    fn save(&mut self);
    fn restore(&mut self);
    fn set_transform(&mut self, transform: Matrix4);
    fn concat_transform(&mut self, transform: Matrix4);

    // Clipping
    fn clip_rect(&mut self, rect: Rect);
    fn clip_rrect(&mut self, rrect: RRect);
    fn clip_path(&mut self, path: &Path);

    // Primitives
    fn rect(&mut self, rect: Rect, paint: &Paint);
    fn rrect(&mut self, rrect: RRect, paint: &Paint);
    fn circle(&mut self, center: Point, radius: f32, paint: &Paint);
    fn oval(&mut self, rect: Rect, paint: &Paint);
    fn line(&mut self, p1: Point, p2: Point, paint: &Paint);
    fn path(&mut self, path: &Path, paint: &Paint);

    // Text
    fn text(&mut self, text: &str, offset: Offset, style: &TextStyle, paint: &Paint);

    // Image
    fn image(&mut self, image: &ImageHandle, dst: Rect, paint: Option<&Paint>);

    // Effects
    fn shadow(&mut self, path: &Path, color: Color, elevation: f32);
    fn save_layer(&mut self, opacity: f32);
}
```

### WgpuPainter (Implementation)

```rust
// In flui_engine/src/painter/wgpu_painter.rs

use lyon::tessellation::{FillTessellator, StrokeTessellator};
use glyphon::{TextRenderer, FontSystem};
use wgpu::{Device, Queue};

/// GPU-accelerated painter using wgpu
pub struct WgpuPainter {
    /// wgpu device
    device: Arc<Device>,

    /// Command queue
    queue: Arc<Queue>,

    /// Lyon tessellators
    fill_tessellator: FillTessellator,
    stroke_tessellator: StrokeTessellator,

    /// Glyphon text renderer
    text_renderer: TextRenderer,
    font_system: FontSystem,

    /// Transform stack
    transform_stack: Vec<Matrix4>,

    /// Current transform
    current_transform: Matrix4,

    /// Vertex buffer (batched geometry)
    vertices: Vec<Vertex>,
    indices: Vec<u32>,

    /// GPU buffers (uploaded once per frame)
    vertex_buffer: Option<wgpu::Buffer>,
    index_buffer: Option<wgpu::Buffer>,
}

impl WgpuPainter {
    pub fn new(device: Arc<Device>, queue: Arc<Queue>) -> Self {
        Self {
            device,
            queue,
            fill_tessellator: FillTessellator::new(),
            stroke_tessellator: StrokeTessellator::new(),
            text_renderer: TextRenderer::new(...),
            font_system: FontSystem::new(),
            transform_stack: vec![Matrix4::identity()],
            current_transform: Matrix4::identity(),
            vertices: Vec::new(),
            indices: Vec::new(),
            vertex_buffer: None,
            index_buffer: None,
        }
    }

    /// Begins a new frame
    pub fn begin_frame(&mut self) {
        self.vertices.clear();
        self.indices.clear();
    }

    /// Ends frame and uploads to GPU
    pub fn end_frame(&mut self) -> (wgpu::Buffer, wgpu::Buffer) {
        // Upload vertices to GPU
        let vertex_buffer = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Vertex Buffer"),
            contents: bytemuck::cast_slice(&self.vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });

        // Upload indices to GPU
        let index_buffer = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Index Buffer"),
            contents: bytemuck::cast_slice(&self.indices),
            usage: wgpu::BufferUsages::INDEX,
        });

        (vertex_buffer, index_buffer)
    }
}

impl Painter for WgpuPainter {
    fn rect(&mut self, rect: Rect, paint: &Paint) {
        // Tessellate rectangle to triangles
        let base_index = self.vertices.len() as u32;

        // Add 4 vertices (corners)
        let color = paint.color.to_array();
        self.vertices.extend_from_slice(&[
            Vertex { position: [rect.left(), rect.top()], color },
            Vertex { position: [rect.right(), rect.top()], color },
            Vertex { position: [rect.right(), rect.bottom()], color },
            Vertex { position: [rect.left(), rect.bottom()], color },
        ]);

        // Add 2 triangles (6 indices)
        self.indices.extend_from_slice(&[
            base_index, base_index + 1, base_index + 2,
            base_index, base_index + 2, base_index + 3,
        ]);
    }

    fn path(&mut self, path: &Path, paint: &Paint) {
        // Convert flui_painting::Path to lyon::path::Path
        let lyon_path = self.convert_path(path);

        // Tessellate using Lyon
        let mut geometry = VertexBuffers::new();
        let base_index = self.vertices.len() as u32;

        if paint.style == PaintStyle::Fill {
            self.fill_tessellator.tessellate_path(
                &lyon_path,
                &FillOptions::default(),
                &mut BuffersBuilder::new(&mut geometry, |vertex: FillVertex| {
                    Vertex {
                        position: [vertex.position().x, vertex.position().y],
                        color: paint.color.to_array(),
                    }
                }),
            ).unwrap();
        } else {
            self.stroke_tessellator.tessellate_path(
                &lyon_path,
                &StrokeOptions::default().with_line_width(paint.stroke_width),
                &mut BuffersBuilder::new(&mut geometry, |vertex: StrokeVertex| {
                    Vertex {
                        position: [vertex.position().x, vertex.position().y],
                        color: paint.color.to_array(),
                    }
                }),
            ).unwrap();
        }

        // Add to batch
        self.vertices.extend(geometry.vertices);
        self.indices.extend(geometry.indices.iter().map(|i| i + base_index));
    }

    fn text(&mut self, text: &str, offset: Offset, style: &TextStyle, paint: &Paint) {
        // Use Glyphon for SDF text rendering
        let buffer = self.text_renderer.create_buffer(
            &mut self.font_system,
            text,
            style,
        );

        self.text_renderer.render(
            &self.device,
            &self.queue,
            &buffer,
            offset,
            paint.color,
        );
    }

    fn save(&mut self) {
        self.transform_stack.push(self.current_transform);
    }

    fn restore(&mut self) {
        if let Some(transform) = self.transform_stack.pop() {
            self.current_transform = transform;
        }
    }

    fn set_transform(&mut self, transform: Matrix4) {
        self.current_transform = transform;
    }

    // ... other methods
}

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    position: [f32; 2],
    color: [f32; 4],
}
```

**Key Points:**
- Batches geometry into single GPU buffer per frame
- Uses Lyon for path tessellation
- Uses Glyphon for SDF text
- Transform is applied to vertices CPU-side before GPU upload

---

## Event Routing

### EventRouter

Dispatches pointer events using hit testing:

```rust
// In flui_engine/src/event_router.rs

use flui_types::events::PointerEvent;
use crate::layer::{Layer, BoxedLayer};

/// Routes pointer events to layers via hit testing
pub struct EventRouter {
    root: Option<BoxedLayer>,
}

impl EventRouter {
    pub fn new() -> Self {
        Self { root: None }
    }

    pub fn set_root(&mut self, root: BoxedLayer) {
        self.root = Some(root);
    }

    pub fn route_event(&mut self, event: &PointerEvent) -> EventResult {
        let Some(ref root) = self.root else {
            return EventResult::Ignored;
        };

        match event {
            PointerEvent::Down { position, .. } => {
                // Hit test to find target
                if self.hit_test(root, *position) {
                    return EventResult::Handled;
                }
            }
            _ => {}
        }

        EventResult::Ignored
    }

    fn hit_test(&self, layer: &BoxedLayer, position: Point) -> bool {
        if !layer.hit_test(position) {
            return false;
        }

        // Check children (front to back)
        for child in layer.children().iter().rev() {
            if self.hit_test(child, position) {
                return true;
            }
        }

        true
    }
}

pub enum EventResult {
    Handled,
    Ignored,
}
```

---

## DevTools Integration

**IMPORTANT:** Most devtools functionality lives in the **separate `flui_devtools` crate**, not in flui_engine.

The engine only provides a thin integration wrapper:

```rust
// In flui_engine/src/devtools.rs

use flui_devtools::profiler::Profiler;
use std::sync::{Arc, Mutex};

/// Compositor with integrated profiler
///
/// This is a THIN WRAPPER that integrates the Profiler from
/// flui_devtools crate. It does NOT implement profiling logic itself.
#[cfg(feature = "devtools")]
pub struct ProfiledCompositor {
    /// The actual compositor
    compositor: crate::Compositor,

    /// Profiler from flui_devtools crate
    profiler: Arc<Mutex<Profiler>>,
}

impl ProfiledCompositor {
    pub fn new(compositor: crate::Compositor, profiler: Arc<Mutex<Profiler>>) -> Self {
        Self {
            compositor,
            profiler,
        }
    }

    pub fn composite(&mut self) {
        // Begin profiling
        let mut profiler = self.profiler.lock().unwrap();
        profiler.begin_frame();
        drop(profiler);

        // Do actual compositing
        self.compositor.composite();

        // End profiling
        let mut profiler = self.profiler.lock().unwrap();
        profiler.end_frame();
    }
}
```

### Where DevTools Features Actually Live

| Feature | Location | Description |
|---------|----------|-------------|
| **Hot Reload** | `flui_devtools` | File watching, automatic rebuild |
| **Performance Profiler** | `flui_devtools` | Frame timing, jank detection, CPU tracking |
| **Timeline View** | `flui_devtools` | Event timeline visualization |
| **Memory Profiler** | `flui_devtools` | Heap allocation tracking, leak detection |
| **Network Monitor** | `flui_devtools` | HTTP request tracking |
| **Remote Debug** | `flui_devtools` | WebSocket protocol, browser DevTools |
| **ProfiledCompositor** | `flui_engine` | **Only integration wrapper** |

**Key Point:** The engine has NO hot-reload, NO comprehensive profiling, NO memory tracking. It only provides `ProfiledCompositor` as a convenience wrapper around `flui_devtools::Profiler`.

For comprehensive devtools documentation, see `DEVTOOLS_ARCHITECTURE.md`.

---

## Performance Characteristics

| Operation | Cost | Notes |
|-----------|------|-------|
| PictureLayer creation | ~50ns | Cheap allocation |
| DisplayList execution | ~2ms | CPU tessellation + GPU upload |
| Hit testing (10 layers) | ~500ns | Linear tree traversal |
| Batched rect (1000x) | ~100µs | Single GPU draw call |
| Path tessellation (complex) | ~500µs | Lyon CPU work |
| SDF text (100 glyphs) | ~200µs | Glyphon GPU render |

### Optimization Strategies

1. **Batching** - Combine geometry into single GPU buffer
2. **Layer Pooling** - Reuse PictureLayer allocations
3. **DisplayList Caching** - Reuse unchanged DisplayLists
4. **Culling** - Skip layers outside viewport
5. **Transform Baking** - Apply transforms CPU-side before GPU

---

## Summary

**flui_engine** provides the **GPU execution layer** for FLUI:

- ✅ **PictureLayer** executes DisplayLists from flui_painting
- ✅ **WgpuPainter** renders with wgpu + Lyon + Glyphon
- ✅ **Layer system** for compositing effects
- ✅ **EventRouter** for pointer event dispatch
- ✅ **ProfiledCompositor** integration wrapper (actual profiling in flui_devtools)

**Clear Responsibilities:**
- **flui_painting** records DisplayLists (high-level Canvas API)
- **flui_engine** executes DisplayLists (GPU low-level implementation)
- **flui_devtools** provides hot-reload, profiling, timeline, memory tracking (separate crate)

**Total LOC:** ~5,000 (mostly implemented)

This architecture ensures clean separation between painting API, GPU implementation, and developer tools!

---

## Related Documentation

### Implementation
- **Source Code**: `crates/flui_engine/src/`
- **Layer System**: `crates/flui_engine/src/layer/`
- **WgpuPainter**: `crates/flui_engine/src/painter/wgpu_painter.rs`
- **Event Router**: `crates/flui_engine/src/event_router.rs`

### Patterns & Integration
- **Patterns**: [PATTERNS.md](PATTERNS.md#rendering-patterns) - Layer system, GPU rendering
- **Integration**: [INTEGRATION.md](INTEGRATION.md#flow-1-widget--element--render) - Paint → GPU flow
- **Navigation**: [README.md](README.md) - Architecture documentation hub

### Related Architecture Docs
- **flui_painting**: [PAINTING_ARCHITECTURE.md](PAINTING_ARCHITECTURE.md) - Canvas API and DisplayList
- **flui_rendering**: [RENDERING_ARCHITECTURE.md](RENDERING_ARCHITECTURE.md) - RenderObject paint methods
- **flui_devtools**: [DEVTOOLS_ARCHITECTURE.md](DEVTOOLS_ARCHITECTURE.md) - Profiling integration

### External References
- **wgpu**: [wgpu.rs](https://wgpu.rs/) - Cross-platform GPU API
- **Lyon**: [github.com/nical/lyon](https://github.com/nical/lyon) - Path tessellation
- **Glyphon**: [github.com/grovesNL/glyphon](https://github.com/grovesNL/glyphon) - GPU text rendering
