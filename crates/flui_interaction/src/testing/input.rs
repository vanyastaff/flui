//! Input event helpers
//!
//! Utilities for creating test events using ui-events types.
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_interaction::testing::input::{pointer_down, pointer_up};
//! use flui_types::Offset;
//! use ui_events::pointer::PointerType;
//!
//! let down = pointer_down(Offset::new(100.0, 100.0), PointerType::Mouse);
//! let up = pointer_up(Offset::new(100.0, 100.0), PointerType::Mouse);
//! ```

use crate::events::{
    make_cancel_event, make_down_event, make_move_event, make_up_event, Code, Key, KeyState,
    KeyboardEvent, Modifiers, NamedKey, PointerEvent, PointerType,
};
use flui_types::geometry::Pixels;

use flui_types::geometry::Offset;
use ui_events::keyboard::Location;

// ============================================================================
// Device Kind Helpers
// ============================================================================

/// Convert platform pointer button to PointerType
///
/// This is a helper for platform integration code.
///
/// # Button Mapping
///
/// - 0, 1, 2: Mouse buttons (left, right, middle)
/// - Others: Touch or stylus
#[inline]
pub fn device_kind_from_button(button: u32) -> PointerType {
    match button {
        0..=2 => PointerType::Mouse,
        _ => PointerType::Touch,
    }
}

// ============================================================================
// Pointer Event Factory Functions
// ============================================================================

/// Create a PointerEvent::Down
#[inline]
pub fn pointer_down(position: Offset<Pixels>, device_kind: PointerType) -> PointerEvent {
    make_down_event(position, device_kind)
}

/// Create a PointerEvent::Up
#[inline]
pub fn pointer_up(position: Offset<Pixels>, device_kind: PointerType) -> PointerEvent {
    make_up_event(position, device_kind)
}

/// Create a PointerEvent::Move
#[inline]
pub fn pointer_move(position: Offset<Pixels>, device_kind: PointerType) -> PointerEvent {
    make_move_event(position, device_kind)
}

/// Create a PointerEvent::Cancel
#[inline]
pub fn pointer_cancel(device_kind: PointerType) -> PointerEvent {
    make_cancel_event(device_kind)
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
    modifiers: Modifiers,
}

impl ModifiersBuilder {
    /// Creates a new modifiers builder with all modifiers disabled.
    #[inline]
    pub const fn new() -> Self {
        Self {
            modifiers: Modifiers::empty(),
        }
    }

    /// Sets the control modifier.
    #[inline]
    pub fn ctrl(mut self, enabled: bool) -> Self {
        if enabled {
            self.modifiers |= Modifiers::CONTROL;
        }
        self
    }

    /// Sets the shift modifier.
    #[inline]
    pub fn shift(mut self, enabled: bool) -> Self {
        if enabled {
            self.modifiers |= Modifiers::SHIFT;
        }
        self
    }

    /// Sets the alt modifier.
    #[inline]
    pub fn alt(mut self, enabled: bool) -> Self {
        if enabled {
            self.modifiers |= Modifiers::ALT;
        }
        self
    }

    /// Sets the meta (Windows/Command) modifier.
    #[inline]
    pub fn meta(mut self, enabled: bool) -> Self {
        if enabled {
            self.modifiers |= Modifiers::META;
        }
        self
    }

    /// Builds the `Modifiers`.
    #[inline]
    pub const fn build(self) -> Modifiers {
        self.modifiers
    }
}

// ============================================================================
// Key Event Builder
// ============================================================================

/// Builder for creating keyboard events with fluent API.
///
/// # Example
///
/// ```rust,ignore
/// use crate::testing::input::KeyEventBuilder;
/// use ui_events::keyboard::Code;
///
/// let event = KeyEventBuilder::new(Code::KeyA)
///     .with_state(KeyState::Down)
///     .with_modifiers(Modifiers::CONTROL)
///     .build();
/// ```
#[derive(Debug, Clone)]
pub struct KeyEventBuilder {
    code: Code,
    key: Key,
    state: KeyState,
    modifiers: Modifiers,
    location: Location,
    repeat: bool,
    is_composing: bool,
}

impl KeyEventBuilder {
    /// Creates a new key event builder with the given code.
    pub fn new(code: Code) -> Self {
        Self {
            code,
            key: Key::Named(NamedKey::Unidentified),
            state: KeyState::Down,
            modifiers: Modifiers::empty(),
            location: Location::Standard,
            repeat: false,
            is_composing: false,
        }
    }

    /// Sets the logical key.
    pub fn with_key(mut self, key: Key) -> Self {
        self.key = key;
        self
    }

    /// Sets the key state.
    pub fn with_state(mut self, state: KeyState) -> Self {
        self.state = state;
        self
    }

    /// Sets the modifiers.
    pub fn with_modifiers(mut self, modifiers: Modifiers) -> Self {
        self.modifiers = modifiers;
        self
    }

    /// Sets the key location.
    pub fn with_location(mut self, location: Location) -> Self {
        self.location = location;
        self
    }

    /// Sets whether this is a repeat event.
    pub fn with_repeat(mut self, repeat: bool) -> Self {
        self.repeat = repeat;
        self
    }

    /// Sets whether this is a composing event.
    pub fn with_composing(mut self, is_composing: bool) -> Self {
        self.is_composing = is_composing;
        self
    }

    /// Builds the `KeyboardEvent`.
    pub fn build(self) -> KeyboardEvent {
        KeyboardEvent {
            state: self.state,
            key: self.key,
            code: self.code,
            location: self.location,
            modifiers: self.modifiers,
            repeat: self.repeat,
            is_composing: self.is_composing,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::PointerEventExt;

    #[test]
    fn test_pointer_event_creation() {
        let pos = Offset::new(100.0, 200.0);
        let event = pointer_down(pos, PointerType::Mouse);

        assert_eq!(event.position(), pos);
    }

    #[test]
    fn test_modifiers_builder() {
        let modifiers = ModifiersBuilder::new().ctrl(true).shift(true).build();

        assert!(modifiers.contains(Modifiers::CONTROL));
        assert!(modifiers.contains(Modifiers::SHIFT));
        assert!(!modifiers.contains(Modifiers::ALT));
        assert!(!modifiers.contains(Modifiers::META));
    }

    #[test]
    fn test_device_kind_from_button() {
        assert_eq!(device_kind_from_button(0), PointerType::Mouse);
        assert_eq!(device_kind_from_button(1), PointerType::Mouse);
        assert_eq!(device_kind_from_button(10), PointerType::Touch);
    }
}
