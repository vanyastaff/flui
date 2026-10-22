//! UIKit touch and Pencil readings in the owned input vocabulary.
//!
//! Each `UITouch` is copied into a plain [`NativeReading`] and assembled by the
//! shared [`pointer_assembly`](crate::shared::pointer_assembly); touch identity
//! and primary admission belong to the receiving view. UIKit reports force
//! only for Pencil or a force-capable trait environment; unsupported force
//! remains absent. Native timestamps keep their uptime on the process
//! timeline.

use std::time::Duration;

use flui_platform::EventTime;
use flui_platform::pointer::{PenOrientation, PenTool, PointerButton, PointerKind};
use objc2::{msg_send, sel};
use objc2_foundation::NSObjectProtocol;
use objc2_foundation::NSProcessInfo;
use objc2_ui_kit::{
    UIForceTouchCapability, UIGestureRecognizerState, UIHoverGestureRecognizer, UITouch,
    UITouchType, UITraitEnvironment, UIView,
};

use crate::shared::pointer_assembly::{
    ContactRegistry, DecodeError, NativeCancel, NativeClock, NativeContact, NativeReading,
    NativeTime, PenAngles, RawSensors, ReadingAction, ReadingTime, decode,
};
use flui_platform::PlatformInput;

/// Which native callback delivered a batch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TouchPhase {
    Down,
    Move,
    Up,
    Cancel,
}

/// The primary Pencil: its hover and its contact are one contact, which keeps
/// the reserved identity one.
const PENCIL: NativeContact = NativeContact(0);
/// The indirect pointer (a trackpad or mouse cursor): one contact for every
/// `UITouch` UIKit makes for it.
const CURSOR: NativeContact = NativeContact(1);

/// Contacts admitted by one UIKit view; no address arithmetic mints identities.
pub(crate) struct TouchInputState {
    contacts: ContactRegistry,
    /// The touch the primary Pencil's contact is, while its tip is down.
    pencil_touch: Option<usize>,
}

impl Default for TouchInputState {
    fn default() -> Self {
        // The uptime and the epoch are read back to back: UIKit timestamps and
        // `Instant` both count the same monotonic uptime.
        let uptime = NSProcessInfo::processInfo().systemUptime();
        let clock = NativeClock::new(
            EventTime::from_nanos(crate::shared::events::event_timestamp_ns()),
            Duration::try_from_secs_f64(uptime).unwrap_or_default(),
            None,
        );
        Self {
            contacts: ContactRegistry::new(clock).with_reserved_contact(PENCIL),
            pencil_touch: None,
        }
    }
}

impl TouchInputState {
    /// The recognizer is restricted to Stylus at registration; a mouse cannot
    /// acquire Pencil sensors through this path.
    pub(crate) fn hover(
        &mut self,
        recognizer: &UIHoverGestureRecognizer,
        view: &UIView,
    ) -> Vec<PlatformInput> {
        if self.pencil_touch.is_some() {
            // Contact samples own this Pencil while its tip is down.
            return Vec::new();
        }
        // SAFETY: UIKit invokes this target with a live gesture recognizer;
        // `state` is its documented enum-valued getter (omitted by the bindings).
        let state: UIGestureRecognizerState = unsafe { msg_send![recognizer, state] };
        let time = ReadingTime::epoch(EventTime::from_nanos(
            crate::shared::events::event_timestamp_ns(),
        ));
        let pencil = |action| {
            NativeReading::new(
                PENCIL,
                PointerKind::Pen { tool: PenTool::Tip },
                action,
                time,
            )
            .with_primary(true)
        };
        if state == UIGestureRecognizerState::Ended
            || state == UIGestureRecognizerState::Cancelled
            || state == UIGestureRecognizerState::Failed
        {
            return self.assemble(&pencil(ReadingAction::Leave));
        }
        if state != UIGestureRecognizerState::Began && state != UIGestureRecognizerState::Changed {
            return Vec::new();
        }
        let location = recognizer.locationInView(Some(view));
        let mut sensors = RawSensors::default();
        // Pencil hover angles were added after UIHoverGestureRecognizer itself.
        if recognizer.respondsToSelector(sel!(altitudeAngle))
            && recognizer.respondsToSelector(sel!(azimuthAngleInView:))
        {
            sensors.angles = pen_angles(
                recognizer.altitudeAngle(),
                recognizer.azimuthAngleInView(Some(view)),
            );
        }
        let moved = pencil(ReadingAction::Move)
            .at(location.x, location.y, 1.0)
            .with_sensors(sensors);
        let mut events = Vec::new();
        if state == UIGestureRecognizerState::Began {
            events.extend(self.assemble(&NativeReading {
                action: ReadingAction::Enter,
                ..moved
            }));
        }
        events.extend(self.assemble(&moved));
        events
    }

    pub(crate) fn convert(&mut self, touch: &UITouch, phase: TouchPhase) -> Vec<PlatformInput> {
        let address = std::ptr::from_ref(touch) as usize;
        let kind = pointer_kind(touch);
        let contact = if kind == PointerKind::Mouse {
            CURSOR
        } else if self.pencil_touch == Some(address)
            || (phase == TouchPhase::Down
                && matches!(kind, PointerKind::Pen { .. })
                && self.pencil_touch.is_none())
        {
            self.pencil_touch = Some(address);
            PENCIL
        } else {
            NativeContact(address as u64)
        };
        if contact == PENCIL && matches!(phase, TouchPhase::Up | TouchPhase::Cancel) {
            self.pencil_touch = None;
        }
        let action = match phase {
            TouchPhase::Down => ReadingAction::Press(PointerButton::PRIMARY),
            TouchPhase::Move => ReadingAction::Move,
            TouchPhase::Up => ReadingAction::Release(PointerButton::PRIMARY),
            TouchPhase::Cancel => ReadingAction::Cancel(NativeCancel::Platform),
        };
        let view = touch.view();
        let location = touch.locationInView(view.as_deref());
        let is_pen = matches!(kind, PointerKind::Pen { .. });
        let has_force = is_pen
            || view.as_ref().is_some_and(|view| {
                view.traitCollection().forceTouchCapability() == UIForceTouchCapability::Available
            });
        let mut sensors = touch_sensors(
            has_force,
            touch.force(),
            touch.maximumPossibleForce(),
            touch.majorRadius(),
        );
        if is_pen {
            sensors.angles = pen_angles(
                touch.altitudeAngle(),
                touch.azimuthAngleInView(view.as_deref()),
            );
        }
        let reading = NativeReading::new(
            contact,
            kind,
            action,
            ReadingTime::coarse(NativeTime::UptimeSeconds(touch.timestamp())),
        )
        .at(location.x, location.y, 1.0)
        .with_sensors(sensors);
        let reading = if contact == PENCIL || contact == CURSOR {
            reading.with_primary(true)
        } else {
            reading
        };
        self.assemble(&reading)
    }

    fn assemble(&mut self, reading: &NativeReading) -> Vec<PlatformInput> {
        decode(&mut self.contacts, reading)
            .unwrap_or_else(DecodeError::into_inputs)
            .into_vec()
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

/// Pencil angles, kept only when UIKit reports a valid pair.
fn pen_angles(altitude: f64, azimuth: f64) -> Option<PenAngles> {
    PenOrientation::try_new(altitude, azimuth)
        .is_ok()
        .then_some(PenAngles::Spherical {
            altitude: Some(altitude),
            azimuth: Some(azimuth),
        })
}

/// A touch's force, normalized by its maximum when the environment reports
/// force, and its contact diameter in points when UIKit measures a radius.
fn touch_sensors(has_force: bool, force: f64, maximum_force: f64, radius: f64) -> RawSensors {
    let normalized_force = force / maximum_force;
    RawSensors {
        pressure: (has_force
            && maximum_force.is_finite()
            && maximum_force > 0.0
            && (0.0..=1.0).contains(&normalized_force))
        .then_some(normalized_force),
        contact_size: (radius > 0.0 && radius.is_finite()).then_some((radius * 2.0, radius * 2.0)),
        ..RawSensors::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_platform::pointer::{PointerEvent, Pressure};

    /// The Down a Pencil-less touch with `sensors` assembles into.
    fn down(sensors: RawSensors) -> flui_platform::pointer::PointerSample {
        let mut contacts = ContactRegistry::new(NativeClock::epoch_only());
        let reading = NativeReading::new(
            NativeContact(9),
            PointerKind::Touch,
            ReadingAction::Press(PointerButton::PRIMARY),
            ReadingTime::epoch(EventTime::from_nanos(10)),
        )
        .at(1.25, 2.5, 1.0)
        .with_sensors(sensors);
        decode(&mut contacts, &reading)
            .expect("valid touch")
            .iter()
            .find_map(|input| match input.as_pointer() {
                Some(PointerEvent::Down(press)) => Some(press.sample),
                _ => None,
            })
            .expect("down")
    }

    #[test]
    fn native_touch_readings_keep_sensor_presence_and_pen_angles() {
        for (has_force, force, maximum, expected) in [
            (false, 1.0, 1.0, None),
            (true, 0.0, 4.0, Some(0.0)),
            (true, 2.0, 4.0, Some(0.5)),
            (true, 2.0, 0.0, None),
            (true, f64::INFINITY, 4.0, None),
        ] {
            let sample = down(touch_sensors(has_force, force, maximum, 3.25));
            assert_eq!(sample.pressure.map(Pressure::get), expected);
            let extent = sample.contact_size.expect("reported radius");
            assert_eq!((extent.get().width, extent.get().height), (6.5, 6.5));
            assert_eq!(sample.orientation, None);
            assert_eq!(sample.tangential_pressure, None);
            assert_eq!(sample.twist, None);
        }
        let pen = down(RawSensors {
            angles: pen_angles(std::f64::consts::FRAC_PI_4, std::f64::consts::FRAC_PI_2),
            ..touch_sensors(true, 1.0, 4.0, 0.0)
        });
        let orientation = pen.orientation.expect("reported Pencil angles");
        assert_eq!(orientation.altitude(), Some(std::f64::consts::FRAC_PI_4));
        assert_eq!(orientation.azimuth(), Some(std::f64::consts::FRAC_PI_2));
        assert_eq!(pen.contact_size, None);
        assert_eq!(pen_angles(f64::NAN, 0.0), None);
    }
}
