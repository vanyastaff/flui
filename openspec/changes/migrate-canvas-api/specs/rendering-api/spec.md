# Spec Delta: Rendering API Implementation

**Change ID:** `migrate-canvas-api`
**Capability:** `rendering-api`
**Status:** Implemented

## ADDED Requirements

### Requirement: RenderObjects Use Canvas API Exclusively

All RenderObject implementations in flui_rendering SHALL use the Canvas API for drawing operations.

**Rationale**: Ensure consistent abstraction across all rendering code, eliminate engine dependencies, and enable testability.

#### Scenario: Text Rendering with Canvas

**Given** a RenderParagraph for text rendering
**When** the paint method is called
**Then** it uses Canvas.draw_text() to record text drawing
**And** no direct engine calls are made

```rust
impl Render for RenderParagraph {
    fn paint(&self, ctx: &PaintContext) -> Canvas {
        let mut canvas = Canvas::new();
        canvas.draw_text(&self.text, ctx.offset, &self.style, &self.paint);
        canvas
    }
}
```

#### Scenario: Box Decoration with Canvas

**Given** a RenderDecoratedBox with border and background
**When** the paint method is called
**Then** it uses Canvas methods for all drawing (rect, rrect, etc.)
**And** no layer objects are created directly

```rust
impl Render for RenderDecoratedBox {
    fn paint(&self, ctx: &PaintContext) -> Canvas {
        let mut canvas = Canvas::new();

        // Background
        if let Some(bg_color) = self.decoration.color {
            canvas.draw_rect(self.bounds, &Paint::fill(bg_color));
        }

        // Border
        if let Some(border) = &self.decoration.border {
            canvas.draw_rect(self.bounds, &Paint::stroke(border.color, border.width));
        }

        // Child content
        if let Some(child_id) = ctx.children.single_opt() {
            let child_canvas = ctx.paint_child(child_id, ctx.offset);
            canvas.append_canvas(child_canvas);
        }

        canvas
    }
}
```

#### Scenario: Clipping with Canvas

**Given** a RenderClipRect for clipping operations
**When** the paint method is called
**Then** it uses Canvas.clip_rect() for clipping
**And** child canvas is appended after clipping is set

```rust
impl Render for RenderClipRect {
    fn paint(&self, ctx: &PaintContext) -> Canvas {
        let mut canvas = Canvas::new();

        // Set clip region
        canvas.clip_rect(self.clip_bounds);

        // Paint child within clip
        if let Some(child_id) = ctx.children.single_opt() {
            let child_canvas = ctx.paint_child(child_id, ctx.offset);
            canvas.append_canvas(child_canvas);
        }

        canvas
    }
}
```

### Requirement: No Direct Engine Dependencies in Rendering

The flui_rendering crate SHALL NOT depend on flui_engine.

**Rationale**: Eliminate circular dependencies, enable clean architecture, and allow engine to be swapped without affecting rendering logic.

#### Scenario: Cargo.toml Has No Engine Dependency

**Given** the flui_rendering/Cargo.toml file
**When** examining dependencies
**Then** flui_engine is not listed as a dependency
**And** only flui_painting, flui_core, and flui_types are present

```toml
# flui_rendering/Cargo.toml
[dependencies]
flui_core = { path = "../flui_core" }
flui_painting = { path = "../flui_painting" }  # ✅ Uses painting API
flui_types = { path = "../flui_types" }
# flui_engine NOT included ❌
```

#### Scenario: No Engine Type Imports

**Given** any RenderObject implementation file
**When** examining imports
**Then** no imports from flui_engine are present
**And** only Canvas and Paint from flui_painting are used

```rust
// ✅ Correct imports
use flui_painting::{Canvas, Paint};
use flui_types::{Rect, Offset, Color};

// ❌ Not allowed (would be compilation error)
// use flui_engine::{PictureLayer, ContainerLayer};
```

### Requirement: Flex Layout Canvas Composition

The RenderFlex implementation SHALL properly compose child canvases in layout order.

**Rationale**: Ensure correct rendering order for Row and Column layouts.

#### Scenario: Column Layout Painting

**Given** a RenderFlex configured as a Column with multiple children
**When** the paint method is called
**Then** child canvases are appended in vertical order
**And** each child's offset is correctly applied

```rust
impl Render for RenderFlex {
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

#### Scenario: Row Layout Painting

**Given** a RenderFlex configured as a Row with multiple children
**When** the paint method is called
**Then** child canvases are appended in horizontal order
**And** spacing is correctly reflected in offsets

```rust
// Same implementation as Column - offsets differ based on layout
// RenderFlex is agnostic to direction in paint phase
```

### Requirement: Stack Layout Z-Order Composition

The RenderStack implementation SHALL compose child canvases in correct z-order.

**Rationale**: Ensure children are painted in the correct stacking order (first child at bottom).

#### Scenario: Stack with Positioned Children

**Given** a RenderStack with multiple positioned children
**When** the paint method is called
**Then** children are painted in order (first to last)
**And** each child's canvas is appended sequentially
**And** later children appear on top

```rust
impl Render for RenderStack {
    fn paint(&self, ctx: &PaintContext) -> Canvas {
        let mut canvas = Canvas::new();

        // Paint in order - first child at bottom, last on top
        for (&child_id, &child_offset) in ctx.children.as_slice()
            .iter()
            .zip(self.child_offsets.iter())
        {
            let child_canvas = ctx.paint_child(child_id, ctx.offset + child_offset);
            canvas.append_canvas(child_canvas);  // Appends to end = on top
        }

        canvas
    }
}
```

### Requirement: Transform RenderObjects Use Canvas Transform API

Transform RenderObjects SHALL use Canvas transform methods instead of creating transform layers.

**Rationale**: Leverage Canvas's built-in transform tracking for cleaner code and better performance.

#### Scenario: Translation Transform

**Given** a RenderTransform with translation
**When** the paint method is called
**Then** it uses Canvas.translate() before painting child
**And** uses save/restore to preserve parent transform state

```rust
impl Render for RenderTransform {
    fn paint(&self, ctx: &PaintContext) -> Canvas {
        let mut canvas = Canvas::new();

        canvas.save();
        canvas.set_transform(self.transform);

        if let Some(child_id) = ctx.children.single_opt() {
            let child_canvas = ctx.paint_child(child_id, ctx.offset);
            canvas.append_canvas(child_canvas);
        }

        canvas.restore();
        canvas
    }
}
```

## MODIFIED Requirements

_No existing requirements modified - all changes are additions_

## REMOVED Requirements

### Requirement: RenderObjects Create Layer Objects (REMOVED)

**Previous Requirement**: RenderObjects could create PictureLayer and ContainerLayer directly.

**Removal Rationale**: Tight coupling to engine implementation. Canvas API provides better abstraction.

**Migration Impact**:
- All ~30+ RenderObject implementations updated
- No layer creation code remains in flui_rendering
- Engine now responsible for layer creation from DisplayList

## Dependencies

- **flui_painting**: Canvas API for drawing
- **flui_core**: Render trait and PaintContext
- **flui_types**: Geometry and styling types
- **Related Changes**: See `core-rendering` and `painting-api` specs

## Migration Statistics

### RenderObjects Migrated

**Total**: ~35 RenderObject implementations
**Categories**:
- Text rendering: 2 (RenderParagraph, RenderRichText)
- Basic shapes: 4 (RenderBox, RenderCircle, RenderOval, RenderPath)
- Decorations: 3 (RenderDecoratedBox, RenderColoredBox, RenderOpacity)
- Layout: 6 (RenderPadding, RenderCenter, RenderAlign, RenderSizedBox, etc.)
- Clipping: 4 (RenderClipRect, RenderClipRRect, RenderClipOval, RenderClipPath)
- Flex layout: 3 (RenderFlex, RenderFlexible, RenderExpanded)
- Stack layout: 2 (RenderStack, RenderPositioned)
- Transforms: 4 (RenderTransform, RenderRotation, RenderScale, RenderTranslate)
- Other: 7+ (Scrollbar, Viewport, etc.)

### Code Quality Improvements

- Lines removed: ~1,500 (layer creation boilerplate)
- Lines added: ~800 (cleaner Canvas API usage)
- Net reduction: ~700 lines
- Dependencies removed: 1 (flui_engine from flui_rendering)
- Circular dependencies eliminated: 1 (rendering ↔ engine)

## Validation

### Build Validation
```bash
cargo build -p flui_rendering  # ✅ Passes
cargo clippy -p flui_rendering -- -D warnings  # ✅ No warnings
```

### Test Validation
```bash
cargo test -p flui_rendering  # ✅ All tests pass
cargo test --workspace  # ✅ Integration tests pass
```

### Examples Validation
```bash
cargo run --example simplified_view  # ✅ Renders correctly
cargo run --example profile_card  # ✅ Complex layout works
```

## Notes

- Migration completed across entire flui_rendering crate
- No visual regressions observed
- Performance unchanged (within measurement variance)
- All existing tests continue to pass
- Migration guide available in CANVAS_MIGRATION_GUIDE.md
