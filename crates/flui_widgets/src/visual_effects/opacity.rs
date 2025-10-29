//! Opacity widget - applies transparency to child
//!
//! A widget that makes its child partially transparent.
//! Similar to Flutter's Opacity widget.

use bon::Builder;
use flui_core::{BoxedWidget, RenderObjectWidget, SingleChildRenderObjectWidget, Widget};
use flui_rendering::{RenderOpacity, SingleArity};

/// A widget that makes its child partially transparent.
///
/// Opacity adjusts the transparency of its child. The opacity value ranges from
/// 0.0 (fully transparent) to 1.0 (fully opaque).
///
/// ## Layout Behavior
///
/// - Passes constraints directly to child
/// - Takes the size of its child
/// - Does not affect layout, only painting
///
/// ## Performance Considerations
///
/// Applying opacity can be expensive, especially if:
/// - The child has many descendants
/// - The opacity is animated
/// - The opacity is applied to frequently changing content
///
/// For better performance:
/// - Use `opacity: 0.0` to make widget invisible (consider `Visibility` instead)
/// - Use `opacity: 1.0` when fully opaque (no overhead)
/// - Avoid animating opacity on complex widget trees
#[derive(Debug, Clone, Builder)]
#[builder(
    on(String, into),
    finish_fn = build_opacity
)]
pub struct Opacity {
    /// Optional key for widget identification
    pub key: Option<String>,

    /// The opacity value (0.0 = transparent, 1.0 = opaque).
    ///
    /// Must be in the range [0.0, 1.0]:
    /// - 0.0: Fully transparent (invisible)
    /// - 0.5: Semi-transparent
    /// - 1.0: Fully opaque (no transparency)
    ///
    /// Values outside this range will be clamped.
    #[builder(default = 1.0)]
    pub opacity: f32,

    /// The child widget.
    #[builder(setters(vis = "", name = child_internal))]
    pub child: Option<BoxedWidget>,
}

impl Opacity {
    /// Creates a new Opacity widget.
    pub fn new(opacity: f32) -> Self {
        Self {
            key: None,
            opacity: opacity.clamp(0.0, 1.0),
            child: None,
        }
    }

    /// Creates an Opacity widget that is fully transparent.
    pub fn transparent() -> Self {
        Self::new(0.0)
    }

    /// Creates an Opacity widget that is fully opaque.
    pub fn opaque() -> Self {
        Self::new(1.0)
    }

    /// Creates an Opacity widget that is semi-transparent (50%).
    pub fn semi_transparent() -> Self {
        Self::new(0.5)
    }

    /// Sets the child widget.
    pub fn set_child<W>(&mut self, child: W)
    where
        W: Widget + std::fmt::Debug + Send + Sync + Clone + 'static,
    {
        self.child = Some(BoxedWidget::new(child));
    }

    /// Validates Opacity configuration.
    pub fn validate(&self) -> Result<(), String> {
        if self.opacity.is_nan() {
            return Err(
                "Invalid opacity: NaN. Must be a finite number between 0.0 and 1.0.".to_string()
            );
        }

        if !(0.0..=1.0).contains(&self.opacity) {
            return Err(format!(
                "Invalid opacity: {}. Must be between 0.0 and 1.0.",
                self.opacity
            ));
        }

        Ok(())
    }
}

impl Default for Opacity {
    fn default() -> Self {
        Self::opaque()
    }
}

// Implement Widget trait with associated type


// Implement RenderObjectWidget
impl RenderObjectWidget for Opacity {
    type RenderObject = RenderOpacity;
    type Arity = SingleArity;

    fn create_render_object(&self) -> Self::RenderObject {
        RenderOpacity::new(self.opacity)
    }

    fn update_render_object(&self, render_object: &mut Self::RenderObject) {
        render_object.set_opacity(self.opacity);
    }
}

impl SingleChildRenderObjectWidget for Opacity {
    fn child(&self) -> &BoxedWidget {
        self.child
            .as_ref()
            .unwrap_or_else(|| panic!("Opacity requires a child"))
    }
}

// bon Builder Extensions
use opacity_builder::{IsUnset, SetChild, State};

// Custom setter for child
impl<S: State> OpacityBuilder<S>
where
    S::Child: IsUnset,
{
    /// Sets the child widget (works in builder chain).
    pub fn child<W: Widget + 'static>(self, child: W) -> OpacityBuilder<SetChild<S>> {
        self.child_internal(BoxedWidget::new(child))
    }
}

// Public build() wrapper
impl<S: State> OpacityBuilder<S> {
    /// Builds the Opacity widget.
    pub fn build(self) -> Opacity {
        self.build_opacity()
    }
}

/// Macro for creating Opacity with declarative syntax.
#[macro_export]
macro_rules! opacity {
    () => {
        $crate::Opacity::default()
    };
    ($($field:ident : $value:expr),* $(,)?) => {
        $crate::Opacity {
            $($field: $value.into(),)*
            ..Default::default()
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_core::LeafRenderObjectElement;
    use flui_types::EdgeInsets;
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
    fn test_opacity_new() {
        let widget = Opacity::new(0.5);
        assert!(widget.key.is_none());
        assert_eq!(widget.opacity, 0.5);
        assert!(widget.child.is_none());
    }

    #[test]
    fn test_opacity_transparent() {
        let widget = Opacity::transparent();
        assert_eq!(widget.opacity, 0.0);
    }

    #[test]
    fn test_opacity_opaque() {
        let widget = Opacity::opaque();
        assert_eq!(widget.opacity, 1.0);
    }

    #[test]
    fn test_opacity_semi_transparent() {
        let widget = Opacity::semi_transparent();
        assert_eq!(widget.opacity, 0.5);
    }

    #[test]
    fn test_opacity_default() {
        let widget = Opacity::default();
        assert_eq!(widget.opacity, 1.0);
    }

    #[test]
    fn test_opacity_clamp_high() {
        let widget = Opacity::new(1.5);
        assert_eq!(widget.opacity, 1.0);
    }

    #[test]
    fn test_opacity_clamp_low() {
        let widget = Opacity::new(-0.5);
        assert_eq!(widget.opacity, 0.0);
    }

    #[test]
    fn test_opacity_builder() {
        let widget = Opacity::builder()
            .opacity(0.75)
            .build();
        assert_eq!(widget.opacity, 0.75);
    }

    #[test]
    fn test_opacity_struct_literal() {
        let widget = Opacity {
            opacity: 0.3,
            ..Default::default()
        };
        assert_eq!(widget.opacity, 0.3);
    }

    #[test]
    fn test_opacity_validate_ok() {
        let widget = Opacity::new(0.0);
        assert!(widget.validate().is_ok());

        let widget = Opacity::new(0.5);
        assert!(widget.validate().is_ok());

        let widget = Opacity::new(1.0);
        assert!(widget.validate().is_ok());
    }

    #[test]
    fn test_opacity_validate_out_of_range() {
        let widget = Opacity {
            opacity: 1.5,
            ..Default::default()
        };
        assert!(widget.validate().is_err());

        let widget = Opacity {
            opacity: -0.5,
            ..Default::default()
        };
        assert!(widget.validate().is_err());
    }

    #[test]
    fn test_opacity_validate_nan() {
        let widget = Opacity {
            opacity: f32::NAN,
            ..Default::default()
        };
        assert!(widget.validate().is_err());
    }

    #[test]
    fn test_opacity_render_object_creation() {
        let widget = Opacity::new(0.5);
        let render_object = widget.create_render_object();
        assert!(render_object.downcast_ref::<RenderOpacity>().is_some());
    }

    #[test]
    fn test_opacity_render_object_update() {
        let widget1 = Opacity::new(0.5);
        let mut render_object = widget1.create_render_object();

        let widget2 = Opacity::new(0.8);
        widget2.update_render_object(&mut *render_object);

        let opacity_render = render_object.downcast_ref::<RenderOpacity>().unwrap();
        assert_eq!(opacity_render.opacity(), 0.8);
    }

    #[test]
    fn test_opacity_macro_empty() {
        let widget = opacity!();
        assert_eq!(widget.opacity, 1.0);
    }

    #[test]
    fn test_opacity_macro_with_value() {
        let widget = opacity! {
            opacity: 0.25,
        };
        assert_eq!(widget.opacity, 0.25);
    }

    #[test]
    fn test_opacity_zero() {
        let widget = Opacity::new(0.0);
        assert_eq!(widget.opacity, 0.0);
        assert!(widget.validate().is_ok());
    }

    #[test]
    fn test_opacity_one() {
        let widget = Opacity::new(1.0);
        assert_eq!(widget.opacity, 1.0);
        assert!(widget.validate().is_ok());
    }

    #[test]
    fn test_opacity_widget_trait() {
        let widget = Opacity::builder()
            .opacity(0.5)
            .child(MockWidget)
            .build();

        // Test that it implements Widget and can create an element
        let _element = widget.into_element();
    }

    #[test]
    fn test_opacity_builder_with_child() {
        let widget = Opacity::builder()
            .opacity(0.5)
            .child(MockWidget)
            .build();

        assert!(widget.child.is_some());
        assert_eq!(widget.opacity, 0.5);
    }

    #[test]
    fn test_opacity_set_child() {
        let mut widget = Opacity::new(0.7);
        widget.set_child(MockWidget);
        assert!(widget.child.is_some());
    }
}
