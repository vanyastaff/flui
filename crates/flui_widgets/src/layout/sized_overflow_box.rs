//! SizedOverflowBox widget - fixed size with child overflow
//!
//! A widget with a specific size that allows its child to have different constraints,
//! potentially causing the child to overflow the widget's bounds.

use bon::Builder;
use flui_core::widget::{RenderWidget, Widget};
use flui_core::{BuildContext, RenderNode};
use flui_rendering::RenderSizedOverflowBox;
use flui_types::Alignment;

/// A widget with a specific size that allows its child to overflow.
///
/// SizedOverflowBox is a combination of SizedBox and OverflowBox:
/// - The widget itself has a specific size (width/height)
/// - The child can have different constraints, allowing it to overflow
/// - The child is aligned within the widget
///
/// ## Key Properties
///
/// - **width**: Explicit width for this widget
/// - **height**: Explicit height for this widget
/// - **child_min_width**: Minimum width constraint for child
/// - **child_max_width**: Maximum width constraint for child
/// - **child_min_height**: Minimum height constraint for child
/// - **child_max_height**: Maximum height constraint for child
/// - **alignment**: How to align the child (default: CENTER)
/// - **child**: The child widget
///
/// ## Common Use Cases
///
/// ### Fixed size with larger child
/// ```rust,ignore
/// SizedOverflowBox::builder()
///     .width(100.0)
///     .height(100.0)
///     .child_max_width(200.0)
///     .child_max_height(200.0)
///     .child(large_image)
///     .build()
/// ```
///
/// ### Clipped preview
/// ```rust,ignore
/// SizedOverflowBox::new(
///     Some(50.0),
///     Some(50.0),
///     content
/// )
/// ```
///
/// ## Examples
///
/// ```rust,ignore
/// // Simple fixed size allowing overflow
/// SizedOverflowBox::new(Some(100.0), Some(100.0), child)
///
/// // With specific child constraints
/// SizedOverflowBox::builder()
///     .width(100.0)
///     .height(100.0)
///     .child_max_width(200.0)
///     .child_max_height(200.0)
///     .alignment(Alignment::TOP_LEFT)
///     .child(widget)
///     .build()
/// ```
#[derive(Debug, Clone, Builder)]
#[builder(on(String, into), finish_fn = build_sized_overflow_box)]
pub struct SizedOverflowBox {
    /// Optional key for widget identification
    pub key: Option<String>,

    /// Explicit width for this widget
    pub width: Option<f32>,

    /// Explicit height for this widget
    pub height: Option<f32>,

    /// Minimum width constraint for child
    pub child_min_width: Option<f32>,

    /// Maximum width constraint for child
    pub child_max_width: Option<f32>,

    /// Minimum height constraint for child
    pub child_min_height: Option<f32>,

    /// Maximum height constraint for child
    pub child_max_height: Option<f32>,

    /// How to align the child
    /// Default: Alignment::CENTER
    #[builder(default = Alignment::CENTER)]
    pub alignment: Alignment,

    /// The child widget
    #[builder(setters(vis = "", name = child_internal))]
    pub child: Option<Widget>,
}

impl SizedOverflowBox {
    /// Creates a new SizedOverflowBox with specific size.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let box = SizedOverflowBox::new(Some(100.0), Some(100.0), child);
    /// ```
    pub fn new(width: Option<f32>, height: Option<f32>, child: Widget) -> Self {
        Self {
            key: None,
            width,
            height,
            child_min_width: None,
            child_max_width: None,
            child_min_height: None,
            child_max_height: None,
            alignment: Alignment::CENTER,
            child: Some(child),
        }
    }

    /// Creates a SizedOverflowBox with child constraints.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let box = SizedOverflowBox::with_child_constraints(
    ///     Some(100.0), Some(100.0),
    ///     None, Some(200.0),
    ///     None, Some(200.0),
    ///     child
    /// );
    /// ```
    pub fn with_child_constraints(
        width: Option<f32>,
        height: Option<f32>,
        child_min_width: Option<f32>,
        child_max_width: Option<f32>,
        child_min_height: Option<f32>,
        child_max_height: Option<f32>,
        child: Widget,
    ) -> Self {
        Self {
            key: None,
            width,
            height,
            child_min_width,
            child_max_width,
            child_min_height,
            child_max_height,
            alignment: Alignment::CENTER,
            child: Some(child),
        }
    }
}

impl Default for SizedOverflowBox {
    fn default() -> Self {
        Self {
            key: None,
            width: None,
            height: None,
            child_min_width: None,
            child_max_width: None,
            child_min_height: None,
            child_max_height: None,
            alignment: Alignment::CENTER,
            child: None,
        }
    }
}

// bon Builder Extensions
use sized_overflow_box_builder::{IsUnset, SetChild, State};

impl<S: State> SizedOverflowBoxBuilder<S>
where
    S::Child: IsUnset,
{
    /// Sets the child widget (works in builder chain).
    pub fn child(self, child: impl flui_core::IntoWidget) -> SizedOverflowBoxBuilder<SetChild<S>> {
        self.child_internal(child.into_widget())
    }
}

impl<S: State> SizedOverflowBoxBuilder<S> {
    /// Builds the SizedOverflowBox widget.
    pub fn build(self) -> Widget {
        Widget::render_object(self.build_sized_overflow_box())
    }
}

// Implement RenderWidget
impl RenderWidget for SizedOverflowBox {
    fn create_render_object(&self, _context: &BuildContext) -> RenderNode {
        let mut render = if self.child_min_width.is_some()
            || self.child_max_width.is_some()
            || self.child_min_height.is_some()
            || self.child_max_height.is_some() {
            RenderSizedOverflowBox::with_child_constraints(
                self.width,
                self.height,
                self.child_min_width,
                self.child_max_width,
                self.child_min_height,
                self.child_max_height,
            )
        } else {
            RenderSizedOverflowBox::new(self.width, self.height)
        };
        render.alignment = self.alignment;
        RenderNode::single(Box::new(render))
    }

    fn update_render_object(&self, _context: &BuildContext, render_object: &mut RenderNode) {
        if let RenderNode::Single { render, .. } = render_object {
            if let Some(sized_overflow_box) = render.downcast_mut::<RenderSizedOverflowBox>() {
                sized_overflow_box.width = self.width;
                sized_overflow_box.height = self.height;
                sized_overflow_box.child_min_width = self.child_min_width;
                sized_overflow_box.child_max_width = self.child_max_width;
                sized_overflow_box.child_min_height = self.child_min_height;
                sized_overflow_box.child_max_height = self.child_max_height;
                sized_overflow_box.alignment = self.alignment;
            }
        }
    }

    fn child(&self) -> Option<&Widget> {
        self.child.as_ref()
    }
}

// Implement IntoWidget for ergonomic API
flui_core::impl_into_widget!(SizedOverflowBox, render);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sized_overflow_box_new() {
        let child = Widget::from(());
        let box_widget = SizedOverflowBox::new(Some(100.0), Some(100.0), child);
        assert_eq!(box_widget.width, Some(100.0));
        assert_eq!(box_widget.height, Some(100.0));
        assert!(box_widget.child.is_some());
    }

    #[test]
    fn test_sized_overflow_box_with_child_constraints() {
        let child = Widget::from(());
        let box_widget = SizedOverflowBox::with_child_constraints(
            Some(100.0),
            Some(100.0),
            None,
            Some(200.0),
            None,
            Some(200.0),
            child,
        );
        assert_eq!(box_widget.child_max_width, Some(200.0));
        assert_eq!(box_widget.child_max_height, Some(200.0));
    }

    #[test]
    fn test_sized_overflow_box_builder() {
        let box_widget = SizedOverflowBox::builder()
            .width(100.0)
            .height(100.0)
            .child_max_width(200.0)
            .alignment(Alignment::TOP_LEFT)
            .build();
    }

    #[test]
    fn test_sized_overflow_box_default() {
        let box_widget = SizedOverflowBox::default();
        assert_eq!(box_widget.width, None);
        assert_eq!(box_widget.height, None);
        assert!(box_widget.child.is_none());
    }
}
