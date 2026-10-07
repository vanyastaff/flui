//! Owned input vocabulary shared with the platform boundary.
//!
//! Pointer samples carry checked positions and optional sensor readings. Device
//! lifecycle and cancellation events need not carry a position or contact identity.

use flui_foundation::geometry::Offset;

/// The platform's owned pointer vocabulary.
pub mod pointer {
    pub use flui_platform_api::pointer::*;
}

/// The platform's owned keyboard vocabulary.
pub mod keyboard {
    pub use flui_platform_api::keyboard::*;
}

pub use cursor_icon::CursorIcon;
pub use keyboard::{Code, Key, KeyEvent, KeyState, Modifiers, NamedKey};
pub use pointer::{
    ButtonChange, CancelReason, DeviceId, InputValueError, PanZoomEvent, PanZoomPhase,
    PanZoomTransform, PointerButton, PointerButtons, PointerCancel, PointerEvent, PointerId,
    PointerInfo, PointerKind, PointerMove, PointerPosition, PointerPress, PointerRelease,
    PointerRole, PointerSample, PointerSignal, ScrollDelta, ScrollEvent,
};

/// Extract the contact metadata when this event describes a contact.
#[must_use]
pub fn get_pointer_info(event: &PointerEvent) -> Option<&PointerInfo> {
    match event {
        PointerEvent::Down(data) => Some(&data.pointer),
        PointerEvent::Up(data) => Some(&data.pointer),
        PointerEvent::ButtonChange(ButtonChange::Pressed(data)) => Some(&data.pointer),
        PointerEvent::ButtonChange(ButtonChange::Released(data)) => Some(&data.pointer),
        PointerEvent::Move(data) => Some(&data.pointer),
        PointerEvent::Cancel(data) => Some(&data.pointer),
        PointerEvent::Enter(data)
        | PointerEvent::Leave(data)
        | PointerEvent::ScrollInertiaCancel(data) => Some(&data.pointer),
        PointerEvent::Scroll(data) => Some(&data.pointer),
        PointerEvent::PanZoom(data) => Some(data.pointer()),
        _ => None,
    }
}

/// Extract the current measured sample, excluding predictions.
#[must_use]
pub fn get_pointer_sample(event: &PointerEvent) -> Option<&PointerSample> {
    match event {
        PointerEvent::Down(data) => Some(&data.sample),
        PointerEvent::Up(data) => Some(&data.sample),
        PointerEvent::ButtonChange(ButtonChange::Pressed(data)) => Some(&data.sample),
        PointerEvent::ButtonChange(ButtonChange::Released(data)) => Some(&data.sample),
        PointerEvent::Move(data) => Some(data.current()),
        _ => None,
    }
}

/// Extract a contact identity without inventing one for device lifecycle events.
#[must_use]
pub fn extract_pointer_id(event: &PointerEvent) -> Option<PointerId> {
    get_pointer_info(event).map(|pointer| pointer.id)
}

/// Canonical geometry and identity queries for pointer events.
pub trait PointerEventExt {
    /// The reported position, when this event carries one.
    fn position(&self) -> Option<Offset<f64>>;
    /// The contact identity, absent for device lifecycle events.
    fn pointer_id(&self) -> Option<PointerId>;
    /// The device kind, including device lifecycle events.
    fn pointer_kind(&self) -> Option<PointerKind>;
    /// The hardware identity, when the platform reports it.
    fn device_id(&self) -> Option<DeviceId>;
}

impl PointerEventExt for PointerEvent {
    fn position(&self) -> Option<Offset<f64>> {
        let position = if let Some(sample) = get_pointer_sample(self) {
            Some(sample.position)
        } else {
            match self {
                PointerEvent::Enter(data)
                | PointerEvent::Leave(data)
                | PointerEvent::ScrollInertiaCancel(data) => data.position,
                PointerEvent::Scroll(data) => Some(data.position),
                PointerEvent::PanZoom(data) => Some(data.position),
                _ => None,
            }
        }?;
        let point = position.get();
        Some(Offset::new(point.x, point.y))
    }

    fn pointer_id(&self) -> Option<PointerId> {
        extract_pointer_id(self)
    }

    fn pointer_kind(&self) -> Option<PointerKind> {
        match self {
            PointerEvent::DeviceAdded(data) | PointerEvent::DeviceRemoved(data) => Some(data.kind),
            _ => get_pointer_info(self).map(|pointer| pointer.kind),
        }
    }

    fn device_id(&self) -> Option<DeviceId> {
        match self {
            PointerEvent::DeviceAdded(data) | PointerEvent::DeviceRemoved(data) => {
                Some(data.device)
            }
            _ => get_pointer_info(self).and_then(|pointer| pointer.device),
        }
    }
}

// ============================================================================
// Scroll event data (compatibility with existing code)
// ============================================================================

/// Scroll event data with position and delta.
///
/// This provides a simpler interface than [`PointerScrollEvent`] for
/// common scroll handling scenarios.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollEventData {
    /// Position where the scroll occurred.
    pub position: Offset<f64>,
    /// Scroll delta in pixels (converted from any scroll unit).
    pub delta: Offset<f64>,
    /// Keyboard modifiers active during scroll.
    pub modifiers: Modifiers,
}

impl ScrollEventData {
    /// Creates new scroll event data.
    pub fn new(position: Offset<f64>, delta: Offset<f64>, modifiers: Modifiers) -> Self {
        Self {
            position,
            delta,
            modifiers,
        }
    }

    /// Converts a ScrollDelta to a logical-pixel offset.
    ///
    /// This is the single owner of the line-height and page-height factors
    /// in the scroll delta contract (see the module docs): backends deliver
    /// normalized signs and units, and only this function turns lines and
    /// pages into pixels.
    pub fn delta_to_offset(delta: &ScrollDelta) -> Offset<f64> {
        match delta {
            ScrollDelta::PixelDelta(pos) => Offset::new(pos.x, pos.y),
            ScrollDelta::LineDelta(x, y) => {
                // One wheel line = 53 logical pixels — the factor commonly
                // applied to GTK scroll units, so wheel speed and
                // `InteractiveViewer`'s scroll-to-scale math feel the same
                // as other Linux UI toolkits tick for tick.
                Offset::new(f64::from(*x) * 53.0, f64::from(*y) * 53.0)
            }
            ScrollDelta::PageDelta(x, y) => {
                // Approximate: 1 page ≈ 400 pixels
                Offset::new(f64::from(*x) * 400.0, f64::from(*y) * 400.0)
            }
        }
    }
}

impl From<&PointerScrollEvent> for ScrollEventData {
    fn from(event: &PointerScrollEvent) -> Self {
        let pos = event.state.position;
        Self {
            position: Offset::new(pos.x, pos.y),
            delta: Self::delta_to_offset(&event.delta),
            modifiers: event.state.modifiers,
        }
    }
}

#[cfg(any(test, feature = "testing"))]
fn test_pointer_id() -> PointerId {
    PointerId::new(core::num::NonZeroU64::MIN)
}

#[cfg(any(test, feature = "testing"))]
fn test_sample(position: Offset<f64>) -> Result<PointerSample, InputValueError> {
    use flui_foundation::geometry::Point;
    Ok(PointerSample::new(
        flui_platform_api::EventTime::from_nanos(0),
        PointerPosition::try_new(Point::new(position.dx, position.dy))?,
    ))
}

/// Construct a checked synthetic Down with the fixture's default contact identity.
#[cfg(any(test, feature = "testing"))]
pub fn make_down_event(
    position: Offset<f64>,
    kind: PointerKind,
) -> Result<PointerEvent, InputValueError> {
    make_down_event_for_id(test_pointer_id(), position, kind)
}

/// Construct a checked synthetic Down for the specified contact.
#[cfg(any(test, feature = "testing"))]
pub fn make_down_event_for_id(
    id: PointerId,
    position: Offset<f64>,
    kind: PointerKind,
) -> Result<PointerEvent, InputValueError> {
    make_down_event_for_id_with_button(id, position, kind, PointerButton::PRIMARY)
}

/// Construct a checked synthetic Down for a specific button.
#[cfg(any(test, feature = "testing"))]
pub fn make_down_event_with_button(
    position: Offset<f64>,
    kind: PointerKind,
    button: PointerButton,
) -> Result<PointerEvent, InputValueError> {
    make_down_event_for_id_with_button(test_pointer_id(), position, kind, button)
}

/// Construct a checked synthetic Down for a contact and button.
#[cfg(any(test, feature = "testing"))]
pub fn make_down_event_for_id_with_button(
    id: PointerId,
    position: Offset<f64>,
    kind: PointerKind,
    button: PointerButton,
) -> Result<PointerEvent, InputValueError> {
    Ok(PointerEvent::Down(PointerPress::new(
        PointerInfo::new(id, kind).with_role(PointerRole::Primary),
        button,
        PointerButtons::NONE,
        test_sample(position)?,
    )))
}

/// Construct a checked synthetic Up with the fixture's default contact identity.
#[cfg(any(test, feature = "testing"))]
pub fn make_up_event(
    position: Offset<f64>,
    kind: PointerKind,
) -> Result<PointerEvent, InputValueError> {
    make_up_event_for_id(test_pointer_id(), position, kind)
}

/// Construct a checked synthetic Up for the specified contact.
#[cfg(any(test, feature = "testing"))]
pub fn make_up_event_for_id(
    id: PointerId,
    position: Offset<f64>,
    kind: PointerKind,
) -> Result<PointerEvent, InputValueError> {
    make_up_event_for_id_with_button(id, position, kind, PointerButton::PRIMARY)
}

/// Construct a checked synthetic Up for a specific button.
#[cfg(any(test, feature = "testing"))]
pub fn make_up_event_with_button(
    position: Offset<f64>,
    kind: PointerKind,
    button: PointerButton,
) -> Result<PointerEvent, InputValueError> {
    make_up_event_for_id_with_button(test_pointer_id(), position, kind, button)
}

/// Construct a checked synthetic Up for a contact and button.
#[cfg(any(test, feature = "testing"))]
pub fn make_up_event_for_id_with_button(
    id: PointerId,
    position: Offset<f64>,
    kind: PointerKind,
    button: PointerButton,
) -> Result<PointerEvent, InputValueError> {
    Ok(PointerEvent::Up(PointerRelease::new(
        PointerInfo::new(id, kind).with_role(PointerRole::Primary),
        button,
        PointerButtons::only(button),
        test_sample(position)?,
    )))
}

/// Construct a checked synthetic contact Move.
#[cfg(any(test, feature = "testing"))]
pub fn make_move_event(
    position: Offset<f64>,
    kind: PointerKind,
) -> Result<PointerEvent, InputValueError> {
    make_move_event_for_id(test_pointer_id(), position, kind)
}

/// Construct a checked synthetic contact Move for the specified contact.
#[cfg(any(test, feature = "testing"))]
pub fn make_move_event_for_id(
    id: PointerId,
    position: Offset<f64>,
    kind: PointerKind,
) -> Result<PointerEvent, InputValueError> {
    Ok(PointerEvent::Move(PointerMove::new(
        PointerInfo::new(id, kind).with_role(PointerRole::Primary),
        PointerButtons::only(PointerButton::PRIMARY),
        test_sample(position)?,
    )))
}

/// Construct a checked synthetic Move for a specific held button.
#[cfg(any(test, feature = "testing"))]
pub fn make_move_event_with_button(
    position: Offset<f64>,
    kind: PointerKind,
    button: PointerButton,
) -> Result<PointerEvent, InputValueError> {
    Ok(PointerEvent::Move(PointerMove::new(
        PointerInfo::new(test_pointer_id(), kind).with_role(PointerRole::Primary),
        PointerButtons::only(button),
        test_sample(position)?,
    )))
}

/// Construct a synthetic cancellation without inventing a position or sensor value.
#[cfg(any(test, feature = "testing"))]
pub fn make_cancel_event(kind: PointerKind) -> PointerEvent {
    make_cancel_event_for_id(test_pointer_id(), kind)
}

/// Construct a synthetic cancellation for the specified contact.
#[cfg(any(test, feature = "testing"))]
pub fn make_cancel_event_for_id(id: PointerId, kind: PointerKind) -> PointerEvent {
    PointerEvent::Cancel(PointerCancel::new(
        PointerInfo::new(id, kind).with_role(PointerRole::Primary),
        flui_platform_api::EventTime::from_nanos(0),
        CancelReason::Platform,
    ))
}
