//! Owned input vocabulary shared with the platform boundary.
//!
//! Pointer samples carry checked positions and optional sensor readings. Device
//! lifecycle and cancellation events need not carry a position or contact identity.

use flui_foundation::geometry::Offset;

/// The platform's owned pointer vocabulary.
pub mod pointer {
    pub use flui_platform::pointer::*;
}

/// The platform's owned keyboard vocabulary.
pub mod keyboard {
    pub use flui_platform::keyboard::*;
}

pub use cursor_icon::CursorIcon;
pub use keyboard::{Code, Key, KeyEvent, KeyState, Modifiers, NamedKey};
pub use pointer::{
    ButtonChange, CancelReason, DeviceId, InputValueError, PanZoomEvent, PanZoomPhase,
    PanZoomTransform, PointerButton, PointerButtons, PointerCancel, PointerEvent, PointerId,
    PointerInfo, PointerKind, PointerMove, PointerPosition, PointerPress, PointerRelease,
    PointerRole, PointerSample, PointerSignal, ScrollDelta, ScrollEvent, SmartZoomEvent,
};

/// The interaction clock: `clock`'s current reading on the event timeline
/// (ADR-0207 §2).
///
/// Every arena and recognizer deadline is an [`EventTime`](flui_platform::EventTime)
/// compared with this reading, so a recording replays to the same decisions
/// once the clock is driven to its arrival times.
pub(crate) fn interaction_now(
    clock: &dyn flui_foundation::MonotonicClock,
) -> flui_platform::EventTime {
    let elapsed = clock.now().saturating_duration_since(clock.epoch());
    flui_platform::EventTime::from_nanos(u64::try_from(elapsed.as_nanos()).unwrap_or(u64::MAX))
}

/// `instant` on `clock`'s event timeline: the inverse of [`instant_at`], for
/// a runtime that samples its frame clock once and hands the reading down.
pub(crate) fn event_time_of(
    clock: &dyn flui_foundation::MonotonicClock,
    instant: web_time::Instant,
) -> flui_platform::EventTime {
    let elapsed = instant.saturating_duration_since(clock.epoch());
    flui_platform::EventTime::from_nanos(u64::try_from(elapsed.as_nanos()).unwrap_or(u64::MAX))
}

/// The instant `time` names on `clock`'s timeline, for consumers that keep
/// [`web_time::Instant`]s (velocity samples, wake scheduling); `None` when it
/// lies past the last instant this platform represents.
pub(crate) fn instant_at(
    clock: &dyn flui_foundation::MonotonicClock,
    time: flui_platform::EventTime,
) -> Option<web_time::Instant> {
    clock
        .epoch()
        .checked_add(core::time::Duration::from_nanos(time.as_nanos()))
}

/// Extract the contact metadata when this event describes a contact.
#[must_use]
pub(crate) fn pointer_info(event: &PointerEvent) -> Option<&PointerInfo> {
    match event {
        PointerEvent::Down(data) | PointerEvent::ButtonChange(ButtonChange::Pressed(data)) => {
            Some(&data.pointer)
        }
        PointerEvent::Up(data) | PointerEvent::ButtonChange(ButtonChange::Released(data)) => {
            Some(&data.pointer)
        }
        PointerEvent::Move(data) => Some(&data.pointer),
        PointerEvent::Cancel(data) => Some(&data.pointer),
        PointerEvent::Enter(data)
        | PointerEvent::Leave(data)
        | PointerEvent::ScrollInertiaCancel(data) => Some(&data.pointer),
        PointerEvent::Scroll(data) => Some(&data.pointer),
        PointerEvent::PanZoom(data) => Some(data.pointer()),
        PointerEvent::SmartZoom(data) => Some(data.pointer()),
        PointerEvent::Revise(data) => Some(&data.pointer),
        PointerEvent::PenSqueeze(data) => Some(&data.pointer),
        // Device lifecycle names a device, not a contact; a variant added
        // later carries no contact until it is named above.
        _ => None,
    }
}

/// Extract the current measured sample, excluding predictions.
#[must_use]
fn pointer_sample(event: &PointerEvent) -> Option<&PointerSample> {
    match event {
        PointerEvent::Down(data) | PointerEvent::ButtonChange(ButtonChange::Pressed(data)) => {
            Some(&data.sample)
        }
        PointerEvent::Up(data) | PointerEvent::ButtonChange(ButtonChange::Released(data)) => {
            Some(&data.sample)
        }
        PointerEvent::Move(data) => Some(data.current()),
        PointerEvent::Revise(data) => Some(&data.sample),
        PointerEvent::PenSqueeze(data) => data.hover.as_ref(),
        // The others carry a position without sensors, or none.
        _ => None,
    }
}

/// Extract the platform's timestamp, including the valid epoch value zero.
#[must_use]
pub(crate) fn event_time(event: &PointerEvent) -> Option<flui_platform::EventTime> {
    Some(match event {
        PointerEvent::Down(data) | PointerEvent::ButtonChange(ButtonChange::Pressed(data)) => {
            data.sample.time
        }
        PointerEvent::Up(data) | PointerEvent::ButtonChange(ButtonChange::Released(data)) => {
            data.sample.time
        }
        PointerEvent::Move(data) => data.current().time,
        PointerEvent::Scroll(data) => data.time,
        PointerEvent::PanZoom(data) => data.time,
        PointerEvent::SmartZoom(data) => data.time,
        PointerEvent::Cancel(data) => data.time,
        PointerEvent::Enter(data)
        | PointerEvent::Leave(data)
        | PointerEvent::ScrollInertiaCancel(data) => data.time,
        PointerEvent::DeviceAdded(data) | PointerEvent::DeviceRemoved(data) => data.time,
        PointerEvent::Revise(data) => data.sample.time,
        PointerEvent::PenSqueeze(data) => data.time,
        // A variant added later has no known time until it is named here.
        _ => return None,
    })
}

mod sealed {
    pub trait Sealed {}

    impl Sealed for super::PointerEvent {}
}

/// Canonical geometry, identity and time queries for owned pointer events.
///
/// Sealed because these queries describe the platform's owned event vocabulary.
pub trait PointerEventExt: sealed::Sealed {
    /// The reported event timestamp, including the valid epoch value zero.
    #[must_use]
    fn time(&self) -> Option<flui_platform::EventTime>;
    /// The reported position, when this event carries one.
    #[must_use]
    fn position(&self) -> Option<Offset<f64>>;
    /// The contact identity, absent for device lifecycle events.
    #[must_use]
    fn pointer_id(&self) -> Option<PointerId>;
    /// The device kind, including device lifecycle events.
    #[must_use]
    fn pointer_kind(&self) -> Option<PointerKind>;
    /// The hardware identity, when the platform reports it.
    #[must_use]
    fn device_id(&self) -> Option<DeviceId>;
}

impl PointerEventExt for PointerEvent {
    fn time(&self) -> Option<flui_platform::EventTime> {
        event_time(self)
    }

    fn position(&self) -> Option<Offset<f64>> {
        let position = if let Some(sample) = pointer_sample(self) {
            Some(sample.position)
        } else {
            match self {
                PointerEvent::Enter(data)
                | PointerEvent::Leave(data)
                | PointerEvent::ScrollInertiaCancel(data) => data.position,
                PointerEvent::Scroll(data) => Some(data.position),
                PointerEvent::PanZoom(data) => Some(data.position),
                PointerEvent::SmartZoom(data) => Some(data.position),
                _ => None,
            }
        }?;
        let point = position.get();
        Some(Offset::new(point.x, point.y))
    }

    fn pointer_id(&self) -> Option<PointerId> {
        pointer_info(self).map(|pointer| pointer.id)
    }

    fn pointer_kind(&self) -> Option<PointerKind> {
        match self {
            PointerEvent::DeviceAdded(data) | PointerEvent::DeviceRemoved(data) => Some(data.kind),
            _ => pointer_info(self).map(|pointer| pointer.kind),
        }
    }

    fn device_id(&self) -> Option<DeviceId> {
        match self {
            PointerEvent::DeviceAdded(data) | PointerEvent::DeviceRemoved(data) => {
                Some(data.device)
            }
            _ => pointer_info(self).and_then(|pointer| pointer.device),
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
        flui_platform::EventTime::from_nanos(0),
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
        flui_platform::EventTime::from_nanos(0),
        CancelReason::Platform,
    ))
}

/// Construct a synthetic one-update pinch with checked position and cumulative scale.
#[cfg(any(test, feature = "testing"))]
pub fn make_pinch_gesture_event(
    position: Offset<f64>,
    fraction: f64,
) -> Result<PointerEvent, InputValueError> {
    use flui_platform::{
        EventTime,
        pointer::{PanZoomPhase, PanZoomTransform},
    };
    let pointer = PointerInfo::new(
        PointerId::try_from(u64::MAX).expect("BUG: nonzero synthetic gesture identity"),
        PointerKind::Trackpad,
    );
    let position = test_sample(position)?.position;
    let value = PanZoomTransform::try_new(1.0 + fraction, 0.0)?;
    Ok(PointerEvent::PanZoom(PanZoomEvent::new(
        pointer,
        EventTime::from_nanos(0),
        position,
        PanZoomPhase::Update(value),
    )))
}

/// Construct a checked synthetic pixel scroll.
#[cfg(any(test, feature = "testing"))]
pub fn make_scroll_event(
    position: Offset<f64>,
    delta: Offset<f64>,
) -> Result<PointerEvent, InputValueError> {
    make_scroll_event_with_modifiers(position, delta, Modifiers::NONE)
}

/// Construct a checked synthetic pixel scroll with explicit modifiers.
#[cfg(any(test, feature = "testing"))]
pub fn make_scroll_event_with_modifiers(
    position: Offset<f64>,
    delta: Offset<f64>,
    modifiers: Modifiers,
) -> Result<PointerEvent, InputValueError> {
    use flui_platform::{EventTime, pointer::ScrollUnit};
    let pointer = PointerInfo::new(test_pointer_id(), PointerKind::Mouse);
    let position = test_sample(position)?.position;
    let delta = ScrollDelta::try_new(ScrollUnit::Pixels, delta.dx, delta.dy)?;
    Ok(PointerEvent::Scroll(
        ScrollEvent::new(pointer, EventTime::from_nanos(0), position, delta)
            .with_modifiers(modifiers),
    ))
}
