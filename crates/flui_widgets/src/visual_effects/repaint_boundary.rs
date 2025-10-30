//! RepaintBoundary widget - optimization boundary for repainting
//!
//! A widget that creates a separate paint layer, isolating the child's
//! repainting from its ancestors for performance optimization.

use bon::Builder;
use flui_core::widget::{RenderWidget, Widget};
use flui_core::{BuildContext, RenderNode};
use flui_rendering::RenderRepaintBoundary;

/// A widget that creates a repaint boundary.
///
/// RepaintBoundary creates a separate paint layer, isolating the child's
/// repainting from its ancestors. When the child repaints, only this
/// subtree needs to be repainted, not the entire widget tree.
///
/// ## Key Properties
///
/// - **child**: The child widget to isolate
///
/// ## Common Use Cases
///
/// ### Wrap animated widget
/// ```rust,ignore
/// RepaintBoundary::new(animated_widget)
/// ```
///
/// ### Isolate frequently changing content
/// ```rust,ignore
/// RepaintBoundary::new(video_player)
/// ```
///
/// ## Performance Benefits
///
/// Use RepaintBoundary to optimize performance when:
/// - A widget repaints frequently (animations, videos, timers)
/// - A complex subtree is expensive to paint
/// - You want to prevent repainting of parent widgets
///
/// ## Examples
///
/// ```rust,ignore
/// // Wrap animated widget
/// RepaintBoundary::new(AnimatedWidget::new(...))
///
/// // Using builder
/// RepaintBoundary::builder()
///     .child(expensive_widget)
///     .build()
/// ```
#[derive(Debug, Clone, Builder)]
#[builder(on(String, into), finish_fn = build_repaint_boundary)]
pub struct RepaintBoundary {
    /// Optional key for widget identification
    pub key: Option<String>,

    /// The child widget to isolate
    #[builder(setters(vis = "", name = child_internal))]
    pub child: Option<Widget>,
}

impl RepaintBoundary {
    /// Creates a new RepaintBoundary.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let boundary = RepaintBoundary::new(child);
    /// ```
    pub fn new(child: Widget) -> Self {
        Self {
            key: None,
            child: Some(child),
        }
    }

    /// Wraps a widget in a RepaintBoundary.
    ///
    /// This is an alias for `new()` for better readability.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let boundary = RepaintBoundary::wrap(animated_widget);
    /// ```
    pub fn wrap(child: Widget) -> Self {
        Self::new(child)
    }
}

impl Default for RepaintBoundary {
    fn default() -> Self {
        Self {
            key: None,
            child: None,
        }
    }
}

// bon Builder Extensions
use repaint_boundary_builder::{IsUnset, SetChild, State};

impl<S: State> RepaintBoundaryBuilder<S>
where
    S::Child: IsUnset,
{
    /// Sets the child widget (works in builder chain).
    pub fn child(self, child: impl flui_core::IntoWidget) -> RepaintBoundaryBuilder<SetChild<S>> {
        self.child_internal(child.into_widget())
    }
}

impl<S: State> RepaintBoundaryBuilder<S> {
    /// Builds the RepaintBoundary widget.
    pub fn build(self) -> Widget {
        Widget::render_object(self.build_repaint_boundary())
    }
}

// Implement RenderWidget
impl RenderWidget for RepaintBoundary {
    fn create_render_object(&self, _context: &BuildContext) -> RenderNode {
        let render = RenderRepaintBoundary::new();
        RenderNode::single(Box::new(render))
    }

    fn update_render_object(&self, _context: &BuildContext, render_object: &mut RenderNode) {
        // RepaintBoundary has no mutable properties to update
        // The is_repaint_boundary flag is always true for this widget
        if let RenderNode::Single { render, .. } = render_object {
            if let Some(boundary) = render.downcast_mut::<RenderRepaintBoundary>() {
                boundary.set_is_repaint_boundary(true);
            }
        }
    }

    fn child(&self) -> Option<&Widget> {
        self.child.as_ref()
    }
}

// Implement IntoWidget for ergonomic API
flui_core::impl_into_widget!(RepaintBoundary, render);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_repaint_boundary_new() {
        let child = Widget::from(());
        let boundary = RepaintBoundary::new(child);
        assert!(boundary.child.is_some());
    }

    #[test]
    fn test_repaint_boundary_wrap() {
        let child = Widget::from(());
        let boundary = RepaintBoundary::wrap(child);
        assert!(boundary.child.is_some());
    }

    #[test]
    fn test_repaint_boundary_builder() {
        let boundary = RepaintBoundary::builder()
            .build();
        assert!(boundary.child.is_none());
    }

    #[test]
    fn test_repaint_boundary_default() {
        let boundary = RepaintBoundary::default();
        assert!(boundary.child.is_none());
    }
}
