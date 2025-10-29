//! DecoratedBox widget - paints decoration around child
//!
//! A widget that paints a Decoration either before or after its child paints.
//! Similar to Flutter's DecoratedBox widget.
//!
//! # Usage Patterns
//!
//! ## 1. Struct Literal
//! ```rust,ignore
//! DecoratedBox {
//!     decoration: BoxDecoration::default().with_color(Color::RED),
//!     ..Default::default()
//! }
//! ```
//!
//! ## 2. Builder Pattern
//! ```rust,ignore
//! DecoratedBox::builder()
//!     .decoration(BoxDecoration::default().with_color(Color::RED))
//!     .child(some_widget)
//!     .build()
//! ```
//!
//! ## 3. Macro
//! ```rust,ignore
//! decorated_box! {
//!     decoration: BoxDecoration::default().with_color(Color::RED),
//! }
//! ```

use bon::Builder;
use flui_core::{BoxedWidget, DynRenderObject, DynWidget, RenderObjectWidget, SingleChildRenderObjectWidget, Widget};
use flui_rendering::{DecorationPosition, RenderDecoratedBox};
use flui_types::styling::BoxDecoration;

/// A widget that paints a Decoration either before or after its child paints.
///
/// DecoratedBox paints a BoxDecoration around or behind its child.
/// Unlike Container, it does not inset the child by the border widths.
///
/// ## Layout Behavior
///
/// - Passes parent constraints directly to child
/// - Takes the size of its child
/// - Does NOT clip the child (use ClipPath for clipping)
///
/// ## Decoration Position
///
/// - `Background`: Paints decoration behind the child (default)
/// - `Foreground`: Paints decoration in front of the child
///
/// ## Examples
///
/// ```rust,ignore
/// // Simple colored box
/// DecoratedBox::builder()
///     .decoration(
///         BoxDecoration::default()
///             .with_color(Color::from_rgb(255, 0, 0))
///     )
///     .child(Text::new("Hello"))
///     .build()
///
/// // Radial gradient moon on night sky
/// DecoratedBox::builder()
///     .decoration(
///         BoxDecoration::default()
///             .with_gradient(Gradient::radial(
///                 Alignment::new(-0.5, -0.6),  // center
///                 0.15,                         // radius
///                 vec![
///                     Color::from_rgb(238, 238, 238),
///                     Color::from_rgb(17, 17, 51),
///                 ],
///                 Some(vec![0.9, 1.0]),        // stops
///             ))
///     )
///     .build()
///
/// // With border and shadow
/// DecoratedBox::builder()
///     .decoration(
///         BoxDecoration::default()
///             .with_color(Color::WHITE)
///             .with_border_radius(BorderRadius::circular(12.0))
///             .with_box_shadow(BoxShadow::new(
///                 Offset::new(0.0, 4.0),
///                 8.0,  // blur
///                 Color::from_rgba(0, 0, 0, 64),
///             ))
///     )
///     .child(some_widget)
///     .build()
/// ```
///
/// ## See Also
///
/// - Container: Higher-level widget that combines decoration, padding, constraints, etc.
/// - ClipPath: For clipping child to a specific shape
/// - CustomPaint: For custom painting effects
#[derive(Debug, Clone, Builder)]
#[builder(
    on(String, into),
    on(BoxDecoration, into),
    finish_fn = build_decorated_box
)]
pub struct DecoratedBox {
    /// Optional key for widget identification
    pub key: Option<String>,

    /// The decoration to paint.
    ///
    /// Use BoxDecoration to specify color, gradient, border, border radius,
    /// box shadows, and background image.
    #[builder(default = BoxDecoration::default())]
    pub decoration: BoxDecoration,

    /// Whether to paint the decoration in foreground or background.
    ///
    /// - Background: Paint decoration behind child (default)
    /// - Foreground: Paint decoration in front of child
    #[builder(default = DecorationPosition::Background)]
    pub position: DecorationPosition,

    /// The child widget.
    #[builder(setters(vis = "", name = child_internal))]
    pub child: Option<BoxedWidget>,
}

impl DecoratedBox {
    /// Creates a new DecoratedBox with background decoration.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let decorated = DecoratedBox::new(
    ///     BoxDecoration::default().with_color(Color::RED)
    /// );
    /// ```
    pub fn new(decoration: BoxDecoration) -> Self {
        Self {
            key: None,
            decoration,
            position: DecorationPosition::Background,
            child: None,
        }
    }

    /// Creates a DecoratedBox with foreground decoration.
    ///
    /// The decoration will be painted in front of the child,
    /// useful for overlays or masks.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let decorated = DecoratedBox::foreground(
    ///     BoxDecoration::default().with_color(Color::from_rgba(0, 0, 0, 128))
    /// );
    /// ```
    pub fn foreground(decoration: BoxDecoration) -> Self {
        Self {
            key: None,
            decoration,
            position: DecorationPosition::Foreground,
            child: None,
        }
    }

    /// Sets the child widget.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let mut decorated = DecoratedBox::new(decoration);
    /// decorated.set_child(Text::new("Hello"));
    /// ```
    pub fn set_child<W: Widget + 'static>(&mut self, child: W) {
        self.child = Some(BoxedWidget::new(child));
    }

    /// Validates DecoratedBox configuration.
    ///
    /// Currently no validation needed for DecoratedBox,
    /// but provided for consistency with other widgets.
    pub fn validate(&self) -> Result<(), String> {
        // BoxDecoration validates itself
        Ok(())
    }
}

impl Default for DecoratedBox {
    fn default() -> Self {
        Self::new(BoxDecoration::default())
    }
}

// Implement Widget trait with associated type


// Implement RenderObjectWidget
impl RenderObjectWidget for DecoratedBox {
    fn create_render_object(&self) -> Box<dyn DynRenderObject> {
        // Create RenderDecoratedBox with current decoration
        use flui_rendering::DecoratedBoxData;

        let data = DecoratedBoxData::with_position(self.decoration.clone(), self.position);
        let render_decorated = RenderDecoratedBox::new(data);

        Box::new(render_decorated)
    }

    fn update_render_object(&self, render_object: &mut dyn DynRenderObject) {
        // Update RenderDecoratedBox when decoration or position changes
        if let Some(decorated) = render_object.downcast_mut::<RenderDecoratedBox>() {
            decorated.set_decoration(self.decoration.clone());
            decorated.set_position(self.position);
        }
    }
}

// Implement SingleChildRenderObjectWidget
impl SingleChildRenderObjectWidget for DecoratedBox {
    fn child(&self) -> &dyn DynWidget {
        self.child
            .as_ref()
            .map(|b| &**b as &dyn DynWidget)
            .unwrap_or_else(|| panic!("DecoratedBox requires a child"))
    }
}

// bon Builder Extensions
use decorated_box_builder::{IsUnset, SetChild, State};

// Custom setter for child
impl<S: State> DecoratedBoxBuilder<S>
where
    S::Child: IsUnset,
{
    /// Sets the child widget (works in builder chain).
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// DecoratedBox::builder()
    ///     .decoration(BoxDecoration::default().with_color(Color::RED))
    ///     .child(Text::new("Hello"))
    ///     .build()
    /// ```
    pub fn child<W: Widget + 'static>(self, child: W) -> DecoratedBoxBuilder<SetChild<S>> {
        self.child_internal(BoxedWidget::new(child))
    }
}

// Public build() wrapper
impl<S: State> DecoratedBoxBuilder<S> {
    /// Builds the DecoratedBox widget.
    ///
    /// Equivalent to calling the generated `build_decorated_box()` finishing function.
    pub fn build(self) -> DecoratedBox {
        self.build_decorated_box()
    }
}

/// Macro for creating DecoratedBox with declarative syntax.
///
/// # Examples
///
/// ```rust,ignore
/// // Simple decoration
/// decorated_box! {
///     decoration: BoxDecoration::default().with_color(Color::RED),
/// }
///
/// // With foreground position
/// decorated_box! {
///     decoration: my_decoration,
///     position: DecorationPosition::Foreground,
/// }
/// ```
#[macro_export]
macro_rules! decorated_box {
    () => {
        $crate::DecoratedBox::default()
    };
    ($($field:ident : $value:expr),* $(,)?) => {
        $crate::DecoratedBox {
            $($field: $value.into(),)*
            ..Default::default()
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::{Alignment, Color, Offset, EdgeInsets};
    use flui_types::styling::{BorderRadius, BoxShadow, Gradient, LinearGradient, TileMode};
    use flui_core::LeafRenderObjectElement;
    use flui_rendering::RenderPadding;

    #[derive(Debug, Clone)]
    struct MockWidget;

    

    impl RenderObjectWidget for MockWidget {
        fn create_render_object(&self) -> Box<dyn DynRenderObject> {
            Box::new(RenderPadding::new(EdgeInsets::ZERO))
        }

        fn update_render_object(&self, _render_object: &mut dyn DynRenderObject) {}
    }

    impl flui_core::LeafRenderObjectWidget for MockWidget {}

    #[test]
    fn test_decorated_box_new() {
        let decoration = BoxDecoration::with_color(Color::RED);
        let widget = DecoratedBox::new(decoration.clone());

        assert!(widget.key.is_none());
        assert_eq!(widget.decoration, decoration);
        assert_eq!(widget.position, DecorationPosition::Background);
        assert!(widget.child.is_none());
    }

    #[test]
    fn test_decorated_box_foreground() {
        let decoration = BoxDecoration::with_color(Color::BLUE);
        let widget = DecoratedBox::foreground(decoration.clone());

        assert_eq!(widget.decoration, decoration);
        assert_eq!(widget.position, DecorationPosition::Foreground);
    }

    #[test]
    fn test_decorated_box_default() {
        let widget = DecoratedBox::default();
        assert_eq!(widget.decoration, BoxDecoration::default());
        assert_eq!(widget.position, DecorationPosition::Background);
    }

    #[test]
    fn test_decorated_box_builder() {
        let decoration = BoxDecoration::with_color(Color::rgb(255, 0, 0));

        let widget = DecoratedBox::builder()
            .decoration(decoration.clone())
            .build();

        assert_eq!(widget.decoration, decoration);
        assert_eq!(widget.position, DecorationPosition::Background);
    }

    #[test]
    fn test_decorated_box_builder_with_position() {
        let decoration = BoxDecoration::with_color(Color::GREEN);

        let widget = DecoratedBox::builder()
            .decoration(decoration.clone())
            .position(DecorationPosition::Foreground)
            .build();

        assert_eq!(widget.decoration, decoration);
        assert_eq!(widget.position, DecorationPosition::Foreground);
    }

    #[test]
    fn test_decorated_box_struct_literal() {
        let decoration = BoxDecoration::with_color(Color::YELLOW);

        let widget = DecoratedBox {
            decoration: decoration.clone(),
            position: DecorationPosition::Foreground,
            ..Default::default()
        };

        assert_eq!(widget.decoration, decoration);
        assert_eq!(widget.position, DecorationPosition::Foreground);
    }

    #[test]
    fn test_decorated_box_with_gradient() {
        let gradient = Gradient::Linear(LinearGradient::new(
            Alignment::TOP_LEFT,
            Alignment::BOTTOM_RIGHT,
            vec![Color::RED, Color::BLUE],
            None,
            TileMode::Clamp,
        ));

        let decoration = BoxDecoration::with_gradient(gradient);
        let widget = DecoratedBox::new(decoration.clone());

        assert_eq!(widget.decoration, decoration);
    }

    #[test]
    fn test_decorated_box_with_border_radius() {
        let decoration = BoxDecoration::with_color(Color::WHITE)
            .set_border_radius(Some(BorderRadius::circular(12.0)));

        let widget = DecoratedBox::new(decoration.clone());
        assert_eq!(widget.decoration, decoration);
    }

    #[test]
    fn test_decorated_box_with_shadow() {
        let shadow = BoxShadow::new(
            Color::rgba(0, 0, 0, 64),
            Offset::new(0.0, 4.0),
            8.0,    // blur_radius
            0.0,    // spread_radius
        );

        let decoration = BoxDecoration::default()
            .set_box_shadow(Some(vec![shadow]));

        let widget = DecoratedBox::new(decoration.clone());
        assert_eq!(widget.decoration, decoration);
    }

    #[test]
    fn test_decorated_box_validate() {
        let widget = DecoratedBox::default();
        assert!(widget.validate().is_ok());

        let decoration = BoxDecoration::with_color(Color::RED);
        let widget = DecoratedBox::new(decoration);
        assert!(widget.validate().is_ok());
    }

    #[test]
    fn test_decorated_box_render_object_creation() {
        let decoration = BoxDecoration::with_color(Color::RED);
        let widget = DecoratedBox::new(decoration.clone());

        let render_object = widget.create_render_object();
        assert!(render_object.downcast_ref::<RenderDecoratedBox>().is_some());
    }

    #[test]
    fn test_decorated_box_render_object_foreground() {
        let decoration = BoxDecoration::with_color(Color::BLUE);
        let widget = DecoratedBox::foreground(decoration);

        let render_object = widget.create_render_object();
        let decorated = render_object.downcast_ref::<RenderDecoratedBox>().unwrap();
        assert_eq!(decorated.position(), DecorationPosition::Foreground);
    }

    #[test]
    fn test_decorated_box_render_object_update() {
        let decoration1 = BoxDecoration::with_color(Color::RED);
        let widget1 = DecoratedBox::new(decoration1);

        let mut render_object = widget1.create_render_object();

        let decoration2 = BoxDecoration::with_color(Color::BLUE);
        let widget2 = DecoratedBox::new(decoration2.clone());

        widget2.update_render_object(&mut *render_object);

        let decorated = render_object.downcast_ref::<RenderDecoratedBox>().unwrap();
        assert_eq!(decorated.decoration(), Some(&decoration2));
    }

    #[test]
    fn test_decorated_box_macro_empty() {
        let widget = decorated_box!();
        assert_eq!(widget.decoration, BoxDecoration::default());
    }

    #[test]
    fn test_decorated_box_macro_with_decoration() {
        let decoration = BoxDecoration::with_color(Color::RED);
        let widget = decorated_box! {
            decoration: decoration.clone(),
        };
        assert_eq!(widget.decoration, decoration);
    }

    #[test]
    fn test_decorated_box_widget_trait() {
        let widget = DecoratedBox::builder()
            .decoration(BoxDecoration::with_color(Color::BLUE))
            .child(MockWidget)
            .build();

        // Test that it implements Widget and can create an element
        let _element = widget.into_element();
    }

    #[test]
    fn test_decorated_box_builder_with_child() {
        let widget = DecoratedBox::builder()
            .decoration(BoxDecoration::with_color(Color::GREEN))
            .child(MockWidget)
            .build();

        assert!(widget.child.is_some());
    }

    #[test]
    fn test_decorated_box_set_child() {
        let mut widget = DecoratedBox::new(BoxDecoration::with_color(Color::RED));
        widget.set_child(MockWidget);
        assert!(widget.child.is_some());
    }
}
