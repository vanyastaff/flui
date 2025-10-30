//! VerticalDivider widget - a vertical dividing line
//!
//! A thin vertical line with padding on top and bottom.
//! Similar to Flutter's VerticalDivider widget.
//!
//! # Usage
//!
//! ```rust,ignore
//! // Default vertical divider (1px gray line)
//! VerticalDivider::new()
//!
//! // Custom thickness and color
//! VerticalDivider::builder()
//!     .thickness(2.0)
//!     .color(Color::BLUE)
//!     .build()
//!
//! // With indent and end indent
//! VerticalDivider::builder()
//!     .indent(20.0)
//!     .end_indent(20.0)
//!     .build()
//! ```

use bon::Builder;
use flui_core::widget::{StatelessWidget, Widget};
use flui_core::BuildContext;
use flui_types::Color;

use crate::{ColoredBox, Container, SizedBox};

/// A thin vertical line, typically used to separate items horizontally.
///
/// VerticalDivider renders as a vertical line with optional padding (indent).
/// The line has a default thickness of 1.0 logical pixels.
///
/// ## Key Properties
///
/// - **width**: The divider's width (includes line + spacing)
/// - **thickness**: The thickness of the line itself (default: 1.0)
/// - **indent**: Empty space at the top edge (default: 0.0)
/// - **end_indent**: Empty space at the bottom edge (default: 0.0)
/// - **color**: The color of the line (default: Color::GRAY)
///
/// ## Layout Behavior
///
/// - Width: Uses specified width, or thickness if width not specified
/// - Height: Fills available height (minus indents)
///
/// ## Common Use Cases
///
/// ### Toolbar separator
/// ```rust,ignore
/// Row::new()
///     .children(vec![
///         IconButton::new("cut"),
///         VerticalDivider::new(),
///         IconButton::new("copy"),
///         VerticalDivider::new(),
///         IconButton::new("paste"),
///     ])
/// ```
///
/// ### Panel separator with indents
/// ```rust,ignore
/// VerticalDivider::builder()
///     .indent(16.0)
///     .end_indent(16.0)
///     .thickness(1.0)
///     .color(Color::rgba(0, 0, 0, 0.12))
///     .build()
/// ```
///
/// ### Thick divider
/// ```rust,ignore
/// VerticalDivider::builder()
///     .thickness(4.0)
///     .color(Color::BLUE)
///     .build()
/// ```
#[derive(Debug, Clone, Builder)]
#[builder(on(String, into), finish_fn = build_vertical_divider)]
pub struct VerticalDivider {
    /// Optional key for widget identification
    pub key: Option<String>,

    /// The divider's total width including spacing.
    /// If null, defaults to thickness.
    pub width: Option<f32>,

    /// The thickness of the dividing line.
    /// Default: 1.0
    #[builder(default = 1.0)]
    pub thickness: f32,

    /// Empty space at the top edge of the divider.
    /// Default: 0.0
    #[builder(default = 0.0)]
    pub indent: f32,

    /// Empty space at the bottom edge of the divider.
    /// Default: 0.0
    #[builder(default = 0.0)]
    pub end_indent: f32,

    /// The color to use when painting the divider.
    /// Default: Color::GRAY
    #[builder(default = Color::rgb(128, 128, 128))]
    pub color: Color,
}

impl VerticalDivider {
    /// Creates a new VerticalDivider with default settings.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let divider = VerticalDivider::new();
    /// ```
    pub fn new() -> Self {
        Self {
            key: None,
            width: None,
            thickness: 1.0,
            indent: 0.0,
            end_indent: 0.0,
            color: Color::rgb(128, 128, 128),
        }
    }

    /// Creates a VerticalDivider with custom color.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let divider = VerticalDivider::with_color(Color::BLUE);
    /// ```
    pub fn with_color(color: Color) -> Self {
        Self {
            color,
            ..Self::new()
        }
    }

    /// Creates a VerticalDivider with custom thickness.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let divider = VerticalDivider::with_thickness(2.0);
    /// ```
    pub fn with_thickness(thickness: f32) -> Self {
        Self {
            thickness,
            ..Self::new()
        }
    }
}

impl Default for VerticalDivider {
    fn default() -> Self {
        Self::new()
    }
}

// bon Builder Extensions
use vertical_divider_builder::{State};

impl<S: State> VerticalDividerBuilder<S> {
    /// Builds the VerticalDivider widget.
    pub fn build(self) -> Widget {
        Widget::stateless(self.build_vertical_divider())
    }
}

// Implement StatelessWidget
impl StatelessWidget for VerticalDivider {
    fn build(&self, _context: &BuildContext) -> Widget {
        // Calculate effective width (use width if specified, otherwise thickness)
        let effective_width = self.width.unwrap_or(self.thickness);

        // If we have indents, we need to wrap in a Container with padding
        if self.indent > 0.0 || self.end_indent > 0.0 {
            Widget::from(Container::builder()
                .width(effective_width)
                .padding(flui_types::EdgeInsets {
                    left: 0.0,
                    right: 0.0,
                    top: self.indent,
                    bottom: self.end_indent,
                })
                .child(ColoredBox::builder()
                    .color(self.color)
                    .child(SizedBox::builder()
                        .width(self.thickness)
                        .build())
                    .build())
                .build())
        } else {
            // Simple case: just a colored box with width
            Widget::from(SizedBox::builder()
                .width(effective_width)
                .child(ColoredBox::builder()
                    .color(self.color)
                    .child(SizedBox::builder()
                        .width(self.thickness)
                        .build())
                    .build())
                .build())
        }
    }
}

// Implement IntoWidget for ergonomic API
flui_core::impl_into_widget!(VerticalDivider, stateless);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vertical_divider_new() {
        let divider = VerticalDivider::new();
        assert_eq!(divider.thickness, 1.0);
        assert_eq!(divider.indent, 0.0);
        assert_eq!(divider.end_indent, 0.0);
    }

    #[test]
    fn test_vertical_divider_with_color() {
        let divider = VerticalDivider::with_color(Color::BLUE);
        assert_eq!(divider.color, Color::BLUE);
    }

    #[test]
    fn test_vertical_divider_with_thickness() {
        let divider = VerticalDivider::with_thickness(2.0);
        assert_eq!(divider.thickness, 2.0);
    }

    #[test]
    fn test_vertical_divider_builder() {
        let divider = VerticalDivider::builder()
            .thickness(2.0)
            .indent(10.0)
            .end_indent(10.0)
            .color(Color::RED)
            .build();

        assert_eq!(divider.thickness, 2.0);
        assert_eq!(divider.indent, 10.0);
        assert_eq!(divider.end_indent, 10.0);
        assert_eq!(divider.color, Color::RED);
    }

    #[test]
    fn test_vertical_divider_default() {
        let divider = VerticalDivider::default();
        assert_eq!(divider.thickness, 1.0);
    }

    #[test]
    fn test_vertical_divider_width() {
        let divider = VerticalDivider::builder()
            .width(20.0)
            .thickness(2.0)
            .build();

        assert_eq!(divider.width, Some(20.0));
        assert_eq!(divider.thickness, 2.0);
    }
}
