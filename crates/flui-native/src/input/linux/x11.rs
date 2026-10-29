//! The X11 host's input: XInput 2's pointers, touches, pens, touchpad pinches
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
//!   are assembled about the cursor. Swipes are the window manager's.
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

    use flui_platform::EventTime;
    use flui_platform::PlatformInput;
    use flui_platform::input_capabilities::PointerKindSet;
    use flui_platform::keyboard::Modifiers;
    use flui_platform::pointer::{
        PenTool, PointerButton, PointerEvent, PointerKind, ScrollPrecision, ScrollUnit,
    };

    use crate::input::assembly::{
        ContactRegistry, ContactScroll, DecodeError, GestureComponent, GestureDelta, GesturePhase,
        GestureReading, NativeCancel, NativeClock, NativeContact, NativeDevice, NativeReading,
        NativeSurface, NativeTime, PenAngles, RawSensors, ReadingAction, ReadingTime, decode,
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

    /// What a slave pointer is to FLUI.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

    /// One master pointer: its labels, the contact it moves now and where,
    /// and its pinch.
    #[derive(Debug, Clone, Copy)]
    struct Master {
        label: NativeDevice,
        /// The label of its touchpad's gestures, once it pinched.
        gestures: Option<NativeDevice>,
        /// The contact its last event moved, the cursor or a pen, and the
        /// window it moved it on.
        current: Option<(NativeSurface, Target)>,
        pinch: Option<Pinch>,
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

        /// The hierarchy reports `slaves` and `masters` removed or disabled:
        /// each ends its contacts and is removed, so an id the server lists
        /// again is a new device.
        pub fn removed(
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

        /// A hierarchy change: `removed_slaves` and `removed_masters` go, and
        /// each of `added` (added, enabled or attached) is described by
        /// `query`, which asks the server for that one device; `None` when it
        /// lists no enabled pointer by that id. One query per device the
        /// change names, so its cost is the change's own size.
        pub fn hierarchy(
            &mut self,
            removed_slaves: &[XiSlave],
            removed_masters: &[XiMaster],
            added: &[u16],
            mut query: impl FnMut(u16) -> Option<DeviceDescription>,
            time: EventTime,
        ) -> DeviceChanges {
            let mut out = self.removed(removed_slaves, removed_masters, time);
            for id in added {
                if let Some(device) = query(*id) {
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
            scale: f64,
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
            .at(event.position.0, event.position.1, scale);
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
        pub fn event(&mut self, surface: NativeSurface, scale: f64, event: &XiEvent) -> Decoded {
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
                XiAction::PinchBegin | XiAction::PinchUpdate { .. } | XiAction::PinchEnd { .. } => {
                }
            }
            decoded
        }

        /// The master's cursor moves to `target` (a pen takes it over, or
        /// gives it back): the contact it moved before leaves the window it
        /// was on, unless a button holds it, and `target` enters where the
        /// event is, unless the event is a crossing of its own.
        fn switch_contact(
            &mut self,
            event: &XiEvent,
            surface: NativeSurface,
            scale: f64,
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

        /// A touchpad pinch of the event's master: one pan-zoom about the
        /// cursor.
        fn pinch(
            &mut self,
            surface: NativeSurface,
            scale: f64,
            event: &XiEvent,
            out: &mut Vec<PlatformInput>,
        ) {
            let Some(mut master) = self.masters.get(&event.master).copied() else {
                return;
            };
            let device = if let Some(device) = master.gestures {
                device
            } else {
                // The touchpad's gestures are a device of their own, announced
                // with the master's first pinch.
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
            };
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
            let mut detents = (0.0, 0.0);
            let mut any = false;
            for axis in &known.scrolls {
                let Some(&(_, value)) = event
                    .valuators
                    .iter()
                    .find(|(number, _)| *number == axis.number)
                else {
                    continue;
                };
                let last = self.scrolled.insert((event.source, axis.number), value);
                let Some(change) = last.map(|last| scroll_detents(value - last, axis.increment))
                else {
                    // The first value after an enter or a change of device
                    // only sets where scrolling starts.
                    continue;
                };
                let Some(change) = change.filter(|change| *change != 0.0) else {
                    continue;
                };
                any = true;
                if axis.horizontal {
                    detents.0 += change;
                } else {
                    detents.1 += change;
                }
            }
            if !any {
                return;
            }
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
            let Ok(delta) = wheel_step(
                ScrollUnit::Detents,
                WheelAmount::offset(x),
                WheelAmount::offset(y),
            ) else {
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

    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::rc::Rc;
    use std::sync::Arc;
    use std::time::Duration;

    use cursor_icon::CursorIcon;
    use flui_platform::input_capabilities::InputCapabilities;
    use flui_platform::keyboard::KeyState;
    use flui_platform::{CursorError, EventTime, PlatformInput, Unsupported, UnsupportedReason};
    use parking_lot::Mutex;
    use x11rb::connection::Connection as _;
    use x11rb::protocol::Event;
    use x11rb::protocol::xinput::{
        self, ConnectionExt as _, DeviceClassData, DeviceType, Fp1616, Fp3232, XIEventMask,
    };
    use x11rb::protocol::xkb::{self, ConnectionExt as _};
    use x11rb::protocol::xproto::{
        self, ChangeWindowAttributesAux, ConnectionExt as _, NotifyMode,
    };

    use super::decode::{
        Axis, AxisLabel, DeviceChanges, DeviceDescription, ScrollAxis, X11Pointers, XiAction,
        XiEvent, XiMaster, XiSlave, XiTime, valuators,
    };
    use crate::input::assembly::{NativeCancel, NativeClock, NativeSurface};
    use crate::input::availability;
    use crate::input::event_timestamp_ns;
    use crate::input::linux::xkb::{Keyboard, KeymapSource};
    use crate::owner::linux::x11::{InputTarget, LoopInput, WindowRef, X11Connection};
    use crate::owner::panic_boundary::contain_owner_callback;
    use crate::owner::slot::OwnerSlot;
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
        /// The cursors loaded so far.
        loaded: HashMap<CursorIcon, xproto::Cursor>,
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
        state: OwnerSlot<Rc<RefCell<State>>>,
        /// The name of the layout the keyboard is in.
        layout: Mutex<String>,
        layout_listeners: Mutex<Vec<Box<dyn FnMut() + Send>>>,
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
            _ => return None,
        })
    }

    impl X11Input {
        /// The input of `connection`'s windows: XInput 2 set up on it, the
        /// devices listed, the keyboard read, and the device hierarchy and
        /// the keyboard's changes heard from here on.
        pub(crate) fn new(connection: &Arc<X11Connection>) -> Arc<Self> {
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
            let layout = keyboard
                .as_ref()
                .map(CoreKeyboard::layout_name)
                .unwrap_or_default();
            let input = Arc::new(Self {
                connection: Arc::clone(connection),
                xinput,
                gestures,
                labels,
                state: OwnerSlot::new(),
                layout: Mutex::new(layout),
                layout_listeners: Mutex::new(Vec::new()),
            });
            let state = State {
                pointers: X11Pointers::new(clock),
                windows: HashMap::new(),
                keyboard,
                cursors,
                loaded: HashMap::new(),
            };
            if input.state.replace(Rc::new(RefCell::new(state))).is_err() {
                tracing::error!("the X11 input keeps no state: owner slots are exhausted");
            }
            if xinput {
                let watched = xcb.xinput_xi_select_events(
                    connection.root(),
                    &[xinput::EventMask {
                        deviceid: xinput::Device::ALL.into(),
                        mask: vec![XIEventMask::HIERARCHY | XIEventMask::DEVICE_CHANGED],
                    }],
                );
                if let Err(error) = watched {
                    tracing::debug!(%error, "the X11 device hierarchy goes unheard");
                }
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
            let keyboard = Keyboard::new(&source)?;
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

        fn with_state<R>(&self, f: impl FnOnce(&mut State) -> R) -> Option<R> {
            let state = self.state.cloned()?;
            let mut state = state.borrow_mut();
            Some(f(&mut state))
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
        /// do), so a change costs one query per device it names, and a
        /// device's events after it in the same batch find it known.
        fn hierarchy(
            &self,
            change: &xinput::HierarchyEvent,
        ) -> Vec<(Arc<WindowDelivery>, Delivery)> {
            let (slaves, masters) = removed_devices(change);
            let added = added_devices(change);
            self.device_changes(|pointers, time| {
                pointers.hierarchy(
                    &slaves,
                    &masters,
                    &added,
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
            .unwrap_or_default()
        }

        /// What a window opening now can be given: the table of what this
        /// server's XInput 2 delivers.
        fn capabilities_now(&self) -> InputCapabilities {
            availability::x11(self.gestures)
        }

        /// The layout's name now, kept; whether it changed.
        fn read_layout(&self) -> bool {
            let name = self
                .with_state(|state| {
                    state
                        .keyboard
                        .as_ref()
                        .map(CoreKeyboard::layout_name)
                        .unwrap_or_default()
                })
                .unwrap_or_default();
            let mut layout = self.layout.lock();
            let changed = *layout != name;
            *layout = name;
            changed
        }

        /// Tell every listener the layout changed, with none of the input's
        /// locks held: a listener may register another.
        fn layout_changed(&self) {
            let mut listeners = std::mem::take(&mut *self.layout_listeners.lock());
            for listener in &mut listeners {
                contain_owner_callback(listener);
            }
            let mut kept = self.layout_listeners.lock();
            listeners.append(&mut kept);
            *kept = listeners;
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
            .unwrap_or_default()
        }

        /// What a key event delivers.
        fn key_event(
            &self,
            event: &xinput::KeyPressEvent,
            state: KeyState,
        ) -> Vec<(Arc<WindowDelivery>, Delivery)> {
            let repeat = u32::from(event.flags) & u32::from(xinput::KeyEventFlags::KEY_REPEAT) != 0;
            let time = EventTime::from_nanos(event_timestamp_ns());
            self.with_state(|input| {
                let Some(target) = input.windows.get(&event.event) else {
                    return Vec::new();
                };
                let delivery = Arc::clone(&target.delivery);
                let Some(core) = input.keyboard.as_mut() else {
                    return Vec::new();
                };
                let Some(key) = core.keyboard.key(event.detail, state, repeat, time) else {
                    return Vec::new();
                };
                vec![(delivery, Delivery::Input(PlatformInput::Keyboard(key)))]
            })
            .unwrap_or_default()
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
            .unwrap_or(false)
        }

        /// Read the keymap again and tell the listeners if the layout's name
        /// moved.
        fn reread_keymap(&self) {
            self.keymap_changed();
            if self.read_layout() {
                self.layout_changed();
            }
        }

        /// The server's keymap changed: read it again, with its layouts'
        /// names.
        fn keymap_changed(&self) {
            let connection = &self.connection;
            self.with_state(|state| {
                if let Some(core) = state.keyboard.as_mut() {
                    if !core.keyboard.reload(&core.source) {
                        tracing::debug!("the X11 keyboard's new keymap cannot be read");
                    }
                    if let Some(device) = core.source.x11_device()
                        && let Ok(device) = u16::try_from(device)
                    {
                        core.layouts = group_names(connection, device);
                    }
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
            .unwrap_or_default()
        }

        /// Select a window's XInput 2 events: every master device's pointer,
        /// touch, key and, on XInput 2.4, pinch events.
        fn select(&self, window: xproto::Window) {
            if !self.xinput {
                return;
            }
            let mut mask = XIEventMask::MOTION
                | XIEventMask::BUTTON_PRESS
                | XIEventMask::BUTTON_RELEASE
                | XIEventMask::ENTER
                | XIEventMask::LEAVE
                | XIEventMask::KEY_PRESS
                | XIEventMask::KEY_RELEASE
                | XIEventMask::TOUCH_BEGIN
                | XIEventMask::TOUCH_UPDATE
                | XIEventMask::TOUCH_END;
            if self.gestures {
                // x11rb names no XInput 2.4 mask bit; each is its event's
                // type, as every XInput 2 mask bit is.
                let bit = |event: u16| XIEventMask::from(1_u32 << u32::from(event));
                mask = mask
                    | bit(xinput::GESTURE_PINCH_BEGIN_EVENT)
                    | bit(xinput::GESTURE_PINCH_UPDATE_EVENT)
                    | bit(xinput::GESTURE_PINCH_END_EVENT);
            }
            let selected = self.connection.xcb().xinput_xi_select_events(
                window,
                &[xinput::EventMask {
                    deviceid: xinput::Device::ALL_MASTER.into(),
                    mask: vec![mask],
                }],
            );
            if let Err(error) = selected {
                tracing::debug!(%error, "an X11 window's input goes unheard");
            }
        }
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
                    .map(|reply| String::from_utf8_lossy(&reply.name).into_owned())
                    .unwrap_or_default()
            })
            .collect()
    }

    /// The surface a window's contacts belong to: its identity, which no
    /// other window of the platform has.
    fn surface_of(window: WindowRef) -> NativeSurface {
        NativeSurface(window.id.0)
    }

    /// Deliver `deliveries` in order, each in a boundary of its own: one
    /// handler's panic leaves the rest delivered, and the first is raised
    /// again after.
    fn deliver(deliveries: Vec<(Arc<WindowDelivery>, Delivery)>) {
        let mut first = None;
        for (delivery, item) in deliveries {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match item {
                Delivery::Input(input) => {
                    delivery.dispatch_input(input);
                }
                Delivery::Hover(hovered) => delivery.report_hover(hovered),
                Delivery::Present(capabilities) => delivery.set_input_capabilities(capabilities),
            }));
            if let Err(payload) = result {
                if first.is_some() {
                    flui_foundation::panic::retain_opaque_payload(payload);
                } else {
                    first = Some(payload);
                }
            }
        }
        if let Some(payload) = first {
            std::panic::resume_unwind(payload);
        }
    }

    /// The devices a hierarchy change reports added, enabled or attached,
    /// and not removed or disabled by it: each is asked of the server.
    fn added_devices(event: &xinput::HierarchyEvent) -> Vec<u16> {
        let has = |info: &xinput::HierarchyInfo, flags: &[xinput::HierarchyMask]| {
            flags
                .iter()
                .any(|flag| u32::from(info.flags) & u32::from(*flag) != 0)
        };
        event
            .infos
            .iter()
            .filter(|info| {
                has(
                    info,
                    &[
                        xinput::HierarchyMask::MASTER_ADDED,
                        xinput::HierarchyMask::SLAVE_ADDED,
                        xinput::HierarchyMask::DEVICE_ENABLED,
                        xinput::HierarchyMask::SLAVE_ATTACHED,
                    ],
                ) && !has(
                    info,
                    &[
                        xinput::HierarchyMask::MASTER_REMOVED,
                        xinput::HierarchyMask::SLAVE_REMOVED,
                        xinput::HierarchyMask::DEVICE_DISABLED,
                    ],
                )
            })
            .map(|info| info.deviceid)
            .collect()
    }

    /// The devices a hierarchy change reports removed or disabled.
    fn removed_devices(event: &xinput::HierarchyEvent) -> (Vec<XiSlave>, Vec<XiMaster>) {
        let gone = |info: &&xinput::HierarchyInfo, flag: xinput::HierarchyMask| {
            u32::from(info.flags)
                & (u32::from(flag) | u32::from(xinput::HierarchyMask::DEVICE_DISABLED))
                != 0
        };
        let slaves = event
            .infos
            .iter()
            .filter(|info| gone(info, xinput::HierarchyMask::SLAVE_REMOVED))
            .filter(|info| info.type_ == DeviceType::SLAVE_POINTER)
            .map(|info| XiSlave(info.deviceid))
            .collect();
        let masters = event
            .infos
            .iter()
            .filter(|info| gone(info, xinput::HierarchyMask::MASTER_REMOVED))
            .filter(|info| info.type_ == DeviceType::MASTER_POINTER)
            .map(|info| XiMaster(info.deviceid))
            .collect();
        (slaves, masters)
    }

    impl LoopInput for X11Input {
        fn capabilities(&self) -> InputCapabilities {
            let present = self
                .with_state(|state| state.pointers.present())
                .unwrap_or_default();
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

        fn event(&self, event: &Event) -> bool {
            let deliveries = match event {
                Event::XinputKeyPress(key) => self.key_event(key, KeyState::Down),
                Event::XinputKeyRelease(key) => self.key_event(key, KeyState::Up),
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
                    if self.keyboard_state(state) && self.read_layout() {
                        self.layout_changed();
                    }
                    Vec::new()
                }
                Event::XkbNewKeyboardNotify(_) | Event::XkbMapNotify(_) => {
                    // An X keymap is bounded by the protocol (key codes 8 to
                    // 255, four groups), so reading it again is bounded work.
                    self.reread_keymap();
                    Vec::new()
                }
                Event::FocusOut(focus) => {
                    if focus.mode == NotifyMode::NORMAL {
                        deliver(self.focus_out(focus.event));
                    }
                    // The window follows its focus too; the loop hands it
                    // the event even when a handler panicked here.
                    return false;
                }
                _ => match copied_event(event) {
                    Some((window, xi)) => self.pointer_event(window, &xi),
                    None => return false,
                },
            };
            deliver(deliveries);
            true
        }

        fn set_cursor(&self, window: WindowRef, cursor: CursorIcon) -> Result<(), CursorError> {
            let xcb = self.connection.xcb();
            let loaded = self
                .with_state(|state| {
                    if !state
                        .windows
                        .get(&window.xid)
                        .is_some_and(|target| target.window.id == window.id)
                    {
                        return Err(CursorError::from(Unsupported::new(
                            UnsupportedReason::NoWindow,
                        )));
                    }
                    if let Some(loaded) = state.loaded.get(&cursor) {
                        return Ok(*loaded);
                    }
                    let Some(handle) = state.cursors.as_ref() else {
                        return Err(CursorError::Backend(
                            "the X server's cursors cannot be loaded".into(),
                        ));
                    };
                    let names = std::iter::once(cursor.name())
                        .chain(cursor.alt_names().iter().copied())
                        .chain(std::iter::once(core_name(cursor)));
                    let mut loaded = x11rb::NONE;
                    for name in names {
                        match handle.load_cursor(xcb, name) {
                            Ok(found) if found != x11rb::NONE => {
                                loaded = found;
                                break;
                            }
                            Ok(_) => {}
                            Err(error) => return Err(CursorError::Backend(error.to_string())),
                        }
                    }
                    if loaded == x11rb::NONE {
                        return Err(CursorError::Backend(format!(
                            "the cursor theme has no {cursor:?} cursor"
                        )));
                    }
                    state.loaded.insert(cursor, loaded);
                    Ok(loaded)
                })
                .unwrap_or_else(|| Err(Unsupported::new(UnsupportedReason::Unavailable).into()))?;
            xcb.change_window_attributes(
                window.xid,
                &ChangeWindowAttributesAux::new().cursor(loaded),
            )
            .map_err(|error| CursorError::Backend(error.to_string()))?;
            xcb.flush()
                .map_err(|error| CursorError::Backend(error.to_string()))
        }

        fn keyboard_layout(&self) -> String {
            self.layout.lock().clone()
        }

        fn on_keyboard_layout_change(&self, callback: Box<dyn FnMut() + Send>) {
            self.layout_listeners.lock().push(callback);
        }
    }

    /// The name of the core cursor font's glyph closest to `cursor`, which a
    /// server without a cursor theme still shows.
    fn core_name(cursor: CursorIcon) -> &'static str {
        match cursor {
            CursorIcon::Pointer => "hand2",
            CursorIcon::Text | CursorIcon::VerticalText => "xterm",
            CursorIcon::Wait | CursorIcon::Progress => "watch",
            CursorIcon::Crosshair | CursorIcon::Cell => "crosshair",
            CursorIcon::Move | CursorIcon::AllScroll => "fleur",
            CursorIcon::Help => "question_arrow",
            CursorIcon::EResize
            | CursorIcon::WResize
            | CursorIcon::EwResize
            | CursorIcon::ColResize => "sb_h_double_arrow",
            CursorIcon::NResize
            | CursorIcon::SResize
            | CursorIcon::NsResize
            | CursorIcon::RowResize => "sb_v_double_arrow",
            CursorIcon::NotAllowed | CursorIcon::NoDrop => "circle",
            _ => "left_ptr",
        }
    }

    impl Drop for X11Input {
        fn drop(&mut self) {
            // The windows' entries hold their deliveries: they go with the
            // input, outside any borrow.
            let state = self.state.take();
            drop(state);
        }
    }

    #[cfg(test)]
    mod tests;
}
