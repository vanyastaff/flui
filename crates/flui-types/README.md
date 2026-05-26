# flui_types

Core type system for FLUI - a modular, Flutter-inspired declarative UI framework for Rust.

## Overview

`flui_types` provides the foundational types used throughout the FLUI framework:

- **Geometry**: Type-safe units (Pixels, DevicePixels), geometric primitives (Point, Rect, Size), and transformations
- **Styling**: Colors with RGBA, HSL operations, and Porter-Duff alpha blending
- **Layout**: Edges, Corners, Constraints for flexible layout systems
- **Typography**: Text styles, alignment, and decoration
- **Gestures**: Event details for touch, drag, scale, and long-press interactions

## Features

✅ **Zero-cost abstractions** - Unit types compile to raw primitives with no runtime overhead  
✅ **Type safety** - Cannot mix incompatible units (Pixels vs DevicePixels) without explicit conversion  
✅ **Performance** - Sub-nanosecond operations (Point+Vec2: 184ps, Rect ops: <2ns, Color ops: <6ns)  
✅ **RTL support** - Bidirectional layout with TextDirection (Ltr/Rtl)  
✅ **GPU-ready** - DevicePixels for pixel-perfect framebuffer alignment  
✅ **Comprehensive** - 500+ tests covering geometry, colors, units, RTL, and edge cases

## Quick Start

Add to your `Cargo.toml`:

```toml
[dependencies]
flui_types = "0.1"
```

### Basic Usage

```rust
use flui_types::geometry::{Point, Rect, Size, px};
use flui_types::styling::Color;

// Create geometric primitives
let position = Point::new(px(100.0), px(200.0));
let size = Size::new(px(800.0), px(600.0));
let bounds = Rect::from_origin_size(position, size);

// Colors and blending
let primary = Color::from_hex("#2196F3").unwrap();
let hover = primary.lighten(0.1);
let overlay = Color::rgba(0, 0, 0, 128).blend_over(primary);

// Rectangle operations
let rect1 = Rect::from_xywh(px(0.0), px(0.0), px(100.0), px(100.0));
let rect2 = Rect::from_xywh(px(50.0), px(50.0), px(100.0), px(100.0));
let intersection = rect1.intersect(&rect2);
```

### Unit Conversions (Layout → GPU Rendering)

```rust
use flui_types::geometry::{DevicePixels, Pixels, ScaleFactor, device_px, px};

// Layout in logical pixels (density-independent)
let button_width = px(100.0);

// Convert to device pixels for GPU rendering via a typed scale factor
let scale = ScaleFactor::<Pixels, DevicePixels>::new(2.0); // Retina display
let device_width = button_width.to_device(scale);
assert_eq!(device_width.get(), 200); // 200 physical pixels

// Round-trip conversion (inverse direction reuses the same factor)
let back_to_logical = device_width.to_logical(scale);
assert_eq!(back_to_logical, button_width);
```

### Color Blending

```rust
use flui_types::styling::Color;

// Linear interpolation (mixing)
let red = Color::rgb(255, 0, 0);
let blue = Color::rgb(0, 0, 255);
let purple = Color::lerp(red, blue, 0.5);

// Alpha blending (Porter-Duff over)
let foreground = Color::rgba(255, 0, 0, 128);
let background = Color::rgb(255, 255, 255);
let blended = foreground.blend_over(background);

// HSL operations
let primary = Color::from_hex("#2196F3").unwrap();
let lighter = primary.lighten(0.2);
let darker = primary.darken(0.2);
```

## Examples

Run the included examples:

```bash
# Basic usage demonstration
cargo run --example types_basic_usage

# Unit conversion pipeline
cargo run --example unit_conversions

# Color blending and manipulation
cargo run --example color_blending
```

## Architecture

### Type-Safe Units

```rust
pub struct Pixels(pub f32);
pub struct DevicePixels(pub i32);

// Cannot accidentally mix units:
let logical = px(100.0);
let device = device_px(200);
// let mixed = logical + device; // ❌ Compile error!

// Must explicitly convert through a typed ScaleFactor:
let scale = ScaleFactor::<Pixels, DevicePixels>::new(2.0);
let converted = device.to_logical(scale);
let sum = logical + converted; // ✅ OK
```

### Generic Geometry

All geometric types are generic over unit types:

```rust
pub struct Point<T: Unit> { pub x: T, pub y: T }
pub struct Rect<T: Unit> { pub min: Point<T>, pub max: Point<T> }
pub struct Size<T: Unit> { pub width: T, pub height: T }

// Use with any unit:
let logical_rect = Rect::<Pixels>::from_xywh(px(0.0), px(0.0), px(100.0), px(100.0));
let device_rect = Rect::<DevicePixels>::from_xywh(device_px(0), device_px(0), device_px(200), device_px(200));
```

### Edges and Corners

```rust
use flui_types::geometry::{Edges, Corners, Radius};

// Padding/margins with Edges
let padding = Edges::all(px(10.0));
let asymmetric = Edges::new(px(10.0), px(20.0), px(10.0), px(20.0));

// Rounded corners
let card_radius = Corners::top(Radius::circular(px(8.0)));
let button_radius = Corners::all(Radius::circular(px(4.0)));
```

## Performance

Benchmarked on AMD Ryzen (Windows 11):

| Operation | Time | Target |
|-----------|------|--------|
| Point::distance | 8.6ns | <10ns ✅ |
| Rect::intersect | 1.8ns | <20ns ✅ |
| Rect::union | 0.9ns | <20ns ✅ |
| Color::lerp | 3.3ns | <20ns ✅ |
| Color::blend_over | 5.1ns | <20ns ✅ |
| Pixels addition | 194ps | - ✅ |
| Point + Vec2 | 184ps | - ✅ |

Run benchmarks yourself:

```bash
cargo bench
```

## Testing

Comprehensive test suite with 500+ tests:

```bash
# Run all tests
cargo test

# Run specific test suite
cargo test --test geometry_tests
cargo test --test color_operations_tests
cargo test --test rtl_support_tests

# Run with coverage
cargo tarpaulin --out Html
```

Current coverage: **>80%** (constitution requirement met)

## RTL Support

Full support for right-to-left layouts:

```rust
use flui_types::typography::TextDirection;

// Helper for semantic construction
fn edges_from_start_end(
    direction: TextDirection,
    start: Pixels,
    end: Pixels,
) -> Edges<Pixels> {
    match direction {
        TextDirection::Ltr => Edges::new(px(0.0), end, px(0.0), start),
        TextDirection::Rtl => Edges::new(px(0.0), start, px(0.0), end),
    }
}
```

## WASM Support

Builds for `wasm32-unknown-unknown`:

```bash
cargo build --target wasm32-unknown-unknown
```

## License

This project is part of the FLUI framework. See the main repository for licensing information.

## Contributing

Contributions are welcome! Please ensure:

- All tests pass: `cargo test`
- Clippy is clean: `cargo clippy -- -D warnings`
- Code is formatted: `cargo fmt`
- New public APIs have documentation
- Performance-critical code includes benchmarks

## Documentation

Generate and view documentation:

```bash
cargo doc --open
```

## Related Crates

- `flui-foundation` - Platform abstractions and core utilities
- `flui-platform` - Cross-platform window and event management
- `flui_rendering` - Render tree and layout engine
- `flui_painting` - Canvas API and compositing
- `flui_widgets` - Widget library

## Design Philosophy

1. **Type Safety First** - Catch bugs at compile time
2. **Zero-Cost Abstractions** - No runtime overhead
3. **Test-Driven** - Write tests before implementation
4. **Constitution-Compliant** - Follow project conventions strictly
5. **Performance** - Sub-nanosecond critical paths

## Inspiration

FLUI's type system draws inspiration from:

- **Flutter** - Three-tree architecture and widget patterns
- **GPUI** - Rust-specific platform abstractions
- **SwiftUI** - Declarative UI paradigms

## FAQ

**Q: Why separate Pixels and DevicePixels?**  
A: Prevents DPI scaling bugs. Layout uses logical Pixels (density-independent), GPU rendering uses DevicePixels (1:1 with framebuffer).

**Q: What's the overhead of unit types?**  
A: Zero. They're newtypes that compile to raw primitives (f32/i32). See benchmarks.

**Q: Can I use this without the rest of FLUI?**  
A: Yes! `flui_types` has minimal dependencies and can be used standalone for type-safe geometry and colors.

**Q: Why not use glam or euclid directly as the public surface?**
A: FLUI needs **Flutter-compatible APIs** (`Rect::from_ltrb`, `Size::area`,
column-major `Matrix4`, etc.) and **unit-type safety** specific to UI
layout (`Pixels`, `DevicePixels`, `Rems`, `Radians`). Both raw glam and
raw euclid would force every call site to either lose the typed barrier
or hand-roll a wrapper.

The chosen split as of U14 (Option D, post-2026-05-25 spike, recorded in
[`docs/research/2026-05-25-u17-spike-report.md`](../../docs/research/2026-05-25-u17-spike-report.md)):

- **flui owns unit-typed wrappers for polish discipline.** `Pixels`,
  `DevicePixels`, `ScaleFactor<Src, Dst>`, `Matrix4`, `Vec2<T>`,
  `Transform2D<T>`, `Rect<T>`, `EdgeInsets`, etc. live in
  `flui-geometry` (re-exported through `flui_types::geometry`) and
  carry the Flutter-parity API + the U1-U12 unit barriers (no
  `From<f32> for Pixels`, no `Pixels * Pixels -> Pixels`, etc.).
- **glam handles SIMD math underneath.** `Matrix4` is a
  `#[repr(transparent)]` newtype around `glam::Mat4` (U14.1); the
  hand-written SSE / NEON paths the framework used to maintain were
  removed in favour of glam's portable-SIMD implementation.
- **mint bridges to the wider math-library ecosystem.** `glam` is built
  with the `mint` feature on, so any consumer that needs to hand a
  `flui-geometry` type to `kurbo`, `nalgebra`, or `cgmath` can do it
  through mint without taking a direct dep on those crates.
- **Flutter compat surfaces ship as extension traits / typed
  constructors** rather than `impl From<glam::Mat4>`-style implicit
  conversions, so the unit barrier holds even where interop matters.

The shorthand: **flui's public types stay Flutter-shaped; glam's hot
math runs through them.** Neither raw `glam::Vec2` nor `euclid::Length`
ever shows up at the framework boundary that an application code path
sees.

**Q: Why was the `simd` Cargo feature removed?**
A: It used to gate hand-written SSE / NEON paths for `Matrix4`
multiplication and `Color::lerp` / `Color::blend_over`. Once `Matrix4`
delegates to glam (U14.1), glam's portable-SIMD path supersedes the
matrix paths; the colour paths were u8-only and modern compilers
auto-vectorise the scalar form at the call sites that matter. Keeping a
feature that gated dead code would have been a maintenance trap.
