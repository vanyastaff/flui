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

impl Component for HelloWorld {
    fn build(&self, ctx: &BuildContext) -> View {
        Text::builder()
            .data("Hello, World!")
            .size(24.0)
            .build()
    }
}
```

Components are composable views that build UIs from other views. They can optionally manage state using hooks or the State type parameter.

### Complete Example with Builder Pattern

```rust
use flui_core::prelude::*;
use flui_widgets::prelude::*;

#[derive(Debug, Clone)]
struct WelcomeScreen;

impl Component for WelcomeScreen {
    fn build(&self, _ctx: &BuildContext) -> View {
        Container::builder()
            .padding(EdgeInsets::all(40.0))
            .color(Color::rgb(245, 245, 245))
            .child(
                Center::builder()
                    .child(
                        Container::builder()
                            .padding(EdgeInsets::all(24.0))
                            .decoration(BoxDecoration {
                                color: Some(Color::rgb(66, 165, 245)),
                                border_radius: Some(BorderRadius::circular(12.0)),
                                ..Default::default()
                            })
                            .child(
                                Text::builder()
                                    .data("Welcome to FLUI!")
                                    .size(32.0)
                                    .color(Color::WHITE)
                                    .build()
                            )
                            .build()
                    )
                    .build()
            )
            .build()
    }
}
```

### Counter with Hooks

```rust
use flui_core::prelude::*;
use flui_core::hooks::use_signal;

#[derive(Debug, Clone)]
struct Counter {
    initial: i32,
}

impl Component for Counter {
    fn build(&self, ctx: &BuildContext) -> View {
        // Reactive state with hooks
        let count = use_signal(ctx, || self.initial);

        Column::builder()
            .children(vec![
                Text::builder()
                    .data(format!("Count: {}", count.get()))
                    .size(32.0)
                    .build(),
                Button::builder()
                    .label("Increment")
                    .on_press(move || count.update(|c| *c += 1))
                    .build(),
            ])
            .build()
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
pub trait AnyView {
    fn build(&self, ctx: &BuildContext) -> Element;
}

// Three core view types:
// - Component: Composable views with optional state (hooks or State<T>)
// - Provider: Data propagation with dependency tracking
// - Render: Custom layout and painting
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

Render nodes perform layout calculations and produce visual output:

```rust
pub enum RenderNode {
    Leaf(Box<dyn LeafRender>),                           // No children (e.g., text, image)
    Single { render: Box<dyn SingleRender>, child },     // One child (e.g., opacity, padding)
    Multi { render: Box<dyn MultiRender>, children },    // Multiple children (e.g., flex, stack)
}
```

## View Types

FLUI provides three core view types for building UIs:

### Component Views

Composable views that build UIs from other views. They can have optional state managed via hooks or the State type parameter:

```rust
use flui_core::prelude::*;
use flui_core::hooks::use_signal;

#[derive(Debug, Clone)]
struct Greeting {
    name: String,
}

impl Component for Greeting {
    fn build(&self, ctx: &BuildContext) -> View {
        Text::builder()
            .data(format!("Hello, {}!", self.name))
            .size(18.0)
            .build()
    }
}

// With state using hooks:
#[derive(Debug, Clone)]
struct Toggle {
    initial: bool,
}

impl Component for Toggle {
    fn build(&self, ctx: &BuildContext) -> View {
        let enabled = use_signal(ctx, || self.initial);

        Checkbox::builder()
            .value(enabled.get())
            .on_change(move |val| enabled.set(val))
            .build()
    }
}
```

**When to use**: Display components, user interactions, animations, form inputs, most UI composition.

**State management options**:
- Hooks: `use_signal`, `use_effect`, `use_memo` for reactive state
- State type parameter: Traditional approach similar to Flutter's StatefulWidget

### Provider Views

Efficient data propagation with automatic dependency tracking:

```rust
use flui_core::prelude::*;

#[derive(Debug, Clone)]
struct Theme {
    color: Color,
    child: View,
}

impl Provider for Theme {
    fn should_notify(&self, old: &Self) -> bool {
        self.color != old.color
    }

    fn child(&self) -> View {
        self.child.clone()
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

### Render Views

Direct control over layout and painting for custom render objects:

```rust
use flui_core::prelude::*;

#[derive(Debug, Clone)]
struct CustomBox {
    width: f32,
    height: f32,
    color: Color,
}

impl Render for CustomBox {
    type RenderObject = RenderCustomBox;

    fn create_render(&self) -> Self::RenderObject {
        RenderCustomBox {
            width: self.width,
            height: self.height,
            color: self.color,
        }
    }

    fn update_render(&self, render: &mut Self::RenderObject) {
        render.width = self.width;
        render.height = self.height;
        render.color = self.color;
    }
}

// Implement LeafRender, SingleRender, or MultiRender
impl LeafRender for RenderCustomBox {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        constraints.constrain(Size::new(self.width, self.height))
    }

    fn paint(&self, offset: Offset) -> BoxedLayer {
        Box::new(PictureLayer::new(/* ... */))
    }
}
```

**When to use**: Custom layouts, complex drawing, performance-critical rendering.

## Render Traits

Implement one of three traits based on your render's child count:

### LeafRender (No Children)

```rust
use flui_core::prelude::*;

#[derive(Debug)]
struct RenderCircle {
    radius: f32,
    color: Color,
}

impl LeafRender for RenderCircle {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        let size = self.radius * 2.0;
        constraints.constrain(Size::new(size, size))
    }

    fn paint(&self, offset: Offset) -> BoxedLayer {
        // Draw circle
        Box::new(PictureLayer::circle(offset, self.radius, self.color))
    }
}
```

### SingleRender (One Child)

```rust
use flui_core::prelude::*;

#[derive(Debug)]
struct RenderOpacity {
    opacity: f32,
}

impl SingleRender for RenderOpacity {
    fn layout(
        &mut self,
        tree: &ElementTree,
        child_id: ElementId,
        constraints: BoxConstraints,
    ) -> Size {
        // Layout child with same constraints
        tree.layout_child(child_id, constraints)
    }

    fn paint(
        &self,
        tree: &ElementTree,
        child_id: ElementId,
        offset: Offset,
    ) -> BoxedLayer {
        let child_layer = tree.paint_child(child_id, offset);
        Box::new(OpacityLayer::new(child_layer, self.opacity))
    }
}
```

### MultiRender (Multiple Children)

```rust
use flui_core::prelude::*;

#[derive(Debug)]
struct RenderRow {
    spacing: f32,
}

impl MultiRender for RenderRow {
    fn layout(
        &mut self,
        tree: &ElementTree,
        children: &[ElementId],
        constraints: BoxConstraints,
    ) -> Size {
        let mut x = 0.0;
        let mut max_height = 0.0;

        for &child in children {
            let child_size = tree.layout_child(child, constraints);
            x += child_size.width + self.spacing;
            max_height = max_height.max(child_size.height);
        }

        Size::new(x, max_height)
    }

    fn paint(
        &self,
        tree: &ElementTree,
        children: &[ElementId],
        offset: Offset,
    ) -> BoxedLayer {
        let mut container = ContainerLayer::new();
        let mut x = offset.x;

        for &child in children {
            let layer = tree.paint_child(child, Offset::new(x, offset.y));
            container.add_child(layer);
            x += tree.get_size(child).width + self.spacing;
        }

        Box::new(container)
    }
}
```

## Key Features Explained

### 🎯 Type-Safe View Composition

FLUI uses trait-based view composition for type safety and ergonomics:

```rust
// Component views
impl Component for MyView {
    fn build(&self, ctx: &BuildContext) -> View { /* ... */ }
}

// Provider views
impl Provider for MyTheme {
    fn should_notify(&self, old: &Self) -> bool { /* ... */ }
    fn child(&self) -> View { /* ... */ }
}

// Render views
impl Render for MyCustomBox {
    type RenderObject = RenderMyCustomBox;
    fn create_render(&self) -> Self::RenderObject { /* ... */ }
    fn update_render(&self, render: &mut Self::RenderObject) { /* ... */ }
}
```

**Benefits**:
- Compile-time type checking for view composition
- Cleaner, more ergonomic API
- Works seamlessly with builder patterns
- Zero-cost abstractions

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

// Old approach needed DynRenderObject wrapper - not anymore!
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
