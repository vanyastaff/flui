# Flui Examples

This directory contains comprehensive examples demonstrating the capabilities of the Flui UI framework.

## Quick Start

All examples require the `flui_app` and `flui_widgets` features to be enabled:

```bash
cargo run --example <example_name> --features="flui_app,flui_widgets"
```

## Examples Overview

### 🎨 Basic Examples

#### widget_hello_world.rs
The simplest Flui application demonstrating the modern builder pattern.

**Features:**
- `impl_into_widget!` macro usage
- Builder pattern for widgets
- Container, Center, and Text widgets
- Basic decoration and styling

**Run:**
```bash
cargo run --example widget_hello_world --features="flui_app,flui_widgets"
```

---

### 💼 Real-World UI Examples

#### profile_card.rs
A beautiful social media-style profile card.

**Demonstrates:**
- ✓ Card widget for elevation and professional styling
- ✓ Row and Column for flexible layout composition
- ✓ ClipOval for circular avatar images
- ✓ Divider for visual section separation
- ✓ Stats display with Row layout
- ✓ Action buttons (Follow, Message)

**Widgets Used:** `Card`, `Container`, `Column`, `Row`, `ClipOval`, `Divider`, `Button`, `Text`, `SizedBox`

**Run:**
```bash
cargo run --example profile_card --features="flui_app,flui_widgets"
```

---

#### dashboard.rs
A complete admin dashboard interface.

**Demonstrates:**
- ✓ Complex nested layouts with Row and Column
- ✓ Multiple Card widgets for information panels
- ✓ Flexible widgets for responsive design
- ✓ Stats cards with colored percentage indicators
- ✓ Activity feed with timestamps
- ✓ Quick actions panel with multiple buttons

**Widgets Used:** `Card`, `Container`, `Column`, `Row`, `Flexible`, `Divider`, `Button`, `Text`, `SizedBox`

**Run:**
```bash
cargo run --example dashboard --features="flui_app,flui_widgets"
```

---

#### photo_gallery.rs
A responsive photo gallery with filter chips.

**Demonstrates:**
- ✓ Wrap widget for responsive grid layout
- ✓ AspectRatio to maintain image proportions
- ✓ ClipRRect for rounded image corners
- ✓ Stack for layered overlay effects
- ✓ Positioned for absolute positioning within Stack
- ✓ Filter chips with active/inactive states
- ✓ Category labels with color coding

**Widgets Used:** `Wrap`, `AspectRatio`, `ClipRRect`, `Stack`, `Positioned`, `Container`, `Column`, `Row`, `Text`, `SizedBox`

**Run:**
```bash
cargo run --example photo_gallery --features="flui_app,flui_widgets"
```

---

#### settings_page.rs
A professional settings/preferences interface.

**Demonstrates:**
- ✓ Sectioned layout using Cards
- ✓ Multiple setting categories (Account, Preferences, Privacy)
- ✓ Dividers for visual organization
- ✓ GestureDetector for interactive list items
- ✓ Arrow indicators for navigable items
- ✓ User avatar in header
- ✓ Save and reset action buttons

**Widgets Used:** `Card`, `Container`, `Column`, `Row`, `Divider`, `GestureDetector`, `ClipOval`, `Button`, `Text`, `SizedBox`

**Run:**
```bash
cargo run --example settings_page --features="flui_app,flui_widgets"
```

---

#### pricing_table.rs
A pricing comparison page with multiple tiers.

**Demonstrates:**
- ✓ Horizontal card layout for comparison
- ✓ Feature lists with checkmark icons
- ✓ "Most Popular" badge overlay
- ✓ Different pricing tiers (Starter, Professional, Enterprise)
- ✓ Color-coded pricing cards
- ✓ Professional pricing page design
- ✓ Responsive card sizing

**Widgets Used:** `Card`, `Container`, `Column`, `Row`, `Divider`, `Button`, `Text`, `SizedBox`

**Run:**
```bash
cargo run --example pricing_table --features="flui_app,flui_widgets"
```

---

## Widget Patterns Demonstrated

### Layout Patterns
- **Flex Layouts**: Using `Row` and `Column` for flexible positioning
- **Stack Layouts**: Layering widgets with `Stack` and `Positioned`
- **Responsive Grids**: Using `Wrap` for adaptive layouts
- **Aspect Ratios**: Maintaining proportions with `AspectRatio`

### Styling Patterns
- **Cards**: Elevated surfaces with `Card` widget
- **Rounded Corners**: Using `ClipRRect` and `BorderRadius`
- **Circular Clipping**: Using `ClipOval` for avatars
- **Decorations**: `BoxDecoration` with colors and borders
- **Spacing**: `SizedBox`, `Padding`, and `EdgeInsets`

### Composition Patterns
- **Builder Pattern**: Fluent API for widget construction
- **Helper Functions**: Extracting reusable widget builders
- **IntoWidget**: Automatic widget conversion with `.into()`
- **Macro Usage**: `impl_into_widget!` for clean integration

### Interactive Patterns
- **Buttons**: Action buttons with `Button` widget
- **Gesture Detection**: Using `GestureDetector` for interactions
- **Visual States**: Active/inactive states with color changes

## Code Style

All examples follow modern Flui best practices:

1. **Use the macro**: Every custom widget uses `flui_core::impl_into_widget!()`
2. **Builder pattern**: Prefer `Widget::builder()` over direct construction
3. **IntoWidget**: Use `.into()` for automatic Widget conversion
4. **Helper functions**: Extract reusable components into helper functions
5. **Type safety**: Leverage Rust's type system for compile-time checks

## Example Template

```rust
use flui_app::run_app;
use flui_core::{BuildContext, IntoWidget, StatelessWidget, Widget};
use flui_widgets::prelude::*;

#[derive(Debug, Clone)]
struct MyApp;

flui_core::impl_into_widget!(MyApp, stateless);

impl StatelessWidget for MyApp {
    fn build(&self, _ctx: &BuildContext) -> Widget {
        Container::builder()
            .child(
                Text::builder()
                    .data("Hello, Flui!")
                    .build()
            )
            .build()
    }
}

fn main() -> Result<(), eframe::Error> {
    run_app(MyApp.into_widget())
}
```

## Contributing

Want to add more examples? Great! Please ensure:
- Examples are well-documented
- Code follows the established patterns
- Examples demonstrate real-world use cases
- Include comments explaining key concepts

## Learn More

- [Flui Documentation](../crates/flui_core/README.md)
- [Widget Catalog](../crates/flui_widgets/README.md)
- [Core Concepts](../docs/)
