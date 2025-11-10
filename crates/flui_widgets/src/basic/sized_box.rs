//! SizedBox widget - a box with fixed dimensions
//!
//! A widget that forces its child to have a specific width and/or height.
//! Similar to Flutter's SizedBox widget.
//!
//! # Usage Patterns
//!
//! ## 1. Convenience Methods (Recommended)
//! ```rust,ignore
//! // Square box
//! SizedBox::square(100.0, child)
//!
//! // Fixed width and height
//! SizedBox::from_size(100.0, 50.0, child)
//!
//! // Width only
//! SizedBox::width_only(200.0, child)
//!
//! // Height only
//! SizedBox::height_only(100.0, child)
//!
//! // Expand to fill
//! SizedBox::expand(child)
//! ```
//!
//! ## 2. Builder Pattern
//! ```rust,ignore
//! SizedBox::builder()
//!     .width(100.0)
//!     .height(50.0)
//!     .child(widget)
//!     .build()
//! ```
//!
//! ## 3. Macro
//! ```rust,ignore
//! sized_box!(child: widget, width: 100.0, height: 50.0)
//! ```
use bon::Builder;
use flui_core::view::{AnyView, IntoElement, View};
use flui_core::BuildContext;
use flui_rendering::RenderConstrainedBox;
use flui_types::BoxConstraints;

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
#[derive(Builder)]
#[builder(
    on(String, into),
    finish_fn(name = build_internal, vis = "")
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
    pub child: Option<Box<dyn AnyView>>,
}

impl std::fmt::Debug for SizedBox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SizedBox")
            .field("key", &self.key)
            .field("width", &self.width)
            .field("height", &self.height)
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

impl Clone for SizedBox {
    fn clone(&self) -> Self {
        Self {
            key: self.key.clone(),
            width: self.width,
            height: self.height,
            child: self.child.clone(),
        }
    }
}

impl SizedBox {
    /// Creates a new empty SizedBox with no constraints.
    ///
    /// Note: Prefer using convenience methods like `SizedBox::square()` for most cases.
    pub const fn new() -> Self {
        Self {
            key: None,
            width: None,
            height: None,
            child: None,
        }
    }

    // ========== Convenience Methods with Child ==========

    /// Creates a square SizedBox with the same width and height.
    ///
    /// Perfect for avatars, icons, or any square content.
    ///
    /// # Example
    /// ```rust,ignore
    /// SizedBox::square(100.0, icon)  // 100x100 box
    /// ```
    pub fn square(size: f32, child: impl View + 'static) -> Self {
        Self::builder()
            .width(size)
            .height(size)
            .child(child)
            .build()
    }

    /// Creates a SizedBox with specific width and height.
    ///
    /// Most common use case - fixed dimensions.
    ///
    /// # Example
    /// ```rust,ignore
    /// SizedBox::from_size(200.0, 100.0, widget)
    /// ```
    pub fn from_size(width: f32, height: f32, child: impl View + 'static) -> Self {
        Self::builder()
            .width(width)
            .height(height)
            .child(child)
            .build()
    }

    /// Creates a SizedBox with only width constrained.
    ///
    /// Height will match the child's natural height.
    ///
    /// # Example
    /// ```rust,ignore
    /// SizedBox::width_only(200.0, flexible_height_content)
    /// ```
    pub fn width_only(width: f32, child: impl View + 'static) -> Self {
        Self::builder().width(width).child(child).build()
    }

    /// Creates a SizedBox with only height constrained.
    ///
    /// Width will match the child's natural width.
    ///
    /// # Example
    /// ```rust,ignore
    /// SizedBox::height_only(100.0, flexible_width_content)
    /// ```
    pub fn height_only(height: f32, child: impl View + 'static) -> Self {
        Self::builder().height(height).child(child).build()
    }

    /// Creates a SizedBox that expands to fill available space.
    ///
    /// Useful for making a widget fill its parent container.
    ///
    /// # Example
    /// ```rust,ignore
    /// SizedBox::expand(content)  // Fills parent
    /// ```
    pub fn expand(child: impl View + 'static) -> Self {
        Self::builder()
            .width(f32::INFINITY)
            .height(f32::INFINITY)
            .child(child)
            .build()
    }

    // ========== Spacing Helpers (No Child) ==========

    /// Creates a SizedBox with no size (shrinks to zero).
    ///
    /// Useful for creating invisible placeholders.
    ///
    /// # Example
    /// ```rust,ignore
    /// SizedBox::shrink()  // 0x0 box
    /// ```
    pub fn shrink() -> Self {
        Self {
            key: None,
            width: Some(0.0),
            height: Some(0.0),
            child: None,
        }
    }

    /// Creates horizontal spacing (width only, no height, no child).
    ///
    /// Perfect for adding space between elements in a Row.
    ///
    /// # Example
    /// ```rust,ignore
    /// Row::new().children(vec![
    ///     Box::new(widget1),
    ///     Box::new(SizedBox::h_space(20.0)),
    ///     Box::new(widget2),
    /// ])
    /// ```
    pub fn h_space(width: f32) -> Self {
        Self {
            key: None,
            width: Some(width),
            height: None,
            child: None,
        }
    }

    /// Creates vertical spacing (height only, no width, no child).
    ///
    /// Perfect for adding space between elements in a Column.
    ///
    /// # Example
    /// ```rust,ignore
    /// Column::new().children(vec![
    ///     Box::new(widget1),
    ///     Box::new(SizedBox::v_space(20.0)),
    ///     Box::new(widget2),
    /// ])
    /// ```
    pub fn v_space(height: f32) -> Self {
        Self {
            key: None,
            width: None,
            height: Some(height),
            child: None,
        }
    }

    /// Validates SizedBox configuration.
    pub fn validate(&self) -> Result<(), String> {
        if let Some(width) = self.width {
            if width < 0.0 || width.is_nan() {
                return Err(format!(
                    "Invalid width: {}. Width must be non-negative and finite (or infinity).",
                    width
                ));
            }
        }

        if let Some(height) = self.height {
            if height < 0.0 || height.is_nan() {
                return Err(format!(
                    "Invalid height: {}. Height must be non-negative and finite (or infinity).",
                    height
                ));
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
    pub fn child(self, child: impl View + 'static) -> SizedBoxBuilder<SetChild<S>> {
        self.child_internal(Box::new(child))
    }
}

// Build wrapper with validation
impl<S: State> SizedBoxBuilder<S> {
    /// Builds the SizedBox widget with automatic validation in debug mode.
    pub fn build(self) -> SizedBox {
        let sized_box = self.build_internal();

        // In debug mode, validate configuration and warn on issues
        #[cfg(debug_assertions)]
        if let Err(e) = sized_box.validate() {
            tracing::warn!("SizedBox validation warning: {}", e);
        }

        sized_box
    }
}

/// Macro for creating SizedBox with declarative syntax.
///
/// # Examples
///
/// ```rust,ignore
/// // Empty sized box
/// sized_box!()
///
/// // With child only (no constraints)
/// sized_box!(child: Text::new("Hello"))
///
/// // With child and size
/// sized_box!(child: widget, width: 100.0, height: 50.0)
///
/// // Properties only (no child, for spacing)
/// sized_box!(width: 100.0, height: 50.0)
/// ```
#[macro_export]
macro_rules! sized_box {
    // Empty sized box
    () => {
        $crate::SizedBox::new()
    };

    // With child only (no constraints)
    (child: $child:expr) => {
        $crate::SizedBox::builder()
            .child($child)
            .build()
    };

    // With child and properties
    (child: $child:expr, $($field:ident : $value:expr),+ $(,)?) => {
        $crate::SizedBox::builder()
            .child($child)
            $(.$field($value))+
            .build()
    };

    // Without child, just properties (for spacing)
    ($($field:ident : $value:expr),+ $(,)?) => {
        $crate::SizedBox {
            $($field: Some($value.into()),)*
            ..Default::default()
        }
    };
}

#[cfg(test)]
mod tests {
    use flui_core::ComponentElement;

    use super::*;

    // Mock widget for testing
    #[derive(Debug, Clone)]
    struct MockView;

    impl View for MockView {
        fn build(self, _ctx: &BuildContext) -> impl IntoElement {
            (RenderPadding::new(EdgeInsets::ZERO), ())
        }
    }

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
        let sized_box = SizedBox::builder().width(100.0).build();
        assert_eq!(sized_box.width, Some(100.0));
        assert!(sized_box.height.is_none());
    }

    #[test]
    fn test_sized_box_builder_chaining() {
        let sized_box = SizedBox::builder().width(200.0).height(100.0).build();

        assert_eq!(sized_box.width, Some(200.0));
        assert_eq!(sized_box.height, Some(100.0));
    }

    #[test]
    fn test_sized_box_expand() {
        let sized_box = SizedBox::expand(MockView);
        assert_eq!(sized_box.width, Some(f32::INFINITY));
        assert_eq!(sized_box.height, Some(f32::INFINITY));
        assert!(sized_box.child.is_some());
    }

    #[test]
    fn test_sized_box_square() {
        let sized_box = SizedBox::square(100.0, MockView);
        assert_eq!(sized_box.width, Some(100.0));
        assert_eq!(sized_box.height, Some(100.0));
        assert!(sized_box.child.is_some());
    }

    #[test]
    fn test_sized_box_from_size() {
        let sized_box = SizedBox::from_size(200.0, 100.0, MockView);
        assert_eq!(sized_box.width, Some(200.0));
        assert_eq!(sized_box.height, Some(100.0));
        assert!(sized_box.child.is_some());
    }

    #[test]
    fn test_sized_box_width_only() {
        let sized_box = SizedBox::width_only(200.0, MockView);
        assert_eq!(sized_box.width, Some(200.0));
        assert!(sized_box.height.is_none());
        assert!(sized_box.child.is_some());
    }

    #[test]
    fn test_sized_box_height_only() {
        let sized_box = SizedBox::height_only(150.0, MockView);
        assert!(sized_box.width.is_none());
        assert_eq!(sized_box.height, Some(150.0));
        assert!(sized_box.child.is_some());
    }

    #[test]
    fn test_sized_box_shrink() {
        let sized_box = SizedBox::shrink();
        assert_eq!(sized_box.width, Some(0.0));
        assert_eq!(sized_box.height, Some(0.0));
        assert!(sized_box.child.is_none());
    }

    #[test]
    fn test_sized_box_h_space() {
        let sized_box = SizedBox::h_space(20.0);
        assert_eq!(sized_box.width, Some(20.0));
        assert!(sized_box.height.is_none());
        assert!(sized_box.child.is_none());
    }

    #[test]
    fn test_sized_box_v_space() {
        let sized_box = SizedBox::v_space(30.0);
        assert!(sized_box.width.is_none());
        assert_eq!(sized_box.height, Some(30.0));
        assert!(sized_box.child.is_none());
    }

    #[test]
    fn test_sized_box_builder_with_child() {
        let sized_box = SizedBox::builder().width(100.0).child(MockView).build();
        assert!(sized_box.child.is_some());
    }

    #[test]
    fn test_sized_box_macro_empty() {
        let sized_box = sized_box!();
        assert!(sized_box.width.is_none());
        assert!(sized_box.child.is_none());
    }

    #[test]
    fn test_sized_box_macro_with_child() {
        let sized_box = sized_box!(child: MockView);
        assert!(sized_box.child.is_some());
        assert!(sized_box.width.is_none());
        assert!(sized_box.height.is_none());
    }

    #[test]
    fn test_sized_box_macro_with_child_and_size() {
        let sized_box = sized_box!(child: MockView, width: 100.0, height: 50.0);
        assert!(sized_box.child.is_some());
        assert_eq!(sized_box.width, Some(100.0));
        assert_eq!(sized_box.height, Some(50.0));
    }

    #[test]
    fn test_sized_box_macro_with_fields() {
        let sized_box = sized_box! {
            width: 100.0,
            height: 50.0,
        };
        assert_eq!(sized_box.width, Some(100.0));
        assert_eq!(sized_box.height, Some(50.0));
        assert!(sized_box.child.is_none());
    }

    #[test]
    fn test_all_convenience_methods() {
        // Test that all convenience methods with child create widgets with children
        assert!(SizedBox::square(100.0, MockView).child.is_some());
        assert!(SizedBox::from_size(200.0, 100.0, MockView).child.is_some());
        assert!(SizedBox::width_only(150.0, MockView).child.is_some());
        assert!(SizedBox::height_only(75.0, MockView).child.is_some());
        assert!(SizedBox::expand(MockView).child.is_some());

        // Test that spacing helpers have no children
        assert!(SizedBox::shrink().child.is_none());
        assert!(SizedBox::h_space(20.0).child.is_none());
        assert!(SizedBox::v_space(30.0).child.is_none());

        // Verify dimensions
        assert_eq!(SizedBox::square(100.0, MockView).width, Some(100.0));
        assert_eq!(SizedBox::square(100.0, MockView).height, Some(100.0));
        assert_eq!(SizedBox::h_space(20.0).width, Some(20.0));
        assert!(SizedBox::h_space(20.0).height.is_none());
        assert!(SizedBox::v_space(30.0).width.is_none());
        assert_eq!(SizedBox::v_space(30.0).height, Some(30.0));
    }

    #[test]
    fn test_sized_box_validate_ok() {
        let sized_box = SizedBox::builder().width(100.0).height(50.0).build();
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
        let sized_box = SizedBox::builder().width(100.0).build();
        assert_eq!(sized_box.width, Some(100.0));
        assert!(sized_box.height.is_none());
    }

    #[test]
    fn test_sized_box_only_height() {
        let sized_box = SizedBox::builder().height(50.0).build();
        assert!(sized_box.width.is_none());
        assert_eq!(sized_box.height, Some(50.0));
    }

    #[test]
    fn test_view_trait() {
        let sized_box = SizedBox::builder().width(100.0).child(MockView).build();

        // Test child field
        assert!(sized_box.child.is_some());
    }

    #[test]
    fn test_single_child_view() {
        let sized_box = SizedBox::builder().width(100.0).child(MockView).build();

        // Test child field - returns Option
        assert!(sized_box.child.is_some());
    }
}

// Implement View for SizedBox - New architecture
impl View for SizedBox {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        let constraints = BoxConstraints::tight_for(self.width, self.height);
        (RenderConstrainedBox::new(constraints), self.child)
    }
}

// SizedBox now implements View trait directly
