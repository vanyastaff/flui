# Architecture Guide

This document explains the internal architecture of `flui_painting` and how it integrates with the rest of FLUI.

## Overview

`flui_painting` implements the **Command Pattern** to separate drawing command recording from GPU execution. This enables:

- **Deferred rendering** - Record now, execute later on GPU
- **Caching** - Reuse DisplayLists across frames
- **Thread safety** - Record on one thread, execute on another
- **Optimization** - Analyze and transform commands before execution

## Component Architecture

```text
┌─────────────────────────────────────────────────────────────┐
│                    flui_rendering                           │
│  RenderObject::paint(ctx: &mut PaintingContext)            │
└────────────────────────┬────────────────────────────────────┘
                         │ Creates Canvas
                         ▼
┌─────────────────────────────────────────────────────────────┐
│                    flui_painting (this crate)               │
│  ┌──────────┐         ┌──────────────┐                     │
│  │  Canvas  │ ──────► │ DisplayList  │                     │
│  │ (mutable)│ records │ (immutable)  │                     │
│  └──────────┘         └──────────────┘                     │
│       │                      │                              │
│       │ Transform Stack      │ Command Sequence             │
│       │ Clip Stack          │ Bounds Tracking              │
│       │ Save/Restore        │ Statistics                   │
└───────┼──────────────────────┼──────────────────────────────┘
        │                      │
        │ finish()             │ Send to GPU thread
        ▼                      ▼
┌─────────────────────────────────────────────────────────────┐
│                     flui_engine                             │
│  WgpuPainter::paint(display_list: &DisplayList)            │
│  ┌────────────┐  ┌─────────────┐  ┌──────────────┐        │
│  │ Tessellate │→ │ GPU Buffers │→ │ Render Pass  │        │
│  └────────────┘  └─────────────┘  └──────────────┘        │
└─────────────────────────────────────────────────────────────┘
```

## Core Types

### Canvas

**Purpose:** Mutable recording context for drawing commands.

**Responsibilities:**
- Record drawing operations into command sequence
- Maintain transform matrix stack
- Manage clipping regions
- Track save/restore state

**Key Implementation Details:**

```rust
pub struct Canvas {
    display_list: DisplayList,        // Accumulated commands
    transform: Matrix4,                // Current transform
    clip_stack: Vec<ClipShape>,        // Active clips
    save_stack: Vec<CanvasState>,      // Saved states
}
```

**Thread Safety:** `Send` but not `Sync` - designed for single-threaded recording, multi-threaded consumption.

### DisplayList

**Purpose:** Immutable sequence of drawing commands ready for GPU execution.

**Responsibilities:**
- Store drawing commands
- Provide command iteration and filtering
- Track bounding boxes
- Enable caching and reuse

**Key Implementation Details:**

```rust
pub struct DisplayList {
    commands: Vec<DrawCommand>,        // Recorded operations
    bounds: Rect,                      // Bounding box
    hit_regions: Vec<HitRegion>,       // Event handling
}
```

**Thread Safety:** `Send + Clone` - can be shared across threads, cheap to clone (Arc internally).

### DrawCommand

**Purpose:** Individual drawing operation with all parameters.

**Design:** Large enum with variants for each operation type.

```rust
pub enum DrawCommand {
    // Shapes
    DrawRect { rect: Rect, paint: Paint, transform: Matrix4 },
    DrawCircle { center: Point, radius: f32, paint: Paint, transform: Matrix4 },
    DrawPath { path: Box<Path>, paint: Paint, transform: Matrix4 },

    // Text & Images
    DrawText { /* ... */ },
    DrawImage { /* ... */ },

    // State
    SaveLayer { /* ... */ },
    RestoreLayer { /* ... */ },

    // Clipping
    ClipRect { /* ... */ },
    ClipPath { /* ... */ },

    // Effects
    ShaderMask { /* ... */ },
    BackdropFilter { /* ... */ },
}
```

**Why enum?** Type-safe, pattern matching, efficient storage.

## Data Flow

### 1. Recording Phase (CPU)

```rust
// RenderObject creates canvas
let mut canvas = Canvas::new();

// Apply transforms
canvas.save();
canvas.translate(100.0, 50.0);
canvas.rotate(PI / 4.0);

// Record drawing operations
canvas.draw_rect(rect, &paint);
canvas.draw_circle(center, radius, &paint);

canvas.restore();

// Finalize immutable DisplayList
let display_list = canvas.finish();
```

**What happens:**
1. Canvas maintains current transform matrix
2. Each draw call creates DrawCommand with current transform
3. Commands appended to DisplayList
4. Bounds updated incrementally

### 2. Composition Phase (CPU)

```rust
// Parent canvas
let mut parent = Canvas::new();

// Children render to their own canvases
let child1 = render_child_1();
let child2 = render_child_2();

// Zero-copy composition
parent.append_canvas(child1);  // O(1) move if parent is empty
parent.append_canvas(child2);  // O(n) append if parent has commands

let final_list = parent.finish();
```

**Optimization:** First child uses `mem::swap` for O(1) composition.

### 3. Execution Phase (GPU)

```rust
// In flui_engine
let painter = WgpuPainter::new(device, queue);

// Execute display list
painter.paint(&display_list);
```

**What happens:**
1. Iterate commands in order
2. Tessellate paths to triangles (lyon)
3. Upload vertices to GPU buffers
4. Execute render pass with recorded state

## Design Patterns

### 1. Command Pattern

**Intent:** Encapsulate drawing operations as objects.

**Benefits:**
- Decouple recording from execution
- Enable undo/redo (not currently used)
- Support caching and replay
- Allow command transformation

### 2. Builder Pattern

**Intent:** Fluent API for canvas operations.

**Example:**

```rust
canvas
    .saved()
    .translated(100.0, 50.0)
    .rotated(PI / 4.0)
    .rect(rect, &paint)
    .restored();
```

**Benefits:**
- Readable, chainable API
- Automatic state cleanup
- Type-safe transformations

### 3. Extension Traits

**Intent:** Add methods without modifying core types.

**Example:**

```rust
// Sealed core trait
pub trait DisplayListCore {
    fn commands(&self) -> impl Iterator<Item = &DrawCommand>;
    fn bounds(&self) -> Rect;
    fn len(&self) -> usize;
}

// Public extension trait
pub trait DisplayListExt: DisplayListCore {
    fn draw_commands(&self) -> impl Iterator<Item = &DrawCommand> {
        self.commands().filter(|c| c.is_draw())
    }
}

// Blanket implementation
impl<T: DisplayListCore> DisplayListExt for T {}
```

**Benefits:**
- Future-proof API
- Users can add their own extensions
- Keeps core trait minimal

### 4. Zero-Cost Abstractions

**Intent:** High-level API with zero runtime overhead.

**Examples:**

1. **Transform enum → Matrix4:**

```rust
pub enum Transform {
    Translate(f32, f32),
    Rotate(f32),
    Scale(f32, f32),
}

impl From<Transform> for Matrix4 {
    // Compiles to direct matrix construction
}
```

2. **Scoped operations:**

```rust
canvas.with_save(|c| {
    c.draw_rect(rect, &paint);
});

// Compiles to:
canvas.save();
canvas.draw_rect(rect, &paint);
canvas.restore();
```

## Memory Management

### Canvas Memory

**Allocation Strategy:**
- `Vec<DrawCommand>` grows dynamically
- `reset()` clears but keeps capacity
- Reuse across frames to avoid allocations

**Typical Pattern:**

```rust
// Per-frame rendering
let mut canvas = Canvas::new();

loop {
    canvas.reset();  // Clear but keep allocations

    // Render frame
    render_frame(&mut canvas);

    let display_list = canvas.finish();
    gpu_thread.send(display_list);
}
```

### DisplayList Memory

**Sharing Strategy:**
- Commands stored in `Vec<DrawCommand>`
- Cheap clone via internal Arc (future optimization)
- Can be cached and reused

**Example:**

```rust
// Cache static content
let background = Canvas::record(|c| {
    c.draw_rect(viewport, &Paint::fill(Color::WHITE));
});

// Reuse every frame
loop {
    let mut canvas = Canvas::new();
    canvas.append_display_list(background.clone());
    // ... rest of frame
}
```

## Transform System

### Transform Stack

Canvas maintains a transform stack for hierarchical transformations:

```rust
canvas.save();                    // Push current transform
canvas.translate(100.0, 50.0);   // Modify transform
canvas.rotate(PI / 4.0);          // Accumulates

// Draw with accumulated transform
canvas.draw_rect(rect, &paint);

canvas.restore();                 // Pop transform
```

**Implementation:**

```rust
struct CanvasState {
    transform: Matrix4,
    clip_depth: usize,
    is_layer: bool,
}

// On save()
self.save_stack.push(CanvasState {
    transform: self.transform,
    clip_depth: self.clip_stack.len(),
    is_layer: false,
});

// On restore()
let state = self.save_stack.pop().unwrap();
self.transform = state.transform;
self.clip_stack.truncate(state.clip_depth);
```

### Transform Baking

Each DrawCommand stores the transform active when it was recorded:

```rust
DrawCommand::DrawRect {
    rect: Rect::from_ltrb(0.0, 0.0, 100.0, 100.0),
    paint: Paint::fill(Color::RED),
    transform: Matrix4::translation(100.0, 50.0, 0.0) * Matrix4::rotation_z(PI / 4.0),
}
```

**Why bake?**
- DisplayList is immutable - no runtime transform computation
- GPU receives final matrices directly
- Enables culling and optimization

## Clipping System

### Clip Stack

Similar to transform stack, clips accumulate:

```rust
enum ClipShape {
    Rect(Rect),
    RRect(RRect),
    Path(Box<Path>),
}

canvas.clip_stack: Vec<ClipShape>
```

**Current Implementation:**
- Clips stored for future optimizations
- Not yet used for culling (TODO)
- Restored on `restore()`

**Future Optimizations:**

```rust
// Cull objects outside clip
if canvas.would_be_clipped(&rect) {
    return; // Skip expensive drawing
}

// Query clip bounds
let visible_area = canvas.local_clip_bounds();
```

## Performance Characteristics

| Operation | Time Complexity | Notes |
|-----------|----------------|-------|
| `Canvas::new()` | O(1) | Empty allocations |
| `draw_*()` | O(1) | Append to Vec |
| `append_canvas()` (empty parent) | O(1) | Vec swap |
| `append_canvas()` (non-empty) | O(n) | Vec extend |
| `finish()` | O(1) | Move DisplayList |
| `reset()` | O(n) | Clear Vec, keep capacity |
| `DisplayList::clone()` | O(n) | Deep clone (Arc planned) |
| Iteration | O(n) | Linear scan |
| Filtering | O(n) | Lazy iterator |

## Integration Points

### With flui_rendering

```rust
impl RenderBox for MyRenderObject {
    fn paint(&self, ctx: &mut PaintingContext) {
        let canvas = ctx.canvas();

        // Use Canvas API
        canvas.save();
        canvas.translate(offset.dx, offset.dy);
        self.paint_children(ctx);
        canvas.restore();
    }
}
```

### With flui_engine

```rust
impl WgpuPainter {
    pub fn paint(&mut self, display_list: &DisplayList) {
        for cmd in display_list.commands() {
            match cmd {
                DrawCommand::DrawRect { rect, paint, transform } => {
                    self.draw_rect_internal(*rect, paint, *transform);
                }
                // ... other commands
            }
        }
    }
}
```

## Future Enhancements

### 1. Arc-based DisplayList

```rust
pub struct DisplayList {
    inner: Arc<DisplayListInner>,
}

struct DisplayListInner {
    commands: Vec<DrawCommand>,
    bounds: Rect,
}
```

**Benefits:** O(1) clone, shared memory.

### 2. Command Culling

```rust
impl Canvas {
    fn draw_rect(&mut self, rect: Rect, paint: &Paint) {
        // Cull if outside clip
        if self.would_be_clipped(&rect) {
            return;
        }

        self.display_list.push(DrawCommand::DrawRect { /* ... */ });
    }
}
```

### 3. Command Merging

```rust
// Merge adjacent compatible draws
DrawCommand::DrawRect(r1) + DrawCommand::DrawRect(r2)
    → DrawCommand::DrawRects(vec![r1, r2])
```

### 4. GPU Command Buffers

Direct mapping to GPU command buffers for minimal CPU overhead.

## Best Practices

### For Library Users

1. **Reuse Canvas allocations:**
   ```rust
   let mut canvas = Canvas::new();
   loop {
       canvas.reset();
       render(&mut canvas);
   }
   ```

2. **Cache static content:**
   ```rust
   let icon = Canvas::record(|c| { /* ... */ });
   // Reuse icon across frames
   ```

3. **Use scoped operations:**
   ```rust
   canvas.with_save(|c| {
       // Auto cleanup
   });
   ```

4. **Batch similar operations:**
   ```rust
   canvas.draw_rects(&[rect1, rect2, rect3], &paint);
   ```

### For Contributors

See [CONTRIBUTING.md](../CONTRIBUTING.md) for detailed guidelines.

1. **Maintain immutability of DisplayList**
2. **Keep DrawCommand variants cheap to clone**
3. **Document performance characteristics**
4. **Add tests for edge cases**
5. **Preserve zero-cost abstractions**

## References

- [Command Pattern - Design Patterns](https://refactoring.guru/design-patterns/command)
- [Skia Graphics Library](https://skia.org/) - Inspiration for Canvas API
- [lyon - Path Tessellation](https://github.com/nical/lyon) - Used by flui_engine
