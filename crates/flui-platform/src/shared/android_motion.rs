//! Android `MotionEvent`s as plain facts, decoded into owned pointer input.
//!
//! The Android backend copies each event out of the NDK into a [`Motion`]
//! and hands it to [`AndroidMotion`], which makes the readings the shared
//! [`ContactRegistry`] decodes. Nothing here names an Android type, so Linux
//! CI runs the translation the device runs.
//!
//! Coordinates and contact axes are physical pixels; assembly divides them
//! by the window scale once. Sensor presence comes from the device's motion
//! ranges ([`DeviceCapabilities`]), not from whether an axis reads zero.
//!
//! What the translation decides, beyond copying fields:
//!
//! - **A hover exit before a press is not a leave.** Android ends a hover
//!   with `ACTION_HOVER_EXIT` at the instant the pen or cursor goes down, and
//!   starts one again with `ACTION_HOVER_ENTER` when it lifts. The exit is
//!   held until the next event: a press from the same device at the same
//!   time continues the hovering contact, so a pen keeps one pointer identity
//!   from hover through contact and back; any other event delivers it as a
//!   leave first, and so does an owner turn [`EXIT_GRACE`] after it with no
//!   event in between ([`AndroidMotion::finish_drain`]): the press may reach
//!   the queue a turn after its exit, and a pen leaving range sends nothing
//!   more. An enter for a contact the registry still holds is a move.
//! - **A touchpad is recognized by its source.** A touchpad in pointer mode
//!   reports finger tools from [`SOURCE_MOUSE`] (Android 14's gesture
//!   converter). Its hovering finger is a cursor, delivered as the mouse. Its
//!   gestures arrive as fake fingers placed around the cursor, which are
//!   not touches: a press whose first pointer carries a pinch factor opens a
//!   pinch, delivered as `PanZoom` scale about the cursor; a press without
//!   one opens a two-finger scroll, delivered as phased `Scroll` from the
//!   cursor by the gesture's scroll distance; a press with a swipe finger
//!   count is a three- or four-finger swipe, which the system navigates with
//!   and FLUI does not translate. The classification reads the gesture axes
//!   (`AXIS_GESTURE_*`, API 34) rather than `getClassification`, which the
//!   NDK event wrapper does not expose.

use std::collections::HashMap;
use std::time::Duration;

use flui_platform_api::EventTime;
use flui_platform_api::keyboard::Modifiers;
use flui_platform_api::pointer::{
    PenTool, PointerButton, PointerButtons, PointerEvent, PointerKind, ScrollDelta, ScrollPhase,
    ScrollPrecision, ScrollUnit,
};
use web_time::Instant;

use super::pointer_assembly::{
    ContactRegistry, ContactScroll, DecodeError, GestureComponent, GestureDelta, GesturePhase,
    GestureReading, MoveBatch, NativeCancel, NativeClock, NativeContact, NativeDevice,
    NativeReading, NativeSurface, PenAngles, RawSensors, ReadingAction, ReadingTime, ScrollPhases,
    ScrollReading, decode,
};
use crate::traits::PlatformInput;

/// `InputDevice.SOURCE_MOUSE`. A touchpad in pointer mode reports from it.
pub const SOURCE_MOUSE: u32 = 0x0000_2002;

/// How long a hover exit waits for the press it may continue into before it
/// is delivered as a leave: a few owner turns at the backend's 16 ms poll,
/// against the microseconds between the two in the dispatcher.
pub const EXIT_GRACE: Duration = Duration::from_millis(40);

/// The Android activity's one input surface.
const SURFACE: NativeSurface = NativeSurface(0);

/// The contact label of a touchpad pinch, apart from every pointer id.
const PINCH: NativeContact = NativeContact(u64::MAX);

/// `MotionEvent` action codes, as the backend reads them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum MotionAction {
    /// `ACTION_DOWN`.
    Down,
    /// `ACTION_UP`.
    Up,
    /// `ACTION_MOVE`.
    Move,
    /// `ACTION_CANCEL`.
    Cancel,
    /// `ACTION_POINTER_DOWN`.
    PointerDown,
    /// `ACTION_POINTER_UP`.
    PointerUp,
    /// `ACTION_HOVER_ENTER`.
    HoverEnter,
    /// `ACTION_HOVER_MOVE`.
    HoverMove,
    /// `ACTION_HOVER_EXIT`.
    HoverExit,
    /// `ACTION_SCROLL`.
    Scroll,
    /// `ACTION_BUTTON_PRESS`.
    ButtonPress,
    /// `ACTION_BUTTON_RELEASE`.
    ButtonRelease,
    /// Any other action, which delivers nothing.
    Other,
}

/// `MotionEvent` tool types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Tool {
    /// `TOOL_TYPE_FINGER`.
    Finger,
    /// `TOOL_TYPE_STYLUS`.
    Stylus,
    /// `TOOL_TYPE_ERASER`.
    Eraser,
    /// `TOOL_TYPE_MOUSE`.
    Mouse,
    /// `TOOL_TYPE_UNKNOWN` and any other.
    Unknown,
}

/// `MotionEvent` button bits.
pub mod button {
    /// `BUTTON_PRIMARY`.
    pub const PRIMARY: u32 = 0x01;
    /// `BUTTON_SECONDARY`.
    pub const SECONDARY: u32 = 0x02;
    /// `BUTTON_TERTIARY`.
    pub const TERTIARY: u32 = 0x04;
    /// `BUTTON_BACK`.
    pub const BACK: u32 = 0x08;
    /// `BUTTON_FORWARD`.
    pub const FORWARD: u32 = 0x10;
    /// `BUTTON_STYLUS_PRIMARY`.
    pub const STYLUS_PRIMARY: u32 = 0x20;
    /// `BUTTON_STYLUS_SECONDARY`.
    pub const STYLUS_SECONDARY: u32 = 0x40;
}

/// The motion axes whose ranges decide which sensors a device reports.
pub mod axis {
    /// `AXIS_PRESSURE`.
    pub const PRESSURE: u32 = 2;
    /// `AXIS_TOUCH_MAJOR`.
    pub const TOUCH_MAJOR: u32 = 4;
    /// `AXIS_TOUCH_MINOR`.
    pub const TOUCH_MINOR: u32 = 5;
    /// `AXIS_ORIENTATION`.
    pub const ORIENTATION: u32 = 8;
    /// `AXIS_TILT`.
    pub const TILT: u32 = 25;
    /// Every axis the decoder asks a device's motion ranges about.
    pub const SENSORS: [u32; 5] = [PRESSURE, TOUCH_MAJOR, TOUCH_MINOR, TILT, ORIENTATION];
}

/// One pointer's axis values at one instant, as the event reports them.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Axes {
    /// `AXIS_X`, physical pixels.
    pub x: f32,
    /// `AXIS_Y`, physical pixels.
    pub y: f32,
    /// `AXIS_PRESSURE`.
    pub pressure: f32,
    /// `AXIS_TOUCH_MAJOR`, physical pixels.
    pub touch_major: f32,
    /// `AXIS_TOUCH_MINOR`, physical pixels.
    pub touch_minor: f32,
    /// `AXIS_TILT`, radians from the screen normal.
    pub tilt: f32,
    /// `AXIS_ORIENTATION`, radians clockwise from up.
    pub orientation: f32,
    /// `AXIS_HSCROLL`.
    pub hscroll: f32,
    /// `AXIS_VSCROLL`.
    pub vscroll: f32,
    /// `AXIS_GESTURE_SCROLL_X_DISTANCE`, physical pixels.
    pub gesture_scroll_x: f32,
    /// `AXIS_GESTURE_SCROLL_Y_DISTANCE`, physical pixels.
    pub gesture_scroll_y: f32,
    /// `AXIS_GESTURE_PINCH_SCALE_FACTOR`, the scale since the last event.
    pub pinch_scale: f32,
    /// `AXIS_GESTURE_SWIPE_FINGER_COUNT`.
    pub swipe_fingers: f32,
}

/// One pointer of an event: its id, tool, current axes and the historical
/// samples batched before them, oldest first.
#[derive(Debug, Clone, PartialEq)]
pub struct PointerFacts {
    /// `getPointerId`.
    pub id: i32,
    /// `getToolType`.
    pub tool: Tool,
    /// The current values.
    pub axes: Axes,
    /// `getHistorical*`, each with its time.
    pub history: Vec<(EventTime, Axes)>,
}

/// One `MotionEvent`, copied out of the NDK.
#[derive(Debug, Clone, PartialEq)]
pub struct Motion<'a> {
    /// `getDeviceId`.
    pub device: i32,
    /// `getSource`.
    pub source: u32,
    /// The action, without its pointer index.
    pub action: MotionAction,
    /// The index of the pointer the action is about.
    pub action_index: usize,
    /// `getActionButton`.
    pub action_button: u32,
    /// `getButtonState`.
    pub buttons: u32,
    /// `FLAG_CANCELED` (API 33): the pointer an `ACTION_POINTER_UP` lifts
    /// was cancelled (a rejected palm), not released.
    pub canceled: bool,
    /// `getMetaState`, as owned modifiers.
    pub modifiers: Modifiers,
    /// `getEventTime`; `None` when it is not a time.
    pub time: Option<EventTime>,
    /// Every pointer, in index order.
    pub pointers: &'a [PointerFacts],
}

/// The axes a device's motion ranges say it reports, for one source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceCapabilities {
    /// The source the ranges were read for.
    pub source: u32,
    /// One bit per axis id.
    pub axes: u64,
}

impl DeviceCapabilities {
    /// Ranges for `axes` of `source`.
    #[must_use]
    pub fn new(source: u32, axes: impl IntoIterator<Item = u32>) -> Self {
        Self {
            source,
            axes: axes
                .into_iter()
                .filter(|axis| *axis < 64)
                .fold(0, |bits, axis| bits | (1_u64 << axis)),
        }
    }

    /// Whether the device reports `axis`.
    #[must_use]
    pub const fn has(self, axis: u32) -> bool {
        axis < 64 && self.axes & (1_u64 << axis) != 0
    }
}

/// Whether an event of `source` doing `action` needs its device's Java query,
/// given the capabilities `cached` for its device id. Only moves (in contact
/// or hovering, the events at the sample rate) use the cache; every other
/// event queries again, so a device the OS removed or reconfigured is seen at
/// the press that starts a sequence and at the lift, exit or cancel that ends
/// one (a device reset cancels its open sequences). A source the cache was
/// not read for queries.
#[must_use]
pub fn needs_device_query(
    cached: Option<DeviceCapabilities>,
    source: u32,
    action: MotionAction,
) -> bool {
    match cached {
        Some(cached) if cached.source == source => {
            !matches!(action, MotionAction::Move | MotionAction::HoverMove)
        }
        _ => true,
    }
}

/// What the backend's Java query found about an event's device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceDiscovery {
    /// The device exists; `replaced` when the OS replaced its object since
    /// the last event (a reconfiguration), which ends its contacts.
    Present {
        /// Its motion ranges.
        capabilities: DeviceCapabilities,
        /// Whether it replaced the device the decoder knew by that id.
        replaced: bool,
    },
    /// No device has the id any longer.
    Removed,
    /// The query failed: the event is delivered without device or sensors.
    Unavailable,
}

/// An exit held until the next event shows whether it continues into a press.
#[derive(Debug, Clone, Copy)]
struct PendingExit {
    device: i32,
    time: EventTime,
    reading: NativeReading,
    held_at: Instant,
}

/// A touchpad gesture open on one device.
#[derive(Debug, Clone, Copy)]
enum TouchpadGesture {
    /// A pinch about `focal`, in physical pixels.
    Pinch { focal: (f64, f64) },
    /// A two-finger scroll from the cursor `cursor`.
    Scroll { cursor: NativeContact },
    /// A three- or four-finger swipe, not translated.
    Swipe,
}

/// Where a device's touchpad cursor last hovered.
#[derive(Debug, Clone, Copy)]
struct Cursor {
    contact: NativeContact,
    position: (f64, f64),
}

/// Input state for the Android window owner: contact and device identity
/// from the shared pointer assembly, and what the translation holds between
/// events.
#[derive(Debug)]
pub struct AndroidMotion {
    contacts: ContactRegistry,
    devices: HashMap<i32, DeviceCapabilities>,
    batch: MoveBatch,
    last_time: Option<EventTime>,
    pending_exit: Option<PendingExit>,
    gestures: HashMap<i32, TouchpadGesture>,
    cursors: HashMap<i32, Cursor>,
    scrolling: ScrollPhases,
}

impl Default for AndroidMotion {
    fn default() -> Self {
        Self::with_registry(ContactRegistry::new(NativeClock::epoch_only()))
    }
}

/// The label of a native device id, preserving its sign bits injectively.
#[must_use]
pub fn native_device(id: i32) -> NativeDevice {
    NativeDevice(u64::from(u32::from_ne_bytes(id.to_ne_bytes())))
}

/// Whether events from `source` with `tool` are a touchpad in pointer mode.
#[must_use]
pub const fn is_touchpad(source: u32, tool: Tool) -> bool {
    source & SOURCE_MOUSE == SOURCE_MOUSE && matches!(tool, Tool::Finger)
}

impl AndroidMotion {
    /// A decoder over `contacts`.
    #[must_use]
    pub fn with_registry(contacts: ContactRegistry) -> Self {
        Self {
            contacts,
            devices: HashMap::new(),
            batch: MoveBatch::new(),
            last_time: None,
            pending_exit: None,
            gestures: HashMap::new(),
            cursors: HashMap::new(),
            scrolling: ScrollPhases::default(),
        }
    }

    /// The registry the decoder assembles through.
    #[must_use]
    pub const fn contacts(&self) -> &ContactRegistry {
        &self.contacts
    }

    /// End every contact: the activity paused, lost focus or its window went
    /// away. A held exit is delivered first.
    pub fn cancel_contacts(&mut self, reason: NativeCancel) -> Vec<PlatformInput> {
        let mut output = self.release_exit();
        self.gestures.clear();
        self.scrolling.cancel_surface(SURFACE);
        if let Some(time) = self.last_time {
            output.extend(self.contacts.cancel_surface(SURFACE, time, reason));
        }
        output
    }

    /// At the end of an owner turn's input drain, `now`: deliver a held exit
    /// as a leave once no press has continued it for [`EXIT_GRACE`].
    pub fn finish_drain(&mut self, now: Instant) -> Vec<PlatformInput> {
        match self.pending_exit {
            Some(pending) if now.saturating_duration_since(pending.held_at) >= EXIT_GRACE => {
                self.release_exit()
            }
            _ => Vec::new(),
        }
    }

    /// Deliver the held exit, if any, as a leave.
    fn release_exit(&mut self) -> Vec<PlatformInput> {
        let Some(pending) = self.pending_exit.take() else {
            return Vec::new();
        };
        decode(&mut self.contacts, &pending.reading)
            .unwrap_or_else(DecodeError::into_inputs)
            .into_vec()
    }

    /// Track the device behind an event from its discovery. `None` when the
    /// event cannot be delivered (a removed device, or no device identity
    /// left); otherwise the device label its readings carry and its
    /// capabilities, both absent for a virtual device without an
    /// `InputDevice`.
    fn discover(
        &mut self,
        id: i32,
        discovery: DeviceDiscovery,
        time: EventTime,
        output: &mut Vec<PlatformInput>,
    ) -> Option<(Option<NativeDevice>, Option<DeviceCapabilities>)> {
        let native = native_device(id);
        let had_device = self.devices.contains_key(&id);
        let replacement = match discovery {
            DeviceDiscovery::Present { replaced, .. } => replaced,
            DeviceDiscovery::Removed => true,
            DeviceDiscovery::Unavailable => false,
        };
        if replacement && self.devices.remove(&id).is_some() {
            self.gestures.remove(&id);
            self.cursors.remove(&id);
            self.scrolling.remove_device(native);
            let (ended, removed) = self.contacts.remove_device(native, time);
            output.extend(ended.into_iter().map(|(_, input)| input));
            output.extend(
                removed.map(|change| PlatformInput::Pointer(PointerEvent::DeviceRemoved(change))),
            );
        }
        match discovery {
            DeviceDiscovery::Present { capabilities, .. } => {
                if self.devices.insert(id, capabilities).is_none() {
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
            DeviceDiscovery::Removed if had_device => None,
            // Android-generated and virtual events can have no InputDevice.
            // Preserve their contact and position, without device or sensors.
            DeviceDiscovery::Removed | DeviceDiscovery::Unavailable => Some((None, None)),
        }
    }

    /// The input `motion` delivers, at `scale` physical pixels per logical
    /// pixel, with wheel axes projected by `wheel`, received at `now`.
    pub fn decode(
        &mut self,
        motion: &Motion<'_>,
        scale: f64,
        wheel: &dyn Fn(f32, f32) -> Option<ScrollDelta>,
        discovery: DeviceDiscovery,
        now: Instant,
    ) -> Vec<PlatformInput> {
        if let Some(time) = motion.time {
            self.last_time = Some(time);
        }
        let Some(delivery_time) = motion.time.or(self.last_time) else {
            return Vec::new();
        };
        let mut output = Vec::new();
        if let Some(pending) = self.pending_exit.take() {
            let continues = pending.device == motion.device
                && motion.action == MotionAction::Down
                && motion.time == Some(pending.time);
            if !continues {
                output.extend(
                    decode(&mut self.contacts, &pending.reading)
                        .unwrap_or_else(DecodeError::into_inputs),
                );
            }
        }
        let Some((device, capabilities)) =
            self.discover(motion.device, discovery, delivery_time, &mut output)
        else {
            return output;
        };
        let event = Event {
            motion,
            device,
            capabilities,
            scale,
            time: delivery_time,
            received: now,
        };
        let touchpad = motion
            .pointers
            .first()
            .is_some_and(|pointer| is_touchpad(motion.source, pointer.tool));
        if touchpad && self.touchpad_gesture(&event, &mut output) {
            return output;
        }
        for (index, pointer) in motion.pointers.iter().enumerate() {
            if matches!(
                motion.action,
                MotionAction::Down
                    | MotionAction::PointerDown
                    | MotionAction::Up
                    | MotionAction::PointerUp
            ) && index != motion.action_index
            {
                continue;
            }
            self.pointer(&event, pointer, touchpad, wheel, &mut output);
        }
        output
    }

    /// Decode `pointer` of a pointer event; a touchpad's is its cursor.
    fn pointer(
        &mut self,
        event: &Event<'_, '_>,
        pointer: &PointerFacts,
        touchpad: bool,
        wheel: &dyn Fn(f32, f32) -> Option<ScrollDelta>,
        output: &mut Vec<PlatformInput>,
    ) {
        let motion = event.motion;
        let Ok(label) = u64::try_from(pointer.id) else {
            return;
        };
        let contact = NativeContact(label);
        let kind = if touchpad {
            PointerKind::Mouse
        } else {
            pointer_kind(pointer.tool)
        };
        let device = event.device;
        if touchpad
            && matches!(
                motion.action,
                MotionAction::HoverEnter | MotionAction::HoverMove | MotionAction::HoverExit
            )
        {
            self.cursors.insert(
                motion.device,
                Cursor {
                    contact,
                    position: (f64::from(pointer.axes.x), f64::from(pointer.axes.y)),
                },
            );
        }
        let in_contact = match motion.action {
            MotionAction::Down | MotionAction::PointerDown | MotionAction::Move => true,
            MotionAction::ButtonPress | MotionAction::ButtonRelease => self
                .contacts
                .buttons(SURFACE, device, contact)
                .is_some_and(|buttons| buttons.contains(PointerButton::PRIMARY)),
            _ => false,
        } && matches!(kind, PointerKind::Touch | PointerKind::Pen { .. });
        let buttons = native_buttons(motion.buttons, in_contact);
        let reading = |action, time: EventTime, axes: &Axes| {
            let mut reading = NativeReading::new(contact, kind, action, ReadingTime::epoch(time))
                .on(SURFACE)
                .at(f64::from(axes.x), f64::from(axes.y), event.scale)
                .with_buttons(buttons)
                .with_modifiers(motion.modifiers)
                .with_sensors(native_sensors(kind, event.capabilities, axes));
            if let Some(device) = device {
                reading = reading.from_device(device);
            }
            reading
        };
        let current = |action| reading(action, event.time, &pointer.axes);
        let open = self.contacts.pointer(SURFACE, device, contact).is_some();
        let assembled = match motion.action {
            MotionAction::Cancel => decode(
                &mut self.contacts,
                &current(ReadingAction::Cancel(NativeCancel::Platform)),
            ),
            // The system took the action pointer back (palm rejection): its
            // sequence ends without a lift a recogniser could count.
            MotionAction::Up | MotionAction::PointerUp if motion.canceled => decode(
                &mut self.contacts,
                &current(ReadingAction::Cancel(NativeCancel::Platform)),
            ),
            MotionAction::Down | MotionAction::PointerDown => {
                let button = if kind == PointerKind::Mouse {
                    buttons.iter().next().unwrap_or(PointerButton::PRIMARY)
                } else {
                    PointerButton::PRIMARY
                };
                decode(&mut self.contacts, &current(ReadingAction::Press(button)))
            }
            MotionAction::Up | MotionAction::PointerUp => {
                let button = if kind == PointerKind::Mouse {
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
                for (time, past) in &pointer.history {
                    self.batch
                        .push_coalesced(reading(ReadingAction::Move, *time, past));
                }
                self.batch
                    .decode(&mut self.contacts, &current(ReadingAction::Move))
            }
            MotionAction::ButtonPress | MotionAction::ButtonRelease => {
                let Some(button) = native_button(motion.action_button) else {
                    return;
                };
                decode(
                    &mut self.contacts,
                    &current(if motion.action == MotionAction::ButtonPress {
                        ReadingAction::Press(button)
                    } else {
                        ReadingAction::Release(button)
                    }),
                )
            }
            // The contact never left: Android re-enters after a press lifts.
            MotionAction::HoverEnter if open => {
                decode(&mut self.contacts, &current(ReadingAction::Move))
            }
            MotionAction::HoverEnter => decode(&mut self.contacts, &current(ReadingAction::Enter)),
            MotionAction::HoverExit => {
                self.pending_exit = Some(PendingExit {
                    device: motion.device,
                    time: event.time,
                    reading: current(ReadingAction::Leave),
                    held_at: event.received,
                });
                return;
            }
            MotionAction::Scroll => {
                // A wheel reports from where the cursor hovers: place it first.
                let mut placed = decode(&mut self.contacts, &current(ReadingAction::Move))
                    .unwrap_or_else(DecodeError::into_inputs);
                if let Some(delta) = wheel(pointer.axes.hscroll, pointer.axes.vscroll) {
                    placed.extend(self.contacts.scroll(&ContactScroll {
                        surface: SURFACE,
                        device,
                        contact,
                        phase: None,
                        delta,
                        precision: ScrollPrecision::Unknown,
                        time: event.time,
                        modifiers: motion.modifiers,
                    }));
                }
                Ok(placed)
            }
            MotionAction::Other => return,
        };
        output.extend(assembled.unwrap_or_else(DecodeError::into_inputs));
    }

    /// Decode a touchpad gesture's fake fingers; `false` when the event is
    /// the touchpad's cursor instead.
    fn touchpad_gesture(&mut self, event: &Event<'_, '_>, output: &mut Vec<PlatformInput>) -> bool {
        let motion = event.motion;
        let Some(first) = motion.pointers.first() else {
            return false;
        };
        let open = self.gestures.get(&motion.device).copied();
        let gesture = match (motion.action, open) {
            // A click presses a button; a gesture presses none.
            (MotionAction::Down, None) if motion.buttons == 0 => {
                let gesture = if first.axes.swipe_fingers > 0.0 {
                    TouchpadGesture::Swipe
                } else if first.axes.pinch_scale > 0.0 {
                    let focal = self.cursors.get(&motion.device).map_or(
                        (f64::from(first.axes.x), f64::from(first.axes.y)),
                        |cursor| cursor.position,
                    );
                    TouchpadGesture::Pinch { focal }
                } else {
                    let cursor = self.cursor(event, first);
                    TouchpadGesture::Scroll { cursor }
                };
                self.gestures.insert(motion.device, gesture);
                gesture
            }
            (
                MotionAction::Move
                | MotionAction::PointerDown
                | MotionAction::PointerUp
                | MotionAction::Up
                | MotionAction::Cancel,
                Some(gesture),
            ) => gesture,
            _ => return false,
        };
        let phase = match motion.action {
            MotionAction::Down => GesturePhase::Began,
            MotionAction::Move => GesturePhase::Changed,
            MotionAction::Up if motion.canceled => GesturePhase::Cancelled,
            MotionAction::Up => GesturePhase::Ended,
            MotionAction::Cancel => GesturePhase::Cancelled,
            // The second finger of a pinch comes and goes inside it.
            _ => return true,
        };
        if matches!(phase, GesturePhase::Ended | GesturePhase::Cancelled) {
            self.gestures.remove(&motion.device);
        }
        match gesture {
            TouchpadGesture::Pinch { focal } => {
                let factor = f64::from(first.axes.pinch_scale);
                let delta = match phase {
                    GesturePhase::Began | GesturePhase::Changed
                        if factor.is_finite() && factor > 0.0 =>
                    {
                        GestureDelta::Scale(factor)
                    }
                    _ => GestureDelta::None,
                };
                let mut reading = NativeReading::new(
                    PINCH,
                    PointerKind::Trackpad,
                    ReadingAction::Gesture(GestureReading {
                        component: GestureComponent::Scale,
                        phase,
                        delta,
                    }),
                    ReadingTime::epoch(event.time),
                )
                .on(SURFACE)
                .at(focal.0, focal.1, event.scale)
                .with_primary(true)
                .with_modifiers(motion.modifiers);
                if let Some(device) = event.device {
                    reading = reading.from_device(device);
                }
                output.extend(
                    decode(&mut self.contacts, &reading).unwrap_or_else(DecodeError::into_inputs),
                );
            }
            TouchpadGesture::Scroll { cursor } => {
                let Some(owned) = self.scrolling.phase(
                    SURFACE,
                    event.device,
                    ScrollReading::fingers(phase, true),
                ) else {
                    return true;
                };
                let (x, y) = if owned == ScrollPhase::Changed {
                    (
                        f64::from(first.axes.gesture_scroll_x) / event.scale,
                        f64::from(first.axes.gesture_scroll_y) / event.scale,
                    )
                } else {
                    (0.0, 0.0)
                };
                let Ok(delta) = ScrollDelta::try_new(ScrollUnit::Pixels, x, y) else {
                    return true;
                };
                output.extend(self.contacts.scroll(&ContactScroll {
                    surface: SURFACE,
                    device: event.device,
                    contact: cursor,
                    phase: Some(owned),
                    delta,
                    precision: ScrollPrecision::Precise,
                    time: event.time,
                    modifiers: motion.modifiers,
                }));
            }
            TouchpadGesture::Swipe => {}
        }
        true
    }

    /// The touchpad cursor a scroll starts from, placed where it last hovered
    /// when its contact is no longer open.
    fn cursor(&mut self, event: &Event<'_, '_>, first: &PointerFacts) -> NativeContact {
        let motion = event.motion;
        let cursor = self.cursors.get(&motion.device).copied().unwrap_or(Cursor {
            contact: NativeContact(0),
            position: (f64::from(first.axes.x), f64::from(first.axes.y)),
        });
        if self
            .contacts
            .pointer(SURFACE, event.device, cursor.contact)
            .is_none()
        {
            let mut reading = NativeReading::new(
                cursor.contact,
                PointerKind::Mouse,
                ReadingAction::Move,
                ReadingTime::epoch(event.time),
            )
            .on(SURFACE)
            .at(cursor.position.0, cursor.position.1, event.scale)
            .with_buttons(PointerButtons::NONE)
            .with_modifiers(motion.modifiers);
            if let Some(device) = event.device {
                reading = reading.from_device(device);
            }
            // Without its cursor the scroll has no position and delivers nothing.
            let _ = decode(&mut self.contacts, &reading);
        }
        cursor.contact
    }
}

/// What every reading of one event shares.
struct Event<'motion, 'pointers> {
    motion: &'motion Motion<'pointers>,
    device: Option<NativeDevice>,
    capabilities: Option<DeviceCapabilities>,
    scale: f64,
    time: EventTime,
    received: Instant,
}

/// The owned kind of a tool.
#[must_use]
pub const fn pointer_kind(tool: Tool) -> PointerKind {
    match tool {
        Tool::Finger => PointerKind::Touch,
        Tool::Stylus => PointerKind::Pen { tool: PenTool::Tip },
        Tool::Eraser => PointerKind::Pen {
            tool: PenTool::Eraser,
        },
        Tool::Mouse => PointerKind::Mouse,
        Tool::Unknown => PointerKind::Unknown,
    }
}

/// The owned button of a native button bit.
#[must_use]
pub const fn native_button(bit: u32) -> Option<PointerButton> {
    match bit {
        button::PRIMARY => Some(PointerButton::PRIMARY),
        button::SECONDARY | button::STYLUS_PRIMARY => Some(PointerButton::SECONDARY),
        button::TERTIARY | button::STYLUS_SECONDARY => Some(PointerButton::AUXILIARY),
        button::BACK => Some(PointerButton::BACK),
        button::FORWARD => Some(PointerButton::FORWARD),
        _ => None,
    }
}

/// The owned buttons of a native button state; a touching contact also holds
/// the primary button.
#[must_use]
pub fn native_buttons(state: u32, contact: bool) -> PointerButtons {
    let mut buttons: PointerButtons = [
        button::PRIMARY,
        button::SECONDARY,
        button::TERTIARY,
        button::BACK,
        button::FORWARD,
        button::STYLUS_PRIMARY,
        button::STYLUS_SECONDARY,
    ]
    .into_iter()
    .filter(|bit| state & bit != 0)
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
#[must_use]
pub fn pen_angles(tilt: Option<f64>, orientation: Option<f64>) -> Option<PenAngles> {
    let altitude = tilt
        .filter(|tilt| (0.0..=f64::from(core::f32::consts::FRAC_PI_2)).contains(tilt))
        .map(|tilt| core::f64::consts::FRAC_PI_2 - tilt.min(core::f64::consts::FRAC_PI_2));
    let azimuth = orientation
        .filter(|orientation| orientation.is_finite())
        .map(|orientation| orientation - core::f64::consts::FRAC_PI_2);
    (altitude.is_some() || azimuth.is_some()).then_some(PenAngles::Spherical { altitude, azimuth })
}

/// The sensors a device's motion ranges say it reports, read from `axes`.
/// Contact axes stay in physical pixels; assembly divides them by the scale.
#[must_use]
pub fn native_sensors(
    kind: PointerKind,
    capabilities: Option<DeviceCapabilities>,
    axes: &Axes,
) -> RawSensors {
    let mut sensors = RawSensors::default();
    let Some(capabilities) = capabilities else {
        return sensors;
    };
    if matches!(kind, PointerKind::Touch | PointerKind::Pen { .. }) {
        if capabilities.has(axis::PRESSURE) {
            // Android pressure may overshoot one by calibration: clamp it.
            let pressure = f64::from(axes.pressure);
            sensors.pressure = pressure.is_finite().then(|| pressure.clamp(0.0, 1.0));
        }
        if capabilities.has(axis::TOUCH_MAJOR) && capabilities.has(axis::TOUCH_MINOR) {
            let (major, minor) = (f64::from(axes.touch_major), f64::from(axes.touch_minor));
            if major.is_finite() && minor.is_finite() && major >= 0.0 && minor >= 0.0 {
                sensors.contact_size = Some((major, minor));
            }
        }
    }
    if matches!(kind, PointerKind::Pen { .. }) {
        sensors.angles = pen_angles(
            capabilities.has(axis::TILT).then(|| f64::from(axes.tilt)),
            capabilities
                .has(axis::ORIENTATION)
                .then(|| f64::from(axes.orientation)),
        );
    }
    sensors
}
