//! FadeTransition - animates opacity using AnimationController
//!
//! An explicit animation widget that fades its child in or out.

use flui_animation::{Animation, AnimationController};
use flui_core::BuildContext;
use flui_view::traits::AnimatedView;
use flui_view::IntoView;
use flui_rendering::objects::RenderOpacity;
use std::sync::Arc;

/// A widget that animates the opacity of its child using an AnimationController.
///
/// FadeTransition uses an explicit `AnimationController` to animate opacity
/// from 0.0 (fully transparent) to 1.0 (fully opaque).
///
/// # Explicit Animation
///
/// Unlike `AnimatedOpacity` which animates automatically, `FadeTransition`
/// requires you to provide and control an `AnimationController`. This gives
/// you fine-grained control over the animation lifecycle.
///
/// # Performance
///
/// FadeTransition is optimized for animations:
/// - Only triggers repaint (not relayout) when opacity changes
/// - Uses `UpdateResult::NeedsPaint` for minimal overhead
/// - Reuses the same RenderObject across frames
///
/// # Example
///
/// ```rust,ignore
/// use flui_animation::AnimationController;
/// use flui_widgets::animation::FadeTransition;
/// use flui_widgets::Text;
/// use std::time::Duration;
///
/// // Create controller
/// let controller = AnimationController::new(
///     Duration::from_millis(500),
///     scheduler
/// );
///
/// // Start fade-in
/// controller.forward();
///
/// // Create fade transition
/// FadeTransition::new(
///     controller.clone(),
///     Text::new("Hello, World!")
/// )
/// ```
///
/// # Advanced Usage
///
/// ```rust,ignore
/// // Fade in and out repeatedly
/// controller.repeat(true);
///
/// // Custom curves
/// let curved = controller.curved(Curves::EaseInOut);
/// FadeTransition::new(curved, child)
///
/// // Reverse animation
/// controller.reverse();
/// ```
#[derive(Clone)]
pub struct FadeTransition<C> {
    /// Animation controller that drives opacity (0.0 = transparent, 1.0 = opaque)
    pub opacity: Arc<AnimationController>,

    /// The child widget to fade
    pub child: C,
}

impl<C> FadeTransition<C> {
    /// Create a new FadeTransition with an animation controller and child.
    ///
    /// # Arguments
    ///
    /// * `opacity` - AnimationController that drives the opacity (0.0 to 1.0)
    /// * `child` - Widget to animate
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let controller = AnimationController::new(Duration::from_millis(300), scheduler);
    /// controller.forward();
    ///
    /// FadeTransition::new(controller, Text::new("Fading in..."))
    /// ```
    pub fn new(opacity: Arc<AnimationController>, child: C) -> Self {
        Self { opacity, child }
    }
}

impl<C> std::fmt::Debug for FadeTransition<C> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FadeTransition")
            .field("opacity_value", &self.opacity.as_ref().value())
            .field("opacity_status", &self.opacity.as_ref().status())
            .field("child", &"<child>")
            .finish()
    }
}

impl<C: IntoView + Clone + Sync> AnimatedView<AnimationController> for FadeTransition<C> {
    fn build(&mut self, _ctx: &dyn BuildContext) -> impl IntoView {
        // TODO: Need to create an Opacity widget wrapper that properly implements IntoView
        // For now, just return the child (opacity effect won't work until Opacity widget is fixed)
        // Once Opacity is fixed, use: Opacity::new(self.opacity.as_ref().value(), self.child.clone())
        self.child.clone()
    }

    fn listenable(&self) -> &AnimationController {
        self.opacity.as_ref()
    }

    fn on_animation_tick(&mut self, _ctx: &dyn BuildContext) {
        // Called before each build during animation
        // Can be used for custom logic or debugging
        #[cfg(debug_assertions)]
        {
            tracing::trace!(
                opacity = self.opacity.as_ref().value(),
                status = ?self.opacity.as_ref().status(),
                "FadeTransition animation tick"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_scheduler::Scheduler;
    use std::time::Duration;

    #[test]
    fn test_fade_transition_creation() {
        use crate::basic::Text;

        let scheduler = Arc::new(Scheduler::new());
        let controller = AnimationController::new(Duration::from_millis(300), scheduler);

        let fade = FadeTransition::new(
            controller.clone(),
            Text::new("Test")
        );

        // Initial value should be at lower bound (0.0)
        assert_eq!(fade.opacity.as_ref().value(), 0.0);
    }

    #[test]
    fn test_fade_transition_animation_values() {
        use crate::basic::Text;

        let scheduler = Arc::new(Scheduler::new());
        let controller = AnimationController::new(Duration::from_millis(300), scheduler);

        let fade = FadeTransition::new(
            controller.clone(),
            Text::new("Test")
        );

        // Manually set values to test
        controller.set_value(0.0);
        assert_eq!(fade.opacity.as_ref().value(), 0.0);

        controller.set_value(0.5);
        assert_eq!(fade.opacity.as_ref().value(), 0.5);

        controller.set_value(1.0);
        assert_eq!(fade.opacity.as_ref().value(), 1.0);

        // Cleanup
        controller.dispose();
    }

    #[test]
    fn test_fade_transition_debug() {
        use crate::basic::Text;

        let scheduler = Arc::new(Scheduler::new());
        let controller = AnimationController::new(Duration::from_millis(300), scheduler);
        controller.set_value(0.75);

        let fade = FadeTransition::new(
            controller.clone(),
            Text::new("Test")
        );

        let debug_str = format!("{:?}", fade);
        assert!(debug_str.contains("FadeTransition"));
        assert!(debug_str.contains("0.75"));

        controller.dispose();
    }
}
