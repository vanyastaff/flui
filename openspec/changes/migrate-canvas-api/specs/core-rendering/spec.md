# Spec Delta: Core Rendering API

**Change ID:** `migrate-canvas-api`
**Capability:** `core-rendering`
**Status:** Implemented

## ADDED Requirements

_No new requirements - only modifications to existing API_

## MODIFIED Requirements

### Requirement: Render Trait Paint Method Signature

The Render trait's paint method SHALL return a Canvas instead of BoxedLayer.

**Rationale**: Decouple RenderObjects from engine-specific layer types, enabling backend-agnostic rendering and better testability.

#### Scenario: Paint Method Returns Canvas

**Given** a RenderObject implementing the Render trait
**When** the `paint()` method is called
**Then** it returns a Canvas instance (not BoxedLayer)
**And** the Canvas contains recorded drawing commands

```rust
// Old signature (removed)
// fn paint(&self, ctx: &PaintContext) -> BoxedLayer

// New signature (implemented)
fn paint(&self, ctx: &PaintContext) -> Canvas {
    let mut canvas = Canvas::new();
    canvas.draw_rect(self.bounds, &self.paint);
    canvas
}
```

#### Scenario: Leaf Render Implementation

**Given** a RenderObject with no children (leaf render)
**When** implementing the paint method
**Then** the implementation creates a Canvas
**And** records drawing commands directly
**And** returns the Canvas without calling finish()

```rust
impl Render for RenderBox {
    fn paint(&self, ctx: &PaintContext) -> Canvas {
        let mut canvas = Canvas::new();
        canvas.draw_rect(self.bounds, &self.paint);
        canvas  // Framework handles finish()
    }
}
```

#### Scenario: Single Child Render Implementation

**Given** a RenderObject with exactly one child
**When** implementing the paint method
**Then** the implementation creates a parent Canvas
**And** paints parent content first
**And** obtains child Canvas via `ctx.paint_child()`
**And** appends child Canvas using `append_canvas()`
**And** returns the composed Canvas

```rust
impl Render for RenderPadding {
    fn paint(&self, ctx: &PaintContext) -> Canvas {
        let child_id = ctx.children.single();
        let child_offset = Offset::new(self.padding.left, self.padding.top);

        let mut canvas = Canvas::new();
        // Parent content (if any)
        canvas.draw_rect(debug_background, &debug_paint);

        // Child content
        let child_canvas = ctx.paint_child(child_id, ctx.offset + child_offset);
        canvas.append_canvas(child_canvas);

        canvas
    }
}
```

#### Scenario: Multi Child Render Implementation

**Given** a RenderObject with multiple children
**When** implementing the paint method
**Then** the implementation creates a parent Canvas
**And** iterates through children
**And** appends each child Canvas in order
**And** returns the composed Canvas

```rust
impl Render for RenderColumn {
    fn paint(&self, ctx: &PaintContext) -> Canvas {
        let mut canvas = Canvas::new();

        for (&child_id, &child_offset) in ctx.children.as_slice()
            .iter()
            .zip(self.child_offsets.iter())
        {
            let child_canvas = ctx.paint_child(child_id, ctx.offset + child_offset);
            canvas.append_canvas(child_canvas);
        }

        canvas
    }
}
```

### Requirement: PaintContext Returns Canvas from Children

The PaintContext SHALL return Canvas from child painting operations instead of BoxedLayer.

**Rationale**: Maintain consistency with the Render trait API change and enable canvas composition.

#### Scenario: paint_child Returns Canvas

**Given** a PaintContext with child elements
**When** `paint_child(child_id, offset)` is called
**Then** it returns a Canvas (not BoxedLayer)
**And** the Canvas contains the child's drawing commands
**And** the Canvas can be composed with the parent Canvas

```rust
let ctx = PaintContext::new(&tree, &children, offset);
let child_canvas = ctx.paint_child(child_id, child_offset);
// child_canvas is Canvas, not BoxedLayer

parent_canvas.append_canvas(child_canvas);
```

#### Scenario: paint_all_children Returns Canvas Vector

**Given** a PaintContext with multiple children
**When** `paint_all_children(offsets)` is called
**Then** it returns a `Vec<Canvas>` (not `Vec<BoxedLayer>`)
**And** each Canvas corresponds to one child
**And** order matches the children order

```rust
let ctx = PaintContext::new(&tree, &children, offset);
let child_canvases: Vec<Canvas> = ctx.paint_all_children(&offsets);

for child_canvas in child_canvases {
    parent_canvas.append_canvas(child_canvas);
}
```

## REMOVED Requirements

### Requirement: Render Trait Returns BoxedLayer (REMOVED)

**Previous Requirement**: The Render trait's paint method returned `Box<dyn Layer>`.

**Removal Rationale**: Tight coupling to engine layer types. Replaced with Canvas-based approach for better abstraction.

**Migration Path**:
- All RenderObjects updated to return Canvas
- PaintContext methods updated
- Layer creation moved to engine

### Requirement: Direct Layer Creation in RenderObjects (REMOVED)

**Previous Requirement**: RenderObjects could directly create PictureLayer and ContainerLayer from flui_engine.

**Removal Rationale**: Created circular dependency and mixed abstraction levels. Canvas API provides better separation.

**Migration Path**:
- Removed flui_engine dependency from flui_rendering
- RenderObjects now use Canvas API exclusively
- Engine creates layers from DisplayList

## Dependencies

- **flui_painting**: Provides Canvas and DisplayList types
- **Related Changes**: See `painting-api` spec for Canvas API details
- **Breaking Change**: Yes - all RenderObject implementations must be updated

## Migration Guide

### For Leaf Renders
```rust
// Before
fn paint(&self, ctx: &PaintContext) -> BoxedLayer {
    let mut picture = PictureLayer::new();
    picture.draw_rect(rect, paint);
    Box::new(picture)
}

// After
fn paint(&self, ctx: &PaintContext) -> Canvas {
    let mut canvas = Canvas::new();
    canvas.draw_rect(rect, &paint);  // Note: &paint
    canvas
}
```

### For Single Child Renders
```rust
// Before
fn paint(&self, ctx: &PaintContext) -> BoxedLayer {
    let child_layer = ctx.paint_child(child_id, offset);
    let mut container = ContainerLayer::new();
    container.add_child(child_layer);
    Box::new(container)
}

// After
fn paint(&self, ctx: &PaintContext) -> Canvas {
    let mut canvas = Canvas::new();
    let child_canvas = ctx.paint_child(child_id, offset);
    canvas.append_canvas(child_canvas);
    canvas
}
```

### For Multi Child Renders
```rust
// Before
fn paint(&self, ctx: &PaintContext) -> BoxedLayer {
    let mut container = ContainerLayer::new();
    for &child_id in ctx.children.as_slice() {
        container.add_child(ctx.paint_child(child_id, offset));
    }
    Box::new(container)
}

// After
fn paint(&self, ctx: &PaintContext) -> Canvas {
    let mut canvas = Canvas::new();
    for &child_id in ctx.children.as_slice() {
        let child_canvas = ctx.paint_child(child_id, offset);
        canvas.append_canvas(child_canvas);
    }
    canvas
}
```

## Notes

- Paint parameter changed from `Paint` to `&Paint` in Canvas methods
- No need to call `canvas.finish()` - framework handles it
- Transform state is automatically tracked by Canvas
- Clipping operations available via Canvas methods
- See CANVAS_MIGRATION_GUIDE.md for complete patterns
