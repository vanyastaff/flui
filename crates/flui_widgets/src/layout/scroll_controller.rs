//! ScrollController - Controls scroll position programmatically
//!
//! Similar to Flutter's ScrollController, allows programmatic control
//! of scroll position.

use parking_lot::Mutex;
use std::sync::Arc;

/// Controller for scroll position
///
/// Can be passed to SingleChildScrollView to programmatically control
/// the scroll position.
///
/// # Example
///
/// ```rust,ignore
/// let controller = ScrollController::new();
///
/// // In your UI
/// SingleChildScrollView::builder()
///     .controller(controller.clone())
///     .child(my_content)
///     .build();
///
/// // Somewhere else
/// controller.scroll_to(100.0);
/// controller.scroll_by(50.0);
/// ```
#[derive(Debug, Clone)]
pub struct ScrollController {
    /// Current scroll offset (thread-safe)
    offset: Arc<Mutex<f32>>,

    /// Maximum scroll offset (set during layout)
    max_offset: Arc<Mutex<f32>>,
}

impl ScrollController {
    /// Create a new scroll controller
    pub fn new() -> Self {
        Self {
            offset: Arc::new(Mutex::new(0.0)),
            max_offset: Arc::new(Mutex::new(0.0)),
        }
    }

    /// Create with initial offset
    pub fn with_offset(offset: f32) -> Self {
        Self {
            offset: Arc::new(Mutex::new(offset.max(0.0))),
            max_offset: Arc::new(Mutex::new(0.0)),
        }
    }

    /// Get current scroll offset
    pub fn offset(&self) -> f32 {
        *self.offset.lock()
    }

    /// Get maximum scroll offset
    pub fn max_offset(&self) -> f32 {
        *self.max_offset.lock()
    }

    /// Set scroll offset (clamped to [0, max_offset])
    pub fn scroll_to(&self, offset: f32) {
        let max = *self.max_offset.lock();
        let clamped = offset.max(0.0).min(max);
        *self.offset.lock() = clamped;
    }

    /// Scroll by delta (positive = scroll down/right)
    pub fn scroll_by(&self, delta: f32) {
        let current = self.offset();
        self.scroll_to(current + delta);
    }

    /// Scroll to the start (offset = 0.0)
    pub fn scroll_to_start(&self) {
        self.scroll_to(0.0);
    }

    /// Scroll to the end (offset = max_offset)
    pub fn scroll_to_end(&self) {
        let max = self.max_offset();
        self.scroll_to(max);
    }

    /// Internal: Update max offset (called by RenderScrollView during layout)
    #[allow(dead_code)]
    pub(crate) fn update_max_offset(&self, max: f32) {
        *self.max_offset.lock() = max.max(0.0);

        // Clamp current offset if it exceeds new max
        let current = *self.offset.lock();
        if current > max {
            *self.offset.lock() = max.max(0.0);
        }
    }

    /// Check if scrolled to top
    pub fn is_at_start(&self) -> bool {
        self.offset() <= 0.0
    }

    /// Check if scrolled to bottom
    pub fn is_at_end(&self) -> bool {
        let offset = self.offset();
        let max = self.max_offset();
        (max - offset).abs() < 1.0 // Within 1px
    }

    /// Get internal offset Arc (for use by RenderScrollView)
    pub(crate) fn offset_arc(&self) -> Arc<Mutex<f32>> {
        Arc::clone(&self.offset)
    }

    /// Get internal max_offset Arc (for use by RenderScrollView)
    pub(crate) fn max_offset_arc(&self) -> Arc<Mutex<f32>> {
        Arc::clone(&self.max_offset)
    }
}

impl Default for ScrollController {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_controller_new() {
        let controller = ScrollController::new();
        assert_eq!(controller.offset(), 0.0);
        assert_eq!(controller.max_offset(), 0.0);
    }

    #[test]
    fn test_scroll_to() {
        let controller = ScrollController::new();
        controller.update_max_offset(100.0);

        controller.scroll_to(50.0);
        assert_eq!(controller.offset(), 50.0);

        // Test clamping
        controller.scroll_to(150.0);
        assert_eq!(controller.offset(), 100.0);

        controller.scroll_to(-10.0);
        assert_eq!(controller.offset(), 0.0);
    }

    #[test]
    fn test_scroll_by() {
        let controller = ScrollController::new();
        controller.update_max_offset(100.0);

        controller.scroll_by(30.0);
        assert_eq!(controller.offset(), 30.0);

        controller.scroll_by(20.0);
        assert_eq!(controller.offset(), 50.0);

        controller.scroll_by(-10.0);
        assert_eq!(controller.offset(), 40.0);
    }

    #[test]
    fn test_scroll_to_edges() {
        let controller = ScrollController::new();
        controller.update_max_offset(100.0);

        // Scroll to middle, then to start
        controller.scroll_to(50.0);
        controller.scroll_to(0.0);
        assert_eq!(controller.offset(), 0.0);

        // Scroll to end using max_offset
        controller.scroll_to(controller.max_offset());
        assert_eq!(controller.offset(), 100.0);
    }

    #[test]
    fn test_is_at_edges() {
        let controller = ScrollController::new();
        controller.update_max_offset(100.0);

        assert!(controller.is_at_start());
        assert!(!controller.is_at_end());

        // Scroll to end using max_offset
        controller.scroll_to(controller.max_offset());
        assert!(!controller.is_at_start());
        assert!(controller.is_at_end());
    }
}
