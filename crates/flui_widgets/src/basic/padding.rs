//! Padding widget - adds empty space around a child
//!
//! A widget that insets its child by the given padding.
//! Similar to Flutter's Padding widget.
//!
//! # Usage Patterns
//!
//! ## 1. Convenience Methods (Recommended)
//! ```rust,ignore
//! // Uniform padding on all sides
//! Padding::all(16.0, child)
//!
//! // Symmetric padding (horizontal, vertical)
//! Padding::symmetric(20.0, 10.0, child)
//!
//! // Only specific sides
//! Padding::only(left: 10.0, child)
//! ```
//!
//! ## 2. Builder Pattern
//! ```rust,ignore
//! Padding::builder()
//!     .padding(EdgeInsets::all(16.0))
//!     .child(some_widget)
//!     .build()
//! ```
//!
//! ## 3. Macro
//! ```rust,ignore
//! padding!(child: widget, padding: EdgeInsets::all(16.0))
//! ```

use bon::Builder;
use flui_core::view::{AnyView, IntoElement, SingleRenderBuilder, View};
use flui_core::BuildContext;
use flui_rendering::RenderPadding;
use flui_types::EdgeInsets;

/// A widget that insets its child by the given padding.
///
/// ## Layout Behavior
///
/// - The padding is applied inside any decoration constraints
/// - Negative padding is not supported and will be clamped to zero
/// - The child size is reduced by the padding amount
///
/// ## Examples
///
/// ```rust,ignore
/// // Uniform padding
/// Padding::builder()
///     .padding(EdgeInsets::all(20.0))
///     .child(Text::new("Hello"))
///     .build()
///
/// // Asymmetric padding
/// Padding::builder()
///     .padding(EdgeInsets::only(left: 10.0, right: 10.0, top: 5.0, bottom: 5.0))
///     .child(some_widget)
///     .build()
/// ```
#[derive(Builder)]
#[builder(
    on(String, into),
    on(EdgeInsets, into),
    finish_fn(name = build_internal, vis = "")
)]
pub struct Padding {
    /// Optional key for widget identification
    pub key: Option<String>,

    /// The amount of space by which to inset the child.
    #[builder(default = EdgeInsets::ZERO)]
    pub padding: EdgeInsets,

    /// The child widget to pad.
    #[builder(setters(vis = "", name = child_internal))]
    pub child: Option<Box<dyn AnyView>>,
}

impl std::fmt::Debug for Padding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Padding")
            .field("key", &self.key)
            .field("padding", &self.padding)
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

impl Clone for Padding {
    fn clone(&self) -> Self {
        Self {
            key: self.key.clone(),
            padding: self.padding,
            child: self.child.clone(),
        }
    }
}

impl Padding {
    /// Creates a new empty Padding with zero padding.
    ///
    /// Note: Prefer using convenience methods like `Padding::all()` for most cases.
    pub const fn new() -> Self {
        Self {
            key: None,
            padding: EdgeInsets::ZERO,
            child: None,
        }
    }

    /// Creates a Padding with uniform padding on all sides.
    ///
    /// Most common use case - adds equal spacing on all four sides.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// Padding::all(16.0, Text::new("Hello"))
    /// ```
    pub fn all(value: f32, child: impl View + 'static) -> Self {
        Self::builder()
            .padding(EdgeInsets::all(value))
            .child(child)
            .build()
    }

    /// Creates a Padding with symmetric horizontal and vertical padding.
    ///
    /// Perfect for responsive layouts - different spacing on x and y axes.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // 20px left/right, 10px top/bottom
    /// Padding::symmetric(20.0, 10.0, content)
    /// ```
    pub fn symmetric(horizontal: f32, vertical: f32, child: impl View + 'static) -> Self {
        Self::builder()
            .padding(EdgeInsets::symmetric(horizontal, vertical))
            .child(child)
            .build()
    }

    /// Creates a Padding with custom padding on specific sides only.
    ///
    /// Flexible method for asymmetric padding. All parameters are optional.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Only left and top padding
    /// Padding::only(widget, left: 10.0, top: 5.0)
    ///
    /// // Only right padding
    /// Padding::only(widget, right: 20.0)
    /// ```
    pub fn only(
        child: impl View + 'static,
        left: Option<f32>,
        top: Option<f32>,
        right: Option<f32>,
        bottom: Option<f32>,
    ) -> Self {
        Self::builder()
            .padding(EdgeInsets::new(
                left.unwrap_or(0.0),
                top.unwrap_or(0.0),
                right.unwrap_or(0.0),
                bottom.unwrap_or(0.0),
            ))
            .child(child)
            .build()
    }

    /// Creates a Padding with only horizontal padding (left and right).
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// Padding::horizontal(20.0, widget)
    /// ```
    pub fn horizontal(value: f32, child: impl View + 'static) -> Self {
        Self::symmetric(value, 0.0, child)
    }

    /// Creates a Padding with only vertical padding (top and bottom).
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// Padding::vertical(10.0, widget)
    /// ```
    pub fn vertical(value: f32, child: impl View + 'static) -> Self {
        Self::symmetric(0.0, value, child)
    }

    /// Creates a Padding with the given EdgeInsets and child.
    ///
    /// Use this when you already have an EdgeInsets instance.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let insets = EdgeInsets::all(16.0);
    /// Padding::from_insets(insets, widget)
    /// ```
    pub fn from_insets(padding: EdgeInsets, child: impl View + 'static) -> Self {
        Self::builder().padding(padding).child(child).build()
    }

    /// Validates padding configuration.
    ///
    /// Returns an error if any padding value is negative.
    pub fn validate(&self) -> Result<(), String> {
        // Padding values should be non-negative
        if self.padding.left < 0.0
            || self.padding.right < 0.0
            || self.padding.top < 0.0
            || self.padding.bottom < 0.0
        {
            return Err("Padding values must be non-negative".to_string());
        }

        Ok(())
    }
}

impl Default for Padding {
    fn default() -> Self {
        Self::new()
    }
}

// Implement View for Padding - Simplified API
impl View for Padding {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        SingleRenderBuilder::new(RenderPadding::new(self.padding)).with_optional_child(self.child)
    }
}

// bon Builder Extensions
use padding_builder::{IsUnset, SetChild, State};

// Custom child setter
impl<S: State> PaddingBuilder<S>
where
    S::Child: IsUnset,
{
    /// Sets the child widget (works in builder chain).
    pub fn child(self, child: impl View + 'static) -> PaddingBuilder<SetChild<S>> {
        self.child_internal(Box::new(child))
    }
}

// Build wrapper with validation
impl<S: State> PaddingBuilder<S> {
    /// Builds the Padding widget with automatic validation in debug mode.
    pub fn build(self) -> Padding {
        let padding = self.build_internal();

        // In debug mode, validate configuration and warn on issues
        #[cfg(debug_assertions)]
        if let Err(e) = padding.validate() {
            tracing::warn!("Padding validation warning: {}", e);
        }

        padding
    }
}

/// Macro for creating Padding with declarative syntax.
///
/// # Examples
///
/// ```rust,ignore
/// // Empty padding
/// padding!()
///
/// // With child only (zero padding)
/// padding!(child: Text::new("Hello"))
///
/// // With child and padding
/// padding!(child: widget, padding: EdgeInsets::all(16.0))
///
/// // Properties only (no child)
/// padding!(padding: EdgeInsets::all(10.0))
/// ```
#[macro_export]
macro_rules! padding {
    // Empty padding
    () => {
        $crate::Padding::new()
    };

    // With child only (zero padding)
    (child: $child:expr) => {
        $crate::Padding::builder()
            .child($child)
            .build()
    };

    // With child and properties
    (child: $child:expr, $($field:ident : $value:expr),+ $(,)?) => {
        $crate::Padding::builder()
            .child($child)
            $(.$field($value))+
            .build()
    };

    // Without child, just properties
    ($($field:ident : $value:expr),+ $(,)?) => {
        $crate::Padding {
            $($field: $value.into(),)*
            ..Default::default()
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_core::view::LeafRenderBuilder;

    // Mock view for testing
    #[derive(Debug, Clone)]
    struct MockView;

    impl View for MockView {
        fn build(self, _ctx: &BuildContext) -> impl IntoElement {
            LeafRenderBuilder::new(RenderPadding::new(EdgeInsets::ZERO))
        }
    }

    #[test]
    fn test_padding_new() {
        let padding = Padding::new();
        assert!(padding.key.is_none());
        assert_eq!(padding.padding, EdgeInsets::ZERO);
        assert!(padding.child.is_none());
    }

    #[test]
    fn test_padding_default() {
        let padding = Padding::default();
        assert_eq!(padding.padding, EdgeInsets::ZERO);
    }

    #[test]
    fn test_padding_all() {
        let padding = Padding::all(16.0, MockView);
        assert_eq!(padding.padding, EdgeInsets::all(16.0));
        assert!(padding.child.is_some());
    }

    #[test]
    fn test_padding_symmetric() {
        let padding = Padding::symmetric(20.0, 10.0, MockView);
        assert_eq!(padding.padding.left, 20.0);
        assert_eq!(padding.padding.right, 20.0);
        assert_eq!(padding.padding.top, 10.0);
        assert_eq!(padding.padding.bottom, 10.0);
        assert!(padding.child.is_some());
    }

    #[test]
    fn test_padding_horizontal() {
        let padding = Padding::horizontal(15.0, MockView);
        assert_eq!(padding.padding.left, 15.0);
        assert_eq!(padding.padding.right, 15.0);
        assert_eq!(padding.padding.top, 0.0);
        assert_eq!(padding.padding.bottom, 0.0);
        assert!(padding.child.is_some());
    }

    #[test]
    fn test_padding_vertical() {
        let padding = Padding::vertical(10.0, MockView);
        assert_eq!(padding.padding.left, 0.0);
        assert_eq!(padding.padding.right, 0.0);
        assert_eq!(padding.padding.top, 10.0);
        assert_eq!(padding.padding.bottom, 10.0);
        assert!(padding.child.is_some());
    }

    #[test]
    fn test_padding_only() {
        let padding = Padding::only(MockView, Some(5.0), Some(10.0), None, None);
        assert_eq!(padding.padding.left, 5.0);
        assert_eq!(padding.padding.top, 10.0);
        assert_eq!(padding.padding.right, 0.0);
        assert_eq!(padding.padding.bottom, 0.0);
        assert!(padding.child.is_some());
    }

    #[test]
    fn test_padding_from_insets() {
        let insets = EdgeInsets::all(12.0);
        let padding = Padding::from_insets(insets, MockView);
        assert_eq!(padding.padding, EdgeInsets::all(12.0));
        assert!(padding.child.is_some());
    }

    #[test]
    fn test_padding_builder() {
        let padding = Padding::builder().padding(EdgeInsets::all(10.0)).build();
        assert_eq!(padding.padding, EdgeInsets::all(10.0));
    }

    #[test]
    fn test_padding_builder_with_child() {
        let padding = Padding::builder()
            .padding(EdgeInsets::all(10.0))
            .child(MockView)
            .build();
        assert!(padding.child.is_some());
    }

    #[test]
    fn test_padding_macro_empty() {
        let padding = padding!();
        assert_eq!(padding.padding, EdgeInsets::ZERO);
    }

    #[test]
    fn test_padding_macro_with_child() {
        let padding = padding!(child: MockView);
        assert!(padding.child.is_some());
        assert_eq!(padding.padding, EdgeInsets::ZERO);
    }

    #[test]
    fn test_padding_macro_with_child_and_padding() {
        let padding = padding!(child: MockView, padding: EdgeInsets::all(20.0));
        assert!(padding.child.is_some());
        assert_eq!(padding.padding, EdgeInsets::all(20.0));
    }

    #[test]
    fn test_padding_macro_with_padding() {
        let padding = padding! {
            padding: EdgeInsets::all(20.0),
        };
        assert_eq!(padding.padding, EdgeInsets::all(20.0));
    }

    #[test]
    fn test_padding_validate_ok() {
        let padding = Padding::all(10.0, MockView);
        assert!(padding.validate().is_ok());
    }

    #[test]
    fn test_padding_validate_negative() {
        let padding = Padding {
            padding: EdgeInsets::new(10.0, -5.0, 0.0, 0.0),
            ..Default::default()
        };
        assert!(padding.validate().is_err());
    }

    #[test]
    fn test_padding_view_trait() {
        let padding = Padding::builder()
            .padding(EdgeInsets::all(10.0))
            .child(MockView)
            .build();

        // Test child field
        assert!(padding.child.is_some());
    }
}

// Padding now implements View trait directly
