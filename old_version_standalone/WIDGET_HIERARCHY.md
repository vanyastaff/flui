# Widget Hierarchy

Nebula-UI implements a Flutter-inspired widget hierarchy adapted for Rust and egui's immediate mode paradigm.

## Architecture Overview

```
Object (Rust's std types)
  └─ NebulaWidget (trait)
      ├─ StatelessWidget (no internal state)
      ├─ StatefulWidget (has internal state)
      └─ RenderObjectWidget (creates render objects)
          ├─ SingleChildWidget (one child)
          │   └─ Container, Align, Padding, Transform
          └─ MultiChildWidget (multiple children)
              └─ Row, Column, Stack, Flex
```

## Comparison with Flutter

| Flutter | Nebula-UI | Notes |
|---------|-----------|-------|
| `Object` | Rust's default | All Rust types inherit from default traits |
| `DiagnosticableTree` | `NebulaWidget::diagnostics()` | Debug information |
| `Widget` | `NebulaWidget` trait | Base widget trait |
| `Key` | `WidgetKey` | Widget identification |
| `StatelessWidget` | `StatelessWidget` trait | Immutable widgets |
| `StatefulWidget` | `StatefulWidget` trait | Mutable state widgets |
| `RenderObjectWidget` | `RenderObjectWidget` trait | Layout & rendering |
| `SingleChildRenderObjectWidget` | `SingleChildWidget` trait | One child |
| `MultiChildRenderObjectWidget` | `MultiChildWidget` trait | Multiple children |

## Base Traits

### `NebulaWidget`

The fundamental trait for all widgets.

```rust
use nebula_ui::widgets::{NebulaWidget, WidgetKey, WidgetDiagnostics};

pub trait NebulaWidget: Debug + 'static {
    fn key(&self) -> Option<WidgetKey>;
    fn diagnostics(&self) -> WidgetDiagnostics;
    fn can_update(&self, other: &dyn Any) -> bool;
}
```

**Key features:**
- Widget identification via `WidgetKey`
- Diagnostic information for debugging
- Update optimization via `can_update()`

### `StatelessWidget`

Widgets that don't manage internal state.

```rust
use nebula_ui::widgets::{StatelessWidget, NebulaWidget};

#[derive(Debug, Clone)]
struct MyButton {
    label: String,
}

impl NebulaWidget for MyButton {}
impl StatelessWidget for MyButton {}

impl egui::Widget for MyButton {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        ui.button(&self.label)
    }
}
```

**Examples:**
- `Text` - Display text
- `Icon` - Display icons
- `Spacer` - Empty space
- Simple `Container` without state

### `StatefulWidget`

Widgets with mutable internal state.

```rust
use nebula_ui::widgets::{StatefulWidget, NebulaWidget};

#[derive(Debug)]
struct Counter {
    count: i32,
}

impl NebulaWidget for Counter {}

impl StatefulWidget for Counter {
    type State = i32;

    fn state(&self) -> &Self::State {
        &self.count
    }

    fn state_mut(&mut self) -> &mut Self::State {
        &mut self.count
    }
}

impl egui::Widget for Counter {
    fn ui(mut self, ui: &mut egui::Ui) -> egui::Response {
        let response = ui.button(format!("Count: {}", self.count));
        if response.clicked() {
            *self.state_mut() += 1;
        }
        response
    }
}
```

**Examples:**
- `TextField` - Text input with cursor state
- `Checkbox` - Toggle state
- `Slider` - Value state
- Animated widgets

### `RenderObjectWidget`

Widgets that participate in layout and rendering.

```rust
use nebula_ui::widgets::{RenderObjectWidget, NebulaWidget, RenderConstraints};
use nebula_ui::types::core::{Size, Rect};

#[derive(Debug)]
struct MyLayoutWidget {
    constraints: Option<RenderConstraints>,
    size: Option<Size>,
}

impl NebulaWidget for MyLayoutWidget {}

impl RenderObjectWidget for MyLayoutWidget {
    fn constraints(&self) -> Option<RenderConstraints> {
        self.constraints
    }

    fn size(&self) -> Option<Size> {
        self.size
    }

    fn rect(&self) -> Option<Rect> {
        // Compute from size and position
        None
    }
}

impl egui::Widget for MyLayoutWidget {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        // Perform layout and rendering
        ui.allocate_response(
            egui::vec2(100.0, 100.0),
            egui::Sense::hover()
        )
    }
}
```

**Examples:**
- `Container` - Box model with decoration
- `Align` - Alignment layout
- `Padding` - Spacing layout
- `Transform` - Transformation layout

### `SingleChildWidget`

Render object widgets with exactly one child.

```rust
use nebula_ui::widgets::{SingleChildWidget, RenderObjectWidget, NebulaWidget};

#[derive(Debug)]
struct Padding {
    padding: f32,
    child: Box<dyn egui::Widget>,
}

impl NebulaWidget for Padding {}
impl RenderObjectWidget for Padding {}

impl SingleChildWidget for Padding {
    type Child = Box<dyn egui::Widget>;

    fn child(&self) -> Option<&Self::Child> {
        Some(&self.child)
    }
}
```

**Examples:**
- `Padding` - Add spacing around child
- `Align` - Position child within parent
- `Transform` - Apply transformations
- `Container` - Box decoration around child

### `MultiChildWidget`

Render object widgets with multiple children.

```rust
use nebula_ui::widgets::{MultiChildWidget, RenderObjectWidget, NebulaWidget};

#[derive(Debug)]
struct Row {
    children: Vec<Box<dyn egui::Widget>>,
}

impl NebulaWidget for Row {}
impl RenderObjectWidget for Row {}

impl MultiChildWidget for Row {
    type Child = Box<dyn egui::Widget>;

    fn children(&self) -> &[Self::Child] {
        &self.children
    }

    fn child_count(&self) -> usize {
        self.children.len()
    }
}
```

**Examples:**
- `Row` - Horizontal layout
- `Column` - Vertical layout
- `Stack` - Layered layout
- `Flex` - Flexible layout

## Widget Keys

Widget keys help identify and optimize widget updates.

```rust
use nebula_ui::widgets::WidgetKey;

// Create unique keys
let key1 = WidgetKey::new();
let key2 = WidgetKey::from_string("my_button");
let key3 = WidgetKey::from_value(42);

// Use keys in widgets
#[derive(Debug)]
struct MyWidget {
    key: Option<WidgetKey>,
}

impl NebulaWidget for MyWidget {
    fn key(&self) -> Option<WidgetKey> {
        self.key
    }
}
```

## Render Constraints

Render constraints define layout boundaries.

```rust
use nebula_ui::widgets::RenderConstraints;
use nebula_ui::types::core::Size;

// Tight constraints (fixed size)
let tight = RenderConstraints::tight(Size::new(100.0, 50.0));
assert!(tight.is_tight());

// Loose constraints (max size)
let loose = RenderConstraints::loose(Size::new(200.0, 100.0));
assert!(!loose.is_tight());

// Unbounded constraints
let unbounded = RenderConstraints::unbounded();
assert!(!unbounded.has_bounded_width());

// Constrain a size
let size = Size::new(150.0, 150.0);
let constrained = loose.constrain(size);
// constrained will be clamped to (150.0, 100.0)
```

## Diagnostics

Debug widgets using diagnostic information.

```rust
use nebula_ui::widgets::{NebulaWidget, WidgetDiagnostics, DiagnosticProperty};

impl NebulaWidget for MyWidget {
    fn diagnostics(&self) -> WidgetDiagnostics {
        WidgetDiagnostics {
            type_name: "MyWidget",
            key: self.key(),
            properties: vec![
                DiagnosticProperty::new("width", self.width),
                DiagnosticProperty::new("height", self.height),
                DiagnosticProperty::new("enabled", self.enabled),
            ],
        }
    }
}

// Print diagnostics
let widget = MyWidget::new();
let diag = widget.diagnostics();
println!("{}: {:?}", diag.type_name, diag.properties);
```

## Integration with egui

All nebula-ui widgets must implement `egui::Widget`:

```rust
// Nebula-UI widget hierarchy
impl NebulaWidget for MyWidget {}        // Base trait
impl StatelessWidget for MyWidget {}     // Stateless marker

// egui integration
impl egui::Widget for MyWidget {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        // Render using egui
        ui.label("Hello")
    }
}
```

## Best Practices

### 1. Choose the Right Base Trait

- **StatelessWidget**: Immutable data, no state management
  - Text, Icon, Image, Spacer

- **StatefulWidget**: Mutable state within widget
  - TextField, Checkbox, Slider, Counter

- **RenderObjectWidget**: Custom layout/rendering
  - Container, Padding, Transform, Row, Column

### 2. Use Widget Keys for Optimization

```rust
// Without key: widget recreated every frame
Container::new().with_child(...)

// With key: widget can be updated in place
Container::new()
    .with_key(WidgetKey::from_string("main_container"))
    .with_child(...)
```

### 3. Implement Diagnostics for Debugging

```rust
impl NebulaWidget for MyWidget {
    fn diagnostics(&self) -> WidgetDiagnostics {
        WidgetDiagnostics {
            type_name: std::any::type_name::<Self>(),
            key: self.key(),
            properties: vec![
                DiagnosticProperty::new("visible", self.visible),
                DiagnosticProperty::new("size", format!("{:?}", self.size)),
            ],
        }
    }
}
```

### 4. Follow Flutter's Widget Patterns

```rust
// Builder pattern
MyWidget::new()
    .with_size(100.0, 50.0)
    .with_color(Color::BLUE)
    .with_child(...)

// Factory methods
Container::from_color(Color::RED)
Container::with_decoration(decoration)
```

## Future Enhancements

Planned additions to the widget system:

1. **InheritedWidget** - Share data down the widget tree
2. **BuildContext** - Widget tree context and navigation
3. **Element Tree** - Separate widget tree from element tree
4. **Widget Inspector** - Visual debugging tool
5. **Hot Reload** - Update widgets without restart

## Examples

See the examples directory:

- `examples/stateless_widget_demo.rs` - StatelessWidget examples
- `examples/stateful_widget_demo.rs` - StatefulWidget examples
- `examples/custom_render_object.rs` - RenderObjectWidget examples
- `examples/widget_keys_demo.rs` - Widget key optimization

## Testing

All widget base traits include comprehensive tests:

```bash
# Run widget base tests
cargo test --lib widgets::base

# Run all widget tests
cargo test --lib widgets
```

Current test coverage:
- `base::widget_key`: 1 test
- `base::render_constraints`: 4 tests
- `base::diagnostic_property`: 1 test

Total: **551 tests passing** in nebula-ui (including widget base)

## References

- [Flutter Widget Class](https://api.flutter.dev/flutter/widgets/Widget-class.html)
- [Flutter StatelessWidget](https://api.flutter.dev/flutter/widgets/StatelessWidget-class.html)
- [Flutter StatefulWidget](https://api.flutter.dev/flutter/widgets/StatefulWidget-class.html)
- [Flutter RenderObjectWidget](https://api.flutter.dev/flutter/widgets/RenderObjectWidget-class.html)
- [Flutter Widget Catalog](https://docs.flutter.dev/ui/widgets)
