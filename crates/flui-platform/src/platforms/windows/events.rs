//! Windows event conversion to W3C ui-events (0.3 API)
//!
//! Converts Win32 messages to W3C-compliant PointerEvent using ui-events 0.3.
//!
//! These functions are ready for use but not yet wired into `window_proc`
//! (Phase 2: event dispatch integration).

// All items in this module are prepared for Phase 2 integration.
#![allow(dead_code)]

use dpi::{PhysicalPosition, PhysicalSize};
use keyboard_types::Modifiers as KeyboardModifiers;
use ui_events::{
    keyboard::{Code, KeyState, KeyboardEvent, Location},
    pointer::{
        PointerButton, PointerButtonEvent, PointerButtons, PointerEvent, PointerOrientation,
        PointerState, PointerUpdate,
    },
};
use windows::Win32::{
    Foundation::{HWND, LPARAM, POINT, WPARAM},
    Graphics::Gdi::ScreenToClient,
    UI::Input::KeyboardAndMouse::{VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT},
};

use super::util::{get_x_lparam, get_y_lparam, is_key_pressed};
use crate::{
    shared::events::{event_timestamp_ns, primary_mouse_info},
    shared::keys,
    traits::{Key, PlatformInput, device_to_logical},
};

// ============================================================================
// Keyboard Event Conversion
// ============================================================================
//
// The translation decisions themselves (vk -> Key fallback, scancode -> Code,
// the WM_CHAR merge) are pure functions in `crate::shared::keys`, where their
// tests execute on every host; this file only unpacks the Win32 message
// parameters and feeds them through. The WM_CHAR pairing model is documented
// on that module.

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
    count: u8,
) -> (PointerState, KeyboardModifiers) {
    pointer_state_at(
        get_x_lparam(lparam),
        get_y_lparam(lparam),
        scale_factor,
        pressure,
        buttons,
        count,
    )
}

/// [`pointer_state`] with the CLIENT-space device coordinates already in
/// hand — for the wheel messages, whose `lParam` needs a screen-to-client
/// conversion first (see [`wheel_pointer_state`]).
#[inline]
/// `count` is the W3C click count — `1` on Down/Up transitions, `0`
/// elsewhere (the cross-wire contract in flui-interaction's module doc).
fn pointer_state_at(
    x: i32,
    y: i32,
    scale_factor: f32,
    pressure: f32,
    buttons: PointerButtons,
    count: u8,
) -> (PointerState, KeyboardModifiers) {
    // SAFETY: see `get_modifiers`'s own `# Safety` section — no
    // precondition to discharge here.
    let modifiers = unsafe { get_modifiers() };
    let logical_x = device_to_logical(x as f32, scale_factor);
    let logical_y = device_to_logical(y as f32, scale_factor);

    let state = PointerState {
        time: event_timestamp_ns(),
        position: PhysicalPosition::new(logical_x as f64, logical_y as f64),
        buttons,
        modifiers,
        count,
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
        1,
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
    let held = held_buttons(wparam);
    // Sensor-less pressure rule: 0.5 while any button is held (a drag),
    // 0.0 on a hover.
    let pressure = if held == PointerButtons::default() {
        0.0
    } else {
        0.5
    };
    let (state, modifiers) = pointer_state(lparam, scale_factor, pressure, held, 0);
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
    pointer_state_at(point.x, point.y, scale_factor, 0.0, held_buttons(wparam), 0)
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

/// Convert WM_KEYDOWN to W3C KeyboardEvent.
///
/// `translated_text` is the drained `WM_CHAR` burst for this keydown (see
/// `window_proc`'s `WM_KEYDOWN` arm and `crate::shared::keys`'s module doc
/// for the pairing model); when present and typeable it becomes the event's
/// `Key::Character`, otherwise the layout-independent virtual-key fallback
/// applies.
pub fn key_down_event(
    wparam: WPARAM,
    lparam: LPARAM,
    translated_text: Option<String>,
) -> PlatformInput {
    let vk = wparam.0 as u16;
    let (scan_code, extended, is_repeat) = keys::parse_key_lparam(lparam.0);

    // SAFETY: see `pointer_state` above — same call, no precondition.
    let modifiers = unsafe { get_modifiers() };
    let fallback = keys::vk_to_key(vk, modifiers.contains(KeyboardModifiers::SHIFT));
    let code = keys::scancode_to_code(scan_code, extended);
    let key = keys::key_for_keydown(
        fallback,
        translated_text,
        modifiers.contains(KeyboardModifiers::ALT),
        modifiers.contains(KeyboardModifiers::CONTROL),
        code,
    );

    PlatformInput::Keyboard(KeyboardEvent {
        state: KeyState::Down,
        key,
        code,
        location: keys::location_for_code(code),
        modifiers,
        repeat: is_repeat,
        is_composing: false,
    })
}

/// Convert WM_KEYUP to W3C KeyboardEvent.
///
/// No `WM_CHAR` pairs with a keyup, so the key is always the virtual-key
/// fallback — for a letter that is its shift-respecting character, for OEM
/// punctuation the US-layout position. Consumers that type text act on
/// `KeyState::Down` only, so this asymmetry never reaches a text field.
pub fn key_up_event(wparam: WPARAM, lparam: LPARAM) -> PlatformInput {
    let vk = wparam.0 as u16;
    let (scan_code, extended, _) = keys::parse_key_lparam(lparam.0);

    // SAFETY: see `pointer_state` above — same call, no precondition.
    let modifiers = unsafe { get_modifiers() };
    let key = keys::vk_to_key(vk, modifiers.contains(KeyboardModifiers::SHIFT));
    let code = keys::scancode_to_code(scan_code, extended);

    PlatformInput::Keyboard(KeyboardEvent {
        state: KeyState::Up,
        key,
        code,
        location: keys::location_for_code(code),
        modifiers,
        repeat: false,
        is_composing: false,
    })
}

/// Build the KeyboardEvent for an out-of-band `WM_CHAR` — one that reached
/// `window_proc` instead of being drained by a `WM_KEYDOWN` (Alt+numpad
/// composition, or a directly-sent message). There is no owning physical
/// key, so it is dispatched as a key-down with `Code::Unidentified` and no
/// paired key-up; the text lane consumes `Key::Character` on key-down only.
pub fn stray_char_event(text: String) -> PlatformInput {
    // SAFETY: see `pointer_state` above — same call, no precondition.
    let modifiers = unsafe { get_modifiers() };

    PlatformInput::Keyboard(KeyboardEvent {
        state: KeyState::Down,
        key: Key::Character(text),
        code: Code::Unidentified,
        location: Location::Standard,
        modifiers,
        repeat: false,
        is_composing: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Win32 wire assembles its keyboard events from the shared,
    /// host-testable tables plus the live modifier query: a WM_KEYDOWN for
    /// VK_A (scancode 0x1E) with the translated text "A" merged in must
    /// carry the shifted character AND the physical-key code — this is the
    /// wire that used to hardcode `Code::Unidentified` and case-fold every
    /// character.
    #[test]
    fn test_key_down_event_carries_text_and_code() {
        let lparam = LPARAM(0x1E << 16); // scancode 0x1E, not extended
        let event = key_down_event(WPARAM(0x41), lparam, Some("A".to_string()));

        if let PlatformInput::Keyboard(kb) = event {
            assert_eq!(kb.state, KeyState::Down);
            assert_eq!(kb.key, Key::Character("A".to_string()));
            assert_eq!(kb.code, Code::KeyA);
            assert_eq!(kb.location, Location::Standard);
            assert!(!kb.repeat);
        } else {
            panic!("Expected Keyboard event");
        }
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
