//! Winit event conversion to W3C ui-events
//!
//! Converts winit 0.30 events to W3C-compliant PlatformInput types.

use std::{sync::LazyLock, time::Instant};

use dpi::{PhysicalPosition, PhysicalSize};
use keyboard_types::Modifiers as KeyboardModifiers;
use ui_events::{
    ScrollDelta,
    keyboard::{Code, KeyState, KeyboardEvent, Location},
    pointer::{
        PointerButton, PointerButtonEvent, PointerButtons, PointerEvent, PointerId, PointerInfo,
        PointerOrientation, PointerState, PointerType, PointerUpdate,
    },
};
use winit::event::{ElementState, MouseButton, MouseScrollDelta};

use crate::traits::PlatformInput;

/// Process-start epoch for monotonic event timestamps.
static PROCESS_START: LazyLock<Instant> = LazyLock::new(Instant::now);

/// Get monotonic timestamp in milliseconds since process start.
#[inline]
fn event_timestamp_ms() -> u64 {
    PROCESS_START.elapsed().as_millis() as u64
}

/// Create a `PointerInfo` for the primary mouse pointer.
#[inline]
fn primary_mouse_info() -> PointerInfo {
    PointerInfo {
        pointer_id: Some(PointerId::PRIMARY),
        pointer_type: PointerType::Mouse,
        persistent_device_id: None,
    }
}

/// Build a `PointerState` from position and scale factor.
fn pointer_state(
    position: winit::dpi::PhysicalPosition<f64>,
    scale_factor: f64,
    pressure: f32,
    modifiers: KeyboardModifiers,
) -> PointerState {
    let logical_x = position.x / scale_factor;
    let logical_y = position.y / scale_factor;

    PointerState {
        time: event_timestamp_ms(),
        position: PhysicalPosition::new(logical_x, logical_y),
        buttons: PointerButtons::default(),
        modifiers,
        count: 1,
        contact_geometry: PhysicalSize::new(1.0, 1.0),
        orientation: PointerOrientation::default(),
        pressure,
        tangential_pressure: 0.0,
        scale_factor,
    }
}

/// Convert winit MouseButton to W3C PointerButton
fn convert_mouse_button(button: MouseButton) -> PointerButton {
    match button {
        // `Other` carries a vendor-specific button id ui-events has no slot
        // for; treat it as the primary button like an unrecognized click.
        MouseButton::Left | MouseButton::Other(_) => PointerButton::Primary,
        MouseButton::Right => PointerButton::Secondary,
        MouseButton::Middle => PointerButton::Auxiliary,
        MouseButton::Back => PointerButton::X1,
        MouseButton::Forward => PointerButton::X2,
    }
}

/// Convert winit modifiers state to keyboard-types Modifiers
pub fn convert_modifiers(modifiers: winit::event::Modifiers) -> KeyboardModifiers {
    let state = modifiers.state();
    let mut mods = KeyboardModifiers::empty();

    if state.shift_key() {
        mods |= KeyboardModifiers::SHIFT;
    }
    if state.control_key() {
        mods |= KeyboardModifiers::CONTROL;
    }
    if state.alt_key() {
        mods |= KeyboardModifiers::ALT;
    }
    if state.super_key() {
        mods |= KeyboardModifiers::META;
    }

    mods
}

/// Convert winit CursorMoved to W3C PointerEvent::Move
pub fn cursor_moved_event(
    position: winit::dpi::PhysicalPosition<f64>,
    scale_factor: f64,
    modifiers: KeyboardModifiers,
) -> PlatformInput {
    let state = pointer_state(position, scale_factor, 0.0, modifiers);

    let event = PointerEvent::Move(PointerUpdate {
        pointer: primary_mouse_info(),
        current: state,
        coalesced: Vec::new(),
        predicted: Vec::new(),
    });

    PlatformInput::Pointer(event)
}

/// Convert winit MouseInput to W3C PointerEvent::Down/Up
pub fn mouse_button_event(
    button: MouseButton,
    state: ElementState,
    position: winit::dpi::PhysicalPosition<f64>,
    scale_factor: f64,
    modifiers: KeyboardModifiers,
) -> PlatformInput {
    let is_down = state == ElementState::Pressed;
    let pointer_button = convert_mouse_button(button);
    let pressure = if is_down { 0.5 } else { 0.0 };
    let pointer_state = pointer_state(position, scale_factor, pressure, modifiers);

    let event = if is_down {
        PointerEvent::Down(PointerButtonEvent {
            pointer: primary_mouse_info(),
            state: pointer_state,
            button: Some(pointer_button),
        })
    } else {
        PointerEvent::Up(PointerButtonEvent {
            pointer: primary_mouse_info(),
            state: pointer_state,
            button: Some(pointer_button),
        })
    };

    PlatformInput::Pointer(event)
}

/// Convert winit MouseWheel to W3C PointerEvent::Scroll
pub fn mouse_wheel_event(
    delta: MouseScrollDelta,
    position: winit::dpi::PhysicalPosition<f64>,
    scale_factor: f64,
    modifiers: KeyboardModifiers,
) -> PlatformInput {
    let scroll_delta = match delta {
        MouseScrollDelta::LineDelta(x, y) => ScrollDelta::LineDelta(x, y),
        MouseScrollDelta::PixelDelta(pos) => {
            ScrollDelta::PixelDelta(PhysicalPosition::new(pos.x, pos.y))
        }
    };

    let state = pointer_state(position, scale_factor, 0.0, modifiers);

    let event = PointerEvent::Scroll(ui_events::pointer::PointerScrollEvent {
        pointer: primary_mouse_info(),
        state,
        delta: scroll_delta,
    });

    PlatformInput::Pointer(event)
}

/// Convert winit key to keyboard-types Key
fn convert_winit_key(key: &winit::keyboard::Key) -> keyboard_types::Key {
    use keyboard_types::{Key as K, NamedKey};

    match key {
        winit::keyboard::Key::Named(named) => {
            let nk = match named {
                winit::keyboard::NamedKey::Enter => NamedKey::Enter,
                winit::keyboard::NamedKey::Tab => NamedKey::Tab,
                winit::keyboard::NamedKey::Space => return K::Character(" ".into()),
                winit::keyboard::NamedKey::Backspace => NamedKey::Backspace,
                winit::keyboard::NamedKey::Delete => NamedKey::Delete,
                winit::keyboard::NamedKey::Escape => NamedKey::Escape,
                winit::keyboard::NamedKey::ArrowLeft => NamedKey::ArrowLeft,
                winit::keyboard::NamedKey::ArrowRight => NamedKey::ArrowRight,
                winit::keyboard::NamedKey::ArrowUp => NamedKey::ArrowUp,
                winit::keyboard::NamedKey::ArrowDown => NamedKey::ArrowDown,
                winit::keyboard::NamedKey::Home => NamedKey::Home,
                winit::keyboard::NamedKey::End => NamedKey::End,
                winit::keyboard::NamedKey::PageUp => NamedKey::PageUp,
                winit::keyboard::NamedKey::PageDown => NamedKey::PageDown,
                winit::keyboard::NamedKey::Insert => NamedKey::Insert,
                winit::keyboard::NamedKey::Shift => NamedKey::Shift,
                winit::keyboard::NamedKey::Control => NamedKey::Control,
                winit::keyboard::NamedKey::Alt => NamedKey::Alt,
                winit::keyboard::NamedKey::Super => NamedKey::Meta,
                winit::keyboard::NamedKey::F1 => NamedKey::F1,
                winit::keyboard::NamedKey::F2 => NamedKey::F2,
                winit::keyboard::NamedKey::F3 => NamedKey::F3,
                winit::keyboard::NamedKey::F4 => NamedKey::F4,
                winit::keyboard::NamedKey::F5 => NamedKey::F5,
                winit::keyboard::NamedKey::F6 => NamedKey::F6,
                winit::keyboard::NamedKey::F7 => NamedKey::F7,
                winit::keyboard::NamedKey::F8 => NamedKey::F8,
                winit::keyboard::NamedKey::F9 => NamedKey::F9,
                winit::keyboard::NamedKey::F10 => NamedKey::F10,
                winit::keyboard::NamedKey::F11 => NamedKey::F11,
                winit::keyboard::NamedKey::F12 => NamedKey::F12,
                winit::keyboard::NamedKey::CapsLock => NamedKey::CapsLock,
                winit::keyboard::NamedKey::NumLock => NamedKey::NumLock,
                winit::keyboard::NamedKey::ScrollLock => NamedKey::ScrollLock,
                winit::keyboard::NamedKey::PrintScreen => NamedKey::PrintScreen,
                winit::keyboard::NamedKey::Pause => NamedKey::Pause,
                winit::keyboard::NamedKey::ContextMenu => NamedKey::ContextMenu,
                _ => NamedKey::Unidentified,
            };
            K::Named(nk)
        }
        winit::keyboard::Key::Character(c) => K::Character(c.to_string()),
        winit::keyboard::Key::Dead(_) | winit::keyboard::Key::Unidentified(_) => {
            K::Named(keyboard_types::NamedKey::Unidentified)
        }
    }
}

/// Convert winit PhysicalKey to ui-events Code
fn convert_physical_key(key: winit::keyboard::PhysicalKey) -> Code {
    use winit::keyboard::{KeyCode, PhysicalKey};

    match key {
        PhysicalKey::Code(code) => match code {
            KeyCode::KeyA => Code::KeyA,
            KeyCode::KeyB => Code::KeyB,
            KeyCode::KeyC => Code::KeyC,
            KeyCode::KeyD => Code::KeyD,
            KeyCode::KeyE => Code::KeyE,
            KeyCode::KeyF => Code::KeyF,
            KeyCode::KeyG => Code::KeyG,
            KeyCode::KeyH => Code::KeyH,
            KeyCode::KeyI => Code::KeyI,
            KeyCode::KeyJ => Code::KeyJ,
            KeyCode::KeyK => Code::KeyK,
            KeyCode::KeyL => Code::KeyL,
            KeyCode::KeyM => Code::KeyM,
            KeyCode::KeyN => Code::KeyN,
            KeyCode::KeyO => Code::KeyO,
            KeyCode::KeyP => Code::KeyP,
            KeyCode::KeyQ => Code::KeyQ,
            KeyCode::KeyR => Code::KeyR,
            KeyCode::KeyS => Code::KeyS,
            KeyCode::KeyT => Code::KeyT,
            KeyCode::KeyU => Code::KeyU,
            KeyCode::KeyV => Code::KeyV,
            KeyCode::KeyW => Code::KeyW,
            KeyCode::KeyX => Code::KeyX,
            KeyCode::KeyY => Code::KeyY,
            KeyCode::KeyZ => Code::KeyZ,
            KeyCode::Digit0 => Code::Digit0,
            KeyCode::Digit1 => Code::Digit1,
            KeyCode::Digit2 => Code::Digit2,
            KeyCode::Digit3 => Code::Digit3,
            KeyCode::Digit4 => Code::Digit4,
            KeyCode::Digit5 => Code::Digit5,
            KeyCode::Digit6 => Code::Digit6,
            KeyCode::Digit7 => Code::Digit7,
            KeyCode::Digit8 => Code::Digit8,
            KeyCode::Digit9 => Code::Digit9,
            KeyCode::Enter => Code::Enter,
            KeyCode::Escape => Code::Escape,
            KeyCode::Backspace => Code::Backspace,
            KeyCode::Tab => Code::Tab,
            KeyCode::Space => Code::Space,
            KeyCode::ShiftLeft => Code::ShiftLeft,
            KeyCode::ShiftRight => Code::ShiftRight,
            KeyCode::ControlLeft => Code::ControlLeft,
            KeyCode::ControlRight => Code::ControlRight,
            KeyCode::AltLeft => Code::AltLeft,
            KeyCode::AltRight => Code::AltRight,
            KeyCode::SuperLeft => Code::MetaLeft,
            KeyCode::SuperRight => Code::MetaRight,
            KeyCode::ArrowLeft => Code::ArrowLeft,
            KeyCode::ArrowRight => Code::ArrowRight,
            KeyCode::ArrowUp => Code::ArrowUp,
            KeyCode::ArrowDown => Code::ArrowDown,
            KeyCode::Home => Code::Home,
            KeyCode::End => Code::End,
            KeyCode::PageUp => Code::PageUp,
            KeyCode::PageDown => Code::PageDown,
            KeyCode::Insert => Code::Insert,
            KeyCode::Delete => Code::Delete,
            KeyCode::F1 => Code::F1,
            KeyCode::F2 => Code::F2,
            KeyCode::F3 => Code::F3,
            KeyCode::F4 => Code::F4,
            KeyCode::F5 => Code::F5,
            KeyCode::F6 => Code::F6,
            KeyCode::F7 => Code::F7,
            KeyCode::F8 => Code::F8,
            KeyCode::F9 => Code::F9,
            KeyCode::F10 => Code::F10,
            KeyCode::F11 => Code::F11,
            KeyCode::F12 => Code::F12,
            _ => Code::Unidentified,
        },
        PhysicalKey::Unidentified(_) => Code::Unidentified,
    }
}

/// Convert winit key location to ui-events Location
fn convert_location(key: winit::keyboard::PhysicalKey) -> Location {
    use winit::keyboard::{KeyCode, PhysicalKey};

    match key {
        PhysicalKey::Code(code) => match code {
            KeyCode::ShiftLeft | KeyCode::ControlLeft | KeyCode::AltLeft | KeyCode::SuperLeft => {
                Location::Left
            }
            KeyCode::ShiftRight
            | KeyCode::ControlRight
            | KeyCode::AltRight
            | KeyCode::SuperRight => Location::Right,
            KeyCode::Numpad0
            | KeyCode::Numpad1
            | KeyCode::Numpad2
            | KeyCode::Numpad3
            | KeyCode::Numpad4
            | KeyCode::Numpad5
            | KeyCode::Numpad6
            | KeyCode::Numpad7
            | KeyCode::Numpad8
            | KeyCode::Numpad9
            | KeyCode::NumpadEnter
            | KeyCode::NumpadAdd
            | KeyCode::NumpadSubtract
            | KeyCode::NumpadMultiply
            | KeyCode::NumpadDivide
            | KeyCode::NumpadDecimal => Location::Numpad,
            _ => Location::Standard,
        },
        PhysicalKey::Unidentified(_) => Location::Standard,
    }
}

/// Convert winit's `Ime` event to [`flui_types::ImeEvent`].
///
/// A pure, unit-tested mapping: winit's `Ime` enum is already
/// [`flui_types::ImeEvent`]'s reference shape (see that type's module doc),
/// so this is a direct variant-for-variant translation with no coordinate
/// or encoding conversion.
pub fn ime_event(event: &winit::event::Ime) -> PlatformInput {
    use winit::event::Ime;

    let ime_event = match event {
        Ime::Enabled => flui_types::ImeEvent::Enabled,
        Ime::Preedit(text, cursor) => flui_types::ImeEvent::Preedit {
            text: text.clone(),
            cursor: *cursor,
        },
        Ime::Commit(text) => flui_types::ImeEvent::Commit(text.clone()),
        Ime::Disabled => flui_types::ImeEvent::Disabled,
    };

    PlatformInput::Ime(ime_event)
}

/// Convert winit KeyboardInput to W3C KeyboardEvent
pub fn keyboard_event(
    event: &winit::event::KeyEvent,
    modifiers: KeyboardModifiers,
) -> PlatformInput {
    let key = convert_winit_key(&event.logical_key);
    let code = convert_physical_key(event.physical_key);
    let location = convert_location(event.physical_key);
    let state = match event.state {
        ElementState::Pressed => KeyState::Down,
        ElementState::Released => KeyState::Up,
    };

    let keyboard_event = KeyboardEvent {
        state,
        key,
        code,
        location,
        modifiers,
        repeat: event.repeat,
        is_composing: false,
    };

    PlatformInput::Keyboard(keyboard_event)
}

#[cfg(test)]
mod ime_tests {
    use flui_types::ImeEvent;
    use winit::event::Ime;

    use super::ime_event;
    use crate::traits::PlatformInput;

    /// Unwraps the `PlatformInput::Ime` arm `ime_event` always produces,
    /// asserting the wrapping variant at the same time so a future change
    /// that wraps IME events in a different `PlatformInput` variant fails
    /// loudly here instead of silently changing what these tests check.
    fn convert(event: &Ime) -> ImeEvent {
        match ime_event(event) {
            PlatformInput::Ime(inner) => inner,
            other => panic!("ime_event must return PlatformInput::Ime, got {other:?}"),
        }
    }

    #[test]
    fn enabled_maps_to_enabled() {
        assert_eq!(convert(&Ime::Enabled), ImeEvent::Enabled);
    }

    #[test]
    fn disabled_maps_to_disabled() {
        assert_eq!(convert(&Ime::Disabled), ImeEvent::Disabled);
    }

    #[test]
    fn commit_carries_the_delivered_text() {
        assert_eq!(
            convert(&Ime::Commit("hello".to_string())),
            ImeEvent::Commit("hello".to_string())
        );
    }

    #[test]
    fn preedit_with_a_cursor_position_is_preserved() {
        assert_eq!(
            convert(&Ime::Preedit("ni".to_string(), Some((1, 2)))),
            ImeEvent::Preedit {
                text: "ni".to_string(),
                cursor: Some((1, 2)),
            }
        );
    }

    #[test]
    fn preedit_with_no_cursor_hides_the_caret() {
        assert_eq!(
            convert(&Ime::Preedit("ni".to_string(), None)),
            ImeEvent::Preedit {
                text: "ni".to_string(),
                cursor: None,
            }
        );
    }
}
