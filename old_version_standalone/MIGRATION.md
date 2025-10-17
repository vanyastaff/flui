# Migration Guide

## Removed Utilities (utils module)

The `utils` module has been removed as all functionality is now available in core types with better implementations.

### Migration Table

| Old (utils)                           | New (core types)                          | Location                |
|---------------------------------------|-------------------------------------------|-------------------------|
| `utils::rect_center(rect)`           | `rect.center()`                           | `types::core::Rect`     |
| `utils::lerp_color(from, to, t)`     | `from.lerp(to, t)`                        | `types::core::Color`    |
| `utils::bezier_point(p0,p1,p2,p3,t)` | `CubicBezier::new(...).at(t)`            | `types::core::path`     |
| `utils::format_shortcut(mods, key)`  | Not migrated (egui-specific utility)     | N/A                     |

### Examples

#### Before (using utils)
```rust
use nebula_ui::utils;

let center = utils::rect_center(rect);
let color = utils::lerp_color(Color32::RED, Color32::BLUE, 0.5);
let point = utils::bezier_point(p0, p1, p2, p3, 0.5);
```

#### After (using core types)
```rust
use nebula_ui::types::core::{Rect, Color, Point, path::CubicBezier};

// Rect center
let center = rect.center();

// Color interpolation
let color = Color::RED.lerp(Color::BLUE, 0.5);

// Bezier curve
let bezier = CubicBezier {
    start: p0,
    control1: p1,
    control2: p2,
    end: p3,
};
let point = bezier.at(0.5);
```

### Why This Change?

1. **No Duplication** - Functionality now lives in the types themselves
2. **Better API** - Methods on types are more discoverable
3. **Type Safety** - Core types provide stronger guarantees
4. **Consistency** - All geometry/color operations in one place
5. **Performance** - Same performance, better ergonomics

### format_shortcut Alternative

If you need keyboard shortcut formatting, you can implement it directly:

```rust
pub fn format_shortcut(modifiers: egui::Modifiers, key: egui::Key) -> String {
    let mut parts = Vec::new();
    if modifiers.ctrl { parts.push("Ctrl"); }
    if modifiers.shift { parts.push("Shift"); }
    if modifiers.alt { parts.push("Alt"); }
    parts.push(&format!("{:?}", key));
    parts.join("+")
}
```

Or use egui's built-in `KeyboardShortcut::format()` method.
