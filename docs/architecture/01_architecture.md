# Chapter 1: Architecture

## 📋 Overview

FLUI Engine построен на фундаментальной трехслойной архитектуре, вдохновленной Flutter, но оптимизированной для Rust с compile-time гарантиями и zero-cost abstractions.

## 🏗️ Three-Layer Architecture

```
┌─────────────────────────────────────────────────────────┐
│                   Widget Layer                          │
│  (Immutable Configuration - What to Display)            │
│                                                          │
│  • StatelessWidget  - pure functions                    │
│  • StatefulWidget   - creates State objects             │
│  • InheritedWidget  - data propagation                  │
│  • ParentDataWidget - layout metadata                   │
│  • RenderObjectWidget - direct rendering                │
└─────────────────────────────────────────────────────────┘
                           ↓
┌─────────────────────────────────────────────────────────┐
│                   Element Layer                         │
│  (Mutable State Holders - Living Instances)             │
│                                                          │
│  • ComponentElement       - for StatelessWidget         │
│  • StatefulElement       - for StatefulWidget + State   │
│  • InheritedElement      - tracks dependents            │
│  • ParentDataElement     - attaches metadata            │
│  • RenderObjectElement   - owns RenderObject            │
└─────────────────────────────────────────────────────────┘
                           ↓
┌─────────────────────────────────────────────────────────┐
│                 RenderObject Layer                      │
│  (Performance-Critical Rendering - Layout & Paint)      │
│                                                          │
│  • layout(constraints) → size                           │
│  • paint() → Layer tree                                 │
│  • Type-safe Arity system (Leaf/Single/Multi)          │
└─────────────────────────────────────────────────────────┘
                           ↓
┌─────────────────────────────────────────────────────────┐
│                    Layer Tree                           │
│  (Compositing - GPU-Optimized)                          │
│                                                          │
│  • PictureLayer   - rasterized content                  │
│  • OffsetLayer    - positioning                         │
│  • TransformLayer - 2D transforms                       │
│  • OpacityLayer   - alpha blending                      │
│  • ClipLayer      - clipping regions                    │
└─────────────────────────────────────────────────────────┘
                           ↓
┌─────────────────────────────────────────────────────────┐
│                  Render Backend                         │
│  (Platform Abstraction)                                 │
│                                                          │
│  • wgpu backend    - GPU-accelerated (primary)          │
│  • soft backend    - CPU rasterizer (fallback)          │
│  • egui backend    - debug/dev tools                    │
└─────────────────────────────────────────────────────────┘
```

## 🔄 Data Flow: From Configuration to Pixels

### Phase 1: Widget Creation (User Code)

```rust
// User creates immutable configuration
let widget = Container::new()
    .width(Some(200.0))
    .height(Some(100.0))
    .padding(EdgeInsets::all(16.0))
    .color(Color::BLUE)
    .child(Box::new(Text::new("Hello, FLUI!")));

// Widgets are cheap to clone (Arc-based internals)
let widget_clone = widget.clone();

// Can be stored, passed around, composed
let composed = Column::new()
    .children(vec![
        Box::new(widget),
        Box::new(widget_clone),
    ]);
```

**Properties:**
- ✅ Immutable
- ✅ Cloneable (cheap via Arc)
- ✅ Serializable (potentially)
- ✅ Pure data (no side effects)
- ✅ Type-safe builder pattern

### Phase 2: Widget → Element (Mounting)

```rust
// Framework creates Element from Widget
impl Widget for Container {
    fn into_element(self) -> RenderObjectElement<Self, SingleArity> {
        RenderObjectElement::new(self)
    }
}

// Element inserted into tree
let element_id = tree.insert(Box::new(element));
```

**Properties:**
- ✅ Mutable (holds state)
- ✅ Unique (one instance)
- ✅ Tree-structured
- ✅ Lifecycle management

### Phase 3: Element → RenderObject (Initialization)

```rust
// Element creates RenderObject from Widget config
impl RenderObjectWidget for Container {
    fn create_render_object(&self) -> RenderContainer {
        RenderContainer {
            width: self.width,
            height: self.height,
            padding: self.padding,
            color: self.color,
        }
    }
}
```

**Properties:**
- ✅ Performance-critical
- ✅ Type-safe (via Arity)
- ✅ Cacheable results
- ✅ GPU-friendly output

### Phase 4: Layout (Size Computation)

```rust
// RenderPipeline orchestrates layout phase
impl RenderPipeline {
    fn flush_layout(&mut self, root_constraints: BoxConstraints) -> Size {
        // 1. Sort dirty nodes by depth (parents first)
        self.nodes_needing_layout.sort_by_key(|&id| self.depth(id));
        
        // 2. Layout each dirty node
        for &node_id in &self.nodes_needing_layout {
            self.layout_node(node_id, constraints);
        }
        
        // 3. Return root size
        self.root_size()
    }
}

// Individual RenderObject layout
impl RenderObject for RenderContainer {
    fn layout(&mut self, cx: &mut LayoutCx<SingleArity>) -> Size {
        // 1. Deflate constraints by padding
        let child_constraints = cx.constraints()
            .deflate(EdgeInsets::all(self.padding));
        
        // 2. Layout child
        let child = cx.child();
        let child_size = cx.layout_child(child, child_constraints);
        
        // 3. Return our size (child + padding)
        Size::new(
            child_size.width + self.padding * 2.0,
            child_size.height + self.padding * 2.0,
        )
    }
}
```

**Algorithm:**
- ✅ Constraints flow down
- ✅ Sizes flow up
- ✅ Single-pass (with relayout boundaries)
- ✅ Cached results

### Phase 5: Paint (Layer Generation)

```rust
// RenderPipeline orchestrates paint phase
impl RenderPipeline {
    fn flush_paint(&mut self) -> BoxedLayer {
        // 1. Sort dirty nodes by depth
        self.nodes_needing_paint.sort_by_key(|&id| self.depth(id));
        
        // 2. Paint each dirty node
        for &node_id in &self.nodes_needing_paint {
            self.paint_node(node_id);
        }
        
        // 3. Return root layer
        self.root_layer()
    }
}

// Individual RenderObject paint
impl RenderObject for RenderContainer {
    fn paint(&self, cx: &PaintCx<SingleArity>) -> BoxedLayer {
        let mut container = ContainerLayer::new();
        
        // 1. Paint background
        if let Some(color) = self.color {
            let mut picture = PictureLayer::new();
            picture.draw_rect(self.bounds(), Paint::new().color(color));
            container.add_child(Box::new(picture));
        }
        
        // 2. Paint child with offset
        let child = cx.child();
        let child_layer = cx.capture_child_layer(child);
        
        let mut offset_layer = OffsetLayer::new(
            Offset::new(self.padding, self.padding)
        );
        offset_layer.add_child(child_layer);
        container.add_child(Box::new(offset_layer));
        
        Box::new(container)
    }
}
```

**Output:**
- ✅ Hierarchical layer tree
- ✅ GPU-optimized primitives
- ✅ Cached rasterization
- ✅ Compositable

### Phase 6: Compositing (Screen Output)

```rust
// Compositor combines layers and renders to screen
impl Compositor {
    fn composite(&mut self, root_layer: BoxedLayer, window: &Window) {
        // 1. Flatten layer tree
        let flattened = self.flatten_layers(root_layer);
        
        // 2. Upload to GPU
        for layer in flattened {
            self.upload_layer(layer);
        }
        
        // 3. Composite on GPU
        self.render_to_surface(window.surface());
        
        // 4. Present
        window.swap_buffers();
    }
}
```

## 📦 Module Organization

### Core Crates

```
flui/
├── flui_core/              # Core framework
│   ├── widget/             # Widget trait system
│   │   ├── stateless.rs
│   │   ├── stateful.rs
│   │   ├── inherited.rs
│   │   ├── parent_data.rs
│   │   └── render_object.rs
│   │
│   ├── element/            # Element tree management
│   │   ├── component.rs
│   │   ├── stateful.rs
│   │   ├── inherited.rs
│   │   ├── render_object_element.rs
│   │   └── element_tree.rs
│   │
│   ├── render/             # RenderObject system
│   │   ├── render_object.rs
│   │   ├── layout_cx.rs
│   │   ├── paint_cx.rs
│   │   ├── render_pipeline.rs
│   │   ├── render_state.rs
│   │   └── cache.rs
│   │
│   └── arity/              # Type-safe arity
│       └── mod.rs
│
├── flui_engine/            # Rendering engine
│   ├── layer/              # Layer system
│   │   ├── picture_layer.rs
│   │   ├── offset_layer.rs
│   │   ├── transform_layer.rs
│   │   ├── opacity_layer.rs
│   │   └── clip_layer.rs
│   │
│   ├── paint/              # Painting primitives
│   │   ├── painter.rs
│   │   ├── paint.rs
│   │   └── path.rs
│   │
│   └── compositor/         # Compositing
│       └── compositor.rs
│
├── flui_rendering/         # Backend abstraction
│   ├── backend/
│   │   ├── wgpu.rs        # GPU backend
│   │   ├── soft.rs        # Software rasterizer
│   │   └── egui.rs        # Debug backend
│   │
│   └── objects/           # Render objects
│       └── effects/
│
├── flui_widgets/          # Standard widgets
│   ├── basic/
│   │   ├── text.rs
│   │   ├── image.rs
│   │   └── container.rs
│   │
│   ├── layout/
│   │   ├── row.rs
│   │   ├── column.rs
│   │   ├── flex.rs
│   │   └── stack.rs
│   │
│   └── interactive/
│       ├── button.rs
│       ├── text_field.rs
│       └── checkbox.rs
│
├── flui_reactive/         # Reactivity system
│   ├── signal.rs
│   ├── effect.rs
│   ├── memo.rs
│   └── scope.rs
│
└── flui_types/            # Common types
    ├── geometry/
    │   ├── size.rs
    │   ├── offset.rs
    │   └── rect.rs
    │
    └── styling/
        ├── color.rs
        ├── edge_insets.rs
        └── box_decoration.rs
```

## 🎯 Design Principles

### 1. Immutability First

```rust
// ✅ Widgets are immutable
#[derive(Clone)]
pub struct Container {
    width: Option<f32>,
    height: Option<f32>,
    child: Option<BoxedWidget>,
}

// ❌ Not this
pub struct Container {
    width: Cell<Option<f32>>,  // Mutable
}
```

**Why:**
- Predictable rebuilds
- Easy to reason about
- Cacheable
- Thread-safe

### 2. Composition Over Inheritance

```rust
// ✅ Composition via traits
trait Widget: Clone + Send + Sync { }
trait StatelessWidget: Widget { }
trait StatefulWidget: Widget { }

// ❌ Not inheritance
class Widget { }
class StatelessWidget extends Widget { }
```

**Why:**
- Flexible
- Trait coherence
- Multiple trait implementations
- Zero-cost

### 3. Type Safety First

```rust
// ✅ Compile-time child count checking
impl RenderObject for RenderOpacity {
    type Arity = SingleArity;  // Exactly one child
    
    fn layout(&mut self, cx: &mut LayoutCx<Self::Arity>) -> Size {
        let child = cx.child();  // ✅ Guaranteed to exist!
    }
}

// ❌ Runtime checking (Flutter approach)
Size layout(BoxConstraints constraints) {
    if (child != null) {  // Runtime check
        child.layout(constraints);
    }
}
```

**Why:**
- Catch errors at compile time
- No runtime crashes
- Better performance (no null checks)
- Clear contracts

### 4. Zero-Cost Abstractions

```rust
// ✅ Monomorphization - no virtual dispatch
impl<W: RenderObjectWidget> RenderObjectElement<W, W::Arity> {
    fn layout(&mut self) -> Size {
        self.render_object.layout(cx)  // Direct call, inlined!
    }
}

// ❌ Not this (dynamic dispatch)
trait DynRenderObject {
    fn layout(&mut self) -> Size;
}

impl Element {
    fn layout(&mut self) -> Size {
        self.render_object.layout()  // Virtual call overhead
    }
}
```

**Why:**
- No performance penalty
- Full inlining
- Better cache utilization
- Predictable performance

### 5. Explicit Over Implicit

```rust
// ✅ Explicit dependencies
impl StatelessWidget for ThemedButton {
    fn build(&self, cx: &BuildContext) -> BoxedWidget {
        let theme = cx.get::<Theme>()?;  // Explicit!
        // ...
    }
}

// ❌ Implicit (Flutter-style)
Widget build(BuildContext context) {
    final theme = Theme.of(context);  // Magic lookup
}
```

**Why:**
- Clear dependencies
- Better IDE support
- Easier to test
- No surprises

## 🚀 Performance Architecture

### 1. Incremental Updates

```
Frame N:
  User Action → Signal.set() → Mark Scopes Dirty
                                      ↓
                              Mark Elements Dirty
                                      ↓
Frame N+1:
  Build Phase    → Rebuild only dirty elements
  Layout Phase   → Relayout only dirty nodes
  Paint Phase    → Repaint only dirty nodes
  Composite      → GPU composite layers
```

### 2. Dirty Tracking

```rust
pub struct RenderPipeline {
    tree: ElementTree,
    
    // Incremental update tracking
    nodes_needing_layout: Vec<ElementId>,
    nodes_needing_paint: Vec<ElementId>,
    
    // Relayout boundaries (optimization)
    relayout_boundaries: HashSet<ElementId>,
}

impl RenderPipeline {
    pub fn mark_needs_layout(&mut self, id: ElementId) {
        // Add to dirty list
        self.nodes_needing_layout.push(id);
        
        // Propagate up to relayout boundary
        if !self.is_relayout_boundary(id) {
            if let Some(parent) = self.parent(id) {
                self.mark_needs_layout(parent);
            }
        }
    }
}
```

### 3. Layout Cache

```rust
pub struct LayoutCache {
    cache: Moka<LayoutCacheKey, LayoutResult>,
}

#[derive(Hash, Eq, PartialEq)]
pub struct LayoutCacheKey {
    element_id: ElementId,
    constraints: BoxConstraints,
    child_count: usize,
}

// Usage:
fn layout(&mut self, cx: &mut LayoutCx) -> Size {
    let key = LayoutCacheKey::new(cx.element_id(), cx.constraints());
    
    if let Some(cached) = LAYOUT_CACHE.get(&key) {
        return cached.size;  // Cache hit!
    }
    
    // Cache miss - compute layout
    let size = self.compute_layout(cx);
    LAYOUT_CACHE.insert(key, LayoutResult { size });
    size
}
```

### 4. Parallel Potential

```rust
// Future: Parallel layout
impl RenderFlex {
    fn layout(&mut self, cx: &mut LayoutCx<MultiArity>) -> Size {
        let children = cx.children();
        
        // Parallel layout (when safe)
        let sizes: Vec<Size> = children
            .par_iter()  // Rayon parallel iterator
            .map(|&child| cx.layout_child(child, constraints))
            .collect();
        
        self.combine_sizes(sizes)
    }
}
```

## 🎨 Flexibility Points

### Extension Points

1. **Custom Widgets** - implement Widget traits
2. **Custom RenderObjects** - implement RenderObject trait
3. **Custom Backends** - implement Backend trait
4. **Custom Layers** - implement Layer trait
5. **Custom Gestures** - implement GestureRecognizer trait

### Plugin Architecture

```rust
pub trait FluiPlugin {
    fn init(&self, app: &mut App);
    fn build(&self) -> BoxedWidget;
}

// Usage:
fn main() {
    App::new()
        .add_plugin(MaterialPlugin)
        .add_plugin(RouterPlugin)
        .add_plugin(AnimationPlugin)
        .run();
}
```

## 📊 Architecture Benefits

| Benefit | How Achieved |
|---------|-------------|
| **Type Safety** | Arity system, borrow checker, Option<T> |
| **Performance** | Zero-cost abstractions, caching, incremental updates |
| **Flexibility** | Trait-based composition, plugin system |
| **Maintainability** | Clear separation of concerns, explicit dependencies |
| **Scalability** | Modular architecture, parallel-ready |
| **Debuggability** | Inspector, profiler, layer visualization |

## 🔗 Cross-References

- **Next:** [Chapter 2: Widget/Element System](02_widget_element_system.md)
- **Related:** [Chapter 3: RenderObject System](03_render_objects.md)
- **See Also:** [Chapter 8: Frame Scheduler](08_frame_scheduler.md)

---

**Key Takeaway:** FLUI's three-layer architecture provides type-safe, high-performance UI rendering with clear separation of concerns and zero-cost abstractions.
