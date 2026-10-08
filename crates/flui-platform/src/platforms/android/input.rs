//! Native Android input translated directly into FLUI's owned vocabulary.
//!
//! Coordinates and contact axes are physical pixels on this wire; both are
//! divided by the window scale once. Sensor presence comes from the device's
//! motion ranges, not from whether an axis happens to read zero.

use std::{collections::HashMap, sync::Arc};

use android_activity::input::{Axis, Button, ButtonState, KeyAction, MotionAction, ToolType};
use flui_foundation::geometry::{Point, Size};
use flui_platform_api::{
    EventTime,
    keyboard::Modifiers as OwnedModifiers,
    pointer::{
        ButtonChange, CancelReason, ContactSize, DeviceId, PenOrientation, PenTool, PointerButton,
        PointerButtons, PointerCancel, PointerDeviceChange, PointerEvent, PointerId, PointerInfo,
        PointerKind, PointerMove, PointerPosition, PointerPress, PointerRelease, PointerRole,
        PointerSample, PointerSignal, Pressure, ScrollDelta, ScrollEvent, ScrollUnit,
    },
};
use keyboard_types::{Key, KeyState, Modifiers, NamedKey};
use ui_events::keyboard::KeyboardEvent;

use crate::{shared::keyboard_adapter::keyboard_input, traits::PlatformInput};

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

struct Device {
    cached: CachedDevice,
    id: DeviceId,
}

struct Contact {
    info: PointerInfo,
    button: PointerButton,
}

/// Input state for the Android window owner. Java device references are used
/// only for identity and capability caching; no user callback lives here.
pub(crate) struct AndroidInputState {
    devices: HashMap<i32, Device>,
    contacts: HashMap<PointerId, Contact>,
    next_device: Option<std::num::NonZeroU64>,
    last_time: Option<EventTime>,
}

impl Default for AndroidInputState {
    fn default() -> Self {
        Self {
            devices: HashMap::new(),
            contacts: HashMap::new(),
            next_device: Some(std::num::NonZeroU64::MIN),
            last_time: None,
        }
    }
}

impl AndroidInputState {
    fn allocate_device_id(&mut self) -> Option<DeviceId> {
        let next = self.next_device?;
        self.next_device = next.checked_add(1);
        Some(DeviceId::new(next))
    }

    pub(crate) fn cancel_contacts(&mut self, reason: CancelReason) -> Vec<PlatformInput> {
        let Some(time) = self.last_time else {
            return Vec::new();
        };
        let mut contacts: Vec<_> = self
            .contacts
            .drain()
            .map(|(_, contact)| contact.info)
            .collect();
        contacts.sort_by_key(|info| info.id);
        contacts
            .into_iter()
            .map(|info| {
                PlatformInput::Pointer(PointerEvent::Cancel(PointerCancel::new(info, time, reason)))
            })
            .collect()
    }

    pub(crate) fn capabilities(&self, id: i32) -> Option<CachedDevice> {
        self.devices.get(&id).map(|device| device.cached.clone())
    }

    pub(crate) fn convert_motion_event(
        &mut self,
        event: &android_activity::input::MotionEvent<'_>,
        scale_factor: f64,
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
        let native_device = event.device_id();
        let had_device = self.devices.contains_key(&native_device);
        let replacement = match &discovery {
            DeviceReading::Present { replaced, .. } => *replaced,
            DeviceReading::Removed => true,
            DeviceReading::Unavailable => false,
        };
        if replacement && let Some(old) = self.devices.remove(&native_device) {
            let mut retired: Vec<_> = self
                .contacts
                .values()
                .map(|contact| contact.info)
                .filter(|info| info.device == Some(old.id))
                .collect();
            retired.sort_by_key(|info| info.id);
            for info in retired {
                self.contacts.remove(&info.id);
                output.push(PlatformInput::Pointer(PointerEvent::Cancel(
                    PointerCancel::new(info, delivery_time, CancelReason::DeviceRemoved),
                )));
            }
            output.push(PlatformInput::Pointer(PointerEvent::DeviceRemoved(
                PointerDeviceChange::new(old.id, PointerKind::Unknown, delivery_time),
            )));
        }
        let capabilities = match discovery {
            DeviceReading::Present { cached, .. } => {
                let capabilities = cached.capabilities;
                if let Some(device) = self.devices.get_mut(&native_device) {
                    device.cached = cached;
                } else {
                    // The counter refuses permanently after its final identity.
                    let Some(id) = self.allocate_device_id() else {
                        return output;
                    };
                    self.devices.insert(native_device, Device { cached, id });
                    output.push(PlatformInput::Pointer(PointerEvent::DeviceAdded(
                        PointerDeviceChange::new(id, PointerKind::Unknown, delivery_time),
                    )));
                }
                Some(capabilities)
            }
            DeviceReading::Removed if had_device => return output,
            // Android-generated/virtual events can have no InputDevice object.
            // Preserve their native contact and position, with absent device/sensors.
            DeviceReading::Removed | DeviceReading::Unavailable => None,
        };
        let modifiers = owned_modifiers(event.meta_state());
        let action = event.action();
        if matches!(action, MotionAction::Cancel) {
            let native = u64::from(u32::from_ne_bytes(native_device.to_ne_bytes()));
            let mut retired: Vec<_> = self
                .contacts
                .values()
                .map(|contact| contact.info)
                .filter(|info| (info.id.get().get() - 1) >> 32 == native)
                .collect();
            retired.sort_by_key(|info| info.id);
            for info in retired {
                self.contacts.remove(&info.id);
                output.push(PlatformInput::Pointer(PointerEvent::Cancel(
                    PointerCancel::new(info, delivery_time, CancelReason::Platform),
                )));
            }
            return output;
        }
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
            let Some(id) = native_pointer_id(native_device, pointer.pointer_id()) else {
                continue;
            };
            let kind = pointer_kind(pointer.tool_type());
            let role = self.contacts.get(&id).map_or_else(
                || {
                    if self
                        .contacts
                        .values()
                        .any(|contact| same_kind(contact.info.kind, kind))
                    {
                        PointerRole::Additional
                    } else {
                        PointerRole::Primary
                    }
                },
                |contact| contact.info.role,
            );
            let mut info = PointerInfo::new(id, kind).with_role(role);
            if let Some(device) = self.devices.get(&native_device) {
                info = info.with_device(device.id);
            }
            let in_contact = match action {
                MotionAction::Down | MotionAction::PointerDown | MotionAction::Move => true,
                MotionAction::ButtonPress | MotionAction::ButtonRelease => {
                    self.contacts.contains_key(&id)
                }
                _ => false,
            } && matches!(kind, PointerKind::Touch | PointerKind::Pen { .. });
            let buttons = native_buttons(event.button_state(), in_contact);
            let sample = time.and_then(|time| {
                native_sample(time, kind, scale_factor, capabilities, |axis| {
                    pointer.axis_value(axis)
                })
            });
            let converted = match action {
                MotionAction::Down | MotionAction::PointerDown => {
                    let Some(sample) = sample else { continue };
                    let button = if matches!(kind, PointerKind::Mouse) {
                        buttons.iter().next().unwrap_or(PointerButton::PRIMARY)
                    } else {
                        PointerButton::PRIMARY
                    };
                    self.contacts.insert(id, Contact { info, button });
                    Some(PointerEvent::Down(
                        PointerPress::new(info, button, buttons, sample).with_modifiers(modifiers),
                    ))
                }
                MotionAction::Up | MotionAction::PointerUp => {
                    let button = self
                        .contacts
                        .remove(&id)
                        .map_or(PointerButton::PRIMARY, |contact| contact.button);
                    Some(match sample {
                        Some(sample) => PointerEvent::Up(
                            PointerRelease::new(info, button, buttons, sample)
                                .with_modifiers(modifiers),
                        ),
                        None => PointerEvent::Cancel(PointerCancel::new(
                            info,
                            delivery_time,
                            CancelReason::InvalidInput,
                        )),
                    })
                }
                MotionAction::Move | MotionAction::HoverMove => {
                    let Some(sample) = sample else { continue };
                    if let Some(contact) = self.contacts.get_mut(&id) {
                        contact.info = info;
                    }
                    let history = pointer
                        .history()
                        .filter_map(|past| {
                            let time = u64::try_from(past.event_time())
                                .ok()
                                .map(EventTime::from_nanos)?;
                            native_sample(time, kind, scale_factor, capabilities, |axis| {
                                past.axis_value(axis)
                            })
                        })
                        .collect();
                    Some(PointerEvent::Move(
                        PointerMove::new(info, buttons, sample)
                            .with_modifiers(modifiers)
                            .with_coalesced(history),
                    ))
                }
                MotionAction::ButtonPress | MotionAction::ButtonRelease => {
                    let Some(sample) = sample else { continue };
                    let Some(button) = native_button(event.action_button()) else {
                        continue;
                    };
                    if matches!(action, MotionAction::ButtonPress) {
                        Some(PointerEvent::ButtonChange(ButtonChange::Pressed(
                            PointerPress::new(info, button, buttons, sample)
                                .with_modifiers(modifiers),
                        )))
                    } else {
                        Some(PointerEvent::ButtonChange(ButtonChange::Released(
                            PointerRelease::new(info, button, buttons, sample)
                                .with_modifiers(modifiers),
                        )))
                    }
                }
                MotionAction::HoverEnter | MotionAction::HoverExit => {
                    let mut signal = PointerSignal::new(info, delivery_time);
                    if let Some(sample) = sample {
                        signal = signal.with_position(sample.position);
                    }
                    Some(if matches!(action, MotionAction::HoverEnter) {
                        PointerEvent::Enter(signal)
                    } else {
                        PointerEvent::Leave(signal)
                    })
                }
                MotionAction::Scroll => {
                    let Some(sample) = sample else { continue };
                    // Android axes are positive left/up, opposite the owned wheel convention.
                    // Their unit is wheel detents; no platform precision or phase is reported.
                    let delta = ScrollDelta::try_new(
                        ScrollUnit::Lines,
                        -f64::from(pointer.axis_value(Axis::Hscroll)),
                        -f64::from(pointer.axis_value(Axis::Vscroll)),
                    )
                    .unwrap_or_else(|_| ScrollDelta::zero(ScrollUnit::Lines));
                    Some(PointerEvent::Scroll(
                        ScrollEvent::new(info, sample.time, sample.position, delta)
                            .with_modifiers(modifiers),
                    ))
                }
                _ => None,
            };
            if let Some(converted) = converted {
                output.push(PlatformInput::Pointer(converted));
            }
        }
        output
    }
}

fn native_pointer_id(device: i32, pointer: i32) -> Option<PointerId> {
    // Preserve the signed native device identity injectively; contact IDs are
    // nonnegative Android IDs. Device and pointer namespaces never collide.
    let device = u64::from(u32::from_ne_bytes(device.to_ne_bytes()));
    let pointer = u64::try_from(pointer).ok()?;
    if pointer > u64::from(u32::MAX) {
        return None;
    }
    PointerId::try_from(((device << 32) | pointer).checked_add(1)?).ok()
}

fn same_kind(a: PointerKind, b: PointerKind) -> bool {
    match (a, b) {
        (PointerKind::Pen { .. }, PointerKind::Pen { .. }) => true,
        _ => a == b,
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

fn logical_contact(major: f64, minor: f64, scale: f64) -> Option<ContactSize> {
    if !scale.is_finite() || scale <= 0.0 {
        return None;
    }
    ContactSize::try_new(Size::new(major / scale, minor / scale)).ok()
}

fn pen_orientation(tilt: f64, orientation: f64) -> Option<PenOrientation> {
    // Android measures tilt from the screen normal, orientation clockwise
    // from upwards. Owned altitude is above the surface, azimuth from rightwards.
    if !tilt.is_finite() || !(0.0..=f64::from(core::f32::consts::FRAC_PI_2)).contains(&tilt) {
        return None;
    }
    PenOrientation::try_new(
        core::f64::consts::FRAC_PI_2 - tilt.min(core::f64::consts::FRAC_PI_2),
        orientation - core::f64::consts::FRAC_PI_2,
    )
    .ok()
}

fn native_sample(
    time: EventTime,
    kind: PointerKind,
    scale: f64,
    capabilities: Option<DeviceCapabilities>,
    mut axis: impl FnMut(Axis) -> f32,
) -> Option<PointerSample> {
    if !scale.is_finite() || scale <= 0.0 {
        return None;
    }
    let position = PointerPosition::try_new(Point::new(
        f64::from(axis(Axis::X)) / scale,
        f64::from(axis(Axis::Y)) / scale,
    ))
    .ok()?;
    let mut sample = PointerSample::new(time, position);
    let Some(capabilities) = capabilities else {
        return Some(sample);
    };
    if matches!(kind, PointerKind::Touch | PointerKind::Pen { .. }) {
        if capabilities.has(Axis::Pressure) {
            sample.pressure = Pressure::saturating(axis(Axis::Pressure)).ok();
        }
        if capabilities.has(Axis::TouchMajor) && capabilities.has(Axis::TouchMinor) {
            sample.contact_size = logical_contact(
                f64::from(axis(Axis::TouchMajor)),
                f64::from(axis(Axis::TouchMinor)),
                scale,
            );
        }
    }
    if matches!(kind, PointerKind::Pen { .. }) {
        sample.orientation = match (
            capabilities.has(Axis::Tilt),
            capabilities.has(Axis::Orientation),
        ) {
            (true, true) => pen_orientation(
                f64::from(axis(Axis::Tilt)),
                f64::from(axis(Axis::Orientation)),
            ),
            (true, false) => {
                let tilt = f64::from(axis(Axis::Tilt));
                if (0.0..=f64::from(core::f32::consts::FRAC_PI_2)).contains(&tilt) {
                    PenOrientation::try_altitude(
                        core::f64::consts::FRAC_PI_2 - tilt.min(core::f64::consts::FRAC_PI_2),
                    )
                    .ok()
                } else {
                    None
                }
            }
            (false, true) => PenOrientation::try_azimuth(
                f64::from(axis(Axis::Orientation)) - core::f64::consts::FRAC_PI_2,
            )
            .ok(),
            (false, false) => None,
        };
    }
    Some(sample)
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
pub fn convert_key_event(event: &android_activity::input::KeyEvent<'_>) -> Option<PlatformInput> {
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

    // Derive the Key from the named key mapping
    let key = if named_key == NamedKey::Unidentified {
        // Try to map character keys (A-Z, 0-9, symbols)
        keycode_to_character(keycode_i32).map_or(Key::Named(NamedKey::Unidentified), |ch| {
            Key::Character(ch.to_string())
        })
    } else {
        Key::Named(named_key)
    };

    let modifiers = convert_meta_state(event.meta_state());
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

/// Map Android keycode to a character, for keys that produce printable
/// characters.
fn keycode_to_character(keycode: i32) -> Option<char> {
    use ui_events::keyboard::android::{
        KEYCODE_0, KEYCODE_1, KEYCODE_2, KEYCODE_3, KEYCODE_4, KEYCODE_5, KEYCODE_6, KEYCODE_7,
        KEYCODE_8, KEYCODE_9, KEYCODE_A, KEYCODE_APOSTROPHE, KEYCODE_AT, KEYCODE_B,
        KEYCODE_BACKSLASH, KEYCODE_C, KEYCODE_COMMA, KEYCODE_D, KEYCODE_E, KEYCODE_EQUALS,
        KEYCODE_F, KEYCODE_G, KEYCODE_GRAVE, KEYCODE_H, KEYCODE_I, KEYCODE_J, KEYCODE_K, KEYCODE_L,
        KEYCODE_LEFT_BRACKET, KEYCODE_M, KEYCODE_MINUS, KEYCODE_N, KEYCODE_O, KEYCODE_P,
        KEYCODE_PERIOD, KEYCODE_PLUS, KEYCODE_POUND, KEYCODE_Q, KEYCODE_R, KEYCODE_RIGHT_BRACKET,
        KEYCODE_S, KEYCODE_SEMICOLON, KEYCODE_SLASH, KEYCODE_SPACE, KEYCODE_STAR, KEYCODE_T,
        KEYCODE_U, KEYCODE_V, KEYCODE_W, KEYCODE_X, KEYCODE_Y, KEYCODE_Z,
    };

    match keycode {
        KEYCODE_A => Some('a'),
        KEYCODE_B => Some('b'),
        KEYCODE_C => Some('c'),
        KEYCODE_D => Some('d'),
        KEYCODE_E => Some('e'),
        KEYCODE_F => Some('f'),
        KEYCODE_G => Some('g'),
        KEYCODE_H => Some('h'),
        KEYCODE_I => Some('i'),
        KEYCODE_J => Some('j'),
        KEYCODE_K => Some('k'),
        KEYCODE_L => Some('l'),
        KEYCODE_M => Some('m'),
        KEYCODE_N => Some('n'),
        KEYCODE_O => Some('o'),
        KEYCODE_P => Some('p'),
        KEYCODE_Q => Some('q'),
        KEYCODE_R => Some('r'),
        KEYCODE_S => Some('s'),
        KEYCODE_T => Some('t'),
        KEYCODE_U => Some('u'),
        KEYCODE_V => Some('v'),
        KEYCODE_W => Some('w'),
        KEYCODE_X => Some('x'),
        KEYCODE_Y => Some('y'),
        KEYCODE_Z => Some('z'),
        KEYCODE_0 => Some('0'),
        KEYCODE_1 => Some('1'),
        KEYCODE_2 => Some('2'),
        KEYCODE_3 => Some('3'),
        KEYCODE_4 => Some('4'),
        KEYCODE_5 => Some('5'),
        KEYCODE_6 => Some('6'),
        KEYCODE_7 => Some('7'),
        KEYCODE_8 => Some('8'),
        KEYCODE_9 => Some('9'),
        KEYCODE_SPACE => Some(' '),
        KEYCODE_COMMA => Some(','),
        KEYCODE_PERIOD => Some('.'),
        KEYCODE_MINUS => Some('-'),
        KEYCODE_EQUALS => Some('='),
        KEYCODE_LEFT_BRACKET => Some('['),
        KEYCODE_RIGHT_BRACKET => Some(']'),
        KEYCODE_BACKSLASH => Some('\\'),
        KEYCODE_SEMICOLON => Some(';'),
        KEYCODE_APOSTROPHE => Some('\''),
        KEYCODE_SLASH => Some('/'),
        KEYCODE_GRAVE => Some('`'),
        KEYCODE_AT => Some('@'),
        KEYCODE_STAR => Some('*'),
        KEYCODE_POUND => Some('#'),
        KEYCODE_PLUS => Some('+'),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_platform_api::pointer::{PenTool, PointerKind};

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
                "terminal device allocation stays refused",
                terminal_device_allocation_stays_refused,
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

    fn fractional_contact_is_logical() {
        let size = logical_contact(18.5, 7.25, 2.0)
            .expect("reported finite axes")
            .get();
        assert_eq!((size.width, size.height), (9.25, 3.625));
        let zero = logical_contact(0.0, 0.0, 2.0)
            .expect("reported zero axes")
            .get();
        assert_eq!((zero.width, zero.height), (0.0, 0.0));
    }

    fn invalid_scale_refuses_geometry() {
        for scale in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert_eq!(logical_contact(18.5, 7.25, scale), None);
        }
    }

    fn native_orientation_changes_basis() {
        let angles = pen_orientation(core::f64::consts::FRAC_PI_4, 0.0).expect("finite pen angles");
        assert_eq!(angles.altitude(), Some(core::f64::consts::FRAC_PI_4));
        assert_eq!(angles.azimuth(), Some(3.0 * core::f64::consts::FRAC_PI_2));
        assert_eq!(pen_orientation(f64::NAN, 0.0), None);
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
        let sample = native_sample(
            EventTime::from_nanos(17),
            PointerKind::Pen { tool: PenTool::Tip },
            2.0,
            Some(capabilities(&[
                Axis::Pressure,
                Axis::Tilt,
                Axis::Orientation,
                Axis::TouchMajor,
                Axis::TouchMinor,
            ])),
            |_| 0.0,
        )
        .expect("reported zero samples");
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
        for capabilities in [None, Some(capabilities(&[]))] {
            let sample = native_sample(
                EventTime::from_nanos(17),
                PointerKind::Pen {
                    tool: PenTool::Eraser,
                },
                2.0,
                capabilities,
                |_| 0.0,
            )
            .expect("position without sensor capabilities");
            assert_eq!(sample.pressure, None);
            assert_eq!(sample.orientation, None);
            assert_eq!(sample.contact_size, None);
        }
        let mouse = native_sample(
            EventTime::from_nanos(17),
            PointerKind::Mouse,
            2.0,
            Some(capabilities(&[Axis::Pressure])),
            |_| 0.5,
        )
        .expect("mouse position");
        assert_eq!(mouse.pressure, None);
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
            let sample = native_sample(
                EventTime::from_nanos(17),
                PointerKind::Pen { tool: PenTool::Tip },
                2.0,
                Some(capabilities(axes)),
                |_| 0.0,
            )
            .expect("position and partial angles");
            let orientation = sample.orientation.expect("one reported angle");
            assert_eq!((orientation.altitude(), orientation.azimuth()), expected);
        }
        assert_eq!(
            pen_orientation(f64::from(core::f32::consts::FRAC_PI_2), 0.0)
                .expect("native flat pen endpoint")
                .altitude(),
            Some(0.0)
        );
    }

    fn native_device_identities_stay_independent() {
        let first = native_pointer_id(1, 0).expect("first device pointer");
        let second = native_pointer_id(2, 0).expect("second device pointer");
        let virtual_device = native_pointer_id(-1, 0).expect("signed native device");
        assert_ne!(first, second);
        assert_ne!(first, virtual_device);
        assert_eq!(first.get().get(), 4_294_967_297);
        assert_eq!(native_pointer_id(1, -1), None);
    }

    fn terminal_device_allocation_stays_refused() {
        let mut owner = AndroidInputState {
            next_device: Some(std::num::NonZeroU64::MAX),
            ..AndroidInputState::default()
        };
        assert_eq!(
            owner
                .allocate_device_id()
                .expect("last admitted identity")
                .get(),
            std::num::NonZeroU64::MAX
        );
        assert_eq!(owner.allocate_device_id(), None);
        owner.devices.clear();
        assert_eq!(owner.allocate_device_id(), None);
    }

    fn lifecycle_cancellation_retires_contacts() {
        let mut owner = AndroidInputState {
            last_time: Some(EventTime::from_nanos(91)),
            ..AndroidInputState::default()
        };
        let first = PointerInfo::new(
            PointerId::try_from(1_u64).expect("fixture ID"),
            PointerKind::Touch,
        );
        let second = PointerInfo::new(
            PointerId::try_from(2_u64).expect("fixture ID"),
            PointerKind::Pen {
                tool: PenTool::Eraser,
            },
        );
        for info in [second, first] {
            owner.contacts.insert(
                info.id,
                Contact {
                    info,
                    button: PointerButton::PRIMARY,
                },
            );
        }
        let events = owner.cancel_contacts(CancelReason::FocusLost);
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
        assert!(owner.cancel_contacts(CancelReason::FocusLost).is_empty());
        owner.contacts.insert(
            first.id,
            Contact {
                info: first,
                button: PointerButton::PRIMARY,
            },
        );
        assert_eq!(owner.cancel_contacts(CancelReason::CaptureLost).len(), 1);
    }
}
