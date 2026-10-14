//! Native Android input translated directly into FLUI's owned vocabulary.
//!
//! Coordinates and contact axes are physical pixels on this wire; both are
//! divided by the window scale once. Sensor presence comes from the device's
//! motion ranges, not from whether an axis happens to read zero.

use std::{collections::HashMap, sync::Arc};

use android_activity::input::{Axis, Button, ButtonState, KeyAction, MotionAction, ToolType};
use flui_platform_api::{
    EventTime,
    keyboard::Modifiers as OwnedModifiers,
    pointer::{PenTool, PointerButton, PointerButtons, PointerEvent, PointerKind, ScrollEvent},
};
use keyboard_types::{Key, KeyState, Modifiers, NamedKey};
use ui_events::keyboard::KeyboardEvent;

use crate::{
    shared::keyboard_adapter::keyboard_input,
    shared::pointer_assembly::{
        ContactRegistry, DecodeError, MoveBatch, NativeCancel, NativeClock, NativeContact,
        NativeDevice, NativeReading, NativeSurface, PenAngles, RawSensors, ReadingAction,
        ReadingTime, decode,
    },
    traits::PlatformInput,
};

#[derive(Clone, Copy)]
pub(crate) struct DeviceCapabilities {
    source: u32,
    axes: u64,
}

impl DeviceCapabilities {
    fn has(self, axis: Axis) -> bool {
        let bit = u32::from(axis);
        bit < 64 && self.axes & (1_u64 << bit) != 0
    }
}

#[derive(Clone)]
pub(crate) struct CachedDevice {
    capabilities: DeviceCapabilities,
    // The cache and a query snapshot share this global reference. Platform is
    // Send + Sync, so Rc cannot represent that ownership here.
    object: Arc<jni::refs::Global<jni::objects::JObject<'static>>>,
}

pub(crate) enum DeviceReading {
    Present {
        cached: CachedDevice,
        replaced: bool,
    },
    Removed,
    Unavailable,
}

/// Query Java's capability metadata before entering the owner-local state guard.
/// InputManager replaces its cached Java object when a device is reconfigured.
/// JNI object identity avoids descriptors (which can be duplicated) and hidden APIs.
pub(crate) fn motion_device(
    app: &android_activity::AndroidApp,
    event: &android_activity::input::MotionEvent<'_>,
    cached: Option<CachedDevice>,
) -> DeviceReading {
    use jni::{JValue, JavaVM, jni_sig, jni_str};

    // SAFETY: AndroidApp owns the running Android VM for its entire lifetime.
    // JavaVM is a borrowed VM handle; constructing it does not destroy the VM.
    #[expect(
        unsafe_code,
        reason = "AndroidApp supplies the live VM handle required by JNI"
    )]
    let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) };
    let result = vm.attach_current_thread(|env| -> jni::errors::Result<_> {
        let result = (|| {
            let device = env
                .call_static_method(
                    jni_str!("android/view/InputDevice"),
                    jni_str!("getDevice"),
                    jni_sig!("(I)Landroid/view/InputDevice;"),
                    &[JValue::Int(event.device_id())],
                )?
                .l()?;
            if device.is_null() {
                return Ok(DeviceReading::Removed);
            }
            let source = u32::from(event.source());
            let replaced = match cached.as_ref() {
                Some(cached) => !env.is_same_object(&device, cached.object.as_ref())?,
                None => false,
            };
            if let Some(cached) = cached.as_ref()
                && !replaced
                && cached.capabilities.source == source
            {
                return Ok(DeviceReading::Present {
                    cached: cached.clone(),
                    replaced,
                });
            }
            let mut axes = 0_u64;
            for axis in [
                Axis::Pressure,
                Axis::TouchMajor,
                Axis::TouchMinor,
                Axis::Tilt,
                Axis::Orientation,
            ] {
                let range = env
                    .call_method(
                        &device,
                        jni_str!("getMotionRange"),
                        jni_sig!("(II)Landroid/view/InputDevice$MotionRange;"),
                        &[
                            JValue::Int(
                                i32::try_from(u32::from(axis))
                                    .expect("BUG: Android sensor axes fit i32"),
                            ),
                            JValue::Int(i32::from_ne_bytes(source.to_ne_bytes())),
                        ],
                    )?
                    .l()?;
                if !range.is_null() {
                    axes |= 1_u64 << u32::from(axis);
                }
            }
            Ok(DeviceReading::Present {
                cached: CachedDevice {
                    capabilities: DeviceCapabilities { source, axes },
                    object: Arc::new(env.new_global_ref(&device)?),
                },
                replaced,
            })
        })();
        if result.is_err() {
            // Capability discovery is optional. Do not leave an exception pending
            // on the event-loop thread or manufacture readings after a failure.
            env.exception_clear();
        }
        result
    });
    result.unwrap_or(DeviceReading::Unavailable)
}

/// The Android activity's one input surface.
const SURFACE: NativeSurface = NativeSurface(0);

/// The label of a native device id, preserving its sign bits injectively.
fn native_device(id: i32) -> NativeDevice {
    NativeDevice(u64::from(u32::from_ne_bytes(id.to_ne_bytes())))
}

/// Input state for the Android window owner. Java device references are used
/// only for capability caching; no user callback lives here. Contact and
/// device identity come from the shared pointer assembly.
pub(crate) struct AndroidInputState {
    contacts: ContactRegistry,
    devices: HashMap<i32, CachedDevice>,
    batch: MoveBatch,
    last_time: Option<EventTime>,
}

impl Default for AndroidInputState {
    fn default() -> Self {
        Self::with_registry(ContactRegistry::new(NativeClock::epoch_only()))
    }
}

impl AndroidInputState {
    fn with_registry(contacts: ContactRegistry) -> Self {
        Self {
            contacts,
            devices: HashMap::new(),
            batch: MoveBatch::new(),
            last_time: None,
        }
    }

    pub(crate) fn cancel_contacts(&mut self, reason: NativeCancel) -> Vec<PlatformInput> {
        let Some(time) = self.last_time else {
            return Vec::new();
        };
        self.contacts.cancel_surface(SURFACE, time, reason)
    }

    pub(crate) fn capabilities(&self, id: i32) -> Option<CachedDevice> {
        self.devices.get(&id).cloned()
    }

    /// Track the device behind an event from its Java discovery. `None` when
    /// the event cannot be delivered (a removed device, or no device identity
    /// left); otherwise the device label its readings carry and its
    /// capabilities, both absent for a virtual device without an
    /// `InputDevice`.
    fn discover(
        &mut self,
        id: i32,
        discovery: DeviceReading,
        time: EventTime,
        output: &mut Vec<PlatformInput>,
    ) -> Option<(Option<NativeDevice>, Option<DeviceCapabilities>)> {
        let native = native_device(id);
        let had_device = self.devices.contains_key(&id);
        let replacement = match &discovery {
            DeviceReading::Present { replaced, .. } => *replaced,
            DeviceReading::Removed => true,
            DeviceReading::Unavailable => false,
        };
        if replacement && self.devices.remove(&id).is_some() {
            let (ended, removed) = self.contacts.remove_device(native, time);
            output.extend(ended.into_iter().map(|(_, input)| input));
            output.extend(
                removed.map(|change| PlatformInput::Pointer(PointerEvent::DeviceRemoved(change))),
            );
        }
        match discovery {
            DeviceReading::Present { cached, .. } => {
                let capabilities = cached.capabilities;
                if self.devices.insert(id, cached).is_none() {
                    // The registry refuses permanently after its final identity.
                    let added = self.contacts.add_device(native, PointerKind::Unknown, time);
                    if added.is_none() && self.contacts.devices().id(native).is_none() {
                        self.devices.remove(&id);
                        return None;
                    }
                    output.extend(
                        added.map(|change| {
                            PlatformInput::Pointer(PointerEvent::DeviceAdded(change))
                        }),
                    );
                }
                Some((Some(native), Some(capabilities)))
            }
            DeviceReading::Removed if had_device => None,
            // Android-generated/virtual events can have no InputDevice object.
            // Preserve their native contact and position, with absent device/sensors.
            DeviceReading::Removed | DeviceReading::Unavailable => Some((None, None)),
        }
    }

    pub(crate) fn convert_motion_event(
        &mut self,
        event: &android_activity::input::MotionEvent<'_>,
        scale_factor: f64,
        scroll_policy: crate::shared::android_scroll::AxisPolicy,
        discovery: DeviceReading,
    ) -> Vec<PlatformInput> {
        let time = u64::try_from(event.event_time())
            .ok()
            .map(EventTime::from_nanos);
        if let Some(time) = time {
            self.last_time = Some(time);
        }
        let Some(delivery_time) = time.or(self.last_time) else {
            return Vec::new();
        };
        let mut output = Vec::new();
        let Some((device, capabilities)) =
            self.discover(event.device_id(), discovery, delivery_time, &mut output)
        else {
            return output;
        };
        let modifiers = owned_modifiers(event.meta_state());
        let action = event.action();
        let held = event.button_state();
        for (index, pointer) in event.pointers().enumerate() {
            if matches!(
                action,
                MotionAction::Down
                    | MotionAction::PointerDown
                    | MotionAction::Up
                    | MotionAction::PointerUp
            ) && index != event.pointer_index()
            {
                continue;
            }
            let Ok(label) = u64::try_from(pointer.pointer_id()) else {
                continue;
            };
            let contact = NativeContact(label);
            let kind = pointer_kind(pointer.tool_type());
            let in_contact = match action {
                MotionAction::Down | MotionAction::PointerDown | MotionAction::Move => true,
                MotionAction::ButtonPress | MotionAction::ButtonRelease => self
                    .contacts
                    .buttons(SURFACE, device, contact)
                    .is_some_and(|buttons| buttons.contains(PointerButton::PRIMARY)),
                _ => false,
            } && matches!(kind, PointerKind::Touch | PointerKind::Pen { .. });
            let buttons = native_buttons(held, in_contact);
            let reading = |action, time: EventTime, axis: &dyn Fn(Axis) -> f32| {
                let mut reading =
                    NativeReading::new(contact, kind, action, ReadingTime::epoch(time))
                        .on(SURFACE)
                        .at(
                            f64::from(axis(Axis::X)),
                            f64::from(axis(Axis::Y)),
                            scale_factor,
                        )
                        .with_buttons(buttons)
                        .with_modifiers(modifiers)
                        .with_sensors(native_sensors(kind, capabilities, axis));
                if let Some(device) = device {
                    reading = reading.from_device(device);
                }
                reading
            };
            let current = |action| {
                reading(action, time.unwrap_or(delivery_time), &|axis| {
                    pointer.axis_value(axis)
                })
            };
            let assembled = match action {
                MotionAction::Cancel => decode(
                    &mut self.contacts,
                    &current(ReadingAction::Cancel(NativeCancel::Platform)),
                ),
                MotionAction::Down | MotionAction::PointerDown => {
                    let button = if matches!(kind, PointerKind::Mouse) {
                        buttons.iter().next().unwrap_or(PointerButton::PRIMARY)
                    } else {
                        PointerButton::PRIMARY
                    };
                    decode(&mut self.contacts, &current(ReadingAction::Press(button)))
                }
                MotionAction::Up | MotionAction::PointerUp => {
                    let button = if matches!(kind, PointerKind::Mouse) {
                        self.contacts
                            .buttons(SURFACE, device, contact)
                            .and_then(|held| held.iter().find(|button| !buttons.contains(*button)))
                            .unwrap_or(PointerButton::PRIMARY)
                    } else {
                        PointerButton::PRIMARY
                    };
                    decode(&mut self.contacts, &current(ReadingAction::Release(button)))
                }
                MotionAction::Move | MotionAction::HoverMove => {
                    for past in pointer.history() {
                        let Some(time) = u64::try_from(past.event_time())
                            .ok()
                            .map(EventTime::from_nanos)
                        else {
                            continue;
                        };
                        self.batch
                            .push_coalesced(reading(ReadingAction::Move, time, &|axis| {
                                past.axis_value(axis)
                            }));
                    }
                    self.batch
                        .decode(&mut self.contacts, &current(ReadingAction::Move))
                }
                MotionAction::ButtonPress | MotionAction::ButtonRelease => {
                    let Some(button) = native_button(event.action_button()) else {
                        continue;
                    };
                    decode(
                        &mut self.contacts,
                        &current(if matches!(action, MotionAction::ButtonPress) {
                            ReadingAction::Press(button)
                        } else {
                            ReadingAction::Release(button)
                        }),
                    )
                }
                MotionAction::HoverEnter => {
                    decode(&mut self.contacts, &current(ReadingAction::Enter))
                }
                MotionAction::HoverExit => {
                    decode(&mut self.contacts, &current(ReadingAction::Leave))
                }
                MotionAction::Scroll => {
                    // A wheel reports from where the cursor hovers: place it first.
                    let mut placed = decode(&mut self.contacts, &current(ReadingAction::Move))
                        .unwrap_or_else(DecodeError::into_inputs);
                    // Android axes are positive left/up, opposite the owned wheel convention.
                    // The accepted activity-context policy already projects
                    // native factors into logical pixels exactly once.
                    if let (Some(info), Some((position, _)), Some(delta)) = (
                        self.contacts.pointer(SURFACE, device, contact),
                        self.contacts.position(SURFACE, device, contact),
                        scroll_policy.delta(
                            pointer.axis_value(Axis::Hscroll),
                            pointer.axis_value(Axis::Vscroll),
                        ),
                    ) {
                        placed.push(PlatformInput::Pointer(PointerEvent::Scroll(
                            ScrollEvent::new(info, time.unwrap_or(delivery_time), position, delta)
                                .with_modifiers(modifiers),
                        )));
                    }
                    Ok(placed)
                }
                _ => continue,
            };
            output.extend(assembled.unwrap_or_else(DecodeError::into_inputs));
        }
        output
    }
}

fn pointer_kind(tool: ToolType) -> PointerKind {
    match tool {
        ToolType::Finger => PointerKind::Touch,
        ToolType::Stylus => PointerKind::Pen { tool: PenTool::Tip },
        ToolType::Eraser => PointerKind::Pen {
            tool: PenTool::Eraser,
        },
        ToolType::Mouse => PointerKind::Mouse,
        _ => PointerKind::Unknown,
    }
}

fn native_button(button: Button) -> Option<PointerButton> {
    match button {
        Button::Primary => Some(PointerButton::PRIMARY),
        Button::Secondary | Button::StylusPrimary => Some(PointerButton::SECONDARY),
        Button::Tertiary | Button::StylusSecondary => Some(PointerButton::AUXILIARY),
        Button::Back => Some(PointerButton::BACK),
        Button::Forward => Some(PointerButton::FORWARD),
        _ => None,
    }
}

fn native_buttons(native: ButtonState, contact: bool) -> PointerButtons {
    let mut buttons: PointerButtons = [
        Button::Primary,
        Button::Secondary,
        Button::Tertiary,
        Button::Back,
        Button::Forward,
        Button::StylusPrimary,
        Button::StylusSecondary,
    ]
    .into_iter()
    .filter(|button| native.0 & u32::from(*button) != 0)
    .filter_map(native_button)
    .collect();
    if contact {
        buttons = buttons.with(PointerButton::PRIMARY);
    }
    buttons
}

/// Android measures tilt from the screen normal and orientation clockwise from
/// upwards; owned altitude is above the surface and azimuth from rightwards.
/// An angle outside its native range is not reported.
fn pen_angles(tilt: Option<f64>, orientation: Option<f64>) -> Option<PenAngles> {
    let altitude = tilt
        .filter(|tilt| (0.0..=f64::from(core::f32::consts::FRAC_PI_2)).contains(tilt))
        .map(|tilt| core::f64::consts::FRAC_PI_2 - tilt.min(core::f64::consts::FRAC_PI_2));
    let azimuth = orientation
        .filter(|orientation| orientation.is_finite())
        .map(|orientation| orientation - core::f64::consts::FRAC_PI_2);
    (altitude.is_some() || azimuth.is_some()).then_some(PenAngles::Spherical { altitude, azimuth })
}

/// The sensors a device's motion ranges say it reports, read from `axis`.
/// Contact axes stay in physical pixels; assembly divides them by the scale.
fn native_sensors(
    kind: PointerKind,
    capabilities: Option<DeviceCapabilities>,
    axis: &dyn Fn(Axis) -> f32,
) -> RawSensors {
    let mut sensors = RawSensors::default();
    let Some(capabilities) = capabilities else {
        return sensors;
    };
    if matches!(kind, PointerKind::Touch | PointerKind::Pen { .. }) {
        if capabilities.has(Axis::Pressure) {
            // Android pressure may overshoot one by calibration: clamp it.
            let pressure = f64::from(axis(Axis::Pressure));
            sensors.pressure = pressure.is_finite().then(|| pressure.clamp(0.0, 1.0));
        }
        if capabilities.has(Axis::TouchMajor) && capabilities.has(Axis::TouchMinor) {
            let (major, minor) = (
                f64::from(axis(Axis::TouchMajor)),
                f64::from(axis(Axis::TouchMinor)),
            );
            if major.is_finite() && minor.is_finite() && major >= 0.0 && minor >= 0.0 {
                sensors.contact_size = Some((major, minor));
            }
        }
    }
    if matches!(kind, PointerKind::Pen { .. }) {
        sensors.angles = pen_angles(
            capabilities
                .has(Axis::Tilt)
                .then(|| f64::from(axis(Axis::Tilt))),
            capabilities
                .has(Axis::Orientation)
                .then(|| f64::from(axis(Axis::Orientation))),
        );
    }
    sensors
}

fn owned_modifiers(meta: android_activity::input::MetaState) -> OwnedModifiers {
    let native = convert_meta_state(meta);
    let mut owned = OwnedModifiers::NONE;
    for (from, to) in [
        (Modifiers::SHIFT, OwnedModifiers::SHIFT),
        (Modifiers::ALT, OwnedModifiers::ALT),
        (Modifiers::CONTROL, OwnedModifiers::CONTROL),
        (Modifiers::META, OwnedModifiers::META),
        (Modifiers::CAPS_LOCK, OwnedModifiers::CAPS_LOCK),
        (Modifiers::NUM_LOCK, OwnedModifiers::NUM_LOCK),
    ] {
        if native.contains(from) {
            owned |= to;
        }
    }
    owned
}

/// Convert an Android `KeyEvent` to a `PlatformInput`.
pub fn convert_key_event(
    app: &android_activity::AndroidApp,
    event: &android_activity::input::KeyEvent<'_>,
) -> Option<PlatformInput> {
    let action = event.action();
    let state = match action {
        KeyAction::Up => KeyState::Up,
        // Multiple = auto-repeat; treat as Down
        KeyAction::Down | KeyAction::Multiple => KeyState::Down,
        _ => return None,
    };

    // Keycode implements Into<u32> via num_enum::IntoPrimitive
    let keycode_i32 = i32::try_from(u32::from(event.key_code())).ok()?;
    let code = ui_events::keyboard::android::keycode_to_code(keycode_i32);
    let named_key = ui_events::keyboard::android::keycode_to_named_key(keycode_i32);
    let location = ui_events::keyboard::android::keycode_to_location(keycode_i32);

    let mapped = if named_key == NamedKey::Unidentified {
        app.device_key_character_map(event.device_id())
            .and_then(|map| map.get(event.key_code(), event.meta_state()))
            .ok()
    } else {
        None
    };
    let modifiers = convert_meta_state(event.meta_state());
    let key = mapped_key(named_key, mapped, keycode_i32, modifiers);
    let repeat = event.repeat_count() > 0;

    Some(keyboard_input(
        KeyboardEvent {
            state,
            key,
            code,
            location,
            modifiers,
            repeat,
            is_composing: false,
        },
        u64::try_from(event.event_time()).ok()?,
    ))
}

fn mapped_key(
    named: NamedKey,
    mapped: Option<android_activity::input::KeyMapChar>,
    keycode: i32,
    modifiers: Modifiers,
) -> Key {
    use android_activity::input::KeyMapChar;
    if named != NamedKey::Unidentified {
        return Key::Named(named);
    }
    match mapped {
        Some(KeyMapChar::Unicode(character)) => Key::Character(character.to_string()),
        Some(KeyMapChar::CombiningAccent(_)) => Key::Named(NamedKey::Dead),
        Some(KeyMapChar::None) => Key::Named(NamedKey::Unidentified),
        None => fallback_character(keycode, modifiers)
            .map_or(Key::Named(NamedKey::Unidentified), |character| {
                Key::Character(character.to_string())
            }),
    }
}

/// Legacy US keycode fallback only when the native map lookup fails.
fn fallback_character(keycode: i32, modifiers: Modifiers) -> Option<char> {
    use ui_events::keyboard::android::{
        KEYCODE_0, KEYCODE_9, KEYCODE_A, KEYCODE_APOSTROPHE, KEYCODE_AT, KEYCODE_BACKSLASH,
        KEYCODE_COMMA, KEYCODE_EQUALS, KEYCODE_GRAVE, KEYCODE_LEFT_BRACKET, KEYCODE_MINUS,
        KEYCODE_PERIOD, KEYCODE_PLUS, KEYCODE_POUND, KEYCODE_RIGHT_BRACKET, KEYCODE_SEMICOLON,
        KEYCODE_SLASH, KEYCODE_SPACE, KEYCODE_STAR, KEYCODE_Z,
    };
    let shift = modifiers.contains(Modifiers::SHIFT);
    if (KEYCODE_A..=KEYCODE_Z).contains(&keycode) {
        let base = if shift ^ modifiers.contains(Modifiers::CAPS_LOCK) {
            b'A'
        } else {
            b'a'
        };
        return u8::try_from(keycode - KEYCODE_A)
            .ok()
            .map(|offset| char::from(base + offset));
    }
    if (KEYCODE_0..=KEYCODE_9).contains(&keycode) {
        let offset = usize::try_from(keycode - KEYCODE_0).ok()?;
        return Some(char::from(if shift {
            b")!@#$%^&*("[offset]
        } else {
            b"0123456789"[offset]
        }));
    }
    let (plain, shifted) = match keycode {
        KEYCODE_SPACE => (' ', ' '),
        KEYCODE_COMMA => (',', '<'),
        KEYCODE_PERIOD => ('.', '>'),
        KEYCODE_MINUS => ('-', '_'),
        KEYCODE_EQUALS => ('=', '+'),
        KEYCODE_LEFT_BRACKET => ('[', '{'),
        KEYCODE_RIGHT_BRACKET => (']', '}'),
        KEYCODE_BACKSLASH => ('\\', '|'),
        KEYCODE_SEMICOLON => (';', ':'),
        KEYCODE_APOSTROPHE => ('\'', '"'),
        KEYCODE_SLASH => ('/', '?'),
        KEYCODE_GRAVE => ('`', '~'),
        KEYCODE_AT => ('@', '@'),
        KEYCODE_PLUS => ('+', '+'),
        KEYCODE_POUND => ('#', '#'),
        KEYCODE_STAR => ('*', '*'),
        _ => return None,
    };
    Some(if shift { shifted } else { plain })
}

/// Convert Android `MetaState` to `keyboard_types::Modifiers`.
fn convert_meta_state(meta: android_activity::input::MetaState) -> Modifiers {
    // MetaState is a newtype wrapping u32: MetaState(pub u32)
    let bits: u32 = meta.0;
    let mut mods = Modifiers::empty();

    // Android meta state flags
    const META_SHIFT: u32 = 0x01;
    const META_ALT: u32 = 0x02;
    const META_CTRL: u32 = 0x1000;
    const META_META: u32 = 0x10000;
    const META_CAPS_LOCK: u32 = 0x0010_0000;
    const META_NUM_LOCK: u32 = 0x0020_0000;

    if bits & META_SHIFT != 0 {
        mods |= Modifiers::SHIFT;
    }
    if bits & META_ALT != 0 {
        mods |= Modifiers::ALT;
    }
    if bits & META_CTRL != 0 {
        mods |= Modifiers::CONTROL;
    }
    if bits & META_META != 0 {
        mods |= Modifiers::META;
    }
    if bits & META_CAPS_LOCK != 0 {
        mods |= Modifiers::CAPS_LOCK;
    }
    if bits & META_NUM_LOCK != 0 {
        mods |= Modifiers::NUM_LOCK;
    }

    mods
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_foundation::geometry::Size;
    use flui_platform_api::pointer::{CancelReason, Pressure};

    #[test]
    fn android_native_key_map_preserves_logical_keys() {
        use android_activity::input::KeyMapChar;
        for (named, mapped, expected) in [
            (
                NamedKey::Unidentified,
                Some(KeyMapChar::Unicode('A')),
                Key::Character("A".into()),
            ),
            (
                NamedKey::Unidentified,
                Some(KeyMapChar::Unicode('Я')),
                Key::Character("Я".into()),
            ),
            (
                NamedKey::Unidentified,
                Some(KeyMapChar::Unicode('!')),
                Key::Character("!".into()),
            ),
            (
                NamedKey::Unidentified,
                Some(KeyMapChar::CombiningAccent('´')),
                Key::Named(NamedKey::Dead),
            ),
            (
                NamedKey::Unidentified,
                Some(KeyMapChar::None),
                Key::Named(NamedKey::Unidentified),
            ),
            (NamedKey::Unidentified, None, Key::Character("a".into())),
            (NamedKey::Enter, None, Key::Named(NamedKey::Enter)),
        ] {
            assert_eq!(
                mapped_key(
                    named,
                    mapped,
                    ui_events::keyboard::android::KEYCODE_A,
                    Modifiers::empty()
                ),
                expected
            );
        }
        for (code, modifiers, expected) in [
            (29, Modifiers::SHIFT, "A"),
            (54, Modifiers::CAPS_LOCK, "Z"),
            (29, Modifiers::SHIFT | Modifiers::CAPS_LOCK, "a"),
            (8, Modifiers::SHIFT, "!"),
            (11, Modifiers::empty(), "4"),
            (76, Modifiers::SHIFT, "?"),
        ] {
            assert_eq!(
                mapped_key(NamedKey::Unidentified, None, code, modifiers),
                Key::Character(expected.into())
            );
        }
    }

    #[test]
    fn android_native_pointer_readings() {
        let rows: &[(&str, fn())] = &[
            ("eraser remains a tool", eraser_remains_a_tool),
            (
                "buttons preserve contact and barrel",
                buttons_preserve_contact_and_barrel,
            ),
            (
                "fractional physical contact is logical",
                fractional_contact_is_logical,
            ),
            (
                "invalid scale refuses geometry",
                invalid_scale_refuses_geometry,
            ),
            (
                "native orientation changes coordinate basis",
                native_orientation_changes_basis,
            ),
            (
                "known zero sensors remain readings",
                known_zero_sensors_remain_readings,
            ),
            (
                "unknown sensor capability remains absent",
                unknown_sensor_capability_remains_absent,
            ),
            (
                "partial pen capabilities remain partial",
                partial_pen_capabilities_remain_partial,
            ),
            (
                "native device identities stay independent",
                native_device_identities_stay_independent,
            ),
            (
                "lifecycle cancellation retires admitted contacts",
                lifecycle_cancellation_retires_contacts,
            ),
        ];
        for (name, row) in rows {
            row();
            eprintln!("passed: {name}");
        }
    }

    fn eraser_remains_a_tool() {
        assert_eq!(
            pointer_kind(ToolType::Eraser),
            PointerKind::Pen {
                tool: PenTool::Eraser
            }
        );
        assert_eq!(
            pointer_kind(ToolType::Stylus),
            PointerKind::Pen { tool: PenTool::Tip }
        );
        assert_eq!(pointer_kind(ToolType::Unknown), PointerKind::Unknown);
    }

    fn buttons_preserve_contact_and_barrel() {
        use flui_platform_api::pointer::PointerButton as Button;
        let held = native_buttons(android_activity::input::ButtonState(0x20 | 0x40), true);
        assert!(held.contains(Button::PRIMARY));
        assert!(held.contains(Button::SECONDARY));
        assert!(held.contains(Button::AUXILIARY));
        assert_eq!(
            native_buttons(android_activity::input::ButtonState(0), false),
            flui_platform_api::pointer::PointerButtons::NONE
        );
    }

    /// The sample a hovering contact of `kind` measures from `sensors` at
    /// `scale`, or why it was refused.
    fn measured(
        kind: PointerKind,
        scale: f64,
        sensors: RawSensors,
    ) -> Result<
        flui_platform_api::pointer::PointerSample,
        crate::shared::pointer_assembly::DecodeFault,
    > {
        let mut contacts = ContactRegistry::new(NativeClock::epoch_only());
        let reading = NativeReading::new(
            NativeContact(0),
            kind,
            ReadingAction::Enter,
            ReadingTime::epoch(EventTime::from_nanos(17)),
        )
        .at(4.0, 6.0, scale)
        .with_sensors(sensors);
        decode(&mut contacts, &reading).map_err(|error| error.fault())?;
        let moved = NativeReading {
            action: ReadingAction::Move,
            ..reading
        };
        let events = decode(&mut contacts, &moved).map_err(|error| error.fault())?;
        Ok(events
            .iter()
            .find_map(|input| match input.as_pointer() {
                Some(PointerEvent::Move(movement)) => Some(*movement.current()),
                _ => None,
            })
            .expect("a hovering contact moves"))
    }

    fn pen() -> PointerKind {
        PointerKind::Pen { tool: PenTool::Tip }
    }

    fn fractional_contact_is_logical() {
        for (raw, expected) in [((18.5, 7.25), (9.25, 3.625)), ((0.0, 0.0), (0.0, 0.0))] {
            let sample = measured(
                PointerKind::Touch,
                2.0,
                RawSensors {
                    contact_size: Some(raw),
                    ..RawSensors::default()
                },
            )
            .expect("reported finite axes");
            let size = sample.contact_size.expect("reported contact").get();
            assert_eq!((size.width, size.height), expected);
        }
    }

    fn invalid_scale_refuses_geometry() {
        for scale in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert!(measured(PointerKind::Touch, scale, RawSensors::default()).is_err());
        }
    }

    fn native_orientation_changes_basis() {
        let sample = measured(
            pen(),
            1.0,
            RawSensors {
                angles: pen_angles(Some(core::f64::consts::FRAC_PI_4), Some(0.0)),
                ..RawSensors::default()
            },
        )
        .expect("finite pen angles");
        let angles = sample.orientation.expect("both angles");
        assert_eq!(angles.altitude(), Some(core::f64::consts::FRAC_PI_4));
        assert_eq!(angles.azimuth(), Some(3.0 * core::f64::consts::FRAC_PI_2));
        assert_eq!(pen_angles(Some(f64::NAN), None), None);
    }

    fn capabilities(axes: &[Axis]) -> DeviceCapabilities {
        DeviceCapabilities {
            source: 0,
            axes: axes
                .iter()
                .fold(0, |bits, axis| bits | (1_u64 << u32::from(*axis))),
        }
    }

    fn known_zero_sensors_remain_readings() {
        let sensors = native_sensors(
            pen(),
            Some(capabilities(&[
                Axis::Pressure,
                Axis::Tilt,
                Axis::Orientation,
                Axis::TouchMajor,
                Axis::TouchMinor,
            ])),
            &|_| 0.0,
        );
        let sample = measured(pen(), 2.0, sensors).expect("reported zero samples");
        assert_eq!(sample.time, EventTime::from_nanos(17));
        assert_eq!(sample.pressure.map(Pressure::get), Some(0.0));
        let orientation = sample.orientation.expect("two reported angles");
        assert_eq!(orientation.altitude(), Some(core::f64::consts::FRAC_PI_2));
        assert_eq!(
            orientation.azimuth(),
            Some(3.0 * core::f64::consts::FRAC_PI_2)
        );
        assert_eq!(
            sample.contact_size.expect("reported contact axes").get(),
            Size::ZERO
        );
        assert_eq!(sample.tangential_pressure, None);
        assert_eq!(sample.twist, None);
    }

    fn unknown_sensor_capability_remains_absent() {
        let eraser = PointerKind::Pen {
            tool: PenTool::Eraser,
        };
        for capabilities in [None, Some(capabilities(&[]))] {
            assert_eq!(
                native_sensors(eraser, capabilities, &|_| 0.0),
                RawSensors::default()
            );
        }
        assert_eq!(
            native_sensors(
                PointerKind::Mouse,
                Some(capabilities(&[Axis::Pressure])),
                &|_| 0.5
            ),
            RawSensors::default(),
            "a mouse has no pressure sensor"
        );
    }

    fn partial_pen_capabilities_remain_partial() {
        for (axes, expected) in [
            (
                &[Axis::Tilt][..],
                (Some(core::f64::consts::FRAC_PI_2), None),
            ),
            (
                &[Axis::Orientation][..],
                (None, Some(3.0 * core::f64::consts::FRAC_PI_2)),
            ),
        ] {
            let sensors = native_sensors(pen(), Some(capabilities(axes)), &|_| 0.0);
            let sample = measured(pen(), 2.0, sensors).expect("position and partial angles");
            let orientation = sample.orientation.expect("one reported angle");
            assert_eq!((orientation.altitude(), orientation.azimuth()), expected);
        }
        let flat = measured(
            pen(),
            1.0,
            RawSensors {
                angles: pen_angles(Some(f64::from(core::f32::consts::FRAC_PI_2)), Some(0.0)),
                ..RawSensors::default()
            },
        )
        .expect("native flat pen endpoint");
        assert_eq!(flat.orientation.expect("both angles").altitude(), Some(0.0));
    }

    fn native_device_identities_stay_independent() {
        let mut contacts = ContactRegistry::new(NativeClock::epoch_only());
        let down = |contacts: &mut ContactRegistry, device| {
            let reading = NativeReading::new(
                NativeContact(0),
                PointerKind::Touch,
                ReadingAction::Press(PointerButton::PRIMARY),
                ReadingTime::epoch(EventTime::from_nanos(1)),
            )
            .at(1.0, 1.0, 1.0)
            .from_device(native_device(device));
            decode(contacts, &reading)
                .expect("admitted")
                .iter()
                .find_map(|input| match input.as_pointer() {
                    Some(PointerEvent::Down(press)) => Some(press.pointer),
                    _ => None,
                })
                .expect("down")
        };
        let first = down(&mut contacts, 1);
        let virtual_device = down(&mut contacts, -1);
        assert_ne!(native_device(1), native_device(-1));
        assert_ne!(
            first.id, virtual_device.id,
            "one pointer label on two devices"
        );
        assert_ne!(first.device, virtual_device.device);
    }

    fn lifecycle_cancellation_retires_contacts() {
        let mut owner = AndroidInputState::default();
        assert!(
            owner.cancel_contacts(NativeCancel::FocusLost).is_empty(),
            "nothing to cancel before any event"
        );
        owner.last_time = Some(EventTime::from_nanos(91));
        let press = |owner: &mut AndroidInputState, label, kind| {
            let reading = NativeReading::new(
                NativeContact(label),
                kind,
                ReadingAction::Press(PointerButton::PRIMARY),
                ReadingTime::epoch(EventTime::from_nanos(50)),
            )
            .on(SURFACE)
            .at(1.0, 1.0, 1.0);
            decode(&mut owner.contacts, &reading)
                .expect("admitted")
                .iter()
                .find_map(|input| match input.as_pointer() {
                    Some(PointerEvent::Down(press)) => Some(press.pointer),
                    _ => None,
                })
                .expect("down")
        };
        let first = press(&mut owner, 4, PointerKind::Touch);
        let second = press(
            &mut owner,
            2,
            PointerKind::Pen {
                tool: PenTool::Eraser,
            },
        );
        let events = owner.cancel_contacts(NativeCancel::FocusLost);
        let [
            PlatformInput::Pointer(PointerEvent::Cancel(first_cancel)),
            PlatformInput::Pointer(PointerEvent::Cancel(second_cancel)),
        ] = &events[..]
        else {
            panic!("two contacts must end in native cancellation");
        };
        assert_eq!(first_cancel.pointer, first);
        assert_eq!(second_cancel.pointer, second);
        assert_eq!(first_cancel.time, EventTime::from_nanos(91));
        assert_eq!(second_cancel.reason, CancelReason::FocusLost);
        assert!(owner.cancel_contacts(NativeCancel::FocusLost).is_empty());
        press(&mut owner, 4, PointerKind::Touch);
        assert_eq!(owner.cancel_contacts(NativeCancel::CaptureLost).len(), 1);
    }
}
