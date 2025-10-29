//! SizedBox widget - a box with fixed dimensions
//!
//! A widget that forces its child to have a specific width and/or height.
//! Similar to Flutter's SizedBox widget.
//!
//! # Usage Patterns
//!
//! ## 1. Struct Literal
//! ```rust,ignore
//! SizedBox {
//!     width: Some(100.0),
//!     height: Some(50.0),
//!     ..Default::default()
//! }
//! ```
//!
//! ## 2. Builder Pattern
//! ```rust,ignore
//! SizedBox::builder()
//!     .width(100.0)
//!     .height(50.0)
//!     .build()
//! ```
//!
//! ## 3. Macro
//! ```rust,ignore
//! sized_box! {
//!     width: 100.0,
//!     height: 50.0,
//! }
use bon::Builder;
use flui_core::{BoxedWidget, DynRenderObject, DynWidget, RenderObjectWidget, SingleChildRenderObjectWidget, Widget};
use flui_rendering::RenderConstrainedBox;

/// A box with a specified size.
///
/// If a child is provided, it will be constrained to the specified size.
/// If no child is provided, the SizedBox will create an empty box with the specified dimensions.
///
/// ## Layout Behavior
///
/// - If both width and height are provided, the box has a tight size constraint
/// - If only width is provided, height is unconstrained
/// - If only height is provided, width is unconstrained
/// - If neither is provided, behaves like an empty container
///
/// ## Examples
///
/// ```rust,ignore
/// // Fixed size box
/// SizedBox::builder()
///     .width(100.0)
///     .height(100.0)
///     .build()
///
/// // Fixed width, flexible height
/// SizedBox::builder()
///     .width(200.0)
///     .child(some_widget)
///     .build()
///
/// // Create spacing
/// SizedBox::builder()
///     .height(20.0)  // 20px vertical spacing
///     .build()
/// ```
#[derive(Debug, Clone, Builder)]
#[builder(
    on(String, into),
    finish_fn = build_sized_box
)]
pub struct SizedBox {
    /// Optional key for widget identification
    pub key: Option<String>,

    /// The width of this box.
    ///
    /// If null, the box will match the width of its child (or be zero if no child).
    pub width: Option<f32>,

    /// The height of this box.
    ///
    /// If null, the box will match the height of its child (or be zero if no child).
    pub height: Option<f32>,

    /// The child widget to constrain.
    ///
    /// If null, the box will be empty with the specified dimensions.
    #[builder(setters(vis = "", name = child_internal))]
    pub child: Option<BoxedWidget>,
}

impl SizedBox {
    /// Creates a new empty SizedBox with no constraints.
    pub fn new() -> Self {
        Self {
            key: None,
            width: None,
            height: None,
            child: None,
        }
    }

    /// Creates a SizedBox that expands to fill available space.
    ///
    /// This is equivalent to a SizedBox with width and height set to f32::INFINITY.
    pub fn expand() -> Self {
        Self {
            key: None,
            width: Some(f32::INFINITY),
            height: Some(f32::INFINITY),
            child: None,
        }
    }

    /// Creates a square SizedBox with the same width and height.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// SizedBox::square(100.0)  // 100x100 box
    /// ```
    pub fn square(size: f32) -> Self {
        Self {
            key: None,
            width: Some(size),
            height: Some(size),
            child: None,
        }
    }

    /// Creates a SizedBox with no size (shrinks to zero).
    ///
    /// Useful for creating invisible spacing or placeholders.
    pub fn shrink() -> Self {
        Self {
            key: None,
            width: Some(0.0),
            height: Some(0.0),
            child: None,
        }
    }

    /// Sets the child widget.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let mut sized_box = SizedBox::square(100.0);
    /// sized_box.set_child(some_widget);
    /// ```
    pub fn set_child(&mut self, child: impl Widget + 'static) {
        self.child = Some(BoxedWidget::new(child));
    }

    /// Validates SizedBox configuration.
    pub fn validate(&self) -> Result<(), String> {
        if let Some(width) = self.width {
            if width < 0.0 || width.is_nan() {
                return Err(format!("Invalid width: {}. Width must be non-negative and finite (or infinity).", width));
            }
        }

        if let Some(height) = self.height {
            if height < 0.0 || height.is_nan() {
                return Err(format!("Invalid height: {}. Height must be non-negative and finite (or infinity).", height));
            }
        }

        Ok(())
    }
}

impl Default for SizedBox {
    fn default() -> Self {
        Self::new()
    }
}



// bon Builder Extensions
use sized_box_builder::{IsUnset, SetChild, State};

// Custom child setter
impl<S: State> SizedBoxBuilder<S>
where
    S::Child: IsUnset,
{
    /// Sets the child widget (works in builder chain).
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// SizedBox::builder()
    ///     .width(100.0)
    ///     .child(some_widget)
    ///     .build()
    /// ```
    pub fn child<W: Widget + 'static>(self, child: W) -> SizedBoxBuilder<SetChild<S>> {
        self.child_internal(BoxedWidget::new(child))
    }
}

// Build wrapper
impl<S: State> SizedBoxBuilder<S> {
    /// Builds the SizedBox widget.
    pub fn build(self) -> SizedBox {
        self.build_sized_box()
    }
}

/// Macro for creating SizedBox with declarative syntax.
///
/// # Examples
///
/// ```rust,ignore
/// sized_box! {
///     width: 100.0,
///     height: 50.0,
/// }
/// ```
#[macro_export]
macro_rules! sized_box {
    () => {
        $crate::SizedBox::new()
    };
    ($($field:ident : $value:expr),* $(,)?) => {
        $crate::SizedBox {
            $($field: Some($value.into()),)*
            ..Default::default()
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    // Mock widget for testing
    #[derive(Debug, Clone)]
    struct MockWidget;

    

    impl RenderObjectWidget for MockWidget {
        fn create_render_object(&self) -> Box<dyn DynRenderObject> {
            use flui_core::{BoxConstraints, EdgeInsets};
            use flui_rendering::RenderPadding;
            Box::new(RenderPadding::new(EdgeInsets::ZERO))
        }

        fn update_render_object(&self, _render_object: &mut dyn DynRenderObject) {}
    }

    impl flui_core::LeafRenderObjectWidget for MockWidget {}

    #[test]
    fn test_sized_box_new() {
        let sized_box = SizedBox::new();
        assert!(sized_box.key.is_none());
        assert!(sized_box.width.is_none());
        assert!(sized_box.height.is_none());
        assert!(sized_box.child.is_none());
    }

    #[test]
    fn test_sized_box_default() {
        let sized_box = SizedBox::default();
        assert!(sized_box.width.is_none());
        assert!(sized_box.height.is_none());
    }

    #[test]
    fn test_sized_box_struct_literal() {
        let sized_box = SizedBox {
            width: Some(100.0),
            height: Some(50.0),
            ..Default::default()
        };
        assert_eq!(sized_box.width, Some(100.0));
        assert_eq!(sized_box.height, Some(50.0));
    }

    #[test]
    fn test_sized_box_builder() {
        let sized_box = SizedBox::builder()
            .width(100.0)
            .build();
        assert_eq!(sized_box.width, Some(100.0));
        assert!(sized_box.height.is_none());
    }

    #[test]
    fn test_sized_box_builder_chaining() {
        let sized_box = SizedBox::builder()
            .width(200.0)
            .height(100.0)
            .build();

        assert_eq!(sized_box.width, Some(200.0));
        assert_eq!(sized_box.height, Some(100.0));
    }

    #[test]
    fn test_sized_box_expand() {
        let sized_box = SizedBox::expand();
        assert_eq!(sized_box.width, Some(f32::INFINITY));
        assert_eq!(sized_box.height, Some(f32::INFINITY));
    }

    #[test]
    fn test_sized_box_square() {
        let sized_box = SizedBox::square(100.0);
        assert_eq!(sized_box.width, Some(100.0));
        assert_eq!(sized_box.height, Some(100.0));
    }

    #[test]
    fn test_sized_box_shrink() {
        let sized_box = SizedBox::shrink();
        assert_eq!(sized_box.width, Some(0.0));
        assert_eq!(sized_box.height, Some(0.0));
    }

    #[test]
    fn test_sized_box_set_child() {
        let mut sized_box = SizedBox::new();
        sized_box.set_child(MockWidget);
        assert!(sized_box.child.is_some());
    }

    #[test]
    fn test_sized_box_builder_with_child() {
        let sized_box = SizedBox::builder()
            .width(100.0)
            .child(MockWidget)
            .build();
        assert!(sized_box.child.is_some());
    }

    #[test]
    fn test_sized_box_macro_empty() {
        let sized_box = sized_box!();
        assert!(sized_box.width.is_none());
    }

    #[test]
    fn test_sized_box_macro_with_fields() {
        let sized_box = sized_box! {
            width: 100.0,
            height: 50.0,
        };
        assert_eq!(sized_box.width, Some(100.0));
        assert_eq!(sized_box.height, Some(50.0));
    }

    #[test]
    fn test_sized_box_validate_ok() {
        let sized_box = SizedBox::builder()
            .width(100.0)
            .height(50.0)
            .build();
        assert!(sized_box.validate().is_ok());
    }

    #[test]
    fn test_sized_box_validate_invalid_width() {
        let sized_box = SizedBox {
            width: Some(-1.0),
            ..Default::default()
        };
        assert!(sized_box.validate().is_err());
    }

    #[test]
    fn test_sized_box_validate_invalid_height() {
        let sized_box = SizedBox {
            height: Some(f32::NAN),
            ..Default::default()
        };
        assert!(sized_box.validate().is_err());
    }

    #[test]
    fn test_sized_box_validate_infinity_ok() {
        let sized_box = SizedBox::expand();
        assert!(sized_box.validate().is_ok());
    }

    #[test]
    fn test_sized_box_only_width() {
        let sized_box = SizedBox::builder()
            .width(100.0)
            .build();
        assert_eq!(sized_box.width, Some(100.0));
        assert!(sized_box.height.is_none());
    }

    #[test]
    fn test_sized_box_only_height() {
        let sized_box = SizedBox::builder()
            .height(50.0)
            .build();
        assert!(sized_box.width.is_none());
        assert_eq!(sized_box.height, Some(50.0));
    }

    #[test]
    fn test_widget_trait() {
        let sized_box = SizedBox::builder()
            .width(100.0)
            .child(MockWidget)
            .build();

        // Test that it implements Widget and can create an element
        let _element = sized_box.into_element();
    }

    #[test]
    fn test_single_child_render_object_widget_trait() {
        let sized_box = SizedBox::builder()
            .width(100.0)
            .child(MockWidget)
            .build();

        // Test child() method
        let _child = sized_box.child();
    }
}

impl RenderObjectWidget for SizedBox {
    fn create_render_object(&self) -> Box<dyn DynRenderObject> {
        use flui_core::BoxConstraints;
        use flui_rendering::objects::layout::constrained_box::ConstrainedBoxData;

        // Create tight constraints for specified dimensions
        let constraints = BoxConstraints::tight_for(self.width, self.height);
        Box::new(RenderConstrainedBox::new(ConstrainedBoxData::new(constraints)))
    }

    fn update_render_object(&self, render_object: &mut dyn DynRenderObject) {
        use flui_core::BoxConstraints;
        if let Some(constrained) = render_object.downcast_mut::<RenderConstrainedBox>() {
            let constraints = BoxConstraints::tight_for(self.width, self.height);
            constrained.set_additional_constraints(constraints);
        }
    }
}

impl SingleChildRenderObjectWidget for SizedBox {
    fn child(&self) -> &dyn DynWidget {
        self.child
            .as_ref()
            .map(|b| &**b as &dyn DynWidget)
            .expect("SizedBox requires a child widget")
    }
}
