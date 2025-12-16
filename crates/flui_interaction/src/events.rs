//! Event types for user interactions.
//!
//! This module provides standardized event types following W3C specifications:
//!
//! - **Pointer events** - Mouse, touch, pen input via [`ui_events`]
//! - **Keyboard events** - Key presses and releases via [`ui_events`]
//! - **Cursor icons** - Standard cursor appearances via [`cursor_icon`]
//!
//! # Pointer Events
//!
//! ```rust,ignore
//! use flui_interaction::events::{PointerEvent, PointerButton, PointerType};
//!
//! fn handle_event(event: &PointerEvent) {
//!     match event {
//!         PointerEvent::Down(button_event) => {
//!             println!("Button pressed: {:?}", button_event.button);
//!         }
//!         PointerEvent::Move(update) => {
//!             println!("Pointer moved to: {:?}", update.current.position);
//!         }
//!         _ => {}
//!     }
//! }
//! ```
//!
//! # Keyboard Events
//!
//! ```rust,ignore
//! use flui_interaction::events::{KeyboardEvent, Key, KeyState};
//!
//! fn handle_key(event: &KeyboardEvent) {
//!     if event.state == KeyState::Pressed {
//!         println!("Key pressed: {:?}", event.key);
//!     }
//! }
//! ```
//!
//! # Cursor Icons
//!
//! ```rust,ignore
//! use flui_interaction::events::CursorIcon;
//!
//! let cursor = CursorIcon::Pointer; // Hand cursor for clickable elements
//! let text_cursor = CursorIcon::Text; // I-beam for text selection
//! ```

// ============================================================================
// Re-exports from ui-events (W3C UI Events specification)
// ============================================================================

/// Pointer event types.
pub mod pointer {
    pub use ui_events::pointer::{
        ContactGeometry, PersistentDeviceId, PointerButton, PointerButtonEvent, PointerButtons,
        PointerEvent, PointerGesture, PointerGestureEvent, PointerId, PointerInfo,
        PointerOrientation, PointerScrollEvent, PointerState, PointerType, PointerUpdate,
    };
}

/// Keyboard event types.
pub mod keyboard {
    pub use ui_events::keyboard::{
        Code, CompositionEvent, CompositionState, Key, KeyState, KeyboardEvent, Location,
        Modifiers, NamedKey, ShortcutMatcher,
    };
}

/// Scroll delta types.
pub use ui_events::ScrollDelta;

// ============================================================================
// Re-exports from cursor-icon (W3C CSS specification)
// ============================================================================

/// Cursor icon following W3C CSS cursor specification.
///
/// Standard cursor appearances for different interaction states.
///
/// # Common Cursors
///
/// - [`CursorIcon::Default`] - Standard arrow cursor
/// - [`CursorIcon::Pointer`] - Hand cursor for clickable elements
/// - [`CursorIcon::Text`] - I-beam for text selection
/// - [`CursorIcon::Wait`] - Busy/loading cursor
/// - [`CursorIcon::Grab`] / [`CursorIcon::Grabbing`] - Drag cursors
/// - [`CursorIcon::NotAllowed`] - Forbidden action
///
/// # Resize Cursors
///
/// - [`CursorIcon::EwResize`] - Horizontal resize
/// - [`CursorIcon::NsResize`] - Vertical resize
/// - [`CursorIcon::NwseResize`] / [`CursorIcon::NeswResize`] - Diagonal resize
pub use cursor_icon::CursorIcon;

// ============================================================================
// Convenience re-exports at module level
// ============================================================================

// Most commonly used pointer types
pub use pointer::{
    PointerButton, PointerButtonEvent, PointerEvent, PointerId, PointerScrollEvent, PointerState,
    PointerType, PointerUpdate,
};

// Most commonly used keyboard types
pub use keyboard::{Code, Key, KeyState, KeyboardEvent, Modifiers, NamedKey};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cursor_icon() {
        let cursor = CursorIcon::Default;
        assert_eq!(cursor, CursorIcon::Default);

        let pointer = CursorIcon::Pointer;
        assert_ne!(cursor, pointer);
    }

    #[test]
    fn test_pointer_type() {
        let mouse = PointerType::Mouse;
        let touch = PointerType::Touch;
        assert_ne!(mouse, touch);
    }

    #[test]
    fn test_key_state() {
        assert_ne!(KeyState::Pressed, KeyState::Released);
    }

    #[test]
    fn test_pointer_button() {
        let primary = PointerButton::Primary;
        let secondary = PointerButton::Secondary;
        assert_ne!(primary, secondary);
    }
}
