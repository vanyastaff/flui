//! Winit native pointer translation and private keyboard conversion.
//!
//! Each winit event is copied into a plain [`NativeReading`] and assembled by
//! the shared [`pointer_assembly`](crate::input::assembly): contacts
//! belong to exact native devices and windows (one surface per window), and
//! native phase changes commit before the platform publishes owned input to
//! user callbacks.
//!
//! What the translation decides, beyond copying fields:
//!
//! - **A device id is labelled per kind of input.** Wayland gives every
//!   pointer and touch event one id, X11 the master device's, AppKit's pinch
//!   the mouse's: the cursor, touches and trackpad gestures behind one id are
//!   separate devices, so each reports its kind.
//! - **A run of moves is one move.** winit stamps an event when it dispatches
//!   it, not when the OS measured it, so the moves a late loop turn
//!   dispatches back to back sit microseconds apart with real displacement.
//!   A move is held until the next event: another move of the same contact
//!   replaces it, anything else (or the end of the turn, [`WinitInput::flush`])
//!   delivers it first.
//! - **X11's first touch is not the mouse.** winit reports a `CursorMoved`
//!   at the first touch's position before its events (0.30 before its press
//!   and moves, not its lift); a held move that any event of a touch at its
//!   position follows is dropped, its lift and cancel included. A fractional
//!   `LineDelta` is a touchpad's smooth scrolling, reported precise.
//! - **A Wayland leave ends a press.** The compositor that takes the pointer
//!   mid-press (a grab, a drag) sends no release, so the mouse's sequence is
//!   cancelled at the leave; on X11 the implicit grab keeps delivering it.

use flui_platform::input_capabilities::InputCapabilities;
use flui_platform::{EventTime, pointer as owned};
use keyboard_types::Modifiers as KeyboardModifiers;
use std::collections::HashMap;
use ui_events_winit::keyboard::from_winit_modifier_state;
use winit::event::{ElementState, MouseButton, MouseScrollDelta};
use winit::event_loop::ActiveEventLoop;

use crate::host;
use crate::input::assembly::{
    ContactRegistry, DecodeError, GestureComponent, GestureDelta, GesturePhase, GestureReading,
    NativeCancel, NativeClock, NativeContact, NativeDevice, NativeReading, NativeSurface,
    PenAngles, RawSensors, ReadingAction, ReadingTime, ScrollPhases, ScrollReading, decode,
};
use crate::input::availability::WinitSession;
use crate::input::event_timestamp_ns;
use crate::input::keys::adapter::keyboard_input;
use crate::input::scroll::{WheelAmount, wheel_step};
use crate::owner::winit::{LoopInput, WinitLoop};
use flui_platform::PlatformInput;

/// The cursor of a device on a window. Touch labels are the platform's touch
/// ids, which no winit backend reports near the top of the `u64` range.
const MOUSE: NativeContact = NativeContact(u64::MAX);
/// The trackpad gesture of a device on a window.
const GESTURE: NativeContact = NativeContact(u64::MAX - 1);

/// One scroll step in the unit the device reported, in FLUI's direction
/// (positive scrolls down and right).
#[derive(Debug, Clone, Copy)]
pub(crate) struct ScrollStep {
    pub(crate) delta: owned::ScrollDelta,
    pub(crate) precision: owned::ScrollPrecision,
}

/// The step a winit wheel delta makes, by the one wheel rule
/// (`crate::input::scroll`): winit's deltas move the content. `scale` is
/// device pixels per logical pixel, and an invalid one refuses a pixel
/// delta; a refused step is zero in its unit. With
/// `fractional_lines_precise` (X11, whose touchpad scrolls in fractions of a
/// line), a line delta that is not whole is precise.
pub(crate) fn scroll_step(
    delta: &MouseScrollDelta,
    scale: f64,
    fractional_lines_precise: bool,
) -> ScrollStep {
    let scale = if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        f64::NAN
    };
    let (unit, x, y, precision) = match delta {
        MouseScrollDelta::LineDelta(x, y) => (
            if cfg!(target_os = "windows") {
                owned::ScrollUnit::Detents
            } else {
                owned::ScrollUnit::Lines
            },
            f64::from(*x),
            f64::from(*y),
            if fractional_lines_precise
                && [x, y]
                    .iter()
                    .any(|value| value.is_finite() && value.fract() != 0.0)
            {
                owned::ScrollPrecision::Precise
            } else {
                owned::ScrollPrecision::Notched
            },
        ),
        MouseScrollDelta::PixelDelta(delta) => (
            owned::ScrollUnit::Pixels,
            delta.x / scale,
            delta.y / scale,
            owned::ScrollPrecision::Precise,
        ),
    };
    let delta = wheel_step(unit, WheelAmount::content(x), WheelAmount::content(y))
        .unwrap_or_else(|_| owned::ScrollDelta::zero(unit));
    ScrollStep { delta, precision }
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

/// The kind of input a winit device id carries: one id can carry several
/// (see the module doc), and each gets its own device label.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KindClass {
    /// The cursor: moves, buttons, wheel, enter and leave.
    Cursor,
    /// Touches.
    Touch,
    /// Trackpad pinch and rotation.
    Gesture,
}

/// The labels minted for one winit device id.
#[derive(Debug, Default, Clone, Copy)]
struct DeviceLabels {
    /// Minted by `DeviceEvent::Added` before any reading told its kind: the
    /// first class that reports takes it.
    unclaimed: Option<NativeDevice>,
    cursor: Option<NativeDevice>,
    touch: Option<NativeDevice>,
    gesture: Option<NativeDevice>,
}

impl DeviceLabels {
    const fn slot(&mut self, class: KindClass) -> &mut Option<NativeDevice> {
        match class {
            KindClass::Cursor => &mut self.cursor,
            KindClass::Touch => &mut self.touch,
            KindClass::Gesture => &mut self.gesture,
        }
    }

    fn all(self) -> impl Iterator<Item = NativeDevice> {
        [self.unclaimed, self.cursor, self.touch, self.gesture]
            .into_iter()
            .flatten()
    }
}

/// A move held until the next event shows whether it is the last of its run.
#[derive(Debug, Clone, Copy)]
struct HeldMove {
    window: flui_platform::WindowId,
    reading: NativeReading,
}

/// The winit loop's input ([`LoopInput`]): its pointers assembled through one
/// registry, and its keys and input-method events translated.
pub(crate) struct WinitInput {
    contacts: ContactRegistry,
    labels: HashMap<winit::event::DeviceId, DeviceLabels>,
    next_label: u64,
    scrolling: ScrollPhases,
    present: flui_platform::input_capabilities::PointerKindSet,
    /// The OS session, once a window told it.
    session: Option<WinitSession>,
    held: Option<HeldMove>,
    /// Touch events of the loop turn so far, on a Wayland session. winit
    /// reports `wl_touch.frame` to no one, so the events one dispatch of the
    /// compositor's socket delivered, which hold whole frames, are the frame.
    touch_frame: Vec<(flui_platform::WindowId, owned::PointerEvent)>,
    /// Keys held down winit names no physical key for, with the key each
    /// was pressed as.
    unplaced: crate::input::keys::UnplacedKeys<winit::keyboard::PhysicalKey>,
}

/// Whether `event` starts, moves or ends a touch contact: what a frame groups.
fn is_touch_contact(event: &owned::PointerEvent) -> bool {
    use owned::PointerEvent::{Cancel, Down, Move, Up};
    let pointer = match event {
        Down(press) => &press.pointer,
        Up(release) => &release.pointer,
        Move(movement) => &movement.pointer,
        Cancel(cancel) => &cancel.pointer,
        _ => return false,
    };
    pointer.kind == owned::PointerKind::Touch
}
impl Default for WinitInput {
    fn default() -> Self {
        Self::with_registry(ContactRegistry::new(NativeClock::epoch_only()))
    }
}

impl WinitInput {
    fn with_registry(contacts: ContactRegistry) -> Self {
        Self {
            contacts,
            labels: HashMap::new(),
            next_label: 0,
            scrolling: ScrollPhases::default(),
            present: flui_platform::input_capabilities::PointerKindSet::empty(),
            session: None,
            held: None,
            touch_frame: Vec::new(),
            unplaced: crate::input::keys::UnplacedKeys::default(),
        }
    }

    /// Translate for windows of `session` from now on.
    pub(crate) const fn observe_session(&mut self, session: WinitSession) {
        self.session = Some(session);
    }

    /// Deliver the held move and the touch frame of the loop turn: the turn
    /// ended, or an event other than pointer input is about to be delivered
    /// after them.
    pub(crate) fn flush(&mut self) -> Vec<(flui_platform::WindowId, PlatformInput)> {
        let settled = self.settle(None);
        let mut output = self.frame_touches(settled);
        output.extend(self.take_touch_frame());
        output
    }

    /// Hold the touch events of `inputs` as the loop turn's frame; anything
    /// else ends the frame in front of it.
    fn frame_touches(
        &mut self,
        inputs: Vec<(flui_platform::WindowId, PlatformInput)>,
    ) -> Vec<(flui_platform::WindowId, PlatformInput)> {
        if self.session != Some(WinitSession::Wayland) {
            return inputs;
        }
        let mut output = Vec::new();
        for (window, input) in inputs {
            match input {
                PlatformInput::Pointer(event) if is_touch_contact(&event) => {
                    if self
                        .touch_frame
                        .first()
                        .is_some_and(|(held, _)| *held != window)
                    {
                        output.extend(self.take_touch_frame());
                    }
                    self.touch_frame.push((window, event));
                }
                input => {
                    output.extend(self.take_touch_frame());
                    output.push((window, input));
                }
            }
        }
        output
    }

    /// The held touch events as one input: a frame, or the lone event.
    fn take_touch_frame(&mut self) -> Vec<(flui_platform::WindowId, PlatformInput)> {
        let Some(&(window, _)) = self.touch_frame.first() else {
            return Vec::new();
        };
        let mut events: Vec<_> = std::mem::take(&mut self.touch_frame)
            .into_iter()
            .map(|(_, event)| event)
            .collect();
        let input = if events.len() == 1 {
            PlatformInput::Pointer(events.remove(0))
        } else {
            match flui_platform::PointerFrame::try_new(events) {
                Ok(frame) => PlatformInput::PointerFrame(frame),
                Err(empty) => unreachable!("BUG: a taken touch frame is not empty: {empty}"),
            }
        };
        vec![(window, input)]
    }

    /// Settle the held move before `next` on `window` is translated: keep it
    /// when `next` is the move that replaces it, drop it when `next` is the
    /// X11 touch it was synthesized for, and otherwise deliver it.
    fn settle(
        &mut self,
        next: Option<(flui_platform::WindowId, &winit::event::WindowEvent)>,
    ) -> Vec<(flui_platform::WindowId, PlatformInput)> {
        use winit::event::{TouchPhase, WindowEvent};
        let Some(held) = self.held else {
            return Vec::new();
        };
        let held_raw = held.reading.position.map(|raw| (raw.x, raw.y));
        let outcome = next.and_then(|(window, event)| {
            if window != held.window {
                return None;
            }
            match event {
                WindowEvent::CursorMoved { device_id, .. } => (held.reading.contact == MOUSE
                    && held.reading.device == Some(self.label(*device_id, KindClass::Cursor)))
                .then_some(true),
                WindowEvent::Touch(touch)
                    if held.reading.contact == MOUSE
                        && self.session == Some(WinitSession::X11)
                        && held_raw == Some((touch.location.x, touch.location.y)) =>
                {
                    Some(false)
                }
                WindowEvent::Touch(touch) => (touch.phase == TouchPhase::Moved
                    && held.reading.contact == NativeContact(touch.id)
                    && held.reading.kind == owned::PointerKind::Touch
                    && held.reading.device == Some(self.label(touch.device_id, KindClass::Touch)))
                .then_some(true),
                _ => None,
            }
        });
        match outcome {
            // Replaced by the move translated next.
            Some(true) => Vec::new(),
            // The synthetic cursor of an X11 touch.
            Some(false) => {
                self.held = None;
                Vec::new()
            }
            None => {
                self.held = None;
                decode(&mut self.contacts, &held.reading)
                    .unwrap_or_else(DecodeError::into_inputs)
                    .into_iter()
                    .map(|input| (held.window, input))
                    .collect()
            }
        }
    }

    /// The device kinds present now, from the registry that mints device ids.
    pub(crate) fn present(&self) -> flui_platform::input_capabilities::PointerKindSet {
        self.contacts.devices().present()
    }

    /// The device kinds present, when they changed since the last call: a
    /// device arrived, left, or its first reading told its kind.
    pub(crate) fn presence_change(
        &mut self,
    ) -> Option<flui_platform::input_capabilities::PointerKindSet> {
        let present = self.present();
        (present != self.present).then(|| {
            self.present = present;
            present
        })
    }

    /// The label for an opaque winit device's `class` of input, minted on
    /// first sight.
    fn label(&mut self, native: winit::event::DeviceId, class: KindClass) -> NativeDevice {
        let labels = self.labels.entry(native).or_default();
        if let Some(label) = *labels.slot(class) {
            return label;
        }
        let label = labels.unclaimed.take().unwrap_or_else(|| {
            self.next_label += 1;
            NativeDevice(self.next_label)
        });
        *labels.slot(class) = Some(label);
        label
    }

    /// The window's input for `event`, after the held move it settles (which
    /// may belong to another window); `None` when `event` is no pointer input.
    pub(crate) fn window_event(
        &mut self,
        window: flui_platform::WindowId,
        event: &winit::event::WindowEvent,
        scale: f64,
        modifiers: KeyboardModifiers,
    ) -> Option<Vec<(flui_platform::WindowId, PlatformInput)>> {
        let settled = self.settle(Some((window, event)));
        let translated = self.translate(window, event, scale, modifiers);
        let mut output = self.frame_touches(settled);
        // Whatever is not a touch ends the frame in front of its own input.
        if !matches!(event, winit::event::WindowEvent::Touch(_)) {
            output.extend(self.take_touch_frame());
        }
        let Some(translated) = translated else {
            return (!output.is_empty()).then_some(output);
        };
        let translated = translated
            .into_iter()
            .map(|input| (window, input))
            .collect();
        output.extend(self.frame_touches(translated));
        Some(output)
    }

    fn translate(
        &mut self,
        window: flui_platform::WindowId,
        event: &winit::event::WindowEvent,
        scale: f64,
        modifiers: KeyboardModifiers,
    ) -> Option<Vec<PlatformInput>> {
        use winit::event::{TouchPhase, WindowEvent};
        let mods = crate::input::keys::adapter::modifiers(modifiers);
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
        let mut output = Vec::new();
        let native = if let Some((native, gesture)) = gesture_reading(event) {
            let device = self.label(native, KindClass::Gesture);
            let cursor = self.label(native, KindClass::Cursor);
            let mut native = reading(
                device,
                GESTURE,
                owned::PointerKind::Trackpad,
                ReadingAction::Gesture(gesture),
            )
            .with_primary(true);
            // A gesture is centred on the device's cursor; once it moves on,
            // the gesture keeps its own last position.
            if let Some((_, raw)) = self.contacts.position(surface, Some(cursor), MOUSE) {
                native = native.at_device(raw.x, raw.y, scale);
            }
            native
        } else {
            match event {
                WindowEvent::CursorMoved {
                    device_id,
                    position,
                } => {
                    let native = mouse(
                        self.label(*device_id, KindClass::Cursor),
                        ReadingAction::Move,
                    )
                    .at_device(position.x, position.y, scale);
                    self.held = Some(HeldMove {
                        window,
                        reading: native,
                    });
                    return Some(output);
                }
                WindowEvent::TouchpadPressure {
                    device_id,
                    pressure,
                    ..
                } => mouse(
                    self.label(*device_id, KindClass::Cursor),
                    ReadingAction::Move,
                )
                .with_sensors(RawSensors {
                    pressure: (0.0..=1.0)
                        .contains(pressure)
                        .then_some(f64::from(*pressure)),
                    ..RawSensors::default()
                }),
                WindowEvent::CursorEntered { device_id } => mouse(
                    self.label(*device_id, KindClass::Cursor),
                    ReadingAction::Enter,
                ),
                WindowEvent::CursorLeft { device_id } => {
                    let device = self.label(*device_id, KindClass::Cursor);
                    let held = self
                        .contacts
                        .buttons(surface, Some(device), MOUSE)
                        .is_some_and(|buttons| !buttons.is_empty());
                    if held && self.session == Some(WinitSession::Wayland) {
                        output.extend(
                            decode(
                                &mut self.contacts,
                                &mouse(device, ReadingAction::Cancel(NativeCancel::Platform)),
                            )
                            .unwrap_or_else(DecodeError::into_inputs),
                        );
                    }
                    mouse(device, ReadingAction::Leave)
                }
                WindowEvent::MouseInput {
                    device_id,
                    state,
                    button,
                } => {
                    let button = native_button(*button);
                    mouse(
                        self.label(*device_id, KindClass::Cursor),
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
                    let native = reading(
                        self.label(touch.device_id, KindClass::Touch),
                        NativeContact(touch.id),
                        owned::PointerKind::Touch,
                        action,
                    )
                    .at_device(touch.location.x, touch.location.y, scale)
                    .with_sensors(touch_sensors(touch.force));
                    if touch.phase == TouchPhase::Moved {
                        self.held = Some(HeldMove {
                            window,
                            reading: native,
                        });
                        return Some(output);
                    }
                    native
                }
                WindowEvent::MouseWheel {
                    device_id,
                    delta,
                    phase,
                } => {
                    let step = scroll_step(delta, scale, self.session == Some(WinitSession::X11));
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
                    let step = scroll_step(&MouseScrollDelta::PixelDelta(pixels), scale, false);
                    return Some(self.scroll(window, device_id, step, *phase, mods));
                }
                _ => return None,
            }
        };
        output.extend(decode(&mut self.contacts, &native).unwrap_or_else(DecodeError::into_inputs));
        Some(output)
    }

    fn scroll(
        &mut self,
        window: flui_platform::WindowId,
        device_id: &winit::event::DeviceId,
        step: ScrollStep,
        phase: winit::event::TouchPhase,
        mods: flui_platform::keyboard::Modifiers,
    ) -> Vec<PlatformInput> {
        // A wheel and a two-finger pan scroll from the device's cursor.
        let device = self.label(*device_id, KindClass::Cursor);
        let surface = NativeSurface(window.0);
        let (Some(pointer), Some((position, _))) = (
            self.contacts.pointer(surface, Some(device), MOUSE),
            self.contacts.position(surface, Some(device), MOUSE),
        ) else {
            return Vec::new();
        };
        let ScrollStep { delta, precision } = step;
        // Only a pixel delta reports finger phases: X11's precise line
        // fractions arrive phaseless, as a wheel's do.
        let native_phase = self.scrolling.phase(
            surface,
            Some(device),
            ScrollReading::fingers(
                gesture_phase(phase),
                delta.unit() == owned::ScrollUnit::Pixels,
            ),
        );
        // A cancel of a gesture that never began moved nothing: it is no
        // wheel tick either.
        if native_phase.is_none() && phase == winit::event::TouchPhase::Cancelled {
            return Vec::new();
        }
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

    pub(crate) fn cancel_window(
        &mut self,
        window: flui_platform::WindowId,
        reason: NativeCancel,
    ) -> Vec<PlatformInput> {
        // The sequence a held move belongs to ends here; the move goes with it.
        self.touch_frame.retain(|(held, _)| *held != window);
        if self.held.is_some_and(|held| held.window == window) {
            self.held = None;
        }
        self.scrolling.cancel_surface(NativeSurface(window.0));
        self.contacts.cancel_surface(
            NativeSurface(window.0),
            EventTime::from_nanos(event_timestamp_ns()),
            reason,
        )
    }

    pub(crate) fn device_event(
        &mut self,
        native: winit::event::DeviceId,
        event: &winit::event::DeviceEvent,
        windows: &[flui_platform::WindowId],
    ) -> Vec<(flui_platform::WindowId, PlatformInput)> {
        use winit::event::DeviceEvent;
        let time = EventTime::from_nanos(event_timestamp_ns());
        if !matches!(event, DeviceEvent::Added | DeviceEvent::Removed) {
            return Vec::new();
        }
        // The platform delivers a held move first, after the presence it
        // reveals; one still held is decoded before the device changes.
        let mut events = self.flush();
        let changes = match event {
            DeviceEvent::Added => {
                // Before any reading says which kind it carries, the first
                // kind that reports takes this label.
                let labels = self.labels.entry(native).or_default();
                let device = labels.all().next().unwrap_or_else(|| {
                    self.next_label += 1;
                    let device = NativeDevice(self.next_label);
                    labels.unclaimed = Some(device);
                    device
                });
                let added = self
                    .contacts
                    .add_device(device, owned::PointerKind::Unknown, time);
                added
                    .map(owned::PointerEvent::DeviceAdded)
                    .into_iter()
                    .collect::<Vec<_>>()
            }
            DeviceEvent::Removed => {
                let Some(labels) = self.labels.remove(&native) else {
                    return events;
                };
                let mut changes = Vec::new();
                for device in labels.all() {
                    self.scrolling.remove_device(device);
                    let (ended, removed) = self.contacts.remove_device(device, time);
                    events.extend(
                        ended
                            .into_iter()
                            .map(|(surface, input)| (flui_platform::WindowId(surface.0), input)),
                    );
                    changes.extend(removed.map(owned::PointerEvent::DeviceRemoved));
                }
                changes
            }
            _ => Vec::new(),
        };
        for change in changes {
            events.extend(
                windows
                    .iter()
                    .map(|window| (*window, PlatformInput::Pointer(change.clone()))),
            );
        }
        events
    }
}

/// Convert winit's `Ime` event to [`flui_platform::ImeEvent`].
///
/// A pure, unit-tested mapping: winit's `Ime` enum is already
/// [`flui_platform::ImeEvent`]'s reference shape (see that type's module doc),
/// so this is a direct variant-for-variant translation with no coordinate
/// or encoding conversion.
fn ime_event(event: &winit::event::Ime) -> PlatformInput {
    use winit::event::Ime;

    let ime_event = match event {
        Ime::Enabled => flui_platform::ImeEvent::Enabled,
        Ime::Preedit(text, cursor) => flui_platform::ImeEvent::Preedit {
            text: text.clone(),
            cursor: *cursor,
        },
        Ime::Commit(text) => flui_platform::ImeEvent::Commit(text.clone()),
        Ime::Disabled => flui_platform::ImeEvent::Disabled,
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
/// `crates/flui-native/ARCHITECTURE.md` §Mapping decisions for why the
/// Win32 and AppKit backends keep their own hand-written `Code`/`Key`
/// tables instead of also delegating here, and the tests of
/// `input::apple::keys` (`cross_backend_physical_key_agreement`) for what
/// keeps the three in agreement.
fn keyboard_event(
    event: winit::event::KeyEvent,
    modifiers: winit::keyboard::ModifiersState,
) -> PlatformInput {
    keyboard_input(
        ui_events_winit::keyboard::from_winit_keyboard_event(event, modifiers),
        event_timestamp_ns(),
    )
}

/// The OS session `event_loop` runs in.
fn session(event_loop: &ActiveEventLoop) -> WinitSession {
    let _ = event_loop;
    #[cfg(target_os = "linux")]
    {
        use winit::platform::wayland::ActiveEventLoopExtWayland;
        if event_loop.is_wayland() {
            WinitSession::Wayland
        } else {
            WinitSession::X11
        }
    }
    #[cfg(target_os = "macos")]
    {
        WinitSession::MacOs
    }
    #[cfg(target_os = "windows")]
    {
        WinitSession::Windows
    }
}

/// The translation the winit loop holds ([`WinitHost::input`](crate::owner::winit::WinitHost::input)).
impl LoopInput for WinitInput {
    fn opening(&mut self, event_loop: &ActiveEventLoop) -> InputCapabilities {
        let session = session(event_loop);
        self.observe_session(session);
        crate::input::availability::winit(session).with_present(self.present())
    }

    fn window_event(
        &mut self,
        window: flui_platform::WindowId,
        event: &winit::event::WindowEvent,
        scale: f64,
        modifiers: winit::keyboard::ModifiersState,
    ) -> Option<Vec<(flui_platform::WindowId, PlatformInput)>> {
        Self::window_event(
            self,
            window,
            event,
            scale,
            from_winit_modifier_state(modifiers),
        )
    }

    fn device_event(
        &mut self,
        device: winit::event::DeviceId,
        event: &winit::event::DeviceEvent,
        windows: &[flui_platform::WindowId],
    ) -> Vec<(flui_platform::WindowId, PlatformInput)> {
        Self::device_event(self, device, event, windows)
    }

    fn presence_change(&mut self) -> Option<flui_platform::input_capabilities::PointerKindSet> {
        Self::presence_change(self)
    }

    fn flush(&mut self) -> Vec<(flui_platform::WindowId, PlatformInput)> {
        Self::flush(self)
    }

    fn focus_lost(&mut self, window: flui_platform::WindowId) -> Vec<PlatformInput> {
        self.cancel_window(window, NativeCancel::FocusLost)
    }

    fn key(
        &mut self,
        event: winit::event::KeyEvent,
        modifiers: winit::keyboard::ModifiersState,
    ) -> Option<PlatformInput> {
        let id = event.physical_key;
        Some(match keyboard_event(event, modifiers) {
            PlatformInput::Keyboard(key) => PlatformInput::Keyboard(self.unplaced.settle(id, key)),
            other => other,
        })
    }

    fn ime(&self, event: &winit::event::Ime) -> Option<PlatformInput> {
        Some(ime_event(event))
    }
}

/// The input position of the winit host's table.
impl host::Input<WinitLoop> for WinitLoop {
    fn keyboard_layout(_: &WinitLoop) -> String {
        // winit does not report the active keyboard layout: it is unknown.
        String::new()
    }

    fn on_keyboard_layout_change(_: &WinitLoop, callback: Box<dyn FnMut() + Send>) {
        // No layout is read, so no change is ever reported.
        drop(callback);
    }
}

#[cfg(test)]
mod pointer_translation_tests {
    use std::time::Instant;

    use super::*;
    use flui_foundation::geometry::Point;
    use flui_platform::pointer::PointerEvent;

    impl WinitInput {
        /// The window event's input as its own loop turn delivers it,
        /// without the announcement of a device first seen through it.
        fn window_input(
            &mut self,
            window: flui_platform::WindowId,
            event: &winit::event::WindowEvent,
            scale: f64,
            modifiers: KeyboardModifiers,
        ) -> Option<Vec<PlatformInput>> {
            let mut inputs = self.window_event(window, event, scale, modifiers)?;
            inputs.extend(self.flush());
            Some(announced_elsewhere(inputs))
        }

        /// The input of `events`, dispatched in one loop turn on `window`.
        fn turn(
            &mut self,
            window: flui_platform::WindowId,
            events: &[winit::event::WindowEvent],
        ) -> Vec<PlatformInput> {
            let mut inputs = Vec::new();
            for event in events {
                inputs.extend(
                    self.window_event(window, event, 1.0, KeyboardModifiers::empty())
                        .unwrap_or_default(),
                );
            }
            inputs.extend(self.flush());
            announced_elsewhere(inputs)
        }
    }

    fn announced_elsewhere(
        inputs: Vec<(flui_platform::WindowId, PlatformInput)>,
    ) -> Vec<PlatformInput> {
        inputs
            .into_iter()
            .map(|(_, input)| input)
            .filter(|input| !matches!(input, PlatformInput::Pointer(PointerEvent::DeviceAdded(_))))
            .collect()
    }

    fn moved(device_id: winit::event::DeviceId, x: f64, y: f64) -> winit::event::WindowEvent {
        winit::event::WindowEvent::CursorMoved {
            device_id,
            position: winit::dpi::PhysicalPosition::new(x, y),
        }
    }

    fn touched(
        device_id: winit::event::DeviceId,
        id: u64,
        phase: winit::event::TouchPhase,
        x: f64,
        y: f64,
    ) -> winit::event::WindowEvent {
        winit::event::WindowEvent::Touch(winit::event::Touch {
            device_id,
            phase,
            location: winit::dpi::PhysicalPosition::new(x, y),
            force: None,
            id,
        })
    }

    fn pressed(
        device_id: winit::event::DeviceId,
        state: ElementState,
    ) -> winit::event::WindowEvent {
        winit::event::WindowEvent::MouseInput {
            device_id,
            state,
            button: MouseButton::Left,
        }
    }

    /// The input as verbs with the pointer kind, device and capability
    /// events left out.
    fn verbs(inputs: &[PlatformInput]) -> Vec<String> {
        inputs
            .iter()
            .filter_map(|input| match input {
                PlatformInput::Pointer(event) => Some(match event {
                    PointerEvent::Enter(_) => "enter".to_owned(),
                    PointerEvent::Leave(_) => "leave".to_owned(),
                    PointerEvent::Move(movement) => {
                        let point = movement.current().position.get();
                        format!(
                            "{:?} move ({}, {})",
                            movement.pointer.kind, point.x, point.y
                        )
                    }
                    PointerEvent::Down(press) => format!("{:?} down", press.pointer.kind),
                    PointerEvent::Up(release) => format!("{:?} up", release.pointer.kind),
                    PointerEvent::Cancel(cancel) => format!("cancel {:?}", cancel.reason),
                    PointerEvent::Scroll(scroll) => {
                        format!("scroll {:?} {:?}", scroll.precision, scroll.phase)
                    }
                    other => format!("{other:?}"),
                }),
                _ => None,
            })
            .collect()
    }

    /// What each OS session's winit makes of the same input, row by row:
    /// The touches one Wayland loop turn delivered are one frame; the turn's
    /// end delivers them, and a session that frames nothing delivers each.
    #[test]
    fn wayland_touches_of_one_loop_turn_are_one_frame() {
        use winit::event::TouchPhase;
        let id = winit::event::DeviceId::dummy();
        let window = flui_platform::WindowId(1);
        let down = |state: &mut WinitInput, contact: u64, x: f64| {
            state
                .window_event(
                    window,
                    &touched(id, contact, TouchPhase::Started, x, 10.0),
                    1.0,
                    KeyboardModifiers::empty(),
                )
                .expect("translated")
        };
        let frames = |inputs: &[(flui_platform::WindowId, PlatformInput)]| {
            inputs
                .iter()
                .filter(|(_, input)| {
                    !matches!(input, PlatformInput::Pointer(PointerEvent::DeviceAdded(_)))
                })
                .map(|(_, input)| match input {
                    PlatformInput::PointerFrame(frame) => frame.len(),
                    PlatformInput::Pointer(_) => 1,
                    other => panic!("unexpected {other:?}"),
                })
                .collect::<Vec<_>>()
        };

        let mut wayland = WinitInput::default();
        wayland.observe_session(WinitSession::Wayland);
        let mut delivered = down(&mut wayland, 1, 100.0);
        delivered.extend(down(&mut wayland, 2, 300.0));
        assert!(
            frames(&delivered).is_empty(),
            "nothing is delivered before the turn ends: {delivered:?}"
        );
        assert_eq!(frames(&wayland.flush()), [2], "one frame of both touches");
        assert!(wayland.flush().is_empty(), "a frame is delivered once");

        let mut x11 = WinitInput::default();
        x11.observe_session(WinitSession::X11);
        let mut delivered = down(&mut x11, 1, 100.0);
        delivered.extend(down(&mut x11, 2, 300.0));
        assert_eq!(frames(&delivered), [1, 1], "X11 reports each touch alone");
    }

    /// Input that is not a touch ends the frame in front of it, so order
    /// holds, and a lone touch stays a plain event.
    #[test]
    fn a_non_touch_event_ends_the_touch_frame_before_it() {
        use winit::event::TouchPhase;
        let id = winit::event::DeviceId::dummy();
        let window = flui_platform::WindowId(1);
        let mut wayland = WinitInput::default();
        wayland.observe_session(WinitSession::Wayland);
        let touch = wayland
            .window_event(
                window,
                &touched(id, 1, TouchPhase::Started, 100.0, 10.0),
                1.0,
                KeyboardModifiers::empty(),
            )
            .expect("translated");
        assert!(
            touch.iter().all(|(_, input)| matches!(
                input,
                PlatformInput::Pointer(PointerEvent::DeviceAdded(_))
            )),
            "held for the turn: {touch:?}"
        );
        let after = wayland
            .window_event(
                window,
                &moved(id, 5.0, 5.0),
                1.0,
                KeyboardModifiers::empty(),
            )
            .expect("translated");
        let order = after
            .iter()
            .filter(|(_, input)| {
                !matches!(input, PlatformInput::Pointer(PointerEvent::DeviceAdded(_)))
            })
            .map(|(_, input)| match input {
                PlatformInput::Pointer(PointerEvent::Down(press)) => {
                    format!("{:?} down", press.pointer.kind)
                }
                PlatformInput::Pointer(_) => "pointer".to_owned(),
                other => format!("{other:?}"),
            })
            .collect::<Vec<_>>();
        assert_eq!(order.first().map(String::as_str), Some("Touch down"));
        assert!(
            order.iter().all(|entry| !entry.contains("Frame")),
            "a lone touch is not a frame: {order:?}"
        );
    }

    /// device ids shared across kinds, runs of moves, X11's synthetic first
    /// touch cursor and fractional line deltas, and a Wayland leave mid-press.
    #[test]
    fn session_quirks_translate_to_one_grammar() {
        use winit::event::{TouchPhase, WindowEvent};
        let id = winit::event::DeviceId::dummy();
        let window = flui_platform::WindowId(1);
        let session = |session| {
            let mut state = WinitInput::default();
            if let Some(session) = session {
                state.observe_session(session);
            }
            state
        };
        let wheel = |y| WindowEvent::MouseWheel {
            device_id: id,
            delta: MouseScrollDelta::LineDelta(0.0, y),
            phase: TouchPhase::Moved,
        };
        /// A row: its name, the session, the events of one loop turn and the
        /// input they deliver.
        type Row = (
            &'static str,
            Option<WinitSession>,
            Vec<WindowEvent>,
            Vec<&'static str>,
        );
        let rows: [Row; 8] = [
            (
                "a run of moves is its last",
                None,
                vec![
                    moved(id, 2.0, 2.0),
                    moved(id, 4.0, 4.0),
                    moved(id, 6.0, 6.0),
                ],
                vec!["Mouse move (6, 6)"],
            ),
            (
                "a move before a press is delivered before it",
                Some(WinitSession::X11),
                vec![moved(id, 2.0, 2.0), pressed(id, ElementState::Pressed)],
                vec!["Mouse move (2, 2)", "Mouse down"],
            ),
            (
                "the cursor X11 moves for the first touch is not the mouse",
                Some(WinitSession::X11),
                vec![
                    moved(id, 2.0, 2.0),
                    pressed(id, ElementState::Pressed),
                    pressed(id, ElementState::Released),
                    moved(id, 40.0, 30.0),
                    touched(id, 0, TouchPhase::Started, 40.0, 30.0),
                    moved(id, 42.0, 30.0),
                    touched(id, 0, TouchPhase::Moved, 42.0, 30.0),
                    touched(id, 0, TouchPhase::Ended, 42.0, 30.0),
                ],
                vec![
                    "Mouse move (2, 2)",
                    "Mouse down",
                    "Mouse up",
                    "Touch down",
                    "Touch move (42, 30)",
                    "Touch up",
                ],
            ),
            (
                "nor is one X11 moves at the touch's lift",
                Some(WinitSession::X11),
                vec![
                    moved(id, 40.0, 30.0),
                    touched(id, 0, TouchPhase::Started, 40.0, 30.0),
                    moved(id, 42.0, 30.0),
                    touched(id, 0, TouchPhase::Ended, 42.0, 30.0),
                ],
                vec!["Touch down", "Touch up"],
            ),
            (
                "elsewhere a cursor move is the mouse wherever it lands",
                Some(WinitSession::Wayland),
                vec![
                    moved(id, 40.0, 30.0),
                    touched(id, 0, TouchPhase::Started, 40.0, 30.0),
                ],
                vec!["Mouse move (40, 30)", "Touch down"],
            ),
            (
                "an X11 fractional line delta is a touchpad's precise scroll",
                Some(WinitSession::X11),
                vec![moved(id, 2.0, 2.0), wheel(0.25), wheel(1.0)],
                vec![
                    "Mouse move (2, 2)",
                    "scroll Precise None",
                    "scroll Notched None",
                ],
            ),
            (
                "a Wayland leave mid-press ends the press",
                Some(WinitSession::Wayland),
                vec![
                    moved(id, 2.0, 2.0),
                    pressed(id, ElementState::Pressed),
                    WindowEvent::CursorLeft { device_id: id },
                    WindowEvent::CursorEntered { device_id: id },
                    pressed(id, ElementState::Pressed),
                ],
                vec![
                    "Mouse move (2, 2)",
                    "Mouse down",
                    "cancel Platform",
                    "leave",
                    "enter",
                    "Mouse down",
                ],
            ),
            (
                "an X11 leave mid-press keeps it: the implicit grab delivers its release",
                Some(WinitSession::X11),
                vec![
                    moved(id, 2.0, 2.0),
                    pressed(id, ElementState::Pressed),
                    WindowEvent::CursorLeft { device_id: id },
                    pressed(id, ElementState::Released),
                ],
                vec!["Mouse move (2, 2)", "Mouse down", "leave", "Mouse up"],
            ),
        ];
        let mut failures = Vec::new();
        for (name, kind, events, expected) in rows {
            let mut state = session(kind);
            let seen = verbs(&state.turn(window, &events));
            if seen != expected {
                failures.push(format!("{name}: {seen:?}"));
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");

        // One device id for the mouse and the touchscreen (Wayland, X11's
        // master device) is two devices of known kind, both present.
        let mut state = session(Some(WinitSession::Wayland));
        state.turn(
            window,
            &[
                moved(id, 2.0, 2.0),
                touched(id, 0, TouchPhase::Started, 9.0, 9.0),
                touched(id, 0, TouchPhase::Ended, 9.0, 9.0),
            ],
        );
        let present = state.present();
        assert!(
            present.contains(flui_platform::input_capabilities::PointerKindSet::MOUSE)
                && present.contains(flui_platform::input_capabilities::PointerKindSet::TOUCH),
            "{present:?}"
        );
    }

    #[test]
    fn native_device_window_contact_and_removal_contract() {
        native_identity_exhaustion_never_reissues_a_contact();
        use winit::event::{DeviceEvent, Touch, TouchPhase, WindowEvent};
        let native = winit::event::DeviceId::dummy();
        let windows = [flui_platform::WindowId(1), flui_platform::WindowId(2)];
        let mut state = WinitInput::default();
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
        let window = flui_platform::WindowId(1);
        let mut state = WinitInput::with_registry(
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
        let window = flui_platform::WindowId(1);
        for pixels in [false, true] {
            let mut state = WinitInput::default();
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
                TouchPhase::Started,
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
                let mut inputs = state
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
                    .expect("wheel handled");
                // A cancel after the gesture ended takes nothing back.
                if index == 4 {
                    assert!(inputs.is_empty(), "an orphan cancel scrolls: {inputs:?}");
                    continue;
                }
                let input = inputs.remove(0);
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
                        .contains(flui_platform::keyboard::Modifiers::SHIFT)
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
        let mut state = WinitInput::default();
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
        use flui_platform::pointer::PanZoomPhase;
        use winit::event::{TouchPhase, WindowEvent};

        let native = winit::event::DeviceId::dummy();
        let window = flui_platform::WindowId(1);
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
            let mut state = WinitInput::default();
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
        let window = flui_platform::WindowId(1);
        let mut state = WinitInput::default();
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
        let window = flui_platform::WindowId(1);
        let mut state = WinitInput::default();
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
            let mut state = WinitInput::default();
            let mut inputs = state
                .window_input(
                    flui_platform::WindowId(1),
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
                    .map(flui_platform::pointer::Pressure::get),
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
    #[test]
    fn translated_events_meet_the_pointer_field_contract() {
        let position = winit::dpi::PhysicalPosition::new(10.0, 10.0);
        let mut state = WinitInput::default();
        let window = flui_platform::WindowId(1);
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

#[cfg(test)]
mod keyboard_conversion_tests {
    //! Pins the winit backend's delegation to `ui-events-winit` (see
    //! `keyboard_event`'s doc in this file) rather than exercising
    //! `keyboard_event` itself: `winit::event::KeyEvent` has a
    //! `pub(crate) platform_specific` field
    //! (`winit-0.30.13/src/event.rs:133`), so nothing outside winit can
    //! construct one. `keyboard_event` now contains no conversion logic of
    //! its own either way — it is one call into
    //! `from_winit_keyboard_event`, which itself calls the
    //! `ui_events_winit::keyboard::from_winit_*` functions tested here
    //! directly. See `every_winit_keycode_maps_to_a_canonical_code`'s doc
    //! for exactly what that leaves unpinned.
    use std::collections::BTreeSet;

    use keyboard_types::Code;
    use ui_events_winit::keyboard::from_winit_code;
    use winit::keyboard::{KeyCode, PhysicalKey};

    /// Every `winit::keyboard::KeyCode` variant, winit 0.30.13
    /// (`winit-0.30.13/src/keyboard.rs`, the `KeyCode` enum), in the exact
    /// order it is declared there. The length assertion in the test below
    /// is what catches a silently truncated copy of this list; the
    /// `BTreeSet` assertion alongside it catches a duplicated entry
    /// standing in for a dropped one (same length, wrong coverage).
    const ALL_WINIT_KEYCODES: &[KeyCode] = &[
        KeyCode::Backquote,
        KeyCode::Backslash,
        KeyCode::BracketLeft,
        KeyCode::BracketRight,
        KeyCode::Comma,
        KeyCode::Digit0,
        KeyCode::Digit1,
        KeyCode::Digit2,
        KeyCode::Digit3,
        KeyCode::Digit4,
        KeyCode::Digit5,
        KeyCode::Digit6,
        KeyCode::Digit7,
        KeyCode::Digit8,
        KeyCode::Digit9,
        KeyCode::Equal,
        KeyCode::IntlBackslash,
        KeyCode::IntlRo,
        KeyCode::IntlYen,
        KeyCode::KeyA,
        KeyCode::KeyB,
        KeyCode::KeyC,
        KeyCode::KeyD,
        KeyCode::KeyE,
        KeyCode::KeyF,
        KeyCode::KeyG,
        KeyCode::KeyH,
        KeyCode::KeyI,
        KeyCode::KeyJ,
        KeyCode::KeyK,
        KeyCode::KeyL,
        KeyCode::KeyM,
        KeyCode::KeyN,
        KeyCode::KeyO,
        KeyCode::KeyP,
        KeyCode::KeyQ,
        KeyCode::KeyR,
        KeyCode::KeyS,
        KeyCode::KeyT,
        KeyCode::KeyU,
        KeyCode::KeyV,
        KeyCode::KeyW,
        KeyCode::KeyX,
        KeyCode::KeyY,
        KeyCode::KeyZ,
        KeyCode::Minus,
        KeyCode::Period,
        KeyCode::Quote,
        KeyCode::Semicolon,
        KeyCode::Slash,
        KeyCode::AltLeft,
        KeyCode::AltRight,
        KeyCode::Backspace,
        KeyCode::CapsLock,
        KeyCode::ContextMenu,
        KeyCode::ControlLeft,
        KeyCode::ControlRight,
        KeyCode::Enter,
        KeyCode::SuperLeft,
        KeyCode::SuperRight,
        KeyCode::ShiftLeft,
        KeyCode::ShiftRight,
        KeyCode::Space,
        KeyCode::Tab,
        KeyCode::Convert,
        KeyCode::KanaMode,
        KeyCode::Lang1,
        KeyCode::Lang2,
        KeyCode::Lang3,
        KeyCode::Lang4,
        KeyCode::Lang5,
        KeyCode::NonConvert,
        KeyCode::Delete,
        KeyCode::End,
        KeyCode::Help,
        KeyCode::Home,
        KeyCode::Insert,
        KeyCode::PageDown,
        KeyCode::PageUp,
        KeyCode::ArrowDown,
        KeyCode::ArrowLeft,
        KeyCode::ArrowRight,
        KeyCode::ArrowUp,
        KeyCode::NumLock,
        KeyCode::Numpad0,
        KeyCode::Numpad1,
        KeyCode::Numpad2,
        KeyCode::Numpad3,
        KeyCode::Numpad4,
        KeyCode::Numpad5,
        KeyCode::Numpad6,
        KeyCode::Numpad7,
        KeyCode::Numpad8,
        KeyCode::Numpad9,
        KeyCode::NumpadAdd,
        KeyCode::NumpadBackspace,
        KeyCode::NumpadClear,
        KeyCode::NumpadClearEntry,
        KeyCode::NumpadComma,
        KeyCode::NumpadDecimal,
        KeyCode::NumpadDivide,
        KeyCode::NumpadEnter,
        KeyCode::NumpadEqual,
        KeyCode::NumpadHash,
        KeyCode::NumpadMemoryAdd,
        KeyCode::NumpadMemoryClear,
        KeyCode::NumpadMemoryRecall,
        KeyCode::NumpadMemoryStore,
        KeyCode::NumpadMemorySubtract,
        KeyCode::NumpadMultiply,
        KeyCode::NumpadParenLeft,
        KeyCode::NumpadParenRight,
        KeyCode::NumpadStar,
        KeyCode::NumpadSubtract,
        KeyCode::Escape,
        KeyCode::Fn,
        KeyCode::FnLock,
        KeyCode::PrintScreen,
        KeyCode::ScrollLock,
        KeyCode::Pause,
        KeyCode::BrowserBack,
        KeyCode::BrowserFavorites,
        KeyCode::BrowserForward,
        KeyCode::BrowserHome,
        KeyCode::BrowserRefresh,
        KeyCode::BrowserSearch,
        KeyCode::BrowserStop,
        KeyCode::Eject,
        KeyCode::LaunchApp1,
        KeyCode::LaunchApp2,
        KeyCode::LaunchMail,
        KeyCode::MediaPlayPause,
        KeyCode::MediaSelect,
        KeyCode::MediaStop,
        KeyCode::MediaTrackNext,
        KeyCode::MediaTrackPrevious,
        KeyCode::Power,
        KeyCode::Sleep,
        KeyCode::AudioVolumeDown,
        KeyCode::AudioVolumeMute,
        KeyCode::AudioVolumeUp,
        KeyCode::WakeUp,
        KeyCode::Meta,
        KeyCode::Hyper,
        KeyCode::Turbo,
        KeyCode::Abort,
        KeyCode::Resume,
        KeyCode::Suspend,
        KeyCode::Again,
        KeyCode::Copy,
        KeyCode::Cut,
        KeyCode::Find,
        KeyCode::Open,
        KeyCode::Paste,
        KeyCode::Props,
        KeyCode::Select,
        KeyCode::Undo,
        KeyCode::Hiragana,
        KeyCode::Katakana,
        KeyCode::F1,
        KeyCode::F2,
        KeyCode::F3,
        KeyCode::F4,
        KeyCode::F5,
        KeyCode::F6,
        KeyCode::F7,
        KeyCode::F8,
        KeyCode::F9,
        KeyCode::F10,
        KeyCode::F11,
        KeyCode::F12,
        KeyCode::F13,
        KeyCode::F14,
        KeyCode::F15,
        KeyCode::F16,
        KeyCode::F17,
        KeyCode::F18,
        KeyCode::F19,
        KeyCode::F20,
        KeyCode::F21,
        KeyCode::F22,
        KeyCode::F23,
        KeyCode::F24,
        KeyCode::F25,
        KeyCode::F26,
        KeyCode::F27,
        KeyCode::F28,
        KeyCode::F29,
        KeyCode::F30,
        KeyCode::F31,
        KeyCode::F32,
        KeyCode::F33,
        KeyCode::F34,
        KeyCode::F35,
    ];

    /// Every winit 0.30.13 `KeyCode` has a non-`Unidentified` canonical
    /// `Code` through `ui-events-winit`'s `from_winit_code` — the property
    /// this whole issue is about. Before this change,
    /// `convert_physical_key`'s own hand table covered 71 of these 194
    /// variants (letters, digits, a small nav/modifier subset, F1..F12) and
    /// fell through to `Unidentified` for the other 123 — numpad, Intl*,
    /// F13+, media/browser/system keys among them.
    ///
    /// What this test does NOT pin: a regression introduced *inside*
    /// `keyboard_event` itself (a wrong function called, a field dropped
    /// from the event it builds) would stay invisible here, because
    /// `winit::event::KeyEvent` cannot be constructed outside winit (its
    /// `platform_specific` field is `pub(crate)`,
    /// `winit-0.30.13/src/event.rs:133`) and so `keyboard_event` can never
    /// be called directly from a test. What this test pins is the
    /// completeness of the `ui-events-winit` version `Cargo.lock` resolves
    /// to — the dependency `keyboard_event` delegates to unconditionally.
    #[test]
    fn every_winit_keycode_maps_to_a_canonical_code() {
        assert_eq!(ALL_WINIT_KEYCODES.len(), 194);
        assert_eq!(
            ALL_WINIT_KEYCODES.iter().collect::<BTreeSet<_>>().len(),
            194,
            "a duplicated entry would keep the length check above green while \
             silently standing in for a variant this list dropped"
        );
        let unidentified: Vec<KeyCode> = ALL_WINIT_KEYCODES
            .iter()
            .copied()
            .filter(|&k| from_winit_code(PhysicalKey::Code(k)) == Code::Unidentified)
            .collect();
        assert!(
            unidentified.is_empty(),
            "{} winit KeyCode variant(s) produced Code::Unidentified: {unidentified:?}",
            unidentified.len()
        );
    }
}
