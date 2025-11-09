//! RepaintBoundary widget - optimization boundary for repainting
//!
//! A widget that creates a separate paint layer, isolating the child's
//! repainting from its ancestors for performance optimization.

use bon::Builder;
use flui_core::view::{AnyView, IntoElement, SingleRenderBuilder, View};
use flui_core::BuildContext;
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
#[derive(Builder)]
#[builder(on(String, into), finish_fn(name = build_internal, vis = ""))]
#[derive(Default)]
pub struct RepaintBoundary {
    /// Optional key for widget identification
    pub key: Option<String>,

    /// The child widget to isolate
    #[builder(setters(vis = "", name = child_internal))]
    pub child: Option<Box<dyn AnyView>>,
}

impl std::fmt::Debug for RepaintBoundary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RepaintBoundary")
            .field("key", &self.key)
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

impl Clone for RepaintBoundary {
    fn clone(&self) -> Self {
        Self {
            key: self.key.clone(),
            child: self.child.clone(),
        }
    }
}

impl RepaintBoundary {
    /// Creates a new RepaintBoundary wrapping a child widget.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let boundary = RepaintBoundary::new(AnimatedWidget::new());
    /// ```
    pub fn new(child: impl View + 'static) -> Self {
        Self {
            key: None,
            child: Some(Box::new(child)),
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
    pub fn wrap(child: impl View + 'static) -> Self {
        Self::new(child)
    }
}

// bon Builder Extensions
use repaint_boundary_builder::{IsUnset, SetChild, State};

impl<S: State> RepaintBoundaryBuilder<S>
where
    S::Child: IsUnset,
{
    /// Sets the child widget (works in builder chain).
    pub fn child(self, child: impl View + 'static) -> RepaintBoundaryBuilder<SetChild<S>> {
        self.child_internal(Box::new(child))
    }
}

// Build wrapper
impl<S: State> RepaintBoundaryBuilder<S> {
    /// Builds the RepaintBoundary widget.
    pub fn build(self) -> RepaintBoundary {
        self.build_internal()
    }
}

// Implement View trait
impl View for RepaintBoundary {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        SingleRenderBuilder::new(RenderRepaintBoundary::new()).with_optional_child(self.child)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_repaint_boundary_new() {
        let boundary = RepaintBoundary::new(crate::SizedBox::new());
        assert!(boundary.child.is_some());
    }

    #[test]
    fn test_repaint_boundary_wrap() {
        let boundary = RepaintBoundary::wrap(crate::SizedBox::new());
        assert!(boundary.child.is_some());
    }

    #[test]
    fn test_repaint_boundary_builder() {
        let boundary = RepaintBoundary::builder().build();
        assert!(boundary.child.is_none());
    }

    #[test]
    fn test_repaint_boundary_default() {
        let boundary = RepaintBoundary::default();
        assert!(boundary.child.is_none());
    }
}
