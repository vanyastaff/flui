//! Input event helpers
//!
//! Utilities for converting platform-specific events to FLUI event types.

use flui_types::{
    events::{
        KeyEvent, KeyEventData, KeyModifiers, LogicalKey, PhysicalKey, PointerDeviceKind,
        PointerEvent, PointerEventData, ScrollDelta, ScrollEventData,
    },
    geometry::Offset,
};

/// Convert platform pointer button to PointerDeviceKind
///
/// This is a helper for platform integration code.
pub fn device_kind_from_button(button: u32) -> PointerDeviceKind {
    match button {
        0 => PointerDeviceKind::Mouse, // Left button
        1 => PointerDeviceKind::Mouse, // Right button
        2 => PointerDeviceKind::Mouse, // Middle button
        _ => PointerDeviceKind::Touch, // Touch or stylus
    }
}

/// Create a PointerEvent::Down
pub fn pointer_down(position: Offset, device_kind: PointerDeviceKind) -> PointerEvent {
    PointerEvent::Down(PointerEventData::new(position, device_kind))
}

/// Create a PointerEvent::Up
pub fn pointer_up(position: Offset, device_kind: PointerDeviceKind) -> PointerEvent {
    PointerEvent::Up(PointerEventData::new(position, device_kind))
}

/// Create a PointerEvent::Move
pub fn pointer_move(position: Offset, device_kind: PointerDeviceKind) -> PointerEvent {
    PointerEvent::Move(PointerEventData::new(position, device_kind))
}

/// Create a PointerEvent::Cancel
pub fn pointer_cancel(position: Offset, device_kind: PointerDeviceKind) -> PointerEvent {
    PointerEvent::Cancel(PointerEventData::new(position, device_kind))
}

/// Create a scroll event (in pixels)
pub fn scroll_event(position: Offset, delta: Offset) -> ScrollEventData {
    ScrollEventData {
        position,
        delta: ScrollDelta::Pixels {
            x: delta.dx,
            y: delta.dy,
        },
        modifiers: KeyModifiers::default(),
    }
}

/// Helper for creating keyboard modifiers
pub struct ModifiersBuilder {
    ctrl: bool,
    shift: bool,
    alt: bool,
    meta: bool,
}

impl ModifiersBuilder {
    pub fn new() -> Self {
        Self {
            ctrl: false,
            shift: false,
            alt: false,
            meta: false,
        }
    }

    pub fn ctrl(mut self, enabled: bool) -> Self {
        self.ctrl = enabled;
        self
    }

    pub fn shift(mut self, enabled: bool) -> Self {
        self.shift = enabled;
        self
    }

    pub fn alt(mut self, enabled: bool) -> Self {
        self.alt = enabled;
        self
    }

    pub fn meta(mut self, enabled: bool) -> Self {
        self.meta = enabled;
        self
    }

    pub fn build(self) -> KeyModifiers {
        KeyModifiers {
            control: self.ctrl,
            shift: self.shift,
            alt: self.alt,
            meta: self.meta,
        }
    }
}

impl Default for ModifiersBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Helper for creating key events
pub struct KeyEventBuilder {
    physical_key: PhysicalKey,
    logical_key: LogicalKey,
    text: Option<String>,
    modifiers: KeyModifiers,
    is_down: bool,
}

impl KeyEventBuilder {
    pub fn new(physical_key: PhysicalKey) -> Self {
        Self {
            physical_key,
            logical_key: LogicalKey::Named(physical_key),
            text: None,
            modifiers: KeyModifiers::default(),
            is_down: true,
        }
    }

    pub fn logical_key(mut self, key: LogicalKey) -> Self {
        self.logical_key = key;
        self
    }

    pub fn logical_character(mut self, ch: impl Into<String>) -> Self {
        self.logical_key = LogicalKey::Character(ch.into());
        self
    }

    pub fn text(mut self, text: impl Into<String>) -> Self {
        self.text = Some(text.into());
        self
    }

    pub fn is_down(mut self, down: bool) -> Self {
        self.is_down = down;
        self
    }

    pub fn modifiers(mut self, modifiers: KeyModifiers) -> Self {
        self.modifiers = modifiers;
        self
    }

    pub fn ctrl(mut self, enabled: bool) -> Self {
        self.modifiers.control = enabled;
        self
    }

    pub fn shift(mut self, enabled: bool) -> Self {
        self.modifiers.shift = enabled;
        self
    }

    pub fn alt(mut self, enabled: bool) -> Self {
        self.modifiers.alt = enabled;
        self
    }

    pub fn meta(mut self, enabled: bool) -> Self {
        self.modifiers.meta = enabled;
        self
    }

    pub fn build(self) -> KeyEvent {
        let data = KeyEventData {
            physical_key: self.physical_key,
            logical_key: self.logical_key,
            text: self.text,
            modifiers: self.modifiers,
            repeat: false,
        };

        if self.is_down {
            KeyEvent::Down(data)
        } else {
            KeyEvent::Up(data)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pointer_event_creation() {
        let pos = Offset::new(100.0, 200.0);
        let event = pointer_down(pos, PointerDeviceKind::Mouse);

        assert_eq!(event.position(), pos);
    }

    #[test]
    fn test_modifiers_builder() {
        let modifiers = ModifiersBuilder::new().ctrl(true).shift(true).build();

        assert!(modifiers.control);
        assert!(modifiers.shift);
        assert!(!modifiers.alt);
        assert!(!modifiers.meta);
    }

    #[test]
    fn test_key_event_builder() {
        let event = KeyEventBuilder::new(PhysicalKey::Enter)
            .logical_character("Enter")
            .text("Enter")
            .ctrl(true)
            .build();

        assert_eq!(event.data().physical_key, PhysicalKey::Enter);
        assert!(matches!(event.data().logical_key, LogicalKey::Character(_)));
        assert!(matches!(event, KeyEvent::Down(_)));
        assert!(event.data().modifiers.control);
    }

    #[test]
    fn test_scroll_event_creation() {
        let pos = Offset::new(50.0, 50.0);
        let delta = Offset::new(0.0, 10.0);
        let event = scroll_event(pos, delta);

        assert_eq!(event.position, pos);
        assert!(matches!(
            event.delta,
            ScrollDelta::Pixels { x: 0.0, y: 10.0 }
        ));
    }

    #[test]
    fn test_device_kind_from_button() {
        assert_eq!(device_kind_from_button(0), PointerDeviceKind::Mouse);
        assert_eq!(device_kind_from_button(1), PointerDeviceKind::Mouse);
        assert_eq!(device_kind_from_button(10), PointerDeviceKind::Touch);
    }
}
