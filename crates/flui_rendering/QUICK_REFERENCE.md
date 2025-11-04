# Render Object Quick Reference

## Minimal Template

```rust
use flui_core::element::{ElementId, ElementTree};
use flui_core::render::SingleRender;
use flui_engine::BoxedLayer;
use flui_types::{Offset, Size, constraints::BoxConstraints};

#[derive(Debug)]
pub struct RenderMyObject {
    // Public config
    pub config: MyConfig,

    // Private cache (if needed)
    cached_size: Size,
}

impl RenderMyObject {
    pub fn new(config: MyConfig) -> Self {
        Self {
            config,
            cached_size: Size::ZERO,
        }
    }

    pub fn set_config(&mut self, config: MyConfig) {
        self.config = config;
    }
}

impl SingleRender for RenderMyObject {
    type Metadata = ();  // ← Don't forget this!

    fn layout(
        &mut self,
        tree: &ElementTree,
        child_id: ElementId,
        constraints: BoxConstraints,
    ) -> Size {
        // 1. Layout child
        let child_size = tree.layout_child(child_id, constraints);

        // 2. Compute our size
        let size = compute_size(child_size);

        // 3. Cache if needed for paint
        self.cached_size = size;

        // 4. Return size
        size
    }

    fn paint(&self, tree: &ElementTree, child_id: ElementId, offset: Offset) -> BoxedLayer {
        // Use cached values from layout
        let size = self.cached_size;

        // Paint child
        let child_layer = tree.paint_child(child_id, offset);

        // Return layer (wrapped or as-is)
        child_layer
    }
}
```

## Common Operations

### Layout Child

```rust
// Pass constraints as-is
let child_size = tree.layout_child(child_id, constraints);

// Loose constraints (child can be smaller)
let child_size = tree.layout_child(child_id, constraints.loosen());

// Tight constraints (child must be exact size)
let tight = BoxConstraints::tight(Size::new(100.0, 100.0));
let child_size = tree.layout_child(child_id, tight);

// Deflate by padding
let deflated = constraints.deflate(&padding);
let child_size = tree.layout_child(child_id, deflated);
```

### Paint Child

```rust
// Paint at offset
tree.paint_child(child_id, offset)

// Paint at local position + parent offset
tree.paint_child(child_id, offset + local_offset)

// Conditional transform
if offset != Offset::ZERO {
    Box::new(TransformLayer::translate(child_layer, offset))
} else {
    child_layer
}
```

### Size Calculation

```rust
// Expand to fill constraints
let size = Size::new(constraints.max_width, constraints.max_height);

// Shrink to child size
let size = child_size;

// Add padding
let size = Size::new(
    child_size.width + padding.horizontal_total(),
    child_size.height + padding.vertical_total(),
);

// Apply factor with clamping
let width = (child_size.width * factor).clamp(constraints.min_width, constraints.max_width);
```

## Three Patterns at a Glance

### 1. Pass-Through (No Cache)
```rust
fn layout(&mut self, tree, child_id, constraints) -> Size {
    tree.layout_child(child_id, modified_constraints)
}

fn paint(&self, tree, child_id, offset) -> BoxedLayer {
    tree.paint_child(child_id, offset + local_offset)
}
```

### 2. Complex Sizing (With Cache)
```rust
fn layout(&mut self, tree, child_id, constraints) -> Size {
    let child_size = tree.layout_child(child_id, constraints);
    let size = compute_my_size(child_size);
    self.cached_size = size;  // Cache!
    size
}

fn paint(&self, tree, child_id, offset) -> BoxedLayer {
    let size = self.cached_size;  // Use cache!
    let position = compute_position(size);
    tree.paint_child(child_id, offset + position)
}
```

### 3. Visual Effect (Layer Wrapper)
```rust
fn layout(&mut self, tree, child_id, constraints) -> Size {
    tree.layout_child(child_id, constraints)  // Pass-through
}

fn paint(&self, tree, child_id, offset) -> BoxedLayer {
    let child_layer = tree.paint_child(child_id, offset);
    Box::new(EffectLayer::new(child_layer, effect_params))  // Wrap!
}
```

## Constraints Cheat Sheet

```rust
// Create constraints
BoxConstraints::new(min_width, max_width, min_height, max_height)
BoxConstraints::tight(size)          // min = max = size
BoxConstraints::loose(size)          // min = 0, max = size
BoxConstraints::expand()             // min = 0, max = infinity

// Modify constraints
constraints.loosen()                  // min → 0
constraints.tighten(size)            // max → size
constraints.deflate(&edge_insets)    // Subtract padding
constraints.constrain(size)          // Clamp size to constraints
```

## Layer Types

```rust
// Transform
TransformLayer::translate(child, offset)
TransformLayer::scale(child, scale_x, scale_y)
TransformLayer::rotate(child, angle)

// Effects
OpacityLayer::new(child, opacity)
ClipRectLayer::new(child, rect)
ClipRRectLayer::new(child, rrect)

// Container
ContainerLayer::new()  // Group layers
```

## Debugging

```rust
// Print during layout
#[cfg(debug_assertions)]
tracing::debug!("Layout: size = {:?}, constraints = {:?}", size, constraints);

// Print during paint
#[cfg(debug_assertions)]
tracing::debug!("Paint: offset = {:?}, child_offset = {:?}", offset, child_offset);

// Add debug name
fn debug_name(&self) -> &'static str {
    "MyRenderObject"
}
```

## Complete Example: RenderAlign

```rust
#[derive(Debug)]
pub struct RenderAlign {
    pub alignment: Alignment,
    child_size: Size,
    size: Size,
}

impl RenderAlign {
    pub fn new(alignment: Alignment) -> Self {
        Self { alignment, child_size: Size::ZERO, size: Size::ZERO }
    }
}

impl SingleRender for RenderAlign {
    type Metadata = ();

    fn layout(&mut self, tree: &ElementTree, child_id: ElementId, constraints: BoxConstraints) -> Size {
        self.child_size = tree.layout_child(child_id, constraints.loosen());
        self.size = Size::new(constraints.max_width, constraints.max_height);
        self.size
    }

    fn paint(&self, tree: &ElementTree, child_id: ElementId, offset: Offset) -> BoxedLayer {
        let available = Size::new(
            self.size.width - self.child_size.width,
            self.size.height - self.child_size.height,
        );
        let aligned_x = (available.width * (self.alignment.x + 1.0)) / 2.0;
        let aligned_y = (available.height * (self.alignment.y + 1.0)) / 2.0;
        tree.paint_child(child_id, offset + Offset::new(aligned_x, aligned_y))
    }
}
```

## Common Mistakes

❌ Missing `type Metadata`
❌ Not caching layout results
❌ Mutating in `paint()` (takes `&self`)
❌ Recomputing layout in paint
❌ Forgetting to apply parent offset

✅ Always add `type Metadata = ()`
✅ Cache layout values for paint
✅ Paint is read-only (`&self`)
✅ Use cached values in paint
✅ Apply offset when painting
