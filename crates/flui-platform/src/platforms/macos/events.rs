//! AppKit input translated directly into the owned pointer vocabulary.
//!
//! Mouse chords belong to the receiving view; trackpad magnify and rotate
//! components share one cumulative session. Two-finger scroll remains scroll,
//! retaining the native finger/momentum phase and delta precision.

use std::num::{NonZeroU8, NonZeroU64};
use std::time::Duration;

use flui_foundation::geometry::{Offset, Point};
use flui_platform_api::EventTime;
use flui_platform_api::keyboard::Modifiers as OwnedModifiers;
use flui_platform_api::keyboard::{Key, KeyEvent, KeyRepeat, KeyState, Modifiers, NamedKey};
use flui_platform_api::pointer::{
    ButtonChange, CancelReason, PanZoomEvent, PanZoomPhase, PanZoomTransform, PointerButton,
    PointerButtons, PointerCancel, PointerEvent, PointerId, PointerInfo, PointerKind, PointerMove,
    PointerPosition, PointerPress, PointerRelease, PointerRole, PointerSample, PointerSignal,
    Pressure, ScrollDelta, ScrollEvent, ScrollPhase, ScrollPrecision, ScrollUnit,
};
use objc2_app_kit::{NSEvent, NSEventModifierFlags, NSEventPhase, NSEventType};
use objc2_foundation::NSProcessInfo;

use crate::traits::PlatformInput;

struct Gesture {
    pointer: PointerInfo,
    transform: PanZoomTransform,
    components: u8,
    position: PointerPosition,
}

/// Owner-local AppKit input state, committed before callbacks run.
pub(super) struct MacInputState {
    buttons: PointerButtons,
    gesture: Option<Gesture>,
    next_gesture: Option<NonZeroU64>,
}

impl Default for MacInputState {
    fn default() -> Self {
        Self {
            buttons: PointerButtons::NONE,
            gesture: None,
            next_gesture: NonZeroU64::new(2),
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
        let time = native_time(
            event.timestamp(),
            NSProcessInfo::processInfo().systemUptime(),
        );
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
        let pointer = mouse_info();
        let modifiers = extract_modifiers(event);
        let location = event.locationInWindow();
        let position =
            PointerPosition::try_new(Point::new(location.x, view_height - location.y)).ok();
        if event_type == NSEventType::MouseEntered || event_type == NSEventType::MouseExited {
            let mut signal = PointerSignal::new(pointer, time);
            if let Some(position) = position {
                signal = signal.with_position(position);
            }
            return vec![PlatformInput::Pointer(
                if event_type == NSEventType::MouseEntered {
                    PointerEvent::Enter(signal)
                } else {
                    PointerEvent::Leave(signal)
                },
            )];
        }
        if event_type == NSEventType::MouseCancelled {
            if self.buttons.is_empty() {
                return Vec::new();
            }
            self.buttons = PointerButtons::NONE;
            return vec![PlatformInput::Pointer(PointerEvent::Cancel(
                PointerCancel::new(pointer, time, CancelReason::Platform),
            ))];
        }
        let is_press = matches!(
            event_type,
            NSEventType::LeftMouseDown | NSEventType::RightMouseDown | NSEventType::OtherMouseDown
        );
        let is_release = matches!(
            event_type,
            NSEventType::LeftMouseUp | NSEventType::RightMouseUp | NSEventType::OtherMouseUp
        );
        if is_press || is_release {
            let Some(button) = mouse_button(event.buttonNumber()) else {
                return Vec::new();
            };
            let before = self.buttons;
            if is_press && before.contains(button) || is_release && !before.contains(button) {
                return Vec::new();
            }
            let Some(position) = position else {
                if is_release {
                    self.buttons = PointerButtons::NONE;
                    return vec![PlatformInput::Pointer(PointerEvent::Cancel(
                        PointerCancel::new(pointer, time, CancelReason::InvalidInput),
                    ))];
                }
                return Vec::new();
            };
            self.buttons = if is_press {
                before.with(button)
            } else {
                before.without(button)
            };
            let sample = PointerSample::new(time, position);
            let event = if is_press {
                let mut press =
                    PointerPress::new(pointer, button, before, sample).with_modifiers(modifiers);
                if let Ok(count) = u8::try_from(event.clickCount())
                    && let Some(count) = NonZeroU8::new(count)
                {
                    press = press.with_click_count(count);
                }
                if before.is_empty() {
                    PointerEvent::Down(press)
                } else {
                    PointerEvent::ButtonChange(ButtonChange::Pressed(press))
                }
            } else {
                let release =
                    PointerRelease::new(pointer, button, before, sample).with_modifiers(modifiers);
                if self.buttons.is_empty() {
                    PointerEvent::Up(release)
                } else {
                    PointerEvent::ButtonChange(ButtonChange::Released(release))
                }
            };
            return vec![PlatformInput::Pointer(event)];
        }
        let position = position.or_else(|| {
            if matches!(
                event_type,
                NSEventType::Magnify | NSEventType::Rotate | NSEventType::EndGesture
            ) && (event_type == NSEventType::EndGesture
                || event.phase().contains(NSEventPhase::Ended)
                || event.phase().contains(NSEventPhase::Cancelled))
            {
                self.gesture.as_ref().map(|gesture| gesture.position)
            } else {
                None
            }
        });
        let Some(position) = position else {
            return Vec::new();
        };
        if matches!(
            event_type,
            NSEventType::MouseMoved
                | NSEventType::LeftMouseDragged
                | NSEventType::RightMouseDragged
                | NSEventType::OtherMouseDragged
                | NSEventType::Pressure
        ) {
            let mut sample = PointerSample::new(time, position);
            // AppKit pressure stages have separate normalized curves. Keep the
            // first-stage reading; summing stages would invent a pressure range.
            if event_type == NSEventType::Pressure
                && event.stage() == 1
                && let Ok(pressure) = Pressure::try_new(event.pressure())
            {
                sample = sample.with_pressure(pressure);
            }
            return vec![PlatformInput::Pointer(PointerEvent::Move(
                PointerMove::new(pointer, self.buttons, sample).with_modifiers(modifiers),
            ))];
        }
        if event_type == NSEventType::ScrollWheel {
            let precise = event.hasPreciseScrollingDeltas();
            let unit = if precise {
                ScrollUnit::Pixels
            } else {
                ScrollUnit::Lines
            };
            let delta =
                ScrollDelta::try_new(unit, -event.scrollingDeltaX(), -event.scrollingDeltaY())
                    .unwrap_or_else(|_| ScrollDelta::zero(unit));
            let mut scroll = ScrollEvent::new(pointer, time, position, delta)
                .with_modifiers(modifiers)
                .with_precision(if precise {
                    ScrollPrecision::Precise
                } else {
                    ScrollPrecision::Notched
                });
            if let Some(phase) = scroll_phase(event.phase(), event.momentumPhase()) {
                scroll = scroll.with_phase(phase);
            }
            return vec![PlatformInput::Pointer(PointerEvent::Scroll(scroll))];
        }
        if event_type == NSEventType::BeginGesture || event_type == NSEventType::EndGesture {
            let phase = if event_type == NSEventType::BeginGesture {
                NSEventPhase::Began
            } else {
                // A native outer terminal also retires a component whose own
                // Ended notification never arrived.
                if let Some(gesture) = self.gesture.as_mut() {
                    gesture.components = 4;
                }
                NSEventPhase::Ended
            };
            return self
                .gesture(4, phase, 1.0, 0.0, position, time, modifiers)
                .into_iter()
                .map(|event| PlatformInput::Pointer(PointerEvent::PanZoom(event)))
                .collect();
        }
        let (component, scale, rotation) = if event_type == NSEventType::Magnify {
            (1, 1.0 + event.magnification(), 0.0)
        } else if event_type == NSEventType::Rotate {
            (2, 1.0, -f64::from(event.rotation()).to_radians())
        } else {
            return Vec::new();
        };
        self.gesture(
            component,
            event.phase(),
            scale,
            rotation,
            position,
            time,
            modifiers,
        )
        .into_iter()
        .map(|event| PlatformInput::Pointer(PointerEvent::PanZoom(event)))
        .collect()
    }

    fn gesture(
        &mut self,
        component: u8,
        phase: NSEventPhase,
        scale: f64,
        rotation: f64,
        position: PointerPosition,
        time: EventTime,
        modifiers: OwnedModifiers,
    ) -> Vec<PanZoomEvent> {
        let mut events = Vec::new();
        if phase.contains(NSEventPhase::Began) {
            if self.gesture.is_none() {
                let Some(id) = self.next_gesture else {
                    return events;
                };
                self.next_gesture = id.get().checked_add(1).and_then(NonZeroU64::new);
                let pointer = PointerInfo::new(PointerId::new(id), PointerKind::Trackpad)
                    .with_role(PointerRole::Primary);
                self.gesture = Some(Gesture {
                    pointer,
                    transform: PanZoomTransform::IDENTITY,
                    components: 0,
                    position,
                });
                events.push(
                    PanZoomEvent::new(pointer, time, position, PanZoomPhase::Start)
                        .with_modifiers(modifiers),
                );
            }
            if let Some(gesture) = self.gesture.as_mut() {
                gesture.components |= component;
            }
        }
        let Some(gesture) = self.gesture.as_mut() else {
            return events;
        };
        gesture.position = position;
        if phase.contains(NSEventPhase::Cancelled) {
            let pointer = gesture.pointer;
            self.gesture = None;
            events.push(
                PanZoomEvent::new(pointer, time, position, PanZoomPhase::Cancelled)
                    .with_modifiers(modifiers),
            );
            return events;
        }
        if gesture.components & component == 0 {
            return events;
        }
        if scale != 1.0 || rotation != 0.0 {
            if let Ok(transform) = PanZoomTransform::try_new(
                Offset::ZERO,
                gesture.transform.scale() * scale,
                gesture.transform.rotation() + rotation,
            ) {
                gesture.transform = transform;
                events.push(
                    PanZoomEvent::new(
                        gesture.pointer,
                        time,
                        position,
                        PanZoomPhase::Update(transform),
                    )
                    .with_modifiers(modifiers),
                );
            }
        }
        if phase.contains(NSEventPhase::Ended) {
            gesture.components &= !component;
            if gesture.components == 0 {
                let pointer = gesture.pointer;
                self.gesture = None;
                events.push(
                    PanZoomEvent::new(pointer, time, position, PanZoomPhase::End)
                        .with_modifiers(modifiers),
                );
            }
        }
        events
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

fn scroll_phase(phase: NSEventPhase, momentum: NSEventPhase) -> Option<ScrollPhase> {
    if momentum.contains(NSEventPhase::Ended) || momentum.contains(NSEventPhase::Cancelled) {
        Some(ScrollPhase::MomentumEnded)
    } else if momentum.contains(NSEventPhase::Began) {
        Some(ScrollPhase::MomentumBegan)
    } else if momentum.contains(NSEventPhase::Changed)
        || momentum.contains(NSEventPhase::Stationary)
    {
        Some(ScrollPhase::MomentumChanged)
    } else if phase.contains(NSEventPhase::Cancelled) {
        Some(ScrollPhase::Cancelled)
    } else if phase.contains(NSEventPhase::Ended) {
        Some(ScrollPhase::Ended)
    } else if phase.contains(NSEventPhase::Began) {
        Some(ScrollPhase::Began)
    } else if phase.contains(NSEventPhase::Changed) || phase.contains(NSEventPhase::Stationary) {
        Some(ScrollPhase::Changed)
    } else {
        None
    }
}

fn native_time(timestamp: f64, uptime: f64) -> EventTime {
    let now = crate::shared::events::event_timestamp_ns();
    let age = Duration::try_from_secs_f64((uptime - timestamp).max(0.0))
        .ok()
        .and_then(|duration| u64::try_from(duration.as_nanos()).ok());
    EventTime::from_nanos(age.map_or(now, |age| now.saturating_sub(age)))
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
        modifiers.insert(Modifiers::SHIFT);
    }
    if flags.contains(NSEventModifierFlags::Control) {
        modifiers.insert(Modifiers::CONTROL);
    }
    if flags.contains(NSEventModifierFlags::Option) {
        modifiers.insert(Modifiers::ALT);
    }
    if flags.contains(NSEventModifierFlags::Command) {
        modifiers.insert(Modifiers::META); // Command = Meta
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
            assert_eq!(scroll_phase(phase, momentum), expected);
        }
    }

    #[test]
    fn native_pinch_and_rotation_share_one_cumulative_gesture() {
        let mut state = MacInputState::default();
        let at = PointerPosition::try_new(flui_foundation::geometry::Point::new(10.25, 20.5))
            .expect("finite");
        let time = EventTime::from_nanos(10);
        let modifiers = flui_platform_api::keyboard::Modifiers::NONE;
        assert!(
            state
                .gesture(1, NSEventPhase::Changed, 1.1, 0.0, at, time, modifiers)
                .is_empty()
        );
        let first = state.gesture(1, NSEventPhase::Began, 1.1, 0.0, at, time, modifiers);
        assert_eq!(first.len(), 2);
        assert_eq!(first[0].phase, PanZoomPhase::Start);
        let id = first[0].pointer().id;
        let joined = state.gesture(
            2,
            NSEventPhase::Began,
            1.0,
            -std::f64::consts::FRAC_PI_6,
            at,
            time,
            modifiers,
        );
        assert_eq!(joined.len(), 1);
        let PanZoomPhase::Update(transform) = joined[0].phase else {
            panic!("update")
        };
        assert_eq!(joined[0].pointer().id, id);
        assert_eq!(transform.scale(), 1.1);
        assert_eq!(transform.rotation(), -std::f64::consts::FRAC_PI_6);
        let ended_component = state.gesture(1, NSEventPhase::Ended, 1.2, 0.0, at, time, modifiers);
        assert_eq!(ended_component.len(), 1);
        let last = state.gesture(
            2,
            NSEventPhase::Ended,
            1.0,
            -std::f64::consts::FRAC_PI_6,
            at,
            time,
            modifiers,
        );
        assert_eq!(last.len(), 2);
        let PanZoomPhase::Update(transform) = last[0].phase else {
            panic!("update")
        };
        assert!((transform.scale() - 1.32).abs() < 1e-12);
        assert_eq!(transform.rotation(), -std::f64::consts::FRAC_PI_3);
        assert_eq!(last[1].phase, PanZoomPhase::End);
        assert_eq!(last[1].pointer().id, id);
        let next = state.gesture(1, NSEventPhase::Began, 1.0, 0.0, at, time, modifiers);
        assert_ne!(next[0].pointer().id, id);
        let cancelled = state.gesture(
            1,
            NSEventPhase::Cancelled,
            f64::NAN,
            0.0,
            at,
            time,
            modifiers,
        );
        assert_eq!(cancelled.len(), 1);
        assert_eq!(cancelled[0].phase, PanZoomPhase::Cancelled);
        assert!(
            state
                .gesture(1, NSEventPhase::Changed, 1.1, 0.0, at, time, modifiers)
                .is_empty()
        );
        let outer = state.gesture(4, NSEventPhase::Began, 1.0, 0.0, at, time, modifiers);
        assert_eq!(outer.len(), 1);
        let outer_id = outer[0].pointer().id;
        state.gesture(1, NSEventPhase::Began, 1.1, 0.0, at, time, modifiers);
        assert!(
            state
                .gesture(1, NSEventPhase::Ended, 1.0, 0.0, at, time, modifiers)
                .is_empty()
        );
        let rotation = state.gesture(2, NSEventPhase::Began, 1.0, 0.1, at, time, modifiers);
        assert_eq!(rotation.len(), 1);
        assert_eq!(rotation[0].pointer().id, outer_id);
        state.gesture(2, NSEventPhase::Ended, 1.0, 0.0, at, time, modifiers);
        let end = state.gesture(4, NSEventPhase::Ended, 1.0, 0.0, at, time, modifiers);
        assert_eq!(end.len(), 1);
        assert_eq!(end[0].phase, PanZoomPhase::End);
        assert_eq!(end[0].pointer().id, outer_id);
    }
}
