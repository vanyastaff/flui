# Styling Types

Visual styling system for nebula-ui, providing Flutter-inspired decoration capabilities with full egui integration.

## Overview

The styling module provides comprehensive visual styling types for decorating UI elements. All types use idiomatic Rust patterns with `impl Into<T>` for maximum flexibility and seamless integration with core types.

## Available Types

### Colors and Shadows

- **[`Color`]** - RGBA color with conversions (in `core` module)
- **[`Shadow`]** - Drop shadow with color, offset, and blur
- **[`BoxShadow`]** - Material Design elevation shadows

### Borders

- **[`BorderStyle`]** - Border line style (Solid, None)
- **[`BorderSide`]** - Single border side with color and width
- **[`Border`]** - Four-sided border configuration
- **[`Radius`]** - Circular or elliptical corner radius
- **[`BorderRadius`]** - Four-corner radius configuration

### Gradients

- **[`TileMode`]** - How gradients tile (Clamp, Repeat, Mirror, Decal)
- **[`GradientStop`]** - Color at a position (0.0 to 1.0)
- **[`LinearGradient`]** - Linear color interpolation
- **[`RadialGradient`]** - Radial color interpolation
- **[`SweepGradient`]** - Angular sweep interpolation
- **[`Gradient`]** - Unified gradient enum

### Decorations

- **[`BoxDecoration`]** - Complete box styling (color, gradient, border, shadows, image)
- **[`ShapeDecoration`]** - Simplified shape styling
- **[`DecorationPresets`]** - Common decoration patterns (card, button, outlined, circle, pill)

### Clipping

- **[`Clip`]** - Clipping behavior (None, HardEdge, AntiAlias, AntiAliasWithSaveLayer)

## Quick Examples

### Simple Color Border

```rust
use nebula_ui::types::{
    core::Color,
    styling::{Border, BorderRadius},
};

// All-sided border
let border = Border::uniform(Color::BLACK, 2.0);

// With rounded corners
let decoration = BoxDecoration::new()
    .with_border(border)
    .with_border_radius(BorderRadius::circular(8.0));
```

### Gradient Background

```rust
use nebula_ui::types::{
    core::{Color, Point},
    styling::{BoxDecoration, LinearGradient, Gradient},
};

// Horizontal gradient
let gradient = LinearGradient::horizontal(Color::RED, Color::BLUE);

let decoration = BoxDecoration::new()
    .with_gradient(Gradient::Linear(gradient));

// Or use helper methods
let vertical = LinearGradient::vertical(Color::from_rgb(255, 100, 50), Color::WHITE);
let diagonal = LinearGradient::diagonal(Color::GREEN, Color::YELLOW);
```

### Material Design Shadows

```rust
use nebula_ui::types::{
    core::{Color, Offset},
    styling::{BoxShadow, BoxDecoration},
};

// Elevation-based shadows (Material Design)
let (key_shadow, ambient_shadow) = BoxShadow::elevation_shadows(4.0);

let decoration = BoxDecoration::new()
    .with_color(Color::WHITE)
    .with_shadows(vec![key_shadow, ambient_shadow]);

// Or single shadow
let simple = BoxShadow::simple(
    Color::from_rgba(0, 0, 0, 50),
    Offset::new(0.0, 2.0),
    4.0,
);
```

### Complex Decoration

```rust
use nebula_ui::types::{
    core::Color,
    styling::{BoxDecoration, Border, BorderRadius, BoxShadow},
};

let decoration = BoxDecoration::new()
    .with_color(Color::WHITE)
    .with_border(Border::uniform(Color::GRAY, 1.0))
    .with_border_radius(BorderRadius::circular(12.0))
    .with_shadow(BoxShadow::elevation(4.0, Color::from_rgba(0, 0, 0, 50)));
```

### Radial Gradient

```rust
use nebula_ui::types::{
    core::{Color, Point},
    styling::{RadialGradient, Gradient},
};

// Centered circular gradient
let gradient = RadialGradient::circle(Color::YELLOW, Color::RED);

// Custom center and radius
let custom = RadialGradient::two_colors(
    Point::new(0.3, 0.3),  // Center at 30%, 30%
    0.7,                    // Radius
    Color::WHITE,
    Color::BLUE,
);
```

### Sweep Gradient (Conic)

```rust
use nebula_ui::types::{
    core::{Color, Point},
    styling::SweepGradient,
};

// Full circle sweep
let sweep = SweepGradient::centered(Color::RED, Color::BLUE);

// Rainbow sweep
let rainbow = SweepGradient::rainbow(Point::new(0.5, 0.5));
```

### Border Variations

```rust
use nebula_ui::types::{
    core::Color,
    styling::{Border, BorderSide},
};

// Different sides
let border = Border::new(
    BorderSide::solid(Color::RED, 1.0),    // top
    BorderSide::solid(Color::GREEN, 2.0),  // right
    BorderSide::solid(Color::BLUE, 3.0),   // bottom
    BorderSide::solid(Color::YELLOW, 4.0), // left
);

// Symmetric
let symmetric = Border::symmetric(
    BorderSide::solid(Color::BLACK, 2.0),  // top & bottom
    BorderSide::solid(Color::GRAY, 1.0),   // left & right
);

// Only one side
let bottom_only = Border::only_bottom(BorderSide::solid(Color::RED, 2.0));
```

### Border Radius Variations

```rust
use nebula_ui::types::styling::{BorderRadius, Radius};

// All corners same
let circular = BorderRadius::circular(8.0);

// Elliptical corners
let elliptical = BorderRadius::all(Radius::elliptical(16.0, 8.0));

// Top corners only
let top_rounded = BorderRadius::vertical_top(Radius::circular(12.0));

// Individual corners
let custom = BorderRadius::new(
    Radius::circular(8.0),   // top-left
    Radius::circular(16.0),  // top-right
    Radius::circular(4.0),   // bottom-left
    Radius::ZERO,            // bottom-right
);
```

## Decoration Presets

Common patterns for quick styling:

```rust
use nebula_ui::types::{
    core::Color,
    styling::DecorationPresets,
};

// Material Design card
let card = DecorationPresets::card(4.0);  // elevation

// Button with elevation
let button = DecorationPresets::button(Color::BLUE, 2.0);

// Outlined (border only)
let outlined = DecorationPresets::outlined(Color::BLACK, 1.0);

// Circular (pill with max radius)
let circle = DecorationPresets::circle(Color::RED);

// Pill shape (fully rounded)
let pill = DecorationPresets::pill(Color::GREEN);
```

## Integration with Core Types

All styling types seamlessly integrate with core types using `impl Into<T>`:

```rust
use nebula_ui::types::{
    core::{Color, Point, Offset, Size},
    styling::*,
};

// Accept Color directly or from conversions
let border = BorderSide::solid(Color::RED, 2.0);
let border_rgb = BorderSide::solid((255, 0, 0), 2.0);  // From (u8, u8, u8)

// Points for gradients
let gradient = LinearGradient::two_colors(
    Point::new(0.0, 0.0),
    Point::new(1.0, 1.0),
    Color::RED,
    Color::BLUE,
);

// Offsets for shadows
let shadow = Shadow::new(
    Color::from_rgba(0, 0, 0, 50),
    Offset::new(2.0, 2.0),
    4.0,
);
```

## Design Principles

### 1. **Type Safety Through Semantics**

Different types for different purposes prevent errors:
- `Color` for all color values
- `Point` for gradient positions (unit square 0.0-1.0)
- `Offset` for shadow displacement
- `Size` for dimensions

### 2. **Idiomatic Rust**

All types use idiomatic patterns:
- `impl Into<T>` for flexible APIs
- `From/Into` traits for conversions
- Builder pattern with `with_*` methods
- Zero-cost abstractions through inlining

### 3. **Flutter-Inspired**

API design follows Flutter conventions:
- `BoxDecoration` similar to Flutter's BoxDecoration
- `LinearGradient`, `RadialGradient`, `SweepGradient`
- `BorderRadius` with elliptical support
- Material Design elevation shadows

### 4. **Composability**

All decorations are composable:

```rust
let base = BoxDecoration::new()
    .with_color(Color::WHITE)
    .with_border_radius(BorderRadius::circular(8.0));

// Add shadow later
let with_shadow = base.clone()
    .with_shadow(BoxShadow::elevation(4.0, Color::BLACK));

// Or gradient
let with_gradient = base.clone()
    .with_gradient(Gradient::Linear(
        LinearGradient::horizontal(Color::RED, Color::BLUE)
    ));
```

## Advanced Patterns

### Conditional Styling

```rust
fn styled_box(is_elevated: bool, color: impl Into<Color>) -> BoxDecoration {
    let mut decoration = BoxDecoration::new()
        .with_color(color)
        .with_border_radius(BorderRadius::circular(8.0));

    if is_elevated {
        let (key, ambient) = BoxShadow::elevation_shadows(4.0);
        decoration = decoration.with_shadows(vec![key, ambient]);
    }

    decoration
}
```

### Animation-Ready

```rust
use nebula_ui::types::{
    core::Color,
    styling::{BoxDecoration, BorderRadius},
};

// Interpolate between decorations
fn lerp_decoration(a: &BoxDecoration, b: &BoxDecoration, t: f32) -> BoxDecoration {
    let color = if let (Some(ca), Some(cb)) = (a.color, b.color) {
        Some(ca.lerp(cb, t))
    } else {
        a.color.or(b.color)
    };

    // Interpolate other properties...
    BoxDecoration {
        color,
        ..a.clone()
    }
}
```

### Scaling

All styling types support scaling:

```rust
let decoration = BoxDecoration::new()
    .with_border(Border::uniform(Color::BLACK, 2.0))
    .with_border_radius(BorderRadius::circular(8.0))
    .with_shadow(BoxShadow::simple(
        Color::BLACK,
        Offset::new(2.0, 2.0),
        4.0,
    ));

// Scale by 2x (for high DPI, zoom, etc.)
let scaled = decoration.scale(2.0);
// Border width: 4.0
// Border radius: 16.0
// Shadow blur: 8.0
// Shadow offset: 4.0, 4.0
```

## Testing

All styling types have comprehensive tests:

```bash
# Test all styling types
cargo test --lib --package nebula-ui types::styling

# Test specific types
cargo test --lib --package nebula-ui types::styling::border
cargo test --lib --package nebula-ui types::styling::gradient
cargo test --lib --package nebula-ui types::styling::shadow
```

## Performance Notes

- All types are `Copy` or `Clone` where appropriate
- `impl Into<T>` has zero runtime cost (monomorphization + inlining)
- Gradients use `Vec<GradientStop>` for flexibility
- Shadows use `Vec<BoxShadow>` for multiple shadows
- All arithmetic operations are simple and fast

## See Also

- **[Core Types](../core/README.md)** - Fundamental geometric and color types
- **[Layout Types](../layout/README.md)** - Layout and positioning types
- **[Typography](../typography/)** - Text styling types
- **[Interaction](../interaction/)** - Interactive state types

## Examples in the Wild

Check the example files for complete usage:
- `examples/comprehensive_demo.rs` - Full showcase
- `examples/demo.rs` - Basic usage
- `examples/extended_demo.rs` - Advanced patterns
