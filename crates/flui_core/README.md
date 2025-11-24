# flui_core

Core reactive UI framework for FLUI - implements the three-tree architecture (View → Element → Render) with Flutter-inspired lifecycle management.

## Overview

`flui_core` is the heart of FLUI, providing:

- **View System** - Declarative UI descriptions with specialized traits
- **Element Tree** - Mutable element lifecycle management with unified architecture
- **Render Protocol** - Layout and paint abstraction layer
- **Pipeline** - Coordinated build/layout/paint phases
- **Hooks** - React-like state management (thread-safe)

## Architecture

### Three-Tree System

```
View Tree (immutable) → Element Tree (mutable) → Render Tree (layout/paint)
     └─ Views                 └─ Elements              └─ RenderObjects
        StatelessView            ViewObject              LeafRender
        StatefulView             RenderViewWrapper       SingleRender
        RenderView               ProviderViewWrapper     MultiRender
```

### Unified Element (v0.7.0)

FLUI uses a **unified Element struct** where all type-specific behavior is delegated to `ViewObject`:

```rust
pub struct Element {
    parent: Option<ElementId>,
    children: Vec<ElementId>,
    lifecycle: ElementLifecycle,
    view_object: Box<dyn ViewObject>,
}
```

**Benefits:**
- Single struct instead of enum - no dispatch overhead
- Extensible - add new view types without changing Element
- Flutter-like architecture with Rust idioms
- Clean separation of concerns

### ViewObject Trait

The `ViewObject` trait provides dynamic dispatch for view lifecycle:

```rust
pub trait ViewObject: Send {
    // Core lifecycle
    fn build(&mut self, ctx: &BuildContext) -> Element;
    fn mode(&self) -> ViewMode;
    
    // Optional lifecycle hooks
    fn init(&mut self, ctx: &BuildContext) {}
    fn did_update(&mut self, new_view: &dyn Any, ctx: &BuildContext) {}
    fn dispose(&mut self, ctx: &BuildContext) {}
    
    // Type-specific methods (default: None)
    fn render_object(&self) -> Option<&dyn RenderObject> { None }
    fn render_state(&self) -> Option<&RenderState> { None }
    fn provided_value(&self) -> Option<&(dyn Any + Send + Sync)> { None }
    fn dependents(&self) -> Option<&[ElementId]> { None }
}
```

**ViewObject Implementations:**
- `StatelessViewWrapper<V>` - Wraps `StatelessView`
- `StatefulViewWrapper<V, S>` - Wraps `StatefulView<S>`
- `AnimatedViewWrapper<V, L>` - Wraps `AnimatedView<L>`
- `ProviderViewWrapper<V, T>` - Wraps `ProviderView<T>`, stores value + dependents
- `ProxyViewWrapper<V>` - Wraps `ProxyView`
- `RenderViewWrapper<V, P, A>` - Wraps `RenderView<P, A>`, stores render object + state

## View Types

### StatelessView - Simple Views

For views without internal state:

```rust
#[derive(Debug)]
struct Greeting {
    name: String,
}

impl StatelessView for Greeting {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        Text::new(format!("Hello, {}!", self.name))
    }
}
```

### StatefulView - Persistent State

For views with mutable state that persists across rebuilds:

```rust
#[derive(Debug)]
struct Counter;

#[derive(Debug)]
struct CounterState {
    count: i32,
}

impl StatefulView<CounterState> for Counter {
    fn build(&self, state: &CounterState, _ctx: &BuildContext) -> impl IntoElement {
        Column::new()
            .child(Text::new(format!("Count: {}", state.count)))
            .child(Button::new("Increment"))
    }
    
    fn init_state(&self) -> CounterState {
        CounterState { count: 0 }
    }
}
```

### RenderView - Custom Layout/Paint

For views that need custom layout and paint logic:

```rust
#[derive(Clone, Debug)]
struct CustomBox {
    color: Color,
    size: Size,
}

#[derive(Debug)]
struct CustomBoxRender {
    color: Color,
    size: Size,
}

impl RenderBox<Leaf> for CustomBoxRender {
    fn layout(&mut self, _ctx: LayoutContext<Leaf, BoxProtocol>) -> Size {
        self.size
    }
    
    fn paint(&self, ctx: &mut PaintContext<Leaf>) {
        ctx.canvas.draw_rect(Rect::from_size(self.size), self.color);
    }
}

impl RenderView<BoxProtocol, Leaf> for CustomBox {
    type RenderObject = CustomBoxRender;
    
    fn create(&self) -> CustomBoxRender {
        CustomBoxRender {
            color: self.color,
            size: self.size,
        }
    }
    
    fn update(&self, render: &mut CustomBoxRender) -> UpdateResult {
        if render.color != self.color || render.size != self.size {
            render.color = self.color;
            render.size = self.size;
            UpdateResult::NeedsLayout
        } else {
            UpdateResult::Unchanged
        }
    }
}
```

## Hooks

FLUI provides React-like hooks for state management. **All hooks are thread-safe** using `Arc`/`Mutex`.

### use_signal - Reactive State

```rust
impl StatelessView for Counter {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        let count = use_signal(ctx, 0);
        let count_clone = count.clone(); // Clone before moving into closure
        
        Column::new()
            .child(Text::new(format!("Count: {}", count.get())))
            .child(Button::new("Increment")
                .on_pressed(move || count_clone.update(|c| *c += 1)))
    }
}
```

### use_memo - Derived State

```rust
let count = use_signal(ctx, 0);
let doubled = use_memo(ctx, move |_| count.get() * 2);

println!("Doubled: {}", doubled.get());
```

### use_effect - Side Effects

```rust
use_effect(ctx, move || {
    println!("Component mounted");
    
    Some(Box::new(move || {
        println!("Component unmounted");
    }))
});
```

**Hook Rules (CRITICAL):**
1. ✅ Always call hooks in the same order
2. ❌ Never call hooks conditionally
3. ❌ Never call hooks in loops with variable iterations
4. ✅ Clone signals before moving into closures

Breaking these rules causes **panics**. See `src/hooks/RULES.md` for details.

## Element Tree

Elements are stored in a `Slab` arena:

```rust
pub struct ElementTree {
    nodes: Slab<ElementNode>,
    roots: Vec<ElementId>,
}
```

**Key Points:**
- ElementId uses `NonZeroUsize` for niche optimization (Option<ElementId> = 8 bytes)
- **CRITICAL:** Slab indices are 0-based but ElementId is 1-based (+1 offset in insert, -1 in get)
- Lifecycle states: Initial → Active → Inactive → Defunct

**Element Access:**
```rust
// Get element
let element = tree.get(element_id)?;

// Check type
if element.is_render() {
    let render = element.render_object().unwrap();
    let state = element.render_state().unwrap();
}

// Children (unified API for all types)
for child_id in element.children() {
    // Process child
}
```

## Pipeline

The rendering pipeline has three coordinated phases:

```rust
pub struct PipelineOwner {
    tree: ElementTree,
    coordinator: FrameCoordinator,
}

impl PipelineOwner {
    // Phase 1: Build - Rebuild dirty elements
    pub fn flush_build(&mut self) { /* ... */ }
    
    // Phase 2: Layout - Compute sizes
    pub fn flush_layout(&mut self, constraints: BoxConstraints) { /* ... */ }
    
    // Phase 3: Paint - Generate display list
    pub fn flush_paint(&mut self) -> Canvas { /* ... */ }
}
```

**Frame Rendering:**
```rust
// 1. Mark elements dirty
pipeline.mark_needs_build(element_id);

// 2. Flush all phases
pipeline.flush_build();
pipeline.flush_layout(constraints);
let canvas = pipeline.flush_paint();

// 3. Render canvas to screen
renderer.render(canvas);
```

## Thread Safety

FLUI is **fully thread-safe**:

- All hooks use `Arc<Mutex<T>>` (parking_lot)
- ViewObject is `Send`
- Element is `Send`
- RwLock used for render objects (concurrent reads)

**Performance:**
- `parking_lot::Mutex` is 2-3x faster than `std::sync::Mutex`
- `parking_lot::RwLock` is 2-3x faster than `std::sync::RwLock`
- No poisoning on panic

## Examples

See `examples/` directory:
- `simplified_view.rs` - Modern View API demo
- `thread_safe_hooks.rs` - Thread-safe hooks demonstration

## Documentation

- `CLAUDE.md` - Full architecture guide
- `docs/API_GUIDE.md` - Comprehensive API documentation
- `docs/PIPELINE_ARCHITECTURE.md` - Pipeline deep dive
- `src/hooks/RULES.md` - Hook usage rules (MUST READ)

## Features

- `parallel` - Enable rayon-based parallel build (thread-safe, stable)

## License

MIT OR Apache-2.0
