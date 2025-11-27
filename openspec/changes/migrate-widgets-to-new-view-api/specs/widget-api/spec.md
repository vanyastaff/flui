# Widget API Specification

## Purpose

This specification defines the widget API patterns that SHALL be supported by all widgets in flui_widgets crate, enabling three ergonomic usage patterns: bon builder, struct literal, and declarative macros.

## ADDED Requirements

### Requirement: Three Usage Patterns

All widgets in flui_widgets SHALL support three usage patterns: bon builder, struct literal, and declarative macros.

#### Scenario: Bon builder pattern creates widget

**GIVEN** a widget type with multiple optional fields
**WHEN** user creates widget using bon builder pattern
**THEN** widget SHALL be created with type-safe builder API
**AND** builder SHALL support method chaining
**AND** optional fields SHALL have sensible defaults
**AND** required fields SHALL be enforced at compile time

**Example:**
```rust
let container = Container::builder()
    .padding(EdgeInsets::all(16.0))
    .color(Color::BLUE)
    .width(200.0)
    .child(Text::new("Hello"))
    .build();
```

#### Scenario: Struct literal creates widget

**GIVEN** a widget type with Default implementation
**WHEN** user creates widget using struct literal with spread operator
**THEN** widget SHALL be created with explicit field initialization
**AND** unspecified fields SHALL use Default values
**AND** syntax SHALL match Flutter's widget initialization

**Example:**
```rust
let container = Container {
    padding: Some(EdgeInsets::all(16.0)),
    color: Some(Color::BLUE),
    width: Some(200.0),
    child: Child::new(Text::new("Hello")),
    ..Default::default()
};
```

#### Scenario: Declarative macro creates widget

**GIVEN** a widget type with common usage patterns
**WHEN** user creates widget using declarative macro
**THEN** widget SHALL be created with concise syntax
**AND** macro SHALL expand to builder or struct literal
**AND** nested widgets SHALL compose naturally

**Example:**
```rust
let container = container! {
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

---

### Requirement: Adapter Layer for RenderObject Conversion

The RenderBoxExt trait SHALL provide ergonomic methods to convert RenderObjects to Elements based on arity.

#### Scenario: Convert leaf RenderObject to Element

**GIVEN** a RenderObject with Leaf arity (no children)
**WHEN** `.leaf()` method is called
**THEN** RenderObject SHALL be wrapped in RenderView
**AND** Element SHALL be created with no children
**AND** type system SHALL enforce Leaf arity at compile time

**Example:**
```rust
impl StatelessView for Text {
    fn build(self, _ctx: &dyn BuildContext) -> impl IntoElement {
        RenderParagraph::new(self.data).leaf()
    }
}
```

#### Scenario: Convert single-child RenderObject to Element

**GIVEN** a RenderObject with Single arity (exactly one child)
**WHEN** `.child(element)` method is called
**THEN** RenderObject SHALL be wrapped in RenderView
**AND** Element SHALL be created with one child attached
**AND** type system SHALL enforce Single arity at compile time

**Example:**
```rust
impl StatelessView for Padding {
    fn build(self, _ctx: &dyn BuildContext) -> impl IntoElement {
        RenderPadding::new(self.padding).child(self.child.into_element())
    }
}
```

#### Scenario: Convert multi-child RenderObject to Element

**GIVEN** a RenderObject with Variable arity (N children)
**WHEN** `.children(vec)` method is called
**THEN** RenderObject SHALL be wrapped in RenderView
**AND** Element SHALL be created with all children attached
**AND** type system SHALL enforce Variable arity at compile time

**Example:**
```rust
impl StatelessView for Row {
    fn build(self, _ctx: &dyn BuildContext) -> impl IntoElement {
        RenderFlex::row().children(self.children.into_inner())
    }
}
```

#### Scenario: Convert optional-child RenderObject to Element

**GIVEN** a RenderObject with Optional arity (0 or 1 child)
**WHEN** `.maybe_child(option)` method is called
**THEN** RenderObject SHALL be wrapped in RenderView
**AND** Element SHALL be created with optional child attached if Some
**AND** Element SHALL be created with no child if None
**AND** type system SHALL enforce Optional arity at compile time

**Example:**
```rust
impl StatelessView for Container {
    fn build(self, _ctx: &dyn BuildContext) -> impl IntoElement {
        RenderSizedBox::new(self.width, self.height)
            .maybe_child(self.child.into_option())
    }
}
```

---

### Requirement: Child Management Helpers

The Child and Children types SHALL provide ergonomic wrappers for child widget storage and conversion.

#### Scenario: Child type manages single optional child

**GIVEN** a widget with a single optional child
**WHEN** Child type is used for storage
**THEN** Child SHALL store Option<Element>
**AND** Child::new() SHALL accept any IntoElement
**AND** Child::none() SHALL represent absence of child
**AND** Child::into_element() SHALL convert to Element with empty fallback
**AND** Child::into_option() SHALL preserve None

**Example:**
```rust
pub struct Padding {
    pub padding: EdgeInsets,
    pub child: Child,  // Manages optional child
}

impl Padding {
    pub fn new(padding: EdgeInsets) -> Self {
        Self {
            padding,
            child: Child::none(),
        }
    }

    pub fn with_child(padding: EdgeInsets, child: impl IntoElement) -> Self {
        Self {
            padding,
            child: Child::new(child),
        }
    }
}
```

#### Scenario: Children type manages multiple children

**GIVEN** a widget with multiple children
**WHEN** Children type is used for storage
**THEN** Children SHALL store Vec<Element>
**AND** Children::new() SHALL accept Vec<impl IntoElement>
**AND** Children::from_vec() SHALL accept Vec<Element>
**AND** Children::push() SHALL accept any IntoElement
**AND** Children::into_inner() SHALL return Vec<Element>

**Example:**
```rust
pub struct Row {
    pub children: Children,  // Manages multiple children
    pub spacing: f32,
}

impl Row {
    pub fn spaced(spacing: f32, children: impl Into<Children>) -> Self {
        let mut row_children = Children::default();
        let children: Children = children.into();

        for (i, child) in children.into_inner().into_iter().enumerate() {
            if i > 0 {
                row_children.push(SizedBox::h_space(spacing));
            }
            row_children.push_element(child);
        }

        Self {
            children: row_children,
            spacing,
        }
    }
}
```

---

### Requirement: Widget Convenience Methods

Widgets SHALL provide convenience constructors for common patterns to reduce boilerplate.

#### Scenario: Convenience constructor creates widget with preset configuration

**GIVEN** a widget with common usage patterns
**WHEN** convenience constructor is called
**THEN** widget SHALL be created with preset configuration
**AND** preset SHALL follow Material Design guidelines where applicable
**AND** preset SHALL reduce boilerplate for common cases

**Example:**
```rust
impl Container {
    // Material Design card
    pub fn card(child: impl IntoElement) -> Self {
        let shadow = BoxShadow::new(
            Color::rgba(0, 0, 0, 25),
            Offset::new(0.0, 2.0),
            4.0,
            0.0
        );
        let decoration = BoxDecoration::default()
            .set_color(Some(Color::WHITE))
            .set_border_radius(Some(BorderRadius::circular(8.0)))
            .set_box_shadow(Some(vec![shadow]));

        Self::builder()
            .decoration(decoration)
            .padding(EdgeInsets::all(16.0))
            .child(child)
            .build()
    }

    // Solid color background
    pub fn colored(color: Color, child: impl IntoElement) -> Self {
        Self::builder()
            .color(color)
            .child(child)
            .build()
    }

    // Outlined container
    pub fn outlined(border_color: Color, child: impl IntoElement) -> Self {
        let decoration = BoxDecoration::default()
            .set_border_radius(Some(BorderRadius::circular(8.0)))
            .set_border(Some(Border::all(BorderSide::new(
                border_color,
                1.0,
                BorderStyle::Solid,
            ))));

        Self::builder()
            .decoration(decoration)
            .padding(EdgeInsets::all(12.0))
            .child(child)
            .build()
    }
}
```

#### Scenario: Convenience constructor with spacing inserts separators

**GIVEN** a layout widget with multiple children
**WHEN** spacing convenience constructor is called
**THEN** widget SHALL insert spacing widgets between children
**AND** spacing SHALL be uniform
**AND** no spacing SHALL appear before first or after last child

**Example:**
```rust
impl Row {
    pub fn spaced(spacing: f32, children: impl Into<Children>) -> Self {
        let children: Children = children.into();
        let mut spaced_children = Children::default();

        for (i, child) in children.into_inner().into_iter().enumerate() {
            if i > 0 {
                spaced_children.push(SizedBox::h_space(spacing));
            }
            spaced_children.push_element(child);
        }

        Self::builder()
            .children(spaced_children)
            .build()
    }
}

// Usage:
let row = Row::spaced(8.0, vec![
    text!("Label 1"),
    text!("Label 2"),
    text!("Label 3"),
]);
```

---

### Requirement: Compilation and Type Safety

All widget patterns SHALL be type-safe and compile without errors.

#### Scenario: Builder enforces required fields at compile time

**GIVEN** a widget with required fields
**WHEN** builder is used without setting required fields
**THEN** code SHALL fail to compile with clear error message
**AND** error SHALL indicate which fields are missing

#### Scenario: Arity mismatch detected at compile time

**GIVEN** a RenderObject with specific arity
**WHEN** incorrect adapter method is called
**THEN** code SHALL fail to compile
**AND** error SHALL indicate arity constraint violation

**Example (compile error):**
```rust
// ❌ Compile error: Leaf arity cannot have children
RenderParagraph::new(data).child(some_child);

// ❌ Compile error: Variable arity cannot use .leaf()
RenderFlex::row().leaf();

// ✅ Correct: Leaf arity uses .leaf()
RenderParagraph::new(data).leaf();

// ✅ Correct: Variable arity uses .children()
RenderFlex::row().children(vec![child1, child2]);
```

#### Scenario: All widgets compile without errors

**GIVEN** all 80+ widgets in flui_widgets
**WHEN** cargo build -p flui_widgets is run
**THEN** build SHALL succeed with 0 errors
**AND** all warnings SHALL be resolved

---

## Related Specs

- **flui-rendering** - RenderObject implementations
- **compositor-layers** - Layer system for rendering
- **interaction-handlers** - Pointer event handling

## Migration Notes

This spec replaces the old widget API where RenderObjects were used directly without adapter methods. All existing widgets (80+) will be migrated to follow these patterns.
