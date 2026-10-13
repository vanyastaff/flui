//! AppKit input translated into the owned pointer vocabulary.
//!
//! Each `NSEvent` is copied into a plain [`NativeReading`] and assembled by
//! the shared [`pointer_assembly`](crate::shared::pointer_assembly): mouse
//! chords belong to the receiving view; trackpad magnify and rotate components
//! share one cumulative session. Two-finger scroll remains scroll, retaining
//! the native finger/momentum phase and delta precision.

use std::num::{NonZeroU8, NonZeroU64};
use std::time::Duration;

use flui_platform_api::EventTime;
use flui_platform_api::keyboard::{Key, KeyEvent, KeyRepeat, KeyState, Modifiers, NamedKey};
use flui_platform_api::pointer::{
    CancelReason, PanZoomEvent, PointerButton, PointerEvent, PointerId, PointerInfo, PointerKind,
    PointerPosition, PointerRole, ScrollDelta, ScrollEvent, ScrollPhase, ScrollPrecision,
    ScrollUnit,
};
use objc2_app_kit::{NSEvent, NSEventModifierFlags, NSEventPhase, NSEventType};
use objc2_foundation::NSProcessInfo;

use crate::shared::pointer_assembly::{
    ClockChoice, ContactRegistry, DecodeError, GestureComponent, GestureDelta, GesturePhase,
    GestureReading, NativeClock, NativeContact, NativeReading, NativeSurface, NativeTime,
    RawSensors, ReadingAction, ReadingTime, ScrollPhases, ScrollReading, decode,
};
use crate::traits::PlatformInput;

/// The view's cursor.
const MOUSE: NativeContact = NativeContact(0);
/// The view's trackpad gesture session.
const GESTURE: NativeContact = NativeContact(1);

/// Owner-local AppKit input state, committed before callbacks run.
pub(super) struct MacInputState {
    contacts: ContactRegistry,
    scrolling: ScrollPhases,
}

impl Default for MacInputState {
    fn default() -> Self {
        // The uptime and the epoch are read back to back: AppKit timestamps
        // and `Instant` both count the same monotonic uptime.
        let uptime = NSProcessInfo::processInfo().systemUptime();
        let clock = NativeClock::new(
            EventTime::from_nanos(crate::shared::events::event_timestamp_ns()),
            Duration::try_from_secs_f64(uptime).unwrap_or_default(),
            None,
        );
        Self {
            contacts: ContactRegistry::new(clock).with_reserved_contact(MOUSE),
            scrolling: ScrollPhases::default(),
        }
    }
}

impl MacInputState {
    /// # Safety
    /// The pointer must refer to the live NSEvent delivered to this view.
    pub(super) unsafe fn convert(
        &mut self,
        event: *mut std::ffi::c_void,
        view_height: f64,
    ) -> Vec<PlatformInput> {
        if event.is_null() {
            return Vec::new();
        }
        // SAFETY: the caller supplies the live event for the duration of this call.
        let event = unsafe { &*event.cast::<NSEvent>() };
        self.convert_typed(event, view_height)
    }

    fn convert_typed(&mut self, event: &NSEvent, view_height: f64) -> Vec<PlatformInput> {
        let reading_time = ReadingTime::coarse(NativeTime::UptimeSeconds(event.timestamp()));
        let time = self
            .contacts
            .clock()
            .stamp(reading_time, ClockChoice::Coarse)
            .unwrap_or_else(|| EventTime::from_nanos(crate::shared::events::event_timestamp_ns()));
        let event_type = event.r#type();
        if event_type == NSEventType::KeyDown || event_type == NSEventType::KeyUp {
            let key_code = event.keyCode();
            let code = crate::shared::keys_macos::keycode_to_code(key_code);
            return vec![PlatformInput::Keyboard(
                KeyEvent::new(
                    if event_type == NSEventType::KeyDown {
                        KeyState::Down
                    } else {
                        KeyState::Up
                    },
                    extract_key(event, key_code),
                    code,
                    time,
                )
                .with_location(crate::shared::keys::location_for_code(code))
                .with_modifiers(extract_modifiers(event))
                .with_repeat(
                    if event_type == NSEventType::KeyDown && event.isARepeat() {
                        KeyRepeat::AutoRepeat
                    } else {
                        KeyRepeat::First
                    },
                ),
            )];
        }
        let modifiers = extract_modifiers(event);
        let location = event.locationInWindow();
        let (x, y) = (location.x, view_height - location.y);
        let placed = |reading: NativeReading| {
            if x.is_finite() && y.is_finite() {
                reading.at(x, y, 1.0)
            } else {
                reading
            }
        };
        let mouse = |action| {
            placed(
                NativeReading::new(MOUSE, PointerKind::Mouse, action, reading_time)
                    .with_primary(true)
                    .with_modifiers(modifiers),
            )
        };
        let reading = if event_type == NSEventType::MouseEntered {
            mouse(ReadingAction::Enter)
        } else if event_type == NSEventType::MouseExited {
            mouse(ReadingAction::Leave)
        } else if event_type == NSEventType::MouseCancelled {
            mouse(ReadingAction::Cancel(CancelReason::Platform))
        } else if matches!(
            event_type,
            NSEventType::LeftMouseDown | NSEventType::RightMouseDown | NSEventType::OtherMouseDown
        ) {
            let Some(button) = mouse_button(event.buttonNumber()) else {
                return Vec::new();
            };
            let mut press = mouse(ReadingAction::Press(button));
            if let Ok(count) = u8::try_from(event.clickCount())
                && let Some(count) = NonZeroU8::new(count)
            {
                press = press.with_click_count(count);
            }
            press
        } else if matches!(
            event_type,
            NSEventType::LeftMouseUp | NSEventType::RightMouseUp | NSEventType::OtherMouseUp
        ) {
            let Some(button) = mouse_button(event.buttonNumber()) else {
                return Vec::new();
            };
            mouse(ReadingAction::Release(button))
        } else if matches!(
            event_type,
            NSEventType::MouseMoved
                | NSEventType::LeftMouseDragged
                | NSEventType::RightMouseDragged
                | NSEventType::OtherMouseDragged
                | NSEventType::Pressure
        ) {
            // AppKit pressure stages have separate normalized curves. Keep the
            // first-stage reading; summing stages would invent a pressure range.
            let pressure = (event_type == NSEventType::Pressure && event.stage() == 1)
                .then(|| f64::from(event.pressure()))
                .filter(|pressure| (0.0..=1.0).contains(pressure));
            mouse(ReadingAction::Move).with_sensors(RawSensors {
                pressure,
                ..RawSensors::default()
            })
        } else if event_type == NSEventType::ScrollWheel {
            return self.scroll(event, time, x, y, modifiers);
        } else if let Some((component, delta)) = gesture_component(event, event_type) {
            let phase = if event_type == NSEventType::BeginGesture {
                NSEventPhase::Began
            } else if event_type == NSEventType::EndGesture {
                NSEventPhase::Ended
            } else {
                event.phase()
            };
            return self
                .gesture(component, phase, delta, (x, y), reading_time, modifiers)
                .into_iter()
                .map(|event| PlatformInput::Pointer(PointerEvent::PanZoom(event)))
                .collect();
        } else {
            return Vec::new();
        };
        decode(&mut self.contacts, &reading)
            .unwrap_or_else(DecodeError::into_inputs)
            .into_vec()
    }

    fn scroll(
        &mut self,
        event: &NSEvent,
        time: EventTime,
        x: f64,
        y: f64,
        modifiers: Modifiers,
    ) -> Vec<PlatformInput> {
        let Ok(position) = PointerPosition::try_new(flui_foundation::geometry::Point::new(x, y))
        else {
            return Vec::new();
        };
        let pointer = self
            .contacts
            .pointer(NativeSurface::default(), None, MOUSE)
            .unwrap_or_else(mouse_info);
        let precise = event.hasPreciseScrollingDeltas();
        let unit = if precise {
            ScrollUnit::Pixels
        } else {
            ScrollUnit::Lines
        };
        let delta = ScrollDelta::try_new(unit, -event.scrollingDeltaX(), -event.scrollingDeltaY())
            .unwrap_or_else(|_| ScrollDelta::zero(unit));
        let mut scroll = ScrollEvent::new(pointer, time, position, delta)
            .with_modifiers(modifiers)
            .with_precision(if precise {
                ScrollPrecision::Precise
            } else {
                ScrollPrecision::Notched
            });
        if let Some(phase) = scroll_phase(
            &mut self.scrolling,
            event.phase(),
            event.momentumPhase(),
            precise,
        ) {
            scroll = scroll.with_phase(phase);
        }
        vec![PlatformInput::Pointer(PointerEvent::Scroll(scroll))]
    }

    /// One trackpad gesture component at view point `(x, y)`; a non-finite
    /// point keeps the gesture's last one.
    fn gesture(
        &mut self,
        component: GestureComponent,
        phase: NSEventPhase,
        delta: GestureDelta,
        (x, y): (f64, f64),
        time: ReadingTime,
        modifiers: Modifiers,
    ) -> Vec<PanZoomEvent> {
        let phase = if phase.contains(NSEventPhase::Cancelled) {
            GesturePhase::Cancelled
        } else if phase.contains(NSEventPhase::Began) {
            GesturePhase::Began
        } else if phase.contains(NSEventPhase::Ended) {
            GesturePhase::Ended
        } else {
            GesturePhase::Changed
        };
        let mut reading = NativeReading::new(
            GESTURE,
            PointerKind::Trackpad,
            ReadingAction::Gesture(GestureReading {
                component,
                phase,
                delta,
            }),
            time,
        )
        .with_primary(true)
        .with_modifiers(modifiers);
        if x.is_finite() && y.is_finite() {
            reading = reading.at(x, y, 1.0);
        }
        decode(&mut self.contacts, &reading)
            .unwrap_or_else(DecodeError::into_inputs)
            .into_iter()
            .filter_map(|input| match input {
                PlatformInput::Pointer(PointerEvent::PanZoom(event)) => Some(event),
                _ => None,
            })
            .collect()
    }
}

/// The gesture component an AppKit gesture event reports, with its change.
fn gesture_component(
    event: &NSEvent,
    event_type: NSEventType,
) -> Option<(GestureComponent, GestureDelta)> {
    if event_type == NSEventType::BeginGesture || event_type == NSEventType::EndGesture {
        Some((GestureComponent::Session, GestureDelta::None))
    } else if event_type == NSEventType::Magnify {
        Some((
            GestureComponent::Scale,
            GestureDelta::Scale(1.0 + event.magnification()),
        ))
    } else if event_type == NSEventType::Rotate {
        // AppKit reports counterclockwise degrees; owned rotation is clockwise radians.
        Some((
            GestureComponent::Rotation,
            GestureDelta::Rotation(-f64::from(event.rotation()).to_radians()),
        ))
    } else {
        None
    }
}

fn mouse_info() -> PointerInfo {
    PointerInfo::new(PointerId::new(NonZeroU64::MIN), PointerKind::Mouse)
        .with_role(PointerRole::Primary)
}

fn mouse_button(number: isize) -> Option<PointerButton> {
    // Native numbers are zero-based; FLUI reserves six for the pen eraser tool.
    let number = match number {
        0..=4 => number + 1,
        5..=30 => number + 2,
        _ => return None,
    };
    PointerButton::try_from(u8::try_from(number).ok()?).ok()
}

/// An AppKit phase as the shared grammar's plain phase; `None` for
/// `MayBegin` and no phase.
fn native_phase(phase: NSEventPhase) -> Option<GesturePhase> {
    if phase.contains(NSEventPhase::Cancelled) {
        Some(GesturePhase::Cancelled)
    } else if phase.contains(NSEventPhase::Ended) {
        Some(GesturePhase::Ended)
    } else if phase.contains(NSEventPhase::Began) {
        Some(GesturePhase::Began)
    } else if phase.contains(NSEventPhase::Changed) || phase.contains(NSEventPhase::Stationary) {
        Some(GesturePhase::Changed)
    } else {
        None
    }
}

/// The owned phase of an AppKit scroll through the shared grammar
/// (ADR-0217 §4): a two-finger pan is a phased scroll, momentum is
/// `Momentum*`.
fn scroll_phase(
    phases: &mut ScrollPhases,
    phase: NSEventPhase,
    momentum: NSEventPhase,
    precise: bool,
) -> Option<ScrollPhase> {
    let reading = ScrollReading {
        fingers: native_phase(phase),
        momentum: native_phase(momentum),
        precise,
    };
    phases.phase(NativeSurface::default(), None, reading)
}

fn extract_key(event: &NSEvent, key_code: u16) -> Key {
    // Check for special keys via key code first.
    if let Some(special_key) = crate::shared::keys_macos::keycode_to_key(key_code) {
        return special_key;
    }

    // `characters` is `None` only when the event has no character mapping;
    // objc2 models the NSString as retained, so there is no null to test.
    let Some(chars) = event.characters() else {
        return Key::Named(NamedKey::Unidentified);
    };

    let chars_str = chars.to_string();
    if chars_str.is_empty() {
        return Key::Named(NamedKey::Unidentified);
    }
    Key::character(chars_str)
}

/// Extract modifiers from NSEvent
///
/// # Safety
///
/// `ns_event` must be a valid, live `NSEvent*`.
fn extract_modifiers(event: &NSEvent) -> Modifiers {
    let flags = event.modifierFlags();

    let mut modifiers = Modifiers::NONE;
    if flags.contains(NSEventModifierFlags::Shift) {
        modifiers |= Modifiers::SHIFT;
    }
    if flags.contains(NSEventModifierFlags::Control) {
        modifiers |= Modifiers::CONTROL;
    }
    if flags.contains(NSEventModifierFlags::Option) {
        modifiers |= Modifiers::ALT;
    }
    if flags.contains(NSEventModifierFlags::Command) {
        modifiers |= Modifiers::META; // Command = Meta
    }
    modifiers
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_scroll_phases_preserve_momentum_and_unphased_wheels() {
        for (phase, momentum, expected) in [
            (NSEventPhase::None, NSEventPhase::None, None),
            (NSEventPhase::MayBegin, NSEventPhase::None, None),
            (
                NSEventPhase::Began,
                NSEventPhase::None,
                Some(ScrollPhase::Began),
            ),
            (
                NSEventPhase::Changed,
                NSEventPhase::None,
                Some(ScrollPhase::Changed),
            ),
            (
                NSEventPhase::Ended,
                NSEventPhase::None,
                Some(ScrollPhase::Ended),
            ),
            (
                NSEventPhase::Cancelled,
                NSEventPhase::None,
                Some(ScrollPhase::Cancelled),
            ),
            (
                NSEventPhase::None,
                NSEventPhase::Began,
                Some(ScrollPhase::MomentumBegan),
            ),
            (
                NSEventPhase::None,
                NSEventPhase::Changed,
                Some(ScrollPhase::MomentumChanged),
            ),
            (
                NSEventPhase::Ended,
                NSEventPhase::Ended,
                Some(ScrollPhase::MomentumEnded),
            ),
        ] {
            assert_eq!(
                scroll_phase(&mut ScrollPhases::default(), phase, momentum, true),
                expected
            );
        }
    }

    #[test]
    fn native_pinch_and_rotation_share_one_cumulative_gesture() {
        use GestureComponent::{Rotation, Scale, Session};
        use flui_platform_api::pointer::PanZoomPhase;
        let mut state = MacInputState::default();
        let at = (10.25, 20.5);
        let time = ReadingTime::coarse(NativeTime::UptimeSeconds(1.0));
        let modifiers = Modifiers::NONE;
        let mut gesture =
            |component, phase, delta| state.gesture(component, phase, delta, at, time, modifiers);
        assert_eq!(
            gesture(Scale, NSEventPhase::Changed, GestureDelta::Scale(1.1)),
            []
        );
        let first = gesture(Scale, NSEventPhase::Began, GestureDelta::Scale(1.1));
        assert_eq!(first.len(), 2);
        assert_eq!(first[0].phase, PanZoomPhase::Start);
        assert_eq!(
            first[0].position.get(),
            flui_foundation::geometry::Point::new(10.25, 20.5)
        );
        let id = first[0].pointer().id;
        let joined = gesture(
            Rotation,
            NSEventPhase::Began,
            GestureDelta::Rotation(-std::f64::consts::FRAC_PI_6),
        );
        assert_eq!(joined.len(), 1);
        let PanZoomPhase::Update(transform) = joined[0].phase else {
            panic!("update")
        };
        assert_eq!(joined[0].pointer().id, id);
        assert_eq!(transform.scale(), 1.1);
        assert_eq!(transform.rotation(), -std::f64::consts::FRAC_PI_6);
        let ended_component = gesture(Scale, NSEventPhase::Ended, GestureDelta::Scale(1.2));
        assert_eq!(ended_component.len(), 1);
        let last = gesture(
            Rotation,
            NSEventPhase::Ended,
            GestureDelta::Rotation(-std::f64::consts::FRAC_PI_6),
        );
        assert_eq!(last.len(), 2);
        let PanZoomPhase::Update(transform) = last[0].phase else {
            panic!("update")
        };
        assert!((transform.scale() - 1.32).abs() < 1e-12);
        assert_eq!(transform.rotation(), -std::f64::consts::FRAC_PI_3);
        assert_eq!(last[1].phase, PanZoomPhase::End);
        assert_eq!(last[1].pointer().id, id);
        let next = gesture(Scale, NSEventPhase::Began, GestureDelta::Scale(1.0));
        assert_ne!(next[0].pointer().id, id);
        let cancelled = gesture(
            Scale,
            NSEventPhase::Cancelled,
            GestureDelta::Scale(f64::NAN),
        );
        assert_eq!(cancelled.len(), 1);
        assert_eq!(cancelled[0].phase, PanZoomPhase::Cancelled);
        assert_eq!(
            gesture(Scale, NSEventPhase::Changed, GestureDelta::Scale(1.1)),
            []
        );
        let outer = gesture(Session, NSEventPhase::Began, GestureDelta::None);
        assert_eq!(outer.len(), 1);
        let outer_id = outer[0].pointer().id;
        gesture(Scale, NSEventPhase::Began, GestureDelta::Scale(1.1));
        assert_eq!(
            gesture(Scale, NSEventPhase::Ended, GestureDelta::Scale(1.0)),
            []
        );
        let rotation = gesture(Rotation, NSEventPhase::Began, GestureDelta::Rotation(0.1));
        assert_eq!(rotation.len(), 1);
        assert_eq!(rotation[0].pointer().id, outer_id);
        gesture(Rotation, NSEventPhase::Ended, GestureDelta::None);
        let end = gesture(Session, NSEventPhase::Ended, GestureDelta::None);
        assert_eq!(end.len(), 1);
        assert_eq!(end[0].phase, PanZoomPhase::End);
        assert_eq!(end[0].pointer().id, outer_id);
    }
}
