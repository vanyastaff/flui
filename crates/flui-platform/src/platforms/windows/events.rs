//! Windows event conversion to W3C ui-events (0.3 API)
//!
//! Converts Win32 messages to W3C-compliant PointerEvent using ui-events 0.3.
//!
//! These functions are ready for use but not yet wired into `window_proc`
//! (Phase 2: event dispatch integration).

// All items in this module are prepared for Phase 2 integration.
#![allow(dead_code)]

use std::{sync::LazyLock, time::Instant};

use dpi::{PhysicalPosition, PhysicalSize};
use keyboard_types::Modifiers as KeyboardModifiers;
use ui_events::{
    keyboard::{Code, KeyState, KeyboardEvent, Location},
    pointer::{
        PointerButton, PointerButtonEvent, PointerButtons, PointerEvent, PointerId, PointerInfo,
        PointerOrientation, PointerState, PointerType, PointerUpdate,
    },
};
use windows::Win32::{
    Foundation::{HWND, LPARAM, POINT, WPARAM},
    Graphics::Gdi::ScreenToClient,
    UI::Input::KeyboardAndMouse::{
        VIRTUAL_KEY, VK_0, VK_1, VK_2, VK_3, VK_4, VK_5, VK_6, VK_7, VK_8, VK_9, VK_A, VK_B,
        VK_BACK, VK_C, VK_CONTROL, VK_D, VK_DELETE, VK_DOWN, VK_E, VK_END, VK_ESCAPE, VK_F, VK_F1,
        VK_F2, VK_F3, VK_F4, VK_F5, VK_F6, VK_F7, VK_F8, VK_F9, VK_F10, VK_F11, VK_F12, VK_G, VK_H,
        VK_HOME, VK_I, VK_INSERT, VK_J, VK_K, VK_L, VK_LCONTROL, VK_LEFT, VK_LMENU, VK_LSHIFT,
        VK_LWIN, VK_M, VK_MENU, VK_N, VK_NEXT, VK_O, VK_P, VK_PRIOR, VK_Q, VK_R, VK_RCONTROL,
        VK_RETURN, VK_RIGHT, VK_RMENU, VK_RSHIFT, VK_RWIN, VK_S, VK_SHIFT, VK_SPACE, VK_T, VK_TAB,
        VK_U, VK_UP, VK_V, VK_W, VK_X, VK_Y, VK_Z,
    },
};

use super::util::{get_x_lparam, get_y_lparam, is_key_pressed};
use crate::traits::{Key, PlatformInput, device_to_logical};

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
///
/// # Safety
///
/// None, in the memory-safety sense — this only calls `is_key_pressed`
/// (see its own `# Safety` section in `util.rs`) with the fixed, in-range
/// virtual-key constants below. `unsafe fn` purely because it calls an
/// `unsafe fn`, not because this function itself has a precondition.
unsafe fn get_modifiers() -> KeyboardModifiers {
    // SAFETY: see the `# Safety` section above.
    unsafe {
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
}

// ============================================================================
// Pointer Event Conversion (W3C ui-events 0.3 API)
// ============================================================================

/// The set of mouse buttons Win32 reports as held in a message's `WPARAM`.
///
/// Every mouse message (`WM_MOUSEMOVE`, `WM_*BUTTONDOWN`, `WM_*BUTTONUP`,
/// `WM_MOUSEWHEEL`) carries the `MK_*` mask in the low word of `WPARAM`, and
/// Win32 already applies the W3C rule for us: the bit for a button is set on
/// its DOWN message and clear on its UP message, i.e. the set held *after*
/// the event. That makes this stateless — unlike the winit backend, which has
/// to track the transition itself because winit does not surface a mask.
///
/// This matters beyond fidelity: the framework tells a drag-move from a hover
/// by asking whether any button is held. A move that always reports an empty
/// set is delivered as a hover, so no gesture recognizer ever sees it and the
/// drag is silently re-interpreted as a tap on release.
///
/// `MK_LBUTTON`/`MK_RBUTTON`/`MK_MBUTTON` are `0x0001`/`0x0002`/`0x0010`
/// (`winuser.h`); the X buttons have no `PointerButton` mapping here.
#[inline]
fn held_buttons(wparam: WPARAM) -> PointerButtons {
    const MK_LBUTTON: usize = 0x0001;
    const MK_RBUTTON: usize = 0x0002;
    const MK_MBUTTON: usize = 0x0010;

    let mask = wparam.0 & 0xffff;
    let mut buttons = PointerButtons::default();
    if mask & MK_LBUTTON != 0 {
        buttons.insert(PointerButton::Primary);
    }
    if mask & MK_RBUTTON != 0 {
        buttons.insert(PointerButton::Secondary);
    }
    if mask & MK_MBUTTON != 0 {
        buttons.insert(PointerButton::Auxiliary);
    }
    buttons
}

/// Build a `PointerState` from LPARAM coordinates and scale factor.
///
/// `buttons` is the held set from [`held_buttons`]; it is a required argument
/// rather than a defaulted field so a new message arm cannot silently ship an
/// empty set.
#[inline]
fn pointer_state(
    lparam: LPARAM,
    scale_factor: f32,
    pressure: f32,
    buttons: PointerButtons,
) -> (PointerState, KeyboardModifiers) {
    pointer_state_at(
        get_x_lparam(lparam),
        get_y_lparam(lparam),
        scale_factor,
        pressure,
        buttons,
    )
}

/// [`pointer_state`] with the CLIENT-space device coordinates already in
/// hand — for the wheel messages, whose `lParam` needs a screen-to-client
/// conversion first (see [`wheel_pointer_state`]).
#[inline]
fn pointer_state_at(
    x: i32,
    y: i32,
    scale_factor: f32,
    pressure: f32,
    buttons: PointerButtons,
) -> (PointerState, KeyboardModifiers) {
    // SAFETY: see `get_modifiers`'s own `# Safety` section — no
    // precondition to discharge here.
    let modifiers = unsafe { get_modifiers() };
    let logical_x = device_to_logical(x as f32, scale_factor);
    let logical_y = device_to_logical(y as f32, scale_factor);

    let state = PointerState {
        time: event_timestamp_ms(),
        position: PhysicalPosition::new(logical_x as f64, logical_y as f64),
        buttons,
        modifiers,
        count: 1,
        contact_geometry: PhysicalSize::new(1.0, 1.0),
        orientation: PointerOrientation::default(),
        pressure,
        tangential_pressure: 0.0,
        scale_factor: scale_factor as f64,
    };
    (state, modifiers)
}

/// Convert WM_LBUTTONDOWN/UP to W3C PointerEvent
pub fn mouse_button_event(
    button: PointerButton,
    is_down: bool,
    wparam: WPARAM,
    lparam: LPARAM,
    scale_factor: f32,
) -> PlatformInput {
    let (state, modifiers) = pointer_state(
        lparam,
        scale_factor,
        if is_down { 0.5 } else { 0.0 },
        held_buttons(wparam),
    );

    let _ = modifiers;

    let event = if is_down {
        PointerEvent::Down(PointerButtonEvent {
            pointer: primary_mouse_info(),
            state,
            button: Some(button),
        })
    } else {
        PointerEvent::Up(PointerButtonEvent {
            pointer: primary_mouse_info(),
            state,
            button: Some(button),
        })
    };

    PlatformInput::Pointer(event)
}

/// Convert WM_MOUSEMOVE to W3C PointerEvent
pub fn mouse_move_event(wparam: WPARAM, lparam: LPARAM, scale_factor: f32) -> PlatformInput {
    let (state, modifiers) = pointer_state(lparam, scale_factor, 0.0, held_buttons(wparam));
    let _ = modifiers;

    let event = PointerEvent::Move(PointerUpdate {
        pointer: primary_mouse_info(),
        current: state,
        coalesced: Vec::new(),
        predicted: Vec::new(),
    });

    PlatformInput::Pointer(event)
}

/// The signed scroll distance both wheel messages carry in the high word of
/// `wParam` (`GET_WHEEL_DELTA_WPARAM`), in multiples of `WHEEL_DELTA` (120).
fn wheel_distance(wparam: WPARAM) -> i16 {
    ((wparam.0 as i32) >> 16) as i16
}

/// Build the pointer state for a wheel message.
///
/// Unlike every other client-area mouse message, `WM_MOUSEWHEEL` and
/// `WM_MOUSEHWHEEL` deliver the cursor position in SCREEN coordinates
/// (both messages' `lParam` docs:
/// <https://learn.microsoft.com/en-us/windows/win32/inputdev/wm-mousewheel>),
/// so the point is converted to client space here before the shared
/// [`pointer_state_at`] path DPI-scales it — otherwise scroll hit-testing
/// targets the wrong child whenever the window's client origin is not the
/// desktop origin.
fn wheel_pointer_state(
    hwnd: HWND,
    wparam: WPARAM,
    lparam: LPARAM,
    scale_factor: f32,
) -> (PointerState, KeyboardModifiers) {
    let mut point = POINT {
        x: get_x_lparam(lparam),
        y: get_y_lparam(lparam),
    };
    // SAFETY: `point` is a live, writable local; `ScreenToClient` writes
    // nothing else. On failure (invalid `hwnd`) it returns FALSE and leaves
    // `point` unchanged.
    let converted = unsafe { ScreenToClient(hwnd, &raw mut point) };
    if !converted.as_bool() {
        tracing::warn!(
            "ScreenToClient failed for a wheel message; scroll position stays in screen space"
        );
    }
    pointer_state_at(point.x, point.y, scale_factor, 0.0, held_buttons(wparam))
}

/// Convert WM_MOUSEWHEEL to W3C PointerEvent with Scroll
///
/// Win32's vertical sign (positive = wheel rotated away from the user) is the
/// inverse of the cross-backend convention — positive = content scrolls down —
/// so `from_win32_wheel` negates it at this boundary; see
/// `crate::shared::scroll` for the sign/unit table and citations. The cursor
/// position arrives in screen coordinates; see [`wheel_pointer_state`].
pub fn mouse_wheel_event(
    hwnd: HWND,
    wparam: WPARAM,
    lparam: LPARAM,
    scale_factor: f32,
) -> PlatformInput {
    let (state, modifiers) = wheel_pointer_state(hwnd, wparam, lparam, scale_factor);
    let _ = modifiers;

    let event = PointerEvent::Scroll(ui_events::pointer::PointerScrollEvent {
        pointer: primary_mouse_info(),
        state,
        delta: crate::shared::scroll::from_win32_wheel(wheel_distance(wparam)),
    });

    PlatformInput::Pointer(event)
}

/// Convert WM_MOUSEHWHEEL to W3C PointerEvent with Scroll
///
/// Win32's horizontal sign (positive = wheel tilted right) already matches
/// the cross-backend convention — positive = content scrolls right — so only
/// the `WHEEL_DELTA` division applies; see `crate::shared::scroll`. The
/// cursor position arrives in screen coordinates; see
/// [`wheel_pointer_state`].
pub fn mouse_hwheel_event(
    hwnd: HWND,
    wparam: WPARAM,
    lparam: LPARAM,
    scale_factor: f32,
) -> PlatformInput {
    let (state, modifiers) = wheel_pointer_state(hwnd, wparam, lparam, scale_factor);
    let _ = modifiers;

    let event = PointerEvent::Scroll(ui_events::pointer::PointerScrollEvent {
        pointer: primary_mouse_info(),
        state,
        delta: crate::shared::scroll::from_win32_hwheel(wheel_distance(wparam)),
    });

    PlatformInput::Pointer(event)
}

// ============================================================================
// Keyboard events (simple wrappers)
// ============================================================================

/// Convert WM_KEYDOWN to W3C KeyboardEvent
pub fn key_down_event(wparam: WPARAM, lparam: LPARAM) -> PlatformInput {
    let vk = VIRTUAL_KEY(wparam.0 as u16);
    let scan_code = ((lparam.0 >> 16) & 0xFF) as u16;
    let is_repeat = (lparam.0 & (1 << 30)) != 0;

    // SAFETY: see `pointer_state` above — same call, no precondition.
    let modifiers = unsafe { get_modifiers() };
    let key = vk_to_key(vk, scan_code);

    PlatformInput::Keyboard(KeyboardEvent {
        state: KeyState::Down,
        key,
        code: Code::Unidentified,
        location: Location::Standard,
        modifiers,
        repeat: is_repeat,
        is_composing: false,
    })
}

/// Convert WM_KEYUP to W3C KeyboardEvent
pub fn key_up_event(wparam: WPARAM, lparam: LPARAM) -> PlatformInput {
    let vk = VIRTUAL_KEY(wparam.0 as u16);
    let scan_code = ((lparam.0 >> 16) & 0xFF) as u16;

    // SAFETY: see `pointer_state` above — same call, no precondition.
    let modifiers = unsafe { get_modifiers() };
    let key = vk_to_key(vk, scan_code);

    PlatformInput::Keyboard(KeyboardEvent {
        state: KeyState::Up,
        key,
        code: Code::Unidentified,
        location: Location::Standard,
        modifiers,
        repeat: false,
        is_composing: false,
    })
}

/// Convert WM_CHAR to a character
pub fn char_from_wparam(wparam: WPARAM) -> Option<char> {
    char::from_u32(wparam.0 as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::NamedKey;

    #[test]
    fn test_vk_to_key() {
        assert!(matches!(vk_to_key(VK_A, 0), Key::Character(_)));
        assert_eq!(vk_to_key(VK_RETURN, 0), Key::Named(NamedKey::Enter));
        assert_eq!(vk_to_key(VK_LEFT, 0), Key::Named(NamedKey::ArrowLeft));
        assert_eq!(vk_to_key(VK_F1, 0), Key::Named(NamedKey::F1));
    }

    #[test]
    fn test_mouse_button_down() {
        let lparam = LPARAM(((0xC8 << 16) | 0x64) as isize); // y=200, x=100
        // WM_LBUTTONDOWN carries MK_LBUTTON in its own WPARAM.
        let event = mouse_button_event(PointerButton::Primary, true, WPARAM(0x0001), lparam, 1.0);

        if let PlatformInput::Pointer(PointerEvent::Down(down_event)) = event {
            assert_eq!(down_event.state.position.x, 100.0);
            assert_eq!(down_event.state.position.y, 200.0);
            assert_eq!(down_event.button, Some(PointerButton::Primary));
            assert!(
                down_event.state.buttons.contains(PointerButton::Primary),
                "a press must report its own button as held"
            );
        } else {
            panic!("Expected Pointer Down event");
        }
    }

    #[test]
    fn test_mouse_move() {
        let lparam = LPARAM(((0xC8 << 16) | 0x64) as isize); // y=200, x=100
        // A drag: WM_MOUSEMOVE with MK_LBUTTON still held.
        let event = mouse_move_event(WPARAM(0x0001), lparam, 1.0);

        if let PlatformInput::Pointer(PointerEvent::Move(move_event)) = event {
            assert_eq!(move_event.current.position.x, 100.0);
            assert_eq!(move_event.current.position.y, 200.0);
            assert!(
                move_event.current.buttons.contains(PointerButton::Primary),
                "a move with a button held is a drag, not a hover; reporting an \
                 empty set here routes it to on_hover and no recognizer sees it"
            );
        } else {
            panic!("Expected Pointer Move event");
        }

        // The same message with nothing held is a genuine hover.
        let hover = mouse_move_event(WPARAM(0), lparam, 1.0);
        if let PlatformInput::Pointer(PointerEvent::Move(move_event)) = hover {
            assert!(move_event.current.buttons.is_empty());
        } else {
            panic!("Expected Pointer Move event");
        }
    }
}

#[cfg(test)]
mod held_button_tests {
    use super::*;

    /// Win32 reports the held set directly, and already applies the W3C
    /// "after the event" rule — a DOWN message carries its own bit, an UP
    /// message does not. Reporting an empty set for an in-contact move is
    /// what makes the framework deliver a drag as a hover.
    ///
    /// If reverted (`pointer_state` back to `PointerButtons::default()`):
    /// the first two assertions read an empty set.
    #[test]
    fn wparam_mask_becomes_the_held_button_set() {
        assert!(
            held_buttons(WPARAM(0x0001)).contains(PointerButton::Primary),
            "MK_LBUTTON must report the primary button held"
        );
        assert!(
            held_buttons(WPARAM(0x0002)).contains(PointerButton::Secondary),
            "MK_RBUTTON must report the secondary button held"
        );
        assert!(
            held_buttons(WPARAM(0x0010)).contains(PointerButton::Auxiliary),
            "MK_MBUTTON must report the auxiliary button held"
        );

        // A move with no button held is a genuine hover.
        assert!(held_buttons(WPARAM(0)).is_empty());

        // The high word carries the wheel delta on WM_MOUSEWHEEL and must not
        // leak into the mask.
        assert!(held_buttons(WPARAM(0x0078_0000)).is_empty());

        let both = held_buttons(WPARAM(0x0003));
        assert!(both.contains(PointerButton::Primary));
        assert!(both.contains(PointerButton::Secondary));
    }
}
