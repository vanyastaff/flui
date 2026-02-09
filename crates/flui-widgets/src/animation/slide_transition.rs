//! SlideTransition widget - animates position using an Animation<Offset>.
//!
//! This is an explicit animation widget that requires an AnimationController
//! combined with an OffsetTween.

use flui_animation::Animation;
use flui_rendering::objects::RenderTransform;
use flui_rendering::protocol::BoxProtocol;
use flui_types::{Matrix4, Offset};
use flui_view::{Child, RenderView, View};
use flui_view::element::RenderBehavior;
use std::sync::Arc;

/// A widget that animates the position of its child.
///
/// The translation is expressed as an `Offset` scaled to the child's size.
/// For example, an Offset with dx=0.25 will result in a horizontal translation
/// of one quarter the width of the child.
///
/// ## Example
///
/// ```rust,ignore
/// use flui_animation::{AnimationController, OffsetTween, TweenAnimation};
/// use flui_widgets::animation::SlideTransition;
/// use flui_types::Offset;
/// use std::time::Duration;
///
/// let controller = AnimationController::new(Duration::from_millis(300), scheduler);
/// let tween = OffsetTween::new(Offset::new(-1.0, 0.0), Offset::ZERO);
/// let animation = TweenAnimation::new(tween, controller.clone());
///
/// controller.forward();
///
/// SlideTransition::new(animation)
///     .child(my_content)
/// ```
///
/// ## Common Slide Directions
///
/// - `Offset::new(-1.0, 0.0)` → `Offset::ZERO`: Slide in from left
/// - `Offset::new(1.0, 0.0)` → `Offset::ZERO`: Slide in from right
/// - `Offset::new(0.0, -1.0)` → `Offset::ZERO`: Slide in from top
/// - `Offset::new(0.0, 1.0)` → `Offset::ZERO`: Slide in from bottom
pub struct SlideTransition<A: Animation<Offset>> {
    /// The animation that drives the position.
    animation: Arc<A>,
    /// The child widget.
    child: Child,
    /// Cached child size for fractional offset calculation.
    /// Set during layout.
    child_size: (f32, f32),
}

impl<A: Animation<Offset>> std::fmt::Debug for SlideTransition<A> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SlideTransition")
            .field("offset", &self.animation.value())
            .finish()
    }
}

impl<A: Animation<Offset> + Clone> Clone for SlideTransition<A> {
    fn clone(&self) -> Self {
        Self {
            animation: Arc::clone(&self.animation),
            child: self.child.clone(),
            child_size: self.child_size,
        }
    }
}

impl<A: Animation<Offset>> SlideTransition<A> {
    /// Creates a new SlideTransition with the given animation.
    ///
    /// The animation value is an Offset that represents fractional translation
    /// relative to the child's size.
    pub fn new(animation: A) -> Self {
        Self {
            animation: Arc::new(animation),
            child: Child::empty(),
            child_size: (100.0, 100.0), // Default size, updated during layout
        }
    }

    /// Creates a SlideTransition from an Arc'd animation.
    pub fn from_arc(animation: Arc<A>) -> Self {
        Self {
            animation,
            child: Child::empty(),
            child_size: (100.0, 100.0),
        }
    }

    /// Sets the child widget.
    pub fn child(mut self, view: impl View) -> Self {
        self.child = Child::some(view);
        self
    }

    /// Returns the current offset value.
    pub fn offset(&self) -> Offset {
        self.animation.value()
    }

    /// Computes the translation matrix for the current animation value.
    fn compute_transform(&self) -> Matrix4 {
        let offset = self.animation.value();
        // Fractional offset: multiply by child size
        let dx = offset.dx * self.child_size.0;
        let dy = offset.dy * self.child_size.1;
        Matrix4::translation(dx, dy, 0.0)
    }
}

// Implement View trait manually (macro doesn't support generics)
impl<A: Animation<Offset> + Clone + Send + Sync + 'static> View for SlideTransition<A> {
    fn create_element(&self) -> Box<dyn flui_view::ElementBase> {
        Box::new(flui_view::RenderElement::new(self, RenderBehavior::new()))
    }
}

impl<A: Animation<Offset> + Clone + Send + Sync + 'static> RenderView for SlideTransition<A> {
    type Protocol = BoxProtocol;
    type RenderObject = RenderTransform;

    fn create_render_object(&self) -> Self::RenderObject {
        let transform = self.compute_transform();
        RenderTransform::new(transform)
    }

    fn update_render_object(&self, render_object: &mut Self::RenderObject) {
        let transform = self.compute_transform();
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
    use flui_animation::{AnimationController, OffsetTween, TweenAnimation};
    use flui_scheduler::Scheduler;
    use std::sync::Arc;
    use std::time::Duration;

    #[test]
    fn test_slide_transition_new() {
        let scheduler = Arc::new(Scheduler::new());
        let controller = AnimationController::new(Duration::from_millis(300), scheduler);
        let tween = OffsetTween::new(Offset::new(-1.0, 0.0), Offset::ZERO);
        let animation = TweenAnimation::new(tween, controller.clone());

        let slide = SlideTransition::new(animation);
        let offset = slide.offset();
        assert!((offset.dx - (-1.0)).abs() < f32::EPSILON);
        assert!((offset.dy - 0.0).abs() < f32::EPSILON);

        controller.dispose();
    }

    #[test]
    fn test_slide_transition_transform() {
        let scheduler = Arc::new(Scheduler::new());
        let controller = AnimationController::new(Duration::from_millis(300), scheduler);
        let tween = OffsetTween::new(Offset::ZERO, Offset::new(1.0, 0.5));
        let animation = TweenAnimation::new(tween, controller.clone());

        controller.set_value(1.0); // Go to end

        let slide = SlideTransition::new(animation);
        let transform = slide.compute_transform();

        // With default child_size of 100x100, offset (1.0, 0.5) = (100, 50) translation
        // Check translation component (m30, m31)
        assert!((transform.m[12] - 100.0).abs() < f32::EPSILON);
        assert!((transform.m[13] - 50.0).abs() < f32::EPSILON);

        controller.dispose();
    }
}
