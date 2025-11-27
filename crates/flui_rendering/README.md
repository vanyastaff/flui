# flui_rendering

[![Crates.io](https://img.shields.io/crates/v/flui_rendering)](https://crates.io/crates/flui_rendering)
[![Documentation](https://docs.rs/flui_rendering/badge.svg)](https://docs.rs/flui_rendering)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](https://github.com/flui-org/flui)

**Rendering infrastructure for FLUI using the Generic Three-Tree Architecture - Provides RenderObject implementations for layout and painting.**

FLUI Rendering implements the render layer of FLUI's three-tree architecture, providing concrete RenderObject implementations that handle layout calculations and painting operations. It uses trait abstractions from `flui-tree` to remain independent of concrete element tree implementations.

## Features

- 🏗️ **Generic Three-Tree Architecture** - Independent of concrete element implementations
- 📐 **Layout Protocols** - Box model and sliver-based layout systems  
- 🎨 **Paint Operations** - Efficient painting with display list generation
- 🖱️ **Hit Testing** - Precise input event targeting
- 📦 **80+ RenderObjects** - Comprehensive library of rendering primitives
- ⚡ **Type-Erased Storage** - Uniform storage with compile-time optimization
- 🔧 **Callback-Based Operations** - Layout/paint via closures for flexibility
- 🚀 **Zero-Cost Abstractions** - Generic types compile to concrete code

## Migration Status

**Current Progress: 79/82 RenderObjects migrated to new arity-based API (96% complete)**

FLUI Rendering has successfully migrated to a modern compile-time arity checking system, replacing the legacy runtime API with type-safe RenderBox<Arity> and SliverRender<Arity> traits.

### Migration Summary

- **Box Objects**: 54/56 migrated (96%)
- **Sliver Objects**: 25/26 migrated (96%)
- **Total**: 79/82 objects (96%)
- **Testing**: 681 unit tests passing
- **Code Quality**: Clippy clean with `-D warnings`

### Completed Migration Phases

1. ✅ **Phase 1: Quick Wins** - 6 objects (RenderEditableLine + 5 sliver proxies)
2. ✅ **Phase 2: Sliver Single Manual** - 5 objects (padding, fill remaining, box adapter, etc.)
3. ✅ **Phase 3: Critical Sliver Foundation** - 3 objects (RenderSliver base, multi-box adaptor, list)
4. ✅ **Phase 4: Variable Box Objects** - 2 objects (RenderFlow, RenderCustomMultiChildLayoutBox)
5. ✅ **Phase 5: Essential Slivers** - 3 objects (fixed extent list, grid, fill viewport)
6. ✅ **Phase 6: Complex Variable Box** - 3 objects (table, list wheel viewport, viewport stub)
7. ✅ **Phase 7: Advanced Slivers** - 8 objects (app bars, headers, grouping, safe area)

### What Was Migrated

**62 RenderBox implementations:**
- Leaf arity: Text, images, custom paint, decorated boxes, etc.
- Single arity: Containers, padding, transform, opacity, clip effects, etc.
- Variable arity: Flex layouts, stack, wrap, table, flow, etc.

**17 SliverRender implementations:**
- Single arity: Padding, fill remaining, box adapter, app bars, persistent headers, safe area
- Variable arity: List, grid, fixed extent list, fill viewport, grouping, prototype extent list

### Deferred Objects (3)

The following objects are deferred pending infrastructure work:
- **RenderAbstractViewport** - Trait definition only, no concrete implementation needed
- **RenderShrinkWrappingViewport** - Placeholder stub, requires full viewport infrastructure
- **RenderOverflowIndicator** - Requires painting infrastructure enhancements

For detailed migration documentation, see [`docs/plan.md`](docs/plan.md) and the OpenSpec proposal at [`openspec/changes/migrate-renderobjects-to-new-api/`](../../../openspec/changes/migrate-renderobjects-to-new-api/).

## Architecture

FLUI Rendering sits between the tree abstractions and concrete implementations:

```text
┌─────────────────────────────────────────────────────────┐
│                    flui-tree                            │
│            (Abstract tree traits)                      │
│  TreeRead │ TreeNav │ TreeWrite │ RenderTreeAccess     │
└─────────────────┬───────────────────────────────────────┘
                  │
┌─────────────────▼───────────────────────────────────────┐
│                flui_rendering                           │
│              (This crate)                               │
│                                                         │
│  RenderObject │ RenderBox<A> │ SliverRender<A>        │
│  LayoutTree   │ PaintTree    │ HitTestTree             │
└─────────────────┬───────────────────────────────────────┘
                  │
┌─────────────────▼───────────────────────────────────────┐
│               flui-pipeline                             │
│         (Concrete implementations)                     │
└─────────────────────────────────────────────────────────┘
```

## Quick Start

Add to your `Cargo.toml`:

```toml
[dependencies]
flui_rendering = "0.1"
flui_types = "0.1"
flui_painting = "0.1"
```

### Basic RenderObject Implementation

```rust
use flui_rendering::core::{RenderObject, RenderBox, BoxConstraints, Size};
use flui_rendering::view::LayoutContext;
use flui_types::{Rect, Color};
use flui_painting::{Canvas, Paint};

pub struct RenderColoredBox {
    color: Color,
    size: Size,
}

impl RenderColoredBox {
    pub fn new(color: Color) -> Self {
        Self {
            color,
            size: Size::ZERO,
        }
    }
}

impl RenderObject for RenderColoredBox {
    fn layout(&mut self, context: &LayoutContext) {
        // Get constraints from context
        let constraints = context.constraints::<BoxConstraints>();
        
        // Calculate size
        self.size = constraints.biggest();
    }
    
    fn paint(&self, canvas: &mut Canvas, offset: flui_types::Offset) {
        let paint = Paint::new().color(self.color);
        let rect = Rect::from_size(self.size).translate(offset);
        canvas.draw_rect(rect, &paint);
    }
    
    fn hit_test(&self, position: flui_types::Point) -> bool {
        Rect::from_size(self.size).contains(position)
    }
}

impl RenderBox<flui_rendering::arity::Leaf> for RenderColoredBox {
    fn perform_layout(&mut self, constraints: BoxConstraints) -> Size {
        self.size = constraints.biggest();
        self.size
    }
}
```

## Core Components

### RenderObject Trait

The base trait for all renderable objects:

```rust
use flui_rendering::core::RenderObject;

pub trait RenderObject: std::any::Any + Send + Sync {
    /// Perform layout calculations
    fn layout(&mut self, context: &LayoutContext);
    
    /// Paint the object to canvas
    fn paint(&self, canvas: &mut Canvas, offset: Offset);
    
    /// Test if a point hits this object
    fn hit_test(&self, position: Point) -> bool;
    
    /// Get object's computed size
    fn size(&self) -> Size;
    
    /// Get debug information
    fn debug_info(&self) -> Vec<(&'static str, String)> {
        vec![]
    }
}
```

### Box Protocol (RenderBox)

For traditional box model layouts:

```rust
use flui_rendering::core::{RenderBox, BoxConstraints};
use flui_rendering::arity::{Leaf, SingleChild, MultiChild};

// Leaf render object (no children)
impl RenderBox<Leaf> for MyLeafRender {
    fn perform_layout(&mut self, constraints: BoxConstraints) -> Size {
        // Calculate size based on constraints
        constraints.constrain(self.intrinsic_size())
    }
}

// Single child render object
impl RenderBox<SingleChild> for MyContainerRender {
    fn perform_layout(
        &mut self, 
        constraints: BoxConstraints,
        child_layout: impl FnOnce(BoxConstraints) -> Size
    ) -> Size {
        // Layout child with modified constraints
        let child_constraints = constraints.deflate(self.padding);
        let child_size = child_layout(child_constraints);
        
        // Calculate our size including padding
        Size::new(
            child_size.width + self.padding.horizontal(),
            child_size.height + self.padding.vertical()
        )
    }
}

// Multi-child render object  
impl RenderBox<MultiChild> for MyFlexRender {
    fn perform_layout(
        &mut self,
        constraints: BoxConstraints,
        child_layout: impl Fn(usize, BoxConstraints) -> Size
    ) -> Size {
        let mut total_height = 0.0;
        let max_width = constraints.max_width;
        
        // Layout each child
        for i in 0..self.child_count() {
            let child_constraints = BoxConstraints::tight_for_width(max_width);
            let child_size = child_layout(i, child_constraints);
            total_height += child_size.height;
        }
        
        Size::new(max_width, total_height)
    }
}
```

### Sliver Protocol (SliverRender)

For scrollable content layouts:

```rust
use flui_rendering::core::{SliverRender, SliverConstraints, SliverGeometry};

impl SliverRender<MultiChild> for RenderSliverList {
    fn perform_layout(
        &mut self,
        constraints: SliverConstraints,
        child_layout: impl Fn(usize, BoxConstraints) -> Size
    ) -> SliverGeometry {
        let mut scroll_extent = 0.0;
        let mut paint_extent = 0.0;
        
        // Layout visible children
        let start_index = self.calculate_start_index(constraints.scroll_offset);
        let end_index = self.calculate_end_index(constraints.remaining_paint_extent);
        
        for i in start_index..=end_index {
            let child_constraints = BoxConstraints::tight_for_width(
                constraints.cross_axis_extent
            );
            let child_size = child_layout(i, child_constraints);
            scroll_extent += child_size.height;
            
            if paint_extent < constraints.remaining_paint_extent {
                paint_extent += child_size.height;
            }
        }
        
        SliverGeometry {
            scroll_extent,
            paint_extent,
            max_paint_extent: paint_extent,
            hit_test_extent: paint_extent,
        }
    }
}
```

## Built-in RenderObjects

### Basic Objects

```rust
use flui_rendering::objects::basic::*;

// Container with decoration
let container = RenderDecoratedBox::new()
    .decoration(BoxDecoration::new()
        .color(Color::BLUE)
        .border(Border::all(2.0, Color::BLACK))
        .border_radius(BorderRadius::circular(8.0)))
    .child(child_render);

// Padding container
let padded = RenderPadding::new(EdgeInsets::all(16.0))
    .child(child_render);

// Sized box
let sized = RenderConstrainedBox::new()
    .constraints(BoxConstraints::tight(Size::new(200.0, 100.0)))
    .child(child_render);
```

### Layout Objects

```rust
use flui_rendering::objects::layout::*;

// Flex layout (Row/Column)
let flex = RenderFlex::new()
    .direction(Axis::Vertical)
    .main_axis_alignment(MainAxisAlignment::SpaceEvenly)
    .cross_axis_alignment(CrossAxisAlignment::Center)
    .children(vec![child1, child2, child3]);

// Stack layout
let stack = RenderStack::new()
    .alignment(Alignment::TopLeft)
    .children(vec![
        (bottom_child, StackPosition::Fill),
        (top_child, StackPosition::Positioned { 
            left: Some(10.0), 
            top: Some(20.0),
            width: None,
            height: None 
        }),
    ]);

// Wrap layout
let wrap = RenderWrap::new()
    .direction(Axis::Horizontal)
    .spacing(8.0)
    .run_spacing(12.0)
    .children(vec![child1, child2, child3, child4]);
```

### Effects Objects

```rust
use flui_rendering::objects::effects::*;

// Opacity effect
let transparent = RenderOpacity::new(0.5)
    .child(child_render);

// Transform effect
let rotated = RenderTransform::new()
    .transform(Matrix4::rotation_z(std::f32::consts::PI / 4.0))
    .alignment(Alignment::Center)
    .child(child_render);

// Clip effects
let clipped = RenderClipRRect::new()
    .border_radius(BorderRadius::circular(16.0))
    .child(child_render);

// Shadow effect
let shadowed = RenderPhysicalModel::new()
    .color(Color::WHITE)
    .shadow_color(Color::BLACK.with_alpha(0.3))
    .elevation(4.0)
    .child(child_render);
```

### Text Objects

```rust
use flui_rendering::objects::text::*;

// Rich text paragraph
let text = RenderParagraph::new()
    .text("Hello, world!")
    .style(TextStyle::new()
        .font_size(16.0)
        .color(Color::BLACK)
        .font_weight(FontWeight::Normal))
    .text_align(TextAlign::Start)
    .max_lines(None);

// Selectable text
let selectable = RenderSelectableText::new()
    .text("Selectable text content")
    .style(text_style)
    .selection_color(Color::BLUE.with_alpha(0.3))
    .cursor_color(Color::BLUE);
```

### Media Objects

```rust
use flui_rendering::objects::media::*;

// Image display
let image = RenderImage::new()
    .image(image_data)
    .fit(BoxFit::Cover)
    .alignment(Alignment::Center)
    .filter_quality(FilterQuality::High);

// Custom painting
let custom = RenderCustomPaint::new()
    .painter(Box::new(|canvas, size| {
        // Custom painting logic
        let paint = Paint::new().color(Color::RED);
        canvas.draw_circle(
            Point::new(size.width / 2.0, size.height / 2.0),
            50.0,
            &paint
        );
    }))
    .size(Size::new(200.0, 200.0));
```

### Sliver Objects

```rust
use flui_rendering::objects::sliver::*;

// Sliver app bar
let app_bar = RenderSliverAppBar::new()
    .title("My App")
    .background_color(Color::BLUE)
    .elevation(4.0)
    .floating(true)
    .pinned(true);

// Sliver list
let list = RenderSliverList::new()
    .delegate(Box::new(|index| {
        RenderContainer::new()
            .height(60.0)
            .color(if index % 2 == 0 { Color::WHITE } else { Color::GRAY_50 })
            .child(RenderText::new(format!("Item {}", index)))
    }))
    .item_count(Some(100));

// Sliver grid
let grid = RenderSliverGrid::new()
    .delegate(grid_delegate)
    .cross_axis_count(2)
    .main_axis_spacing(8.0)
    .cross_axis_spacing(8.0);
```

## Advanced Usage

### Custom RenderObject with State

```rust
use flui_rendering::core::{RenderObject, RenderBox};
use flui_types::{Size, Color, Point};

pub struct RenderProgressBar {
    progress: f32, // 0.0 to 1.0
    color: Color,
    background_color: Color,
    size: Size,
}

impl RenderProgressBar {
    pub fn new(progress: f32) -> Self {
        Self {
            progress: progress.clamp(0.0, 1.0),
            color: Color::BLUE,
            background_color: Color::GRAY_200,
            size: Size::ZERO,
        }
    }
    
    pub fn progress(mut self, progress: f32) -> Self {
        self.progress = progress.clamp(0.0, 1.0);
        self
    }
    
    pub fn color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }
}

impl RenderObject for RenderProgressBar {
    fn layout(&mut self, context: &LayoutContext) {
        let constraints = context.constraints::<BoxConstraints>();
        self.size = constraints.biggest();
    }
    
    fn paint(&self, canvas: &mut Canvas, offset: Offset) {
        let rect = Rect::from_size(self.size).translate(offset);
        
        // Draw background
        let bg_paint = Paint::new().color(self.background_color);
        canvas.draw_rect(rect, &bg_paint);
        
        // Draw progress
        let progress_width = self.size.width * self.progress;
        let progress_rect = Rect::new(
            rect.left(),
            rect.top(),
            rect.left() + progress_width,
            rect.bottom()
        );
        
        let progress_paint = Paint::new().color(self.color);
        canvas.draw_rect(progress_rect, &progress_paint);
    }
    
    fn hit_test(&self, position: Point) -> bool {
        Rect::from_size(self.size).contains(position)
    }
    
    fn size(&self) -> Size {
        self.size
    }
}

impl RenderBox<flui_rendering::arity::Leaf> for RenderProgressBar {
    fn perform_layout(&mut self, constraints: BoxConstraints) -> Size {
        self.size = constraints.constrain(Size::new(200.0, 8.0));
        self.size
    }
}
```

### Custom Sliver Implementation

```rust
use flui_rendering::core::{SliverRender, SliverConstraints, SliverGeometry};

pub struct RenderSliverHeader {
    height: f32,
    background_color: Color,
    pinned: bool,
}

impl SliverRender<flui_rendering::arity::Leaf> for RenderSliverHeader {
    fn perform_layout(&mut self, constraints: SliverConstraints) -> SliverGeometry {
        let paint_extent = if self.pinned {
            self.height.min(constraints.remaining_paint_extent)
        } else {
            (self.height - constraints.scroll_offset).max(0.0)
                .min(constraints.remaining_paint_extent)
        };
        
        SliverGeometry {
            scroll_extent: self.height,
            paint_extent,
            max_paint_extent: self.height,
            hit_test_extent: paint_extent,
        }
    }
}
```

### Performance Optimization

```rust
use flui_rendering::core::{RenderObject, RepaintBoundary};

// Create repaint boundary to isolate expensive repaints
pub struct RenderExpensiveWidget {
    // ... expensive rendering state
    needs_repaint: bool,
}

impl RenderObject for RenderExpensiveWidget {
    fn paint(&self, canvas: &mut Canvas, offset: Offset) {
        // Only repaint if needed
        if self.needs_repaint {
            // Expensive painting operations
            self.paint_complex_graphics(canvas, offset);
        }
    }
}

// Use repaint boundary to isolate
let bounded = RepaintBoundary::new()
    .child(RenderExpensiveWidget::new());
```

## Integration with FLUI Framework

### Using with Elements

```rust
use flui_core::{Element, RenderElement};
use flui_rendering::objects::basic::RenderContainer;

// Create render object
let render_object = RenderContainer::new()
    .color(Color::BLUE)
    .padding(EdgeInsets::all(16.0));

// Wrap in element
let element = RenderElement::new(render_object);
```

### Custom Widget with RenderObject

```rust
use flui_core::{Widget, RenderObjectWidget};
use flui_rendering::objects::basic::RenderColoredBox;

pub struct ColoredBox {
    pub color: Color,
    pub child: Option<Box<dyn Widget>>,
}

impl RenderObjectWidget for ColoredBox {
    type RenderObject = RenderColoredBox;
    
    fn create_render_object(&self) -> Self::RenderObject {
        RenderColoredBox::new(self.color)
    }
    
    fn update_render_object(&self, render_object: &mut Self::RenderObject) {
        render_object.color = self.color;
    }
}
```

## Testing

Test your render objects with layout and visual regression testing:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use flui_rendering::testing::*;

    #[test]
    fn test_progress_bar_layout() {
        let mut progress_bar = RenderProgressBar::new(0.5);
        let constraints = BoxConstraints::tight(Size::new(200.0, 20.0));
        
        let size = progress_bar.perform_layout(constraints);
        
        assert_eq!(size.width, 200.0);
        assert_eq!(size.height, 20.0);
    }

    #[test]
    fn test_progress_bar_painting() {
        let progress_bar = RenderProgressBar::new(0.75)
            .color(Color::GREEN);
        
        let mut canvas = TestCanvas::new(Size::new(100.0, 10.0));
        progress_bar.paint(&mut canvas, Offset::ZERO);
        
        // Verify background rect
        assert_canvas_contains_rect(&canvas, 
            Rect::new(0.0, 0.0, 100.0, 10.0),
            Color::GRAY_200);
        
        // Verify progress rect
        assert_canvas_contains_rect(&canvas,
            Rect::new(0.0, 0.0, 75.0, 10.0),
            Color::GREEN);
    }

    #[test]
    fn test_hit_testing() {
        let progress_bar = RenderProgressBar::new(0.5);
        let mut context = LayoutContext::new(
            BoxConstraints::tight(Size::new(100.0, 20.0))
        );
        
        progress_bar.layout(&context);
        
        // Test points inside bounds
        assert!(progress_bar.hit_test(Point::new(50.0, 10.0)));
        assert!(progress_bar.hit_test(Point::new(0.0, 0.0)));
        assert!(progress_bar.hit_test(Point::new(99.0, 19.0)));
        
        // Test points outside bounds
        assert!(!progress_bar.hit_test(Point::new(-1.0, 10.0)));
        assert!(!progress_bar.hit_test(Point::new(50.0, -1.0)));
        assert!(!progress_bar.hit_test(Point::new(101.0, 10.0)));
    }
}
```

## Performance Characteristics

- **Type-Erased Storage**: Uniform storage with minimal overhead
- **Compile-Time Optimization**: Generic arity types optimize to concrete code
- **Efficient Layout**: Callback-based layout avoids unnecessary allocations
- **Paint Optimization**: Display list generation for GPU efficiency
- **Memory Efficient**: Object pooling and reuse where possible

## Contributing

We welcome contributions to FLUI Rendering! See [CONTRIBUTING.md](../../CONTRIBUTING.md) for guidelines.

### Development

```bash
# Run tests
cargo test -p flui_rendering

# Run with all features
cargo test -p flui_rendering --all-features

# Run benchmarks
cargo bench -p flui_rendering

# Check documentation
cargo doc -p flui_rendering --open
```

### Adding New RenderObjects

1. Choose appropriate protocol (RenderBox or SliverRender)
2. Determine arity (Leaf, SingleChild, or MultiChild) 
3. Implement required traits
4. Add comprehensive tests
5. Update documentation
6. Add usage examples

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](../../LICENSE-APACHE))
- MIT License ([LICENSE-MIT](../../LICENSE-MIT))

at your option.

## Related Crates

- [`flui-tree`](../flui-tree) - Abstract tree traits used by this crate
- [`flui_painting`](../flui_painting) - 2D graphics and canvas API
- [`flui_types`](../flui_types) - Basic geometry and layout types
- [`flui_core`](../flui_core) - Core framework that consumes render objects
- [`flui_widgets`](../flui_widgets) - High-level widgets built on render objects

---

**FLUI Rendering** - Flexible, efficient, and type-safe rendering primitives for modern UI frameworks.