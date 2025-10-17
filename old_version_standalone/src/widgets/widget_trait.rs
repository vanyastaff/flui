//! WidgetExt trait - Extension trait for egui widgets
//!
//! This module defines the WidgetExt extension trait that adds nebula-ui capabilities
//! to all egui::Widget implementations. This provides validation, debugging, and sizing
//! hints without creating a separate widget type.
//!
//! # Key Field Pattern
//!
//! All widgets should include an optional `key: Option<egui::Id>` field for state persistence.
//! This integrates beautifully with bon builders:
//!
//! ```ignore
//! Container::builder()
//!     .key("my_container")  // Optional ID for state persistence
//!     .width(300.0)
//!     .ui(ui);
//! ```
//!
//! # Closure Widgets
//!
//! Following egui's pattern, closures can be used as widgets via blanket implementation.
//! This enables functions that return `impl Widget`:
//!
//! ```ignore
//! // Define a widget function that returns a closure
//! pub fn custom_slider(value: &mut f32) -> impl egui::Widget + '_ {
//!     move |ui: &mut egui::Ui| {
//!         ui.horizontal(|ui| {
//!             ui.label("Value:");
//!             ui.add(egui::Slider::new(value, 0.0..=1.0));
//!         })
//!         .response
//!     }
//! }
//!
//! // Use it like any other widget
//! ui.add(custom_slider(&mut my_value));
//! ```
//!
//! # Widget vs Controller
//!
//! | Aspect | Widget | Controller |
//! |--------|--------|------------|
//! | Ownership | `self` (move) | `&mut self` (borrow) |
//! | Lifetime | One frame | Multiple frames |
//! | Pattern | Declarative UI | Imperative state |
//! | Example | Container, Row | AnimationController |
//!
//! See `docs/WIDGET_VS_CONTROLLER.md` for detailed comparison.

/// WidgetExt - Extension trait for egui::Widget
///
/// Adds nebula-ui capabilities to all egui widgets:
/// - Validation before rendering
/// - Debug visualization overlays
/// - Size hints for layout optimization
/// - Custom ID management
///
/// # Example
/// ```ignore
/// use nebula_ui::widgets::WidgetExt;
///
/// Container::builder()
///     .width(300.0)
///     .color(Color::BLUE)
///     .build()?  // Validates configuration
///     .ui(ui);   // egui::Widget::ui() method
/// ```
pub trait WidgetExt: egui::Widget + Sized {
    /// Optional widget ID for state persistence.
    ///
    /// If provided, egui will use this ID to persist state across frames.
    /// This is useful for stateful widgets like:
    /// - Scroll areas that need to remember position
    /// - Collapsing headers that remember their state
    /// - Text inputs that need to track focus
    ///
    /// **Note:** Most widgets should expose a `key` field in their struct
    /// and return it here. This is more ergonomic than overriding this method.
    ///
    /// # Example
    /// ```ignore
    /// // For widgets with key field (preferred):
    /// pub struct Container {
    ///     pub key: Option<egui::Id>,
    ///     // ... other fields
    /// }
    ///
    /// impl WidgetExt for Container {
    ///     fn id(&self) -> Option<egui::Id> {
    ///         self.key  // Simply return the key field
    ///     }
    /// }
    ///
    /// // Usage:
    /// Container::builder()
    ///     .key("my_container")  // Clean bon builder API!
    ///     .ui(ui);
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
    ///                 let total_w = w + self.padding.horizontal_total() + self.margin.horizontal_total();
    ///                 let total_h = h + self.padding.vertical_total() + self.margin.vertical_total();
    ///                 Some(egui::vec2(total_w, total_h))
    ///             }
    ///             _ => None,  // Size depends on child or available space
    ///         }
    ///     }
    /// }
    /// ```
    ///
    /// See `docs/SIZE_HINT_QUICK_START.md` for detailed guide.
    fn size_hint(&self, _ui: &egui::Ui) -> Option<egui::Vec2> {
        None
    }

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
    ///     .build_ui(ui)?;  // Returns Result
    ///
    /// // This will fail validation
    /// Container::builder()
    ///     .width(-100.0)  // Invalid!
    ///     .build_ui(ui)?;  // Returns Err("Width cannot be negative")
    /// ```
    fn build_ui(self, ui: &mut egui::Ui) -> Result<egui::Response, String>
    where
        Self: Sized,
    {
        self.validate()?;
        Ok(self.ui(ui))
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

/// Widget wrapper that adds debug visualization.
///
/// Only available in debug builds. Created by calling `.with_debug()`.
#[cfg(debug_assertions)]
pub struct WithDebug<W: WidgetExt> {
    widget: W,
}

#[cfg(debug_assertions)]
impl<W: WidgetExt> egui::Widget for WithDebug<W> {
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
            egui::CornerRadius::ZERO,
            egui::Stroke::new(1.0, egui::Color32::RED),
            egui::epaint::StrokeKind::Outside,
        );

        // Widget name and size info at top-left
        let mut debug_text = name.to_string();
        if let Some(hint) = size_hint {
            debug_text.push_str(&format!("\nHint: {:.0}×{:.0}", hint.x, hint.y));
        }
        debug_text.push_str(&format!(
            "\nActual: {:.0}×{:.0}",
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
}

#[cfg(debug_assertions)]
impl<W: WidgetExt> WidgetExt for WithDebug<W> {
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
