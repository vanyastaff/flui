//! Winit native pointer translation and private keyboard conversion.
//!
//! Pointer state belongs to exact native devices and windows. Native phase
//! changes commit before the platform publishes owned input to user callbacks.

use flui_foundation::geometry::{Offset, Point};
use flui_platform_api::{EventTime, pointer as owned};
use keyboard_types::Modifiers as KeyboardModifiers;
use std::collections::{HashMap, HashSet};
use winit::event::{ElementState, MouseButton, MouseScrollDelta};

use crate::{
    shared::events::event_timestamp_ns, shared::keyboard_adapter::keyboard_input,
    traits::PlatformInput,
};

#[derive(Clone, Copy)]
enum GestureDelta {
    Pinch(f64),
    Pan(Offset<f64>),
    Rotation(f32),
}

impl GestureDelta {
    fn from_native(
        event: &winit::event::WindowEvent,
        scale: f64,
    ) -> Option<(winit::event::DeviceId, Self, winit::event::TouchPhase)> {
        use winit::event::WindowEvent;
        let scale = if scale.is_finite() && scale > 0.0 {
            scale
        } else {
            f64::NAN
        };
        match event {
            WindowEvent::PinchGesture {
                device_id,
                delta,
                phase,
            } => Some((*device_id, Self::Pinch(*delta), *phase)),
            WindowEvent::RotationGesture {
                device_id,
                delta,
                phase,
            } => Some((*device_id, Self::Rotation(*delta), *phase)),
            WindowEvent::PanGesture {
                device_id,
                delta,
                phase,
            } => Some((
                *device_id,
                Self::Pan(Offset::new(
                    f64::from(delta.x) / scale,
                    f64::from(delta.y) / scale,
                )),
                *phase,
            )),
            _ => None,
        }
    }

    fn component(self) -> u8 {
        match self {
            Self::Pinch(_) => 1,
            Self::Pan(_) => 2,
            Self::Rotation(_) => 4,
        }
    }

    fn apply(self, previous: owned::PanZoomTransform) -> Option<owned::PanZoomTransform> {
        let (pan, scale, rotation) = match self {
            Self::Pinch(delta) => (
                previous.pan(),
                previous.scale() * (1.0 + delta),
                previous.rotation(),
            ),
            Self::Pan(delta) => (
                previous.pan() + delta,
                previous.scale(),
                previous.rotation(),
            ),
            Self::Rotation(delta) => (
                previous.pan(),
                previous.scale(),
                previous.rotation() - f64::from(delta).to_radians(),
            ),
        };
        owned::PanZoomTransform::try_new(pan, scale, rotation).ok()
    }
}

struct PanZoomContact {
    pointer: owned::PointerInfo,
    components: u8,
    transform: owned::PanZoomTransform,
    position: owned::PointerPosition,
}

#[derive(Default)]
struct PanZoomState {
    contact: Option<PanZoomContact>,
}

impl PanZoomState {
    fn event(
        &mut self,
        delta: GestureDelta,
        phase: winit::event::TouchPhase,
        pointer: owned::PointerInfo,
        position: owned::PointerPosition,
        modifiers: KeyboardModifiers,
    ) -> Vec<PlatformInput> {
        use winit::event::TouchPhase;
        let mut events = Vec::new();
        let component = delta.component();
        if phase == TouchPhase::Started {
            if self
                .contact
                .as_ref()
                .is_some_and(|contact| contact.components & component != 0)
            {
                return events;
            }
            if let Some(contact) = self.contact.as_mut() {
                contact.components |= component;
            } else {
                self.contact = Some(PanZoomContact {
                    pointer,
                    components: component,
                    transform: owned::PanZoomTransform::IDENTITY,
                    position,
                });
                events.push(pan_zoom_input(
                    pointer,
                    position,
                    owned::PanZoomPhase::Start,
                    modifiers,
                ));
            }
        }
        let Some(contact) = self.contact.as_mut() else {
            return events;
        };
        if phase == TouchPhase::Cancelled {
            let contact = self
                .contact
                .take()
                .expect("BUG: active native gesture was checked");
            events.push(pan_zoom_input(
                contact.pointer,
                position,
                owned::PanZoomPhase::Cancelled,
                modifiers,
            ));
            return events;
        }
        if contact.components & component == 0 {
            return events;
        }
        contact.position = position;
        if let Some(transform) = delta.apply(contact.transform)
            && transform != contact.transform
        {
            contact.transform = transform;
            events.push(pan_zoom_input(
                contact.pointer,
                position,
                owned::PanZoomPhase::Update(transform),
                modifiers,
            ));
        }
        if phase == TouchPhase::Ended {
            contact.components &= !component;
            if contact.components == 0 {
                let contact = self
                    .contact
                    .take()
                    .expect("BUG: active native gesture was checked");
                events.push(pan_zoom_input(
                    contact.pointer,
                    position,
                    owned::PanZoomPhase::End,
                    modifiers,
                ));
            }
        }
        events
    }

    fn cancel(&mut self, modifiers: KeyboardModifiers) -> Option<PlatformInput> {
        let contact = self.contact.take()?;
        Some(pan_zoom_input(
            contact.pointer,
            contact.position,
            owned::PanZoomPhase::Cancelled,
            modifiers,
        ))
    }
}

fn pan_zoom_input(
    pointer: owned::PointerInfo,
    position: owned::PointerPosition,
    phase: owned::PanZoomPhase,
    modifiers: KeyboardModifiers,
) -> PlatformInput {
    PlatformInput::Pointer(owned::PointerEvent::PanZoom(
        owned::PanZoomEvent::new(
            pointer,
            EventTime::from_nanos(event_timestamp_ns()),
            position,
            phase,
        )
        .with_modifiers(crate::shared::keyboard_adapter::modifiers(modifiers)),
    ))
}

struct MouseContact {
    pointer: owned::PointerInfo,
    position: Option<owned::PointerPosition>,
    held: HashSet<MouseButton>,
    suppressed: HashSet<MouseButton>,
    scroll_started: bool,
}

impl MouseContact {
    fn event(
        &mut self,
        event: &winit::event::WindowEvent,
        scale: f64,
        mods: flui_platform_api::keyboard::Modifiers,
    ) -> Vec<PlatformInput> {
        use winit::event::WindowEvent;
        let mut inputs = Vec::new();
        match event {
            WindowEvent::CursorMoved { position, .. } => {
                self.position = native_position(*position, scale);
                if let Some(position) = self.position {
                    inputs.push(PlatformInput::Pointer(owned::PointerEvent::Move(
                        owned::PointerMove::new(
                            self.pointer,
                            self.buttons(),
                            native_sample(position),
                        )
                        .with_modifiers(mods),
                    )));
                }
            }
            WindowEvent::TouchpadPressure { pressure, .. } => {
                if let Some(position) = self.position {
                    let mut sample = native_sample(position);
                    sample.pressure = owned::Pressure::try_new(*pressure).ok();
                    inputs.push(PlatformInput::Pointer(owned::PointerEvent::Move(
                        owned::PointerMove::new(self.pointer, self.buttons(), sample)
                            .with_modifiers(mods),
                    )));
                }
            }
            WindowEvent::CursorEntered { .. } | WindowEvent::CursorLeft { .. } => {
                let mut signal = owned::PointerSignal::new(
                    self.pointer,
                    EventTime::from_nanos(event_timestamp_ns()),
                );
                signal.position = self.position;
                let event = if matches!(event, WindowEvent::CursorEntered { .. }) {
                    owned::PointerEvent::Enter(signal)
                } else {
                    owned::PointerEvent::Leave(signal)
                };
                inputs.push(PlatformInput::Pointer(event));
            }
            _ => {}
        }
        inputs
    }

    fn buttons(&self) -> owned::PointerButtons {
        self.held
            .iter()
            .fold(owned::PointerButtons::NONE, |held, button| {
                held.with(native_button(*button))
            })
    }
}

fn native_button(button: MouseButton) -> owned::PointerButton {
    // Preserve native vendor-button identity within the vocabulary's finite
    // non-actuating band. Raw held buttons remain distinct before this mapping.
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

fn native_position(
    position: winit::dpi::PhysicalPosition<f64>,
    scale: f64,
) -> Option<owned::PointerPosition> {
    if !scale.is_finite() || scale <= 0.0 {
        return None;
    }
    owned::PointerPosition::try_new(Point::new(position.x / scale, position.y / scale)).ok()
}

fn native_sample(position: owned::PointerPosition) -> owned::PointerSample {
    owned::PointerSample::new(EventTime::from_nanos(event_timestamp_ns()), position)
}

fn native_touch_sample(
    position: owned::PointerPosition,
    force: Option<winit::event::Force>,
) -> owned::PointerSample {
    let mut sample = native_sample(position);
    if let Some(force) = force {
        let normalized = force.normalized();
        if (0.0..=1.0).contains(&normalized) {
            // Validate f64 before rounding into the owned f32 sensor representation.
            #[expect(
                clippy::cast_possible_truncation,
                reason = "validated normalized force fits f32"
            )]
            let normalized = normalized as f32;
            sample.pressure = owned::Pressure::try_new(normalized).ok();
        }
        if let winit::event::Force::Calibrated {
            altitude_angle: Some(altitude),
            ..
        } = force
        {
            sample.orientation = owned::PenOrientation::try_altitude(altitude).ok();
        }
    }
    sample
}

fn next_identity(counter: &mut Option<u64>) -> Option<u64> {
    let value = (*counter)?;
    *counter = value.checked_add(1);
    Some(value)
}

type DeviceWindow = (crate::traits::WindowId, winit::event::DeviceId);

enum NativeKind {
    Unreported,
    Reported(owned::PointerKind),
    Mixed,
}

impl NativeKind {
    fn observe(&mut self, kind: owned::PointerKind) {
        if kind == owned::PointerKind::Unknown {
            return;
        }
        match self {
            Self::Unreported => *self = Self::Reported(kind),
            Self::Reported(previous) if *previous != kind => *self = Self::Mixed,
            _ => {}
        }
    }

    fn reported(&self) -> owned::PointerKind {
        match self {
            Self::Reported(kind) => *kind,
            Self::Unreported | Self::Mixed => owned::PointerKind::Unknown,
        }
    }
}

pub(super) struct NativePointerState {
    devices: HashMap<winit::event::DeviceId, (owned::DeviceId, NativeKind)>,
    mice: HashMap<DeviceWindow, MouseContact>,
    contacts: HashMap<(crate::traits::WindowId, winit::event::DeviceId, u64), owned::PointerInfo>,
    gestures: HashMap<DeviceWindow, PanZoomState>,
    next_device: Option<u64>,
    next_pointer: Option<u64>,
}

impl Default for NativePointerState {
    fn default() -> Self {
        Self {
            devices: HashMap::new(),
            mice: HashMap::new(),
            contacts: HashMap::new(),
            gestures: HashMap::new(),
            next_device: Some(1),
            next_pointer: Some(1),
        }
    }
}

impl NativePointerState {
    fn device(
        &mut self,
        native: winit::event::DeviceId,
        kind: owned::PointerKind,
    ) -> Option<owned::DeviceId> {
        if let Some((id, observed)) = self.devices.get_mut(&native) {
            observed.observe(kind);
            return Some(*id);
        }
        let id = owned::DeviceId::try_from(next_identity(&mut self.next_device)?).ok()?;
        let mut observed = NativeKind::Unreported;
        observed.observe(kind);
        self.devices.insert(native, (id, observed));
        Some(id)
    }

    fn pointer(
        &mut self,
        native: winit::event::DeviceId,
        kind: owned::PointerKind,
        role: owned::PointerRole,
    ) -> Option<owned::PointerInfo> {
        let device = self.device(native, kind)?;
        let id = owned::PointerId::try_from(next_identity(&mut self.next_pointer)?).ok()?;
        Some(
            owned::PointerInfo::new(id, kind)
                .with_device(device)
                .with_role(role),
        )
    }

    fn mouse(&mut self, key: DeviceWindow) -> Option<&mut MouseContact> {
        if !self.mice.contains_key(&key) {
            let pointer = self.pointer(
                key.1,
                owned::PointerKind::Mouse,
                owned::PointerRole::Primary,
            )?;
            self.mice.insert(
                key,
                MouseContact {
                    pointer,
                    position: None,
                    held: HashSet::new(),
                    suppressed: HashSet::new(),
                    scroll_started: false,
                },
            );
        }
        self.mice.get_mut(&key)
    }

    pub(super) fn window_event(
        &mut self,
        window: crate::traits::WindowId,
        event: &winit::event::WindowEvent,
        scale: f64,
        modifiers: KeyboardModifiers,
    ) -> Option<Vec<PlatformInput>> {
        use winit::event::WindowEvent;
        if let Some((device, delta, phase)) = GestureDelta::from_native(event, scale) {
            return self.gesture((window, device), delta, phase, modifiers);
        }
        let mods = crate::shared::keyboard_adapter::modifiers(modifiers);
        match event {
            WindowEvent::CursorMoved { device_id, .. }
            | WindowEvent::TouchpadPressure { device_id, .. }
            | WindowEvent::CursorEntered { device_id }
            | WindowEvent::CursorLeft { device_id } => {
                Some(self.mouse((window, *device_id))?.event(event, scale, mods))
            }
            WindowEvent::MouseInput {
                device_id,
                state,
                button,
            } => self.mouse_button(window, device_id, state, button, mods),
            WindowEvent::Touch(touch) => self.touch(window, touch, scale, mods),
            WindowEvent::MouseWheel {
                device_id,
                delta,
                phase,
            } => self.scroll(window, device_id, delta, phase, scale, mods),
            _ => None,
        }
    }

    fn mouse_button(
        &mut self,
        window: crate::traits::WindowId,
        device_id: &winit::event::DeviceId,
        state: &ElementState,
        button: &MouseButton,
        mods: flui_platform_api::keyboard::Modifiers,
    ) -> Option<Vec<PlatformInput>> {
        let mut inputs = Vec::new();
        let mouse = self.mouse((window, *device_id))?;
        let before = mouse.held.is_empty();
        match state {
            ElementState::Pressed if mouse.position.is_none() => {
                mouse.suppressed.insert(*button);
                return Some(inputs);
            }
            ElementState::Pressed => {
                if !mouse.held.insert(*button) {
                    return Some(inputs);
                }
            }
            ElementState::Released => {
                if mouse.suppressed.remove(button) || !mouse.held.remove(button) {
                    return Some(inputs);
                }
            }
        }
        let Some(position) = mouse.position else {
            mouse.held.clear();
            inputs.push(PlatformInput::Pointer(owned::PointerEvent::Cancel(
                owned::PointerCancel::new(
                    mouse.pointer,
                    EventTime::from_nanos(event_timestamp_ns()),
                    owned::CancelReason::InvalidInput,
                ),
            )));
            return Some(inputs);
        };
        let button = native_button(*button);
        let held = mouse.buttons();
        let sample = native_sample(position);
        let event = if *state == ElementState::Pressed {
            let press =
                owned::PointerPress::new(mouse.pointer, button, held, sample).with_modifiers(mods);
            if before {
                owned::PointerEvent::Down(press)
            } else {
                owned::PointerEvent::ButtonChange(owned::ButtonChange::Pressed(press))
            }
        } else {
            let release = owned::PointerRelease::new(mouse.pointer, button, held, sample)
                .with_modifiers(mods);
            if mouse.held.is_empty() {
                owned::PointerEvent::Up(release)
            } else {
                owned::PointerEvent::ButtonChange(owned::ButtonChange::Released(release))
            }
        };
        inputs.push(PlatformInput::Pointer(event));
        Some(inputs)
    }

    fn touch(
        &mut self,
        window: crate::traits::WindowId,
        touch: &winit::event::Touch,
        scale: f64,
        mods: flui_platform_api::keyboard::Modifiers,
    ) -> Option<Vec<PlatformInput>> {
        use winit::event::TouchPhase;
        let mut inputs = Vec::new();
        let key = (window, touch.device_id, touch.id);
        let position = native_position(touch.location, scale);
        let pointer = if touch.phase == TouchPhase::Started {
            if self.contacts.contains_key(&key) || position.is_none() {
                return Some(inputs);
            }
            let role = if self
                .contacts
                .keys()
                .any(|(w, device, _)| *w == window && *device == touch.device_id)
            {
                owned::PointerRole::Additional
            } else {
                owned::PointerRole::Primary
            };
            let pointer = self.pointer(touch.device_id, owned::PointerKind::Touch, role)?;
            self.contacts.insert(key, pointer);
            pointer
        } else {
            let Some(pointer) = self.contacts.get(&key).copied() else {
                return Some(inputs);
            };
            pointer
        };
        if matches!(touch.phase, TouchPhase::Ended | TouchPhase::Cancelled) {
            self.contacts.remove(&key);
        }
        if touch.phase == TouchPhase::Cancelled
            || (touch.phase == TouchPhase::Ended && position.is_none())
        {
            inputs.push(PlatformInput::Pointer(owned::PointerEvent::Cancel(
                owned::PointerCancel::new(
                    pointer,
                    EventTime::from_nanos(event_timestamp_ns()),
                    if touch.phase == TouchPhase::Cancelled {
                        owned::CancelReason::Platform
                    } else {
                        owned::CancelReason::InvalidInput
                    },
                ),
            )));
        } else if let Some(position) = position {
            let sample = native_touch_sample(position, touch.force);
            let event = match touch.phase {
                TouchPhase::Started => owned::PointerEvent::Down(
                    owned::PointerPress::new(
                        pointer,
                        owned::PointerButton::PRIMARY,
                        owned::PointerButtons::only(owned::PointerButton::PRIMARY),
                        sample,
                    )
                    .with_modifiers(mods),
                ),
                TouchPhase::Moved => owned::PointerEvent::Move(
                    owned::PointerMove::new(
                        pointer,
                        owned::PointerButtons::only(owned::PointerButton::PRIMARY),
                        sample,
                    )
                    .with_modifiers(mods),
                ),
                TouchPhase::Ended => owned::PointerEvent::Up(
                    owned::PointerRelease::new(
                        pointer,
                        owned::PointerButton::PRIMARY,
                        owned::PointerButtons::NONE,
                        sample,
                    )
                    .with_modifiers(mods),
                ),
                TouchPhase::Cancelled => {
                    unreachable!("BUG: cancellation returned before sampling")
                }
            };
            inputs.push(PlatformInput::Pointer(event));
        }
        Some(inputs)
    }

    fn scroll(
        &mut self,
        window: crate::traits::WindowId,
        device_id: &winit::event::DeviceId,
        delta: &MouseScrollDelta,
        phase: &winit::event::TouchPhase,
        scale: f64,
        mods: flui_platform_api::keyboard::Modifiers,
    ) -> Option<Vec<PlatformInput>> {
        use winit::event::TouchPhase;
        let mut inputs = Vec::new();
        let mouse = self.mouse((window, *device_id))?;
        let Some(position) = mouse.position else {
            return Some(inputs);
        };
        let scale = if scale.is_finite() && scale > 0.0 {
            scale
        } else {
            f64::NAN
        };
        let (unit, x, y, precision) = match delta {
            MouseScrollDelta::LineDelta(x, y) => (
                owned::ScrollUnit::Lines,
                -f64::from(*x),
                -f64::from(*y),
                owned::ScrollPrecision::Notched,
            ),
            MouseScrollDelta::PixelDelta(delta) => (
                owned::ScrollUnit::Pixels,
                -delta.x / scale,
                -delta.y / scale,
                owned::ScrollPrecision::Precise,
            ),
        };
        let delta = owned::ScrollDelta::try_new(unit, x, y)
            .unwrap_or_else(|_| owned::ScrollDelta::zero(unit));
        let native_phase = match phase {
            TouchPhase::Started => {
                mouse.scroll_started = true;
                Some(owned::ScrollPhase::Began)
            }
            TouchPhase::Moved if mouse.scroll_started || unit == owned::ScrollUnit::Pixels => {
                Some(owned::ScrollPhase::Changed)
            }
            TouchPhase::Moved => None,
            TouchPhase::Ended => {
                mouse.scroll_started = false;
                Some(owned::ScrollPhase::Ended)
            }
            TouchPhase::Cancelled => {
                mouse.scroll_started = false;
                Some(owned::ScrollPhase::Cancelled)
            }
        };
        let mut scroll = owned::ScrollEvent::new(
            mouse.pointer,
            EventTime::from_nanos(event_timestamp_ns()),
            position,
            delta,
        )
        .with_modifiers(mods)
        .with_precision(precision);
        scroll.phase = native_phase;
        inputs.push(PlatformInput::Pointer(owned::PointerEvent::Scroll(scroll)));
        Some(inputs)
    }

    fn gesture(
        &mut self,
        key: DeviceWindow,
        delta: GestureDelta,
        phase: winit::event::TouchPhase,
        modifiers: KeyboardModifiers,
    ) -> Option<Vec<PlatformInput>> {
        let position = self
            .mice
            .get(&key)
            .and_then(|mouse| mouse.position)
            .or_else(|| {
                self.gestures
                    .get(&key)
                    .and_then(|gesture| gesture.contact.as_ref())
                    .map(|contact| contact.position)
            })?;
        let pointer = if let Some(contact) = self
            .gestures
            .get(&key)
            .and_then(|gesture| gesture.contact.as_ref())
        {
            contact.pointer
        } else {
            if phase != winit::event::TouchPhase::Started {
                return Some(Vec::new());
            }
            self.pointer(
                key.1,
                owned::PointerKind::Trackpad,
                owned::PointerRole::Primary,
            )?
        };
        Some(
            self.gestures
                .entry(key)
                .or_default()
                .event(delta, phase, pointer, position, modifiers),
        )
    }

    pub(super) fn cancel_window(
        &mut self,
        window: crate::traits::WindowId,
        reason: owned::CancelReason,
    ) -> Vec<PlatformInput> {
        let mut events = Vec::new();
        let time = EventTime::from_nanos(event_timestamp_ns());
        for (_, pointer) in self.contacts.extract_if(|(w, _, _), _| *w == window) {
            events.push(PlatformInput::Pointer(owned::PointerEvent::Cancel(
                owned::PointerCancel::new(pointer, time, reason),
            )));
        }
        for (_, mouse) in self.mice.extract_if(|(w, _), _| *w == window) {
            if !mouse.held.is_empty() {
                events.push(PlatformInput::Pointer(owned::PointerEvent::Cancel(
                    owned::PointerCancel::new(mouse.pointer, time, reason),
                )));
            }
        }
        for (_, mut gesture) in self.gestures.extract_if(|(w, _), _| *w == window) {
            events.extend(gesture.cancel(KeyboardModifiers::empty()));
        }
        events.sort_by_key(|input| match input {
            PlatformInput::Pointer(owned::PointerEvent::Cancel(cancel)) => {
                cancel.pointer.id.get().get()
            }
            PlatformInput::Pointer(owned::PointerEvent::PanZoom(gesture)) => {
                gesture.pointer().id.get().get()
            }
            _ => 0,
        });
        events
    }

    pub(super) fn device_event(
        &mut self,
        native: winit::event::DeviceId,
        event: &winit::event::DeviceEvent,
        windows: &[crate::traits::WindowId],
    ) -> Vec<(crate::traits::WindowId, PlatformInput)> {
        use winit::event::DeviceEvent;
        if !matches!(event, DeviceEvent::Added | DeviceEvent::Removed) {
            return Vec::new();
        }
        let Some(device) = self.device(native, owned::PointerKind::Unknown) else {
            return Vec::new();
        };
        let mut events = Vec::new();
        let time = EventTime::from_nanos(event_timestamp_ns());
        let kind = self
            .devices
            .get(&native)
            .map_or(owned::PointerKind::Unknown, |(_, kind)| kind.reported());
        if matches!(event, DeviceEvent::Removed) {
            self.devices.remove(&native);
            for ((window, _, _), pointer) in self
                .contacts
                .extract_if(|(_, native_id, _), _| *native_id == native)
            {
                events.push((
                    window,
                    PlatformInput::Pointer(owned::PointerEvent::Cancel(owned::PointerCancel::new(
                        pointer,
                        time,
                        owned::CancelReason::DeviceRemoved,
                    ))),
                ));
            }
            for ((window, _), mouse) in self
                .mice
                .extract_if(|(_, native_id), _| *native_id == native)
            {
                if !mouse.held.is_empty() {
                    events.push((
                        window,
                        PlatformInput::Pointer(owned::PointerEvent::Cancel(
                            owned::PointerCancel::new(
                                mouse.pointer,
                                time,
                                owned::CancelReason::DeviceRemoved,
                            ),
                        )),
                    ));
                }
            }
            for ((window, _), mut gesture) in self
                .gestures
                .extract_if(|(_, native_id), _| *native_id == native)
            {
                if let Some(input) = gesture.cancel(KeyboardModifiers::empty()) {
                    events.push((window, input));
                }
            }
        }
        events.sort_by_key(|(window, input)| {
            (
                window.0,
                match input {
                    PlatformInput::Pointer(owned::PointerEvent::Cancel(cancel)) => {
                        cancel.pointer.id.get().get()
                    }
                    PlatformInput::Pointer(owned::PointerEvent::PanZoom(gesture)) => {
                        gesture.pointer().id.get().get()
                    }
                    _ => 0,
                },
            )
        });
        for window in windows {
            let change = owned::PointerDeviceChange::new(
                device,
                if matches!(event, DeviceEvent::Added) {
                    owned::PointerKind::Unknown
                } else {
                    kind
                },
                time,
            );
            let event = if matches!(event, DeviceEvent::Added) {
                owned::PointerEvent::DeviceAdded(change)
            } else {
                owned::PointerEvent::DeviceRemoved(change)
            };
            events.push((*window, PlatformInput::Pointer(event)));
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
    use flui_platform_api::pointer::PointerEvent;

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
                .window_event(
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
            .window_event(
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
        let native = winit::event::DeviceId::dummy();
        let window = crate::traits::WindowId(1);
        let mut state = NativePointerState {
            next_device: Some(u64::MAX),
            next_pointer: Some(u64::MAX),
            ..NativePointerState::default()
        };
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
            .window_event(
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
            down.pointer.device.expect("actual device").get().get(),
            u64::MAX
        );
        let terminal = state
            .window_event(
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
                    .window_event(
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
            state.window_event(
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
                    .window_event(
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
        state.window_event(
            window,
            &WindowEvent::CursorMoved {
                device_id: native,
                position: winit::dpi::PhysicalPosition::new(1.0, 2.0),
            },
            1.0,
            KeyboardModifiers::empty(),
        );
        let input = state
            .window_event(
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
        use flui_foundation::geometry::Point;
        use flui_platform_api::pointer::{
            PanZoomPhase, PointerId, PointerInfo, PointerKind, PointerPosition,
        };
        use winit::event::TouchPhase;

        let info = |id| {
            PointerInfo::new(
                PointerId::try_from(id).expect("nonzero test id"),
                PointerKind::Trackpad,
            )
        };
        let at = PointerPosition::try_new(Point::new(10.0, 15.0)).expect("finite position");
        for competing in [false, true] {
            let mut stream = PanZoomState::default();
            let mut events = Vec::new();
            let mut push = |stream: &mut PanZoomState, delta, phase, id| {
                events.extend(stream.event(delta, phase, info(id), at, KeyboardModifiers::empty()));
            };
            push(
                &mut stream,
                GestureDelta::Pinch(0.0),
                TouchPhase::Moved,
                3_u64,
            );
            push(
                &mut stream,
                GestureDelta::Pinch(0.0),
                TouchPhase::Started,
                3,
            );
            push(
                &mut stream,
                GestureDelta::Pinch(0.0),
                TouchPhase::Started,
                99,
            );
            push(&mut stream, GestureDelta::Pinch(0.5), TouchPhase::Moved, 99);
            if competing {
                push(
                    &mut stream,
                    GestureDelta::Rotation(0.0),
                    TouchPhase::Started,
                    99,
                );
                push(
                    &mut stream,
                    GestureDelta::Rotation(90.0),
                    TouchPhase::Moved,
                    99,
                );
            }
            push(
                &mut stream,
                GestureDelta::Pinch(f64::NAN),
                TouchPhase::Moved,
                99,
            );
            push(&mut stream, GestureDelta::Pinch(0.0), TouchPhase::Ended, 99);
            if competing {
                push(
                    &mut stream,
                    GestureDelta::Rotation(0.0),
                    TouchPhase::Cancelled,
                    99,
                );
            }
            push(
                &mut stream,
                GestureDelta::Pan(Offset::new(0.0, 0.0)),
                TouchPhase::Started,
                4,
            );
            push(
                &mut stream,
                GestureDelta::Pan(Offset::new(4.0, -2.0)),
                TouchPhase::Moved,
                4,
            );
            push(
                &mut stream,
                GestureDelta::Pan(Offset::new(f64::NAN, 0.0)),
                TouchPhase::Ended,
                4,
            );
            let events: Vec<_> = events
                .into_iter()
                .map(|event| match event {
                    PlatformInput::Pointer(PointerEvent::PanZoom(event)) => event,
                    _ => panic!("only PanZoom expected"),
                })
                .collect();
            assert_eq!(
                events.len(),
                if competing { 7 } else { 6 },
                "duplicate/orphan/invalid phases produce no extra messages"
            );
            assert!(matches!(events[0].phase, PanZoomPhase::Start));
            assert_eq!(events[0].pointer().id, info(3_u64).id);
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
            assert!(
                events[..=terminal]
                    .iter()
                    .all(|event| event.pointer().id == info(3_u64).id)
            );
            assert!(matches!(events[terminal + 1].phase, PanZoomPhase::Start));
            let PanZoomPhase::Update(pan) = events[terminal + 2].phase else {
                panic!("pan update")
            };
            assert_eq!(pan.pan(), flui_foundation::geometry::Offset::new(4.0, -2.0));
            assert_eq!(pan.scale(), 1.0, "new stream resets cumulative zoom");
            assert!(
                matches!(events[terminal + 3].phase, PanZoomPhase::End),
                "invalid final delta preserves native End"
            );
            assert!(
                events[terminal + 1..]
                    .iter()
                    .all(|event| event.pointer().id == info(4_u64).id)
            );
        }
    }

    fn native_pan_zoom_converter_keeps_terminal_after_invalid_position() {
        use winit::event::{TouchPhase, WindowEvent};
        let native = winit::event::DeviceId::dummy();
        let window = crate::traits::WindowId(1);
        let mut state = NativePointerState::default();
        state.window_event(
            window,
            &WindowEvent::CursorMoved {
                device_id: native,
                position: winit::dpi::PhysicalPosition::new(20.0, 30.0),
            },
            2.0,
            KeyboardModifiers::empty(),
        );
        let first = state
            .window_event(
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
        state.window_event(
            window,
            &WindowEvent::CursorMoved {
                device_id: native,
                position: winit::dpi::PhysicalPosition::new(f64::NAN, 30.0),
            },
            2.0,
            KeyboardModifiers::empty(),
        );
        let terminal = state
            .window_event(
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
                .window_event(
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
                .window_event(
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
                down.sample.pressure.map(|pressure| pressure.get()),
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
            .window_event(window, &moved, 1.0, KeyboardModifiers::empty())
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
            .window_event(window, &moved, 1.0, KeyboardModifiers::empty())
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
            .window_event(window, &pressed, 1.0, KeyboardModifiers::empty())
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
