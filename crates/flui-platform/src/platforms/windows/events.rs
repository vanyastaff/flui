//! Windows event conversion to W3C ui-events (0.3 API)
//!
//! Converts Win32 messages to W3C-compliant PointerEvent using ui-events 0.3.

use std::time::Instant;
use windows::Win32::Foundation::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;

use super::util::*;
use crate::traits::{
    device_to_logical, offset_from_coords, Key, KeyboardEvent, Modifiers, PlatformInput, ScrollDelta,
};
use dpi::{PhysicalPosition, PhysicalSize};
use keyboard_types::Modifiers as KeyboardModifiers;
use ui_events::pointer::{
    PointerButton, PointerButtonEvent, PointerEvent, PointerId, PointerInfo, PointerState,
    PointerType, PointerUpdate,
};

// ============================================================================
// Keyboard Event Conversion
// ============================================================================

/// Convert VK_* to keyboard-types Key
fn vk_to_key(vk: VIRTUAL_KEY, _scan_code: u16) -> Key {
    use keyboard_types::{Key as K, NamedKey};

    match vk {
        // Named keys
        VK_RETURN => K::Named(NamedKey::Enter),
        VK_TAB => K::Named(NamedKey::Tab),
        VK_SPACE => K::Character(" ".into()),
        VK_BACK => K::Named(NamedKey::Backspace),
        VK_DELETE => K::Named(NamedKey::Delete),
        VK_ESCAPE => K::Named(NamedKey::Escape),

        VK_LEFT => K::Named(NamedKey::ArrowLeft),
        VK_RIGHT => K::Named(NamedKey::ArrowRight),
        VK_UP => K::Named(NamedKey::ArrowUp),
        VK_DOWN => K::Named(NamedKey::ArrowDown),

        VK_HOME => K::Named(NamedKey::Home),
        VK_END => K::Named(NamedKey::End),
        VK_PRIOR => K::Named(NamedKey::PageUp),
        VK_NEXT => K::Named(NamedKey::PageDown),
        VK_INSERT => K::Named(NamedKey::Insert),

        VK_F1 => K::Named(NamedKey::F1),
        VK_F2 => K::Named(NamedKey::F2),
        VK_F3 => K::Named(NamedKey::F3),
        VK_F4 => K::Named(NamedKey::F4),
        VK_F5 => K::Named(NamedKey::F5),
        VK_F6 => K::Named(NamedKey::F6),
        VK_F7 => K::Named(NamedKey::F7),
        VK_F8 => K::Named(NamedKey::F8),
        VK_F9 => K::Named(NamedKey::F9),
        VK_F10 => K::Named(NamedKey::F10),
        VK_F11 => K::Named(NamedKey::F11),
        VK_F12 => K::Named(NamedKey::F12),

        // Modifiers
        VK_LSHIFT | VK_RSHIFT => K::Named(NamedKey::Shift),
        VK_LCONTROL | VK_RCONTROL => K::Named(NamedKey::Control),
        VK_LMENU | VK_RMENU => K::Named(NamedKey::Alt),
        VK_LWIN | VK_RWIN => K::Named(NamedKey::Meta),

        // Letters
        VK_A => K::Character("a".into()),
        VK_B => K::Character("b".into()),
        VK_C => K::Character("c".into()),
        VK_D => K::Character("d".into()),
        VK_E => K::Character("e".into()),
        VK_F => K::Character("f".into()),
        VK_G => K::Character("g".into()),
        VK_H => K::Character("h".into()),
        VK_I => K::Character("i".into()),
        VK_J => K::Character("j".into()),
        VK_K => K::Character("k".into()),
        VK_L => K::Character("l".into()),
        VK_M => K::Character("m".into()),
        VK_N => K::Character("n".into()),
        VK_O => K::Character("o".into()),
        VK_P => K::Character("p".into()),
        VK_Q => K::Character("q".into()),
        VK_R => K::Character("r".into()),
        VK_S => K::Character("s".into()),
        VK_T => K::Character("t".into()),
        VK_U => K::Character("u".into()),
        VK_V => K::Character("v".into()),
        VK_W => K::Character("w".into()),
        VK_X => K::Character("x".into()),
        VK_Y => K::Character("y".into()),
        VK_Z => K::Character("z".into()),

        // Numbers
        VK_0 => K::Character("0".into()),
        VK_1 => K::Character("1".into()),
        VK_2 => K::Character("2".into()),
        VK_3 => K::Character("3".into()),
        VK_4 => K::Character("4".into()),
        VK_5 => K::Character("5".into()),
        VK_6 => K::Character("6".into()),
        VK_7 => K::Character("7".into()),
        VK_8 => K::Character("8".into()),
        VK_9 => K::Character("9".into()),

        _ => K::Named(NamedKey::Unidentified),
    }
}

/// Get current modifiers state
unsafe fn get_modifiers() -> KeyboardModifiers {
    let mut mods = KeyboardModifiers::empty();

    if is_key_pressed(VK_SHIFT.0 as i32) {
        mods |= KeyboardModifiers::SHIFT;
    }
    if is_key_pressed(VK_CONTROL.0 as i32) {
        mods |= KeyboardModifiers::CONTROL;
    }
    if is_key_pressed(VK_MENU.0 as i32) {
        mods |= KeyboardModifiers::ALT;
    }
    if is_key_pressed(VK_LWIN.0 as i32) || is_key_pressed(VK_RWIN.0 as i32) {
        mods |= KeyboardModifiers::META;
    }

    mods
}

// ============================================================================
// Pointer Event Conversion (W3C ui-events 0.3 API)
// ============================================================================

/// Convert WM_LBUTTONDOWN/UP to W3C PointerEvent
pub fn mouse_button_event(
    button: PointerButton,
    is_down: bool,
    lparam: LPARAM,
    scale_factor: f32,
) -> PlatformInput {
    let x = get_x_lparam(lparam);
    let y = get_y_lparam(lparam);

    let modifiers = unsafe { get_modifiers() };

    // Convert to logical pixels then to physical for ui-events
    let logical_x = device_to_logical(x as f32, scale_factor);
    let logical_y = device_to_logical(y as f32, scale_factor);

    let pointer_info = PointerInfo {
        pointer_id: Some(PointerId::PRIMARY),
        pointer_type: PointerType::Mouse,
        persistent_device_id: None,
    };

    let state = PointerState {
        time: Instant::now().elapsed().as_millis() as u64,
        position: PhysicalPosition::new(logical_x as f64, logical_y as f64),
        buttons: Default::default(),
        modifiers,
        count: 1,
        contact_geometry: PhysicalSize::new(1.0, 1.0),
        orientation: Default::default(),
        pressure: if is_down { 0.5 } else { 0.0 },
        tangential_pressure: 0.0,
        scale_factor: scale_factor as f64,
    };

    let event = if is_down {
        PointerEvent::Down(PointerButtonEvent {
            pointer: pointer_info,
            state,
            button: Some(button),
        })
    } else {
        PointerEvent::Up(PointerButtonEvent {
            pointer: pointer_info,
            state,
            button: Some(button),
        })
    };

    PlatformInput::Pointer(event)
}

/// Convert WM_MOUSEMOVE to W3C PointerEvent
pub fn mouse_move_event(lparam: LPARAM, scale_factor: f32) -> PlatformInput {
    let x = get_x_lparam(lparam);
    let y = get_y_lparam(lparam);

    let modifiers = unsafe { get_modifiers() };

    let logical_x = device_to_logical(x as f32, scale_factor);
    let logical_y = device_to_logical(y as f32, scale_factor);

    let pointer_info = PointerInfo {
        pointer_id: Some(PointerId::PRIMARY),
        pointer_type: PointerType::Mouse,
        persistent_device_id: None,
    };

    let state = PointerState {
        time: Instant::now().elapsed().as_millis() as u64,
        position: PhysicalPosition::new(logical_x as f64, logical_y as f64),
        buttons: Default::default(),
        modifiers,
        count: 1,
        contact_geometry: PhysicalSize::new(1.0, 1.0),
        orientation: Default::default(),
        pressure: 0.0,
        tangential_pressure: 0.0,
        scale_factor: scale_factor as f64,
    };

    let event = PointerEvent::Move(PointerUpdate {
        pointer: pointer_info,
        current: state,
        coalesced: Vec::new(),
        predicted: Vec::new(),
    });

    PlatformInput::Pointer(event)
}

/// Convert WM_MOUSEWHEEL to W3C PointerEvent with Scroll
pub fn mouse_wheel_event(wparam: WPARAM, lparam: LPARAM, scale_factor: f32) -> PlatformInput {
    let delta = ((wparam.0 as i32) >> 16) as i16 as f32;
    let lines = delta / 120.0; // WHEEL_DELTA = 120

    let x = get_x_lparam(lparam);
    let y = get_y_lparam(lparam);

    let modifiers = unsafe { get_modifiers() };

    let logical_x = device_to_logical(x as f32, scale_factor);
    let logical_y = device_to_logical(y as f32, scale_factor);

    let pointer_info = PointerInfo {
        pointer_id: Some(PointerId::PRIMARY),
        pointer_type: PointerType::Mouse,
        persistent_device_id: None,
    };

    let state = PointerState {
        time: Instant::now().elapsed().as_millis() as u64,
        position: PhysicalPosition::new(logical_x as f64, logical_y as f64),
        buttons: Default::default(),
        modifiers,
        count: 1,
        contact_geometry: PhysicalSize::new(1.0, 1.0),
        orientation: Default::default(),
        pressure: 0.0,
        tangential_pressure: 0.0,
        scale_factor: scale_factor as f64,
    };

    let event = PointerEvent::Scroll(ui_events::pointer::PointerScrollEvent {
        pointer: pointer_info,
        state,
        delta: ScrollDelta::LineDelta(0.0, lines),
    });

    PlatformInput::Pointer(event)
}

// ============================================================================
// Keyboard events (simple wrappers)
// ============================================================================

/// Convert WM_KEYDOWN to KeyboardEvent
pub fn key_down_event(wparam: WPARAM, lparam: LPARAM) -> PlatformInput {
    let vk = VIRTUAL_KEY(wparam.0 as u16);
    let scan_code = ((lparam.0 >> 16) & 0xFF) as u16;
    let is_repeat = (lparam.0 & (1 << 30)) != 0;

    let modifiers = unsafe { get_modifiers() };
    let key = vk_to_key(vk, scan_code);

    PlatformInput::Keyboard(KeyboardEvent {
        key,
        modifiers: modifiers.into(),
        is_down: true,
        is_repeat,
    })
}

/// Convert WM_KEYUP to KeyboardEvent
pub fn key_up_event(wparam: WPARAM, lparam: LPARAM) -> PlatformInput {
    let vk = VIRTUAL_KEY(wparam.0 as u16);
    let scan_code = ((lparam.0 >> 16) & 0xFF) as u16;

    let modifiers = unsafe { get_modifiers() };
    let key = vk_to_key(vk, scan_code);

    PlatformInput::Keyboard(KeyboardEvent {
        key,
        modifiers: modifiers.into(),
        is_down: false,
        is_repeat: false,
    })
}

/// Convert WM_CHAR to text
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
    fn test_vk_to_key() {
        assert!(matches!(vk_to_key(VK_A, 0), Key::Character(_)));
        assert_eq!(vk_to_key(VK_RETURN, 0), Key::Named(NamedKey::Enter));
        assert_eq!(vk_to_key(VK_LEFT, 0), Key::Named(NamedKey::ArrowLeft));
        assert_eq!(vk_to_key(VK_F1, 0), Key::Named(NamedKey::F1));
    }

    #[test]
    fn test_mouse_button_down() {
        let lparam = LPARAM(((200 << 16) | 100) as isize);
        let event = mouse_button_event(PointerButton::Primary, true, lparam, 1.0);

        if let PlatformInput::Pointer(PointerEvent::Down(down_event)) = event {
            assert_eq!(down_event.state.position.x, 100.0);
            assert_eq!(down_event.state.position.y, 200.0);
            assert_eq!(down_event.button, Some(PointerButton::Primary));
        } else {
            panic!("Expected Pointer Down event");
        }
    }

    #[test]
    fn test_mouse_move() {
        let lparam = LPARAM(((200 << 16) | 100) as isize);
        let event = mouse_move_event(lparam, 1.0);

        if let PlatformInput::Pointer(PointerEvent::Move(move_event)) = event {
            assert_eq!(move_event.current.position.x, 100.0);
            assert_eq!(move_event.current.position.y, 200.0);
        } else {
            panic!("Expected Pointer Move event");
        }
    }
}
