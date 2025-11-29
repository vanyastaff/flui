# Patterns

Common patterns and best practices when working with `flui_types`.

## Contents

- [Geometry Patterns](#geometry-patterns)
- [Layout Patterns](#layout-patterns)
- [Animation Patterns](#animation-patterns)
- [Painting Patterns](#painting-patterns)
- [Input Handling Patterns](#input-handling-patterns)
- [Anti-patterns](#anti-patterns)

---

## Geometry Patterns

### Creating Rect

```rust
use flui_types::geometry::{Rect, Point, Size};

// Pattern 1: From left-top and size (most common)
let rect = Rect::from_ltwh(10.0, 20.0, 100.0, 50.0);

// Pattern 2: From point and size
let rect = Rect::from_origin_size(
    Point::new(10.0, 20.0),
    Size::new(100.0, 50.0),
);

// Pattern 3: From two points (corners)
let rect = Rect::from_points(
    Point::new(10.0, 20.0),   // top-left
    Point::new(110.0, 70.0),  // bottom-right
);

// Pattern 4: From center and size
let rect = Rect::from_center(
    Point::new(60.0, 45.0),   // center
    Size::new(100.0, 50.0),
);
```

### Transforming Rect

```rust
// Translate
let moved = rect.translate(Offset::new(10.0, 20.0));

// Expand (padding outward)
let expanded = rect.inflate(5.0, 5.0);

// Shrink (padding inward)
let shrunk = rect.deflate(5.0, 5.0);

// With EdgeInsets
let padded = rect.inflate_by(EdgeInsets::all(10.0));

// Scale from center
let scaled = Rect::from_center(rect.center(), rect.size * 1.5);
```

### Intersection Checks

```rust
// Contains point?
if rect.contains(point) {
    // Point is inside
}

// Overlaps?
if rect.overlaps(&other) {
    // There's an intersection
}

// Get intersection
if let Some(intersection) = rect.intersect(&other) {
    // Work with intersection
}

// Union (bounding box)
let union = rect.union(&other);
```

### Normalization

```rust
// Rect with negative sizes → normalized
let weird = Rect::from_ltwh(100.0, 100.0, -50.0, -30.0);
let normal = weird.normalize();
// normal = Rect { left: 50, top: 70, width: 50, height: 30 }
```

---

## Layout Patterns

### BoxConstraints

```rust
use flui_types::layout::BoxConstraints;

// Pattern 1: Tight constraints (exact size)
let tight = BoxConstraints::tight(Size::new(100.0, 50.0));

// Pattern 2: Loose constraints (up to maximum)
let loose = BoxConstraints::loose(Size::new(300.0, 200.0));

// Pattern 3: Only width is fixed
let width_constrained = BoxConstraints::tight_for_width(100.0);

// Pattern 4: Range
let ranged = BoxConstraints::new(
    50.0, 200.0,   // min/max width
    30.0, 100.0,   // min/max height
);
```

### Applying Constraints

```rust
// Fit size within constraints
let desired = Size::new(500.0, 300.0);
let actual = constraints.constrain(desired);

// Check validity
if constraints.is_satisfied_by(size) {
    // Size satisfies constraints
}

// Largest possible size
let max_size = constraints.biggest();

// Smallest possible size
let min_size = constraints.smallest();
```

### Alignment

```rust
use flui_types::layout::Alignment;
use flui_types::geometry::{Size, Offset};

// Compute offset for alignment
fn align_child(parent: Size, child: Size, alignment: Alignment) -> Offset {
    alignment.along_size(parent - child)
}

// Example: centering
let parent = Size::new(200.0, 100.0);
let child = Size::new(50.0, 30.0);
let offset = Alignment::CENTER.along_size(parent - child);
// offset = Offset(75.0, 35.0)
```

### RelativeRect for Positioned

```rust
use flui_types::geometry::RelativeRect;

// Pattern 1: Fixed position from corner
let positioned = RelativeRect::from_ltrb(10.0, 20.0, f32::INFINITY, f32::INFINITY);
// left: 10, top: 20, stretches right-down

// Pattern 2: Fixed width, anchored right
let right_aligned = RelativeRect::from_ltrb(f32::INFINITY, 10.0, 20.0, f32::INFINITY);
// right: 20, top: 10

// Pattern 3: Stretch to parent with padding
let fill = RelativeRect::from_ltrb(10.0, 10.0, 10.0, 10.0);
// 10px padding on all sides

// Pattern 4: Fill completely
let full = RelativeRect::fill();
```

---

## Animation Patterns

### Choosing a Curve

```rust
use flui_types::animation::Curves;

// UI animations (fast start, smooth end)
let curve = Curves::EASE_OUT;

// Element appearance
let curve = Curves::EASE_OUT_CUBIC;

// Element disappearance
let curve = Curves::EASE_IN_CUBIC;

// Bounce (for game UI)
let curve = Curves::BOUNCE_OUT;

// Spring (for drag & drop)
let curve = Curves::ELASTIC_OUT;

// Linear (for progress bars)
let curve = Curves::LINEAR;
```

### Value Interpolation

```rust
use flui_types::animation::Curves;

fn animate_value<T: Lerp>(from: T, to: T, t: f32, curve: impl Curve) -> T {
    let curved_t = curve.transform(t);
    from.lerp(&to, curved_t)
}

// Usage
let position = animate_value(
    Point::new(0.0, 0.0),
    Point::new(100.0, 50.0),
    0.5,  // 50% of animation
    Curves::EASE_OUT,
);
```

### Animation Chains

```rust
use flui_types::animation::{Curves, Interval};

// First half — one animation
let first_half = Interval::new(0.0, 0.5, Curves::EASE_OUT);

// Second half — another
let second_half = Interval::new(0.5, 1.0, Curves::EASE_IN);
```

---

## Painting Patterns

### Colors

```rust
use flui_types::painting::{Color, Colors};

// Pattern 1: Predefined colors
let primary = Colors::BLUE;

// Pattern 2: Custom with transparency
let overlay = Color::rgba(0, 0, 0, 128);  // 50% black

// Pattern 3: Modify existing
let lighter = primary.with_alpha(128);
let dimmed = primary.with_opacity(0.5);
```

### Gradients

```rust
use flui_types::painting::{LinearGradient, RadialGradient};
use flui_types::layout::Alignment;

// Vertical gradient (top to bottom)
let vertical = LinearGradient::new(
    Alignment::TOP_CENTER,
    Alignment::BOTTOM_CENTER,
    vec![Colors::WHITE, Colors::BLACK],
);

// Horizontal gradient
let horizontal = LinearGradient::new(
    Alignment::CENTER_LEFT,
    Alignment::CENTER_RIGHT,
    vec![Colors::RED, Colors::BLUE],
);

// Diagonal gradient
let diagonal = LinearGradient::new(
    Alignment::TOP_LEFT,
    Alignment::BOTTOM_RIGHT,
    vec![Colors::PURPLE, Colors::ORANGE],
);

// Radial from center
let radial = RadialGradient::new(
    Alignment::CENTER,
    0.5,  // radius
    vec![Colors::WHITE, Colors::TRANSPARENT],
);
```

### BoxDecoration

```rust
use flui_types::painting::{BoxDecoration, BoxShadow, Border, BorderRadius};

// Card with shadow
let card = BoxDecoration {
    color: Some(Colors::WHITE),
    border_radius: Some(BorderRadius::all(Radius::circular(8.0))),
    box_shadow: vec![
        BoxShadow {
            color: Color::rgba(0, 0, 0, 25),
            offset: Offset::new(0.0, 2.0),
            blur_radius: 4.0,
            spread_radius: 0.0,
        }
    ],
    ..Default::default()
};

// Button with border
let button = BoxDecoration {
    color: Some(Colors::BLUE),
    border: Some(Border::all(BorderSide {
        color: Colors::BLUE_DARK,
        width: 1.0,
    })),
    border_radius: Some(BorderRadius::all(Radius::circular(4.0))),
    ..Default::default()
};
```

---

## Input Handling Patterns

### Hit Testing

```rust
use flui_types::gestures::HitTestBehavior;

// Pattern 1: Transparent to touches (passes through)
let behavior = HitTestBehavior::Translucent;

// Pattern 2: Absorbs all touches
let behavior = HitTestBehavior::Opaque;

// Pattern 3: Only responds to direct hits
let behavior = HitTestBehavior::DeferToChild;
```

### Velocity Tracking

```rust
use flui_types::gestures::{Velocity, VelocityTracker};
use std::time::Duration;

let mut tracker = VelocityTracker::new();

// On each move event
tracker.add_position(Duration::from_millis(0), Point::new(0.0, 0.0));
tracker.add_position(Duration::from_millis(16), Point::new(10.0, 5.0));
tracker.add_position(Duration::from_millis(32), Point::new(25.0, 12.0));

// On release
if let Some(velocity) = tracker.get_velocity() {
    // velocity.pixels_per_second — speed in px/s
    if velocity.pixels_per_second.distance() > 100.0 {
        // Fast swipe!
    }
}
```

---

## Anti-patterns

### Creation in Hot Paths

```rust
// ❌ Bad: creating EdgeInsets every frame
fn layout(&self) -> EdgeInsets {
    EdgeInsets::symmetric(16.0, 8.0)  // Allocation every call
}

// ✅ Good: constant
const PADDING: EdgeInsets = EdgeInsets::symmetric_const(16.0, 8.0);
fn layout(&self) -> EdgeInsets {
    PADDING
}
```

### Redundant Computations

```rust
// ❌ Bad: computing center twice
let center1 = rect.center();
let center2 = rect.center();

// ✅ Good: cache it
let center = rect.center();
// use center twice
```

### Ignoring NaN/Infinity

```rust
// ❌ Bad: no validation
let size = Size::new(width / divisor, height);

// ✅ Good: validate
let size = Size::new(
    if divisor != 0.0 { width / divisor } else { 0.0 },
    height,
);
assert!(size.is_finite());
```

### Incorrect Alignment Usage

```rust
// ❌ Bad: forgot that Alignment is -1 to 1
let align = Alignment::new(0.5, 0.5);  // This is NOT center!

// ✅ Good: use constants or remember the range
let align = Alignment::CENTER;  // (0.0, 0.0)
let align = Alignment::new(0.0, 0.0);  // Equivalent
```

### Mixing FractionalOffset and Alignment

```rust
// ❌ Bad: confusing coordinate systems
let fractional = FractionalOffset::new(0.5, 0.5);  // This is center
let alignment = Alignment::new(0.5, 0.5);  // This is NOT center!

// ✅ Good: explicit conversion
let alignment = fractional.to_alignment();  // Alignment(0.0, 0.0)
```

---

## Type Composition

### Custom Type with Geometry

```rust
use flui_types::geometry::{Rect, Point, Size, Offset};
use flui_types::painting::Color;

#[derive(Clone, Copy, Debug)]
pub struct ColoredRect {
    pub rect: Rect,
    pub color: Color,
}

impl ColoredRect {
    pub fn new(rect: Rect, color: Color) -> Self {
        Self { rect, color }
    }
    
    pub fn contains(&self, point: Point) -> bool {
        self.rect.contains(point)
    }
    
    pub fn translate(&self, offset: Offset) -> Self {
        Self {
            rect: self.rect.translate(offset),
            color: self.color,
        }
    }
}
```

### Custom Decoration

```rust
use flui_types::painting::{BoxDecoration, Color, BorderRadius, Radius};

pub struct Theme {
    pub primary: Color,
    pub surface: Color,
    pub border_radius: f32,
}

impl Theme {
    pub fn card_decoration(&self) -> BoxDecoration {
        BoxDecoration {
            color: Some(self.surface),
            border_radius: Some(BorderRadius::all(
                Radius::circular(self.border_radius)
            )),
            ..Default::default()
        }
    }
    
    pub fn button_decoration(&self) -> BoxDecoration {
        BoxDecoration {
            color: Some(self.primary),
            border_radius: Some(BorderRadius::all(
                Radius::circular(self.border_radius / 2.0)
            )),
            ..Default::default()
        }
    }
}
```

---

## See Also

- [GUIDE.md](GUIDE.md) — Complete Guide
- [ARCHITECTURE.md](ARCHITECTURE.md) — Architecture
- [PERFORMANCE.md](PERFORMANCE.md) — Optimization
- [CHEATSHEET.md](CHEATSHEET.md) — Quick Reference
