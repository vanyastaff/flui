//! Native Android input: `MotionEvent`s copied into plain facts for the
//! shared translation (`shared::android_motion`), and `KeyEvent`s into the
//! owned keyboard vocabulary.
//!
//! What stays here is what needs the NDK or the JVM: reading the event's
//! fields and discovering its device's motion ranges through Java.

use std::{collections::HashMap, sync::Arc};

use android_activity::input::{Axis, KeyAction, MotionAction as NdkAction, ToolType};
use flui_platform_api::{EventTime, keyboard::Modifiers as OwnedModifiers};
use keyboard_types::{Key, KeyState, Modifiers, NamedKey};
use ui_events::keyboard::KeyboardEvent;

use crate::{
    shared::android_motion::{
        AndroidMotion, Axes, DeviceCapabilities, DeviceDiscovery, Motion, MotionAction,
        PointerFacts, Tool, axis,
    },
    shared::keyboard_adapter::keyboard_input,
    shared::pointer_assembly::NativeCancel,
    traits::PlatformInput,
};

/// `AXIS_GESTURE_SCROLL_X_DISTANCE` (API 34).
const AXIS_GESTURE_SCROLL_X_DISTANCE: u32 = 50;
/// `AXIS_GESTURE_SCROLL_Y_DISTANCE` (API 34).
const AXIS_GESTURE_SCROLL_Y_DISTANCE: u32 = 51;
/// `AXIS_GESTURE_PINCH_SCALE_FACTOR` (API 34).
const AXIS_GESTURE_PINCH_SCALE_FACTOR: u32 = 52;
/// `AXIS_GESTURE_SWIPE_FINGER_COUNT` (API 34).
const AXIS_GESTURE_SWIPE_FINGER_COUNT: u32 = 53;

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
            let mut reported = Vec::with_capacity(axis::SENSORS.len());
            for axis in axis::SENSORS {
                let range = env
                    .call_method(
                        &device,
                        jni_str!("getMotionRange"),
                        jni_sig!("(II)Landroid/view/InputDevice$MotionRange;"),
                        &[
                            JValue::Int(
                                i32::try_from(axis).expect("BUG: Android sensor axes fit i32"),
                            ),
                            JValue::Int(i32::from_ne_bytes(source.to_ne_bytes())),
                        ],
                    )?
                    .l()?;
                if !range.is_null() {
                    reported.push(axis);
                }
            }
            Ok(DeviceReading::Present {
                cached: CachedDevice {
                    capabilities: DeviceCapabilities::new(source, reported),
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

/// Input state for the Android window owner: the shared translation, and
/// the Java device objects its capability cache compares by identity. No
/// user callback lives here.
#[derive(Default)]
pub(crate) struct AndroidInputState {
    motion: AndroidMotion,
    devices: HashMap<i32, CachedDevice>,
}

impl AndroidInputState {
    pub(crate) fn cancel_contacts(&mut self, reason: NativeCancel) -> Vec<PlatformInput> {
        self.motion.cancel_contacts(reason)
    }

    /// What the translation held that is due by the end of this drain.
    pub(crate) fn finish_drain(&mut self) -> Vec<PlatformInput> {
        self.motion.finish_drain(web_time::Instant::now())
    }

    pub(crate) fn capabilities(&self, id: i32) -> Option<CachedDevice> {
        self.devices.get(&id).cloned()
    }

    pub(crate) fn convert_motion_event(
        &mut self,
        event: &android_activity::input::MotionEvent<'_>,
        scale_factor: f64,
        scroll_policy: crate::shared::android_scroll::AxisPolicy,
        reading: DeviceReading,
    ) -> Vec<PlatformInput> {
        let id = event.device_id();
        let discovery = match reading {
            DeviceReading::Present { cached, replaced } => {
                let capabilities = cached.capabilities;
                self.devices.insert(id, cached);
                DeviceDiscovery::Present {
                    capabilities,
                    replaced,
                }
            }
            DeviceReading::Removed => {
                self.devices.remove(&id);
                DeviceDiscovery::Removed
            }
            DeviceReading::Unavailable => DeviceDiscovery::Unavailable,
        };
        let pointers: Vec<PointerFacts> = event
            .pointers()
            .map(|pointer| PointerFacts {
                id: pointer.pointer_id(),
                tool: tool(pointer.tool_type()),
                axes: axes(&|axis| pointer.axis_value(axis)),
                history: pointer
                    .history()
                    .filter_map(|past| {
                        let time = u64::try_from(past.event_time())
                            .ok()
                            .map(EventTime::from_nanos)?;
                        Some((time, axes(&|axis| past.axis_value(axis))))
                    })
                    .collect(),
            })
            .collect();
        let motion = Motion {
            device: id,
            source: u32::from(event.source()),
            action: action(event.action()),
            action_index: event.pointer_index(),
            action_button: u32::from(event.action_button()),
            buttons: event.button_state().0,
            modifiers: owned_modifiers(event.meta_state()),
            time: u64::try_from(event.event_time())
                .ok()
                .map(EventTime::from_nanos),
            pointers: &pointers,
        };
        // The accepted activity-context policy already projects native
        // factors into logical pixels exactly once.
        self.motion.decode(
            &motion,
            scale_factor,
            &|horizontal, vertical| scroll_policy.delta(horizontal, vertical),
            discovery,
            web_time::Instant::now(),
        )
    }
}

fn action(action: NdkAction) -> MotionAction {
    match action {
        NdkAction::Down => MotionAction::Down,
        NdkAction::Up => MotionAction::Up,
        NdkAction::Move => MotionAction::Move,
        NdkAction::Cancel => MotionAction::Cancel,
        NdkAction::PointerDown => MotionAction::PointerDown,
        NdkAction::PointerUp => MotionAction::PointerUp,
        NdkAction::HoverEnter => MotionAction::HoverEnter,
        NdkAction::HoverMove => MotionAction::HoverMove,
        NdkAction::HoverExit => MotionAction::HoverExit,
        NdkAction::Scroll => MotionAction::Scroll,
        NdkAction::ButtonPress => MotionAction::ButtonPress,
        NdkAction::ButtonRelease => MotionAction::ButtonRelease,
        _ => MotionAction::Other,
    }
}

fn tool(tool: ToolType) -> Tool {
    match tool {
        ToolType::Finger => Tool::Finger,
        ToolType::Stylus => Tool::Stylus,
        ToolType::Eraser => Tool::Eraser,
        ToolType::Mouse => Tool::Mouse,
        _ => Tool::Unknown,
    }
}

/// The axes the translation reads, from `value`.
fn axes(value: &dyn Fn(Axis) -> f32) -> Axes {
    Axes {
        x: value(Axis::X),
        y: value(Axis::Y),
        pressure: value(Axis::Pressure),
        touch_major: value(Axis::TouchMajor),
        touch_minor: value(Axis::TouchMinor),
        tilt: value(Axis::Tilt),
        orientation: value(Axis::Orientation),
        hscroll: value(Axis::Hscroll),
        vscroll: value(Axis::Vscroll),
        gesture_scroll_x: value(Axis::from(AXIS_GESTURE_SCROLL_X_DISTANCE)),
        gesture_scroll_y: value(Axis::from(AXIS_GESTURE_SCROLL_Y_DISTANCE)),
        pinch_scale: value(Axis::from(AXIS_GESTURE_PINCH_SCALE_FACTOR)),
        swipe_fingers: value(Axis::from(AXIS_GESTURE_SWIPE_FINGER_COUNT)),
    }
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

/// The key presses an input method's editor action arrives as: `key`
/// pressed and released at `uptime_ms`, with Shift when `shift`.
pub(crate) fn editor_action_keys(
    key: flui_platform_api::keyboard::NamedKey,
    shift: bool,
    uptime_ms: i64,
) -> [PlatformInput; 2] {
    use flui_platform_api::keyboard::{Code, Key as OwnedKey, KeyEvent, KeyState as OwnedState};
    let time = EventTime::from_nanos(
        u64::try_from(uptime_ms)
            .unwrap_or(0)
            .saturating_mul(1_000_000),
    );
    let modifiers = if shift {
        OwnedModifiers::SHIFT
    } else {
        OwnedModifiers::NONE
    };
    [OwnedState::Down, OwnedState::Up].map(|state| {
        PlatformInput::Keyboard(
            KeyEvent::new(state, OwnedKey::Named(key), Code::Unidentified, time)
                .with_modifiers(modifiers),
        )
    })
}

/// Convert an Android `KeyEvent` to a `PlatformInput`. `composing` is whether
/// the input method composes now: a key it passed on during a composition is
/// still its own (`shared::android_ime::key_composition`).
pub fn convert_key_event(
    app: &android_activity::AndroidApp,
    event: &android_activity::input::KeyEvent<'_>,
    composing: bool,
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
            is_composing: crate::shared::android_ime::key_composition(composing)
                == flui_platform_api::keyboard::ImeComposition::Active,
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
}
