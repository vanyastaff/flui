//! Center widget - centers its child
//!
//! A widget that centers its child within itself.
//! Similar to Flutter's Center widget.
//!
//! # Usage Patterns
//!
//! ## 1. Struct Literal
//! ```rust,ignore
//! Center {
//!     child: Some(Box::new(some_widget)),
//!     ..Default::default()
//! }
//! ```
//!
//! ## 2. Builder Pattern
//! ```rust,ignore
//! Center::builder()
//!     .child(some_widget)
//!     .build()
//! ```
//!
//! ## 3. Macro
//! ```rust,ignore
//! center! {}
//! ```

use bon::Builder;
use flui_core::render::RenderNode;
use flui_core::widget::{RenderWidget, Widget};
use flui_core::BuildContext;
use flui_rendering::RenderAlign;
use flui_types::Alignment;

/// A widget that centers its child within the available space.
///
/// Center positions its child at the center of the available space, both horizontally and vertically.
///
/// ## Layout Behavior
///
/// - Centers child both horizontally and vertically
/// - Takes all available space if unconstrained
/// - If `width_factor` or `height_factor` are specified, the Center sizes itself
///   as a multiple of the child's size
///
/// ## Examples
///
/// ```rust,ignore
/// // Simple centering
/// Center::builder()
///     .child(Text::new("Hello"))
///     .build()
///
/// // With size factors
/// Center::builder()
///     .width_factor(2.0)  // Center takes 2x child width
///     .height_factor(1.5) // Center takes 1.5x child height
///     .child(some_widget)
///     .build()
/// ```
#[derive(Debug, Clone, Builder)]
#[builder(
    on(String, into),
    finish_fn = build_center
)]
pub struct Center {
    /// Optional key for widget identification
    pub key: Option<String>,

    /// Multiplier for child width to determine Center width.
    ///
    /// If null, Center takes all available horizontal space.
    /// If non-null, Center width = child width * width_factor.
    pub width_factor: Option<f32>,

    /// Multiplier for child height to determine Center height.
    ///
    /// If null, Center takes all available vertical space.
    /// If non-null, Center height = child height * height_factor.
    pub height_factor: Option<f32>,

    /// The child widget to center.
    #[builder(setters(vis = "", name = child_internal))]
    pub child: Option<Widget>,
}

impl Center {
    /// Creates a new Center widget.
    pub fn new() -> Self {
        Self {
            key: None,
            width_factor: None,
            height_factor: None,
            child: None,
        }
    }

    /// Sets the child widget.
    pub fn set_child(&mut self, child: Widget) {
        self.child = Some(child);
    }

    /// Validates Center configuration.
    pub fn validate(&self) -> Result<(), String> {
        if let Some(width_factor) = self.width_factor {
            if width_factor <= 0.0 || width_factor.is_nan() || width_factor.is_infinite() {
                return Err(format!(
                    "Invalid width_factor: {}. Must be positive and finite.",
                    width_factor
                ));
            }
        }

        if let Some(height_factor) = self.height_factor {
            if height_factor <= 0.0 || height_factor.is_nan() || height_factor.is_infinite() {
                return Err(format!(
                    "Invalid height_factor: {}. Must be positive and finite.",
                    height_factor
                ));
            }
        }

        Ok(())
    }
}

impl Default for Center {
    fn default() -> Self {
        Self::new()
    }
}

// bon Builder Extensions
use center_builder::{IsUnset, SetChild, State};

// Custom child setter
impl<S: State> CenterBuilder<S>
where
    S::Child: IsUnset,
{
    /// Sets the child widget (works in builder chain).
    ///
    /// Accepts anything that implements `IntoWidget` for ergonomic API.
    pub fn child(self, child: impl flui_core::IntoWidget) -> CenterBuilder<SetChild<S>> {
        self.child_internal(child.into_widget())
    }
}

// Build wrapper
impl<S: State> CenterBuilder<S> {
    /// Build the Center and return it as a Widget.
    ///
    /// This automatically wraps the Center in a Widget::render_object() for convenience.
    pub fn build(self) -> flui_core::Widget {
        flui_core::Widget::render(self.build_center())
    }
}

/// Macro for creating Center with declarative syntax.
#[macro_export]
macro_rules! center {
    () => {
        $crate::Center::new()
    };
    ($($field:ident : $value:expr),* $(,)?) => {
        $crate::Center {
            $($field: Some($value.into()),)*
            ..Default::default()
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_rendering::RenderPadding;
    use flui_types::EdgeInsets;

    #[derive(Debug, Clone)]
    struct MockWidget;

    impl RenderWidget for MockWidget {
        fn create_render_object(&self, _context: &BuildContext) -> RenderNode {
            RenderNode::single(Box::new(RenderPadding::new(EdgeInsets::ZERO)))
        }

        fn update_render_object(&self, _context: &BuildContext, _render_object: &mut RenderNode) {}
    }

    #[test]
    fn test_center_new() {
        let center = Center::new();
        assert!(center.key.is_none());
        assert!(center.width_factor.is_none());
        assert!(center.height_factor.is_none());
        assert!(center.child.is_none());
    }

    #[test]
    fn test_center_default() {
        let center = Center::default();
        assert!(center.width_factor.is_none());
    }

    #[test]
    fn test_center_builder() {
        let center = Center::builder().build();
        assert!(center.child.is_none());
    }

    #[test]
    fn test_center_builder_with_child() {
        let center = Center::builder().child(Widget::from(MockWidget)).build();
        assert!(center.child.is_some());
    }

    #[test]
    fn test_center_builder_with_factors() {
        let center = Center::builder()
            .width_factor(2.0)
            .height_factor(1.5)
            .build();
        assert_eq!(center.width_factor, Some(2.0));
        assert_eq!(center.height_factor, Some(1.5));
    }

    #[test]
    fn test_center_set_child() {
        let mut center = Center::new();
        center.set_child(Widget::from(MockWidget));
        assert!(center.child.is_some());
    }

    #[test]
    fn test_center_macro_empty() {
        let center = center!();
        assert!(center.child.is_none());
    }

    #[test]
    fn test_center_macro_with_factors() {
        let center = center! {
            width_factor: 2.0,
        };
        assert_eq!(center.width_factor, Some(2.0));
    }

    #[test]
    fn test_center_validate_ok() {
        let center = Center::builder().width_factor(1.5).build();
        assert!(center.validate().is_ok());
    }

    #[test]
    fn test_center_validate_invalid_width_factor() {
        let center = Center {
            width_factor: Some(-1.0),
            ..Default::default()
        };
        assert!(center.validate().is_err());
    }

    #[test]
    fn test_center_validate_zero_height_factor() {
        let center = Center {
            height_factor: Some(0.0),
            ..Default::default()
        };
        assert!(center.validate().is_err());
    }

    #[test]
    fn test_center_widget_trait() {
        let _widget = Center::builder().child(Widget::from(MockWidget)).build();

        // Test that it implements Widget
        // Widget creation is tested through the builder pattern
    }

    #[test]
    fn test_single_child_render_object_widget_trait() {
        let widget = Center::builder()
            .width_factor(2.0)
            .child(Widget::from(MockWidget))
            .build();

        // Test child() method
        assert!(widget.child.is_some());
    }
}

// Implement RenderWidget
impl RenderWidget for Center {
    fn create_render_object(&self, _context: &BuildContext) -> RenderNode {
        RenderNode::single(Box::new(RenderAlign::with_factors(
            Alignment::CENTER,
            self.width_factor,
            self.height_factor,
        )))
    }

    fn update_render_object(&self, _context: &BuildContext, render_object: &mut RenderNode) {
        if let RenderNode::Single { render, .. } = render_object {
            if let Some(align) = render.downcast_mut::<RenderAlign>() {
                align.set_alignment(Alignment::CENTER);
                align.set_width_factor(self.width_factor);
                align.set_height_factor(self.height_factor);
            }
        }
    }

    fn child(&self) -> Option<&Widget> {
        self.child.as_ref()
    }
}

// Implement IntoWidget for ergonomic API
flui_core::impl_into_widget!(Center, render);
