//! RenderPointerListener - Detects and handles pointer events
//!
//! Implements Flutter's Listener widget for detecting low-level pointer events
//! (mouse clicks, touches, pen input). Provides callbacks for down, up, move,
//! and cancel events without gesture recognition.
//!
//! # Flutter Equivalence
//!
//! | FLUI | Flutter |
//! |------|---------|
//! | `RenderPointerListener` | `RenderPointerListener` from `package:flutter/src/rendering/proxy_box.dart` |
//! | `PointerCallbacks` | Listener widget callbacks |
//! | `on_pointer_down` | `onPointerDown` (PointerDownEvent) |
//! | `on_pointer_up` | `onPointerUp` (PointerUpEvent) |
//! | `on_pointer_move` | `onPointerMove` (PointerMoveEvent) |
//! | `on_pointer_cancel` | `onPointerCancel` (PointerCancelEvent) |
//!
//! # Layout Protocol
//!
//! 1. **Pass constraints to child**
//!    - Child receives same constraints (proxy behavior)
//!
//! 2. **Cache size**
//!    - Store child size for hit region bounds calculation
//!
//! 3. **Return child size**
//!    - Container size = child size (no size change)
//!
//! # Paint Protocol
//!
//! 1. **Calculate hit region bounds**
//!    - Bounds = Rect from offset with cached size
//!    - Used for pointer event hit testing
//!
//! 2. **Register hit region**
//!    - Add hit region to canvas with unified event handler
//!    - System routes pointer events to this region
//!
//! 3. **Paint child**
//!    - Child painted at widget offset
//!    - No visual changes from pointer detection
//!
//! # Event Handling Protocol
//!
//! 1. **Pointer down**
//!    - Triggered when pointer pressed within bounds
//!    - Calls `on_pointer_down` callback with event data
//!    - Event contains position, device, button info
//!
//! 2. **Pointer move**
//!    - Triggered when pointer moves within bounds
//!    - Calls `on_pointer_move` callback with event data
//!    - Provides position delta and velocity
//!
//! 3. **Pointer up**
//!    - Triggered when pointer released
//!    - Calls `on_pointer_up` callback with event data
//!
//! 4. **Pointer cancel**
//!    - Triggered when pointer event cancelled (system interruption)
//!    - Calls `on_pointer_cancel` callback with event data
//!
//! # Performance
//!
//! - **Layout**: O(1) - pass-through to child + size cache
//! - **Paint**: O(1) - pass-through to child + hit region registration
//! - **Event handling**: O(1) - callback invocation per event
//! - **Memory**: ~64 bytes (4 Arc callbacks + cached size)
//!
//! # Use Cases
//!
//! - **Custom gestures**: Build custom gesture recognizers
//! - **Drawing apps**: Track pointer movement for drawing
//! - **Drag operations**: Implement drag-and-drop with pointer events
//! - **Games**: Low-level input handling for game controls
//! - **Signature capture**: Track precise pointer movement
//! - **Interactive canvas**: Direct pointer manipulation
//!
//! # Difference from GestureDetector
//!
//! **RenderPointerListener (this):**
//! - Low-level pointer events (down, up, move, cancel)
//! - No gesture recognition (no tap, double-tap, long-press)
//! - All pointer events captured
//! - Simple callback-based API
//!
//! **GestureDetector:**
//! - High-level gesture recognition
//! - Recognizes taps, drags, scales, long presses
//! - Complex gesture arena resolution
//! - More user-friendly for common interactions
//!
//! # Examples
//!
//! ```rust,ignore
//! use flui_rendering::{RenderPointerListener, PointerCallbacks};
//!
//! // Track pointer down events
//! let callbacks = PointerCallbacks::new()
//!     .with_on_pointer_down(|event| {
//!         println!("Pointer down at: {:?}", event.position());
//!     });
//! let listener = RenderPointerListener::new(callbacks);
//!
//! // Track all pointer events
//! let all_events = PointerCallbacks::new()
//!     .with_on_pointer_down(|e| println!("Down: {:?}", e.position()))
//!     .with_on_pointer_move(|e| println!("Move: {:?}", e.position()))
//!     .with_on_pointer_up(|e| println!("Up: {:?}", e.position()))
//!     .with_on_pointer_cancel(|e| println!("Cancel"));
//! let tracker = RenderPointerListener::new(all_events);
//! ```

use flui_interaction::{EventPropagation, HitTestEntry, HitTestResult};
use flui_rendering::{BoxHitTestCtx, BoxLayoutCtx, BoxPaintCtx, RenderBox, Single};
use flui_rendering::{RenderObject, RenderResult};
use flui_types::events::PointerEvent;
use flui_types::{Offset, Rect, Size};
use std::sync::Arc;

/// Handler type for pointer events
pub type PointerEventHandler = Arc<dyn Fn(&PointerEvent) + Send + Sync>;

/// Pointer event callbacks
///
/// These callbacks are called when pointer events occur within the widget's bounds.
#[derive(Clone)]
pub struct PointerCallbacks {
    /// Called when pointer is pressed down
    pub on_pointer_down: Option<PointerEventHandler>,

    /// Called when pointer is released
    pub on_pointer_up: Option<PointerEventHandler>,

    /// Called when pointer moves
    pub on_pointer_move: Option<PointerEventHandler>,

    /// Called when pointer is cancelled
    pub on_pointer_cancel: Option<PointerEventHandler>,
}

impl PointerCallbacks {
    /// Create new empty callbacks
    pub fn new() -> Self {
        Self {
            on_pointer_down: None,
            on_pointer_up: None,
            on_pointer_move: None,
            on_pointer_cancel: None,
        }
    }

    /// Set on_pointer_down callback
    pub fn with_on_pointer_down<F>(mut self, callback: F) -> Self
    where
        F: Fn(&PointerEvent) + Send + Sync + 'static,
    {
        self.on_pointer_down = Some(Arc::new(callback));
        self
    }

    /// Set on_pointer_up callback
    pub fn with_on_pointer_up<F>(mut self, callback: F) -> Self
    where
        F: Fn(&PointerEvent) + Send + Sync + 'static,
    {
        self.on_pointer_up = Some(Arc::new(callback));
        self
    }

    /// Set on_pointer_move callback
    pub fn with_on_pointer_move<F>(mut self, callback: F) -> Self
    where
        F: Fn(&PointerEvent) + Send + Sync + 'static,
    {
        self.on_pointer_move = Some(Arc::new(callback));
        self
    }

    /// Set on_pointer_cancel callback
    pub fn with_on_pointer_cancel<F>(mut self, callback: F) -> Self
    where
        F: Fn(&PointerEvent) + Send + Sync + 'static,
    {
        self.on_pointer_cancel = Some(Arc::new(callback));
        self
    }
}

impl Default for PointerCallbacks {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for PointerCallbacks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PointerCallbacks")
            .field("on_pointer_down", &self.on_pointer_down.is_some())
            .field("on_pointer_up", &self.on_pointer_up.is_some())
            .field("on_pointer_move", &self.on_pointer_move.is_some())
            .field("on_pointer_cancel", &self.on_pointer_cancel.is_some())
            .finish()
    }
}

impl RenderObject for RenderPointerListener {}

/// RenderObject that detects and handles low-level pointer events.
///
/// Listens for pointer events (mouse clicks, touches, pen input) and invokes
/// callbacks for each event type. Provides direct access to pointer events
/// without gesture recognition.
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
/// **Proxy** - Passes constraints unchanged, only adds pointer event handling.
///
/// # Use Cases
///
/// - **Custom gestures**: Build custom gesture recognizers from raw events
/// - **Drawing/painting**: Track continuous pointer movement
/// - **Drag-and-drop**: Implement custom drag operations
/// - **Game input**: Low-level control for games
/// - **Signature capture**: Precise pointer tracking
/// - **Direct manipulation**: Custom interactive behaviors
///
/// # Flutter Compliance
///
/// Matches Flutter's RenderPointerListener behavior:
/// - Passes constraints unchanged to child (proxy for layout)
/// - Size determined by child
/// - Captures all pointer events within bounds
/// - Provides callbacks for down, up, move, cancel
/// - Events include position, device, button info
/// - No gesture recognition (raw events only)
/// - Registers hit region for event routing
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::{RenderPointerListener, PointerCallbacks};
///
/// // Track pointer movement for drawing
/// let callbacks = PointerCallbacks::new()
///     .with_on_pointer_down(|event| {
///         println!("Start drawing at: {:?}", event.position());
///     })
///     .with_on_pointer_move(|event| {
///         println!("Draw line to: {:?}", event.position());
///     })
///     .with_on_pointer_up(|event| {
///         println!("Finish drawing at: {:?}", event.position());
///     });
///
/// let listener = RenderPointerListener::new(callbacks);
/// ```
#[derive(Debug)]
pub struct RenderPointerListener {
    /// Event callbacks
    pub callbacks: PointerCallbacks,

    /// Cached size from last layout
    size: Size,
}

impl RenderPointerListener {
    /// Create new RenderPointerListener
    pub fn new(callbacks: PointerCallbacks) -> Self {
        Self {
            callbacks,
            size: Size::ZERO,
        }
    }

    /// Get the callbacks
    pub fn callbacks(&self) -> &PointerCallbacks {
        &self.callbacks
    }

    /// Set new callbacks
    pub fn set_callbacks(&mut self, callbacks: PointerCallbacks) {
        self.callbacks = callbacks;
        // No need to mark needs_layout or needs_paint - callbacks don't affect rendering
    }

    /// Create the unified event handler from individual callbacks
    #[allow(dead_code)]
    fn create_handler(&self) -> PointerEventHandler {
        let callbacks = self.callbacks.clone();
        Arc::new(move |event: &PointerEvent| match event {
            PointerEvent::Down(_) => {
                if let Some(callback) = &callbacks.on_pointer_down {
                    callback(event);
                }
            }
            PointerEvent::Up(_) => {
                if let Some(callback) = &callbacks.on_pointer_up {
                    callback(event);
                }
            }
            PointerEvent::Move(_) => {
                if let Some(callback) = &callbacks.on_pointer_move {
                    callback(event);
                }
            }
            PointerEvent::Cancel(_) => {
                if let Some(callback) = &callbacks.on_pointer_cancel {
                    callback(event);
                }
            }
            _ => {}
        })
    }
}

impl RenderBox<Single> for RenderPointerListener {
    fn layout(&mut self, mut ctx: BoxLayoutCtx<'_, Single>) -> RenderResult<Size> {
        // Single arity: use ctx.single_child() which returns ElementId directly
        let child_id = ctx.single_child();

        // Proxy behavior: pass constraints unchanged to child
        let size = ctx.layout_child(child_id, ctx.constraints, true)?;

        // Cache size for hit region bounds calculation in paint
        self.size = size;

        Ok(size)
    }

    fn paint(&self, ctx: &mut BoxPaintCtx<'_, Single>) {
        // Single arity: use ctx.single_child() which returns ElementId directly
        let child_id = ctx.single_child();
        let offset = ctx.offset;

        // Register hit region for pointer event handling
        // This connects the GestureDetector callbacks to the hit test system
        let bounds =
            flui_types::Rect::from_xywh(offset.dx, offset.dy, self.size.width, self.size.height);

        // Create unified handler from our callbacks
        let callbacks = self.callbacks.clone();
        let handler: flui_painting::HitRegionHandler =
            std::sync::Arc::new(move |event| match event {
                flui_types::events::PointerEvent::Down(_) => {
                    if let Some(callback) = &callbacks.on_pointer_down {
                        callback(event);
                    }
                }
                flui_types::events::PointerEvent::Up(_) => {
                    if let Some(callback) = &callbacks.on_pointer_up {
                        callback(event);
                    }
                }
                flui_types::events::PointerEvent::Move(_) => {
                    if let Some(callback) = &callbacks.on_pointer_move {
                        callback(event);
                    }
                }
                flui_types::events::PointerEvent::Cancel(_) => {
                    if let Some(callback) = &callbacks.on_pointer_cancel {
                        callback(event);
                    }
                }
                _ => {}
            });

        // Add hit region to canvas
        ctx.canvas_mut()
            .add_hit_region(flui_painting::HitRegion::new(bounds, handler));

        tracing::trace!(
            bounds = ?bounds,
            has_down = self.callbacks.on_pointer_down.is_some(),
            has_up = self.callbacks.on_pointer_up.is_some(),
            "RenderPointerListener: registered hit region"
        );

        // Paint child
        ctx.paint_child(child_id, offset);
    }

    fn hit_test(&self, ctx: &BoxHitTestCtx<'_, Single>, result: &mut HitTestResult) -> bool {
        // Check if position is within bounds
        let bounds = Rect::from_min_size(Offset::ZERO, ctx.size());
        if !bounds.contains(ctx.position) {
            return false;
        }

        // Create unified pointer event handler from callbacks
        let callbacks = self.callbacks.clone();
        let handler = Arc::new(move |event: &PointerEvent| -> EventPropagation {
            match event {
                PointerEvent::Down(_) => {
                    if let Some(callback) = &callbacks.on_pointer_down {
                        callback(event);
                    }
                }
                PointerEvent::Up(_) => {
                    if let Some(callback) = &callbacks.on_pointer_up {
                        callback(event);
                    }
                }
                PointerEvent::Move(_) => {
                    if let Some(callback) = &callbacks.on_pointer_move {
                        callback(event);
                    }
                }
                PointerEvent::Cancel(_) => {
                    if let Some(callback) = &callbacks.on_pointer_cancel {
                        callback(event);
                    }
                }
                _ => {}
            }
            // Continue propagation to allow other handlers to process
            EventPropagation::Continue
        });

        // Add hit test entry with handler
        let entry = HitTestEntry::with_handler(ctx.element_id(), ctx.position, bounds, handler);
        result.add(entry);

        // Also test children
        ctx.hit_test_children(result);

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_pointer_listener_new() {
        let callbacks = PointerCallbacks::new();
        let listener = RenderPointerListener::new(callbacks);
        assert!(listener.callbacks().on_pointer_down.is_none());
        assert!(listener.callbacks().on_pointer_up.is_none());
        assert!(listener.callbacks().on_pointer_move.is_none());
        assert!(listener.callbacks().on_pointer_cancel.is_none());
    }

    #[test]
    fn test_render_pointer_listener_with_callbacks() {
        let callbacks = PointerCallbacks::new()
            .with_on_pointer_down(|_| {})
            .with_on_pointer_up(|_| {});

        let listener = RenderPointerListener::new(callbacks);
        assert!(listener.callbacks().on_pointer_down.is_some());
        assert!(listener.callbacks().on_pointer_up.is_some());
        assert!(listener.callbacks().on_pointer_move.is_none());
    }

    #[test]
    fn test_render_pointer_listener_set_callbacks() {
        let callbacks1 = PointerCallbacks::new();
        let mut listener = RenderPointerListener::new(callbacks1);

        let callbacks2 = PointerCallbacks::new().with_on_pointer_down(|_| {});
        listener.set_callbacks(callbacks2);
        assert!(listener.callbacks().on_pointer_down.is_some());
    }

    #[test]
    fn test_pointer_callbacks_debug() {
        let callbacks = PointerCallbacks::new()
            .with_on_pointer_down(|_| {})
            .with_on_pointer_move(|_| {});

        let debug_str = format!("{:?}", callbacks);
        assert!(debug_str.contains("PointerCallbacks"));
        assert!(debug_str.contains("on_pointer_down"));
    }

    #[test]
    fn test_pointer_callbacks_builder() {
        let callbacks = PointerCallbacks::new()
            .with_on_pointer_down(|_| {})
            .with_on_pointer_up(|_| {})
            .with_on_pointer_move(|_| {})
            .with_on_pointer_cancel(|_| {});

        assert!(callbacks.on_pointer_down.is_some());
        assert!(callbacks.on_pointer_up.is_some());
        assert!(callbacks.on_pointer_move.is_some());
        assert!(callbacks.on_pointer_cancel.is_some());
    }
}
