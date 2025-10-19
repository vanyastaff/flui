//! Container widget - a composition of layout and decoration widgets
//!
//! Container is a convenience widget that combines common styling and
//! layout properties. It's similar to Flutter's Container widget.

use flui_core::{BoxConstraints, Widget};
use flui_types::styling::BoxDecoration;
use flui_types::{Alignment, Color, EdgeInsets, Size};

/// A convenience widget that combines common painting, positioning, and sizing widgets.
///
/// Container is one of the most commonly used widgets. It combines several simpler
/// widgets to provide a convenient API for common styling and layout needs.
///
/// # Layout behavior
///
/// Container's layout behavior depends on several factors:
///
/// - If the widget has no child, no height, no width, no constraints, and parent
///   provides bounded constraints, Container tries to be as small as possible.
///
/// - If the widget has no child and no alignment, but has height, width, or constraints,
///   Container tries to be as small as possible given the combination of those constraints
///   and the parent's constraints.
///
/// - If the widget has no child, no height, no width, no constraints, and parent provides
///   unbounded constraints, Container tries to expand.
///
/// - If the widget has an alignment, and the parent provides bounded constraints,
///   Container tries to expand to fit the parent.
///
/// - If the widget has an alignment, and the parent provides unbounded constraints,
///   Container tries to size itself to the child.
///
/// - If the widget has a child but no height, width, alignment, or constraints, Container
///   passes constraints through to child and sizes itself to child.
///
/// - If the widget has width or height, those properties override constraints.
///
/// # Examples
///
/// ```rust,ignore
/// use flui_widgets::*;
///
/// // Simple colored box
/// Container::new()
///     .width(100.0)
///     .height(100.0)
///     .color(Color::RED)
///     .build();
///
/// // Container with padding and decoration
/// Container::new()
///     .padding(EdgeInsets::all(16.0))
///     .decoration(BoxDecoration {
///         color: Some(Color::BLUE),
///         border_radius: Some(BorderRadius::circular(8.0)),
///         ..Default::default()
///     })
///     .child(Text::new("Hello"))
///     .build();
/// ```
#[derive(Debug, Clone)]
pub struct Container {
    /// Unique key for this widget
    pub key: Option<String>,

    /// Align the child within the container.
    ///
    /// If non-null, the container will expand to fill its parent and position its child
    /// within itself according to the given value.
    ///
    /// Ignored if child is null.
    pub alignment: Option<Alignment>,

    /// Empty space to inscribe inside the decoration. The child is placed inside this padding.
    pub padding: Option<EdgeInsets>,

    /// The color to paint behind the child.
    ///
    /// This property should not be used with [decoration] at the same time.
    /// Prefer using [decoration.color] instead.
    pub color: Option<Color>,

    /// The decoration to paint behind the child.
    ///
    /// Use the [color] property to specify a simple solid color.
    pub decoration: Option<BoxDecoration>,

    /// Empty space to surround the decoration and child.
    pub margin: Option<EdgeInsets>,

    /// The width of this container.
    ///
    /// If null, the container will try to be as wide as possible given constraints.
    /// If non-null, the width is an exact constraint.
    pub width: Option<f32>,

    /// The height of this container.
    ///
    /// If null, the container will try to be as tall as possible given constraints.
    /// If non-null, the height is an exact constraint.
    pub height: Option<f32>,

    /// Additional constraints to apply to the child.
    ///
    /// Width and height constraints override min/max constraints in BoxConstraints.
    pub constraints: Option<BoxConstraints>,

    /// The child contained by the container.
    ///
    /// If null, the container will size itself according to other properties.
    pub child: Option<Box<dyn Widget>>,
}

impl Container {
    /// Creates a new Container widget.
    pub fn new() -> Self {
        Self {
            key: None,
            alignment: None,
            padding: None,
            color: None,
            decoration: None,
            margin: None,
            width: None,
            height: None,
            constraints: None,
            child: None,
        }
    }

    /// Sets the key for this widget.
    pub fn key(mut self, key: impl Into<String>) -> Self {
        self.key = Some(key.into());
        self
    }

    /// Sets the alignment of the child within the container.
    pub fn alignment(mut self, alignment: Alignment) -> Self {
        self.alignment = Some(alignment);
        self
    }

    /// Sets the padding around the child.
    pub fn padding(mut self, padding: EdgeInsets) -> Self {
        self.padding = Some(padding);
        self
    }

    /// Sets the background color.
    pub fn color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }

    /// Sets the decoration.
    pub fn decoration(mut self, decoration: BoxDecoration) -> Self {
        self.decoration = Some(decoration);
        self
    }

    /// Sets the margin around the container.
    pub fn margin(mut self, margin: EdgeInsets) -> Self {
        self.margin = Some(margin);
        self
    }

    /// Sets the width of the container.
    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }

    /// Sets the height of the container.
    pub fn height(mut self, height: f32) -> Self {
        self.height = Some(height);
        self
    }

    /// Sets additional constraints for the container.
    pub fn constraints(mut self, constraints: BoxConstraints) -> Self {
        self.constraints = Some(constraints);
        self
    }

    /// Sets the child widget.
    pub fn child(mut self, child: impl Widget + 'static) -> Self {
        self.child = Some(Box::new(child));
        self
    }

    /// Builds the final widget tree.
    ///
    /// This method constructs the actual widget tree by composing simpler widgets.
    /// The order of composition is:
    ///
    /// 1. Apply constraints (if any)
    /// 2. Apply decoration/color (if any)
    /// 3. Apply padding (if any)
    /// 4. Apply alignment (if any)
    /// 5. Wrap child
    ///
    /// Note: This is a placeholder implementation. In a real implementation,
    /// this would return the composed widget tree using RenderObjectWidgets.
    pub fn build(self) -> Self {
        // For now, just return self
        // In a real implementation, this would create the widget tree:
        // margin -> constraints -> decoration -> padding -> alignment -> child
        self
    }
}

impl Default for Container {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for Container {
    fn create_element(&self) -> Box<dyn flui_core::Element> {
        // Placeholder: In a real implementation, this would create a ComponentElement
        // that builds the child widget tree in its build() method
        todo!("Container::create_element - requires full Element implementation")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_container_new() {
        let container = Container::new();
        assert!(container.key.is_none());
        assert!(container.child.is_none());
        assert!(container.width.is_none());
        assert!(container.height.is_none());
    }

    #[test]
    fn test_container_builder_width_height() {
        let container = Container::new().width(100.0).height(200.0).build();

        assert_eq!(container.width, Some(100.0));
        assert_eq!(container.height, Some(200.0));
    }

    #[test]
    fn test_container_builder_color() {
        let red = Color::rgb(255, 0, 0);
        let container = Container::new().color(red).build();

        assert_eq!(container.color, Some(red));
    }

    #[test]
    fn test_container_builder_padding() {
        let padding = EdgeInsets::all(16.0);
        let container = Container::new().padding(padding).build();

        assert_eq!(container.padding, Some(padding));
    }

    #[test]
    fn test_container_builder_margin() {
        let margin = EdgeInsets::symmetric(10.0, 20.0);
        let container = Container::new().margin(margin).build();

        assert_eq!(container.margin, Some(margin));
    }

    #[test]
    fn test_container_builder_alignment() {
        let container = Container::new().alignment(Alignment::CENTER).build();

        assert_eq!(container.alignment, Some(Alignment::CENTER));
    }

    #[test]
    fn test_container_builder_constraints() {
        let constraints = BoxConstraints::tight(Size::new(100.0, 100.0));
        let container = Container::new().constraints(constraints).build();

        assert_eq!(container.constraints, Some(constraints));
    }

    #[test]
    fn test_container_builder_key() {
        let container = Container::new().key("my-container").build();

        assert_eq!(container.key, Some("my-container".to_string()));
    }

    #[test]
    fn test_container_builder_chaining() {
        let blue = Color::rgb(0, 0, 255);
        let container = Container::new()
            .width(200.0)
            .height(100.0)
            .padding(EdgeInsets::all(8.0))
            .color(blue)
            .alignment(Alignment::CENTER)
            .build();

        assert_eq!(container.width, Some(200.0));
        assert_eq!(container.height, Some(100.0));
        assert_eq!(container.padding, Some(EdgeInsets::all(8.0)));
        assert_eq!(container.color, Some(blue));
        assert_eq!(container.alignment, Some(Alignment::CENTER));
    }

    #[test]
    fn test_container_default() {
        let container = Container::default();
        assert!(container.child.is_none());
        assert!(container.width.is_none());
    }

    #[test]
    fn test_container_decoration() {
        let green = Color::rgb(0, 255, 0);
        let decoration = BoxDecoration {
            color: Some(green),
            ..Default::default()
        };
        let container = Container::new().decoration(decoration.clone()).build();

        assert_eq!(container.decoration, Some(decoration));
    }

    #[test]
    fn test_container_full_composition() {
        let container = Container::new()
            .key("test")
            .width(300.0)
            .height(150.0)
            .padding(EdgeInsets::all(16.0))
            .margin(EdgeInsets::symmetric(8.0, 12.0))
            .color(Color::rgb(255, 0, 0))
            .alignment(Alignment::TOP_LEFT)
            .constraints(BoxConstraints::new(0.0, 500.0, 0.0, 300.0))
            .build();

        assert_eq!(container.key, Some("test".to_string()));
        assert_eq!(container.width, Some(300.0));
        assert_eq!(container.height, Some(150.0));
        assert!(container.padding.is_some());
        assert!(container.margin.is_some());
        assert!(container.color.is_some());
        assert_eq!(container.alignment, Some(Alignment::TOP_LEFT));
        assert!(container.constraints.is_some());
    }
}
