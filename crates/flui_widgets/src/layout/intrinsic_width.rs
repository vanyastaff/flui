//! IntrinsicWidth widget - sizes child to intrinsic width
//!
//! A widget that sizes its child to the child's intrinsic width.
//! Similar to Flutter's IntrinsicWidth widget.

use bon::Builder;
use flui_core::widget::{RenderWidget, Widget};
use flui_core::{BuildContext, render::RenderNode};
use flui_rendering::RenderIntrinsicWidth;

/// A widget that sizes its child to the child's intrinsic width.
///
/// IntrinsicWidth forces the child to be as wide as it "naturally" wants to be,
/// ignoring the parent's width constraints (within reason). This is useful for
/// making text and other widgets take up only as much horizontal space as they need.
///
/// ## Key Properties
///
/// - **step_width**: Rounds intrinsic width to nearest multiple (optional)
/// - **step_height**: Rounds intrinsic height to nearest multiple (optional)
///
/// ## Layout Behavior
///
/// 1. Queries child for its intrinsic width
/// 2. Constrains child to that width
/// 3. Optionally rounds dimensions to step multiples
///
/// ## Common Use Cases
///
/// ### Equal width buttons in Column
/// ```rust,ignore
/// Column::new()
///     .children(vec![
///         IntrinsicWidth::new(Button::new("OK")),
///         IntrinsicWidth::new(Button::new("Cancel")),
///         IntrinsicWidth::new(Button::new("Apply")),
///     ])
/// // All buttons will have the same width (widest one)
/// ```
///
/// ### Text field that matches content
/// ```rust,ignore
/// IntrinsicWidth::new(
///     TextField::new()
///         .hint("Enter name")
/// )
/// // Field will be as wide as its content
/// ```
///
/// ### Stepped sizing
/// ```rust,ignore
/// IntrinsicWidth::builder()
///     .step_width(50.0)  // Rounds to 50, 100, 150, etc.
///     .child(widget)
///     .build()
/// ```
///
/// ## Performance Note
///
/// IntrinsicWidth can be expensive because it forces a second layout pass.
/// Use sparingly and avoid nesting multiple IntrinsicWidth widgets.
///
/// ## Examples
///
/// ```rust,ignore
/// // Basic usage
/// IntrinsicWidth::new(child_widget)
///
/// // With step width
/// IntrinsicWidth::builder()
///     .step_width(25.0)
///     .child(widget)
///     .build()
///
/// // With both steps
/// IntrinsicWidth::builder()
///     .step_width(10.0)
///     .step_height(10.0)
///     .child(widget)
///     .build()
/// ```
#[derive(Debug, Clone, Builder)]
#[builder(on(String, into), finish_fn = build_intrinsic_width)]
pub struct IntrinsicWidth {
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

impl IntrinsicWidth {
    /// Creates a new IntrinsicWidth widget.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let widget = IntrinsicWidth::new(child);
    /// ```
    pub fn new(child: Widget) -> Self {
        Self {
            key: None,
            step_width: None,
            step_height: None,
            child: Some(child),
        }
    }

    /// Creates IntrinsicWidth with step width.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let widget = IntrinsicWidth::with_step_width(50.0, child);
    /// ```
    pub fn with_step_width(step_width: f32, child: Widget) -> Self {
        Self {
            key: None,
            step_width: Some(step_width),
            step_height: None,
            child: Some(child),
        }
    }

    /// Creates IntrinsicWidth with both step dimensions.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let widget = IntrinsicWidth::with_steps(10.0, 10.0, child);
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

impl Default for IntrinsicWidth {
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
use intrinsic_width_builder::{IsUnset, SetChild, State};

impl<S: State> IntrinsicWidthBuilder<S>
where
    S::Child: IsUnset,
{
    /// Sets the child widget (works in builder chain).
    pub fn child(self, child: Widget) -> IntrinsicWidthBuilder<SetChild<S>> {
        self.child_internal(child)
    }
}

impl<S: State> IntrinsicWidthBuilder<S> {
    /// Builds the IntrinsicWidth widget.
    pub fn build(self) -> IntrinsicWidth {
        self.build_intrinsic_width()
    }
}

// Implement RenderWidget
impl RenderWidget for IntrinsicWidth {
    fn create_render_object(&self, _context: &BuildContext) -> RenderNode {
        let render = match (self.step_width, self.step_height) {
            (Some(w), Some(h)) => RenderIntrinsicWidth::with_steps(w, h),
            (Some(w), None) => RenderIntrinsicWidth::with_step_width(w),
            (None, Some(h)) => RenderIntrinsicWidth::with_step_height(h),
            (None, None) => RenderIntrinsicWidth::new(),
        };
        RenderNode::single(Box::new(render))
    }

    fn update_render_object(&self, _context: &BuildContext, render_object: &mut RenderNode) {
        if let RenderNode::Single { render, .. } = render_object {
            if let Some(intrinsic) = render.downcast_mut::<RenderIntrinsicWidth>() {
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
flui_core::impl_into_widget!(IntrinsicWidth, render);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_intrinsic_width_new() {
        let widget = IntrinsicWidth::new(Widget::from(()));
        assert!(widget.child.is_some());
        assert_eq!(widget.step_width, None);
        assert_eq!(widget.step_height, None);
    }

    #[test]
    fn test_intrinsic_width_with_step_width() {
        let widget = IntrinsicWidth::with_step_width(50.0, Widget::from(()));
        assert_eq!(widget.step_width, Some(50.0));
        assert_eq!(widget.step_height, None);
    }

    #[test]
    fn test_intrinsic_width_with_steps() {
        let widget = IntrinsicWidth::with_steps(10.0, 20.0, Widget::from(()));
        assert_eq!(widget.step_width, Some(10.0));
        assert_eq!(widget.step_height, Some(20.0));
    }

    #[test]
    fn test_intrinsic_width_builder() {
        let widget = IntrinsicWidth::builder()
            .step_width(25.0)
            .build();
        assert_eq!(widget.step_width, Some(25.0));
    }

    #[test]
    fn test_intrinsic_width_default() {
        let widget = IntrinsicWidth::default();
        assert!(widget.child.is_none());
        assert_eq!(widget.step_width, None);
        assert_eq!(widget.step_height, None);
    }
}
