# Enhanced Widget Trait - Proposal

## Current State

Сейчас Widget трейт минималистичен:

```rust
pub trait Widget {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response;
}
```

## Proposed Enhanced Widget Trait

Вдохновлено Flutter Widget API, но адаптировано для Rust/egui:

```rust
/// Core widget trait for nebula-ui.
///
/// This trait defines the interface for all widgets in the framework.
/// Inspired by Flutter's Widget, but adapted for Rust and egui's immediate mode paradigm.
pub trait Widget: Sized {
    /// Render the widget to the UI and return the response.
    ///
    /// This is the main method that every widget must implement.
    /// The widget consumes itself (moves ownership) during rendering,
    /// which is typical for immediate mode GUIs.
    ///
    /// # Example
    /// ```ignore
    /// Container::builder()
    ///     .width(300.0)
    ///     .color(Color::BLUE)
    ///     .ui(ui);
    /// ```
    fn ui(self, ui: &mut egui::Ui) -> egui::Response;

    // ═══════════════════════════════════════════════════════════════════
    // Optional methods with default implementations
    // ═══════════════════════════════════════════════════════════════════

    /// Optional widget ID for state persistence.
    ///
    /// If provided, egui will use this ID to persist state across frames.
    /// This is useful for stateful widgets like:
    /// - Animations that need to track progress
    /// - Scroll areas that need to remember position
    /// - Collapsing headers that remember their state
    /// - Text inputs that need to track focus
    ///
    /// # Example
    /// ```ignore
    /// impl Widget for MyStatefulWidget {
    ///     fn id(&self) -> Option<egui::Id> {
    ///         Some(egui::Id::new("my_widget_unique_id"))
    ///     }
    /// }
    /// ```
    fn id(&self) -> Option<egui::Id> {
        None
    }

    /// Validate widget configuration before rendering.
    ///
    /// Called to check if the widget has valid configuration.
    /// Returns `Ok(())` if valid, or `Err(message)` with error description.
    ///
    /// Default implementation always returns `Ok(())`.
    ///
    /// # Example
    /// ```ignore
    /// impl Widget for Container {
    ///     fn validate(&self) -> Result<(), String> {
    ///         if let Some(width) = self.width {
    ///             if width < 0.0 {
    ///                 return Err("Width cannot be negative".to_string());
    ///             }
    ///         }
    ///         Ok(())
    ///     }
    /// }
    /// ```
    fn validate(&self) -> Result<(), String> {
        Ok(())
    }

    /// Get widget's type name for diagnostics and debugging.
    ///
    /// Returns the full type name by default (e.g., "nebula_ui::widgets::Container").
    /// Widgets can override this to provide a shorter, more readable name.
    ///
    /// # Example
    /// ```ignore
    /// impl Widget for Container {
    ///     fn debug_name(&self) -> &'static str {
    ///         "Container"  // Override for cleaner output
    ///     }
    /// }
    /// ```
    fn debug_name(&self) -> &'static str {
        std::any::type_name::<Self>()
    }

    /// Get optional size hint for layout optimization.
    ///
    /// If the widget knows its desired size in advance, it can return it here.
    /// This helps egui and parent widgets optimize layout calculations.
    ///
    /// Returns `None` by default (size unknown until rendering).
    ///
    /// # When to return Some:
    /// - Widget has fixed dimensions (width + height)
    /// - Widget has minimum constraints
    /// - Widget can measure itself without rendering (e.g., text)
    ///
    /// # When to return None:
    /// - Widget size depends on children
    /// - Widget size depends on available space
    /// - Widget size depends on content not yet known
    ///
    /// # Example
    /// ```ignore
    /// impl Widget for Container {
    ///     fn size_hint(&self, ui: &egui::Ui) -> Option<egui::Vec2> {
    ///         // If we have fixed size, return it
    ///         match (self.width, self.height) {
    ///             (Some(w), Some(h)) => {
    ///                 // Add padding and margin
    ///                 let total_w = w + self.padding.horizontal() + self.margin.horizontal();
    ///                 let total_h = h + self.padding.vertical() + self.margin.vertical();
    ///                 Some(egui::vec2(total_w, total_h))
    ///             }
    ///             _ => None,  // Size depends on child or available space
    ///         }
    ///     }
    /// }
    /// ```
    ///
    /// See `docs/WIDGET_TRAIT_SIZE_HINT.md` for detailed guide.
    fn size_hint(&self, _ui: &egui::Ui) -> Option<egui::Vec2> {
        None
    }

    // ═══════════════════════════════════════════════════════════════════
    // Convenience methods (provided by default)
    // ═══════════════════════════════════════════════════════════════════

    /// Build and render with validation (convenience method).
    ///
    /// Validates the widget configuration before rendering.
    /// Returns `Ok(response)` if valid, or `Err(message)` if validation fails.
    ///
    /// # Example
    /// ```ignore
    /// // This will validate and render
    /// Container::builder()
    ///     .width(300.0)
    ///     .color(Color::BLUE)
    ///     .build(ui)?;  // Returns Result
    ///
    /// // This will fail validation
    /// Container::builder()
    ///     .width(-100.0)  // Invalid!
    ///     .build(ui)?;  // Returns Err("Width cannot be negative")
    /// ```
    fn build(self, ui: &mut egui::Ui) -> Result<egui::Response, String> {
        self.validate()?;
        Ok(self.ui(ui))
    }

    /// Render with custom ID (convenience method).
    ///
    /// Wraps the widget in a `WithId` wrapper that overrides the widget's ID.
    /// Useful for when you need to ensure unique IDs for repeated widgets.
    ///
    /// # Example
    /// ```ignore
    /// // Render multiple similar widgets with unique IDs
    /// for (i, item) in items.iter().enumerate() {
    ///     Container::builder()
    ///         .width(200.0)
    ///         .color(item.color)
    ///         .with_id(format!("item_{}", i))
    ///         .ui(ui);
    /// }
    /// ```
    fn with_id(self, id: impl Into<egui::Id>) -> WithId<Self> {
        WithId {
            widget: self,
            id: id.into(),
        }
    }

    /// Render with debug visualization overlay (convenience method for development).
    ///
    /// Only available in debug builds. Draws a red border around the widget
    /// and shows its type name. Useful for understanding layout and debugging.
    ///
    /// # Example
    /// ```ignore
    /// Container::builder()
    ///     .width(300.0)
    ///     .with_debug()  // Shows border + name overlay
    ///     .ui(ui);
    /// ```
    #[cfg(debug_assertions)]
    fn with_debug(self) -> WithDebug<Self> {
        WithDebug { widget: self }
    }
}

// ═══════════════════════════════════════════════════════════════════
// Helper wrapper types for convenience methods
// ═══════════════════════════════════════════════════════════════════

/// Widget wrapper that overrides the widget's ID.
///
/// Created by calling `.with_id()` on any widget.
pub struct WithId<W: Widget> {
    widget: W,
    id: egui::Id,
}

impl<W: Widget> Widget for WithId<W> {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        // Use egui's push_id to create a new ID scope
        ui.push_id(self.id, |ui| self.widget.ui(ui)).inner
    }

    fn id(&self) -> Option<egui::Id> {
        Some(self.id)
    }

    // Forward other methods to inner widget
    fn validate(&self) -> Result<(), String> {
        self.widget.validate()
    }

    fn debug_name(&self) -> &'static str {
        self.widget.debug_name()
    }

    fn size_hint(&self, ui: &egui::Ui) -> Option<egui::Vec2> {
        self.widget.size_hint(ui)
    }
}

/// Widget wrapper that adds debug visualization.
///
/// Only available in debug builds. Created by calling `.with_debug()`.
#[cfg(debug_assertions)]
pub struct WithDebug<W: Widget> {
    widget: W,
}

#[cfg(debug_assertions)]
impl<W: Widget> Widget for WithDebug<W> {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let name = self.widget.debug_name();
        let size_hint = self.widget.size_hint(ui);

        // Render the actual widget
        let response = self.widget.ui(ui);

        // Draw debug overlay
        let painter = ui.painter();

        // Red border around widget
        painter.rect_stroke(
            response.rect,
            0.0,
            egui::Stroke::new(1.0, egui::Color32::RED),
        );

        // Widget name at top-left
        let mut debug_text = name.to_string();
        if let Some(hint) = size_hint {
            debug_text.push_str(&format!("\nHint: {:.0}x{:.0}", hint.x, hint.y));
        }
        debug_text.push_str(&format!(
            "\nActual: {:.0}x{:.0}",
            response.rect.width(),
            response.rect.height()
        ));

        painter.text(
            response.rect.left_top(),
            egui::Align2::LEFT_TOP,
            debug_text,
            egui::FontId::monospace(10.0),
            egui::Color32::RED,
        );

        response
    }

    fn id(&self) -> Option<egui::Id> {
        self.widget.id()
    }

    fn validate(&self) -> Result<(), String> {
        self.widget.validate()
    }

    fn debug_name(&self) -> &'static str {
        self.widget.debug_name()
    }

    fn size_hint(&self, ui: &egui::Ui) -> Option<egui::Vec2> {
        self.widget.size_hint(ui)
    }
}
```

---

## Usage Examples

### 1. Basic usage (unchanged)

```rust
Container::builder()
    .width(300.0)
    .color(Color::BLUE)
    .ui(ui);
```

### 2. With validation

```rust
// Will validate before rendering
match Container::builder()
    .width(-100.0)  // Invalid!
    .build(ui)
{
    Ok(response) => { /* success */ }
    Err(e) => { ui.colored_label(Color::RED, format!("Error: {}", e)); }
}
```

### 3. With custom ID

```rust
// Useful for dynamic lists
for (i, item) in items.iter().enumerate() {
    Container::colored(item.color)
        .with_id(format!("item_{}", i))
        .ui(ui);
}
```

### 4. With debug overlay (debug builds only)

```rust
#[cfg(debug_assertions)]
{
    Container::builder()
        .width(300.0)
        .height(200.0)
        .with_debug()  // Shows border + size info
        .ui(ui);
}
```

### 5. Size hints for optimization

```rust
// Parent widget can check size before rendering
if let Some(size) = child_widget.size_hint(ui) {
    // Pre-allocate space or adjust layout
    ui.allocate_space(size);
}

child_widget.ui(ui);
```

---

## Implementation for Container

```rust
impl Widget for Container {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        // Existing implementation...
    }

    fn validate(&self) -> Result<(), String> {
        // Existing implementation...
    }

    fn debug_name(&self) -> &'static str {
        "Container"  // Clean name instead of full path
    }

    fn size_hint(&self, ui: &egui::Ui) -> Option<egui::Vec2> {
        // Has fixed size?
        let has_width = self.width.is_some() || self.min_width.is_some();
        let has_height = self.height.is_some() || self.min_height.is_some();

        if !has_width && !has_height {
            return None;  // Size depends on child
        }

        // Calculate with very large available space
        let large = egui::vec2(f32::INFINITY, f32::INFINITY);
        let content = self.calculate_size(large);

        if content.x.is_infinite() || content.y.is_infinite() {
            return None;
        }

        // Add padding and margin
        Some(egui::vec2(
            content.x + self.padding.horizontal() + self.margin.horizontal(),
            content.y + self.padding.vertical() + self.margin.vertical(),
        ))
    }
}
```

---

## Comparison with Flutter

### Flutter Widget methods:
```dart
class Widget {
  Element createElement();  // ❌ Not needed (no element tree in egui)
  bool canUpdate();         // ❌ Not needed (immediate mode)
  List<DiagnosticsNode> debugDescribeChildren();  // ❌ Too complex
  void debugFillProperties();  // ✅ Partially (our debug_name())
  String toStringShort();   // ✅ (our debug_name())
}
```

### Our Widget methods:
```rust
trait Widget {
    fn ui(self, ui: &mut egui::Ui) -> Response;  // ✅ Core method
    fn id(&self) -> Option<egui::Id>;  // ✅ State persistence
    fn validate(&self) -> Result<(), String>;  // ✅ Configuration validation
    fn debug_name(&self) -> &'static str;  // ✅ Debugging
    fn size_hint(&self, ui: &egui::Ui) -> Option<Vec2>;  // ✅ Layout optimization
    fn build(self, ui: &mut egui::Ui) -> Result<Response>;  // ✅ Convenience
    fn with_id(self, id: impl Into<egui::Id>) -> WithId<Self>;  // ✅ Convenience
    fn with_debug(self) -> WithDebug<Self>;  // ✅ Debug visualization
}
```

---

## Benefits

1. ✅ **Backward compatible** - default implementations for all new methods
2. ✅ **Opt-in features** - widgets use only what they need
3. ✅ **Layout optimization** - `size_hint()` helps parent widgets
4. ✅ **Better debugging** - `.with_debug()` visualizes layout issues
5. ✅ **State management** - `id()` enables persistent state
6. ✅ **Validation** - catch errors before rendering
7. ✅ **Zero-cost abstractions** - all inline, no runtime overhead

---

## Migration Path

### Phase 1: Add trait to crate (backward compatible)
- Add enhanced Widget trait
- All existing code continues to work
- No breaking changes

### Phase 2: Add helper methods to types
- Add `horizontal()` and `vertical()` to EdgeInsets
- Update Container to implement new methods

### Phase 3: Update examples
- Show `.with_debug()` in debug examples
- Show `.with_id()` for stateful widgets
- Show `size_hint()` for layout optimization

### Phase 4: Documentation
- Add examples to docs
- Add tutorial on writing custom widgets
- Add guide on layout optimization

---

## Next Steps

Хочешь, чтобы я:

1. ✅ Создал полный файл с трейтом?
2. ✅ Имплементировал для Container?
3. ✅ Добавил тесты?
4. ✅ Создал example с демонстрацией всех возможностей?

Дай знать, что реализовать!
