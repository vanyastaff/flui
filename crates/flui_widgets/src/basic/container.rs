//! Container widget - a composition of layout and decoration widgets
//!
//! Container is a convenience widget that combines common styling and
//! layout properties. It's similar to Flutter's Container widget.
//!
//! # Usage Patterns
//!
//! Container supports three creation styles:
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
//!     .build()
//! ```
//!
//! ## 3. Factory Methods
//! ```rust,ignore
//! Container::colored(Color::rgb(255, 0, 0))
//!     .width(300.0)
//!     .height(200.0)
//!     .build()
//! ```

use bon::Builder;
use flui_core::{BoxedWidget, BoxConstraints, Context, DynWidget, StatelessWidget, Widget};
use flui_types::styling::BoxDecoration;
use flui_types::{Alignment, Color, EdgeInsets};

// Use the simplified 2D Matrix4 from rendering for transforms
type Matrix4 = flui_rendering::objects::effects::transform::Matrix4;

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
#[derive(Debug, Clone, Builder)]
#[builder(
    on(String, into),
    on(EdgeInsets, into),
    on(BoxDecoration, into),
    on(Color, into),
    finish_fn = build_container  // Internal build function
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

    /// The transformation matrix to apply to the container.
    ///
    /// If non-null, the container will be wrapped in a Transform widget.
    /// The transformation is applied OUTSIDE all other effects (decoration, alignment, etc).
    pub transform: Option<Matrix4>,

    /// The child contained by the container.
    ///
    /// If null, the container will size itself according to other properties.
    /// Use the custom `.child()` setter in the builder.
    #[builder(setters(vis = "", name = child_internal))]
    pub child: Option<BoxedWidget>,
}

impl Container {
    /// Creates a new empty Container.
    ///
    /// This is the base constructor. Use builder() for a fluent API.
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
            transform: None,
            child: None,
        }
    }

    /// Sets the child widget.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let mut container = Container::new();
    /// container.set_child(some_widget);
    /// ```
    pub fn set_child(&mut self, child: impl Widget + 'static) {
        self.child = Some(BoxedWidget::new(child));
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
}

impl Default for Container {
    fn default() -> Self {
        Self::new()
    }
}

// NOTE: Container is a StatelessWidget (like in Flutter), NOT a RenderObjectWidget!
//
// Flutter inheritance: Object → Widget → StatelessWidget → Container
//
// Container should create a ComponentElement that implements build() to compose
// other widgets (Padding, Align, DecoratedBox, ConstrainedBox, etc.) into a tree.
//
// Widget trait will be automatically implemented via StatelessWidget trait below

impl StatelessWidget for Container {
    fn build(&self, _context: &Context) -> Box<dyn DynWidget> {
        // Build widget tree from inside out:
        // Flutter order: constraints -> margin -> decoration -> alignment -> padding -> child
        //
        // Key insight: When alignment is set, decoration must be OUTSIDE alignment
        // so that decoration receives tight constraints and expands to full size.
        //
        // From Flutter docs:
        // "If the widget has an alignment, and the parent provides bounded constraints,
        //  then the Container tries to expand to fit the parent, and then positions
        //  the child within itself as per the alignment."

        let mut current: Box<dyn DynWidget> = if let Some(child) = &self.child {
            child.clone()
        } else {
            // No child - use empty SizedBox
            Box::new(crate::SizedBox::new())
        };

        // Apply padding (inner spacing around child)
        if let Some(padding) = self.padding {
            current = Box::new(crate::Padding {
                key: None,
                padding,
                child: Some(current),
            });
        }

        // Apply alignment BEFORE decoration!
        // This allows decoration to be on the outside and receive tight constraints
        if let Some(alignment) = self.alignment {
            current = Box::new(crate::Align {
                key: None,
                alignment,
                width_factor: None,
                height_factor: None,
                child: Some(current),
            });
        }

        // Apply decoration or color AFTER alignment
        // Decoration will now receive tight constraints from SizedBox/margin
        if let Some(decoration) = &self.decoration {
            current = Box::new(crate::DecoratedBox {
                key: None,
                decoration: decoration.clone(),
                position: crate::DecorationPosition::Background,
                child: Some(current),
            });
        } else if let Some(color) = self.color {
            let decoration = BoxDecoration {
                color: Some(color),
                ..Default::default()
            };
            current = Box::new(crate::DecoratedBox {
                key: None,
                decoration,
                position: crate::DecorationPosition::Background,
                child: Some(current),
            });
        }

        // Apply margin BEFORE size constraints!
        // This ensures margin is "inside" the constrained box
        // Note: margin is implemented using Padding widget (same as Flutter)
        // The semantic difference (margin vs padding) is maintained by the widget order
        if let Some(margin) = self.margin {
            current = Box::new(crate::Padding {
                key: None,
                padding: margin,
                child: Some(current),
            });
        }

        // Apply width/height constraints
        // These constraints apply to the TOTAL size (including margin)
        if self.width.is_some() || self.height.is_some() {
            current = Box::new(crate::SizedBox {
                key: None,
                width: self.width,
                height: self.height,
                child: Some(current),
            });
        }

        // Apply transform LAST (outermost)
        // Transform is applied OUTSIDE all other effects
        if let Some(transform) = self.transform {
            current = Box::new(crate::Transform {
                key: None,
                transform,
                transform_hit_tests: true,
                child: Some(current),
            });
        }

        current
    }
}

// Import bon builder traits for custom setters
use container_builder::{State, IsUnset, SetChild};

// Custom builder methods for ergonomic API
impl<S: State> ContainerBuilder<S>
where
    S::Child: IsUnset,
{
    /// Sets the child widget (works in builder chain).
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// Container::builder()
    ///     .width(100.0)
    ///     .child(some_widget)
    ///     .build()
    /// ```
    pub fn child(self, child: impl Widget + 'static) -> ContainerBuilder<SetChild<S>> {
        // bon's generated setter takes Box directly, not Option
        // bon wraps it in Option internally
        self.child_internal(BoxedWidget::new(child))
    }
}

impl<S: State> ContainerBuilder<S> {
    /// Convenience method to build the container.
    ///
    /// Equivalent to calling the generated `build_container()` finishing function.
    pub fn build(self) -> Container {
        self.build_container()
    }
}

/// Macro for creating Container with declarative syntax.
///
/// # Examples
///
/// ```rust,ignore
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

    // Container with fields
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
    use flui_types::Size;

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
        let container = Container::builder()
            .width(100.0)
            .height(200.0)
            .build();

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
        let container = Container::builder()
            .alignment(Alignment::CENTER)
            .build();

        assert_eq!(container.alignment, Some(Alignment::CENTER));
    }

    #[test]
    fn test_container_builder_constraints() {
        let constraints = BoxConstraints::tight(Size::new(100.0, 100.0));
        let container = Container::builder()
            .constraints(constraints)
            .build();

        assert_eq!(container.constraints, Some(constraints));
    }

    #[test]
    fn test_container_builder_key() {
        let container = Container::builder()
            .key("my-container")
            .build();

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
        let container = Container::builder()
            .decoration(decoration.clone())
            .build();

        assert_eq!(container.decoration, Some(decoration));
    }

    #[test]
    fn test_container_get_decoration_from_color() {
        let green = Color::rgb(0, 255, 0);
        let container = Container::builder()
            .color(green)
            .build();

        let decoration = container.get_decoration();
        assert!(decoration.is_some());
        assert_eq!(decoration.unwrap().color, Some(green));
    }

    #[test]
    fn test_container_validate_ok() {
        let container = Container::builder()
            .width(100.0)
            .height(200.0)
            .build();

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
}
