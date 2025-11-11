# FLUI Engine

Low-level GPU-accelerated compositor for FLUI. This crate implements **ONLY compositor primitives** - all high-level layers (Transform, Opacity, Clip, etc.) belong in `flui_rendering`.

## Clean Architecture (v0.7.0+)

flui-engine follows Clean Architecture with strict separation of concerns:

- **Engine**: Low-level compositor primitives (Picture, Container)
- **Rendering**: High-level RenderObjects (Transform, Opacity, Clip, Layout)
- **Widgets**: User-facing UI components

## Architecture

```text
RenderObject (flui_rendering)
    ↓
Canvas → DisplayList (flui_painting)
    ↓
PictureLayer (flui_engine)
    ↓
CommandRenderer (Visitor Pattern)
    ↓
WgpuPainter → GPU (wgpu)
```

## Compositor Primitives (Low-Level Only)

### Layer System

**ContainerLayer** - Groups multiple child layers
```rust
let mut container = ContainerLayer::new();
container.append_child(picture1);
container.append_child(picture2);
```

**PictureLayer** - Leaf layer with drawing commands
```rust
let mut picture = PictureLayer::from_canvas(canvas);

// Modern API (Clean Architecture)
let mut renderer = WgpuRenderer::new(painter);
picture.render(&mut renderer);

// Legacy API (deprecated)
picture.paint(&mut painter); // Still works but deprecated
```

### CommandRenderer (Visitor Pattern)

Clean separation between command generation and execution:

```rust
use flui_engine::{WgpuRenderer, CommandRenderer};

// Production GPU renderer
let mut renderer = WgpuRenderer::new(painter);
picture_layer.render(&mut renderer);

// Debug renderer (development)
#[cfg(debug_assertions)]
let mut debug_renderer = DebugRenderer::new(viewport);
picture_layer.render(&mut debug_renderer);
```

### Paint Type (Unified)

All painting uses `flui_painting::Paint` (no duplicates):

```rust
use flui_painting::Paint;

let paint = Paint::fill(Color::RED);
let paint = Paint::stroke(Color::BLUE, 2.0);
```

## What's NOT in Engine

The following are **NOT** compositor primitives and belong in `flui_rendering`:

- ❌ Transform, Opacity, Offset (layout logic)
- ❌ Clipping (handled by RenderObjects)
- ❌ Event handling (pointer, scroll, gestures)
- ❌ Layer pooling (premature optimization)

Use `RenderTransform`, `RenderOpacity`, etc. from `flui_rendering` instead.

## Usage

### Basic Example

```rust
use flui_engine::{PictureLayer, WgpuRenderer};
use flui_painting::Canvas;

// Create canvas with drawing commands
let mut canvas = Canvas::new();
canvas.draw_rect(rect, &Paint::fill(Color::RED));

// Create picture layer
let picture = PictureLayer::from_canvas(canvas);

// Render using Clean Architecture
let mut renderer = WgpuRenderer::new(painter);
picture.render(&mut renderer);
```

### Multiple Layers

```rust
use flui_engine::ContainerLayer;

let mut container = ContainerLayer::new();

// Add multiple picture layers
for canvas in canvases {
    container.append_child(Box::new(PictureLayer::from_canvas(canvas)));
}

// Render entire tree
let mut renderer = WgpuRenderer::new(painter);
container.paint(&mut painter); // Container uses legacy API
```

## Features

### GPU Acceleration

- **wgpu**: Cross-platform GPU rendering (Vulkan/Metal/DX12/WebGPU)
- **Instanced rendering**: 100x performance for UI primitives
- **Buffer pooling**: Minimal allocation overhead
- **Tessellation**: Lyon-based path to triangle conversion

### Clean Architecture

- **Visitor Pattern**: Commands execute polymorphically
- **SOLID Principles**: Single responsibility, open/closed, etc.
- **Type Safety**: Compile-time checks, no `Any` casts
- **Zero Unsafe**: All raw pointers replaced with `Arc`

### Performance

- **98% complexity reduction**: PictureLayer went from 250 lines → 5 lines
- **Zero conversion overhead**: Single Paint type
- **Cache-friendly**: Contiguous memory layout
- **Parallel-ready**: Thread-safe design

## Migration from v0.6.0

### Paint Type
```rust
// OLD (v0.6.0)
use flui_engine::painter::Paint;

// NEW (v0.7.0+)
use flui_painting::Paint;
```

### PictureLayer Rendering
```rust
// OLD (deprecated)
picture.paint(&mut painter);

// NEW (recommended)
let mut renderer = WgpuRenderer::new(painter);
picture.render(&mut renderer);
```

### Deleted Layers
```rust
// OLD (v0.6.0) - these are DELETED
use flui_engine::layer::{
    TransformLayer,  // Use RenderTransform
    OpacityLayer,    // Use RenderOpacity
    ClipRectLayer,   // Use RenderClipRect
    OffsetLayer,     // Use RenderPositioned
    ScrollableLayer, // Use RenderScrollView
};

// NEW (v0.7.0+) - use RenderObjects
use flui_rendering::{
    RenderTransform,
    RenderOpacity,
    RenderClipRect,
    RenderPositioned,
    RenderScrollView,
};
```

## Documentation

- **REFACTORING_SUMMARY.md** - Complete refactoring overview
- **CLEAN_ARCHITECTURE_MIGRATION.md** - Migration guide
- **TODO_PHASE2_CLEANUP.md** - Cleanup tracking

## Version

- **Current**: v0.7.0 (Clean Architecture)
- **Breaking Changes**: Yes (Paint unification, layer removal)
- **Migration Deadline**: v0.8.0 (legacy `paint()` removed)

## License

MIT OR Apache-2.0
