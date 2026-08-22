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
//! - NSEvent.characters / keyCode → keyboard_types::Key (named-key
//!   intercepts and the physical-key `Code` table live in
//!   `crate::shared::keys_macos`, where their tests execute on any host)
//! - NSEventModifierFlags → keyboard_types::Modifiers
//! - NSEventType → PointerEvent / KeyboardEvent
//! - NSPoint → logical pixels (NSEvent coordinates are already logical;
//!   the Y axis is flipped from bottom-left to top-left origin)

use cocoa::{
    appkit::{NSEventModifierFlags, NSEventType},
    base::id,
    foundation::NSPoint,
};
use dpi::{PhysicalPosition, PhysicalSize};
use keyboard_types::{Key, Modifiers, NamedKey};
use objc::{class, msg_send, sel, sel_impl};
use ui_events::{
    keyboard::{KeyState, KeyboardEvent},
    pointer::{
        PointerButton, PointerButtonEvent, PointerButtons, PointerEvent, PointerInfo,
        PointerOrientation, PointerScrollEvent, PointerState, PointerType, PointerUpdate,
    },
};

use crate::{
    shared::events::{event_timestamp_ns, primary_mouse_info},
    traits::PlatformInput,
};

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
                let key_code: u16 = msg_send![ns_event, keyCode];
                let key = extract_key(ns_event, key_code);
                let code = crate::shared::keys_macos::keycode_to_code(key_code);
                let modifiers = extract_modifiers(ns_event);
                let is_repeat: bool = msg_send![ns_event, isARepeat];

                Some(PlatformInput::Keyboard(KeyboardEvent {
                    state: KeyState::Down,
                    key,
                    code,
                    location: crate::shared::keys::location_for_code(code),
                    modifiers,
                    repeat: is_repeat,
                    is_composing: false,
                }))
            }

            NSEventType::NSKeyUp => {
                let key_code: u16 = msg_send![ns_event, keyCode];
                let key = extract_key(ns_event, key_code);
                let code = crate::shared::keys_macos::keycode_to_code(key_code);
                let modifiers = extract_modifiers(ns_event);

                Some(PlatformInput::Keyboard(KeyboardEvent {
                    state: KeyState::Up,
                    key,
                    code,
                    location: crate::shared::keys::location_for_code(code),
                    modifiers,
                    repeat: false,
                    is_composing: false,
                }))
            }

            // Mouse button events
            NSEventType::NSLeftMouseDown => Some(convert_mouse_button(
                ns_event,
                scale_factor,
                view_height,
                PointerButton::Primary,
                true,
            )),

            NSEventType::NSLeftMouseUp => Some(convert_mouse_button(
                ns_event,
                scale_factor,
                view_height,
                PointerButton::Primary,
                false,
            )),

            NSEventType::NSRightMouseDown => Some(convert_mouse_button(
                ns_event,
                scale_factor,
                view_height,
                PointerButton::Secondary,
                true,
            )),

            NSEventType::NSRightMouseUp => Some(convert_mouse_button(
                ns_event,
                scale_factor,
                view_height,
                PointerButton::Secondary,
                false,
            )),

            NSEventType::NSOtherMouseDown => Some(convert_mouse_button(
                ns_event,
                scale_factor,
                view_height,
                PointerButton::Auxiliary,
                true,
            )),

            NSEventType::NSOtherMouseUp => Some(convert_mouse_button(
                ns_event,
                scale_factor,
                view_height,
                PointerButton::Auxiliary,
                false,
            )),

            // Mouse movement events
            NSEventType::NSMouseMoved
            | NSEventType::NSLeftMouseDragged
            | NSEventType::NSRightMouseDragged
            | NSEventType::NSOtherMouseDragged => {
                Some(convert_mouse_move(ns_event, scale_factor, view_height))
            }

            // Scroll events
            NSEventType::NSScrollWheel => {
                Some(convert_scroll_event(ns_event, scale_factor, view_height))
            }

            // Trackpad pinch/rotation — the native producer for the pan-zoom
            // lane (ordinary macOS apps select THIS backend, not winit).
            // `magnification` is the fraction the shared conversion expects;
            // `rotation` is counterclockwise degrees, converted to the
            // lane's clockwise radians in the same shared function the winit
            // boundary uses, so the two backends cannot drift.
            NSEventType::NSEventTypeMagnify => {
                let magnification: f64 = msg_send![ns_event, magnification];
                crate::shared::gestures::pinch(magnification).map(|gesture| {
                    convert_gesture_event(ns_event, scale_factor, view_height, gesture)
                })
            }
            NSEventType::NSEventTypeRotate => {
                let degrees: f32 = msg_send![ns_event, rotation];
                Some(convert_gesture_event(
                    ns_event,
                    scale_factor,
                    view_height,
                    crate::shared::gestures::rotation_ccw_degrees(degrees),
                ))
            }

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
/// The shared named-key table (`shared::keys_macos`) intercepts keys whose
/// `NSEvent.characters` are control characters or private-use-area
/// codepoints; everything else types through `characters`, the layout- and
/// modifier-aware translation.
///
/// # Safety
///
/// `ns_event` must be a valid, live `NSEvent*` of a keyboard event, and
/// `key_code` must be its `keyCode`.
unsafe fn extract_key(ns_event: id, key_code: u16) -> Key {
    // SAFETY: caller guarantees `ns_event` is a valid NSEvent*; `characters`
    // returns an autoreleased NSString whose UTF8String buffer is valid while
    // the event is alive (we copy it into an owned String before returning).
    unsafe {
        // Check for special keys via key code first
        if let Some(special_key) = crate::shared::keys_macos::keycode_to_key(key_code) {
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

        let chars_str = std::ffi::CStr::from_ptr(chars_ptr.cast())
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

// ============================================================================
// Mouse Event Conversion
// ============================================================================

/// Build a `PointerState` from an NSEvent's window-relative location.
///
/// # Safety
///
/// `ns_event` must be a valid, live `NSEvent*` of a mouse event.
/// `count` is the W3C click count — `1` on Down/Up transitions, `0`
/// elsewhere; `pressure` follows the sensor-less rule (`0.5` while any
/// button is held per the live `pressedMouseButtons` state, `0.0`
/// otherwise) — the cross-wire contract in flui-interaction's module doc.
unsafe fn pointer_state(
    ns_event: id,
    scale_factor: f64,
    view_height: f64,
    count: u8,
) -> PointerState {
    // SAFETY: caller guarantees `ns_event` is a valid NSEvent*;
    // `locationInWindow` is a plain NSPoint getter.
    unsafe {
        // NSEvent.locationInWindow is already in logical coordinates with a
        // bottom-left origin; flip Y to the framework's top-left origin.
        let location: NSPoint = msg_send![ns_event, locationInWindow];
        let modifiers = extract_modifiers(ns_event);
        let buttons = extract_mouse_buttons();
        let pressure = if buttons == PointerButtons::default() {
            0.0
        } else {
            0.5
        };

        PointerState {
            time: event_timestamp_ns(),
            position: PhysicalPosition::new(location.x, view_height - location.y),
            buttons,
            modifiers,
            count,
            contact_geometry: PhysicalSize::new(1.0, 1.0),
            orientation: PointerOrientation::default(),
            pressure,
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
) -> PlatformInput {
    // SAFETY: caller guarantees `ns_event` validity; forwarded to
    // `pointer_state` under the same contract.
    unsafe {
        // The held-set-derived pressure is already right for both edges:
        // `pressedMouseButtons` includes the pressed button at Down and
        // excludes it at Up.
        let state = pointer_state(ns_event, scale_factor, view_height, 1);

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

        PlatformInput::Pointer(event)
    }
}

/// Convert mouse movement event
///
/// # Safety
///
/// `ns_event` must be a valid, live `NSEvent*` of a mouse-move event.
unsafe fn convert_mouse_move(ns_event: id, scale_factor: f64, view_height: f64) -> PlatformInput {
    // SAFETY: caller guarantees `ns_event` validity; forwarded to
    // `pointer_state` under the same contract.
    unsafe {
        let state = pointer_state(ns_event, scale_factor, view_height, 0);

        PlatformInput::Pointer(PointerEvent::Move(PointerUpdate {
            pointer: primary_mouse_info(),
            current: state,
            coalesced: Vec::new(),
            predicted: Vec::new(),
        }))
    }
}

/// Convert scroll wheel event
///
/// AppKit's `scrollingDeltaX/Y` are positive for a swipe/scroll UP or LEFT
/// (Apple's NSEvent docs: same sign as the legacy `deltaX/deltaY`; the
/// system applies the natural-scrolling preference before delivery) — the
/// inverse of the cross-backend convention (positive = content scrolls
/// down/right), so `from_appkit` negates both axes at this boundary. Precise
/// (trackpad) deltas are points, which are already logical pixels — no
/// scale-factor conversion. See `crate::shared::scroll` for the sign/unit
/// table and citations.
///
/// # Safety
///
/// `ns_event` must be a valid, live `NSEvent*` of a scroll-wheel event.
unsafe fn convert_scroll_event(ns_event: id, scale_factor: f64, view_height: f64) -> PlatformInput {
    // SAFETY: caller guarantees `ns_event` validity; `scrollingDeltaX/Y` and
    // `hasPreciseScrollingDeltas` are documented NSEvent getters.
    unsafe {
        let state = pointer_state(ns_event, scale_factor, view_height, 0);

        let delta_x: f64 = msg_send![ns_event, scrollingDeltaX];
        let delta_y: f64 = msg_send![ns_event, scrollingDeltaY];

        // Precise scrolling (trackpad) delivers point deltas; a conventional
        // mouse wheel delivers whole lines.
        let has_precise_delta: bool = msg_send![ns_event, hasPreciseScrollingDeltas];

        let delta = crate::shared::scroll::from_appkit(delta_x, delta_y, has_precise_delta);

        PlatformInput::Pointer(PointerEvent::Scroll(PointerScrollEvent {
            pointer: primary_mouse_info(),
            state,
            delta,
        }))
    }
}

/// Assemble a `PointerEvent::Gesture` around an already-converted
/// [`PointerGesture`] at the event's own location, with the shared
/// synthetic gesture identity (see `shared::gestures`).
///
/// # Safety
///
/// Caller guarantees `ns_event` validity; `pointer_state` reads only
/// documented NSEvent getters.
unsafe fn convert_gesture_event(
    ns_event: id,
    scale_factor: f64,
    view_height: f64,
    gesture: ui_events::pointer::PointerGesture,
) -> PlatformInput {
    // SAFETY: forwarded caller guarantee.
    let state = unsafe { pointer_state(ns_event, scale_factor, view_height, 0) };
    PlatformInput::Pointer(PointerEvent::Gesture(
        ui_events::pointer::PointerGestureEvent {
            pointer: PointerInfo {
                pointer_id: ui_events::pointer::PointerId::new(
                    crate::shared::gestures::GESTURE_POINTER_ID,
                ),
                pointer_type: PointerType::Touch,
                persistent_device_id: None,
            },
            gesture,
            state,
        },
    ))
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
