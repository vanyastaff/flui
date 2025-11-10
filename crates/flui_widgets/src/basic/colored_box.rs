//! ColoredBox widget - a box with a solid color background
//!
//! A widget that paints a box with a solid color.
//! Similar to Flutter's ColoredBox widget.

use bon::Builder;
use flui_core::view::{AnyView, IntoElement, View};
use flui_core::BuildContext;
use flui_rendering::RenderColoredBox;
use flui_types::Color;

/// A widget that paints a box with a solid color.
///
/// ColoredBox is a simple and efficient way to add a colored background.
/// It's more efficient than Container or DecoratedBox when you only need
/// a solid color (no gradients, borders, or shadows).
///
/// ## Layout Behavior
///
/// - With child: Takes the size of the child
/// - Without child: Expands to fill available space
///
/// ## Common Use Cases
///
/// ### Simple background color
/// ```rust,ignore
/// ColoredBox::new(Color::BLUE, Text::new("Hello"))
/// ```
///
/// ### Full-screen background
/// ```rust,ignore
/// ColoredBox::builder()
///     .color(Color::rgb(240, 240, 240))
///     .child(MyAppContent::new())
///     .build()
/// ```
///
/// ### Colored spacer
/// ```rust,ignore
/// Row::new()
///     .children(vec![
///         widget1,
///         ColoredBox::builder()
///             .color(Color::RED)
///             .child(SizedBox::builder().width(2.0).build())  // 2px red line
///             .build(),
///         widget2,
///     ])
/// ```
///
/// ## Examples
///
/// ```rust,ignore
/// // Simple colored background
/// ColoredBox::new(Color::RED, child_widget)
///
/// // Using builder
/// ColoredBox::builder()
///     .color(Color::rgb(255, 0, 0))
///     .child(Text::new("Red background"))
///     .build()
///
/// // Named colors
/// ColoredBox::new(Color::BLUE, icon)
/// ColoredBox::new(Color::TRANSPARENT, widget)  // No visual effect
/// ```
#[derive(Builder)]
#[builder(on(String, into), on(Color, into), finish_fn(name = build_internal, vis = ""))]
pub struct ColoredBox {
    /// Optional key for widget identification
    pub key: Option<String>,

    /// The color to fill the box with.
    /// Default: Color::TRANSPARENT
    #[builder(default = Color::TRANSPARENT)]
    pub color: Color,

    /// The child widget (optional).
    #[builder(setters(vis = "", name = child_internal))]
    pub child: Option<Box<dyn AnyView>>,
}

// Manual Debug implementation since AnyView doesn't implement Debug
impl std::fmt::Debug for ColoredBox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ColoredBox")
            .field("key", &self.key)
            .field("color", &self.color)
            .field(
                "child",
                &if self.child.is_some() {
                    "<AnyView>"
                } else {
                    "None"
                },
            )
            .finish()
    }
}

// Manual Clone implementation since AnyView doesn't implement Clone
impl Clone for ColoredBox {
    fn clone(&self) -> Self {
        Self {
            key: self.key.clone(),
            color: self.color,
            child: self.child.clone(),
        }
    }
}

impl ColoredBox {
    /// Creates a new ColoredBox with the given color.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let widget = ColoredBox::new(Color::BLUE, Text::new("Hello"));
    /// ```
    pub fn new(color: Color, child: impl View + 'static) -> Self {
        Self {
            key: None,
            color,
            child: Some(Box::new(child)),
        }
    }

    /// Sets the child widget.
    pub fn set_child(&mut self, child: impl View + 'static) {
        self.child = Some(Box::new(child));
    }
}

impl Default for ColoredBox {
    fn default() -> Self {
        Self {
            key: None,
            color: Color::TRANSPARENT,
            child: None,
        }
    }
}

// bon Builder Extensions
use colored_box_builder::{IsUnset, SetChild, State};

impl<S: State> ColoredBoxBuilder<S>
where
    S::Child: IsUnset,
{
    /// Sets the child widget (works in builder chain).
    pub fn child(self, child: impl View + 'static) -> ColoredBoxBuilder<SetChild<S>> {
        self.child_internal(Box::new(child))
    }
}

impl<S: State> ColoredBoxBuilder<S> {
    /// Builds the ColoredBox widget.
    pub fn build(self) -> ColoredBox {
        self.build_internal()
    }
}

// Implement View for ColoredBox - New architecture
impl View for ColoredBox {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        (RenderColoredBox::new(self.color), self.child)
    }
}

// ColoredBox now implements View trait directly

#[cfg(test)]
mod tests {
    use super::*;

    // Mock view for testing
    #[derive(Debug, Clone)]
    struct MockView;

    impl View for MockView {
        fn build(self, _ctx: &BuildContext) -> impl IntoElement {
            (RenderPadding::new(EdgeInsets::ZERO), ())}
    }

    #[test]
    fn test_colored_box_new() {
        let widget = ColoredBox::new(Color::RED, MockView);
        assert_eq!(widget.color, Color::RED);
        assert!(widget.child.is_some());
    }

    #[test]
    fn test_colored_box_builder() {
        let widget = ColoredBox::builder().color(Color::BLUE).build();
        assert_eq!(widget.color, Color::BLUE);
    }

    #[test]
    fn test_colored_box_default() {
        let widget = ColoredBox::default();
        assert_eq!(widget.color, Color::TRANSPARENT);
        assert!(widget.child.is_none());
    }

    #[test]
    fn test_colored_box_set_child() {
        let mut widget = ColoredBox::default();
        assert!(widget.child.is_none());

        widget.set_child(MockView);
        assert!(widget.child.is_some());
    }

    #[test]
    fn test_colored_box_with_rgb() {
        let widget = ColoredBox::new(Color::rgb(255, 128, 0), MockView);
        assert_eq!(widget.color, Color::rgb(255, 128, 0));
    }
}
