# Canvas Migration Guide for RenderObjects

This document provides patterns for migrating RenderObjects from returning `BoxedLayer` to returning `Canvas`.

## Overview

The refactoring changes `Render::paint()` signature from:
```rust
fn paint(&self, ctx: &PaintContext) -> BoxedLayer
```

To:
```rust
fn paint(&self, ctx: &PaintContext) -> Canvas
```

## Three Main Patterns

### Pattern 1: Leaf Render (No Children)

**Before:**
```rust
fn paint(&self, ctx: &PaintContext) -> BoxedLayer {
    let mut picture = PictureLayer::new();
    picture.draw_rect(rect, paint);
    Box::new(picture)
}
```

**After:**
```rust
fn paint(&self, ctx: &PaintContext) -> Canvas {
    let mut canvas = Canvas::new();
    canvas.draw_rect(rect, &paint);  // Note: paint is &Paint
    canvas
}
```

**Changes:**
1. Create `Canvas` instead of `PictureLayer`
2. Use Canvas draw methods (same API, but paint is `&Paint`)
3. Return canvas directly (no Box)

### Pattern 2: Single Child Render

**Before:**
```rust
fn paint(&self, ctx: &PaintContext) -> BoxedLayer {
    let mut picture = PictureLayer::new();
    picture.draw_rect(bg_rect, bg_paint);

    let child_layer = ctx.tree.paint_child(child_id, offset);

    let mut container = pool::acquire_container();
    container.add_child(Box::new(picture));
    container.add_child(child_layer);
    Box::new(container)
}
```

**After:**
```rust
fn paint(&self, ctx: &PaintContext) -> Canvas {
    let mut canvas = Canvas::new();
    canvas.draw_rect(bg_rect, &bg_paint);

    let child_canvas = ctx.tree.paint_child(child_id, offset);
    canvas.append_canvas(child_canvas);

    canvas
}
```

**Changes:**
1. Create `Canvas` for parent
2. Draw parent content
3. Get child canvas from `paint_child()` (now returns Canvas)
4. Use `canvas.append_canvas(child_canvas)` to compose
5. Return parent canvas

### Pattern 3: Multi-Child Render

**Before:**
```rust
fn paint(&self, ctx: &PaintContext) -> BoxedLayer {
    let mut container = pool::acquire_container();

    for &child_id in ctx.children.as_slice() {
        let child_layer = ctx.tree.paint_child(child_id, offset);
        container.add_child(child_layer);
    }

    Box::new(container)
}
```

**After:**
```rust
fn paint(&self, ctx: &PaintContext) -> Canvas {
    let mut canvas = Canvas::new();

    for &child_id in ctx.children.as_slice() {
        let child_canvas = ctx.tree.paint_child(child_id, offset);
        canvas.append_canvas(child_canvas);
    }

    canvas
}
```

**Changes:**
1. Create `Canvas` for parent
2. Loop through children, get each child canvas
3. Append each child canvas to parent
4. Return parent canvas

## Import Changes

**Remove:**
```rust
use flui_engine::{BoxedLayer, PictureLayer, ContainerLayer, layer::pool};
```

**Keep (if needed):**
```rust
use flui_painting::{Canvas, Paint};
```

## Common Gotchas

1. **Paint reference:** Canvas methods take `&Paint`, not `Paint`
   - Before: `picture.draw_rect(rect, paint)`
   - After: `canvas.draw_rect(rect, &paint)`

2. **No `finish()`:** Don't call `canvas.finish()` - just return the canvas

3. **No Box:** Return `Canvas` directly, not `Box<Canvas>`

4. **Offset handling:** Canvas handles offsets via transform, but for compatibility,
   you may still pass offset to child paint calls

## Files to Update

Run this to find all files needing updates:
```bash
grep -r "fn paint.*BoxedLayer" crates/flui_rendering/src/
```

## Verification

After updating a file:
```bash
cargo check -p flui_rendering
```

## Examples

See these updated files for reference:
- `crates/flui_rendering/src/objects/text/paragraph.rs` - Leaf render
- `crates/flui_rendering/src/objects/special/colored_box.rs` - Single child render
