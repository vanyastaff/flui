# Architecture

Internal architecture and design of `flui_types`.

## Overview

`flui_types` is the foundational crate of FLUI, providing core data types with zero dependencies on the rest of the framework.

```
┌─────────────────────────────────────────────────────────────┐
│                      flui_app                               │
├─────────────────────────────────────────────────────────────┤
│                    flui_widgets                             │
├─────────────────────────────────────────────────────────────┤
│    flui_rendering    │    flui_interaction                  │
├──────────────────────┴──────────────────────────────────────┤
│                      flui_core                              │
├─────────────────────────────────────────────────────────────┤
│                     flui_types  ← YOU ARE HERE              │
└─────────────────────────────────────────────────────────────┘
```

## Module Structure

### Module Hierarchy

```
flui_types/
├── geometry/           # Core geometry
│   ├── point.rs       # Point
│   ├── size.rs        # Size
│   ├── rect.rs        # Rect
│   ├── offset.rs      # Offset
│   ├── radius.rs      # Radius, BorderRadius
│   ├── insets.rs      # EdgeInsets
│   └── relative_rect.rs # RelativeRect
│
├── layout/            # Layout types
│   ├── axis.rs        # Axis, AxisDirection
│   ├── alignment.rs   # Alignment, MainAxisAlignment, CrossAxisAlignment
│   ├── constraints.rs # BoxConstraints
│   ├── fractional_offset.rs # FractionalOffset
│   ├── table.rs       # TableColumnWidth, TableCellVerticalAlignment
│   └── viewport.rs    # CacheExtentStyle
│
├── animation/         # Animation
│   ├── curves.rs      # Curve, Curves
│   └── duration.rs    # Duration wrappers
│
├── physics/           # Scroll physics
│   ├── simulation.rs  # Simulation trait
│   ├── spring.rs      # SpringDescription, SpringSimulation
│   ├── friction.rs    # FrictionSimulation
│   └── clamping.rs    # ClampingScrollSimulation
│
├── painting/          # Painting
│   ├── color.rs       # Color, Colors
│   ├── gradient.rs    # Gradient, LinearGradient, RadialGradient
│   ├── shadow.rs      # BoxShadow
│   ├── border.rs      # Border, BorderSide
│   ├── decoration.rs  # BoxDecoration, ShapeDecoration
│   ├── clipping.rs    # Clip
│   └── image.rs       # ImageFit, ImageRepeat
│
├── typography/        # Typography
│   ├── font.rs        # FontWeight, FontStyle
│   ├── text_style.rs  # TextStyle
│   ├── text_align.rs  # TextAlign, TextDirection
│   └── text_overflow.rs # TextOverflow
│
├── gestures/          # Gestures and input
│   ├── hit_test.rs    # HitTestBehavior
│   ├── drag.rs        # DragStartBehavior
│   └── velocity.rs    # Velocity, VelocityTracker
│
├── platform/          # Platform types
│   ├── brightness.rs  # Brightness
│   ├── locale.rs      # Locale
│   └── target.rs      # TargetPlatform
│
└── sliver/            # Sliver layout
    ├── constraints.rs # SliverConstraints
    └── geometry.rs    # SliverGeometry
```

## Design Principles

### 1. Zero Dependencies

`flui_types` has no dependencies on other FLUI crates:

```toml
[dependencies]
# External dependencies only
serde = { version = "1.0", optional = true }
glam = "0.30"
```

**Why this matters:**
- Fast compilation
- Can be used standalone
- No circular dependencies
- Simple testing

### 2. Copy Semantics for Geometry

All geometry types implement `Copy`:

```rust
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}
```

**Benefits:**
- No heap allocations when passing
- Lives in CPU registers
- Thread-safe by default
- SIMD compatible

### 3. Builder Pattern for Complex Types

```rust
// Simple case — constructor
let color = Color::rgb(255, 128, 0);

// Complex case — builder
let style = TextStyle::new()
    .with_color(Colors::BLACK)
    .with_size(16.0)
    .with_weight(FontWeight::BOLD);
```

### 4. Constants for Common Values

```rust
impl Alignment {
    pub const TOP_LEFT: Self = Self { x: -1.0, y: -1.0 };
    pub const CENTER: Self = Self { x: 0.0, y: 0.0 };
    pub const BOTTOM_RIGHT: Self = Self { x: 1.0, y: 1.0 };
}

impl Colors {
    pub const RED: Color = Color::from_rgb(255, 0, 0);
    pub const GREEN: Color = Color::from_rgb(0, 255, 0);
    pub const BLUE: Color = Color::from_rgb(0, 0, 255);
}
```

## Data Types

### Geometry Primitives

```
Point ──────► Position in space
   │
   ├── x: f32
   └── y: f32

Size ───────► Object dimensions
   │
   ├── width: f32
   └── height: f32

Rect ───────► Rectangle = Point + Size
   │
   ├── origin: Point (or left, top)
   └── size: Size (or width, height)

Offset ─────► Displacement (vector)
   │
   ├── dx: f32
   └── dy: f32
```

### Type Relationships

```
                    ┌──────────────┐
                    │ BoxConstraints│
                    └──────┬───────┘
                           │ constrain()
                           ▼
┌────────┐   offset   ┌────────┐   inflate   ┌────────┐
│ Point  │ ─────────► │  Rect  │ ◄────────── │EdgeInsets│
└────────┘            └────────┘             └────────┘
     │                     │
     │ + Size              │ .size
     ▼                     ▼
┌────────┐            ┌────────┐
│  Rect  │            │  Size  │
└────────┘            └────────┘
```

### Alignment System

```
Alignment: -1.0 ────────── 0.0 ────────── 1.0
              │             │              │
              ▼             ▼              ▼
           LEFT          CENTER         RIGHT
           TOP           CENTER        BOTTOM

FractionalOffset: 0.0 ────────── 0.5 ────────── 1.0
                    │             │              │
                    ▼             ▼              ▼
                 LEFT          CENTER         RIGHT
```

## Serde Serialization

### Enabling

```toml
flui_types = { version = "0.1", features = ["serde"] }
```

### Supported Types

| Module | Types |
|--------|-------|
| geometry | Point, Size, Rect, Offset, Radius, EdgeInsets |
| layout | Axis, Alignment, BoxConstraints |
| painting | Color, Gradient, BoxShadow |
| animation | Curve (built-in) |

### Example

```rust
use flui_types::geometry::Rect;
use serde_json;

let rect = Rect::from_ltwh(10.0, 20.0, 100.0, 50.0);
let json = serde_json::to_string(&rect)?;
// {"left":10.0,"top":20.0,"width":100.0,"height":50.0}

let restored: Rect = serde_json::from_str(&json)?;
```

## Interoperability

### With glam

```rust
use flui_types::geometry::{Point, Offset};
use glam::Vec2;

// Point ↔ Vec2
let point = Point::new(10.0, 20.0);
let vec: Vec2 = point.into();
let back: Point = vec.into();

// Offset ↔ Vec2
let offset = Offset::new(5.0, 10.0);
let vec: Vec2 = offset.into();
```

### With wgpu

```rust
use flui_types::painting::Color;

let color = Color::rgba(255, 128, 0, 200);
let wgpu_color: wgpu::Color = color.into();
```

## Thread Safety

All types in `flui_types` are thread-safe:

```rust
// All geometry types: Send + Sync
fn assert_send<T: Send>() {}
fn assert_sync<T: Sync>() {}

assert_send::<Point>();
assert_sync::<Point>();
assert_send::<Rect>();
assert_sync::<Rect>();
```

**Why:**
- All types are `Copy` or contain only `Copy` data
- No interior mutability (RefCell, Mutex)
- No raw pointers

## Optimizations

### Type Sizes

```rust
// Compact representations
size_of::<Point>()   == 8   // 2 × f32
size_of::<Size>()    == 8   // 2 × f32
size_of::<Rect>()    == 16  // 4 × f32
size_of::<Color>()   == 4   // RGBA packed

// Option optimizations
size_of::<Option<Point>>() == 12  // Point + 1 byte + padding
```

### Inlining

Critical methods are marked `#[inline]`:

```rust
impl Point {
    #[inline]
    pub fn distance_to(&self, other: Point) -> f32 {
        let dx = self.x - other.x;
        let dy = self.y - other.y;
        (dx * dx + dy * dy).sqrt()
    }
}
```

### SIMD (Planned)

```rust
#[cfg(feature = "simd")]
impl Rect {
    pub fn intersect_simd(&self, other: &Rect) -> Option<Rect> {
        // Uses SIMD for parallel comparisons
    }
}
```

## Extending

### Adding a New Type

1. Create a file in the appropriate module:

```rust
// geometry/circle.rs
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Circle {
    pub center: Point,
    pub radius: f32,
}

impl Circle {
    pub fn new(center: Point, radius: f32) -> Self {
        Self { center, radius }
    }
    
    pub fn contains(&self, point: Point) -> bool {
        self.center.distance_to(point) <= self.radius
    }
}
```

2. Export in `mod.rs`:

```rust
// geometry/mod.rs
pub mod circle;
pub use circle::Circle;
```

3. Add tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_circle_contains() {
        let circle = Circle::new(Point::ZERO, 10.0);
        assert!(circle.contains(Point::new(5.0, 5.0)));
        assert!(!circle.contains(Point::new(20.0, 20.0)));
    }
}
```

## Testing

### Running Tests

```bash
# All tests
cargo test -p flui_types

# With serde
cargo test -p flui_types --features serde

# Specific module
cargo test -p flui_types geometry::
```

### Test Structure

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    // Basic tests
    #[test]
    fn test_creation() { ... }
    
    // Edge cases
    #[test]
    fn test_zero_size() { ... }
    
    // Mathematical properties
    #[test]
    fn test_commutative() { ... }
}
```

## See Also

- [GUIDE.md](GUIDE.md) — User Guide
- [PATTERNS.md](PATTERNS.md) — Usage Patterns
- [PERFORMANCE.md](PERFORMANCE.md) — Performance Optimization
- [CHEATSHEET.md](CHEATSHEET.md) — Quick Reference
