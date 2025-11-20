//! Scrollable - Gesture and physics coordinator for scrolling
//!
//! This widget handles scroll gestures, physics, and scroll position management.
//! It delegates visual rendering to a Viewport child.

use crate::layout::ScrollController;
use flui_core::view::{BuildContext, IntoElement, View};
use flui_types::layout::AxisDirection;

/// Scrollable widget that handles gestures and scroll physics
///
/// This widget is responsible for:
/// - Detecting scroll gestures (drag, fling)
/// - Applying scroll physics (bounce, clamp)
/// - Managing scroll position via ScrollController
/// - Coordinating with Viewport for visual rendering
///
/// # Architecture
///
/// ```text
/// Scrollable
///   ├── Gesture Detection Layer
///   ├── Physics Layer
///   ├── ScrollController (state)
///   └── Viewport (child)
/// ```
///
/// # Example
///
/// ```rust,ignore
/// use flui_widgets::scrolling::Scrollable;
/// use flui_widgets::layout::ScrollController;
///
/// let controller = ScrollController::new();
///
/// Scrollable::new(viewport_widget)
///     .axis_direction(AxisDirection::TopToBottom)
///     .controller(controller)
/// ```
#[derive(Clone)]
pub struct Scrollable {
    /// The viewport child that renders scrollable content
    pub child: Box<dyn >,

    /// Scroll axis direction
    pub axis_direction: AxisDirection,

    /// Optional scroll controller for programmatic control
    pub controller: Option<ScrollController>,

    /// Whether physics are enabled (bounce, overscroll)
    pub physics_enabled: bool,

    /// Whether to reverse the scroll direction
    pub reverse: bool,
}

impl std::fmt::Debug for Scrollable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Scrollable")
            .field("child", &"<dyn >")
            .field("axis_direction", &self.axis_direction)
            .field("controller", &self.controller)
            .field("physics_enabled", &self.physics_enabled)
            .field("reverse", &self.reverse)
            .finish()
    }
}

impl Scrollable {
    /// Create new Scrollable with a viewport child
    pub fn new(child: impl View + 'static) -> Self {
        Self {
            child: Box::new(child),
            axis_direction: AxisDirection::TopToBottom,
            controller: None,
            physics_enabled: true,
            reverse: false,
        }
    }

    /// Set the axis direction
    pub fn axis_direction(mut self, direction: AxisDirection) -> Self {
        self.axis_direction = direction;
        self
    }

    /// Set the scroll controller
    pub fn controller(mut self, controller: ScrollController) -> Self {
        self.controller = Some(controller);
        self
    }

    /// Enable or disable scroll physics
    pub fn physics_enabled(mut self, enabled: bool) -> Self {
        self.physics_enabled = enabled;
        self
    }

    /// Set whether to reverse scroll direction
    pub fn reverse(mut self, reverse: bool) -> Self {
        self.reverse = reverse;
        self
    }
}

impl View for Scrollable {
    fn build(&self, _ctx: &BuildContext) -> impl IntoElement {
        // TODO: Implement gesture detection and physics
        // For now, just pass through to child (Viewport)
        // In full implementation:
        // 1. Wrap child in GestureDetector for drag events
        // 2. Apply scroll physics (bounce, clamp, etc.)
        // 3. Update ScrollController position
        // 4. Pass scroll offset to Viewport

        // Placeholder: Just return the viewport child
        // The viewport will handle rendering slivers
        self.child
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::sized_box::SizedBox;

    #[test]
    fn test_scrollable_new() {
        let child = SizedBox::new().width(100.0).height(100.0);
        let scrollable = Scrollable::new(child);

        assert_eq!(scrollable.axis_direction, AxisDirection::TopToBottom);
        assert!(scrollable.physics_enabled);
        assert!(!scrollable.reverse);
    }

    #[test]
    fn test_scrollable_builder() {
        let child = SizedBox::new().width(100.0).height(100.0);
        let controller = ScrollController::new();

        let scrollable = Scrollable::new(child)
            .axis_direction(AxisDirection::LeftToRight)
            .controller(controller.clone())
            .physics_enabled(false)
            .reverse(true);

        assert_eq!(scrollable.axis_direction, AxisDirection::LeftToRight);
        assert!(!scrollable.physics_enabled);
        assert!(scrollable.reverse);
        assert!(scrollable.controller.is_some());
    }
}
