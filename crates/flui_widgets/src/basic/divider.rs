//! Divider widget - a horizontal dividing line
//!
//! A thin horizontal line with padding on either side.
//! Similar to Flutter's Divider widget.
//!
//! # Usage
//!
//! ```rust,ignore
//! // Default divider (1px gray line)
//! Divider::new()
//!
//! // Custom thickness and color
//! Divider::builder()
//!     .thickness(2.0)
//!     .color(Color::BLUE)
//!     .build()
//!
//! // With indent and end indent
//! Divider::builder()
//!     .indent(20.0)
//!     .end_indent(20.0)
//!     .build()
//! ```

use bon::Builder;
use flui_core::view::{IntoElement, View};
use flui_core::BuildContext;
use flui_types::Color;

use crate::{ColoredBox, Container, SizedBox};

/// A thin horizontal line, typically used in lists to separate items.
///
/// Divider renders as a horizontal line with optional padding (indent).
/// The line has a default thickness of 1.0 logical pixels.
///
/// ## Key Properties
///
/// - **height**: The divider's height (includes line + spacing)
/// - **thickness**: The thickness of the line itself (default: 1.0)
/// - **indent**: Empty space to the leading edge (default: 0.0)
/// - **end_indent**: Empty space to the trailing edge (default: 0.0)
/// - **color**: The color of the line (default: Color::GRAY)
///
/// ## Layout Behavior
///
/// - Width: Fills available width (minus indents)
/// - Height: Uses specified height, or thickness if height not specified
///
/// ## Common Use Cases
///
/// ### List separator
/// ```rust,ignore
/// Column::new()
///     .children(vec![
///         ListTile::new("Item 1"),
///         Divider::new(),
///         ListTile::new("Item 2"),
///         Divider::new(),
///         ListTile::new("Item 3"),
///     ])
/// ```
///
/// ### Section divider with indents
/// ```rust,ignore
/// Divider::builder()
///     .indent(16.0)
///     .end_indent(16.0)
///     .thickness(1.0)
///     .color(Color::rgba(0, 0, 0, 0.12))
///     .build()
/// ```
///
/// ### Thick divider
/// ```rust,ignore
/// Divider::builder()
///     .thickness(4.0)
///     .color(Color::BLUE)
///     .build()
/// ```
#[derive(Debug, Clone, Builder)]
#[builder(on(String, into), finish_fn(name = build_internal, vis = ""))]
pub struct Divider {
    /// Optional key for widget identification
    pub key: Option<String>,

    /// The divider's total height including spacing.
    /// If null, defaults to thickness.
    pub height: Option<f32>,

    /// The thickness of the dividing line.
    /// Default: 1.0
    #[builder(default = 1.0)]
    pub thickness: f32,

    /// Empty space to the leading edge of the divider.
    /// Default: 0.0
    #[builder(default = 0.0)]
    pub indent: f32,

    /// Empty space to the trailing edge of the divider.
    /// Default: 0.0
    #[builder(default = 0.0)]
    pub end_indent: f32,

    /// The color to use when painting the divider.
    /// Default: Color::GRAY
    #[builder(default = Color::rgb(128, 128, 128))]
    pub color: Color,
}

impl Divider {
    /// Creates a new Divider with default settings.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let divider = Divider::new();
    /// ```
    pub fn new() -> Self {
        Self {
            key: None,
            height: None,
            thickness: 1.0,
            indent: 0.0,
            end_indent: 0.0,
            color: Color::rgb(128, 128, 128),
        }
    }

    /// Creates a Divider with custom color.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let divider = Divider::with_color(Color::BLUE);
    /// ```
    pub fn with_color(color: Color) -> Self {
        Self {
            color,
            ..Self::new()
        }
    }

    /// Creates a Divider with custom thickness.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let divider = Divider::with_thickness(2.0);
    /// ```
    pub fn with_thickness(thickness: f32) -> Self {
        Self {
            thickness,
            ..Self::new()
        }
    }
}

impl Default for Divider {
    fn default() -> Self {
        Self::new()
    }
}

// bon Builder Extensions
use divider_builder::State;

impl<S: State> DividerBuilder<S> {
    /// Builds the Divider widget.
    pub fn build(self) -> Divider {
        self.build_internal()
    }
}

// Implement View trait
impl View for Divider {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        // Calculate effective height (use height if specified, otherwise thickness)
        let effective_height = self.height.unwrap_or(self.thickness);

        // Build the child view
        if self.indent > 0.0 || self.end_indent > 0.0 {
            // If we have indents, we need to wrap in a Container with padding
            Container::builder()
                .height(effective_height)
                .padding(flui_types::EdgeInsets {
                    left: self.indent,
                    right: self.end_indent,
                    top: 0.0,
                    bottom: 0.0,
                })
                .child(
                    ColoredBox::builder()
                        .color(self.color)
                        .child(SizedBox::builder().height(self.thickness).build())
                        .build(),
                )
                .build()
                .into_element()
        } else {
            // Simple case: just a colored box with height
            SizedBox::builder()
                .height(effective_height)
                .child(
                    ColoredBox::builder()
                        .color(self.color)
                        .child(SizedBox::builder().height(self.thickness).build())
                        .build(),
                )
                .build()
                .into_element()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_divider_new() {
        let divider = Divider::new();
        assert_eq!(divider.thickness, 1.0);
        assert_eq!(divider.indent, 0.0);
        assert_eq!(divider.end_indent, 0.0);
    }

    #[test]
    fn test_divider_with_color() {
        let divider = Divider::with_color(Color::BLUE);
        assert_eq!(divider.color, Color::BLUE);
    }

    #[test]
    fn test_divider_with_thickness() {
        let divider = Divider::with_thickness(2.0);
        assert_eq!(divider.thickness, 2.0);
    }

    #[test]
    fn test_divider_builder() {
        let divider = Divider::builder()
            .thickness(2.0)
            .indent(10.0)
            .end_indent(10.0)
            .color(Color::RED)
            .build_divider();

        assert_eq!(divider.thickness, 2.0);
        assert_eq!(divider.indent, 10.0);
        assert_eq!(divider.end_indent, 10.0);
        assert_eq!(divider.color, Color::RED);
    }

    #[test]
    fn test_divider_default() {
        let divider = Divider::default();
        assert_eq!(divider.thickness, 1.0);
    }

    #[test]
    fn test_divider_height() {
        let divider = Divider::builder()
            .height(20.0)
            .thickness(2.0)
            .build_divider();

        assert_eq!(divider.height, Some(20.0));
        assert_eq!(divider.thickness, 2.0);
    }
}
