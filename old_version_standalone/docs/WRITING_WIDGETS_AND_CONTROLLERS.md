# Guide: Writing Widgets and Controllers in nebula-ui

This comprehensive guide explains how to properly write Widgets and Controllers in nebula-ui, following established patterns and best practices.

## Table of Contents

1. [Widget Architecture](#widget-architecture)
2. [Controller Architecture](#controller-architecture)
3. [Writing a Widget](#writing-a-widget)
4. [Writing a Controller](#writing-a-controller)
5. [Best Practices](#best-practices)
6. [Common Patterns](#common-patterns)
7. [Testing](#testing)

---

## Widget Architecture

### Core Concepts

**Widgets are immutable, declarative UI elements that:**
- Are created and consumed each frame (move semantics: `self`)
- Represent the **what** of the UI (declarative)
- Are stateless or manage only presentation state
- Use bon builders for ergonomic API

**Key traits:**
- `egui::Widget` - Core rendering trait from egui
- `WidgetExt` - nebula-ui extension trait adding validation, debugging, size hints

### Widget Lifecycle

```
Create → Configure → Validate → Render → Consumed
  ↓          ↓           ↓         ↓         ↓
 new()   builder()   validate()  ui()    destroyed
```

---

## Controller Architecture

### Core Concepts

**Controllers are mutable, imperative state managers that:**
- Live across multiple frames (borrow semantics: `&mut self`)
- Represent the **how** of state changes (imperative)
- Manage animation, interaction, and complex state
- Update via `update(&mut self, ctx)` each frame

**Key traits:**
- `Controller` - nebula-ui trait for stateful controllers

### Controller Lifecycle

```
Create → Initialize → Update → Update → ... → Reset
  ↓          ↓          ↓         ↓              ↓
 new()   setup()   update()  update()       reset()
                      ↑          ↑
                      └──────────┘
                    Lives across frames
```

---

## Writing a Widget

Widgets can be created in two ways:
1. **Struct Widgets** - Full-featured with bon builders (for complex widgets)
2. **Closure Widgets** - Lightweight functions (for simple, ad-hoc widgets)

### Struct Widgets

For complex, reusable widgets with configuration options.

#### Step 1: Define the Struct

**Required fields:**
- `key: Option<egui::Id>` - For state persistence (always first field)
- Configuration fields specific to your widget
- `child: Option<Box<dyn FnOnce(&mut egui::Ui) -> Response>>` - If widget can contain children

**Use bon Builder:**
```rust
use bon::Builder;

#[derive(Builder)]
#[builder(
    on(EdgeInsets, into),      // Auto-convert into EdgeInsets
    on(Color, into),            // Auto-convert into Color
    finish_fn(vis = "", name = build_internal)  // Hide standard build()
)]
pub struct MyWidget {
    /// Widget key for state persistence
    #[builder(into)]
    pub key: Option<egui::Id>,

    /// Width of the widget
    pub width: Option<f32>,

    /// Height of the widget
    pub height: Option<f32>,

    /// Background color
    #[builder(into)]
    pub color: Option<Color>,

    /// Padding around content
    #[builder(default = EdgeInsets::ZERO, into)]
    pub padding: EdgeInsets,

    /// Optional child (if widget is a container)
    #[builder(setters(vis = "", name = child_internal))]
    pub child: Option<Box<dyn FnOnce(&mut egui::Ui) -> egui::Response>>,
}
```

### Step 2: Implement Core Methods

```rust
impl MyWidget {
    /// Create a new widget with default values
    pub fn new() -> Self {
        Self {
            key: None,
            width: None,
            height: None,
            color: None,
            padding: EdgeInsets::ZERO,
            child: None,
        }
    }

    /// Validate widget configuration
    pub fn validate(&self) -> Result<(), String> {
        // Check for invalid values
        if let Some(width) = self.width {
            if width < 0.0 || width.is_nan() || width.is_infinite() {
                return Err(format!("Invalid width: {}", width));
            }
        }

        if let Some(height) = self.height {
            if height < 0.0 || height.is_nan() || height.is_infinite() {
                return Err(format!("Invalid height: {}", height));
            }
        }

        // Check for logical conflicts
        if let (Some(w), Some(h)) = (self.width, self.height) {
            if w > 1000.0 && h > 1000.0 {
                // Warning: very large widget
            }
        }

        Ok(())
    }
}

impl Default for MyWidget {
    fn default() -> Self {
        Self::new()
    }
}
```

### Step 3: Add bon Builder Smart Setters

```rust
use my_widget_builder::{State, SetChild, IsComplete, IsUnset};

// Smart setter for .child() in builder chain
impl<S: State> MyWidgetBuilder<S> {
    /// Add a child widget using a closure
    pub fn child<F>(self, child: F) -> MyWidgetBuilder<SetChild<S>>
    where
        S::Child: IsUnset,
        F: FnOnce(&mut egui::Ui) -> egui::Response + 'static,
    {
        let boxed: Box<dyn FnOnce(&mut egui::Ui) -> egui::Response> = Box::new(child);
        self.child_internal(boxed)
    }
}

// Custom finishing functions
impl<S: IsComplete> MyWidgetBuilder<S> {
    /// Build and render in one step
    pub fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let widget = self.build_internal();
        egui::Widget::ui(widget, ui)
    }

    /// Build with validation, then render
    pub fn build(self, ui: &mut egui::Ui) -> Result<egui::Response, String> {
        let widget = self.build_internal();
        widget.validate()?;
        Ok(egui::Widget::ui(widget, ui))
    }

    /// Build with validation (returns widget for reuse)
    pub fn try_build(self) -> Result<MyWidget, String> {
        let widget = self.build_internal();
        widget.validate()?;
        Ok(widget)
    }
}
```

### Step 4: Implement egui::Widget

```rust
impl egui::Widget for MyWidget {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        // If key is provided, wrap in ID scope for state persistence
        if let Some(key) = self.key {
            ui.push_id(key, |ui| self.render(ui)).inner
        } else {
            self.render(ui)
        }
    }
}

impl MyWidget {
    /// Internal rendering method (separated to handle key scoping)
    fn render(self, ui: &mut egui::Ui) -> egui::Response {
        // 1. Calculate size
        let size = egui::vec2(
            self.width.unwrap_or(100.0),
            self.height.unwrap_or(50.0),
        );

        // 2. Allocate space
        let (rect, mut response) = ui.allocate_exact_size(size, egui::Sense::hover());

        // 3. Paint background
        if let Some(color) = self.color {
            let egui_color = egui::Color32::from_rgba_premultiplied(
                color.r, color.g, color.b, color.a
            );
            ui.painter().rect_filled(rect, 0.0, egui_color);
        }

        // 4. Render child if present
        if let Some(child_fn) = self.child {
            let inner_rect = rect.shrink2(egui::vec2(
                self.padding.left + self.padding.right,
                self.padding.top + self.padding.bottom,
            ));

            let ui_builder = egui::UiBuilder::new()
                .max_rect(inner_rect);

            let mut child_ui = ui.new_child(ui_builder);
            child_fn(&mut child_ui);
        }

        response
    }
}
```

### Step 5: Implement WidgetExt

```rust
use crate::widgets::WidgetExt;

impl WidgetExt for MyWidget {
    fn id(&self) -> Option<egui::Id> {
        self.key
    }

    fn validate(&self) -> Result<(), String> {
        MyWidget::validate(self)
    }

    fn debug_name(&self) -> &'static str {
        "MyWidget"
    }

    fn size_hint(&self, _ui: &egui::Ui) -> Option<egui::Vec2> {
        // Return size if known in advance
        if let (Some(w), Some(h)) = (self.width, self.height) {
            Some(egui::vec2(
                w + self.padding.horizontal_total(),
                h + self.padding.vertical_total(),
            ))
        } else {
            None  // Size depends on content/available space
        }
    }
}
```

### Step 6: Add Factory Methods (Optional)

```rust
impl MyWidget {
    /// Create a widget with a specific color
    pub fn colored(color: impl Into<Color>) -> Self {
        Self {
            color: Some(color.into()),
            ..Self::new()
        }
    }

    /// Create a widget with fixed size
    pub fn sized(width: f32, height: f32) -> Self {
        Self {
            width: Some(width),
            height: Some(height),
            ..Self::new()
        }
    }
}
```

---

### Closure Widgets

For simple, lightweight, ad-hoc widgets without complex configuration.

Closure widgets leverage egui's blanket implementation: `impl<F> Widget for F where F: FnOnce(&mut Ui) -> Response`.

#### Pattern 1: Inline Closure

```rust
// Quick, inline widget
ui.add(|ui: &mut egui::Ui| {
    ui.horizontal(|ui| {
        ui.label("Status:");
        ui.colored_label(egui::Color32::GREEN, "Online");
    })
    .response
});
```

#### Pattern 2: Function Returning impl Widget

The most flexible closure widget pattern - enables reusable widget functions:

```rust
/// Custom slider widget function
pub fn labeled_slider(label: &str, value: &mut f32, range: std::ops::RangeInclusive<f32>)
    -> impl egui::Widget + '_
{
    move |ui: &mut egui::Ui| {
        ui.horizontal(|ui| {
            ui.label(label);
            ui.add(egui::Slider::new(value, range));
        })
        .response
    }
}

// Usage
ui.add(labeled_slider("Volume:", &mut volume, 0.0..=1.0));
```

#### Pattern 3: Stateful Closure (with captures)

```rust
pub fn progress_bar(progress: f32, color: Color) -> impl egui::Widget {
    move |ui: &mut egui::Ui| {
        let desired_size = egui::vec2(ui.available_width(), 20.0);
        let (rect, response) = ui.allocate_exact_size(desired_size, egui::Sense::hover());

        if ui.is_rect_visible(rect) {
            // Draw background
            ui.painter().rect_filled(rect, 4.0, egui::Color32::DARK_GRAY);

            // Draw progress
            let progress_rect = egui::Rect::from_min_size(
                rect.min,
                egui::vec2(rect.width() * progress, rect.height()),
            );
            let egui_color = egui::Color32::from_rgba_premultiplied(
                color.r, color.g, color.b, color.a
            );
            ui.painter().rect_filled(progress_rect, 4.0, egui_color);
        }

        response
    }
}

// Usage
ui.add(progress_bar(0.75, Color::GREEN));
```

#### When to Use Each Form

| Use Case | Form | Why |
|----------|------|-----|
| **Quick, one-off UI** | Inline closure | No boilerplate |
| **Reusable component** | Function → `impl Widget` | Clean, composable |
| **Complex configuration** | Struct Widget (bon) | Type-safe, validatable |
| **State across frames** | Controller | Mutable, persistent |

#### Closure Widget Limitations

❌ **Cannot have:**
- key field for state persistence
- validate() before rendering
- size_hint() optimization
- debug visualization
- Full WidgetExt capabilities

✅ **Can do:**
- Quick prototyping
- Simple reusable components
- Capture variables from scope
- Return from functions
- Compose with other widgets

#### Example: Combining Both Forms

```rust
// Complex base widget with bon builder
let container = Container::builder()
    .key("my_panel")
    .width(400.0)
    .padding(EdgeInsets::all(16.0));

// Add simple closure widget as child
container.child(|ui| {
    // Inline closure widget for dynamic content
    ui.add(|ui: &mut egui::Ui| {
        ui.vertical(|ui| {
            ui.heading("Quick Panel");
            ui.add(labeled_slider("Volume", &mut volume, 0.0..=1.0));
            ui.add(progress_bar(progress, Color::BLUE));
        })
        .response
    })
}).ui(ui);
```

---

## Writing a Controller

### Step 1: Define the Struct

**Controllers do NOT use bon builders** - they are mutable state containers.

```rust
/// Animation controller for smooth value transitions
pub struct AnimationController {
    /// Current animated value (0.0 to 1.0)
    pub value: f64,

    /// Target value we're animating towards
    pub target: f64,

    /// Animation duration
    pub duration: Duration,

    /// Current animation state
    pub state: AnimationState,

    /// Time when animation started
    pub start_time: Option<f64>,

    /// Animation curve (easing function)
    pub curve: Box<dyn Curve>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnimationState {
    Idle,
    Forward,
    Reverse,
    Completed,
}
```

### Step 2: Implement Constructor and Core Methods

```rust
impl AnimationController {
    /// Create a new animation controller
    pub fn new(duration: Duration) -> Self {
        Self {
            value: 0.0,
            target: 0.0,
            duration,
            state: AnimationState::Idle,
            start_time: None,
            curve: Box::new(Curves::ease_in_out()),
        }
    }

    /// Start animation forward (0.0 → 1.0)
    pub fn forward(&mut self) {
        self.target = 1.0;
        self.state = AnimationState::Forward;
        self.start_time = None;  // Will be set on first tick
    }

    /// Start animation reverse (1.0 → 0.0)
    pub fn reverse(&mut self) {
        self.target = 0.0;
        self.state = AnimationState::Reverse;
        self.start_time = None;
    }

    /// Stop animation at current value
    pub fn stop(&mut self) {
        self.state = AnimationState::Idle;
        self.start_time = None;
    }

    /// Check if animation is currently running
    pub fn is_animating(&self) -> bool {
        matches!(self.state, AnimationState::Forward | AnimationState::Reverse)
    }

    /// Update animation (call each frame)
    pub fn tick(&mut self) {
        if !self.is_animating() {
            return;
        }

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs_f64();

        // Initialize start time on first tick
        if self.start_time.is_none() {
            self.start_time = Some(now);
        }

        let elapsed = now - self.start_time.unwrap();
        let duration_secs = self.duration.as_secs_f64();

        if elapsed >= duration_secs {
            // Animation complete
            self.value = self.target;
            self.state = AnimationState::Completed;
            self.start_time = None;
        } else {
            // Interpolate using curve
            let t = elapsed / duration_secs;
            let curved_t = self.curve.transform(t);

            let start = if self.state == AnimationState::Forward { 0.0 } else { 1.0 };
            self.value = start + (self.target - start) * curved_t;
        }
    }
}
```

### Step 3: Implement Controller Trait

```rust
use crate::controllers::Controller;

impl Controller for AnimationController {
    fn update(&mut self, ctx: &egui::Context) {
        self.tick();

        // Request repaint if still animating
        if self.is_active() {
            ctx.request_repaint();
        }
    }

    fn reset(&mut self) {
        self.value = 0.0;
        self.target = 0.0;
        self.state = AnimationState::Idle;
        self.start_time = None;
    }

    fn debug_name(&self) -> &'static str {
        "AnimationController"
    }

    fn is_active(&self) -> bool {
        self.is_animating()
    }
}
```

### Step 4: Add Builder Pattern (Optional)

Controllers can use traditional builder pattern (NOT bon):

```rust
pub struct AnimationControllerBuilder {
    duration: Duration,
    curve: Option<Box<dyn Curve>>,
    initial_value: f64,
}

impl AnimationControllerBuilder {
    pub fn new(duration: Duration) -> Self {
        Self {
            duration,
            curve: None,
            initial_value: 0.0,
        }
    }

    pub fn curve(mut self, curve: impl Curve + 'static) -> Self {
        self.curve = Some(Box::new(curve));
        self
    }

    pub fn initial_value(mut self, value: f64) -> Self {
        self.initial_value = value;
        self
    }

    pub fn build(self) -> AnimationController {
        let mut controller = AnimationController::new(self.duration);
        if let Some(curve) = self.curve {
            controller.curve = curve;
        }
        controller.value = self.initial_value;
        controller
    }
}

impl AnimationController {
    pub fn builder(duration: Duration) -> AnimationControllerBuilder {
        AnimationControllerBuilder::new(duration)
    }
}
```

---

## Best Practices

### Widgets

#### ✅ DO:
- Always include `key: Option<egui::Id>` as the first field
- Use `#[builder(into)]` for fields that can be converted (Color, EdgeInsets, etc.)
- Implement `validate()` to catch configuration errors early
- Implement `size_hint()` if size can be known in advance
- Use `render()` method pattern for separating key handling from rendering
- Provide factory methods for common use cases (`colored()`, `sized()`, etc.)
- Make fields public for struct literal syntax support
- Use `EdgeInsets`, `Color`, and other nebula-ui types consistently

#### ❌ DON'T:
- Don't use `&mut self` methods in widgets (use Controllers instead)
- Don't store mutable state in widgets (they're recreated each frame)
- Don't implement both `Widget` and `Controller` on the same type
- Don't forget to handle the `key` field in `egui::Widget::ui()`
- Don't use bon builders for Controllers (only for Widgets)

### Controllers

#### ✅ DO:
- Use `&mut self` for all state-modifying methods
- Implement `Controller` trait properly
- Call `ctx.request_repaint()` in `update()` if animation is active
- Provide clear start/stop/reset methods
- Document state transitions clearly
- Use traditional builder pattern if needed (not bon)

#### ❌ DON'T:
- Don't implement `egui::Widget` for Controllers
- Don't use bon builders for Controllers
- Don't consume controllers (`self` move)
- Don't forget to call `update()` each frame
- Don't forget to stop repainting when animation completes

---

## Common Patterns

### Pattern 1: Widget with Optional Child

```rust
#[derive(Builder)]
pub struct Panel {
    #[builder(into)]
    pub key: Option<egui::Id>,

    pub width: Option<f32>,

    #[builder(setters(vis = "", name = child_internal))]
    pub child: Option<Box<dyn FnOnce(&mut egui::Ui) -> egui::Response>>,
}

// Smart setter for child
impl<S: State> PanelBuilder<S> {
    pub fn child<F>(self, child: F) -> PanelBuilder<SetChild<S>>
    where
        S::Child: IsUnset,
        F: FnOnce(&mut egui::Ui) -> egui::Response + 'static,
    {
        self.child_internal(Box::new(child))
    }
}
```

### Pattern 2: Widget with Validation

```rust
impl MyWidget {
    pub fn validate(&self) -> Result<(), String> {
        if let Some(width) = self.width {
            if width < 0.0 {
                return Err("Width cannot be negative".to_string());
            }
        }

        if self.width.is_some() && (self.min_width.is_some() || self.max_width.is_some()) {
            return Err("Cannot set both 'width' and 'min_width'/'max_width'".to_string());
        }

        Ok(())
    }
}
```

### Pattern 3: Controller with Callbacks

```rust
pub struct ScrollController {
    pub offset: f32,
    pub velocity: f32,
    pub on_scroll: Option<Box<dyn FnMut(f32)>>,
}

impl ScrollController {
    pub fn scroll_to(&mut self, offset: f32) {
        self.offset = offset;

        // Notify listener
        if let Some(ref mut callback) = self.on_scroll {
            callback(offset);
        }
    }
}
```

### Pattern 4: Widget + Controller Combo

```rust
// Widget (declarative, immediate)
pub struct AnimatedContainer {
    pub key: Option<egui::Id>,
    pub width: Option<f32>,
    // ... other fields
}

// Controller (stateful, persistent)
pub struct AnimatedContainerController {
    pub animation: AnimationController,
    pub current_width: f32,
}

// Usage:
fn my_ui(ui: &mut egui::Ui, controller: &mut AnimatedContainerController) {
    controller.animation.update(ui.ctx());

    AnimatedContainer::builder()
        .key("animated")
        .width(controller.current_width)
        .ui(ui);
}
```

---

## Testing

### Widget Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_widget_creation() {
        let widget = MyWidget::new();
        assert!(widget.key.is_none());
        assert!(widget.width.is_none());
    }

    #[test]
    fn test_widget_validation() {
        let widget = MyWidget {
            width: Some(-10.0),
            ..Default::default()
        };

        assert!(widget.validate().is_err());
    }

    #[test]
    fn test_widget_size_hint() {
        let widget = MyWidget {
            width: Some(100.0),
            height: Some(50.0),
            padding: EdgeInsets::all(10.0),
            ..Default::default()
        };

        let hint = widget.size_hint(&create_test_ui());
        assert_eq!(hint, Some(egui::vec2(120.0, 70.0)));
    }

    #[test]
    fn test_bon_builder() {
        let widget = MyWidget::builder()
            .key("test")
            .width(100.0)
            .try_build()
            .unwrap();

        assert!(widget.key.is_some());
        assert_eq!(widget.width, Some(100.0));
    }
}
```

### Controller Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_controller_creation() {
        let controller = AnimationController::new(Duration::milliseconds(300));
        assert_eq!(controller.value, 0.0);
        assert_eq!(controller.state, AnimationState::Idle);
    }

    #[test]
    fn test_controller_forward() {
        let mut controller = AnimationController::new(Duration::milliseconds(300));
        controller.forward();

        assert_eq!(controller.state, AnimationState::Forward);
        assert_eq!(controller.target, 1.0);
        assert!(controller.is_animating());
    }

    #[test]
    fn test_controller_tick() {
        let mut controller = AnimationController::new(Duration::milliseconds(100));
        controller.forward();

        // Simulate ticks
        for _ in 0..10 {
            controller.tick();
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        // Should be near completion
        assert!(controller.value > 0.5);
    }

    #[test]
    fn test_controller_reset() {
        let mut controller = AnimationController::new(Duration::milliseconds(300));
        controller.forward();
        controller.value = 0.5;

        controller.reset();

        assert_eq!(controller.value, 0.0);
        assert_eq!(controller.state, AnimationState::Idle);
    }
}
```

---

## Summary

### Widget Checklist

- [ ] Struct with `#[derive(Builder)]`
- [ ] First field: `key: Option<egui::Id>` with `#[builder(into)]`
- [ ] Public fields for struct literal support
- [ ] `new()` and `Default` implementation
- [ ] `validate()` method
- [ ] bon smart setters for `child()` if needed
- [ ] bon custom finishing functions (`.ui()`, `.build()`, `.try_build()`)
- [ ] `egui::Widget` implementation with key handling
- [ ] `render()` internal method
- [ ] `WidgetExt` implementation
- [ ] Factory methods for common cases (optional)
- [ ] Tests for creation, validation, size hints

### Controller Checklist

- [ ] Struct with mutable state fields
- [ ] `new()` constructor
- [ ] State modification methods using `&mut self`
- [ ] `Controller` trait implementation
- [ ] `update(&mut self, ctx)` with repaint requests
- [ ] `reset(&mut self)` to initial state
- [ ] `is_active()` for activity checking
- [ ] Traditional builder pattern if needed (NOT bon)
- [ ] Tests for state transitions and updates

---

## Example: Complete Widget

See [Container](../src/widgets/primitives/container.rs) for a complete, production-ready example implementing all patterns.

## Example: Complete Controller

See [AnimationController](../src/controllers/animation.rs) for a complete, production-ready controller example.
