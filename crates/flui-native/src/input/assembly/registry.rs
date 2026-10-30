//! Contact and device identity, and the pure decoder over them.

use std::num::NonZeroU64;

use flui_foundation::IdMint;
use flui_foundation::geometry::{Point, Size};
use flui_platform::EventTime;
use flui_platform::input_capabilities::PointerKindSet;
use flui_platform::pointer::{
    ButtonChange, CancelReason, ContactSize, DeviceId, PanZoomEvent, PenOrientation,
    PointerButtons, PointerCancel, PointerDeviceChange, PointerEvent, PointerId, PointerInfo,
    PointerKind, PointerMove, PointerPosition, PointerPress, PointerRelease, PointerRole,
    PointerSample, PointerSignal, Pressure, TangentialPressure, Twist,
};

use super::batch::MoveBatch;
use super::buttons::{ButtonTransition, ClickCounter, ClickSettings, HeldButtons};
use super::clock::{ClockChoice, NativeClock, ReadingTime};
use super::pan_zoom::{GesturePhase, PanZoomAccumulator};
use super::{
    Assembled, DecodeError, DecodeFault, Ending, NativeCancel, NativeContact, NativeDevice,
    NativeReading, NativeSurface, PenAngles, RawPoint, ReadingAction,
};
use flui_platform::PlatformInput;

/// Assemble one reading into owned input, committing it to `contacts`.
///
/// # Errors
///
/// [`DecodeFault::Orphan`] when a release, leave, cancel or gesture update
/// belongs to no open contact: nothing is emitted. Any other fault cancels the
/// reading's open contact with [`CancelReason::InvalidInput`]; the cancel is in
/// the error ([`DecodeError::into_inputs`]). Nothing else is committed.
pub fn decode(
    contacts: &mut ContactRegistry,
    reading: &NativeReading,
) -> Result<Assembled, DecodeError> {
    journal!(contacts.journal, super::capture::Captured::Reading(reading));
    contacts.assemble(reading, None)
}

/// [`decode`] for a reading that a call the registry already reported
/// derives, such as a touchpad viewport's pinch: replaying that call derives
/// it again, so it is not reported on its own.
pub(crate) fn decode_derived(
    contacts: &mut ContactRegistry,
    reading: &NativeReading,
) -> Result<Assembled, DecodeError> {
    contacts.assemble(reading, None)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ObservedKind {
    Unreported,
    Reported(PointerKind),
    Mixed,
}

impl ObservedKind {
    fn observe(&mut self, kind: PointerKind) {
        if kind == PointerKind::Unknown {
            return;
        }
        match *self {
            Self::Unreported => *self = Self::Reported(kind),
            Self::Reported(previous) if previous != kind => *self = Self::Mixed,
            Self::Reported(_) | Self::Mixed => {}
        }
    }

    fn reported(self) -> PointerKind {
        match self {
            Self::Reported(kind) => kind,
            Self::Unreported | Self::Mixed => PointerKind::Unknown,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct DeviceEntry {
    native: NativeDevice,
    id: DeviceId,
    kind: ObservedKind,
    clock: Option<ClockChoice>,
}

/// The input devices a backend has seen, each with a minted [`DeviceId`], the
/// kind its readings report and the clock its sequences use.
///
/// Identities are minted in order and never reissued: after the last one,
/// admission refuses permanently.
#[derive(Debug)]
pub struct DeviceRegistry {
    devices: Vec<DeviceEntry>,
    ids: IdMint<NonZeroU64>,
}

/// The last device identity hardware may take: those above it are the
/// reserved contactless devices (ADR-0207 §4).
const LAST_HARDWARE_DEVICE: u64 = DeviceId::KEYBOARD.get().get() - 1;

impl Default for DeviceRegistry {
    fn default() -> Self {
        Self::starting_at(NonZeroU64::MIN)
    }
}

impl DeviceRegistry {
    /// A registry minting identities from `first`; it never mints a
    /// reserved contactless device id, so one started among them is
    /// already exhausted.
    #[must_use]
    pub const fn starting_at(first: NonZeroU64) -> Self {
        Self {
            devices: Vec::new(),
            ids: IdMint::starting_after(first.get() - 1).ending_at(LAST_HARDWARE_DEVICE),
        }
    }

    /// The identity minted for `native`, while it is present.
    #[must_use]
    pub fn id(&self, native: NativeDevice) -> Option<DeviceId> {
        self.entry(native).map(|entry| entry.id)
    }

    /// The kind `native`'s readings report: [`PointerKind::Unknown`] before
    /// any reading or after readings of different kinds.
    #[must_use]
    pub fn kind(&self, native: NativeDevice) -> Option<PointerKind> {
        self.entry(native).map(|entry| entry.kind.reported())
    }

    /// The kinds of the devices present: what
    /// [`InputCapabilities::present`](flui_platform::input_capabilities::InputCapabilities::present)
    /// reports, from the same registry that mints the events' device ids. A
    /// device whose kind is not known yet, or whose readings disagree, counts
    /// as no kind.
    #[must_use]
    pub fn present(&self) -> PointerKindSet {
        self.devices
            .iter()
            .fold(PointerKindSet::empty(), |present, entry| {
                present | PointerKindSet::of(entry.kind.reported())
            })
    }

    /// The device whose identity is `id`, while it is present.
    #[must_use]
    pub fn native(&self, id: DeviceId) -> Option<NativeDevice> {
        self.devices
            .iter()
            .find(|entry| entry.id == id)
            .map(|entry| entry.native)
    }

    /// Announce `native` of `kind`; `None` when it is already present or no
    /// identity is left.
    pub fn add_device(
        &mut self,
        native: NativeDevice,
        kind: PointerKind,
        time: EventTime,
    ) -> Option<PointerDeviceChange> {
        let (id, added) = self.admit(native, kind)?;
        added.then(|| PointerDeviceChange::new(id, kind, time))
    }

    /// Retire `native`: its removal, if it was present.
    pub fn remove_device(
        &mut self,
        native: NativeDevice,
        time: EventTime,
    ) -> Option<PointerDeviceChange> {
        self.remove(native)
            .map(|entry| PointerDeviceChange::new(entry.id, entry.kind.reported(), time))
    }

    fn entry(&self, native: NativeDevice) -> Option<&DeviceEntry> {
        self.devices.iter().find(|entry| entry.native == native)
    }

    fn entry_mut(&mut self, native: NativeDevice) -> Option<&mut DeviceEntry> {
        self.devices.iter_mut().find(|entry| entry.native == native)
    }

    fn can_admit(&self, native: Option<NativeDevice>) -> bool {
        native.is_none_or(|native| self.entry(native).is_some() || !self.ids.is_exhausted())
    }

    /// Admit `native` reporting `kind`; the identity, and whether it is new.
    fn admit(&mut self, native: NativeDevice, kind: PointerKind) -> Option<(DeviceId, bool)> {
        if let Some(entry) = self.entry_mut(native) {
            entry.kind.observe(kind);
            return Some((entry.id, false));
        }
        // Hardware never takes a reserved contactless device id (ADR-0207 §4):
        // the mint ends below them.
        let next = self.ids.next().ok()?;
        let mut observed = ObservedKind::Unreported;
        observed.observe(kind);
        let id = DeviceId::new(next);
        self.devices.push(DeviceEntry {
            native,
            id,
            kind: observed,
            clock: None,
        });
        Some((id, true))
    }

    fn remove(&mut self, native: NativeDevice) -> Option<DeviceEntry> {
        let index = self
            .devices
            .iter()
            .position(|entry| entry.native == native)?;
        Some(self.devices.remove(index))
    }

    /// The clock a new sequence from `native` uses for a reading at `time`.
    fn sequence_clock(
        &self,
        native: Option<NativeDevice>,
        time: ReadingTime,
        clock: &NativeClock,
    ) -> ClockChoice {
        let supported = if clock.supports(time, ClockChoice::Counter) {
            ClockChoice::Counter
        } else {
            ClockChoice::Coarse
        };
        match native.and_then(|native| self.entry(native)?.clock) {
            Some(ClockChoice::Coarse) => ClockChoice::Coarse,
            Some(ClockChoice::Counter) | None => supported,
        }
    }

    /// Record the clock `native`'s sequences use: once a reading lacks the
    /// counter, the device stays on its coarse stamp.
    fn settle_clock(&mut self, native: Option<NativeDevice>, choice: ClockChoice) {
        if let Some(entry) = native.and_then(|native| self.entry_mut(native)) {
            entry.clock = Some(match entry.clock {
                Some(ClockChoice::Coarse) => ClockChoice::Coarse,
                Some(ClockChoice::Counter) | None => choice,
            });
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ContactKey {
    surface: NativeSurface,
    device: Option<NativeDevice>,
    contact: NativeContact,
}

impl ContactKey {
    const fn of(reading: &NativeReading) -> Self {
        Self {
            surface: reading.surface,
            device: reading.device,
            contact: reading.contact,
        }
    }
}

#[derive(Debug, Clone)]
struct Contact {
    key: ContactKey,
    info: PointerInfo,
    buttons: HeldButtons,
    clock: ClockChoice,
    last_time: Option<EventTime>,
    last_position: Option<(PointerPosition, RawPoint)>,
    origin: Option<NativeReading>,
    gesture: PanZoomAccumulator,
    /// Whether an `Enter` was delivered for it, which its removal pairs
    /// with a `Leave`.
    entered: bool,
}

impl Contact {
    /// A mouse is the one cursor of its device: its contact outlives each
    /// press sequence and each visit to the surface.
    fn persists(&self) -> bool {
        self.info.kind == PointerKind::Mouse
    }

    /// A mouse or pen can hover: its `Up` returns it to hovering on the same
    /// identity, and only the platform's leave (out of range, off the
    /// surface) ends a pen. A touch has no hover and ends at `Up`.
    fn hovers(&self) -> bool {
        matches!(self.info.kind, PointerKind::Mouse | PointerKind::Pen { .. })
    }

    /// A pen or a mouse hovering between sequences, whose label the platform
    /// may hand to another kind once it is gone without a leave: Android's
    /// `input` injector reports its mouse, pen and touch on one device and
    /// pointer id, and a hovering mouse sends no exit before a pen touches.
    fn hovering(&self) -> bool {
        matches!(self.info.kind, PointerKind::Pen { .. } | PointerKind::Mouse)
            && !self.buttons.in_sequence()
            && !self.gesture.is_active()
    }

    /// The event that ends this contact's sequence, if it has one in progress.
    fn ending(&self, time: EventTime, reason: CancelReason) -> Option<Ending> {
        if self.gesture.is_active() {
            let position = self.last_position?.0;
            let mut gesture = self.gesture;
            let phase = gesture.cancel()?;
            return Some(Ending::PanZoom(Box::new(PanZoomEvent::new(
                self.info, time, position, phase,
            ))));
        }
        self.buttons
            .in_sequence()
            .then(|| self.cancelled(time, reason))
    }

    /// The `Leave` that pairs this contact's `Enter` when the contact is
    /// removed at `time`: a touch ending at `Up`, or any contact a cancel or a
    /// retirement removes (a hovering pen or the mouse when the window loses
    /// focus). The platform's own later leave then finds no contact (as a DOM
    /// `pointerleave` follows `pointerup`).
    fn removal_leave(&self, time: EventTime) -> Option<PlatformInput> {
        self.entered.then(|| {
            let mut signal = PointerSignal::new(self.info, time);
            if let Some((position, _)) = self.last_position {
                signal = signal.with_position(position);
            }
            PlatformInput::Pointer(PointerEvent::Leave(signal))
        })
    }

    /// This contact's sequence ending with `reason`, in progress or not.
    fn cancelled(&self, time: EventTime, reason: CancelReason) -> Ending {
        Ending::Pointer(PointerCancel::new(self.info, time, reason))
    }
}

/// The open contacts of a backend's surfaces, with the devices, clock and
/// click settings they are assembled against.
#[derive(Debug)]
pub struct ContactRegistry {
    clock: NativeClock,
    devices: DeviceRegistry,
    contacts: Vec<Contact>,
    pointers: IdMint<NonZeroU64>,
    reserved: Option<NativeContact>,
    clicks: ClickCounter,
    #[cfg(all(debug_assertions, feature = "native-capture"))]
    pub(super) journal: super::capture::Journal,
}

const RESERVED_POINTER: NonZeroU64 = NonZeroU64::MIN;

/// The fingers one touch device keeps down at once. Panels report ten, the
/// largest twenty-odd; past this a device is minting touch ids it never
/// lifts, and each further press is refused rather than held forever.
pub const MAX_TOUCHES: usize = 32;

/// What a validated reading measured.
#[derive(Debug, Clone, Copy)]
struct Measured {
    time: EventTime,
    position: Option<(PointerPosition, RawPoint)>,
    sample: Option<PointerSample>,
}

impl ContactRegistry {
    /// A registry stamping readings on `clock`, minting pointer identities
    /// from one and counting no clicks.
    #[must_use]
    pub fn new(clock: NativeClock) -> Self {
        Self {
            clock,
            devices: DeviceRegistry::default(),
            contacts: Vec::new(),
            pointers: IdMint::new(),
            reserved: None,
            clicks: ClickCounter::default(),
            #[cfg(all(debug_assertions, feature = "native-capture"))]
            journal: super::capture::Journal::open(clock),
        }
    }

    /// Report `call`, made on this registry from outside it, to the native
    /// capture the registry reports to.
    #[cfg(all(debug_assertions, feature = "native-capture"))]
    pub(crate) fn report(&self, call: super::capture::Captured<'_>) {
        self.journal.record(call);
    }

    /// This registry with `contact` always given pointer identity one (the
    /// backend's primary mouse); others are minted from two.
    #[must_use]
    pub fn with_reserved_contact(mut self, contact: NativeContact) -> Self {
        journal!(
            self.journal,
            super::capture::Captured::ReservedContact(contact)
        );
        self.reserved = Some(contact);
        self.pointers.reserve_through(RESERVED_POINTER.get());
        self
    }

    /// This registry minting pointer identities from `first`.
    #[must_use]
    pub fn with_first_pointer(mut self, first: NonZeroU64) -> Self {
        journal!(self.journal, super::capture::Captured::FirstPointer(first));
        self.pointers = IdMint::starting_after(first.get() - 1);
        self
    }

    /// This registry minting device identities from `first`.
    #[must_use]
    pub fn with_first_device(mut self, first: NonZeroU64) -> Self {
        journal!(self.journal, super::capture::Captured::FirstDevice(first));
        self.devices = DeviceRegistry::starting_at(first);
        self
    }

    /// Count clicks by the OS `settings`, at start or after they changed;
    /// a click run in progress ends.
    pub fn set_click_settings(&mut self, settings: Option<ClickSettings>) {
        journal!(
            self.journal,
            super::capture::Captured::ClickSettings(settings)
        );
        self.clicks.set_settings(settings);
    }

    /// The clock readings are stamped on.
    #[must_use]
    pub const fn clock(&self) -> &NativeClock {
        &self.clock
    }

    /// The devices seen so far.
    #[must_use]
    pub const fn devices(&self) -> &DeviceRegistry {
        &self.devices
    }

    /// `time` on the event epoch by its coarse stamp, for events assembled
    /// outside a contact (wheel, keyboard).
    #[must_use]
    pub fn stamp(&self, time: ReadingTime) -> Option<EventTime> {
        self.clock.stamp(time, ClockChoice::Coarse)
    }

    /// The open contact `key` names. The reserved contact is the backend's
    /// one cursor whichever device drives it (a reading may name the mouse
    /// or none), so it is found by surface and label alone.
    fn find(&self, key: ContactKey) -> Option<usize> {
        let reserved = self.reserved == Some(key.contact);
        self.contacts.iter().position(|contact| {
            contact.key == key
                || (reserved
                    && contact.key.surface == key.surface
                    && contact.key.contact == key.contact)
        })
    }

    /// Whether `surface` has an open contact of `kind`.
    #[must_use]
    pub fn has_open_kind(&self, surface: NativeSurface, kind: PointerKind) -> bool {
        self.contacts
            .iter()
            .any(|contact| contact.key.surface == surface && contact.info.kind == kind)
    }

    /// The identity of the open contact `contact` of `device` on `surface`.
    #[must_use]
    pub fn pointer(
        &self,
        surface: NativeSurface,
        device: Option<NativeDevice>,
        contact: NativeContact,
    ) -> Option<PointerInfo> {
        let index = self.find(ContactKey {
            surface,
            device,
            contact,
        })?;
        Some(self.contacts[index].info)
    }

    /// The last position of that open contact, in logical pixels and as the
    /// device pixels it was read in.
    #[must_use]
    pub fn position(
        &self,
        surface: NativeSurface,
        device: Option<NativeDevice>,
        contact: NativeContact,
    ) -> Option<(PointerPosition, RawPoint)> {
        let index = self.find(ContactKey {
            surface,
            device,
            contact,
        })?;
        self.contacts[index].last_position
    }

    /// The buttons that open contact holds.
    #[must_use]
    pub fn buttons(
        &self,
        surface: NativeSurface,
        device: Option<NativeDevice>,
        contact: NativeContact,
    ) -> Option<PointerButtons> {
        let index = self.find(ContactKey {
            surface,
            device,
            contact,
        })?;
        Some(self.contacts[index].buttons.held())
    }

    /// The reading that started `pointer`'s current press sequence, kept
    /// until the sequence ends so a native drag can start from it.
    #[must_use]
    pub fn origin(&self, pointer: PointerId) -> Option<&NativeReading> {
        self.contacts
            .iter()
            .find(|contact| contact.info.id == pointer)?
            .origin
            .as_ref()
    }

    /// The native contact of a pen in range (touching or hovering): the one
    /// `device` reported, or for `None` the pen read most recently. A pen
    /// whose platform leave arrived is out of range and has none. What a pen
    /// actuator plays on (ADR-0210 §5).
    #[must_use]
    pub fn pen_contact(&self, device: Option<DeviceId>) -> Option<NativeContact> {
        let pens = self
            .contacts
            .iter()
            .filter(|contact| matches!(contact.info.kind, PointerKind::Pen { .. }));
        match device {
            Some(device) => pens
                .filter(|contact| contact.info.device == Some(device))
                .max_by_key(|contact| contact.last_time),
            None => pens.max_by_key(|contact| contact.last_time),
        }
        .map(|contact| contact.key.contact)
    }

    /// Whether `input`'s contact is still open, so it may still be delivered
    /// after reentrant user code ran in between. Events that end or need no
    /// contact are always delivered.
    #[must_use]
    pub fn delivers(&self, input: &PlatformInput) -> bool {
        if let PlatformInput::PointerFrame(frame) = input {
            return frame
                .events()
                .iter()
                .any(|event| self.delivers(&PlatformInput::Pointer(event.clone())));
        }
        let info = match input {
            PlatformInput::Pointer(
                PointerEvent::Down(press)
                | PointerEvent::ButtonChange(ButtonChange::Pressed(press)),
            ) => press.pointer,
            PlatformInput::Pointer(PointerEvent::ButtonChange(ButtonChange::Released(release))) => {
                release.pointer
            }
            PlatformInput::Pointer(PointerEvent::Move(movement)) => movement.pointer,
            PlatformInput::Pointer(PointerEvent::Enter(signal)) => signal.pointer,
            _ => return true,
        };
        self.contacts.iter().any(|contact| contact.info == info)
    }

    /// Announce `native` of `kind`; `None` when it is already present or no
    /// identity is left.
    pub fn add_device(
        &mut self,
        native: NativeDevice,
        kind: PointerKind,
        time: EventTime,
    ) -> Option<PointerDeviceChange> {
        journal!(
            self.journal,
            super::capture::Captured::AddDevice {
                device: native,
                kind,
                time,
            }
        );
        self.devices.add_device(native, kind, time)
    }

    /// Retire `native`: its contacts' sequences end with
    /// [`CancelReason::DeviceRemoved`], ordered by surface and pointer, and
    /// then the device's removal, if it was present.
    pub fn remove_device(
        &mut self,
        native: NativeDevice,
        time: EventTime,
    ) -> (
        Vec<(NativeSurface, PlatformInput)>,
        Option<PointerDeviceChange>,
    ) {
        journal!(
            self.journal,
            super::capture::Captured::RemoveDevice {
                device: native,
                time,
            }
        );
        let ended = self.retire(
            |key| key.device == Some(native),
            time,
            CancelReason::DeviceRemoved,
        );
        let removed = self.devices.remove_device(native, time);
        (ended, removed)
    }

    /// End every contact on `surface` with `reason`, ordered by pointer.
    pub fn cancel_surface(
        &mut self,
        surface: NativeSurface,
        time: EventTime,
        reason: NativeCancel,
    ) -> Vec<PlatformInput> {
        journal!(
            self.journal,
            super::capture::Captured::CancelSurface {
                surface,
                time,
                reason,
            }
        );
        self.retire(|key| key.surface == surface, time, reason.into())
            .into_iter()
            .map(|(_, input)| input)
            .collect()
    }

    /// End the open contacts labelled `contact` on `surface`, of any device,
    /// with `reason`: for a platform event that names only the label (a
    /// capture loss). Like every retirement, a contact with no sequence in
    /// progress closes without an event.
    pub fn cancel_contact(
        &mut self,
        surface: NativeSurface,
        contact: NativeContact,
        time: EventTime,
        reason: NativeCancel,
    ) -> Vec<PlatformInput> {
        journal!(
            self.journal,
            super::capture::Captured::CancelContact {
                surface,
                contact,
                time,
                reason,
            }
        );
        self.retire(
            |key| key.surface == surface && key.contact == contact,
            time,
            reason.into(),
        )
        .into_iter()
        .map(|(_, input)| input)
        .collect()
    }

    fn retire(
        &mut self,
        mut matches: impl FnMut(&ContactKey) -> bool,
        time: EventTime,
        reason: CancelReason,
    ) -> Vec<(NativeSurface, PlatformInput)> {
        let mut ended = Vec::new();
        self.contacts.retain(|contact| {
            if !matches(&contact.key) {
                return true;
            }
            let time = contact.last_time.map_or(time, |last| last.max(time));
            if let Some(ending) = contact.ending(time, reason) {
                ended.push((contact.key.surface, contact.info.id, ending.into_input()));
            }
            if let Some(leave) = contact.removal_leave(time) {
                ended.push((contact.key.surface, contact.info.id, leave));
            }
            false
        });
        // Stable: a touch's cancel stays before its leave.
        ended.sort_by_key(|(surface, id, _)| (*surface, *id));
        ended
            .into_iter()
            .map(|(surface, _, input)| (surface, input))
            .collect()
    }

    /// Validate `reading` against its contact's clock without committing. A
    /// reading without a position is measured at `fallback`, the contact's last.
    fn measure(
        &self,
        reading: &NativeReading,
        choice: ClockChoice,
        floor: Option<EventTime>,
        fallback: Option<(PointerPosition, RawPoint)>,
    ) -> Result<Measured, DecodeFault> {
        let time = self
            .clock
            .stamp(reading.time, choice)
            .ok_or(DecodeFault::Time)?;
        let time = floor.map_or(time, |floor| time.max(floor));
        let needs_scale = reading.position.is_some() || reading.sensors.contact_size.is_some();
        if needs_scale && !(reading.scale.is_finite() && reading.scale > 0.0) {
            return Err(DecodeFault::Scale(reading.scale));
        }
        let position = reading
            .position
            .map(|raw| {
                PointerPosition::try_new(Point::new(raw.x / reading.scale, raw.y / reading.scale))
                    .map(|position| (position, raw))
            })
            .transpose()?
            .or(fallback);
        let sample = position
            .map(|(position, _)| sample(time, position, reading))
            .transpose()?;
        Ok(Measured {
            time,
            position,
            sample,
        })
    }

    /// Open a contact for `reading`, announcing its device if it is new; its
    /// index, or `None` when no pointer or device identity is left.
    fn admit(
        &mut self,
        reading: &NativeReading,
        clock: ClockChoice,
        time: EventTime,
        output: &mut Assembled,
    ) -> Option<usize> {
        let reserved = self.reserved == Some(reading.contact);
        if (!reserved && self.pointers.is_exhausted()) || !self.devices.can_admit(reading.device) {
            return None;
        }
        if reading.kind == PointerKind::Touch && self.touches_of(reading.device) >= MAX_TOUCHES {
            return None;
        }
        let id = if reserved {
            RESERVED_POINTER
        } else {
            self.pointers.next().ok()?
        };
        let shares_kind = self.contacts.iter().any(|contact| {
            contact.key.surface == reading.surface
                && std::mem::discriminant(&contact.info.kind)
                    == std::mem::discriminant(&reading.kind)
        });
        let role = if reading.primary.unwrap_or(!shares_kind) {
            PointerRole::Primary
        } else {
            PointerRole::Additional
        };
        let mut info = PointerInfo::new(PointerId::new(id), reading.kind).with_role(role);
        if let Some(native) = reading.device {
            let (device, added) = self.devices.admit(native, reading.kind)?;
            info = info.with_device(device);
            if added {
                output.push(PlatformInput::Pointer(PointerEvent::DeviceAdded(
                    PointerDeviceChange::new(device, reading.kind, time),
                )));
            }
        }
        self.contacts.push(Contact {
            key: ContactKey::of(reading),
            info,
            buttons: HeldButtons::for_kind(reading.kind),
            clock,
            last_time: None,
            last_position: None,
            origin: None,
            gesture: PanZoomAccumulator::default(),
            entered: false,
        });
        Some(self.contacts.len() - 1)
    }

    /// The touch contacts `device` holds open.
    fn touches_of(&self, device: Option<NativeDevice>) -> usize {
        self.contacts
            .iter()
            .filter(|contact| {
                contact.info.kind == PointerKind::Touch && contact.key.device == device
            })
            .count()
    }

    /// When `reading` ends `contact`: its own stamp when it has one, never
    /// before the contact's last.
    fn ending_time(&self, contact: &Contact, reading: &NativeReading) -> EventTime {
        let floor = contact.last_time.unwrap_or(EventTime::from_nanos(0));
        self.clock
            .stamp(reading.time, contact.clock)
            .map_or(floor, |time| time.max(floor))
    }

    /// Fail `reading`: end its open contact, if any, as invalid input.
    fn fail(
        &mut self,
        index: Option<usize>,
        reading: &NativeReading,
        fault: DecodeFault,
    ) -> DecodeError {
        let cancel = index.map(|index| {
            let contact = self.contacts.remove(index);
            let time = self.ending_time(&contact, reading);
            contact
                .ending(time, CancelReason::InvalidInput)
                .unwrap_or_else(|| contact.cancelled(time, CancelReason::InvalidInput))
        });
        if cancel.is_some() {
            tracing::warn!(%fault, contact = ?reading.contact, "pointer reading refused; its contact is cancelled");
        } else {
            tracing::debug!(%fault, contact = ?reading.contact, "pointer reading refused");
        }
        DecodeError { fault, cancel }
    }

    /// The decoder behind [`decode`] and [`MoveBatch::decode`].
    pub(super) fn assemble(
        &mut self,
        reading: &NativeReading,
        batch: Option<&mut MoveBatch>,
    ) -> Result<Assembled, DecodeError> {
        let key = ContactKey::of(reading);
        let mut index = self.find(key);
        // A hovering pen or mouse whose label comes back as another kind left
        // without a leave: its hover ends with one, and the reading is a new
        // contact.
        let mut stale = None;
        if let Some(found) = index
            && self.contacts[found].info.kind != reading.kind
        {
            if !self.contacts[found].hovering() {
                return Err(self.fail(Some(found), reading, DecodeFault::IdentityChanged));
            }
            stale = index.take();
        }
        let needs_open = match reading.action {
            ReadingAction::Leave | ReadingAction::Release(_) | ReadingAction::Cancel(_) => true,
            ReadingAction::Move => reading.kind == PointerKind::Touch,
            ReadingAction::Gesture(gesture) => gesture.phase != GesturePhase::Began,
            ReadingAction::Enter | ReadingAction::Press(_) => false,
        };
        if index.is_none() && needs_open {
            // A removed contact's `Leave` was delivered when it was removed;
            // the platform's own leave that follows is expected, and refused silently.
            if reading.action == ReadingAction::Leave {
                return Err(DecodeError {
                    fault: DecodeFault::Orphan,
                    cancel: None,
                });
            }
            return Err(self.fail(None, reading, DecodeFault::Orphan));
        }
        let (choice, floor) = match index {
            Some(index) => (self.contacts[index].clock, self.contacts[index].last_time),
            None => (
                self.devices
                    .sequence_clock(reading.device, reading.time, &self.clock),
                None,
            ),
        };
        if let ReadingAction::Cancel(reason) = reading.action {
            let reason = CancelReason::from(reason);
            let index = index.expect("BUG: a cancel needs an open contact");
            let time = self.ending_time(&self.contacts[index], reading);
            let contact = &mut self.contacts[index];
            // A mouse stays the device's cursor: only a sequence in progress
            // ends. Any other contact closes, with or without one.
            let ending = if contact.persists() {
                let ending = contact.ending(time, reason);
                contact.buttons.clear();
                contact.origin = None;
                contact.gesture = PanZoomAccumulator::default();
                contact.last_time = Some(time);
                ending
            } else {
                let contact = self.contacts.remove(index);
                let ending = contact
                    .ending(time, reason)
                    .unwrap_or_else(|| contact.cancelled(time, reason));
                return Ok([Some(ending.into_input()), contact.removal_leave(time)]
                    .into_iter()
                    .flatten()
                    .collect());
            };
            return Ok(ending.map(Ending::into_input).into_iter().collect());
        }
        let mut batch = batch;
        let coalesced_floor = match batch.as_deref_mut() {
            Some(batch) => match batch.measure(&self.clock, choice, floor) {
                Ok(floor) => floor,
                Err(fault) => return Err(self.fail(index, reading, fault)),
            },
            None => floor,
        };
        let fallback = index.and_then(|index| self.contacts[index].last_position);
        let measured = match self.measure(reading, choice, coalesced_floor, fallback) {
            Ok(measured) => measured,
            Err(fault) => return Err(self.fail(index, reading, fault)),
        };
        let mut output = Assembled::new();
        let index = if let Some(index) = index {
            index
        } else {
            let ended = stale.map(|stale| (stale, self.contacts.remove(stale)));
            if let Some((_, hover)) = &ended {
                let time = hover
                    .last_time
                    .map_or(measured.time, |last| last.max(measured.time));
                let mut signal = PointerSignal::new(hover.info, time);
                if let Some((position, _)) = hover.last_position {
                    signal = signal.with_position(position);
                }
                output.push(PlatformInput::Pointer(PointerEvent::Leave(signal)));
            }
            let Some(index) = self.admit(reading, choice, measured.time, &mut output) else {
                if let Some((stale, hover)) = ended {
                    self.contacts.insert(stale, hover);
                }
                return Err(self.fail(None, reading, DecodeFault::Exhausted));
            };
            index
        };
        if let Some(native) = reading.device {
            let admitted = self.devices.admit(native, reading.kind);
            let contact = &mut self.contacts[index];
            // Only the reserved contact is found across devices: it carries
            // the device that drives it now. A reading naming none keeps it.
            if let Some((device, added)) = admitted
                && contact.key.device != Some(native)
            {
                contact.key.device = Some(native);
                contact.info = contact.info.with_device(device);
                if added {
                    output.push(PlatformInput::Pointer(PointerEvent::DeviceAdded(
                        PointerDeviceChange::new(device, reading.kind, measured.time),
                    )));
                }
            }
            let used = if self.clock.supports(reading.time, choice) {
                choice
            } else {
                ClockChoice::Coarse
            };
            self.devices.settle_clock(Some(native), used);
        }
        let contact = &mut self.contacts[index];
        contact.last_time = Some(measured.time);
        let previous_position = contact.last_position.map(|(position, _)| position);
        if let Some(position) = measured.position {
            contact.last_position = Some(position);
        }
        let info = contact.info;
        let time = measured.time;
        let modifiers = reading.modifiers;
        let current = measured.sample;
        let mut remove = false;
        match reading.action {
            ReadingAction::Enter | ReadingAction::Leave => {
                let mut signal = PointerSignal::new(info, time);
                if let Some((position, _)) = contact.last_position {
                    signal = signal.with_position(position);
                }
                if reading.action == ReadingAction::Enter {
                    contact.entered = true;
                    output.push(PlatformInput::Pointer(PointerEvent::Enter(signal)));
                } else {
                    // Its leave is delivered: a later removal has none to pair.
                    contact.entered = false;
                    if !contact.persists() {
                        if contact.buttons.in_sequence() {
                            output.push(PlatformInput::Pointer(PointerEvent::Cancel(
                                PointerCancel::new(info, time, CancelReason::Platform),
                            )));
                        }
                        remove = true;
                    }
                    output.push(PlatformInput::Pointer(PointerEvent::Leave(signal)));
                }
            }
            ReadingAction::Move => {
                if let Some(reported) = reading.buttons
                    && contact.buttons.forget_lost_releases(reported)
                {
                    // Where the release happened is unknown: the press is
                    // taken back, not completed here.
                    contact.origin = None;
                    contact.buttons.clear();
                    output.push(PlatformInput::Pointer(PointerEvent::Cancel(
                        PointerCancel::new(info, time, CancelReason::Platform),
                    )));
                }
                if let Some(sample) = measured.sample {
                    let held = reading.buttons.unwrap_or(contact.buttons.held());
                    let mut movement =
                        PointerMove::new(info, held, sample).with_modifiers(modifiers);
                    if let Some(batch) = batch {
                        movement = batch.attach(movement);
                    }
                    output.push(PlatformInput::Pointer(PointerEvent::Move(movement)));
                }
            }
            ReadingAction::Press(button) => {
                if let Some(sample) = current {
                    let transition = contact.buttons.press_transition(button);
                    if transition != ButtonTransition::Ignored {
                        let before = reading.buttons.unwrap_or(contact.buttons.held());
                        let mut press = PointerPress::new(info, button, before, sample)
                            .with_modifiers(modifiers);
                        let raw = contact
                            .last_position
                            .map_or(RawPoint { x: 0.0, y: 0.0 }, |(_, raw)| raw);
                        let count = reading.click_count.or_else(|| {
                            (info.kind == PointerKind::Mouse)
                                .then(|| self.clicks.press(info.id, button, time, raw))?
                        });
                        if let Some(count) = count {
                            press = press.with_click_count(count);
                        }
                        let contact = &mut self.contacts[index];
                        contact.buttons.press(button, reading.buttons);
                        output.push(PlatformInput::Pointer(
                            if transition == ButtonTransition::Down {
                                contact.origin = Some(*reading);
                                PointerEvent::Down(press)
                            } else {
                                PointerEvent::ButtonChange(ButtonChange::Pressed(press))
                            },
                        ));
                    }
                } else {
                    contact.buttons.suppress(button);
                }
            }
            ReadingAction::Release(button) => {
                let transition = contact.buttons.release_transition(button, reading.buttons);
                let before = reading.buttons.unwrap_or(contact.buttons.held());
                contact.buttons.release(button, reading.buttons);
                if let Some(sample) = current
                    && transition != ButtonTransition::Ignored
                {
                    let release =
                        PointerRelease::new(info, button, before, sample).with_modifiers(modifiers);
                    if transition == ButtonTransition::Up {
                        contact.origin = None;
                        contact.buttons.clear();
                        remove = !contact.hovers();
                        output.push(PlatformInput::Pointer(PointerEvent::Up(release)));
                        if remove {
                            output.extend(contact.removal_leave(sample.time));
                        }
                    } else {
                        output.push(PlatformInput::Pointer(PointerEvent::ButtonChange(
                            ButtonChange::Released(release),
                        )));
                    }
                }
            }
            ReadingAction::Gesture(gesture) => {
                if let Some((position, _)) = contact.last_position {
                    let focal_moved =
                        previous_position.is_some_and(|previous| previous != position);
                    for phase in contact.gesture.apply(
                        gesture.component,
                        gesture.phase,
                        gesture.delta,
                        focal_moved,
                    ) {
                        output.push(PlatformInput::Pointer(PointerEvent::PanZoom(
                            PanZoomEvent::new(info, time, position, phase)
                                .with_modifiers(modifiers),
                        )));
                    }
                }
                remove = !contact.gesture.is_active();
            }
            ReadingAction::Cancel(_) => unreachable!("BUG: cancels return before measuring"),
        }
        if remove {
            self.contacts.remove(index);
        }
        Ok(output)
    }
}

/// The sample `reading` measured at `position` and `time`.
pub(super) fn sample(
    time: EventTime,
    position: PointerPosition,
    reading: &NativeReading,
) -> Result<PointerSample, DecodeFault> {
    let sensors = reading.sensors;
    let mut sample = PointerSample::new(time, position);
    if let Some(pressure) = sensors.pressure {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "the f32 sensor representation"
        )]
        let pressure = pressure as f32;
        sample = sample.with_pressure(Pressure::try_new(pressure)?);
    }
    if let Some(pressure) = sensors.tangential_pressure {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "the f32 sensor representation"
        )]
        let pressure = pressure as f32;
        sample = sample.with_tangential_pressure(TangentialPressure::try_new(pressure)?);
    }
    if let Some(angles) = sensors.angles
        && let Some(orientation) = orientation(angles)?
    {
        sample = sample.with_orientation(orientation);
    }
    if let Some(degrees) = sensors.twist_degrees {
        if !(0.0..360.0).contains(&degrees) {
            return Err(DecodeFault::Angles);
        }
        sample = sample.with_twist(Twist::try_new(degrees.to_radians())?);
    }
    if let Some((width, height)) = sensors.contact_size {
        sample = sample.with_contact_size(ContactSize::try_new(Size::new(
            width / reading.scale,
            height / reading.scale,
        ))?);
    }
    Ok(sample)
}

fn orientation(angles: PenAngles) -> Result<Option<PenOrientation>, DecodeFault> {
    Ok(match angles {
        PenAngles::TiltDegrees { x, y } => {
            if !(-90.0..=90.0).contains(&x) || !(-90.0..=90.0).contains(&y) {
                return Err(DecodeFault::Angles);
            }
            let x = x.to_radians().tan();
            let y = y.to_radians().tan();
            let distance = x.hypot(y);
            Some(if distance == 0.0 {
                PenOrientation::try_altitude(std::f64::consts::FRAC_PI_2)?
            } else {
                PenOrientation::try_new(
                    1.0_f64.atan2(distance),
                    y.atan2(x).rem_euclid(std::f64::consts::TAU),
                )?
            })
        }
        PenAngles::Spherical { altitude, azimuth } => match (altitude, azimuth) {
            (Some(altitude), Some(azimuth)) => Some(PenOrientation::try_new(altitude, azimuth)?),
            (Some(altitude), None) => Some(PenOrientation::try_altitude(altitude)?),
            (None, Some(azimuth)) => Some(PenOrientation::try_azimuth(azimuth)?),
            (None, None) => None,
        },
    })
}
