# flui_widgets

High-level, composable UI widgets for Flui - Flutter-inspired UI framework in Rust.

## Overview

`flui_widgets` provides the widget layer of Flui's three-tree architecture:
- **Widgets** (this crate): Immutable configuration objects that describe UI
- **Elements** (`flui_core`): Mutable widget lifecycle managers
- **RenderObjects** (`flui_rendering`): Layout and painting primitives

This crate contains user-facing widgets that developers interact with to build UIs.

## Available Widgets

### Layout Widgets
- **Container**: A convenience widget combining sizing, padding, decoration, and constraints

### Coming Soon (Week 5-6)
- Row, Column: Flex layouts
- Stack: Layered layouts
- SizedBox, Padding, Center, Align: Basic positioning
- Expanded, Flexible: Flex children

## Usage

All widgets support three creation syntaxes:

### 1. Struct Literal (Flutter-like)
```rust
use flui_widgets::Container;

let widget = Container {
    width: Some(300.0),
    height: Some(200.0),
    padding: Some(EdgeInsets::all(20.0)),
    color: Some(Color::rgb(255, 0, 0)),
    ..Default::default()
};
```

### 2. Builder Pattern (Type-safe with bon)
```rust
use flui_widgets::Container;

let widget = Container::builder()
    .width(300.0)
    .height(200.0)
    .padding(EdgeInsets::all(20.0))
    .color(Color::rgb(255, 0, 0))
    .build();
```

### 3. Declarative Macro
```rust
use flui_widgets::container;

let widget = container! {
    width: 300.0,
    height: 200.0,
    padding: EdgeInsets::all(20.0),
    color: Color::rgb(255, 0, 0),
};
```

## Features

- **Type-safe builders**: Powered by [bon](https://github.com/elastio/bon) for ergonomic, compile-time checked builders
- **Multiple syntaxes**: Choose the style that fits your use case
- **Automatic conversions**: `.into()` support for common types (String, EdgeInsets, Color, etc.)
- **Validation**: Built-in configuration validation
- **Well-tested**: Comprehensive test coverage for all widgets
- **Well-documented**: Examples and usage patterns for every widget

## For Contributors

### Creating New Widgets

See [WIDGET_GUIDELINES.md](./WIDGET_GUIDELINES.md) for comprehensive documentation on:
- Standard widget structure
- bon builder configuration
- Custom setters and finishing functions
- Macro implementation
- Testing requirements
- Documentation standards

### Quick Start Template

Use [WIDGET_TEMPLATE.rs](./WIDGET_TEMPLATE.rs) as a starting point for new widgets. It includes:
- Complete struct definition with bon builder
- Standard implementations (new, Default, Widget trait)
- Custom builder extensions
- Declarative macro
- Full test suite

### Example: Container Widget

See [container.rs](./src/container.rs) for a complete reference implementation following all guidelines.

## Architecture

Widgets in Flui are immutable configuration objects that:
1. Store widget properties (size, color, padding, etc.)
2. Implement the `Widget` trait from `flui_core`
3. Create `Element` instances that manage widget lifecycle
4. Eventually build `RenderObject` trees for layout and painting

Currently, the Element system is under development, so widgets use `todo!()` placeholders for `create_element()`.

## Dependencies

- `flui_core`: Widget and Element abstractions
- `flui_rendering`: RenderObject implementations
- `flui_types`: Common types (Color, EdgeInsets, Alignment, etc.)
- `bon`: Builder pattern macro

## Testing

Run all widget tests:
```bash
cargo test -p flui_widgets
```

Run tests for all workspace crates:
```bash
cargo test --workspace
```

## License

This project is part of the Flui framework.

## Roadmap

Week 5-6 focuses on implementing core layout widgets:

**Week 5** (6 widgets):
- Container ✅
- Row
- Column
- SizedBox
- Padding
- Center
- Align

**Week 6** (10 widgets):
- Expanded
- Flexible
- Stack
- Positioned
- Wrap
- ListView (basic)
- GridView (basic)
- AspectRatio
- FittedBox
- ConstrainedBox

See [ROADMAP_NEXT.md](../../ROADMAP_NEXT.md) for detailed plans.
