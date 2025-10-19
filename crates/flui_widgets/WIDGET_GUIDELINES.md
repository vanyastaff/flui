# Widget Implementation Guidelines

This document describes the standard patterns and best practices for implementing widgets in `flui_widgets`.

## Architecture Overview

Flui follows Flutter's three-tree architecture:
- **Widget Tree**: Immutable configuration objects (this crate)
- **Element Tree**: Mutable widget lifecycle managers (`flui_core`)
- **RenderObject Tree**: Layout and painting primitives (`flui_rendering`)

## Standard Widget Structure

Every widget in `flui_widgets` should follow this template:

### 1. File Structure

```rust
//! Widget name - brief description
//!
//! Detailed explanation of what the widget does and how it works.
//!
//! # Usage Patterns
//!
//! Widget supports three creation styles:
//!
//! ## 1. Struct Literal (Flutter-like)
//! ```rust,ignore
//! WidgetName {
//!     property: Some(value),
//!     ..Default::default()
//! }
//! ```
//!
//! ## 2. Builder Pattern (Type-safe with bon)
//! ```rust,ignore
//! WidgetName::builder()
//!     .property(value)
//!     .build()
//! ```
//!
//! ## 3. Macro (Declarative)
//! ```rust,ignore
//! widget_name! {
//!     property: value,
//! }
//! ```

use bon::Builder;
use flui_core::{Widget, BoxConstraints};
use flui_types::*;

// Widget struct definition...
```

### 2. Widget Struct with bon Builder

```rust
#[derive(Debug, Clone, Builder)]
#[builder(
    // Type conversions - enable .into() for common types
    on(String, into),
    on(EdgeInsets, into),
    on(BoxDecoration, into),
    on(Color, into),

    // Custom finish function (private internal build)
    finish_fn = build_widget_name
)]
pub struct WidgetName {
    /// Optional key for widget identification
    pub key: Option<String>,

    /// Main properties with documentation
    pub property1: Option<Type1>,

    /// Properties with defaults
    #[builder(default = DefaultValue)]
    pub property2: Type2,

    /// Child widget (if applicable)
    /// Use custom setter via private child_internal
    #[builder(setters(vis = "", name = child_internal))]
    pub child: Option<Box<dyn Widget>>,
}
```

### 3. Implementation Block

```rust
impl WidgetName {
    /// Creates a new widget with default values.
    pub fn new() -> Self {
        Self {
            key: None,
            property1: None,
            property2: DefaultValue,
            child: None,
        }
    }

    /// Sets the child widget (for struct literal usage).
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let mut widget = WidgetName::new();
    /// widget.set_child(some_widget);
    /// ```
    pub fn set_child(&mut self, child: impl Widget + 'static) {
        self.child = Some(Box::new(child));
    }

    /// Validates widget configuration.
    ///
    /// Returns Ok(()) if valid, or an error message describing the issue.
    pub fn validate(&self) -> Result<(), String> {
        // Check for invalid configurations
        // Return Err with descriptive message if invalid
        Ok(())
    }
}

impl Default for WidgetName {
    fn default() -> Self {
        Self::new()
    }
}
```

### 4. Widget Trait Implementation

```rust
impl Widget for WidgetName {
    fn create_element(&self) -> Box<dyn flui_core::Element> {
        // For RenderObjectWidget, create RenderObjectElement
        Box::new(flui_core::RenderObjectElement::new(self.clone()))
    }
}
```

### 5. RenderObjectWidget Implementation

For widgets that directly manage RenderObjects (most basic widgets):

```rust
use flui_core::{RenderObject, RenderObjectWidget};
use flui_rendering::RenderYourObject;

impl RenderObjectWidget for WidgetName {
    fn create_render_object(&self) -> Box<dyn RenderObject> {
        // Create the appropriate RenderObject for this widget
        Box::new(RenderYourObject::new(self.property1, self.property2))
    }

    fn update_render_object(&self, render_object: &mut dyn RenderObject) {
        // Update RenderObject when widget configuration changes
        if let Some(render) = render_object.downcast_mut::<RenderYourObject>() {
            render.set_property1(self.property1);
            render.set_property2(self.property2);
        }
    }
}
```

**For Multi-Child Widgets (Row, Column, Stack):**

```rust
use flui_core::MultiChildRenderObjectWidget;

impl MultiChildRenderObjectWidget for WidgetName {
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
}
```

**Widget Type Decision Tree:**

**When to use RenderObjectWidget:**
- Your widget directly controls **layout** (positioning, sizing)
- Your widget directly controls **painting** (drawing, rendering)
- Examples: Padding, Center, Align, SizedBox, Row, Column

**When to use StatelessWidget:**
- Your widget **composes** other widgets into a tree
- Your widget provides **convenience API** (combines multiple simpler widgets)
- Examples: Container (= Padding + Align + DecoratedBox + ConstrainedBox)

**Decision:**
- **Single child + own layout/paint** → Implement `RenderObjectWidget`
- **Multiple children + own layout/paint** → Implement `RenderObjectWidget` + `MultiChildRenderObjectWidget`
- **Composes other widgets** → Implement as `StatelessWidget` (future - currently use ComponentElement)

### 6. bon Builder Extensions

```rust
// Import bon builder traits for custom setters
use widget_name_builder::{State, IsUnset, SetChild};

// Custom setter for child (if widget has children)
impl<S: State> WidgetNameBuilder<S>
where
    S::Child: IsUnset,
{
    /// Sets the child widget (works in builder chain).
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// WidgetName::builder()
    ///     .property(value)
    ///     .child(some_widget)
    ///     .build()
    /// ```
    pub fn child(self, child: impl Widget + 'static) -> WidgetNameBuilder<SetChild<S>> {
        // bon wraps Box in Option internally
        self.child_internal(Box::new(child) as Box<dyn Widget>)
    }
}

// Public build() wrapper for convenience
impl<S: State> WidgetNameBuilder<S> {
    /// Convenience method to build the widget.
    ///
    /// Equivalent to calling the generated `build_widget_name()` finishing function.
    pub fn build(self) -> WidgetName {
        self.build_widget_name()
    }
}
```

### 7. Declarative Macro

```rust
/// Macro for creating WidgetName with declarative syntax.
///
/// # Examples
///
/// ```rust,ignore
/// widget_name! {
///     property1: value1,
///     property2: value2,
/// }
/// ```
#[macro_export]
macro_rules! widget_name {
    // Empty widget
    () => {
        $crate::WidgetName::new()
    };

    // Widget with fields
    ($($field:ident : $value:expr),* $(,)?) => {
        $crate::WidgetName {
            $($field: Some($value.into()),)*
            ..Default::default()
        }
    };
}
```

### 8. Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_widget_new() {
        let widget = WidgetName::new();
        assert!(widget.key.is_none());
        // Test default state
    }

    #[test]
    fn test_widget_default() {
        let widget = WidgetName::default();
        // Test Default implementation
    }

    #[test]
    fn test_widget_struct_literal() {
        let widget = WidgetName {
            property1: Some(value),
            ..Default::default()
        };
        assert_eq!(widget.property1, Some(value));
    }

    #[test]
    fn test_widget_builder() {
        let widget = WidgetName::builder()
            .property1(value)
            .build();
        assert_eq!(widget.property1, Some(value));
    }

    #[test]
    fn test_widget_builder_chaining() {
        let widget = WidgetName::builder()
            .property1(value1)
            .property2(value2)
            .build();
        // Test multiple properties
    }

    #[test]
    fn test_widget_macro_empty() {
        let widget = widget_name!();
        assert!(widget.property1.is_none());
    }

    #[test]
    fn test_widget_macro_with_fields() {
        let widget = widget_name! {
            property1: value,
        };
        assert_eq!(widget.property1, Some(value));
    }

    #[test]
    fn test_widget_validate_ok() {
        let widget = WidgetName::builder()
            .property1(valid_value)
            .build();
        assert!(widget.validate().is_ok());
    }

    #[test]
    fn test_widget_validate_invalid() {
        let widget = WidgetName {
            property1: Some(invalid_value),
            ..Default::default()
        };
        assert!(widget.validate().is_err());
    }
}
```

## bon Builder Configuration

### Common Attributes

1. **Type conversions**: Enable automatic `.into()` for ergonomic API
   ```rust
   #[builder(on(Type, into))]
   ```

2. **Default values**: Specify defaults for non-Option fields
   ```rust
   #[builder(default = value)]
   ```

3. **Private setters**: Hide internal setters (e.g., for child)
   ```rust
   #[builder(setters(vis = "", name = internal_name))]
   ```

4. **Custom finish function**: Rename build function for internal use
   ```rust
   #[builder(finish_fn = build_internal)]
   ```

### Custom Setters Pattern

For child widgets, always use this pattern:

```rust
// In struct definition
#[builder(setters(vis = "", name = child_internal))]
pub child: Option<Box<dyn Widget>>,

// Custom public setter
impl<S: State> WidgetBuilder<S>
where
    S::Child: IsUnset,
{
    pub fn child(self, child: impl Widget + 'static) -> WidgetBuilder<SetChild<S>> {
        self.child_internal(Box::new(child) as Box<dyn Widget>)
    }
}
```

## Macro Guidelines

### Naming Convention
- Macro name: `snake_case` version of struct name
- Example: `Container` → `container!`

### Macro Structure
```rust
#[macro_export]
macro_rules! widget_name {
    () => { $crate::WidgetName::new() };
    ($($field:ident : $value:expr),* $(,)?) => {
        $crate::WidgetName {
            $($field: Some($value.into()),)*
            ..Default::default()
        }
    };
}
```

### Important Notes
- Always use `$crate::` prefix for widget reference
- Support trailing commas with `$(,)?`
- Use `.into()` for automatic type conversions
- Wrap values in `Some()` for Option fields

## Testing Requirements

Every widget must have:

1. **Creation tests**: `new()`, `default()`, struct literal
2. **Builder tests**: Basic and chaining
3. **Macro tests**: Empty and with fields
4. **Validation tests**: Valid and invalid configurations
5. **Property tests**: Each property setter and getter
6. **Edge cases**: Boundary conditions, conflicts

Minimum 10-15 tests per widget.

## Documentation Requirements

### Module-level docs
- Brief description
- Usage examples for all 3 syntax styles
- Flutter equivalent (if applicable)
- Common use cases

### Struct docs
- What the widget does
- Layout/sizing behavior
- Relationship to child widgets
- Important constraints or limitations

### Field docs
- Purpose of each field
- Default value if applicable
- How it affects layout/rendering
- Valid value ranges

### Method docs
- What the method does
- Parameters and return values
- Usage examples
- Related methods

## Complete Examples

See the following reference implementations:
- **RenderObjectWidget**: [Center](./src/basic/center.rs) - Simple single-child widget using RenderPositionedBox
- **MultiChildRenderObjectWidget**: [Row](./src/layout/row.rs) - Multi-child flex layout using RenderFlex
- **Composite Widget**: [Container](./src/basic/container.rs) - Complex widget composing multiple RenderObjects

## Checklist for New Widgets

- [ ] Struct with `#[derive(Debug, Clone, Builder)]`
- [ ] bon builder configuration (conversions, finish_fn)
- [ ] `new()` and `Default` implementations
- [ ] `validate()` method if applicable
- [ ] `Widget` trait implementation (create RenderObjectElement)
- [ ] `RenderObjectWidget` trait implementation (create_render_object, update_render_object)
- [ ] `MultiChildRenderObjectWidget` if applicable (children getter)
- [ ] Custom builder extensions (child setter, build wrapper)
- [ ] Declarative macro
- [ ] Comprehensive tests (10-15 minimum)
- [ ] Full documentation with examples
- [ ] Export from module `mod.rs` and crate `lib.rs`
- [ ] Add to README if it's a major widget

## Common Patterns

### Optional vs Required Fields
- Use `Option<T>` for truly optional properties
- Use `#[builder(default)]` for properties with sensible defaults
- Required fields should use builder pattern without Option

### Child Widget Handling
- Single child: `child: Option<Box<dyn Widget>>`
- Multiple children: `children: Vec<Box<dyn Widget>>`
- Use private `child_internal` setter with custom public `child()`

### Type Conversions
Common types that benefit from `.into()`:
- `String` (from `&str`)
- `EdgeInsets` (from `f32` via `all()`)
- `Color` (from tuples, hex strings)
- `BoxDecoration` (from `Color`)

### Validation
Always validate:
- Numeric ranges (negative, NaN, infinite)
- Conflicting properties
- Required dependencies
- Logical constraints

Return descriptive error messages:
```rust
Err(format!("Invalid width: {}. Width must be positive and finite.", width))
```

## References

- [bon builder documentation](https://docs.rs/bon/latest/bon/)
- [Flutter Widget catalog](https://api.flutter.dev/flutter/widgets/widgets-library.html)
- [`flui_core::Widget` trait](../flui_core/src/widget.rs)
- [`flui_core::RenderObjectWidget` trait](../flui_core/src/widget.rs)
- [`flui_rendering` RenderObjects](../flui_rendering/src/)
