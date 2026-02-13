//! Android input event conversion
//!
//! Converts `android-activity` input events (`MotionEvent`, `KeyEvent`) to
//! the platform-agnostic `PlatformInput` types (W3C-compliant `ui-events`).
//!
//! # Touch Event Mapping
//!
//! ```text
//! MotionAction::Down / PointerDown → PointerEvent::Down
//! MotionAction::Up / PointerUp     → PointerEvent::Up
//! MotionAction::Move                → PointerEvent::Move
//! MotionAction::Cancel              → PointerEvent::Cancel
//! MotionAction::HoverEnter          → PointerEvent::Enter
//! MotionAction::HoverExit           → PointerEvent::Leave
//! ```

use crate::traits::{
    PlatformInput, PointerButton, PointerButtons, PointerEvent, PointerType,
};
use android_activity::input::{KeyAction, MotionAction, ToolType};
use dpi::PhysicalPosition;
use keyboard_types::{Key, KeyState, Modifiers, NamedKey};
use ui_events::keyboard::KeyboardEvent;
use ui_events::pointer::{
    ContactGeometry, PointerButtonEvent, PointerId, PointerInfo, PointerState, PointerUpdate,
};

/// Convert an Android `MotionEvent` to one or more `PlatformInput` events.
///
/// A single `MotionAction::Move` event carries updated positions for ALL active
/// pointers, so we emit one `PlatformInput` per pointer in that case.
///
/// Returns a `Vec` because multi-touch Move events produce multiple outputs.
pub fn convert_motion_event(
    event: &android_activity::input::MotionEvent<'_>,
    scale_factor: f64,
) -> Vec<PlatformInput> {
    let action = event.action();
    let time_ns = event.event_time() as u64;
    let modifiers = convert_meta_state(event.meta_state());

    match action {
        MotionAction::Down | MotionAction::PointerDown => {
            let idx = event.pointer_index();
            let pointer = event.pointer_at_index(idx);
            let info = make_pointer_info(&pointer);
            let state = make_pointer_state(&pointer, time_ns, scale_factor, modifiers);

            vec![PlatformInput::Pointer(PointerEvent::Down(
                PointerButtonEvent {
                    button: Some(PointerButton::Primary),
                    pointer: info,
                    state,
                },
            ))]
        }

        MotionAction::Up | MotionAction::PointerUp => {
            let idx = event.pointer_index();
            let pointer = event.pointer_at_index(idx);
            let info = make_pointer_info(&pointer);
            let state = make_pointer_state(&pointer, time_ns, scale_factor, modifiers);

            vec![PlatformInput::Pointer(PointerEvent::Up(
                PointerButtonEvent {
                    button: Some(PointerButton::Primary),
                    pointer: info,
                    state,
                },
            ))]
        }

        MotionAction::Move => {
            // Move events carry all pointers — emit one event per pointer
            event
                .pointers()
                .map(|pointer| {
                    let info = make_pointer_info(&pointer);
                    let state = make_pointer_state(&pointer, time_ns, scale_factor, modifiers);

                    PlatformInput::Pointer(PointerEvent::Move(PointerUpdate {
                        pointer: info,
                        current: state,
                        coalesced: Vec::new(),
                        predicted: Vec::new(),
                    }))
                })
                .collect()
        }

        MotionAction::Cancel => {
            // Cancel all active pointers
            event
                .pointers()
                .map(|pointer| {
                    let info = make_pointer_info(&pointer);
                    PlatformInput::Pointer(PointerEvent::Cancel(info))
                })
                .collect()
        }

        MotionAction::HoverEnter => {
            let idx = event.pointer_index();
            let pointer = event.pointer_at_index(idx);
            let info = make_pointer_info(&pointer);
            vec![PlatformInput::Pointer(PointerEvent::Enter(info))]
        }

        MotionAction::HoverExit => {
            let idx = event.pointer_index();
            let pointer = event.pointer_at_index(idx);
            let info = make_pointer_info(&pointer);
            vec![PlatformInput::Pointer(PointerEvent::Leave(info))]
        }

        MotionAction::HoverMove => {
            // Treat hover move as a regular move (stylus hovering, mouse)
            event
                .pointers()
                .map(|pointer| {
                    let info = make_pointer_info(&pointer);
                    let state = make_pointer_state(&pointer, time_ns, scale_factor, modifiers);

                    PlatformInput::Pointer(PointerEvent::Move(PointerUpdate {
                        pointer: info,
                        current: state,
                        coalesced: Vec::new(),
                        predicted: Vec::new(),
                    }))
                })
                .collect()
        }

        _ => Vec::new(),
    }
}

/// Convert an Android `KeyEvent` to a `PlatformInput`.
pub fn convert_key_event(
    event: &android_activity::input::KeyEvent<'_>,
) -> Option<PlatformInput> {
    let action = event.action();
    let state = match action {
        KeyAction::Down => KeyState::Down,
        KeyAction::Up => KeyState::Up,
        // Multiple = auto-repeat; treat as Down
        KeyAction::Multiple => KeyState::Down,
        _ => return None,
    };

    // Keycode implements Into<u32> via num_enum::IntoPrimitive
    let keycode_i32: i32 = u32::from(event.key_code()) as i32;
    let code = ui_events::keyboard::android::keycode_to_code(keycode_i32);
    let named_key = ui_events::keyboard::android::keycode_to_named_key(keycode_i32);
    let location = ui_events::keyboard::android::keycode_to_location(keycode_i32);

    // Derive the Key from the named key mapping
    let key = if named_key != NamedKey::Unidentified {
        Key::Named(named_key)
    } else {
        // Try to map character keys (A-Z, 0-9, symbols)
        keycode_to_character(keycode_i32)
            .map(|ch| Key::Character(ch.to_string().into()))
            .unwrap_or(Key::Named(NamedKey::Unidentified))
    };

    let modifiers = convert_meta_state(event.meta_state());
    let repeat = event.repeat_count() > 0;

    Some(PlatformInput::Keyboard(KeyboardEvent {
        state,
        key,
        code,
        location,
        modifiers,
        repeat,
        is_composing: false,
    }))
}

// ============================================================================
// Helpers
// ============================================================================

/// Build `PointerInfo` from an Android pointer.
fn make_pointer_info(pointer: &android_activity::input::Pointer<'_>) -> PointerInfo {
    // Android pointer IDs start at 0. PointerId::PRIMARY is 1 (NonZeroU64::MIN).
    // Offset by 1 to avoid collision with PRIMARY reservation.
    let pid = pointer.pointer_id() as u64 + 1;

    PointerInfo {
        pointer_id: PointerId::new(pid),
        persistent_device_id: None,
        pointer_type: convert_tool_type(pointer.tool_type()),
    }
}

/// Build `PointerState` from an Android pointer.
fn make_pointer_state(
    pointer: &android_activity::input::Pointer<'_>,
    time_ns: u64,
    scale_factor: f64,
    modifiers: Modifiers,
) -> PointerState {
    // Android reports coordinates in physical (device) pixels
    let x = pointer.x() as f64;
    let y = pointer.y() as f64;

    // Pressure: Android returns 0.0-1.0 for touch, 0.0 for no contact
    let pressure = pointer.pressure();

    // Touch size as contact geometry (physical pixels)
    let size = pointer.size();
    let contact = if size > 0.0 {
        ContactGeometry {
            width: size as f64,
            height: size as f64,
        }
    } else {
        ContactGeometry {
            width: 1.0,
            height: 1.0,
        }
    };

    PointerState {
        time: time_ns,
        position: PhysicalPosition::new(x, y),
        buttons: PointerButtons::default(),
        modifiers,
        count: 1,
        contact_geometry: contact,
        orientation: Default::default(),
        pressure,
        tangential_pressure: 0.0,
        scale_factor,
    }
}

/// Convert Android `ToolType` to W3C `PointerType`.
fn convert_tool_type(tool: ToolType) -> PointerType {
    match tool {
        ToolType::Finger => PointerType::Touch,
        ToolType::Stylus | ToolType::Eraser => PointerType::Pen,
        ToolType::Mouse => PointerType::Mouse,
        _ => PointerType::Unknown,
    }
}

/// Convert Android `MetaState` to `keyboard_types::Modifiers`.
fn convert_meta_state(meta: android_activity::input::MetaState) -> Modifiers {
    // MetaState is a newtype wrapping u32: MetaState(pub u32)
    let bits: u32 = meta.0;
    let mut mods = Modifiers::empty();

    // Android meta state flags
    const META_SHIFT: u32 = 0x01;
    const META_ALT: u32 = 0x02;
    const META_CTRL: u32 = 0x1000;
    const META_META: u32 = 0x10000;
    const META_CAPS_LOCK: u32 = 0x100000;
    const META_NUM_LOCK: u32 = 0x200000;

    if bits & META_SHIFT != 0 {
        mods |= Modifiers::SHIFT;
    }
    if bits & META_ALT != 0 {
        mods |= Modifiers::ALT;
    }
    if bits & META_CTRL != 0 {
        mods |= Modifiers::CONTROL;
    }
    if bits & META_META != 0 {
        mods |= Modifiers::META;
    }
    if bits & META_CAPS_LOCK != 0 {
        mods |= Modifiers::CAPS_LOCK;
    }
    if bits & META_NUM_LOCK != 0 {
        mods |= Modifiers::NUM_LOCK;
    }

    mods
}

/// Map Android keycode to a character, for keys that produce printable characters.
fn keycode_to_character(keycode: i32) -> Option<char> {
    use ui_events::keyboard::android::*;

    match keycode {
        KEYCODE_A => Some('a'),
        KEYCODE_B => Some('b'),
        KEYCODE_C => Some('c'),
        KEYCODE_D => Some('d'),
        KEYCODE_E => Some('e'),
        KEYCODE_F => Some('f'),
        KEYCODE_G => Some('g'),
        KEYCODE_H => Some('h'),
        KEYCODE_I => Some('i'),
        KEYCODE_J => Some('j'),
        KEYCODE_K => Some('k'),
        KEYCODE_L => Some('l'),
        KEYCODE_M => Some('m'),
        KEYCODE_N => Some('n'),
        KEYCODE_O => Some('o'),
        KEYCODE_P => Some('p'),
        KEYCODE_Q => Some('q'),
        KEYCODE_R => Some('r'),
        KEYCODE_S => Some('s'),
        KEYCODE_T => Some('t'),
        KEYCODE_U => Some('u'),
        KEYCODE_V => Some('v'),
        KEYCODE_W => Some('w'),
        KEYCODE_X => Some('x'),
        KEYCODE_Y => Some('y'),
        KEYCODE_Z => Some('z'),
        KEYCODE_0 => Some('0'),
        KEYCODE_1 => Some('1'),
        KEYCODE_2 => Some('2'),
        KEYCODE_3 => Some('3'),
        KEYCODE_4 => Some('4'),
        KEYCODE_5 => Some('5'),
        KEYCODE_6 => Some('6'),
        KEYCODE_7 => Some('7'),
        KEYCODE_8 => Some('8'),
        KEYCODE_9 => Some('9'),
        KEYCODE_SPACE => Some(' '),
        KEYCODE_COMMA => Some(','),
        KEYCODE_PERIOD => Some('.'),
        KEYCODE_MINUS => Some('-'),
        KEYCODE_EQUALS => Some('='),
        KEYCODE_LEFT_BRACKET => Some('['),
        KEYCODE_RIGHT_BRACKET => Some(']'),
        KEYCODE_BACKSLASH => Some('\\'),
        KEYCODE_SEMICOLON => Some(';'),
        KEYCODE_APOSTROPHE => Some('\''),
        KEYCODE_SLASH => Some('/'),
        KEYCODE_GRAVE => Some('`'),
        KEYCODE_AT => Some('@'),
        KEYCODE_STAR => Some('*'),
        KEYCODE_POUND => Some('#'),
        KEYCODE_PLUS => Some('+'),
        _ => None,
    }
}
