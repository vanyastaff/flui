# flui_painting

[![Crates.io](https://img.shields.io/crates/v/flui_painting)](https://crates.io/crates/flui_painting)
[![Documentation](https://docs.rs/flui_painting/badge.svg)](https://docs.rs/flui_painting)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](https://github.com/flui-org/flui)

**2D graphics primitives and canvas API for FLUI framework - Record drawing commands into optimized display lists for GPU rendering.**

FLUI Painting provides a high-level 2D graphics API that records drawing operations into efficient display lists. It serves as the bridge between FLUI's layout system and the GPU rendering engine, offering both immediate-mode and retained-mode painting patterns.

## Features

- 🎨 **Canvas API** - Familiar HTML5 Canvas-like drawing interface
- 📝 **Display Lists** - Efficient command recording and playback
- 🔧 **Paint Objects** - Configurable styling (colors, gradients, shadows)
- 📐 **Path Operations** - Complex shapes with Bézier curves and arcs
- 🖌️ **Text Rendering** - Rich text layout with font styling
- 🖼️ **Image Drawing** - Optimized image compositing with transformations
- ⚡ **GPU Optimized** - Commands designed for efficient GPU execution
- 🔄 **Transform Stack** - Hierarchical coordinate transformations
- 🎭 **Clipping** - Rectangle, rounded rectangle, and path clipping
- 🌈 **Gradients** - Linear and radial gradient support

## Architecture

FLUI Painting sits between the layout and rendering layers:

```text
┌─────────────────────────────────────────────────────────┐
│                flui_widgets                             │
│            (High-level UI widgets)                     │
└─────────────────┬───────────────────────────────────────┘
                  │
┌─────────────────▼───────────────────────────────────────┐
│               flui_painting                             │
│        (2D graphics primitives & Canvas)               │
│                                                         │
│  Canvas API  │  Display Lists  │  Paint Objects        │
│  Path Ops    │  Text Layout    │  Transformations      │
└─────────────────┬───────────────────────────────────────┘
                  │
┌─────────────────▼───────────────────────────────────────┐
│                flui_engine                              │
│           (GPU rendering & rasterization)              │
└─────────────────────────────────────────────────────────┘
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
use flui_painting::{Canvas, Paint, Path};
use flui_types::{Color, Rect, Point, Size};

// Create a canvas
let mut canvas = Canvas::new(Size::new(800.0, 600.0));

// Create paint objects
let red_paint = Paint::new()
    .color(Color::RED)
    .stroke_width(2.0);

let blue_fill = Paint::new()
    .color(Color::BLUE)
    .style(PaintStyle::Fill);

// Draw shapes
canvas.draw_rect(
    Rect::new(10.0, 10.0, 100.0, 50.0),
    &red_paint
);

canvas.draw_circle(
    Point::new(200.0, 100.0),
    25.0,
    &blue_fill
);

// Generate display list for GPU rendering
let display_list = canvas.finalize();
```

### Path Drawing

```rust
use flui_painting::{Canvas, Paint, Path};

let mut canvas = Canvas::new(Size::new(400.0, 400.0));
let paint = Paint::new().color(Color::GREEN).stroke_width(3.0);

// Create a custom path
let mut path = Path::new();
path.move_to(Point::new(50.0, 50.0));
path.line_to(Point::new(150.0, 50.0));
path.quadratic_to(
    Point::new(200.0, 100.0),  // Control point
    Point::new(150.0, 150.0)   // End point
);
path.close();

canvas.draw_path(&path, &paint);
```

### Text Rendering

```rust
use flui_painting::{Canvas, Paint, TextStyle};
use flui_types::{FontWeight, Point};

let mut canvas = Canvas::new(Size::new(300.0, 200.0));

let text_paint = Paint::new().color(Color::BLACK);
let text_style = TextStyle::new()
    .font_family("Arial")
    .font_size(24.0)
    .font_weight(FontWeight::Bold);

canvas.draw_text(
    "Hello, FLUI!",
    Point::new(50.0, 100.0),
    &text_paint,
    &text_style
);
```

## Core Components

### Canvas

The main drawing surface that records all drawing operations:

```rust
use flui_painting::Canvas;
use flui_types::Size;

// Create canvas with specific size
let mut canvas = Canvas::new(Size::new(1920.0, 1080.0));

// Drawing operations
canvas.save();  // Save current state
canvas.translate(100.0, 100.0);
canvas.rotate(std::f32::consts::PI / 4.0);  // 45 degrees
// ... draw operations with transforms applied
canvas.restore();  // Restore previous state

// Clipping
canvas.clip_rect(Rect::new(0.0, 0.0, 200.0, 200.0));
// ... subsequent draws are clipped to rectangle

// Get final display list
let display_list = canvas.finalize();
```

### Paint Objects

Configure how shapes and text are rendered:

```rust
use flui_painting::{Paint, PaintStyle, BlendMode};
use flui_types::{Color, Gradient};

// Solid color fill
let fill_paint = Paint::new()
    .color(Color::BLUE)
    .style(PaintStyle::Fill);

// Stroked outline
let stroke_paint = Paint::new()
    .color(Color::RED)
    .style(PaintStyle::Stroke)
    .stroke_width(2.0)
    .stroke_cap(StrokeCap::Round)
    .stroke_join(StrokeJoin::Miter);

// Gradient fill
let gradient = Gradient::linear(
    Point::new(0.0, 0.0),
    Point::new(100.0, 100.0),
    vec![
        (0.0, Color::RED),
        (0.5, Color::GREEN),
        (1.0, Color::BLUE),
    ]
);

let gradient_paint = Paint::new()
    .gradient(gradient)
    .style(PaintStyle::Fill);

// Shadow effect
let shadow_paint = Paint::new()
    .color(Color::BLACK.with_alpha(0.3))
    .blur_radius(5.0)
    .offset(Point::new(2.0, 2.0));
```

### Display Lists

Efficient command recording and playback:

```rust
use flui_painting::{DisplayList, DisplayListBuilder};

// Manual display list creation
let mut builder = DisplayListBuilder::new();
builder.push_transform(Matrix4::scale(2.0));
builder.push_rect(rect, &paint);
builder.push_text("Hello", point, &paint, &style);
let display_list = builder.build();

// Display list operations
println!("Command count: {}", display_list.len());
println!("Estimated GPU memory: {} bytes", display_list.gpu_memory_size());

// Iterate through commands
for command in display_list.commands() {
    match command {
        DrawCommand::Rect { rect, paint } => {
            println!("Drawing rectangle: {:?}", rect);
        }
        DrawCommand::Text { text, position, .. } => {
            println!("Drawing text '{}' at {:?}", text, position);
        }
        _ => {}
    }
}

// Optimize display list
let optimized = display_list.optimize();
```

## Advanced Drawing

### Complex Paths

```rust
use flui_painting::{Path, PathEffect};

let mut path = Path::new();

// Move and line operations
path.move_to(Point::new(10.0, 10.0));
path.line_to(Point::new(100.0, 10.0));
path.line_to(Point::new(100.0, 100.0));

// Curves
path.quadratic_to(
    Point::new(150.0, 100.0),  // Control point
    Point::new(150.0, 50.0)    // End point
);

path.cubic_to(
    Point::new(200.0, 50.0),   // Control point 1
    Point::new(200.0, 150.0),  // Control point 2
    Point::new(150.0, 150.0)   // End point
);

// Arcs
path.arc_to(
    Point::new(100.0, 150.0),  // End point
    Point::new(75.0, 125.0),   // Center through point
    25.0                        // Radius
);

path.close();

// Path effects
let dashed_paint = Paint::new()
    .color(Color::BLACK)
    .path_effect(PathEffect::dash(&[10.0, 5.0], 0.0));

canvas.draw_path(&path, &dashed_paint);
```

### Image Drawing

```rust
use flui_painting::Canvas;
use flui_types::{Rect, Matrix4};

// Draw image at original size
canvas.draw_image(&image, Point::new(100.0, 100.0));

// Draw image scaled to fit rectangle
canvas.draw_image_rect(
    &image,
    None,  // Use entire source image
    Rect::new(0.0, 0.0, 200.0, 150.0),  // Destination
    &Paint::new()
);

// Draw with transformation
canvas.save();
canvas.transform(&Matrix4::rotation_z(0.5));
canvas.draw_image(&image, Point::ZERO);
canvas.restore();

// Draw with custom paint (tinting, blending)
let tinted_paint = Paint::new()
    .color(Color::RED.with_alpha(0.5))
    .blend_mode(BlendMode::Multiply);

canvas.draw_image_rect(
    &image,
    None,
    dest_rect,
    &tinted_paint
);
```

### Text Layout

```rust
use flui_painting::{TextLayout, TextStyle, Paragraph};

// Simple text
let style = TextStyle::new()
    .font_family("Helvetica")
    .font_size(16.0)
    .color(Color::BLACK);

canvas.draw_text("Simple text", point, &Paint::new(), &style);

// Rich text paragraph
let mut paragraph = Paragraph::new();
paragraph.add_text("Hello ", &TextStyle::new().color(Color::BLACK));
paragraph.add_text("World", &TextStyle::new()
    .color(Color::RED)
    .font_weight(FontWeight::Bold));

let layout = paragraph.layout(300.0); // Max width
canvas.draw_paragraph(&layout, Point::new(50.0, 50.0));

// Text with custom baseline
canvas.draw_text_on_path(
    "Text following path",
    &path,
    0.0,  // Distance along path
    &Paint::new(),
    &style
);
```

### Gradients and Effects

```rust
use flui_painting::{Gradient, Shadow, MaskFilter};
use flui_types::{Point, Color};

// Linear gradient
let linear = Gradient::linear(
    Point::new(0.0, 0.0),
    Point::new(100.0, 0.0),
    vec![
        (0.0, Color::RED),
        (1.0, Color::BLUE),
    ]
);

// Radial gradient
let radial = Gradient::radial(
    Point::new(50.0, 50.0),  // Center
    50.0,                     // Radius
    vec![
        (0.0, Color::WHITE),
        (1.0, Color::BLACK),
    ]
);

// Conical gradient (sweep)
let conical = Gradient::conical(
    Point::new(100.0, 100.0),  // Center
    0.0,                        // Start angle
    vec![
        (0.0, Color::RED),
        (0.33, Color::GREEN),
        (0.66, Color::BLUE),
        (1.0, Color::RED),
    ]
);

// Apply gradients
let gradient_paint = Paint::new()
    .gradient(linear)
    .style(PaintStyle::Fill);

canvas.draw_rect(rect, &gradient_paint);

// Shadows
let shadow = Shadow::new()
    .color(Color::BLACK.with_alpha(0.5))
    .offset(Point::new(3.0, 3.0))
    .blur_radius(5.0);

let shadow_paint = Paint::new()
    .color(Color::BLUE)
    .shadow(shadow);

canvas.draw_circle(center, radius, &shadow_paint);

// Blur effects
let blur_paint = Paint::new()
    .color(Color::GREEN)
    .mask_filter(MaskFilter::blur(3.0));

canvas.draw_rect(rect, &blur_paint);
```

## Coordinate Systems and Transformations

### Transform Stack

```rust
// Save/restore pattern
canvas.save();
    canvas.translate(100.0, 50.0);
    canvas.rotate(0.785); // 45 degrees
    canvas.scale(1.5, 1.0);
    // Draw operations use combined transform
    canvas.draw_rect(rect, &paint);
canvas.restore(); // Restore previous transform

// Manual transform manipulation
let transform = Matrix4::translation(50.0, 50.0) 
    * Matrix4::rotation_z(0.5)
    * Matrix4::scale(2.0);

canvas.save();
canvas.transform(&transform);
canvas.draw_circle(Point::ZERO, 20.0, &paint);
canvas.restore();
```

### Coordinate Conversion

```rust
use flui_types::{Point, Matrix4};

// Get current transform
let current_transform = canvas.transform();

// Transform points between coordinate systems
let local_point = Point::new(10.0, 20.0);
let global_point = current_transform.transform_point(local_point);

// Inverse transform
let inverse = current_transform.inverse().unwrap();
let back_to_local = inverse.transform_point(global_point);
```

## Clipping

### Rectangle Clipping

```rust
// Simple rectangle clip
canvas.clip_rect(Rect::new(50.0, 50.0, 200.0, 150.0));

// Rounded rectangle clip
canvas.clip_rounded_rect(
    Rect::new(50.0, 50.0, 200.0, 150.0),
    10.0  // Corner radius
);

// With anti-aliasing
canvas.clip_rect_with_aa(rect, true);
```

### Path Clipping

```rust
// Create clipping path
let mut clip_path = Path::new();
clip_path.add_oval(Rect::new(0.0, 0.0, 100.0, 100.0));

// Apply clip
canvas.clip_path(&clip_path, ClipOp::Intersect);

// Multiple clips combine
canvas.clip_rect(Rect::new(25.0, 25.0, 75.0, 75.0));
// Effective clip is intersection of circle and rectangle
```

## Performance Optimization

### Display List Optimization

```rust
use flui_painting::{DisplayList, OptimizationFlags};

let display_list = canvas.finalize();

// Optimize for GPU performance
let optimized = display_list.optimize_with_flags(
    OptimizationFlags::MERGE_ADJACENT_RECTS
        | OptimizationFlags::CULL_OFFSCREEN_COMMANDS
        | OptimizationFlags::BATCH_SIMILAR_PAINTS
);

// Analyze performance characteristics
let stats = display_list.analyze();
println!("Draw calls: {}", stats.draw_call_count);
println!("GPU memory: {} KB", stats.gpu_memory_kb);
println!("CPU time estimate: {:?}", stats.cpu_time_estimate);
```

### Batch Operations

```rust
// Batch similar operations
canvas.begin_batch();
for i in 0..100 {
    let rect = Rect::new(i as f32 * 10.0, 0.0, 8.0, 8.0);
    canvas.draw_rect(rect, &paint); // Batched automatically
}
canvas.end_batch();

// Manual command batching
let rects = vec![/* many rectangles */];
canvas.draw_rects(&rects, &paint); // Single draw call
```

### Memory Management

```rust
// Reuse canvas to avoid allocations
let mut canvas = Canvas::new(size);

for frame in 0..1000 {
    canvas.clear(Color::WHITE);
    
    // Draw frame content
    draw_frame_content(&mut canvas, frame);
    
    let display_list = canvas.finalize_and_reset();
    // Canvas is ready for reuse
    
    render_display_list(display_list);
}
```

## Integration with FLUI

### Widget Painting

```rust
use flui_painting::Canvas;
use flui_core::{RenderObject, PaintContext};

struct MyRenderObject {
    color: Color,
    border_width: f32,
}

impl RenderObject for MyRenderObject {
    fn paint(&self, context: &PaintContext) {
        let canvas = context.canvas();
        let size = context.size();
        
        // Background
        let bg_paint = Paint::new()
            .color(self.color)
            .style(PaintStyle::Fill);
        
        canvas.draw_rect(
            Rect::from_size(size),
            &bg_paint
        );
        
        // Border
        if self.border_width > 0.0 {
            let border_paint = Paint::new()
                .color(Color::BLACK)
                .style(PaintStyle::Stroke)
                .stroke_width(self.border_width);
            
            canvas.draw_rect(
                Rect::from_size(size),
                &border_paint
            );
        }
    }
}
```

### Custom Painters

```rust
use flui_painting::{CustomPainter, Canvas};

struct ChartPainter {
    data: Vec<f32>,
    line_color: Color,
}

impl CustomPainter for ChartPainter {
    fn paint(&self, canvas: &mut Canvas, size: Size) {
        let width = size.width;
        let height = size.height;
        
        let paint = Paint::new()
            .color(self.line_color)
            .stroke_width(2.0)
            .style(PaintStyle::Stroke);
        
        let mut path = Path::new();
        
        for (i, &value) in self.data.iter().enumerate() {
            let x = (i as f32) * width / (self.data.len() as f32 - 1.0);
            let y = height - (value * height);
            
            if i == 0 {
                path.move_to(Point::new(x, y));
            } else {
                path.line_to(Point::new(x, y));
            }
        }
        
        canvas.draw_path(&path, &paint);
    }
}

// Use in widget
CustomPaint::new()
    .painter(ChartPainter {
        data: vec![0.1, 0.3, 0.8, 0.4, 0.6],
        line_color: Color::BLUE,
    })
    .size(Size::new(300.0, 200.0))
```

## Testing

Test your painting code with visual regression testing:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use flui_painting::testing::*;

    #[test]
    fn test_rect_drawing() {
        let mut canvas = Canvas::new(Size::new(100.0, 100.0));
        let paint = Paint::new().color(Color::RED);
        
        canvas.draw_rect(
            Rect::new(10.0, 10.0, 50.0, 30.0),
            &paint
        );
        
        let display_list = canvas.finalize();
        
        // Check command count
        assert_eq!(display_list.len(), 1);
        
        // Check command type
        match &display_list.commands()[0] {
            DrawCommand::Rect { rect, paint } => {
                assert_eq!(rect.width(), 50.0);
                assert_eq!(rect.height(), 30.0);
                assert_eq!(paint.color(), Color::RED);
            }
            _ => panic!("Expected rect command"),
        }
    }

    #[test]
    fn test_visual_output() {
        let mut canvas = Canvas::new(Size::new(200.0, 200.0));
        
        // Draw test pattern
        draw_test_pattern(&mut canvas);
        
        let display_list = canvas.finalize();
        
        // Render to pixel buffer for comparison
        let pixels = render_to_pixels(&display_list, Size::new(200.0, 200.0));
        
        // Compare with reference image
        assert_visual_match!("test_pattern", pixels);
    }
}
```

## Contributing

We welcome contributions to FLUI Painting! See [CONTRIBUTING.md](../../CONTRIBUTING.md) for guidelines.

### Development

```bash
# Run tests
cargo test -p flui_painting

# Run with visual tests
cargo test -p flui_painting --features visual-tests

# Run benchmarks
cargo bench -p flui_painting

# Check documentation
cargo doc -p flui_painting --open
```

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](../../LICENSE-APACHE))
- MIT License ([LICENSE-MIT](../../LICENSE-MIT))

at your option.

## Related Crates

- [`flui_types`](../flui_types) - Basic geometry and math types used throughout
- [`flui_engine`](../flui_engine) - GPU rendering engine that consumes display lists
- [`flui_widgets`](../flui_widgets) - High-level widgets that use painting operations
- [`flui_core`](../flui_core) - Core framework providing the RenderObject system

---

**FLUI Painting** - Expressive 2D graphics with GPU-optimized performance.