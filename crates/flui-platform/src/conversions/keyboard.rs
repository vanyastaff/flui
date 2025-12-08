//! Keyboard event conversion from winit to FLUI
//!
//! Converts winit's keyboard events to FLUI's strongly-typed keyboard events.

use flui_types::events::{KeyEvent, KeyEventData, KeyModifiers, LogicalKey, PhysicalKey};
use winit::event::{ElementState, KeyEvent as WinitKeyEvent};
use winit::keyboard::{Key, KeyCode, ModifiersState, NamedKey, SmolStr};

/// Convert winit KeyEvent to FLUI KeyEvent
///
/// # Example
///
/// ```rust,ignore
/// use winit::event::KeyEvent as WinitKeyEvent;
/// use flui_platform::conversions::convert_key_event;
///
/// fn handle_winit_event(winit_event: &WinitKeyEvent, modifiers: ModifiersState) {
///     let flui_event = convert_key_event(winit_event, modifiers);
///     // Route to FLUI event system
/// }
/// ```
#[inline]
pub fn convert_key_event(event: &WinitKeyEvent, modifiers: ModifiersState) -> KeyEvent {
    let physical_key = convert_physical_key(event.physical_key);
    let logical_key = convert_logical_key(&event.logical_key);
    let text = extract_text(&event.text);
    let key_modifiers = convert_modifiers(modifiers);

    let data = KeyEventData {
        physical_key,
        logical_key,
        text,
        modifiers: key_modifiers,
        repeat: event.repeat,
    };

    match event.state {
        ElementState::Pressed => KeyEvent::Down(data),
        ElementState::Released => KeyEvent::Up(data),
    }
}

/// Convert winit PhysicalKey (KeyCode) to FLUI PhysicalKey
#[inline]
fn convert_physical_key(key_code: winit::keyboard::PhysicalKey) -> PhysicalKey {
    use winit::keyboard::PhysicalKey as WinitPhysical;

    match key_code {
        WinitPhysical::Code(code) => match code {
            // Letters
            KeyCode::KeyA => PhysicalKey::KeyA,
            KeyCode::KeyB => PhysicalKey::KeyB,
            KeyCode::KeyC => PhysicalKey::KeyC,
            KeyCode::KeyD => PhysicalKey::KeyD,
            KeyCode::KeyE => PhysicalKey::KeyE,
            KeyCode::KeyF => PhysicalKey::KeyF,
            KeyCode::KeyG => PhysicalKey::KeyG,
            KeyCode::KeyH => PhysicalKey::KeyH,
            KeyCode::KeyI => PhysicalKey::KeyI,
            KeyCode::KeyJ => PhysicalKey::KeyJ,
            KeyCode::KeyK => PhysicalKey::KeyK,
            KeyCode::KeyL => PhysicalKey::KeyL,
            KeyCode::KeyM => PhysicalKey::KeyM,
            KeyCode::KeyN => PhysicalKey::KeyN,
            KeyCode::KeyO => PhysicalKey::KeyO,
            KeyCode::KeyP => PhysicalKey::KeyP,
            KeyCode::KeyQ => PhysicalKey::KeyQ,
            KeyCode::KeyR => PhysicalKey::KeyR,
            KeyCode::KeyS => PhysicalKey::KeyS,
            KeyCode::KeyT => PhysicalKey::KeyT,
            KeyCode::KeyU => PhysicalKey::KeyU,
            KeyCode::KeyV => PhysicalKey::KeyV,
            KeyCode::KeyW => PhysicalKey::KeyW,
            KeyCode::KeyX => PhysicalKey::KeyX,
            KeyCode::KeyY => PhysicalKey::KeyY,
            KeyCode::KeyZ => PhysicalKey::KeyZ,

            // Numbers
            KeyCode::Digit0 => PhysicalKey::Digit0,
            KeyCode::Digit1 => PhysicalKey::Digit1,
            KeyCode::Digit2 => PhysicalKey::Digit2,
            KeyCode::Digit3 => PhysicalKey::Digit3,
            KeyCode::Digit4 => PhysicalKey::Digit4,
            KeyCode::Digit5 => PhysicalKey::Digit5,
            KeyCode::Digit6 => PhysicalKey::Digit6,
            KeyCode::Digit7 => PhysicalKey::Digit7,
            KeyCode::Digit8 => PhysicalKey::Digit8,
            KeyCode::Digit9 => PhysicalKey::Digit9,

            // Function keys
            KeyCode::F1 => PhysicalKey::F1,
            KeyCode::F2 => PhysicalKey::F2,
            KeyCode::F3 => PhysicalKey::F3,
            KeyCode::F4 => PhysicalKey::F4,
            KeyCode::F5 => PhysicalKey::F5,
            KeyCode::F6 => PhysicalKey::F6,
            KeyCode::F7 => PhysicalKey::F7,
            KeyCode::F8 => PhysicalKey::F8,
            KeyCode::F9 => PhysicalKey::F9,
            KeyCode::F10 => PhysicalKey::F10,
            KeyCode::F11 => PhysicalKey::F11,
            KeyCode::F12 => PhysicalKey::F12,

            // Modifiers
            KeyCode::ShiftLeft => PhysicalKey::ShiftLeft,
            KeyCode::ShiftRight => PhysicalKey::ShiftRight,
            KeyCode::ControlLeft => PhysicalKey::ControlLeft,
            KeyCode::ControlRight => PhysicalKey::ControlRight,
            KeyCode::AltLeft => PhysicalKey::AltLeft,
            KeyCode::AltRight => PhysicalKey::AltRight,
            KeyCode::SuperLeft => PhysicalKey::MetaLeft,
            KeyCode::SuperRight => PhysicalKey::MetaRight,

            // Navigation
            KeyCode::ArrowUp => PhysicalKey::ArrowUp,
            KeyCode::ArrowDown => PhysicalKey::ArrowDown,
            KeyCode::ArrowLeft => PhysicalKey::ArrowLeft,
            KeyCode::ArrowRight => PhysicalKey::ArrowRight,
            KeyCode::Home => PhysicalKey::Home,
            KeyCode::End => PhysicalKey::End,
            KeyCode::PageUp => PhysicalKey::PageUp,
            KeyCode::PageDown => PhysicalKey::PageDown,

            // Special keys
            KeyCode::Enter => PhysicalKey::Enter,
            KeyCode::Space => PhysicalKey::Space,
            KeyCode::Tab => PhysicalKey::Tab,
            KeyCode::Backspace => PhysicalKey::Backspace,
            KeyCode::Delete => PhysicalKey::Delete,
            KeyCode::Escape => PhysicalKey::Escape,
            KeyCode::Insert => PhysicalKey::Insert,

            // Lock keys
            KeyCode::CapsLock => PhysicalKey::CapsLock,
            KeyCode::NumLock => PhysicalKey::NumLock,
            KeyCode::ScrollLock => PhysicalKey::ScrollLock,

            // Special system keys
            KeyCode::PrintScreen => PhysicalKey::PrintScreen,
            KeyCode::Pause => PhysicalKey::Pause,

            // Symbols, numpad, and media keys not yet defined in PhysicalKey
            // Map to Unidentified for now - they can be accessed via LogicalKey
            KeyCode::Minus
            | KeyCode::Equal
            | KeyCode::BracketLeft
            | KeyCode::BracketRight
            | KeyCode::Backslash
            | KeyCode::Semicolon
            | KeyCode::Quote
            | KeyCode::Backquote
            | KeyCode::Comma
            | KeyCode::Period
            | KeyCode::Slash
            | KeyCode::Numpad0
            | KeyCode::Numpad1
            | KeyCode::Numpad2
            | KeyCode::Numpad3
            | KeyCode::Numpad4
            | KeyCode::Numpad5
            | KeyCode::Numpad6
            | KeyCode::Numpad7
            | KeyCode::Numpad8
            | KeyCode::Numpad9
            | KeyCode::NumpadAdd
            | KeyCode::NumpadSubtract
            | KeyCode::NumpadMultiply
            | KeyCode::NumpadDivide
            | KeyCode::NumpadDecimal
            | KeyCode::NumpadEnter
            | KeyCode::NumpadEqual
            | KeyCode::AudioVolumeUp
            | KeyCode::AudioVolumeDown
            | KeyCode::AudioVolumeMute => PhysicalKey::Unidentified,

            // Fallback for unmapped keys
            _ => PhysicalKey::Unidentified,
        },
        WinitPhysical::Unidentified(_) => PhysicalKey::Unidentified,
    }
}

/// Convert winit LogicalKey to FLUI LogicalKey
#[inline]
fn convert_logical_key(key: &Key) -> LogicalKey {
    match key {
        Key::Named(named) => {
            // Convert NamedKey to PhysicalKey for consistency
            let physical = match named {
                NamedKey::Enter => PhysicalKey::Enter,
                NamedKey::Tab => PhysicalKey::Tab,
                NamedKey::Space => PhysicalKey::Space,
                NamedKey::ArrowDown => PhysicalKey::ArrowDown,
                NamedKey::ArrowLeft => PhysicalKey::ArrowLeft,
                NamedKey::ArrowRight => PhysicalKey::ArrowRight,
                NamedKey::ArrowUp => PhysicalKey::ArrowUp,
                NamedKey::End => PhysicalKey::End,
                NamedKey::Home => PhysicalKey::Home,
                NamedKey::PageDown => PhysicalKey::PageDown,
                NamedKey::PageUp => PhysicalKey::PageUp,
                NamedKey::Backspace => PhysicalKey::Backspace,
                NamedKey::Delete => PhysicalKey::Delete,
                NamedKey::Insert => PhysicalKey::Insert,
                NamedKey::Escape => PhysicalKey::Escape,
                NamedKey::CapsLock => PhysicalKey::CapsLock,
                NamedKey::NumLock => PhysicalKey::NumLock,
                NamedKey::ScrollLock => PhysicalKey::ScrollLock,
                NamedKey::Shift => PhysicalKey::ShiftLeft,
                NamedKey::Control => PhysicalKey::ControlLeft,
                NamedKey::Alt => PhysicalKey::AltLeft,
                NamedKey::Meta => PhysicalKey::MetaLeft,
                NamedKey::F1 => PhysicalKey::F1,
                NamedKey::F2 => PhysicalKey::F2,
                NamedKey::F3 => PhysicalKey::F3,
                NamedKey::F4 => PhysicalKey::F4,
                NamedKey::F5 => PhysicalKey::F5,
                NamedKey::F6 => PhysicalKey::F6,
                NamedKey::F7 => PhysicalKey::F7,
                NamedKey::F8 => PhysicalKey::F8,
                NamedKey::F9 => PhysicalKey::F9,
                NamedKey::F10 => PhysicalKey::F10,
                NamedKey::F11 => PhysicalKey::F11,
                NamedKey::F12 => PhysicalKey::F12,
                _ => PhysicalKey::Unidentified,
            };
            LogicalKey::Named(physical)
        }
        Key::Character(ch) => LogicalKey::Character(ch.to_string()),
        Key::Unidentified(_) => LogicalKey::Named(PhysicalKey::Unidentified),
        Key::Dead(_) => LogicalKey::Named(PhysicalKey::Unidentified),
    }
}

/// Extract text from winit key event
#[inline]
fn extract_text(text: &Option<SmolStr>) -> Option<String> {
    text.as_ref().map(|s| s.to_string())
}

/// Convert winit ModifiersState to FLUI KeyModifiers
#[inline]
pub fn convert_modifiers(modifiers: ModifiersState) -> KeyModifiers {
    KeyModifiers {
        control: modifiers.control_key(),
        shift: modifiers.shift_key(),
        alt: modifiers.alt_key(),
        meta: modifiers.super_key(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_convert_physical_key() {
        use winit::keyboard::PhysicalKey as WinitPhysical;

        assert_eq!(
            convert_physical_key(WinitPhysical::Code(KeyCode::KeyA)),
            PhysicalKey::KeyA
        );
        assert_eq!(
            convert_physical_key(WinitPhysical::Code(KeyCode::Enter)),
            PhysicalKey::Enter
        );
        assert_eq!(
            convert_physical_key(WinitPhysical::Code(KeyCode::Digit5)),
            PhysicalKey::Digit5
        );
    }

    #[test]
    fn test_convert_logical_key() {
        assert!(matches!(
            convert_logical_key(&Key::Character("a".into())),
            LogicalKey::Character(_)
        ));
        assert!(matches!(
            convert_logical_key(&Key::Named(NamedKey::Enter)),
            LogicalKey::Named(PhysicalKey::Enter)
        ));
    }

    #[test]
    fn test_convert_modifiers() {
        let mut modifiers = ModifiersState::empty();
        modifiers.set(ModifiersState::CONTROL, true);
        modifiers.set(ModifiersState::SHIFT, true);

        let flui_mods = convert_modifiers(modifiers);
        assert!(flui_mods.control);
        assert!(flui_mods.shift);
        assert!(!flui_mods.alt);
        assert!(!flui_mods.meta);
    }
}
