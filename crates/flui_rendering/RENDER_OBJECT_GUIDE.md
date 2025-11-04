# Render Object Implementation Guide

## Overview

This guide shows how to implement render objects in FLUI's new architecture. Use this as a reference when creating or updating render objects.

## Architecture Summary

### Key Traits

1. **`SingleRender`** - For render objects with exactly one child
2. **`MultiRender`** - For render objects with multiple children
3. **`LeafRender`** - For render objects with no children (primitives)

### Core Concepts

- **Layout Phase**: Compute sizes and positions, returns `Size`
- **Paint Phase**: Create layer tree for rendering, returns `BoxedLayer`
- **Metadata**: Optional associated data (use `type Metadata = ()` if not needed)

---

## Pattern 1: Simple Single-Child Render Object

**Example: `RenderPadding`**

```rust
use flui_core::element::{ElementId, ElementTree};
use flui_core::render::SingleRender;
use flui_engine::BoxedLayer;
use flui_types::{EdgeInsets, Offset, Size, constraints::BoxConstraints};

/// RenderObject that adds padding around its child
#[derive(Debug, Clone)]
pub struct RenderPadding {
    /// The padding to apply
    pub padding: EdgeInsets,
}

impl RenderPadding {
    pub fn new(padding: EdgeInsets) -> Self {
        Self { padding }
    }

    pub fn set_padding(&mut self, padding: EdgeInsets) {
        self.padding = padding;
    }
}

impl SingleRender for RenderPadding {
    /// No metadata needed for simple padding
    type Metadata = ();

    fn layout(
        &mut self,
        tree: &ElementTree,
        child_id: ElementId,
        constraints: BoxConstraints,
    ) -> Size {
        let padding = self.padding;

        // Step 1: Deflate constraints by padding
        let child_constraints = constraints.deflate(&padding);

        // Step 2: Layout child with deflated constraints
        let child_size = tree.layout_child(child_id, child_constraints);

        // Step 3: Add padding to child size for our size
        Size::new(
            child_size.width + padding.horizontal_total(),
            child_size.height + padding.vertical_total(),
        )
    }

    fn paint(&self, tree: &ElementTree, child_id: ElementId, offset: Offset) -> BoxedLayer {
        // Apply padding offset and paint child
        let child_offset = Offset::new(self.padding.left, self.padding.top);
        tree.paint_child(child_id, offset + child_offset)
    }
}
```

### Key Points:
- ✅ **`type Metadata = ()`** - No metadata needed
- ✅ **Layout deflates constraints** - Child gets reduced space
- ✅ **Paint applies offset** - Child painted at padded position
- ✅ **Simple and stateless** - No caching needed

---

## Pattern 2: Single-Child with Layout Caching

**Example: `RenderAlign`**

```rust
use flui_core::element::{ElementId, ElementTree};
use flui_core::render::SingleRender;
use flui_engine::BoxedLayer;
use flui_types::{Alignment, Offset, Size, constraints::BoxConstraints};

/// RenderObject that aligns its child within available space
#[derive(Debug)]
pub struct RenderAlign {
    /// Alignment specification
    pub alignment: Alignment,
    pub width_factor: Option<f32>,
    pub height_factor: Option<f32>,

    // Cached values from layout for paint phase
    child_size: Size,
    size: Size,
}

impl RenderAlign {
    pub fn new(alignment: Alignment) -> Self {
        Self {
            alignment,
            width_factor: None,
            height_factor: None,
            child_size: Size::ZERO,
            size: Size::ZERO,
        }
    }

    pub fn with_factors(
        alignment: Alignment,
        width_factor: Option<f32>,
        height_factor: Option<f32>,
    ) -> Self {
        Self {
            alignment,
            width_factor,
            height_factor,
            child_size: Size::ZERO,
            size: Size::ZERO,
        }
    }
}

impl SingleRender for RenderAlign {
    /// No metadata needed
    type Metadata = ();

    fn layout(
        &mut self,
        tree: &ElementTree,
        child_id: ElementId,
        constraints: BoxConstraints,
    ) -> Size {
        // Step 1: Layout child with loose constraints
        let child_size = tree.layout_child(child_id, constraints.loosen());

        // Step 2: Cache child size for paint
        self.child_size = child_size;

        // Step 3: Calculate our size based on factors
        let width = if let Some(factor) = self.width_factor {
            (child_size.width * factor).clamp(constraints.min_width, constraints.max_width)
        } else {
            constraints.max_width  // Expand to fill
        };

        let height = if let Some(factor) = self.height_factor {
            (child_size.height * factor).clamp(constraints.min_height, constraints.max_height)
        } else {
            constraints.max_height  // Expand to fill
        };

        let size = Size::new(width, height);

        // Step 4: Cache our size for paint
        self.size = size;
        size
    }

    fn paint(&self, tree: &ElementTree, child_id: ElementId, offset: Offset) -> BoxedLayer {
        // Use cached values from layout
        let size = self.size;
        let child_size = self.child_size;

        // Calculate aligned position in local coordinates
        let available_width = size.width - child_size.width;
        let available_height = size.height - child_size.height;

        // Convert normalized alignment (-1.0 to 1.0) to pixel offset
        let aligned_x = (available_width * (self.alignment.x + 1.0)) / 2.0;
        let aligned_y = (available_height * (self.alignment.y + 1.0)) / 2.0;

        let local_child_offset = Offset::new(aligned_x, aligned_y);

        // Paint child at aligned position
        let child_layer = tree.paint_child(child_id, local_child_offset);

        // Apply parent offset
        if offset != Offset::ZERO {
            Box::new(flui_engine::TransformLayer::translate(child_layer, offset))
        } else {
            child_layer
        }
    }
}
```

### Key Points:
- ✅ **Caches layout results** - `child_size` and `size` stored for paint
- ✅ **Loose constraints** - Child can be smaller than max
- ✅ **Alignment calculation** - Converts normalized coords to pixels
- ✅ **TransformLayer for offset** - Applies parent offset when non-zero

---

## Pattern 3: Effect/Wrapper Render Object

**Example: `RenderOpacity`** (conceptual)

```rust
use flui_core::element::{ElementId, ElementTree};
use flui_core::render::SingleRender;
use flui_engine::{BoxedLayer, OpacityLayer};
use flui_types::{Offset, Size, constraints::BoxConstraints};

/// RenderObject that applies opacity to its child
#[derive(Debug, Clone)]
pub struct RenderOpacity {
    pub opacity: f32,
}

impl RenderOpacity {
    pub fn new(opacity: f32) -> Self {
        Self { opacity: opacity.clamp(0.0, 1.0) }
    }

    pub fn set_opacity(&mut self, opacity: f32) {
        self.opacity = opacity.clamp(0.0, 1.0);
    }
}

impl SingleRender for RenderOpacity {
    type Metadata = ();

    fn layout(
        &mut self,
        tree: &ElementTree,
        child_id: ElementId,
        constraints: BoxConstraints,
    ) -> Size {
        // Pass-through: child determines our size
        tree.layout_child(child_id, constraints)
    }

    fn paint(&self, tree: &ElementTree, child_id: ElementId, offset: Offset) -> BoxedLayer {
        // Paint child
        let child_layer = tree.paint_child(child_id, offset);

        // Wrap in opacity layer
        Box::new(OpacityLayer::new(child_layer, self.opacity))
    }
}
```

### Key Points:
- ✅ **Pass-through layout** - Child size becomes our size
- ✅ **Wraps child layer** - Applies visual effect in paint
- ✅ **Stateless** - No caching needed

---

## Common Patterns

### 1. Constraint Manipulation

```rust
// Tight constraints - child must be exact size
let tight = BoxConstraints::tight(Size::new(100.0, 100.0));

// Loose constraints - child can be smaller
let loose = constraints.loosen();

// Deflate by padding
let deflated = constraints.deflate(&padding);

// Tighten to specific size
let tightened = constraints.tighten(Size::new(200.0, 200.0));
```

### 2. Offset Handling

```rust
// Simple offset addition
tree.paint_child(child_id, offset + local_offset)

// Conditional transform layer
if offset != Offset::ZERO {
    Box::new(TransformLayer::translate(child_layer, offset))
} else {
    child_layer
}
```

### 3. Layout Caching Pattern

```rust
#[derive(Debug)]
pub struct MyRender {
    // Configuration (public)
    pub some_config: f32,

    // Cached layout values (private)
    cached_size: Size,
    cached_child_data: Vec<ChildData>,
}

impl SingleRender for MyRender {
    fn layout(&mut self, ...) -> Size {
        // Compute layout
        let size = compute_size();

        // Cache for paint
        self.cached_size = size;

        size
    }

    fn paint(&self, ...) -> BoxedLayer {
        // Use cached values
        let size = self.cached_size;
        // ...
    }
}
```

---

## Testing Pattern

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new() {
        let render = RenderMyObject::new(config);
        assert_eq!(render.config, expected);
    }

    #[test]
    fn test_setters() {
        let mut render = RenderMyObject::new(initial);
        render.set_config(new_value);
        assert_eq!(render.config, new_value);
    }

    // Integration tests with real ElementTree should be in flui_core
}
```

---

## Checklist for New Render Objects

- [ ] Add `type Metadata = ()` (or custom type if needed)
- [ ] Implement `layout()` - compute sizes, cache if needed
- [ ] Implement `paint()` - create layer tree
- [ ] Add constructor `new()` and any `with_*()` variants
- [ ] Add setters for mutable properties
- [ ] Add `#[derive(Debug)]`
- [ ] Document behavior in doc comments
- [ ] Add examples in doc comments
- [ ] Add basic unit tests for constructors/setters
- [ ] Consider implementing `Default` if sensible

---

## Common Mistakes to Avoid

❌ **Forgetting `type Metadata = ()`**
```rust
impl SingleRender for MyRender {
    // Missing: type Metadata = ();
    fn layout(...) -> Size { ... }
}
```

❌ **Not caching layout results for paint**
```rust
fn layout(&mut self, ...) -> Size {
    let size = compute_complex_layout();
    size  // ❌ Lost! Need to cache
}

fn paint(&self, ...) -> BoxedLayer {
    let size = compute_complex_layout();  // ❌ Recomputing!
}
```

✅ **Correct caching pattern**
```rust
fn layout(&mut self, ...) -> Size {
    let size = compute_complex_layout();
    self.cached_size = size;  // ✅ Cache it
    size
}

fn paint(&self, ...) -> BoxedLayer {
    let size = self.cached_size;  // ✅ Use cached value
}
```

❌ **Mutating in paint()**
```rust
fn paint(&self, ...) -> BoxedLayer {
    self.counter += 1;  // ❌ Paint takes &self, not &mut self
}
```

---

## See Also

- **SingleRender trait**: `crates/flui_core/src/render/render_traits.rs`
- **Example objects**: `crates/flui_rendering/src/objects/layout/`
- **ElementTree API**: `crates/flui_core/src/element/element_tree.rs`
- **Constraints**: `crates/flui_types/src/constraints.rs`

---

## Summary

### Three Main Patterns:

1. **Simple Pass-Through** (`RenderPadding`)
   - Modify constraints → layout child → return modified size
   - No caching needed

2. **Complex Sizing** (`RenderAlign`)
   - Layout child → compute our size → cache both for paint
   - Need caching for alignment calculations

3. **Visual Effects** (`RenderOpacity`)
   - Pass-through layout → wrap child layer in effect layer
   - No caching needed

**Golden Rule**: Layout computes and caches, paint uses cached values and creates layers.
