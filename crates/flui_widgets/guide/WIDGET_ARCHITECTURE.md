# Widget Architecture - ViewMode Protocol Organization

## Overview

FLUI widgets are organized by their **ViewMode**, which determines their lifecycle, capabilities, and rendering behavior. This document provides a comprehensive guide for implementing widgets across all ViewMode protocols.

## ViewMode Protocols

### Component Views (Build children)

| ViewMode | Purpose | State | Lifecycle | Example Widgets |
|----------|---------|-------|-----------|-----------------|
| **Stateless** | Immutable configuration, no state | None | build() only | Container, Padding, Center, Align |
| **Stateful** | Mutable state, lifecycle hooks | Generic `<S>` | init, build, dispose | Checkbox, TextField, AnimatedBuilder |
| **Animated** | Animation-driven updates | Animation controller | tick, build | FadeTransition, RotationTransition |
| **Provider** | Inherited data to descendants | Provided value | build, dependents | Theme, MediaQuery, InheritedWidget |
| **Proxy** | Transparent wrapper | None | build (passthrough) | Builder, LayoutBuilder |

### Render Views (Layout/Paint)

| ViewMode | Purpose | Protocol | Children | Example Widgets |
|----------|---------|----------|----------|-----------------|
| **RenderBox** | Box model layout | BoxConstraints → Size | 0-N | RenderPadding, RenderFlex, RenderImage |
| **RenderSliver** | Scroll-aware layout | SliverConstraints → SliverGeometry | 0-N | RenderSliverList, RenderSliverGrid |

---

## Widget Categories by ViewMode

### 1. Stateless Widgets

**When to use:**
- Pure presentation, no mutable state
- Configuration only (colors, sizes, alignments)
- Composes other widgets

**Implementation:**
```rust
use flui_core::view::{StatelessView, IntoElement};
use flui_core::BuildContext;

#[derive(Debug, Clone)]
pub struct MyStatelessWidget {
    pub color: Color,
    pub child: Option<Box<dyn IntoElement>>,
}

impl StatelessView for MyStatelessWidget {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        // Return composed widget tree
        Container::builder()
            .color(self.color)
            .child(self.child)
            .build()
    }
}
```

**Flutter Equivalents:**
- Container, Padding, Center, Align, SizedBox
- Row, Column, Stack, Wrap
- Text, Icon, Image (without state)
- Card, Divider, Spacer
- DecoratedBox, Transform, Opacity (const values)

**FLUI Examples:**
- `Container` - Composition widget (padding, decoration, constraints)
- `Padding` - Adds space around child
- `Center` - Centers child
- `Align` - Positions child by alignment
- `SizedBox` - Fixed dimensions
- `ColoredBox` - Solid color background
- `Divider` - Horizontal separator

**File Organization:**
```
crates/flui_widgets/src/
  basic/
    container.rs      // Stateless
    padding.rs        // Stateless
    center.rs         // Stateless
    align.rs          // Stateless
    sized_box.rs      // Stateless
    colored_box.rs    // Stateless
```

---

### 2. Stateful Widgets

**When to use:**
- Mutable state that changes over time
- User interaction (input, selection)
- Local animation controllers
- Lifecycle hooks needed (init, dispose)

**Implementation:**
```rust
use flui_core::view::{StatefulView, IntoElement};
use flui_core::BuildContext;

#[derive(Debug, Clone)]
pub struct MyStatefulWidget {
    pub initial_value: i32,
}

#[derive(Debug)]
pub struct MyStatefulWidgetState {
    pub value: i32,
}

impl StatefulView for MyStatefulWidget {
    type State = MyStatefulWidgetState;

    fn create_state(&self) -> Self::State {
        MyStatefulWidgetState {
            value: self.initial_value,
        }
    }

    fn build(&self, state: &mut Self::State, ctx: &BuildContext) -> impl IntoElement {
        // Use state.value in build
        Text::new(format!("Count: {}", state.value))
    }
}

impl MyStatefulWidgetState {
    pub fn increment(&mut self) {
        self.value += 1;
        // Mark dirty for rebuild
    }
}
```

**Flutter Equivalents:**
- TextField, Checkbox, Radio, Switch, Slider
- AnimatedBuilder, AnimatedContainer
- TabController, PageController
- Custom stateful widgets with setState

**FLUI Examples:**
- `Checkbox` - Toggle state
- `TextField` - Text input with cursor
- `Slider` - Draggable value selector
- `Switch` - On/off toggle
- `RadioButton` - Single selection from group
- `TabBar` - Tab selection state
- `ExpansionPanel` - Expand/collapse state

**File Organization:**
```
crates/flui_widgets/src/
  input/
    checkbox.rs       // Stateful
    text_field.rs     // Stateful
    slider.rs         // Stateful
    switch.rs         // Stateful
  navigation/
    tab_bar.rs        // Stateful
  material/
    expansion_panel.rs // Stateful
```

---

### 3. Animated Widgets

**When to use:**
- Animation-driven state updates
- Automatic rebuilds on animation ticks
- Transitions and effects

**Implementation:**
```rust
use flui_core::view::{AnimatedView, IntoElement};
use flui_core::BuildContext;
use flui_animation::{Animation, AnimationController};

#[derive(Debug, Clone)]
pub struct FadeTransition {
    pub opacity: Animation<f32>,
    pub child: Box<dyn IntoElement>,
}

impl AnimatedView for FadeTransition {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        // Rebuild on animation tick
        Opacity::new(self.opacity.value(), self.child)
    }

    fn animation(&self) -> &Animation<f32> {
        &self.opacity
    }
}
```

**Flutter Equivalents:**
- FadeTransition, ScaleTransition, SlideTransition
- RotationTransition, SizeTransition
- AnimatedOpacity, AnimatedContainer, AnimatedPositioned
- TweenAnimationBuilder

**FLUI Examples:**
- `FadeTransition` - Opacity animation
- `ScaleTransition` - Scale animation
- `SlideTransition` - Position animation
- `RotationTransition` - Rotation animation
- `AnimatedOpacity` - Animated opacity
- `AnimatedContainer` - Animated container properties
- `AnimatedBuilder` - Generic animation builder

**File Organization:**
```
crates/flui_widgets/src/
  animation/
    fade_transition.rs      // Animated
    scale_transition.rs     // Animated
    slide_transition.rs     // Animated
    rotation_transition.rs  // Animated
    animated_opacity.rs     // Animated
    animated_container.rs   // Animated
```

---

### 4. Provider Widgets

**When to use:**
- Share data down the tree (theme, locale, media query)
- Dependency injection
- Context-aware configurations

**Implementation:**
```rust
use flui_core::view::{ProviderView, IntoElement};
use flui_core::BuildContext;

#[derive(Debug, Clone)]
pub struct ThemeProvider {
    pub theme: Theme,
    pub child: Box<dyn IntoElement>,
}

impl ProviderView for ThemeProvider {
    type Provided = Theme;

    fn provided_value(&self) -> &Self::Provided {
        &self.theme
    }

    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        // Just return child, framework handles inheritance
        self.child
    }
}

// Usage in descendants
impl StatelessView for ThemedButton {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        let theme = ctx.depend_on::<Theme>();
        Container::builder()
            .color(theme.primary_color)
            .build()
    }
}
```

**Flutter Equivalents:**
- InheritedWidget (base)
- Theme, MediaQuery, Localizations
- Provider (external package)

**FLUI Examples:**
- `Theme` - App-wide theme data
- `MediaQuery` - Screen size, orientation, padding
- `Localizations` - Localized strings
- `DefaultTextStyle` - Default text styling
- `IconTheme` - Default icon styling

**File Organization:**
```
crates/flui_widgets/src/
  theme/
    theme.rs          // Provider
    media_query.rs    // Provider
  localization/
    localizations.rs  // Provider
  styling/
    default_text_style.rs // Provider
    icon_theme.rs     // Provider
```

---

### 5. Proxy Widgets

**When to use:**
- Transparent wrapper (builder pattern)
- Callback-based child creation
- Lazy child construction

**Implementation:**
```rust
use flui_core::view::{ProxyView, IntoElement};
use flui_core::BuildContext;

pub type WidgetBuilder = Box<dyn Fn(&BuildContext) -> Box<dyn IntoElement>>;

#[derive(Clone)]
pub struct Builder {
    pub builder: WidgetBuilder,
}

impl ProxyView for Builder {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        (self.builder)(ctx)
    }
}
```

**Flutter Equivalents:**
- Builder
- LayoutBuilder (receives constraints)
- OrientationBuilder (receives orientation)

**FLUI Examples:**
- `Builder` - Lazy widget construction
- `LayoutBuilder` - Build based on constraints
- `OrientationBuilder` - Build based on orientation

**File Organization:**
```
crates/flui_widgets/src/
  basic/
    builder.rs         // Proxy
    layout_builder.rs  // Proxy
```

---

### 6. RenderBox Widgets

**When to use:**
- Custom layout logic
- Custom painting
- Direct control over children positioning

**Implementation:**
```rust
use flui_core::view::RenderView;
use flui_rendering::{RenderBox, Leaf, LayoutContext, PaintContext};
use flui_types::{Size, BoxConstraints};

pub struct CustomWidget {
    pub color: Color,
}

impl RenderView for CustomWidget {
    type RenderObject = RenderCustomWidget;

    fn create_render_object(&self) -> Self::RenderObject {
        RenderCustomWidget {
            color: self.color,
            size: Size::ZERO,
        }
    }

    fn update_render_object(&self, render_object: &mut Self::RenderObject) {
        render_object.color = self.color;
    }
}

pub struct RenderCustomWidget {
    pub color: Color,
    size: Size,
}

impl RenderBox<Leaf> for RenderCustomWidget {
    fn layout<T>(&mut self, ctx: LayoutContext<'_, T, Leaf, BoxProtocol>) -> Size {
        self.size = ctx.constraints.biggest();
        self.size
    }

    fn paint<T>(&self, ctx: &mut PaintContext<'_, T, Leaf>) {
        let rect = Rect::from_xywh(
            ctx.offset.dx,
            ctx.offset.dy,
            self.size.width,
            self.size.height,
        );
        ctx.canvas().rect(rect, &Paint::fill(self.color));
    }
}
```

**Flutter Equivalents:**
- RenderObjectWidget subclasses
- Most rendering primitives in `rendering` library

**FLUI Examples:**
- See `flui_rendering` crate for 82+ RenderBox implementations
- Widget wrappers delegate to these render objects

**File Organization:**
```
crates/flui_rendering/src/objects/
  (82+ RenderBox implementations)

crates/flui_widgets/src/
  basic/
    // Widget wrappers that create render objects
```

---

### 7. RenderSliver Widgets

**When to use:**
- Scrolling lists/grids
- Lazy loading content
- Sticky headers
- Infinite scroll

**Implementation:**
```rust
use flui_core::view::RenderView;
use flui_rendering::{SliverRender, Variable, LayoutContext, PaintContext};
use flui_types::{SliverConstraints, SliverGeometry};

pub struct SliverListView {
    pub item_count: usize,
}

impl RenderView for SliverListView {
    type RenderObject = RenderSliverList;

    fn create_render_object(&self) -> Self::RenderObject {
        RenderSliverList {
            item_count: self.item_count,
        }
    }
}

pub struct RenderSliverList {
    item_count: usize,
}

impl SliverRender<Variable> for RenderSliverList {
    fn layout<T>(&mut self, ctx: LayoutContext<'_, T, Variable, SliverProtocol>) -> SliverGeometry {
        // Sliver layout logic
    }

    fn paint<T>(&self, ctx: &mut PaintContext<'_, T, Variable>) {
        // Paint visible children
    }
}
```

**Flutter Equivalents:**
- SliverList, SliverGrid, SliverAppBar
- SliverPadding, SliverFillViewport
- CustomScrollView children

**FLUI Examples:**
- See `flui_rendering` crate for 26+ SliverRender implementations
- Widget wrappers for scrolling views

**File Organization:**
```
crates/flui_rendering/src/objects/sliver/
  (26+ SliverRender implementations)

crates/flui_widgets/src/
  scrolling/
    list_view.rs      // Widget wrapper
    grid_view.rs      // Widget wrapper
    custom_scroll_view.rs
```

---

## Widget Classification from Flutter

Based on `flutter_widgets_full.md`, here's how to classify each widget:

### Layout Widgets → Stateless

```rust
// Basic Layout
Container          // Stateless (composes Padding, Align, DecoratedBox)
SizedBox           // Stateless
Padding            // Stateless
Center             // Stateless
Align              // Stateless
FittedBox          // Stateless
AspectRatio        // Stateless
ConstrainedBox     // Stateless
LimitedBox         // Stateless
OverflowBox        // Stateless
SizedOverflowBox   // Stateless
FractionallySizedBox // Stateless
IntrinsicHeight    // Stateless
IntrinsicWidth     // Stateless
Baseline           // Stateless

// Multi-child Layout
Row                // Stateless (creates RenderFlex)
Column             // Stateless (creates RenderFlex)
Stack              // Stateless (creates RenderStack)
Wrap               // Stateless (creates RenderWrap)
Flow               // Stateless (creates RenderFlow)
Table              // Stateless (creates RenderTable)
```

### Scrolling Widgets → Mixed

```rust
// Viewport Containers
SingleChildScrollView // Stateful (scroll position)
CustomScrollView      // Stateful (scroll controller)
ScrollConfiguration   // Provider (scroll physics)

// List Views
ListView             // Stateful (scroll state)
ListView.builder     // Stateful (lazy loading)
GridView             // Stateful (scroll state)
GridView.builder     // Stateful (lazy loading)

// Sliver Widgets (RenderSliver)
SliverList           // RenderSliver
SliverGrid           // RenderSliver
SliverAppBar         // RenderSliver + Stateful (collapse state)
SliverPadding        // RenderSliver
SliverFillViewport   // RenderSliver
```

### Text Widgets → Stateless

```rust
Text                // Stateless
RichText            // Stateless
SelectableText      // Stateful (selection state)
DefaultTextStyle    // Provider
```

### Input Widgets → Stateful

```rust
TextField           // Stateful (text, cursor, selection)
TextFormField       // Stateful (form state)
Checkbox            // Stateful (checked state)
Radio               // Stateful (selected state)
Switch              // Stateful (on/off state)
Slider              // Stateful (value, dragging)
RangeSlider         // Stateful (start, end values)
DropdownButton      // Stateful (selected value, open/closed)
Autocomplete        // Stateful (suggestions, focus)
Form                // Stateful (form state, validation)
```

### Button Widgets → Stateless (with callbacks)

```rust
ElevatedButton      // Stateless
TextButton          // Stateless
OutlinedButton      // Stateless
IconButton          // Stateless
FloatingActionButton // Stateless
```

### Interaction Widgets → Mixed

```rust
GestureDetector     // Stateless (callbacks only)
InkWell             // Stateful (ripple animation)
InkResponse         // Stateful (ripple animation)
Draggable           // Stateful (drag state)
DragTarget          // Stateful (drag-over state)
LongPressDraggable  // Stateful (drag state)
Dismissible         // Stateful (swipe state)
```

### Animation Widgets → Animated

```rust
// Implicit Animations
AnimatedContainer   // Animated
AnimatedOpacity     // Animated
AnimatedPadding     // Animated
AnimatedPositioned  // Animated
AnimatedAlign       // Animated
AnimatedSize        // Animated
AnimatedRotation    // Animated
AnimatedScale       // Animated
AnimatedDefaultTextStyle // Animated

// Explicit Transitions
FadeTransition      // Animated
ScaleTransition     // Animated
SlideTransition     // Animated
RotationTransition  // Animated
SizeTransition      // Animated
PositionedTransition // Animated
AlignTransition     // Animated
DecoratedBoxTransition // Animated

// Animation Builders
TweenAnimationBuilder // Animated
AnimatedBuilder     // Animated
```

### Visual Effects → Stateless

```rust
Opacity             // Stateless (const opacity)
Transform           // Stateless (const transform)
DecoratedBox        // Stateless
ClipRect            // Stateless
ClipRRect           // Stateless
ClipOval            // Stateless
ClipPath            // Stateless
BackdropFilter      // Stateless
ShaderMask          // Stateless
ColorFiltered       // Stateless
```

### Material Widgets → Mixed

```rust
// Surfaces
Scaffold            // Stateful (drawer state, snackbar)
AppBar              // Stateless
Card                // Stateless
Drawer              // Stateful (open/closed)
BottomSheet         // Stateful (position)

// Navigation
BottomNavigationBar // Stateful (selected index)
NavigationRail      // Stateful (selected index)
TabBar              // Stateful (selected tab)
TabBarView          // Stateful (page controller)

// Dialogs
AlertDialog         // Stateless
SimpleDialog        // Stateless
Dialog              // Stateless
BottomSheet         // Stateful
```

### Theme/Provider Widgets → Provider

```rust
Theme               // Provider
MediaQuery          // Provider
Localizations       // Provider
DefaultTextStyle    // Provider
IconTheme           // Provider
InheritedWidget     // Provider (base class)
```

---

## Implementation Strategy

### Phase 1: Core Infrastructure (DONE)

✅ ViewMode enum in `flui-foundation`
✅ ViewObject trait in `flui-element`
✅ Wrapper implementations (Stateless, Stateful, etc.)

### Phase 2: Basic Stateless Widgets (IN PROGRESS)

Priority widgets (most commonly used):
1. Container, Padding, Center, Align, SizedBox
2. Row, Column, Stack
3. Text, Icon
4. Card, Divider

### Phase 3: Stateful Input Widgets

1. TextField (highest priority)
2. Checkbox, Radio, Switch
3. Slider, DropdownButton

### Phase 4: Animation Framework

1. Animation, AnimationController
2. FadeTransition, ScaleTransition
3. AnimatedContainer, AnimatedOpacity
4. TweenAnimationBuilder

### Phase 5: Provider System

1. Theme, ThemeData
2. MediaQuery, MediaQueryData
3. DefaultTextStyle
4. Localizations

### Phase 6: Material Design

1. Scaffold, AppBar
2. ElevatedButton, TextButton, OutlinedButton
3. BottomNavigationBar, TabBar
4. Drawer, BottomSheet

### Phase 7: Advanced Scrolling

1. ListView, GridView (stateful wrappers)
2. CustomScrollView
3. ScrollController, ScrollPhysics
4. RefreshIndicator

### Phase 8: Gestures & Interaction

1. GestureDetector (stateless)
2. InkWell, InkResponse (stateful with ripple)
3. Draggable, DragTarget
4. Dismissible

---

## File Organization

```
crates/flui_widgets/src/
├── basic/              # Stateless layout & containers
│   ├── container.rs
│   ├── padding.rs
│   ├── center.rs
│   ├── align.rs
│   ├── sized_box.rs
│   └── ...
├── layout/             # Stateless multi-child layouts
│   ├── row.rs
│   ├── column.rs
│   ├── stack.rs
│   ├── wrap.rs
│   └── ...
├── text/               # Stateless text widgets
│   ├── text.rs
│   ├── rich_text.rs
│   └── ...
├── input/              # Stateful input widgets
│   ├── text_field.rs
│   ├── checkbox.rs
│   ├── radio.rs
│   ├── slider.rs
│   └── ...
├── animation/          # Animated widgets
│   ├── fade_transition.rs
│   ├── scale_transition.rs
│   ├── animated_container.rs
│   └── ...
├── theme/              # Provider widgets
│   ├── theme.rs
│   ├── media_query.rs
│   └── ...
├── material/           # Material Design widgets (mixed)
│   ├── scaffold.rs     # Stateful
│   ├── app_bar.rs      # Stateless
│   ├── card.rs         # Stateless
│   ├── button.rs       # Stateless
│   └── ...
├── scrolling/          # Scrolling widgets (mixed)
│   ├── list_view.rs    # Stateful
│   ├── grid_view.rs    # Stateful
│   ├── scroll_view.rs  # Stateful
│   └── ...
├── gestures/           # Gesture widgets (stateless + stateful)
│   ├── detector.rs     # Stateless
│   ├── ink_well.rs     # Stateful
│   └── ...
└── lib.rs              # Re-exports
```

---

## Best Practices

### 1. Choose the Right ViewMode

| Need | Use |
|------|-----|
| Immutable config | Stateless |
| Mutable state | Stateful |
| Animation-driven | Animated |
| Inherited data | Provider |
| Builder pattern | Proxy |
| Custom layout/paint | RenderBox/RenderSliver |

### 2. Composition over Custom Rendering

Prefer composing existing widgets (Stateless) over creating RenderObjects:

```rust
// ✅ GOOD - Compose existing widgets
impl StatelessView for MyCard {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        Container::builder()
            .padding(EdgeInsets::all(16.0))
            .decoration(BoxDecoration::new()
                .border_radius(BorderRadius::circular(8.0))
                .box_shadow(vec![BoxShadow::default()]))
            .child(self.child)
            .build()
    }
}

// ❌ BAD - Custom RenderObject for simple case
// Only use when you need custom layout/paint logic
```

### 3. State Management

Keep state minimal and local:

```rust
// ✅ GOOD - Minimal state
#[derive(Debug)]
pub struct CheckboxState {
    pub checked: bool,
}

// ❌ BAD - Duplicate props in state
#[derive(Debug)]
pub struct CheckboxState {
    pub checked: bool,
    pub color: Color,     // Should be in widget, not state
    pub size: f32,        // Should be in widget, not state
}
```

### 4. Builder Patterns

Use `bon::Builder` for ergonomic APIs:

```rust
use bon::Builder;

#[derive(Debug, Clone, Builder)]
pub struct Button {
    pub label: String,
    #[builder(default)]
    pub color: Option<Color>,
    #[builder(default)]
    pub on_tap: Option<ButtonCallback>,
}

// Usage:
Button::builder()
    .label("Click me")
    .color(Color::BLUE)
    .on_tap(|| println!("Tapped!"))
    .build()
```

### 5. Type Safety

Leverage Rust's type system:

```rust
// ✅ GOOD - Type-safe alignment
pub enum Alignment {
    TopLeft,
    Center,
    BottomRight,
    // ...
}

// ❌ BAD - Stringly-typed
pub alignment: String, // "top-left", "center", etc.
```

---

## Testing Guidelines

Each widget should have:

1. **Unit tests** - Widget creation, builder patterns
2. **Build tests** - Verify build() output
3. **State tests** - State mutations (for Stateful)
4. **Integration tests** - Composition with other widgets

Example:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_button_creation() {
        let button = Button::new("Test");
        assert_eq!(button.label, "Test");
    }

    #[test]
    fn test_button_builder() {
        let button = Button::builder()
            .label("Test")
            .color(Color::BLUE)
            .build();
        assert_eq!(button.color, Color::BLUE);
    }

    #[test]
    fn test_checkbox_state() {
        let checkbox = Checkbox::new(false);
        let mut state = checkbox.create_state();
        assert!(!state.checked);

        state.toggle();
        assert!(state.checked);
    }
}
```

---

## Resources

- **Flutter Widget Catalog**: https://docs.flutter.dev/ui/widgets
- **FLUI RenderObjects**: `crates/flui_rendering/docs/RENDER_OBJECTS_CATALOG.md`
- **ViewMode Reference**: `crates/flui-foundation/src/view_mode.rs`
- **Widget Examples**: `crates/flui_widgets/src/`

---

## Summary

| ViewMode | Count (Flutter) | Priority | Status |
|----------|-----------------|----------|--------|
| Stateless | ~150 | HIGH | In Progress |
| Stateful | ~50 | HIGH | Planned |
| Animated | ~30 | MEDIUM | Planned |
| Provider | ~10 | MEDIUM | Planned |
| Proxy | ~5 | LOW | Planned |
| RenderBox | 56 | DONE | ✅ Complete |
| RenderSliver | 26 | DONE | ✅ Complete |

**Next Steps:**
1. Complete basic Stateless widgets (Container, Row, Column, Text)
2. Implement Stateful input widgets (TextField, Checkbox)
3. Build Animation framework (AnimationController, Transitions)
4. Add Provider system (Theme, MediaQuery)
5. Expand Material Design widgets (Scaffold, AppBar, Buttons)

---

**Last Updated:** 2025-11-27
**Status:** Living document - update as architecture evolves
