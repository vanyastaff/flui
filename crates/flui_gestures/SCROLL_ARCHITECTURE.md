# Scroll Events Architecture

## Two Event Models in FLUI

### Pointer Events (Tap, Click, Drag) - HIT TESTING

**Use case:** Need to know EXACTLY which widget was clicked/touched

**Flow:**
```
EventRouter.route_pointer_event(event)
    ↓
root.hit_test(position, &mut result)
    ↓
Layers add themselves to HitTestResult with handlers
    ↓
result.dispatch(event) → calls all handlers (front to back)
```

**Widgets using this:**
- GestureDetector (tap, drag, long-press)
- Button
- Interactive widgets (checkboxes, sliders, etc.)

---

### Scroll Events - EVENT BUBBLING

**Use case:** Multiple layers might handle scroll (nested scroll containers)

**Flow:**
```
EventRouter.route_scroll_event(event)
    ↓
root.handle_event(event)
    ↓
Each layer decides:
  - Handle it (return true)
  - Pass to children (call child.handle_event())
  - Ignore it (return false)
```

**Widgets using this:**
- ScrollView (ListView, GridView, SingleChildScrollView)
- Nested scrollable containers
- Custom scrollable widgets

---

## Scroll Event Handling

### Layer::handle_event() Implementation

**For scrollable layers:**

```rust
impl Layer for ScrollViewLayer {
    fn handle_event(&mut self, event: &Event) -> bool {
        match event {
            Event::Scroll(scroll_data) => {
                // Check if we should handle this scroll
                let direction = self.scroll_direction; // Vertical or Horizontal

                match scroll_data.delta {
                    ScrollDelta::Lines { x, y } | ScrollDelta::Pixels { x, y } => {
                        let should_handle = match direction {
                            Axis::Vertical => y.abs() > x.abs(),
                            Axis::Horizontal => x.abs() > y.abs(),
                        };

                        if should_handle {
                            // Update scroll position
                            self.scroll_offset += delta;
                            self.clamp_scroll();
                            return true; // Event handled
                        }
                    }
                }

                // Not our scroll direction, pass to child
                if let Some(child) = &mut self.child {
                    return child.handle_event(event);
                }

                false // Not handled
            }
            _ => {
                // Other events pass through
                if let Some(child) = &mut self.child {
                    child.handle_event(event)
                } else {
                    false
                }
            }
        }
    }
}
```

### Nested Scrolling Example

```rust
// Outer: Vertical scroll
let outer = ScrollView::vertical()
    .child(
        Column::new()
            .children(vec![
                // Inner: Horizontal scroll
                ScrollView::horizontal()
                    .child(Row::new().children(wide_items)),

                // More vertical content
                Text::new("More content..."),
            ])
    );
```

**Scroll event processing:**

1. User scrolls vertically (y > x):
   ```
   Event::Scroll(y=10, x=0)
       ↓
   OuterScrollView.handle_event():
     - y.abs() > x.abs() → true
     - Handles vertical scroll
     - return true ✅
   ```

2. User scrolls horizontally (x > y):
   ```
   Event::Scroll(y=0, x=10)
       ↓
   OuterScrollView.handle_event():
     - y.abs() > x.abs() → false
     - Passes to child ↓
       ↓
   InnerScrollView.handle_event():
     - x.abs() > y.abs() → true
     - Handles horizontal scroll
     - return true ✅
   ```

---

## Why Two Different Models?

### Pointer Events → Hit Testing
- **Spatial query**: "What's at position (x, y)?"
- **Multiple targets**: Nested widgets both get event (gesture arena resolves)
- **Precise targeting**: Need exact widget bounds

**Example:** Click at (100, 50)
```
Container (0,0 - 200,200)  ← HIT
  └─ Button (80,40 - 120,60)  ← HIT (more specific)
      └─ Text (90,45 - 110,55)  ← HIT (most specific)
```
All three added to HitTestResult, Button wins gesture arena.

### Scroll Events → Bubbling
- **Logical query**: "Who wants to handle this scroll?"
- **Single handler**: First layer that handles it stops propagation
- **Direction-aware**: Vertical vs horizontal scroll

**Example:** Vertical scroll
```
VerticalScrollView  ← HANDLES (return true)
  └─ HorizontalScrollView  ← Never reached
      └─ Image  ← Never reached
```

---

## Implementation in flui_gestures

### ScrollGestureRecognizer (Future)

For custom scroll handling via GestureDetector:

```rust
pub struct ScrollGestureRecognizer {
    on_scroll_start: Option<Arc<dyn Fn(&ScrollEventData) + Send + Sync>>,
    on_scroll_update: Option<Arc<dyn Fn(&ScrollEventData) + Send + Sync>>,
    on_scroll_end: Option<Arc<dyn Fn() + Send + Sync>>,
}
```

**Usage:**
```rust
GestureDetector::builder()
    .on_scroll_update(|data| {
        println!("Scrolled: {:?}", data.delta);
    })
    .child(child)
    .build()
```

**Implementation:**
```rust
impl Layer for GestureDetectorLayer {
    fn handle_event(&mut self, event: &Event) -> bool {
        match event {
            Event::Scroll(data) => {
                if let Some(cb) = &self.recognizer.on_scroll_update {
                    cb(data);
                    return true; // Handled
                }
                // Not handled, pass to child
                self.child.handle_event(event)
            }
            _ => self.child.handle_event(event)
        }
    }
}
```

---

## Scroll vs Drag

### Scroll Event
- **Source:** Mouse wheel, trackpad two-finger
- **Data:** ScrollDelta (Lines or Pixels)
- **No position tracking:** Just delta
- **Bubbles through layers**

### Drag Gesture
- **Source:** Pointer down → move → up
- **Data:** PointerEvent with position
- **Tracks pointer:** Start, update, end
- **Hit testing:** Which widget was touched

**Example: Scrollable vs Draggable**

```rust
// Scrollable list (mouse wheel)
ScrollView::vertical()
    .on_scroll(|delta| { ... })
    .child(list)

// Draggable list (touch/mouse drag)
GestureDetector::builder()
    .on_pan_start(|details| { ... })
    .on_pan_update(|details| { ... })
    .on_pan_end(|details| { ... })
    .child(list)
```

Many scrollable widgets support BOTH:
- Scroll events (mouse wheel)
- Pan gestures (touch drag)

---

## Current Status

### What Works ✅
- EventRouter.route_scroll_event() implemented
- Event bubbling through Layer::handle_event()
- ScrollEventData with Lines/Pixels delta

### What's Needed ⏳
- ScrollView widget with Layer::handle_event() implementation
- ScrollController for programmatic scrolling
- Nested scroll coordination
- Scroll physics (bounce, friction)
- ScrollGestureRecognizer in flui_gestures

---

## Example: Complete ScrollView

```rust
pub struct ScrollView {
    direction: Axis,
    child: Box<dyn AnyView>,
    controller: Option<ScrollController>,
}

impl View for ScrollView {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        RenderScrollView::new(self.direction, self.controller)
    }
}

pub struct RenderScrollView {
    direction: Axis,
    scroll_offset: f32,
    max_scroll: f32,
}

impl SingleRender for RenderScrollView {
    fn layout(&mut self, tree: &ElementTree, child_id: ElementId, constraints: BoxConstraints) -> Size {
        // Layout child with infinite constraint in scroll direction
        let child_constraints = match self.direction {
            Axis::Vertical => BoxConstraints::new(
                constraints.min_width, constraints.max_width,
                0.0, f32::INFINITY
            ),
            Axis::Horizontal => BoxConstraints::new(
                0.0, f32::INFINITY,
                constraints.min_height, constraints.max_height
            ),
        };

        let child_size = tree.layout_child(child_id, child_constraints);

        // Our size is constrained, child can be larger
        let size = constraints.constrain(child_size);

        // Calculate max scroll
        self.max_scroll = match self.direction {
            Axis::Vertical => (child_size.height - size.height).max(0.0),
            Axis::Horizontal => (child_size.width - size.width).max(0.0),
        };

        size
    }

    fn paint(&self, tree: &ElementTree, child_id: ElementId, offset: Offset) -> BoxedLayer {
        // Paint child with scroll offset
        let child_offset = match self.direction {
            Axis::Vertical => offset - Offset::new(0.0, self.scroll_offset),
            Axis::Horizontal => offset - Offset::new(self.scroll_offset, 0.0),
        };

        let child_layer = tree.paint_child(child_id, child_offset);

        // Wrap in clip layer
        let clip_rect = Rect::from_origin_size(offset, self.size);
        Box::new(ClipRectLayer::new(child_layer, clip_rect))
    }
}

// This would be in the Layer implementation for ScrollView's layer
impl Layer for ScrollViewLayer {
    fn handle_event(&mut self, event: &Event) -> bool {
        match event {
            Event::Scroll(data) => {
                let delta = match data.delta {
                    ScrollDelta::Lines { x, y } => {
                        match self.direction {
                            Axis::Vertical => y * 20.0, // 20px per line
                            Axis::Horizontal => x * 20.0,
                        }
                    }
                    ScrollDelta::Pixels { x, y } => {
                        match self.direction {
                            Axis::Vertical => y,
                            Axis::Horizontal => x,
                        }
                    }
                };

                // Update scroll offset
                self.scroll_offset = (self.scroll_offset - delta).clamp(0.0, self.max_scroll);

                // Mark needs repaint
                self.mark_needs_paint();

                true // Event handled
            }
            _ => {
                // Pass other events to child
                self.child.handle_event(event)
            }
        }
    }
}
```

---

## Summary

| Aspect | Pointer Events | Scroll Events |
|--------|---------------|---------------|
| **Routing** | Hit testing → HitTestResult | Event bubbling → handle_event() |
| **Multiple handlers** | Yes (gesture arena resolves) | No (first handler wins) |
| **Position** | Exact (x, y) coordinates | No position, just delta |
| **Use case** | Tap, click, drag, gestures | Mouse wheel, trackpad scroll |
| **Widgets** | GestureDetector, Button | ScrollView, ListView |
| **Implementation** | Layer.hit_test() + handlers | Layer.handle_event() |

Both systems work together through EventRouter, providing complete input handling!
