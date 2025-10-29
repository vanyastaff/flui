//! Event translation from winit to flui events

use flui_types::{
    Event, KeyEvent, KeyEventData, KeyModifiers, LogicalKey, Offset, PhysicalKey, PointerButton,
    PointerDeviceKind, PointerEvent, PointerEventData, ScrollDelta, ScrollEventData,
    WindowEvent as FluiWindowEvent,
};
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::keyboard::{KeyCode, ModifiersState, PhysicalKey as WinitPhysicalKey};

/// Translate winit WindowEvent to flui Event
pub fn translate_window_event(event: &WindowEvent, modifiers: &ModifiersState) -> Option<Event> {
    match event {
        WindowEvent::CursorMoved { position, .. } => {
            let data = PointerEventData::new(
                Offset::new(position.x as f32, position.y as f32),
                PointerDeviceKind::Mouse,
            );
            Some(Event::Pointer(PointerEvent::Move(data)))
        }

        WindowEvent::MouseInput { state, button, .. } => {
            // Position will be set by cursor tracking
            let data = PointerEventData::new(Offset::ZERO, PointerDeviceKind::Mouse)
                .with_button(translate_mouse_button(*button));

            let pointer_event = match state {
                ElementState::Pressed => PointerEvent::Down(data),
                ElementState::Released => PointerEvent::Up(data),
            };

            Some(Event::Pointer(pointer_event))
        }

        WindowEvent::MouseWheel { delta, .. } => {
            let scroll_delta = match delta {
                MouseScrollDelta::LineDelta(x, y) => ScrollDelta::Lines { x: *x, y: *y },
                MouseScrollDelta::PixelDelta(pos) => ScrollDelta::Pixels {
                    x: pos.x as f32,
                    y: pos.y as f32,
                },
            };

            let scroll_data = ScrollEventData {
                position: Offset::ZERO, // Will be filled by cursor tracking
                delta: scroll_delta,
                modifiers: translate_modifiers(modifiers),
            };

            Some(Event::Scroll(scroll_data))
        }

        WindowEvent::KeyboardInput {
            event: key_event, ..
        } => {
            let physical_key = translate_physical_key(&key_event.physical_key);
            let logical_key = if let Some(text) = &key_event.text {
                LogicalKey::Character(text.to_string())
            } else {
                LogicalKey::Named(physical_key)
            };

            let mut key_data = KeyEventData::new(physical_key, logical_key)
                .with_modifiers(translate_modifiers(modifiers));

            key_data.repeat = key_event.repeat;

            if let Some(text) = &key_event.text {
                key_data.text = Some(text.to_string());
            }

            let flui_key_event = match key_event.state {
                ElementState::Pressed => KeyEvent::Down(key_data),
                ElementState::Released => KeyEvent::Up(key_data),
            };

            Some(Event::Key(flui_key_event))
        }

        WindowEvent::Resized(size) => Some(Event::Window(FluiWindowEvent::Resized {
            width: size.width,
            height: size.height,
        })),

        WindowEvent::Focused(focused) => {
            if *focused {
                Some(Event::Window(FluiWindowEvent::Focused))
            } else {
                Some(Event::Window(FluiWindowEvent::Unfocused))
            }
        }

        WindowEvent::CloseRequested => Some(Event::Window(FluiWindowEvent::CloseRequested)),

        WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
            Some(Event::Window(FluiWindowEvent::ScaleFactorChanged {
                scale_factor: *scale_factor,
            }))
        }

        _ => None, // Ignore other events
    }
}

/// Translate winit mouse button to flui pointer button
fn translate_mouse_button(button: MouseButton) -> PointerButton {
    match button {
        MouseButton::Left => PointerButton::Primary,
        MouseButton::Right => PointerButton::Secondary,
        MouseButton::Middle => PointerButton::Middle,
        MouseButton::Back => PointerButton::Other(4),
        MouseButton::Forward => PointerButton::Other(5),
        MouseButton::Other(id) => PointerButton::Other(id as u8),
    }
}

/// Translate winit modifiers to flui modifiers
fn translate_modifiers(modifiers: &ModifiersState) -> KeyModifiers {
    KeyModifiers {
        shift: modifiers.shift_key(),
        control: modifiers.control_key(),
        alt: modifiers.alt_key(),
        meta: modifiers.super_key(),
    }
}

/// Translate winit physical key to flui physical key
fn translate_physical_key(key: &WinitPhysicalKey) -> PhysicalKey {
    match key {
        WinitPhysicalKey::Code(code) => translate_key_code(*code),
        WinitPhysicalKey::Unidentified(_) => PhysicalKey::Unidentified,
    }
}

/// Translate winit KeyCode to flui PhysicalKey
fn translate_key_code(code: KeyCode) -> PhysicalKey {
    match code {
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

        // Navigation
        KeyCode::ArrowUp => PhysicalKey::ArrowUp,
        KeyCode::ArrowDown => PhysicalKey::ArrowDown,
        KeyCode::ArrowLeft => PhysicalKey::ArrowLeft,
        KeyCode::ArrowRight => PhysicalKey::ArrowRight,
        KeyCode::Home => PhysicalKey::Home,
        KeyCode::End => PhysicalKey::End,
        KeyCode::PageUp => PhysicalKey::PageUp,
        KeyCode::PageDown => PhysicalKey::PageDown,

        // Editing
        KeyCode::Backspace => PhysicalKey::Backspace,
        KeyCode::Delete => PhysicalKey::Delete,
        KeyCode::Insert => PhysicalKey::Insert,
        KeyCode::Enter => PhysicalKey::Enter,
        KeyCode::Tab => PhysicalKey::Tab,
        KeyCode::Escape => PhysicalKey::Escape,
        KeyCode::Space => PhysicalKey::Space,

        // Modifiers
        KeyCode::ShiftLeft => PhysicalKey::ShiftLeft,
        KeyCode::ShiftRight => PhysicalKey::ShiftRight,
        KeyCode::ControlLeft => PhysicalKey::ControlLeft,
        KeyCode::ControlRight => PhysicalKey::ControlRight,
        KeyCode::AltLeft => PhysicalKey::AltLeft,
        KeyCode::AltRight => PhysicalKey::AltRight,
        KeyCode::SuperLeft => PhysicalKey::MetaLeft,
        KeyCode::SuperRight => PhysicalKey::MetaRight,

        // Lock keys
        KeyCode::CapsLock => PhysicalKey::CapsLock,
        KeyCode::NumLock => PhysicalKey::NumLock,
        KeyCode::ScrollLock => PhysicalKey::ScrollLock,

        // Other
        KeyCode::PrintScreen => PhysicalKey::PrintScreen,
        KeyCode::Pause => PhysicalKey::Pause,

        _ => PhysicalKey::Unidentified,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_translate_mouse_button() {
        assert_eq!(
            translate_mouse_button(MouseButton::Left),
            PointerButton::Primary
        );
        assert_eq!(
            translate_mouse_button(MouseButton::Right),
            PointerButton::Secondary
        );
        assert_eq!(
            translate_mouse_button(MouseButton::Middle),
            PointerButton::Middle
        );
    }

    #[test]
    fn test_translate_modifiers() {
        let mut modifiers = ModifiersState::empty();
        let flui_mods = translate_modifiers(&modifiers);
        assert!(flui_mods.is_empty());

        modifiers.insert(ModifiersState::SHIFT);
        let flui_mods = translate_modifiers(&modifiers);
        assert!(flui_mods.shift);
    }

    #[test]
    fn test_translate_key_code() {
        assert_eq!(translate_key_code(KeyCode::KeyA), PhysicalKey::KeyA);
        assert_eq!(translate_key_code(KeyCode::Enter), PhysicalKey::Enter);
        assert_eq!(translate_key_code(KeyCode::Escape), PhysicalKey::Escape);
    }
}
