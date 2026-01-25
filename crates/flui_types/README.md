# flui_types

[![Crates.io](https://img.shields.io/crates/v/flui_types.svg)](https://crates.io/crates/flui_types)
[![Documentation](https://docs.rs/flui_types/badge.svg)](https://docs.rs/flui_types)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](../../LICENSE)

Core type definitions for the FLUI UI framework. This crate provides all the fundamental types you need to build user interfaces: geometry, layout, colors, animations, physics, and more.

## Installation

```toml
[dependencies]
flui_types = "0.1"
```

### Optional Features

```toml
# Enable SIMD acceleration (3-4x faster for math operations)
flui_types = { version = "0.1", features = ["simd"] }

# Enable serialization with serde
flui_types = { version = "0.1", features = ["serde"] }
```

## Usage

```rust
use flui_types::prelude::*;

// Geometry
let size = Size::new(300.0, 200.0);
let rect = Rect::from_xywh(10.0, 20.0, size.width, size.height);
let center = rect.center();

// Layout
let padding = Edges::all(px(16.0));
let alignment = Alignment::CENTER;

// Colors
let primary = Color::rgb(66, 133, 244);
let faded = primary.with_opacity(0.5);

// Animation curves
let progress = Curve::EaseInOut.transform(0.5);
```

## What's Included

### Geometry

Position and transform your UI elements:

```rust
use flui_types::geometry::*;

let point = Point::new(100.0, 200.0);
let offset = Offset::new(10.0, 20.0);
let size = Size::new(300.0, 400.0);
let rect = Rect::from_xywh(0.0, 0.0, 300.0, 400.0);
let rrect = RRect::from_rect_xy(rect, 8.0, 8.0); // Rounded corners

// Transformations
let transform = Matrix4::translation(50.0, 100.0, 0.0);
let rotated = Matrix4::rotation_z(std::f32::consts::PI / 4.0);

// Relative positioning (for Stack/Positioned widgets)
let positioned = RelativeRect::from_ltrb(10.0, 20.0, 30.0, 40.0);
```

### Layout

Control how widgets are sized and positioned:

```rust
use flui_types::layout::*;

// Alignment
let align = Alignment::TOP_LEFT;
let fractional = FractionalOffset::new(0.25, 0.75);

// Spacing
let padding = Edges::symmetric(px(16.0), px(8.0));
let margin = Edges::new(px(20.0), px(10.0), px(20.0), px(10.0));

// Flex layout
let direction = Axis::Horizontal;
let main_align = MainAxisAlignment::SpaceBetween;
let cross_align = CrossAxisAlignment::Center;

// Table layout
let column_width = TableColumnWidth::Flex(1.0);
let cell_align = TableCellVerticalAlignment::Middle;

// Constraints
let constraints = BoxConstraints::tight(Size::new(100.0, 50.0));
let loose = BoxConstraints::loose(Size::new(200.0, 100.0));
```

### Colors and Styling

Beautiful colors with full alpha support:

```rust
use flui_types::styling::*;

// Create colors
let red = Color::rgb(255, 0, 0);
let semi_transparent = Color::rgba(255, 0, 0, 128);
let from_hex = Color::from_hex(0xFF6633FF);

// Predefined colors
let white = Color::WHITE;
let black = Color::BLACK;

// Manipulate colors
let lighter = red.with_luminance(0.7);
let faded = red.with_opacity(0.5);
let blended = red.blend_over(white);

// Interpolate for animations
let middle = Color::lerp(Color::RED, Color::BLUE, 0.5);

// Accessibility
let contrast = white.contrast_ratio(black); // 21.0
```

### Animation

Smooth, natural motion:

```rust
use flui_types::animation::*;

// Built-in curves
let linear = Curve::Linear;
let ease = Curve::EaseInOut;
let bounce = Curve::BounceOut;

// Transform animation progress
let value = ease.transform(0.5);

// Tweens for interpolation
let tween = Tween::new(0.0, 100.0);
let current = tween.transform(0.5); // 50.0

// Animation status
let status = AnimationStatus::Forward;
if status.is_running() {
    // Animation in progress
}
```

### Physics

Natural-feeling scroll and fling behaviors:

```rust
use flui_types::physics::*;

// Spring animation (for bouncy effects)
let spring = SpringDescription::new(1.0, 100.0, 10.0);
let simulation = SpringSimulation::new(spring, 0.0, 100.0, 0.0);

// Friction (for scroll deceleration)
let friction = FrictionSimulation::new(0.135, 0.0, 1000.0);
let position = friction.x(0.5); // Position at t=0.5s
let velocity = friction.dx(0.5); // Velocity at t=0.5s

// Gravity (for free-fall effects)
let gravity = GravitySimulation::new(9.8, 0.0, 0.0, 100.0);
```

### Typography

Text styling and layout:

```rust
use flui_types::typography::*;

// Text alignment
let align = TextAlign::Center;
let direction = TextDirection::Ltr;

// Font properties
let weight = FontWeight::BOLD;
let style = FontStyle::Italic;

// Text selection
let selection = TextSelection::range(5, 15);
let cursor = TextSelection::collapsed(10);
```

### Gestures

Handle touch and pointer input:

```rust
use flui_types::gestures::*;

// Velocity tracking
let velocity = Velocity::new(500.0, -300.0);
let speed = velocity.pixels_per_second.distance();

// Pointer data
let pointer = PointerData::builder()
    .position(Point::new(100.0, 200.0))
    .pressure(0.8)
    .build();
```

### Platform

Platform-aware types:

```rust
use flui_types::platform::*;

// Theme brightness
let brightness = Brightness::Dark;
let bg = brightness.background_color();

// Locale
let locale = Locale::new("en", "US");

// Device orientation
let orientation = DeviceOrientation::LandscapeLeft;
```

## Performance

All types are designed for high performance:

- **Zero allocations** - Stack-allocated with `Copy` semantics
- **SIMD optimized** - Enable `simd` feature for 3-4x faster math
- **Compact memory** - `Color` is just 4 bytes, `Point` is 8 bytes
- **Const constructors** - Many types can be created at compile time

```rust
// Compile-time constants
const PADDING: Edges<Pixels> = Edges::all(px(16.0));
const PRIMARY: Color = Color::rgb(66, 133, 244);
const ORIGIN: Point = Point::ZERO;
```

## SIMD Support

The `simd` feature enables hardware acceleration on supported platforms:

| Platform | Status |
|----------|--------|
| Windows/Linux/macOS (x86_64) | SSE2 optimized |
| macOS (Apple Silicon) | NEON optimized |
| iOS/Android (ARM64) | NEON optimized |
| Other platforms | Automatic fallback |

## Documentation

Full API documentation is available at [docs.rs/flui_types](https://docs.rs/flui_types).

```bash
# Generate locally
cargo doc -p flui_types --open
```

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](../../LICENSE-APACHE))
- MIT license ([LICENSE-MIT](../../LICENSE-MIT))

at your option.

## Part of FLUI

This crate is part of the [FLUI](https://github.com/user/flui) UI framework for Rust.
