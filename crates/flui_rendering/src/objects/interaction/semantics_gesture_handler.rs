//! RenderSemanticsGestureHandler - Handles gestures from accessibility tools
//!
//! Implements Flutter's SemanticsGestureHandler that listens for gestures from
//! the semantics server (accessibility tools like screen readers). Provides
//! callbacks for accessibility-triggered taps, long presses, and scroll gestures.
//!
//! # Flutter Equivalence
//!
//! | FLUI | Flutter |
//! |------|---------|
//! | `RenderSemanticsGestureHandler` | `RenderSemanticsGestureHandler` from `package:flutter/src/rendering/proxy_box.dart` |
//! | `SemanticsGestureCallbacks` | Semantics action callbacks |
//! | `on_tap` | `onTap` semantic action |
//! | `on_long_press` | `onLongPress` semantic action |
//! | `on_horizontal_drag_update` | `onScrollLeft/Right` semantic actions |
//! | `on_vertical_drag_update` | `onScrollUp/Down` semantic actions |
//! | `scroll_factor` | `scrollFactor` property (default 0.8) |
//!
//! # Layout Protocol
//!
//! 1. **Pass constraints to child**
//!    - Child receives same constraints (proxy behavior)
//!
//! 2. **Cache size**
//!    - Store child size for scroll calculations
//!    - Scroll delta = direction × size × scroll_factor
//!
//! 3. **Return child size**
//!    - Container size = child size (no size change)
//!
//! # Paint Protocol
//!
//! 1. **Paint child normally**
//!    - Child painted at widget offset
//!    - No visual changes from semantics handling
//!
//! 2. **Register semantics actions** (framework integration)
//!    - Semantics tree registers actions based on which callbacks are set
//!    - Accessibility framework invokes callbacks for semantic gestures
//!
//! # Event Handling Protocol
//!
//! 1. **Semantic tap**
//!    - Triggered when accessibility tool performs tap action
//!    - Calls `on_tap` callback if provided
//!
//! 2. **Semantic long press**
//!    - Triggered when accessibility tool performs long press action
//!    - Calls `on_long_press` callback if provided
//!
//! 3. **Horizontal scroll**
//!    - Triggered when accessibility tool scrolls left/right
//!    - Calls `on_horizontal_drag_update` with delta
//!    - Delta = direction × width × scroll_factor
//!    - Positive = scroll right, negative = scroll left
//!
//! 4. **Vertical scroll**
//!    - Triggered when accessibility tool scrolls up/down
//!    - Calls `on_vertical_drag_update` with delta
//!    - Delta = direction × height × scroll_factor
//!    - Positive = scroll down, negative = scroll up
//!
//! # Performance
//!
//! - **Layout**: O(1) - pass-through to child + size cache
//! - **Paint**: O(1) - pass-through to child
//! - **Event handling**: O(1) - callback invocation per event
//! - **Memory**: ~56 bytes (4 Arc callbacks + scroll_factor + size)
//!
//! # Use Cases
//!
//! - **Accessible buttons**: Screen reader tap gestures
//! - **Accessible scrolling**: Screen reader scroll gestures
//! - **Long press actions**: Accessibility context menus
//! - **Custom semantic actions**: App-specific accessibility gestures
//! - **Focus navigation**: Accessibility-driven UI navigation
//! - **Voice control**: Voice-triggered gesture handling
//!
//! # Scroll Factor
//!
//! The `scroll_factor` property (default 0.8) determines the proportion of
//! the widget's dimensions used for scroll calculations:
//!
//! - 0.8 = 80% of widget size per scroll action
//! - For 200px width: leftward scroll = -160px drag
//! - For 300px height: downward scroll = 240px drag
//!
//! # Examples
//!
//! ```rust,ignore
//! use flui_rendering::{RenderSemanticsGestureHandler, SemanticsGestureCallbacks};
//!
//! // Accessible button with tap and long press
//! let callbacks = SemanticsGestureCallbacks::new()
//!     .with_on_tap(|| println!("Accessibility tap"))
//!     .with_on_long_press(|| println!("Accessibility long press"));
//! let button = RenderSemanticsGestureHandler::new(callbacks);
//!
//! // Scrollable list with accessibility gestures
//! let scroll_callbacks = SemanticsGestureCallbacks::new()
//!     .with_on_vertical_drag_update(|delta| {
//!         println!("Accessibility scroll: {}", delta);
//!     });
//! let list = RenderSemanticsGestureHandler::new(scroll_callbacks);
//!
//! // Custom scroll factor (50% of widget size)
//! let custom = RenderSemanticsGestureHandler::with_scroll_factor(callbacks, 0.5);
//! ```

use crate::core::{BoxLayoutCtx, BoxPaintCtx, RenderBox, Single};
use crate::{RenderObject, RenderResult};
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

/// RenderObject that handles gestures from accessibility tools.
///
/// Listens for gestures from the semantics server (e.g., screen readers) and
/// invokes callbacks for accessibility-triggered actions. Enables visually
/// impaired users to interact with the app through assistive technologies.
///
/// # Arity
///
/// `Single` - Must have exactly 1 child.
///
/// # Protocol
///
/// Box protocol - Uses `BoxConstraints` and returns `Size`.
///
/// # Pattern
///
/// **Proxy** - Passes constraints unchanged, only adds accessibility handling.
///
/// # Use Cases
///
/// - **Accessible buttons**: Screen reader tap and long press gestures
/// - **Accessible scrolling**: Screen reader scroll up/down/left/right
/// - **Long press menus**: Context menus via accessibility
/// - **Custom semantic actions**: App-specific accessibility gestures
/// - **Focus navigation**: Keyboard and screen reader navigation
/// - **Voice control**: Voice-triggered gesture handling
///
/// # Flutter Compliance
///
/// Matches Flutter's RenderSemanticsGestureHandler behavior:
/// - Passes constraints unchanged to child (proxy for layout)
/// - Size determined by child
/// - Registers semantic actions based on callbacks
/// - Scroll gestures use scroll_factor (default 0.8)
/// - Callbacks invoked by accessibility framework
/// - No visual changes (only affects semantics tree)
///
/// # Scroll Factor
///
/// The `scroll_factor` property (default 0.8) determines the proportion of
/// the widget's dimensions used for scroll calculations:
///
/// - 0.8 = 80% of widget size per scroll action
/// - For 200px width: leftward scroll = -160px drag
/// - For 300px height: downward scroll = 240px drag
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::{RenderSemanticsGestureHandler, SemanticsGestureCallbacks};
///
/// // Accessible button
/// let callbacks = SemanticsGestureCallbacks::new()
///     .with_on_tap(|| println!("Accessibility tap"))
///     .with_on_long_press(|| println!("Accessibility long press"));
/// let handler = RenderSemanticsGestureHandler::new(callbacks);
///
/// // Scrollable with custom scroll factor
/// let scroll = RenderSemanticsGestureHandler::with_scroll_factor(callbacks, 0.5);
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

impl RenderObject for RenderSemanticsGestureHandler {}

impl RenderBox<Single> for RenderSemanticsGestureHandler {
    fn layout(&mut self, mut ctx: BoxLayoutCtx<'_, Single>) -> RenderResult<Size> {
        // Single arity: use ctx.single_child() which returns ElementId directly
        let child_id = ctx.single_child();

        // Proxy behavior: pass constraints unchanged to child
        let size = ctx.layout_child(child_id, ctx.constraints)?;

        // Cache size for scroll delta calculations
        // Delta = direction × size × scroll_factor
        self.size = size;

        Ok(size)
    }

    fn paint(&self, ctx: &mut BoxPaintCtx<'_, Single>) {
        // Single arity: use ctx.single_child() which returns ElementId directly
        let child_id = ctx.single_child();

        // Proxy behavior: paint child at widget offset
        // Semantics handling doesn't affect visual rendering
        ctx.paint_child(child_id, ctx.offset);

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
