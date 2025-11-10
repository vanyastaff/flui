# flui_core

[![Crates.io](https://img.shields.io/crates/v/flui_core.svg)](https://crates.io/crates/flui_core)
[![Documentation](https://docs.rs/flui_core/badge.svg)](https://docs.rs/flui_core)
[![License](https://img.shields.io/crates/l/flui_core.svg)](https://github.com/yourusername/flui/blob/main/LICENSE)

**Core framework for FLUI - A reactive UI framework for Rust inspired by Flutter**

FLUI provides a declarative, view-based API for building high-performance user interfaces in Rust with automatic state management and efficient rendering.

## Features

- 🎯 **Type-Safe View System** - Compile-time view type checking with zero runtime overhead
- 🚀 **High Performance** - Enum-based dispatch (3-4x faster than Box<dyn> trait objects)
- ♻️ **Automatic Reactivity** - Smart rebuilding only when state actually changes with hooks
- 🎨 **Flexible Rendering** - Clean separation between views, state, and rendering
- 🔧 **Modern Hooks API** - Use signals, effects, and memos for reactive state
- 🏗️ **Component-Based** - Composable views with clean interfaces
- 📦 **Efficient Memory** - Slab-based element tree with O(1) access
- 🔌 **Provider Pattern** - Efficient data propagation with automatic dependency tracking

## Quick Start

Add to your `Cargo.toml`:

```toml
[dependencies]
flui_core = "0.1"
```

### Hello World

```rust
use flui_core::prelude::*;

#[derive(Debug, Clone)]
struct HelloWorld;

impl View for HelloWorld {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        Text::new("Hello, World!")
    }
}
```

Views are composable UI components that build UIs from other views or renderers. They can manage state using hooks.

### Counter with State

```rust
use flui_core::prelude::*;

#[derive(Debug, Clone)]
struct Counter {
    initial: i32,
}

impl View for Counter {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        // Reactive state with hooks
        let count = use_signal(ctx, self.initial);

        // Clone signal before moving into closure
        let count_clone = count.clone();

        // Return a tuple: (Renderer, Option<child>) or just another View
        Column {
            children: vec![
                Box::new(Text::new(format!("Count: {}", count.get()))),
                Box::new(Button {
                    label: "Increment".to_string(),
                    on_press: Some(Box::new(move || {
                        count_clone.update(|c| *c += 1);
                    })),
                }),
            ],
        }
    }
}
```

### Tuple Syntax for Renderers

```rust
use flui_core::prelude::*;

#[derive(Debug, Clone)]
struct Padding {
    padding: f32,
    child: Option<Box<dyn AnyView>>,
}

impl View for Padding {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        // Tuple syntax: (Renderer, Option<child>)
        (RenderPadding::new(self.padding), self.child)
    }
}
```

## Architecture

FLUI uses a **three-tree architecture** for optimal performance:

```text
┌─────────────────┐      ┌─────────────────┐      ┌─────────────────┐
│  View Tree      │      │  Element Tree   │      │  Render Tree    │
│                 │      │                 │      │                 │
│  (Immutable     │ ───> │  (Mutable       │ ───> │  (Layout &      │
│   Configuration)│      │   State)        │      │   Paint)        │
└─────────────────┘      └─────────────────┘      └─────────────────┘
```

### View Tree (Immutable Configuration)

Views are lightweight, immutable descriptions of what the UI should look like:

```rust
pub trait View: 'static {
    fn build(self, ctx: &BuildContext) -> impl IntoElement;
}

// Views return one of:
// - Another View (composition)
// - Tuple of (Renderer, children) for custom rendering
// - Element (via IntoElement trait)
```

### Element Tree (Mutable State)

Elements hold the living state and lifecycle of views. They persist across rebuilds:

```rust
pub enum Element {
    Component(ComponentElement),      // Component view instance with optional state
    Provider(InheritedElement),       // Provider for data propagation
    Render(RenderElement),            // Bridge to render tree
}
```

### Render Tree (Layout & Paint)

Renderers perform layout calculations and produce visual output:

```rust
pub trait Render: Send + Sync + Debug + 'static {
    fn layout(&mut self, ctx: &LayoutContext) -> Size;
    fn paint(&self, ctx: &PaintContext) -> BoxedLayer;
    fn arity(&self) -> Arity { Arity::Variable }
}

// Context structs provide access to children and tree:
// - LayoutContext: constraints, children, tree access for layout
// - PaintContext: offset, children, tree access for painting
```

## View Patterns

FLUI provides a unified View trait with different implementation patterns:

### Composable Views

Views that build UIs from other views:

```rust
use flui_core::prelude::*;

#[derive(Debug, Clone)]
struct Greeting {
    name: String,
}

impl View for Greeting {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        Text::new(format!("Hello, {}!", self.name))
    }
}

// With state using hooks:
#[derive(Debug, Clone)]
struct Toggle {
    initial: bool,
}

impl View for Toggle {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        let enabled = use_signal(ctx, self.initial);
        let enabled_clone = enabled.clone();

        Checkbox {
            value: enabled.get(),
            on_change: Some(Box::new(move |val| enabled_clone.set(val))),
        }
    }
}
```

**When to use**: Display components, user interactions, animations, form inputs, most UI composition.

**State management**:
- Hooks: `use_signal`, `use_effect`, `use_memo` for reactive state

### Provider Pattern

Efficient data propagation with automatic dependency tracking:

```rust
use flui_core::prelude::*;

#[derive(Debug, Clone)]
struct Theme {
    color: Color,
    child: Box<dyn AnyView>,
}

impl View for Theme {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        // Providers are implemented via Element::Provider variant
        // This provides theme data to descendants
        Element::Provider(ProviderElement::new(self, self.child))
    }
}

// Access from descendants:
let theme = ctx.get_provider::<Theme>()?;
let color = theme.color;
```

**When to use**: Themes, localization, configuration, app-wide state.

**Key features**:
- Automatic dependency tracking
- Only rebuilds dependents when data changes
- Type-safe access via generics

### Custom Renderers

Direct control over layout and painting using tuple syntax:

```rust
use flui_core::prelude::*;

// View that wraps a custom renderer
#[derive(Debug, Clone)]
struct CustomBox {
    width: f32,
    height: f32,
    color: Color,
}

impl View for CustomBox {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        // Tuple syntax: (Renderer, ())
        // The () indicates no children (leaf renderer)
        (RenderCustomBox {
            width: self.width,
            height: self.height,
            color: self.color,
        }, ())
    }
}

// Implement the Render trait
#[derive(Debug)]
struct RenderCustomBox {
    width: f32,
    height: f32,
    color: Color,
}

impl Render for RenderCustomBox {
    fn layout(&mut self, ctx: &LayoutContext) -> Size {
        ctx.constraints.constrain(Size::new(self.width, self.height))
    }

    fn paint(&self, ctx: &PaintContext) -> BoxedLayer {
        Box::new(PictureLayer::new(/* draw at ctx.offset */))
    }

    fn arity(&self) -> Arity {
        Arity::Exact(0)  // No children
    }
}
```

**When to use**: Custom layouts, complex drawing, performance-critical rendering.

## Render Patterns

The Render trait handles all child counts through context structs:

### Leaf Renderer (No Children)

```rust
use flui_core::prelude::*;

#[derive(Debug)]
struct RenderCircle {
    radius: f32,
    color: Color,
}

impl Render for RenderCircle {
    fn layout(&mut self, ctx: &LayoutContext) -> Size {
        let size = self.radius * 2.0;
        ctx.constraints.constrain(Size::new(size, size))
    }

    fn paint(&self, ctx: &PaintContext) -> BoxedLayer {
        // Draw circle at ctx.offset
        Box::new(PictureLayer::circle(ctx.offset, self.radius, self.color))
    }

    fn arity(&self) -> Arity {
        Arity::Exact(0)  // No children
    }
}
```

### Single Child Renderer

```rust
use flui_core::prelude::*;

#[derive(Debug)]
struct RenderOpacity {
    opacity: f32,
}

impl Render for RenderOpacity {
    fn layout(&mut self, ctx: &LayoutContext) -> Size {
        // Get single child from context
        let child_id = ctx.children.single();
        // Layout child with same constraints
        ctx.layout_child(child_id, ctx.constraints)
    }

    fn paint(&self, ctx: &PaintContext) -> BoxedLayer {
        let child_id = ctx.children.single();
        let child_layer = ctx.paint_child(child_id, ctx.offset);
        Box::new(OpacityLayer::new(child_layer, self.opacity))
    }

    fn arity(&self) -> Arity {
        Arity::Exact(1)  // Exactly one child
    }
}
```

### Multiple Children Renderer

```rust
use flui_core::prelude::*;

#[derive(Debug)]
struct RenderRow {
    spacing: f32,
}

impl Render for RenderRow {
    fn layout(&mut self, ctx: &LayoutContext) -> Size {
        let mut x = 0.0;
        let mut max_height = 0.0;

        // Iterate over children
        for &child_id in ctx.children.as_slice() {
            let child_size = ctx.layout_child(child_id, ctx.constraints);
            x += child_size.width + self.spacing;
            max_height = max_height.max(child_size.height);
        }

        Size::new(x, max_height)
    }

    fn paint(&self, ctx: &PaintContext) -> BoxedLayer {
        let mut container = ContainerLayer::new();
        let mut x = 0.0;

        for &child_id in ctx.children.as_slice() {
            let offset = Offset::new(x, 0.0);
            let layer = ctx.paint_child(child_id, ctx.offset + offset);
            container.add_child(layer);
            x += ctx.get_size(child_id).width + self.spacing;
        }

        Box::new(container)
    }

    fn arity(&self) -> Arity {
        Arity::Variable  // Any number of children
    }
}
```

## Key Features Explained

### 🎯 Unified View Trait

FLUI uses a single unified View trait for all UI components:

```rust
pub trait View: 'static {
    fn build(self, ctx: &BuildContext) -> impl IntoElement;
}

// All views implement this single trait:
// - Composable views return other views
// - Custom renderers return tuples (Renderer, children)
// - Providers create Element::Provider directly
```

**Benefits**:
- Single trait to learn (no Component/Provider/Render distinctions)
- Compile-time type checking via impl IntoElement
- Thread-local BuildContext (no &mut needed)
- Zero-cost abstractions with inline expansion

### 🏗️ Builder Pattern

Most built-in views in FLUI support the builder pattern for ergonomic construction:

```rust
// Simple text view:
let text = Text::builder()
    .data("Hello")
    .size(24.0)
    .color(Color::WHITE)
    .build();

// Complex nested composition:
Container::builder()
    .width(300.0)
    .height(200.0)
    .padding(EdgeInsets::all(20.0))
    .decoration(BoxDecoration {
        color: Some(Color::BLUE),
        border_radius: Some(BorderRadius::circular(16.0)),
        ..Default::default()
    })
    .child(
        Text::builder()
            .data("Nested content")
            .build()
    )
    .build()
```

The builder pattern provides a fluent, ergonomic API for view composition with compile-time type checking.

### 🎯 Object-Safe Traits

All traits are object-safe from the start - no need for wrapper traits:

```rust
// ✅ Works directly!
let render: Box<dyn LeafRender> = Box::new(RenderCircle { /* ... */ });

// Old approach needed wrapper - not anymore!
```

### 🚀 Enum-Based Dispatch

Using enums instead of `Box<dyn>` provides **3-4x performance improvement**:

```rust
// Match-based dispatch (fast!)
match element {
    Element::Component(c) => c.build(),
    Element::Provider(p) => p.propagate(),
    Element::Render(r) => r.layout(),
}

// Compiler can heavily optimize enum dispatch with:
// - Inline optimizations
// - Better cache locality
// - No virtual function overhead
```

### 🔧 Modern Hooks API

FLUI provides a hooks-based API for reactive state management:

```rust
use flui_core::hooks::*;

#[derive(Debug, Clone)]
struct MyCounter;

impl Component for MyCounter {
    fn build(&self, ctx: &BuildContext) -> View {
        // Reactive state
        let count = use_signal(ctx, || 0);

        // Side effects
        use_effect(ctx, || {
            println!("Count changed: {}", count.get());
        }, &[count.get()]);

        // Memoized computation
        let doubled = use_memo(ctx, || count.get() * 2, &[count.get()]);

        Column::builder()
            .children(vec![
                Text::builder()
                    .data(format!("Count: {} (doubled: {})", count.get(), doubled))
                    .build(),
                Button::builder()
                    .on_press(move || count.update(|c| *c += 1))
                    .build(),
            ])
            .build()
    }
}
```

Hooks provide a clean, composable way to manage state without boilerplate.

### 📦 Slab-Based Element Tree

Efficient memory layout with O(1) access:

```rust
pub struct ElementTree {
    nodes: Slab<ElementNode>,  // Contiguous memory allocation
}

// Fast access by ID
let element = tree.get(element_id)?;  // O(1)
```

## Performance

FLUI is designed for high performance:

| Operation | Complexity | Notes |
|-----------|------------|-------|
| Element lookup | O(1) | Slab-based indexing |
| Element dispatch | O(1) | Enum match (inline-able) |
| State updates | O(affected) | Only rebuilds dirty subtree |
| Layout cache | O(1) | Constraint-based memoization |

**Benchmark results** (vs trait objects):
- Element dispatch: **3-4x faster**
- Memory usage: **30% less**
- Binary size: **20% smaller**

## Examples

See the [examples](../../examples/) directory for complete applications:

- **[hello_world](../../examples/hello_world.rs)** - Basic Component view
- **[counter_signal](../../examples/counter_signal.rs)** - Component with hooks (use_signal)
- **[counter_set_state](../../examples/counter_set_state.rs)** - Component with State type parameter
- **[theme](../../examples/theme.rs)** - Provider for data propagation
- **[custom_render](../../examples/custom_render.rs)** - Custom Render view
- **[layout](../../examples/layout.rs)** - Flex layout system

### View Composition Example

```rust
use flui_core::prelude::*;
use flui_widgets::prelude::*;

// Composing views with the builder pattern
let my_ui = Column::builder()
    .main_axis_alignment(MainAxisAlignment::Center)
    .children(vec![
        Text::builder()
            .data("Title")
            .size(32.0)
            .color(Color::BLACK)
            .build(),
        Container::builder()
            .padding(EdgeInsets::symmetric(10.0, 20.0))
            .child(
                Text::builder()
                    .data("Subtitle")
                    .size(16.0)
                    .build()
            )
            .build(),
    ])
    .build();
```

## Testing

Run the test suite:

```bash
cargo test -p flui_core
```

Run integration tests:

```bash
cargo test -p flui_core --test render_architecture_test
```

## Documentation

Generate and open the documentation:

```bash
cargo doc -p flui_core --open
```

## Contributing

Contributions are welcome! Please read the [Contributing Guide](../../CONTRIBUTING.md) first.

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](../../LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](../../LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

## Related Crates

- **[flui_engine](../flui_engine)** - Low-level rendering engine with layer compositing
- **[flui_types](../flui_types)** - Common types (Size, Offset, Color, etc.)
- **[flui_painting](../flui_painting)** - Painting and styling primitives
- **[flui_rendering](../flui_rendering)** - Built-in render objects (Text, Image, Flex, etc.)

## Comparison with Flutter

| Feature | Flutter | FLUI |
|---------|---------|------|
| Language | Dart | Rust |
| View tree | Runtime Widget tree | Enum-based compile-time |
| State | StatefulWidget | Hooks or State<T> |
| Rendering | Skia | Pluggable backends |
| Hot reload | ✅ Yes | 🚧 Planned |
| FFI | C/C++ | Native Rust |

FLUI takes inspiration from Flutter's architecture but leverages Rust's type system for additional safety and performance. The view-based API provides a modern, reactive approach to UI development.
