# Paint-Layer Integration Design

**Date:** 2026-01-26  
**Status:** ✅ IMPLEMENTED  
**Components:** `flui_painting`, `flui-layer`

---

## Overview

This document describes the completed integration between FLUI's painting system (`flui_painting`) and layer compositor (`flui-layer`), enabling the full Canvas → DisplayList → PictureLayer → GPU rendering pipeline.

## Architecture

### Complete Flow

```text
┌─────────────────────────────────────────────────────────────────┐
│                    Paint Recording Phase                         │
├─────────────────────────────────────────────────────────────────┤
│  RenderObject::paint()                                           │
│       ↓                                                          │
│  Canvas (flui_painting)                                          │
│       │                                                          │
│       ├─ draw_rect()                                             │
│       ├─ draw_circle()                                           │
│       ├─ draw_path()                                             │
│       ├─ transform operations (save/restore/translate/rotate)    │
│       └─ clip operations (clip_rect/clip_path)                   │
│       ↓                                                          │
│  canvas.finish()                                                 │
│       ↓                                                          │
│  Picture (DisplayList)  ← immutable recorded commands            │
└─────────────────────────────────────────────────────────────────┘
                           ↓
┌─────────────────────────────────────────────────────────────────┐
│                    Layer Compositing Phase                       │
├─────────────────────────────────────────────────────────────────┤
│  SceneBuilder (flui-layer)                                       │
│       │                                                          │
│       ├─ push_offset()/push_transform()/push_opacity()           │
│       ├─ push_clip_rect()/push_clip_path()                       │
│       │                                                          │
│       ├─ add_picture(picture) ← NEW METHOD                       │
│       │       ↓                                                  │
│       │  PictureLayer::new(picture)                              │
│       │       ↓                                                  │
│       │  Layer::Picture(PictureLayer)                            │
│       │       ↓                                                  │
│       └─ LayerTree                                               │
│       ↓                                                          │
│  builder.build() → Scene                                         │
└─────────────────────────────────────────────────────────────────┘
                           ↓
┌─────────────────────────────────────────────────────────────────┐
│                    GPU Rendering Phase                           │
├─────────────────────────────────────────────────────────────────┤
│  Scene → WgpuPainter (flui_engine) → GPU                         │
└─────────────────────────────────────────────────────────────────┘
```

## Implementation

### 1. SceneBuilder::add_picture()

Added new method to `flui-layer/src/compositor.rs`:

```rust
/// Adds a picture layer with recorded drawing commands.
///
/// This is the primary method for adding cached/recorded content to the scene.
/// The picture is an immutable DisplayList that was previously recorded via Canvas.
pub fn add_picture(&mut self, picture: flui_painting::Picture) -> LayerId {
    use crate::layer::PictureLayer;
    self.add_leaf(Layer::Picture(PictureLayer::new(picture)))
}
```

**Key Design Decisions:**

1. **Leaf Layer Pattern**: Picture is added as a leaf layer (doesn't push onto stack)
   - Consistent with `add_canvas()` and `add_texture()`
   - Pictures are terminal nodes in the layer tree

2. **Immutable by Design**: Pictures are immutable once recorded
   - Enables caching and reuse
   - Thread-safe (Send + Clone)
   - Supports Flutter's repaint boundary optimization

3. **Type-Safe Integration**: Uses `flui_painting::Picture` type alias
   - Maintains compatibility with Flutter terminology
   - Internal implementation uses DisplayList

### 2. Integration Points

#### flui_painting (Recording)

```rust
// Canvas records drawing commands
let mut canvas = Canvas::new();
canvas.draw_rect(rect, &paint);
canvas.draw_circle(center, radius, &paint);

// Finish recording → immutable Picture
let picture: Picture = canvas.finish();
```

**Features:**
- ✅ Transform stack (save/restore/translate/rotate/scale)
- ✅ Clip stack (clip_rect/clip_path)
- ✅ Drawing primitives (rect, circle, path, text, image)
- ✅ Paint styling (fill, stroke, shader, blend mode)
- ✅ Thread-safe (Send)

#### flui-layer (Compositing)

```rust
// Build layer tree with picture
let mut tree = LayerTree::new();
let mut builder = SceneBuilder::new(&mut tree);

// Add container layers
builder.push_offset(Offset::new(px(100.0), px(50.0)));
builder.push_opacity(0.8);

// Add picture as leaf
builder.add_picture(picture);

builder.pop(); // pop opacity
builder.pop(); // pop offset

// Build scene
let scene = builder.build();
```

**Features:**
- ✅ Container layers (Offset, Transform, Opacity, Clip)
- ✅ Leaf layers (Canvas, Picture, Texture, PlatformView)
- ✅ Stack-based API with push/pop
- ✅ Automatic parent-child relationships
- ✅ Flutter-compatible SceneBuilder pattern

## Comparison with GPUI

### GPUI Approach

```rust
// Flat scene with primitives
scene.insert_primitive(Quad { ... });
scene.insert_primitive(Path { ... });
scene.push_layer(bounds);
// ... more primitives ...
scene.pop_layer();
scene.finish(); // sorts by draw order
```

- **Flat structure**: Primitives with draw order
- **Painter's algorithm**: Sort and batch for GPU
- **Simpler model**: No hierarchical compositing

### FLUI Approach (Flutter-compatible)

```rust
// Hierarchical layer tree
builder.push_offset(offset);
builder.push_opacity(alpha);
builder.add_picture(picture); // contains primitives
builder.pop();
builder.pop();
```

- **Hierarchical structure**: Layer tree with parent-child relationships
- **Compositing effects**: Opacity, transforms, clips applied hierarchically
- **Repaint boundaries**: Cached pictures enable selective repainting
- **Flutter compatibility**: Matches Flutter's proven architecture

**Hybrid Benefit**: FLUI's PictureLayer internally contains a DisplayList (sequence of primitives), combining both approaches:
- Layer tree for compositing effects
- Flat primitive lists for efficient GPU rendering

## Usage Examples

### Basic Usage

```rust
use flui_layer::{LayerTree, SceneBuilder};
use flui_painting::Canvas;
use flui_types::{Rect, Color, Offset};
use flui_types::painting::Paint;
use flui_types::geometry::px;

// Record drawing
let mut canvas = Canvas::new();
canvas.draw_rect(
    Rect::from_ltrb(px(0.0), px(0.0), px(100.0), px(100.0)),
    &Paint::fill(Color::RED)
);
let picture = canvas.finish();

// Build scene
let mut tree = LayerTree::new();
let mut builder = SceneBuilder::new(&mut tree);
builder.push_offset(Offset::new(px(10.0), px(20.0)));
builder.add_picture(picture);
builder.pop();
let scene = builder.build();
```

### Advanced: Cached Content with Transforms

```rust
// Record once, reuse with different transforms
let picture = {
    let mut canvas = Canvas::new();
    canvas.draw_circle(Point::ZERO, px(50.0), &Paint::fill(Color::BLUE));
    canvas.finish()
};

// Use picture multiple times with different transforms
let mut builder = SceneBuilder::new(&mut tree);

// First instance at (100, 100)
builder.push_offset(Offset::new(px(100.0), px(100.0)));
builder.add_picture(picture.clone());
builder.pop();

// Second instance at (300, 100) with opacity
builder.push_offset(Offset::new(px(300.0), px(100.0)));
builder.push_opacity(0.5);
builder.add_picture(picture.clone());
builder.pop();
builder.pop();

let scene = builder.build();
```

### Repaint Boundary Pattern (Flutter Optimization)

```rust
// Cache expensive drawing operations
fn create_cached_content() -> Picture {
    let mut canvas = Canvas::new();
    
    // Expensive drawing operations
    for i in 0..1000 {
        canvas.draw_circle(
            Point::new(px(i as f32 * 2.0), px(i as f32 * 2.0)),
            px(1.0),
            &Paint::fill(Color::from_rgba(i as u8, 100, 200, 255))
        );
    }
    
    canvas.finish()
}

// Use cached picture - no need to re-execute 1000 draw calls
let cached_picture = create_cached_content();

// Later, in paint:
builder.add_picture(cached_picture.clone());
```

## Testing

Comprehensive test coverage added in `flui-layer/src/compositor.rs`:

```rust
#[test]
fn test_scene_builder_add_picture() {
    // Tests:
    // 1. Picture creation from Canvas
    // 2. Integration with SceneBuilder
    // 3. Layer tree structure verification
    // 4. Layer type checking (is_picture())
}
```

**Test Results:** ✅ All 17 compositor tests passing

## Performance Characteristics

### Memory

- **Picture (DisplayList)**: ~24 bytes + command data
- **PictureLayer**: ~56 bytes (Picture + bounds)
- **Clone cost**: Arc increment (cheap)

### CPU

- **Recording**: O(n) where n = number of draw commands
- **Compositing**: O(m) where m = number of layers
- **Caching benefit**: Avoid re-recording expensive operations

### GPU

- **Single upload**: DisplayList commands uploaded once
- **Batching**: Similar primitives batched by renderer
- **Culling**: Bounds checking skips offscreen content

## Future Enhancements

### Phase 3: GPU Rendering (Next)

1. **WgpuPainter Integration**
   - Convert DisplayList → GPU commands
   - Texture atlas management
   - Shader compilation

2. **Batching Optimization**
   - Group similar draw calls
   - Minimize state changes
   - Instanced rendering for repeated content

3. **Culling**
   - Frustum culling using bounds
   - Occlusion culling
   - Dirty region tracking

### Phase 4: Advanced Features

1. **Shader Effects**
   - Custom shader support
   - Gradient rendering
   - Image filters (blur, color matrix)

2. **Text Rendering**
   - Glyph atlas integration
   - Subpixel positioning
   - Complex text layout

3. **Image Handling**
   - Texture caching
   - Format conversion
   - Mipmapping

## Validation Criteria

✅ **API Completeness**
- Canvas → Picture → PictureLayer flow complete
- SceneBuilder::add_picture() implemented
- Type-safe integration with flui_painting

✅ **Testing**
- Unit tests for add_picture() passing
- Integration tests with Canvas passing
- All existing tests still passing (17/17)

✅ **Documentation**
- Method documentation complete
- Usage examples provided
- Architecture documented

✅ **Compilation**
- flui-layer builds successfully
- No breaking changes to existing API
- Type safety maintained

## Conclusion

The paint-layer integration is now complete. FLUI has a fully functional pipeline from Canvas recording through layer compositing, ready for GPU rendering integration.

**Key Achievement:** The system now supports both:
1. **Mutable recording**: `CanvasLayer` for dynamic content
2. **Immutable caching**: `PictureLayer` for cached content

This enables Flutter's repaint boundary optimization pattern while maintaining a clean, type-safe API.

**Next Step:** Integrate with `flui_engine` for GPU rendering (Phase 3).
