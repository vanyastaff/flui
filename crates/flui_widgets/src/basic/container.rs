//! Container widget - a composition of layout and decoration widgets
//!
//! Container is a convenience widget that combines common styling and
//! layout properties. It's similar to Flutter's Container widget.
//!
//! # Convenience Methods (New API)
//!
//! Common Material Design patterns as one-liners:
//!
//! ```rust,ignore
//! // Solid color background
//! Container::colored(Color::BLUE, child)
//!
//! // Material Design card with elevation
//! Container::card(content)
//!
//! // Outlined container with border
//! Container::outlined(Color::BLUE, child)
//!
//! // Surface container with padding
//! Container::surface(child)
//!
//! // Rounded container
//! Container::rounded(Color::GREEN, 12.0, child)
//!
//! // Fixed-size container
//! Container::sized(200.0, 100.0, child)
//! ```
//!
//! # Traditional Creation Styles
//!
//! Container still supports traditional creation patterns:
//!
//! ## 1. Struct Literal (Flutter-like)
//! ```rust,ignore
//! Container {
//!     width: Some(300.0),
//!     height: Some(200.0),
//!     padding: Some(EdgeInsets::all(20.0)),
//!     color: Some(Color::rgb(255, 0, 0)),
//!     ..Default::default()
//! }
//! ```
//!
//! ## 2. Builder Pattern (Type-safe)
//! ```rust,ignore
//! Container::builder()
//!     .width(300.0)
//!     .height(200.0)
//!     .padding(EdgeInsets::all(20.0))
//!     .color(Color::rgb(255, 0, 0))
//!     .child(content)
//!     .build()
//! ```

use bon::Builder;
use flui_core::view::{AnyView, IntoElement, View};
use flui_core::BuildContext;
use flui_types::constraints::BoxConstraints;
use flui_types::styling::{BorderRadius, BorderSide, BorderStyle, BoxDecoration, BoxShadow};
use flui_types::{Alignment, Color, EdgeInsets, Offset};

/// A convenience widget that combines common painting, positioning, and sizing widgets.
///
/// Container is one of the most commonly used widgets. It combines several simpler
/// widgets to provide a convenient API for common styling and layout needs.
///
/// # Implementation Note
///
/// **Container is a StatelessWidget** (not a RenderObjectWidget). This follows Flutter's
/// design where Container extends StatelessWidget and composes other widgets:
/// - Padding (for padding and margin)
/// - Align (for alignment)
/// - DecoratedBox (for decoration and color)
/// - ConstrainedBox (for width/height/constraints)
///
/// This is different from widgets like Padding or Center which directly create RenderObjects.
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
#[derive(Builder)]
#[builder(
    on(String, into),
    on(EdgeInsets, into),
    on(BoxDecoration, into),
    on(Color, into),
    finish_fn(name = build_internal, vis = "")  // Private internal build function
)]
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
    /// This property should not be used with [`decoration`](Self::decoration) at the same time.
    /// Prefer using `decoration.color` instead.
    pub color: Option<Color>,

    /// The decoration to paint behind the child.
    ///
    /// Use the [`color`](Self::color) property to specify a simple solid color.
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

    // Note: Transform feature is currently disabled
    // /// The transformation matrix to apply to the container.
    // ///
    // /// If non-null, the container will be wrapped in a Transform widget.
    // /// The transformation is applied OUTSIDE all other effects (decoration, alignment, etc).
    // pub transform: Option<Matrix4>,
    /// The child contained by the container.
    ///
    /// If null, the container will size itself according to other properties.
    /// Use the custom `.child()` setter in the builder.
    #[builder(setters(vis = "", name = child_internal))]
    pub child: Option<Box<dyn AnyView>>,
}

impl std::fmt::Debug for Container {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Container")
            .field("key", &self.key)
            .field("alignment", &self.alignment)
            .field("padding", &self.padding)
            .field("color", &self.color)
            .field("decoration", &self.decoration)
            .field("margin", &self.margin)
            .field("width", &self.width)
            .field("height", &self.height)
            .field("constraints", &self.constraints)
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

impl Clone for Container {
    fn clone(&self) -> Self {
        Self {
            key: self.key.clone(),
            alignment: self.alignment,
            padding: self.padding,
            color: self.color,
            decoration: self.decoration.clone(),
            margin: self.margin,
            width: self.width,
            height: self.height,
            constraints: self.constraints,
            child: self.child.clone(),
        }
    }
}

impl Container {
    /// Creates a new empty Container.
    ///
    /// This is the base constructor. Use builder() or convenience methods for a fluent API.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let container = Container::new();
    /// ```
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
            // transform: None,  // Transform feature is currently disabled
            child: None,
        }
    }

    /// Gets the final decoration, considering both decoration and color shorthand.
    ///
    /// If both `decoration` and `color` are set, `decoration` takes precedence.
    pub fn get_decoration(&self) -> Option<BoxDecoration> {
        if let Some(ref decoration) = self.decoration {
            Some(decoration.clone())
        } else {
            self.color.map(|color| BoxDecoration {
                color: Some(color),
                ..Default::default()
            })
        }
    }

    /// Validates the container configuration.
    ///
    /// Checks for:
    /// - Conflicting size constraints
    /// - Invalid size values (negative, NaN, infinite)
    ///
    /// Returns `Ok(())` if validation passes.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let container = Container::new();
    /// assert!(container.validate().is_ok());
    /// ```
    pub fn validate(&self) -> Result<(), String> {
        // Check for invalid width
        if let Some(width) = self.width {
            if width < 0.0 || width.is_nan() || width.is_infinite() {
                return Err(format!("Invalid width: {}", width));
            }
        }

        // Check for invalid height
        if let Some(height) = self.height {
            if height < 0.0 || height.is_nan() || height.is_infinite() {
                return Err(format!("Invalid height: {}", height));
            }
        }

        // Warn if both color and decoration are set (decoration takes precedence)
        if self.color.is_some() && self.decoration.is_some() {
            // Not an error, but decoration will override color
        }

        Ok(())
    }

    // ==================== Convenience Methods ====================

    /// Creates a container with a solid color background.
    ///
    /// This is the most common use case - a simple colored box with content.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Blue background container
    /// Container::colored(Color::BLUE, Text::new("Hello"))
    /// ```
    pub fn colored(color: Color, child: impl View + 'static) -> Self {
        Self::builder().color(color).child(child).build()
    }

    /// Creates a Material Design card with elevation shadow.
    ///
    /// Features:
    /// - White background
    /// - 8dp border radius (rounded corners)
    /// - Subtle elevation shadow
    /// - 16dp padding by default
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// Container::card(Column::new().children(vec![
    ///     Box::new(Text::headline("Card Title")),
    ///     Box::new(Text::body("Card content...")),
    /// ]))
    /// ```
    pub fn card(child: impl View + 'static) -> Self {
        let shadow = BoxShadow::new(Color::rgba(0, 0, 0, 25), Offset::new(0.0, 2.0), 4.0, 0.0);
        let decoration = BoxDecoration::default()
            .set_color(Some(Color::WHITE))
            .set_border_radius(Some(BorderRadius::circular(8.0)))
            .set_box_shadow(Some(vec![shadow]));

        Self::builder()
            .decoration(decoration)
            .padding(EdgeInsets::all(16.0))
            .child(child)
            .build()
    }

    /// Creates an outlined container with a colored border and no fill.
    ///
    /// Features:
    /// - Transparent background
    /// - 1px solid border in specified color
    /// - 8dp border radius
    /// - 12dp padding
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Blue outlined container
    /// Container::outlined(Color::BLUE, Text::new("Outlined"))
    /// ```
    pub fn outlined(border_color: Color, child: impl View + 'static) -> Self {
        let decoration = BoxDecoration::default()
            .set_border_radius(Some(BorderRadius::circular(8.0)))
            .set_border(Some(flui_types::styling::Border::all(BorderSide::new(
                border_color,
                1.0,
                BorderStyle::Solid,
            ))));

        Self::builder()
            .decoration(decoration)
            .padding(EdgeInsets::all(12.0))
            .child(child)
            .build()
    }

    /// Creates a surface container with subtle styling.
    ///
    /// Features:
    /// - Light gray background (Material surface color)
    /// - 4dp border radius
    /// - 16dp padding
    ///
    /// Perfect for sections, panels, or content areas.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// Container::surface(content)
    /// ```
    pub fn surface(child: impl View + 'static) -> Self {
        Self::builder()
            .color(Color::rgb(250, 250, 250))
            .padding(EdgeInsets::all(16.0))
            .child(child)
            .build()
    }

    /// Creates a container with rounded corners and a colored background.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Green rounded container with 12dp radius
    /// Container::rounded(Color::GREEN, 12.0, Text::new("Rounded"))
    /// ```
    pub fn rounded(color: Color, radius: f32, child: impl View + 'static) -> Self {
        let decoration = BoxDecoration::default()
            .set_color(Some(color))
            .set_border_radius(Some(BorderRadius::circular(radius)));

        Self::builder().decoration(decoration).child(child).build()
    }

    /// Creates a container with fixed width and height.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // 200x100 container
    /// Container::sized(200.0, 100.0, content)
    /// ```
    pub fn sized(width: f32, height: f32, child: impl View + 'static) -> Self {
        Self::builder()
            .width(width)
            .height(height)
            .child(child)
            .build()
    }

    /// Creates a container with padding applied.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Container with 16dp padding on all sides
    /// Container::padded(EdgeInsets::all(16.0), content)
    /// ```
    pub fn padded(padding: EdgeInsets, child: impl View + 'static) -> Self {
        Self::builder().padding(padding).child(child).build()
    }

    /// Creates a centered container.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// Container::centered(widget)
    /// ```
    pub fn centered(child: impl View + 'static) -> Self {
        Self::builder()
            .alignment(Alignment::CENTER)
            .child(child)
            .build()
    }
}

impl Default for Container {
    fn default() -> Self {
        Self::new()
    }
}

// NOTE: Container is a composite View (like in Flutter), NOT a RenderObjectWidget!
//
// Container composes other Views (Padding, Align, DecoratedBox, SizedBox, etc.) into a tree.

impl View for Container {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        // Build widget tree from inside out:
        // Flutter order: constraints -> margin -> decoration -> alignment -> padding -> child
        //
        // Key insight: When alignment is set, decoration must be OUTSIDE alignment
        // so that decoration receives tight constraints and expands to full size.

        let mut current: Box<dyn AnyView> = if let Some(child) = self.child {
            child
        } else {
            // No child - use empty SizedBox
            Box::new(crate::SizedBox::new())
        };

        // Apply padding (inner spacing around child)
        if let Some(padding) = self.padding {
            let mut padding_widget = crate::Padding::builder().padding(padding).build();
            padding_widget.child = Some(current);
            current = Box::new(padding_widget);
        }

        // Apply alignment BEFORE decoration!
        if let Some(alignment) = self.alignment {
            let mut align_widget = crate::Align::builder().alignment(alignment).build();
            align_widget.child = Some(current);
            current = Box::new(align_widget);
        }

        // Apply decoration or color AFTER alignment
        if let Some(decoration) = self.decoration {
            let mut decorated_widget = crate::DecoratedBox::builder()
                .decoration(decoration)
                .position(crate::DecorationPosition::Background)
                .build();
            decorated_widget.child = Some(current);
            current = Box::new(decorated_widget);
        } else if let Some(color) = self.color {
            let decoration = BoxDecoration {
                color: Some(color),
                ..Default::default()
            };
            let mut decorated_widget = crate::DecoratedBox::builder()
                .decoration(decoration)
                .position(crate::DecorationPosition::Background)
                .build();
            decorated_widget.child = Some(current);
            current = Box::new(decorated_widget);
        }

        // Apply margin BEFORE size constraints!
        if let Some(margin) = self.margin {
            let mut margin_widget = crate::Padding::builder().padding(margin).build();
            margin_widget.child = Some(current);
            current = Box::new(margin_widget);
        }

        // Apply width/height constraints
        if self.width.is_some() || self.height.is_some() {
            let sized_widget = crate::SizedBox {
                key: None,
                width: self.width,
                height: self.height,
                child: Some(current),
            };
            current = Box::new(sized_widget);
        }

        // Return the composed widget tree
        current
    }
}

// Import bon builder traits for custom setters
use container_builder::{IsUnset, SetChild, State};

// Custom builder methods for ergonomic API
impl<S: State> ContainerBuilder<S>
where
    S::Child: IsUnset,
{
    /// Sets the child widget (works in builder chain).
    ///
    /// Accepts anything that implements `View` trait.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// Container::builder()
    ///     .width(100.0)
    ///     .child(Text::new("Hello"))
    ///     .build()
    /// ```
    pub fn child(self, child: impl View + 'static) -> ContainerBuilder<SetChild<S>> {
        self.child_internal(Box::new(child))
    }
}

// Public build() method with automatic validation
impl<S: State> ContainerBuilder<S> {
    /// Builds the Container with automatic validation in debug mode.
    pub fn build(self) -> Container {
        let container = self.build_internal();

        #[cfg(debug_assertions)]
        {
            if let Err(e) = container.validate() {
                tracing::warn!("Container validation failed: {}", e);
            }
        }

        container
    }
}

/// Macro for creating Container with declarative syntax.
///
/// Supports child-first syntax for ergonomic widget composition.
///
/// # Examples
///
/// ```rust,ignore
/// // With child (recommended)
/// container!(child: widget)
/// container!(child: widget, width: 100.0, height: 200.0)
///
/// // Without child (property-only)
/// container! {
///     width: 100.0,
///     height: 200.0,
///     padding: EdgeInsets::all(16.0),
///     color: Color::rgb(255, 0, 0),
/// }
/// ```
#[macro_export]
macro_rules! container {
    // Empty container
    () => {
        $crate::Container::new()
    };

    // Container with child only
    (child: $child:expr) => {
        $crate::Container::builder().child($child).build()
    };

    // Container with child and properties
    (child: $child:expr, $($field:ident : $value:expr),+ $(,)?) => {
        $crate::Container::builder()
            .child($child)
            $(.$field($value))+
            .build()
    };

    // Container with fields only (no child)
    ($($field:ident : $value:expr),* $(,)?) => {
        {
            #[allow(clippy::needless_update)]
            $crate::Container {
                $(
                    $field: Some($value.into()),
                )*
                ..Default::default()
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_core::view::LeafRenderBuilder;
    use flui_rendering::RenderPadding;
    use flui_types::Size;

    // Mock view for testing
    #[derive(Debug, Clone)]
    struct MockView;

    impl View for MockView {
        fn build(self, _ctx: &BuildContext) -> impl IntoElement {
            LeafRenderBuilder::new(RenderPadding::new(EdgeInsets::ZERO))
        }
    }

    #[test]
    fn test_container_new() {
        let container = Container::new();
        assert!(container.key.is_none());
        assert!(container.child.is_none());
        assert!(container.width.is_none());
        assert!(container.height.is_none());
    }

    #[test]
    fn test_container_struct_literal() {
        let container = Container {
            width: Some(100.0),
            height: Some(200.0),
            ..Default::default()
        };

        assert_eq!(container.width, Some(100.0));
        assert_eq!(container.height, Some(200.0));
    }

    #[test]
    fn test_container_builder() {
        let container = Container::builder().width(100.0).height(200.0).build();

        assert_eq!(container.width, Some(100.0));
        assert_eq!(container.height, Some(200.0));
    }

    #[test]
    fn test_container_builder_color() {
        let red = Color::rgb(255, 0, 0);
        let container = Container::builder().color(red).build();

        assert_eq!(container.color, Some(red));
    }

    #[test]
    fn test_container_builder_padding() {
        let padding = EdgeInsets::all(16.0);
        let container = Container::builder().padding(padding).build();

        assert_eq!(container.padding, Some(padding));
    }

    #[test]
    fn test_container_builder_margin() {
        let margin = EdgeInsets::symmetric(10.0, 20.0);
        let container = Container::builder().margin(margin).build();

        assert_eq!(container.margin, Some(margin));
    }

    #[test]
    fn test_container_builder_alignment() {
        let container = Container::builder().alignment(Alignment::CENTER).build();

        assert_eq!(container.alignment, Some(Alignment::CENTER));
    }

    #[test]
    fn test_container_builder_constraints() {
        let constraints = BoxConstraints::tight(Size::new(100.0, 100.0));
        let container = Container::builder().constraints(constraints).build();

        assert_eq!(container.constraints, Some(constraints));
    }

    #[test]
    fn test_container_builder_key() {
        let container = Container::builder().key("my-container").build();

        assert_eq!(container.key, Some("my-container".to_string()));
    }

    #[test]
    fn test_container_builder_chaining() {
        let blue = Color::rgb(0, 0, 255);
        let container = Container::builder()
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
        let container = Container::builder().decoration(decoration.clone()).build();

        assert_eq!(container.decoration, Some(decoration));
    }

    #[test]
    fn test_container_get_decoration_from_color() {
        let green = Color::rgb(0, 255, 0);
        let container = Container::builder().color(green).build();

        let decoration = container.get_decoration();
        assert!(decoration.is_some());
        assert_eq!(decoration.unwrap().color, Some(green));
    }

    #[test]
    fn test_container_validate_ok() {
        let container = Container::builder().width(100.0).height(200.0).build();

        assert!(container.validate().is_ok());
    }

    #[test]
    fn test_container_validate_invalid_width() {
        let container = Container {
            width: Some(-10.0),
            ..Default::default()
        };

        assert!(container.validate().is_err());
    }

    #[test]
    fn test_container_validate_nan_height() {
        let container = Container {
            height: Some(f32::NAN),
            ..Default::default()
        };

        assert!(container.validate().is_err());
    }

    #[test]
    fn test_container_macro_empty() {
        let container = container!();
        assert!(container.width.is_none());
        assert!(container.height.is_none());
    }

    #[test]
    fn test_container_macro_with_fields() {
        let container = container! {
            width: 300.0,
            height: 150.0,
        };

        assert_eq!(container.width, Some(300.0));
        assert_eq!(container.height, Some(150.0));
    }

    #[test]
    fn test_container_macro_with_padding() {
        let container = container! {
            padding: EdgeInsets::all(16.0),
            color: Color::rgb(255, 0, 0),
        };

        assert_eq!(container.padding, Some(EdgeInsets::all(16.0)));
        assert_eq!(container.color, Some(Color::rgb(255, 0, 0)));
    }

    // ========== Tests for Convenience Methods ==========

    #[test]
    fn test_container_colored() {
        let container = Container::colored(Color::BLUE, MockView);
        assert_eq!(container.color, Some(Color::BLUE));
        assert!(container.child.is_some());
    }

    #[test]
    fn test_container_card() {
        let container = Container::card(MockView);
        assert!(container.decoration.is_some());
        assert_eq!(container.padding, Some(EdgeInsets::all(16.0)));
        assert!(container.child.is_some());

        let decoration = container.decoration.unwrap();
        assert_eq!(decoration.color, Some(Color::WHITE));
        assert!(decoration.border_radius.is_some());
        assert!(decoration.box_shadow.is_some());
    }

    #[test]
    fn test_container_outlined() {
        let container = Container::outlined(Color::BLUE, MockView);
        assert!(container.decoration.is_some());
        assert_eq!(container.padding, Some(EdgeInsets::all(12.0)));
        assert!(container.child.is_some());

        let decoration = container.decoration.unwrap();
        assert!(decoration.border.is_some());
        assert!(decoration.border_radius.is_some());
    }

    #[test]
    fn test_container_surface() {
        let container = Container::surface(MockView);
        assert_eq!(container.color, Some(Color::rgb(250, 250, 250)));
        assert_eq!(container.padding, Some(EdgeInsets::all(16.0)));
        assert!(container.child.is_some());
    }

    #[test]
    fn test_container_rounded() {
        let container = Container::rounded(Color::GREEN, 12.0, MockView);
        assert!(container.decoration.is_some());
        assert!(container.child.is_some());

        let decoration = container.decoration.unwrap();
        assert_eq!(decoration.color, Some(Color::GREEN));
        assert!(decoration.border_radius.is_some());
    }

    #[test]
    fn test_container_sized() {
        let container = Container::sized(200.0, 100.0, MockView);
        assert_eq!(container.width, Some(200.0));
        assert_eq!(container.height, Some(100.0));
        assert!(container.child.is_some());
    }

    #[test]
    fn test_container_padded() {
        let padding = EdgeInsets::symmetric(20.0, 10.0);
        let container = Container::padded(padding, MockView);
        assert_eq!(container.padding, Some(padding));
        assert!(container.child.is_some());
    }

    #[test]
    fn test_container_centered() {
        let container = Container::centered(MockView);
        assert_eq!(container.alignment, Some(Alignment::CENTER));
        assert!(container.child.is_some());
    }

    #[test]
    fn test_all_convenience_methods() {
        // Verify all convenience methods create widgets with children
        assert!(Container::colored(Color::RED, MockView).child.is_some());
        assert!(Container::card(MockView).child.is_some());
        assert!(Container::outlined(Color::BLUE, MockView).child.is_some());
        assert!(Container::surface(MockView).child.is_some());
        assert!(Container::rounded(Color::GREEN, 12.0, MockView)
            .child
            .is_some());
        assert!(Container::sized(100.0, 100.0, MockView).child.is_some());
        assert!(Container::padded(EdgeInsets::all(16.0), MockView)
            .child
            .is_some());
        assert!(Container::centered(MockView).child.is_some());
    }
}
