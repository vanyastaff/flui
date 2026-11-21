//! XInput 2 events and devices as plain data, and their assembly.

#![forbid(unsafe_code)]

use std::collections::{HashMap, HashSet};
use std::num::NonZeroU8;

use smallvec::SmallVec;

use flui_foundation::geometry::DevicePixelRatio;
use flui_platform::EventTime;
use flui_platform::PlatformInput;
use flui_platform::input::capabilities::PointerKindSet;
use flui_platform::input::keyboard::Modifiers;
use flui_platform::input::pointer::{
    MotionAcceleration, MotionDelta, PenTool, PointerButton, PointerEvent, PointerKind,
    PointerMove, ScrollDelta, ScrollInversion, ScrollPrecision, ScrollUnit,
};

use crate::input::assembly::{
    ContactRegistry, ContactScroll, DecodeError, GestureComponent, GestureDelta, GesturePhase,
    GestureReading, MoveBatch, NativeCancel, NativeClock, NativeContact, NativeDevice,
    NativeReading, NativeSurface, NativeTime, PenAngles, RawSensors, ReadingAction, ReadingTime,
    SwipeReading, decode,
};
use crate::input::linux::xkb::{XkbModifier, modifiers_of};
use crate::input::scroll::{WheelAmount, wheel_step};

// The contact labels of the devices that are not touch screens. An
// XInput 2 touch id is 32 bits, so a touch never reaches them; the
// pinch's label is the one every backend gives a trackpad gesture.

/// The cursor of a master pointer on a window.
pub const CURSOR: NativeContact = NativeContact(u64::MAX);
/// A touchpad pinch of a master pointer.
pub const PINCH: NativeContact = NativeContact(u64::MAX - 1);
/// A pen on a window.
pub const PEN: NativeContact = NativeContact(u64::MAX - 2);
/// A touchpad swipe of a master pointer.
pub const SWIPE: NativeContact = NativeContact(u64::MAX - 3);

/// A master device's XInput 2 id, which the server gives another device
/// once this one is gone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct XiMaster(pub u16);

/// A slave (physical) device's XInput 2 id, which the server gives
/// another device once this one is gone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct XiSlave(pub u16);

/// How far the server's time may stand from the monotonic clock and still
/// be read as it: a local server keeps that clock; a remote one does not.
pub const SERVER_TIME_TRUSTED_WITHIN_MS: u32 = 1_000;

/// When an event happened: the server's time on it, and the monotonic
/// clock and the event timeline read as it arrived.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct XiTime {
    /// The event's `time`, milliseconds of the server's clock, wrapping.
    pub server_ms: u32,
    /// The monotonic clock in milliseconds as the event arrived, the
    /// clock the loop's [`NativeClock`] is anchored on.
    pub observed_ms: u64,
    /// The event timeline as the event arrived.
    pub arrival: EventTime,
}

impl XiTime {
    /// The reading's time: the server's while it is within
    /// [`SERVER_TIME_TRUSTED_WITHIN_MS`] of the monotonic clock, placed
    /// across the 49.7-day wrap; the arrival otherwise.
    #[must_use]
    pub fn reading_time(self) -> ReadingTime {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "the server's time is the low 32 bits of a millisecond clock"
        )]
        let observed = self.observed_ms as u32;
        let apart = self
            .server_ms
            .wrapping_sub(observed)
            .min(observed.wrapping_sub(self.server_ms));
        if apart <= SERVER_TIME_TRUSTED_WITHIN_MS {
            ReadingTime::coarse(NativeTime::TickMillis {
                tick: self.server_ms,
                observed_ms: self.observed_ms,
            })
        } else {
            ReadingTime::epoch(self.arrival)
        }
    }
}

/// What an XInput 2 event says happened.
#[derive(Debug, Clone, Copy, PartialEq)]
#[expect(
    clippy::exhaustive_enums,
    reason = "a mirror of the protocol's events, decoded arm by arm, and a `_` arm would drop input"
)]
pub enum XiAction {
    /// `XI_Motion`.
    Motion,
    /// `XI_ButtonPress`.
    ButtonPress,
    /// `XI_ButtonRelease`.
    ButtonRelease,
    /// `XI_Enter`.
    Enter,
    /// `XI_Leave`; `grabbed` when a grab took the pointer away
    /// (`XINotifyGrab`).
    Leave {
        /// The leave is a grab's.
        grabbed: bool,
    },
    /// `XI_TouchBegin`.
    TouchBegin,
    /// `XI_TouchUpdate`.
    TouchUpdate,
    /// `XI_TouchEnd`.
    TouchEnd,
    /// `XI_GesturePinchBegin`.
    PinchBegin,
    /// `XI_GesturePinchUpdate`.
    PinchUpdate {
        /// The scale since the pinch began.
        scale: f64,
        /// The rotation since the last update, in degrees, clockwise.
        rotation: f64,
    },
    /// `XI_GesturePinchEnd`; `cancelled` when the gesture was
    /// (`XIGesturePinchEventCancelled`).
    PinchEnd {
        /// The pinch was cancelled.
        cancelled: bool,
    },
    /// `XI_GestureSwipeBegin`; the event's `detail` is the finger count.
    SwipeBegin,
    /// `XI_GestureSwipeUpdate`.
    SwipeUpdate {
        /// The fingers' accelerated step since the last event, in device
        /// pixels.
        dx: f64,
        /// The vertical step.
        dy: f64,
    },
    /// `XI_GestureSwipeEnd`; `cancelled` when the gesture was
    /// (`XIGestureSwipeEventCancelled`).
    SwipeEnd {
        /// The swipe was cancelled.
        cancelled: bool,
    },
}

/// One XInput 2 pointer, touch or gesture event, copied out of the
/// protocol.
#[derive(Debug, Clone, PartialEq)]
pub struct XiEvent {
    /// What happened.
    pub action: XiAction,
    /// The master device it came through.
    pub master: XiMaster,
    /// The physical device that produced it.
    pub source: XiSlave,
    /// The button of a press or release; the touch id of a touch.
    pub detail: u32,
    /// Where, in device pixels of the window.
    pub position: (f64, f64),
    /// The server emulated it from other input (`XIPointerEmulated`).
    pub emulated: bool,
    /// The X11 modifier mask in effect.
    pub modifiers: u32,
    /// The valuators it carries: number and value.
    pub valuators: Vec<(u16, f64)>,
    /// When it happened.
    pub time: XiTime,
}

/// The modifiers of an X11 modifier mask (`ShiftMask` … `Mod5Mask`), as
/// XInput 2 events carry them, by the XKB modifier each bit is.
#[must_use]
pub fn modifiers_of_mask(mask: u32) -> Modifiers {
    modifiers_of(|modifier| {
        let bit = match modifier {
            XkbModifier::Shift => 0x01,
            XkbModifier::Lock => 0x02,
            XkbModifier::Control => 0x04,
            XkbModifier::Mod1 => 0x08,
            XkbModifier::Mod2 => 0x10,
            XkbModifier::Mod4 => 0x40,
            XkbModifier::Mod5 => 0x80,
        };
        mask & bit != 0
    })
}

/// What a valuator measures, as its label atom names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(clippy::exhaustive_enums, reason = "the open end is Other")]
pub enum AxisLabel {
    /// `Abs Pressure`.
    Pressure,
    /// `Abs Tilt X`.
    TiltX,
    /// `Abs Tilt Y`.
    TiltY,
    /// Anything else.
    Other,
}

/// A valuator of a device.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Axis {
    /// Its number in events.
    pub number: u16,
    /// What it measures.
    pub label: AxisLabel,
    /// The least value it reports.
    pub min: f64,
    /// The greatest value it reports.
    pub max: f64,
}

/// A valuator that scrolls.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollAxis {
    /// Its number in events.
    pub number: u16,
    /// It scrolls horizontally.
    pub horizontal: bool,
    /// The change that is one wheel detent; a positive change scrolls
    /// down or right.
    pub increment: f64,
}

/// An XInput 2 device as the server lists it (`XIQueryDevice`).
#[derive(Debug, Clone, PartialEq)]
pub struct DeviceDescription {
    /// The device's id.
    pub id: u16,
    /// It is a master pointer.
    pub master: bool,
    /// It is a slave pointer.
    pub pointer: bool,
    /// Its name.
    pub name: String,
    /// It is a touch screen (`XIDirectTouch`).
    pub direct_touch: bool,
    /// Its valuators.
    pub axes: Vec<Axis>,
    /// Its scroll valuators.
    pub scrolls: Vec<ScrollAxis>,
}

/// One device of a hierarchy change, as the server sends it
/// (`XIHierarchyInfo`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HierarchyInfo {
    /// The device's id.
    pub id: u16,
    /// What the device is to the server (`XIMasterPointer` to
    /// `XIFloatingSlave`). A device the change removed has none: the
    /// server sends 0 for it, so what it was is read from the flags.
    pub device_use: u16,
    /// What happened to it (`XIMasterAdded` to `XIDeviceDisabled`).
    pub flags: u32,
}

/// Which kind of device a hierarchy change takes away.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Gone {
    Master,
    Slave,
}

impl HierarchyInfo {
    /// `XIMasterAdded`.
    pub const MASTER_ADDED: u32 = 1 << 0;
    /// `XIMasterRemoved`.
    pub const MASTER_REMOVED: u32 = 1 << 1;
    /// `XISlaveAdded`.
    pub const SLAVE_ADDED: u32 = 1 << 2;
    /// `XISlaveRemoved`.
    pub const SLAVE_REMOVED: u32 = 1 << 3;
    /// `XISlaveAttached`.
    pub const SLAVE_ATTACHED: u32 = 1 << 4;
    /// `XIDeviceEnabled`.
    pub const DEVICE_ENABLED: u32 = 1 << 6;
    /// `XIDeviceDisabled`.
    pub const DEVICE_DISABLED: u32 = 1 << 7;
    /// `XIMasterPointer`, the use of a master pointer.
    pub const MASTER_POINTER: u16 = 1;

    /// What the change takes away, if it takes this device: a removal
    /// says which kind in its flag, and a disabled device, which the
    /// server still lists, in its use.
    fn gone(self) -> Option<Gone> {
        if self.flags & Self::MASTER_REMOVED != 0 {
            Some(Gone::Master)
        } else if self.flags & Self::SLAVE_REMOVED != 0 {
            Some(Gone::Slave)
        } else if self.flags & Self::DEVICE_DISABLED != 0 {
            Some(if self.device_use == Self::MASTER_POINTER {
                Gone::Master
            } else {
                Gone::Slave
            })
        } else {
            None
        }
    }

    /// Whether the change brings this device: added, enabled or attached,
    /// and not taken away by the same change.
    fn arrived(self) -> bool {
        let brought =
            Self::MASTER_ADDED | Self::SLAVE_ADDED | Self::DEVICE_ENABLED | Self::SLAVE_ATTACHED;
        self.flags & brought != 0 && self.gone().is_none()
    }
}

/// What a slave pointer is to FLUI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    clippy::exhaustive_enums,
    reason = "assembly handles each role, and a `_` arm would drop the device's input"
)]
pub enum DeviceRole {
    /// It moves its master's cursor: a mouse, a touchpad, XTest.
    Cursor,
    /// A touch screen.
    Touchscreen,
    /// A pen with this tool.
    Pen(PenTool),
}

/// The role of a slave pointer, from its touch class, its name (the
/// tablet drivers name the stylus and the eraser) and its valuators.
#[must_use]
pub fn role(device: &DeviceDescription) -> DeviceRole {
    if device.direct_touch {
        return DeviceRole::Touchscreen;
    }
    let name = device.name.to_lowercase();
    if name.contains("eraser") {
        return DeviceRole::Pen(PenTool::Eraser);
    }
    let has = |label| device.axes.iter().any(|axis| axis.label == label);
    let named_pen = name.contains("stylus") || name.contains(" pen");
    if named_pen || (has(AxisLabel::Pressure) && has(AxisLabel::TiltX)) {
        return DeviceRole::Pen(PenTool::Tip);
    }
    DeviceRole::Cursor
}

/// The contact an event moves: its device's label, the contact's and its
/// kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Target {
    /// The device's label.
    pub device: NativeDevice,
    /// The contact's label.
    pub contact: NativeContact,
    /// What kind of pointer it is.
    pub kind: PointerKind,
}

/// A slave pointer the decoder knows.
#[derive(Debug, Clone)]
struct Known {
    role: DeviceRole,
    /// Its label, for a touch screen or a pen; a cursor's contacts are
    /// its master's.
    label: Option<NativeDevice>,
    axes: Vec<Axis>,
    scrolls: Vec<ScrollAxis>,
}

/// A pinch in progress.
#[derive(Debug, Clone, Copy)]
struct Pinch {
    surface: NativeSurface,
    /// The scale since it began, as the last update said.
    scale: f64,
}

/// A swipe in progress.
#[derive(Debug, Clone, Copy)]
struct OpenSwipe {
    surface: NativeSurface,
    fingers: NonZeroU8,
}

/// One master pointer: its labels, the contact it moves now and where,
/// and its pinch or swipe.
#[derive(Debug, Clone, Copy)]
struct Master {
    label: NativeDevice,
    /// The label of its touchpad's gestures, from its first pinch or swipe.
    gestures: Option<NativeDevice>,
    /// The contact its last event moved, the cursor or a pen, and the
    /// window it moved it on.
    current: Option<(NativeSurface, Target)>,
    pinch: Option<Pinch>,
    swipe: Option<OpenSwipe>,
}

/// What one event delivers to its window.
#[derive(Debug, Default)]
pub struct Decoded {
    /// The input, in order.
    pub inputs: Vec<PlatformInput>,
    /// The pointer entered (`true`) or left (`false`) the window.
    pub hover: Option<bool>,
}

/// What a change of the devices delivers: each input with the window it
/// belongs to, or none for every window.
pub type DeviceChanges = Vec<(Option<NativeSurface>, PlatformInput)>;

/// The pointer devices, masters and slaves, the decoder knows. A machine
/// has a handful and a multi-seat rig a few dozen; past this the server
/// is listing devices without end, and the further ones are not known.
const MAX_DEVICES: usize = 128;

/// The valuators of a device the decoder reads. Xorg gives a device at
/// most 36 (`MAX_VALUATORS`); further ones are not read, so what each
/// scroll valuator last reported stays bounded too.
const MAX_VALUATORS: usize = 36;

/// The first [`MAX_VALUATORS`] of `listed`.
fn first_valuators<T: Clone>(listed: &[T]) -> Vec<T> {
    listed[..listed.len().min(MAX_VALUATORS)].to_vec()
}

/// The X11 host's pointers: the shared registry, the devices the server
/// lists and what each scroll valuator last reported.
#[derive(Debug)]
pub struct X11Pointers {
    contacts: ContactRegistry,
    devices: HashMap<XiSlave, Known>,
    masters: HashMap<XiMaster, Master>,
    /// The last value of each scroll valuator, by device and number.
    scrolled: HashMap<(XiSlave, u16), f64>,
    /// The hovering contacts inside each window: entered and not left.
    inside: HashSet<(NativeSurface, NativeDevice, NativeContact)>,
    next_label: u64,
}

impl X11Pointers {
    /// No device known yet; readings are placed on the timeline by
    /// `clock`, anchored on the monotonic clock the server's time keeps.
    #[must_use]
    pub fn new(clock: NativeClock) -> Self {
        Self {
            contacts: ContactRegistry::new(clock),
            devices: HashMap::new(),
            masters: HashMap::new(),
            scrolled: HashMap::new(),
            inside: HashSet::new(),
            next_label: 0,
        }
    }

    /// The device kinds present.
    #[must_use]
    pub fn present(&self) -> PointerKindSet {
        self.contacts.devices().present()
    }

    fn mint(&mut self) -> NativeDevice {
        self.next_label += 1;
        NativeDevice(self.next_label)
    }

    /// `slaves` and `masters` are gone: each ends its contacts and is
    /// removed, so an id the server lists again is a new device.
    fn removed(
        &mut self,
        slaves: &[XiSlave],
        masters: &[XiMaster],
        time: EventTime,
    ) -> DeviceChanges {
        let mut out = Vec::new();
        for slave in slaves {
            if let Some(known) = self.devices.remove(slave) {
                self.reset_scrolling(*slave);
                if let Some(label) = known.label {
                    out.extend(self.remove(label, time));
                }
            }
        }
        for master in masters {
            if let Some(gone) = self.masters.remove(master) {
                for label in [Some(gone.label), gone.gestures].into_iter().flatten() {
                    out.extend(self.remove(label, time));
                }
            }
        }
        out
    }

    /// The server lists `devices` now: a device it no longer lists, or
    /// lists in another role, is removed, its contacts cancelled; a new
    /// one is announced.
    pub fn devices(&mut self, devices: &[DeviceDescription], time: EventTime) -> DeviceChanges {
        let listed = |id: u16| devices.iter().find(|device| device.id == id);
        let gone_slaves: Vec<XiSlave> = self
            .devices
            .iter()
            .filter(|(slave, known)| {
                listed(slave.0)
                    .filter(|device| device.pointer)
                    .is_none_or(|device| role(device) != known.role)
            })
            .map(|(slave, _)| *slave)
            .collect();
        let gone_masters: Vec<XiMaster> = self
            .masters
            .keys()
            .filter(|master| !listed(master.0).is_some_and(|device| device.master))
            .copied()
            .collect();
        let mut out = self.removed(&gone_slaves, &gone_masters, time);
        for device in devices {
            self.admit(device, time, &mut out);
        }
        out
    }

    /// A hierarchy change, as the server sent it: what it removes or
    /// disables goes first, then each device it adds, enables or attaches
    /// is described by `query`, which asks the server for that one
    /// device; `None` when it lists no enabled pointer by that id.
    ///
    /// A change asks at most `MAX_DEVICES` (128) times however many devices
    /// it names, and never for a device it could not know: once
    /// `MAX_DEVICES` are known, only those are asked about again.
    pub fn hierarchy(
        &mut self,
        change: &[HierarchyInfo],
        mut query: impl FnMut(u16) -> Option<DeviceDescription>,
        time: EventTime,
    ) -> DeviceChanges {
        let gone = |wanted: Gone| {
            change
                .iter()
                .filter(move |info| info.gone() == Some(wanted))
        };
        let slaves: Vec<XiSlave> = gone(Gone::Slave).map(|info| XiSlave(info.id)).collect();
        let masters: Vec<XiMaster> = gone(Gone::Master).map(|info| XiMaster(info.id)).collect();
        let mut out = self.removed(&slaves, &masters, time);
        let mut asked = 0;
        for info in change.iter().filter(|info| info.arrived()) {
            if asked == MAX_DEVICES {
                break;
            }
            let known = self.devices.contains_key(&XiSlave(info.id))
                || self.masters.contains_key(&XiMaster(info.id));
            if !known && self.is_full() {
                continue;
            }
            asked += 1;
            if let Some(device) = query(info.id) {
                out.extend(self.described(&device, time));
            }
        }
        out
    }

    /// The server describes `device` now: a new one is announced, a known
    /// one takes its classes, and one known in another role, or no longer
    /// a pointer, is removed first.
    pub fn described(&mut self, device: &DeviceDescription, time: EventTime) -> DeviceChanges {
        let slave = XiSlave(device.id);
        let mut out = Vec::new();
        if self
            .devices
            .get(&slave)
            .is_some_and(|known| !device.pointer || role(device) != known.role)
        {
            out.extend(self.removed(&[slave], &[], time));
        }
        self.admit(device, time, &mut out);
        out
    }

    /// A device's classes changed (`XIDeviceChanged`): a known slave reads
    /// its valuators and scroll valuators from what the event carries,
    /// and its scrolling starts over.
    pub fn classes_changed(&mut self, slave: XiSlave, axes: &[Axis], scrolls: &[ScrollAxis]) {
        if let Some(known) = self.devices.get_mut(&slave) {
            known.axes = first_valuators(axes);
            known.scrolls = first_valuators(scrolls);
            self.reset_scrolling(slave);
        }
    }

    /// Know `device`, as the server describes it, announcing it if new.
    fn admit(&mut self, device: &DeviceDescription, time: EventTime, out: &mut DeviceChanges) {
        if device.master {
            let master = XiMaster(device.id);
            if self.masters.contains_key(&master) || self.is_full() {
                return;
            }
            let label = self.mint();
            self.masters.insert(
                master,
                Master {
                    label,
                    gestures: None,
                    current: None,
                    pinch: None,
                    swipe: None,
                },
            );
            if let Some(change) = self.contacts.add_device(label, PointerKind::Mouse, time) {
                out.push((
                    None,
                    PlatformInput::Pointer(PointerEvent::DeviceAdded(change)),
                ));
            }
            return;
        }
        if !device.pointer {
            return;
        }
        let slave = XiSlave(device.id);
        if let Some(known) = self.devices.get_mut(&slave) {
            // The same device: its classes may have changed.
            known.axes = first_valuators(&device.axes);
            known.scrolls = first_valuators(&device.scrolls);
            return;
        }
        if self.is_full() {
            return;
        }
        let role = role(device);
        let kind = match role {
            DeviceRole::Cursor => None,
            DeviceRole::Touchscreen => Some(PointerKind::Touch),
            DeviceRole::Pen(tool) => Some(PointerKind::Pen { tool }),
        };
        let label = kind.map(|_| self.mint());
        if let (Some(label), Some(kind)) = (label, kind)
            && let Some(change) = self.contacts.add_device(label, kind, time)
        {
            out.push((
                None,
                PlatformInput::Pointer(PointerEvent::DeviceAdded(change)),
            ));
        }
        self.devices.insert(
            slave,
            Known {
                role,
                label,
                axes: first_valuators(&device.axes),
                scrolls: first_valuators(&device.scrolls),
            },
        );
    }

    /// Whether it knows [`MAX_DEVICES`] devices, and takes no more.
    fn is_full(&self) -> bool {
        self.devices.len() + self.masters.len() >= MAX_DEVICES
    }

    fn remove(&mut self, label: NativeDevice, time: EventTime) -> DeviceChanges {
        for master in self.masters.values_mut() {
            if master
                .current
                .is_some_and(|(_, current)| current.device == label)
            {
                master.current = None;
            }
        }
        self.inside.retain(|(_, device, _)| *device != label);
        let (ended, removed) = self.contacts.remove_device(label, time);
        let mut out: DeviceChanges = ended
            .into_iter()
            .map(|(surface, input)| (Some(surface), input))
            .collect();
        if let Some(change) = removed {
            out.push((
                None,
                PlatformInput::Pointer(PointerEvent::DeviceRemoved(change)),
            ));
        }
        out
    }

    /// A slave device changed: the values its scroll valuators report
    /// start over.
    pub fn reset_scrolling(&mut self, source: XiSlave) {
        self.scrolled.retain(|(device, _), _| *device != source);
    }

    /// Whether `event` is a motion of its master's cursor (a mouse's or a
    /// touchpad's), not a pen's or a touch's.
    #[must_use]
    pub fn moves_cursor(&self, event: &XiEvent) -> bool {
        matches!(event.action, XiAction::Motion)
            && self
                .target_of(event)
                .is_some_and(|target| target.contact == CURSOR)
    }

    /// The cursor of master `master` held in place on the window
    /// `surface` moved by `delta`, the device's own counts (`XI_RawMotion`'s
    /// unaccelerated valuators) from slave `source`, at `time`: its raw
    /// motion, with the identity and resting place of the cursor's
    /// contact there, or of the reserved pointer for a master not known.
    /// `None` from a pen or a touch screen, whose valuators are
    /// positions, and for a time off the event timeline.
    #[must_use]
    pub fn raw_motion(
        &self,
        surface: NativeSurface,
        master: Option<XiMaster>,
        source: XiSlave,
        time: XiTime,
        delta: MotionDelta,
    ) -> Option<PlatformInput> {
        if self
            .devices
            .get(&source)
            .is_some_and(|known| known.role != DeviceRole::Cursor)
        {
            return None;
        }
        let device = master
            .and_then(|master| self.masters.get(&master))
            .map(|master| master.label);
        self.contacts.raw_motion(
            surface,
            device,
            CURSOR,
            time.reading_time(),
            delta,
            MotionAcceleration::None,
        )
    }

    /// End every contact on `surface` with `reason`; what moves on
    /// another window is left as it is.
    pub fn cancel_surface(
        &mut self,
        surface: NativeSurface,
        reason: NativeCancel,
        time: EventTime,
    ) -> Vec<PlatformInput> {
        for master in self.masters.values_mut() {
            if master.current.is_some_and(|(on, _)| on == surface) {
                master.current = None;
            }
            if master.swipe.is_some_and(|swipe| swipe.surface == surface) {
                master.swipe = None;
            }
            if master.pinch.is_some_and(|pinch| pinch.surface == surface) {
                master.pinch = None;
            }
        }
        self.inside.retain(|(inside, _, _)| *inside != surface);
        self.contacts.cancel_surface(surface, time, reason)
    }

    /// The contact `event` moves: a pen's own, a touch, or its master's
    /// cursor. `None` for a device the server has not listed, and for a
    /// touch screen's crossing, which is no contact.
    fn target_of(&self, event: &XiEvent) -> Option<Target> {
        let known = self.devices.get(&event.source);
        match known.map(|known| (known.role, known.label)) {
            Some((DeviceRole::Pen(tool), Some(device))) => Some(Target {
                device,
                contact: PEN,
                kind: PointerKind::Pen { tool },
            }),
            Some((DeviceRole::Touchscreen, Some(device))) => matches!(
                event.action,
                XiAction::TouchBegin | XiAction::TouchUpdate | XiAction::TouchEnd
            )
            .then_some(Target {
                device,
                contact: NativeContact(u64::from(event.detail)),
                kind: PointerKind::Touch,
            }),
            _ => {
                let master = self.masters.get(&event.master)?;
                Some(Target {
                    device: master.label,
                    contact: CURSOR,
                    kind: PointerKind::Mouse,
                })
            }
        }
    }

    /// The pen's sensors in `event`, from the source's valuators.
    fn sensors(&self, event: &XiEvent) -> RawSensors {
        let Some(known) = self.devices.get(&event.source) else {
            return RawSensors::default();
        };
        if !matches!(known.role, DeviceRole::Pen(_)) {
            return RawSensors::default();
        }
        let value = |label| {
            let axis = known.axes.iter().find(|axis| axis.label == label)?;
            let (_, value) = event
                .valuators
                .iter()
                .find(|(number, _)| *number == axis.number)?;
            Some((*axis, *value))
        };
        let pressure = value(AxisLabel::Pressure).and_then(|(axis, value)| {
            let span = axis.max - axis.min;
            (span.is_finite() && span > 0.0 && value.is_finite())
                .then(|| ((value - axis.min) / span).clamp(0.0, 1.0))
        });
        let tilt = |label| value(label).and_then(|(axis, value)| tilt_degrees(axis, value));
        let angles = match (tilt(AxisLabel::TiltX), tilt(AxisLabel::TiltY)) {
            (Some(x), Some(y)) => Some(PenAngles::TiltDegrees { x, y }),
            _ => None,
        };
        RawSensors {
            pressure,
            angles,
            ..RawSensors::default()
        }
    }

    /// The reading of `target` doing `action` for `event` on `surface`.
    fn reading(
        event: &XiEvent,
        surface: NativeSurface,
        scale: DevicePixelRatio,
        target: Target,
        action: ReadingAction,
    ) -> NativeReading {
        let reading = NativeReading::new(
            target.contact,
            target.kind,
            action,
            event.time.reading_time(),
        )
        .on(surface)
        .from_device(target.device)
        .with_modifiers(modifiers_of_mask(event.modifiers))
        .at_device(event.position.0, event.position.1, scale);
        if target.kind == PointerKind::Touch {
            reading
        } else {
            reading.with_primary(true)
        }
    }

    fn assemble(&mut self, reading: &NativeReading, out: &mut Vec<PlatformInput>) {
        out.extend(decode(&mut self.contacts, reading).unwrap_or_else(DecodeError::into_inputs));
    }

    /// What `event`, delivered to the window `surface` at `scale` device
    /// pixels per logical pixel, delivers there.
    pub fn event(
        &mut self,
        surface: NativeSurface,
        scale: DevicePixelRatio,
        event: &XiEvent,
    ) -> Decoded {
        let mut decoded = Decoded::default();
        if event.emulated {
            return decoded;
        }
        if matches!(
            event.action,
            XiAction::PinchBegin | XiAction::PinchUpdate { .. } | XiAction::PinchEnd { .. }
        ) {
            self.pinch(surface, scale, event, &mut decoded.inputs);
            return decoded;
        }
        if matches!(
            event.action,
            XiAction::SwipeBegin | XiAction::SwipeUpdate { .. } | XiAction::SwipeEnd { .. }
        ) {
            self.swipe(surface, scale, event, &mut decoded.inputs);
            return decoded;
        }
        let Some(target) = self.target_of(event) else {
            return decoded;
        };
        let out = &mut decoded.inputs;
        if target.kind != PointerKind::Touch {
            self.switch_contact(event, surface, scale, target, out);
        }
        let sensors = self.sensors(event);
        let read =
            |action| Self::reading(event, surface, scale, target, action).with_sensors(sensors);
        let key = (surface, target.device, target.contact);
        match event.action {
            XiAction::Motion => {
                // A scroll valuator's motion that leaves the cursor where
                // it was moves nothing.
                let moved = self
                    .contacts
                    .position(surface, Some(target.device), target.contact)
                    .is_none_or(|(_, raw)| (raw.x, raw.y) != event.position);
                if moved || !self.carries_scrolling(event) {
                    self.assemble(&read(ReadingAction::Move), out);
                }
                self.scroll_by_valuators(event, surface, target, out);
            }
            XiAction::ButtonPress => match wheel_button(event.detail) {
                Some(detents) => {
                    self.scroll(
                        event,
                        surface,
                        target,
                        detents,
                        ScrollPrecision::Notched,
                        out,
                    );
                }
                None => self.assemble(&read(ReadingAction::Press(button(event.detail))), out),
            },
            XiAction::ButtonRelease => {
                if wheel_button(event.detail).is_none() {
                    self.assemble(&read(ReadingAction::Release(button(event.detail))), out);
                }
            }
            XiAction::Enter => {
                self.reset_scrolling(event.source);
                decoded.hover = Some(true);
                self.assemble(&read(ReadingAction::Enter), out);
                self.inside.insert(key);
            }
            XiAction::Leave { grabbed } => {
                let pressed = self
                    .contacts
                    .buttons(surface, Some(target.device), target.contact)
                    .is_some_and(|buttons| !buttons.is_empty());
                if grabbed && pressed {
                    self.assemble(&read(ReadingAction::Cancel(NativeCancel::Platform)), out);
                }
                decoded.hover = Some(false);
                self.assemble(&read(ReadingAction::Leave), out);
                self.inside.remove(&key);
                if let Some(master) = self.masters.get_mut(&event.master) {
                    master.current = None;
                }
            }
            XiAction::TouchBegin => {
                self.assemble(&read(ReadingAction::Press(PointerButton::PRIMARY)), out);
            }
            XiAction::TouchUpdate => self.assemble(&read(ReadingAction::Move), out),
            XiAction::TouchEnd => {
                self.assemble(&read(ReadingAction::Release(PointerButton::PRIMARY)), out);
            }
            XiAction::PinchBegin
            | XiAction::PinchUpdate { .. }
            | XiAction::PinchEnd { .. }
            | XiAction::SwipeBegin
            | XiAction::SwipeUpdate { .. }
            | XiAction::SwipeEnd { .. } => {}
        }
        decoded
    }

    /// What `motions`, one pointer's run over the window `surface`
    /// (the loop's `PointerRun`: one device and source, nothing between
    /// them, oldest first), deliver there: one move at the last position,
    /// which carries the earlier ones as the positions it passed through,
    /// so a burst costs its window one move. A motion that carries
    /// scrolling is delivered on its own, as [`event`](Self::event)
    /// delivers it, between the moves before and after it.
    pub fn motions(
        &mut self,
        surface: NativeSurface,
        scale: DevicePixelRatio,
        motions: &[XiEvent],
    ) -> Vec<PlatformInput> {
        let mut out = Vec::new();
        let mut moves: Vec<&XiEvent> = Vec::new();
        for motion in motions.iter().filter(|motion| !motion.emulated) {
            let only_moves =
                matches!(motion.action, XiAction::Motion) && !self.carries_scrolling(motion);
            if !only_moves {
                self.moved_through(surface, scale, &std::mem::take(&mut moves), &mut out);
            }
            if only_moves {
                moves.push(motion);
            } else {
                out.extend(self.event(surface, scale, motion).inputs);
            }
        }
        self.moved_through(surface, scale, &moves, &mut out);
        out
    }

    /// One move of the contact `moves` move, at the last of them, through
    /// the earlier ones: the latest
    /// [`PointerMove::COALESCED_HISTORY_LIMIT`] of them, the history a
    /// move keeps of a burst however long it is.
    fn moved_through(
        &mut self,
        surface: NativeSurface,
        scale: DevicePixelRatio,
        moves: &[&XiEvent],
        out: &mut Vec<PlatformInput>,
    ) {
        let Some((last, passed)) = moves.split_last() else {
            return;
        };
        let Some(target) = self.target_of(last) else {
            return;
        };
        if let Some(first) = moves.first()
            && target.kind != PointerKind::Touch
        {
            self.switch_contact(first, surface, scale, target, out);
        }
        let reading = |pointers: &Self, motion: &XiEvent| {
            Self::reading(motion, surface, scale, target, ReadingAction::Move)
                .with_sensors(pointers.sensors(motion))
        };
        let current = reading(self, last);
        if passed.is_empty() {
            self.assemble(&current, out);
            return;
        }
        let kept = passed
            .len()
            .saturating_sub(PointerMove::COALESCED_HISTORY_LIMIT);
        let mut batch = MoveBatch::new();
        for motion in &passed[kept..] {
            batch.push_coalesced(reading(self, motion));
        }
        out.extend(
            batch
                .decode(&mut self.contacts, &current)
                .unwrap_or_else(DecodeError::into_inputs),
        );
    }

    /// The master's cursor moves to `target` (a pen takes it over, or
    /// gives it back): the contact it moved before leaves the window it
    /// was on, unless a button holds it, and `target` enters where the
    /// event is, unless the event is a crossing of its own.
    fn switch_contact(
        &mut self,
        event: &XiEvent,
        surface: NativeSurface,
        scale: DevicePixelRatio,
        target: Target,
        out: &mut Vec<PlatformInput>,
    ) {
        let Some(master) = self.masters.get_mut(&event.master) else {
            return;
        };
        let previous = master.current.replace((surface, target));
        if let Some((on, previous)) = previous.filter(|(_, previous)| *previous != target) {
            let held = self
                .contacts
                .buttons(on, Some(previous.device), previous.contact);
            if held.is_some_and(flui_platform::input::pointer::PointerButtons::is_empty) {
                let leave = Self::reading(event, on, scale, previous, ReadingAction::Leave);
                self.assemble(&leave, out);
                self.inside.remove(&(on, previous.device, previous.contact));
            }
        }
        let key = (surface, target.device, target.contact);
        let crossing = matches!(event.action, XiAction::Enter | XiAction::Leave { .. });
        if !crossing && !self.inside.contains(&key) {
            let enter = Self::reading(event, surface, scale, target, ReadingAction::Enter);
            self.assemble(&enter, out);
            self.inside.insert(key);
        }
    }

    /// The label of `master`'s touchpad gestures: a device of their own,
    /// announced with the master's first gesture.
    fn gestures_of(
        &mut self,
        master: &mut Master,
        event: &XiEvent,
        out: &mut Vec<PlatformInput>,
    ) -> NativeDevice {
        if let Some(device) = master.gestures {
            return device;
        }
        let device = self.mint();
        let time = self
            .contacts
            .stamp(event.time.reading_time())
            .unwrap_or(event.time.arrival);
        if let Some(change) = self
            .contacts
            .add_device(device, PointerKind::Trackpad, time)
        {
            out.push(PlatformInput::Pointer(PointerEvent::DeviceAdded(change)));
        }
        master.gestures = Some(device);
        device
    }

    /// A touchpad swipe of the event's master: one swipe of its finger
    /// count where the cursor rests, its steps summed into its travel.
    fn swipe(
        &mut self,
        surface: NativeSurface,
        scale: DevicePixelRatio,
        event: &XiEvent,
        out: &mut Vec<PlatformInput>,
    ) {
        let Some(mut master) = self.masters.get(&event.master).copied() else {
            return;
        };
        let device = self.gestures_of(&mut master, event, out);
        let swipe = |fingers, phase, (dx, dy)| {
            ReadingAction::Swipe(SwipeReading {
                fingers,
                phase,
                dx,
                dy,
            })
        };
        // A swipe open on another window than the event's ends there,
        // cancelled: it is never left open behind the one that follows.
        let cancel = ReadingAction::Cancel(NativeCancel::Platform);
        let mut steps: SmallVec<[(NativeSurface, ReadingAction); 2]> = SmallVec::new();
        match event.action {
            XiAction::SwipeBegin => {
                if let Some(open) = master.swipe.take()
                    && open.surface != surface
                {
                    steps.push((open.surface, cancel));
                }
                // The server names at least one finger; a count past a
                // byte is still a swipe of every finger a byte holds.
                let fingers = u8::try_from(event.detail).unwrap_or(u8::MAX);
                if let Some(fingers) = NonZeroU8::new(fingers) {
                    master.swipe = Some(OpenSwipe { surface, fingers });
                    steps.push((surface, swipe(fingers, GesturePhase::Began, (0.0, 0.0))));
                }
            }
            XiAction::SwipeUpdate { dx, dy } => {
                if let Some(open) = master.swipe.filter(|open| open.surface == surface) {
                    steps.push((
                        surface,
                        swipe(open.fingers, GesturePhase::Changed, (dx, dy)),
                    ));
                }
            }
            XiAction::SwipeEnd { cancelled } => {
                if let Some(open) = master.swipe.take() {
                    let phase = if cancelled {
                        GesturePhase::Cancelled
                    } else {
                        GesturePhase::Ended
                    };
                    steps.push(if open.surface == surface {
                        (surface, swipe(open.fingers, phase, (0.0, 0.0)))
                    } else {
                        (open.surface, cancel)
                    });
                }
            }
            _ => {}
        }
        self.masters.insert(event.master, master);
        let target = Target {
            device,
            contact: SWIPE,
            kind: PointerKind::Trackpad,
        };
        for (on, action) in steps {
            let reading = Self::reading(event, on, scale, target, action);
            self.assemble(&reading, out);
        }
    }

    /// A touchpad pinch of the event's master: one pan-zoom about the
    /// cursor.
    fn pinch(
        &mut self,
        surface: NativeSurface,
        scale: DevicePixelRatio,
        event: &XiEvent,
        out: &mut Vec<PlatformInput>,
    ) {
        let Some(mut master) = self.masters.get(&event.master).copied() else {
            return;
        };
        let device = self.gestures_of(&mut master, event, out);
        let readings: Vec<(GestureComponent, GesturePhase, GestureDelta)> = match event.action {
            XiAction::PinchBegin => {
                master.pinch = Some(Pinch {
                    surface,
                    scale: 1.0,
                });
                vec![
                    (
                        GestureComponent::Scale,
                        GesturePhase::Began,
                        GestureDelta::None,
                    ),
                    (
                        GestureComponent::Rotation,
                        GesturePhase::Began,
                        GestureDelta::None,
                    ),
                ]
            }
            XiAction::PinchUpdate {
                scale: since_begin,
                rotation,
            } => {
                let Some(pinch) = master
                    .pinch
                    .as_mut()
                    .filter(|pinch| pinch.surface == surface)
                else {
                    self.masters.insert(event.master, master);
                    return;
                };
                // The scale is since the pinch began; the assembly takes
                // each update's own factor.
                let factor = if since_begin.is_finite() && since_begin > 0.0 && pinch.scale > 0.0 {
                    let factor = since_begin / pinch.scale;
                    pinch.scale = since_begin;
                    factor
                } else {
                    1.0
                };
                vec![
                    (
                        GestureComponent::Scale,
                        GesturePhase::Changed,
                        GestureDelta::Scale(factor),
                    ),
                    (
                        GestureComponent::Rotation,
                        GesturePhase::Changed,
                        GestureDelta::Rotation(rotation.to_radians()),
                    ),
                ]
            }
            XiAction::PinchEnd { cancelled } => {
                if master
                    .pinch
                    .take()
                    .is_none_or(|pinch| pinch.surface != surface)
                {
                    self.masters.insert(event.master, master);
                    return;
                }
                let phase = if cancelled {
                    GesturePhase::Cancelled
                } else {
                    GesturePhase::Ended
                };
                vec![
                    (GestureComponent::Scale, phase, GestureDelta::None),
                    (GestureComponent::Rotation, phase, GestureDelta::None),
                ]
            }
            _ => Vec::new(),
        };
        self.masters.insert(event.master, master);
        let target = Target {
            device,
            contact: PINCH,
            kind: PointerKind::Trackpad,
        };
        for (component, phase, delta) in readings {
            let reading = Self::reading(
                event,
                surface,
                scale,
                target,
                ReadingAction::Gesture(GestureReading {
                    component,
                    phase,
                    delta,
                }),
            );
            self.assemble(&reading, out);
        }
    }

    /// Whether `event` carries a value of one of its source's scroll
    /// valuators.
    fn carries_scrolling(&self, event: &XiEvent) -> bool {
        self.devices.get(&event.source).is_some_and(|known| {
            known.scrolls.iter().any(|axis| {
                event
                    .valuators
                    .iter()
                    .any(|(number, _)| *number == axis.number)
            })
        })
    }

    /// Scroll by the source's scroll valuators in `event`.
    fn scroll_by_valuators(
        &mut self,
        event: &XiEvent,
        surface: NativeSurface,
        target: Target,
        out: &mut Vec<PlatformInput>,
    ) {
        let Some(known) = self.devices.get(&event.source) else {
            return;
        };
        let mut moves = Vec::with_capacity(known.scrolls.len());
        for axis in &known.scrolls {
            let Some(&(_, value)) = event
                .valuators
                .iter()
                .find(|(number, _)| *number == axis.number)
            else {
                continue;
            };
            // The first value after an enter or a change of device only
            // sets where scrolling starts.
            if let Some(last) = self.scrolled.insert((event.source, axis.number), value) {
                moves.push(ValuatorMove {
                    horizontal: axis.horizontal,
                    change: value - last,
                    increment: axis.increment,
                });
            }
        }
        let Some(detents) = valuator_detents(moves) else {
            return;
        };
        let whole = [detents.0, detents.1]
            .iter()
            .all(|detents| detents.fract() == 0.0);
        let precision = if whole {
            ScrollPrecision::Notched
        } else {
            ScrollPrecision::Precise
        };
        self.scroll(event, surface, target, detents, precision, out);
    }

    fn scroll(
        &mut self,
        event: &XiEvent,
        surface: NativeSurface,
        target: Target,
        (x, y): (f64, f64),
        precision: ScrollPrecision,
        out: &mut Vec<PlatformInput>,
    ) {
        let Some(delta) = detent_step((x, y)) else {
            return;
        };
        let Some(time) = self.contacts.stamp(event.time.reading_time()) else {
            return;
        };
        let scroll = ContactScroll {
            surface,
            device: Some(target.device),
            contact: target.contact,
            phase: None,
            delta,
            precision,
            inversion: ScrollInversion::Unknown,
            time,
            modifiers: modifiers_of_mask(event.modifiers),
        };
        out.extend(self.contacts.scroll(&scroll));
    }
}

/// A tilt valuator's value in degrees from vertical: as reported when its
/// range is within ±90 (the tablet drivers report degrees), else its
/// range spread over ±90. `None` for a value or range that is not finite.
fn tilt_degrees(axis: Axis, value: f64) -> Option<f64> {
    if !(value.is_finite() && axis.min.is_finite() && axis.max.is_finite()) {
        return None;
    }
    if axis.min >= -90.0 && axis.max <= 90.0 {
        return Some(value.clamp(-90.0, 90.0));
    }
    let span = axis.max - axis.min;
    (span > 0.0).then(|| ((value - axis.min) / span * 180.0 - 90.0).clamp(-90.0, 90.0))
}

/// The step a scroll of `(x, y)` detents makes, whether from a legacy
/// wheel button or the scroll valuators: X11's detents grow the scroll
/// offset as FLUI's delta does. `None` for an amount that is not finite.
pub(crate) fn detent_step((x, y): (f64, f64)) -> Option<ScrollDelta> {
    wheel_step(
        ScrollUnit::Detents,
        WheelAmount::offset(x),
        WheelAmount::offset(y),
    )
    .ok()
}

/// How far one scroll valuator moved since its last value, and how much
/// a detent is on it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ValuatorMove {
    /// The valuator scrolls horizontally.
    pub(crate) horizontal: bool,
    /// Its value now less its last one.
    pub(crate) change: f64,
    /// Its value's change for one detent.
    pub(crate) increment: f64,
}

/// The detents `moves` scroll (horizontal, vertical); `None` when none
/// moved: a change of zero, or one that is no number of increments, is
/// no move.
pub(crate) fn valuator_detents(
    moves: impl IntoIterator<Item = ValuatorMove>,
) -> Option<(f64, f64)> {
    let mut detents = None;
    for ValuatorMove {
        horizontal,
        change,
        increment,
    } in moves
    {
        let Some(change) = scroll_detents(change, increment).filter(|change| *change != 0.0) else {
            continue;
        };
        let (x, y) = detents.get_or_insert((0.0, 0.0));
        if horizontal {
            *x += change;
        } else {
            *y += change;
        }
    }
    detents
}

/// A scroll valuator's `change` in detents of `increment`; `None` for
/// one that is not finite or an increment that is zero.
fn scroll_detents(change: f64, increment: f64) -> Option<f64> {
    let detents = change / increment;
    (increment != 0.0 && detents.is_finite()).then_some(detents)
}

/// The scroll a legacy wheel button makes, in detents: 4 up, 5 down, 6
/// left, 7 right.
#[must_use]
pub const fn wheel_button(button: u32) -> Option<(f64, f64)> {
    match button {
        4 => Some((0.0, -1.0)),
        5 => Some((0.0, 1.0)),
        6 => Some((-1.0, 0.0)),
        7 => Some((1.0, 0.0)),
        _ => None,
    }
}

/// The FLUI button an X button is: 1 left, 2 middle, 3 right, 8 back, 9
/// forward; a vendor button past those shares the vocabulary's
/// non-actuating band, as winit's does.
#[must_use]
pub fn button(button: u32) -> PointerButton {
    match button {
        1 => PointerButton::PRIMARY,
        2 => PointerButton::AUXILIARY,
        3 => PointerButton::SECONDARY,
        8 => PointerButton::BACK,
        9 => PointerButton::FORWARD,
        other => {
            let band = u8::try_from(other.saturating_sub(10) % 26)
                .expect("BUG: a remainder of 26 fits a byte");
            PointerButton::try_from(7 + band)
                .expect("BUG: a vendor button maps into the non-actuating 7..=32 band")
        }
    }
}

/// The valuators an event's mask names, in order, paired with its values:
/// the number of each set bit. A bit past the numbers a valuator has (16
/// bits) and a value the mask names but the event lacks are left out.
#[must_use]
pub fn valuators(mask: &[u32], values: &[f64]) -> Vec<(u16, f64)> {
    let numbers = mask.iter().enumerate().flat_map(|(word, bits)| {
        (0..32_u32)
            .filter(move |bit| bits & (1 << bit) != 0)
            .map(move |bit| {
                u32::try_from(word)
                    .ok()
                    .and_then(|word| word.checked_mul(32)?.checked_add(bit))
                    .and_then(|number| u16::try_from(number).ok())
            })
    });
    numbers
        .zip(values.iter().copied())
        .filter_map(|(number, value)| Some((number?, value)))
        .collect()
}

#[cfg(test)]
mod tests;
