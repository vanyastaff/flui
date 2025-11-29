//! Keyboard event types
//!
//! This module provides types for keyboard events including physical keys,
//! logical keys, modifiers, and key events.

/// Physical key on the keyboard
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PhysicalKey {
    // Letter keys
    /// Key A
    KeyA,
    /// Key B
    KeyB,
    /// Key C
    KeyC,
    /// Key D
    KeyD,
    /// Key E
    KeyE,
    /// Key F
    KeyF,
    /// Key G
    KeyG,
    /// Key H
    KeyH,
    /// Key I
    KeyI,
    /// Key J
    KeyJ,
    /// Key K
    KeyK,
    /// Key L
    KeyL,
    /// Key M
    KeyM,
    /// Key N
    KeyN,
    /// Key O
    KeyO,
    /// Key P
    KeyP,
    /// Key Q
    KeyQ,
    /// Key R
    KeyR,
    /// Key S
    KeyS,
    /// Key T
    KeyT,
    /// Key U
    KeyU,
    /// Key V
    KeyV,
    /// Key W
    KeyW,
    /// Key X
    KeyX,
    /// Key Y
    KeyY,
    /// Key Z
    KeyZ,

    // Number keys
    /// Digit 0
    Digit0,
    /// Digit 1
    Digit1,
    /// Digit 2
    Digit2,
    /// Digit 3
    Digit3,
    /// Digit 4
    Digit4,
    /// Digit 5
    Digit5,
    /// Digit 6
    Digit6,
    /// Digit 7
    Digit7,
    /// Digit 8
    Digit8,
    /// Digit 9
    Digit9,

    // Function keys
    /// F1 key
    F1,
    /// F2 key
    F2,
    /// F3 key
    F3,
    /// F4 key
    F4,
    /// F5 key
    F5,
    /// F6 key
    F6,
    /// F7 key
    F7,
    /// F8 key
    F8,
    /// F9 key
    F9,
    /// F10 key
    F10,
    /// F11 key
    F11,
    /// F12 key
    F12,

    // Navigation keys
    /// Arrow up
    ArrowUp,
    /// Arrow down
    ArrowDown,
    /// Arrow left
    ArrowLeft,
    /// Arrow right
    ArrowRight,
    /// Home key
    Home,
    /// End key
    End,
    /// Page up
    PageUp,
    /// Page down
    PageDown,

    // Editing keys
    /// Backspace key
    Backspace,
    /// Delete key
    Delete,
    /// Insert key
    Insert,
    /// Enter key
    Enter,
    /// Tab key
    Tab,
    /// Escape key
    Escape,
    /// Space key
    Space,

    // Modifier keys
    /// Left shift
    ShiftLeft,
    /// Right shift
    ShiftRight,
    /// Left control
    ControlLeft,
    /// Right control
    ControlRight,
    /// Left alt
    AltLeft,
    /// Right alt
    AltRight,
    /// Left meta (Windows/Command key)
    MetaLeft,
    /// Right meta (Windows/Command key)
    MetaRight,

    // Other common keys
    /// Caps lock
    CapsLock,
    /// Num lock
    NumLock,
    /// Scroll lock
    ScrollLock,
    /// Print screen
    PrintScreen,
    /// Pause key
    Pause,

    /// Unidentified key
    Unidentified,
}

/// Logical key - the meaning of the key press
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum LogicalKey {
    /// Character input (e.g., 'a', 'A', '1', '!')
    Character(String),

    /// Named key (non-character keys)
    Named(PhysicalKey),
}

/// Keyboard modifiers state
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct KeyModifiers {
    /// Shift key is pressed
    pub shift: bool,
    /// Control key is pressed
    pub control: bool,
    /// Alt/Option key is pressed
    pub alt: bool,
    /// Meta/Command/Windows key is pressed
    pub meta: bool,
}

impl KeyModifiers {
    /// Create new modifiers with all keys released
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if no modifiers are pressed
    pub fn is_empty(&self) -> bool {
        !self.shift && !self.control && !self.alt && !self.meta
    }
}

/// Keyboard event data
#[derive(Debug, Clone)]
pub struct KeyEventData {
    /// Physical key that was pressed
    pub physical_key: PhysicalKey,

    /// Logical key (what the key means)
    pub logical_key: LogicalKey,

    /// Text produced by the key press (if any)
    pub text: Option<String>,

    /// Current modifier keys state
    pub modifiers: KeyModifiers,

    /// Whether this is a repeat event (key held down)
    pub repeat: bool,
}

impl KeyEventData {
    /// Create new keyboard event data
    pub fn new(physical_key: PhysicalKey, logical_key: LogicalKey) -> Self {
        Self {
            physical_key,
            logical_key,
            text: None,
            modifiers: KeyModifiers::new(),
            repeat: false,
        }
    }

    /// Set the text produced by this key press
    pub fn with_text(mut self, text: String) -> Self {
        self.text = Some(text);
        self
    }

    /// Set the modifiers state
    pub fn with_modifiers(mut self, modifiers: KeyModifiers) -> Self {
        self.modifiers = modifiers;
        self
    }
}

/// Keyboard event types
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum KeyEvent {
    /// Key was pressed down
    Down(KeyEventData),

    /// Key was released
    Up(KeyEventData),
}

impl KeyEvent {
    /// Get the event data
    pub fn data(&self) -> &KeyEventData {
        match self {
            KeyEvent::Down(data) => data,
            KeyEvent::Up(data) => data,
        }
    }

    /// Get mutable event data
    pub fn data_mut(&mut self) -> &mut KeyEventData {
        match self {
            KeyEvent::Down(data) => data,
            KeyEvent::Up(data) => data,
        }
    }

    /// Get the physical key
    pub fn physical_key(&self) -> PhysicalKey {
        self.data().physical_key
    }

    /// Get the logical key
    pub fn logical_key(&self) -> &LogicalKey {
        &self.data().logical_key
    }

    /// Get the text produced by this key press
    pub fn text(&self) -> Option<&str> {
        self.data().text.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_event() {
        let data = KeyEventData::new(PhysicalKey::KeyA, LogicalKey::Character("a".to_string()));
        let event = KeyEvent::Down(data);

        assert_eq!(event.physical_key(), PhysicalKey::KeyA);
    }

    #[test]
    fn test_key_modifiers() {
        let modifiers = KeyModifiers {
            shift: true,
            control: false,
            alt: false,
            meta: false,
        };

        assert!(!modifiers.is_empty());
    }
}
