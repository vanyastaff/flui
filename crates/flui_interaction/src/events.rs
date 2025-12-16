//! Event types for user interactions.
//!
//! This module provides standardized event types following W3C specifications:
//!
//! - **Pointer events** - Mouse, touch, pen input via [`ui_events`]
//! - **Keyboard events** - Key presses and releases via [`ui_events`]
//! - **Cursor icons** - Standard cursor appearances via [`cursor_icon`]
//!
//! # Input Events
//!
//! The [`InputEvent`] enum wraps W3C-compliant events and adds device lifecycle:
//!
//! ```rust,ignore
//! use flui_interaction::events::{InputEvent, PointerEvent};
//!
//! fn handle_event(event: &InputEvent) {
//!     match event {
//!         InputEvent::Pointer(PointerEvent::Down(button_event)) => {
//!             println!("Button pressed: {:?}", button_event.button);
//!         }
//!         InputEvent::DeviceAdded { device_id, pointer_type } => {
//!             println!("Device connected: {:?}", device_id);
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
//!     if event.state == KeyState::Down {
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

use flui_types::geometry::Offset;

// ============================================================================
// Re-exports from ui-events (W3C UI Events specification)
// ============================================================================

/// Pointer event types from ui-events.
pub mod pointer {
    pub use ui_events::pointer::{
        ContactGeometry, PersistentDeviceId, PointerButton, PointerButtonEvent, PointerButtons,
        PointerEvent, PointerGesture, PointerGestureEvent, PointerId, PointerInfo,
        PointerOrientation, PointerScrollEvent, PointerState, PointerType, PointerUpdate,
    };
}

/// Keyboard event types from ui-events.
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

// Pointer types from ui-events
pub use pointer::{
    PointerButton, PointerButtonEvent, PointerButtons, PointerEvent, PointerId, PointerInfo,
    PointerScrollEvent, PointerState, PointerType, PointerUpdate,
};

// Keyboard types from ui-events
pub use keyboard::{Code, Key, KeyState, KeyboardEvent, Modifiers, NamedKey};

// ============================================================================
// Extended Input Event (wraps ui-events + device lifecycle)
// ============================================================================

/// Device identifier type.
///
/// This is a simple integer ID for device tracking. For more detailed
/// device identification, use [`PointerInfo::persistent_device_id`].
pub type DeviceId = i32;

/// Extended input event that wraps W3C-compliant events.
///
/// This enum extends [`ui_events::pointer::PointerEvent`] with:
/// - Device lifecycle events (`DeviceAdded`, `DeviceRemoved`)
/// - Keyboard events
/// - Scroll events with position
///
/// # Device Lifecycle
///
/// Unlike the W3C spec, we track device connection/disconnection:
///
/// ```rust,ignore
/// match event {
///     InputEvent::DeviceAdded { device_id, pointer_type } => {
///         // New pointing device connected
///     }
///     InputEvent::DeviceRemoved { device_id } => {
///         // Device disconnected
///     }
///     _ => {}
/// }
/// ```
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum InputEvent {
    /// A pointer event (down, up, move, enter, leave, etc.)
    Pointer(PointerEvent),

    /// A keyboard event.
    Keyboard(KeyboardEvent),

    /// A pointing device was added/connected.
    ///
    /// This is not part of W3C spec but useful for device tracking.
    DeviceAdded {
        /// The device identifier.
        device_id: DeviceId,
        /// The type of pointing device.
        pointer_type: PointerType,
    },

    /// A pointing device was removed/disconnected.
    ///
    /// This is not part of W3C spec but useful for device tracking.
    DeviceRemoved {
        /// The device identifier.
        device_id: DeviceId,
    },
}

impl InputEvent {
    /// Returns the device ID if this is a device-related event.
    ///
    /// For pointer events, returns 0 for primary pointer, or uses
    /// a hash of the persistent device ID if available.
    pub fn device_id(&self) -> Option<DeviceId> {
        match self {
            InputEvent::DeviceAdded { device_id, .. } => Some(*device_id),
            InputEvent::DeviceRemoved { device_id } => Some(*device_id),
            InputEvent::Pointer(event) => {
                // Extract pointer_id from the event
                let info = get_pointer_info(event)?;
                // Use 0 for primary pointer, otherwise hash the persistent device ID
                if info
                    .pointer_id
                    .map(|id| id.is_primary_pointer())
                    .unwrap_or(true)
                {
                    Some(0)
                } else if let Some(persistent_id) = info.persistent_device_id {
                    // Use a simple hash of the persistent device ID
                    use std::hash::{Hash, Hasher};
                    let mut hasher = std::collections::hash_map::DefaultHasher::new();
                    persistent_id.hash(&mut hasher);
                    Some((hasher.finish() & 0x7FFFFFFF) as DeviceId)
                } else {
                    Some(0)
                }
            }
            InputEvent::Keyboard(_) => None,
        }
    }

    /// Returns true if this is a pointer event.
    pub fn is_pointer(&self) -> bool {
        matches!(self, InputEvent::Pointer(_))
    }

    /// Returns true if this is a keyboard event.
    pub fn is_keyboard(&self) -> bool {
        matches!(self, InputEvent::Keyboard(_))
    }

    /// Returns true if this is a device lifecycle event.
    pub fn is_device_lifecycle(&self) -> bool {
        matches!(
            self,
            InputEvent::DeviceAdded { .. } | InputEvent::DeviceRemoved { .. }
        )
    }

    /// Returns the pointer event if this is one.
    pub fn as_pointer(&self) -> Option<&PointerEvent> {
        match self {
            InputEvent::Pointer(event) => Some(event),
            _ => None,
        }
    }

    /// Returns the keyboard event if this is one.
    pub fn as_keyboard(&self) -> Option<&KeyboardEvent> {
        match self {
            InputEvent::Keyboard(event) => Some(event),
            _ => None,
        }
    }
}

impl From<PointerEvent> for InputEvent {
    fn from(event: PointerEvent) -> Self {
        InputEvent::Pointer(event)
    }
}

impl From<KeyboardEvent> for InputEvent {
    fn from(event: KeyboardEvent) -> Self {
        InputEvent::Keyboard(event)
    }
}

// ============================================================================
// Helper functions for extracting data from pointer events
// ============================================================================

/// Extracts PointerInfo from a PointerEvent.
fn get_pointer_info(event: &PointerEvent) -> Option<&PointerInfo> {
    match event {
        PointerEvent::Down(e) => Some(&e.pointer),
        PointerEvent::Up(e) => Some(&e.pointer),
        PointerEvent::Move(e) => Some(&e.pointer),
        PointerEvent::Cancel(info) | PointerEvent::Enter(info) | PointerEvent::Leave(info) => {
            Some(info)
        }
        PointerEvent::Scroll(e) => Some(&e.pointer),
        PointerEvent::Gesture(e) => Some(&e.pointer),
    }
}

/// Extracts PointerState from a PointerEvent.
fn get_pointer_state(event: &PointerEvent) -> Option<&PointerState> {
    match event {
        PointerEvent::Down(e) => Some(&e.state),
        PointerEvent::Up(e) => Some(&e.state),
        PointerEvent::Move(e) => Some(&e.current),
        PointerEvent::Scroll(e) => Some(&e.state),
        PointerEvent::Gesture(e) => Some(&e.state),
        _ => None,
    }
}

// ============================================================================
// Helper trait for extracting position from pointer events
// ============================================================================

/// Extension trait for extracting position from pointer events.
pub trait PointerEventExt {
    /// Returns the position of the pointer event.
    fn position(&self) -> Offset;

    /// Returns the pointer type if available.
    fn pointer_type(&self) -> Option<PointerType>;
}

impl PointerEventExt for PointerEvent {
    fn position(&self) -> Offset {
        if let Some(state) = get_pointer_state(self) {
            let pos = state.position;
            Offset::new(pos.x as f32, pos.y as f32)
        } else {
            Offset::ZERO
        }
    }

    fn pointer_type(&self) -> Option<PointerType> {
        get_pointer_info(self).map(|info| info.pointer_type)
    }
}

// ============================================================================
// Scroll event data (compatibility with existing code)
// ============================================================================

/// Scroll event data with position and delta.
///
/// This provides a simpler interface than [`PointerScrollEvent`] for
/// common scroll handling scenarios.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollEventData {
    /// Position where the scroll occurred.
    pub position: Offset,
    /// Scroll delta in pixels (converted from any scroll unit).
    pub delta: Offset,
    /// Keyboard modifiers active during scroll.
    pub modifiers: Modifiers,
}

impl ScrollEventData {
    /// Creates new scroll event data.
    pub fn new(position: Offset, delta: Offset, modifiers: Modifiers) -> Self {
        Self {
            position,
            delta,
            modifiers,
        }
    }

    /// Converts a ScrollDelta to pixel offset.
    pub fn delta_to_offset(delta: &ScrollDelta) -> Offset {
        match delta {
            ScrollDelta::PixelDelta(pos) => Offset::new(pos.x as f32, pos.y as f32),
            ScrollDelta::LineDelta(x, y) => {
                // Approximate: 1 line ≈ 20 pixels
                Offset::new(*x * 20.0, *y * 20.0)
            }
            ScrollDelta::PageDelta(x, y) => {
                // Approximate: 1 page ≈ 400 pixels
                Offset::new(*x * 400.0, *y * 400.0)
            }
        }
    }
}

impl From<&PointerScrollEvent> for ScrollEventData {
    fn from(event: &PointerScrollEvent) -> Self {
        let pos = event.state.position;
        Self {
            position: Offset::new(pos.x as f32, pos.y as f32),
            delta: Self::delta_to_offset(&event.delta),
            modifiers: event.state.modifiers,
        }
    }
}

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
        assert_ne!(KeyState::Down, KeyState::Up);
    }

    #[test]
    fn test_pointer_button() {
        let primary = PointerButton::Primary;
        let secondary = PointerButton::Secondary;
        assert_ne!(primary, secondary);
    }

    #[test]
    fn test_input_event_device_added() {
        let event = InputEvent::DeviceAdded {
            device_id: 1,
            pointer_type: PointerType::Mouse,
        };
        assert!(event.is_device_lifecycle());
        assert_eq!(event.device_id(), Some(1));
    }

    #[test]
    fn test_input_event_device_removed() {
        let event = InputEvent::DeviceRemoved { device_id: 1 };
        assert!(event.is_device_lifecycle());
        assert_eq!(event.device_id(), Some(1));
    }

    #[test]
    fn test_scroll_delta_to_offset() {
        let delta = ScrollDelta::LineDelta(0.0, -3.0);
        let offset = ScrollEventData::delta_to_offset(&delta);
        assert!(offset.dy < 0.0); // Scrolling up
    }
}
