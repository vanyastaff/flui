//! FadeTransition widget - animates opacity using an Animation<f32>.
//!
//! This is an explicit animation widget that requires an AnimationController.

use flui_animation::Animation;
use flui_rendering::objects::RenderOpacity;
use flui_rendering::protocol::BoxProtocol;
use flui_view::{Child, RenderView, View};
use flui_view::element::RenderBehavior;
use std::sync::Arc;

/// A widget that animates the opacity of its child.
///
/// The opacity is driven by an `Animation<f32>` (typically from `AnimationController`).
/// Values are clamped to 0.0-1.0 range.
///
/// ## Example
///
/// ```rust,ignore
/// use flui_animation::AnimationController;
/// use flui_widgets::animation::FadeTransition;
/// use std::time::Duration;
///
/// let controller = AnimationController::new(Duration::from_millis(300), scheduler);
/// controller.forward();
///
/// FadeTransition::new(controller.clone())
///     .child(my_content)
/// ```
///
/// ## Comparison with AnimatedOpacity
///
/// - `FadeTransition`: You control the animation with an `AnimationController`
/// - `AnimatedOpacity`: Animation happens automatically when opacity changes
pub struct FadeTransition<A: Animation<f32>> {
    /// The animation that drives the opacity.
    animation: Arc<A>,
    /// The child widget.
    child: Child,
}

impl<A: Animation<f32>> std::fmt::Debug for FadeTransition<A> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FadeTransition")
            .field("opacity", &self.animation.value())
            .finish()
    }
}

impl<A: Animation<f32> + Clone> Clone for FadeTransition<A> {
    fn clone(&self) -> Self {
        Self {
            animation: Arc::clone(&self.animation),
            child: self.child.clone(),
        }
    }
}

impl<A: Animation<f32>> FadeTransition<A> {
    /// Creates a new FadeTransition with the given animation.
    ///
    /// The animation value (0.0-1.0) controls the opacity:
    /// - 0.0 = fully transparent
    /// - 1.0 = fully opaque
    pub fn new(animation: A) -> Self {
        Self {
            animation: Arc::new(animation),
            child: Child::empty(),
        }
    }

    /// Creates a FadeTransition from an Arc'd animation.
    ///
    /// Use this when you need to share the animation with other widgets.
    pub fn from_arc(animation: Arc<A>) -> Self {
        Self {
            animation,
            child: Child::empty(),
        }
    }

    /// Sets the child widget.
    pub fn child(mut self, view: impl View) -> Self {
        self.child = Child::some(view);
        self
    }

    /// Returns the current opacity value.
    pub fn opacity(&self) -> f32 {
        self.animation.value().clamp(0.0, 1.0)
    }
}

// Implement View trait manually (macro doesn't support generics)
impl<A: Animation<f32> + Clone + Send + Sync + 'static> View for FadeTransition<A> {
    fn create_element(&self) -> Box<dyn flui_view::ElementBase> {
        Box::new(flui_view::RenderElement::new(self, RenderBehavior::new()))
    }
}

impl<A: Animation<f32> + Clone + Send + Sync + 'static> RenderView for FadeTransition<A> {
    type Protocol = BoxProtocol;
    type RenderObject = RenderOpacity;

    fn create_render_object(&self) -> Self::RenderObject {
        RenderOpacity::new(self.opacity())
    }

    fn update_render_object(&self, render_object: &mut Self::RenderObject) {
        let current_opacity = self.opacity();
        if (render_object.opacity() - current_opacity).abs() > f32::EPSILON {
            render_object.set_opacity(current_opacity);
        }
    }

    fn has_children(&self) -> bool {
        self.child.is_some()
    }

    fn visit_child_views(&self, visitor: &mut dyn FnMut(&dyn View)) {
        if let Some(child_view) = self.child.as_ref() {
            visitor(child_view);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_animation::AnimationController;
    use flui_scheduler::Scheduler;
    use std::sync::Arc;
    use std::time::Duration;

    #[test]
    fn test_fade_transition_new() {
        let scheduler = Arc::new(Scheduler::new());
        let controller = AnimationController::new(Duration::from_millis(300), scheduler);

        let fade = FadeTransition::new(controller.clone());
        assert!((fade.opacity() - 0.0).abs() < f32::EPSILON);

        controller.dispose();
    }

    #[test]
    fn test_fade_transition_opacity() {
        let scheduler = Arc::new(Scheduler::new());
        let controller = AnimationController::new(Duration::from_millis(300), scheduler);
        controller.set_value(0.5);

        let fade = FadeTransition::new(controller.clone());
        assert!((fade.opacity() - 0.5).abs() < f32::EPSILON);

        controller.dispose();
    }

    #[test]
    fn test_fade_transition_clamp() {
        let scheduler = Arc::new(Scheduler::new());
        let controller =
            AnimationController::with_bounds(Duration::from_millis(300), scheduler, -1.0, 2.0)
                .unwrap();
        controller.set_value(1.5);

        let fade = FadeTransition::new(controller.clone());
        // Should clamp to 1.0
        assert!((fade.opacity() - 1.0).abs() < f32::EPSILON);

        controller.dispose();
    }
}
