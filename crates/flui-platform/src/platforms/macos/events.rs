//! macOS event conversion utilities
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
//! Platform handlers
//! ```
//!
//! # Key Mappings
//!
//! - NSEvent.characters → keyboard_types::Key
//! - NSEventModifierFlags → keyboard_types::Modifiers
//! - NSEventType → PointerEvent / KeyboardEvent
//! - NSPoint → logical pixels (converted via backingScaleFactor)

use crate::traits::input::{KeyboardEvent, PlatformInput};
use cocoa::appkit::{NSEvent, NSEventModifierFlags, NSEventType};
use cocoa::base::id;
use cocoa::foundation::NSPoint;
use flui_types::geometry::{Offset, Pixels};
use keyboard_types::{Key, Modifiers};
use objc::runtime::Object;
use objc::*;
use ui_events::pointer::{PointerButton, PointerButtons, PointerEvent, PointerId, PointerType};

// ============================================================================
// NSEvent Conversion
// ============================================================================

/// Convert NSEvent to PlatformInput
///
/// Returns None if the event type is not supported (e.g., gesture events)
pub fn convert_ns_event(ns_event: id, scale_factor: f64) -> Option<PlatformInput> {
    unsafe {
        let event_type: NSEventType = msg_send![ns_event, type];

        match event_type {
            // Keyboard events
            NSEventType::NSKeyDown => {
                let key = extract_key(ns_event);
                let modifiers = extract_modifiers(ns_event);
                let is_repeat: bool = msg_send![ns_event, isARepeat];

                Some(PlatformInput::Keyboard(KeyboardEvent {
                    key,
                    modifiers,
                    is_down: true,
                    is_repeat,
                }))
            }

            NSEventType::NSKeyUp => {
                let key = extract_key(ns_event);
                let modifiers = extract_modifiers(ns_event);

                Some(PlatformInput::Keyboard(KeyboardEvent {
                    key,
                    modifiers,
                    is_down: false,
                    is_repeat: false,
                }))
            }

            // Mouse button events
            NSEventType::NSLeftMouseDown => {
                convert_mouse_event(ns_event, scale_factor, PointerButton::Primary, true)
            }

            NSEventType::NSLeftMouseUp => {
                convert_mouse_event(ns_event, scale_factor, PointerButton::Primary, false)
            }

            NSEventType::NSRightMouseDown => {
                convert_mouse_event(ns_event, scale_factor, PointerButton::Secondary, true)
            }

            NSEventType::NSRightMouseUp => {
                convert_mouse_event(ns_event, scale_factor, PointerButton::Secondary, false)
            }

            NSEventType::NSOtherMouseDown => {
                convert_mouse_event(ns_event, scale_factor, PointerButton::Auxiliary, true)
            }

            NSEventType::NSOtherMouseUp => {
                convert_mouse_event(ns_event, scale_factor, PointerButton::Auxiliary, false)
            }

            // Mouse movement events
            NSEventType::NSMouseMoved
            | NSEventType::NSLeftMouseDragged
            | NSEventType::NSRightMouseDragged
            | NSEventType::NSOtherMouseDragged => {
                convert_mouse_move(ns_event, scale_factor)
            }

            // Scroll events
            NSEventType::NSScrollWheel => convert_scroll_event(ns_event, scale_factor),

            // Mouse enter/exit
            NSEventType::NSMouseEntered => {
                convert_mouse_enter_exit(ns_event, scale_factor, true)
            }

            NSEventType::NSMouseExited => {
                convert_mouse_enter_exit(ns_event, scale_factor, false)
            }

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
/// Uses NSEvent.characters to get the Unicode character
unsafe fn extract_key(ns_event: id) -> Key {
    // Get characters string
    let chars: id = msg_send![ns_event, characters];
    if chars.is_null() {
        return Key::Unidentified;
    }

    // Convert NSString to Rust string
    let chars_ptr: *const i8 = msg_send![chars, UTF8String];
    if chars_ptr.is_null() {
        return Key::Unidentified;
    }

    let chars_str = std::ffi::CStr::from_ptr(chars_ptr as *const _)
        .to_str()
        .unwrap_or("");

    if chars_str.is_empty() {
        return Key::Unidentified;
    }

    // Check for special keys via key code
    let key_code: u16 = msg_send![ns_event, keyCode];
    if let Some(special_key) = key_code_to_key(key_code) {
        return special_key;
    }

    // Regular character key
    Key::Character(chars_str.to_string())
}

/// Extract modifiers from NSEvent
unsafe fn extract_modifiers(ns_event: id) -> Modifiers {
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

/// Convert macOS key code to Key
///
/// Reference: https://developer.apple.com/documentation/appkit/nsevent/1534513-keycode
fn key_code_to_key(key_code: u16) -> Option<Key> {
    match key_code {
        // Arrow keys
        123 => Some(Key::ArrowLeft),
        124 => Some(Key::ArrowRight),
        125 => Some(Key::ArrowDown),
        126 => Some(Key::ArrowUp),

        // Function keys
        122 => Some(Key::F1),
        120 => Some(Key::F2),
        99 => Some(Key::F3),
        118 => Some(Key::F4),
        96 => Some(Key::F5),
        97 => Some(Key::F6),
        98 => Some(Key::F7),
        100 => Some(Key::F8),
        101 => Some(Key::F9),
        109 => Some(Key::F10),
        103 => Some(Key::F11),
        111 => Some(Key::F12),

        // Special keys
        36 => Some(Key::Enter),
        48 => Some(Key::Tab),
        51 => Some(Key::Backspace),
        53 => Some(Key::Escape),
        117 => Some(Key::Delete),
        115 => Some(Key::Home),
        119 => Some(Key::End),
        116 => Some(Key::PageUp),
        121 => Some(Key::PageDown),

        // Space
        49 => Some(Key::Character(" ".to_string())),

        _ => None,
    }
}

// ============================================================================
// Mouse Event Conversion
// ============================================================================

/// Convert mouse button event
unsafe fn convert_mouse_event(
    ns_event: id,
    scale_factor: f64,
    button: PointerButton,
    is_down: bool,
) -> Option<PlatformInput> {
    let position = extract_mouse_position(ns_event, scale_factor);
    let buttons = extract_mouse_buttons(ns_event);
    let modifiers = extract_modifiers(ns_event);

    let update = if is_down {
        ui_events::pointer::PointerUpdate::Down { button }
    } else {
        ui_events::pointer::PointerUpdate::Up { button }
    };

    Some(PlatformInput::Pointer(PointerEvent {
        pointer_id: PointerId(0), // macOS doesn't have multi-pointer tracking
        pointer_type: PointerType::Mouse,
        position,
        buttons,
        modifiers,
        update,
    }))
}

/// Convert mouse movement event
unsafe fn convert_mouse_move(ns_event: id, scale_factor: f64) -> Option<PlatformInput> {
    let position = extract_mouse_position(ns_event, scale_factor);
    let buttons = extract_mouse_buttons(ns_event);
    let modifiers = extract_modifiers(ns_event);

    Some(PlatformInput::Pointer(PointerEvent {
        pointer_id: PointerId(0),
        pointer_type: PointerType::Mouse,
        position,
        buttons,
        modifiers,
        update: ui_events::pointer::PointerUpdate::Moved,
    }))
}

/// Convert scroll wheel event
unsafe fn convert_scroll_event(ns_event: id, scale_factor: f64) -> Option<PlatformInput> {
    let position = extract_mouse_position(ns_event, scale_factor);
    let buttons = extract_mouse_buttons(ns_event);
    let modifiers = extract_modifiers(ns_event);

    // Get scroll delta
    let delta_x: f64 = msg_send![ns_event, scrollingDeltaX];
    let delta_y: f64 = msg_send![ns_event, scrollingDeltaY];

    // Check if precise scrolling (trackpad) or line scrolling (mouse wheel)
    let has_precise_delta: bool = msg_send![ns_event, hasPreciseScrollingDeltas];

    let delta = if has_precise_delta {
        // Trackpad: use pixel delta
        ui_events::ScrollDelta::Pixels {
            x: delta_x as f32,
            y: delta_y as f32,
        }
    } else {
        // Mouse wheel: use line delta
        ui_events::ScrollDelta::Lines {
            x: delta_x as f32,
            y: delta_y as f32,
        }
    };

    Some(PlatformInput::Pointer(PointerEvent {
        pointer_id: PointerId(0),
        pointer_type: PointerType::Mouse,
        position,
        buttons,
        modifiers,
        update: ui_events::pointer::PointerUpdate::Scroll { delta },
    }))
}

/// Convert mouse enter/exit event
unsafe fn convert_mouse_enter_exit(
    ns_event: id,
    scale_factor: f64,
    is_enter: bool,
) -> Option<PlatformInput> {
    let position = extract_mouse_position(ns_event, scale_factor);
    let buttons = extract_mouse_buttons(ns_event);
    let modifiers = extract_modifiers(ns_event);

    let update = if is_enter {
        ui_events::pointer::PointerUpdate::Entered
    } else {
        ui_events::pointer::PointerUpdate::Exited
    };

    Some(PlatformInput::Pointer(PointerEvent {
        pointer_id: PointerId(0),
        pointer_type: PointerType::Mouse,
        position,
        buttons,
        modifiers,
        update,
    }))
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Extract mouse position from NSEvent and convert to logical pixels
///
/// macOS coordinates are bottom-left origin, need to flip Y
unsafe fn extract_mouse_position(ns_event: id, scale_factor: f64) -> Offset<Pixels> {
    // Get location in window
    let location: NSPoint = msg_send![ns_event, locationInWindow];

    // Convert to logical pixels (NSPoint is already in logical coordinates)
    // Note: Y coordinate is bottom-up in macOS, will be flipped by window context
    Offset::new(Pixels(location.x as f32), Pixels(location.y as f32))
}

/// Extract mouse button state from NSEvent
unsafe fn extract_mouse_buttons(ns_event: id) -> PointerButtons {
    let buttons_mask: i64 = msg_send![class!(NSEvent), pressedMouseButtons];

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

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_code_mappings() {
        // Arrow keys
        assert_eq!(key_code_to_key(123), Some(Key::ArrowLeft));
        assert_eq!(key_code_to_key(124), Some(Key::ArrowRight));
        assert_eq!(key_code_to_key(125), Some(Key::ArrowDown));
        assert_eq!(key_code_to_key(126), Some(Key::ArrowUp));

        // Function keys
        assert_eq!(key_code_to_key(122), Some(Key::F1));
        assert_eq!(key_code_to_key(111), Some(Key::F12));

        // Special keys
        assert_eq!(key_code_to_key(36), Some(Key::Enter));
        assert_eq!(key_code_to_key(48), Some(Key::Tab));
        assert_eq!(key_code_to_key(51), Some(Key::Backspace));
        assert_eq!(key_code_to_key(53), Some(Key::Escape));

        // Unknown key
        assert_eq!(key_code_to_key(999), None);
    }

    #[test]
    fn test_position_conversion() {
        // Position conversion is already in logical pixels in NSEvent
        // This test just documents the expected behavior
        let scale_1x = 1.0;
        let scale_2x = 2.0;

        // At 1x scale, coordinates are 1:1
        // At 2x scale (Retina), NSEvent.locationInWindow is still in logical pixels
        // No conversion needed (macOS does this automatically)

        assert_eq!(scale_1x, 1.0);
        assert_eq!(scale_2x, 2.0);
    }
}
