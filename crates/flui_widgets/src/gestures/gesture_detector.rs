//! GestureDetector widget for handling user interactions
//!
//! Based on Flutter's GestureDetector. Wraps a child widget and provides
//! callbacks for various pointer events.

use std::sync::Arc;

use flui_core::{BuildContext, StatelessWidget, Widget};
use flui_types::events::{PointerEvent, PointerEventData};

/// Callback for pointer events
pub type PointerEventCallback = Arc<dyn Fn(&PointerEventData) + Send + Sync>;

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
#[derive(Clone)]
pub struct GestureDetector {
    /// Child widget
    pub child: Box<dyn Widget>,

    /// Called when the user taps on the widget
    pub on_tap: Option<PointerEventCallback>,

    /// Called when the user presses down on the widget
    pub on_tap_down: Option<PointerEventCallback>,

    /// Called when the user releases the tap
    pub on_tap_up: Option<PointerEventCallback>,

    /// Called when the tap is cancelled
    pub on_tap_cancel: Option<PointerEventCallback>,
}

impl GestureDetector {
    /// Create a new GestureDetector with a child
    pub fn new(child: Box<dyn Widget>) -> Self {
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

    /// Handle a pointer event
    pub fn handle_event(&self, event: &PointerEvent) {
        match event {
            PointerEvent::Down(data) => {
                if let Some(on_tap_down) = &self.on_tap_down {
                    on_tap_down(data);
                }
            }
            PointerEvent::Up(data) => {
                // Trigger tap callback on pointer up
                if let Some(on_tap) = &self.on_tap {
                    on_tap(data);
                }
                if let Some(on_tap_up) = &self.on_tap_up {
                    on_tap_up(data);
                }
            }
            PointerEvent::Cancel(data) => {
                if let Some(on_tap_cancel) = &self.on_tap_cancel {
                    on_tap_cancel(data);
                }
            }
            _ => {}
        }
    }
}

impl StatelessWidget for GestureDetector {
    fn build(&self, _context: &BuildContext) -> Box<dyn Widget> {
        // For now, just return the child
        // In a full implementation, we would wrap this in a special Element
        // that participates in hit testing and event routing
        dyn_clone::clone_box(&*self.child)
    }
}

impl std::fmt::Debug for GestureDetector {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GestureDetector")
            .field("child", &"<widget>")
            .field("on_tap", &self.on_tap.is_some())
            .field("on_tap_down", &self.on_tap_down.is_some())
            .field("on_tap_up", &self.on_tap_up.is_some())
            .field("on_tap_cancel", &self.on_tap_cancel.is_some())
            .finish()
    }
}

/// Builder for GestureDetector
pub struct GestureDetectorBuilder {
    child: Option<Box<dyn Widget>>,
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
    pub fn child(mut self, child: impl Widget + 'static) -> Self {
        self.child = Some(Box::new(child));
        self
    }

    /// Set the onTap callback
    pub fn on_tap<F>(mut self, callback: F) -> Self
    where
        F: Fn(&PointerEventData) + Send + Sync + 'static,
    {
        self.on_tap = Some(Arc::new(callback));
        self
    }

    /// Set the onTapDown callback
    pub fn on_tap_down<F>(mut self, callback: F) -> Self
    where
        F: Fn(&PointerEventData) + Send + Sync + 'static,
    {
        self.on_tap_down = Some(Arc::new(callback));
        self
    }

    /// Set the onTapUp callback
    pub fn on_tap_up<F>(mut self, callback: F) -> Self
    where
        F: Fn(&PointerEventData) + Send + Sync + 'static,
    {
        self.on_tap_up = Some(Arc::new(callback));
        self
    }

    /// Set the onTapCancel callback
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
    use crate::basic::SizedBox;
    use flui_types::{Offset, events::PointerDeviceKind};
    use std::sync::atomic::{AtomicBool, Ordering};

    #[test]
    fn test_gesture_detector_builder() {
        let tapped = Arc::new(AtomicBool::new(false));
        let tapped_clone = Arc::clone(&tapped);

        let child = SizedBox::builder()
            .width(100.0)
            .height(100.0)
            .build();

        let detector = GestureDetector::builder()
            .child(child)
            .on_tap(move |_| {
                tapped_clone.store(true, Ordering::Relaxed);
            })
            .build();

        // Simulate a tap
        let event_data = PointerEventData::new(
            Offset::new(50.0, 50.0),
            PointerDeviceKind::Mouse,
        );
        detector.handle_event(&PointerEvent::Up(event_data));

        assert!(tapped.load(Ordering::Relaxed));
    }

    #[test]
    fn test_gesture_detector_tap_down() {
        let pressed = Arc::new(AtomicBool::new(false));
        let pressed_clone = Arc::clone(&pressed);

        let child = SizedBox::builder()
            .width(100.0)
            .height(100.0)
            .build();

        let detector = GestureDetector::builder()
            .child(child)
            .on_tap_down(move |_| {
                pressed_clone.store(true, Ordering::Relaxed);
            })
            .build();

        let event_data = PointerEventData::new(
            Offset::new(50.0, 50.0),
            PointerDeviceKind::Mouse,
        );
        detector.handle_event(&PointerEvent::Down(event_data));

        assert!(pressed.load(Ordering::Relaxed));
    }
}
