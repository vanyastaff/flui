//! The X11 host's input: XInput 2's pointers, touches, pens and smooth
//! scrolling, XKB's keyboard, and the cursor.
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
//! - **A grab that takes the pointer mid-press ends the press.** A `Leave` in
//!   grab mode (the window manager's move, another client's grab) cancels the
//!   pressed contact; the implicit grab of a press delivers its release.

pub mod decode {
    //! XInput 2 events and devices as plain data, and their assembly.

    #![forbid(unsafe_code)]

    use std::collections::{HashMap, HashSet};

    use flui_platform::EventTime;
    use flui_platform::PlatformInput;
    use flui_platform::input_capabilities::PointerKindSet;
    use flui_platform::pointer::{
        PenTool, PointerButton, PointerEvent, PointerKind, ScrollPrecision, ScrollUnit,
    };

    use crate::input::assembly::{
        ContactRegistry, ContactScroll, DecodeError, NativeCancel, NativeClock, NativeContact,
        NativeDevice, NativeReading, NativeSurface, PenAngles, RawSensors, ReadingAction,
        ReadingTime, decode,
    };
    use crate::input::linux::xkb::keys::ActiveModifiers;
    use crate::input::scroll::{WheelAmount, wheel_step};

    /// The cursor of a master pointer on a window.
    pub const CURSOR: NativeContact = NativeContact(u64::MAX);
    /// A pen on a window.
    pub const PEN: NativeContact = NativeContact(u64::MAX - 2);

    /// What an XInput 2 event says happened.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    }

    /// One XInput 2 pointer or touch event, copied out of the protocol.
    #[derive(Debug, Clone, PartialEq)]
    pub struct XiEvent {
        /// What happened.
        pub action: XiAction,
        /// The master device it came through.
        pub device: u16,
        /// The physical (slave) device that produced it.
        pub source: u16,
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
        /// When it arrived, on the event timeline.
        pub time: EventTime,
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
        /// The device's id, which the server gives another device once this
        /// one is gone.
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

    /// One master pointer: its label and the contact it moves now.
    #[derive(Debug, Clone, Copy)]
    struct Master {
        label: NativeDevice,
        /// The contact the master's last event moved: the cursor, or a pen.
        current: Option<(NativeDevice, NativeContact, PointerKind)>,
    }

    /// What one event delivers to its window.
    #[derive(Debug, Default)]
    pub struct Decoded {
        /// The input, in order.
        pub inputs: Vec<PlatformInput>,
        /// The pointer entered (`true`) or left (`false`) the window.
        pub hover: Option<bool>,
    }

    /// The X11 host's pointers: the shared registry, the devices the server
    /// lists and what each scroll valuator last reported.
    #[derive(Debug)]
    pub struct X11Pointers {
        contacts: ContactRegistry,
        devices: HashMap<u16, Known>,
        masters: HashMap<u16, Master>,
        /// The last value of each scroll valuator, by device and number.
        scrolled: HashMap<(u16, u16), f64>,
        /// The hovering contacts inside each window: entered and not left.
        inside: HashSet<(NativeSurface, NativeDevice, NativeContact)>,
        next_label: u64,
    }

    impl Default for X11Pointers {
        fn default() -> Self {
            Self::new()
        }
    }

    impl X11Pointers {
        /// No device known yet; readings are stamped on the event timeline.
        #[must_use]
        pub fn new() -> Self {
            Self {
                contacts: ContactRegistry::new(NativeClock::epoch_only()),
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

        /// The server lists `devices` now: a device it no longer lists is
        /// removed, its contacts cancelled; a new one is announced. Each
        /// input names the surface it belongs to, or none for every window.
        pub fn devices(
            &mut self,
            devices: &[DeviceDescription],
            time: EventTime,
        ) -> Vec<(Option<NativeSurface>, PlatformInput)> {
            let mut out = Vec::new();
            let listed_masters: Vec<u16> = devices
                .iter()
                .filter(|device| device.master)
                .map(|device| device.id)
                .collect();
            let listed_slaves: Vec<&DeviceDescription> =
                devices.iter().filter(|device| device.pointer).collect();
            // Gone first: an id the server gives another device is a new one.
            let gone_slaves: Vec<u16> = self
                .devices
                .iter()
                .filter(|(id, known)| {
                    listed_slaves
                        .iter()
                        .find(|device| device.id == **id)
                        .is_none_or(|device| role(device) != known.role)
                })
                .map(|(id, _)| *id)
                .collect();
            for id in gone_slaves {
                if let Some(known) = self.devices.remove(&id) {
                    self.scrolled.retain(|(device, _), _| *device != id);
                    if let Some(label) = known.label {
                        out.extend(self.remove(label, time));
                    }
                }
            }
            let gone_masters: Vec<u16> = self
                .masters
                .keys()
                .filter(|id| !listed_masters.contains(id))
                .copied()
                .collect();
            for id in gone_masters {
                if let Some(master) = self.masters.remove(&id) {
                    out.extend(self.remove(master.label, time));
                }
            }
            for id in listed_masters {
                if self.masters.contains_key(&id) {
                    continue;
                }
                let label = self.mint();
                self.masters.insert(
                    id,
                    Master {
                        label,
                        current: None,
                    },
                );
                if let Some(change) = self.contacts.add_device(label, PointerKind::Mouse, time) {
                    out.push((
                        None,
                        PlatformInput::Pointer(PointerEvent::DeviceAdded(change)),
                    ));
                }
            }
            for device in listed_slaves {
                if let Some(known) = self.devices.get_mut(&device.id) {
                    // The same device: its classes may have changed.
                    known.axes.clone_from(&device.axes);
                    known.scrolls.clone_from(&device.scrolls);
                    continue;
                }
                let role = role(device);
                let label = match role {
                    DeviceRole::Cursor => None,
                    DeviceRole::Touchscreen | DeviceRole::Pen(_) => Some(self.mint()),
                };
                if let Some(label) = label {
                    let kind = match role {
                        DeviceRole::Pen(tool) => PointerKind::Pen { tool },
                        _ => PointerKind::Touch,
                    };
                    if let Some(change) = self.contacts.add_device(label, kind, time) {
                        out.push((
                            None,
                            PlatformInput::Pointer(PointerEvent::DeviceAdded(change)),
                        ));
                    }
                }
                self.devices.insert(
                    device.id,
                    Known {
                        role,
                        label,
                        axes: device.axes.clone(),
                        scrolls: device.scrolls.clone(),
                    },
                );
            }
            out
        }

        fn remove(
            &mut self,
            label: NativeDevice,
            time: EventTime,
        ) -> Vec<(Option<NativeSurface>, PlatformInput)> {
            for master in self.masters.values_mut() {
                if master.current.is_some_and(|(device, _, _)| device == label) {
                    master.current = None;
                }
            }
            self.inside.retain(|(_, device, _)| *device != label);
            let (ended, removed) = self.contacts.remove_device(label, time);
            let mut out: Vec<_> = ended
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
        pub fn reset_scrolling(&mut self, source: u16) {
            self.scrolled.retain(|(device, _), _| *device != source);
        }

        /// End every contact on `surface` with `reason`.
        pub fn cancel_surface(
            &mut self,
            surface: NativeSurface,
            reason: NativeCancel,
            time: EventTime,
        ) -> Vec<PlatformInput> {
            for master in self.masters.values_mut() {
                master.current = None;
            }
            self.inside.retain(|(inside, _, _)| *inside != surface);
            self.contacts.cancel_surface(surface, time, reason)
        }

        /// The device, contact and kind `event` moves: a pen's own, a touch,
        /// or its master's cursor. `None` for a device the server has not
        /// listed.
        fn contact_of(
            &self,
            event: &XiEvent,
        ) -> Option<(NativeDevice, NativeContact, PointerKind)> {
            let known = self.devices.get(&event.source);
            match known.map(|known| (known.role, known.label)) {
                Some((DeviceRole::Pen(tool), Some(label))) => {
                    Some((label, PEN, PointerKind::Pen { tool }))
                }
                Some((DeviceRole::Touchscreen, Some(label))) => Some((
                    label,
                    NativeContact(u64::from(event.detail)),
                    PointerKind::Touch,
                )),
                _ => {
                    let master = self.masters.get(&event.device)?;
                    Some((master.label, CURSOR, PointerKind::Mouse))
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

        /// The reading of `contact` doing `action` for `event` on `surface`.
        fn reading(
            event: &XiEvent,
            surface: NativeSurface,
            scale: f64,
            (device, contact, kind): (NativeDevice, NativeContact, PointerKind),
            action: ReadingAction,
        ) -> NativeReading {
            let reading = NativeReading::new(contact, kind, action, ReadingTime::epoch(event.time))
                .on(surface)
                .from_device(device)
                .with_modifiers(ActiveModifiers::from_x11_mask(event.modifiers).modifiers())
                .at(event.position.0, event.position.1, scale);
            if kind == PointerKind::Touch {
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
            let Some(target) = self.contact_of(event) else {
                return decoded;
            };
            let out = &mut decoded.inputs;
            let touch = target.2 == PointerKind::Touch;
            if !touch {
                self.switch_contact(event, surface, scale, target, out);
            }
            let sensors = self.sensors(event);
            let read =
                |action| Self::reading(event, surface, scale, target, action).with_sensors(sensors);
            match event.action {
                XiAction::Motion => {
                    // A scroll valuator's motion that leaves the cursor where
                    // it was moves nothing.
                    let moved = self
                        .contacts
                        .position(surface, Some(target.0), target.1)
                        .is_none_or(|(_, raw)| (raw.x, raw.y) != event.position);
                    if moved || !self.carries_scrolling(event) {
                        self.assemble(&read(ReadingAction::Move), out);
                    }
                    self.scroll_by_valuators(event, surface, target, out);
                }
                XiAction::ButtonPress => match wheel_button(event.detail) {
                    Some((x, y)) => {
                        self.scroll(
                            event,
                            surface,
                            target,
                            (x, y),
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
                    self.inside.insert((surface, target.0, target.1));
                }
                XiAction::Leave { grabbed } => {
                    let pressed = self
                        .contacts
                        .buttons(surface, Some(target.0), target.1)
                        .is_some_and(|buttons| !buttons.is_empty());
                    if grabbed && pressed {
                        self.assemble(&read(ReadingAction::Cancel(NativeCancel::Platform)), out);
                    }
                    decoded.hover = Some(false);
                    self.assemble(&read(ReadingAction::Leave), out);
                    self.inside.remove(&(surface, target.0, target.1));
                    if let Some(master) = self.masters.get_mut(&event.device) {
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
            }
            decoded
        }

        /// The master's cursor moves to `target` (a pen takes it over, or
        /// gives it back): the contact it moved before leaves, unless a
        /// button holds it, and `target` enters where the event is, unless
        /// the event is an enter of its own.
        fn switch_contact(
            &mut self,
            event: &XiEvent,
            surface: NativeSurface,
            scale: f64,
            target: (NativeDevice, NativeContact, PointerKind),
            out: &mut Vec<PlatformInput>,
        ) {
            let Some(master) = self.masters.get_mut(&event.device) else {
                return;
            };
            let previous = master.current.replace(target);
            let Some(previous) = previous.filter(|previous| *previous != target) else {
                return;
            };
            let held = self.contacts.buttons(surface, Some(previous.0), previous.1);
            if held.is_some_and(flui_platform::pointer::PointerButtons::is_empty) {
                let leave = Self::reading(event, surface, scale, previous, ReadingAction::Leave);
                self.assemble(&leave, out);
                self.inside.remove(&(surface, previous.0, previous.1));
            }
            let inside = self.inside.contains(&(surface, target.0, target.1));
            if !inside && !matches!(event.action, XiAction::Enter | XiAction::Leave { .. }) {
                let enter = Self::reading(event, surface, scale, target, ReadingAction::Enter);
                self.assemble(&enter, out);
                self.inside.insert((surface, target.0, target.1));
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
            target: (NativeDevice, NativeContact, PointerKind),
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
            target: (NativeDevice, NativeContact, PointerKind),
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
            let scroll = ContactScroll {
                surface,
                device: Some(target.0),
                contact: target.1,
                phase: None,
                delta,
                precision,
                time: event.time,
                modifiers: ActiveModifiers::from_x11_mask(event.modifiers).modifiers(),
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

    /// The layout at index `group` of the comma-separated layouts in
    /// `_XKB_RULES_NAMES` (rules, model, layouts, variants, options, each
    /// ended by a zero), with its variant: `us`, `de(nodeadkeys)`. Empty when
    /// the property says nothing of it.
    #[must_use]
    pub fn layout_name(property: &[u8], group: u32) -> String {
        let mut fields = property.split(|byte| *byte == 0);
        let layouts = fields.nth(2).unwrap_or_default();
        let variants = fields.next().unwrap_or_default();
        let nth = |list: &[u8]| {
            usize::try_from(group)
                .ok()
                .and_then(|group| list.split(|byte| *byte == b',').nth(group))
                .map(|item| String::from_utf8_lossy(item).trim().to_owned())
                .unwrap_or_default()
        };
        let (layout, variant) = (nth(layouts), nth(variants));
        if layout.is_empty() || variant.is_empty() {
            layout
        } else {
            format!("{layout}({variant})")
        }
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
    use std::ptr::NonNull;
    use std::rc::Rc;
    use std::sync::Arc;

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
        self, AtomEnum, ChangeWindowAttributesAux, ConnectionExt as _, NotifyMode,
    };

    use super::decode::{
        Axis, AxisLabel, DeviceDescription, ScrollAxis, X11Pointers, XiAction, XiEvent, layout_name,
    };
    use crate::input::assembly::{NativeCancel, NativeSurface};
    use crate::input::availability;
    use crate::input::event_timestamp_ns;
    use crate::input::linux::xkb::{Keyboard, KeyboardSource};
    use crate::owner::linux::x11::{InputTarget, LoopInput, WindowRef, X11Connection};
    use crate::owner::panic_boundary::contain_owner_callback;
    use crate::owner::slot::OwnerSlot;
    use crate::owner::window_delivery::WindowDelivery;

    /// The XInput 2 version the input speaks: 2.2 brought touch.
    const XI_VERSION: (u16, u16) = (2, 2);

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

    /// What the input keeps on the owner thread.
    struct State {
        pointers: X11Pointers,
        /// The open windows, by their X id.
        windows: HashMap<xproto::Window, InputTarget>,
        keyboard: Option<(Keyboard, KeyboardSource)>,
        cursors: Option<x11rb::cursor::Handle>,
        /// The cursors loaded so far.
        loaded: HashMap<CursorIcon, xproto::Cursor>,
    }

    /// The X11 loop's input: the windows' pointers, keys and cursors on the
    /// platform's connection.
    pub(crate) struct X11Input {
        connection: Arc<X11Connection>,
        /// XInput 2 answers, at the version the input speaks.
        xinput: bool,
        labels: LabelAtoms,
        state: OwnerSlot<Rc<RefCell<State>>>,
        /// The layout the keyboard is in, by the name `_XKB_RULES_NAMES`
        /// gives it.
        layout: Mutex<String>,
        layout_listeners: Mutex<Vec<Box<dyn FnMut() + Send>>>,
    }

    impl std::fmt::Debug for X11Input {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("X11Input")
                .field("xinput", &self.xinput)
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

    /// The valuators an event carries: the set bits of its mask, in order,
    /// with its values.
    fn valuators(mask: &[u32], values: &[Fp3232]) -> Vec<(u16, f64)> {
        let numbers = mask.iter().enumerate().flat_map(|(word, bits)| {
            (0..32_u16)
                .filter(move |bit| bits & (1 << bit) != 0)
                .map(move |bit| u16::try_from(word).map_or(u16::MAX, |word| word * 32 + bit))
        });
        numbers.zip(values.iter().copied().map(fp3232)).collect()
    }

    impl X11Input {
        /// The input of `connection`'s windows: XInput 2 set up on it, the
        /// devices listed, the keyboard read, and the device hierarchy and
        /// the keyboard's changes heard from here on.
        pub(crate) fn new(connection: &Arc<X11Connection>) -> Arc<Self> {
            let xcb = connection.xcb();
            let xinput = xcb
                .xinput_xi_query_version(XI_VERSION.0, XI_VERSION.1)
                .ok()
                .and_then(|cookie| cookie.reply().ok())
                .is_some_and(|reply| (reply.major_version, reply.minor_version) >= XI_VERSION);
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
            let input = Arc::new(Self {
                connection: Arc::clone(connection),
                xinput,
                labels,
                state: OwnerSlot::new(),
                layout: Mutex::new(String::new()),
                layout_listeners: Mutex::new(Vec::new()),
            });
            let state = State {
                pointers: X11Pointers::new(),
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
                // The devices present before any window opens: their
                // announcements reach no window, only the presence.
                let _ = input.read_devices();
            }
            input.read_layout();
            input
        }

        /// The core keyboard, its XKB events selected.
        fn keyboard(connection: &X11Connection) -> Option<(Keyboard, KeyboardSource)> {
            let raw = NonNull::new(connection.xcb().get_raw_xcb_connection())?;
            let Some(source) = KeyboardSource::x11_core_keyboard(raw) else {
                tracing::warn!("libxkbcommon-x11 or XKB is missing: X11 windows get no keys");
                return None;
            };
            let keyboard = Keyboard::new(source)?;
            let events = xkb::EventType::NEW_KEYBOARD_NOTIFY
                | xkb::EventType::MAP_NOTIFY
                | xkb::EventType::STATE_NOTIFY;
            let device = u16::try_from(source.device()).ok()?;
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
            Some((keyboard, source))
        }

        fn with_state<R>(&self, f: impl FnOnce(&mut State) -> R) -> Option<R> {
            let state = self.state.cloned()?;
            let mut state = state.borrow_mut();
            Some(f(&mut state))
        }

        /// The devices the server lists now, as the pure decoder reads them.
        fn devices(&self) -> Vec<DeviceDescription> {
            let Some(reply) = self
                .connection
                .xcb()
                .xinput_xi_query_device(xinput::Device::ALL)
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
                    for class in info.classes {
                        match class.data {
                            DeviceClassData::Valuator(valuator) => description.axes.push(Axis {
                                number: valuator.number,
                                label: self.labels.label(valuator.label),
                                min: fp3232(valuator.min),
                                max: fp3232(valuator.max),
                            }),
                            DeviceClassData::Scroll(scroll) => {
                                description.scrolls.push(ScrollAxis {
                                    number: scroll.number,
                                    horizontal: scroll.scroll_type
                                        == xinput::ScrollType::HORIZONTAL,
                                    increment: fp3232(scroll.increment),
                                });
                            }
                            DeviceClassData::Touch(touch) => {
                                description.direct_touch = touch.mode == xinput::TouchMode::DIRECT;
                            }
                            _ => {}
                        }
                    }
                    description
                })
                .collect()
        }

        /// Read the device list again: what changed goes to every window,
        /// with the kinds present.
        fn read_devices(&self) -> Vec<(Arc<WindowDelivery>, Delivery)> {
            let devices = self.devices();
            let time = EventTime::from_nanos(event_timestamp_ns());
            self.with_state(|state| {
                let changes = state.pointers.devices(&devices, time);
                let present = availability::x11().with_present(state.pointers.present());
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

        /// The layout names `_XKB_RULES_NAMES` lists (`us,ru`), read again,
        /// and the one the keyboard is in kept.
        fn read_layout(&self) -> bool {
            let xcb = self.connection.xcb();
            let names = xcb
                .intern_atom(true, b"_XKB_RULES_NAMES")
                .ok()
                .and_then(|cookie| cookie.reply().ok())
                .filter(|reply| reply.atom != 0)
                .and_then(|reply| {
                    xcb.get_property(
                        false,
                        self.connection.root(),
                        reply.atom,
                        AtomEnum::STRING,
                        0,
                        1024,
                    )
                    .ok()?
                    .reply()
                    .ok()
                })
                .map(|reply| reply.value)
                .unwrap_or_default();
            let group = self
                .with_state(|state| {
                    state
                        .keyboard
                        .as_ref()
                        .map_or(0, |(keyboard, _)| keyboard.layout())
                })
                .unwrap_or(0);
            let name = layout_name(&names, group);
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

        /// The XInput 2 event `event` carries, and the window it names.
        fn xi_event(event: &Event) -> Option<(xproto::Window, XiEvent)> {
            let time = EventTime::from_nanos(event_timestamp_ns());
            let pointer = |action, e: &xinput::ButtonPressEvent| {
                (
                    e.event,
                    XiEvent {
                        action,
                        device: e.deviceid,
                        source: e.sourceid,
                        detail: e.detail,
                        position: (fp1616(e.event_x), fp1616(e.event_y)),
                        emulated: u32::from(e.flags)
                            & u32::from(xinput::PointerEventFlags::POINTER_EMULATED)
                            != 0,
                        modifiers: e.mods.effective,
                        valuators: valuators(&e.valuator_mask, &e.axisvalues),
                        time,
                    },
                )
            };
            let touch = |action, e: &xinput::TouchBeginEvent| {
                (
                    e.event,
                    XiEvent {
                        action,
                        device: e.deviceid,
                        source: e.sourceid,
                        detail: e.detail,
                        position: (fp1616(e.event_x), fp1616(e.event_y)),
                        emulated: false,
                        modifiers: e.mods.effective,
                        valuators: valuators(&e.valuator_mask, &e.axisvalues),
                        time,
                    },
                )
            };
            let crossing = |action, e: &xinput::EnterEvent| {
                (
                    e.event,
                    XiEvent {
                        action,
                        device: e.deviceid,
                        source: e.sourceid,
                        detail: 0,
                        position: (fp1616(e.event_x), fp1616(e.event_y)),
                        emulated: false,
                        modifiers: e.mods.effective,
                        valuators: Vec::new(),
                        time,
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
                _ => return None,
            })
        }

        /// What a pointer or touch event delivers.
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
                let Some((keyboard, _)) = input.keyboard.as_mut() else {
                    return Vec::new();
                };
                let key = keyboard.key(event.detail, state, repeat, time);
                vec![(delivery, Delivery::Input(PlatformInput::Keyboard(key)))]
            })
            .unwrap_or_default()
        }

        /// The server's keyboard state changed: whether its layout did.
        fn keyboard_state(&self, event: &xkb::StateNotifyEvent) -> bool {
            self.with_state(|state| {
                let Some((keyboard, _)) = state.keyboard.as_mut() else {
                    return false;
                };
                let before = keyboard.layout();
                keyboard.update_mask(
                    u32::from(event.base_mods),
                    u32::from(event.latched_mods),
                    u32::from(event.locked_mods),
                    [
                        u32::from(event.base_group.cast_unsigned()),
                        u32::from(event.latched_group.cast_unsigned()),
                        u32::from(u8::from(event.locked_group)),
                    ],
                );
                keyboard.layout() != before
            })
            .unwrap_or(false)
        }

        /// The server's keymap changed: read it again.
        fn keymap_changed(&self) {
            self.with_state(|state| {
                if let Some((keyboard, source)) = state.keyboard.as_mut()
                    && !keyboard.reload(*source)
                {
                    tracing::debug!("the X11 keyboard's new keymap cannot be read");
                }
            });
        }

        /// The focus left `window`: its contacts end.
        fn focus_out(&self, window: xproto::Window) -> Vec<(Arc<WindowDelivery>, Delivery)> {
            let time = EventTime::from_nanos(event_timestamp_ns());
            self.with_state(|state| {
                let Some(target) = state.windows.get(&window) else {
                    return Vec::new();
                };
                let delivery = Arc::clone(&target.delivery);
                state
                    .pointers
                    .cancel_surface(surface_of(target.window), NativeCancel::FocusLost, time)
                    .into_iter()
                    .map(|input| (Arc::clone(&delivery), Delivery::Input(input)))
                    .collect()
            })
            .unwrap_or_default()
        }

        /// Select a window's XInput 2 events: every master device's pointer,
        /// touch and key events.
        fn select(&self, window: xproto::Window) {
            if !self.xinput {
                return;
            }
            let mask = XIEventMask::MOTION
                | XIEventMask::BUTTON_PRESS
                | XIEventMask::BUTTON_RELEASE
                | XIEventMask::ENTER
                | XIEventMask::LEAVE
                | XIEventMask::KEY_PRESS
                | XIEventMask::KEY_RELEASE
                | XIEventMask::TOUCH_BEGIN
                | XIEventMask::TOUCH_UPDATE
                | XIEventMask::TOUCH_END;
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

    impl LoopInput for X11Input {
        fn capabilities(&self) -> InputCapabilities {
            let present = self
                .with_state(|state| state.pointers.present())
                .unwrap_or_default();
            availability::x11().with_present(present)
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
                    let _ = state.pointers.cancel_surface(
                        surface_of(window),
                        NativeCancel::FocusLost,
                        time,
                    );
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
                Event::XinputHierarchy(_) => self.read_devices(),
                Event::XinputDeviceChanged(changed) => {
                    self.with_state(|state| state.pointers.reset_scrolling(changed.sourceid));
                    if changed.reason == xinput::ChangeReason::DEVICE_CHANGE {
                        self.read_devices()
                    } else {
                        Vec::new()
                    }
                }
                Event::XkbStateNotify(state) => {
                    if self.keyboard_state(state) && self.read_layout() {
                        self.layout_changed();
                    }
                    Vec::new()
                }
                Event::XkbNewKeyboardNotify(_) | Event::XkbMapNotify(_) => {
                    self.keymap_changed();
                    if self.read_layout() {
                        self.layout_changed();
                    }
                    Vec::new()
                }
                Event::FocusOut(focus) => {
                    if focus.mode == NotifyMode::NORMAL {
                        deliver(self.focus_out(focus.event));
                    }
                    // The window follows its focus too.
                    return false;
                }
                _ => match Self::xi_event(event) {
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
}
