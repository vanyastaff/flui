# flui_types User Guide

Complete guide to using `flui_types` in your FLUI applications.

## Table of Contents

1. [Getting Started](#getting-started)
2. [Geometry Basics](#geometry-basics)
3. [Layout System](#layout-system)
4. [Working with Colors](#working-with-colors)
5. [Animation and Physics](#animation-and-physics)
6. [Text and Typography](#text-and-typography)
7. [Handling Input](#handling-input)
8. [Common Recipes](#common-recipes)
9. [Troubleshooting](#troubleshooting)

## Getting Started

### Installation

Add `flui_types` to your `Cargo.toml`:

```toml
[dependencies]
flui_types = "0.1"
```

For better performance, enable SIMD:

```toml
flui_types = { version = "0.1", features = ["simd"] }
```

### Using the Prelude

Import common types with the prelude:

```rust
use flui_types::prelude::*;

fn main() {
    let point = Point::new(10.0, 20.0);
    let color = Color::RED;
    let padding = Edges::all(px(16.0));
}
```

Or import specific modules:

```rust
use flui_types::geometry::{Point, Rect, Size, Edges, px};
use flui_types::layout::Alignment;
use flui_types::styling::Color;
```

## Geometry Basics

### Understanding Point vs Offset

`Point` represents an **absolute position** in 2D space:

```rust
let cursor_position = Point::new(100.0, 200.0);
let screen_center = Point::new(960.0, 540.0);
```

`Offset` represents a **relative displacement**:

```rust
let movement = Offset::new(10.0, -5.0);  // Moved right 10, up 5
let velocity = Offset::new(100.0, 0.0);  // 100 pixels/sec horizontally
```

Move a point by an offset:

```rust
let start = Point::new(50.0, 50.0);
let delta = Offset::new(25.0, 10.0);
let end = start + delta;  // Point(75.0, 60.0)
```

### Working with Sizes

`Size` represents dimensions (width and height):

```rust
let button_size = Size::new(120.0, 48.0);
let square = Size::square(64.0);

// Common operations
let area = button_size.area();           // 5760.0
let ratio = button_size.aspect_ratio();  // 2.5
let is_empty = button_size.is_empty();   // false

// Swap dimensions
let rotated = button_size.flipped();     // Size(48.0, 120.0)
```

### Rectangles

`Rect` is the workhorse for layout calculations:

```rust
// Create from position and size
let button = Rect::from_xywh(10.0, 20.0, 100.0, 40.0);

// Create from edges
let panel = Rect::from_ltrb(0.0, 0.0, 300.0, 200.0);

// Create centered
let dialog = Rect::from_center_size(
    Point::new(400.0, 300.0),  // center
    Size::new(200.0, 150.0)    // size
);
```

Common rectangle operations:

```rust
let rect = Rect::from_xywh(10.0, 20.0, 100.0, 50.0);

// Query properties
let center = rect.center();           // Point(60.0, 45.0)
let size = rect.size();               // Size(100.0, 50.0)
let top_left = Point::new(rect.left(), rect.top());

// Hit testing
let click = Point::new(50.0, 40.0);
if rect.contains(click) {
    println!("Button clicked!");
}

// Intersection
let other = Rect::from_xywh(80.0, 30.0, 100.0, 50.0);
if rect.intersects(other) {
    let overlap = rect.intersect(other);
}

// Expand/shrink
let with_margin = rect.inflate(10.0, 10.0);
let content_area = rect.deflate(8.0, 8.0);
```

### Rounded Rectangles

`RRect` adds corner radii for buttons, cards, etc:

```rust
let rect = Rect::from_xywh(0.0, 0.0, 200.0, 100.0);

// Uniform corners
let button = RRect::from_rect_xy(rect, 8.0, 8.0);

// Different corners
let card = RRect::from_rect_corners(
    rect,
    Radius::circular(16.0),  // top-left
    Radius::circular(16.0),  // top-right
    Radius::circular(0.0),   // bottom-right
    Radius::circular(0.0),   // bottom-left
);
```

### Transformations

Use `Matrix4` for complex transformations:

```rust
// Translation
let moved = Matrix4::translation(100.0, 50.0, 0.0);

// Rotation (around Z axis, in radians)
let rotated = Matrix4::rotation_z(std::f32::consts::PI / 4.0);  // 45 degrees

// Scaling
let scaled = Matrix4::scaling(2.0, 2.0, 1.0);  // 2x size

// Combine transformations (order matters!)
let transform = Matrix4::IDENTITY
    .translate(100.0, 100.0, 0.0)  // Move to position
    .rotate_z(0.5)                  // Rotate
    .scale(1.5, 1.5, 1.0);          // Scale up

// Apply to geometry
let point = Point::new(10.0, 20.0);
let transformed = transform.transform_point(point);

let rect = Rect::from_xywh(0.0, 0.0, 50.0, 30.0);
let transformed_rect = transform.transform_rect(rect);
```

## Layout System

### Alignment

`Alignment` uses a -1 to 1 coordinate system:

```rust
// Predefined alignments
Alignment::TOP_LEFT      // x=-1, y=-1
Alignment::CENTER        // x=0,  y=0
Alignment::BOTTOM_RIGHT  // x=1,  y=1

// Custom alignment (25% from left, 75% from top)
let custom = Alignment::new(-0.5, 0.5);
```

Position a child within a parent:

```rust
let parent = Size::new(200.0, 100.0);
let child = Size::new(50.0, 30.0);

let alignment = Alignment::CENTER;
let offset = alignment.along_offset(parent, child);
// offset = Offset(75.0, 35.0) - top-left position of centered child
```

### FractionalOffset

`FractionalOffset` uses 0 to 1 coordinates (often more intuitive):

```rust
FractionalOffset::TOP_LEFT      // dx=0.0, dy=0.0
FractionalOffset::CENTER        // dx=0.5, dy=0.5
FractionalOffset::BOTTOM_RIGHT  // dx=1.0, dy=1.0

// Convert between systems
let alignment = Alignment::CENTER;
let fractional = FractionalOffset::from_alignment(alignment);
let back = fractional.to_alignment();
```

### Edge Insets (Padding & Margins)

`Edges<Pixels>` represents spacing on all four sides:

```rust
// Uniform on all sides
let padding = Edges::all(px(16.0));

// Symmetric (vertical, horizontal)
let card_padding = Edges::symmetric(px(24.0), px(16.0));

// Individual sides (top, right, bottom, left)
let asymmetric = Edges::new(
    px(20.0),   // top
    px(10.0),   // right
    px(30.0),   // bottom
    px(10.0),   // left
);

// Common patterns
let horizontal_only = Edges::horizontal(px(16.0));
let vertical_only = Edges::vertical(px(16.0));
```

Apply to rectangles:

```rust
let outer = Rect::from_xywh(0.0, 0.0, 200.0, 100.0);
let padding = Edges::all(px(16.0));

let inner = padding.deflate_rect(outer);
// inner = Rect(16, 16, 168, 68) - smaller by padding
```

### Box Constraints

`BoxConstraints` control how widgets are sized:

```rust
// Fixed size
let tight = BoxConstraints::tight(Size::new(100.0, 50.0));
assert!(tight.is_tight());

// Maximum size (can be smaller)
let loose = BoxConstraints::loose(Size::new(200.0, 100.0));

// Unbounded (infinite max)
let expand = BoxConstraints::expand();

// Custom constraints
let custom = BoxConstraints::new(
    50.0,   // min_width
    200.0,  // max_width
    30.0,   // min_height
    100.0,  // max_height
);
```

Constrain sizes:

```rust
let constraints = BoxConstraints::new(50.0, 200.0, 30.0, 100.0);
let requested = Size::new(300.0, 20.0);
let actual = constraints.constrain(requested);
// actual = Size(200.0, 30.0) - clamped to constraints
```

### RelativeRect for Positioned

`RelativeRect` specifies distances from parent edges (for Stack/Positioned):

```rust
// Distance from each edge
let positioned = RelativeRect::from_ltrb(
    10.0,   // 10px from left
    20.0,   // 20px from top
    30.0,   // 30px from right
    40.0,   // 40px from bottom
);

// Fill the entire parent
let fill = RelativeRect::FILL;

// Convert to actual rect
let parent_size = Size::new(300.0, 200.0);
let actual_rect = positioned.to_rect(parent_size);
```

## Working with Colors

### Creating Colors

```rust
// RGB (fully opaque)
let red = Color::rgb(255, 0, 0);
let green = Color::rgb(0, 255, 0);

// RGBA (with alpha)
let semi_red = Color::rgba(255, 0, 0, 128);  // 50% transparent

// From hex (0xRRGGBBAA)
let material_blue = Color::from_hex(0x2196F3FF);

// Predefined constants
let white = Color::WHITE;
let black = Color::BLACK;
let transparent = Color::TRANSPARENT;
```

### Modifying Colors

```rust
let base = Color::rgb(100, 150, 200);

// Change opacity
let faded = base.with_opacity(0.5);      // 50% transparent
let more_opaque = base.with_alpha(200);  // Specific alpha value

// Adjust brightness
let lighter = base.with_luminance(0.8);  // Lighter
let darker = base.with_luminance(0.3);   // Darker

// Change individual channels
let redder = base.with_red(200);
```

### Color Blending

```rust
let foreground = Color::rgba(255, 0, 0, 128);  // Semi-transparent red
let background = Color::WHITE;

// Alpha composite (Porter-Duff SrcOver)
let result = foreground.blend_over(background);
```

### Color Interpolation

```rust
let start = Color::RED;
let end = Color::BLUE;

// Interpolate (0.0 = start, 1.0 = end)
let quarter = Color::lerp(start, end, 0.25);  // Reddish purple
let half = Color::lerp(start, end, 0.5);      // Purple
let three_quarter = Color::lerp(start, end, 0.75);  // Bluish purple
```

### Accessibility

```rust
let text = Color::rgb(50, 50, 50);
let background = Color::WHITE;

// Check contrast ratio (WCAG)
let ratio = text.contrast_ratio(background);
// Ratio >= 4.5:1 is recommended for normal text
// Ratio >= 3:1 is recommended for large text

if ratio >= 4.5 {
    println!("Good contrast!");
}

// Get relative luminance
let luminance = text.relative_luminance();  // 0.0 (black) to 1.0 (white)
```

## Animation and Physics

### Animation Curves

Curves control the pacing of animations:

```rust
// Built-in curves
let linear = Curve::Linear;         // Constant speed
let ease = Curve::EaseInOut;        // Slow start and end
let ease_in = Curve::EaseIn;        // Slow start
let ease_out = Curve::EaseOut;      // Slow end
let bounce = Curve::BounceOut;      // Bouncy ending

// Apply curve to animation progress (0.0 to 1.0)
let t = 0.5;  // Halfway through animation
let curved_t = ease.transform(t);  // Curved progress
```

### Tweens

Tweens interpolate between values:

```rust
// Float tween
let opacity_tween = Tween::new(0.0, 1.0);
let opacity = opacity_tween.transform(0.5);  // 0.5

// Use with curves
let t = 0.5;
let curved_t = Curve::EaseInOut.transform(t);
let animated_opacity = opacity_tween.transform(curved_t);
```

### Physics Simulations

#### Spring Animation

Natural, bouncy motion:

```rust
let spring = SpringDescription::new(
    1.0,    // mass
    100.0,  // stiffness (higher = faster)
    10.0,   // damping (higher = less bounce)
);

// Critical damping (no bounce)
let critical = SpringDescription::with_damping_ratio(1.0, 100.0, 1.0);

// Underdamped (bouncy)
let bouncy = SpringDescription::with_damping_ratio(1.0, 100.0, 0.5);

// Create simulation
let sim = SpringSimulation::new(
    spring,
    0.0,    // start position
    100.0,  // end position  
    0.0,    // initial velocity
);

// Query at any time
let position = sim.x(0.5);     // Position at t=0.5s
let velocity = sim.dx(0.5);    // Velocity at t=0.5s
let done = sim.is_done(2.0);   // Settled by t=2s?
```

#### Friction (Scroll Deceleration)

```rust
let sim = FrictionSimulation::new(
    0.135,   // drag coefficient (0.135 is good for scroll)
    0.0,     // start position
    1000.0,  // initial velocity (pixels/sec)
);

// Simulate scroll fling
let mut t = 0.0;
while !sim.is_done(t) {
    let pos = sim.x(t);
    let vel = sim.dx(t);
    println!("t={:.2}: pos={:.1}, vel={:.1}", t, pos, vel);
    t += 0.1;
}

// Get final resting position
let final_pos = sim.final_x();
```

#### Gravity

```rust
let sim = GravitySimulation::new(
    980.0,   // acceleration (pixels/sec²)
    0.0,     // start position
    0.0,     // initial velocity
    500.0,   // end position (ground)
);

let position = sim.x(0.5);
let velocity = sim.dx(0.5);
```

## Text and Typography

### Text Style

```rust
let style = TextStyle::new()
    .with_color(Color::BLACK)
    .with_font_size(16.0)
    .with_font_weight(FontWeight::NORMAL)
    .with_font_style(FontStyle::Normal)
    .with_letter_spacing(0.0)
    .with_word_spacing(0.0)
    .with_height(1.5);  // Line height multiplier
```

### Font Weight

```rust
FontWeight::THIN          // 100
FontWeight::EXTRA_LIGHT   // 200
FontWeight::LIGHT         // 300
FontWeight::NORMAL        // 400
FontWeight::MEDIUM        // 500
FontWeight::SEMI_BOLD     // 600
FontWeight::BOLD          // 700
FontWeight::EXTRA_BOLD    // 800
FontWeight::BLACK         // 900

// Check if bold
if weight.is_bold() {  // weight >= 600
    // ...
}

// Interpolate weights
let animated = FontWeight::lerp(FontWeight::NORMAL, FontWeight::BOLD, 0.5);
```

### Text Alignment

```rust
TextAlign::Left
TextAlign::Center
TextAlign::Right
TextAlign::Justify

// RTL-aware
TextAlign::Start  // Left in LTR, Right in RTL
TextAlign::End    // Right in LTR, Left in RTL
```

### Text Selection

```rust
// Cursor position (collapsed selection)
let cursor = TextSelection::collapsed(10);

// Selected range
let selection = TextSelection::range(5, 15);

// Query
if selection.is_collapsed() {
    // Just a cursor
} else {
    let start = selection.start();
    let end = selection.end();
}
```

## Handling Input

### Velocity Tracking

```rust
// From touch movement
let velocity = Velocity::new(500.0, -200.0);  // pixels/sec

// Get magnitude
let speed = velocity.pixels_per_second.distance();

// Clamp for fling
let clamped = velocity.clamp_magnitude(100.0, 8000.0);
```

### Pointer Data

```rust
let pointer = PointerData::builder()
    .position(Point::new(100.0, 200.0))
    .delta(Offset::new(5.0, 3.0))
    .pressure(0.8)
    .device(PointerDeviceKind::Touch)
    .buttons(0x01)  // Primary button
    .build();

// Check button state
let is_pressed = pointer.buttons & 0x01 != 0;
```

## Common Recipes

### Centering a Widget

```rust
fn center_rect(child: Size, parent: Size) -> Rect {
    let offset = Alignment::CENTER.along_offset(parent, child);
    Rect::from_xywh(offset.dx, offset.dy, child.width, child.height)
}
```

### Padding a Rectangle

```rust
fn with_padding(rect: Rect, padding: Edges<Pixels>) -> Rect {
    padding.deflate_rect(rect)
}

fn add_margin(rect: Rect, margin: Edges<Pixels>) -> Rect {
    margin.inflate_rect(rect)
}
```

### Hit Testing

```rust
fn hit_test(widgets: &[(Rect, &str)], point: Point) -> Option<&str> {
    // Test in reverse order (top-most first)
    for (rect, name) in widgets.iter().rev() {
        if rect.contains(point) {
            return Some(name);
        }
    }
    None
}
```

### Animated Color Transition

```rust
fn animate_color(from: Color, to: Color, progress: f32, curve: Curve) -> Color {
    let t = curve.transform(progress.clamp(0.0, 1.0));
    Color::lerp(from, to, t)
}
```

### Scroll Position Clamping

```rust
fn clamp_scroll(
    offset: f32,
    viewport: f32,
    content: f32,
) -> f32 {
    let max_scroll = (content - viewport).max(0.0);
    offset.clamp(0.0, max_scroll)
}
```

### Aspect Ratio Fitting

```rust
fn fit_contain(content: Size, container: Size) -> Size {
    let scale = (container.width / content.width)
        .min(container.height / content.height);
    Size::new(content.width * scale, content.height * scale)
}

fn fit_cover(content: Size, container: Size) -> Size {
    let scale = (container.width / content.width)
        .max(container.height / content.height);
    Size::new(content.width * scale, content.height * scale)
}
```

## Troubleshooting

### NaN and Infinity

Many operations can produce NaN or infinity. Always validate:

```rust
let size = Size::new(width, height);
if !size.is_finite() {
    // Handle invalid size
}

let rect = Rect::from_xywh(x, y, w, h);
if !rect.is_finite() {
    // Handle invalid rect
}
```

### Constraint Violations

```rust
let constraints = BoxConstraints::new(50.0, 100.0, 30.0, 60.0);

// This might fail if constraints are invalid
debug_assert!(constraints.is_normalized());

// Always constrain sizes
let size = constraints.constrain(requested_size);
```

### Color Overflow

Color channels are clamped to 0-255:

```rust
// These are safe
let c = Color::rgb(300, -50, 128);  // Clamped to (255, 0, 128)
```

### Matrix Singularity

```rust
let matrix = Matrix4::scaling(0.0, 0.0, 0.0);  // Singular!

if let Some(inverse) = matrix.inverse() {
    // Safe to use inverse
} else {
    // Matrix is singular, can't invert
}
```

## Next Steps

- [Architecture](ARCHITECTURE.md) - Understand the type system design
- [Patterns](PATTERNS.md) - Learn idiomatic usage patterns
- [Performance](PERFORMANCE.md) - Optimize your code
- [Cheatsheet](CHEATSHEET.md) - Quick reference
