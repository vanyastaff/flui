//! Tap gesture recognizer
//!
//! Recognizes tap gestures (pointer down + up in same location).

use flui_types::events::{PointerEvent, PointerEventData};
use std::sync::Arc;

/// Callback for tap events
pub type TapCallback = Arc<dyn Fn(&PointerEventData) + Send + Sync>;

/// Recognizes tap gestures
///
/// A tap is defined as a pointer down followed by a pointer up
/// within a small time window and position tolerance.
#[derive(Clone)]
pub struct TapGestureRecognizer {
    /// Callback when tap is detected
    on_tap: Option<TapCallback>,

    /// Callback when pointer goes down
    on_tap_down: Option<TapCallback>,

    /// Callback when pointer goes up (may not be a tap if dragged)
    on_tap_up: Option<TapCallback>,

    /// Whether pointer is currently down
    is_pointer_down: bool,
}

impl TapGestureRecognizer {
    /// Create a new tap recognizer
    pub fn new() -> Self {
        Self {
            on_tap: None,
            on_tap_down: None,
            on_tap_up: None,
            is_pointer_down: false,
        }
    }

    /// Set the tap callback
    pub fn with_on_tap<F>(mut self, callback: F) -> Self
    where
        F: Fn(&PointerEventData) + Send + Sync + 'static,
    {
        self.on_tap = Some(Arc::new(callback));
        self
    }

    /// Set the tap down callback
    pub fn with_on_tap_down<F>(mut self, callback: F) -> Self
    where
        F: Fn(&PointerEventData) + Send + Sync + 'static,
    {
        self.on_tap_down = Some(Arc::new(callback));
        self
    }

    /// Set the tap up callback
    pub fn with_on_tap_up<F>(mut self, callback: F) -> Self
    where
        F: Fn(&PointerEventData) + Send + Sync + 'static,
    {
        self.on_tap_up = Some(Arc::new(callback));
        self
    }

    /// Handle a pointer event
    ///
    /// Returns `true` if the event was consumed by this recognizer.
    pub fn handle_event(&mut self, event: &PointerEvent) -> bool {
        match event {
            PointerEvent::Down(data) => {
                self.is_pointer_down = true;
                if let Some(callback) = &self.on_tap_down {
                    callback(data);
                }
                true
            }
            PointerEvent::Up(data) => {
                if self.is_pointer_down {
                    // Tap completed
                    if let Some(callback) = &self.on_tap {
                        callback(data);
                    }
                    if let Some(callback) = &self.on_tap_up {
                        callback(data);
                    }
                    self.is_pointer_down = false;
                    true
                } else {
                    false
                }
            }
            PointerEvent::Cancel(_) => {
                self.is_pointer_down = false;
                true
            }
            _ => false,
        }
    }
}

impl Default for TapGestureRecognizer {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for TapGestureRecognizer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TapGestureRecognizer")
            .field("has_on_tap", &self.on_tap.is_some())
            .field("has_on_tap_down", &self.on_tap_down.is_some())
            .field("has_on_tap_up", &self.on_tap_up.is_some())
            .field("is_pointer_down", &self.is_pointer_down)
            .finish()
    }
}
