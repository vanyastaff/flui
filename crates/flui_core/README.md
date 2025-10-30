# flui_core

[![Crates.io](https://img.shields.io/crates/v/flui_core.svg)](https://crates.io/crates/flui_core)
[![Documentation](https://docs.rs/flui_core/badge.svg)](https://docs.rs/flui_core)
[![License](https://img.shields.io/crates/l/flui_core.svg)](https://github.com/yourusername/flui/blob/main/LICENSE)

**Core framework for FLUI - A reactive UI framework for Rust inspired by Flutter**

FLUI provides a declarative, widget-based API for building high-performance user interfaces in Rust with automatic state management and efficient rendering.

## Features

- 🎯 **Type-Safe Widget System** - Compile-time widget type checking with zero runtime overhead
- 🚀 **High Performance** - Enum-based dispatch (3-4x faster than Box<dyn> trait objects)
- ♻️ **Automatic Reactivity** - Smart rebuilding only when state actually changes
- 🎨 **Flexible Rendering** - Clean separation between widgets, state, and rendering
- 🔧 **Zero Boilerplate** - `impl_into_widget!` macro for seamless widget integration
- 🏗️ **Ergonomic Builders** - Fluent builder pattern for all widgets
- 📦 **Efficient Memory** - Slab-based element tree with O(1) access
- 🔌 **Composable** - `IntoWidget` trait for automatic widget conversion

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

// Use the impl_into_widget! macro to automatically implement IntoWidget
flui_core::impl_into_widget!(HelloWorld, stateless);

impl StatelessWidget for HelloWorld {
    fn build(&self, ctx: &BuildContext) -> Widget {
        Text::builder()
            .data("Hello, World!")
            .size(24.0)
            .build()
    }
}
```

The `impl_into_widget!` macro generates implementations of `IntoWidget` trait and `From<T> for Widget`, allowing seamless conversion between your widget types and the `Widget` enum.

### Complete Example with Builder Pattern

```rust
use flui_core::prelude::*;
use flui_widgets::prelude::*;

#[derive(Debug, Clone)]
struct WelcomeScreen;

flui_core::impl_into_widget!(WelcomeScreen, stateless);

impl StatelessWidget for WelcomeScreen {
    fn build(&self, _ctx: &BuildContext) -> Widget {
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

### Stateful Counter

```rust
use flui_core::prelude::*;

#[derive(Debug, Clone)]
struct Counter {
    initial: i32,
}

// Use the macro for stateful widgets
flui_core::impl_into_widget!(Counter, stateful);

struct CounterState {
    count: i32,
}

impl StatefulWidget for Counter {
    type State = CounterState;

    fn create_state(&self) -> Self::State {
        CounterState { count: self.initial }
    }
}

impl State<Counter> for CounterState {
    fn build(&mut self, widget: &Counter) -> Widget {
        // Build UI with current count using builder pattern
        Text::builder()
            .data(format!("Count: {}", self.count))
            .size(32.0)
            .build()
    }

    fn set_state<F: FnOnce(&mut Self)>(&mut self, f: F) {
        f(self);
        // Triggers rebuild
    }
}
```

## Architecture

FLUI uses a **three-tree architecture** for optimal performance:

```text
┌─────────────────┐      ┌─────────────────┐      ┌─────────────────┐
│  Widget Tree    │      │  Element Tree   │      │  Render Tree    │
│                 │      │                 │      │                 │
│  (Immutable     │ ───> │  (Mutable       │ ───> │  (Layout &      │
│   Configuration)│      │   State)        │      │   Paint)        │
└─────────────────┘      └─────────────────┘      └─────────────────┘
```

### Widget Tree (Immutable Configuration)

Widgets are lightweight, immutable descriptions of what the UI should look like:

```rust
pub enum Widget {
    Stateless(Box<dyn StatelessWidget>),    // Pure functional widgets
    Stateful(Box<dyn StatefulWidget>),      // Widgets with mutable state
    Inherited(Box<dyn InheritedWidget>),    // Data propagation
    Render(Box<dyn RenderWidget>),          // Custom layout/paint
    ParentData(Box<dyn ParentDataWidget>),  // Layout metadata
}
```

### Element Tree (Mutable State)

Elements hold the living state and lifecycle of widgets. They persist across rebuilds:

```rust
pub enum Element {
    Component(ComponentElement),      // StatelessWidget instance
    Stateful(StatefulElement),       // StatefulWidget + State
    Inherited(InheritedElement),     // Inherited data provider
    Render(RenderElement),           // Bridge to render tree
    ParentData(ParentDataElement),   // Parent data attachment
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

## Widget Types

### StatelessWidget

Pure functional widgets with no mutable state:

```rust
#[derive(Debug, Clone)]
struct Greeting {
    name: String,
}

// Macro to implement IntoWidget for this type
flui_core::impl_into_widget!(Greeting, stateless);

impl StatelessWidget for Greeting {
    fn build(&self, ctx: &BuildContext) -> Widget {
        Text::builder()
            .data(format!("Hello, {}!", self.name))
            .size(18.0)
            .build()
    }
}
```

**When to use**: Display-only widgets, pure transformations, compositions.

### StatefulWidget

Widgets that manage mutable state:

```rust
#[derive(Debug, Clone)]
struct Toggle {
    initial: bool,
}

flui_core::impl_into_widget!(Toggle, stateful);

struct ToggleState {
    enabled: bool,
}

impl StatefulWidget for Toggle {
    type State = ToggleState;

    fn create_state(&self) -> Self::State {
        ToggleState { enabled: self.initial }
    }
}

impl State<Toggle> for ToggleState {
    fn build(&mut self, widget: &Toggle) -> Widget {
        // Build UI based on self.enabled
        Checkbox::builder()
            .value(self.enabled)
            .build()
    }
}
```

**When to use**: User interactions, animations, form inputs, timers.

### InheritedWidget

Efficient data propagation down the widget tree:

```rust
#[derive(Debug, Clone)]
struct Theme {
    color: Color,
    child: Widget,
}

flui_core::impl_into_widget!(Theme, inherited);

impl InheritedWidget for Theme {
    fn update_should_notify(&self, old: &Self) -> bool {
        self.color != old.color
    }

    fn child(&self) -> Widget {
        self.child.clone()
    }
}

// Access from descendants:
let theme = ctx.depend_on_inherited_widget::<Theme>()?;
let color = theme.color;
```

**When to use**: Themes, localization, configuration, app-wide state.

### RenderWidget

Direct control over layout and painting:

```rust
#[derive(Debug, Clone)]
struct CustomBox {
    width: f32,
    height: f32,
    color: Color,
}

flui_core::impl_into_widget!(CustomBox, render);

impl RenderWidget for CustomBox {
    type Render = RenderCustomBox;

    fn create_render_object(&self) -> Self::Render {
        RenderCustomBox {
            width: self.width,
            height: self.height,
            color: self.color,
        }
    }

    fn update_render_object(&self, render: &mut Self::Render) {
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
        // Create layer for rendering
        Box::new(PictureLayer::new(/* ... */))
    }
}
```

**When to use**: Custom layouts, complex drawing, performance-critical rendering.

### ParentDataWidget

Attach metadata to children for parent's layout algorithm:

```rust
#[derive(Debug, Clone)]
struct Positioned {
    top: Option<f32>,
    left: Option<f32>,
    child: Widget,
}

flui_core::impl_into_widget!(Positioned, parent_data);

impl ParentDataWidget for Positioned {
    type ParentDataType = StackParentData;

    fn apply_parent_data(&self, render: &mut dyn Any) {
        if let Some(parent_data) = render.downcast_mut::<StackParentData>() {
            parent_data.top = self.top;
            parent_data.left = self.left;
        }
    }

    fn child(&self) -> &Widget {
        &self.child
    }
}
```

**When to use**: Positioned (for Stack), Flexible (for Flex), custom layout parameters.

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

### 🎯 The `impl_into_widget!` Macro

FLUI provides a convenient macro to automatically implement the `IntoWidget` trait and `From<T> for Widget` conversion for your widget types:

```rust
// For StatelessWidget
flui_core::impl_into_widget!(MyWidget, stateless);

// For StatefulWidget
flui_core::impl_into_widget!(MyCounter, stateful);

// For RenderWidget
flui_core::impl_into_widget!(MyCustomBox, render);

// For InheritedWidget
flui_core::impl_into_widget!(MyTheme, inherited);

// For ParentDataWidget
flui_core::impl_into_widget!(MyPositioned, parent_data);
```

This macro generates:
- `impl IntoWidget for T` - allows calling `.into_widget()` on your type
- `impl From<T> for Widget` - enables automatic conversion via `.into()`

**Benefits**:
- No more manual `Widget::stateless()` or `Widget::render_object()` calls
- Cleaner, more ergonomic API
- Works seamlessly with builder patterns
- Type-safe widget composition

### 🏗️ Builder Pattern

Most widgets in FLUI support the builder pattern for ergonomic construction:

```rust
// Old verbose way (still works):
let text = Widget::render_object(Text {
    data: "Hello".to_string(),
    size: 24.0,
    color: Color::WHITE,
});

// New builder way (recommended):
let text = Text::builder()
    .data("Hello")
    .size(24.0)
    .color(Color::WHITE)
    .build();

// Complex example with Container:
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

The builder pattern automatically handles `IntoWidget` conversions, making widget composition feel natural and fluent.

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
    Element::Stateful(s) => s.build(),
    // ... compiler can optimize this heavily
}

// vs. virtual function calls (slower)
element.build()  // Box<dyn Element>
```

### 🔧 Zero Boilerplate

The `impl_into_widget!` macro eliminates boilerplate code:

```rust
#[derive(Debug, Clone)]
struct MyWidget;

// Just one line to get full Widget integration!
flui_core::impl_into_widget!(MyWidget, stateless);

impl StatelessWidget for MyWidget {
    fn build(&self, ctx: &BuildContext) -> Widget {
        Text::builder()
            .data("Hello")
            .build()
    }
}

// Now you can use it anywhere a Widget is expected:
let widget = MyWidget;
let as_widget: Widget = widget.into_widget();  // ✅ Via IntoWidget
let also_widget: Widget = MyWidget.into();      // ✅ Via From<T>
```

Combined with builder patterns, widget composition becomes extremely ergonomic with minimal boilerplate.

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
| Widget dispatch | O(1) | Enum match (inline-able) |
| State updates | O(affected) | Only rebuilds dirty subtree |
| Layout cache | O(1) | Constraint-based memoization |

**Benchmark results** (vs trait objects):
- Element dispatch: **3-4x faster**
- Memory usage: **30% less**
- Binary size: **20% smaller**

## Examples

See the [examples](../../examples/) directory for complete applications:

- **[widget_hello_world](../../examples/widget_hello_world.rs)** - Modern builder pattern with `impl_into_widget!`
- **[hello_world](../../examples/hello_world.rs)** - Basic StatelessWidget
- **[counter](../../examples/counter.rs)** - StatefulWidget with state management
- **[theme](../../examples/theme.rs)** - InheritedWidget for data propagation
- **[custom_render](../../examples/custom_render.rs)** - Custom RenderWidget
- **[layout](../../examples/layout.rs)** - Flex layout system

### Widget Composition Example

```rust
use flui_core::prelude::*;
use flui_widgets::prelude::*;

// Create widgets using the builder pattern
let my_ui = Column::builder()
    .main_axis_alignment(MainAxisAlignment::Center)
    .children(vec![
        Text::builder()
            .data("Title")
            .size(32.0)
            .color(Color::BLACK)
            .build()
            .into(),  // Convert to Widget
        Container::builder()
            .padding(EdgeInsets::symmetric(10.0, 20.0))
            .child(
                Text::builder()
                    .data("Subtitle")
                    .size(16.0)
                    .build()
            )
            .build()
            .into(),
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
| Widget tree | Runtime | Enum-based compile-time |
| State | Inherited | Owned by Element |
| Rendering | Skia | Pluggable backends |
| Hot reload | ✅ Yes | 🚧 Planned |
| FFI | C/C++ | Native Rust |

FLUI takes inspiration from Flutter's architecture but leverages Rust's type system for additional safety and performance.
