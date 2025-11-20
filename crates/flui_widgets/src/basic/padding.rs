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
use flui_core::element::Element;
use flui_core::render::RenderBoxExt;
use flui_core::view::{IntoElement, View};
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
    pub child: Option<Element>,
}

impl std::fmt::Debug for Padding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Padding")
            .field("key", &self.key)
            .field("padding", &self.padding)
            .field(
                "child",
                &if self.child.is_some() {
                    "<Element>"
                } else {
                    "None"
                },
            )
            .finish()
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
    pub fn all(value: f32, child: impl IntoElement) -> Self {
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
    pub fn symmetric(horizontal: f32, vertical: f32, child: impl IntoElement) -> Self {
        Self::builder()
            .padding(EdgeInsets::symmetric(horizontal, vertical))
            .child(child)
            .build()
    }

    /// Creates a Padding with custom padding on specific sides only.
    ///
    /// Flexible method for asymmetric padding.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// Padding::only(10.0, 5.0, 10.0, 5.0, content)  // left, top, right, bottom
    /// ```
    pub fn only(left: f32, top: f32, right: f32, bottom: f32, child: impl IntoElement) -> Self {
        Self::builder()
            .padding(EdgeInsets::only(left, top, right, bottom))
            .child(child)
            .build()
    }

    /// Creates a Padding with only left padding.
    pub fn left(value: f32, child: impl IntoElement) -> Self {
        Self::builder()
            .padding(EdgeInsets::only(value, 0.0, 0.0, 0.0))
            .child(child)
            .build()
    }

    /// Creates a Padding with only top padding.
    pub fn top(value: f32, child: impl IntoElement) -> Self {
        Self::builder()
            .padding(EdgeInsets::only(0.0, value, 0.0, 0.0))
            .child(child)
            .build()
    }

    /// Creates a Padding with only right padding.
    pub fn right(value: f32, child: impl IntoElement) -> Self {
        Self::builder()
            .padding(EdgeInsets::only(0.0, 0.0, value, 0.0))
            .child(child)
            .build()
    }

    /// Creates a Padding with only bottom padding.
    pub fn bottom(value: f32, child: impl IntoElement) -> Self {
        Self::builder()
            .padding(EdgeInsets::only(0.0, 0.0, 0.0, value))
            .child(child)
            .build()
    }

    /// Validates Padding configuration.
    pub fn validate(&self) -> Result<(), String> {
        if self.padding.left < 0.0
            || self.padding.top < 0.0
            || self.padding.right < 0.0
            || self.padding.bottom < 0.0
        {
            return Err("Negative padding values are not supported".to_string());
        }
        Ok(())
    }
}

impl Default for Padding {
    fn default() -> Self {
        Self::new()
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
    pub fn child(self, child: impl IntoElement) -> PaddingBuilder<SetChild<S>> {
        self.child_internal(Some(child.into_element()))
    }
}

// Build wrapper with validation
impl<S: State> PaddingBuilder<S> {
    /// Builds the Padding widget with automatic validation in debug mode.
    pub fn build(self) -> Padding {
        let padding = self.build_internal();

        #[cfg(debug_assertions)]
        if let Err(e) = padding.validate() {
            tracing::warn!("Padding validation warning: {}", e);
        }

        padding
    }
}

/// Macro for creating Padding with declarative syntax.
#[macro_export]
macro_rules! padding {
    // Empty padding
    () => {
        $crate::Padding::new()
    };

    // With child only
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
            $($field: Some($value.into()),)*
            ..Default::default()
        }
    };
}

// Implement View for Padding
impl View for Padding {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        RenderPadding::new(self.padding).child_opt(self.child)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_rendering::RenderEmpty;

    // Mock view for testing
    #[derive(Debug, Clone)]
    struct MockView;

    impl View for MockView {
        fn build(self, _ctx: &BuildContext) -> impl IntoElement {
            RenderEmpty.leaf()
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
    fn test_padding_builder() {
        let padding = Padding::builder().padding(EdgeInsets::all(16.0)).build();
        assert_eq!(padding.padding, EdgeInsets::all(16.0));
    }

    #[test]
    fn test_padding_validate_ok() {
        let padding = Padding::builder().padding(EdgeInsets::all(16.0)).build();
        assert!(padding.validate().is_ok());
    }

    #[test]
    fn test_padding_macro_empty() {
        let padding = padding!();
        assert_eq!(padding.padding, EdgeInsets::ZERO);
    }
}
