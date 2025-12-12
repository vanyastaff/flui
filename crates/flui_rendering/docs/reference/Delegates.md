# Delegates

**Custom behavior delegates for extensible render objects**

---

## Overview

FLUI provides 6 delegate traits that allow users to customize layout, painting, and clipping behavior without subclassing. Delegates are pure Rust traits with minimal boilerplate, providing type-safe extensibility.

---

## Delegate Types

| Delegate | Purpose | Used By |
|----------|---------|---------|
| **CustomPainter** | Custom painting | RenderCustomPaint |
| **CustomClipper** | Custom clipping shapes | RenderClip* objects |
| **SingleChildLayoutDelegate** | Custom single-child layout | RenderCustomSingleChildLayoutBox |
| **MultiChildLayoutDelegate** | Custom multi-child layout | RenderCustomMultiChildLayoutBox |
| **FlowDelegate** | Flow layout algorithm | RenderFlow |
| **SliverGridDelegate** | Grid layout in slivers | RenderSliverGrid |

---

## 1. CustomPainter

Provides custom painting on a canvas.

### Trait Definition

```rust
pub trait CustomPainter: Send + Sync + Debug {
    /// Paint custom content on canvas
    fn paint(&mut self, canvas: &mut Canvas, size: Size);
    
    /// Whether this painter should repaint when replaced
    fn should_repaint(&self, old_delegate: &dyn CustomPainter) -> bool;
    
    /// Hit test at given position (optional)
    fn hit_test(&self, position: Offset) -> bool {
        false
    }
    
    /// Build semantics for accessibility (optional)
    fn semantics_builder(&self) -> Option<SemanticsBuilder> {
        None
    }
    
    /// Whether to rebuild semantics (optional)
    fn should_rebuild_semantics(&self, old_delegate: &dyn CustomPainter) -> bool {
        true
    }
}
```

### Example Implementation

```rust
use skia_safe::{Canvas, Paint, PaintStyle, Color, Rect};

#[derive(Debug)]
pub struct CheckerboardPainter {
    cell_size: f32,
    color1: Color,
    color2: Color,
}

impl CheckerboardPainter {
    pub fn new(cell_size: f32) -> Self {
        Self {
            cell_size,
            color1: Color::WHITE,
            color2: Color::from_rgb(200, 200, 200),
        }
    }
}

impl CustomPainter for CheckerboardPainter {
    fn paint(&mut self, canvas: &mut Canvas, size: Size) {
        let cols = (size.width / self.cell_size).ceil() as i32;
        let rows = (size.height / self.cell_size).ceil() as i32;
        
        let mut paint = Paint::default();
        paint.set_style(PaintStyle::Fill);
        
        for row in 0..rows {
            for col in 0..cols {
                let color = if (row + col) % 2 == 0 {
                    self.color1
                } else {
                    self.color2
                };
                
                paint.set_color(color);
                
                let rect = Rect::from_xywh(
                    col as f32 * self.cell_size,
                    row as f32 * self.cell_size,
                    self.cell_size,
                    self.cell_size,
                );
                
                canvas.draw_rect(rect, &paint);
            }
        }
    }
    
    fn should_repaint(&self, old: &dyn CustomPainter) -> bool {
        // Repaint if any property changed
        if let Some(old) = old.as_any().downcast_ref::<Self>() {
            self.cell_size != old.cell_size ||
            self.color1 != old.color1 ||
            self.color2 != old.color2
        } else {
            true
        }
    }
}
```

### Usage

```rust
let painter = CheckerboardPainter::new(20.0);
let render_custom = RenderCustomPaint::new(Box::new(painter));
```

---

## 2. CustomClipper

Defines custom clipping shapes.

### Trait Definition

```rust
pub trait CustomClipper<T: Clone>: Send + Sync + Debug {
    /// Get the clip shape for the given size
    fn get_clip(&self, size: Size) -> T;
    
    /// Get approximate bounding rect (for optimization)
    fn get_approximate_clip_rect(&self, size: Size) -> Rect {
        Rect::from_size(size)
    }
    
    /// Whether to reclip when delegate changes
    fn should_reclip(&self, old_clipper: &dyn CustomClipper<T>) -> bool;
}
```

### Example: Triangle Clipper

```rust
use skia_safe::Path;

#[derive(Debug)]
pub struct TriangleClipper;

impl CustomClipper<Path> for TriangleClipper {
    fn get_clip(&self, size: Size) -> Path {
        let mut path = Path::new();
        
        // Top center
        path.move_to((size.width / 2.0, 0.0));
        
        // Bottom right
        path.line_to((size.width, size.height));
        
        // Bottom left
        path.line_to((0.0, size.height));
        
        // Close path
        path.close();
        
        path
    }
    
    fn should_reclip(&self, _old_clipper: &dyn CustomClipper<Path>) -> bool {
        false  // Shape never changes
    }
}
```

### Example: Rounded Rectangle Clipper

```rust
use skia_safe::RRect;

#[derive(Debug)]
pub struct RoundedRectClipper {
    border_radius: f32,
}

impl RoundedRectClipper {
    pub fn new(border_radius: f32) -> Self {
        Self { border_radius }
    }
}

impl CustomClipper<RRect> for RoundedRectClipper {
    fn get_clip(&self, size: Size) -> RRect {
        let rect = Rect::from_size(size);
        RRect::new_rect_xy(rect, self.border_radius, self.border_radius)
    }
    
    fn should_reclip(&self, old: &dyn CustomClipper<RRect>) -> bool {
        if let Some(old) = old.as_any().downcast_ref::<Self>() {
            self.border_radius != old.border_radius
        } else {
            true
        }
    }
}
```

### Usage

```rust
let clipper = Box::new(TriangleClipper);
let render_clip = RenderClipPath::new(clipper);
```

---

## 3. SingleChildLayoutDelegate

Custom layout algorithm for single child.

### Trait Definition

```rust
pub trait SingleChildLayoutDelegate: Send + Sync + Debug {
    /// Get the size for given constraints
    fn get_size(&self, constraints: BoxConstraints) -> Size;
    
    /// Get constraints for child
    fn get_constraints_for_child(&self, constraints: BoxConstraints) -> BoxConstraints;
    
    /// Get position for child given parent and child sizes
    fn get_position_for_child(&self, size: Size, child_size: Size) -> Offset;
    
    /// Whether to relayout when delegate changes
    fn should_relayout(&self, old_delegate: &dyn SingleChildLayoutDelegate) -> bool;
}
```

### Example: Aspect Ratio Delegate

```rust
#[derive(Debug)]
pub struct AspectRatioDelegate {
    aspect_ratio: f32,  // width / height
}

impl AspectRatioDelegate {
    pub fn new(aspect_ratio: f32) -> Self {
        Self { aspect_ratio }
    }
}

impl SingleChildLayoutDelegate for AspectRatioDelegate {
    fn get_size(&self, constraints: BoxConstraints) -> Size {
        let width = constraints.max_width;
        let height = width / self.aspect_ratio;
        
        if height <= constraints.max_height {
            Size::new(width, height)
        } else {
            let height = constraints.max_height;
            let width = height * self.aspect_ratio;
            Size::new(width, height)
        }
    }
    
    fn get_constraints_for_child(&self, constraints: BoxConstraints) -> BoxConstraints {
        let size = self.get_size(constraints);
        BoxConstraints::tight(size)
    }
    
    fn get_position_for_child(&self, size: Size, child_size: Size) -> Offset {
        Offset::new(
            (size.width - child_size.width) / 2.0,
            (size.height - child_size.height) / 2.0,
        )
    }
    
    fn should_relayout(&self, old: &dyn SingleChildLayoutDelegate) -> bool {
        if let Some(old) = old.as_any().downcast_ref::<Self>() {
            self.aspect_ratio != old.aspect_ratio
        } else {
            true
        }
    }
}
```

---

## 4. MultiChildLayoutDelegate

Custom layout algorithm for multiple children with IDs.

### Trait Definition

```rust
pub trait MultiChildLayoutDelegate: Send + Sync + Debug {
    /// Check if child with given ID exists
    fn has_child(&self, child_id: &str) -> bool;
    
    /// Layout child and return its size
    fn layout_child(&mut self, child_id: &str, constraints: BoxConstraints) -> Size;
    
    /// Position child at given offset
    fn position_child(&mut self, child_id: &str, offset: Offset);
    
    /// Get the size for given constraints
    fn get_size(&self, constraints: BoxConstraints) -> Size;
    
    /// Perform layout (main entry point)
    fn perform_layout(&mut self, size: Size);
    
    /// Whether to relayout when delegate changes
    fn should_relayout(&self, old_delegate: &dyn MultiChildLayoutDelegate) -> bool;
}
```

### Example: Custom Dialog Delegate

```rust
#[derive(Debug)]
pub struct DialogLayoutDelegate {
    padding: f32,
}

impl DialogLayoutDelegate {
    pub fn new(padding: f32) -> Self {
        Self { padding }
    }
}

impl MultiChildLayoutDelegate for DialogLayoutDelegate {
    fn has_child(&self, child_id: &str) -> bool {
        matches!(child_id, "title" | "content" | "actions")
    }
    
    fn layout_child(&mut self, child_id: &str, constraints: BoxConstraints) -> Size {
        // Implementation handled by RenderCustomMultiChildLayoutBox
        Size::ZERO
    }
    
    fn position_child(&mut self, child_id: &str, offset: Offset) {
        // Implementation handled by RenderCustomMultiChildLayoutBox
    }
    
    fn get_size(&self, constraints: BoxConstraints) -> Size {
        constraints.biggest()
    }
    
    fn perform_layout(&mut self, size: Size) {
        let inner_width = size.width - 2.0 * self.padding;
        let mut y = self.padding;
        
        // Layout title
        if self.has_child("title") {
            let title_constraints = BoxConstraints {
                min_width: inner_width,
                max_width: inner_width,
                min_height: 0.0,
                max_height: f32::INFINITY,
            };
            let title_size = self.layout_child("title", title_constraints);
            self.position_child("title", Offset::new(self.padding, y));
            y += title_size.height + self.padding;
        }
        
        // Layout content
        if self.has_child("content") {
            let content_constraints = BoxConstraints {
                min_width: inner_width,
                max_width: inner_width,
                min_height: 0.0,
                max_height: f32::INFINITY,
            };
            let content_size = self.layout_child("content", content_constraints);
            self.position_child("content", Offset::new(self.padding, y));
            y += content_size.height + self.padding;
        }
        
        // Layout actions
        if self.has_child("actions") {
            let actions_constraints = BoxConstraints {
                min_width: inner_width,
                max_width: inner_width,
                min_height: 0.0,
                max_height: 60.0,
            };
            let actions_size = self.layout_child("actions", actions_constraints);
            self.position_child("actions", Offset::new(self.padding, y));
        }
    }
    
    fn should_relayout(&self, old: &dyn MultiChildLayoutDelegate) -> bool {
        if let Some(old) = old.as_any().downcast_ref::<Self>() {
            self.padding != old.padding
        } else {
            true
        }
    }
}
```

---

## 5. FlowDelegate

Controls flow layout with custom constraints and painting.

### Trait Definition

```rust
pub trait FlowDelegate: Send + Sync + Debug {
    /// Get the size for given constraints
    fn get_size(&self, constraints: BoxConstraints) -> Size;
    
    /// Get constraints for child at index
    fn get_constraints_for_child(&self, i: usize, constraints: BoxConstraints) -> BoxConstraints;
    
    /// Paint children with custom transforms
    fn paint_children(&self, context: FlowPaintingContext);
    
    /// Whether to relayout when delegate changes
    fn should_relayout(&self, old_delegate: &dyn FlowDelegate) -> bool;
    
    /// Whether to repaint when delegate changes
    fn should_repaint(&self, old_delegate: &dyn FlowDelegate) -> bool;
}

pub struct FlowPaintingContext<'a> {
    pub size: Size,
    pub child_count: usize,
    // Methods to paint children with transforms
}

impl<'a> FlowPaintingContext<'a> {
    pub fn child_size(&self, i: usize) -> Size;
    pub fn paint_child(&mut self, i: usize, transform: Matrix4);
}
```

### Example: Circular Flow Delegate

```rust
#[derive(Debug)]
pub struct CircularFlowDelegate {
    radius: f32,
}

impl CircularFlowDelegate {
    pub fn new(radius: f32) -> Self {
        Self { radius }
    }
}

impl FlowDelegate for CircularFlowDelegate {
    fn get_size(&self, constraints: BoxConstraints) -> Size {
        let diameter = self.radius * 2.0;
        constraints.constrain(Size::new(diameter, diameter))
    }
    
    fn get_constraints_for_child(&self, _i: usize, _constraints: BoxConstraints) -> BoxConstraints {
        BoxConstraints::loose(Size::new(100.0, 100.0))
    }
    
    fn paint_children(&self, mut context: FlowPaintingContext) {
        let center_x = self.radius;
        let center_y = self.radius;
        
        for i in 0..context.child_count {
            let angle = 2.0 * std::f32::consts::PI * (i as f32) / (context.child_count as f32);
            let child_size = context.child_size(i);
            
            let x = center_x + self.radius * angle.cos() - child_size.width / 2.0;
            let y = center_y + self.radius * angle.sin() - child_size.height / 2.0;
            
            let transform = Matrix4::translate(x, y, 0.0);
            context.paint_child(i, transform);
        }
    }
    
    fn should_relayout(&self, old: &dyn FlowDelegate) -> bool {
        if let Some(old) = old.as_any().downcast_ref::<Self>() {
            self.radius != old.radius
        } else {
            true
        }
    }
    
    fn should_repaint(&self, old: &dyn FlowDelegate) -> bool {
        self.should_relayout(old)
    }
}
```

---

## 6. SliverGridDelegate

Defines grid layout in slivers.

### Trait Definition

```rust
pub trait SliverGridDelegate: Send + Sync + Debug {
    /// Get the grid layout for given constraints
    fn get_layout(&self, constraints: SliverConstraints) -> SliverGridLayout;
    
    /// Whether to relayout when delegate changes
    fn should_relayout(&self, old_delegate: &dyn SliverGridDelegate) -> bool;
}

pub struct SliverGridLayout {
    pub cross_axis_count: usize,
    pub main_axis_stride: f32,
    pub cross_axis_stride: f32,
    pub child_main_axis_extent: f32,
    pub child_cross_axis_extent: f32,
    pub reverse_cross_axis: bool,
}
```

### Example: Fixed Cross Axis Count

```rust
#[derive(Debug)]
pub struct SliverGridDelegateWithFixedCrossAxisCount {
    cross_axis_count: usize,
    main_axis_spacing: f32,
    cross_axis_spacing: f32,
    child_aspect_ratio: f32,
}

impl SliverGridDelegateWithFixedCrossAxisCount {
    pub fn new(cross_axis_count: usize) -> Self {
        Self {
            cross_axis_count,
            main_axis_spacing: 0.0,
            cross_axis_spacing: 0.0,
            child_aspect_ratio: 1.0,
        }
    }
}

impl SliverGridDelegate for SliverGridDelegateWithFixedCrossAxisCount {
    fn get_layout(&self, constraints: SliverConstraints) -> SliverGridLayout {
        let used_cross_axis = self.cross_axis_spacing * (self.cross_axis_count - 1) as f32;
        let child_cross_axis_extent = 
            (constraints.cross_axis_extent - used_cross_axis) / self.cross_axis_count as f32;
        
        let child_main_axis_extent = child_cross_axis_extent / self.child_aspect_ratio;
        
        SliverGridLayout {
            cross_axis_count: self.cross_axis_count,
            main_axis_stride: child_main_axis_extent + self.main_axis_spacing,
            cross_axis_stride: child_cross_axis_extent + self.cross_axis_spacing,
            child_main_axis_extent,
            child_cross_axis_extent,
            reverse_cross_axis: axis_direction_is_reversed(constraints.cross_axis_direction),
        }
    }
    
    fn should_relayout(&self, old: &dyn SliverGridDelegate) -> bool {
        if let Some(old) = old.as_any().downcast_ref::<Self>() {
            self.cross_axis_count != old.cross_axis_count ||
            self.main_axis_spacing != old.main_axis_spacing ||
            self.cross_axis_spacing != old.cross_axis_spacing ||
            self.child_aspect_ratio != old.child_aspect_ratio
        } else {
            true
        }
    }
}
```

### Example: Max Cross Axis Extent

```rust
#[derive(Debug)]
pub struct SliverGridDelegateWithMaxCrossAxisExtent {
    max_cross_axis_extent: f32,
    main_axis_spacing: f32,
    cross_axis_spacing: f32,
    child_aspect_ratio: f32,
}

impl SliverGridDelegate for SliverGridDelegateWithMaxCrossAxisExtent {
    fn get_layout(&self, constraints: SliverConstraints) -> SliverGridLayout {
        let cross_axis_count = (constraints.cross_axis_extent / 
            (self.max_cross_axis_extent + self.cross_axis_spacing)).ceil().max(1.0) as usize;
        
        // Use SliverGridDelegateWithFixedCrossAxisCount logic
        // ... similar to above
        
        todo!("Calculate layout based on max extent")
    }
    
    fn should_relayout(&self, old: &dyn SliverGridDelegate) -> bool {
        if let Some(old) = old.as_any().downcast_ref::<Self>() {
            self.max_cross_axis_extent != old.max_cross_axis_extent ||
            self.main_axis_spacing != old.main_axis_spacing ||
            self.cross_axis_spacing != old.cross_axis_spacing ||
            self.child_aspect_ratio != old.child_aspect_ratio
        } else {
            true
        }
    }
}
```

---

## Delegate Comparison

| Delegate | Children | Purpose | Complexity |
|----------|----------|---------|------------|
| **CustomPainter** | Any | Drawing | Low |
| **CustomClipper** | Any | Clipping | Low |
| **SingleChildLayoutDelegate** | 1 | Layout | Medium |
| **MultiChildLayoutDelegate** | N (with IDs) | Layout | High |
| **FlowDelegate** | N | Layout + Paint | High |
| **SliverGridDelegate** | N (in sliver) | Grid layout | Medium |

---

## File Organization

```
flui-rendering/src/delegates/
├── mod.rs
├── custom_painter.rs               # CustomPainter trait
├── custom_clipper.rs               # CustomClipper<T> trait
├── single_child_layout_delegate.rs # SingleChildLayoutDelegate trait
├── multi_child_layout_delegate.rs  # MultiChildLayoutDelegate trait
├── flow_delegate.rs                # FlowDelegate trait
└── sliver_grid_delegate.rs         # SliverGridDelegate trait
```

---

## Usage Patterns

### Pattern 1: Stateless Delegates

```rust
#[derive(Debug)]
pub struct MyDelegate;

impl CustomPainter for MyDelegate {
    fn paint(&mut self, canvas: &mut Canvas, size: Size) {
        // Stateless painting
    }
    
    fn should_repaint(&self, _old: &dyn CustomPainter) -> bool {
        false  // Never repaint
    }
}
```

### Pattern 2: Stateful Delegates

```rust
#[derive(Debug)]
pub struct AnimatedDelegate {
    progress: f32,
}

impl CustomPainter for AnimatedDelegate {
    fn paint(&mut self, canvas: &mut Canvas, size: Size) {
        // Paint based on progress
    }
    
    fn should_repaint(&self, old: &dyn CustomPainter) -> bool {
        if let Some(old) = old.as_any().downcast_ref::<Self>() {
            self.progress != old.progress
        } else {
            true
        }
    }
}
```

### Pattern 3: Delegates with Configuration

```rust
#[derive(Debug)]
pub struct ConfigurableDelegate {
    config: Configuration,
}

impl CustomPainter for ConfigurableDelegate {
    fn paint(&mut self, canvas: &mut Canvas, size: Size) {
        // Paint based on config
    }
    
    fn should_repaint(&self, old: &dyn CustomPainter) -> bool {
        if let Some(old) = old.as_any().downcast_ref::<Self>() {
            self.config != old.config
        } else {
            true
        }
    }
}
```

---

## Summary

| Delegate | Trait Objects | Usage |
|----------|---------------|-------|
| **CustomPainter** | `Box<dyn CustomPainter>` | RenderCustomPaint |
| **CustomClipper** | `Box<dyn CustomClipper<T>>` | RenderClip* |
| **SingleChildLayoutDelegate** | `Box<dyn SingleChildLayoutDelegate>` | RenderCustomSingleChildLayoutBox |
| **MultiChildLayoutDelegate** | `Box<dyn MultiChildLayoutDelegate>` | RenderCustomMultiChildLayoutBox |
| **FlowDelegate** | `Box<dyn FlowDelegate>` | RenderFlow |
| **SliverGridDelegate** | `Box<dyn SliverGridDelegate>` | RenderSliverGrid |

---

## Next Steps

- [[Object Catalog]] - Objects that use delegates
- [[Implementation Guide]] - Creating custom delegates
- [[Delegation Pattern]] - Different pattern (ambassador)

---

**See Also:**
- [[Trait Hierarchy]] - Core trait system
- [[Protocol]] - Type system foundation
