# nebula-ui

Core UI components and controllers for the Nebula workflow system. This crate provides reusable UI building blocks that are used by higher-level UI crates.

## Architecture

nebula-ui serves as the foundation for all UI functionality in Nebula:

```
nebula-ui (this crate)
    ↓
nebula-parameter-ui (widgets for parameters)
    ↓
nebula-node-ui (node editor components)
    ↓
nebula-workflow-ui (workflow visualization)
    ↓
nebula-app (final application)
```

## Features

### Controllers

The crate provides Flutter-inspired controllers for managing UI state:

- **AnimationController**: Manages animations with curves (Linear, EaseIn, EaseOut, Bounce, Elastic)
- **ValidationController**: Handles validation state with debouncing and error display
- **FocusController**: Tracks focus, hover, and interaction states
- **VisibilityController**: Controls visibility with fade/collapse animations
- **ChangeTracker**: Implements undo/redo functionality and dirty state tracking
- **InputController**: Manages text input with editing modes and masks
- **ThemeController**: Manages application themes with transitions, custom themes, and persistence

### Theme System

- Dark and light themes
- Customizable color palettes
- Spacing and typography configuration
- Animation timing settings

### Utilities

- Color interpolation
- Bezier curve calculations
- Rectangle center calculation
- Keyboard shortcut formatting

## Usage

```rust
use nebula_ui::prelude::*;
use std::time::Duration;

// Create controllers
let mut animation = AnimationController::new(Duration::from_millis(300))
    .with_curve(AnimationCurve::EaseInOut);

let mut focus = FocusController::default();

let mut visibility = VisibilityController::new()
    .with_hide_mode(HideMode::Fade);

// Theme controller with transitions and custom themes
let mut theme_controller = ThemeController::new()
    .with_transition(ThemeTransition::Fade(Duration::from_millis(300)))
    .with_persistence("app_theme");

// Register custom theme
let custom = ThemeBuilder::dark()
    .primary(egui::Color32::from_rgb(139, 92, 246))
    .secondary(egui::Color32::from_rgb(236, 72, 153))
    .build();
theme_controller.register_theme("Purple", custom);

// Use in egui
animation.toggle();
let progress = animation.tick();

visibility.apply(ui, |ui| {
    ui.label("This content can fade in/out");
});

// Apply theme with transitions
theme_controller.apply(ctx);
```

## Examples

Run the demo to see all controllers in action:

```bash
cargo run --example demo
```

## Design Philosophy

- **Fixed Design**: The design is controlled by the library developer, not customizable by end users
- **Composition**: Build complex UIs by combining simple, reusable controllers
- **Performance**: Efficient state management with minimal allocations
- **Type Safety**: Strong typing with Rust's type system
- **No Circular Dependencies**: Clear hierarchy prevents circular dependencies between UI crates