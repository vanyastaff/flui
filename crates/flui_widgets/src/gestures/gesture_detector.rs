//! GestureDetector widget for handling user interactions
//!
//! Based on Flutter's GestureDetector. Wraps a child widget and provides
//! callbacks for various pointer events.
//!
//! # Implementation Note
//!
//! Currently uses StatelessWidget approach with global registry for event dispatch.
//! This is a temporary solution until SingleChildRenderObjectElement is implemented.
//!
//! Future: Use RenderPointerListener with proper Element infrastructure.

use std::sync::Arc;

use flui_core::widget::{Widget, StatelessWidget};
use flui_core::BuildContext;
use flui_types::events::{PointerEvent, PointerEventData};
use parking_lot::RwLock;

/// Callback for pointer events
pub type PointerEventCallback = Arc<dyn Fn(&PointerEventData) + Send + Sync>;

/// Global registry of GestureDetectors for event dispatch
///
/// This is a simplified approach that works without SingleChildRenderObjectElement.
/// The registry is populated during widget build and cleared/rebuilt as needed.
static GESTURE_HANDLERS: once_cell::sync::Lazy<RwLock<Vec<Arc<GestureHandler>>>> =
    once_cell::sync::Lazy::new(|| RwLock::new(Vec::new()));

/// Handler for gesture events
#[derive(Clone)]
struct GestureHandler {
    on_tap: Option<PointerEventCallback>,
    on_tap_down: Option<PointerEventCallback>,
    on_tap_up: Option<PointerEventCallback>,
    on_tap_cancel: Option<PointerEventCallback>,
}

impl GestureHandler {
    fn handle_event(&self, event: &PointerEvent) {
        match event {
            PointerEvent::Down(data) => {
                if let Some(callback) = &self.on_tap_down {
                    callback(data);
                }
            }
            PointerEvent::Up(data) => {
                if let Some(callback) = &self.on_tap {
                    callback(data);
                }
                if let Some(callback) = &self.on_tap_up {
                    callback(data);
                }
            }
            PointerEvent::Cancel(data) => {
                if let Some(callback) = &self.on_tap_cancel {
                    callback(data);
                }
            }
            _ => {}
        }
    }
}

/// Dispatch event to all registered gesture handlers
pub fn dispatch_gesture_event(event: &PointerEvent) {
    let handlers = GESTURE_HANDLERS.read();
    for handler in handlers.iter() {
        handler.handle_event(event);
    }
}

/// Clear all registered handlers (called before rebuild)
pub fn clear_gesture_handlers() {
    GESTURE_HANDLERS.write().clear();
}

/// GestureDetector widget
///
/// Wraps a child widget and provides callbacks for user interactions.
///
/// # Example
///
/// ```rust,ignore
/// GestureDetector::builder()
///     .on_tap(|_| println!("Tapped!"))
///     .child(Text::new("Click me"))
///     .build()
/// ```
///
/// # Implementation
///
/// Currently uses StatelessWidget that registers event handlers globally.
/// This allows proper rendering while we implement SingleChildRenderObjectElement.
#[derive(Clone)]
pub struct GestureDetector {
    /// Child widget
    pub child: Widget,

    /// On tap callback (pointer up)
    pub on_tap: Option<PointerEventCallback>,

    /// On tap down callback
    pub on_tap_down: Option<PointerEventCallback>,

    /// On tap up callback
    pub on_tap_up: Option<PointerEventCallback>,

    /// On tap cancel callback
    pub on_tap_cancel: Option<PointerEventCallback>,
}

impl GestureDetector {
    /// Create a new GestureDetector with a child
    pub fn new(child: Widget) -> Self {
        Self {
            child,
            on_tap: None,
            on_tap_down: None,
            on_tap_up: None,
            on_tap_cancel: None,
        }
    }

    /// Builder for GestureDetector
    pub fn builder() -> GestureDetectorBuilder {
        GestureDetectorBuilder::new()
    }

    /// Register this detector's handlers
    fn register(&self) {
        let handler = Arc::new(GestureHandler {
            on_tap: self.on_tap.clone(),
            on_tap_down: self.on_tap_down.clone(),
            on_tap_up: self.on_tap_up.clone(),
            on_tap_cancel: self.on_tap_cancel.clone(),
        });

        GESTURE_HANDLERS.write().push(handler);
    }
}

impl StatelessWidget for GestureDetector {
    fn build(&self, _context: &BuildContext) -> Widget {
        // Register handlers when building
        self.register();

        // Return child directly - this ensures proper rendering
        self.child.clone()
    }
}

impl std::fmt::Debug for GestureDetector {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GestureDetector")
            .field("child", &"<Widget>")
            .field("has_on_tap", &self.on_tap.is_some())
            .field("has_on_tap_down", &self.on_tap_down.is_some())
            .field("has_on_tap_up", &self.on_tap_up.is_some())
            .field("has_on_tap_cancel", &self.on_tap_cancel.is_some())
            .finish()
    }
}

/// Builder for GestureDetector
pub struct GestureDetectorBuilder {
    child: Option<Widget>,
    on_tap: Option<PointerEventCallback>,
    on_tap_down: Option<PointerEventCallback>,
    on_tap_up: Option<PointerEventCallback>,
    on_tap_cancel: Option<PointerEventCallback>,
}

impl GestureDetectorBuilder {
    /// Create a new builder
    pub fn new() -> Self {
        Self {
            child: None,
            on_tap: None,
            on_tap_down: None,
            on_tap_up: None,
            on_tap_cancel: None,
        }
    }

    /// Set the child widget
    pub fn child(mut self, child: Widget) -> Self {
        self.child = Some(child);
        self
    }

    /// Set the on_tap callback
    pub fn on_tap<F>(mut self, callback: F) -> Self
    where
        F: Fn(&PointerEventData) + Send + Sync + 'static,
    {
        self.on_tap = Some(Arc::new(callback));
        self
    }

    /// Set the on_tap_down callback
    pub fn on_tap_down<F>(mut self, callback: F) -> Self
    where
        F: Fn(&PointerEventData) + Send + Sync + 'static,
    {
        self.on_tap_down = Some(Arc::new(callback));
        self
    }

    /// Set the on_tap_up callback
    pub fn on_tap_up<F>(mut self, callback: F) -> Self
    where
        F: Fn(&PointerEventData) + Send + Sync + 'static,
    {
        self.on_tap_up = Some(Arc::new(callback));
        self
    }

    /// Set the on_tap_cancel callback
    pub fn on_tap_cancel<F>(mut self, callback: F) -> Self
    where
        F: Fn(&PointerEventData) + Send + Sync + 'static,
    {
        self.on_tap_cancel = Some(Arc::new(callback));
        self
    }

    /// Build the GestureDetector
    pub fn build(self) -> GestureDetector {
        GestureDetector {
            child: self.child.expect("GestureDetector requires a child"),
            on_tap: self.on_tap,
            on_tap_down: self.on_tap_down,
            on_tap_up: self.on_tap_up,
            on_tap_cancel: self.on_tap_cancel,
        }
    }
}

impl Default for GestureDetectorBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SizedBox;

    #[test]
    fn test_gesture_detector_builder() {
        let detector = GestureDetector::builder()
            .child(SizedBox::builder().width(100.0).height(100.0).build())
            .on_tap(|_| {})
            .build();

        assert!(detector.on_tap.is_some());
    }

    #[test]
    fn test_gesture_detector_new() {
        let child = Box::new(SizedBox::builder().width(100.0).height(100.0).build());
        let detector = GestureDetector::new(child);

        assert!(detector.on_tap.is_none());
    }
}

// Implement IntoWidget for ergonomic API
flui_core::impl_into_widget!(GestureDetector, stateless);
