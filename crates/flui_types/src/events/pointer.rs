//! Pointer event types
//!
//! This module provides types for pointer events (mouse, touch, stylus).
//! Based on Flutter's pointer event system.

use crate::gestures::PointerDeviceKind;
use crate::Offset;

/// Mouse button that was pressed
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PointerButton {
    /// Primary button (usually left mouse button)
    Primary,
    /// Secondary button (usually right mouse button)
    Secondary,
    /// Middle button
    Middle,
    /// Additional button
    Other(u8),
}

/// Base pointer event data
///
/// Contains common fields for all pointer events.
#[derive(Debug, Clone)]
pub struct PointerEventData {
    /// Position in global coordinates
    pub position: Offset,
    /// Position in local widget coordinates (set during hit testing)
    pub local_position: Offset,
    /// Device that generated the event
    pub device_kind: PointerDeviceKind,
    /// Pointer device ID
    pub device: i32,
    /// Button that was pressed (for down/up events)
    pub button: Option<PointerButton>,
    /// Buttons currently pressed
    pub buttons: u8,
}

impl PointerEventData {
    /// Create new pointer event data
    pub fn new(position: Offset, device_kind: PointerDeviceKind) -> Self {
        Self {
            position,
            local_position: position,
            device_kind,
            device: 0,
            button: None,
            buttons: 0,
        }
    }

    /// Create with button
    pub fn with_button(mut self, button: PointerButton) -> Self {
        self.button = Some(button);
        self
    }
}

/// Pointer event types
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum PointerEvent {
    /// Pointer device was added (connected)
    Added {
        /// Device ID
        device: i32,
        /// Device kind
        device_kind: PointerDeviceKind,
    },
    /// Pointer device was removed (disconnected)
    Removed {
        /// Device ID
        device: i32,
    },
    /// Pointer pressed down
    Down(PointerEventData),
    /// Pointer released
    Up(PointerEventData),
    /// Pointer moved
    Move(PointerEventData),
    /// Pointer hover (moved without button pressed)
    Hover(PointerEventData),
    /// Pointer entered widget bounds
    Enter(PointerEventData),
    /// Pointer exited widget bounds
    Exit(PointerEventData),
    /// Event cancelled
    Cancel(PointerEventData),
    /// Scroll event (mouse wheel, trackpad scroll)
    Scroll {
        /// Device ID
        device: i32,
        /// Position where scroll occurred
        position: Offset,
        /// Scroll delta
        scroll_delta: Offset,
    },
}

impl PointerEvent {
    /// Get the event data (if available)
    ///
    /// Returns None for Added, Removed, and Scroll events which don't have PointerEventData
    pub fn data(&self) -> Option<&PointerEventData> {
        match self {
            PointerEvent::Down(data)
            | PointerEvent::Up(data)
            | PointerEvent::Move(data)
            | PointerEvent::Hover(data)
            | PointerEvent::Enter(data)
            | PointerEvent::Exit(data)
            | PointerEvent::Cancel(data) => Some(data),
            PointerEvent::Added { .. }
            | PointerEvent::Removed { .. }
            | PointerEvent::Scroll { .. } => None,
        }
    }

    /// Get mutable event data (if available)
    ///
    /// Returns None for Added, Removed, and Scroll events which don't have PointerEventData
    pub fn data_mut(&mut self) -> Option<&mut PointerEventData> {
        match self {
            PointerEvent::Down(data)
            | PointerEvent::Up(data)
            | PointerEvent::Move(data)
            | PointerEvent::Hover(data)
            | PointerEvent::Enter(data)
            | PointerEvent::Exit(data)
            | PointerEvent::Cancel(data) => Some(data),
            PointerEvent::Added { .. }
            | PointerEvent::Removed { .. }
            | PointerEvent::Scroll { .. } => None,
        }
    }

    /// Get position in global coordinates
    pub fn position(&self) -> Offset {
        match self {
            PointerEvent::Scroll { position, .. } => *position,
            _ => self.data().map(|d| d.position).unwrap_or(Offset::ZERO),
        }
    }

    /// Get device ID
    pub fn device(&self) -> i32 {
        match self {
            PointerEvent::Added { device, .. }
            | PointerEvent::Removed { device }
            | PointerEvent::Scroll { device, .. } => *device,
            _ => self.data().map(|d| d.device).unwrap_or(0),
        }
    }

    /// Get position in local widget coordinates
    pub fn local_position(&self) -> Offset {
        self.data()
            .map(|d| d.local_position)
            .unwrap_or(Offset::ZERO)
    }

    /// Set local position (used during hit testing)
    pub fn set_local_position(&mut self, position: Offset) {
        if let Some(data) = self.data_mut() {
            data.local_position = position;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pointer_event_data() {
        let data = PointerEventData::new(Offset::new(10.0, 20.0), PointerDeviceKind::Mouse);
        assert_eq!(data.position, Offset::new(10.0, 20.0));
        assert_eq!(data.device_kind, PointerDeviceKind::Mouse);
    }

    #[test]
    fn test_pointer_event() {
        let data = PointerEventData::new(Offset::new(5.0, 10.0), PointerDeviceKind::Touch);
        let event = PointerEvent::Down(data);

        assert_eq!(event.position(), Offset::new(5.0, 10.0));
    }
}
