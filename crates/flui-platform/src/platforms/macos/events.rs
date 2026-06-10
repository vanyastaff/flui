//! macOS event conversion to W3C ui-events (0.3 API)
//!
//! Converts NSEvent to platform-agnostic ui-events types.
//!
//! # Architecture
//!
//! ```text
//! NSEvent (Cocoa)
//!     ↓
//! convert_ns_event() (this module)
//!     ↓
//! PlatformInput (ui-events wrapper)
//!     ↓
//! WindowCallbacks::dispatch_input
//! ```
//!
//! # Key Mappings
//!
//! - NSEvent.characters / keyCode → keyboard_types::Key
//! - NSEventModifierFlags → keyboard_types::Modifiers
//! - NSEventType → PointerEvent / KeyboardEvent
//! - NSPoint → logical pixels (NSEvent coordinates are already logical;
//!   the Y axis is flipped from bottom-left to top-left origin)

use std::{sync::LazyLock, time::Instant};

use cocoa::{
    appkit::{NSEventModifierFlags, NSEventType},
    base::id,
    foundation::NSPoint,
};
use dpi::{PhysicalPosition, PhysicalSize};
use keyboard_types::{Key, Modifiers, NamedKey};
use objc::{class, msg_send, sel, sel_impl};
use ui_events::{
    ScrollDelta,
    keyboard::{Code, KeyState, KeyboardEvent, Location},
    pointer::{
        PointerButton, PointerButtonEvent, PointerButtons, PointerEvent, PointerId, PointerInfo,
        PointerScrollEvent, PointerState, PointerType, PointerUpdate,
    },
};

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

// ============================================================================
// NSEvent Conversion
// ============================================================================

/// Convert NSEvent to PlatformInput
///
/// `view_height` is the receiving view's logical height, used to flip the
/// Y axis from macOS bottom-left origin to the framework's top-left origin.
///
/// Returns None if the event type is not supported (e.g., gesture events)
///
/// # Safety
///
/// `ns_event` must be a valid, live `NSEvent*` for the duration of the call.
pub unsafe fn convert_ns_event(
    ns_event: id,
    scale_factor: f64,
    view_height: f64,
) -> Option<PlatformInput> {
    // SAFETY: caller guarantees `ns_event` is a valid NSEvent*; all messages
    // sent below are documented NSEvent selectors.
    unsafe {
        let event_type: NSEventType = msg_send![ns_event, type];

        match event_type {
            // Keyboard events
            NSEventType::NSKeyDown => {
                let key = extract_key(ns_event);
                let modifiers = extract_modifiers(ns_event);
                let is_repeat: bool = msg_send![ns_event, isARepeat];

                Some(PlatformInput::Keyboard(KeyboardEvent {
                    state: KeyState::Down,
                    key,
                    code: Code::Unidentified,
                    location: Location::Standard,
                    modifiers,
                    repeat: is_repeat,
                    is_composing: false,
                }))
            }

            NSEventType::NSKeyUp => {
                let key = extract_key(ns_event);
                let modifiers = extract_modifiers(ns_event);

                Some(PlatformInput::Keyboard(KeyboardEvent {
                    state: KeyState::Up,
                    key,
                    code: Code::Unidentified,
                    location: Location::Standard,
                    modifiers,
                    repeat: false,
                    is_composing: false,
                }))
            }

            // Mouse button events
            NSEventType::NSLeftMouseDown => convert_mouse_button(
                ns_event,
                scale_factor,
                view_height,
                PointerButton::Primary,
                true,
            ),

            NSEventType::NSLeftMouseUp => convert_mouse_button(
                ns_event,
                scale_factor,
                view_height,
                PointerButton::Primary,
                false,
            ),

            NSEventType::NSRightMouseDown => convert_mouse_button(
                ns_event,
                scale_factor,
                view_height,
                PointerButton::Secondary,
                true,
            ),

            NSEventType::NSRightMouseUp => convert_mouse_button(
                ns_event,
                scale_factor,
                view_height,
                PointerButton::Secondary,
                false,
            ),

            NSEventType::NSOtherMouseDown => convert_mouse_button(
                ns_event,
                scale_factor,
                view_height,
                PointerButton::Auxiliary,
                true,
            ),

            NSEventType::NSOtherMouseUp => convert_mouse_button(
                ns_event,
                scale_factor,
                view_height,
                PointerButton::Auxiliary,
                false,
            ),

            // Mouse movement events
            NSEventType::NSMouseMoved
            | NSEventType::NSLeftMouseDragged
            | NSEventType::NSRightMouseDragged
            | NSEventType::NSOtherMouseDragged => {
                convert_mouse_move(ns_event, scale_factor, view_height)
            }

            // Scroll events
            NSEventType::NSScrollWheel => convert_scroll_event(ns_event, scale_factor, view_height),

            // Mouse enter/exit carry no useful position payload in the W3C
            // model — Enter/Leave only identify the pointer.
            NSEventType::NSMouseEntered => Some(PlatformInput::Pointer(PointerEvent::Enter(
                primary_mouse_info(),
            ))),

            NSEventType::NSMouseExited => Some(PlatformInput::Pointer(PointerEvent::Leave(
                primary_mouse_info(),
            ))),

            // Unsupported events
            _ => None,
        }
    }
}

// ============================================================================
// Keyboard Event Conversion
// ============================================================================

/// Extract key from NSEvent
///
/// Uses NSEvent.keyCode for special keys, NSEvent.characters otherwise
///
/// # Safety
///
/// `ns_event` must be a valid, live `NSEvent*` of a keyboard event.
unsafe fn extract_key(ns_event: id) -> Key {
    // SAFETY: caller guarantees `ns_event` is a valid NSEvent*; `characters`
    // returns an autoreleased NSString whose UTF8String buffer is valid while
    // the event is alive (we copy it into an owned String before returning).
    unsafe {
        // Check for special keys via key code first
        let key_code: u16 = msg_send![ns_event, keyCode];
        if let Some(special_key) = key_code_to_key(key_code) {
            return special_key;
        }

        // Get characters string
        let chars: id = msg_send![ns_event, characters];
        if chars.is_null() {
            return Key::Named(NamedKey::Unidentified);
        }

        // Convert NSString to Rust string
        let chars_ptr: *const i8 = msg_send![chars, UTF8String];
        if chars_ptr.is_null() {
            return Key::Named(NamedKey::Unidentified);
        }

        let chars_str = std::ffi::CStr::from_ptr(chars_ptr as *const _)
            .to_str()
            .unwrap_or("");

        if chars_str.is_empty() {
            return Key::Named(NamedKey::Unidentified);
        }

        // Regular character key
        Key::Character(chars_str.to_string())
    }
}

/// Extract modifiers from NSEvent
///
/// # Safety
///
/// `ns_event` must be a valid, live `NSEvent*`.
unsafe fn extract_modifiers(ns_event: id) -> Modifiers {
    // SAFETY: caller guarantees `ns_event` is a valid NSEvent*;
    // `modifierFlags` is a plain NSUInteger getter.
    unsafe {
        let flags: NSEventModifierFlags = msg_send![ns_event, modifierFlags];

        let mut modifiers = Modifiers::empty();

        if flags.contains(NSEventModifierFlags::NSShiftKeyMask) {
            modifiers.insert(Modifiers::SHIFT);
        }
        if flags.contains(NSEventModifierFlags::NSControlKeyMask) {
            modifiers.insert(Modifiers::CONTROL);
        }
        if flags.contains(NSEventModifierFlags::NSAlternateKeyMask) {
            modifiers.insert(Modifiers::ALT);
        }
        if flags.contains(NSEventModifierFlags::NSCommandKeyMask) {
            modifiers.insert(Modifiers::META); // Command = Meta
        }

        modifiers
    }
}

/// Convert macOS key code to Key
///
/// Reference: https://developer.apple.com/documentation/appkit/nsevent/1534513-keycode
fn key_code_to_key(key_code: u16) -> Option<Key> {
    let named = match key_code {
        // Arrow keys
        123 => NamedKey::ArrowLeft,
        124 => NamedKey::ArrowRight,
        125 => NamedKey::ArrowDown,
        126 => NamedKey::ArrowUp,

        // Function keys
        122 => NamedKey::F1,
        120 => NamedKey::F2,
        99 => NamedKey::F3,
        118 => NamedKey::F4,
        96 => NamedKey::F5,
        97 => NamedKey::F6,
        98 => NamedKey::F7,
        100 => NamedKey::F8,
        101 => NamedKey::F9,
        109 => NamedKey::F10,
        103 => NamedKey::F11,
        111 => NamedKey::F12,

        // Special keys
        36 => NamedKey::Enter,
        48 => NamedKey::Tab,
        51 => NamedKey::Backspace,
        53 => NamedKey::Escape,
        117 => NamedKey::Delete,
        115 => NamedKey::Home,
        119 => NamedKey::End,
        116 => NamedKey::PageUp,
        121 => NamedKey::PageDown,

        // Space
        49 => return Some(Key::Character(" ".to_string())),

        _ => return None,
    };
    Some(Key::Named(named))
}

// ============================================================================
// Mouse Event Conversion
// ============================================================================

/// Build a `PointerState` from an NSEvent's window-relative location.
///
/// # Safety
///
/// `ns_event` must be a valid, live `NSEvent*` of a mouse event.
unsafe fn pointer_state(ns_event: id, scale_factor: f64, view_height: f64) -> PointerState {
    // SAFETY: caller guarantees `ns_event` is a valid NSEvent*;
    // `locationInWindow` is a plain NSPoint getter.
    unsafe {
        // NSEvent.locationInWindow is already in logical coordinates with a
        // bottom-left origin; flip Y to the framework's top-left origin.
        let location: NSPoint = msg_send![ns_event, locationInWindow];
        let modifiers = extract_modifiers(ns_event);
        let buttons = extract_mouse_buttons();

        PointerState {
            time: event_timestamp_ms(),
            position: PhysicalPosition::new(location.x, view_height - location.y),
            buttons,
            modifiers,
            count: 1,
            contact_geometry: PhysicalSize::new(1.0, 1.0),
            orientation: Default::default(),
            pressure: 0.0,
            tangential_pressure: 0.0,
            scale_factor,
        }
    }
}

/// Convert mouse button event
///
/// # Safety
///
/// `ns_event` must be a valid, live `NSEvent*` of a mouse-button event.
unsafe fn convert_mouse_button(
    ns_event: id,
    scale_factor: f64,
    view_height: f64,
    button: PointerButton,
    is_down: bool,
) -> Option<PlatformInput> {
    // SAFETY: caller guarantees `ns_event` validity; forwarded to
    // `pointer_state` under the same contract.
    unsafe {
        let mut state = pointer_state(ns_event, scale_factor, view_height);
        state.pressure = if is_down { 0.5 } else { 0.0 };

        let button_event = PointerButtonEvent {
            pointer: primary_mouse_info(),
            state,
            button: Some(button),
        };

        let event = if is_down {
            PointerEvent::Down(button_event)
        } else {
            PointerEvent::Up(button_event)
        };

        Some(PlatformInput::Pointer(event))
    }
}

/// Convert mouse movement event
///
/// # Safety
///
/// `ns_event` must be a valid, live `NSEvent*` of a mouse-move event.
unsafe fn convert_mouse_move(
    ns_event: id,
    scale_factor: f64,
    view_height: f64,
) -> Option<PlatformInput> {
    // SAFETY: caller guarantees `ns_event` validity; forwarded to
    // `pointer_state` under the same contract.
    unsafe {
        let state = pointer_state(ns_event, scale_factor, view_height);

        Some(PlatformInput::Pointer(PointerEvent::Move(PointerUpdate {
            pointer: primary_mouse_info(),
            current: state,
            coalesced: Vec::new(),
            predicted: Vec::new(),
        })))
    }
}

/// Convert scroll wheel event
///
/// # Safety
///
/// `ns_event` must be a valid, live `NSEvent*` of a scroll-wheel event.
unsafe fn convert_scroll_event(
    ns_event: id,
    scale_factor: f64,
    view_height: f64,
) -> Option<PlatformInput> {
    // SAFETY: caller guarantees `ns_event` validity; `scrollingDeltaX/Y` and
    // `hasPreciseScrollingDeltas` are documented NSEvent getters.
    unsafe {
        let state = pointer_state(ns_event, scale_factor, view_height);

        // Get scroll delta
        let delta_x: f64 = msg_send![ns_event, scrollingDeltaX];
        let delta_y: f64 = msg_send![ns_event, scrollingDeltaY];

        // Check if precise scrolling (trackpad) or line scrolling (mouse wheel)
        let has_precise_delta: bool = msg_send![ns_event, hasPreciseScrollingDeltas];

        let delta = if has_precise_delta {
            // Trackpad: use pixel delta
            ScrollDelta::PixelDelta(PhysicalPosition::new(delta_x, delta_y))
        } else {
            // Mouse wheel: use line delta
            ScrollDelta::LineDelta(delta_x as f32, delta_y as f32)
        };

        Some(PlatformInput::Pointer(PointerEvent::Scroll(
            PointerScrollEvent {
                pointer: primary_mouse_info(),
                state,
                delta,
            },
        )))
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Extract global mouse button state via `NSEvent.pressedMouseButtons`
///
/// # Safety
///
/// Must be called with AppKit loaded (any process that created an NSEvent).
unsafe fn extract_mouse_buttons() -> PointerButtons {
    // SAFETY: `+[NSEvent pressedMouseButtons]` is a class method returning a
    // plain NSUInteger bitmask; no object lifetime is involved.
    unsafe {
        let buttons_mask: u64 = msg_send![class!(NSEvent), pressedMouseButtons];

        let mut buttons = PointerButtons::default();

        if (buttons_mask & 0x1) != 0 {
            buttons.insert(PointerButton::Primary);
        }
        if (buttons_mask & 0x2) != 0 {
            buttons.insert(PointerButton::Secondary);
        }
        if (buttons_mask & 0x4) != 0 {
            buttons.insert(PointerButton::Auxiliary);
        }

        buttons
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_code_mappings() {
        // Arrow keys
        assert_eq!(key_code_to_key(123), Some(Key::Named(NamedKey::ArrowLeft)));
        assert_eq!(key_code_to_key(124), Some(Key::Named(NamedKey::ArrowRight)));
        assert_eq!(key_code_to_key(125), Some(Key::Named(NamedKey::ArrowDown)));
        assert_eq!(key_code_to_key(126), Some(Key::Named(NamedKey::ArrowUp)));

        // Function keys
        assert_eq!(key_code_to_key(122), Some(Key::Named(NamedKey::F1)));
        assert_eq!(key_code_to_key(111), Some(Key::Named(NamedKey::F12)));

        // Special keys
        assert_eq!(key_code_to_key(36), Some(Key::Named(NamedKey::Enter)));
        assert_eq!(key_code_to_key(48), Some(Key::Named(NamedKey::Tab)));
        assert_eq!(key_code_to_key(51), Some(Key::Named(NamedKey::Backspace)));
        assert_eq!(key_code_to_key(53), Some(Key::Named(NamedKey::Escape)));

        // Space maps to a character key
        assert_eq!(key_code_to_key(49), Some(Key::Character(" ".to_string())));

        // Unknown key
        assert_eq!(key_code_to_key(999), None);
    }
}
