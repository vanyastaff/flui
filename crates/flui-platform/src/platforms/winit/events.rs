//! Winit native pointer translation and private keyboard conversion.
//!
//! Each winit event is copied into a plain [`NativeReading`] and assembled by
//! the shared [`pointer_assembly`](crate::shared::pointer_assembly): contacts
//! belong to exact native devices and windows (one surface per window), and
//! native phase changes commit before the platform publishes owned input to
//! user callbacks.

use flui_platform_api::{EventTime, pointer as owned};
use keyboard_types::Modifiers as KeyboardModifiers;
use std::collections::HashMap;
use winit::event::{ElementState, MouseButton, MouseScrollDelta};

use crate::{
    shared::events::event_timestamp_ns,
    shared::keyboard_adapter::keyboard_input,
    shared::pointer_assembly::{
        ContactRegistry, DecodeError, GestureComponent, GestureDelta, GesturePhase, GestureReading,
        NativeCancel, NativeClock, NativeContact, NativeDevice, NativeReading, NativeSurface,
        PenAngles, RawSensors, ReadingAction, ReadingTime, ScrollPhases, ScrollReading, decode,
    },
    traits::PlatformInput,
};

/// The cursor of a device on a window. Touch labels are the platform's touch
/// ids, which no winit backend reports near the top of the `u64` range.
const MOUSE: NativeContact = NativeContact(u64::MAX);
/// The trackpad gesture of a device on a window.
const GESTURE: NativeContact = NativeContact(u64::MAX - 1);

/// One scroll step in the unit the device reported, already in the owned
/// sign convention (positive scrolls down and right).
#[derive(Debug, Clone, Copy)]
struct ScrollStep {
    unit: owned::ScrollUnit,
    x: f64,
    y: f64,
    precision: owned::ScrollPrecision,
}

/// The step a winit wheel delta makes; `scale` is device pixels per logical
/// pixel, and an invalid one refuses a pixel delta (it becomes zero).
fn wheel_step(delta: &MouseScrollDelta, scale: f64) -> ScrollStep {
    let scale = if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        f64::NAN
    };
    match delta {
        MouseScrollDelta::LineDelta(x, y) => ScrollStep {
            unit: if cfg!(target_os = "windows") {
                owned::ScrollUnit::Detents
            } else {
                owned::ScrollUnit::Lines
            },
            x: -f64::from(*x),
            y: -f64::from(*y),
            precision: owned::ScrollPrecision::Notched,
        },
        MouseScrollDelta::PixelDelta(delta) => ScrollStep {
            unit: owned::ScrollUnit::Pixels,
            x: -delta.x / scale,
            y: -delta.y / scale,
            precision: owned::ScrollPrecision::Precise,
        },
    }
}

fn native_button(button: MouseButton) -> owned::PointerButton {
    // Native vendor buttons share the vocabulary's finite non-actuating band,
    // so two vendor buttons 26 apart are one owned button.
    match button {
        MouseButton::Left => owned::PointerButton::PRIMARY,
        MouseButton::Right => owned::PointerButton::SECONDARY,
        MouseButton::Middle => owned::PointerButton::AUXILIARY,
        MouseButton::Back => owned::PointerButton::BACK,
        MouseButton::Forward => owned::PointerButton::FORWARD,
        MouseButton::Other(id) => owned::PointerButton::try_from(7 + (id % 26) as u8)
            .expect("BUG: vendor button maps into the non-actuating 7..=32 band"),
    }
}

/// Winit's touch force, kept only when it is a valid normalized reading.
fn touch_sensors(force: Option<winit::event::Force>) -> RawSensors {
    let mut sensors = RawSensors::default();
    if let Some(force) = force {
        let normalized = force.normalized();
        if (0.0..=1.0).contains(&normalized) {
            sensors.pressure = Some(normalized);
        }
        if let winit::event::Force::Calibrated {
            altitude_angle: Some(altitude),
            ..
        } = force
            && (0.0..=core::f64::consts::FRAC_PI_2).contains(&altitude)
        {
            sensors.angles = Some(PenAngles::Spherical {
                altitude: Some(altitude),
                azimuth: None,
            });
        }
    }
    sensors
}

fn gesture_phase(phase: winit::event::TouchPhase) -> GesturePhase {
    match phase {
        winit::event::TouchPhase::Started => GesturePhase::Began,
        winit::event::TouchPhase::Moved => GesturePhase::Changed,
        winit::event::TouchPhase::Ended => GesturePhase::Ended,
        winit::event::TouchPhase::Cancelled => GesturePhase::Cancelled,
    }
}

/// The gesture reading a winit trackpad event makes, with its device.
fn gesture_reading(
    event: &winit::event::WindowEvent,
) -> Option<(winit::event::DeviceId, GestureReading)> {
    use winit::event::WindowEvent;
    let (device, component, delta, phase) = match event {
        WindowEvent::PinchGesture {
            device_id,
            delta,
            phase,
        } => (
            device_id,
            GestureComponent::Scale,
            GestureDelta::Scale(1.0 + delta),
            phase,
        ),
        WindowEvent::RotationGesture {
            device_id,
            delta,
            phase,
        } => (
            device_id,
            GestureComponent::Rotation,
            // Winit reports counterclockwise degrees; owned rotation is clockwise radians.
            GestureDelta::Rotation(-f64::from(*delta).to_radians()),
            phase,
        ),
        _ => return None,
    };
    Some((
        *device,
        GestureReading {
            component,
            phase: gesture_phase(*phase),
            delta,
        },
    ))
}

pub(super) struct NativePointerState {
    contacts: ContactRegistry,
    labels: HashMap<winit::event::DeviceId, NativeDevice>,
    next_label: u64,
    scrolling: ScrollPhases,
    present: flui_platform_api::input_capabilities::PointerKindSet,
}

impl Default for NativePointerState {
    fn default() -> Self {
        Self::with_registry(ContactRegistry::new(NativeClock::epoch_only()))
    }
}

impl NativePointerState {
    fn with_registry(contacts: ContactRegistry) -> Self {
        Self {
            contacts,
            labels: HashMap::new(),
            next_label: 0,
            scrolling: ScrollPhases::default(),
            present: flui_platform_api::input_capabilities::PointerKindSet::empty(),
        }
    }

    /// The device kinds present now, from the registry that mints device ids.
    pub(super) fn present(&self) -> flui_platform_api::input_capabilities::PointerKindSet {
        self.contacts.devices().present()
    }

    /// The device kinds present, when they changed since the last call: a
    /// device arrived, left, or its first reading told its kind.
    pub(super) fn presence_change(
        &mut self,
    ) -> Option<flui_platform_api::input_capabilities::PointerKindSet> {
        let present = self.present();
        (present != self.present).then(|| {
            self.present = present;
            present
        })
    }

    /// The label for an opaque winit device, minted on first sight.
    fn label(&mut self, native: winit::event::DeviceId) -> NativeDevice {
        *self.labels.entry(native).or_insert_with(|| {
            self.next_label += 1;
            NativeDevice(self.next_label)
        })
    }

    pub(super) fn window_event(
        &mut self,
        window: crate::traits::WindowId,
        event: &winit::event::WindowEvent,
        scale: f64,
        modifiers: KeyboardModifiers,
    ) -> Option<Vec<PlatformInput>> {
        use winit::event::{TouchPhase, WindowEvent};
        let mods = crate::shared::keyboard_adapter::modifiers(modifiers);
        let surface = NativeSurface(window.0);
        let now = ReadingTime::epoch(EventTime::from_nanos(event_timestamp_ns()));
        let reading = |device, contact, kind, action| {
            NativeReading::new(contact, kind, action, now)
                .on(surface)
                .from_device(device)
                .with_modifiers(mods)
        };
        let mouse = |device, action| {
            reading(device, MOUSE, owned::PointerKind::Mouse, action).with_primary(true)
        };
        let native = if let Some((native, gesture)) = gesture_reading(event) {
            let device = self.label(native);
            let mut native = reading(
                device,
                GESTURE,
                owned::PointerKind::Trackpad,
                ReadingAction::Gesture(gesture),
            )
            .with_primary(true);
            // A gesture is centred on the device's cursor; once it moves on,
            // the gesture keeps its own last position.
            if let Some((_, raw)) = self.contacts.position(surface, Some(device), MOUSE) {
                native = native.at(raw.x, raw.y, scale);
            }
            native
        } else {
            match event {
                WindowEvent::CursorMoved {
                    device_id,
                    position,
                } => mouse(self.label(*device_id), ReadingAction::Move)
                    .at(position.x, position.y, scale),
                WindowEvent::TouchpadPressure {
                    device_id,
                    pressure,
                    ..
                } => mouse(self.label(*device_id), ReadingAction::Move).with_sensors(RawSensors {
                    pressure: (0.0..=1.0)
                        .contains(pressure)
                        .then_some(f64::from(*pressure)),
                    ..RawSensors::default()
                }),
                WindowEvent::CursorEntered { device_id } => {
                    mouse(self.label(*device_id), ReadingAction::Enter)
                }
                WindowEvent::CursorLeft { device_id } => {
                    mouse(self.label(*device_id), ReadingAction::Leave)
                }
                WindowEvent::MouseInput {
                    device_id,
                    state,
                    button,
                } => {
                    let button = native_button(*button);
                    mouse(
                        self.label(*device_id),
                        if *state == ElementState::Pressed {
                            ReadingAction::Press(button)
                        } else {
                            ReadingAction::Release(button)
                        },
                    )
                }
                WindowEvent::Touch(touch) => {
                    let action = match touch.phase {
                        TouchPhase::Started => ReadingAction::Press(owned::PointerButton::PRIMARY),
                        TouchPhase::Moved => ReadingAction::Move,
                        TouchPhase::Ended => ReadingAction::Release(owned::PointerButton::PRIMARY),
                        TouchPhase::Cancelled => ReadingAction::Cancel(NativeCancel::Platform),
                    };
                    reading(
                        self.label(touch.device_id),
                        NativeContact(touch.id),
                        owned::PointerKind::Touch,
                        action,
                    )
                    .at(touch.location.x, touch.location.y, scale)
                    .with_sensors(touch_sensors(touch.force))
                }
                WindowEvent::MouseWheel {
                    device_id,
                    delta,
                    phase,
                } => {
                    let step = wheel_step(delta, scale);
                    return Some(self.scroll(window, device_id, step, *phase, mods));
                }
                // A two-finger pan is a phased scroll on every backend
                // (ADR-0217 §4): the content follows the fingers.
                WindowEvent::PanGesture {
                    device_id,
                    delta,
                    phase,
                } => {
                    let pixels =
                        winit::dpi::PhysicalPosition::new(f64::from(delta.x), f64::from(delta.y));
                    let step = wheel_step(&MouseScrollDelta::PixelDelta(pixels), scale);
                    return Some(self.scroll(window, device_id, step, *phase, mods));
                }
                _ => return None,
            }
        };
        Some(
            decode(&mut self.contacts, &native)
                .unwrap_or_else(DecodeError::into_inputs)
                .into_vec(),
        )
    }

    fn scroll(
        &mut self,
        window: crate::traits::WindowId,
        device_id: &winit::event::DeviceId,
        step: ScrollStep,
        phase: winit::event::TouchPhase,
        mods: flui_platform_api::keyboard::Modifiers,
    ) -> Vec<PlatformInput> {
        let device = self.label(*device_id);
        let surface = NativeSurface(window.0);
        let (Some(pointer), Some((position, _))) = (
            self.contacts.pointer(surface, Some(device), MOUSE),
            self.contacts.position(surface, Some(device), MOUSE),
        ) else {
            return Vec::new();
        };
        let ScrollStep {
            unit,
            x,
            y,
            precision,
        } = step;
        let delta = owned::ScrollDelta::try_new(unit, x, y)
            .unwrap_or_else(|_| owned::ScrollDelta::zero(unit));
        let native_phase = self.scrolling.phase(
            surface,
            Some(device),
            ScrollReading::fingers(
                gesture_phase(phase),
                precision == owned::ScrollPrecision::Precise,
            ),
        );
        let mut scroll = owned::ScrollEvent::new(
            pointer,
            EventTime::from_nanos(event_timestamp_ns()),
            position,
            delta,
        )
        .with_modifiers(mods)
        .with_precision(precision);
        scroll.phase = native_phase;
        vec![PlatformInput::Pointer(owned::PointerEvent::Scroll(scroll))]
    }

    pub(super) fn cancel_window(
        &mut self,
        window: crate::traits::WindowId,
        reason: NativeCancel,
    ) -> Vec<PlatformInput> {
        self.scrolling.cancel_surface(NativeSurface(window.0));
        self.contacts.cancel_surface(
            NativeSurface(window.0),
            EventTime::from_nanos(event_timestamp_ns()),
            reason,
        )
    }

    pub(super) fn device_event(
        &mut self,
        native: winit::event::DeviceId,
        event: &winit::event::DeviceEvent,
        windows: &[crate::traits::WindowId],
    ) -> Vec<(crate::traits::WindowId, PlatformInput)> {
        use winit::event::DeviceEvent;
        let time = EventTime::from_nanos(event_timestamp_ns());
        let (mut events, change) = match event {
            DeviceEvent::Added => {
                let device = self.label(native);
                let added = self
                    .contacts
                    .add_device(device, owned::PointerKind::Unknown, time);
                (Vec::new(), added.map(owned::PointerEvent::DeviceAdded))
            }
            DeviceEvent::Removed => {
                let Some(device) = self.labels.remove(&native) else {
                    return Vec::new();
                };
                self.scrolling.remove_device(device);
                let (ended, removed) = self.contacts.remove_device(device, time);
                let ended = ended
                    .into_iter()
                    .map(|(surface, input)| (crate::traits::WindowId(surface.0), input))
                    .collect();
                (ended, removed.map(owned::PointerEvent::DeviceRemoved))
            }
            _ => return Vec::new(),
        };
        if let Some(change) = change {
            events.extend(
                windows
                    .iter()
                    .map(|window| (*window, PlatformInput::Pointer(change.clone()))),
            );
        }
        events
    }
}

/// Convert winit's `Ime` event to [`flui_platform_api::ImeEvent`].
///
/// A pure, unit-tested mapping: winit's `Ime` enum is already
/// [`flui_platform_api::ImeEvent`]'s reference shape (see that type's module doc),
/// so this is a direct variant-for-variant translation with no coordinate
/// or encoding conversion.
pub fn ime_event(event: &winit::event::Ime) -> PlatformInput {
    use winit::event::Ime;

    let ime_event = match event {
        Ime::Enabled => flui_platform_api::ImeEvent::Enabled,
        Ime::Preedit(text, cursor) => flui_platform_api::ImeEvent::Preedit {
            text: text.clone(),
            cursor: *cursor,
        },
        Ime::Commit(text) => flui_platform_api::ImeEvent::Commit(text.clone()),
        Ime::Disabled => flui_platform_api::ImeEvent::Disabled,
    };

    PlatformInput::Ime(ime_event)
}

/// Convert a winit `KeyboardInput` (event + live modifiers state) to a W3C
/// `KeyboardEvent`, wrapped as a `PlatformInput`.
///
/// The whole event delegates to
/// `ui_events_winit::keyboard::from_winit_keyboard_event` (the same
/// ecosystem bridge Masonry and Xilem use) — code, logical key, location,
/// down/up state, `repeat`, modifiers, and `is_composing: false` (FLUI has
/// no other source for IME-composition state on this path, so this is the
/// same constant the crate itself hard-codes, not a FLUI-specific value
/// lost by delegating). Nothing here hand-assembles a `KeyboardEvent` field
/// any more. `Code::Unidentified` therefore means exactly one thing: winit
/// itself reported `PhysicalKey::Unidentified`, i.e. the OS/backend could
/// not name the physical key — never an incomplete conversion table. See
/// `crates/flui-platform/ARCHITECTURE.md` §Mapping decisions for why the
/// Win32 and AppKit backends keep their own hand-written `Code`/`Key`
/// tables instead of also delegating here, and this module's
/// `keyboard_tests::cross_backend_physical_key_agreement` for what keeps
/// the three in agreement.
pub fn keyboard_event(
    event: winit::event::KeyEvent,
    modifiers: winit::keyboard::ModifiersState,
) -> PlatformInput {
    keyboard_input(
        ui_events_winit::keyboard::from_winit_keyboard_event(event, modifiers),
        event_timestamp_ns(),
    )
}

#[cfg(test)]
mod pointer_translation_tests {
    use std::time::Instant;

    use super::*;
    use flui_foundation::geometry::Point;
    use flui_platform_api::pointer::PointerEvent;

    impl NativePointerState {
        /// The window event's input without the announcement of a device
        /// first seen through it.
        fn window_input(
            &mut self,
            window: crate::traits::WindowId,
            event: &winit::event::WindowEvent,
            scale: f64,
            modifiers: KeyboardModifiers,
        ) -> Option<Vec<PlatformInput>> {
            let mut inputs = self.window_event(window, event, scale, modifiers)?;
            inputs.retain(|input| {
                !matches!(input, PlatformInput::Pointer(PointerEvent::DeviceAdded(_)))
            });
            Some(inputs)
        }
    }

    #[test]
    fn native_device_window_contact_and_removal_contract() {
        native_identity_exhaustion_never_reissues_a_contact();
        use winit::event::{DeviceEvent, Touch, TouchPhase, WindowEvent};
        let native = winit::event::DeviceId::dummy();
        let windows = [crate::traits::WindowId(1), crate::traits::WindowId(2)];
        let mut state = NativePointerState::default();
        let added = state.device_event(native, &DeviceEvent::Added, &windows);
        let PlatformInput::Pointer(PointerEvent::DeviceAdded(first)) = &added[0].1 else {
            panic!("native Added")
        };
        assert_eq!(
            first.kind,
            owned::PointerKind::Unknown,
            "Added does not name a native device kind"
        );
        let device = first.device;
        let mut contact = |window, id, phase| {
            state
                .window_input(
                    window,
                    &WindowEvent::Touch(Touch {
                        device_id: native,
                        id,
                        phase,
                        location: winit::dpi::PhysicalPosition::new(20.0, 30.0),
                        force: None,
                    }),
                    2.0,
                    KeyboardModifiers::empty(),
                )
                .expect("native touch handled")
                .remove(0)
        };
        let PlatformInput::Pointer(PointerEvent::Down(primary)) =
            contact(windows[0], 0, TouchPhase::Started)
        else {
            panic!("first contact")
        };
        let PlatformInput::Pointer(PointerEvent::Down(additional)) =
            contact(windows[0], 1, TouchPhase::Started)
        else {
            panic!("additional contact")
        };
        let PlatformInput::Pointer(PointerEvent::Down(independent)) =
            contact(windows[1], 0, TouchPhase::Started)
        else {
            panic!("other window contact")
        };
        assert_eq!(primary.pointer.role, owned::PointerRole::Primary);
        assert_eq!(additional.pointer.role, owned::PointerRole::Additional);
        assert_eq!(independent.pointer.role, owned::PointerRole::Primary);
        assert_eq!(primary.pointer.device, Some(device));
        assert_ne!(primary.pointer.id, additional.pointer.id);
        assert_ne!(
            primary.pointer.id, independent.pointer.id,
            "same native contact label in a different window does not alias"
        );
        let removed = state.device_event(native, &DeviceEvent::Removed, &windows);
        assert_eq!(removed.len(), 5);
        let mut canceled: Vec<_> = removed[..3]
            .iter()
            .map(|(_, input)| match input {
                PlatformInput::Pointer(PointerEvent::Cancel(cancel)) => {
                    assert_eq!(cancel.reason, owned::CancelReason::DeviceRemoved);
                    cancel.pointer.id
                }
                _ => panic!("all active contacts cancel before native removal is published"),
            })
            .collect();
        canceled.sort();
        let mut expected = vec![
            primary.pointer.id,
            additional.pointer.id,
            independent.pointer.id,
        ];
        expected.sort();
        assert_eq!(canceled, expected);
        assert!(removed[3..].iter().all(|(_, input)| matches!(input, PlatformInput::Pointer(PointerEvent::DeviceRemoved(change)) if change.device == device && change.kind == owned::PointerKind::Touch)));
        let next = state
            .window_input(
                windows[0],
                &WindowEvent::Touch(Touch {
                    device_id: native,
                    id: 0,
                    phase: TouchPhase::Started,
                    location: winit::dpi::PhysicalPosition::new(20.0, 30.0),
                    force: None,
                }),
                2.0,
                KeyboardModifiers::empty(),
            )
            .expect("contact after removal handled")
            .remove(0);
        let PlatformInput::Pointer(PointerEvent::Down(next)) = next else {
            panic!("next contact")
        };
        assert_eq!(next.pointer.role, owned::PointerRole::Primary);
        assert_ne!(next.pointer.device, Some(device));
        assert!(!expected.contains(&next.pointer.id));
    }

    fn native_identity_exhaustion_never_reissues_a_contact() {
        use winit::event::{DeviceEvent, Touch, TouchPhase, WindowEvent};
        // The last identity hardware takes, below the reserved contactless ones.
        let last_hardware =
            std::num::NonZeroU64::new(owned::DeviceId::KEYBOARD.get().get() - 1).expect("nonzero");
        let native = winit::event::DeviceId::dummy();
        let window = crate::traits::WindowId(1);
        let mut state = NativePointerState::with_registry(
            ContactRegistry::new(NativeClock::epoch_only())
                .with_first_pointer(std::num::NonZeroU64::MAX)
                .with_first_device(last_hardware),
        );
        let touch = |phase, id| {
            WindowEvent::Touch(Touch {
                device_id: native,
                phase,
                location: winit::dpi::PhysicalPosition::new(1.0, 2.0),
                force: None,
                id,
            })
        };
        let accepted = state
            .window_input(
                window,
                &touch(TouchPhase::Started, 0),
                1.0,
                KeyboardModifiers::empty(),
            )
            .expect("last identity accepted");
        let PlatformInput::Pointer(PointerEvent::Down(down)) = accepted[0] else {
            panic!("Down")
        };
        assert_eq!(down.pointer.id.get().get(), u64::MAX);
        assert_eq!(
            down.pointer.device.expect("actual device").get(),
            last_hardware
        );
        let terminal = state
            .window_input(
                window,
                &touch(TouchPhase::Ended, 0),
                1.0,
                KeyboardModifiers::empty(),
            )
            .expect("last contact retires");
        assert!(matches!(
            terminal.as_slice(),
            [PlatformInput::Pointer(PointerEvent::Up(_))]
        ));
        for id in [1, 2] {
            assert!(
                state
                    .window_input(
                        window,
                        &touch(TouchPhase::Started, id),
                        1.0,
                        KeyboardModifiers::empty()
                    )
                    .unwrap_or_default()
                    .is_empty()
            );
        }
        let removed = state.device_event(native, &DeviceEvent::Removed, &[window]);
        assert_eq!(removed.len(), 1);
        for _ in 0..2 {
            assert!(
                state
                    .device_event(native, &DeviceEvent::Added, &[window])
                    .is_empty()
            );
        }
    }

    #[test]
    fn native_scroll_units_precision_and_phases() {
        use winit::event::{TouchPhase, WindowEvent};
        let native = winit::event::DeviceId::dummy();
        let window = crate::traits::WindowId(1);
        for pixels in [false, true] {
            let mut state = NativePointerState::default();
            state.window_input(
                window,
                &WindowEvent::CursorMoved {
                    device_id: native,
                    position: winit::dpi::PhysicalPosition::new(100.0, 50.0),
                },
                2.0,
                KeyboardModifiers::empty(),
            );
            let delta = if pixels {
                MouseScrollDelta::PixelDelta(winit::dpi::PhysicalPosition::new(10.0, -20.0))
            } else {
                MouseScrollDelta::LineDelta(1.0, -2.0)
            };
            let phases = [
                TouchPhase::Moved,
                TouchPhase::Started,
                TouchPhase::Moved,
                TouchPhase::Ended,
                TouchPhase::Cancelled,
            ];
            for (index, phase) in phases.into_iter().enumerate() {
                let invalid_end = phase == TouchPhase::Ended;
                let input_delta = if invalid_end {
                    if pixels {
                        MouseScrollDelta::PixelDelta(winit::dpi::PhysicalPosition::new(
                            f64::NAN,
                            0.0,
                        ))
                    } else {
                        MouseScrollDelta::LineDelta(f32::NAN, 0.0)
                    }
                } else {
                    delta
                };
                let input = state
                    .window_input(
                        window,
                        &WindowEvent::MouseWheel {
                            device_id: native,
                            delta: input_delta,
                            phase,
                        },
                        2.0,
                        KeyboardModifiers::SHIFT,
                    )
                    .expect("wheel handled")
                    .remove(0);
                let PlatformInput::Pointer(PointerEvent::Scroll(scroll)) = input else {
                    panic!("owned Scroll")
                };
                assert_eq!(scroll.position.get(), Point::new(50.0, 25.0));
                assert_eq!(
                    scroll.phase,
                    match phase {
                        TouchPhase::Started => Some(owned::ScrollPhase::Began),
                        TouchPhase::Moved if pixels || index > 0 =>
                            Some(owned::ScrollPhase::Changed),
                        TouchPhase::Moved => None,
                        TouchPhase::Ended => Some(owned::ScrollPhase::Ended),
                        TouchPhase::Cancelled => Some(owned::ScrollPhase::Cancelled),
                    }
                );
                assert_eq!(
                    scroll.delta.unit(),
                    if pixels {
                        owned::ScrollUnit::Pixels
                    } else if cfg!(target_os = "windows") {
                        owned::ScrollUnit::Detents
                    } else {
                        owned::ScrollUnit::Lines
                    }
                );
                assert_eq!(
                    scroll.precision,
                    if pixels {
                        owned::ScrollPrecision::Precise
                    } else {
                        owned::ScrollPrecision::Notched
                    }
                );
                assert!(
                    scroll
                        .modifiers
                        .contains(flui_platform_api::keyboard::Modifiers::SHIFT)
                );
                if invalid_end {
                    assert_eq!((scroll.delta.x(), scroll.delta.y()), (0.0, 0.0));
                    assert_eq!(scroll.phase, Some(owned::ScrollPhase::Ended));
                } else {
                    assert_eq!(
                        (scroll.delta.x(), scroll.delta.y()),
                        if pixels { (-5.0, 10.0) } else { (-1.0, 2.0) }
                    );
                }
            }
        }
        let mut state = NativePointerState::default();
        state.window_input(
            window,
            &WindowEvent::CursorMoved {
                device_id: native,
                position: winit::dpi::PhysicalPosition::new(1.0, 2.0),
            },
            1.0,
            KeyboardModifiers::empty(),
        );
        let input = state
            .window_input(
                window,
                &WindowEvent::MouseWheel {
                    device_id: native,
                    delta: MouseScrollDelta::LineDelta(0.0, 1.0),
                    phase: TouchPhase::Moved,
                },
                1.0,
                KeyboardModifiers::empty(),
            )
            .expect("wheel handled")
            .remove(0);
        let PlatformInput::Pointer(PointerEvent::Scroll(scroll)) = input else {
            panic!("owned Scroll")
        };
        assert_eq!(
            scroll.phase, None,
            "a plain wheel tick does not start a gesture"
        );
    }

    #[test]
    fn native_pan_zoom_phase_and_recovery_matrix() {
        native_pan_zoom_converter_keeps_terminal_after_invalid_position();
        use flui_platform_api::pointer::PanZoomPhase;
        use winit::event::{TouchPhase, WindowEvent};

        let native = winit::event::DeviceId::dummy();
        let window = crate::traits::WindowId(1);
        let pinch = |delta, phase| WindowEvent::PinchGesture {
            device_id: native,
            delta,
            phase,
        };
        let rotation = |delta, phase| WindowEvent::RotationGesture {
            device_id: native,
            delta,
            phase,
        };
        for competing in [false, true] {
            let mut state = NativePointerState::default();
            state.window_input(
                window,
                &WindowEvent::CursorMoved {
                    device_id: native,
                    position: winit::dpi::PhysicalPosition::new(10.0, 15.0),
                },
                1.0,
                KeyboardModifiers::empty(),
            );
            let mut stream = vec![
                pinch(0.0, TouchPhase::Moved),
                pinch(0.0, TouchPhase::Started),
                pinch(0.0, TouchPhase::Started),
                pinch(0.5, TouchPhase::Moved),
            ];
            if competing {
                stream.push(rotation(0.0, TouchPhase::Started));
                stream.push(rotation(90.0, TouchPhase::Moved));
            }
            stream.push(pinch(f64::NAN, TouchPhase::Moved));
            stream.push(pinch(0.0, TouchPhase::Ended));
            if competing {
                stream.push(rotation(0.0, TouchPhase::Cancelled));
            }
            let events: Vec<_> = stream
                .iter()
                .flat_map(|event| {
                    state
                        .window_input(window, event, 1.0, KeyboardModifiers::empty())
                        .expect("native gesture handled")
                })
                .map(|event| match event {
                    PlatformInput::Pointer(PointerEvent::PanZoom(event)) => event,
                    other => panic!("only PanZoom expected, got {other:?}"),
                })
                .collect();
            assert_eq!(
                events.len(),
                if competing { 4 } else { 3 },
                "duplicate/orphan/invalid phases produce no extra messages"
            );
            assert!(matches!(events[0].phase, PanZoomPhase::Start));
            let PanZoomPhase::Update(zoom) = events[1].phase else {
                panic!("pinch update")
            };
            assert_eq!(zoom.scale(), 1.5);
            let terminal = if competing { 3 } else { 2 };
            if competing {
                let PanZoomPhase::Update(rotation) = events[2].phase else {
                    panic!("rotation update")
                };
                assert_eq!(rotation.scale(), 1.5);
                assert_eq!(rotation.rotation(), -core::f64::consts::FRAC_PI_2);
                assert!(matches!(events[terminal].phase, PanZoomPhase::Cancelled));
            } else {
                assert!(matches!(events[terminal].phase, PanZoomPhase::End));
            }
            let first = events[0].pointer().id;
            assert!(
                events[..=terminal]
                    .iter()
                    .all(|event| event.pointer().id == first)
            );
        }
    }

    /// Winit's two-finger `PanGesture` is a phased pixel scroll at the
    /// cursor, never a pan-zoom: the content follows the fingers, and an
    /// invalid final delta keeps the native `Ended`.
    #[test]
    fn winit_pan_gesture_is_a_phased_scroll() {
        use winit::event::{TouchPhase, WindowEvent};
        let native = winit::event::DeviceId::dummy();
        let window = crate::traits::WindowId(1);
        let mut state = NativePointerState::default();
        state.window_input(
            window,
            &WindowEvent::CursorMoved {
                device_id: native,
                position: winit::dpi::PhysicalPosition::new(10.0, 15.0),
            },
            2.0,
            KeyboardModifiers::empty(),
        );
        let pan = |x, y, phase| WindowEvent::PanGesture {
            device_id: native,
            delta: winit::dpi::PhysicalPosition::new(x, y),
            phase,
        };
        let scrolls: Vec<_> = [
            pan(0.0, 0.0, TouchPhase::Started),
            pan(4.0, -2.0, TouchPhase::Moved),
            pan(f32::NAN, 0.0, TouchPhase::Ended),
        ]
        .iter()
        .flat_map(|event| {
            state
                .window_input(window, event, 2.0, KeyboardModifiers::empty())
                .expect("native pan handled")
        })
        .map(|event| match event {
            PlatformInput::Pointer(PointerEvent::Scroll(scroll)) => scroll,
            other => panic!("only Scroll expected, got {other:?}"),
        })
        .collect();
        assert_eq!(
            scrolls
                .iter()
                .map(|scroll| scroll.phase)
                .collect::<Vec<_>>(),
            [
                Some(owned::ScrollPhase::Began),
                Some(owned::ScrollPhase::Changed),
                Some(owned::ScrollPhase::Ended),
            ]
        );
        assert_eq!((scrolls[1].delta.x(), scrolls[1].delta.y()), (-2.0, 1.0));
        assert_eq!(scrolls[1].delta.unit(), owned::ScrollUnit::Pixels);
        assert_eq!(scrolls[1].precision, owned::ScrollPrecision::Precise);
        assert_eq!(scrolls[1].position.get(), Point::new(5.0, 7.5));
        assert_eq!((scrolls[2].delta.x(), scrolls[2].delta.y()), (0.0, 0.0));
    }

    fn native_pan_zoom_converter_keeps_terminal_after_invalid_position() {
        use winit::event::{TouchPhase, WindowEvent};
        let native = winit::event::DeviceId::dummy();
        let window = crate::traits::WindowId(1);
        let mut state = NativePointerState::default();
        state.window_input(
            window,
            &WindowEvent::CursorMoved {
                device_id: native,
                position: winit::dpi::PhysicalPosition::new(20.0, 30.0),
            },
            2.0,
            KeyboardModifiers::empty(),
        );
        let first = state
            .window_input(
                window,
                &WindowEvent::PinchGesture {
                    device_id: native,
                    delta: 0.5,
                    phase: TouchPhase::Started,
                },
                2.0,
                KeyboardModifiers::empty(),
            )
            .expect("native pinch handled");
        assert_eq!(
            first.len(),
            2,
            "native start and actual initial delta are delivered"
        );
        let PlatformInput::Pointer(PointerEvent::PanZoom(start)) = first[0] else {
            panic!("Start")
        };
        let PlatformInput::Pointer(PointerEvent::PanZoom(update)) = first[1] else {
            panic!("Update")
        };
        assert!(matches!(start.phase, owned::PanZoomPhase::Start));
        assert_eq!(start.pointer().id, update.pointer().id);
        let owned::PanZoomPhase::Update(transform) = update.phase else {
            panic!("Update transform")
        };
        assert_eq!(transform.scale(), 1.5);
        state.window_input(
            window,
            &WindowEvent::CursorMoved {
                device_id: native,
                position: winit::dpi::PhysicalPosition::new(f64::NAN, 30.0),
            },
            2.0,
            KeyboardModifiers::empty(),
        );
        let terminal = state
            .window_input(
                window,
                &WindowEvent::PinchGesture {
                    device_id: native,
                    delta: f64::NAN,
                    phase: TouchPhase::Ended,
                },
                2.0,
                KeyboardModifiers::empty(),
            )
            .expect("native pinch handled");
        assert_eq!(terminal.len(), 1);
        let PlatformInput::Pointer(PointerEvent::PanZoom(end)) = terminal[0] else {
            panic!("End")
        };
        assert!(matches!(end.phase, owned::PanZoomPhase::End));
        assert_eq!(end.pointer().id, start.pointer().id);
        assert_eq!(end.position.get(), Point::new(10.0, 15.0));
        assert!(
            state
                .window_input(
                    window,
                    &WindowEvent::PinchGesture {
                        device_id: native,
                        delta: 0.5,
                        phase: TouchPhase::Moved,
                    },
                    2.0,
                    KeyboardModifiers::empty()
                )
                .unwrap_or_default()
                .is_empty()
        );
    }

    #[test]
    fn native_touch_sensor_presence_is_preserved() {
        for (name, force, expected) in [
            ("absent", None, None),
            (
                "zero",
                Some(winit::event::Force::Normalized(0.0)),
                Some(0.0),
            ),
            (
                "normalized",
                Some(winit::event::Force::Normalized(0.75)),
                Some(0.75),
            ),
            (
                "invalid",
                Some(winit::event::Force::Normalized(f64::NAN)),
                None,
            ),
            (
                "out_of_range",
                Some(winit::event::Force::Normalized(2.0)),
                None,
            ),
            (
                "calibrated_altitude",
                Some(winit::event::Force::Calibrated {
                    force: 0.0,
                    max_possible_force: 1.0,
                    altitude_angle: Some(core::f64::consts::FRAC_PI_4),
                }),
                Some(0.0),
            ),
        ] {
            let touch = winit::event::Touch {
                device_id: winit::event::DeviceId::dummy(),
                phase: winit::event::TouchPhase::Started,
                location: winit::dpi::PhysicalPosition::new(20.0, 30.0),
                force,
                id: 0,
            };
            let mut state = NativePointerState::default();
            let mut inputs = state
                .window_input(
                    crate::traits::WindowId(1),
                    &winit::event::WindowEvent::Touch(touch),
                    2.0,
                    KeyboardModifiers::empty(),
                )
                .expect("native touch handled");
            let PlatformInput::Pointer(PointerEvent::Down(down)) = inputs.remove(0) else {
                panic!("{name}: expected native touch Down")
            };
            assert_eq!(
                down.sample
                    .pressure
                    .map(flui_platform_api::pointer::Pressure::get),
                expected,
                "{name}"
            );
            assert_eq!(
                down.sample.position.get(),
                flui_foundation::geometry::Point::new(10.0, 15.0),
                "{name}"
            );
            assert_eq!(
                down.sample.contact_size, None,
                "{name}: winit reports no contact size"
            );
            if name == "calibrated_altitude" {
                let orientation = down.sample.orientation.expect("native altitude retained");
                assert_eq!(orientation.altitude(), Some(core::f64::consts::FRAC_PI_4));
                assert_eq!(orientation.azimuth(), None, "winit provides no azimuth");
            } else {
                assert_eq!(down.sample.orientation, None);
            }
        }
    }

    /// The cross-wire field contract (flui-interaction's module doc): time
    /// in NANOSECONDS, no pressure sensor on a mouse, and no invented click count.
    pub(super) fn translated_events_meet_the_pointer_field_contract() {
        let position = winit::dpi::PhysicalPosition::new(10.0, 10.0);
        let mut state = NativePointerState::default();
        let window = crate::traits::WindowId(1);
        let device = winit::event::DeviceId::dummy();
        let moved = winit::event::WindowEvent::CursorMoved {
            device_id: device,
            position,
        };

        // time: nanosecond scale — spin ~2ms of real time between two
        // stamps; a millisecond stamp would show a delta of ~2, a
        // nanosecond stamp ~2_000_000. (A bounded spin on Instant, not a
        // pacing sleep: elapsed time IS the measured phenomenon here.)
        let PlatformInput::Pointer(PointerEvent::Move(first)) = state
            .window_input(window, &moved, 1.0, KeyboardModifiers::empty())
            .expect("native move handled")
            .remove(0)
        else {
            panic!("expected Move");
        };
        let spin_start = Instant::now();
        while spin_start.elapsed() < std::time::Duration::from_millis(2) {
            std::hint::spin_loop();
        }
        let PlatformInput::Pointer(PointerEvent::Move(second)) = state
            .window_input(window, &moved, 1.0, KeyboardModifiers::empty())
            .expect("native move handled")
            .remove(0)
        else {
            panic!("expected Move");
        };
        let delta = second.current().time.as_nanos() - first.current().time.as_nanos();
        assert!(
            delta >= 1_000_000,
            "~2ms between stamps must read as ~2,000,000 time units — the \
             contract is nanoseconds, and a millisecond stamp reads {delta}"
        );

        // pressure + count on a Down with a held button.
        let pressed = winit::event::WindowEvent::MouseInput {
            device_id: device,
            state: ElementState::Pressed,
            button: MouseButton::Left,
        };
        let PlatformInput::Pointer(PointerEvent::Down(down)) = state
            .window_input(window, &pressed, 1.0, KeyboardModifiers::empty())
            .expect("native press handled")
            .remove(0)
        else {
            panic!("expected Down");
        };
        assert_eq!(down.sample.pressure, None, "a mouse has no pressure sensor");
        assert_eq!(
            down.click_count.map(core::num::NonZeroU8::get),
            None,
            "winit MouseInput reports no click count"
        );

        // A hover move carries neither.
        assert_eq!(first.current().pressure, None);
    }
}

// The winit-vs-ui-events-winit keyboard conversion tests live in their own
// file: two cohesive families (completeness of the delegated conversion,
// and cross-backend agreement with the Win32/AppKit hand-written tables)
// large enough that this file's production code should not sit in its
// first quarter. A sibling module rather than a nested one, so its tests
// sit beside `pointer_translation_tests` rather than two
// segments below them — same shape as `platform.rs`'s `real_loop_tests`.
#[cfg(test)]
#[path = "events/keyboard_tests.rs"]
mod keyboard_tests;
