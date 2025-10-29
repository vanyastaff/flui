# FLUI Core - Typed Rendering Architecture

**Clean, typed implementation based on idea.md**

## Status: 🚧 In Progress

This is a complete rewrite of `flui_core` with zero compromises. The old implementation
has been moved to `flui_core_old`.

## Architecture

### Three-Tree Pattern (idea.md Chapter 1)

```
Widget Tree (immutable configuration)
    ↓
Element Tree (living instances)
    ↓
Render Tree (layout & paint)
```

### Typed Arity System (idea.md Chapter 2)

```rust
// Compile-time child count constraints
pub trait RenderArity {
    const CHILD_COUNT: Option<usize>;
}

pub struct LeafArity;    // No children
pub struct SingleArity;  // One child
pub struct MultiArity;   // Multiple children
```

### Render with Arity (idea.md Chapter 2.3)

```rust
pub trait Render: Send + Sync + 'static {
    type Arity: RenderArity;

    fn layout(&mut self, cx: &mut LayoutCx<Self::Arity>) -> Size;
    fn paint(&self, cx: &PaintCx<Self::Arity>) -> BoxedLayer;
}
```

### Typed Contexts (idea.md Chapter 3)

```rust
// Different types for different arities!
LayoutCx<LeafArity>    // NO .child() or .children()
LayoutCx<SingleArity>  // YES .child(), NO .children()
LayoutCx<MultiArity>   // NO .child(), YES .children()
```

### Widget → Render Link (idea.md Chapter 4)

```rust
pub trait RenderWidget: Widget {
    type Render: Render;

    fn create_render_object(&self) -> Self::Render;
    fn update_render_object(&self, render: &mut Self::Render);
}
```

## Implementation Progress

### ✅ Completed

- [x] Project structure
- [x] RenderArity traits (LeafArity, SingleArity, MultiArity)
- [x] Typed Render trait with Layer return
- [x] Widget and RenderWidget traits
- [x] ElementId and ElementTree (stub)

### 🚧 In Progress

- [ ] LayoutCx<A: RenderArity> with specialized methods
- [ ] PaintCx<A: RenderArity> with specialized methods
- [ ] LayoutCache integration

### 📋 TODO

- [ ] Full ElementTree implementation
- [ ] Example Renders (RenderParagraph, RenderOpacity, RenderFlex)
- [ ] RenderPipeline integration
- [ ] Compositor integration
- [ ] Demo examples

## Key Benefits

### 1. Compile-Time Safety

```rust
// ✅ This compiles - SingleArity has .child()
impl Render for RenderOpacity {
    type Arity = SingleArity;

    fn layout(&mut self, cx: &mut LayoutCx<Self::Arity>) -> Size {
        let child = cx.child(); // ✅ Method exists!
        cx.layout_child(child, cx.constraints())
    }
}

// ❌ This doesn't compile - LeafArity has no .child()
impl Render for RenderParagraph {
    type Arity = LeafArity;

    fn layout(&mut self, cx: &mut LayoutCx<Self::Arity>) -> Size {
        let child = cx.child(); // ❌ Compile error: method not found!
        // ...
    }
}
```

### 2. Zero-Cost Abstractions

- No `Box<dyn Render>` - everything monomorphized
- No `downcast_mut` - types known at compile time
- Full inline potential for LLVM optimization

### 3. IDE Support

```rust
impl Render for RenderFlex {
    type Arity = MultiArity;

    fn layout(&mut self, cx: &mut LayoutCx<Self::Arity>) -> Size {
        // IDE autocomplete shows:
        // ✅ cx.children()      - returns &[ElementId]
        // ✅ cx.child_count()   - returns usize
        // ✅ cx.layout_child()  - layout a child
        // ✅ cx.constraints()   - get constraints
        //
        // But NOT:
        // ❌ cx.child()         - not available for MultiArity!
    }
}
```

### 4. Integration with flui_engine

```rust
fn paint(&self, cx: &PaintCx<Self::Arity>) -> BoxedLayer {
    // Build scene using flui_engine layers
    let mut picture = PictureLayer::new();
    picture.draw_rect(self.rect, self.paint);

    // Capture child layers
    let child_layer = cx.capture_child_layer(cx.child());

    // Compose with effects
    let opacity = OpacityLayer::new(child_layer, self.opacity);

    Box::new(opacity)
}
```

## Migration from Old flui_core

The old implementation is preserved in `flui_core_old`. Key differences:

| Feature | Old (flui_core_old) | New (flui_core) |
|---------|---------------------|-----------------|
| Child count | Runtime checks | Compile-time types |
| Render storage | `Box<dyn>` | Monomorphized |
| Paint output | Direct to painter | Returns Layer |
| Arity validation | Assertions | Type system |
| IDE support | Generic methods | Specialized per arity |

## Examples

### Example 1: Leaf Render (No Children)

```rust
pub struct RenderParagraph {
    pub text: String,
    pub font_size: f32,
}

impl Render for RenderParagraph {
    type Arity = LeafArity;

    fn layout(&mut self, cx: &mut LayoutCx<Self::Arity>) -> Size {
        // Only constraints available
        let constraints = cx.constraints();

        // Calculate text size
        let width = self.text.len() as f32 * self.font_size * 0.6;
        let height = self.font_size * 1.2;

        constraints.constrain(Size::new(width, height))
    }

    fn paint(&self, cx: &PaintCx<Self::Arity>) -> BoxedLayer {
        // Create picture layer
        let mut picture = PictureLayer::new();
        picture.draw_text(/* ... */);

        Box::new(picture)
    }
}
```

### Example 2: Single-Child Render

```rust
pub struct RenderOpacity {
    pub opacity: f32,
}

impl Render for RenderOpacity {
    type Arity = SingleArity;

    fn layout(&mut self, cx: &mut LayoutCx<Self::Arity>) -> Size {
        // .child() method available!
        let child = cx.child();
        cx.layout_child(child, cx.constraints())
    }

    fn paint(&self, cx: &PaintCx<Self::Arity>) -> BoxedLayer {
        // .capture_child_layer() available!
        let child_layer = cx.capture_child_layer(cx.child());

        Box::new(OpacityLayer::new(child_layer, self.opacity))
    }
}
```

### Example 3: Multi-Child Render

```rust
pub struct RenderFlex {
    pub spacing: f32,
}

impl Render for RenderFlex {
    type Arity = MultiArity;

    fn layout(&mut self, cx: &mut LayoutCx<Self::Arity>) -> Size {
        // .children() method available!
        let mut total = 0.0;

        for &child in cx.children() {
            let size = cx.layout_child(child, cx.constraints());
            total += size.width + self.spacing;
        }

        Size::new(total, /* ... */)
    }

    fn paint(&self, cx: &PaintCx<Self::Arity>) -> BoxedLayer {
        let mut container = ContainerLayer::new();

        for &child in cx.children() {
            let layer = cx.capture_child_layer(child);
            container.add_child(layer);
        }

        Box::new(container)
    }
}
```

## Building

```bash
cd crates/flui_core
cargo build
cargo test
```

## Documentation

See [idea.md](../../idea.md) for the complete architecture specification.

## License

MIT OR Apache-2.0
