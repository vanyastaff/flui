//! Event types for user interactions
//!
//! This module provides event types for all user interactions:
//! - **Pointer events**: Mouse, touch, stylus input
//! - **Keyboard events**: Key presses and releases
//! - **Window events**: Resize, focus, theme changes
//! - **Scroll events**: Mouse wheel, touchpad scroll
//!
//! Based on Flutter's event system architecture.
//!
//! # Note on Hit Testing
//!
//! Hit testing types (`HitTestEntry`, `HitTestResult`) are in `flui_interaction`
//! crate as they require additional infrastructure (transform stacks, event
//! propagation control) that belongs in the interaction layer.

pub mod keyboard;
pub mod mouse_cursor;
pub mod pointer;
pub mod window;

// Re-export all public types
pub use keyboard::{KeyEvent, KeyEventData, KeyModifiers, LogicalKey, PhysicalKey};
pub use mouse_cursor::{MouseCursor, SystemMouseCursor, SystemMouseCursors};
pub use pointer::{PointerButton, PointerEvent, PointerEventData};
pub use window::{ScrollDelta, ScrollEventData, Theme, WindowEvent};

// Re-export PointerDeviceKind from gestures for convenience
pub use crate::gestures::PointerDeviceKind;

/// Unified event type
///
/// Represents any user input event that can occur in the application.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum Event {
    /// Pointer event (mouse, touch, etc.)
    Pointer(PointerEvent),

    /// Keyboard event
    Key(KeyEvent),

    /// Scroll event (mouse wheel, touchpad)
    Scroll(ScrollEventData),

    /// Window event
    Window(WindowEvent),
}

impl Event {
    /// Create a pointer event
    pub fn pointer(event: PointerEvent) -> Self {
        Event::Pointer(event)
    }

    /// Create a keyboard event
    pub fn key(event: KeyEvent) -> Self {
        Event::Key(event)
    }

    /// Create a scroll event
    pub fn scroll(data: ScrollEventData) -> Self {
        Event::Scroll(data)
    }

    /// Create a window event
    pub fn window(event: WindowEvent) -> Self {
        Event::Window(event)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gestures::PointerDeviceKind;
    use crate::Offset;

    #[test]
    fn test_unified_event() {
        let pointer_data = PointerEventData::new(Offset::new(10.0, 20.0), PointerDeviceKind::Mouse);
        let event = Event::pointer(PointerEvent::Down(pointer_data));

        assert!(matches!(event, Event::Pointer(_)));
    }
}
