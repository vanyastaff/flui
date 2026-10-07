//! Input event helpers
//!
//! Utilities for creating checked events in the owned input vocabulary.
//!
//! # Example
//!
//! ```rust
//! use flui_interaction::testing::input::{pointer_down, pointer_up};
//! use flui_foundation::geometry::Offset;
//! use flui_platform_api::pointer::PointerKind;
//!
//! let down = pointer_down(Offset::new(100.0, 100.0), PointerKind::Mouse)?;
//! let up = pointer_up(Offset::new(100.0, 100.0), PointerKind::Mouse)?;
//! assert!(matches!(down, flui_interaction::PointerEvent::Down(_)));
//! assert!(matches!(up, flui_interaction::PointerEvent::Up(_)));
//! # Ok::<(), flui_platform_api::pointer::InputValueError>(())
//! ```

use flui_foundation::geometry::Offset;
use flui_platform_api::EventTime;
use flui_platform_api::keyboard::{
    Code, ImeComposition, Key, KeyEvent, KeyRepeat, KeyState, Location, Modifiers, NamedKey,
};
use flui_platform_api::pointer::{InputValueError, PointerEvent, PointerKind};

use crate::events::{make_cancel_event, make_down_event, make_move_event, make_up_event};

// ============================================================================
// Device Kind Helpers
// ============================================================================

/// Convert the test's button convention to a pointer kind.
///
/// This is a helper for platform integration code.
///
/// # Button Mapping
///
/// - 0, 1, 2: Mouse buttons (left, right, middle)
/// - Others: Touch or stylus
#[inline]
pub fn device_kind_from_button(button: u32) -> PointerKind {
    match button {
        0..=2 => PointerKind::Mouse,
        _ => PointerKind::Touch,
    }
}

// ============================================================================
// Pointer Event Factory Functions
// ============================================================================

/// Create a PointerEvent::Down
#[inline]
pub fn pointer_down(
    position: Offset<f64>,
    device_kind: PointerKind,
) -> Result<PointerEvent, InputValueError> {
    make_down_event(position, device_kind)
}

/// Create a PointerEvent::Up
#[inline]
pub fn pointer_up(
    position: Offset<f64>,
    device_kind: PointerKind,
) -> Result<PointerEvent, InputValueError> {
    make_up_event(position, device_kind)
}

/// Create a PointerEvent::Move
#[inline]
pub fn pointer_move(
    position: Offset<f64>,
    device_kind: PointerKind,
) -> Result<PointerEvent, InputValueError> {
    make_move_event(position, device_kind)
}

/// Create a PointerEvent::Cancel
#[inline]
pub fn pointer_cancel(device_kind: PointerKind) -> PointerEvent {
    make_cancel_event(device_kind)
}

// ============================================================================
// Modifiers Builder
// ============================================================================

/// Builder for creating keyboard modifiers with fluent API.
///
/// # Example
///
/// ```rust
/// use flui_interaction::testing::input::ModifiersBuilder;
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
            modifiers: Modifiers::NONE,
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
/// ```rust
/// use flui_interaction::testing::input::KeyEventBuilder;
/// use flui_platform_api::keyboard::{Code, KeyState, Modifiers};
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
            modifiers: Modifiers::NONE,
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

    /// Builds the owned `KeyEvent` at the synthetic time origin.
    pub fn build(self) -> KeyEvent {
        KeyEvent::new(self.state, self.key, self.code, EventTime::from_nanos(0))
            .with_location(self.location)
            .with_modifiers(self.modifiers)
            .with_repeat(if self.repeat {
                KeyRepeat::AutoRepeat
            } else {
                KeyRepeat::First
            })
            .with_composition(if self.is_composing {
                ImeComposition::Active
            } else {
                ImeComposition::Inactive
            })
    }
}
