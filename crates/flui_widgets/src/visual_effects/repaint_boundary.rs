//! RepaintBoundary widget - optimization boundary for repainting
//!
//! A widget that creates a separate paint layer, isolating the child's
//! repainting from its ancestors for performance optimization.

use bon::Builder;
use flui_core::view::{AnyView, ChangeFlags, View};
use flui_core::render::RenderNode;
use flui_core::{BuildContext, Element};
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
#[builder(on(String, into), finish_fn = build_repaint_boundary)]
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
            .field("child", &if self.child.is_some() { "<AnyView>" } else { "None" })
            .finish()
    }
}

impl Clone for RepaintBoundary {
    fn clone(&self) -> Self {
        Self {
            key: self.key.clone(),
            child: None,
        }
    }
}

impl RepaintBoundary {
    /// Creates a new RepaintBoundary.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let boundary = RepaintBoundary::new(Box::new(child));
    /// ```
    pub fn new(child: Box<dyn AnyView>) -> Self {
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
    /// let boundary = RepaintBoundary::wrap(Box::new(animated_widget));
    /// ```
    pub fn wrap(child: Box<dyn AnyView>) -> Self {
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
    pub fn child(self, child: impl View + 'static) -> RepaintBoundaryBuilder<SetChild<S>> {
        self.child_internal(Box::new(child))
    }
}

// Implement View trait
impl View for RepaintBoundary {
    type Element = Element;
    type State = Option<Box<dyn std::any::Any>>;

    fn build(self, ctx: &mut BuildContext) -> (Self::Element, Self::State) {
        // Build child first
        let (child_id, child_state) = if let Some(child) = self.child {
            let (elem, state) = child.build_any(ctx);
            let id = ctx.tree().write().insert(elem.into_element());
            (Some(id), Some(state))
        } else {
            (None, None)
        };

        // Create RenderRepaintBoundary
        let render = RenderRepaintBoundary::new();

        let render_node = RenderNode::Single {
            render: Box::new(render),
            child: child_id,
        };

        let render_element = flui_core::element::RenderElement::new(render_node);
        (Element::Render(render_element), child_state)
    }

    fn rebuild(
        self,
        _prev: &Self,
        _state: &mut Self::State,
        _element: &mut Self::Element,
    ) -> ChangeFlags {
        // RepaintBoundary has no mutable properties to update
        // The is_repaint_boundary flag is always true for this widget
        // Child changes handled by element tree
        ChangeFlags::NONE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_repaint_boundary_new() {
        let boundary = RepaintBoundary::new(Box::new(crate::SizedBox::new()));
        assert!(boundary.child.is_some());
    }

    #[test]
    fn test_repaint_boundary_wrap() {
        let boundary = RepaintBoundary::wrap(Box::new(crate::SizedBox::new()));
        assert!(boundary.child.is_some());
    }

    #[test]
    fn test_repaint_boundary_builder() {
        let boundary = RepaintBoundary::builder()
            .build_repaint_boundary();
        assert!(boundary.child.is_none());
    }

    #[test]
    fn test_repaint_boundary_default() {
        let boundary = RepaintBoundary::default();
        assert!(boundary.child.is_none());
    }
}
