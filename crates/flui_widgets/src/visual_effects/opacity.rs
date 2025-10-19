//! Opacity widget - applies transparency to child
//!
//! A widget that makes its child partially transparent.
//! Similar to Flutter's Opacity widget.
//!
//! # Usage Patterns
//!
//! ## 1. Struct Literal
//! ```rust,ignore
//! Opacity {
//!     opacity: 0.5,
//!     ..Default::default()
//! }
//! ```
//!
//! ## 2. Builder Pattern
//! ```rust,ignore
//! Opacity::builder()
//!     .opacity(0.5)
//!     .child(some_widget)
//!     .build()
//! ```
//!
//! ## 3. Macro
//! ```rust,ignore
//! opacity! {
//!     opacity: 0.5,
//! }
//! ```

use bon::Builder;
use flui_core::{RenderObject, RenderObjectWidget, Widget};
use flui_rendering::RenderOpacity;

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
///
/// ## Examples
///
/// ```rust,ignore
/// // Semi-transparent image
/// Opacity::builder()
///     .opacity(0.5)
///     .child(Image::network(url))
///     .build()
///
/// // Fade out effect
/// Opacity::builder()
///     .opacity(0.2)
///     .child(Text::new("Faded text"))
///     .build()
///
/// // Fully transparent (invisible)
/// Opacity::builder()
///     .opacity(0.0)
///     .child(widget)
///     .build()
/// ```
///
/// ## See Also
///
/// - AnimatedOpacity: For animated opacity transitions
/// - Visibility: For hiding widgets without rendering overhead
/// - FadeTransition: For animation-based fading
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
    pub child: Option<Box<dyn Widget>>,
}

impl Opacity {
    /// Creates a new Opacity widget.
    ///
    /// # Arguments
    ///
    /// * `opacity` - The opacity value (0.0 to 1.0)
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Semi-transparent
    /// let widget = Opacity::new(0.5);
    ///
    /// // Fully opaque
    /// let widget = Opacity::new(1.0);
    ///
    /// // Fully transparent
    /// let widget = Opacity::new(0.0);
    /// ```
    pub fn new(opacity: f32) -> Self {
        Self {
            key: None,
            opacity: opacity.clamp(0.0, 1.0),
            child: None,
        }
    }

    /// Creates an Opacity widget that is fully transparent.
    ///
    /// Equivalent to `Opacity::new(0.0)`.
    pub fn transparent() -> Self {
        Self::new(0.0)
    }

    /// Creates an Opacity widget that is fully opaque.
    ///
    /// Equivalent to `Opacity::new(1.0)`.
    pub fn opaque() -> Self {
        Self::new(1.0)
    }

    /// Creates an Opacity widget that is semi-transparent (50%).
    ///
    /// Equivalent to `Opacity::new(0.5)`.
    pub fn semi_transparent() -> Self {
        Self::new(0.5)
    }

    /// Sets the child widget.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let mut widget = Opacity::new(0.5);
    /// widget.set_child(Text::new("Hello"));
    /// ```
    pub fn set_child(&mut self, child: impl Widget + 'static) {
        self.child = Some(Box::new(child));
    }

    /// Validates Opacity configuration.
    ///
    /// Returns an error if opacity is not in range [0.0, 1.0] or is NaN.
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

impl Widget for Opacity {
    fn create_element(&self) -> Box<dyn flui_core::Element> {
        Box::new(flui_core::RenderObjectElement::new(self.clone()))
    }
}

impl RenderObjectWidget for Opacity {
    fn create_render_object(&self) -> Box<dyn RenderObject> {
        Box::new(RenderOpacity::new(self.opacity.clamp(0.0, 1.0)))
    }

    fn update_render_object(&self, render_object: &mut dyn RenderObject) {
        if let Some(opacity_render) = render_object.downcast_mut::<RenderOpacity>() {
            opacity_render.set_opacity(self.opacity.clamp(0.0, 1.0));
        }
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
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// Opacity::builder()
    ///     .opacity(0.5)
    ///     .child(Text::new("Hello"))
    ///     .build()
    /// ```
    pub fn child(self, child: impl Widget + 'static) -> OpacityBuilder<SetChild<S>> {
        self.child_internal(Box::new(child) as Box<dyn Widget>)
    }
}

// Public build() wrapper
impl<S: State> OpacityBuilder<S> {
    /// Builds the Opacity widget.
    ///
    /// Equivalent to calling the generated `build_opacity()` finishing function.
    pub fn build(self) -> Opacity {
        self.build_opacity()
    }
}

/// Macro for creating Opacity with declarative syntax.
///
/// # Examples
///
/// ```rust,ignore
/// // Semi-transparent
/// opacity! {
///     opacity: 0.5,
/// }
///
/// // Fully transparent
/// opacity! {
///     opacity: 0.0,
/// }
/// ```
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
}
