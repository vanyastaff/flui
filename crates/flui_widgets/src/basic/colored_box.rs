//! ColoredBox widget - a box with a solid color background
//!
//! A widget that paints a box with a solid color.
//! Similar to Flutter's ColoredBox widget.

use bon::Builder;
use flui_core::widget::{Widget, RenderWidget};
use flui_core::render::RenderNode;
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
#[derive(Debug, Clone, Builder)]
#[builder(on(String, into), on(Color, into), finish_fn = build_colored_box)]
pub struct ColoredBox {
    /// Optional key for widget identification
    pub key: Option<String>,

    /// The color to fill the box with.
    /// Default: Color::TRANSPARENT
    #[builder(default = Color::TRANSPARENT)]
    pub color: Color,

    /// The child widget (optional).
    #[builder(setters(vis = "", name = child_internal))]
    pub child: Option<Widget>,
}

impl ColoredBox {
    /// Creates a new ColoredBox with the given color.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let widget = ColoredBox::new(Color::BLUE, Text::new("Hello"));
    /// ```
    pub fn new(color: Color, child: Widget) -> Self {
        Self {
            key: None,
            color,
            child: Some(child),
        }
    }

    /// Sets the child widget.
    pub fn set_child(&mut self, child: Widget) {
        self.child = Some(child);
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
    pub fn child(self, child: Widget) -> ColoredBoxBuilder<SetChild<S>> {
        self.child_internal(child)
    }
}

impl<S: State> ColoredBoxBuilder<S> {
    /// Builds the ColoredBox widget.
    pub fn build(self) -> ColoredBox {
        self.build_colored_box()
    }
}

// Implement RenderWidget
impl RenderWidget for ColoredBox {
    fn create_render_object(&self, _context: &BuildContext) -> RenderNode {
        RenderNode::single(Box::new(RenderColoredBox::new(self.color)))
    }

    fn update_render_object(&self, _context: &BuildContext, render_object: &mut RenderNode) {
        if let RenderNode::Single { render, .. } = render_object {
            if let Some(colored_box) = render.downcast_mut::<RenderColoredBox>() {
                colored_box.set_color(self.color);
            }
        }
    }

    fn child(&self) -> Option<&Widget> {
        self.child.as_ref()
    }
}

// Implement IntoWidget for ergonomic API
flui_core::impl_into_widget!(ColoredBox, render);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_colored_box_new() {
        let widget = ColoredBox::new(Color::RED, Widget::from(()));
        assert_eq!(widget.color, Color::RED);
        assert!(widget.child.is_some());
    }

    #[test]
    fn test_colored_box_builder() {
        let widget = ColoredBox::builder()
            .color(Color::BLUE)
            .build();
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

        widget.set_child(Widget::from(()));
        assert!(widget.child.is_some());
    }

    #[test]
    fn test_colored_box_with_rgb() {
        let widget = ColoredBox::new(Color::rgb(255, 128, 0), Widget::from(()));
        assert_eq!(widget.color, Color::rgb(255, 128, 0));
    }
}
