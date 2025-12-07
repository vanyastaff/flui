//! ColoredBox widget - a box with a solid color background
//!
//! A widget that paints a box with a solid color.
//! Similar to Flutter's ColoredBox widget.

use bon::Builder;
use flui_core::element::Element;
use flui_core::view::children::Child;
use flui_core::view::{IntoElement, StatelessView};
use flui_core::BuildContext;
use flui_rendering::objects::RenderColoredBox;
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
    #[builder(default, setters(vis = "", name = child_internal))]
    pub child: Child,
}

// Manual Debug implementation
impl std::fmt::Debug for ColoredBox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ColoredBox")
            .field("key", &self.key)
            .field("color", &self.color)
            .field(
                "child",
                &if self.child.is_some() {
                    "<child>"
                } else {
                    "None"
                },
            )
            .finish()
    }
}

impl ColoredBox {
    /// Creates a new ColoredBox with the given color and child.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let widget = ColoredBox::new(Color::BLUE, Text::new("Hello"));
    /// ```
    pub fn new(color: Color, child: impl IntoElement) -> Self {
        Self {
            key: None,
            color,
            child: Child::new(child),
        }
    }

    /// Creates a ColoredBox with just a color (no child).
    ///
    /// The box will expand to fill available space.
    pub fn color_only(color: Color) -> Self {
        Self {
            key: None,
            color,
            child: Child::none(),
        }
    }
}

impl Default for ColoredBox {
    fn default() -> Self {
        Self {
            key: None,
            color: Color::TRANSPARENT,
            child: Child::none(),
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
    pub fn child(self, child: impl IntoElement) -> ColoredBoxBuilder<SetChild<S>> {
        self.child_internal(Child::new(child))
    }
}

impl<S: State> ColoredBoxBuilder<S> {
    /// Builds the ColoredBox widget.
    pub fn build(self) -> ColoredBox {
        self.build_internal()
    }
}

// Implement View for ColoredBox
impl StatelessView for ColoredBox {
    fn build(self, _ctx: &dyn BuildContext) -> impl IntoElement {
        // RenderColoredBox is a Leaf render (no children)
        // If we have a child, we need to use a Stack to layer them
        if self.child.is_some() {
            // Use Stack to layer colored background with child
            use flui_rendering::objects::RenderStack;
            let child: Element = self.child.into_element();
            RenderStack::default()
                .children(vec![
                    RenderColoredBox::new(self.color).leaf().into_element(),
                    child,
                ])
                .into_element()
        } else {
            // Just the colored box as a leaf
            RenderColoredBox::new(self.color).leaf().into_element()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_rendering::objects::RenderEmpty;

    // Mock view for testing
    #[derive(Debug, Clone)]
    struct MockView;

    impl StatelessView for MockView {
        fn build(self, _ctx: &dyn BuildContext) -> impl IntoElement {
            RenderEmpty.leaf()
        }
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
    fn test_colored_box_color_only() {
        let widget = ColoredBox::color_only(Color::GREEN);
        assert_eq!(widget.color, Color::GREEN);
        assert!(widget.child.is_none());
    }

    #[test]
    fn test_colored_box_with_rgb() {
        let widget = ColoredBox::new(Color::rgb(255, 128, 0), MockView);
        assert_eq!(widget.color, Color::rgb(255, 128, 0));
    }
}
