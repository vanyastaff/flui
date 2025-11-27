# Widget Implementation Guide - Practical Examples

## Table of Contents

- [Stateless Widgets](#stateless-widgets)
- [Stateful Widgets](#stateful-widgets)
- [Animated Widgets](#animated-widgets)
- [Provider Widgets](#provider-widgets)
- [Proxy Widgets](#proxy-widgets)
- [RenderBox Widgets](#renderbox-widgets)
- [Testing Widgets](#testing-widgets)

---

## Stateless Widgets

### Template: Basic Stateless Widget

```rust
use flui_core::view::{StatelessView, IntoElement};
use flui_core::BuildContext;
use flui_types::Color;

/// A simple colored box widget
#[derive(Debug, Clone)]
pub struct ColoredBox {
    pub color: Color,
    pub child: Option<Box<dyn IntoElement>>,
}

impl ColoredBox {
    /// Create a new ColoredBox
    pub fn new(color: Color) -> Self {
        Self { color, child: None }
    }

    /// Create with a child
    pub fn with_child(color: Color, child: impl IntoElement + 'static) -> Self {
        Self {
            color,
            child: Some(Box::new(child)),
        }
    }
}

impl StatelessView for ColoredBox {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        // Delegate to RenderObject
        use flui_rendering::objects::basic::RenderColoredBox;

        RenderColoredBox::new(self.color, self.child)
    }
}
```

### Example 1: Container (Composition Widget)

```rust
use bon::Builder;
use flui_core::view::{StatelessView, IntoElement};
use flui_core::BuildContext;
use flui_types::{Color, EdgeInsets, BoxConstraints, Alignment};

/// Container - composes multiple layout widgets
#[derive(Debug, Clone, Builder)]
pub struct Container {
    #[builder(default)]
    pub child: Option<Box<dyn IntoElement>>,

    #[builder(default)]
    pub padding: Option<EdgeInsets>,

    #[builder(default)]
    pub margin: Option<EdgeInsets>,

    #[builder(default)]
    pub color: Option<Color>,

    #[builder(default)]
    pub width: Option<f32>,

    #[builder(default)]
    pub height: Option<f32>,

    #[builder(default)]
    pub constraints: Option<BoxConstraints>,

    #[builder(default)]
    pub alignment: Option<Alignment>,
}

impl Container {
    /// Create empty container
    pub fn new() -> Self {
        Self {
            child: None,
            padding: None,
            margin: None,
            color: None,
            width: None,
            height: None,
            constraints: None,
            alignment: None,
        }
    }

    /// Quick constructors
    pub fn colored(color: Color, child: impl IntoElement + 'static) -> Self {
        Self::builder()
            .color(Some(color))
            .child(Some(Box::new(child)))
            .build()
    }

    pub fn padded(padding: EdgeInsets, child: impl IntoElement + 'static) -> Self {
        Self::builder()
            .padding(Some(padding))
            .child(Some(Box::new(child)))
            .build()
    }
}

impl StatelessView for Container {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        let mut current: Box<dyn IntoElement> = match self.child {
            Some(child) => child,
            None => Box::new(crate::Empty),
        };

        // Apply transformations from inside out (like Flutter)

        // 1. Padding (innermost)
        if let Some(padding) = self.padding {
            current = Box::new(crate::Padding::new(padding, current));
        }

        // 2. Alignment
        if let Some(alignment) = self.alignment {
            current = Box::new(crate::Align::new(alignment, current));
        }

        // 3. Decoration (color, borders, shadows)
        if let Some(color) = self.color {
            current = Box::new(crate::ColoredBox::new(color, current));
        }

        // 4. Constraints (width, height, or explicit constraints)
        if self.width.is_some() || self.height.is_some() || self.constraints.is_some() {
            let constraints = self.constraints.unwrap_or_else(|| {
                BoxConstraints::new(
                    self.width.unwrap_or(0.0),
                    self.width.unwrap_or(f32::INFINITY),
                    self.height.unwrap_or(0.0),
                    self.height.unwrap_or(f32::INFINITY),
                )
            });
            current = Box::new(crate::ConstrainedBox::new(constraints, current));
        }

        // 5. Margin (outermost)
        if let Some(margin) = self.margin {
            current = Box::new(crate::Padding::new(margin, current));
        }

        current
    }
}
```

### Example 2: Row/Column (Multi-child Layout)

```rust
use flui_core::view::{StatelessView, IntoElement};
use flui_core::BuildContext;
use flui_types::{Axis, MainAxisAlignment, CrossAxisAlignment};

/// Row widget - horizontal layout
#[derive(Debug, Clone)]
pub struct Row {
    pub children: Vec<Box<dyn IntoElement>>,
    pub main_axis_alignment: MainAxisAlignment,
    pub cross_axis_alignment: CrossAxisAlignment,
    pub main_axis_size: MainAxisSize,
}

impl Row {
    pub fn new(children: Vec<Box<dyn IntoElement>>) -> Self {
        Self {
            children,
            main_axis_alignment: MainAxisAlignment::Start,
            cross_axis_alignment: CrossAxisAlignment::Center,
            main_axis_size: MainAxisSize::Max,
        }
    }

    /// Builder pattern
    pub fn builder() -> RowBuilder {
        RowBuilder::new()
    }
}

impl StatelessView for Row {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        use flui_rendering::objects::layout::RenderFlex;

        RenderFlex::new(Axis::Horizontal)
            .main_axis_alignment(self.main_axis_alignment)
            .cross_axis_alignment(self.cross_axis_alignment)
            .main_axis_size(self.main_axis_size)
            .children(self.children)
    }
}

/// Column widget - vertical layout
#[derive(Debug, Clone)]
pub struct Column {
    pub children: Vec<Box<dyn IntoElement>>,
    pub main_axis_alignment: MainAxisAlignment,
    pub cross_axis_alignment: CrossAxisAlignment,
    pub main_axis_size: MainAxisSize,
}

impl StatelessView for Column {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        use flui_rendering::objects::layout::RenderFlex;

        RenderFlex::new(Axis::Vertical)
            .main_axis_alignment(self.main_axis_alignment)
            .cross_axis_alignment(self.cross_axis_alignment)
            .main_axis_size(self.main_axis_size)
            .children(self.children)
    }
}

// Convenient macro for creating rows/columns
#[macro_export]
macro_rules! row {
    ($($child:expr),* $(,)?) => {
        $crate::Row::new(vec![$(Box::new($child)),*])
    };
}

#[macro_export]
macro_rules! column {
    ($($child:expr),* $(,)?) => {
        $crate::Column::new(vec![$(Box::new($child)),*])
    };
}
```

---

## Stateful Widgets

### Template: Basic Stateful Widget

```rust
use flui_core::view::{StatefulView, IntoElement};
use flui_core::BuildContext;

/// Widget configuration (immutable)
#[derive(Debug, Clone)]
pub struct Counter {
    pub initial_value: i32,
    pub on_changed: Option<Arc<dyn Fn(i32) + Send + Sync>>,
}

/// Widget state (mutable)
#[derive(Debug)]
pub struct CounterState {
    pub value: i32,
}

impl Counter {
    pub fn new(initial_value: i32) -> Self {
        Self {
            initial_value,
            on_changed: None,
        }
    }
}

impl StatefulView for Counter {
    type State = CounterState;

    fn create_state(&self) -> Self::State {
        CounterState {
            value: self.initial_value,
        }
    }

    fn build(&self, state: &mut Self::State, ctx: &BuildContext) -> impl IntoElement {
        use crate::{Column, Text, Button};

        let value = state.value;
        let on_changed = self.on_changed.clone();

        column![
            Text::new(format!("Count: {}", value)),
            Button::new("Increment")
                .on_tap(move || {
                    state.value += 1;
                    if let Some(ref callback) = on_changed {
                        callback(state.value);
                    }
                    // Mark dirty for rebuild
                    ctx.mark_needs_build();
                }),
        ]
    }
}

// State methods (for external access)
impl CounterState {
    pub fn increment(&mut self, ctx: &BuildContext) {
        self.value += 1;
        ctx.mark_needs_build();
    }

    pub fn reset(&mut self, ctx: &BuildContext) {
        self.value = 0;
        ctx.mark_needs_build();
    }
}
```

### Example 1: Checkbox

```rust
use bon::Builder;
use flui_core::view::{StatefulView, IntoElement};
use flui_core::BuildContext;
use flui_types::Color;
use std::sync::Arc;

pub type CheckboxCallback = Arc<dyn Fn(bool) + Send + Sync>;

/// Checkbox widget
#[derive(Debug, Clone, Builder)]
pub struct Checkbox {
    pub value: bool,

    #[builder(default)]
    pub on_changed: Option<CheckboxCallback>,

    #[builder(default = Color::rgb(33, 150, 243))]
    pub active_color: Color,

    #[builder(default = 24.0)]
    pub size: f32,
}

#[derive(Debug)]
pub struct CheckboxState {
    pub checked: bool,
    pub hovered: bool,
    pub pressed: bool,
}

impl Checkbox {
    pub fn new(value: bool) -> Self {
        Self {
            value,
            on_changed: None,
            active_color: Color::rgb(33, 150, 243),
            size: 24.0,
        }
    }
}

impl StatefulView for Checkbox {
    type State = CheckboxState;

    fn create_state(&self) -> Self::State {
        CheckboxState {
            checked: self.value,
            hovered: false,
            pressed: false,
        }
    }

    fn build(&self, state: &mut Self::State, ctx: &BuildContext) -> impl IntoElement {
        use crate::{GestureDetector, Container, CustomPaint};

        let checked = state.checked;
        let active_color = self.active_color;
        let size = self.size;
        let on_changed = self.on_changed.clone();

        GestureDetector::new()
            .on_tap(move || {
                state.checked = !state.checked;
                if let Some(ref callback) = on_changed {
                    callback(state.checked);
                }
                ctx.mark_needs_build();
            })
            .on_hover_enter(move || {
                state.hovered = true;
                ctx.mark_needs_build();
            })
            .on_hover_exit(move || {
                state.hovered = false;
                ctx.mark_needs_build();
            })
            .child(
                Container::builder()
                    .width(Some(size))
                    .height(Some(size))
                    .child(Some(Box::new(
                        CustomPaint::new(Box::new(move |canvas, bounds| {
                            // Draw checkbox
                            let rect = Rect::from_size(bounds);
                            let paint = Paint::fill(if checked {
                                active_color
                            } else {
                                Color::TRANSPARENT
                            });

                            canvas.draw_rounded_rect(rect, 2.0, &paint);

                            // Draw border
                            let border_paint = Paint::stroke(
                                if checked { active_color } else { Color::GRAY },
                                2.0
                            );
                            canvas.draw_rounded_rect(rect, 2.0, &border_paint);

                            // Draw checkmark if checked
                            if checked {
                                // ... draw checkmark path
                            }
                        }))
                    )))
                    .build()
            )
    }
}
```

### Example 2: TextField

```rust
use flui_core::view::{StatefulView, IntoElement};
use flui_core::BuildContext;
use flui_types::{Color, TextStyle};
use std::sync::Arc;

pub type TextChangedCallback = Arc<dyn Fn(&str) + Send + Sync>;

/// TextField widget - single-line text input
#[derive(Debug, Clone)]
pub struct TextField {
    pub initial_value: String,
    pub placeholder: Option<String>,
    pub style: TextStyle,
    pub on_changed: Option<TextChangedCallback>,
    pub max_length: Option<usize>,
    pub obscure_text: bool,
}

#[derive(Debug)]
pub struct TextFieldState {
    pub text: String,
    pub cursor_position: usize,
    pub selection_start: Option<usize>,
    pub selection_end: Option<usize>,
    pub is_focused: bool,
    pub cursor_visible: bool,
}

impl TextField {
    pub fn new() -> Self {
        Self {
            initial_value: String::new(),
            placeholder: None,
            style: TextStyle::default(),
            on_changed: None,
            max_length: None,
            obscure_text: false,
        }
    }

    pub fn with_placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = Some(placeholder.into());
        self
    }
}

impl StatefulView for TextField {
    type State = TextFieldState;

    fn create_state(&self) -> Self::State {
        TextFieldState {
            text: self.initial_value.clone(),
            cursor_position: self.initial_value.len(),
            selection_start: None,
            selection_end: None,
            is_focused: false,
            cursor_visible: true,
        }
    }

    fn build(&self, state: &mut Self::State, ctx: &BuildContext) -> impl IntoElement {
        use crate::{GestureDetector, Container};
        use flui_rendering::objects::text::RenderEditableLine;

        let text = state.text.clone();
        let style = self.style.clone();
        let on_changed = self.on_changed.clone();

        GestureDetector::new()
            .on_tap(move || {
                state.is_focused = true;
                ctx.mark_needs_build();
            })
            .child(
                Container::builder()
                    .padding(Some(EdgeInsets::all(8.0)))
                    .child(Some(Box::new(
                        RenderEditableLine::new(text, style)
                    )))
                    .build()
            )
    }
}

impl TextFieldState {
    pub fn insert_text(&mut self, text: &str, ctx: &BuildContext) {
        self.text.insert_str(self.cursor_position, text);
        self.cursor_position += text.len();
        ctx.mark_needs_build();
    }

    pub fn delete_before_cursor(&mut self, ctx: &BuildContext) {
        if self.cursor_position > 0 {
            self.text.remove(self.cursor_position - 1);
            self.cursor_position -= 1;
            ctx.mark_needs_build();
        }
    }
}
```

---

## Animated Widgets

### Template: Transition Widget

```rust
use flui_core::view::{AnimatedView, IntoElement};
use flui_core::BuildContext;
use flui_animation::Animation;

/// FadeTransition - animates opacity
#[derive(Debug, Clone)]
pub struct FadeTransition {
    pub opacity: Animation<f32>,
    pub child: Box<dyn IntoElement>,
}

impl FadeTransition {
    pub fn new(opacity: Animation<f32>, child: impl IntoElement + 'static) -> Self {
        Self {
            opacity,
            child: Box::new(child),
        }
    }
}

impl AnimatedView for FadeTransition {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        use crate::Opacity;

        // Rebuilds on every animation tick
        Opacity::new(self.opacity.value(), self.child)
    }

    fn animation(&self) -> Vec<&dyn AnimationListenable> {
        vec![&self.opacity as &dyn AnimationListenable]
    }
}
```

### Example 1: AnimatedContainer

```rust
use flui_core::view::{AnimatedView, IntoElement};
use flui_core::BuildContext;
use flui_animation::{AnimationController, Curve, Duration};
use flui_types::{Color, EdgeInsets, BoxDecoration};

/// AnimatedContainer - implicitly animates property changes
#[derive(Debug, Clone)]
pub struct AnimatedContainer {
    pub child: Option<Box<dyn IntoElement>>,
    pub width: Option<f32>,
    pub height: Option<f32>,
    pub color: Option<Color>,
    pub padding: Option<EdgeInsets>,
    pub decoration: Option<BoxDecoration>,
    pub duration: Duration,
    pub curve: Curve,
}

#[derive(Debug)]
pub struct AnimatedContainerState {
    pub controller: AnimationController,
    pub width_animation: Animation<f32>,
    pub height_animation: Animation<f32>,
    pub color_animation: Animation<Color>,
}

impl AnimatedContainer {
    pub fn new() -> Self {
        Self {
            child: None,
            width: None,
            height: None,
            color: None,
            padding: None,
            decoration: None,
            duration: Duration::from_millis(300),
            curve: Curve::EaseInOut,
        }
    }
}

impl AnimatedView for AnimatedContainer {
    type State = AnimatedContainerState;

    fn create_state(&self) -> Self::State {
        AnimatedContainerState {
            controller: AnimationController::new(self.duration),
            width_animation: Animation::new(self.width.unwrap_or(0.0)),
            height_animation: Animation::new(self.height.unwrap_or(0.0)),
            color_animation: Animation::new(self.color.unwrap_or(Color::TRANSPARENT)),
        }
    }

    fn build(&self, state: &mut Self::State, ctx: &BuildContext) -> impl IntoElement {
        use crate::Container;

        // Animate to new target values
        if state.width_animation.target() != self.width.unwrap_or(0.0) {
            state.width_animation.animate_to(self.width.unwrap_or(0.0), self.duration);
        }

        Container::builder()
            .width(Some(state.width_animation.value()))
            .height(Some(state.height_animation.value()))
            .color(Some(state.color_animation.value()))
            .padding(self.padding)
            .decoration(self.decoration.clone())
            .child(self.child.clone())
            .build()
    }

    fn animations(&self, state: &Self::State) -> Vec<&dyn AnimationListenable> {
        vec![
            &state.width_animation,
            &state.height_animation,
            &state.color_animation,
        ]
    }
}
```

### Example 2: SlideTransition

```rust
use flui_core::view::{AnimatedView, IntoElement};
use flui_core::BuildContext;
use flui_animation::Animation;
use flui_types::Offset;

/// SlideTransition - animates position
#[derive(Debug, Clone)]
pub struct SlideTransition {
    pub position: Animation<Offset>,
    pub child: Box<dyn IntoElement>,
}

impl SlideTransition {
    pub fn new(position: Animation<Offset>, child: impl IntoElement + 'static) -> Self {
        Self {
            position,
            child: Box::new(child),
        }
    }
}

impl AnimatedView for SlideTransition {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        use crate::Transform;

        Transform::translate(
            self.position.value().dx,
            self.position.value().dy,
            self.child,
        )
    }

    fn animations(&self) -> Vec<&dyn AnimationListenable> {
        vec![&self.position]
    }
}
```

---

## Provider Widgets

### Template: Basic Provider

```rust
use flui_core::view::{ProviderView, IntoElement};
use flui_core::BuildContext;

/// Theme data structure
#[derive(Debug, Clone)]
pub struct ThemeData {
    pub primary_color: Color,
    pub accent_color: Color,
    pub text_theme: TextTheme,
    pub brightness: Brightness,
}

/// Theme provider widget
#[derive(Debug, Clone)]
pub struct Theme {
    pub data: ThemeData,
    pub child: Box<dyn IntoElement>,
}

impl Theme {
    pub fn new(data: ThemeData, child: impl IntoElement + 'static) -> Self {
        Self {
            data,
            child: Box::new(child),
        }
    }

    /// Get theme from context (in descendant widgets)
    pub fn of(ctx: &BuildContext) -> ThemeData {
        ctx.depend_on::<ThemeData>()
            .cloned()
            .unwrap_or_else(ThemeData::default)
    }
}

impl ProviderView for Theme {
    type Provided = ThemeData;

    fn provided_value(&self) -> &Self::Provided {
        &self.data
    }

    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        // Just return child, framework handles data propagation
        self.child
    }
}
```

### Example 1: MediaQuery

```rust
use flui_core::view::{ProviderView, IntoElement};
use flui_core::BuildContext;
use flui_types::Size;

/// Screen/device information
#[derive(Debug, Clone)]
pub struct MediaQueryData {
    pub size: Size,
    pub device_pixel_ratio: f32,
    pub text_scale_factor: f32,
    pub padding: EdgeInsets,
    pub view_insets: EdgeInsets,
    pub platform_brightness: Brightness,
}

/// MediaQuery provider
#[derive(Debug, Clone)]
pub struct MediaQuery {
    pub data: MediaQueryData,
    pub child: Box<dyn IntoElement>,
}

impl MediaQuery {
    pub fn new(data: MediaQueryData, child: impl IntoElement + 'static) -> Self {
        Self {
            data,
            child: Box::new(child),
        }
    }

    /// Get media query data from context
    pub fn of(ctx: &BuildContext) -> MediaQueryData {
        ctx.depend_on::<MediaQueryData>()
            .cloned()
            .expect("MediaQuery not found in widget tree")
    }

    /// Shortcuts
    pub fn size_of(ctx: &BuildContext) -> Size {
        Self::of(ctx).size
    }

    pub fn pixel_ratio_of(ctx: &BuildContext) -> f32 {
        Self::of(ctx).device_pixel_ratio
    }
}

impl ProviderView for MediaQuery {
    type Provided = MediaQueryData;

    fn provided_value(&self) -> &Self::Provided {
        &self.data
    }

    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        self.child
    }
}

// Usage in widgets:
impl StatelessView for ResponsiveContainer {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        let screen_size = MediaQuery::size_of(ctx);

        // Responsive layout based on screen size
        if screen_size.width < 600.0 {
            // Mobile layout
            Column::new(self.children)
        } else {
            // Desktop layout
            Row::new(self.children)
        }
    }
}
```

---

## Proxy Widgets

### Template: Builder Pattern

```rust
use flui_core::view::{ProxyView, IntoElement};
use flui_core::BuildContext;

pub type WidgetBuilder = Box<dyn Fn(&BuildContext) -> Box<dyn IntoElement> + Send + Sync>;

/// Builder widget - lazy widget construction
#[derive(Clone)]
pub struct Builder {
    pub builder: WidgetBuilder,
}

impl Builder {
    pub fn new(builder: impl Fn(&BuildContext) -> Box<dyn IntoElement> + Send + Sync + 'static) -> Self {
        Self {
            builder: Box::new(builder),
        }
    }
}

impl ProxyView for Builder {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        (self.builder)(ctx)
    }
}
```

### Example: LayoutBuilder

```rust
use flui_core::view::{ProxyView, IntoElement};
use flui_core::BuildContext;
use flui_types::BoxConstraints;

pub type LayoutWidgetBuilder = Box<dyn Fn(&BuildContext, BoxConstraints) -> Box<dyn IntoElement> + Send + Sync>;

/// LayoutBuilder - build based on constraints
#[derive(Clone)]
pub struct LayoutBuilder {
    pub builder: LayoutWidgetBuilder,
}

impl LayoutBuilder {
    pub fn new(
        builder: impl Fn(&BuildContext, BoxConstraints) -> Box<dyn IntoElement> + Send + Sync + 'static
    ) -> Self {
        Self {
            builder: Box::new(builder),
        }
    }
}

impl ProxyView for LayoutBuilder {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        // Get constraints from parent
        let constraints = ctx.constraints();
        (self.builder)(ctx, constraints)
    }
}

// Usage:
LayoutBuilder::new(|ctx, constraints| {
    if constraints.max_width < 600.0 {
        Box::new(Text::new("Mobile"))
    } else {
        Box::new(Text::new("Desktop"))
    }
})
```

---

## RenderBox Widgets

### Template: Custom RenderObject Widget

See `flui_rendering` documentation for complete RenderBox implementation details.

**Widget wrapper:**
```rust
use flui_core::view::RenderView;
use flui_rendering::{RenderBox, Leaf};

/// Widget that creates a custom render object
#[derive(Debug, Clone)]
pub struct CustomWidget {
    pub color: Color,
    pub size: f32,
}

impl RenderView for CustomWidget {
    type RenderObject = RenderCustomWidget;

    fn create_render_object(&self) -> Self::RenderObject {
        RenderCustomWidget {
            color: self.color,
            size: self.size,
            cached_size: Size::ZERO,
        }
    }

    fn update_render_object(&self, render_object: &mut Self::RenderObject) {
        render_object.color = self.color;
        render_object.size = self.size;
    }
}
```

---

## Testing Widgets

### Unit Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_widget_creation() {
        let widget = MyWidget::new();
        assert_eq!(widget.initial_value, 0);
    }

    #[test]
    fn test_builder_pattern() {
        let widget = MyWidget::builder()
            .initial_value(42)
            .color(Color::BLUE)
            .build();

        assert_eq!(widget.initial_value, 42);
        assert_eq!(widget.color, Color::BLUE);
    }
}
```

### State Tests (Stateful widgets)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_state_creation() {
        let widget = Counter::new(10);
        let state = widget.create_state();
        assert_eq!(state.value, 10);
    }

    #[test]
    fn test_state_mutation() {
        let widget = Counter::new(0);
        let mut state = widget.create_state();

        state.increment();
        assert_eq!(state.value, 1);

        state.increment();
        assert_eq!(state.value, 2);
    }
}
```

### Build Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use flui_core::testing::TestBuildContext;

    #[test]
    fn test_widget_build() {
        let widget = MyStatelessWidget::new();
        let ctx = TestBuildContext::new();

        let element = widget.build(&ctx);

        // Verify build output
        assert!(element.is_some());
    }
}
```

---

## Best Practices Summary

1. **Choose the right ViewMode** - See decision matrix in WIDGET_ARCHITECTURE.md
2. **Composition over custom rendering** - Prefer Stateless composition
3. **Builder patterns** - Use `bon::Builder` for ergonomic APIs
4. **Type safety** - Leverage Rust enums over strings
5. **Testing** - Unit + build + state tests for all widgets
6. **Documentation** - Doc comments with examples
7. **Derive Clone** - Most widgets should derive Clone
8. **Send + Sync** - All widgets must be thread-safe

---

## Resources

- **WIDGET_ARCHITECTURE.md** - High-level architecture guide
- **flui_rendering docs** - RenderObject implementation details
- **Flutter Widget Catalog** - Reference implementations
- **Examples**: `crates/flui_widgets/src/*/` - Working widget examples

---

**Last Updated:** 2025-11-27
