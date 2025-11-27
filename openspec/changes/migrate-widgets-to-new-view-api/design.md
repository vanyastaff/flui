# Design: Widget Migration Architecture

## Overview

This document describes the architectural approach for migrating 80+ widgets to the new View API while maintaining backward compatibility and enabling three usage patterns: bon builder, struct literal, and macros.

## Architecture Layers

```
┌─────────────────────────────────────────────────────┐
│  User Code (Application)                             │
│  - Uses widgets via builder, struct, or macro       │
└──────────────────┬──────────────────────────────────┘
                   │
                   ▼
┌─────────────────────────────────────────────────────┐
│  Widget Layer (flui_widgets)                         │
│  - Implements StatelessView/StatefulView            │
│  - Provides three usage patterns                    │
│  - Composes RenderObjects via adapter methods       │
└──────────────────┬──────────────────────────────────┘
                   │
                   ▼
┌─────────────────────────────────────────────────────┐
│  Adapter Layer (flui_core::render::RenderBoxExt)    │
│  - .leaf() → Element (Leaf arity)                   │
│  - .child(e) → Element (Single arity)               │
│  - .children(vec) → Element (Variable arity)        │
│  - .maybe_child(opt) → Element (Optional arity)     │
└──────────────────┬──────────────────────────────────┘
                   │
                   ▼
┌─────────────────────────────────────────────────────┐
│  RenderView Wrapper (flui_rendering::view)          │
│  - RenderView::new(render_object)                   │
│  - Handles Element creation                         │
│  - Manages child attachment                         │
└──────────────────┬──────────────────────────────────┘
                   │
                   ▼
┌─────────────────────────────────────────────────────┐
│  RenderObject Layer (flui_rendering::objects)       │
│  - RenderBox<A> trait implementations               │
│  - layout() and paint() methods                     │
│  - Type-safe arity system (Leaf, Single, etc.)     │
└─────────────────────────────────────────────────────┘
```

## Three Usage Patterns

### Pattern 1: Bon Builder (Recommended)

**Advantages:**
- Type-safe at compile time
- IDE auto-completion
- Optional fields handled automatically
- Method chaining

**Example:**
```rust
use flui_widgets::prelude::*;

let widget = Container::builder()
    .padding(EdgeInsets::all(16.0))
    .color(Color::BLUE)
    .width(200.0)
    .height(100.0)
    .child(Text::new("Hello"))
    .build();
```

**Implementation:**
```rust
use bon::Builder;

#[derive(Builder)]
#[builder(
    on(String, into),
    on(EdgeInsets, into),
    on(Color, into),
    finish_fn(name = build_internal, vis = "")
)]
pub struct Container {
    #[builder(default)]
    pub padding: Option<EdgeInsets>,

    #[builder(default)]
    pub color: Option<Color>,

    #[builder(default, setters(vis = "", name = child_internal))]
    pub child: Child,

    // ... other fields
}

impl StatelessView for Container {
    fn build(self, _ctx: &dyn BuildContext) -> impl IntoElement {
        // Compose using adapter methods
        let mut current = self.child.into_element();

        if let Some(padding) = self.padding {
            current = RenderPadding::new(padding).child(current);
        }

        if let Some(color) = self.color {
            let decoration = BoxDecoration { color: Some(color), ..Default::default() };
            current = RenderDecoratedBox::new(decoration).maybe_child(Some(current));
        }

        current
    }
}
```

### Pattern 2: Struct Literal (Flutter-like)

**Advantages:**
- Familiar to Flutter developers
- Explicit field initialization
- Good for simple configurations

**Example:**
```rust
let widget = Container {
    padding: Some(EdgeInsets::all(16.0)),
    color: Some(Color::BLUE),
    width: Some(200.0),
    height: Some(100.0),
    child: Child::new(Text::new("Hello")),
    ..Default::default()
};
```

**Implementation:**
```rust
impl Default for Container {
    fn default() -> Self {
        Self {
            padding: None,
            color: None,
            width: None,
            height: None,
            child: Child::none(),
            // ... other fields
        }
    }
}
```

### Pattern 3: Declarative Macros (Ergonomic)

**Advantages:**
- Concise syntax
- Good for nested widget trees
- Reduces boilerplate

**Example:**
```rust
let widget = container! {
    padding: EdgeInsets::all(16.0),
    color: Color::BLUE,
    child: text!("Hello")
};

let layout = column![
    text!("Title"),
    row![
        button!("Cancel"),
        button!("OK"),
    ],
];
```

**Implementation:**
```rust
#[macro_export]
macro_rules! container {
    // With child
    (child: $child:expr, $($field:ident : $value:expr),* $(,)?) => {
        $crate::Container::builder()
            .child($child)
            $(.$field($value))*
            .build()
    };

    // Without child
    ($($field:ident : $value:expr),* $(,)?) => {
        $crate::Container {
            $(
                $field: Some($value.into()),
            )*
            ..Default::default()
        }
    };
}

#[macro_export]
macro_rules! column {
    [$($child:expr),* $(,)?] => {
        $crate::Column::builder()
            .children(vec![$($child.into_element()),*])
            .build()
    };
}
```

## Adapter Layer Design

### RenderBoxExt Trait

```rust
// Location: crates/flui_core/src/render.rs

pub trait RenderBoxExt<A: Arity>: RenderBox<A> + Sized {
    /// Convert leaf render object to element (no children)
    fn leaf(self) -> Element
    where
        A: Leaf,
    {
        RenderView::new(self).into_element()
    }

    /// Add single child to render object
    fn child(self, child: impl IntoElement) -> Element
    where
        A: Single,
    {
        let mut element = RenderView::new(self).into_element();
        // Attach child through element tree
        element.attach_child(child.into_element());
        element
    }

    /// Add multiple children to render object
    fn children(self, children: Vec<Element>) -> Element
    where
        A: Variable,
    {
        let mut element = RenderView::new(self).into_element();
        // Attach children through element tree
        for child in children {
            element.attach_child(child);
        }
        element
    }

    /// Add optional child to render object
    fn maybe_child(self, child: Option<Element>) -> Element
    where
        A: Optional,
    {
        let mut element = RenderView::new(self).into_element();
        if let Some(child) = child {
            element.attach_child(child);
        }
        element
    }
}

// Blanket implementation for all RenderBox types
impl<A: Arity, R: RenderBox<A>> RenderBoxExt<A> for R {}
```

**Key Design Decisions:**

1. **Generic over Arity:** Compile-time safety for child count
2. **Sized bound:** Enables value-based API (not trait objects)
3. **Blanket implementation:** Works for all RenderBox types automatically
4. **Where clauses:** Ensures methods only available for correct arity

## Child Management

### Child Helper Type

```rust
// Location: crates/flui-view/src/children.rs

/// Single optional child wrapper
#[derive(Debug, Default)]
pub struct Child(Option<Element>);

impl Child {
    pub fn new(widget: impl IntoElement) -> Self {
        Self(Some(widget.into_element()))
    }

    pub fn none() -> Self {
        Self(None)
    }

    pub fn is_some(&self) -> bool {
        self.0.is_some()
    }

    pub fn is_none(&self) -> bool {
        self.0.is_none()
    }

    pub fn into_element(self) -> Element {
        self.0.unwrap_or_else(|| EmptyView.into_element())
    }

    pub fn into_option(self) -> Option<Element> {
        self.0
    }
}

impl From<Element> for Child {
    fn from(element: Element) -> Self {
        Self(Some(element))
    }
}
```

### Children Helper Type

```rust
/// Multiple children wrapper
#[derive(Debug, Default)]
pub struct Children(Vec<Element>);

impl Children {
    pub fn new(widgets: Vec<impl IntoElement>) -> Self {
        Self(widgets.into_iter().map(|w| w.into_element()).collect())
    }

    pub fn from_vec(elements: Vec<Element>) -> Self {
        Self(elements)
    }

    pub fn push(&mut self, widget: impl IntoElement) {
        self.0.push(widget.into_element());
    }

    pub fn push_element(&mut self, element: Element) {
        self.0.push(element);
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn into_inner(self) -> Vec<Element> {
        self.0
    }
}

impl From<Vec<Element>> for Children {
    fn from(elements: Vec<Element>) -> Self {
        Self(elements)
    }
}

impl<T: IntoElement> From<Vec<T>> for Children {
    fn from(widgets: Vec<T>) -> Self {
        Self::new(widgets)
    }
}
```

## Widget Implementation Template

### Stateless Widget (Single Child)

```rust
use bon::Builder;
use flui_core::view::{StatelessView, Child, IntoElement};
use flui_core::BuildContext;
use flui_rendering::RenderPadding;
use flui_types::EdgeInsets;

#[derive(Debug, Builder)]
#[builder(on(EdgeInsets, into), finish_fn(name = build_internal, vis = ""))]
pub struct Padding {
    pub padding: EdgeInsets,

    #[builder(default, setters(vis = "", name = child_internal))]
    pub child: Child,
}

impl Padding {
    pub fn new(padding: EdgeInsets) -> Self {
        Self {
            padding,
            child: Child::none(),
        }
    }
}

impl Default for Padding {
    fn default() -> Self {
        Self {
            padding: EdgeInsets::ZERO,
            child: Child::none(),
        }
    }
}

impl StatelessView for Padding {
    fn build(self, _ctx: &dyn BuildContext) -> impl IntoElement {
        RenderPadding::new(self.padding).maybe_child(self.child.into_option())
    }
}

// Custom builder methods
use padding_builder::{IsUnset, SetChild, State};

impl<S: State> PaddingBuilder<S>
where
    S::Child: IsUnset,
{
    pub fn child(self, child: impl IntoElement) -> PaddingBuilder<SetChild<S>> {
        self.child_internal(Child::new(child))
    }
}

impl<S: State> PaddingBuilder<S> {
    pub fn build(self) -> Padding {
        self.build_internal()
    }
}

// Macro
#[macro_export]
macro_rules! padding {
    ($padding:expr, $child:expr) => {
        $crate::Padding::builder()
            .padding($padding)
            .child($child)
            .build()
    };
}
```

### Stateless Widget (Multiple Children)

```rust
use bon::Builder;
use flui_core::view::{StatelessView, Children, IntoElement};
use flui_core::BuildContext;
use flui_rendering::RenderFlex;
use flui_types::layout::{Axis, MainAxisAlignment, CrossAxisAlignment};

#[derive(Debug, Builder)]
#[builder(
    on(MainAxisAlignment, into),
    on(CrossAxisAlignment, into),
    finish_fn(name = build_internal, vis = "")
)]
pub struct Row {
    #[builder(default, setters(vis = "", name = children_internal))]
    pub children: Children,

    #[builder(default = MainAxisAlignment::Start)]
    pub main_axis_alignment: MainAxisAlignment,

    #[builder(default = CrossAxisAlignment::Center)]
    pub cross_axis_alignment: CrossAxisAlignment,
}

impl Row {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn spaced(spacing: f32, children: impl Into<Children>) -> Self {
        let children: Children = children.into();
        let mut spaced = Children::default();

        for (i, child) in children.into_inner().into_iter().enumerate() {
            if i > 0 {
                spaced.push(SizedBox::h_space(spacing));
            }
            spaced.push_element(child);
        }

        Self {
            children: spaced,
            ..Default::default()
        }
    }
}

impl Default for Row {
    fn default() -> Self {
        Self {
            children: Children::default(),
            main_axis_alignment: MainAxisAlignment::Start,
            cross_axis_alignment: CrossAxisAlignment::Center,
        }
    }
}

impl StatelessView for Row {
    fn build(self, _ctx: &dyn BuildContext) -> impl IntoElement {
        RenderFlex::new(Axis::Horizontal)
            .with_main_axis_alignment(self.main_axis_alignment)
            .with_cross_axis_alignment(self.cross_axis_alignment)
            .children(self.children.into_inner())
    }
}

// Custom builder methods
use row_builder::{IsUnset, SetChildren, State};

impl<S: State> RowBuilder<S>
where
    S::Children: IsUnset,
{
    pub fn children(self, children: impl Into<Children>) -> RowBuilder<SetChildren<S>> {
        self.children_internal(children.into())
    }
}

impl<S: State> RowBuilder<S> {
    pub fn build(self) -> Row {
        self.build_internal()
    }
}

// Macro
#[macro_export]
macro_rules! row {
    [$($child:expr),* $(,)?] => {
        $crate::Row::builder()
            .children(vec![$($child.into_element()),*])
            .build()
    };
}
```

## Testing Strategy

### Unit Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builder_pattern() {
        let widget = Container::builder()
            .padding(EdgeInsets::all(8.0))
            .color(Color::BLUE)
            .build();

        assert_eq!(widget.padding, Some(EdgeInsets::all(8.0)));
        assert_eq!(widget.color, Some(Color::BLUE));
    }

    #[test]
    fn test_struct_literal() {
        let widget = Container {
            padding: Some(EdgeInsets::all(8.0)),
            color: Some(Color::BLUE),
            ..Default::default()
        };

        assert_eq!(widget.padding, Some(EdgeInsets::all(8.0)));
        assert_eq!(widget.color, Some(Color::BLUE));
    }

    #[test]
    fn test_macro() {
        let widget = container! {
            padding: EdgeInsets::all(8.0),
            color: Color::BLUE,
        };

        assert_eq!(widget.padding, Some(EdgeInsets::all(8.0)));
        assert_eq!(widget.color, Some(Color::BLUE));
    }

    #[test]
    fn test_convenience_methods() {
        let colored = Container::colored(Color::RED, Text::new("Hello"));
        assert_eq!(colored.color, Some(Color::RED));

        let card = Container::card(Text::new("Card content"));
        assert!(card.decoration.is_some());
    }
}
```

### Integration Tests

```rust
#[cfg(test)]
mod integration {
    use super::*;

    #[test]
    fn test_widget_composes_correctly() {
        let widget = Container::builder()
            .padding(EdgeInsets::all(16.0))
            .color(Color::BLUE)
            .child(Text::new("Hello"))
            .build();

        // Build element tree
        let ctx = MockBuildContext::new();
        let element = widget.build(&ctx).into_element();

        // Verify element structure
        assert!(element.is_render());
        // More assertions...
    }
}
```

## Migration Checklist

For each widget:

- [ ] Update `build()` method to use adapter methods
- [ ] Add `#[derive(Builder)]` with bon
- [ ] Implement `Default` trait
- [ ] Add custom builder methods for `.child()` and `.children()`
- [ ] Add declarative macro if useful
- [ ] Add convenience constructors (e.g., `Container::colored()`)
- [ ] Update unit tests
- [ ] Add integration test
- [ ] Update doc comments with all three usage patterns
- [ ] Verify compilation: `cargo build -p flui_widgets`
- [ ] Run tests: `cargo test -p flui_widgets`

## Performance Considerations

1. **Zero-cost abstractions:** Adapter methods compile to same code as manual RenderView usage
2. **Child storage:** `Child` and `Children` have minimal overhead (Option/Vec)
3. **Macro expansion:** Macros expand to builder calls at compile time
4. **Bon builder:** No runtime overhead - all resolved at compile time

## Future Improvements

1. **Type-safe style system:** Strongly-typed style properties (future work)
2. **Animation integration:** Animated widget support (future work)
3. **Theme system:** Material Design theme widgets (future work)
4. **Hot reload:** Widget tree hot reloading (future work)
