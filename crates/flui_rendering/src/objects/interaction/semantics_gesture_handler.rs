//! RenderSemanticsGestureHandler - handles gestures for accessibility
//!
//! This RenderObject listens for gestures from the semantics server
//! (e.g., accessibility tools like screen readers).
//!
//! Flutter reference: https://api.flutter.dev/flutter/rendering/RenderSemanticsGestureHandler-class.html

use crate::core::{BoxLayoutCtx, BoxPaintCtx, FullRenderTree, RenderBox, Single};
use flui_types::Size;
use std::sync::Arc;

/// Callback type for semantics gesture events
pub type SemanticsGestureCallback = Arc<dyn Fn() + Send + Sync>;

/// Callback type for drag update events
pub type SemanticsDragCallback = Arc<dyn Fn(f32) + Send + Sync>;

/// Callbacks for semantics gesture events
///
/// These callbacks are invoked by accessibility tools (e.g., screen readers)
/// when users perform gestures on this element.
#[derive(Clone, Default)]
pub struct SemanticsGestureCallbacks {
    /// Called when users tap the render object via accessibility
    pub on_tap: Option<SemanticsGestureCallback>,

    /// Called during prolonged pressing via accessibility
    pub on_long_press: Option<SemanticsGestureCallback>,

    /// Called for horizontal drag/scroll gestures via accessibility
    /// The parameter is the scroll delta (positive = right, negative = left)
    pub on_horizontal_drag_update: Option<SemanticsDragCallback>,

    /// Called for vertical drag/scroll gestures via accessibility
    /// The parameter is the scroll delta (positive = down, negative = up)
    pub on_vertical_drag_update: Option<SemanticsDragCallback>,
}

impl SemanticsGestureCallbacks {
    /// Create new empty callbacks
    pub fn new() -> Self {
        Self::default()
    }

    /// Set on_tap callback
    pub fn with_on_tap<F>(mut self, callback: F) -> Self
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.on_tap = Some(Arc::new(callback));
        self
    }

    /// Set on_long_press callback
    pub fn with_on_long_press<F>(mut self, callback: F) -> Self
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.on_long_press = Some(Arc::new(callback));
        self
    }

    /// Set on_horizontal_drag_update callback
    pub fn with_on_horizontal_drag_update<F>(mut self, callback: F) -> Self
    where
        F: Fn(f32) + Send + Sync + 'static,
    {
        self.on_horizontal_drag_update = Some(Arc::new(callback));
        self
    }

    /// Set on_vertical_drag_update callback
    pub fn with_on_vertical_drag_update<F>(mut self, callback: F) -> Self
    where
        F: Fn(f32) + Send + Sync + 'static,
    {
        self.on_vertical_drag_update = Some(Arc::new(callback));
        self
    }
}

impl std::fmt::Debug for SemanticsGestureCallbacks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SemanticsGestureCallbacks")
            .field("on_tap", &self.on_tap.is_some())
            .field("on_long_press", &self.on_long_press.is_some())
            .field(
                "on_horizontal_drag_update",
                &self.on_horizontal_drag_update.is_some(),
            )
            .field(
                "on_vertical_drag_update",
                &self.on_vertical_drag_update.is_some(),
            )
            .finish()
    }
}

/// RenderObject that handles gestures for accessibility
///
/// This widget listens for gestures from the semantics server (e.g., an
/// accessibility tool like a screen reader) and invokes callbacks.
///
/// # Scroll Factor
///
/// The `scroll_factor` property determines how much of the box's dimension
/// is used for scroll gesture calculations. For example, with a factor of 0.8
/// and a 200-pixel width, a leftward scroll gesture produces a 160-pixel drag.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::{RenderSemanticsGestureHandler, SemanticsGestureCallbacks};
///
/// let callbacks = SemanticsGestureCallbacks::new()
///     .with_on_tap(|| println!("Tapped via accessibility"))
///     .with_on_long_press(|| println!("Long pressed via accessibility"));
///
/// let handler = RenderSemanticsGestureHandler::new(callbacks);
/// ```
#[derive(Debug)]
pub struct RenderSemanticsGestureHandler {
    /// Gesture callbacks for accessibility
    pub callbacks: SemanticsGestureCallbacks,

    /// Scroll factor for drag gestures (default 0.8)
    ///
    /// Determines the proportion of the box's dimensions used for scroll
    /// gesture calculations.
    pub scroll_factor: f32,

    /// Cached size from last layout
    size: Size,
}

impl RenderSemanticsGestureHandler {
    /// Create new RenderSemanticsGestureHandler
    pub fn new(callbacks: SemanticsGestureCallbacks) -> Self {
        Self {
            callbacks,
            scroll_factor: 0.8,
            size: Size::ZERO,
        }
    }

    /// Create with custom scroll factor
    pub fn with_scroll_factor(callbacks: SemanticsGestureCallbacks, scroll_factor: f32) -> Self {
        Self {
            callbacks,
            scroll_factor: scroll_factor.clamp(0.0, 1.0),
            size: Size::ZERO,
        }
    }

    /// Get the callbacks
    pub fn callbacks(&self) -> &SemanticsGestureCallbacks {
        &self.callbacks
    }

    /// Set new callbacks
    pub fn set_callbacks(&mut self, callbacks: SemanticsGestureCallbacks) {
        self.callbacks = callbacks;
    }

    /// Get the scroll factor
    pub fn scroll_factor(&self) -> f32 {
        self.scroll_factor
    }

    /// Set the scroll factor
    pub fn set_scroll_factor(&mut self, factor: f32) {
        self.scroll_factor = factor.clamp(0.0, 1.0);
    }

    /// Handle semantic tap action
    pub fn handle_tap(&self) {
        if let Some(callback) = &self.callbacks.on_tap {
            callback();
        }
    }

    /// Handle semantic long press action
    pub fn handle_long_press(&self) {
        if let Some(callback) = &self.callbacks.on_long_press {
            callback();
        }
    }

    /// Handle semantic horizontal scroll action
    ///
    /// Direction: positive = scroll right, negative = scroll left
    pub fn handle_horizontal_scroll(&self, direction: f32) {
        if let Some(callback) = &self.callbacks.on_horizontal_drag_update {
            let delta = direction * self.size.width * self.scroll_factor;
            callback(delta);
        }
    }

    /// Handle semantic vertical scroll action
    ///
    /// Direction: positive = scroll down, negative = scroll up
    pub fn handle_vertical_scroll(&self, direction: f32) {
        if let Some(callback) = &self.callbacks.on_vertical_drag_update {
            let delta = direction * self.size.height * self.scroll_factor;
            callback(delta);
        }
    }
}

impl Default for RenderSemanticsGestureHandler {
    fn default() -> Self {
        Self::new(SemanticsGestureCallbacks::default())
    }
}

impl<T: FullRenderTree> RenderBox<T, Single> for RenderSemanticsGestureHandler {
    fn layout<T>(&mut self, mut ctx: LayoutContext<'_, T, Single, BoxProtocol>) -> Size
    where
        T: crate::core::LayoutTree,
    {
        let child_id = ctx.children.single();

        // Layout child with same constraints
        let size = ctx.layout_child(child_id, ctx.constraints);

        // Cache size for scroll calculations
        self.size = size;

        size
    }

    fn paint<T>(&self, ctx: &mut PaintContext<'_, T, Single>)
    where
        T: crate::core::PaintTree,
    {
        let child_id = ctx.children.single();

        // Paint child normally
        let _ = ctx.paint_child(child_id, ctx.offset);

        // Note: Semantics gesture handling requires integration with accessibility:
        // 1. Semantics tree registers actions based on which callbacks are set
        // 2. Accessibility framework invokes callbacks for semantic gestures
        // This render object provides the structure; gesture handling is done by the framework
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_semantics_gesture_handler_new() {
        let callbacks = SemanticsGestureCallbacks::new();
        let handler = RenderSemanticsGestureHandler::new(callbacks);

        assert!((handler.scroll_factor() - 0.8).abs() < f32::EPSILON);
        assert!(handler.callbacks().on_tap.is_none());
    }

    #[test]
    fn test_semantics_gesture_handler_with_scroll_factor() {
        let callbacks = SemanticsGestureCallbacks::new();
        let handler = RenderSemanticsGestureHandler::with_scroll_factor(callbacks, 0.5);

        assert!((handler.scroll_factor() - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn test_semantics_gesture_handler_scroll_factor_clamped() {
        let callbacks = SemanticsGestureCallbacks::new();
        let handler = RenderSemanticsGestureHandler::with_scroll_factor(callbacks, 1.5);

        // Should be clamped to 1.0
        assert!((handler.scroll_factor() - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_semantics_gesture_callbacks_builder() {
        use std::sync::atomic::{AtomicBool, Ordering};

        let tapped = Arc::new(AtomicBool::new(false));
        let tapped_clone = tapped.clone();

        let callbacks = SemanticsGestureCallbacks::new()
            .with_on_tap(move || tapped_clone.store(true, Ordering::SeqCst))
            .with_on_long_press(|| {});

        let handler = RenderSemanticsGestureHandler::new(callbacks);

        assert!(handler.callbacks().on_tap.is_some());
        assert!(handler.callbacks().on_long_press.is_some());
        assert!(handler.callbacks().on_horizontal_drag_update.is_none());

        // Test callback execution
        handler.handle_tap();
        assert!(tapped.load(Ordering::SeqCst));
    }

    #[test]
    fn test_semantics_gesture_callbacks_debug() {
        let callbacks = SemanticsGestureCallbacks::new()
            .with_on_tap(|| {})
            .with_on_vertical_drag_update(|_| {});

        let debug_str = format!("{:?}", callbacks);
        assert!(debug_str.contains("SemanticsGestureCallbacks"));
        assert!(debug_str.contains("on_tap"));
    }
}
