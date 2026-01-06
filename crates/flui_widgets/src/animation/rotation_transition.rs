//! RotationTransition widget - animates rotation using an Animation<f32>.
//!
//! This is an explicit animation widget that requires an AnimationController.

use flui_animation::Animation;
use flui_rendering::objects::RenderTransform;
use flui_rendering::wrapper::BoxWrapper;
use flui_types::{Alignment, Matrix4};
use flui_view::{Child, RenderView, View};
use std::f32::consts::TAU;
use std::sync::Arc;

/// A widget that animates the rotation of its child.
///
/// The rotation is driven by an `Animation<f32>` where the value represents
/// the number of turns (1.0 = 360 degrees = 2π radians).
///
/// ## Example
///
/// ```rust,ignore
/// use flui_animation::AnimationController;
/// use flui_widgets::animation::RotationTransition;
/// use std::time::Duration;
///
/// let controller = AnimationController::new(Duration::from_millis(1000), scheduler);
/// controller.repeat(false); // Continuous rotation
///
/// RotationTransition::new(controller.clone())
///     .child(my_icon)
/// ```
///
/// ## Rotation Values
///
/// - 0.0 = no rotation
/// - 0.25 = 90 degrees (quarter turn)
/// - 0.5 = 180 degrees (half turn)
/// - 1.0 = 360 degrees (full turn)
/// - 2.0 = 720 degrees (two full turns)
pub struct RotationTransition<A: Animation<f32>> {
    /// The animation that drives the rotation.
    animation: Arc<A>,
    /// Alignment for the rotation origin.
    alignment: Alignment,
    /// The child widget.
    child: Child,
}

impl<A: Animation<f32>> std::fmt::Debug for RotationTransition<A> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RotationTransition")
            .field("turns", &self.animation.value())
            .field("alignment", &self.alignment)
            .finish()
    }
}

impl<A: Animation<f32> + Clone> Clone for RotationTransition<A> {
    fn clone(&self) -> Self {
        Self {
            animation: Arc::clone(&self.animation),
            alignment: self.alignment,
            child: self.child.clone(),
        }
    }
}

impl<A: Animation<f32>> RotationTransition<A> {
    /// Creates a new RotationTransition with the given animation.
    ///
    /// The animation value represents the number of turns:
    /// - 0.0 = 0 degrees
    /// - 0.5 = 180 degrees
    /// - 1.0 = 360 degrees
    pub fn new(animation: A) -> Self {
        Self {
            animation: Arc::new(animation),
            alignment: Alignment::CENTER,
            child: Child::empty(),
        }
    }

    /// Creates a RotationTransition from an Arc'd animation.
    pub fn from_arc(animation: Arc<A>) -> Self {
        Self {
            animation,
            alignment: Alignment::CENTER,
            child: Child::empty(),
        }
    }

    /// Sets the alignment for the rotation origin.
    ///
    /// The rotation will be applied around this point.
    /// Default is center.
    pub fn alignment(mut self, alignment: Alignment) -> Self {
        self.alignment = alignment;
        self
    }

    /// Sets the child widget.
    pub fn child(mut self, view: impl View) -> Self {
        self.child = Child::some(view);
        self
    }

    /// Returns the current rotation in turns.
    pub fn turns(&self) -> f32 {
        self.animation.value()
    }

    /// Returns the current rotation in radians.
    pub fn radians(&self) -> f32 {
        self.animation.value() * TAU
    }

    /// Returns the current rotation in degrees.
    pub fn degrees(&self) -> f32 {
        self.animation.value() * 360.0
    }
}

// Implement View trait manually (macro doesn't support generics)
impl<A: Animation<f32> + Clone + Send + Sync + 'static> View for RotationTransition<A> {
    fn create_element(&self) -> Box<dyn flui_view::ElementBase> {
        Box::new(flui_view::RenderElement::new(self))
    }
}

impl<A: Animation<f32> + Clone + Send + Sync + 'static> RenderView for RotationTransition<A> {
    type RenderObject = BoxWrapper<RenderTransform>;

    fn create_render_object(&self) -> Self::RenderObject {
        let radians = self.radians();
        let transform = Matrix4::rotation_z(radians);
        let render = RenderTransform::new(transform).with_alignment(self.alignment);
        BoxWrapper::new(render)
    }

    fn update_render_object(&self, render_object: &mut Self::RenderObject) {
        let radians = self.radians();
        let transform = Matrix4::rotation_z(radians);
        render_object.inner_mut().set_transform(transform);
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
    use std::f32::consts::PI;
    use std::sync::Arc;
    use std::time::Duration;

    #[test]
    fn test_rotation_transition_new() {
        let scheduler = Arc::new(Scheduler::new());
        let controller = AnimationController::new(Duration::from_millis(300), scheduler);

        let rotation = RotationTransition::new(controller.clone());
        assert!((rotation.turns() - 0.0).abs() < f32::EPSILON);

        controller.dispose();
    }

    #[test]
    fn test_rotation_transition_half_turn() {
        let scheduler = Arc::new(Scheduler::new());
        let controller = AnimationController::new(Duration::from_millis(300), scheduler);
        controller.set_value(0.5); // Half turn

        let rotation = RotationTransition::new(controller.clone());
        assert!((rotation.turns() - 0.5).abs() < f32::EPSILON);
        assert!((rotation.radians() - PI).abs() < 0.0001);
        assert!((rotation.degrees() - 180.0).abs() < 0.0001);

        controller.dispose();
    }

    #[test]
    fn test_rotation_transition_full_turn() {
        let scheduler = Arc::new(Scheduler::new());
        let controller = AnimationController::new(Duration::from_millis(300), scheduler);
        controller.set_value(1.0); // Full turn

        let rotation = RotationTransition::new(controller.clone());
        assert!((rotation.turns() - 1.0).abs() < f32::EPSILON);
        assert!((rotation.radians() - TAU).abs() < 0.0001);
        assert!((rotation.degrees() - 360.0).abs() < 0.0001);

        controller.dispose();
    }

    #[test]
    fn test_rotation_transition_alignment() {
        let scheduler = Arc::new(Scheduler::new());
        let controller = AnimationController::new(Duration::from_millis(300), scheduler);

        let rotation =
            RotationTransition::new(controller.clone()).alignment(Alignment::BOTTOM_RIGHT);
        assert_eq!(rotation.alignment, Alignment::BOTTOM_RIGHT);

        controller.dispose();
    }
}
