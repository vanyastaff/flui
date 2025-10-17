# Types Module - Flutter-Inspired UI System for egui

**Complete type system for building n8n-like interfaces with Flutter-style API on egui**

## Vision

This module provides a comprehensive, type-safe foundation for building complex node-based workflow UIs (like n8n) using egui with a Flutter-inspired API. Every type is designed with ergonomics, type safety, and zero-cost abstractions in mind.

## Module Structure

```
types/
├── core/           # Fundamental geometric & color types
├── layout/         # Layout and positioning
├── styling/        # Visual styling (colors, gradients, borders, shadows)
├── typography/     # Text styling and fonts
├── interaction/    # User interaction & animations
└── utility/        # Platform detection & helpers
```

## Core Philosophy

### 1. **Flutter-Inspired API**
```rust
// Flutter-style declarative UI building
let node_card = BoxDecoration::new()
    .with_color(Color::WHITE)
    .with_border_radius(BorderRadius::circular(12.0))
    .with_shadow(BoxShadow::elevation(4.0, Color::from_rgba(0, 0, 0, 50)))
    .with_border(Border::uniform(Color::GRAY, 1.0));
```

### 2. **Type Safety with Ergonomics**
```rust
// Strong typing prevents errors
let position = Point::new(100.0, 200.0);   // Not (f32, f32)
let offset = Offset::new(10.0, 20.0);      // Different semantic meaning
let size = Size::new(300.0, 400.0);        // Clear intent

// But flexible with conversions
let point: Point = (100.0, 200.0).into();
let color: Color = (255, 100, 50).into();
```

### 3. **Zero-Cost Abstractions**
```rust
// impl Into<T> compiles to same code as concrete types
fn style_node(color: impl Into<Color>) -> BoxDecoration {
    BoxDecoration::new().with_color(color)  // No runtime cost
}

// All geometric types are Copy
let p1 = Point::new(0.0, 0.0);
let p2 = p1;  // Copy, not move
```

## Building Blocks for n8n UI

### Node Representation

```rust
use nebula_ui::types::{
    core::{Point, Size, Rect, Color},
    styling::{BoxDecoration, BorderRadius, BoxShadow, Border},
};

pub struct WorkflowNode {
    position: Point,
    size: Size,
    decoration: BoxDecoration,
    // ... other properties
}

impl WorkflowNode {
    pub fn new(x: f32, y: f32) -> Self {
        Self {
            position: Point::new(x, y),
            size: Size::new(200.0, 100.0),
            decoration: BoxDecoration::new()
                .with_color(Color::WHITE)
                .with_border_radius(BorderRadius::circular(8.0))
                .with_shadow(BoxShadow::elevation(2.0, Color::from_rgba(0, 0, 0, 30)))
                .with_border(Border::uniform(Color::from_rgb(200, 200, 200), 1.0)),
        }
    }

    pub fn bounds(&self) -> Rect {
        Rect::from_min_size(self.position, self.size)
    }

    pub fn contains(&self, point: impl Into<Point>) -> bool {
        self.bounds().contains(point)
    }

    pub fn selected_style(&self) -> BoxDecoration {
        self.decoration.clone()
            .with_border(Border::uniform(Color::BLUE, 2.0))
            .with_shadow(BoxShadow::elevation(4.0, Color::from_rgba(66, 133, 244, 100)))
    }
}
```

### Connection Lines (Bezier Curves)

```rust
use nebula_ui::types::core::{Point, Path, path::CubicBezier};

pub struct Connection {
    pub from: Point,
    pub to: Point,
}

impl Connection {
    pub fn path(&self) -> Path {
        let control_offset = (self.to.x - self.from.x).abs() * 0.5;

        let bezier = CubicBezier {
            start: self.from,
            control1: Point::new(self.from.x + control_offset, self.from.y),
            control2: Point::new(self.to.x - control_offset, self.to.y),
            end: self.to,
        };

        // Build smooth curve
        let mut path = Path::new().move_to(self.from);
        for i in 1..=20 {
            let t = i as f32 / 20.0;
            path = path.line_to(bezier.at(t));
        }
        path
    }
}
```

### Canvas/Viewport

```rust
use nebula_ui::types::core::{Point, Offset, Rect, Scale, Transform};

pub struct Canvas {
    pub viewport: Rect,
    pub zoom: Scale,
    pub pan_offset: Offset,
}

impl Canvas {
    pub fn screen_to_canvas(&self, screen_point: Point) -> Point {
        let transformed = screen_point - self.pan_offset;
        Point::new(
            transformed.x / self.zoom.x,
            transformed.y / self.zoom.y,
        )
    }

    pub fn canvas_to_screen(&self, canvas_point: Point) -> Point {
        Point::new(
            canvas_point.x * self.zoom.x,
            canvas_point.y * self.zoom.y,
        ) + self.pan_offset
    }

    pub fn transform(&self) -> Transform {
        Transform::identity()
            .translate(self.pan_offset)
            .scale(self.zoom)
    }
}
```

### Node Categories/Themes

```rust
use nebula_ui::types::{
    core::Color,
    styling::BoxDecoration,
    typography::TextStyle,
};

pub enum NodeCategory {
    Trigger,
    Action,
    Transform,
    Control,
}

impl NodeCategory {
    pub fn color(&self) -> Color {
        match self {
            NodeCategory::Trigger => Color::from_rgb(103, 58, 183),   // Purple
            NodeCategory::Action => Color::from_rgb(33, 150, 243),    // Blue
            NodeCategory::Transform => Color::from_rgb(255, 152, 0),  // Orange
            NodeCategory::Control => Color::from_rgb(76, 175, 80),    // Green
        }
    }

    pub fn decoration(&self) -> BoxDecoration {
        BoxDecoration::new()
            .with_color(Color::WHITE)
            .with_border(Border::uniform(self.color(), 2.0))
            .with_border_radius(BorderRadius::circular(8.0))
    }

    pub fn header_style(&self) -> TextStyle {
        TextStyle::new()
            .with_color(Color::WHITE)
            .with_weight(FontWeight::Bold)
    }
}
```

### Animation States

```rust
use nebula_ui::types::{
    core::{Duration, Opacity},
    interaction::curves::Curve,
};

pub struct NodeAnimationState {
    pub hover: bool,
    pub selected: bool,
    pub dragging: bool,
    pub opacity: Opacity,
}

impl NodeAnimationState {
    pub fn target_opacity(&self) -> Opacity {
        if self.dragging {
            Opacity::new(0.8)
        } else if !self.selected {
            Opacity::OPAQUE
        } else {
            Opacity::OPAQUE
        }
    }

    pub fn animate_to(&mut self, target: Opacity, dt: Duration) {
        // Smooth animation using curves
        let t = (dt.as_seconds() * 5.0).min(1.0);
        self.opacity = self.opacity.lerp(target, Curve::ease_in_out().transform(t));
    }
}
```

## Key Features for n8n-like UI

### ✅ Geometric Types (core/)

Perfect for node-based UIs:
- **Point** - Node positions, connection endpoints
- **Offset** - Dragging, panning
- **Size** - Node dimensions
- **Rect** - Node bounds, selection areas
- **Circle** - Port hit detection
- **Path** - Connection curves
- **Bounds** - Canvas boundaries
- **Transform** - Zoom, pan, rotate

### ✅ Styling (styling/)

Rich visual appearance:
- **Color** - Node colors, themes, states
- **Gradient** - Beautiful backgrounds
- **Border** - Node outlines, selection
- **BorderRadius** - Rounded corners
- **Shadow** - Depth, elevation, hover states
- **BoxDecoration** - Complete node styling

### ✅ Layout (layout/)

Flexible positioning:
- **EdgeInsets** - Node padding, margins
- **Alignment** - Port alignment, labels
- **BoxConstraints** - Responsive sizing
- **AspectRatio** - Maintain proportions
- **FlexLayout** - Node content layout

### ✅ Typography (typography/)

Professional text rendering:
- **FontFamily** - Custom fonts, monospace for code
- **FontSize** - Scalable text
- **FontWeight** - Visual hierarchy
- **TextStyle** - Complete text styling
- **TextDecoration** - Links, emphasis

### ✅ Interaction (interaction/)

Smooth user experience:
- **Curves** - Easing functions for animations
- **WidgetState** - Hover, active, disabled states
- **ValidationState** - Input validation
- **InputType** - Various input modes

### ✅ Utility (utility/)

Cross-platform support:
- **TargetPlatform** - Platform detection
- **Brightness** - Light/dark themes

## Complete n8n Node Example

```rust
use nebula_ui::types::{
    core::*,
    styling::*,
    typography::*,
    layout::*,
    interaction::*,
};

pub struct N8nNode {
    // Position & Size
    pub id: String,
    pub position: Point,
    pub size: Size,

    // Visual
    pub category: NodeCategory,
    pub icon: String,
    pub title: String,

    // State
    pub selected: bool,
    pub hovered: bool,
    pub disabled: bool,

    // Ports
    pub inputs: Vec<Port>,
    pub outputs: Vec<Port>,
}

impl N8nNode {
    pub fn decoration(&self) -> BoxDecoration {
        let base_color = if self.disabled {
            Color::GRAY
        } else {
            self.category.color()
        };

        let mut decoration = BoxDecoration::new()
            .with_color(Color::WHITE)
            .with_border_radius(BorderRadius::circular(12.0))
            .with_border(Border::uniform(base_color, if self.selected { 3.0 } else { 2.0 }));

        // Add shadow based on state
        if self.hovered && !self.disabled {
            decoration = decoration.with_shadow(
                BoxShadow::elevation(6.0, base_color.with_opacity(0.3))
            );
        } else {
            decoration = decoration.with_shadow(
                BoxShadow::elevation(2.0, Color::from_rgba(0, 0, 0, 20))
            );
        }

        decoration
    }

    pub fn header_decoration(&self) -> BoxDecoration {
        BoxDecoration::new()
            .with_gradient(Gradient::Linear(
                LinearGradient::vertical(
                    self.category.color(),
                    self.category.color().with_opacity(0.8),
                )
            ))
            .with_border_radius(BorderRadius::vertical_top(Radius::circular(12.0)))
    }

    pub fn title_style(&self) -> TextStyle {
        TextStyle::new()
            .with_color(Color::WHITE)
            .with_size(FontSize::MEDIUM)
            .with_weight(FontWeight::SemiBold)
    }

    pub fn bounds(&self) -> Rect {
        Rect::from_min_size(self.position, self.size)
    }

    pub fn contains_point(&self, point: impl Into<Point>) -> bool {
        self.bounds().contains(point)
    }

    pub fn get_input_port_position(&self, index: usize) -> Option<Point> {
        if index >= self.inputs.len() {
            return None;
        }

        let port_spacing = self.size.height / (self.inputs.len() + 1) as f32;
        let y = self.position.y + port_spacing * (index + 1) as f32;

        Some(Point::new(self.position.x, y))
    }

    pub fn get_output_port_position(&self, index: usize) -> Option<Point> {
        if index >= self.outputs.len() {
            return None;
        }

        let port_spacing = self.size.height / (self.outputs.len() + 1) as f32;
        let y = self.position.y + port_spacing * (index + 1) as f32;

        Some(Point::new(self.position.x + self.size.width, y))
    }
}

pub struct Port {
    pub name: String,
    pub port_type: PortType,
    pub radius: f32,
}

impl Port {
    pub fn decoration(&self, hovered: bool) -> BoxDecoration {
        BoxDecoration::new()
            .with_color(if hovered { Color::BLUE } else { Color::WHITE })
            .with_border(Border::uniform(Color::from_rgb(100, 100, 100), 2.0))
            .with_border_radius(BorderRadius::circular(999.0))
    }

    pub fn hit_test(&self, position: Point, point: impl Into<Point>) -> bool {
        Circle::new(position, self.radius).contains(point)
    }
}

pub enum PortType {
    Data,
    Control,
    Event,
}
```

## Next Steps for n8n UI

### 1. Widget Components
Create reusable widgets using these types:
- `NodeWidget` - Renders individual nodes
- `ConnectionWidget` - Renders bezier connections
- `CanvasWidget` - Main workflow canvas
- `MiniMapWidget` - Navigation minimap
- `NodePaletteWidget` - Drag & drop node palette

### 2. Controllers
Use the existing controllers:
- `AnimationController` - Smooth transitions
- `FocusController` - Keyboard navigation
- `InputController` - Text input in nodes
- `ValidationController` - Node configuration validation
- `VisibilityController` - Show/hide node details

### 3. State Management
```rust
pub struct WorkflowState {
    pub nodes: Vec<N8nNode>,
    pub connections: Vec<Connection>,
    pub selected_nodes: Vec<String>,
    pub canvas: Canvas,
}
```

### 4. Interaction Patterns
- Drag & drop nodes
- Draw connections
- Multi-select
- Zoom & pan
- Undo/redo
- Copy/paste

## Resources

- **[Core Types](core/README.md)** - Geometric primitives
- **[Styling Types](styling/README.md)** - Visual appearance
- **[Layout Types](layout/README.md)** - Positioning & sizing
- **[Typography](typography/README.md)** - Text styling
- **[Interaction](interaction/)** - Animations & states

## Testing

All types have comprehensive tests:

```bash
# Test all types
cargo test --lib --package nebula-ui types

# Test specific modules
cargo test --lib --package nebula-ui types::core
cargo test --lib --package nebula-ui types::styling
cargo test --lib --package nebula-ui types::layout
cargo test --lib --package nebula-ui types::typography
```

## Performance

- All geometric types are `Copy` (zero-cost)
- `impl Into<T>` compiles to same code as concrete types
- No heap allocations for basic operations
- Suitable for 60fps+ rendering
- Scales to thousands of nodes

## Why This Approach?

1. **Type Safety** - Catch errors at compile time
2. **Ergonomics** - Flutter-like API feels natural
3. **Performance** - Zero-cost abstractions
4. **Maintainability** - Clear, well-organized code
5. **Extensibility** - Easy to add new features
6. **Documentation** - Self-documenting types

## Example: Complete Mini n8n

See `examples/n8n_workflow.rs` (coming soon) for a complete working example of a node-based workflow editor built with these types.

---

**Built for creating beautiful, performant node-based UIs with egui** 🚀
