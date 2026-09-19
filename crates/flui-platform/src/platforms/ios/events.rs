//! iOS touch event conversion.
//!
//! Converts a UIKit `UITouch` (and its `NSSet` batch) into the
//! platform-agnostic `PlatformInput` types — the W3C-compliant `ui-events`
//! vocabulary every backend speaks.
//!
//! # Mapping
//!
//! ```text
//! touchesBegan:     → PointerEvent::Down   (one per touch)
//! touchesMoved:     → PointerEvent::Move
//! touchesEnded:     → PointerEvent::Up
//! touchesCancelled: → PointerEvent::Cancel
//! ```
//!
//! Coordinates come back from `locationInView:` in the view's point space
//! (top-left origin, matching the framework's convention), so the only
//! transform is the `scale` the caller supplies for
//! [`PointerState::scale_factor`].
//!
//! # Force and pen
//!
//! A touch's `force` is its pressure (0.0–1.0 for a plain finger on hardware
//! that reports it), and an Apple Pencil reports `UITouchTypeStylus` with a
//! real `azimuthAngleInView:`. Both map straight onto the `ui-events` fields
//! that carry the same meaning.

use dpi::PhysicalPosition;
use keyboard_types::Modifiers;
use objc2_ui_kit::{UITouch, UITouchType};
use ui_events::pointer::{
    ContactGeometry, PointerButton, PointerButtonEvent, PointerButtons, PointerEvent, PointerId,
    PointerInfo, PointerOrientation, PointerState, PointerType, PointerUpdate,
};

use crate::traits::PlatformInput;

/// The phase a batch of touches was delivered under, so a multi-touch batch
/// knows which callback it came from even though each `UITouch` carries its
/// own (equal) `phase`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TouchPhase {
    Down,
    Move,
    Up,
    Cancel,
}

/// Convert one `UITouch` into a single `PointerEvent`.
///
/// `scale` is the view's `contentScaleFactor`, recorded on the event so a
/// consumer can convert the point-space position to device pixels.
pub(super) fn touch_to_pointer_events(
    touch: &UITouch,
    phase: TouchPhase,
    scale: f64,
) -> Vec<PlatformInput> {
    let info = pointer_info(touch);
    let state = pointer_state(touch, phase, scale);

    let event = match phase {
        TouchPhase::Down => PointerEvent::Down(PointerButtonEvent {
            button: Some(PointerButton::Primary),
            pointer: info,
            state,
        }),
        TouchPhase::Up => PointerEvent::Up(PointerButtonEvent {
            button: Some(PointerButton::Primary),
            pointer: info,
            state,
        }),
        TouchPhase::Move => PointerEvent::Move(PointerUpdate {
            pointer: info,
            current: state,
            coalesced: Vec::new(),
            predicted: Vec::new(),
        }),
        TouchPhase::Cancel => PointerEvent::Cancel(info),
    };

    vec![PlatformInput::Pointer(event)]
}

/// Stable pointer identity for a `UITouch`.
///
/// UIKit's `UITouch` objects are recycled by the system, but within a single
/// touch's lifetime the same object represents the same finger — which is
/// exactly the identity a recognizer needs. The pointer's address is that
/// identity, offset by one because `PointerId::PRIMARY` is 1
/// (`NonZeroU64::MIN`) and Android's own conversion reserves it the same way.
fn pointer_info(touch: &UITouch) -> PointerInfo {
    let address = std::ptr::from_ref::<UITouch>(touch) as u64;
    PointerInfo {
        pointer_id: PointerId::new(address.wrapping_add(1) | 1),
        persistent_device_id: None,
        pointer_type: pointer_type(touch),
    }
}

/// W3C pointer type from UIKit's touch kind.
fn pointer_type(touch: &UITouch) -> PointerType {
    let kind = touch.r#type();
    if kind == UITouchType::Stylus {
        PointerType::Pen
    } else if kind == UITouchType::IndirectPointer {
        // A trackpad/mouse-derived touch (iPad pointer support).
        PointerType::Mouse
    } else {
        // `Direct` (a finger) and `Indirect` (an indirect touch) both read
        // as touch to the framework.
        PointerType::Touch
    }
}

/// Build the W3C `PointerState` for a touch.
///
/// `buttons` follows the W3C contract — the set held *after* this event, so a
/// Down and a Move report the primary button, an Up reports an empty set.
/// The framework's contact-vs-hover discrimination reads it, so a drag
/// without it would be delivered as a hover.
fn pointer_state(touch: &UITouch, phase: TouchPhase, scale: f64) -> PointerState {
    let location = touch.locationInView(None);

    let mut buttons = PointerButtons::default();
    if matches!(phase, TouchPhase::Down | TouchPhase::Move) {
        buttons.insert(PointerButton::Primary);
    }

    // `force` is pressure on hardware that reports it (0.0 otherwise). UIKit
    // does not report tangential pressure; 0.0 is the honest value.
    // `PointerState` carries both as `f32`.
    let pressure = touch.force() as f32;

    // UIKit reports no contact-area size; a 1x1 point contact is the neutral
    // value the framework expects when a backend cannot measure it.
    let contact = ContactGeometry {
        width: 1.0,
        height: 1.0,
    };

    // A Pencil's tilt is an azimuth around the surface normal; UITouch
    // exposes it directly, and `PointerOrientation` carries the two angles.
    let orientation = if pointer_type(touch) == PointerType::Pen {
        PointerOrientation {
            altitude: 0.0,
            azimuth: touch.azimuthAngleInView(None) as f32,
        }
    } else {
        PointerOrientation::default()
    };

    PointerState {
        time: (touch.timestamp() * 1_000_000_000.0) as u64,
        position: PhysicalPosition::new(location.x, location.y),
        buttons,
        modifiers: Modifiers::empty(),
        count: u8::from(matches!(phase, TouchPhase::Down | TouchPhase::Up)),
        contact_geometry: contact,
        orientation,
        pressure,
        tangential_pressure: 0.0,
        scale_factor: scale,
    }
}
