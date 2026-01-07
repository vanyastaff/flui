//! ScaleTransition widget - animates scale using an Animation<f32>.
//!
//! This is an explicit animation widget that requires an AnimationController.

use flui_animation::Animation;
use flui_rendering::objects::RenderTransform;
use flui_rendering::protocol::BoxProtocol;
use flui_types::{Alignment, Matrix4};
use flui_view::{Child, RenderView, View};
use flui_view::element::RenderBehavior;
use std::sync::Arc;

/// A widget that animates the scale of its child.
///
/// The scale is driven by an `Animation<f32>`. The transformation is applied
/// around an alignment point (default: center).
///
/// ## Example
///
/// ```rust,ignore
/// use flui_animation::AnimationController;
/// use flui_widgets::animation::ScaleTransition;
/// use std::time::Duration;
///
/// let controller = AnimationController::new(Duration::from_millis(300), scheduler);
/// controller.forward();
///
/// ScaleTransition::new(controller.clone())
///     .child(my_content)
/// ```
///
/// ## Scale Values
///
/// - 0.0 = invisible (scaled to zero)
/// - 0.5 = half size
/// - 1.0 = normal size
/// - 2.0 = double size
pub struct ScaleTransition<A: Animation<f32>> {
    /// The animation that drives the scale.
    animation: Arc<A>,
    /// Alignment for the scale origin.
    alignment: Alignment,
    /// The child widget.
    child: Child,
}

impl<A: Animation<f32>> std::fmt::Debug for ScaleTransition<A> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScaleTransition")
            .field("scale", &self.animation.value())
            .field("alignment", &self.alignment)
            .finish()
    }
}

impl<A: Animation<f32> + Clone> Clone for ScaleTransition<A> {
    fn clone(&self) -> Self {
        Self {
            animation: Arc::clone(&self.animation),
            alignment: self.alignment,
            child: self.child.clone(),
        }
    }
}

impl<A: Animation<f32>> ScaleTransition<A> {
    /// Creates a new ScaleTransition with the given animation.
    ///
    /// The animation value controls the scale factor.
    /// Scale is applied around the center by default.
    pub fn new(animation: A) -> Self {
        Self {
            animation: Arc::new(animation),
            alignment: Alignment::CENTER,
            child: Child::empty(),
        }
    }

    /// Creates a ScaleTransition from an Arc'd animation.
    pub fn from_arc(animation: Arc<A>) -> Self {
        Self {
            animation,
            alignment: Alignment::CENTER,
            child: Child::empty(),
        }
    }

    /// Sets the alignment for the scale origin.
    ///
    /// The scale transformation will be applied around this point.
    pub fn alignment(mut self, alignment: Alignment) -> Self {
        self.alignment = alignment;
        self
    }

    /// Sets the child widget.
    pub fn child(mut self, view: impl View) -> Self {
        self.child = Child::some(view);
        self
    }

    /// Returns the current scale value.
    pub fn scale(&self) -> f32 {
        self.animation.value()
    }
}

// Implement View trait manually (macro doesn't support generics)
impl<A: Animation<f32> + Clone + Send + Sync + 'static> View for ScaleTransition<A> {
    fn create_element(&self) -> Box<dyn flui_view::ElementBase> {
        Box::new(flui_view::RenderElement::new(self, RenderBehavior::new()))
    }
}

impl<A: Animation<f32> + Clone + Send + Sync + 'static> RenderView for ScaleTransition<A> {
    type Protocol = BoxProtocol;
    type RenderObject = RenderTransform;

    fn create_render_object(&self) -> Self::RenderObject {
        let scale = self.animation.value();
        let transform = Matrix4::scaling(scale, scale, 1.0);
        RenderTransform::new(transform).with_alignment(self.alignment)
    }

    fn update_render_object(&self, render_object: &mut Self::RenderObject) {
        let scale = self.animation.value();
        let transform = Matrix4::scaling(scale, scale, 1.0);
        render_object.set_transform(transform);
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
    fn test_scale_transition_new() {
        let scheduler = Arc::new(Scheduler::new());
        let controller = AnimationController::new(Duration::from_millis(300), scheduler);

        let scale = ScaleTransition::new(controller.clone());
        assert!((scale.scale() - 0.0).abs() < f32::EPSILON);

        controller.dispose();
    }

    #[test]
    fn test_scale_transition_value() {
        let scheduler = Arc::new(Scheduler::new());
        let controller = AnimationController::new(Duration::from_millis(300), scheduler);
        controller.set_value(0.5);

        let scale = ScaleTransition::new(controller.clone());
        assert!((scale.scale() - 0.5).abs() < f32::EPSILON);

        controller.dispose();
    }

    #[test]
    fn test_scale_transition_alignment() {
        let scheduler = Arc::new(Scheduler::new());
        let controller = AnimationController::new(Duration::from_millis(300), scheduler);

        let scale = ScaleTransition::new(controller.clone()).alignment(Alignment::TOP_LEFT);
        assert_eq!(scale.alignment, Alignment::TOP_LEFT);

        controller.dispose();
    }
}
