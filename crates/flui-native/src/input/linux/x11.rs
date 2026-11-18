//! The X11 host's input: XInput 2's pointers, touches, pens, touchpad pinches and swipes
//! and smooth scrolling, XKB's keyboard, and the cursor.
//!
//! The pure half ([`decode`]) turns XInput 2 events, copied into plain
//! [`XiEvent`](decode::XiEvent)s, and the server's device list into owned
//! input through the shared pointer assembly; it is checked on every host,
//! and the conformance table and the native recordings run it.
//!
//! The live half (`X11Input`) is the loop's input (`owner::linux::x11`'s
//! `LoopInput`): it selects each window's XInput 2 events as the window
//! opens, reads the events the loop hands it, keeps the keyboard's XKB state
//! as the server reports it, follows the device hierarchy, and sets each
//! window's cursor from the cursor theme.
//!
//! What the translation decides:
//!
//! - **One cursor per master pointer.** A mouse and a touchpad move the same
//!   cursor; its contact is the master device's, a mouse. A pen moves it too,
//!   but is a contact of its own (the pen's device), so the cursor leaves as
//!   the pen takes the master over, and the other way round.
//! - **Emulated events are not input.** The server emulates the legacy wheel
//!   buttons from a device's smooth-scroll valuators, and pointer events from
//!   a touch; both carry `XIPointerEmulated`, and the valuators and the touch
//!   are read instead.
//! - **A wheel turn is detents.** A scroll valuator's change divided by its
//!   increment is the wheel's rotation, fractional for a touchpad; the legacy
//!   wheel buttons of a device with no scroll valuator (XTest's) are whole
//!   detents. Smooth scrolling has no phases on X11: the scroll is unphased.
//! - **A pinch is one pan-zoom.** XInput 2.4 reports a touchpad pinch's scale
//!   since it began and its rotation since the last update, as Wayland's
//!   `pointer-gestures` does; each update's own factor and clockwise radians
//!   are assembled about the cursor.
//! - **A swipe is a gesture.** XInput 2.4 reports a touchpad swipe of its
//!   finger count with the fingers' accelerated step since the last event;
//!   the steps are summed into the travel since the swipe began. XInput has
//!   no hold, so nothing stops a fling from resting fingers here.
//! - **A grab that takes the pointer mid-press ends the press.** A `Leave` in
//!   grab mode (the window manager's move, another client's grab) cancels the
//!   pressed contact; the implicit grab of a press delivers its release.
//! - **A device unplugged is gone.** One the hierarchy reports removed ends
//!   its contacts and is removed before the list is read again, so a device
//!   plugged back in under the same id is a new device.
//! - **The server's time, while it is the time.** An event is stamped with
//!   the server's millisecond time while that is within a second of the
//!   monotonic clock (a local server keeps it), so a burst read in one turn
//!   keeps its spacing; otherwise with its arrival.

pub mod decode {
    //! XInput 2 events and devices as plain data, and their assembly.

    #![forbid(unsafe_code)]

    use std::collections::{HashMap, HashSet};
    use std::num::NonZeroU8;

    use smallvec::SmallVec;

    use flui_foundation::geometry::DevicePixelRatio;
    use flui_platform::EventTime;
    use flui_platform::PlatformInput;
    use flui_platform::input_capabilities::PointerKindSet;
    use flui_platform::keyboard::Modifiers;
    use flui_platform::pointer::{
        MotionAcceleration, MotionDelta, PenTool, PointerButton, PointerEvent, PointerKind,
        PointerMove, ScrollDelta, ScrollInversion, ScrollPrecision, ScrollUnit,
    };

    use crate::input::assembly::{
        ContactRegistry, ContactScroll, DecodeError, GestureComponent, GestureDelta, GesturePhase,
        GestureReading, MoveBatch, NativeCancel, NativeClock, NativeContact, NativeDevice,
        NativeReading, NativeSurface, NativeTime, PenAngles, RawSensors, ReadingAction,
        ReadingTime, SwipeReading, decode,
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
            let brought = Self::MASTER_ADDED
                | Self::SLAVE_ADDED
                | Self::DEVICE_ENABLED
                | Self::SLAVE_ATTACHED;
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
            out.extend(
                decode(&mut self.contacts, reading).unwrap_or_else(DecodeError::into_inputs),
            );
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
                if held.is_some_and(flui_platform::pointer::PointerButtons::is_empty) {
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
                    let factor =
                        if since_begin.is_finite() && since_begin > 0.0 && pinch.scale > 0.0 {
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
            let Some(change) = scroll_detents(change, increment).filter(|change| *change != 0.0)
            else {
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
}

#[cfg(x11_host)]
pub(crate) use live::X11Input;

#[cfg(x11_host)]
mod live {
    //! The loop's input over the platform's connection.

    use std::cell::{Cell, RefCell};
    use std::collections::HashMap;
    use std::rc::Rc;
    use std::sync::Arc;
    use std::time::Duration;

    use cursor_icon::CursorIcon;
    use flui_platform::input_capabilities::InputCapabilities;
    use flui_platform::keyboard::{KeyPress, KeyState};
    use flui_platform::pointer::MotionDelta;
    use flui_platform::{
        Cursor, CursorError, CursorGrab, CursorImage, EventTime, PlatformFailure, PlatformInput,
        Unsupported, UnsupportedReason,
    };

    use x11rb::connection::Connection as _;
    use x11rb::protocol::Event;
    use x11rb::protocol::render;
    use x11rb::protocol::xinput::{
        self, ConnectionExt as _, DeviceClassData, DeviceType, Fp1616, Fp3232, XIEventMask,
    };
    use x11rb::protocol::xkb::{self, ConnectionExt as _};
    use x11rb::protocol::xproto::{
        self, ChangeWindowAttributesAux, ConnectionExt as _, EventMask, GrabMode, GrabStatus,
        NotifyMode,
    };

    use super::decode::{
        Axis, AxisLabel, DeviceChanges, DeviceDescription, HierarchyInfo, ScrollAxis, X11Pointers,
        XiAction, XiEvent, XiMaster, XiSlave, XiTime, valuators,
    };
    use crate::input::assembly::{NativeCancel, NativeClock, NativeSurface};
    use crate::input::availability;
    use crate::input::event_timestamp_ns;
    use crate::input::linux::xkb::{Keyboard, KeymapSource};
    use crate::owner::layout_callback::LayoutCallback;
    use crate::owner::linux::x11::{
        InputTarget, LoopInput, LoopText, PointerRun, WindowRef, X11Connection, X11Key,
    };

    use crate::owner::window_delivery::WindowDelivery;

    /// The XInput 2 version the input asks for: 2.4 brought touchpad
    /// gestures.
    const XI_ASKED: (u16, u16) = (2, 4);
    /// The least it works with: 2.2 brought touch.
    const XI_LEAST: (u16, u16) = (2, 2);

    /// What one turn delivers to a window, once no borrow is held.
    enum Delivery {
        Input(PlatformInput),
        Hover(bool),
        Present(InputCapabilities),
    }

    /// The label atoms a pen's valuators are named by.
    #[derive(Debug, Clone, Copy)]
    struct LabelAtoms {
        pressure: xproto::Atom,
        tilt_x: xproto::Atom,
        tilt_y: xproto::Atom,
    }

    impl LabelAtoms {
        fn label(&self, atom: xproto::Atom) -> AxisLabel {
            if atom == 0 {
                AxisLabel::Other
            } else if atom == self.pressure {
                AxisLabel::Pressure
            } else if atom == self.tilt_x {
                AxisLabel::TiltX
            } else if atom == self.tilt_y {
                AxisLabel::TiltY
            } else {
                AxisLabel::Other
            }
        }
    }

    /// The core keyboard: its keymap and state, where they come from, and
    /// the names of its layouts.
    struct CoreKeyboard {
        keyboard: Keyboard,
        source: KeymapSource,
        /// The layouts' names, by index, as XKB names the groups.
        layouts: Vec<String>,
    }

    /// What the input keeps on the owner thread.
    struct State {
        pointers: X11Pointers,
        /// The open windows, by their X id.
        windows: HashMap<xproto::Window, InputTarget>,
        keyboard: Option<CoreKeyboard>,
        cursors: Option<x11rb::cursor::Handle>,
        /// The theme's cursors loaded so far.
        loaded: HashMap<CursorIcon, xproto::Cursor>,
        /// The cursor of no pixels, made at its first use: a hidden cursor's
        /// and a locked pointer's.
        blank: Option<xproto::Cursor>,
        /// The last application image made a cursor, and that cursor: a
        /// repeated image is the cursor already made.
        image: Option<(CursorImage, xproto::Cursor)>,
        /// The Render format an image is made a cursor in (`ARGB32`), asked
        /// for at the first image.
        argb32: Option<render::Pictformat>,
    }

    /// The pointer a window holds (`CursorGrab`), and for a lock, the point
    /// of the window's content, in device pixels, it is held at (a point of
    /// the content, not of the screen, so it moves with the window) and the
    /// master pointer
    /// the grab took (the client's pointer), whose cursor the raw motion is.
    #[derive(Debug, Clone, Copy)]
    struct Held {
        window: WindowRef,
        grab: CursorGrab,
        at: (i16, i16),
        master: Option<XiMaster>,
    }

    /// The X11 loop's input: the windows' pointers, keys and cursors on the
    /// platform's connection.
    pub(crate) struct X11Input {
        connection: Arc<X11Connection>,
        /// XInput 2 answers, at 2.2 or later.
        xinput: bool,
        /// XInput 2 answers at 2.4: touchpad gestures.
        gestures: bool,
        labels: LabelAtoms,
        state: Rc<RefCell<State>>,
        /// The name of the layout the keyboard is in.
        layout: RefCell<String>,
        /// The application's keyboard layout callback.
        layout_callback: LayoutCallback,
        /// The layout's name moved this turn: the callback hears it at the
        /// turn's settle.
        layout_moved: Cell<bool>,
        /// The server's keymap changed since it was last read: it is read
        /// again before the next event that reads it, or once the turn's
        /// events are heard, so a burst of changes costs one read.
        keymap_stale: Cell<bool>,
        /// The windows' text input, which every key goes through: the
        /// input method of the focused field takes it or lets it through.
        text: Rc<dyn LoopText>,
        /// The pointer a window holds: one at most, as the server grabs it
        /// for one window of the client.
        held: Cell<Option<Held>>,
    }

    impl std::fmt::Debug for X11Input {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("X11Input")
                .field("xinput", &self.xinput)
                .field("gestures", &self.gestures)
                .finish_non_exhaustive()
        }
    }

    /// An X11 fixed-point 16.16 number.
    fn fp1616(value: Fp1616) -> f64 {
        f64::from(value) / 65536.0
    }

    /// An X11 fixed-point 32.32 number.
    fn fp3232(value: Fp3232) -> f64 {
        f64::from(value.integral) + f64::from(value.frac) / 4_294_967_296.0
    }

    /// The monotonic clock, which a local X server stamps its events on.
    fn monotonic() -> Duration {
        let now = rustix::time::clock_gettime(rustix::time::ClockId::Monotonic);
        Duration::new(
            u64::try_from(now.tv_sec).unwrap_or(0),
            u32::try_from(now.tv_nsec).unwrap_or(0),
        )
    }

    /// An event's time: the server's, with the clocks read as it arrived.
    fn xi_time(server_ms: xproto::Timestamp) -> XiTime {
        XiTime {
            server_ms,
            observed_ms: u64::try_from(monotonic().as_millis()).unwrap_or(u64::MAX),
            arrival: EventTime::from_nanos(event_timestamp_ns()),
        }
    }

    /// The fields every XInput 2 pointer, touch and crossing event shares.
    struct XiFields<'a> {
        window: xproto::Window,
        master: u16,
        source: u16,
        detail: u32,
        time: xproto::Timestamp,
        position: (Fp1616, Fp1616),
        modifiers: u32,
        mask: &'a [u32],
        values: &'a [Fp3232],
        emulated: bool,
    }

    /// The event `fields` make, as `action`, and the window it names.
    fn xi_event(action: XiAction, fields: &XiFields<'_>) -> (xproto::Window, XiEvent) {
        let values: Vec<f64> = fields.values.iter().copied().map(fp3232).collect();
        (
            fields.window,
            XiEvent {
                action,
                master: XiMaster(fields.master),
                source: XiSlave(fields.source),
                detail: fields.detail,
                position: (fp1616(fields.position.0), fp1616(fields.position.1)),
                emulated: fields.emulated,
                modifiers: fields.modifiers,
                valuators: valuators(fields.mask, &values),
                time: xi_time(fields.time),
            },
        )
    }

    /// The XInput 2 event `event` carries, and the window it names.
    fn copied_event(event: &Event) -> Option<(xproto::Window, XiEvent)> {
        let pointer = |action, e: &xinput::ButtonPressEvent| {
            let emulated =
                u32::from(e.flags) & u32::from(xinput::PointerEventFlags::POINTER_EMULATED) != 0;
            xi_event(
                action,
                &XiFields {
                    window: e.event,
                    master: e.deviceid,
                    source: e.sourceid,
                    detail: e.detail,
                    time: e.time,
                    position: (e.event_x, e.event_y),
                    modifiers: e.mods.effective,
                    mask: &e.valuator_mask,
                    values: &e.axisvalues,
                    emulated,
                },
            )
        };
        let touch = |action, e: &xinput::TouchBeginEvent| {
            xi_event(
                action,
                &XiFields {
                    window: e.event,
                    master: e.deviceid,
                    source: e.sourceid,
                    detail: e.detail,
                    time: e.time,
                    position: (e.event_x, e.event_y),
                    modifiers: e.mods.effective,
                    mask: &e.valuator_mask,
                    values: &e.axisvalues,
                    emulated: false,
                },
            )
        };
        let crossing = |action, e: &xinput::EnterEvent| {
            xi_event(
                action,
                &XiFields {
                    window: e.event,
                    master: e.deviceid,
                    source: e.sourceid,
                    detail: 0,
                    time: e.time,
                    position: (e.event_x, e.event_y),
                    modifiers: e.mods.effective,
                    mask: &[],
                    values: &[],
                    emulated: false,
                },
            )
        };
        let pinch = |action, e: &xinput::GesturePinchBeginEvent| {
            xi_event(
                action,
                &XiFields {
                    window: e.event,
                    master: e.deviceid,
                    source: e.sourceid,
                    detail: e.detail,
                    time: e.time,
                    position: (e.event_x, e.event_y),
                    modifiers: e.mods.effective,
                    mask: &[],
                    values: &[],
                    emulated: false,
                },
            )
        };
        let swipe = |action, e: &xinput::GestureSwipeBeginEvent| {
            xi_event(
                action,
                &XiFields {
                    window: e.event,
                    master: e.deviceid,
                    source: e.sourceid,
                    detail: e.detail,
                    time: e.time,
                    position: (e.event_x, e.event_y),
                    modifiers: e.mods.effective,
                    mask: &[],
                    values: &[],
                    emulated: false,
                },
            )
        };
        Some(match event {
            Event::XinputMotion(e) => pointer(XiAction::Motion, e),
            Event::XinputButtonPress(e) => pointer(XiAction::ButtonPress, e),
            Event::XinputButtonRelease(e) => pointer(XiAction::ButtonRelease, e),
            Event::XinputTouchBegin(e) => touch(XiAction::TouchBegin, e),
            Event::XinputTouchUpdate(e) => touch(XiAction::TouchUpdate, e),
            Event::XinputTouchEnd(e) => touch(XiAction::TouchEnd, e),
            Event::XinputEnter(e) => crossing(XiAction::Enter, e),
            Event::XinputLeave(e) => crossing(
                XiAction::Leave {
                    grabbed: e.mode == xinput::NotifyMode::GRAB,
                },
                e,
            ),
            Event::XinputGesturePinchBegin(e) => pinch(XiAction::PinchBegin, e),
            Event::XinputGesturePinchUpdate(e) => pinch(
                XiAction::PinchUpdate {
                    scale: fp1616(e.scale),
                    rotation: fp1616(e.delta_angle),
                },
                e,
            ),
            Event::XinputGesturePinchEnd(e) => pinch(
                XiAction::PinchEnd {
                    cancelled: u32::from(e.flags)
                        & u32::from(xinput::GesturePinchEventFlags::GESTURE_PINCH_CANCELLED)
                        != 0,
                },
                e,
            ),
            Event::XinputGestureSwipeBegin(e) => swipe(XiAction::SwipeBegin, e),
            Event::XinputGestureSwipeUpdate(e) => swipe(
                XiAction::SwipeUpdate {
                    dx: fp1616(e.delta_x),
                    dy: fp1616(e.delta_y),
                },
                e,
            ),
            Event::XinputGestureSwipeEnd(e) => swipe(
                XiAction::SwipeEnd {
                    cancelled: u32::from(e.flags)
                        & u32::from(xinput::GestureSwipeEventFlags::GESTURE_SWIPE_CANCELLED)
                        != 0,
                },
                e,
            ),
            _ => return None,
        })
    }

    impl X11Input {
        /// The input of `connection`'s windows: XInput 2 set up on it, the
        /// devices listed, the keyboard read, and the device hierarchy and
        /// the keyboard's changes heard from here on.
        pub(crate) fn new(connection: &Arc<X11Connection>, text: Rc<dyn LoopText>) -> Rc<Self> {
            let xcb = connection.xcb();
            let version = xcb
                .xinput_xi_query_version(XI_ASKED.0, XI_ASKED.1)
                .ok()
                .and_then(|cookie| cookie.reply().ok())
                .map(|reply| (reply.major_version, reply.minor_version));
            let xinput = version.is_some_and(|version| version >= XI_LEAST);
            let gestures = version.is_some_and(|version| version >= XI_ASKED);
            if !xinput {
                tracing::warn!("the X server has no XInput 2.2: X11 windows get no pointer input");
            }
            let atom = |name: &[u8]| {
                xcb.intern_atom(true, name)
                    .ok()
                    .and_then(|cookie| cookie.reply().ok())
                    .map_or(0, |reply| reply.atom)
            };
            let labels = LabelAtoms {
                pressure: atom(b"Abs Pressure"),
                tilt_x: atom(b"Abs Tilt X"),
                tilt_y: atom(b"Abs Tilt Y"),
            };
            let keyboard = Self::keyboard(connection);
            let cursors = x11rb::resource_manager::new_from_default(xcb)
                .ok()
                .and_then(|database| {
                    x11rb::cursor::Handle::new(xcb, connection.screen_number(), &database)
                        .ok()?
                        .reply()
                        .ok()
                });
            // The server's time is milliseconds of the monotonic clock: the
            // registry places it on the event timeline from the two read
            // together now.
            let clock = NativeClock::new(
                EventTime::from_nanos(event_timestamp_ns()),
                monotonic(),
                None,
            );
            let layout = keyboard.as_ref().map_or_default(CoreKeyboard::layout_name);
            let state = State {
                pointers: X11Pointers::new(clock),
                windows: HashMap::new(),
                keyboard,
                cursors,
                loaded: HashMap::new(),
                blank: None,
                image: None,
                argb32: None,
            };
            let input = Rc::new(Self {
                connection: Arc::clone(connection),
                xinput,
                gestures,
                labels,
                state: Rc::new(RefCell::new(state)),
                layout: RefCell::new(layout),
                layout_callback: LayoutCallback::default(),
                layout_moved: Cell::new(false),
                keymap_stale: Cell::new(false),
                text,
                held: Cell::new(None),
            });
            if xinput {
                input.select_root_events(false);
                input.read_devices();
            }
            input
        }

        /// The core keyboard, its XKB events selected.
        fn keyboard(connection: &Arc<X11Connection>) -> Option<CoreKeyboard> {
            let Some(source) = KeymapSource::x11_core_keyboard(connection) else {
                tracing::warn!("libxkbcommon-x11 or XKB is missing: X11 windows get no keys");
                return None;
            };
            let keyboard = Keyboard::try_new(&source)?;
            let device = u16::try_from(source.x11_device()?).ok()?;
            let events = xkb::EventType::NEW_KEYBOARD_NOTIFY
                | xkb::EventType::MAP_NOTIFY
                | xkb::EventType::STATE_NOTIFY;
            let selected = connection.xcb().xkb_select_events(
                device,
                xkb::EventType::from(0_u16),
                events,
                xkb::MapPart::from(0xff_u16),
                xkb::MapPart::from(0xff_u16),
                &xkb::SelectEventsAux::new(),
            );
            if let Err(error) = selected {
                tracing::debug!(%error, "the X11 keyboard's changes go unheard");
            }
            let layouts = group_names(connection, device);
            Some(CoreKeyboard {
                keyboard,
                source,
                layouts,
            })
        }

        fn with_state<R>(&self, f: impl FnOnce(&mut State) -> R) -> R {
            f(&mut self.state.borrow_mut())
        }

        /// The devices the server lists for `device` (one id, or
        /// `Device::ALL`), as the pure decoder reads them: the enabled ones
        /// and the master pointers.
        fn query(&self, device: impl Into<xinput::DeviceId>) -> Vec<DeviceDescription> {
            let Some(reply) = self
                .connection
                .xcb()
                .xinput_xi_query_device(device)
                .ok()
                .and_then(|cookie| cookie.reply().ok())
            else {
                return Vec::new();
            };
            reply
                .infos
                .into_iter()
                .filter(|info| info.enabled || info.type_ == DeviceType::MASTER_POINTER)
                .map(|info| {
                    let mut description = DeviceDescription {
                        id: info.deviceid,
                        master: info.type_ == DeviceType::MASTER_POINTER,
                        pointer: info.type_ == DeviceType::SLAVE_POINTER,
                        name: String::from_utf8_lossy(&info.name).into_owned(),
                        direct_touch: false,
                        axes: Vec::new(),
                        scrolls: Vec::new(),
                    };
                    self.read_classes(&info.classes, &mut description);
                    description
                })
                .collect()
        }

        /// Read `classes` (of a query's answer, or an `XIDeviceChanged`) into
        /// `description`.
        fn read_classes(
            &self,
            classes: &[xinput::DeviceClass],
            description: &mut DeviceDescription,
        ) {
            for class in classes {
                match &class.data {
                    DeviceClassData::Valuator(valuator) => description.axes.push(Axis {
                        number: valuator.number,
                        label: self.labels.label(valuator.label),
                        min: fp3232(valuator.min),
                        max: fp3232(valuator.max),
                    }),
                    DeviceClassData::Scroll(scroll) => {
                        description.scrolls.push(ScrollAxis {
                            number: scroll.number,
                            horizontal: scroll.scroll_type == xinput::ScrollType::HORIZONTAL,
                            increment: fp3232(scroll.increment),
                        });
                    }
                    DeviceClassData::Touch(touch) => {
                        description.direct_touch = touch.mode == xinput::TouchMode::DIRECT;
                    }
                    _ => {}
                }
            }
        }

        /// The devices present before any window opens, from one listing of
        /// them all: their announcements reach no window, only the presence.
        fn read_devices(&self) {
            let devices = self.query(xinput::Device::ALL);
            drop(self.device_changes(|pointers, time| pointers.devices(&devices, time)));
        }

        /// A hierarchy change: what it removes goes, and each device it adds,
        /// enables or attaches is asked of the server by its own id (as GTK's
        /// `handle_hierarchy_changed` and winit's `xinput2_hierarchy_changed`
        /// do), so a change costs one query per device it names, up to the
        /// devices the decoder knows, and a device's events after it in the
        /// same batch find it known.
        fn hierarchy(
            &self,
            change: &xinput::HierarchyEvent,
        ) -> Vec<(Arc<WindowDelivery>, Delivery)> {
            let change: Vec<HierarchyInfo> = change
                .infos
                .iter()
                .map(|info| HierarchyInfo {
                    id: info.deviceid,
                    device_use: u16::from(info.type_),
                    flags: u32::from(info.flags),
                })
                .collect();
            self.device_changes(|pointers, time| {
                pointers.hierarchy(
                    &change,
                    |id| self.query(id).into_iter().find(|device| device.id == id),
                    time,
                )
            })
        }

        /// What `change` makes of the devices goes to every window, with the
        /// kinds present.
        fn device_changes(
            &self,
            change: impl FnOnce(&mut X11Pointers, EventTime) -> DeviceChanges,
        ) -> Vec<(Arc<WindowDelivery>, Delivery)> {
            let time = EventTime::from_nanos(event_timestamp_ns());
            let capabilities = self.capabilities_now();
            self.with_state(|state| {
                let changes = change(&mut state.pointers, time);
                let present = capabilities.with_present(state.pointers.present());
                let mut out = Vec::new();
                for (surface, input) in changes {
                    for target in state.windows.values() {
                        if surface.is_none_or(|surface| surface == surface_of(target.window)) {
                            out.push((
                                Arc::clone(&target.delivery),
                                Delivery::Input(input.clone()),
                            ));
                        }
                    }
                }
                for target in state.windows.values() {
                    out.push((Arc::clone(&target.delivery), Delivery::Present(present)));
                }
                out
            })
        }

        /// What a window opening now can be given: the table of what this
        /// server's XInput 2 delivers.
        fn capabilities_now(&self) -> InputCapabilities {
            availability::x11(self.gestures)
        }

        /// The layout's name now, kept; whether it changed.
        fn read_layout(&self) -> bool {
            let name = self.with_state(|state| {
                state
                    .keyboard
                    .as_ref()
                    .map_or_default(CoreKeyboard::layout_name)
            });
            let mut layout = self.layout.borrow_mut();
            let changed = *layout != name;
            *layout = name;
            changed
        }

        /// Note that the layout's name moved: the application's callback hears
        /// it once the turn's events are delivered (`settle`), so its
        /// panic cannot cost the event that noticed the change.
        fn layout_changed(&self) {
            self.layout_moved.set(true);
        }

        /// What a pointer, touch or gesture event delivers.
        fn pointer_event(
            &self,
            window: xproto::Window,
            event: &XiEvent,
        ) -> Vec<(Arc<WindowDelivery>, Delivery)> {
            self.with_state(|state| {
                let Some(target) = state.windows.get(&window) else {
                    return Vec::new();
                };
                let delivery = Arc::clone(&target.delivery);
                let decoded =
                    state
                        .pointers
                        .event(surface_of(target.window), target.scale.get(), event);
                // An enter is heard before the input it starts, a leave after
                // what ends the contact.
                let mut out = Vec::new();
                if decoded.hover == Some(true) {
                    out.push((Arc::clone(&delivery), Delivery::Hover(true)));
                }
                out.extend(
                    decoded
                        .inputs
                        .into_iter()
                        .map(|input| (Arc::clone(&delivery), Delivery::Input(input))),
                );
                if decoded.hover == Some(false) {
                    out.push((delivery, Delivery::Hover(false)));
                }
                out
            })
        }

        /// What a pointer's motion over `window` delivers: `last`, and the
        /// motions `passed` it came through in the same turn (the rest of
        /// its [`PointerRun`]), as one move.
        fn pointer_motions(
            &self,
            window: xproto::Window,
            passed: &[Event],
            last: XiEvent,
        ) -> Vec<(Arc<WindowDelivery>, Delivery)> {
            let motions: Vec<XiEvent> = passed
                .iter()
                .filter_map(copied_event)
                .map(|(_, motion)| motion)
                .chain(std::iter::once(last))
                .collect();
            self.with_state(|state| {
                let Some(target) = state.windows.get(&window) else {
                    return Vec::new();
                };
                let delivery = Arc::clone(&target.delivery);
                state
                    .pointers
                    .motions(surface_of(target.window), target.scale.get(), &motions)
                    .into_iter()
                    .map(|input| (Arc::clone(&delivery), Delivery::Input(input)))
                    .collect()
            })
        }

        /// What a key event delivers.
        fn key_event(&self, event: &xinput::KeyPressEvent, state: KeyState) -> Option<X11Key> {
            let repeat = u32::from(event.flags) & u32::from(xinput::KeyEventFlags::KEY_REPEAT) != 0;
            let state = match state {
                KeyState::Down(_) if repeat => KeyState::Down(KeyPress::AutoRepeat),
                state => state,
            };
            let time = EventTime::from_nanos(event_timestamp_ns());
            let shared = Rc::downgrade(&self.state);
            self.with_state(|input| {
                let target = input.windows.get(&event.event)?;
                let window = target.window;
                let delivery = Arc::clone(&target.delivery);
                let core = input.keyboard.as_mut()?;
                let keysym = core.keyboard.keysym(event.detail);
                let keycode = event.detail;
                let (held, held_keysym) = core.keyboard.key_held(keycode, state, time)?;
                // The core protocol's state: the modifier mask, and the
                // group in bits 13 and 14.
                let group = u32::from(event.group.effective) & 0b11;
                let mask = (event.mods.effective & 0xff) | (group << 13);
                Some(X11Key {
                    window,
                    keysym,
                    keycode: u8::try_from(event.detail).unwrap_or(u8::MAX),
                    state: u16::try_from(mask).unwrap_or(u16::MAX),
                    release: state == KeyState::Up,
                    time: event.time,
                    // Only a key typed as it is feeds compose: one the input
                    // method takes is its own sequence's.
                    deliver: Box::new(move || {
                        let key = shared
                            .upgrade()
                            .and_then(|state| {
                                let mut state = state.try_borrow_mut().ok()?;
                                let core = state.keyboard.as_mut()?;
                                Some(core.keyboard.key_typed(keycode, held_keysym, held.clone()))
                            })
                            .unwrap_or(held);
                        delivery.dispatch_input(PlatformInput::Keyboard(key));
                    }),
                })
            })
        }

        /// The server's keyboard state changed: whether its layout did.
        fn keyboard_state(&self, event: &xkb::StateNotifyEvent) -> bool {
            self.with_state(|state| {
                let Some(core) = state.keyboard.as_mut() else {
                    return false;
                };
                core.keyboard.update_mask(
                    u32::from(event.base_mods),
                    u32::from(event.latched_mods),
                    u32::from(event.locked_mods),
                    [
                        u32::from(event.base_group.cast_unsigned()),
                        u32::from(event.latched_group.cast_unsigned()),
                        u32::from(u8::from(event.locked_group)),
                    ],
                )
            })
        }

        /// Read the keymap again if the server changed it since the last
        /// read, and tell the listeners if the layout's name moved. An X
        /// keymap is bounded by the protocol (key codes 8 to 255, four
        /// groups), so one read is bounded work; however many changes the
        /// server reported since, they cost this one.
        fn read_stale_keymap(&self) {
            if !self.keymap_stale.take() {
                return;
            }
            self.keymap_changed();
            if self.read_layout() {
                self.layout_changed();
            }
        }

        /// The server's keymap changed: read it again, with its layouts'
        /// names. The keymap is compiled under the input's borrow; the names
        /// are asked for, and a failure reported to the `tracing`
        /// subscriber, with it released.
        fn keymap_changed(&self) {
            let reloaded = self.with_state(|state| {
                let core = state.keyboard.as_mut()?;
                let compiled = core.keyboard.reload(&core.source);
                let device = core
                    .source
                    .x11_device()
                    .and_then(|device| u16::try_from(device).ok());
                Some((compiled, device))
            });
            let Some((compiled, device)) = reloaded else {
                return;
            };
            if !compiled {
                tracing::debug!("the X11 keyboard's new keymap cannot be read");
            }
            let Some(device) = device else {
                return;
            };
            let layouts = group_names(&self.connection, device);
            self.with_state(|state| {
                if let Some(core) = state.keyboard.as_mut() {
                    core.layouts = layouts;
                }
            });
        }

        /// The focus left `window`: its contacts end, the compose sequence in
        /// progress ends and each key still held is released there.
        fn focus_out(&self, window: xproto::Window) -> Vec<(Arc<WindowDelivery>, Delivery)> {
            let time = EventTime::from_nanos(event_timestamp_ns());
            self.with_state(|state| {
                let Some(target) = state.windows.get(&window) else {
                    return Vec::new();
                };
                let delivery = Arc::clone(&target.delivery);
                let mut inputs = state.pointers.cancel_surface(
                    surface_of(target.window),
                    NativeCancel::FocusLost,
                    time,
                );
                if let Some(core) = state.keyboard.as_mut() {
                    inputs.extend(
                        core.keyboard
                            .leave(time)
                            .into_iter()
                            .map(PlatformInput::Keyboard),
                    );
                }
                inputs
                    .into_iter()
                    .map(|input| (Arc::clone(&delivery), Delivery::Input(input)))
                    .collect()
            })
        }

        /// Let go of the pointer a window holds, if one does: the grab ends,
        /// and a lock's raw motion is no longer heard.
        fn let_go(&self) {
            let Some(held) = self.held.take() else {
                return;
            };
            if let Err(error) = self.connection.xcb().ungrab_pointer(x11rb::CURRENT_TIME) {
                tracing::debug!(%error, "cannot let go of the X11 pointer");
            }
            if held.grab == CursorGrab::Locked {
                self.select_root_events(false);
            }
        }

        /// Let go of the pointer if the window of X id `xid` holds it: the
        /// window lost the focus or was unmapped, and the server ended the
        /// grab or will.
        fn let_go_of(&self, xid: xproto::Window) {
            if self.held.get().is_some_and(|held| held.window.xid == xid) {
                self.let_go();
            }
        }

        /// Select what the input hears of every device on the root window:
        /// the device hierarchy and the devices' changes, and while a window
        /// holds the pointer in place, the raw motion (`XI_RawMotion`) XInput
        /// 2 sends there whoever grabs the pointer.
        fn select_root_events(&self, raw_motion: bool) {
            let mut mask = XIEventMask::HIERARCHY | XIEventMask::DEVICE_CHANGED;
            if raw_motion {
                mask |= XIEventMask::RAW_MOTION;
            }
            let selected = self.connection.xcb().xinput_xi_select_events(
                self.connection.root(),
                &[xinput::EventMask {
                    deviceid: xinput::Device::ALL.into(),
                    mask: vec![mask],
                }],
            );
            if let Err(error) = selected {
                tracing::debug!(%error, "the X11 device hierarchy goes unheard");
            }
        }

        /// The point of `window` a lock holds the pointer at, in device
        /// pixels: where the pointer is when it is over the content, the
        /// content's centre otherwise.
        fn lock_point(&self, window: WindowRef) -> (i16, i16) {
            let xcb = self.connection.xcb();
            let pointer = xcb.query_pointer(window.xid).ok();
            let geometry = xcb.get_geometry(window.xid).ok();
            let pointer = pointer.and_then(|cookie| cookie.reply().ok());
            let Some(geometry) = geometry.and_then(|cookie| cookie.reply().ok()) else {
                return (0, 0);
            };
            let inside = |at: i16, extent: u16| (0..i32::from(extent)).contains(&i32::from(at));
            match pointer {
                Some(pointer)
                    if pointer.same_screen
                        && inside(pointer.win_x, geometry.width)
                        && inside(pointer.win_y, geometry.height) =>
                {
                    (pointer.win_x, pointer.win_y)
                }
                _ => {
                    let half = |extent: u16| i16::try_from(extent / 2).unwrap_or(i16::MAX);
                    (half(geometry.width), half(geometry.height))
                }
            }
        }

        /// Put the pointer at `at` of the window of X id `xid`.
        fn warp_to(&self, xid: xproto::Window, at: (i16, i16)) {
            let warped =
                self.connection
                    .xcb()
                    .warp_pointer(x11rb::NONE, xid, 0, 0, 0, 0, at.0, at.1);
            if let Err(error) = warped {
                tracing::debug!(%error, "cannot hold the X11 pointer in place");
            }
        }

        /// The window of X id `xid` was moved, resized or rescaled to a
        /// content of `width` by `height` device pixels (`ConfigureNotify`).
        /// A lock's point is the content's, so it moves with the window; it
        /// is held inside the content as it shrinks, and the pointer, which
        /// the server left where it was on the screen, is put back there. A
        /// confinement follows the window by itself: the grab's `confine_to`
        /// is the window, and the server keeps the pointer inside it as it is
        /// reconfigured.
        fn window_reconfigured(&self, xid: xproto::Window, width: u16, height: u16) {
            let Some(mut held) = self
                .held
                .get()
                .filter(|held| held.grab == CursorGrab::Locked && held.window.xid == xid)
            else {
                return;
            };
            let inside = |at: i16, extent: u16| {
                let last = i16::try_from(extent.saturating_sub(1)).unwrap_or(i16::MAX);
                at.clamp(0, last)
            };
            held.at = (inside(held.at.0, width), inside(held.at.1, height));
            self.held.set(Some(held));
            self.warp_to(xid, held.at);
        }

        /// Whether `event`, over the window of X id `xid`, is a motion of the
        /// cursor that window holds in place: it is no move, and the pointer
        /// it took away from where the lock holds it is put back there.
        fn held_in_place(&self, xid: xproto::Window, event: &XiEvent) -> bool {
            let Some(held) = self
                .held
                .get()
                .filter(|held| held.grab == CursorGrab::Locked && held.window.xid == xid)
            else {
                return false;
            };
            if !self.with_state(|state| state.pointers.moves_cursor(event)) {
                return false;
            }
            let at = (f64::from(held.at.0), f64::from(held.at.1));
            if (event.position.0.round(), event.position.1.round()) != at {
                self.warp_to(xid, held.at);
            }
            true
        }

        /// `XI_RawMotion` while a window holds the pointer in place: a
        /// physical device's unaccelerated motion along valuators 0 and 1 as
        /// that window's raw motion, of the master pointer the grab took. A
        /// motion along neither delivers nothing.
        ///
        /// Only a slave device's own event is read: the server sends a
        /// grabbed master's raw event to the grabbing client twice, through
        /// its grab and through its root selection, and its slave's once.
        fn raw_motion(&self, raw: &xinput::RawMotionEvent) -> Vec<(Arc<WindowDelivery>, Delivery)> {
            let Some(held) = self
                .held
                .get()
                .filter(|held| held.grab == CursorGrab::Locked)
            else {
                return Vec::new();
            };
            if raw.deviceid != raw.sourceid {
                return Vec::new();
            }
            let values: Vec<f64> = raw.axisvalues_raw.iter().copied().map(fp3232).collect();
            let moved = valuators(&raw.valuator_mask, &values);
            let along = |axis: u16| {
                moved
                    .iter()
                    .find(|(number, _)| *number == axis)
                    .map_or(0.0, |(_, value)| *value)
            };
            let (dx, dy) = (along(0), along(1));
            if dx == 0.0 && dy == 0.0 {
                return Vec::new();
            }
            let Ok(delta) = MotionDelta::try_new(dx, dy) else {
                return Vec::new();
            };
            self.with_state(|state| {
                let Some(target) = state
                    .windows
                    .get(&held.window.xid)
                    .filter(|target| target.window == held.window)
                else {
                    return Vec::new();
                };
                state
                    .pointers
                    .raw_motion(
                        surface_of(target.window),
                        held.master,
                        XiSlave(raw.sourceid),
                        xi_time(raw.time),
                        delta,
                    )
                    .map(|input| (Arc::clone(&target.delivery), Delivery::Input(input)))
                    .into_iter()
                    .collect()
            })
        }

        /// Select a window's XInput 2 events: every master device's pointer,
        /// touch, key and, on XInput 2.4, pinch events.
        fn select(&self, window: xproto::Window) {
            if !self.xinput {
                return;
            }
            let mut events = vec![
                xinput::MOTION_EVENT,
                xinput::BUTTON_PRESS_EVENT,
                xinput::BUTTON_RELEASE_EVENT,
                xinput::ENTER_EVENT,
                xinput::LEAVE_EVENT,
                xinput::KEY_PRESS_EVENT,
                xinput::KEY_RELEASE_EVENT,
                xinput::TOUCH_BEGIN_EVENT,
                xinput::TOUCH_UPDATE_EVENT,
                xinput::TOUCH_END_EVENT,
            ];
            if self.gestures {
                events.extend([
                    xinput::GESTURE_PINCH_BEGIN_EVENT,
                    xinput::GESTURE_PINCH_UPDATE_EVENT,
                    xinput::GESTURE_PINCH_END_EVENT,
                    xinput::GESTURE_SWIPE_BEGIN_EVENT,
                    xinput::GESTURE_SWIPE_UPDATE_EVENT,
                    xinput::GESTURE_SWIPE_END_EVENT,
                ]);
            }
            let selected = self.connection.xcb().xinput_xi_select_events(
                window,
                &[xinput::EventMask {
                    deviceid: xinput::Device::ALL_MASTER.into(),
                    mask: event_mask(&events),
                }],
            );
            if let Err(error) = selected {
                tracing::debug!(%error, "an X11 window's input goes unheard");
            }
        }
    }

    /// The XInput 2 mask selecting `events`, by type: the bit of an event is
    /// its type, and the mask is as many 32-bit words as the highest type
    /// needs (`XI_GestureSwipeEnd`, type 32, is the second word's first bit;
    /// x11rb names no mask bit past `XI_BarrierLeave`).
    pub(super) fn event_mask(events: &[u16]) -> Vec<XIEventMask> {
        let words = events
            .iter()
            .map(|event| usize::from(*event) / 32 + 1)
            .max()
            .unwrap_or(0);
        let mut mask = vec![0_u32; words];
        for event in events {
            mask[usize::from(*event) / 32] |= 1 << (event % 32);
        }
        mask.into_iter().map(XIEventMask::from).collect()
    }

    impl CoreKeyboard {
        /// The name of the layout in effect; empty when XKB names none.
        fn layout_name(&self) -> String {
            usize::try_from(self.keyboard.layout())
                .ok()
                .and_then(|index| self.layouts.get(index))
                .cloned()
                .unwrap_or_default()
        }
    }

    /// The names XKB gives the core keyboard's layouts (its groups), in
    /// order: `English (US)`, `Russian`.
    fn group_names(connection: &X11Connection, device: u16) -> Vec<String> {
        let xcb = connection.xcb();
        let Some(reply) = xcb
            .xkb_get_names(device, xkb::NameDetail::GROUP_NAMES)
            .ok()
            .and_then(|cookie| cookie.reply().ok())
        else {
            return Vec::new();
        };
        reply
            .value_list
            .groups
            .unwrap_or_default()
            .into_iter()
            .map(|atom| {
                xcb.get_atom_name(atom)
                    .ok()
                    .and_then(|cookie| cookie.reply().ok())
                    .map_or_default(|reply| String::from_utf8_lossy(&reply.name).into_owned())
            })
            .collect()
    }

    /// The surface a window's contacts belong to: its identity, which no
    /// other window of the platform has.
    fn surface_of(window: WindowRef) -> NativeSurface {
        NativeSurface::of_window(window.id)
    }

    /// Deliver `deliveries` in order, each in a boundary of its own: one
    /// handler's panic leaves the rest delivered, and the first is raised
    /// again after.
    fn deliver(deliveries: Vec<(Arc<WindowDelivery>, Delivery)>) {
        let mut failure = flui_foundation::panic::FirstPanic::new();
        for (delivery, item) in deliveries {
            failure.contain(|| match item {
                Delivery::Input(input) => {
                    delivery.dispatch_input(input);
                }
                Delivery::Hover(hovered) => delivery.report_hover(hovered),
                Delivery::Present(capabilities) => delivery.set_input_capabilities(capabilities),
            });
        }
        failure.resume();
    }

    impl LoopInput for X11Input {
        fn capabilities(&self) -> InputCapabilities {
            let present = self.with_state(|state| state.pointers.present());
            self.capabilities_now().with_present(present)
        }

        fn window_opened(&self, target: InputTarget) {
            let xid = target.window.xid;
            self.select(xid);
            let replaced = self.with_state(|state| state.windows.insert(xid, target));
            // A stale entry of the X id goes outside the borrow: it holds a
            // delivery, and with it a handler.
            drop(replaced);
        }

        fn window_closed(&self, window: WindowRef) {
            self.let_pointer_go(window);
            let removed = self.with_state(|state| {
                // The X id may already name a later window: only this one's
                // entry goes.
                if state
                    .windows
                    .get(&window.xid)
                    .is_some_and(|target| target.window.id == window.id)
                {
                    let time = EventTime::from_nanos(event_timestamp_ns());
                    // The window's handler hears nothing more.
                    drop(state.pointers.cancel_surface(
                        surface_of(window),
                        NativeCancel::FocusLost,
                        time,
                    ));
                    return state.windows.remove(&window.xid);
                }
                None
            });
            // The entry goes outside the borrow: it holds the delivery.
            drop(removed);
        }

        fn pointer_taken(&self, window: xproto::Window) {
            let time = EventTime::from_nanos(event_timestamp_ns());
            let deliveries = self.with_state(|state| {
                let Some(target) = state.windows.get(&window) else {
                    return Vec::new();
                };
                let delivery = Arc::clone(&target.delivery);
                state
                    .pointers
                    .cancel_surface(surface_of(target.window), NativeCancel::CaptureLost, time)
                    .into_iter()
                    .map(|input| (Arc::clone(&delivery), Delivery::Input(input)))
                    .collect()
            });
            deliver(deliveries);
        }

        fn event(&self, event: &Event) -> bool {
            let deliveries = match event {
                Event::XinputKeyPress(key) | Event::XinputKeyRelease(key) => {
                    let state = if matches!(event, Event::XinputKeyPress(_)) {
                        KeyState::Down(KeyPress::First)
                    } else {
                        KeyState::Up
                    };
                    // A key is read with the keymap the server has now.
                    self.read_stale_keymap();
                    // The text input delivers it, with no borrow of the input
                    // held: now, or once the input method answered for it.
                    if let Some(key) = self.key_event(key, state) {
                        self.text.key(key);
                    }
                    return true;
                }
                Event::XinputHierarchy(change) => self.hierarchy(change),
                Event::XinputDeviceChanged(changed) => {
                    // The classes the event carries are the device's now, as
                    // GTK's `handle_device_changed` reads them: no query.
                    let mut classes = DeviceDescription {
                        id: changed.deviceid,
                        master: false,
                        pointer: true,
                        name: String::new(),
                        direct_touch: false,
                        axes: Vec::new(),
                        scrolls: Vec::new(),
                    };
                    if changed.reason == xinput::ChangeReason::DEVICE_CHANGE {
                        self.read_classes(&changed.classes, &mut classes);
                    }
                    self.with_state(|state| {
                        state.pointers.reset_scrolling(XiSlave(changed.sourceid));
                        if changed.reason == xinput::ChangeReason::DEVICE_CHANGE {
                            state.pointers.classes_changed(
                                XiSlave(changed.deviceid),
                                &classes.axes,
                                &classes.scrolls,
                            );
                        }
                    });
                    Vec::new()
                }
                Event::XkbStateNotify(state) => {
                    // The state is the state of the keymap the server has now.
                    self.read_stale_keymap();
                    if self.keyboard_state(state) && self.read_layout() {
                        self.layout_changed();
                    }
                    Vec::new()
                }
                Event::XkbNewKeyboardNotify(_) | Event::XkbMapNotify(_) => {
                    self.keymap_stale.set(true);
                    Vec::new()
                }
                Event::FocusOut(focus) => {
                    // A grab of the keyboard (a window manager's switcher)
                    // moves no focus; a focus that moved takes the window's
                    // hold of the pointer with it.
                    if focus.mode != NotifyMode::GRAB {
                        self.let_go_of(focus.event);
                    }
                    if focus.mode == NotifyMode::NORMAL {
                        deliver(self.focus_out(focus.event));
                    }
                    // The window follows its focus too; the loop hands it
                    // the event even when a handler panicked here.
                    return false;
                }
                Event::ConfigureNotify(configured) => {
                    self.window_reconfigured(
                        configured.window,
                        configured.width,
                        configured.height,
                    );
                    // The window reads itself again too.
                    return false;
                }
                Event::UnmapNotify(unmapped) => {
                    // The server ends the grab of a window that is no longer
                    // viewable; the window reads itself again too.
                    self.let_go_of(unmapped.window);
                    return false;
                }
                Event::XinputRawMotion(raw) => self.raw_motion(raw),
                _ => match copied_event(event) {
                    Some((window, xi)) if self.held_in_place(window, &xi) => Vec::new(),
                    Some((window, xi)) => {
                        if matches!(
                            event,
                            Event::XinputButtonPress(_) | Event::XinputTouchBegin(_)
                        ) {
                            // The keys the window holds are typed before
                            // the press that may move its caret.
                            self.text.pointer_pressed(window);
                        }
                        self.pointer_event(window, &xi)
                    }
                    None => return false,
                },
            };
            deliver(deliveries);
            true
        }

        fn motions(&self, run: &PointerRun) -> bool {
            let Some((window, last)) = copied_event(run.last()) else {
                return false;
            };
            // A run is one pointer's over one window: held in place, it is
            // put back once, after its last motion.
            if self.held_in_place(window, &last) {
                return true;
            }
            let deliveries = if run.passed().is_empty() {
                self.pointer_event(window, &last)
            } else {
                self.pointer_motions(window, run.passed(), last)
            };
            deliver(deliveries);
            true
        }

        fn settle(&self) {
            self.read_stale_keymap();
            // After the turn's events: the callback's panic is the turn's
            // failure, raised once every event was heard.
            if self.layout_moved.take() {
                self.layout_callback.changed().or_resume(());
            }
        }

        /// A system cursor from the theme, a hidden one as the cursor of no
        /// pixels, an image as a Render cursor made of it. The cursor is the
        /// window's attribute: a lock's grab shows its blank cursor over it
        /// until the grab ends.
        fn set_cursor(&self, window: WindowRef, cursor: &Cursor) -> Result<(), CursorError> {
            let native = self.with_state(|state| {
                if !state
                    .windows
                    .get(&window.xid)
                    .is_some_and(|target| target.window.id == window.id)
                {
                    return Err(CursorError::from(Unsupported::new(
                        UnsupportedReason::NoWindow,
                    )));
                }
                match cursor {
                    Cursor::Icon(icon) => state.theme_cursor(&self.connection, *icon),
                    Cursor::Hidden => state.blank_cursor(&self.connection),
                    Cursor::Image(image) => state.image_cursor(&self.connection, image),
                }
            })?;
            let xcb = self.connection.xcb();
            xcb.change_window_attributes(
                window.xid,
                &ChangeWindowAttributesAux::new().cursor(native),
            )
            .map_err(|error| CursorError::failed(error.to_string()))?;
            xcb.flush()
                .map_err(|error| CursorError::failed(error.to_string()))
        }

        /// The server's pointer grab of `window`, confined to it. Confined,
        /// the pointer moves inside the window's content. Locked, the grab
        /// shows the blank cursor, the master pointers' raw motion is heard
        /// on the root window, and each motion of the cursor is put back
        /// where the lock holds it (`held_in_place`): a warp,
        /// rather than a confinement to a window of one pixel, whose
        /// crossing would read as the pointer leaving the window.
        fn hold_pointer(&self, window: WindowRef, grab: CursorGrab) -> Result<(), PlatformFailure> {
            self.let_go();
            let locked = match grab {
                CursorGrab::None => return Ok(()),
                CursorGrab::Confined => false,
                CursorGrab::Locked => true,
            };
            let failed = |message: String| PlatformFailure::Failed { message };
            let cursor = if locked {
                self.with_state(|state| state.blank_cursor(&self.connection))
                    .map_err(|error| failed(error.to_string()))?
            } else {
                x11rb::NONE
            };
            let at = if locked {
                self.lock_point(window)
            } else {
                (0, 0)
            };
            let xcb = self.connection.xcb();
            let answer = xcb
                .grab_pointer(
                    true,
                    window.xid,
                    EventMask::NO_EVENT,
                    GrabMode::ASYNC,
                    GrabMode::ASYNC,
                    window.xid,
                    cursor,
                    x11rb::CURRENT_TIME,
                )
                .map_err(|error| failed(error.to_string()))?
                .reply()
                .map_err(|error| failed(error.to_string()))?;
            if answer.status != GrabStatus::SUCCESS {
                return Err(failed(format!(
                    "the X server refused the pointer grab: {:?}",
                    answer.status
                )));
            }
            // The core grab takes the client's pointer, a master.
            let master = locked
                .then(|| xcb.xinput_xi_get_client_pointer(window.xid).ok())
                .flatten()
                .and_then(|cookie| cookie.reply().ok())
                .map(|reply| XiMaster(reply.deviceid))
                .filter(|master| master.0 != 0);
            self.held.set(Some(Held {
                window,
                grab,
                at,
                master,
            }));
            if locked {
                self.select_root_events(true);
                self.warp_to(window.xid, at);
            }
            Ok(())
        }

        fn let_pointer_go(&self, window: WindowRef) {
            if self.held.get().is_some_and(|held| held.window == window) {
                self.let_go();
            }
        }

        fn pointer_grab(&self, window: WindowRef) -> CursorGrab {
            self.held
                .get()
                .filter(|held| held.window == window)
                .map_or(CursorGrab::None, |held| held.grab)
        }

        fn keyboard_layout(&self) -> String {
            self.layout.borrow().clone()
        }

        fn on_keyboard_layout_change(&self, callback: Box<dyn FnMut()>) {
            self.layout_callback.register(callback);
        }
    }

    mod cursors;
    #[cfg(test)]
    mod tests;
}
