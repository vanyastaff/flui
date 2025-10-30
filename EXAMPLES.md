# Flui Examples Gallery

Welcome to the Flui examples gallery! This document showcases all available examples with visual descriptions and key learning points.

## Table of Contents

1. [Getting Started](#getting-started)
2. [Basic Examples](#basic-examples)
3. [Real-World UI Examples](#real-world-ui-examples)
4. [Widget Patterns](#widget-patterns)

---

## Getting Started

All examples can be run using:

```bash
cargo run --example <name> --features="flui_app,flui_widgets"
```

For example:
```bash
cargo run --example profile_card --features="flui_app,flui_widgets"
```

---

## Basic Examples

### Hello World (`widget_hello_world.rs`)

**What it demonstrates:**
- Simplest possible Flui application
- Modern builder pattern
- `impl_into_widget!` macro usage
- Basic widget composition

**Key widgets:**
- `Container` - for layout and styling
- `Center` - for centering content
- `Text` - for displaying text

**Code snippet:**
```rust
Container::builder()
    .padding(EdgeInsets::all(40.0))
    .color(Color::rgb(245, 245, 245))
    .child(
        Center::builder()
            .child(
                Container::builder()
                    .decoration(BoxDecoration {
                        color: Some(Color::rgb(66, 165, 245)),
                        border_radius: Some(BorderRadius::circular(12.0)),
                        ..Default::default()
                    })
                    .child(Text::builder().data("Hello, Flui!").build())
                    .build()
            )
            .build()
    )
    .build()
```

---

## Real-World UI Examples

### 1. Profile Card (`profile_card.rs`)

**What it demonstrates:**
A beautiful social media-style profile card with avatar, name, title, stats, and action buttons.

**Visual Layout:**
```
┌─────────────────────────────┐
│         Card Widget         │
│  ┌───────────────────────┐  │
│  │   ●●●  (Avatar)       │  │
│  │    John Doe           │  │
│  │  Senior Rust Dev      │  │
│  ├───────────────────────┤  │
│  │ 128    2.5K    312    │  │
│  │ Posts  Followers  F...│  │
│  ├───────────────────────┤  │
│  │ [Follow] [Message]    │  │
│  └───────────────────────┘  │
└─────────────────────────────┘
```

**Key widgets used:**
- `Card` - Elevated surface with shadow
- `ClipOval` - Circular avatar clipping
- `Column` - Vertical layout
- `Row` - Horizontal layouts for stats and buttons
- `Divider` - Visual separation
- `Button` - Action buttons
- `Text` - All text content

**Learn:**
- How to create professional card layouts
- Using ClipOval for circular images
- Composing complex layouts with Row/Column
- Building reusable stat components

**Run:**
```bash
cargo run --example profile_card --features="flui_app,flui_widgets"
```

---

### 2. Dashboard (`dashboard.rs`)

**What it demonstrates:**
A complete admin dashboard with header, stats cards, activity feed, and quick actions panel.

**Visual Layout:**
```
┌─────────────────────────────────────────────────┐
│ Dashboard              Welcome back, John!      │
├─────────────────────────────────────────────────┤
│  ┌──────┐  ┌──────┐  ┌──────┐                  │
│  │Users │  │Revenue│ │Sessions│                 │
│  │12.4K │  │$45.6K│  │1,892  │                 │
│  │+12.5%│  │+8.2% │  │-3.1%  │                 │
│  └──────┘  └──────┘  └──────┘                  │
├─────────────────────────────────────────────────┤
│ ┌─────────────────┐  ┌──────────────┐          │
│ │ Recent Activity │  │Quick Actions │          │
│ │ • New user...   │  │ [Add User]   │          │
│ │ • Payment...    │  │ [Report]     │          │
│ │ • Backup...     │  │ [Analytics]  │          │
│ └─────────────────┘  └──────────────┘          │
└─────────────────────────────────────────────────┘
```

**Key widgets used:**
- `Card` - Multiple cards for panels
- `Flexible` - Responsive layout
- `Row`/`Column` - Complex nested layouts
- `Divider` - Section separation
- `Button` - Action buttons
- `Container` - Spacing and decoration

**Learn:**
- Complex multi-panel layouts
- Responsive design with Flexible
- Creating reusable card components
- Building activity feeds
- Color-coded stat indicators

**Run:**
```bash
cargo run --example dashboard --features="flui_app,flui_widgets"
```

---

### 3. Photo Gallery (`photo_gallery.rs`)

**What it demonstrates:**
A responsive photo gallery with filter chips and image cards.

**Visual Layout:**
```
┌─────────────────────────────────────────┐
│ Photo Gallery                           │
│ Explore beautiful landscapes...         │
│ [All] [Landscape] [Nature] [Urban]      │
├─────────────────────────────────────────┤
│ ┌────┐ ┌────┐ ┌────┐ ┌────┐            │
│ │Img │ │Img │ │Img │ │Img │            │
│ │    │ │    │ │    │ │    │            │
│ └────┘ └────┘ └────┘ └────┘            │
│ ┌────┐ ┌────┐ ┌────┐ ┌────┐            │
│ │Img │ │Img │ │Img │ │Img │            │
│ │    │ │    │ │    │ │    │            │
│ └────┘ └────┘ └────┘ └────┘            │
└─────────────────────────────────────────┘
```

**Key widgets used:**
- `Wrap` - Responsive grid layout
- `AspectRatio` - Maintain image proportions
- `ClipRRect` - Rounded corners
- `Stack` - Layered overlays
- `Positioned` - Absolute positioning
- `Container` - Decorations and colors

**Learn:**
- Creating responsive grids with Wrap
- Maintaining aspect ratios
- Overlay effects with Stack
- Filter chips with active states
- Category labels

**Run:**
```bash
cargo run --example photo_gallery --features="flui_app,flui_widgets"
```

---

### 4. Settings Page (`settings_page.rs`)

**What it demonstrates:**
A professional settings interface with multiple sections and interactive items.

**Visual Layout:**
```
┌─────────────────────────────────┐
│ Settings                    ●●● │
│ Manage your account...          │
├─────────────────────────────────┤
│ ┌─────────────────────────────┐ │
│ │ Account                     │ │
│ │ ─────────────────────────── │ │
│ │ Email    john@ex...      >  │ │
│ │ Password ••••••••        >  │ │
│ │ 2FA      Enabled            │ │
│ └─────────────────────────────┘ │
│ ┌─────────────────────────────┐ │
│ │ Preferences                 │ │
│ │ ─────────────────────────── │ │
│ │ Language English         >  │ │
│ │ Theme    Light          >   │ │
│ └─────────────────────────────┘ │
│           [Reset] [Save Changes]│
└─────────────────────────────────┘
```

**Key widgets used:**
- `Card` - Section containers
- `Divider` - Visual organization
- `GestureDetector` - Interactive items
- `Row`/`Column` - Layout structure
- `ClipOval` - User avatar
- `Button` - Action buttons

**Learn:**
- Sectioned settings layout
- Interactive list items
- Arrow indicators for navigation
- Multiple setting categories
- Professional UI organization

**Run:**
```bash
cargo run --example settings_page --features="flui_app,flui_widgets"
```

---

### 5. Pricing Table (`pricing_table.rs`)

**What it demonstrates:**
A pricing comparison page with three tiers and feature lists.

**Visual Layout:**
```
┌─────────────────────────────────────────────────┐
│          Choose Your Plan                       │
│   Select the perfect plan for your needs        │
├─────────────────────────────────────────────────┤
│ ┌──────┐  ┌─────────────┐  ┌──────┐            │
│ │Starter│  │ MOST POPULAR│  │Enter-│            │
│ │      │  ├─────────────┤  │prise │            │
│ │ $9   │  │Professional │  │ $99  │            │
│ │/month│  │    $29      │  │/month│            │
│ │      │  │   /month    │  │      │            │
│ │──────│  │─────────────│  │──────│            │
│ │✓ 5 P │  │✓ Unlimited  │  │✓ Unl │            │
│ │✓ 10GB│  │✓ 100 GB     │  │✓ 1 TB│            │
│ │✓ Basic│ │✓ Priority   │  │✓ 24/7│            │
│ │      │  │✓ Analytics  │  │✓ SLA │            │
│ │[Plan]│  │[Get Started]│  │[Plan]│            │
│ └──────┘  └─────────────┘  └──────┘            │
└─────────────────────────────────────────────────┘
```

**Key widgets used:**
- `Card` - Pricing tier cards
- `Row` - Horizontal layout
- `Column` - Vertical content
- `Divider` - Section separation
- `Container` - Badges and decorations
- `Button` - CTA buttons

**Learn:**
- Horizontal card comparison layout
- Feature lists with checkmarks
- Badge overlays (Most Popular)
- Color-coded pricing tiers
- Professional pricing page design

**Run:**
```bash
cargo run --example pricing_table --features="flui_app,flui_widgets"
```

---

## Widget Patterns

### Layout Patterns

#### Flex Layouts (Row/Column)
Used in: All examples

```rust
Row::builder()
    .main_axis_alignment(MainAxisAlignment::SpaceBetween)
    .children(vec![widget1, widget2])
    .build()
```

**When to use:**
- Horizontal (`Row`) or vertical (`Column`) arrangements
- Need alignment control
- Flexible spacing between children

#### Stack Layouts
Used in: Photo Gallery

```rust
Stack::builder()
    .children(vec![
        Positioned::builder()
            .top(0.0)
            .child(background)
            .build()
            .into(),
        Positioned::builder()
            .bottom(0.0)
            .child(overlay)
            .build()
            .into(),
    ])
    .build()
```

**When to use:**
- Overlaying widgets
- Absolute positioning
- Creating layered effects

#### Wrap Layout
Used in: Photo Gallery

```rust
Wrap::builder()
    .spacing(16.0)
    .run_spacing(16.0)
    .children(vec![/* cards */])
    .build()
```

**When to use:**
- Responsive grids
- Dynamic content that wraps
- Unknown number of items

### Styling Patterns

#### Card Elevation
Used in: All examples

```rust
Card::builder()
    .child(content)
    .build()
```

**When to use:**
- Grouping related content
- Creating visual hierarchy
- Professional appearance

#### Rounded Corners
Used in: Profile Card, Photo Gallery, Pricing Table

```rust
ClipRRect::builder()
    .border_radius(BorderRadius::circular(12.0))
    .child(content)
    .build()
```

**When to use:**
- Modern, friendly appearance
- Image clipping
- Decorative elements

#### Circular Clipping
Used in: Profile Card, Settings Page

```rust
ClipOval::builder()
    .child(Container::builder()
        .width(100.0)
        .height(100.0)
        .build())
    .build()
```

**When to use:**
- Avatar images
- Circular icons
- Badge designs

### Composition Patterns

#### Builder Pattern
Used in: All examples

```rust
Container::builder()
    .padding(EdgeInsets::all(20.0))
    .color(Color::WHITE)
    .child(Text::builder().data("Hello").build())
    .build()
```

**Benefits:**
- Fluent, readable API
- Optional parameters
- Type-safe construction

#### Helper Functions
Used in: All examples

```rust
fn build_stat_card(title: &str, value: &str) -> Widget {
    Card::builder()
        .child(/* ... */)
        .build()
}
```

**Benefits:**
- Reusable components
- Cleaner code
- Easier maintenance

#### IntoWidget Trait
Used in: All examples

```rust
vec![
    widget1.into(),  // Automatic conversion
    widget2.into(),
    widget3.into(),
]
```

**Benefits:**
- Seamless widget conversion
- Less boilerplate
- Type safety

---

## Best Practices

### 1. Use the Macro
Always use `impl_into_widget!` for custom widgets:

```rust
#[derive(Debug, Clone)]
struct MyWidget;

flui_core::impl_into_widget!(MyWidget, stateless);
```

### 2. Builder Pattern
Prefer builders over direct construction:

```rust
// ✅ Good
Text::builder().data("Hello").size(20.0).build()

// ❌ Avoid
Text { data: "Hello".to_string(), size: 20.0, ..Default::default() }
```

### 3. Extract Helpers
Create helper functions for reusable components:

```rust
fn build_button(label: &str, color: Color) -> Widget {
    Button::builder()
        .text(label)
        .color(color)
        .build()
}
```

### 4. Use SizedBox for Spacing
Explicit spacing is clearer than padding:

```rust
Column::builder()
    .children(vec![
        widget1.into(),
        SizedBox::builder().height(16.0).build().into(),
        widget2.into(),
    ])
    .build()
```

### 5. Leverage IntoWidget
Use `.into()` for automatic conversion:

```rust
vec![
    Container::builder().build().into(),  // Widget
    Text::builder().build().into(),       // Widget
]
```

---

## Next Steps

1. **Run the examples** - See them in action
2. **Read the code** - Study the implementation
3. **Modify examples** - Experiment with changes
4. **Build your own** - Apply patterns to your project

## Resources

- [Flui Core Documentation](crates/flui_core/README.md)
- [Widget Catalog](crates/flui_widgets/README.md)
- [Examples Source Code](examples/)

---

Happy coding with Flui! 🎨✨
