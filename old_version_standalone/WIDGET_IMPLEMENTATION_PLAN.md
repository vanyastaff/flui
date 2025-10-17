# nebula-ui Widget Implementation Plan 🎯

**Date**: 2025-10-16
**Goal**: Build widget system for n8n-style node-based UI

## 📊 Current Status

**Infrastructure**: ✅ 100% Complete
- 7 Production Controllers (AnimationController, FocusController, etc.)
- 50+ Types (Geometry, Layout, Styling, Typography, Interaction)
- 445 Tests Passing

**Widgets**: ❌ 0% Complete
- No widget implementations
- No rendering layer
- **This is the blocker for any UI**

---

## 🎯 Implementation Priority

### **TIER 1: FOUNDATION** (Week 1) - HIGHEST PRIORITY

#### 1. Widget Trait ⭐⭐⭐
**File**: `src/widgets/widget.rs`
**LOC**: 50-100
**Status**: Must implement first

```rust
pub trait Widget {
    fn ui(&mut self, ui: &mut egui::Ui) -> egui::Response;
    fn id(&self) -> Option<egui::Id> { None }
}
```

#### 2. Container ⭐⭐⭐
**File**: `src/widgets/container.rs`
**LOC**: 200-300
**Dependencies**: BoxDecoration (✅), Padding (✅), Size (✅)

**Features**:
- Background color/gradient
- Border & border radius
- Box shadows
- Padding
- Size constraints
- Child widget

**Example**:
```rust
Container::new()
    .with_decoration(BoxDecoration::new()
        .with_color(Color::WHITE)
        .with_border_radius(BorderRadius::circular(12.0))
        .with_shadow(BoxShadow::elevation(2.0)))
    .with_padding(Padding::all(16.0))
    .child(|ui| {
        ui.label("Hello");
    })
    .ui(ui);
```

#### 3. Text ⭐⭐
**File**: `src/widgets/text.rs`
**LOC**: 100-150
**Dependencies**: TextStyle (✅)

**Features**:
- Rich text styling
- Text alignment
- Max lines / overflow
- Selection support

---

### **TIER 2: INTERACTION** (Week 1-2) - CRITICAL FOR n8n

#### 4. GestureDetector ⭐⭐⭐ CRITICAL
**File**: `src/widgets/gesture_detector.rs`
**LOC**: 400-600
**Dependencies**: None (uses egui::Response)

**Features**:
- `on_tap` - Click handler
- `on_drag` - Drag events (delta, position)
- `on_drag_start` / `on_drag_end`
- `on_hover` - Hover enter/exit
- `on_long_press` - Long press detection

**Critical for**: Dragging nodes, detecting clicks, hover effects

**Example**:
```rust
GestureDetector::new()
    .on_tap(|| println!("Tapped!"))
    .on_drag(|delta| {
        node_position += delta;
    })
    .child(|ui| {
        // Node content
    })
    .ui(ui);
```

#### 5. Draggable ⭐⭐⭐ CRITICAL FOR n8n
**File**: `src/widgets/draggable.rs`
**LOC**: 300-400
**Dependencies**: GestureDetector

**Features**:
- Drag source
- Data payload
- Drag feedback (visual)
- Drag start/end callbacks

**Critical for**: Moving nodes around canvas

---

### **TIER 3: GRAPHICS** (Week 2) - **MOST CRITICAL FOR n8n**

#### 6. CustomPaint ⭐⭐⭐⭐⭐ **HIGHEST PRIORITY FOR n8n**
**File**: `src/widgets/custom_paint.rs`
**LOC**: 500-800
**Dependencies**: Path (✅), CubicBezier (✅), Color (✅), StrokeStyle (✅)

**WHY CRITICAL**: Cannot draw connection lines between nodes without this!

**Features**:
- Canvas drawing interface
- Path rendering
- Bezier curve drawing
- Stroke & fill
- Clipping
- Transformations

**Example for n8n connections**:
```rust
CustomPaint::new()
    .painter(|ui, painter, rect| {
        // Draw connection line
        let path = CubicBezier::new(
            start_point,
            control1,
            control2,
            end_point,
        );

        painter.draw_path(
            path,
            StrokeStyle::rounded(2.5),
            Color::BLUE,
        );
    })
    .ui(ui);
```

**API Design**:
```rust
pub struct Painter<'a> {
    ui: &'a mut egui::Ui,
    shapes: &'a mut Vec<egui::Shape>,
}

impl<'a> Painter<'a> {
    pub fn draw_line(&mut self, from: Point, to: Point, stroke: StrokeStyle, color: Color);
    pub fn draw_path(&mut self, path: &Path, stroke: StrokeStyle, color: Color);
    pub fn draw_bezier(&mut self, bezier: &CubicBezier, stroke: StrokeStyle, color: Color);
    pub fn fill_rect(&mut self, rect: Rect, color: Color);
    pub fn fill_circle(&mut self, center: Point, radius: f32, color: Color);
}
```

#### 7. Stack ⭐⭐⭐
**File**: `src/widgets/stack.rs`
**LOC**: 200-300
**Dependencies**: None

**Features**:
- Overlay multiple children
- Absolute positioning
- Z-order control

**Critical for**: Layering nodes and connections on canvas

---

### **TIER 4: n8n SPECIFIC** (Week 3-4)

#### 8. Node Widget System ⭐⭐⭐⭐⭐
**Files**:
- `src/widgets/node/node_widget.rs` (300-400 LOC)
- `src/widgets/node/node_port.rs` (150-200 LOC)
- `src/widgets/node/node_header.rs` (100-150 LOC)

**Build on**: Container, Text, GestureDetector

**Features**:
- Draggable nodes
- Input/output ports
- Port hit detection
- Node selection state
- Node resize handles

**Example**:
```rust
NodeWidget::new("HTTP Request")
    .with_category(NodeCategory::Action)
    .with_inputs(vec![
        NodePort::new("input", PortType::Main),
    ])
    .with_outputs(vec![
        NodePort::new("output", PortType::Main),
    ])
    .with_position(Point::new(100.0, 200.0))
    .with_size(Size::new(200.0, 150.0))
    .ui(ui);
```

#### 9. Connection Renderer ⭐⭐⭐⭐⭐
**File**: `src/widgets/connection/connection_line.rs`
**LOC**: 400-600
**Dependencies**: CustomPaint, CubicBezier

**Features**:
- Bezier connection curves
- Connection hover/selection
- Animated flow indicators
- Connection labels

**Example**:
```rust
ConnectionLine::new(from_port, to_port)
    .with_color(Color::BLUE)
    .with_stroke_width(2.5)
    .with_flow_animation(true)
    .ui(ui);
```

#### 10. Workspace Canvas ⭐⭐⭐⭐⭐
**File**: `src/widgets/workspace/canvas.rs`
**LOC**: 800-1200
**Dependencies**: Stack, CustomPaint, Draggable, GestureDetector

**Features**:
- Zoom & Pan
- Grid background
- Node management
- Connection management
- Selection box
- Minimap

---

## 🚀 Week-by-Week Implementation Plan

### **Week 1: Foundation**
- [ ] Day 1-2: Widget trait + Container
- [ ] Day 3: Text widget
- [ ] Day 4-5: GestureDetector (complex but essential)

**Deliverable**: Can render basic containers with text

### **Week 2: Critical Graphics**
- [ ] Day 1-3: **CustomPaint** (MOST IMPORTANT)
- [ ] Day 4: Stack widget
- [ ] Day 5: Draggable widget

**Deliverable**: Can draw lines and move things

### **Week 3: n8n Foundation**
- [ ] Day 1-2: NodeWidget base
- [ ] Day 3: NodePort system
- [ ] Day 4-5: Connection rendering

**Deliverable**: Basic node-based UI

### **Week 4: Polish & Features**
- [ ] Day 1-2: Workspace canvas
- [ ] Day 3-4: Zoom & Pan
- [ ] Day 5: Selection & interactions

**Deliverable**: Working n8n-style editor

---

## 📝 Widget Implementation Template

Every widget should follow this pattern:

```rust
//! Brief description
//!
//! Example:
//! ```
//! # use nebula_ui::widgets::Widget;
//! Widget::new()
//!     .with_property(value)
//!     .ui(ui);
//! ```

use crate::types::*;
use egui;

/// Widget documentation
pub struct Widget {
    // Properties
    property: Type,
}

impl Widget {
    /// Create new widget
    pub fn new() -> Self {
        Self {
            property: Default::default(),
        }
    }

    /// Builder: set property
    pub fn with_property(mut self, value: impl Into<Type>) -> Self {
        self.property = value.into();
        self
    }

    /// Render widget
    pub fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        // Implementation
        ui.label("Widget")
    }
}

impl Default for Widget {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_widget_creation() {
        let widget = Widget::new();
        assert_eq!(widget.property, Type::default());
    }
}
```

---

## 🎯 Success Criteria

### Week 1 Success:
- ✅ Container renders with decoration
- ✅ Text renders with style
- ✅ GestureDetector detects clicks and drags

### Week 2 Success:
- ✅ CustomPaint draws lines and curves
- ✅ Can draw bezier curve between two points
- ✅ Stack positions widgets absolutely

### Week 3 Success:
- ✅ Node renders with ports
- ✅ Connections render as curves
- ✅ Can drag nodes around

### Week 4 Success:
- ✅ Full canvas with zoom/pan
- ✅ Can create and connect nodes
- ✅ Working n8n demo

---

## 🔥 IMMEDIATE ACTION ITEMS

1. **TODAY**: Implement Widget trait (30 min)
2. **TODAY**: Start Container widget (2-3 hours)
3. **TOMORROW**: Finish Container, start Text (3-4 hours)
4. **THIS WEEK**: Complete GestureDetector (6-8 hours)
5. **NEXT WEEK**: CustomPaint (CRITICAL - 10-12 hours)

---

## 📊 Metrics to Track

- [ ] Widgets implemented: 0/10
- [ ] Tests passing: 445 → ?
- [ ] Example demos: 0 → 5
- [ ] Documentation pages: 0 → 10
- [ ] LOC in widgets/: 0 → ~5,000

---

## 💡 Key Design Decisions

1. **Use egui's immediate mode** - Don't fight the framework
2. **Builder pattern everywhere** - Consistent API
3. **`impl Into<T>` for flexibility** - Just like types
4. **Controllers separate from widgets** - Composition over inheritance
5. **Examples for every widget** - Documentation-driven development

---

**Ready to build! Let's start with the Widget trait and Container! 🚀**
