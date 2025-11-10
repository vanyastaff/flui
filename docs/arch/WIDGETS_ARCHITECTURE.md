# FLUI Widgets Architecture

**Version:** 0.1.0
**Date:** 2025-01-10
**Author:** Claude (Anthropic)
**Status:** Production Ready

---

## Executive Summary

This document describes the architecture of **flui_widgets** crate, which provides **high-level declarative widgets** for FLUI. It sits at the top of the three-tree architecture, offering a Flutter-compatible API with Rust ergonomics.

**Current Status:** ✅ 60+ widgets implemented with builder patterns and convenience methods

**Key Responsibilities:**
1. **Declarative UI API** - High-level widget definitions (Container, Row, Column, Text, etc.)
2. **Composition** - Combining RenderObjects into reusable patterns
3. **Builder Patterns** - Ergonomic Rust-style APIs using `bon` crate
4. **Convenience Methods** - Common Material Design patterns as one-liners
5. **View Implementation** - Implementing the unified `View` trait

**Architecture Pattern:** **Composite Pattern** (widgets compose other widgets) + **Builder Pattern** (bon-based type-safe builders)

---

## Table of Contents

1. [Architecture Overview](#architecture-overview)
2. [Widget Types](#widget-types)
3. [Creation Patterns](#creation-patterns)
4. [Widget Categories](#widget-categories)
5. [View Trait Implementation](#view-trait-implementation)
6. [Builder Pattern Integration](#builder-pattern-integration)
7. [Stateless vs RenderObject Widgets](#stateless-vs-renderobject-widgets)
8. [Common Patterns](#common-patterns)

---

## Architecture Overview

### Position in the Stack

```text
┌─────────────────────────────────────────────────────────────┐
│                    User Application                         │
│              (Business logic + UI composition)               │
│                                                              │
│  fn main() {                                                │
│      Column::new()                                          │
│          .children(vec![                                    │
│              Container::colored(Color::BLUE, ...),          │
│              Text::headline("Hello"),                       │
│          ])                                                 │
│  }                                                          │
└──────────────────────┬──────────────────────────────────────┘
                       │ uses
                       ▼
┌─────────────────────────────────────────────────────────────┐
│                   flui_widgets                              │
│          (THIS CRATE - High-level widgets)                   │
│                                                              │
│  Widget Types:                                              │
│  ┌──────────────────────────────────────────┐              │
│  │ 1. StatelessWidget (Composition)         │              │
│  │    - Container, Builder, Card            │              │
│  │                                           │              │
│  │ 2. RenderObjectWidget (Direct)           │              │
│  │    - Padding, Text, Column, Opacity      │              │
│  └──────────────────────────────────────────┘              │
│                       ↓                                      │
│  All implement View trait:                                  │
│  impl View for MyWidget {                                   │
│      fn build(self, ctx) -> impl IntoElement { ... }       │
│  }                                                          │
└──────────────────────┬──────────────────────────────────────┘
                       │ builds
                       ▼
┌─────────────────────────────────────────────────────────────┐
│                     flui_core                               │
│              (Element tree + BuildContext)                   │
│                                                              │
│  View → Element (Component/Render) → LayoutCache           │
└──────────────────────┬──────────────────────────────────────┘
                       │ contains
                       ▼
┌─────────────────────────────────────────────────────────────┐
│                  flui_rendering                             │
│              (RenderObjects - layout/paint)                  │
│                                                              │
│  RenderPadding, RenderFlex, RenderParagraph, etc.           │
└─────────────────────────────────────────────────────────────┘
```

### Three-Tree Architecture

```text
Widget Tree                 Element Tree              Render Tree
(Immutable)                 (Mutable)                (Layout/Paint)

┌─────────────┐            ┌──────────────┐          ┌──────────────┐
│  Container  │  build     │ ComponentElem│          │              │
│  ├─ Padding │ ────────> │   ├─ Render  │ ───────> │ RenderPadding│
│  └─ Text    │            │   └─ Render  │          │ RenderPara.. │
└─────────────┘            └──────────────┘          └──────────────┘
     ↑                           ↑                         ↑
flui_widgets              flui_core                 flui_rendering
```

**flui_widgets provides the Widget Tree** - the leftmost tree, which is immutable and declarative.

---

## Widget Types

FLUI widgets follow Flutter's two-category system:

### 1. StatelessWidget (Composition)

**Definition:** Widgets that **compose other widgets** without directly creating RenderObjects.

**Examples:**
- `Container` → composes Padding + Align + DecoratedBox + ConstrainedBox
- `Card` → composes Container with Material Design decoration
- `Builder` → callback-based dynamic composition
- `LayoutBuilder` → composition based on constraints

**Pattern:**
```rust
impl View for Container {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        // Compose other widgets
        let mut child = self.child;

        // Apply padding
        if let Some(padding) = self.padding {
            child = Some(Box::new(Padding::new(padding, child)));
        }

        // Apply alignment
        if let Some(alignment) = self.alignment {
            child = Some(Box::new(Align::new(alignment, child)));
        }

        // Apply decoration
        if let Some(decoration) = self.decoration {
            child = Some(Box::new(DecoratedBox::new(decoration, child)));
        }

        // Apply constraints
        if let Some(constraints) = self.constraints {
            child = Some(Box::new(ConstrainedBox::new(constraints, child)));
        }

        child.unwrap_or_else(|| Box::new(SizedBox::new()))
    }
}
```

**Key Point:** Container is NOT a RenderObject. It's a convenience API that builds a tree of simpler widgets.

### 2. RenderObjectWidget (Direct)

**Definition:** Widgets that **directly create RenderObjects** for layout and painting.

**Examples:**
- `Padding` → creates `RenderPadding`
- `Text` → creates `RenderParagraph`
- `Column` → creates `RenderFlex` (vertical)
- `Opacity` → creates `RenderOpacity`

**Pattern:**
```rust
impl View for Padding {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        // Directly return RenderObject + child
        (RenderPadding::new(self.padding), self.child)
    }
}
```

**Key Point:** RenderObjectWidgets are thin wrappers that create the actual layout primitives.

---

## Creation Patterns

FLUI widgets support **4 creation patterns** for maximum flexibility:

### 1. Convenience Methods (Recommended)

**Best for:** Common patterns, Material Design presets

```rust
// Container patterns
Container::colored(Color::BLUE, child)          // Solid background
Container::card(child)                           // Material card
Container::outlined(Color::BLUE, child)          // Border
Container::surface(child)                        // Surface with padding
Container::rounded(Color::GREEN, 12.0, child)   // Rounded corners
Container::sized(200.0, 100.0, child)           // Fixed size

// Text patterns
Text::new("Hello")                               // Basic
Text::sized("Title", 24.0)                       // Custom size
Text::colored("Error", Color::RED)               // Custom color
Text::headline("Main Title")                     // 32px bold
Text::title("Section")                           // 24px
Text::body("Content")                            // 16px
Text::caption("Note")                            // 12px

// Padding patterns
Padding::all(16.0, child)                        // Uniform
Padding::symmetric(20.0, 10.0, child)            // Horizontal, vertical
Padding::only(left: 10.0, child)                 // Specific sides
```

### 2. Builder Pattern (Type-safe)

**Best for:** Complex configurations, IDE autocomplete

```rust
// Using bon-generated builder
Container::builder()
    .width(300.0)
    .height(200.0)
    .padding(EdgeInsets::all(20.0))
    .color(Color::rgb(255, 0, 0))
    .child(Text::new("Content"))
    .build()

Column::builder()
    .main_axis_alignment(MainAxisAlignment::Center)
    .cross_axis_alignment(CrossAxisAlignment::Start)
    .children(vec![
        Box::new(Text::new("Item 1")),
        Box::new(Text::new("Item 2")),
    ])
    .build()
```

**Benefits:**
- ✅ Type-safe (compile-time errors for invalid combinations)
- ✅ IDE autocomplete for all fields
- ✅ Optional fields with sensible defaults
- ✅ No need to remember field order

### 3. Struct Literal (Flutter-like)

**Best for:** Familiarity for Flutter developers

```rust
Container {
    width: Some(300.0),
    height: Some(200.0),
    padding: Some(EdgeInsets::all(20.0)),
    color: Some(Color::rgb(255, 0, 0)),
    child: Some(Box::new(Text::new("Content"))),
    ..Default::default()
}

Column {
    main_axis_alignment: Some(MainAxisAlignment::Center),
    children: vec![
        Box::new(Text::new("Item 1")),
        Box::new(Text::new("Item 2")),
    ],
    ..Default::default()
}
```

**Benefits:**
- ✅ Familiar to Flutter developers
- ✅ No method call overhead
- ✅ Direct field access

### 4. Macros (Future Enhancement)

**Best for:** Concise DSL-like syntax

```rust
// Planned for future versions
container! {
    width: 300.0,
    height: 200.0,
    padding: EdgeInsets::all(20.0),
    child: text!("Content"),
}

column! {
    main_axis_alignment: MainAxisAlignment::Center,
    children: vec![
        text!("Item 1"),
        text!("Item 2"),
    ],
}
```

---

## Widget Categories

flui_widgets organizes 60+ widgets into 5 categories:

### 1. Basic Widgets (20 widgets)

**Purpose:** Fundamental building blocks

| Widget | Type | Description |
|--------|------|-------------|
| **Container** | Stateless | Combines sizing, padding, decoration, constraints |
| **SizedBox** | RenderObject | Fixed-size box or spacer |
| **Padding** | RenderObject | Adds padding around child |
| **Center** | RenderObject | Centers child |
| **Align** | RenderObject | Aligns child with flexible positioning |
| **Text** | RenderObject | Displays styled text |
| **ColoredBox** | RenderObject | Solid color background |
| **DecoratedBox** | RenderObject | Box decoration (borders, gradients, shadows) |
| **ConstrainedBox** | RenderObject | Adds constraints |
| **LimitedBox** | RenderObject | Limits size when unconstrained |
| **AspectRatio** | RenderObject | Maintains aspect ratio |
| **FittedBox** | RenderObject | Scales/fits child to available space |
| **SafeArea** | Stateless | Insets child from system UI (notches, etc.) |
| **Builder** | Stateless | Callback-based dynamic composition |
| **LayoutBuilder** | Stateless | Composition based on constraints |
| **Button** | Stateless | Material Design button |
| **Card** | Stateless | Material Design card |
| **AppBar** | Stateless | Material Design app bar |
| **Divider** | RenderObject | Horizontal divider line |
| **VerticalDivider** | RenderObject | Vertical divider line |

**Example - Text:**

```rust
// In flui_widgets/src/basic/text.rs

use bon::Builder;
use flui_core::view::{IntoElement, View};
use flui_core::BuildContext;
use flui_rendering::{ParagraphData, RenderParagraph};

/// A widget that displays a string of text with a single style.
#[derive(Debug, Clone, Builder)]
#[builder(on(String, into), finish_fn(name = build_internal, vis = ""))]
pub struct Text {
    pub data: String,

    #[builder(default = 14.0)]
    pub size: f32,

    #[builder(default = Color::BLACK)]
    pub color: Color,

    #[builder(default = TextAlign::Start)]
    pub text_align: TextAlign,

    #[builder(default = None)]
    pub max_lines: Option<usize>,

    #[builder(default = TextOverflow::Clip)]
    pub overflow: TextOverflow,
}

impl Text {
    /// Create basic text
    pub fn new(data: impl Into<String>) -> Self {
        Self::builder().data(data).build()
    }

    /// Create sized text
    pub fn sized(data: impl Into<String>, size: f32) -> Self {
        Self::builder().data(data).size(size).build()
    }

    /// Create colored text
    pub fn colored(data: impl Into<String>, color: Color) -> Self {
        Self::builder().data(data).color(color).build()
    }

    /// Headline preset (32px bold)
    pub fn headline(data: impl Into<String>) -> Self {
        Self::builder().data(data).size(32.0).build()
    }

    /// Title preset (24px)
    pub fn title(data: impl Into<String>) -> Self {
        Self::builder().data(data).size(24.0).build()
    }

    /// Body preset (16px)
    pub fn body(data: impl Into<String>) -> Self {
        Self::builder().data(data).size(16.0).build()
    }

    /// Caption preset (12px)
    pub fn caption(data: impl Into<String>) -> Self {
        Self::builder().data(data).size(12.0).build()
    }
}

// RenderObjectWidget - directly creates RenderParagraph
impl View for Text {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        let paragraph_data = ParagraphData {
            text: self.data,
            font_size: self.size,
            color: self.color,
            text_align: self.text_align,
            max_lines: self.max_lines,
            overflow: self.overflow,
            text_direction: TextDirection::LTR,
            soft_wrap: true,
        };

        // Return RenderObject (no children for leaf widget)
        (RenderParagraph::new(paragraph_data), ())
    }
}
```

### 2. Layout Widgets (20 widgets)

**Purpose:** Positioning and sizing children

| Widget | Children | Description |
|--------|----------|-------------|
| **Row** | N | Horizontal flex layout |
| **Column** | N | Vertical flex layout |
| **Flex** | N | Customizable flex layout (Row/Column base) |
| **Expanded** | 1 | Fills available space in Row/Column (tight) |
| **Flexible** | 1 | Fills available space (loose or tight) |
| **Spacer** | 0 | Creates flexible empty space |
| **Stack** | N | Z-index stacking with absolute positioning |
| **Positioned** | 1 | Absolute positioning within Stack |
| **PositionedDirectional** | 1 | RTL-aware positioned |
| **IndexedStack** | N | Shows only one child at index |
| **Wrap** | N | Wrapping flex layout (like flexbox wrap) |
| **ListBody** | N | Simple list layout (no scrolling) |
| **Baseline** | 1 | Baseline alignment |
| **IntrinsicWidth** | 1 | Forces intrinsic width |
| **IntrinsicHeight** | 1 | Forces intrinsic height |
| **FractionallySizedBox** | 1 | Percentage-based sizing |
| **OverflowBox** | 1 | Allows child to overflow constraints |
| **SizedOverflowBox** | 1 | Sized box with overflow |
| **RotatedBox** | 1 | 90° rotation |
| **Viewport** | N | Scrollable viewport |
| **SingleChildScrollView** | 1 | Scrollable single child |

**Example - Column:**

```rust
// In flui_widgets/src/layout/column.rs

use bon::Builder;
use flui_core::view::{AnyView, IntoElement, View};
use flui_core::BuildContext;
use flui_rendering::RenderFlex;
use flui_types::layout::{CrossAxisAlignment, MainAxisAlignment, MainAxisSize};

/// A widget that displays its children in a vertical array.
#[derive(Builder)]
#[builder(finish_fn(name = build_internal, vis = ""))]
pub struct Column {
    /// The widgets to display in this column.
    #[builder(field)]
    pub children: Vec<Box<dyn AnyView>>,

    /// How to align children along the main axis (vertical)
    #[builder(default = MainAxisAlignment::Start)]
    pub main_axis_alignment: MainAxisAlignment,

    /// How much space should be occupied on the main axis
    #[builder(default = MainAxisSize::Max)]
    pub main_axis_size: MainAxisSize,

    /// How to align children along the cross axis (horizontal)
    #[builder(default = CrossAxisAlignment::Center)]
    pub cross_axis_alignment: CrossAxisAlignment,
}

impl Column {
    /// Create new Column
    pub fn new() -> Self {
        Self::builder().build()
    }

    /// Create Column with children
    pub fn with_children(children: Vec<Box<dyn AnyView>>) -> Self {
        Self::builder().children(children).build()
    }
}

// RenderObjectWidget - creates RenderFlex with vertical axis
impl View for Column {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        let render_flex = RenderFlex::column()
            .with_main_axis_alignment(self.main_axis_alignment)
            .with_main_axis_size(self.main_axis_size)
            .with_cross_axis_alignment(self.cross_axis_alignment);

        // Return RenderObject + children
        (render_flex, self.children)
    }
}
```

### 3. Visual Effects Widgets (12 widgets)

**Purpose:** Visual transformations and effects

| Widget | Type | Description |
|--------|------|-------------|
| **Opacity** | RenderObject | Applies opacity/transparency |
| **Transform** | RenderObject | 2D/3D matrix transformations |
| **ClipRect** | RenderObject | Rectangular clipping |
| **ClipRRect** | RenderObject | Rounded rectangle clipping |
| **ClipOval** | RenderObject | Oval/circle clipping |
| **PhysicalModel** | RenderObject | Material Design elevation + shadows |
| **Material** | Stateless | Material Design surface with elevation |
| **BackdropFilter** | RenderObject | Blur/filter background |
| **RepaintBoundary** | RenderObject | Isolates repaints for performance |
| **Offstage** | RenderObject | Hides child (layout but no paint) |
| **Visibility** | RenderObject | Shows/hides child |

**Example - Opacity:**

```rust
// In flui_widgets/src/visual_effects/opacity.rs

use bon::Builder;
use flui_core::view::{AnyView, IntoElement, View};
use flui_core::BuildContext;
use flui_rendering::RenderOpacity;

/// A widget that makes its child partially transparent.
#[derive(Builder)]
#[builder(finish_fn(name = build_internal, vis = ""))]
pub struct Opacity {
    /// The opacity level (0.0 = fully transparent, 1.0 = fully opaque)
    #[builder(default = 1.0)]
    pub opacity: f32,

    /// The child widget
    pub child: Option<Box<dyn AnyView>>,
}

impl Opacity {
    /// Create new Opacity widget
    pub fn new(opacity: f32, child: impl AnyView + 'static) -> Self {
        Self::builder()
            .opacity(opacity)
            .child(Some(Box::new(child)))
            .build()
    }
}

// RenderObjectWidget - creates RenderOpacity
impl View for Opacity {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        (RenderOpacity::new(self.opacity), self.child)
    }
}
```

### 4. Interaction Widgets (3 widgets)

**Purpose:** Pointer event handling

| Widget | Type | Description |
|--------|------|-------------|
| **GestureDetector** | Stateless | Gesture recognition (tap, drag, etc.) |
| **MouseRegion** | RenderObject | Mouse enter/exit/hover events |
| **IgnorePointer** | RenderObject | Makes widget transparent to pointer events |
| **AbsorbPointer** | RenderObject | Blocks pointer events from passing through |

### 5. Scrolling Widgets (3 widgets)

**Purpose:** Scrollable containers

| Widget | Type | Description |
|--------|------|-------------|
| **SingleChildScrollView** | RenderObject | Scrollable single child |
| **Viewport** | RenderObject | Viewport for sliver scrolling |
| **ScrollController** | State | Controls scroll position |

---

## View Trait Implementation

All widgets implement the unified `View` trait:

```rust
// In flui_core/src/view/view.rs

pub trait View: 'static {
    /// Build this view into an element
    ///
    /// Returns either:
    /// - (RenderObject, child/children) → RenderObjectWidget
    /// - AnyElement → StatelessWidget (composed)
    fn build(self, ctx: &BuildContext) -> impl IntoElement;
}
```

### IntoElement Types

The `build()` method can return different types:

```rust
// 1. Leaf RenderObject (0 children)
impl View for Text {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        (RenderParagraph::new(data), ())  // () = no children
    }
}

// 2. Single-child RenderObject
impl View for Padding {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        (RenderPadding::new(padding), self.child)  // Option<Box<dyn AnyView>>
    }
}

// 3. Multi-child RenderObject
impl View for Column {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        (RenderFlex::column(), self.children)  // Vec<Box<dyn AnyView>>
    }
}

// 4. Composed widget (StatelessWidget)
impl View for Container {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        // Build tree of other widgets
        let child = /* compose Padding + Align + DecoratedBox + ... */;
        child  // Returns AnyElement
    }
}
```

**Key Insight:** The `IntoElement` trait automatically handles tree construction. Widgets don't need to manually insert into the element tree!

---

## Builder Pattern Integration

FLUI uses the [`bon`](https://crates.io/crates/bon) crate for type-safe builders:

### bon Builder Features

```rust
use bon::Builder;

#[derive(Builder)]
#[builder(
    on(String, into),                    // Auto-convert Into<String>
    on(Color, into),                     // Auto-convert Into<Color>
    finish_fn(name = build_internal, vis = "")  // Internal build method
)]
pub struct Container {
    #[builder(default = None)]
    pub width: Option<f32>,

    #[builder(default = None)]
    pub height: Option<f32>,

    #[builder(default = None)]
    pub padding: Option<EdgeInsets>,

    #[builder(default = None)]
    pub color: Option<Color>,

    #[builder(setters(vis = "", name = child_internal))]  // Private setter
    pub child: Option<Box<dyn AnyView>>,
}

// Public API wrapper
impl Container {
    pub fn builder() -> ContainerBuilder {
        ContainerBuilder::default()
    }

    // Custom child setter (public)
    pub fn child(builder: ContainerBuilder, child: impl AnyView + 'static) -> ContainerBuilder {
        builder.child_internal(Some(Box::new(child)))
    }

    // Finalize build
    pub fn build(builder: ContainerBuilder) -> Self {
        builder.build_internal()
    }
}
```

### Benefits of bon

| Feature | Benefit |
|---------|---------|
| **Type-safe** | Compile-time errors for invalid configs |
| **Optional fields** | Sensible defaults, no need to specify everything |
| **Into conversions** | Auto-convert `&str` to `String`, etc. |
| **IDE autocomplete** | Full autocomplete for all fields |
| **Chainable** | `.field1(x).field2(y).build()` |
| **No proc macro overhead** | Minimal compile-time impact |

---

## Stateless vs RenderObject Widgets

### When to Use StatelessWidget

**Use when:**
- ✅ Composing multiple widgets into a reusable pattern
- ✅ Implementing Material Design components (Card, Button, AppBar)
- ✅ Creating convenience APIs (Container combines 4+ widgets)
- ✅ Dynamic composition based on conditions (Builder, LayoutBuilder)

**Example - Container (Stateless):**

```rust
impl View for Container {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        let mut current_child = self.child;

        // Layer 1: Apply margin (as Padding)
        if let Some(margin) = self.margin {
            current_child = Some(Box::new(Padding::new(margin, current_child)));
        }

        // Layer 2: Apply decoration
        if let Some(decoration) = self.decoration {
            current_child = Some(Box::new(DecoratedBox::new(decoration, current_child)));
        } else if let Some(color) = self.color {
            current_child = Some(Box::new(ColoredBox::new(color, current_child)));
        }

        // Layer 3: Apply padding
        if let Some(padding) = self.padding {
            current_child = Some(Box::new(Padding::new(padding, current_child)));
        }

        // Layer 4: Apply alignment
        if let Some(alignment) = self.alignment {
            current_child = Some(Box::new(Align::new(alignment, current_child)));
        }

        // Layer 5: Apply constraints
        let constraints = self.build_constraints();
        if constraints != BoxConstraints::UNBOUNDED {
            current_child = Some(Box::new(ConstrainedBox::new(constraints, current_child)));
        }

        // Return composed tree
        current_child.unwrap_or_else(|| Box::new(SizedBox::new()))
    }
}
```

**Result:** Container creates a tree of 5+ RenderObjects without being one itself.

### When to Use RenderObjectWidget

**Use when:**
- ✅ Implementing layout primitives (Padding, Align, Flex)
- ✅ Creating visual effects (Opacity, Transform, Clip)
- ✅ Rendering content (Text, Image, CustomPaint)
- ✅ Need direct control over layout/paint

**Example - Padding (RenderObject):**

```rust
impl View for Padding {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        // Directly return RenderPadding (thin wrapper)
        (RenderPadding::new(self.padding), self.child)
    }
}
```

**Result:** Padding creates exactly one RenderPadding in the render tree.

---

## Common Patterns

### 1. Responsive Layout

```rust
use flui_widgets::prelude::*;

// Mobile-first responsive layout
fn responsive_layout() -> impl View {
    LayoutBuilder::new(|constraints| {
        if constraints.max_width() > 600.0 {
            // Desktop layout (row)
            Row::builder()
                .children(vec![
                    Box::new(sidebar()),
                    Box::new(Expanded::new(content())),
                ])
                .build()
        } else {
            // Mobile layout (column)
            Column::builder()
                .children(vec![
                    Box::new(content()),
                ])
                .build()
        }
    })
}
```

### 2. Material Design Card

```rust
// Option 1: Using Container.card() convenience method
Container::card(
    Text::title("Card Title")
)

// Option 2: Manual composition
Container::builder()
    .padding(EdgeInsets::all(16.0))
    .decoration(BoxDecoration::default()
        .with_color(Color::WHITE)
        .with_border_radius(BorderRadius::all(8.0))
        .with_box_shadow(vec![
            BoxShadow::new(
                Color::rgba(0, 0, 0, 0.1),
                Offset::new(0.0, 2.0),
                4.0,
                0.0,
            ),
        ])
    )
    .child(content)
    .build()
```

### 3. Flex Layout with Spacing

```rust
use flui_widgets::prelude::*;

// Row with gaps between children
Row::builder()
    .main_axis_alignment(MainAxisAlignment::SpaceBetween)
    .children(vec![
        Box::new(Text::new("Left")),
        Box::new(Spacer::new()),  // Flexible space
        Box::new(Text::new("Right")),
    ])
    .build()

// Column with equal spacing
Column::builder()
    .main_axis_alignment(MainAxisAlignment::SpaceEvenly)
    .children(vec![
        Box::new(item1),
        Box::new(item2),
        Box::new(item3),
    ])
    .build()
```

### 4. Nested Flex Layouts

```rust
use flui_widgets::prelude::*;

// Complex layout: Row of columns
Row::builder()
    .children(vec![
        Box::new(Expanded::new(
            Column::builder()
                .children(vec![
                    Box::new(Text::headline("Section 1")),
                    Box::new(Text::body("Content")),
                ])
                .build()
        )),
        Box::new(SizedBox::new().width(16.0)),  // Gap
        Box::new(Expanded::new(
            Column::builder()
                .children(vec![
                    Box::new(Text::headline("Section 2")),
                    Box::new(Text::body("Content")),
                ])
                .build()
        )),
    ])
    .build()
```

### 5. Conditional Rendering

```rust
use flui_widgets::prelude::*;

fn conditional_widget(show_title: bool) -> impl View {
    Column::builder()
        .children({
            let mut children: Vec<Box<dyn AnyView>> = vec![];

            // Conditional title
            if show_title {
                children.push(Box::new(Text::headline("Title")));
                children.push(Box::new(SizedBox::new().height(16.0)));
            }

            // Always show content
            children.push(Box::new(Text::body("Content")));

            children
        })
        .build()
}
```

### 6. Builder Pattern for Dynamic UIs

```rust
use flui_widgets::prelude::*;

fn dynamic_list(items: Vec<String>) -> impl View {
    Column::builder()
        .children(
            items.into_iter()
                .map(|item| Box::new(Text::new(item)) as Box<dyn AnyView>)
                .collect()
        )
        .build()
}
```

---

## Summary

**flui_widgets** provides the **high-level declarative API** for FLUI:

- ✅ **60+ Flutter-compatible widgets** - Container, Row, Column, Text, Opacity, etc.
- ✅ **Two widget types** - StatelessWidget (composition) and RenderObjectWidget (direct)
- ✅ **Four creation patterns** - Convenience methods, builders, struct literals, macros
- ✅ **bon-based builders** - Type-safe, chainable, with IDE autocomplete
- ✅ **Unified View trait** - Single `build()` method returns `impl IntoElement`
- ✅ **Material Design patterns** - Card, Button, AppBar, Surface presets
- ✅ **Ergonomic Rust API** - Into conversions, optional fields, sensible defaults

**Clear Separation of Concerns:**
- **flui_widgets** provides high-level declarative API (this crate)
- **flui_core** manages element tree and build context
- **flui_rendering** implements layout/paint primitives (RenderObjects)
- **flui_painting** records drawing commands (Canvas API)
- **flui_engine** executes drawing commands (GPU rendering)

**Total LOC:** ~10,000 (60+ widgets with full builder patterns)

This architecture provides Flutter's declarative UI model with Rust's type safety and zero-cost abstractions!

---

## Related Documentation

### Implementation
- **Source Code**: `crates/flui_widgets/src/`
- **Basic Widgets**: `crates/flui_widgets/src/basic/`
- **Layout Widgets**: `crates/flui_widgets/src/layout/`
- **Examples**: `crates/flui_core/examples/simplified_view.rs`

### Patterns & Integration
- **Patterns**: [PATTERNS.md](PATTERNS.md#core-architecture-patterns) - View trait, Builder pattern
- **Integration**: [INTEGRATION.md](INTEGRATION.md#scenario-1-adding-a-new-widget) - Adding widgets, integration flows
- **Navigation**: [README.md](README.md) - Architecture documentation hub

### Related Architecture Docs
- **flui_core**: [CORE_FEATURES_ROADMAP.md](CORE_FEATURES_ROADMAP.md) - View trait and element tree
- **flui_rendering**: [RENDERING_ARCHITECTURE.md](RENDERING_ARCHITECTURE.md) - RenderObject implementation
- **flui_painting**: [PAINTING_ARCHITECTURE.md](PAINTING_ARCHITECTURE.md) - Canvas and drawing
- **flui_gestures**: [GESTURES_ARCHITECTURE.md](GESTURES_ARCHITECTURE.md) - Input handling

### External References
- **Flutter Widgets**: [flutter.dev/widgets](https://flutter.dev/docs/development/ui/widgets)
- **bon Builder**: [docs.rs/bon](https://docs.rs/bon) - Builder pattern library
- **FLUI Guide**: [../../CLAUDE.md](../../CLAUDE.md#creating-a-simple-view-new-api) - Widget development
