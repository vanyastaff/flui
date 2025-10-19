//! Event types for pointer and gesture handling
//!
//! This module provides event types for user interactions like mouse clicks,
//! touches, and gestures. Based on Flutter's pointer event system.

use crate::{Offset, Size};

/// Device type that generated the pointer event
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PointerDeviceKind {
    /// Mouse pointer
    Mouse,
    /// Touch screen
    Touch,
    /// Stylus/pen
    Stylus,
    /// Trackpad
    Trackpad,
    /// Unknown device
    Unknown,
}

/// Mouse button that was pressed
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
/// Contains common fields for all pointer events
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
pub enum PointerEvent {
    /// Pointer pressed down
    Down(PointerEventData),
    /// Pointer released
    Up(PointerEventData),
    /// Pointer moved
    Move(PointerEventData),
    /// Pointer entered widget bounds
    Enter(PointerEventData),
    /// Pointer exited widget bounds
    Exit(PointerEventData),
    /// Event cancelled
    Cancel(PointerEventData),
}

impl PointerEvent {
    /// Get the event data
    pub fn data(&self) -> &PointerEventData {
        match self {
            PointerEvent::Down(data) => data,
            PointerEvent::Up(data) => data,
            PointerEvent::Move(data) => data,
            PointerEvent::Enter(data) => data,
            PointerEvent::Exit(data) => data,
            PointerEvent::Cancel(data) => data,
        }
    }

    /// Get mutable event data
    pub fn data_mut(&mut self) -> &mut PointerEventData {
        match self {
            PointerEvent::Down(data) => data,
            PointerEvent::Up(data) => data,
            PointerEvent::Move(data) => data,
            PointerEvent::Enter(data) => data,
            PointerEvent::Exit(data) => data,
            PointerEvent::Cancel(data) => data,
        }
    }

    /// Get position in global coordinates
    pub fn position(&self) -> Offset {
        self.data().position
    }

    /// Get position in local widget coordinates
    pub fn local_position(&self) -> Offset {
        self.data().local_position
    }

    /// Set local position (used during hit testing)
    pub fn set_local_position(&mut self, position: Offset) {
        self.data_mut().local_position = position;
    }
}

/// Hit test result entry
///
/// Represents a widget that was hit during hit testing
#[derive(Debug, Clone)]
pub struct HitTestEntry {
    /// Position where the hit occurred in local coordinates
    pub local_position: Offset,
    /// Bounds of the widget that was hit
    pub bounds: Size,
}

impl HitTestEntry {
    /// Create a new hit test entry
    pub fn new(local_position: Offset, bounds: Size) -> Self {
        Self {
            local_position,
            bounds,
        }
    }
}

/// Hit test result
///
/// Contains all widgets that were hit during hit testing,
/// ordered from front to back
#[derive(Debug, Clone, Default)]
pub struct HitTestResult {
    /// Stack of hit entries (front to back)
    entries: Vec<HitTestEntry>,
}

impl HitTestResult {
    /// Create a new empty hit test result
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Add an entry to the result
    pub fn add(&mut self, entry: HitTestEntry) {
        self.entries.push(entry);
    }

    /// Get all entries
    pub fn entries(&self) -> &[HitTestEntry] {
        &self.entries
    }

    /// Check if any widget was hit
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Get the top-most (front) entry
    pub fn front(&self) -> Option<&HitTestEntry> {
        self.entries.first()
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

    #[test]
    fn test_hit_test_result() {
        let mut result = HitTestResult::new();
        assert!(result.is_empty());

        result.add(HitTestEntry::new(
            Offset::new(1.0, 2.0),
            Size::new(100.0, 50.0),
        ));

        assert!(!result.is_empty());
        assert_eq!(result.entries().len(), 1);
    }
}
