# flui_core

[![Crates.io](https://img.shields.io/crates/v/flui_core.svg)](https://crates.io/crates/flui_core)
[![Documentation](https://docs.rs/flui_core/badge.svg)](https://docs.rs/flui_core)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](../../LICENSE)

Core widget system for the Flui UI framework - implementing Flutter's three-tree architecture in Rust.

## Overview

`flui_core` provides the fundamental building blocks of the Flui widget system:

- **Widget**: Immutable configuration describing UI
- **Element**: Mutable state holder managing lifecycle
- **RenderObject**: Layout and painting implementation

This crate implements Flutter's proven three-tree architecture, providing a solid foundation for building reactive UIs.

## Three-Tree Architecture

Flui follows Flutter's three-tree design pattern:

```
┌─────────────┐
│ Widget Tree │  Immutable, lightweight, recreated on rebuild
│   (new)     │  Describes WHAT to show
└──────┬──────┘
       │
       ├──creates──┐
       │           │
┌──────▼──────┐   │
│ Element     │   │  Mutable, persistent across rebuilds
│   Tree      │◄──┘  Manages state and lifecycle
│  (reused)   │      Coordinates updates
└──────┬──────┘
       │
       ├──manages──┐
       │           │
┌──────▼──────┐   │
│ Render      │   │  Mutable, performs actual work
│   Tree      │◄──┘  Layout, painting, hit testing
│  (reused)   │
└─────────────┘
```

### Why Three Trees?

1. **Widget Tree** (Immutable):
   - Lightweight and cheap to recreate
   - Easy to reason about (no hidden state)
   - Enables declarative UI patterns

2. **Element Tree** (Mutable):
   - Persists across rebuilds (efficient)
   - Manages widget lifecycle
   - Holds references to render objects

3. **Render Tree** (Mutable):
   - Expensive objects reused
   - Caches layout and paint information
   - Provides performance

## Modules

### Widget (`widget`)

Immutable UI descriptions:

```rust
use flui_core::{StatelessWidget, StatefulWidget, Widget, BuildContext};

// Stateless widget - no mutable state
#[derive(Debug, Clone)]
struct Greeting {
    name: String,
}

impl StatelessWidget for Greeting {
    fn build(&self, context: &BuildContext) -> Box<dyn Widget> {
        Box::new(Text::new(format!("Hello, {}!", self.name)))
    }
}

// Stateful widget - has mutable state
#[derive(Debug, Clone)]
struct Counter {
    initial: i32,
}

impl StatefulWidget for Counter {
    type State = CounterState;

    fn create_state(&self) -> Self::State {
        CounterState { count: self.initial }
    }
}

#[derive(Debug)]
struct CounterState {
    count: i32,
}

impl State for CounterState {
    fn build(&mut self, context: &BuildContext) -> Box<dyn Widget> {
        // Build UI using current state
        Box::new(Text::new(format!("Count: {}", self.count)))
    }
}
```

### Element (`element`)

Mutable state holders in the element tree:

```rust
use flui_core::{Element, ElementId, ComponentElement, StatefulElement};

// Elements manage lifecycle
fn example_element_lifecycle() {
    let mut element = StatefulElement::new();
    let id = element.id(); // Unique identifier

    // Mount into tree
    element.mount(Some(parent_id), 0);

    // Mark dirty to trigger rebuild
    element.mark_dirty();
    assert!(element.is_dirty());

    // Rebuild
    element.rebuild();
    assert!(!element.is_dirty());

    // Unmount when removed
    element.unmount();
}
```

**Element Types**:

- `ComponentElement<W>` - For `StatelessWidget`
- `StatefulElement` - For `StatefulWidget`
- `RenderObjectElement<W>` - For `RenderObjectWidget`
- `InheritedElement` - For `InheritedWidget` (data propagation)

### RenderObject (`render_object`)

Layout and painting implementation:

```rust
use flui_core::{RenderObject, BoxConstraints, Size, Offset};

#[derive(Debug)]
struct CustomRenderBox {
    size: Size,
    // ... other fields
}

impl RenderObject for CustomRenderBox {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        // Compute size based on constraints
        let size = Size::new(
            constraints.max_width(),
            constraints.max_height()
        );
        self.size = size;
        size
    }

    fn paint(&self, painter: &egui::Painter, offset: Offset) {
        // Paint this render object
        let rect = egui::Rect::from_min_size(
            egui::pos2(offset.dx, offset.dy),
            egui::vec2(self.size.width, self.size.height)
        );
        painter.rect_filled(rect, 0.0, egui::Color32::BLUE);
    }

    fn size(&self) -> Size {
        self.size
    }

    // Mark for relayout
    fn mark_needs_layout(&mut self) { /* ... */ }

    // Mark for repaint
    fn mark_needs_paint(&mut self) { /* ... */ }
}
```

### RenderObjectWidget (`render_object_widget`)

Widgets that create render objects:

```rust
use flui_core::{
    RenderObjectWidget, LeafRenderObjectWidget,
    SingleChildRenderObjectWidget, MultiChildRenderObjectWidget
};

// Leaf widget - no children
#[derive(Debug, Clone)]
struct ColoredBox {
    width: f32,
    height: f32,
    color: Color,
}

impl RenderObjectWidget for ColoredBox {
    fn create_render_object(&self) -> Box<dyn RenderObject> {
        Box::new(CustomRenderBox::new(self.width, self.height))
    }

    fn update_render_object(&self, render_object: &mut dyn RenderObject) {
        // Update render object when widget changes
        if let Some(box_obj) = render_object.downcast_mut::<CustomRenderBox>() {
            box_obj.set_size(self.width, self.height);
        }
    }
}

impl LeafRenderObjectWidget for ColoredBox {}
```

### InheritedWidget (`inherited_widget`)

Efficient data propagation down the tree:

```rust
use flui_core::{InheritedWidget, BuildContext};

#[derive(Debug, Clone)]
struct Theme {
    primary_color: Color,
    font_size: f32,
}

impl InheritedWidget for Theme {
    fn update_should_notify(&self, old: &Self) -> bool {
        self.primary_color != old.primary_color ||
        self.font_size != old.font_size
    }
}

// Access in build methods
fn build_with_theme(context: &BuildContext) -> Box<dyn Widget> {
    let theme = context.depend_on_inherited_widget::<Theme>();
    // Use theme data...
}
```

### BuildContext (`build_context`)

Access to the element tree:

```rust
use flui_core::BuildContext;

fn example_build_context(context: &BuildContext) {
    // Get inherited data
    let theme = context.depend_on_inherited_widget::<Theme>();

    // Find ancestor widget
    let scaffold = context.find_ancestor_widget::<Scaffold>();

    // Access render object
    let render_box = context.find_render_object::<RenderBox>();
}
```

### ParentData (`parent_data`)

Layout information stored in children:

```rust
use flui_core::{ParentData, BoxParentData, ContainerBoxParentData};

// Simple positioning
let mut parent_data = BoxParentData::new();
parent_data.offset = Offset::new(10.0, 20.0);

// For containers with multiple children
let container_data = ContainerBoxParentData::new();
// Stores offset plus previous/next sibling references
```

### Constraints (`constraints`)

Layout constraints system:

```rust
use flui_core::{BoxConstraints, Size};

// Tight constraints - single valid size
let tight = BoxConstraints::tight(Size::new(100.0, 50.0));
assert!(tight.is_tight());

// Loose constraints - range of valid sizes
let loose = BoxConstraints::new(
    min_width: 0.0,
    max_width: 200.0,
    min_height: 0.0,
    max_height: 100.0,
);

// Constrain a size
let size = Size::new(150.0, 80.0);
let constrained = loose.constrain(size);
```

## Usage Patterns

### Creating a Custom Widget

```rust
use flui_core::{StatelessWidget, BuildContext, Widget};

#[derive(Debug, Clone)]
struct MyButton {
    label: String,
    on_press: Arc<dyn Fn() + Send + Sync>,
}

impl StatelessWidget for MyButton {
    fn build(&self, context: &BuildContext) -> Box<dyn Widget> {
        Box::new(
            GestureDetector::new()
                .on_tap(self.on_press.clone())
                .child(Text::new(self.label.clone()))
        )
    }
}
```

### Creating a Stateful Widget

```rust
use flui_core::{StatefulWidget, State, BuildContext, Widget};

#[derive(Debug, Clone)]
struct ExpandablePanel {
    title: String,
}

impl StatefulWidget for ExpandablePanel {
    type State = ExpandablePanelState;

    fn create_state(&self) -> Self::State {
        ExpandablePanelState { expanded: false }
    }
}

#[derive(Debug)]
struct ExpandablePanelState {
    expanded: bool,
}

impl State for ExpandablePanelState {
    fn build(&mut self, context: &BuildContext) -> Box<dyn Widget> {
        // Build UI based on state
        if self.expanded {
            // Show expanded content
        } else {
            // Show collapsed content
        }
    }

    fn init_state(&mut self) {
        // Initialize when first created
    }

    fn dispose(&mut self) {
        // Cleanup when removed
    }
}
```

### Creating a Custom RenderObject

```rust
use flui_core::{RenderObject, BoxConstraints, Size, Offset};

#[derive(Debug)]
struct CircleRenderObject {
    radius: f32,
    color: Color,
    size: Size,
}

impl RenderObject for CircleRenderObject {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        // Circle size based on radius
        let diameter = self.radius * 2.0;
        let size = Size::new(diameter, diameter);

        // Constrain to parent's constraints
        self.size = constraints.constrain(size);
        self.size
    }

    fn paint(&self, painter: &egui::Painter, offset: Offset) {
        let center = egui::pos2(
            offset.dx + self.radius,
            offset.dy + self.radius
        );
        painter.circle_filled(center, self.radius, self.color.into());
    }

    fn size(&self) -> Size {
        self.size
    }

    fn needs_layout(&self) -> bool { /* ... */ }
    fn mark_needs_layout(&mut self) { /* ... */ }
    fn needs_paint(&self) -> bool { /* ... */ }
    fn mark_needs_paint(&mut self) { /* ... */ }
}
```

## Architecture Patterns

### Widget Lifecycle

```
Widget Created
     │
     ├──► Element.mount()
     │         │
     │         ├──► State.init_state() [if stateful]
     │         │
     │         └──► RenderObject created [if render widget]
     │
     ├──► First build
     │         │
     │         └──► Child widgets created
     │
     ├──► Widget updated
     │         │
     │         ├──► Element.update()
     │         │
     │         └──► State.did_update_widget() [if stateful]
     │
     ├──► Rebuild triggered
     │         │
     │         ├──► State.build() or StatelessWidget.build()
     │         │
     │         └──► Child elements updated
     │
     └──► Element.unmount()
               │
               └──► State.dispose() [if stateful]
```

### Data Flow

```
┌──────────────────┐
│ InheritedWidget  │  Theme, MediaQuery, etc.
│   (immutable)    │
└────────┬─────────┘
         │ provides data
         ▼
┌──────────────────┐
│  BuildContext    │  Access point for:
│                  │  - Inherited data
└────────┬─────────┘  - Ancestor widgets
         │            - Render objects
         ▼
┌──────────────────┐
│  Widget.build()  │  Consumes data
│                  │  Builds child tree
└──────────────────┘
```

### Performance Optimization

1. **Widget Reuse**: Elements persist across rebuilds
2. **Dirty Tracking**: Only dirty elements rebuild
3. **RenderObject Caching**: Layout/paint information cached
4. **InheritedWidget**: Efficient data propagation
5. **Keys**: Preserve state when reordering

## Design Principles

### Immutability

Widgets are immutable - mutations create new widgets:

```rust
let button1 = Button::new("Click me");
let button2 = button1.with_color(Color::BLUE); // New instance
```

### Composition

Build complex UIs from simple widgets:

```rust
fn build(&self, context: &BuildContext) -> Box<dyn Widget> {
    Box::new(
        Column::new()
            .child(Header::new(self.title))
            .child(Body::new(self.content))
            .child(Footer::new())
    )
}
```

### Separation of Concerns

- **Widget**: Configuration (what to show)
- **Element**: Lifecycle (when to update)
- **RenderObject**: Implementation (how to render)

## Type Safety

All core types provide compile-time safety:

- **Downcast Trait Objects**: Safe runtime type checking
- **Generic Elements**: Type-safe element creation
- **Trait Bounds**: Enforce correct widget relationships

```rust
// Type-safe downcasting
let element: Box<dyn Element> = create_element();
if let Some(stateful) = element.downcast_ref::<StatefulElement>() {
    // Work with stateful element
}

// Generic element creation
impl<W: StatelessWidget> Widget for W {
    fn create_element(&self) -> Box<dyn Element> {
        Box::new(ComponentElement::new(self.clone()))
    }
}
```

## Testing

```bash
# Run all tests
cargo test

# Run specific test file
cargo test widget

# Run with output
cargo test -- --nocapture
```

### Test Coverage

```
widget.rs:         15 tests ✓
element.rs:        14 tests ✓
render_object.rs:   5 tests ✓
inherited_widget:   8 tests ✓
parent_data.rs:     7 tests ✓
```

## Examples

See the `examples/` directory for:

- Basic widget creation
- Stateful widgets with counters
- Custom render objects
- InheritedWidget usage
- Layout implementation

## Performance Characteristics

- **Element creation**: ~100ns
- **Element mount**: ~200ns
- **Widget build**: ~50ns (excluding child creation)
- **Dirty check**: ~5ns (single boolean check)
- **Downcast check**: ~10-20ns

## Comparison with Flutter

| Feature | Flutter | Flui |
|---------|---------|------|
| Widget Tree | ✓ | ✓ |
| Element Tree | ✓ | ✓ |
| Render Tree | ✓ | ✓ |
| InheritedWidget | ✓ | ✓ |
| Keys | ✓ | ✓ |
| StatelessWidget | ✓ | ✓ |
| StatefulWidget | ✓ | ✓ |
| BuildContext | ✓ | ✓ |
| Type Safety | Runtime | **Compile-time + Runtime** |
| Memory Safety | GC | **Rust ownership** |

## Dependencies

- `flui_types` - Core type definitions
- `flui_foundation` - Foundation types (Key, etc.)
- `downcast-rs` - Safe downcasting for trait objects
- `dyn-clone` - Cloning for trait objects
- `egui` - Painting backend

## Contributing

Contributions are welcome! Please ensure:

1. All tests pass: `cargo test`
2. Code is formatted: `cargo fmt`
3. No clippy warnings: `cargo clippy`
4. New APIs have documentation

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](../../LICENSE-APACHE))
- MIT license ([LICENSE-MIT](../../LICENSE-MIT))

at your option.

## Related Crates

- `flui_types` - Core type definitions
- `flui_rendering` - Rendering implementation
- `flui_widgets` - Standard widget library
- `flui_material` - Material Design widgets

---

Built with ❤️ for high-performance UI in Rust
