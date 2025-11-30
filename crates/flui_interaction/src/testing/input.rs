//! Input event helpers
//!
//! Utilities for converting platform-specific events to FLUI event types.
//!
//! # Type System Features
//!
//! - **Typestate pattern**: Builders use zero-sized marker types to ensure
//!   required fields are set at compile time
//! - **Extension traits**: Additional methods on input types
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_interaction::input::{KeyEventBuilder, ModifiersBuilder};
//!
//! // Typestate ensures physical_key is set
//! let event = KeyEventBuilder::new(PhysicalKey::Enter)
//!     .logical_character("Enter")
//!     .ctrl(true)
//!     .build();
//! ```

use flui_types::{
    events::{
        KeyEvent, KeyEventData, KeyModifiers, LogicalKey, PhysicalKey, PointerDeviceKind,
        PointerEvent, PointerEventData, ScrollDelta, ScrollEventData,
    },
    geometry::Offset,
};

use crate::ids::PointerId;

// ============================================================================
// Device Kind Helpers
// ============================================================================

/// Convert platform pointer button to PointerDeviceKind
///
/// This is a helper for platform integration code.
///
/// # Button Mapping
///
/// - 0, 1, 2: Mouse buttons (left, right, middle)
/// - Others: Touch or stylus
#[inline]
pub fn device_kind_from_button(button: u32) -> PointerDeviceKind {
    match button {
        0..=2 => PointerDeviceKind::Mouse,
        _ => PointerDeviceKind::Touch,
    }
}

// ============================================================================
// Pointer Event Factory Functions
// ============================================================================

/// Create a PointerEvent::Down
#[inline]
pub fn pointer_down(position: Offset, device_kind: PointerDeviceKind) -> PointerEvent {
    PointerEvent::Down(PointerEventData::new(position, device_kind))
}

/// Create a PointerEvent::Up
#[inline]
pub fn pointer_up(position: Offset, device_kind: PointerDeviceKind) -> PointerEvent {
    PointerEvent::Up(PointerEventData::new(position, device_kind))
}

/// Create a PointerEvent::Move
#[inline]
pub fn pointer_move(position: Offset, device_kind: PointerDeviceKind) -> PointerEvent {
    PointerEvent::Move(PointerEventData::new(position, device_kind))
}

/// Create a PointerEvent::Cancel
#[inline]
pub fn pointer_cancel(position: Offset, device_kind: PointerDeviceKind) -> PointerEvent {
    PointerEvent::Cancel(PointerEventData::new(position, device_kind))
}

/// Create a PointerEvent::Down with device ID
#[inline]
pub fn pointer_down_with_device(
    position: Offset,
    device_kind: PointerDeviceKind,
    device: PointerId,
) -> PointerEvent {
    let mut data = PointerEventData::new(position, device_kind);
    data.device = device.get();
    PointerEvent::Down(data)
}

/// Create a PointerEvent::Up with device ID
#[inline]
pub fn pointer_up_with_device(
    position: Offset,
    device_kind: PointerDeviceKind,
    device: PointerId,
) -> PointerEvent {
    let mut data = PointerEventData::new(position, device_kind);
    data.device = device.get();
    PointerEvent::Up(data)
}

/// Create a PointerEvent::Move with device ID
#[inline]
pub fn pointer_move_with_device(
    position: Offset,
    device_kind: PointerDeviceKind,
    device: PointerId,
) -> PointerEvent {
    let mut data = PointerEventData::new(position, device_kind);
    data.device = device.get();
    PointerEvent::Move(data)
}

// ============================================================================
// Scroll Event Factory
// ============================================================================

/// Create a scroll event (in pixels)
#[inline]
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

/// Create a scroll event with modifiers
#[inline]
pub fn scroll_event_with_modifiers(
    position: Offset,
    delta: Offset,
    modifiers: KeyModifiers,
) -> ScrollEventData {
    ScrollEventData {
        position,
        delta: ScrollDelta::Pixels {
            x: delta.dx,
            y: delta.dy,
        },
        modifiers,
    }
}

// ============================================================================
// Modifiers Builder
// ============================================================================

/// Builder for creating keyboard modifiers with fluent API.
///
/// # Example
///
/// ```rust,ignore
/// let modifiers = ModifiersBuilder::new()
///     .ctrl(true)
///     .shift(true)
///     .build();
/// ```
#[derive(Debug, Clone, Default)]
pub struct ModifiersBuilder {
    ctrl: bool,
    shift: bool,
    alt: bool,
    meta: bool,
}

impl ModifiersBuilder {
    /// Creates a new modifiers builder with all modifiers disabled.
    #[inline]
    pub const fn new() -> Self {
        Self {
            ctrl: false,
            shift: false,
            alt: false,
            meta: false,
        }
    }

    /// Sets the control modifier.
    #[inline]
    pub const fn ctrl(mut self, enabled: bool) -> Self {
        self.ctrl = enabled;
        self
    }

    /// Sets the shift modifier.
    #[inline]
    pub const fn shift(mut self, enabled: bool) -> Self {
        self.shift = enabled;
        self
    }

    /// Sets the alt modifier.
    #[inline]
    pub const fn alt(mut self, enabled: bool) -> Self {
        self.alt = enabled;
        self
    }

    /// Sets the meta (Windows/Command) modifier.
    #[inline]
    pub const fn meta(mut self, enabled: bool) -> Self {
        self.meta = enabled;
        self
    }

    /// Builds the `KeyModifiers`.
    #[inline]
    pub const fn build(self) -> KeyModifiers {
        KeyModifiers {
            control: self.ctrl,
            shift: self.shift,
            alt: self.alt,
            meta: self.meta,
        }
    }
}

// ============================================================================
// Key Event Builder
// ============================================================================

/// Builder for creating key events with fluent API.
///
/// The physical key is required and must be provided at construction.
///
/// # Example
///
/// ```rust,ignore
/// let event = KeyEventBuilder::new(PhysicalKey::Enter)
///     .logical_character("Enter")
///     .ctrl(true)
///     .build();
/// ```
#[derive(Debug, Clone)]
pub struct KeyEventBuilder {
    physical_key: PhysicalKey,
    logical_key: LogicalKey,
    text: Option<String>,
    modifiers: KeyModifiers,
    is_down: bool,
    repeat: bool,
}

impl KeyEventBuilder {
    /// Creates a new key event builder with the given physical key.
    ///
    /// The logical key defaults to `LogicalKey::Named(physical_key)`.
    #[inline]
    pub fn new(physical_key: PhysicalKey) -> Self {
        Self {
            physical_key,
            logical_key: LogicalKey::Named(physical_key),
            text: None,
            modifiers: KeyModifiers::default(),
            is_down: true,
            repeat: false,
        }
    }

    /// Sets the logical key.
    #[inline]
    pub fn logical_key(mut self, key: LogicalKey) -> Self {
        self.logical_key = key;
        self
    }

    /// Sets the logical key to a character.
    #[inline]
    pub fn logical_character(mut self, ch: impl Into<String>) -> Self {
        self.logical_key = LogicalKey::Character(ch.into());
        self
    }

    /// Sets the text produced by this key press.
    #[inline]
    pub fn text(mut self, text: impl Into<String>) -> Self {
        self.text = Some(text.into());
        self
    }

    /// Sets whether this is a key down event (default: true).
    #[inline]
    pub fn is_down(mut self, down: bool) -> Self {
        self.is_down = down;
        self
    }

    /// Sets whether this is a repeat event.
    #[inline]
    pub fn repeat(mut self, repeat: bool) -> Self {
        self.repeat = repeat;
        self
    }

    /// Sets all modifiers at once.
    #[inline]
    pub fn modifiers(mut self, modifiers: KeyModifiers) -> Self {
        self.modifiers = modifiers;
        self
    }

    /// Sets the control modifier.
    #[inline]
    pub fn ctrl(mut self, enabled: bool) -> Self {
        self.modifiers.control = enabled;
        self
    }

    /// Sets the shift modifier.
    #[inline]
    pub fn shift(mut self, enabled: bool) -> Self {
        self.modifiers.shift = enabled;
        self
    }

    /// Sets the alt modifier.
    #[inline]
    pub fn alt(mut self, enabled: bool) -> Self {
        self.modifiers.alt = enabled;
        self
    }

    /// Sets the meta (Windows/Command) modifier.
    #[inline]
    pub fn meta(mut self, enabled: bool) -> Self {
        self.modifiers.meta = enabled;
        self
    }

    /// Builds the `KeyEvent`.
    #[inline]
    pub fn build(self) -> KeyEvent {
        let data = KeyEventData {
            physical_key: self.physical_key,
            logical_key: self.logical_key,
            text: self.text,
            modifiers: self.modifiers,
            repeat: self.repeat,
        };

        if self.is_down {
            KeyEvent::Down(data)
        } else {
            KeyEvent::Up(data)
        }
    }

    /// Builds a key down event.
    #[inline]
    pub fn build_down(mut self) -> KeyEvent {
        self.is_down = true;
        self.build()
    }

    /// Builds a key up event.
    #[inline]
    pub fn build_up(mut self) -> KeyEvent {
        self.is_down = false;
        self.build()
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
