//! GestureDetector widget for handling user interactions
//!
//! Based on Flutter's GestureDetector. Wraps a child widget and provides
//! callbacks for various pointer events.

use flui_core::element::Element;
use flui_core::view::{BuildContext, IntoElement, StatelessView};
use flui_rendering::objects::PointerCallbacks;
use flui_types::events::{PointerEvent, PointerEventData};
use std::sync::Arc;

/// Callback for tap events (no event data)
pub type TapCallback = Arc<dyn Fn() + Send + Sync>;

/// Callback for pointer events with data
pub type PointerCallback = Arc<dyn Fn(&PointerEventData) + Send + Sync>;

/// GestureDetector widget
///
/// Wraps a child widget and provides callbacks for user interactions.
///
/// # Example
///
/// ```rust,ignore
/// use flui_interaction::GestureDetector;
///
/// GestureDetector::builder()
///     .on_tap(|| println!("Tapped!"))
///     .child(Text::new("Click me"))
///     .build()
/// ```
pub struct GestureDetector {
    /// Child widget
    pub child: Element,

    /// On tap callback (pointer up)
    pub on_tap: Option<TapCallback>,

    /// On tap down callback
    pub on_tap_down: Option<PointerCallback>,

    /// On tap up callback
    pub on_tap_up: Option<PointerCallback>,

    /// On tap cancel callback
    pub on_tap_cancel: Option<PointerCallback>,
}

impl GestureDetector {
    /// Create a new GestureDetector with a child
    pub fn new(child: Element) -> Self {
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
}

impl StatelessView for GestureDetector {
    fn build(self, _ctx: &dyn BuildContext) -> impl IntoElement {
        // Create PointerCallbacks from our simpler callbacks
        let mut callbacks = PointerCallbacks::new();

        // Track pointer down state for tap detection
        let pointer_down = Arc::new(std::sync::atomic::AtomicBool::new(false));

        // on_tap_down callback
        if let Some(on_tap_down) = self.on_tap_down.clone() {
            let pointer_down_clone = pointer_down.clone();
            callbacks = callbacks.with_on_pointer_down(move |event: &PointerEvent| {
                pointer_down_clone.store(true, std::sync::atomic::Ordering::SeqCst);
                if let PointerEvent::Down(data) = event {
                    on_tap_down(data);
                }
            });
        } else {
            // Still need to track pointer down for tap
            let pointer_down_clone = pointer_down.clone();
            callbacks = callbacks.with_on_pointer_down(move |_event: &PointerEvent| {
                pointer_down_clone.store(true, std::sync::atomic::Ordering::SeqCst);
            });
        }

        // on_tap and on_tap_up callback
        let (on_tap, on_tap_up) = (self.on_tap.clone(), self.on_tap_up.clone());
        if on_tap.is_some() || on_tap_up.is_some() {
            let pointer_down_clone = pointer_down.clone();
            callbacks = callbacks.with_on_pointer_up(move |event: &PointerEvent| {
                if pointer_down_clone.load(std::sync::atomic::Ordering::SeqCst) {
                    // Only trigger tap if pointer was down first
                    if let Some(ref callback) = on_tap {
                        callback();
                    }
                    if let (Some(ref callback), PointerEvent::Up(data)) = (&on_tap_up, event) {
                        callback(data);
                    }
                    pointer_down_clone.store(false, std::sync::atomic::Ordering::SeqCst);
                }
            });
        } else if let Some(on_tap) = self.on_tap.clone() {
            let pointer_down_clone = pointer_down.clone();
            callbacks = callbacks.with_on_pointer_up(move |_event: &PointerEvent| {
                if pointer_down_clone.load(std::sync::atomic::Ordering::SeqCst) {
                    on_tap();
                    pointer_down_clone.store(false, std::sync::atomic::Ordering::SeqCst);
                }
            });
        } else if let Some(on_tap_up) = self.on_tap_up.clone() {
            let pointer_down_clone = pointer_down.clone();
            callbacks = callbacks.with_on_pointer_up(move |event: &PointerEvent| {
                if pointer_down_clone.load(std::sync::atomic::Ordering::SeqCst) {
                    if let PointerEvent::Up(data) = event {
                        on_tap_up(data);
                    }
                    pointer_down_clone.store(false, std::sync::atomic::Ordering::SeqCst);
                }
            });
        }

        // on_tap_cancel callback
        if let Some(on_tap_cancel) = self.on_tap_cancel.clone() {
            let pointer_down_clone = pointer_down.clone();
            callbacks = callbacks.with_on_pointer_cancel(move |event: &PointerEvent| {
                if let PointerEvent::Cancel(data) = event {
                    on_tap_cancel(data);
                }
                pointer_down_clone.store(false, std::sync::atomic::Ordering::SeqCst);
            });
        }

        // Return RenderPointerListener with child
        // The RenderPointerListener will create PointerListenerLayer
        // which registers hit test handlers with EventRouter
        use flui_core::render::RenderBoxExt;
        flui_rendering::objects::RenderPointerListener::new(callbacks).child(self.child)
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
    child: Option<Element>,
    on_tap: Option<TapCallback>,
    on_tap_down: Option<PointerCallback>,
    on_tap_up: Option<PointerCallback>,
    on_tap_cancel: Option<PointerCallback>,
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
    pub fn child(mut self, child: impl IntoElement) -> Self {
        self.child = Some(child.into_element());
        self
    }

    /// Set the on_tap callback
    pub fn on_tap<F>(mut self, callback: F) -> Self
    where
        F: Fn() + Send + Sync + 'static,
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
    use flui_core::render::RenderBoxExt;
    use flui_rendering::RenderEmpty;

    // Mock widget for testing
    #[derive(Debug, Clone)]
    struct MockWidget;

    impl StatelessView for MockWidget {
        fn build(self, _ctx: &dyn BuildContext) -> impl IntoElement {
            RenderEmpty.leaf()
        }
    }

    #[test]
    fn test_gesture_detector_builder() {
        let detector = GestureDetector::builder()
            .child(MockWidget)
            .on_tap(|| {})
            .build();

        assert!(detector.on_tap.is_some());
    }

    #[test]
    fn test_gesture_detector_new() {
        let detector = GestureDetector::new(MockWidget.into_element());

        assert!(detector.on_tap.is_none());
    }

    #[test]
    fn test_gesture_detector_with_all_callbacks() {
        let detector = GestureDetector::builder()
            .child(MockWidget)
            .on_tap(|| {})
            .on_tap_down(|_| {})
            .on_tap_up(|_| {})
            .on_tap_cancel(|_| {})
            .build();

        assert!(detector.on_tap.is_some());
        assert!(detector.on_tap_down.is_some());
        assert!(detector.on_tap_up.is_some());
        assert!(detector.on_tap_cancel.is_some());
    }
}
