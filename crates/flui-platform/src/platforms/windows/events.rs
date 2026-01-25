//! Windows event conversion to PlatformInput

use std::time::Instant;
use windows::Win32::Foundation::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;

use super::util::*;
use crate::traits::{
    KeyCode, KeyDownEvent, KeyUpEvent, LogicalKey, Modifiers, MouseButton, NamedKey, PlatformInput,
    PointerEvent, PointerKind, PointerPhase, ScrollDelta, ScrollPhase, ScrollWheelEvent,
};
use flui_types::geometry::{px, Point};

/// Convert VK_* to KeyCode
pub fn vk_to_keycode(vk: VIRTUAL_KEY) -> KeyCode {
    match vk {
        VK_A => KeyCode::KeyA,
        VK_B => KeyCode::KeyB,
        VK_C => KeyCode::KeyC,
        VK_D => KeyCode::KeyD,
        VK_E => KeyCode::KeyE,
        VK_F => KeyCode::KeyF,
        VK_G => KeyCode::KeyG,
        VK_H => KeyCode::KeyH,
        VK_I => KeyCode::KeyI,
        VK_J => KeyCode::KeyJ,
        VK_K => KeyCode::KeyK,
        VK_L => KeyCode::KeyL,
        VK_M => KeyCode::KeyM,
        VK_N => KeyCode::KeyN,
        VK_O => KeyCode::KeyO,
        VK_P => KeyCode::KeyP,
        VK_Q => KeyCode::KeyQ,
        VK_R => KeyCode::KeyR,
        VK_S => KeyCode::KeyS,
        VK_T => KeyCode::KeyT,
        VK_U => KeyCode::KeyU,
        VK_V => KeyCode::KeyV,
        VK_W => KeyCode::KeyW,
        VK_X => KeyCode::KeyX,
        VK_Y => KeyCode::KeyY,
        VK_Z => KeyCode::KeyZ,

        VK_0 => KeyCode::Digit0,
        VK_1 => KeyCode::Digit1,
        VK_2 => KeyCode::Digit2,
        VK_3 => KeyCode::Digit3,
        VK_4 => KeyCode::Digit4,
        VK_5 => KeyCode::Digit5,
        VK_6 => KeyCode::Digit6,
        VK_7 => KeyCode::Digit7,
        VK_8 => KeyCode::Digit8,
        VK_9 => KeyCode::Digit9,

        VK_F1 => KeyCode::F1,
        VK_F2 => KeyCode::F2,
        VK_F3 => KeyCode::F3,
        VK_F4 => KeyCode::F4,
        VK_F5 => KeyCode::F5,
        VK_F6 => KeyCode::F6,
        VK_F7 => KeyCode::F7,
        VK_F8 => KeyCode::F8,
        VK_F9 => KeyCode::F9,
        VK_F10 => KeyCode::F10,
        VK_F11 => KeyCode::F11,
        VK_F12 => KeyCode::F12,

        VK_LEFT => KeyCode::ArrowLeft,
        VK_RIGHT => KeyCode::ArrowRight,
        VK_UP => KeyCode::ArrowUp,
        VK_DOWN => KeyCode::ArrowDown,

        VK_HOME => KeyCode::Home,
        VK_END => KeyCode::End,
        VK_PRIOR => KeyCode::PageUp,
        VK_NEXT => KeyCode::PageDown,

        VK_BACK => KeyCode::Backspace,
        VK_DELETE => KeyCode::Delete,
        VK_RETURN => KeyCode::Enter,
        VK_TAB => KeyCode::Tab,
        VK_SPACE => KeyCode::Space,
        VK_ESCAPE => KeyCode::Escape,

        VK_LSHIFT => KeyCode::ShiftLeft,
        VK_RSHIFT => KeyCode::ShiftRight,
        VK_LCONTROL => KeyCode::ControlLeft,
        VK_RCONTROL => KeyCode::ControlRight,
        VK_LMENU => KeyCode::AltLeft,
        VK_RMENU => KeyCode::AltRight,
        VK_LWIN => KeyCode::MetaLeft,
        VK_RWIN => KeyCode::MetaRight,

        VK_CAPITAL => KeyCode::CapsLock,
        VK_NUMLOCK => KeyCode::NumLock,
        VK_SCROLL => KeyCode::ScrollLock,
        VK_SNAPSHOT => KeyCode::PrintScreen,
        VK_PAUSE => KeyCode::Pause,
        VK_INSERT => KeyCode::Insert,

        _ => KeyCode::Unknown,
    }
}

/// Convert VK_* to LogicalKey
pub fn vk_to_logical_key(vk: VIRTUAL_KEY, _scan_code: u16) -> LogicalKey {
    // TODO: Use keyboard layout to get character
    match vk {
        VK_RETURN => LogicalKey::Named(NamedKey::Enter),
        VK_TAB => LogicalKey::Named(NamedKey::Tab),
        VK_SPACE => LogicalKey::Named(NamedKey::Space),
        VK_BACK => LogicalKey::Named(NamedKey::Backspace),
        VK_DELETE => LogicalKey::Named(NamedKey::Delete),
        VK_ESCAPE => LogicalKey::Named(NamedKey::Escape),

        VK_LEFT => LogicalKey::Named(NamedKey::ArrowLeft),
        VK_RIGHT => LogicalKey::Named(NamedKey::ArrowRight),
        VK_UP => LogicalKey::Named(NamedKey::ArrowUp),
        VK_DOWN => LogicalKey::Named(NamedKey::ArrowDown),

        VK_HOME => LogicalKey::Named(NamedKey::Home),
        VK_END => LogicalKey::Named(NamedKey::End),
        VK_PRIOR => LogicalKey::Named(NamedKey::PageUp),
        VK_NEXT => LogicalKey::Named(NamedKey::PageDown),
        VK_INSERT => LogicalKey::Named(NamedKey::Insert),

        VK_F1 => LogicalKey::Named(NamedKey::F1),
        VK_F2 => LogicalKey::Named(NamedKey::F2),
        VK_F3 => LogicalKey::Named(NamedKey::F3),
        VK_F4 => LogicalKey::Named(NamedKey::F4),
        VK_F5 => LogicalKey::Named(NamedKey::F5),
        VK_F6 => LogicalKey::Named(NamedKey::F6),
        VK_F7 => LogicalKey::Named(NamedKey::F7),
        VK_F8 => LogicalKey::Named(NamedKey::F8),
        VK_F9 => LogicalKey::Named(NamedKey::F9),
        VK_F10 => LogicalKey::Named(NamedKey::F10),
        VK_F11 => LogicalKey::Named(NamedKey::F11),
        VK_F12 => LogicalKey::Named(NamedKey::F12),

        // TODO: Get character from keyboard layout
        _ => LogicalKey::Character("".to_string()),
    }
}

/// Get current modifiers state
pub unsafe fn get_modifiers() -> Modifiers {
    Modifiers {
        shift: is_key_pressed(VK_SHIFT.0 as i32),
        control: is_key_pressed(VK_CONTROL.0 as i32),
        alt: is_key_pressed(VK_MENU.0 as i32),
        meta: is_key_pressed(VK_LWIN.0 as i32) || is_key_pressed(VK_RWIN.0 as i32),
    }
}

/// Convert WM_LBUTTONDOWN/UP to PointerEvent
pub fn mouse_button_event(
    button: MouseButton,
    phase: PointerPhase,
    lparam: LPARAM,
    scale_factor: f32,
) -> PlatformInput {
    let x = get_x_lparam(lparam);
    let y = get_y_lparam(lparam);

    let modifiers = unsafe { get_modifiers() };

    let event = PointerEvent {
        pointer_id: 0, // Mouse is always ID 0
        device_id: 0,
        kind: PointerKind::Mouse(button),
        position: Point::new(
            px(device_to_logical(x, scale_factor)),
            px(device_to_logical(y, scale_factor)),
        ),
        delta: Point::new(px(0.0), px(0.0)), // TODO: Calculate from previous position
        modifiers,
        phase,
        timestamp: Instant::now(),
        click_count: 1, // TODO: Detect double-click
        pressure: None,
        tilt: None,
    };

    PlatformInput::Pointer(event)
}

/// Convert WM_MOUSEMOVE to PointerEvent
pub fn mouse_move_event(
    lparam: LPARAM,
    scale_factor: f32,
    current_button: Option<MouseButton>,
) -> PlatformInput {
    let x = get_x_lparam(lparam);
    let y = get_y_lparam(lparam);

    let modifiers = unsafe { get_modifiers() };

    let kind = if let Some(button) = current_button {
        PointerKind::Mouse(button)
    } else {
        PointerKind::Mouse(MouseButton::Left) // Default when hovering
    };

    let event = PointerEvent {
        pointer_id: 0,
        device_id: 0,
        kind,
        position: Point::new(
            px(device_to_logical(x, scale_factor)),
            px(device_to_logical(y, scale_factor)),
        ),
        delta: Point::new(px(0.0), px(0.0)), // TODO: Calculate from previous position
        modifiers,
        phase: PointerPhase::Move,
        timestamp: Instant::now(),
        click_count: 0,
        pressure: None,
        tilt: None,
    };

    PlatformInput::Pointer(event)
}

/// Convert WM_MOUSEWHEEL to ScrollWheelEvent
pub fn mouse_wheel_event(wparam: WPARAM, lparam: LPARAM, scale_factor: f32) -> PlatformInput {
    // Get wheel delta (HIWORD of wparam)
    let delta = ((wparam.0 as i32) >> 16) as i16 as f32;
    let lines = delta / 120.0; // WHEEL_DELTA = 120

    let x = get_x_lparam(lparam);
    let y = get_y_lparam(lparam);

    let modifiers = unsafe { get_modifiers() };

    let event = ScrollWheelEvent {
        position: Point::new(
            px(device_to_logical(x, scale_factor)),
            px(device_to_logical(y, scale_factor)),
        ),
        delta: ScrollDelta::Lines { x: 0.0, y: lines },
        modifiers,
        phase: ScrollPhase::Changed,
    };

    PlatformInput::ScrollWheel(event)
}

/// Convert WM_KEYDOWN to KeyDownEvent
pub fn key_down_event(wparam: WPARAM, lparam: LPARAM) -> PlatformInput {
    let vk = VIRTUAL_KEY(wparam.0 as u16);
    let scan_code = ((lparam.0 >> 16) & 0xFF) as u16;
    let is_repeat = (lparam.0 & (1 << 30)) != 0;

    let modifiers = unsafe { get_modifiers() };

    let event = KeyDownEvent {
        key_code: vk_to_keycode(vk),
        logical_key: vk_to_logical_key(vk, scan_code),
        text: None, // WM_CHAR provides text
        modifiers,
        is_repeat,
    };

    PlatformInput::KeyDown(event)
}

/// Convert WM_KEYUP to KeyUpEvent
pub fn key_up_event(wparam: WPARAM, lparam: LPARAM) -> PlatformInput {
    let vk = VIRTUAL_KEY(wparam.0 as u16);
    let scan_code = ((lparam.0 >> 16) & 0xFF) as u16;

    let modifiers = unsafe { get_modifiers() };

    let event = KeyUpEvent {
        key_code: vk_to_keycode(vk),
        logical_key: vk_to_logical_key(vk, scan_code),
        modifiers,
    };

    PlatformInput::KeyUp(event)
}

/// Convert WM_CHAR to text for KeyDownEvent
pub fn char_to_text(wparam: WPARAM) -> String {
    let code_point = wparam.0 as u32;
    if let Some(c) = char::from_u32(code_point) {
        c.to_string()
    } else {
        String::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vk_to_keycode() {
        assert_eq!(vk_to_keycode(VK_A), KeyCode::KeyA);
        assert_eq!(vk_to_keycode(VK_RETURN), KeyCode::Enter);
        assert_eq!(vk_to_keycode(VK_LEFT), KeyCode::ArrowLeft);
        assert_eq!(vk_to_keycode(VK_F1), KeyCode::F1);
    }

    #[test]
    fn test_mouse_button_event() {
        let lparam = LPARAM(((200 << 16) | 100) as isize);
        let event = mouse_button_event(MouseButton::Left, PointerPhase::Down, lparam, 1.0);

        if let PlatformInput::Pointer(ptr) = event {
            assert_eq!(ptr.position.x.0, 100.0);
            assert_eq!(ptr.position.y.0, 200.0);
            assert_eq!(ptr.phase, PointerPhase::Down);
            assert!(matches!(ptr.kind, PointerKind::Mouse(MouseButton::Left)));
        } else {
            panic!("Expected PointerEvent");
        }
    }
}
