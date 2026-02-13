//! Hit test target trait.

use std::any::Any;

use super::HitTestEntry;

/// A target that can receive hit test events.
///
/// Render objects implement this trait to handle pointer events
/// that occur at positions within their bounds.
///
/// # Flutter Equivalence
///
/// Corresponds to Flutter's `HitTestTarget` abstract class.
pub trait HitTestTarget: Any + Send + Sync {
    /// Handles an event at the given hit test entry.
    ///
    /// Called when a pointer event occurs at a position that hit this target.
    /// The entry contains the local position of the event.
    fn handle_event(&self, event: &PointerEvent, entry: &HitTestEntry);

    /// Returns a debug label for this target.
    fn debug_label(&self) -> &'static str {
        "HitTestTarget"
    }
}

/// A pointer event that can be dispatched to hit test targets.
///
/// This is a simplified event type used for hit testing.
/// The full event system is in flui_interaction.
///
/// # Flutter Equivalence
///
/// Corresponds to Flutter's `PointerEvent` and its subclasses.
#[derive(Debug, Clone)]
pub struct PointerEvent {
    /// The type of pointer event.
    pub kind: PointerEventKind,
    /// The position of the event in global coordinates.
    pub position: flui_types::Offset,
    /// The pointer ID.
    pub pointer: i32,
    /// The button that triggered the event (for mouse).
    pub buttons: i32,
    /// The time of the event.
    pub time_stamp: std::time::Duration,
    /// Whether this event is synthesized.
    pub synthesized: bool,
    /// The delta movement since the last event.
    pub delta: flui_types::Offset,
    /// The pressure of the pointer (0.0 to 1.0 for touch/stylus).
    pub pressure: f32,
    /// The minimum pressure this device can report.
    pub pressure_min: f32,
    /// The maximum pressure this device can report.
    pub pressure_max: f32,
    /// The scroll delta for scroll events.
    pub scroll_delta: flui_types::Offset,
    /// The type of device that generated this event.
    pub device_kind: PointerDeviceKind,
    /// Whether this is a primary pointer (first finger, left mouse button).
    pub is_primary: bool,
}

impl PointerEvent {
    /// Creates a new pointer event.
    pub fn new(kind: PointerEventKind, position: flui_types::Offset) -> Self {
        Self {
            kind,
            position,
            pointer: 0,
            buttons: 0,
            time_stamp: std::time::Duration::ZERO,
            synthesized: false,
            delta: flui_types::Offset::ZERO,
            pressure: 1.0,
            pressure_min: 0.0,
            pressure_max: 1.0,
            scroll_delta: flui_types::Offset::ZERO,
            device_kind: PointerDeviceKind::Mouse,
            is_primary: true,
        }
    }

    /// Creates a down event.
    pub fn down(position: flui_types::Offset) -> Self {
        Self::new(PointerEventKind::Down, position)
    }

    /// Creates a move event.
    pub fn move_to(position: flui_types::Offset) -> Self {
        Self::new(PointerEventKind::Move, position)
    }

    /// Creates an up event.
    pub fn up(position: flui_types::Offset) -> Self {
        Self::new(PointerEventKind::Up, position)
    }

    /// Creates a cancel event.
    pub fn cancel(position: flui_types::Offset) -> Self {
        Self::new(PointerEventKind::Cancel, position)
    }

    /// Creates a hover event.
    pub fn hover(position: flui_types::Offset) -> Self {
        Self::new(PointerEventKind::Hover, position)
    }

    /// Creates a scroll event.
    pub fn scroll(position: flui_types::Offset, scroll_delta: flui_types::Offset) -> Self {
        Self {
            scroll_delta,
            ..Self::new(PointerEventKind::Scroll, position)
        }
    }

    /// Creates an enter event.
    pub fn enter(position: flui_types::Offset) -> Self {
        Self::new(PointerEventKind::Enter, position)
    }

    /// Creates an exit event.
    pub fn exit(position: flui_types::Offset) -> Self {
        Self::new(PointerEventKind::Exit, position)
    }

    // ===== Builder methods =====

    /// Sets the pointer ID.
    pub fn with_pointer(mut self, pointer: i32) -> Self {
        self.pointer = pointer;
        self
    }

    /// Sets the buttons.
    pub fn with_buttons(mut self, buttons: i32) -> Self {
        self.buttons = buttons;
        self
    }

    /// Sets the time stamp.
    pub fn with_time_stamp(mut self, time_stamp: std::time::Duration) -> Self {
        self.time_stamp = time_stamp;
        self
    }

    /// Sets the delta movement.
    pub fn with_delta(mut self, delta: flui_types::Offset) -> Self {
        self.delta = delta;
        self
    }

    /// Sets the pressure.
    pub fn with_pressure(mut self, pressure: f32) -> Self {
        self.pressure = pressure;
        self
    }

    /// Sets the scroll delta.
    pub fn with_scroll_delta(mut self, scroll_delta: flui_types::Offset) -> Self {
        self.scroll_delta = scroll_delta;
        self
    }

    /// Sets the device kind.
    pub fn with_device_kind(mut self, device_kind: PointerDeviceKind) -> Self {
        self.device_kind = device_kind;
        self
    }

    /// Sets whether this is the primary pointer.
    pub fn with_primary(mut self, is_primary: bool) -> Self {
        self.is_primary = is_primary;
        self
    }

    /// Sets this as a synthesized event.
    pub fn synthesized(mut self) -> Self {
        self.synthesized = true;
        self
    }

    // ===== Query methods =====

    /// Returns whether this is a down event.
    pub fn is_down(&self) -> bool {
        self.kind == PointerEventKind::Down
    }

    /// Returns whether this is a move event.
    pub fn is_move(&self) -> bool {
        self.kind == PointerEventKind::Move
    }

    /// Returns whether this is an up event.
    pub fn is_up(&self) -> bool {
        self.kind == PointerEventKind::Up
    }

    /// Returns whether this is a cancel event.
    pub fn is_cancel(&self) -> bool {
        self.kind == PointerEventKind::Cancel
    }

    /// Returns whether this is a hover event.
    pub fn is_hover(&self) -> bool {
        self.kind == PointerEventKind::Hover
    }

    /// Returns whether this is a scroll event.
    pub fn is_scroll(&self) -> bool {
        self.kind == PointerEventKind::Scroll
    }

    /// Returns whether the pointer is currently down (in contact).
    pub fn is_pointer_down(&self) -> bool {
        matches!(self.kind, PointerEventKind::Down | PointerEventKind::Move)
    }

    /// Returns the normalized pressure (0.0 to 1.0).
    pub fn normalized_pressure(&self) -> f32 {
        if self.pressure_max == self.pressure_min {
            1.0
        } else {
            (self.pressure - self.pressure_min) / (self.pressure_max - self.pressure_min)
        }
    }

    /// Returns a copy of this event with position transformed by an offset.
    pub fn transformed(&self, offset: flui_types::Offset) -> Self {
        Self {
            position: flui_types::Offset::new(
                self.position.dx - offset.dx,
                self.position.dy - offset.dy,
            ),
            ..self.clone()
        }
    }
}

impl Default for PointerEvent {
    fn default() -> Self {
        Self::new(PointerEventKind::Move, flui_types::Offset::ZERO)
    }
}

/// The type of input device that generated a pointer event.
///
/// # Flutter Equivalence
///
/// Corresponds to Flutter's `PointerDeviceKind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PointerDeviceKind {
    /// A touch screen.
    Touch,
    /// A mouse.
    #[default]
    Mouse,
    /// A stylus or pen.
    Stylus,
    /// An inverted stylus (eraser end).
    InvertedStylus,
    /// A trackpad.
    Trackpad,
    /// An unknown device.
    Unknown,
}

/// The kind of pointer event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointerEventKind {
    /// Pointer has made contact.
    Down,
    /// Pointer has moved while in contact.
    Move,
    /// Pointer has stopped making contact.
    Up,
    /// Pointer event was cancelled.
    Cancel,
    /// Pointer is hovering over target.
    Hover,
    /// Pointer has entered target bounds.
    Enter,
    /// Pointer has exited target bounds.
    Exit,
    /// Mouse scroll event.
    Scroll,
    /// Pointer signal (platform-specific).
    Signal,
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::geometry::px;

    #[test]
    fn test_pointer_event_down() {
        let event = PointerEvent::down(flui_types::Offset::new(px(100.0), px(200.0)));
        assert_eq!(event.kind, PointerEventKind::Down);
        assert_eq!(event.position.dx, 100.0);
        assert_eq!(event.position.dy, 200.0);
    }

    #[test]
    fn test_pointer_event_builder() {
        let event = PointerEvent::down(flui_types::Offset::ZERO)
            .with_pointer(42)
            .with_buttons(1)
            .with_time_stamp(std::time::Duration::from_millis(100));

        assert_eq!(event.pointer, 42);
        assert_eq!(event.buttons, 1);
        assert_eq!(event.time_stamp, std::time::Duration::from_millis(100));
    }

    #[test]
    fn test_pointer_event_kinds() {
        assert_eq!(
            PointerEvent::down(flui_types::Offset::ZERO).kind,
            PointerEventKind::Down
        );
        assert_eq!(
            PointerEvent::move_to(flui_types::Offset::ZERO).kind,
            PointerEventKind::Move
        );
        assert_eq!(
            PointerEvent::up(flui_types::Offset::ZERO).kind,
            PointerEventKind::Up
        );
        assert_eq!(
            PointerEvent::cancel(flui_types::Offset::ZERO).kind,
            PointerEventKind::Cancel
        );
    }
}
