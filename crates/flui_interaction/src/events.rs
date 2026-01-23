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
use flui_types::geometry::Pixels;
use flui_types::geometry::PixelDelta;


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

/// Alias for KeyboardEvent for compatibility
pub type KeyEvent = KeyboardEvent;

/// Generic event enum covering all input types
#[derive(Debug, Clone)]
pub enum Event {
    /// Pointer event (mouse, touch, pen)
    Pointer(PointerEvent),
    /// Keyboard event
    Keyboard(KeyboardEvent),
    /// Key event (alias for Keyboard)
    Key(KeyboardEvent),
    /// Scroll event
    Scroll(ScrollEventData),
}

// ============================================================================
// Compatibility PointerEventData struct
// ============================================================================

/// Base pointer event data for gesture recognition.
///
/// This struct provides compatibility with legacy gesture recognizers
/// while wrapping W3C-compliant ui-events underneath.
#[derive(Debug, Clone)]
pub struct PointerEventData {
    /// Position in global coordinates
    pub position: Offset<Pixels>,
    /// Position in local widget coordinates (set during hit testing)
    pub local_position: Offset<Pixels>,
    /// Device that generated the event
    pub device_kind: PointerType,
    /// Pointer device ID
    pub device: i32,
    /// Buttons currently pressed
    pub buttons: PointerButtons,
    /// Pressure of the touch (0.0 to 1.0)
    pub pressure: f32,
    /// Time stamp in nanoseconds
    pub time_stamp: u64,
}

impl PointerEventData {
    /// Create new pointer event data
    pub fn new(position: Offset<Pixels>, device_kind: PointerType) -> Self {
        Self {
            position,
            local_position: position,
            device_kind,
            device: 0,
            buttons: PointerButtons::new(),
            pressure: 0.0,
            time_stamp: 0,
        }
    }

    /// Create with device ID
    pub fn with_device(mut self, device: i32) -> Self {
        self.device = device;
        self
    }

    /// Create with pressure
    pub fn with_pressure(mut self, pressure: f32) -> Self {
        self.pressure = pressure.clamp(0.0, 1.0);
        self
    }

    /// Create with buttons
    pub fn with_buttons(mut self, buttons: PointerButtons) -> Self {
        self.buttons = buttons;
        self
    }

    /// Create with time stamp
    pub fn with_time_stamp(mut self, time_stamp: u64) -> Self {
        self.time_stamp = time_stamp;
        self
    }

    /// Returns the normalized pressure (0.0 to 1.0)
    pub fn normalized_pressure(&self) -> f32 {
        self.pressure
    }

    /// Returns true if the device supports pressure sensing
    pub fn supports_pressure(&self) -> bool {
        self.pressure > 0.0
    }

    /// Returns true if this is a force press (pressure > threshold)
    ///
    /// Default threshold is 0.4 (40% of max pressure).
    pub fn is_force_press(&self) -> bool {
        self.is_force_press_at(0.4)
    }

    /// Returns true if pressure exceeds the given threshold (0.0 to 1.0)
    pub fn is_force_press_at(&self, threshold: f32) -> bool {
        self.normalized_pressure() >= threshold
    }

    /// Create from a ui-events PointerEvent
    pub fn from_pointer_event(event: &PointerEvent) -> Option<Self> {
        let info = get_pointer_info(event)?;
        let state = get_pointer_state(event);

        let (position, time_stamp, buttons) = if let Some(s) = state {
            let pos = s.position;
            (
                Offset::new(pos.x as f32, pos.y as f32),
                s.time, // time is already u64 nanoseconds
                s.buttons,
            )
        } else {
            (Offset::ZERO, 0, PointerButtons::new())
        };

        // Convert pointer ID: 0 for primary, hash for others
        let device = match info.pointer_id {
            Some(id) if id.is_primary_pointer() => 0,
            Some(id) => {
                use std::hash::{Hash, Hasher};
                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                id.hash(&mut hasher);
                (hasher.finish() & 0x7FFFFFFF) as i32
            }
            None => 0,
        };

        Some(Self {
            position,
            local_position: position,
            device_kind: info.pointer_type,
            device,
            buttons,
            pressure: state.map(|s| s.pressure).unwrap_or(0.0),
            time_stamp,
        })
    }
}

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
    fn position(&self) -> Offset<Pixels>;

    /// Returns the pointer type if available.
    fn pointer_type(&self) -> Option<PointerType>;
}

impl PointerEventExt for PointerEvent {
    fn position(&self) -> Offset<Pixels> {
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
    pub position: Offset<Pixels>,
    /// Scroll delta in pixels (converted from any scroll unit).
    pub delta: Offset<PixelDelta>,
    /// Keyboard modifiers active during scroll.
    pub modifiers: Modifiers,
}

impl ScrollEventData {
    /// Creates new scroll event data.
    pub fn new(position: Offset<Pixels>, delta: Offset<PixelDelta>, modifiers: Modifiers) -> Self {
        Self {
            position,
            delta,
            modifiers,
        }
    }

    /// Converts a ScrollDelta to pixel offset.
    pub fn delta_to_offset(delta: &ScrollDelta) -> Offset<PixelDelta> {
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

// ============================================================================
// Test helper functions
// ============================================================================

/// Create a PointerEvent::Down for testing
pub fn make_down_event(position: Offset<Pixels>, pointer_type: PointerType) -> PointerEvent {
    use ui_events::pointer::{
        ContactGeometry, PointerButtonEvent, PointerOrientation, PointerState,
    };

    PointerEvent::Down(PointerButtonEvent {
        button: Some(PointerButton::Primary),
        pointer: PointerInfo {
            pointer_id: Some(PointerId::PRIMARY),
            pointer_type,
            persistent_device_id: None,
        },
        state: PointerState {
            time: 0,
            position: dpi::PhysicalPosition::new(position.dx.get() as f64, position.dy.get() as f64),
            buttons: PointerButtons::from(PointerButton::Primary),
            modifiers: Modifiers::empty(),
            count: 1,
            contact_geometry: ContactGeometry {
                width: 1.0,
                height: 1.0,
            },
            orientation: PointerOrientation::default(),
            pressure: 1.0,
            tangential_pressure: 0.0,
            scale_factor: 1.0,
        },
    })
}

/// Create a PointerEvent::Up for testing
pub fn make_up_event(position: Offset<Pixels>, pointer_type: PointerType) -> PointerEvent {
    use ui_events::pointer::{
        ContactGeometry, PointerButtonEvent, PointerOrientation, PointerState,
    };

    PointerEvent::Up(PointerButtonEvent {
        button: Some(PointerButton::Primary),
        pointer: PointerInfo {
            pointer_id: Some(PointerId::PRIMARY),
            pointer_type,
            persistent_device_id: None,
        },
        state: PointerState {
            time: 0,
            position: dpi::PhysicalPosition::new(position.dx.get() as f64, position.dy.get() as f64),
            buttons: PointerButtons::new(),
            modifiers: Modifiers::empty(),
            count: 1,
            contact_geometry: ContactGeometry {
                width: 1.0,
                height: 1.0,
            },
            orientation: PointerOrientation::default(),
            pressure: 0.0,
            tangential_pressure: 0.0,
            scale_factor: 1.0,
        },
    })
}

/// Create a PointerEvent::Move for testing
pub fn make_move_event(position: Offset<Pixels>, pointer_type: PointerType) -> PointerEvent {
    use ui_events::pointer::{ContactGeometry, PointerOrientation, PointerState, PointerUpdate};

    PointerEvent::Move(PointerUpdate {
        pointer: PointerInfo {
            pointer_id: Some(PointerId::PRIMARY),
            pointer_type,
            persistent_device_id: None,
        },
        current: PointerState {
            time: 0,
            position: dpi::PhysicalPosition::new(position.dx.get() as f64, position.dy.get() as f64),
            buttons: PointerButtons::from(PointerButton::Primary),
            modifiers: Modifiers::empty(),
            count: 0,
            contact_geometry: ContactGeometry {
                width: 1.0,
                height: 1.0,
            },
            orientation: PointerOrientation::default(),
            pressure: 1.0,
            tangential_pressure: 0.0,
            scale_factor: 1.0,
        },
        coalesced: vec![],
        predicted: vec![],
    })
}

/// Create a PointerEvent::Cancel for testing
pub fn make_cancel_event(pointer_type: PointerType) -> PointerEvent {
    PointerEvent::Cancel(PointerInfo {
        pointer_id: Some(PointerId::PRIMARY),
        pointer_type,
        persistent_device_id: None,
    })
}

/// Create a PointerEvent::Scroll for testing
pub fn make_scroll_event(position: Offset<Pixels>, delta: Offset<Pixels>) -> PointerEvent {
    use ui_events::pointer::{
        ContactGeometry, PointerOrientation, PointerScrollEvent, PointerState,
    };

    PointerEvent::Scroll(PointerScrollEvent {
        pointer: PointerInfo {
            pointer_id: Some(PointerId::PRIMARY),
            pointer_type: PointerType::Mouse,
            persistent_device_id: None,
        },
        delta: ScrollDelta::PixelDelta(dpi::PhysicalPosition::new(
            delta.dx.get() as f64,
            delta.dy.get() as f64,
        )),
        state: PointerState {
            time: 0,
            position: dpi::PhysicalPosition::new(position.dx.get() as f64, position.dy.get() as f64),
            buttons: PointerButtons::new(),
            modifiers: Modifiers::empty(),
            count: 0,
            contact_geometry: ContactGeometry {
                width: 1.0,
                height: 1.0,
            },
            orientation: PointerOrientation::default(),
            pressure: 0.0,
            tangential_pressure: 0.0,
            scale_factor: 1.0,
        },
    })
}

/// Kind of pointer event for event construction
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointerEventKind {
    /// Pointer down
    Down,
    /// Pointer move
    Move,
    /// Pointer up
    Up,
    /// Pointer cancel
    Cancel,
}

/// Create a PointerEvent from PointerEventData
pub fn make_pointer_event(kind: PointerEventKind, data: PointerEventData) -> PointerEvent {
    use ui_events::pointer::{
        ContactGeometry, PointerButtonEvent, PointerOrientation, PointerState, PointerUpdate,
    };

    let pointer_info = PointerInfo {
        pointer_id: Some(PointerId::PRIMARY),
        pointer_type: data.device_kind,
        persistent_device_id: None,
    };

    let state = PointerState {
        time: data.time_stamp,
        position: dpi::PhysicalPosition::new(data.position.dx.get() as f64, data.position.dy.get() as f64),
        buttons: data.buttons,
        modifiers: Modifiers::empty(),
        count: 1,
        contact_geometry: ContactGeometry {
            width: 1.0,
            height: 1.0,
        },
        orientation: PointerOrientation::default(),
        pressure: data.pressure,
        tangential_pressure: 0.0,
        scale_factor: 1.0,
    };

    match kind {
        PointerEventKind::Down => PointerEvent::Down(PointerButtonEvent {
            button: Some(PointerButton::Primary),
            pointer: pointer_info,
            state,
        }),
        PointerEventKind::Up => PointerEvent::Up(PointerButtonEvent {
            button: Some(PointerButton::Primary),
            pointer: pointer_info,
            state,
        }),
        PointerEventKind::Move => PointerEvent::Move(PointerUpdate {
            pointer: pointer_info,
            current: state,
            coalesced: vec![],
            predicted: vec![],
        }),
        PointerEventKind::Cancel => PointerEvent::Cancel(pointer_info),
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
