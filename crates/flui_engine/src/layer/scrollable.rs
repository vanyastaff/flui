//! ScrollableLayer - Layer that handles scroll events
//!
//! Wraps a child layer and intercepts scroll events to update scroll offset.

use super::base::Layer;
use super::BoxedLayer;
use crate::painter::Painter;
use flui_types::events::{Event, HitTestResult, ScrollDelta};
use flui_types::{Offset, Rect};
use std::sync::Arc;

/// Callback type for scroll events
/// Parameters: (delta_x, delta_y) - scroll delta in pixels
pub type ScrollCallback = Arc<dyn Fn(f32, f32) + Send + Sync>;

/// A layer that handles scroll events
///
/// This layer wraps a child layer and intercepts scroll events within its bounds,
/// calling a callback with the scroll delta.
///
/// # Example
///
/// ```rust,ignore
/// let scroll_callback = Arc::new(|dx, dy| {
///     println!("Scrolled: dx={}, dy={}", dx, dy);
/// });
///
/// let scrollable = ScrollableLayer::new(child_layer, bounds, scroll_callback);
/// ```
pub struct ScrollableLayer {
    /// The child layer to wrap
    child: BoxedLayer,

    /// The bounds of this scrollable region (for hit testing)
    bounds: Rect,

    /// Callback to invoke when scroll events occur
    on_scroll: ScrollCallback,
}

impl ScrollableLayer {
    /// Create a new scrollable layer
    pub fn new(child: BoxedLayer, bounds: Rect, on_scroll: ScrollCallback) -> Self {
        Self {
            child,
            bounds,
            on_scroll,
        }
    }

    /// Convert scroll delta to pixels
    fn delta_to_pixels(delta: &ScrollDelta) -> (f32, f32) {
        match delta {
            ScrollDelta::Lines { x, y } => {
                // Typical line height is about 20 pixels
                const PIXELS_PER_LINE: f32 = 20.0;
                (x * PIXELS_PER_LINE, y * PIXELS_PER_LINE)
            }
            ScrollDelta::Pixels { x, y } => (*x, *y),
        }
    }
}

impl Layer for ScrollableLayer {
    fn paint(&self, painter: &mut dyn Painter) {
        // Just paint the child
        self.child.paint(painter);
    }

    fn bounds(&self) -> Rect {
        self.bounds
    }

    fn is_visible(&self) -> bool {
        self.child.is_visible()
    }

    fn hit_test(&self, position: Offset, result: &mut HitTestResult) -> bool {
        // First check if we're within our bounds
        if !self.bounds.contains(position) {
            return false;
        }

        // Then delegate to child
        self.child.hit_test(position, result)
    }

    fn handle_event(&mut self, event: &Event) -> bool {
        // Check if this is a scroll event
        if let Event::Scroll(scroll_data) = event {
            // Check if the scroll position is within our bounds
            if self.bounds.contains(scroll_data.position) {
                // Convert delta to pixels
                let (dx, dy) = Self::delta_to_pixels(&scroll_data.delta);

                // Call the scroll callback
                (self.on_scroll)(dx, dy);

                // Event was handled
                return true;
            }
        }

        // For other events or if scroll wasn't in our bounds, delegate to child
        self.child.handle_event(event)
    }

    fn mark_needs_paint(&mut self) {
        self.child.mark_needs_paint();
    }

    fn dispose(&mut self) {
        self.child.dispose();
    }

    fn is_disposed(&self) -> bool {
        self.child.is_disposed()
    }

    fn debug_description(&self) -> String {
        format!(
            "ScrollableLayer(bounds: {:?}, child: {})",
            self.bounds,
            self.child.debug_description()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_delta_to_pixels() {
        // Test line delta
        let (dx, dy) = ScrollableLayer::delta_to_pixels(&ScrollDelta::Lines { x: 1.0, y: 2.0 });
        assert_eq!(dx, 20.0);
        assert_eq!(dy, 40.0);

        // Test pixel delta
        let (dx, dy) = ScrollableLayer::delta_to_pixels(&ScrollDelta::Pixels { x: 10.0, y: 20.0 });
        assert_eq!(dx, 10.0);
        assert_eq!(dy, 20.0);
    }
}
