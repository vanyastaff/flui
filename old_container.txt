//! Container widget - a box with decoration, padding, and size constraints.
//!
//! Similar to Flutter's Container, this is the most versatile widget for layout and styling.
//!
//! # Example
//!
//! ```rust,no_run
//! use nebula_ui::widgets::primitives::Container;
//! use nebula_ui::widgets::WidgetExt;
//! use nebula_ui::types::styling::BoxDecoration;
//! use nebula_ui::types::core::Color;
//! use nebula_ui::types::layout::EdgeInsets;
//!
//! # fn example(ui: &mut egui::Ui) {
//! // Simple colored box using bon builder
//! Container::builder()
//!     .decoration(BoxDecoration::new().with_color(Color::from_rgb(200, 200, 255)))
//!     .padding(EdgeInsets::all(16.0))
//!     .min_width(100.0)
//!     .min_height(50.0)
//!     .child(|ui| { ui.label("Hello!") })
//!     .ui(ui);
//! # }
//! ```

use bon::Builder;
use crate::types::core::{Color, Transform};
use crate::types::layout::{EdgeInsets, Alignment, BoxConstraints};
use crate::types::styling::{BoxDecoration, Clip};
use crate::painters::{DecorationPainter, TransformPainter};
use egui::{self, Sense};
use crate::widgets::WidgetExt;

/// A widget that combines common painting, positioning, and sizing widgets.
///
/// A container first applies transform, then margin, then decoration, then padding,
/// then constraints, and finally renders its aligned child.
///
/// ## Usage Patterns
///
/// Container supports two main creation styles:
///
/// ### 1. Struct Literal (Flutter-like - for simple cases)
/// ```ignore
/// Container {
///     width: Some(300.0),
///     height: Some(200.0),
///     padding: EdgeInsets::all(20.0),
///     ..Default::default()
/// }.ui(ui);
/// ```
///
/// ### 2. bon Builder (Type-safe - for complex cases)
/// ```ignore
/// Container::builder()
///     .width(300.0)
///     .height(200.0)
///     .padding(EdgeInsets::all(20.0))
///     .child(|ui| { ui.label("Hello") })
///     .ui(ui);  // ← Builds and renders in one step!
/// ```
///
/// ### 3. Factory Methods (for common patterns)
/// ```ignore
/// // Factory methods return Container - use bon builder or struct fields to extend
/// let mut container = Container::colored(Color::BLUE);
/// container.width = Some(300.0);
/// container.ui(ui);
///
/// // Or use bon builder:
/// Container::builder()
///     .color(Color::BLUE)
///     .width(300.0)
///     .ui(ui);
/// ```
#[derive(Builder)]
#[builder(
    on(EdgeInsets, into),
    on(BoxDecoration, into),
    on(Color, into),
    finish_fn(vis = "", name = build_internal)  // Make standard build private
)]
pub struct Container {
    /// Optional key for widget identification and state persistence
    ///
    /// When provided, egui will use this ID to persist state across frames.
    /// This is useful for maintaining scroll position, focus state, etc.
    ///
    /// # Example
    /// ```ignore
    /// Container::builder()
    ///     .key("my_container")
    ///     .child(|ui| { ui.label("Stateful widget") })
    ///     .ui(ui);
    /// ```
    #[builder(into)]
    pub key: Option<egui::Id>,

    /// Optional decoration (background, border, shadows, etc.)
    pub decoration: Option<BoxDecoration>,

    /// Optional foreground decoration (painted over the child)
    pub foreground_decoration: Option<BoxDecoration>,

    /// Shorthand for setting decoration color
    pub color: Option<Color>,

    /// Padding around the child (inside decoration)
    #[builder(default = EdgeInsets::ZERO)]
    pub padding: EdgeInsets,

    /// Margin around the container (outside decoration)
    #[builder(default = EdgeInsets::ZERO)]
    pub margin: EdgeInsets,

    /// Alignment of the child within the container
    pub alignment: Option<Alignment>,

    /// Box constraints for size
    pub constraints: Option<BoxConstraints>,

    /// Minimum width constraint (merged into constraints)
    pub min_width: Option<f32>,

    /// Maximum width constraint (merged into constraints)
    pub max_width: Option<f32>,

    /// Minimum height constraint (merged into constraints)
    pub min_height: Option<f32>,

    /// Maximum height constraint (merged into constraints)
    pub max_height: Option<f32>,

    /// Fixed width (overrides min/max)
    pub width: Option<f32>,

    /// Fixed height (overrides min/max)
    pub height: Option<f32>,

    /// Transform matrix (rotation, scale, translation)
    pub transform: Option<Transform>,

    /// Alignment of the origin for transform
    pub transform_alignment: Option<Alignment>,

    /// Clipping behavior
    #[builder(default = Clip::None)]
    pub clip_behavior: Clip,

    /// Child rendering function (use manual .child() method or bon builder setter)
    /// Note: bon builder smart setter allows `.child()` directly in builder chain
    #[builder(setters(vis = "", name = child_internal))]
    pub child: Option<Box<dyn FnOnce(&mut egui::Ui) -> egui::Response>>,
}

impl Container {
    /// Create a new empty container.
    pub fn new() -> Self {
        Self {
            key: None,
            decoration: None,
            foreground_decoration: None,
            color: None,
            padding: EdgeInsets::ZERO,
            margin: EdgeInsets::ZERO,
            alignment: None,
            constraints: None,
            min_width: None,
            max_width: None,
            min_height: None,
            max_height: None,
            width: None,
            height: None,
            transform: None,
            transform_alignment: None,
            clip_behavior: Clip::None,
            child: None,
        }
    }

    /// Create a container with a solid color background.
    ///
    /// This is a convenient shorthand for creating a colored box.
    ///
    /// # Example
    /// ```ignore
    /// Container::colored(Color::BLUE)
    ///     .padding(10.0)
    ///     .child(|ui| { ui.label("Blue box") })
    ///     .ui(ui);
    /// ```
    pub fn colored(color: impl Into<Color>) -> Self {
        let mut container = Self::new();
        container.color = Some(color.into());
        container
    }

    /// Create a container with a border.
    ///
    /// Creates a container with only a border decoration, no background fill.
    ///
    /// # Example
    /// ```ignore
    /// Container::bordered(2.0, Color::RED)
    ///     .padding(15.0)
    ///     .child(|ui| { ui.label("Bordered") })
    ///     .ui(ui);
    /// ```
    pub fn bordered(border_width: f32, border_color: impl Into<Color>) -> Self {
        use crate::types::styling::{Border, BorderRadius};
        let mut container = Self::new();
        container.decoration = Some(
            BoxDecoration::new()
                .with_border(Border::uniform(border_color.into(), border_width))
                .with_border_radius(BorderRadius::ZERO)
        );
        container
    }

    /// Create a rounded container with a solid color.
    ///
    /// Combines color background with rounded corners in one convenient method.
    ///
    /// # Example
    /// ```ignore
    /// Container::rounded(Color::GREEN, 12.0)
    ///     .padding(16.0)
    ///     .child(|ui| { ui.label("Rounded") })
    ///     .ui(ui);
    /// ```
    pub fn rounded(color: impl Into<Color>, radius: f32) -> Self {
        use crate::types::styling::BorderRadius;
        let mut container = Self::new();
        let mut decoration = BoxDecoration::new();
        decoration.color = Some(color.into());
        decoration.border_radius = Some(BorderRadius::circular(radius));
        container.decoration = Some(decoration);
        container
    }

    /// Helper to calculate the desired size based on constraints.
    fn calculate_size(&self, available: egui::Vec2) -> egui::Vec2 {
        use crate::types::core::Size;

        let mut size = available;

        // Apply fixed sizes first
        if let Some(w) = self.width {
            size.x = w;
        }
        if let Some(h) = self.height {
            size.y = h;
        }

        // Build combined constraints from individual min/max and BoxConstraints
        if let Some(constraints) = &self.constraints {
            let final_size = Size::new(size.x, size.y);
            let constrained = constraints.constrain(final_size);
            size = egui::vec2(constrained.width, constrained.height);
        } else {
            // Apply individual min/max constraints
            if let Some(min_w) = self.min_width {
                size.x = size.x.max(min_w);
            }
            if let Some(min_h) = self.min_height {
                size.y = size.y.max(min_h);
            }
            if let Some(max_w) = self.max_width {
                size.x = size.x.min(max_w);
            }
            if let Some(max_h) = self.max_height {
                size.y = size.y.min(max_h);
            }
        }

        size
    }

    /// Get the final decoration, considering both decoration and color shorthand.
    fn get_decoration(&self) -> Option<BoxDecoration> {
        if let Some(ref decoration) = self.decoration {
            Some(decoration.clone())
        } else if let Some(color) = self.color {
            let mut decoration = BoxDecoration::new();
            decoration.color = Some(color);
            Some(decoration)
        } else {
            None
        }
    }

    /// Validate container configuration for potential issues.
    ///
    /// Checks for:
    /// - Conflicting size constraints (width with min/max_width, etc.)
    /// - Invalid size values (negative, NaN, infinite)
    /// - Conflicting decoration settings
    ///
    /// Returns Ok(()) if validation passes, or an error message describing the issue.
    pub fn validate(&self) -> Result<(), String> {
        // Check for conflicting width constraints
        if let Some(width) = self.width {
            if width < 0.0 || width.is_nan() || width.is_infinite() {
                return Err(format!("Invalid width: {}", width));
            }
            if self.min_width.is_some() || self.max_width.is_some() {
                return Err("Cannot set both 'width' and 'min_width'/'max_width'".to_string());
            }
        }

        // Check for conflicting height constraints
        if let Some(height) = self.height {
            if height < 0.0 || height.is_nan() || height.is_infinite() {
                return Err(format!("Invalid height: {}", height));
            }
            if self.min_height.is_some() || self.max_height.is_some() {
                return Err("Cannot set both 'height' and 'min_height'/'max_height'".to_string());
            }
        }

        // Validate min/max constraints
        if let (Some(min_w), Some(max_w)) = (self.min_width, self.max_width) {
            if min_w > max_w {
                return Err(format!("min_width ({}) > max_width ({})", min_w, max_w));
            }
        }

        if let (Some(min_h), Some(max_h)) = (self.min_height, self.max_height) {
            if min_h > max_h {
                return Err(format!("min_height ({}) > max_height ({})", min_h, max_h));
            }
        }

        // Warn if both color and decoration are set (decoration takes precedence)
        if self.color.is_some() && self.decoration.is_some() {
            // This is not an error, but could be logged as a warning
            // For now, we allow it (decoration will take precedence)
        }

        Ok(())
    }

}

impl Default for Container {
    fn default() -> Self {
        Self::new()
    }
}

// Import bon builder traits for custom setter and finishing functions
use container_builder::{IsUnset, State, SetChild, IsComplete};

// Smart setter for bon builder to enable .child() in builder chain
impl<S: State> ContainerBuilder<S> {
    /// Add a child widget using a closure (works directly in bon builder chain!)
    ///
    /// # Example
    /// ```ignore
    /// Container::builder()
    ///     .width(300.0)
    ///     .child(|ui| { ui.label("Hello") })  // ← .child() works in builder chain!
    ///     .build()
    ///     .ui(ui);
    /// ```
    pub fn child<F>(
        self,
        child: F
    ) -> ContainerBuilder<SetChild<S>>
    where
        S::Child: IsUnset,
        F: FnOnce(&mut egui::Ui) -> egui::Response + 'static,
    {
        // bon generates child_internal that accepts Box directly, not Option
        // It wraps in Option internally
        let boxed: Box<dyn FnOnce(&mut egui::Ui) -> egui::Response> = Box::new(child);
        self.child_internal(boxed)
    }
}

// Custom finishing functions for ergonomic API
impl<S: IsComplete> ContainerBuilder<S> {
    /// Build the container and immediately render it to UI.
    ///
    /// This is the most convenient way to use the builder - combines build + ui in one call.
    ///
    /// # Example
    /// ```ignore
    /// Container::builder()
    ///     .width(300.0)
    ///     .color(Color::BLUE)
    ///     .child(|ui| { ui.label("Hello") })
    ///     .ui(ui);  // ← No need for .build().ui(ui)!
    /// ```
    pub fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let container = self.build_internal();
        egui::Widget::ui(container, ui)
    }

    /// Build and render to UI with validation.
    ///
    /// This is the most convenient validated API - combines build + validate + render.
    ///
    /// # Example
    /// ```ignore
    /// Container::builder()
    ///     .width(300.0)
    ///     .color(Color::BLUE)
    ///     .child(|ui| { ui.label("Hello") })
    ///     .build(ui)?;  // ← Validates and renders!
    /// ```
    pub fn build(self, ui: &mut egui::Ui) -> Result<egui::Response, String> {
        let container = self.build_internal();
        container.validate()?;
        Ok(egui::Widget::ui(container, ui))
    }

    /// Build the container with validation (returns Container for reuse).
    ///
    /// Returns an error if the container has invalid configuration (e.g., conflicting constraints).
    /// Use this when you need the Container for later use.
    ///
    /// # Example
    /// ```ignore
    /// let container = Container::builder()
    ///     .width(300.0)
    ///     .min_width(200.0)  // ← This conflicts with width!
    ///     .try_build()?;  // Returns Err
    /// ```
    pub fn try_build(self) -> Result<Container, String> {
        let container = self.build_internal();
        container.validate()?;
        Ok(container)
    }
}

impl egui::Widget for Container {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        // If key is provided, wrap in an ID scope for state persistence
        if let Some(key) = self.key {
            ui.push_id(key, |ui| self.render(ui)).inner
        } else {
            self.render(ui)
        }
    }
}

impl Container {
    /// Internal rendering method (separated to handle key scoping)
    fn render(self, ui: &mut egui::Ui) -> egui::Response {
        let available_size = ui.available_size();
        let desired_size = self.calculate_size(available_size);

        // Apply margin - allocate extra space around the container
        let margin_size = egui::vec2(
            desired_size.x + self.margin.left + self.margin.right,
            desired_size.y + self.margin.top + self.margin.bottom,
        );

        let (outer_rect, response) = ui.allocate_exact_size(margin_size, Sense::hover());

        // Calculate rect inside margin
        let rect = egui::Rect::from_min_size(
            egui::pos2(
                outer_rect.min.x + self.margin.left,
                outer_rect.min.y + self.margin.top,
            ),
            desired_size,
        );

        // Calculate transform origin if needed
        let transform_origin = if self.transform.is_some() {
            if let Some(alignment) = self.transform_alignment {
                let offset_x = rect.width() * (alignment.x + 1.0) / 2.0;
                let offset_y = rect.height() * (alignment.y + 1.0) / 2.0;
                egui::pos2(rect.min.x + offset_x, rect.min.y + offset_y)
            } else {
                rect.center()
            }
        } else {
            rect.center() // Default, won't be used
        };

        // Note: Clipping is handled by egui's UiBuilder when creating child_ui.
        // The clip_behavior is stored for future use when we implement custom rendering.

        // 1. Render background decoration (behind child)
        if let Some(decoration) = self.get_decoration() {
            // If transform is specified, render with transformation
            if let Some(transform) = &self.transform {
                TransformPainter::paint_transformed_decoration(
                    ui.painter(),
                    rect,
                    transform_origin,
                    transform,
                    &decoration,
                );
            } else {
                DecorationPainter::paint(ui.painter(), rect, &decoration);
            }
        }

        // 2. Render child with padding and alignment
        if let Some(child_fn) = self.child {
            // Calculate inner rect with padding
            let padding = self.padding;
            let inner_rect = rect.shrink2(egui::vec2(
                padding.left + padding.right,
                padding.top + padding.bottom,
            ));

            // Determine layout alignment based on our Alignment
            let layout = if let Some(alignment) = self.alignment {
                // Convert our Alignment to egui's Align using Into trait
                egui::Layout::top_down(alignment.into())
            } else {
                egui::Layout::top_down(egui::Align::Min)
            };

            // Build UI with clipping if needed
            let ui_builder = egui::UiBuilder::new()
                .max_rect(inner_rect)
                .layout(layout);

            // Apply clipping using clip_rect if requested
            if self.clip_behavior.should_clip() {
                // Save current clip rect and intersect with our rect
                let old_clip_rect = ui.clip_rect();
                let new_clip_rect = old_clip_rect.intersect(inner_rect);

                // Create child UI with clipping
                let mut child_ui = ui.new_child(ui_builder);
                child_ui.set_clip_rect(new_clip_rect);
                child_fn(&mut child_ui);
            } else {
                // No clipping - render normally
                let mut child_ui = ui.new_child(ui_builder);
                child_fn(&mut child_ui);
            }
        }

        // 3. Render foreground decoration (on top of child)
        if let Some(foreground) = &self.foreground_decoration {
            // If transform is specified, render with transformation
            if let Some(transform) = &self.transform {
                TransformPainter::paint_transformed_decoration(
                    ui.painter(),
                    rect,
                    transform_origin,
                    transform,
                    foreground,
                );
            } else {
                DecorationPainter::paint(ui.painter(), rect, foreground);
            }
        }

        response
    }
}

// Implement nebula-ui WidgetExt trait (extension of egui::Widget)
impl WidgetExt for Container {
    fn id(&self) -> Option<egui::Id> {
        self.key
    }

    fn validate(&self) -> Result<(), String> {
        // Use existing validate method
        Container::validate(self)
    }

    fn debug_name(&self) -> &'static str {
        "Container"
    }

    fn size_hint(&self, _ui: &egui::Ui) -> Option<egui::Vec2> {
        // Has fixed size?
        let has_width = self.width.is_some() || self.min_width.is_some();
        let has_height = self.height.is_some() || self.min_height.is_some();

        if !has_width && !has_height {
            return None;  // Size depends on child
        }

        // If we have fixed width and height, return them with padding/margin
        if let (Some(w), Some(h)) = (self.width, self.height) {
            return Some(egui::vec2(
                w + self.padding.horizontal_total() + self.margin.horizontal_total(),
                h + self.padding.vertical_total() + self.margin.vertical_total(),
            ));
        }

        // If we have min constraints, return them
        if let (Some(min_w), Some(min_h)) = (self.min_width, self.min_height) {
            return Some(egui::vec2(
                min_w + self.padding.horizontal_total() + self.margin.horizontal_total(),
                min_h + self.padding.vertical_total() + self.margin.vertical_total(),
            ));
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::core::Color;

    #[test]
    fn test_container_creation() {
        let container = Container::new();
        assert!(container.decoration.is_none());
        assert_eq!(container.padding, EdgeInsets::ZERO);
    }

    #[test]
    fn test_container_with_decoration() {
        let mut decoration = BoxDecoration::new();
        decoration.color = Some(Color::RED);

        let mut container = Container::new();
        container.decoration = Some(decoration);

        assert!(container.decoration.is_some());
    }

    #[test]
    fn test_container_with_padding() {
        let container = Container {
            padding: EdgeInsets::all(16.0),
            ..Default::default()
        };

        assert_eq!(container.padding.left, 16.0);
        assert_eq!(container.padding.top, 16.0);
    }

    #[test]
    fn test_container_with_size() {
        let container = Container {
            width: Some(100.0),
            height: Some(50.0),
            ..Default::default()
        };

        assert_eq!(container.width, Some(100.0));
        assert_eq!(container.height, Some(50.0));
    }

    #[test]
    fn test_container_calculate_size() {
        let container = Container {
            min_width: Some(100.0),
            max_width: Some(200.0),
            min_height: Some(50.0),
            ..Default::default()
        };

        let size = container.calculate_size(egui::vec2(150.0, 30.0));
        assert_eq!(size.x, 150.0); // Within range
        assert_eq!(size.y, 50.0);  // Clamped to min

        let size = container.calculate_size(egui::vec2(250.0, 100.0));
        assert_eq!(size.x, 200.0); // Clamped to max
        assert_eq!(size.y, 100.0); // Within range
    }

    #[test]
    fn test_container_with_margin() {
        let container = Container {
            margin: EdgeInsets::all(20.0),
            ..Default::default()
        };

        assert_eq!(container.margin.left, 20.0);
        assert_eq!(container.margin.top, 20.0);
        assert_eq!(container.margin.right, 20.0);
        assert_eq!(container.margin.bottom, 20.0);
    }

    #[test]
    fn test_container_with_alignment() {
        let container = Container {
            alignment: Some(Alignment::CENTER),
            ..Default::default()
        };

        assert!(container.alignment.is_some());
        assert_eq!(container.alignment.unwrap(), Alignment::CENTER);
    }

    #[test]
    fn test_container_with_foreground_decoration() {
        let mut foreground = BoxDecoration::new();
        foreground.color = Some(Color::from_rgba(255, 0, 0, 128));

        let container = Container {
            foreground_decoration: Some(foreground),
            ..Default::default()
        };

        assert!(container.foreground_decoration.is_some());
    }

    #[test]
    fn test_container_with_color() {
        let container = Container {
            color: Some(Color::RED),
            ..Default::default()
        };

        assert!(container.color.is_some());
        assert_eq!(container.color.unwrap(), Color::RED);
    }

    #[test]
    fn test_container_with_box_constraints() {
        use crate::types::core::Size;

        let constraints = BoxConstraints::tight(Size::new(100.0, 50.0));
        let container = Container {
            constraints: Some(constraints),
            ..Default::default()
        };

        assert!(container.constraints.is_some());
        assert!(container.constraints.unwrap().is_tight());
    }

    #[test]
    fn test_container_with_transform() {
        let transform = Transform::rotate_degrees(45.0);
        let container = Container {
            transform: Some(transform),
            ..Default::default()
        };

        assert!(container.transform.is_some());
    }

    #[test]
    fn test_container_with_transform_alignment() {
        let container = Container {
            transform_alignment: Some(Alignment::TOP_LEFT),
            ..Default::default()
        };

        assert!(container.transform_alignment.is_some());
        assert_eq!(container.transform_alignment.unwrap(), Alignment::TOP_LEFT);
    }

    #[test]
    fn test_container_with_clip_behavior() {
        let container = Container {
            clip_behavior: Clip::AntiAlias,
            ..Default::default()
        };

        assert_eq!(container.clip_behavior, Clip::AntiAlias);
    }

    #[test]
    fn test_container_get_decoration_priority() {
        // Decoration takes precedence over color
        let mut decoration = BoxDecoration::new();
        decoration.color = Some(Color::BLUE);

        let container = Container {
            color: Some(Color::RED),
            decoration: Some(decoration),
            ..Default::default()
        };

        let final_decoration = container.get_decoration();
        assert!(final_decoration.is_some());
        // Decoration should be used, not color shorthand
    }

    #[test]
    fn test_container_get_decoration_from_color() {
        let container = Container {
            color: Some(Color::GREEN),
            ..Default::default()
        };

        let decoration = container.get_decoration();
        assert!(decoration.is_some());
    }

    #[test]
    fn test_container_calculate_size_with_constraints() {
        let constraints = BoxConstraints::new(50.0, 150.0, 30.0, 100.0);
        let container = Container {
            constraints: Some(constraints),
            ..Default::default()
        };

        let size = container.calculate_size(egui::vec2(200.0, 200.0));
        assert_eq!(size.x, 150.0); // Clamped to max
        assert_eq!(size.y, 100.0); // Clamped to max

        let size = container.calculate_size(egui::vec2(25.0, 25.0));
        assert_eq!(size.x, 50.0); // Clamped to min
        assert_eq!(size.y, 30.0); // Clamped to min
    }

    #[test]
    fn test_container_colored_factory() {
        let container = Container::colored(Color::BLUE);

        assert!(container.color.is_some());
        assert_eq!(container.color.unwrap(), Color::BLUE);

        // Verify it can be extended with struct fields
        let mut chained = Container::colored(Color::RED);
        chained.padding = EdgeInsets::all(10.0);
        chained.width = Some(100.0);

        assert_eq!(chained.color.unwrap(), Color::RED);
        assert_eq!(chained.padding, EdgeInsets::all(10.0));
        assert_eq!(chained.width, Some(100.0));
    }

    #[test]
    fn test_container_bordered_factory() {
        use crate::types::styling::Border;

        let container = Container::bordered(2.0, Color::RED);

        assert!(container.decoration.is_some());
        let decoration = container.decoration.unwrap();
        assert!(decoration.border.is_some());

        let border = decoration.border.unwrap();
        assert!(border.is_uniform());
        assert_eq!(border.top.width, 2.0);
        assert_eq!(border.top.color, Color::RED);

        // Verify extendability
        let mut chained = Container::bordered(3.0, Color::GREEN);
        chained.padding = EdgeInsets::all(15.0);

        assert_eq!(chained.padding, EdgeInsets::all(15.0));
    }

    #[test]
    fn test_container_rounded_factory() {
        use crate::types::styling::BorderRadius;

        let container = Container::rounded(Color::GREEN, 12.0);

        assert!(container.decoration.is_some());
        let decoration = container.decoration.unwrap();

        // Should have color
        assert!(decoration.color.is_some());
        assert_eq!(decoration.color.unwrap(), Color::GREEN);

        // Should have border radius
        assert!(decoration.border_radius.is_some());
        let radius = decoration.border_radius.unwrap();
        assert_eq!(radius.top_left.x, 12.0);
        assert_eq!(radius.top_right.x, 12.0);
        assert_eq!(radius.bottom_left.x, 12.0);
        assert_eq!(radius.bottom_right.x, 12.0);

        // Verify extendability
        let mut chained = Container::rounded(Color::BLUE, 8.0);
        chained.padding = EdgeInsets::all(20.0);
        chained.width = Some(200.0);

        assert_eq!(chained.padding, EdgeInsets::all(20.0));
        assert_eq!(chained.width, Some(200.0));
    }
}
