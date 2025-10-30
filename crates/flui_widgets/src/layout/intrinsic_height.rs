//! IntrinsicHeight widget - sizes child to intrinsic height
//!
//! A widget that sizes its child to the child's intrinsic height.
//! Similar to Flutter's IntrinsicHeight widget.

use bon::Builder;
use flui_core::widget::{RenderWidget, Widget};
use flui_core::{BuildContext, render::RenderNode};
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
#[derive(Debug, Clone, Builder)]
#[builder(on(String, into), finish_fn = build_intrinsic_height)]
pub struct IntrinsicHeight {
    /// Optional key for widget identification
    pub key: Option<String>,

    /// Step width - rounds intrinsic width to nearest multiple
    pub step_width: Option<f32>,

    /// Step height - rounds intrinsic height to nearest multiple
    pub step_height: Option<f32>,

    /// The child widget to size
    #[builder(setters(vis = "", name = child_internal))]
    pub child: Option<Widget>,
}

impl IntrinsicHeight {
    /// Creates a new IntrinsicHeight widget.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let widget = IntrinsicHeight::new(child);
    /// ```
    pub fn new(child: Widget) -> Self {
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
    /// let widget = IntrinsicHeight::with_step_height(50.0, child);
    /// ```
    pub fn with_step_height(step_height: f32, child: Widget) -> Self {
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
    /// let widget = IntrinsicHeight::with_steps(10.0, 10.0, child);
    /// ```
    pub fn with_steps(step_width: f32, step_height: f32, child: Widget) -> Self {
        Self {
            key: None,
            step_width: Some(step_width),
            step_height: Some(step_height),
            child: Some(child),
        }
    }
}

impl Default for IntrinsicHeight {
    fn default() -> Self {
        Self {
            key: None,
            step_width: None,
            step_height: None,
            child: None,
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
    pub fn child(self, child: Widget) -> IntrinsicHeightBuilder<SetChild<S>> {
        self.child_internal(child)
    }
}

impl<S: State> IntrinsicHeightBuilder<S> {
    /// Builds the IntrinsicHeight widget.
    pub fn build(self) -> IntrinsicHeight {
        self.build_intrinsic_height()
    }
}

// Implement RenderWidget
impl RenderWidget for IntrinsicHeight {
    fn create_render_object(&self, _context: &BuildContext) -> RenderNode {
        let render = match (self.step_width, self.step_height) {
            (Some(w), Some(h)) => RenderIntrinsicHeight::with_steps(w, h),
            (Some(w), None) => RenderIntrinsicHeight::with_step_width(w),
            (None, Some(h)) => RenderIntrinsicHeight::with_step_height(h),
            (None, None) => RenderIntrinsicHeight::new(),
        };
        RenderNode::single(Box::new(render))
    }

    fn update_render_object(&self, _context: &BuildContext, render_object: &mut RenderNode) {
        if let RenderNode::Single { render, .. } = render_object {
            if let Some(intrinsic) = render.downcast_mut::<RenderIntrinsicHeight>() {
                intrinsic.step_width = self.step_width;
                intrinsic.step_height = self.step_height;
            }
        }
    }

    fn child(&self) -> Option<&Widget> {
        self.child.as_ref()
    }
}

// Implement IntoWidget for ergonomic API
flui_core::impl_into_widget!(IntrinsicHeight, render);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_intrinsic_height_new() {
        let widget = IntrinsicHeight::new(Widget::from(()));
        assert!(widget.child.is_some());
        assert_eq!(widget.step_width, None);
        assert_eq!(widget.step_height, None);
    }

    #[test]
    fn test_intrinsic_height_with_step_height() {
        let widget = IntrinsicHeight::with_step_height(50.0, Widget::from(()));
        assert_eq!(widget.step_height, Some(50.0));
        assert_eq!(widget.step_width, None);
    }

    #[test]
    fn test_intrinsic_height_with_steps() {
        let widget = IntrinsicHeight::with_steps(10.0, 20.0, Widget::from(()));
        assert_eq!(widget.step_width, Some(10.0));
        assert_eq!(widget.step_height, Some(20.0));
    }

    #[test]
    fn test_intrinsic_height_builder() {
        let widget = IntrinsicHeight::builder()
            .step_height(25.0)
            .build();
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
