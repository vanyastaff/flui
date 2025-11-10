//! IntrinsicHeight widget - sizes child to intrinsic height
//!
//! A widget that sizes its child to the child's intrinsic height.
//! Similar to Flutter's IntrinsicHeight widget.

use bon::Builder;
use flui_core::view::{AnyView, IntoElement, RenderBuilder, View};
use flui_core::BuildContext;
use flui_rendering::RenderIntrinsicHeight;

/// A widget that sizes its child to the child's intrinsic height.
///
/// IntrinsicHeight forces the child to be as tall as it "naturally" wants to be,
/// ignoring the parent's height constraints (within reason). This is useful for
/// making widgets take up only as much vertical space as they need.
///
/// ## Key Properties
///
/// - **step_width**: Rounds intrinsic width to nearest multiple (optional)
/// - **step_height**: Rounds intrinsic height to nearest multiple (optional)
///
/// ## Layout Behavior
///
/// 1. Queries child for its intrinsic height
/// 2. Constrains child to that height
/// 3. Optionally rounds dimensions to step multiples
///
/// ## Common Use Cases
///
/// ### Equal height columns in Row
/// ```rust,ignore
/// Row::new()
///     .children(vec![
///         IntrinsicHeight::new(
///             Column::new().children(vec![
///                 Text::new("Short"),
///                 Text::new("Text"),
///             ])
///         ),
///         IntrinsicHeight::new(
///             Column::new().children(vec![
///                 Text::new("Much longer text"),
///                 Text::new("That takes more"),
///                 Text::new("Vertical space"),
///             ])
///         ),
///     ])
/// // Both columns will have the same height (tallest one)
/// ```
///
/// ### Stepped sizing
/// ```rust,ignore
/// IntrinsicHeight::builder()
///     .step_height(50.0)  // Rounds to 50, 100, 150, etc.
///     .child(widget)
///     .build()
/// ```
///
/// ## Performance Note
///
/// IntrinsicHeight can be expensive because it forces a second layout pass.
/// Use sparingly and avoid nesting multiple IntrinsicHeight widgets.
///
/// ## Examples
///
/// ```rust,ignore
/// // Basic usage
/// IntrinsicHeight::new(child_widget)
///
/// // With step height
/// IntrinsicHeight::builder()
///     .step_height(25.0)
///     .child(widget)
///     .build()
///
/// // With both steps
/// IntrinsicHeight::builder()
///     .step_width(10.0)
///     .step_height(10.0)
///     .child(widget)
///     .build()
/// ```
#[derive(Builder, Default)]
#[builder(on(String, into), finish_fn(name = build_internal, vis = ""))]
pub struct IntrinsicHeight {
    /// Optional key for widget identification
    pub key: Option<String>,

    /// Step width - rounds intrinsic width to nearest multiple
    pub step_width: Option<f32>,

    /// Step height - rounds intrinsic height to nearest multiple
    pub step_height: Option<f32>,

    /// The child widget to size
    #[builder(setters(vis = "", name = child_internal))]
    pub child: Option<Box<dyn AnyView>>,
}

impl std::fmt::Debug for IntrinsicHeight {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IntrinsicHeight")
            .field("key", &self.key)
            .field("step_width", &self.step_width)
            .field("step_height", &self.step_height)
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

impl Clone for IntrinsicHeight {
    fn clone(&self) -> Self {
        Self {
            key: self.key.clone(),
            step_width: self.step_width,
            step_height: self.step_height,
            child: self.child.clone(),
        }
    }
}

impl IntrinsicHeight {
    /// Creates a new IntrinsicHeight widget.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let widget = IntrinsicHeight::new(Box::new(child));
    /// ```
    pub fn new(child: Box<dyn AnyView>) -> Self {
        Self {
            key: None,
            step_width: None,
            step_height: None,
            child: Some(child),
        }
    }

    /// Creates IntrinsicHeight with step height.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let widget = IntrinsicHeight::with_step_height(50.0, Box::new(child));
    /// ```
    pub fn with_step_height(step_height: f32, child: Box<dyn AnyView>) -> Self {
        Self {
            key: None,
            step_width: None,
            step_height: Some(step_height),
            child: Some(child),
        }
    }

    /// Creates IntrinsicHeight with both step dimensions.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let widget = IntrinsicHeight::with_steps(10.0, 10.0, Box::new(child));
    /// ```
    pub fn with_steps(step_width: f32, step_height: f32, child: Box<dyn AnyView>) -> Self {
        Self {
            key: None,
            step_width: Some(step_width),
            step_height: Some(step_height),
            child: Some(child),
        }
    }
}

// bon Builder Extensions
use intrinsic_height_builder::{IsUnset, SetChild, State};

impl<S: State> IntrinsicHeightBuilder<S>
where
    S::Child: IsUnset,
{
    /// Sets the child widget (works in builder chain).
    pub fn child(self, child: impl View + 'static) -> IntrinsicHeightBuilder<SetChild<S>> {
        self.child_internal(Box::new(child))
    }
}

// Public build() wrapper
impl<S: State> IntrinsicHeightBuilder<S> {
    /// Builds the IntrinsicHeight widget.
    pub fn build(self) -> IntrinsicHeight {
        self.build_internal()
    }
}

// Implement View trait - Simplified API
impl View for IntrinsicHeight {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        let render = match (self.step_width, self.step_height) {
            (Some(w), Some(h)) => RenderIntrinsicHeight::with_steps(w, h),
            (Some(w), None) => RenderIntrinsicHeight::with_step_width(w),
            (None, Some(h)) => RenderIntrinsicHeight::with_step_height(h),
            (None, None) => RenderIntrinsicHeight::new(),
        };

        RenderBuilder::new(render).maybe_child(self.child)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_intrinsic_height_new() {
        let widget = IntrinsicHeight::new(Box::new(crate::SizedBox::new()));
        assert!(widget.child.is_some());
        assert_eq!(widget.step_width, None);
        assert_eq!(widget.step_height, None);
    }

    #[test]
    fn test_intrinsic_height_with_step_height() {
        let widget = IntrinsicHeight::with_step_height(50.0, Box::new(crate::SizedBox::new()));
        assert_eq!(widget.step_height, Some(50.0));
        assert_eq!(widget.step_width, None);
    }

    #[test]
    fn test_intrinsic_height_with_steps() {
        let widget = IntrinsicHeight::with_steps(10.0, 20.0, Box::new(crate::SizedBox::new()));
        assert_eq!(widget.step_width, Some(10.0));
        assert_eq!(widget.step_height, Some(20.0));
    }

    #[test]
    fn test_intrinsic_height_builder() {
        let widget = IntrinsicHeight::builder()
            .step_height(25.0)
            .build_intrinsic_height();
        assert_eq!(widget.step_height, Some(25.0));
    }

    #[test]
    fn test_intrinsic_height_default() {
        let widget = IntrinsicHeight::default();
        assert!(widget.child.is_none());
        assert_eq!(widget.step_width, None);
        assert_eq!(widget.step_height, None);
    }
}
