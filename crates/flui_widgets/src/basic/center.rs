//! Center widget - centers its child
//!
//! A widget that centers its child within itself.
//! Similar to Flutter's Center widget.
//!
//! # Usage Patterns
//!
//! ## 1. Builder Pattern (Recommended)
//! ```rust,ignore
//! Center::builder()
//!     .child(some_widget)
//!     .build()
//! ```
//!
//! ## 2. Convenience Methods
//! ```rust,ignore
//! // Most common - center with expand
//! Center::with_child(some_widget)
//!
//! // Tight sizing (wraps child exactly)
//! Center::tight(some_widget)
//!
//! // Custom factors
//! Center::with_factors(some_widget, 2.0, 1.5)
//! ```
//!
//! ## 3. Macro
//! ```rust,ignore
//! center!(child: some_widget)
//! center!(child: some_widget, width_factor: 2.0)
//! ```

use bon::Builder;
use flui_core::render::RenderBoxExt;
use flui_core::view::children::Child;
use flui_core::view::{IntoElement, StatelessView};
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
/// - **Without factors**: Expands to fill all available space
/// - **With factors**: Sizes itself as `child_size * factor` (clamped to constraints)
///
/// ## Performance Notes
///
/// - Center with factors (tight sizing) is more efficient as it doesn't expand
/// - Without factors, Center takes all available space which may affect parent layouts
/// - Consider using `Align` widget directly if you need different alignments
///
/// ## Examples
///
/// ```rust,ignore
/// // Simple centering (expands to fill space)
/// Center::with_child(Text::new("Hello"))
///
/// // Tight wrapping (1:1 with child size)
/// Center::tight(some_widget)
///
/// // With size factors
/// Center::with_factors(some_widget, 2.0, 1.5)
///
/// // Using builder for more control
/// Center::builder()
///     .child(Text::new("Hello"))
///     .width_factor(2.0)
///     .key("my-center".to_string())
///     .build()
/// ```
#[derive(Builder)]
#[builder(
    on(String, into),
    finish_fn(name = build_internal, vis = "")
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
    #[builder(default, setters(vis = "", name = child_internal))]
    pub child: Child,
}

// Manual Debug implementation
impl std::fmt::Debug for Center {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Center")
            .field("key", &self.key)
            .field("width_factor", &self.width_factor)
            .field("height_factor", &self.height_factor)
            .field(
                "child",
                &if self.child.is_some() {
                    "<child>"
                } else {
                    "None"
                },
            )
            .finish()
    }
}

impl Center {
    /// Creates a new empty Center widget.
    ///
    /// Note: Prefer using `Center::with_child()` or `Center::builder()` for most cases.
    pub fn new() -> Self {
        Self {
            key: None,
            width_factor: None,
            height_factor: None,
            child: Child::none(),
        }
    }

    /// Creates a Center with a child (most common use case).
    ///
    /// The Center will expand to fill available space and center the child.
    ///
    /// # Example
    /// ```rust,ignore
    /// let centered = Center::with_child(Text::new("Hello"));
    /// ```
    pub fn with_child(child: impl IntoElement) -> Self {
        Self::builder().child(child).build()
    }

    /// Creates a Center that wraps the child tightly (factors = 1.0).
    ///
    /// This makes the Center exactly the same size as its child,
    /// which is more efficient than expanding to fill space.
    ///
    /// # Example
    /// ```rust,ignore
    /// let tight_center = Center::tight(Text::new("Wrapped"));
    /// ```
    pub fn tight(child: impl IntoElement) -> Self {
        Self::builder()
            .child(child)
            .width_factor(1.0)
            .height_factor(1.0)
            .build()
    }

    /// Creates a Center with custom size factors.
    ///
    /// The Center's size will be `child_size * factor` in each dimension.
    ///
    /// # Example
    /// ```rust,ignore
    /// // Center is 2x child width and 1.5x child height
    /// let scaled = Center::with_factors(some_widget, 2.0, 1.5);
    /// ```
    pub fn with_factors(child: impl IntoElement, width_factor: f32, height_factor: f32) -> Self {
        Self::builder()
            .child(child)
            .width_factor(width_factor)
            .height_factor(height_factor)
            .build()
    }

    /// Validates Center configuration.
    ///
    /// Returns an error if factors are zero, negative, NaN, or infinite.
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
    pub fn child(self, child: impl IntoElement) -> CenterBuilder<SetChild<S>> {
        self.child_internal(Child::new(child))
    }
}

// Build wrapper with validation
impl<S: State> CenterBuilder<S> {
    /// Builds the Center widget with automatic validation in debug mode.
    pub fn build(self) -> Center {
        let center = self.build_internal();

        #[cfg(debug_assertions)]
        if let Err(e) = center.validate() {
            tracing::warn!("Center validation warning: {}", e);
        }

        center
    }
}

/// Macro for creating Center with declarative syntax.
#[macro_export]
macro_rules! center {
    // Empty center
    () => {
        $crate::Center::new()
    };

    // With child only
    (child: $child:expr) => {
        $crate::Center::builder()
            .child($child)
            .build()
    };

    // With child and properties
    (child: $child:expr, $($field:ident : $value:expr),+ $(,)?) => {
        $crate::Center::builder()
            .child($child)
            $(.$field($value))+
            .build()
    };

    // Without child, just properties
    ($($field:ident : $value:expr),+ $(,)?) => {
        $crate::Center {
            $($field: Some($value.into()),)*
            ..Default::default()
        }
    };
}

// Implement View for Center
impl StatelessView for Center {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        RenderAlign::with_factors(Alignment::CENTER, self.width_factor, self.height_factor)
            .maybe_child(self.child)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_rendering::RenderEmpty;

    // Mock view for testing
    #[derive(Debug, Clone)]
    struct MockView;

    impl StatelessView for MockView {
        fn build(self, _ctx: &BuildContext) -> impl IntoElement {
            RenderEmpty.leaf()
        }
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
    fn test_center_builder_with_factors() {
        let center = Center::builder()
            .width_factor(2.0)
            .height_factor(1.5)
            .build();
        assert_eq!(center.width_factor, Some(2.0));
        assert_eq!(center.height_factor, Some(1.5));
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
    fn test_center_macro_empty() {
        let center = center!();
        assert!(center.child.is_none());
    }
}
