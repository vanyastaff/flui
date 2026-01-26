# flui-types Quickstart Guide

**Date**: 2026-01-26
**Branch**: `001-flui-types`
**Related**: [spec.md](spec.md), [data-model.md](data-model.md), [contracts/README.md](contracts/README.md)

## Purpose

This guide provides practical examples and common patterns for using the flui-types crate. Perfect for developers who want to get started quickly.

---

## Installation

Add to your `Cargo.toml`:
```toml
[dependencies]
flui_types = "0.1.0"
```

---

## Basic Usage

### Unit Types: Preventing Cross-Platform Bugs

The core value proposition of flui-types is compile-time prevention of unit mixing bugs:

```rust
use flui_types::prelude::*;

// ✅ Type-safe layout (device-independent)
let button_width = Pixels(100.0);
let button_height = Pixels(50.0);

// ✅ Explicit conversion for rendering
let scale_factor = 2.0; // Retina display
let device_width = button_width.to_device_pixels(scale_factor); // DevicePixels(200.0)

// ❌ This won't compile (caught at compile time!)
// let mixed = button_width + device_width; // ERROR: type mismatch
```

**Why This Matters**: In traditional UI frameworks, mixing logical and device pixels causes subtle cross-platform bugs:
- Buttons too small on high-DPI displays
- Text clipped at wrong scale factors
- Hit testing using wrong coordinate spaces

With flui-types, these bugs are **impossible** - the compiler prevents them.

---

### Geometric Primitives: Type-Safe Calculations

All geometric types are generic over unit types, ensuring consistency:

```rust
use flui_types::geometry::*;

// Create a button rectangle (device-independent layout)
let button = Rect::from_ltwh(
    Pixels(10.0),   // left
    Pixels(10.0),   // top
    Pixels(100.0),  // width
    Pixels(50.0),   // height
);

// Hit testing (all in same unit type)
let tap_position = Point::new(Pixels(50.0), Pixels(30.0));
if button.contains(tap_position) {
    println!("Button tapped!");
}

// Padding (type-safe insets)
let padding = EdgeInsets::all(Pixels(8.0));
let content_area = button.inset_by(padding);

// Result: content_area is Rect<Pixels>, guaranteed consistent
assert_eq!(content_area.size.width, Pixels(84.0)); // 100 - 8*2
```

---

### Colors: Multiple Blending Modes

```rust
use flui_types::styling::*;

// From design specs (hex codes)
let brand_color = Color::from_hex("#FF5733").unwrap();

// Hover state (reduced opacity)
let hover_color = brand_color.with_opacity(0.8);

// Linear interpolation (smooth transitions)
let mixed = Color::RED.mix(&Color::BLUE, 0.5); // Purple

// Alpha compositing (layering semi-transparent colors)
let overlay = Color::from_rgba(255, 255, 255, 0.3); // 30% white
let result = overlay.blend_over(&brand_color);

// Lighten for highlights (perceptually uniform via HSL)
let highlight = brand_color.lighten(0.2); // 20% lighter

// RGB scaling for darkening (direct multiplication)
let shadow = brand_color.scale(0.7); // 70% brightness
```

---

## Common Patterns

### Pattern 1: Layout to Rendering Pipeline

The typical flow in a UI framework: layout in logical pixels, render in device pixels.

```rust
use flui_types::prelude::*;

// 1. Layout Phase (device-independent)
fn layout_button() -> Rect<Pixels> {
    Rect::from_ltwh(
        Pixels(10.0),
        Pixels(10.0),
        Pixels(100.0),
        Pixels(50.0),
    )
}

// 2. Rendering Phase (device-specific)
fn render_button(layout_rect: Rect<Pixels>, window: &Window) {
    // Get scale factor from window
    let scale_factor = window.scale_factor(); // 2.0 on Retina

    // Convert entire rectangle to device pixels
    let device_rect = Rect::new(
        layout_rect.origin.to_device_pixels(scale_factor),
        Size::new(
            layout_rect.size.width.to_device_pixels(scale_factor),
            layout_rect.size.height.to_device_pixels(scale_factor),
        ),
    );

    // Now safe to pass to GPU
    gpu_draw_rect(device_rect);
}
```

**Key Insight**: Type system guarantees you never accidentally pass logical pixels to the GPU.

---

### Pattern 2: Responsive Spacing with Rems

Font-relative units enable accessible layouts that scale with user preferences.

```rust
use flui_types::prelude::*;

// User's font size preference (from settings)
fn get_base_font_size(settings: &UserSettings) -> Pixels {
    Pixels(settings.font_size) // e.g., 16.0 (default) or 20.0 (large text)
}

// Define spacing in rems (scales with font size)
fn calculate_padding(settings: &UserSettings) -> Pixels {
    let base_font = get_base_font_size(settings);
    let padding_rems = Rems(1.5); // 1.5× base font size

    // Convert to pixels based on current settings
    padding_rems.to_pixels(base_font.0)
    // Default (16px): 24px
    // Large text (20px): 30px
}

// Layout respects accessibility settings
fn layout_card(settings: &UserSettings) -> Rect<Pixels> {
    let padding = calculate_padding(settings);

    Rect::from_ltwh(
        padding,
        padding,
        Pixels(200.0),
        Pixels(100.0),
    )
}
```

**Benefit**: Users with visual impairments get proportionally larger spacing automatically.

---

### Pattern 3: Color Theming with Type Safety

```rust
use flui_types::styling::*;

// Theme definition
struct Theme {
    primary: Color,
    background: Color,
    text: Color,
}

impl Theme {
    // Derive hover states (10% lighter)
    fn hover_color(&self) -> Color {
        self.primary.lighten(0.1)
    }

    // Derive active states (10% darker)
    fn active_color(&self) -> Color {
        self.primary.darken(0.1)
    }

    // Derive disabled states (50% opacity)
    fn disabled_color(&self) -> Color {
        self.primary.with_opacity(0.5)
    }

    // Overlay for focus rings
    fn focus_overlay(&self) -> Color {
        // Semi-transparent white overlay
        Color::from_rgba(255, 255, 255, 0.2).blend_over(&self.primary)
    }
}

// Usage
let theme = Theme {
    primary: Color::from_hex("#007AFF").unwrap(), // iOS blue
    background: Color::WHITE,
    text: Color::BLACK,
};

// Button states derived automatically
let button_normal = theme.primary;
let button_hover = theme.hover_color();
let button_active = theme.active_color();
let button_disabled = theme.disabled_color();
```

---

### Pattern 4: Hit Testing with Padding

```rust
use flui_types::geometry::*;

struct Button {
    frame: Rect<Pixels>,
    padding: EdgeInsets<Pixels>,
}

impl Button {
    fn hit_test(&self, point: Point<Pixels>) -> bool {
        // Hit test against outer frame
        if !self.frame.contains(point) {
            return false;
        }

        // Check if inside content area (excluding padding)
        let content_area = self.frame.inset_by(self.padding);
        content_area.contains(point)
    }

    fn visual_bounds(&self) -> Rect<Pixels> {
        // Expand by padding for visual feedback (hover glow, etc.)
        self.frame.inflate(Pixels(2.0))
    }
}

// Usage
let button = Button {
    frame: Rect::from_ltwh(Pixels(0.0), Pixels(0.0), Pixels(100.0), Pixels(50.0)),
    padding: EdgeInsets::all(Pixels(8.0)),
};

// Tap at button edge (inside frame, outside content)
let tap = Point::new(Pixels(95.0), Pixels(25.0));
assert!(!button.hit_test(tap)); // Outside content area
```

---

### Pattern 5: Rectangle Operations

```rust
use flui_types::geometry::*;

// Clipping: find visible portion of element
fn clip_to_viewport(element: Rect<Pixels>, viewport: Rect<Pixels>) -> Rect<Pixels> {
    element.intersect(viewport)
}

// Layout: find bounding box of multiple elements
fn bounding_box(elements: &[Rect<Pixels>]) -> Rect<Pixels> {
    let mut bounds = elements[0];
    for element in &elements[1..] {
        bounds = bounds.union(*element);
    }
    bounds
}

// Usage
let card1 = Rect::from_ltwh(Pixels(10.0), Pixels(10.0), Pixels(100.0), Pixels(80.0));
let card2 = Rect::from_ltwh(Pixels(60.0), Pixels(50.0), Pixels(100.0), Pixels(80.0));

let overlap = card1.intersect(card2); // Overlapping region
let total_area = card1.union(card2);  // Bounding box of both
```

---

## Testing Your Code

### Unit Testing with flui-types

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::prelude::*;

    #[test]
    fn test_button_hit_area() {
        let button = Rect::from_ltwh(
            Pixels(0.0),
            Pixels(0.0),
            Pixels(100.0),
            Pixels(50.0),
        );

        // Inside button
        assert!(button.contains(Point::new(Pixels(50.0), Pixels(25.0))));

        // Outside button (right edge)
        assert!(!button.contains(Point::new(Pixels(150.0), Pixels(25.0))));

        // Edge case: exactly on boundary (inclusive)
        assert!(button.contains(Point::new(Pixels(100.0), Pixels(50.0))));
    }

    #[test]
    fn test_color_mixing() {
        let red = Color::RED;
        let blue = Color::BLUE;

        // 50/50 mix should be purple
        let purple = red.mix(&blue, 0.5);
        assert_eq!(purple.r, 0.5);
        assert_eq!(purple.b, 0.5);
        assert_eq!(purple.g, 0.0);
    }

    #[test]
    fn test_unit_conversion_round_trip() {
        let logical = Pixels(100.0);
        let scale = 2.0;

        // Round-trip should preserve value
        let device = logical.to_device_pixels(scale);
        let back = device.to_logical_pixels(scale);

        assert!((logical.0 - back.0).abs() < 1e-6); // Within epsilon
    }
}
```

---

### Property Testing (Advanced)

For comprehensive edge case coverage:

```rust
#[cfg(test)]
mod property_tests {
    use proptest::prelude::*;
    use flui_types::geometry::*;

    // Generate arbitrary rectangles for testing
    fn arbitrary_rect() -> impl Strategy<Value = Rect<Pixels>> {
        (0.0f32..1000.0, 0.0f32..1000.0, 0.0f32..1000.0, 0.0f32..1000.0)
            .prop_map(|(x, y, w, h)| {
                Rect::from_ltwh(Pixels(x), Pixels(y), Pixels(w), Pixels(h))
            })
    }

    proptest! {
        #[test]
        fn rect_intersection_commutative(
            r1 in arbitrary_rect(),
            r2 in arbitrary_rect()
        ) {
            // Order shouldn't matter for intersection
            let i1 = r1.intersect(r2);
            let i2 = r2.intersect(r1);
            prop_assert!(i1.approx_eq(i2));
        }

        #[test]
        fn rect_union_contains_both(
            r1 in arbitrary_rect(),
            r2 in arbitrary_rect()
        ) {
            let union = r1.union(r2);

            // Union must contain all corners of both rectangles
            prop_assert!(union.contains(r1.origin));
            prop_assert!(union.contains(r2.origin));
        }
    }
}
```

---

## Performance Tips

### 1. Use Copy Semantics

All types are `Copy` - prefer passing by value:

```rust
// ✅ Preferred: pass by value (cheap Copy)
fn calculate_area(rect: Rect<Pixels>) -> f32 {
    rect.size.area()
}

// ❌ Unnecessary: passing by reference
fn calculate_area_slow(rect: &Rect<Pixels>) -> f32 {
    rect.size.area()
}
```

**Why**: Passing small `Copy` types by value is faster (no pointer indirection) and enables better compiler optimizations.

---

### 2. Trust the Optimizer

Unit conversions with constant scale factors are free:

```rust
// This compiles to a simple multiplication
const SCALE: f32 = 2.0;

fn to_device(logical: Pixels) -> DevicePixels {
    logical.to_device_pixels(SCALE) // Optimized to: logical * 2.0
}
```

**Assembly** (release mode):
```asm
mulss xmm0, 2.0  ; Single instruction, no function call
```

---

### 3. Avoid Allocations

All operations are stack-based:

```rust
// ✅ Zero allocations
let p1 = Point::new(Pixels(10.0), Pixels(20.0));
let p2 = Point::new(Pixels(30.0), Pixels(40.0));
let distance = p1.distance_to(p2); // Stack-only

// ✅ Even collections are stack values
let corners = Corners::all(Pixels(8.0)); // No heap
```

---

### 4. Batch Conversions

Convert once, use many times:

```rust
// ❌ Wasteful: converting per operation
fn render_many(points: &[Point<Pixels>], scale: f32) {
    for point in points {
        let device_point = point.to_device_pixels(scale);
        gpu_draw_point(device_point);
    }
}

// ✅ Better: convert entire batch
fn render_many_optimized(points: &[Point<Pixels>], scale: f32) {
    let device_points: Vec<_> = points
        .iter()
        .map(|p| p.to_device_pixels(scale))
        .collect();

    gpu_draw_points(&device_points); // Single GPU call
}
```

---

## Edge Cases and Gotchas

### 1. Negative Rectangle Dimensions

Negative dimensions are normalized automatically:

```rust
// Negative width: origin adjusted
let rect = Rect::from_ltwh(Pixels(100.0), Pixels(50.0), Pixels(-20.0), Pixels(30.0));

// Origin shifted left by 20
assert_eq!(rect.origin.x, Pixels(80.0));

// Dimensions clamped positive
assert_eq!(rect.size.width, Pixels(20.0));

// Visual bounds preserved
assert_eq!(rect.right(), Pixels(100.0));
```

**Rationale**: Preserves intended visual bounds while ensuring size is always non-negative.

---

### 2. Invalid Hex Colors

```rust
// Debug builds: panic with clear message
#[cfg(debug_assertions)]
{
    // This will panic
    let color = Color::from_hex("#GGHHII");
    // panic!("Invalid hex color '#GGHHII': expected format #RRGGBB or #RRGGBBAA")
}

// Release builds: fallback with warning
#[cfg(not(debug_assertions))]
{
    // Returns transparent black + logs warning
    let color = Color::from_hex("#GGHHII");
    assert_eq!(color, Color::TRANSPARENT);
    // tracing::warn!("Invalid hex color '#GGHHII', using transparent black");
}
```

**Best Practice**: Always use `Color::from_hex(...).unwrap()` in debug builds to catch issues early.

---

### 3. Floating-Point Equality

Use `approx_eq()` instead of `==`:

```rust
let p1 = Point::new(Pixels(10.0), Pixels(20.0));
let p2 = p1.offset_by(Offset::new(Pixels(1e-7), Pixels(0.0)));

// ❌ May fail due to floating-point precision
// assert_eq!(p1, p2);

// ✅ Correct: use epsilon tolerance
assert!(p1.approx_eq(p2)); // Within 1e-6 epsilon
```

---

### 4. Empty Rectangles

Always check `is_empty()` before operations:

```rust
let r1 = Rect::from_ltwh(Pixels(0.0), Pixels(0.0), Pixels(100.0), Pixels(0.0));

if r1.is_empty() {
    // Skip rendering for empty rectangles
    return;
}

// Safe to calculate area (non-zero)
let area = r1.size.area();
```

---

## Advanced Usage

### Custom Unit Types (Rare)

You can define custom unit types for specialized use cases:

```rust
use flui_types::units::Unit;

#[derive(Copy, Clone, PartialEq, Debug, Default)]
pub struct Centimeters(pub f32);

impl Unit for Centimeters {
    const ZERO: Self = Centimeters(0.0);

    fn to_f32(self) -> f32 { self.0 }
    fn from_f32(value: f32) -> Self { Centimeters(value) }
}

// Now usable with geometric types
let physical_size = Size::new(Centimeters(10.0), Centimeters(5.0));
```

**When to Use**: Physical dimensions, scientific applications, print layouts.

---

### Generic Functions Over Units

Write functions that work with any unit type:

```rust
use flui_types::units::Unit;

fn center_of_rect<T: Unit>(rect: Rect<T>) -> Point<T> {
    rect.center()
}

// Works with any unit
let logical_rect = Rect::from_ltwh(Pixels(0.0), Pixels(0.0), Pixels(100.0), Pixels(50.0));
let logical_center = center_of_rect(logical_rect); // Point<Pixels>

let device_rect = Rect::from_ltwh(DevicePixels(0.0), DevicePixels(0.0), DevicePixels(200.0), DevicePixels(100.0));
let device_center = center_of_rect(device_rect); // Point<DevicePixels>
```

---

## Migration from Other Libraries

### From raw f32 coordinates

```rust
// Before (error-prone)
fn old_layout() -> (f32, f32, f32, f32) {
    (10.0, 10.0, 100.0, 50.0) // What units? Unknown!
}

// After (type-safe)
fn new_layout() -> Rect<Pixels> {
    Rect::from_ltwh(
        Pixels(10.0),
        Pixels(10.0),
        Pixels(100.0),
        Pixels(50.0),
    )
}
```

---

### From euclid (Mozilla's library)

```rust
// euclid uses type tags
use euclid::{Point2D, Size2D, Rect};

struct LogicalPixel;
struct DevicePixel;

let point: Point2D<f32, LogicalPixel> = Point2D::new(10.0, 20.0);

// flui-types uses newtype pattern (simpler, clearer errors)
use flui_types::prelude::*;

let point = Point::new(Pixels(10.0), Pixels(20.0));
```

**Advantages**: Better error messages, simpler mental model, no phantom types.

---

## Next Steps

### Learn More

- **[spec.md](spec.md)** - Complete specification with user stories
- **[data-model.md](data-model.md)** - Detailed type definitions
- **[contracts/README.md](contracts/README.md)** - API guarantees and testing strategies

### Explore Examples

```bash
# Run basic usage example
cargo run --example basic_usage

# Run unit conversion example
cargo run --example unit_conversions

# Run color blending example
cargo run --example color_blending
```

### Run Tests

```bash
# All tests (unit + integration + property)
cargo test

# Property tests only
cargo test --test property_tests

# Benchmarks
cargo bench
```

---

## Getting Help

### Common Questions

**Q: Can I mix Pixels and DevicePixels?**
A: No, this is caught at compile time. Use explicit conversion methods like `to_device_pixels(scale)`.

**Q: Why use Rems instead of Pixels for spacing?**
A: Rems scale with user font preferences, improving accessibility for visually impaired users.

**Q: What's the difference between `mix()` and `blend_over()`?**
A: `mix()` is linear interpolation (no alpha consideration), `blend_over()` is Porter-Duff alpha compositing.

**Q: Can I use flui-types without the rest of Flui?**
A: Yes! It's a standalone crate with zero Flui dependencies.

---

**Ready to build type-safe UIs!** 🎉
