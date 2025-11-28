# flui_painting

[![Crates.io](https://img.shields.io/crates/v/flui_painting)](https://crates.io/crates/flui_painting)
[![Documentation](https://docs.rs/flui_painting/badge.svg)](https://docs.rs/flui_painting)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](https://github.com/flui-org/flui)

**High-performance Canvas API for recording 2D drawing commands into optimized display lists for GPU rendering.**

FLUI Painting provides a high-level Canvas API that records drawing operations into efficient DisplayLists. It serves as the bridge between FLUI's rendering layer and the GPU engine, following the Command Pattern for deferred execution.

## Features

- **Canvas API** - Intuitive drawing interface with state management
- **Display Lists** - Immutable command sequences for efficient GPU execution
- **Paint Objects** - Configurable styling (colors, strokes, blend modes)
- **Path Operations** - Complex shapes with Bezier curves and arcs
- **Image Drawing** - Images, textures, 9-slice, tiling, and filtering
- **Zero-Copy Composition** - Efficient parent-child canvas merging
- **Transform Stack** - Hierarchical coordinate transformations (translate, rotate, scale, skew)
- **Clipping** - Rectangle, rounded rectangle, and path clipping with ClipOp support
- **Advanced Effects** - Gradients, shadows, shader masks, backdrop filters
- **Thread-Safe** - Canvas is `Send`, DisplayList is `Send + Clone`
- **Chaining API** - Fluent builder-style methods for concise code
- **Batch Drawing** - Draw multiple shapes in a single call
- **Debug Helpers** - Visual debugging tools for development

## Architecture

```text
RenderObject (flui_rendering)
    | calls paint()
Canvas API (flui_painting - this crate)
    | records commands
DisplayList (immutable)
    | sent to GPU thread
WgpuPainter (flui_engine - executes commands)
```

## Quick Start

Add to your `Cargo.toml`:

```toml
[dependencies]
flui_painting = "0.1"
flui_types = "0.1"
```

### Basic Canvas Usage

```rust
use flui_painting::{Canvas, Paint};
use flui_types::{geometry::Rect, styling::Color};

// Create a canvas
let mut canvas = Canvas::new();

// Draw shapes
let rect = Rect::from_ltrb(10.0, 10.0, 100.0, 50.0);
let paint = Paint::fill(Color::BLUE);
canvas.draw_rect(rect, &paint);

// Finish and get display list for GPU
let display_list = canvas.finish();
```

## Transform Operations

### Basic Transforms

```rust
use flui_painting::Canvas;
use flui_types::geometry::Transform;
use std::f32::consts::PI;

let mut canvas = Canvas::new();

// Translation
canvas.translate(100.0, 50.0);

// Rotation (radians, counter-clockwise)
canvas.rotate(PI / 4.0);  // 45 degrees

// Rotation around pivot point
canvas.rotate_around(PI / 2.0, center_x, center_y);

// Uniform scaling
canvas.scale_uniform(2.0);

// Non-uniform scaling
canvas.scale_xy(2.0, 0.5);  // Stretch horizontally, compress vertically

// Skew transform (horizontal and vertical shear)
canvas.skew(0.2, 0.0);  // Horizontal shear (~11.3 degrees)
canvas.skew(0.0, 0.3);  // Vertical shear
canvas.skew(0.2, 0.1);  // Combined shear

// Using high-level Transform API
canvas.transform(Transform::rotate_around(PI / 2.0, 50.0, 50.0));
```

### Save/Restore Pattern

```rust
let mut canvas = Canvas::new();

// Initial save count is 1
assert_eq!(canvas.save_count(), 1);

canvas.save();
assert_eq!(canvas.save_count(), 2);

canvas.translate(100.0, 50.0);
canvas.rotate(PI / 4.0);
canvas.draw_rect(rect, &paint);

canvas.restore();
assert_eq!(canvas.save_count(), 1);

// Restore to specific save count
canvas.save();  // count = 2
canvas.save();  // count = 3
canvas.save();  // count = 4
canvas.restore_to_count(2);  // Restores back to count = 2
```

## Clipping

### Basic Clipping

```rust
canvas.clip_rect(Rect::from_ltrb(0.0, 0.0, 100.0, 100.0));
canvas.clip_rrect(RRect::from_rect_circular(rect, 10.0));
canvas.clip_path(&path);
```

### Extended Clipping with ClipOp

```rust
use flui_types::painting::{Clip, ClipOp};

// Intersect (default) - new clip intersects with existing
canvas.clip_rect_ext(rect, ClipOp::Intersect, Clip::AntiAlias);

// Difference - subtract from existing clip region
canvas.clip_rect_ext(hole_rect, ClipOp::Difference, Clip::AntiAlias);

// Same for rounded rectangles and paths
canvas.clip_rrect_ext(rrect, ClipOp::Intersect, Clip::HardEdge);
canvas.clip_path_ext(&path, ClipOp::Difference, Clip::AntiAliasWithSaveLayer);
```

### Query Clip State

```rust
let local_bounds = canvas.local_clip_bounds();
let device_bounds = canvas.device_clip_bounds();

// Culling optimization
if canvas.would_be_clipped(&rect) == Some(true) {
    // Skip drawing - rect is outside clip
}
```

## Drawing Primitives

### Basic Shapes

```rust
canvas.draw_line(p1, p2, &paint);
canvas.draw_rect(rect, &paint);
canvas.draw_rrect(rrect, &paint);
canvas.draw_circle(center, radius, &paint);
canvas.draw_oval(rect, &paint);
canvas.draw_path(&path, &paint);

// Advanced primitives
canvas.draw_arc(rect, start_angle, sweep_angle, use_center, &paint);
canvas.draw_drrect(outer, inner, &paint);  // Ring/border
canvas.draw_points_with_mode(PointMode::Polygon, points, &paint);
```

### Convenience Shapes

```rust
// Rounded rectangle with uniform radius
canvas.draw_rounded_rect(rect, 10.0, &paint);

// Rounded rectangle with per-corner radii
canvas.draw_rounded_rect_corners(rect, 5.0, 10.0, 15.0, 20.0, &paint);

// Pill shape (fully rounded ends)
canvas.draw_pill(rect, &paint);

// Ring (circle with hole)
canvas.draw_ring(center, outer_radius, inner_radius, &paint);
```

### Batch Drawing

```rust
// Draw multiple rectangles
let rects = [rect1, rect2, rect3];
canvas.draw_rects(&rects, &paint);

// Draw multiple circles
let circles = [(center1, radius1), (center2, radius2)];
canvas.draw_circles(&circles, &paint);

// Draw multiple lines
let lines = [(p1, p2), (p3, p4)];
canvas.draw_lines(&lines, &paint);
```

### Fill Entire Canvas

```rust
// Fill with solid color or gradient
let background = Paint::fill(Color::from_rgb(30, 30, 30));
canvas.draw_paint(&background);
```

## Text and Images

### Text

```rust
canvas.draw_text("Hello", offset, &text_style, &paint);
```

### Images

```rust
canvas.draw_image(image, dst_rect, Some(&paint));
canvas.draw_image_repeat(image, dst, ImageRepeat::Repeat, None);
canvas.draw_image_nine_slice(image, center_slice, dst, None);
canvas.draw_image_filtered(image, dst, ColorFilter::grayscale(), None);

// External textures (video, camera)
canvas.draw_texture(texture_id, dst, src, FilterQuality::Medium, 1.0);
```

### Replay DisplayList

```rust
// Record once, replay many times
let icon = Canvas::record(|c| {
    c.draw_circle(Point::new(16.0, 16.0), 14.0, &outline);
    c.draw_path(&checkmark, &fill);
});

// Replay the recorded picture
canvas.draw_picture(&icon);

// Or replay with offset
canvas.with_translate(50.0, 0.0, |c| {
    c.draw_picture(&icon);
});
```

## Layers and Effects

### Save Layer

```rust
// Group drawings with opacity
canvas.save_layer(Some(bounds), &Paint::new().with_opacity(0.5));
canvas.draw_rect(rect1, &red_paint);
canvas.draw_rect(rect2, &blue_paint);
canvas.restore(); // Composites at 50% opacity

// Convenience methods
canvas.save_layer_opacity(Some(bounds), 0.5);
canvas.save_layer_blend(Some(bounds), BlendMode::Multiply);
```

### Gradients

```rust
use flui_types::painting::Shader;

let gradient = Shader::linear_gradient(
    Offset::new(0.0, 0.0),
    Offset::new(200.0, 0.0),
    vec![Color::RED, Color::BLUE],
    None,
);
canvas.draw_gradient(rect, gradient);
canvas.draw_gradient_rrect(rrect, gradient);
```

### Shader Mask

```rust
canvas.draw_shader_mask(bounds, shader, BlendMode::SrcOver, |child| {
    child.draw_rect(rect, &paint);
});
```

### Backdrop Filter

```rust
canvas.draw_backdrop_filter(bounds, ImageFilter::blur(10.0), BlendMode::SrcOver, Some(|child| {
    child.draw_rect(panel_rect, &semi_transparent_paint);
}));
```

## Scoped Operations (Closure-based API)

The `with_*` methods provide safe, ergonomic alternatives to manual save/restore:

```rust
// Automatic save/restore - state is always restored
canvas.with_save(|c| {
    c.translate(100.0, 100.0);
    c.rotate(PI / 4.0);
    c.draw_rect(rect, &paint);
});
// Transform is back to original here

// Transform shortcuts
canvas.with_translate(100.0, 50.0, |c| { /* ... */ });
canvas.with_rotate(PI / 4.0, |c| { /* ... */ });
canvas.with_rotate_around(PI / 2.0, 50.0, 50.0, |c| { /* ... */ });
canvas.with_scale(2.0, |c| { /* ... */ });
canvas.with_scale_xy(2.0, 0.5, |c| { /* ... */ });
canvas.with_transform(Transform::rotate(PI / 4.0), |c| { /* ... */ });

// Clipping
canvas.with_clip_rect(clip_rect, |c| { /* ... */ });
canvas.with_clip_rrect(rounded_rect, |c| { /* ... */ });
canvas.with_clip_path(&path, |c| { /* ... */ });

// Layers with effects
canvas.with_opacity(0.5, Some(bounds), |c| { /* ... */ });
canvas.with_blend_mode(BlendMode::Multiply, Some(bounds), |c| { /* ... */ });

// Nested operations
canvas.with_translate(100.0, 100.0, |c| {
    c.with_rotate(PI / 4.0, |c| {
        c.with_scale(2.0, |c| {
            c.draw_rect(rect, &paint);
        });
    });
});

// Return values from closures
let bounds = canvas.with_save(|c| {
    c.translate(50.0, 50.0);
    c.draw_rect(rect, &paint);
    c.bounds()
});
```

## Chaining API

Fluent builder-style methods that return `&mut Self`:

```rust
canvas
    .translated(100.0, 50.0)
    .rotated(PI / 4.0)
    .scaled(2.0)
    .rect(rect, &paint)
    .circle(center, radius, &paint)
    .restored();

// With transforms
canvas
    .saved()
    .translated(50.0, 50.0)
    .rotated_around(PI / 2.0, 25.0, 25.0)
    .scaled_xy(2.0, 0.5)
    .transformed(Transform::skew(0.1, 0.0))
    .rect(rect, &paint)
    .restored();

// With clipping
canvas
    .saved()
    .clipped_rect(viewport)
    .clipped_rrect(inner_rounded)
    .clipped_path(&custom_path)
    .rect(rect, &paint)
    .restored();

// Drawing shapes
canvas
    .rect(rect1, &paint1)
    .rrect(rrect, &paint2)
    .rounded_rect(rect, 10.0, &paint)  // uniform corner radius
    .circle(center, radius, &paint3)
    .oval(oval_rect, &paint4)
    .line(p1, p2, &stroke)
    .path(&custom_path, &fill)
    .arc(rect, start_angle, sweep_angle, use_center, &paint)
    .drrect(outer_rrect, inner_rrect, &paint)  // ring/border
    .text("Hello", offset, &style, &paint);

// Images and textures
canvas
    .image(img, dst_rect, Some(&paint))
    .image_repeat(img, dst, ImageRepeat::Repeat, None)
    .image_nine_slice(img, center_slice, dst, None)
    .image_filtered(img, dst, ColorFilter::grayscale(), None)
    .texture(texture_id, dst, None, FilterQuality::Medium, 1.0);

// Effects
canvas
    .shadow(&path, shadow_color, elevation)
    .gradient(rect, linear_gradient)
    .gradient_rrect(rrect, radial_gradient);

// Points and vertices
canvas
    .points(PointMode::Polygon, points, &paint)
    .vertices(verts, Some(colors), None, indices, &paint);

// Conditional drawing
canvas
    .also(|c| {
        // Execute arbitrary code
        c.draw_rect(rect, &paint);
    })
    .when(show_border, |c| {
        c.draw_rect(border_rect, &border_paint);
    })
    .when_else(is_selected,
        |c| c.draw_rect(rect, &selected_paint),
        |c| c.draw_rect(rect, &normal_paint),
    );
```

## Conditional and Grid Drawing

### Conditional Drawing

```rust
// Draw only if condition is true
canvas.draw_if(is_visible, |c| {
    c.draw_rect(rect, &paint);
});

// Draw only if option is Some
canvas.draw_if_some(maybe_image, |c, image| {
    c.draw_image(image, rect, None);
});

// Draw only if rect is not clipped
canvas.draw_rect_if(rect, &paint, |r| !r.is_empty());
```

### Grid and Repeat Patterns

```rust
// Draw grid of items
canvas.draw_grid(4, 3, 50.0, 50.0, |c, row, col| {
    let color = if (row + col) % 2 == 0 { Color::WHITE } else { Color::BLACK };
    c.draw_rect(Rect::from_xywh(0.0, 0.0, 45.0, 45.0), &Paint::fill(color));
});

// Repeat horizontally
canvas.repeat_x(5, 60.0, |c, i| {
    c.draw_circle(Point::new(25.0, 25.0), 20.0, &paint);
});

// Repeat vertically
canvas.repeat_y(5, 60.0, |c, i| {
    c.draw_circle(Point::new(25.0, 25.0), 20.0, &paint);
});

// Repeat radially (around a circle)
canvas.repeat_radial(8, 100.0, |c, i| {
    c.draw_circle(Point::ZERO, 15.0, &paint);
});
```

## Debug Helpers

Visual debugging tools for development:

```rust
// Debug rectangle with 1px stroke
canvas.debug_rect(rect, Color::RED);

// Debug point marker
canvas.debug_point(point, 5.0, Color::GREEN);

// Debug coordinate axes
canvas.debug_axes(100.0);  // Draws X (red) and Y (green) axes

// Debug grid overlay
canvas.debug_grid(bounds, 50.0, Color::from_rgba(128, 128, 128, 128));
```

## Factory Methods

```rust
// Create and record in one call
let icon = Canvas::record(|c| {
    c.draw_circle(Point::new(16.0, 16.0), 14.0, &outline);
    c.draw_path(&checkmark, &fill);
});

// Build canvas with initial setup
let mut canvas = Canvas::build(|c| {
    c.translate(100.0, 100.0);
    c.clip_rect(viewport);
});
canvas.draw_rect(rect, &paint);
```

## Canvas Composition

```rust
// Zero-copy append (efficient for parent-child)
let mut parent = Canvas::new();
parent.draw_rect(background, &bg_paint);

let child = render_child();
parent.append_canvas(child);  // Zero-copy move

// With opacity
parent.append_canvas_with_opacity(child, 0.5);

// From cached display list
parent.append_display_list(cached);
parent.append_display_list_at_offset(&cached, offset);
```

## Canvas Query Methods

```rust
let mut canvas = Canvas::new();
canvas.draw_rect(rect, &paint);

// Query state
assert!(!canvas.is_empty());
assert_eq!(canvas.len(), 1);

// Save count (initial = 1)
assert_eq!(canvas.save_count(), 1);

// Get bounds of all recorded commands
let bounds = canvas.bounds();

// Get current transform matrix
let matrix = canvas.transform_matrix();

// Reset canvas for reuse (preserves allocations)
canvas.reset();
```

## Display List

The DisplayList is an immutable sequence of drawing commands with powerful querying capabilities:

### Basic Usage

```rust
let display_list = canvas.finish();

// Query
assert!(!display_list.is_empty());
println!("Commands: {}", display_list.len());
println!("Bounds: {:?}", display_list.bounds());

// Iterate all commands
for cmd in display_list.commands() {
    match cmd {
        DrawCommand::DrawRect { rect, paint, transform } => {
            // Process rect command
        }
        _ => {}
    }
}
```

### Command Type Discrimination

```rust
use flui_painting::CommandKind;

// Check command kind
for cmd in display_list.commands() {
    match cmd.kind() {
        CommandKind::Draw => println!("Drawing command"),
        CommandKind::Clip => println!("Clipping command"),
        CommandKind::Effect => println!("Effect command"),
        CommandKind::Layer => println!("Layer command"),
    }

    // Type-specific checks
    if cmd.is_shape() { /* rect, circle, path, etc. */ }
    if cmd.is_image() { /* image, texture */ }
    if cmd.is_text() { /* text command */ }
    if cmd.is_clip() { /* clipping command */ }
}
```

### Filtered Iterators

```rust
// Iterate only drawing commands
for cmd in display_list.draw_commands() { /* ... */ }

// Iterate only clipping commands
for cmd in display_list.clip_commands() { /* ... */ }

// Iterate only shape commands (rects, circles, paths, etc.)
for cmd in display_list.shape_commands() { /* ... */ }

// Iterate only image commands
for cmd in display_list.image_commands() { /* ... */ }

// Iterate only text commands
for cmd in display_list.text_commands() { /* ... */ }
```

### Statistics

```rust
// Quick count by kind
let (draw, clip, effect, layer) = display_list.count_by_kind();

// Detailed statistics
let stats = display_list.stats();
println!("{}", stats);
// Output: DisplayList: 15 commands (draw: 10, clip: 2, effect: 1, layer: 2) | shapes: 8, images: 1, text: 1, hits: 0

// Access individual stats
println!("Total: {}", stats.total);
println!("Shapes: {}", stats.shapes);
println!("Hit regions: {}", stats.hit_regions);
```

### Transform Operations

```rust
// Apply transform to all commands
let mut display_list = canvas.finish();
display_list.apply_transform(Matrix4::translation(50.0, 50.0, 0.0));

// Transform individual command
let cmd = &mut display_list.commands_mut().next().unwrap();
cmd.apply_transform(Matrix4::rotation_z(PI / 4.0));

// Access command transform
if let Some(transform) = cmd.transform() {
    println!("Transform: {:?}", transform);
}
```

### Filter and Map

```rust
// Filter commands
let shapes_only = display_list.filter(|cmd| cmd.is_shape());

// Map/transform commands
let translated = display_list.map(|mut cmd| {
    cmd.apply_transform(Matrix4::translation(10.0, 10.0, 0.0));
    cmd
});
```

### Effects

```rust
// Apply opacity to all commands
let faded = display_list.to_opacity(0.5);
```

### Hit Regions

```rust
// Hit regions for event handling
for region in display_list.hit_regions() {
    if region.contains(pointer_pos) {
        (region.handler)(&event);
    }
}
```

## Thread Safety

```rust
// Canvas is Send (can be moved to another thread)
let canvas = Canvas::new();
std::thread::spawn(move || {
    let display_list = canvas.finish();
});

// DisplayList is Send + Clone
let display_list = canvas.finish();
let dl_clone = display_list.clone();
std::thread::spawn(move || {
    render(display_list);
});
```

## Performance Tips

1. **Use `append_canvas`** for parent-child composition - it's zero-copy
2. **Query `would_be_clipped`** before expensive drawing operations
3. **Use `reset()`** to reuse canvas allocations across frames
4. **Batch similar operations** - use `draw_rects`, `draw_circles`, `draw_lines`
5. **Cache DisplayLists** for static content (use `draw_picture` to replay)
6. **Use chaining API** - reduces method call overhead
7. **Use `draw_if`** - skip drawing logic when condition is false
8. **Query `stats()`** - profile command distribution

## API Reference

### Transform Methods

| Method | Description |
|--------|-------------|
| `translate(dx, dy)` | Translate coordinate system |
| `rotate(radians)` | Rotate around origin |
| `rotate_around(radians, px, py)` | Rotate around pivot point |
| `scale_uniform(factor)` | Uniform scaling |
| `scale_xy(sx, sy)` | Non-uniform scaling |
| `skew(sx, sy)` | Horizontal and vertical shear |
| `transform(transform)` | Apply Transform enum |
| `set_transform(transform)` | Replace current transform |

### State Management

| Method | Description |
|--------|-------------|
| `save()` | Push state to stack |
| `restore()` | Pop state from stack |
| `save_count()` | Get current save count (initial = 1) |
| `restore_to_count(count)` | Restore to specific save count |
| `save_layer(bounds, paint)` | Save with layer effects |
| `save_layer_opacity(bounds, opacity)` | Save with opacity |
| `save_layer_blend(bounds, mode)` | Save with blend mode |

### Clipping Methods

| Method | Description |
|--------|-------------|
| `clip_rect(rect)` | Clip to rectangle |
| `clip_rrect(rrect)` | Clip to rounded rectangle |
| `clip_path(path)` | Clip to path |
| `clip_rect_ext(rect, op, behavior)` | Clip with ClipOp and anti-alias |
| `clip_rrect_ext(rrect, op, behavior)` | Clip with ClipOp and anti-alias |
| `clip_path_ext(path, op, behavior)` | Clip with ClipOp and anti-alias |

### Query Methods

| Method | Description |
|--------|-------------|
| `is_empty()` | Check if no commands recorded |
| `len()` | Number of commands |
| `bounds()` | Bounding box of all commands |
| `transform_matrix()` | Current transform matrix |
| `local_clip_bounds()` | Clip bounds in local coordinates |
| `device_clip_bounds()` | Clip bounds in device coordinates |
| `would_be_clipped(rect)` | Check if rect would be clipped |

## Documentation

### Guides

- **[Architecture Guide](docs/ARCHITECTURE.md)** - Internal architecture and design patterns
- **[Performance Guide](docs/PERFORMANCE.md)** - Optimization techniques and benchmarking
- **[Migration Guide](docs/MIGRATION.md)** - Upgrading between versions
- **[Contributing Guide](CONTRIBUTING.md)** - How to contribute
- **[Changelog](CHANGELOG.md)** - Version history and changes

### API Reference

- **[docs.rs](https://docs.rs/flui_painting)** - Full API documentation
- `cargo doc --open` - Build and view documentation locally

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](../../LICENSE-APACHE))
- MIT License ([LICENSE-MIT](../../LICENSE-MIT))

at your option.

## Related Crates

- [`flui_types`](../flui_types) - Geometry, colors, and painting types
- [`flui_engine`](../flui_engine) - GPU rendering engine (executes DisplayLists)
- [`flui_rendering`](../flui_rendering) - RenderObject system that uses Canvas

---

**FLUI Painting** - Expressive 2D graphics with GPU-optimized performance.
