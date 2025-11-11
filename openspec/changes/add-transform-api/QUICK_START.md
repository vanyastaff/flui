# Transform API - Quick Start Guide

Quick reference for using the Transform API in FLUI.

## Basic Usage

### Import
```rust
use flui_types::geometry::Transform;
use std::f32::consts::PI;
```

### Simple Transforms

```rust
// Translation (move)
let translate = Transform::translate(50.0, 100.0);

// Rotation (in radians)
let rotate = Transform::rotate(PI / 4.0);  // 45 degrees

// Uniform scale
let scale = Transform::scale(2.0);  // 2x size

// Non-uniform scale
let scale_xy = Transform::scale_xy(2.0, 0.5);  // Stretch horizontally

// Skew (for italic text, perspective)
let italic = Transform::skew(0.2, 0.0);  // Horizontal skew
```

### Composition (Combining Transforms)

```rust
// Fluent API - chain with .then()
let transform = Transform::translate(50.0, 50.0)
    .then(Transform::rotate(PI / 4.0))
    .then(Transform::scale(2.0));

// Order matters: translate → rotate → scale
```

### Advanced Transforms

```rust
// Rotate around a pivot point
let rotate_around = Transform::rotate_around(
    PI / 4.0,  // angle
    100.0,     // pivot_x (center of rotation)
    100.0      // pivot_y
);

// Scale around a pivot point
let scale_around = Transform::scale_around(
    2.0,   // x scale
    2.0,   // y scale
    50.0,  // pivot_x
    50.0   // pivot_y
);
```

## Canvas Integration

```rust
use flui_painting::Canvas;

let mut canvas = Canvas::new();

// Save/restore pattern (recommended)
canvas.save();
canvas.transform(Transform::rotate(PI / 4.0));
canvas.draw_rect(rect, &paint);
canvas.restore();

// Multiple transforms
canvas.save();
canvas.transform(Transform::translate(50.0, 50.0));
canvas.transform(Transform::rotate(PI / 4.0));
canvas.draw_circle(center, radius, &paint);
canvas.restore();
```

## RenderObject Usage

```rust
use flui_rendering::RenderTransform;

// Create with Transform (recommended)
let render = RenderTransform::new(
    Transform::rotate(PI / 4.0)
);

// With pivot/alignment
let render = RenderTransform::with_alignment(
    Transform::scale(2.0),
    Offset::new(0.5, 0.5)  // Center alignment
);

// Backward compatibility with Matrix4
let render = RenderTransform::from_matrix(matrix);
```

## Conversion to Matrix4

```rust
// Automatic conversion (recommended)
canvas.transform(Transform::rotate(PI / 4.0));

// Explicit conversion via Into trait
let matrix: Matrix4 = Transform::rotate(PI / 4.0).into();

// Reference conversion (no move)
let transform = Transform::scale(2.0);
let matrix: Matrix4 = (&transform).into();

// Convenience method
let matrix = transform.to_matrix();
```

## Query Methods

```rust
let transform = Transform::translate(10.0, 20.0);

// Check transform type
if transform.is_identity() {
    // No transformation needed
}

if transform.has_translation() {
    // Transform includes translation
}

if transform.has_rotation() {
    // Transform includes rotation
}

if transform.has_scale() {
    // Transform includes scaling
}
```

## Inverse Transforms

```rust
let transform = Transform::rotate(PI / 4.0);

// Inverse returns Option (Some transforms aren't invertible)
if let Some(inverse) = transform.inverse() {
    // Apply inverse transformation
    canvas.transform(inverse);
}

// Scale by zero is NOT invertible
let scale_zero = Transform::scale(0.0);
assert!(scale_zero.inverse().is_none());
```

## Common Patterns

### UI Container Transform
```rust
// Center a widget and scale it
let container_transform = Transform::translate(width / 2.0, height / 2.0)
    .then(Transform::scale(1.2))
    .then(Transform::translate(-widget_width / 2.0, -widget_height / 2.0));
```

### Button Hover Animation
```rust
// Scale up slightly on hover
let hover_transform = Transform::scale_around(
    1.05,  // 5% larger
    1.05,
    button_center_x,
    button_center_y
);
```

### Italic Text
```rust
// Skew text horizontally for italic effect
let italic = Transform::skew(0.2, 0.0);
canvas.transform(italic);
canvas.draw_text(text, position, font_size, &paint);
```

### Card Flip Animation
```rust
// Rotate around Y-axis (perspective)
let flip_angle = PI * animation_progress;  // 0 to PI
let flip = Transform::rotate(flip_angle)
    .then(Transform::scale_xy(1.0, 0.8));  // Perspective scale
```

### Parallax Scrolling
```rust
// Move layers at different speeds
let background = Transform::translate(scroll_x * 0.3, 0.0);
let midground = Transform::translate(scroll_x * 0.6, 0.0);
let foreground = Transform::translate(scroll_x * 1.0, 0.0);
```

## Matrix Decomposition

```rust
// Extract transform components from Matrix4
let transform = Transform::from(matrix);
let (tx, ty, rotation, sx, sy) = transform.decompose();

println!("Translation: ({}, {})", tx, ty);
println!("Rotation: {} radians", rotation);
println!("Scale: ({}, {})", sx, sy);
```

## Performance Tips

1. **Identity Optimization**: Identity transforms are free
   ```rust
   Transform::Identity  // No matrix allocation, no-op
   ```

2. **Composition Flattening**: Nested compositions are automatically flattened
   ```rust
   // These are equivalent in performance:
   let t1 = Transform::translate(10.0, 20.0)
       .then(Transform::rotate(PI / 4.0));

   let t2 = Transform::Compose(vec![
       Transform::translate(10.0, 20.0),
       Transform::rotate(PI / 4.0)
   ]);
   // Both flatten to same representation
   ```

3. **Reuse Transforms**: Store commonly used transforms
   ```rust
   // Good - compute once
   let hover_transform = Transform::scale_around(1.05, 1.05, cx, cy);

   // Bad - recompute every frame
   for _ in 0..1000 {
       let t = Transform::scale_around(1.05, 1.05, cx, cy);
   }
   ```

## Backward Compatibility

All existing Matrix4 code continues to work:

```rust
// Old code - still works
canvas.transform(matrix);

// New code - also works
canvas.transform(Transform::rotate(PI / 4.0));

// Mix and match
canvas.save();
canvas.transform(matrix);              // Matrix4
canvas.transform(Transform::scale(2.0));  // Transform
canvas.restore();
```

## See Also

- Full examples: `examples/transform_demo.rs`
- Skew demo: `examples/test_skew.rs`
- API docs: `CLAUDE.md` (Transform API section)
- Implementation: `crates/flui_types/src/geometry/transform.rs`
