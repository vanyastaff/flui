//! UIKit touch and Pencil readings in the owned input vocabulary.
//!
//! Touch identity and primary admission belong to the receiving view. UIKit reports
//! force only for Pencil or a force-capable trait environment; unsupported force
//! remains absent. Native timestamps retain delivery age on the process timeline.

use std::collections::HashMap;
use std::num::NonZeroU64;
use std::time::Duration;

use flui_foundation::geometry::{Point, Size};
use flui_platform_api::EventTime;
use flui_platform_api::pointer::{
    CancelReason, ContactSize, PenOrientation, PenTool, PointerButton, PointerButtons,
    PointerCancel, PointerEvent, PointerId, PointerInfo, PointerKind, PointerMove, PointerPosition,
    PointerPress, PointerRelease, PointerRole, PointerSample, PointerSignal, Pressure,
};
use objc2::{msg_send, sel};
use objc2_foundation::NSObjectProtocol;
use objc2_foundation::NSProcessInfo;
use objc2_ui_kit::{
    UIForceTouchCapability, UIGestureRecognizerState, UIHoverGestureRecognizer, UITouch,
    UITouchType, UIView,
};

use crate::traits::PlatformInput;

/// Which native callback delivered a batch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TouchPhase {
    Down,
    Move,
    Up,
    Cancel,
}

/// Contacts admitted by one UIKit view; no address arithmetic mints identities.
pub(super) struct TouchInputState {
    contacts: HashMap<usize, PointerInfo>,
    next_id: Option<NonZeroU64>,
    hovering: bool,
}

impl Default for TouchInputState {
    fn default() -> Self {
        Self {
            contacts: HashMap::new(),
            // One is reserved for the view's Pencil hover recognizer.
            next_id: NonZeroU64::new(2),
            hovering: false,
        }
    }
}

impl TouchInputState {
    /// The recognizer is restricted to Stylus at registration; a mouse cannot
    /// acquire Pencil sensors through this path.
    pub(super) fn hover(
        &mut self,
        recognizer: &UIHoverGestureRecognizer,
        view: &UIView,
    ) -> Vec<PlatformInput> {
        // SAFETY: UIKit invokes this target with a live gesture recognizer;
        // `state` is its documented enum-valued getter (omitted by the bindings).
        let state: UIGestureRecognizerState = unsafe { msg_send![recognizer, state] };
        let time = EventTime::from_nanos(crate::shared::events::event_timestamp_ns());
        let info = PointerInfo::new(
            PointerId::new(NonZeroU64::MIN),
            PointerKind::Pen { tool: PenTool::Tip },
        )
        .with_role(PointerRole::Primary);
        if state == UIGestureRecognizerState::Ended
            || state == UIGestureRecognizerState::Cancelled
            || state == UIGestureRecognizerState::Failed
        {
            if !std::mem::take(&mut self.hovering) {
                return Vec::new();
            }
            return vec![PlatformInput::Pointer(PointerEvent::Leave(
                PointerSignal::new(info, time),
            ))];
        }
        if state != UIGestureRecognizerState::Began && state != UIGestureRecognizerState::Changed {
            return Vec::new();
        }
        let location = recognizer.locationInView(Some(view));
        let Ok(position) = PointerPosition::try_new(Point::new(location.x, location.y)) else {
            return Vec::new();
        };
        let mut sample = PointerSample::new(time, position);
        // Pencil hover angles were added after UIHoverGestureRecognizer itself.
        // SAFETY: selectors are getter queries, with no lifetime-bearing argument.
        if unsafe { recognizer.respondsToSelector(sel!(altitudeAngle)) }
            && unsafe { recognizer.respondsToSelector(sel!(azimuthAngleInView:)) }
            && let Ok(orientation) = PenOrientation::try_new(
                recognizer.altitudeAngle(),
                recognizer.azimuthAngleInView(Some(view)),
            )
        {
            sample = sample.with_orientation(orientation);
        }
        let mut events = Vec::new();
        if !self.hovering {
            self.hovering = true;
            events.push(PlatformInput::Pointer(PointerEvent::Enter(
                PointerSignal::new(info, time).with_position(position),
            )));
        }
        events.push(PlatformInput::Pointer(PointerEvent::Move(
            PointerMove::new(info, PointerButtons::NONE, sample),
        )));
        events
    }

    pub(super) fn convert(&mut self, touch: &UITouch, phase: TouchPhase) -> Vec<PlatformInput> {
        let address = std::ptr::from_ref(touch) as usize;
        let kind = pointer_kind(touch);
        let time = native_time(
            touch.timestamp(),
            NSProcessInfo::processInfo().systemUptime(),
        );
        let info = if phase == TouchPhase::Down {
            if self.contacts.contains_key(&address) {
                return Vec::new();
            }
            let Some(id) = self.next_id else {
                return Vec::new();
            };
            self.next_id = id.get().checked_add(1).and_then(NonZeroU64::new);
            let role = if self.contacts.values().any(|contact| contact.kind == kind) {
                PointerRole::Additional
            } else {
                PointerRole::Primary
            };
            let info = PointerInfo::new(PointerId::new(id), kind).with_role(role);
            // Validate position before admitting an undeliverable contact.
            if pointer_position(touch).is_none() {
                return Vec::new();
            }
            self.contacts.insert(address, info);
            info
        } else if matches!(phase, TouchPhase::Up | TouchPhase::Cancel) {
            let Some(info) = self.contacts.remove(&address) else {
                return Vec::new();
            };
            info
        } else {
            let Some(info) = self.contacts.get(&address).copied() else {
                return Vec::new();
            };
            info
        };

        if phase == TouchPhase::Cancel {
            return vec![PlatformInput::Pointer(PointerEvent::Cancel(
                PointerCancel::new(info, time, CancelReason::Platform),
            ))];
        }
        let Some(position) = pointer_position(touch) else {
            return if phase == TouchPhase::Up {
                vec![PlatformInput::Pointer(PointerEvent::Cancel(
                    PointerCancel::new(info, time, CancelReason::InvalidInput),
                ))]
            } else {
                Vec::new()
            };
        };
        let is_pen = matches!(kind, PointerKind::Pen { .. });
        let has_force = is_pen
            || touch.view().is_some_and(|view| {
                view.traitCollection().forceTouchCapability() == UIForceTouchCapability::Available
            });
        let orientation = is_pen.then(|| (touch.altitudeAngle(), touch.azimuthAngleInView(None)));
        let sample = touch_sample(
            position,
            time,
            has_force,
            touch.force(),
            touch.maximumPossibleForce(),
            touch.majorRadius(),
            orientation,
        );
        let event = match phase {
            TouchPhase::Down => PointerEvent::Down(PointerPress::new(
                info,
                PointerButton::PRIMARY,
                PointerButtons::NONE,
                sample,
            )),
            TouchPhase::Move => PointerEvent::Move(PointerMove::new(
                info,
                PointerButtons::only(PointerButton::PRIMARY),
                sample,
            )),
            TouchPhase::Up => PointerEvent::Up(PointerRelease::new(
                info,
                PointerButton::PRIMARY,
                PointerButtons::NONE,
                sample,
            )),
            TouchPhase::Cancel => unreachable!("cancel handled before sensor reading"),
        };
        vec![PlatformInput::Pointer(event)]
    }
}

fn pointer_kind(touch: &UITouch) -> PointerKind {
    let kind = touch.r#type();
    if kind == UITouchType::Stylus {
        PointerKind::Pen { tool: PenTool::Tip }
    } else if kind == UITouchType::IndirectPointer {
        PointerKind::Mouse
    } else {
        PointerKind::Touch
    }
}

fn pointer_position(touch: &UITouch) -> Option<PointerPosition> {
    let location = touch.locationInView(None);
    PointerPosition::try_new(Point::new(location.x, location.y)).ok()
}

fn touch_sample(
    position: PointerPosition,
    time: EventTime,
    has_force: bool,
    force: f64,
    maximum_force: f64,
    radius: f64,
    orientation: Option<(f64, f64)>,
) -> PointerSample {
    let mut sample = PointerSample::new(time, position);
    if has_force
        && maximum_force.is_finite()
        && maximum_force > 0.0
        && let Ok(pressure) = Pressure::try_new(force / maximum_force)
    {
        sample = sample.with_pressure(pressure);
    }
    if radius > 0.0
        && let Ok(size) = ContactSize::try_new(Size::new(radius * 2.0, radius * 2.0))
    {
        sample = sample.with_contact_size(size);
    }
    if let Some((altitude, azimuth)) = orientation
        && let Ok(orientation) = PenOrientation::try_new(altitude, azimuth)
    {
        sample = sample.with_orientation(orientation);
    }
    sample
}

/// UIKit and AppKit use uptime seconds. Preserve the native sample's delivery age.
fn native_time(timestamp: f64, uptime: f64) -> EventTime {
    let now = crate::shared::events::event_timestamp_ns();
    let age = Duration::try_from_secs_f64((uptime - timestamp).max(0.0))
        .ok()
        .and_then(|duration| u64::try_from(duration.as_nanos()).ok());
    EventTime::from_nanos(age.map_or(now, |age| now.saturating_sub(age)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_touch_readings_keep_sensor_presence_and_pen_angles() {
        let position = PointerPosition::try_new(flui_foundation::geometry::Point::new(1.25, 2.5))
            .expect("finite position");
        let time = flui_platform_api::EventTime::from_nanos(10);
        for (has_force, force, maximum, expected) in [
            (false, 1.0, 1.0, None),
            (true, 0.0, 4.0, Some(0.0)),
            (true, 2.0, 4.0, Some(0.5)),
            (true, 2.0, 0.0, None),
            (true, f64::INFINITY, 4.0, None),
        ] {
            let sample = touch_sample(position, time, has_force, force, maximum, 3.25, None);
            assert_eq!(sample.pressure.map(Pressure::get), expected);
            let extent = sample.contact_size.expect("reported radius");
            assert_eq!((extent.get().width, extent.get().height), (6.5, 6.5));
            assert_eq!(sample.orientation, None);
            assert_eq!(sample.tangential_pressure, None);
            assert_eq!(sample.twist, None);
        }
        let pen = touch_sample(
            position,
            time,
            true,
            1.0,
            4.0,
            0.0,
            Some((std::f64::consts::FRAC_PI_4, std::f64::consts::FRAC_PI_2)),
        );
        let orientation = pen.orientation.expect("reported Pencil angles");
        assert_eq!(orientation.altitude(), Some(std::f64::consts::FRAC_PI_4));
        assert_eq!(orientation.azimuth(), Some(std::f64::consts::FRAC_PI_2));
        assert_eq!(pen.contact_size, None);
    }
}
