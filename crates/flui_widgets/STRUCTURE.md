# flui_widgets - Project Structure

This document describes the organization of the `flui_widgets` crate.

## Directory Structure

```
crates/flui_widgets/
├── Cargo.toml
├── README.md              # Crate overview and usage examples
├── CHANGELOG.md           # Version history and changes
├── WIDGET_GUIDELINES.md   # Guide for implementing new widgets
├── WIDGET_TEMPLATE.rs     # Template for new widget implementations
├── STRUCTURE.md           # This file
└── src/
    ├── lib.rs             # Crate root with re-exports and prelude
    ├── basic/             # Basic single-child layout widgets
    │   ├── mod.rs
    │   ├── align.rs       # Flexible alignment widget
    │   ├── center.rs      # Center alignment widget
    │   ├── container.rs   # Convenience widget combining multiple properties
    │   ├── padding.rs     # Padding wrapper widget
    │   └── sized_box.rs   # Fixed-size box widget
    └── layout/            # Multi-child layout widgets
        ├── mod.rs
        ├── column.rs      # Vertical flex layout
        └── row.rs         # Horizontal flex layout
```

## Module Organization

### `lib.rs`
- Root module with crate-level documentation
- Re-exports all widgets for convenient top-level access
- `prelude` module with common imports

### `basic/` - Basic Widgets
Single-child widgets for fundamental layout operations:
- **Container** (19 tests) - Combines sizing, padding, decoration, constraints
- **SizedBox** (18 tests) - Fixed dimensions
- **Padding** (11 tests) - Insets child by padding
- **Center** (11 tests) - Centers child within available space
- **Align** (17 tests) - Flexible child positioning

**Total: 76 tests**

### `layout/` - Layout Widgets
Multi-child widgets for complex layouts:
- **Row** (13 tests) - Horizontal flex layout
- **Column** (13 tests) - Vertical flex layout

**Total: 26 tests**

## Future Categories

### `visual/` - Visual Effects (Planned)
- Opacity - transparency control
- Transform - 2D/3D transformations
- ClipRRect - rounded rectangle clipping
- DecoratedBox - decoration wrapper

### `flex/` - Flex Children (Planned)
- Expanded - fills available space in flex
- Flexible - flexible space distribution
- Spacer - empty spacing widget

### `scrolling/` - Scrollable Widgets (Planned)
- ScrollView - scrollable container
- ListView - scrollable list
- GridView - scrollable grid

### `input/` - Input Widgets (Future)
- Button - clickable button
- TextField - text input
- Checkbox - checkbox input
- Radio - radio button

### `text/` - Text Widgets (Future)
- Text - simple text rendering
- RichText - styled text rendering

## Widget Implementation Pattern

All widgets follow a consistent pattern:

1. **Three syntax styles**:
   - Struct literal: `Widget { field: value, ..Default::default() }`
   - Builder pattern: `Widget::builder().field(value).build()`
   - Declarative macro: `widget! { field: value }`

2. **bon Builder integration**:
   - Custom finish function: `finish_fn = build_widget_name`
   - Private child setter: `setters(vis = "", name = child_internal)`
   - Public child wrapper with type safety

3. **Standard methods**:
   - `new()` - Create with defaults
   - `set_child()` - Add child (if applicable)
   - `validate()` - Configuration validation
   - Factory methods for common patterns

4. **Testing**:
   - Minimum 10-15 tests per widget
   - Coverage: new, default, builder, macro, validation
   - Edge cases and boundary conditions

## Import Patterns

### For Library Users

```rust
// Option 1: Use prelude for everything
use flui_widgets::prelude::*;

// Option 2: Import specific widgets
use flui_widgets::{Container, Row, Column};

// Option 3: Use module paths
use flui_widgets::basic::Container;
use flui_widgets::layout::{Row, Column};
```

### For Widget Developers

When implementing new widgets, place them in the appropriate category:
- Single-child basic widgets → `basic/`
- Multi-child layout widgets → `layout/`
- Visual effects → `visual/` (create when needed)
- Scrolling widgets → `scrolling/` (create when needed)

Update:
1. Add module file (e.g., `basic/my_widget.rs`)
2. Add to category `mod.rs` (e.g., `basic/mod.rs`)
3. Re-export in category module
4. Widget is automatically available via prelude and top-level

## Statistics (Current)

- **7 widgets implemented**
- **102 tests** (all passing)
- **~2,500 lines of code**
- **0 clippy warnings**
- **100% documented**

## Guidelines

See [WIDGET_GUIDELINES.md](./WIDGET_GUIDELINES.md) for detailed implementation guidelines.

Use [WIDGET_TEMPLATE.rs](./WIDGET_TEMPLATE.rs) as a starting point for new widgets.
