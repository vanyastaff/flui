# flui_types Documentation

Welcome to the `flui_types` documentation! This directory contains comprehensive guides for developers.

## Quick Links

- **[Cheatsheet](CHEATSHEET.md)** - Quick reference for all types
- **[User Guide](GUIDE.md)** - Complete guide to using flui_types
- **[Architecture](ARCHITECTURE.md)** - Deep dive into type system design
- **[Design Patterns](PATTERNS.md)** - Patterns and idioms
- **[Performance](PERFORMANCE.md)** - SIMD, benchmarks, optimization

## Documentation Structure

### For New Users

Start here if you're new to `flui_types`:

1. **[User Guide](GUIDE.md)** - Read this first
   - Quick start
   - Geometry basics
   - Layout system
   - Colors and styling
   - Animation and physics
   - Common recipes

### For Advanced Users

Deep dive into internals and optimization:

2. **[Architecture](ARCHITECTURE.md)** - Type system design
   - Module organization
   - Type relationships
   - Memory layout
   - Flutter comparison
   - Extension points

3. **[Design Patterns](PATTERNS.md)** - Code patterns
   - Copy semantics
   - Builder pattern
   - Const constructors
   - Type safety idioms
   - Conversion traits

4. **[Performance](PERFORMANCE.md)** - Optimization guide
   - SIMD acceleration
   - Memory efficiency
   - Benchmarks
   - Profiling tips
   - Common pitfalls

## Quick Reference

### Installation

```toml
[dependencies]
flui_types = "0.1"

# Optional: SIMD acceleration
flui_types = { version = "0.1", features = ["simd"] }

# Optional: Serialization
flui_types = { version = "0.1", features = ["serde"] }
```

### Basic Example

```rust
use flui_types::prelude::*;

fn main() {
    // Geometry
    let rect = Rect::from_xywh(10.0, 20.0, 100.0, 50.0);
    let center = rect.center();
    
    // Layout
    let padding = EdgeInsets::all(16.0);
    let alignment = Alignment::CENTER;
    
    // Styling
    let color = Color::rgb(66, 133, 244);
    let faded = color.with_opacity(0.5);
    
    // Animation
    let progress = Curve::EaseInOut.transform(0.5);
}
```

### Feature Flags

| Feature | Description |
|---------|-------------|
| `simd` | SIMD acceleration (3-4x faster math) |
| `serde` | Serialization support |

## Key Concepts

### 1. Geometry Types

Spatial primitives for UI layout:

```rust
// Absolute position
let point = Point::new(100.0, 200.0);

// Relative displacement  
let offset = Offset::new(10.0, 20.0);

// Dimensions
let size = Size::new(300.0, 400.0);

// Rectangle (min/max based)
let rect = Rect::from_xywh(0.0, 0.0, 300.0, 400.0);

// 4x4 transformation matrix
let transform = Matrix4::translation(50.0, 100.0, 0.0);
```

### 2. Layout Types

Control widget positioning:

```rust
// Alignment (-1 to 1 coordinate system)
let align = Alignment::TOP_LEFT; // (-1, -1)

// Fractional offset (0 to 1 coordinate system)
let frac = FractionalOffset::CENTER; // (0.5, 0.5)

// Padding/margins
let insets = EdgeInsets::symmetric(16.0, 8.0);

// Box constraints
let constraints = BoxConstraints::tight(Size::new(100.0, 50.0));
```

### 3. Color System

32-bit RGBA colors:

```rust
// Create colors
let red = Color::rgb(255, 0, 0);
let transparent = Color::rgba(255, 0, 0, 128);

// Manipulate
let lighter = red.with_luminance(0.7);
let blended = red.blend_over(Color::WHITE);

// Interpolate
let purple = Color::lerp(Color::RED, Color::BLUE, 0.5);
```

### 4. Animation

Curves and physics:

```rust
// Easing curves
let value = Curve::EaseInOut.transform(0.5);

// Spring simulation
let spring = SpringDescription::new(1.0, 100.0, 10.0);

// Friction (scroll physics)
let friction = FrictionSimulation::new(0.135, 0.0, 1000.0);
```

## Type Categories

| Category | Types | Purpose |
|----------|-------|---------|
| **Geometry** | `Point`, `Offset`, `Size`, `Rect`, `RRect`, `Matrix4` | Spatial primitives |
| **Layout** | `Alignment`, `EdgeInsets`, `BoxConstraints`, `Axis` | Widget layout |
| **Styling** | `Color`, `Paint`, `Gradient`, `BoxDecoration` | Visual appearance |
| **Typography** | `TextStyle`, `FontWeight`, `TextAlign` | Text rendering |
| **Animation** | `Curve`, `Tween`, `AnimationStatus` | Motion |
| **Physics** | `SpringDescription`, `FrictionSimulation` | Natural motion |
| **Gestures** | `Velocity`, `PointerData` | Input handling |
| **Platform** | `Brightness`, `Locale`, `Orientation` | Platform integration |

## Performance Characteristics

All types are optimized for UI workloads:

| Operation | Cost | Notes |
|-----------|------|-------|
| Point arithmetic | ~1ns | Single instruction |
| Rect intersection | ~2-3ns | Few comparisons |
| Color blending | ~5ns (scalar), ~2ns (SIMD) | Alpha compositing |
| Matrix multiply | ~25ns (scalar), ~8ns (SIMD) | 16 muls + 12 adds |

## Memory Layout

Compact, cache-friendly types:

| Type | Size | Layout |
|------|------|--------|
| `Point` | 8 bytes | 2 × f32 |
| `Size` | 8 bytes | 2 × f32 |
| `Rect` | 16 bytes | 4 × f32 |
| `Color` | 4 bytes | u32 (packed RGBA) |
| `Matrix4` | 64 bytes | 16 × f32 |
| `EdgeInsets` | 16 bytes | 4 × f32 |

## Best Practices

### Do

- Use `prelude::*` for common types
- Enable `simd` feature for math-heavy code
- Use const constructors where possible
- Leverage `Copy` semantics

### Don't

- Allocate in hot loops (all types are stack-allocated)
- Ignore `#[must_use]` warnings
- Mix `Point` and `Offset` incorrectly
- Forget to handle NaN/infinity edge cases

## API Reference

Full documentation at:
- **docs.rs**: https://docs.rs/flui_types
- **Local**: `cargo doc -p flui_types --open`

## Testing

```bash
# All tests
cargo test -p flui_types

# With SIMD
cargo test -p flui_types --features simd

# Specific module
cargo test -p flui_types geometry
```

## See Also

- [Main README](../README.md) - Project overview
- [FLUI Framework](../../README.md) - Parent project

Last updated: 2025-11-29
