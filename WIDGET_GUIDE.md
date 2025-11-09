# FLUI Widget Development Guide

Complete guide for creating and improving widgets with the bon builder pattern in FLUI.

## Table of Contents

1. [Overview](#overview)
2. [Widget Patterns](#widget-patterns)
3. [bon Builder Integration](#bon-builder-integration)
4. [API Design Guidelines](#api-design-guidelines)
5. [Testing Guidelines](#testing-guidelines)
6. [Common Patterns](#common-patterns)
7. [Migration Checklist](#migration-checklist)

---

## Overview

FLUI uses the bon builder pattern for ergonomic, type-safe widget construction. This guide covers:

- How to structure widgets with bon
- Modern finish_fn pattern
- Custom builder methods
- Validation strategies
- Testing approaches

### Key Principles

1. **Ergonomic API**: Accept `impl View + 'static` instead of `Box<dyn AnyView>`
2. **Type Safety**: Use bon's type state pattern for required fields
3. **Validation**: Validate in debug mode, skip in release
4. **Deprecation**: Deprecate mutable APIs, guide users to builder pattern
5. **Consistency**: Follow established patterns across all widgets

---

## Widget Patterns

### Single-Child Widget

For widgets with exactly one child:

```rust
use bon::Builder;
use flui_core::view::{AnyView, IntoElement, SingleRenderBuilder, View};
use flui_core::BuildContext;

#[derive(Builder)]
#[builder(
    on(String, into),           // Auto-convert String fields
    finish_fn(name = build_internal, vis = "")  // Modern bon pattern
)]
pub struct MyWidget {
    /// Optional key for widget identification
    pub key: Option<String>,

    /// Widget-specific properties
    pub my_property: f32,

    /// The child widget
    #[builder(setters(vis = "", name = child_internal))]
    pub child: Option<Box<dyn AnyView>>,
}

impl MyWidget {
    /// Creates a new MyWidget.
    pub fn new(my_property: f32, child: impl View + 'static) -> Self {
        Self {
            key: None,
            my_property,
            child: Some(Box::new(child)),
        }
    }

    /// Validates widget configuration.
    pub fn validate(&self) -> Result<(), String> {
        if self.my_property < 0.0 {
            return Err("my_property must be non-negative".to_string());
        }
        Ok(())
    }
}

// bon Builder Extensions
use my_widget_builder::{IsUnset, SetChild, State};

impl<S: State> MyWidgetBuilder<S>
where
    S::Child: IsUnset,
{
    /// Sets the child widget (works in builder chain).
    pub fn child(self, child: impl View + 'static) -> MyWidgetBuilder<SetChild<S>> {
        self.child_internal(Box::new(child))
    }
}

impl<S: State> MyWidgetBuilder<S> {
    /// Builds the widget with optional validation.
    pub fn build(self) -> MyWidget {
        let widget = self.build_internal();

        #[cfg(debug_assertions)]
        {
            if let Err(e) = widget.validate() {
                tracing::warn!("MyWidget validation failed: {}", e);
            }
        }

        widget
    }
}

impl View for MyWidget {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        let render = RenderMyWidget::new(self.my_property);
        SingleRenderBuilder::new(render).with_optional_child(self.child)
    }
}
```

### Multi-Child Widget

For widgets with multiple children:

```rust
#[derive(Builder)]
#[builder(
    on(String, into),
    finish_fn(name = build_internal, vis = "")
)]
pub struct MyMultiWidget {
    /// The child widgets
    ///
    /// IMPORTANT: Fields with #[builder(field)] MUST be first!
    #[builder(field)]
    pub children: Vec<Box<dyn AnyView>>,

    pub key: Option<String>,
    pub my_property: f32,
}

// bon Builder Extensions - Custom methods for children
impl<S: my_multi_widget_builder::State> MyMultiWidgetBuilder<S> {
    /// Sets all children at once.
    pub fn children(mut self, children: Vec<Box<dyn AnyView>>) -> Self {
        self.children = children;
        self
    }

    /// Adds a single child widget (chainable).
    pub fn child(mut self, child: impl AnyView + 'static) -> Self {
        self.children.push(Box::new(child));
        self
    }

    /// Builds the widget with optional validation.
    pub fn build(self) -> MyMultiWidget {
        let widget = self.build_internal();

        #[cfg(debug_assertions)]
        {
            if let Err(e) = widget.validate() {
                tracing::warn!("MyMultiWidget validation failed: {}", e);
            }
        }

        widget
    }
}

impl View for MyMultiWidget {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        let render = RenderMyMultiWidget::new(self.my_property);
        MultiRenderBuilder::new(render).with_children(self.children)
    }
}
```

### No-Child Widget (Leaf)

For widgets without children:

```rust
#[derive(Builder, Debug, Clone)]
#[builder(
    on(i32, into),
    finish_fn(name = build_internal, vis = "")
)]
pub struct MyLeafWidget {
    #[builder(default = 1)]
    pub my_property: i32,
}

impl MyLeafWidget {
    pub fn new() -> Self {
        Self { my_property: 1 }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.my_property <= 0 {
            return Err("my_property must be positive".to_string());
        }
        Ok(())
    }
}

// bon Builder Extensions
use my_leaf_widget_builder::State;

impl<S: State> MyLeafWidgetBuilder<S> {
    pub fn build(self) -> MyLeafWidget {
        let widget = self.build_internal();

        #[cfg(debug_assertions)]
        {
            if let Err(e) = widget.validate() {
                tracing::warn!("MyLeafWidget validation failed: {}", e);
            }
        }

        widget
    }
}

impl View for MyLeafWidget {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        LeafRenderBuilder::new(RenderMyLeafWidget::new(self.my_property))
    }
}
```

---

## bon Builder Integration

### Modern finish_fn Pattern

**✅ Correct (Modern):**
```rust
#[builder(
    finish_fn(name = build_internal, vis = "")
)]
```

**❌ Incorrect (Old):**
```rust
#[builder(finish_fn = build_my_widget)]
```

### The `#[builder(field)]` Attribute

For multi-child widgets, use `#[builder(field)]` to expose the field for custom methods:

**Key Rules:**
1. Fields with `#[builder(field)]` MUST come FIRST in struct definition
2. bon does NOT generate setter methods for `field` attributes
3. Must manually implement both `.children(vec)` and `.child(item)` methods
4. Need `S: {struct_name}_builder::State` trait bound

```rust
#[derive(Builder)]
#[builder(finish_fn(name = build_internal, vis = ""))]
pub struct Widget {
    // ✅ CORRECT: field attribute first
    #[builder(field)]
    pub children: Vec<Box<dyn AnyView>>,

    // Other fields after
    pub key: Option<String>,
    pub property: f32,
}

// ❌ WRONG: field attribute not first
pub struct Widget {
    pub key: Option<String>,
    #[builder(field)]  // ERROR: Must be first!
    pub children: Vec<Box<dyn AnyView>>,
}
```

### Custom Child Setters

For single-child widgets with type state:

```rust
// Import bon's type state types
use widget_builder::{IsUnset, SetChild, State};

impl<S: State> WidgetBuilder<S>
where
    S::Child: IsUnset,  // Only available when child not set
{
    pub fn child(self, child: impl View + 'static) -> WidgetBuilder<SetChild<S>> {
        self.child_internal(Box::new(child))
    }
}
```

### Optional vs Required Children

**Optional child (most widgets):**
```rust
pub struct Widget {
    #[builder(setters(vis = "", name = child_internal))]
    pub child: Option<Box<dyn AnyView>>,
}

// Type state: IsUnset -> SetChild
impl<S: State> WidgetBuilder<S>
where
    S::Child: IsUnset,
{
    pub fn child(self, child: impl View + 'static) -> WidgetBuilder<SetChild<S>> {
        self.child_internal(Box::new(child))
    }
}
```

**Required child:**
```rust
pub struct Widget {
    #[builder(setters(vis = "", name = child_internal))]
    pub child: Box<dyn AnyView>,  // Not Option!
}

// build() requires child to be set
impl<S: State> WidgetBuilder<S>
where
    S::Child: IsSet,  // Child MUST be set
{
    pub fn build(self) -> Widget {
        self.build_internal()
    }
}
```

---

## API Design Guidelines

### 1. Ergonomic Constructors

Accept `impl View + 'static` instead of `Box<dyn AnyView>`:

**✅ Good:**
```rust
pub fn new(child: impl View + 'static) -> Self {
    Self {
        child: Some(Box::new(child)),
    }
}
```

**❌ Bad:**
```rust
pub fn new(child: Box<dyn AnyView>) -> Self {
    Self {
        child: Some(child),
    }
}
```

### 2. Deprecate Mutable APIs

Mark mutable methods as deprecated:

```rust
/// Sets the child widget.
#[deprecated(note = "Use builder pattern with .child() instead")]
pub fn set_child(&mut self, child: Box<dyn AnyView>) {
    self.child = Some(child);
}
```

### 3. Validation Strategy

- **Debug mode**: Validate and log warnings
- **Release mode**: Skip validation for performance

```rust
impl<S: State> WidgetBuilder<S> {
    pub fn build(self) -> Widget {
        let widget = self.build_internal();

        #[cfg(debug_assertions)]
        {
            if let Err(e) = widget.validate() {
                tracing::warn!("Widget validation failed: {}", e);
            }
        }

        widget
    }
}

impl Widget {
    pub fn validate(&self) -> Result<(), String> {
        if self.width < 0.0 {
            return Err("width must be non-negative".to_string());
        }
        Ok(())
    }
}
```

### 4. Convenience Methods

Provide specialized constructors for common use cases:

```rust
impl Row {
    /// Creates a Row with centered alignment.
    pub fn centered(children: Vec<Box<dyn AnyView>>) -> Self {
        Self::builder()
            .main_axis_alignment(MainAxisAlignment::Center)
            .cross_axis_alignment(CrossAxisAlignment::Center)
            .children(children)
            .build()
    }

    /// Creates a Row with spacing between children.
    pub fn spaced(spacing: f32, children: Vec<Box<dyn AnyView>>) -> Self {
        // Implementation with SizedBox spacers
    }
}
```

---

## Testing Guidelines

### Update Builder Tests

Change old `build_xxx()` to modern `build()`:

**✅ Correct:**
```rust
#[test]
fn test_widget_builder() {
    let widget = Widget::builder()
        .property(42.0)
        .child(SizedBox::new())
        .build();
    assert_eq!(widget.property, 42.0);
}
```

**❌ Incorrect:**
```rust
#[test]
fn test_widget_builder() {
    let widget = Widget::builder()
        .property(42.0)
        .child(SizedBox::new())
        .build_widget();  // Old pattern!
}
```

### Update API Tests

Remove `Box::new()` wrappers:

**✅ Correct:**
```rust
#[test]
fn test_widget_new() {
    let widget = Widget::new(42.0, SizedBox::new());
    assert!(widget.child.is_some());
}
```

**❌ Incorrect:**
```rust
#[test]
fn test_widget_new() {
    let widget = Widget::new(42.0, Box::new(SizedBox::new()));
    assert!(widget.child.is_some());
}
```

### Test Deprecated Methods

Use `#[allow(deprecated)]` for deprecated API tests:

```rust
#[test]
#[allow(deprecated)]
fn test_set_child() {
    let mut widget = Widget::new();
    widget.set_child(Box::new(SizedBox::new()));
    assert!(widget.child.is_some());
}
```

### Validation Tests

Test both valid and invalid configurations:

```rust
#[test]
fn test_validate_ok() {
    let widget = Widget::new(42.0, SizedBox::new());
    assert!(widget.validate().is_ok());
}

#[test]
fn test_validate_invalid() {
    let mut widget = Widget::new(42.0, SizedBox::new());
    widget.property = -1.0;
    assert!(widget.validate().is_err());
}
```

---

## Common Patterns

### Pattern 1: Alignment Property

For widgets with alignment:

```rust
#[derive(Builder)]
#[builder(
    on(Alignment, into),  // Auto-convert Alignment
    finish_fn(name = build_internal, vis = "")
)]
pub struct Widget {
    #[builder(default = Alignment::CENTER)]
    pub alignment: Alignment,
    // ...
}
```

### Pattern 2: Constraints

For widgets with size constraints:

```rust
pub struct Widget {
    pub min_width: Option<f32>,
    pub max_width: Option<f32>,
    pub min_height: Option<f32>,
    pub max_height: Option<f32>,
}

impl Widget {
    pub fn validate(&self) -> Result<(), String> {
        if let (Some(min), Some(max)) = (self.min_width, self.max_width) {
            if min > max {
                return Err("min_width cannot be greater than max_width".to_string());
            }
        }
        // Similar for height...
        Ok(())
    }
}
```

### Pattern 3: Flex Children

For widgets in flex layouts (Row/Column/Flex):

```rust
#[derive(Builder)]
pub struct FlexWidget {
    #[builder(default = 1)]
    pub flex: i32,

    #[builder(setters(vis = "", name = child_internal))]
    pub child: Box<dyn AnyView>,
}

impl View for FlexWidget {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        SingleRenderBuilder::new(
            RenderFlexItem::new(FlexItemMetadata {
                flex: self.flex,
                fit: FlexFit::Tight,
            })
        )
        .with_child(self.child)
    }
}
```

### Pattern 4: Directional Properties

For RTL/LTR aware widgets:

```rust
pub struct DirectionalWidget {
    pub start: Option<f32>,  // Left in LTR, right in RTL
    pub end: Option<f32>,    // Right in LTR, left in RTL
    pub text_direction: Option<TextDirection>,
}

impl DirectionalWidget {
    fn resolve_to_absolute(&self, text_direction: TextDirection) -> (Option<f32>, Option<f32>) {
        match text_direction {
            TextDirection::Ltr => (self.start, self.end),
            TextDirection::Rtl => (self.end, self.start),
        }
    }
}
```

---

## Migration Checklist

Use this checklist when upgrading a widget to the modern bon pattern:

### ✅ Struct Definition

- [ ] Update `finish_fn` to modern pattern: `finish_fn(name = build_internal, vis = "")`
- [ ] Add `on(Type, into)` for convertible types (String, Alignment, etc.)
- [ ] For multi-child widgets: Move `children` field to FIRST position with `#[builder(field)]`
- [ ] For single-child widgets: Use `#[builder(setters(vis = "", name = child_internal))]`

### ✅ Constructor Methods

- [ ] Update `new()` to accept `impl View + 'static` instead of `Box<dyn AnyView>`
- [ ] Update other constructors (`with_*`, convenience methods) similarly
- [ ] Deprecate mutable methods (`set_child()`, `add_child()`, etc.)

### ✅ Builder Extensions

- [ ] Import bon's type state types: `{widget}_builder::{IsUnset, SetChild, State}`
- [ ] Implement custom `.child()` method with proper trait bounds
- [ ] For multi-child: Implement both `.children(vec)` and `.child(item)` methods
- [ ] Add custom `.build()` method with validation

### ✅ Validation

- [ ] Implement `validate()` method if needed
- [ ] Call validation in `build()` with `#[cfg(debug_assertions)]`
- [ ] Use `tracing::warn!()` for validation failures

### ✅ Tests

- [ ] Update builder tests: `build_xxx()` → `build()`
- [ ] Update API tests: Remove `Box::new()` wrappers
- [ ] Add `#[allow(deprecated)]` to tests for deprecated methods
- [ ] Verify all tests pass: `cargo test -p flui_widgets`

### ✅ Documentation

- [ ] Update doc examples to show modern API
- [ ] Update builder pattern examples
- [ ] Document validation behavior
- [ ] Add examples of common use cases

### ✅ Final Verification

- [ ] Build succeeds: `cargo build -p flui_widgets`
- [ ] Tests pass: `cargo test -p flui_widgets`
- [ ] Update SUMMARY.md with widget completion
- [ ] No clippy warnings for the widget

---

## Examples from Real Widgets

### Example 1: RotatedBox (Single-Child, Simple)

```rust
#[derive(Builder)]
#[builder(
    on(String, into),
    on(QuarterTurns, into),
    finish_fn(name = build_internal, vis = "")
)]
pub struct RotatedBox {
    pub key: Option<String>,

    #[builder(default = QuarterTurns::Zero)]
    pub quarter_turns: QuarterTurns,

    #[builder(setters(vis = "", name = child_internal))]
    pub child: Option<Box<dyn AnyView>>,
}

impl RotatedBox {
    pub fn new(quarter_turns: QuarterTurns, child: impl View + 'static) -> Self {
        Self {
            key: None,
            quarter_turns,
            child: Some(Box::new(child)),
        }
    }

    pub fn rotate_90(child: impl View + 'static) -> Self {
        Self::new(QuarterTurns::One, child)
    }
}

// Builder extensions...
```

### Example 2: Stack (Multi-Child)

```rust
#[derive(Builder)]
#[builder(
    on(String, into),
    on(Alignment, into),
    finish_fn(name = build_internal, vis = "")
)]
pub struct Stack {
    #[builder(field)]  // FIRST!
    pub children: Vec<Box<dyn AnyView>>,

    pub key: Option<String>,

    #[builder(default = Alignment::TOP_LEFT)]
    pub alignment: Alignment,

    #[builder(default = StackFit::Loose)]
    pub fit: StackFit,
}

impl<S: stack_builder::State> StackBuilder<S> {
    pub fn children(mut self, children: Vec<Box<dyn AnyView>>) -> Self {
        self.children = children;
        self
    }

    pub fn child(mut self, child: impl AnyView + 'static) -> Self {
        self.children.push(Box::new(child));
        self
    }

    pub fn build(self) -> Stack {
        // Validation...
    }
}
```

### Example 3: Spacer (Leaf, No Child)

```rust
#[derive(Builder, Debug, Clone)]
#[builder(
    on(i32, into),
    finish_fn(name = build_internal, vis = "")
)]
pub struct Spacer {
    #[builder(default = 1)]
    pub flex: i32,
}

impl Spacer {
    pub fn new() -> Self {
        Self { flex: 1 }
    }

    pub fn with_flex(flex: i32) -> Self {
        Self { flex }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.flex <= 0 {
            return Err("Spacer requires flex > 0".to_string());
        }
        Ok(())
    }
}

impl<S: State> SpacerBuilder<S> {
    pub fn build(self) -> Spacer {
        let spacer = self.build_internal();

        #[cfg(debug_assertions)]
        {
            if let Err(e) = spacer.validate() {
                tracing::warn!("Spacer validation failed: {}", e);
            }
        }

        spacer
    }
}
```

---

## Troubleshooting

### Common Errors

**Error: "field attribute must be first"**
```
Move the field with #[builder(field)] to the first position in the struct.
```

**Error: "child_internal not found"**
```
Make sure you have #[builder(setters(vis = "", name = child_internal))] on the child field.
```

**Error: "IsUnset not found"**
```
Import the type state types: use {widget}_builder::{IsUnset, SetChild, State};
```

**Error: "build method conflicts"**
```
Make sure finish_fn uses build_internal, not build: finish_fn(name = build_internal, vis = "")
```

### Performance Notes

- Validation only runs in debug mode (`#[cfg(debug_assertions)]`)
- `Box::new()` wrapping is unavoidable for `dyn AnyView` but happens at construction, not in hot paths
- bon generates zero-cost abstractions - no runtime overhead
- Type state checking is compile-time only

---

## Summary

### Key Takeaways

1. **Modern bon pattern**: `finish_fn(name = build_internal, vis = "")`
2. **Ergonomic API**: Accept `impl View + 'static`
3. **Type safety**: Use bon's type state for required fields
4. **Multi-child**: Use `#[builder(field)]` on children (must be first!)
5. **Validation**: Debug-only with `#[cfg(debug_assertions)]`
6. **Deprecation**: Guide users to builder pattern
7. **Testing**: Update to modern `build()`, remove `Box::new()`

### Resources

- bon documentation: https://bon-rs.com/
- FLUI examples: `crates/flui_widgets/src/layout/`
- Session summary: `SUMMARY.md`
- Architecture: `docs/FINAL_ARCHITECTURE_V2.md`

---

**Last updated**: Session 2 - Widget Improvements (11 widgets completed)
