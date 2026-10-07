//! FLUI's pointer event vocabulary (ADR-0089 §4, ADR-0143).
//!
//! A backend turns each native mouse, touch, pen and trackpad message into a [`PointerEvent`].
//! The vocabulary is FLUI's own, so no upstream input crate's release reaches a Stable
//! signature, and it carries what the W3C Pointer Events model and the platforms report:
//!
//! - **Identity.** A [`PointerId`] names one contact (a finger, a pen, the mouse) for as long as
//!   it is down or in range; a [`DeviceId`] names the hardware behind it for as long as it is
//!   connected; [`PointerRole`] says whether the contact is its kind's primary one.
//! - **Readings.** A [`PointerSample`] holds the position ([`PointerPosition`], logical pixels,
//!   ADR-0098), the time on the shared monotonic timeline ([`EventTime`]) and the sensors the
//!   device has. A missing sensor is `None`, never a stand-in value. A move also carries the
//!   samples the platform coalesced into it and the ones it predicts.
//! - **Sequences.** A contact is `Down`, any number of `Move`s and `ButtonChange`s, then `Up`
//!   or `Cancel`. A sequence never just stops: losing the window's capture, the device or the
//!   gesture to the system ends it with a [`PointerCancel`] that says why.
//! - **Scrolling and trackpad gestures.** [`ScrollEvent`] keeps the delta's unit, its
//!   precision and its gesture phase; [`PanZoomEvent`] carries a trackpad's pan, pinch and
//!   rotation as one cumulative transform between `Start` and `End`.
//!
//! Every number is a validated newtype ([`PointerPosition`], [`Pressure`], [`PenOrientation`],
//! [`ScrollDelta`], [`PanZoomTransform`], …) whose fallible constructor states its NaN,
//! infinity and range policy and returns an [`InputValueError`], so no NaN or infinity reaches
//! hit-testing, velocity tracking or layout.
//!
//! # Examples
//!
//! A backend building a mouse press:
//!
//! ```
//! use flui_foundation::geometry::Point;
//! use flui_platform_api::EventTime;
//! use flui_platform_api::keyboard::Modifiers;
//! use flui_platform_api::pointer::{
//!     PointerButton, PointerButtons, PointerEvent, PointerId, PointerInfo, PointerKind,
//!     PointerPosition, PointerPress, PointerRole, PointerSample,
//! };
//!
//! let mouse = PointerInfo::new(PointerId::try_from(1_u64)?, PointerKind::Mouse)
//!     .with_role(PointerRole::Primary);
//! let at = PointerSample::new(
//!     EventTime::from_nanos(1_000),
//!     PointerPosition::try_new(Point::new(12.0, 30.5))?,
//! );
//! let press = PointerEvent::Down(
//!     PointerPress::new(mouse, PointerButton::PRIMARY, PointerButtons::NONE, at)
//!         .with_modifiers(Modifiers::SHIFT),
//! );
//! let PointerEvent::Down(down) = &press else { unreachable!() };
//! assert!(down.buttons().contains(PointerButton::PRIMARY));
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

mod buttons;
mod pan_zoom;
mod sample;
mod scroll;
mod value;

use core::marker::PhantomData;
use core::num::{NonZeroU8, NonZeroU64, TryFromIntError};

pub use buttons::{InvalidButtonNumber, PointerButton, PointerButtons};
pub use pan_zoom::{PanZoomEvent, PanZoomPhase, PanZoomTransform};
pub use sample::PointerSample;
pub use scroll::{ScrollDelta, ScrollEvent, ScrollPhase, ScrollPrecision, ScrollUnit};
pub use value::{
    ContactSize, InputValueError, PenOrientation, PointerPosition, Pressure, Quantity,
    TangentialPressure, Twist,
};

use crate::EventTime;
use crate::keyboard::Modifiers;

/// One contact: a finger, a pen, the mouse, a trackpad gesture.
///
/// Unique among the window's active pointers while the contact is down or in range. A backend
/// may reuse an id once its sequence has ended with `Up`, `Cancel` or `Leave`, so a consumer
/// that keeps per-pointer state drops it at the end of the sequence. The id says nothing about
/// the contact's kind or whether it is primary: [`PointerInfo`] carries both.
///
/// # Examples
///
/// ```
/// use flui_platform_api::pointer::PointerId;
///
/// let finger = PointerId::try_from(7_u64)?;
/// assert_eq!(finger.get().get(), 7);
/// assert!(PointerId::try_from(0_u64).is_err());
/// # Ok::<(), core::num::TryFromIntError>(())
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PointerId(NonZeroU64);

impl PointerId {
    /// The id `value`.
    #[must_use]
    pub const fn new(value: NonZeroU64) -> Self {
        Self(value)
    }

    /// The id's value.
    #[must_use]
    pub const fn get(self) -> NonZeroU64 {
        self.0
    }
}

impl From<NonZeroU64> for PointerId {
    fn from(value: NonZeroU64) -> Self {
        Self(value)
    }
}

impl From<PointerId> for NonZeroU64 {
    fn from(id: PointerId) -> Self {
        id.0
    }
}

impl TryFrom<u64> for PointerId {
    type Error = TryFromIntError;

    /// The id `value`.
    ///
    /// # Errors
    ///
    /// [`TryFromIntError`] for zero.
    fn try_from(value: u64) -> Result<Self, Self::Error> {
        NonZeroU64::try_from(value).map(Self)
    }
}

/// The hardware behind a pointer: one mouse, one pen, one touch screen.
///
/// Stable for as long as the device stays connected, so two mice or two pens can be told
/// apart. Whether the same physical device keeps its id across a reconnection is up to the
/// platform.
///
/// # Examples
///
/// ```
/// use flui_platform_api::pointer::DeviceId;
///
/// let pen = DeviceId::try_from(0x2a_u64)?;
/// assert_eq!(pen.get().get(), 0x2a);
/// # Ok::<(), core::num::TryFromIntError>(())
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DeviceId(NonZeroU64);

impl DeviceId {
    /// The id `value`.
    #[must_use]
    pub const fn new(value: NonZeroU64) -> Self {
        Self(value)
    }

    /// The id's value.
    #[must_use]
    pub const fn get(self) -> NonZeroU64 {
        self.0
    }
}

impl From<NonZeroU64> for DeviceId {
    fn from(value: NonZeroU64) -> Self {
        Self(value)
    }
}

impl From<DeviceId> for NonZeroU64 {
    fn from(id: DeviceId) -> Self {
        id.0
    }
}

impl TryFrom<u64> for DeviceId {
    type Error = TryFromIntError;

    /// The id `value`.
    ///
    /// # Errors
    ///
    /// [`TryFromIntError`] for zero.
    fn try_from(value: u64) -> Result<Self, Self::Error> {
        NonZeroU64::try_from(value).map(Self)
    }
}

/// Which end of a pen is in use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum PenTool {
    /// The writing tip.
    #[default]
    Tip,
    /// The eraser end. It can come into range, hover and touch exactly like the tip, and the
    /// user can flip the pen mid-hover, so it is the pen's tool, not a button.
    Eraser,
}

/// The kind of device behind a pointer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum PointerKind {
    /// A mouse or other indirect cursor device.
    Mouse,
    /// A finger on a touch screen.
    Touch,
    /// A pen or stylus, with the end in use.
    Pen {
        /// The tip or the eraser.
        tool: PenTool,
    },
    /// A trackpad gesture ([`PanZoomEvent`]). A trackpad that moves the cursor reports as
    /// [`Mouse`](Self::Mouse).
    Trackpad,
    /// The platform did not say.
    Unknown,
}

/// Whether a contact is its kind's primary one: the W3C `isPrimary`.
///
/// The mouse, the first finger down while no other finger touches and the first pen in range
/// are primary. A consumer that follows only one contact follows the primary one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PointerRole {
    /// The primary contact of its kind.
    Primary,
    /// Any further contact.
    #[default]
    Additional,
}

/// Who an event comes from: the contact, its device and its kind.
///
/// Built with [`PointerInfo::new`], [`with_device`](Self::with_device) and
/// [`with_role`](Self::with_role).
///
/// # Examples
///
/// ```
/// use flui_platform_api::pointer::{DeviceId, PenTool, PointerId, PointerInfo, PointerKind, PointerRole};
///
/// let pen = PointerInfo::new(PointerId::try_from(3_u64)?, PointerKind::Pen { tool: PenTool::Eraser })
///     .with_device(DeviceId::try_from(9_u64)?)
///     .with_role(PointerRole::Primary);
/// assert!(pen.is_primary());
/// # Ok::<(), core::num::TryFromIntError>(())
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct PointerInfo {
    /// The contact.
    pub id: PointerId,
    /// The hardware behind it, when the platform reports it.
    pub device: Option<DeviceId>,
    /// The kind of device.
    pub kind: PointerKind,
    /// Whether this is its kind's primary contact.
    pub role: PointerRole,
}

impl PointerInfo {
    /// An [additional](PointerRole::Additional) contact `id` of `kind`, with no device id.
    #[must_use]
    pub const fn new(id: PointerId, kind: PointerKind) -> Self {
        Self {
            id,
            device: None,
            kind,
            role: PointerRole::Additional,
        }
    }

    /// This contact, reported by `device`.
    #[must_use]
    pub const fn with_device(self, device: DeviceId) -> Self {
        Self {
            device: Some(device),
            ..self
        }
    }

    /// This contact in `role`.
    #[must_use]
    pub const fn with_role(self, role: PointerRole) -> Self {
        Self { role, ..self }
    }

    /// Whether this is its kind's primary contact.
    #[must_use]
    pub const fn is_primary(&self) -> bool {
        matches!(self.role, PointerRole::Primary)
    }
}

mod sealed {
    pub trait Sealed {}
}

/// The direction of a [`PointerButtonEvent`], fixed in its type: [`Press`] or [`Release`].
///
/// Sealed: the two directions are the only ones, so a `Down` can only ever carry a press and
/// an `Up` a release.
pub trait ButtonDirection: sealed::Sealed + Copy + core::fmt::Debug + PartialEq {
    /// Whether the event's button went down.
    const PRESSED: bool;
}

/// A button went down: the direction of [`PointerPress`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Press;

/// A button came up: the direction of [`PointerRelease`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Release;

impl sealed::Sealed for Press {}
impl sealed::Sealed for Release {}

impl ButtonDirection for Press {
    const PRESSED: bool = true;
}

impl ButtonDirection for Release {
    const PRESSED: bool = false;
}

/// A button went down.
pub type PointerPress = PointerButtonEvent<Press>;

/// A button came up.
pub type PointerRelease = PointerButtonEvent<Release>;

/// A button went down or up on a pointer, the direction `D` fixed by the type (typestate).
///
/// `buttons` is the set held **after** the change: on a press it contains `button`, on a
/// release it does not. [`PointerButtonEvent::new`] makes that true whatever the platform
/// reported. [`PointerEvent::Down`] takes only a [`PointerPress`] and [`PointerEvent::Up`]
/// only a [`PointerRelease`], so a release cannot start a sequence.
///
/// # Examples
///
/// ```compile_fail
/// # use flui_foundation::geometry::Point;
/// # use flui_platform_api::EventTime;
/// # use flui_platform_api::pointer::*;
/// # let info = PointerInfo::new(PointerId::try_from(1_u64).unwrap(), PointerKind::Mouse);
/// # let at = PointerSample::new(EventTime::from_nanos(0), PointerPosition::try_new(Point::new(0.0, 0.0)).unwrap());
/// let release = PointerRelease::new(info, PointerButton::PRIMARY, PointerButtons::NONE, at);
/// let _ = PointerEvent::Down(release); // a release is not a press
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct PointerButtonEvent<D: ButtonDirection> {
    /// Who pressed or released.
    pub pointer: PointerInfo,
    button: PointerButton,
    buttons: PointerButtons,
    /// The modifiers held.
    pub modifiers: Modifiers,
    /// The platform's click count for a press (2 for a double-click), or `None` when the
    /// platform does not count clicks and the framework's recognizers decide.
    pub click_count: Option<NonZeroU8>,
    /// Where and when it happened.
    pub sample: PointerSample,
    direction: PhantomData<D>,
}

impl<D: ButtonDirection> PointerButtonEvent<D> {
    /// The button that changed.
    #[must_use]
    pub fn button(&self) -> PointerButton {
        self.button
    }

    /// The buttons held after the change; [`new`](Self::new) keeps it consistent with
    /// [`button`](Self::button), and the fields cannot be changed apart.
    #[must_use]
    pub fn buttons(&self) -> PointerButtons {
        self.buttons
    }

    /// `button` changing in direction `D` on top of the platform's `buttons`. The stored set
    /// holds `button` after a press and not after a release.
    #[must_use]
    pub const fn new(
        pointer: PointerInfo,
        button: PointerButton,
        buttons: PointerButtons,
        sample: PointerSample,
    ) -> Self {
        let buttons = if D::PRESSED {
            buttons.with(button)
        } else {
            buttons.without(button)
        };
        Self {
            pointer,
            button,
            buttons,
            modifiers: Modifiers::NONE,
            click_count: None,
            sample,
            direction: PhantomData,
        }
    }

    /// This event with `modifiers` held.
    #[must_use]
    pub const fn with_modifiers(self, modifiers: Modifiers) -> Self {
        Self { modifiers, ..self }
    }

    /// This event with the platform's click count.
    #[must_use]
    pub const fn with_click_count(self, click_count: NonZeroU8) -> Self {
        Self {
            click_count: Some(click_count),
            ..self
        }
    }
}

/// A further button changed while a contact continues: [`PointerEvent::ButtonChange`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ButtonChange {
    /// Another button went down.
    Pressed(PointerPress),
    /// A button came up while another is still held.
    Released(PointerRelease),
}

/// A pointer moved, with the readings the platform coalesced into this event and the ones it
/// predicts.
///
/// `coalesced` holds the readings between the previous move and `current`, oldest first,
/// without `current` itself; `predicted` holds readings expected after `current`, oldest
/// first. A consumer that only needs the latest position reads `current`; velocity tracking
/// and ink read every coalesced reading.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct PointerMove {
    /// Who moved.
    pub pointer: PointerInfo,
    /// The buttons held.
    pub buttons: PointerButtons,
    /// The modifiers held.
    pub modifiers: Modifiers,
    current: PointerSample,
    coalesced: Vec<PointerSample>,
    predicted: Vec<PointerSample>,
}

/// Two movements describe different contact metadata and cannot be coalesced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("pointer metadata differs between movements")]
pub struct MismatchedPointerInfo;

impl PointerMove {
    /// Fold an earlier movement's measured readings into this dispatch.
    ///
    /// The complete [`PointerInfo`] must match; refusal leaves both movements
    /// unchanged. This dispatch keeps its current sample, predicted readings,
    /// buttons and modifiers, while its history includes `older`'s history and
    /// current sample before its own history. As in [`with_coalesced`](Self::with_coalesced),
    /// readings are sorted oldest first, future readings and exact copies of
    /// this dispatch's current sample are excluded. Repeated historical readings
    /// remain distinct: a coarse timestamp does not identify a reading.
    pub fn try_coalesce(&mut self, older: &Self) -> Result<(), MismatchedPointerInfo> {
        if self.pointer != older.pointer {
            return Err(MismatchedPointerInfo);
        }
        let mut samples = older.coalesced.clone();
        samples.push(older.current);
        samples.extend_from_slice(&self.coalesced);
        samples.retain(|sample| sample.time <= self.current.time && *sample != self.current);
        samples.sort_by_key(|sample| sample.time);
        self.coalesced = samples;
        Ok(())
    }

    /// The latest reading.
    #[must_use]
    pub const fn current(&self) -> &PointerSample {
        &self.current
    }

    /// Earlier readings folded into this event, oldest first and not later than
    /// [`current`](Self::current). Read-only, so the ordering cannot be broken after
    /// [`with_coalesced`](Self::with_coalesced) established it.
    #[must_use]
    pub fn coalesced(&self) -> &[PointerSample] {
        &self.coalesced
    }

    /// Readings the platform predicts after [`current`](Self::current), oldest first.
    #[must_use]
    pub fn predicted(&self) -> &[PointerSample] {
        &self.predicted
    }

    /// A move to `current` with no coalesced or predicted readings and no modifiers.
    #[must_use]
    pub const fn new(
        pointer: PointerInfo,
        buttons: PointerButtons,
        current: PointerSample,
    ) -> Self {
        Self {
            pointer,
            buttons,
            modifiers: Modifiers::NONE,
            current,
            coalesced: Vec::new(),
            predicted: Vec::new(),
        }
    }

    /// This move with `modifiers` held.
    #[must_use]
    pub fn with_modifiers(self, modifiers: Modifiers) -> Self {
        Self { modifiers, ..self }
    }

    /// This move with the coalesced readings `samples`: sorted oldest first, keeping only
    /// those not later than `current` and dropping an exact copy of `current` (a reading
    /// that merely shares its time is kept).
    #[must_use]
    pub fn with_coalesced(self, mut samples: Vec<PointerSample>) -> Self {
        let current = self.current;
        samples.retain(|sample| sample.time <= current.time && *sample != current);
        samples.sort_by_key(|sample| sample.time);
        Self {
            coalesced: samples,
            ..self
        }
    }

    /// This move with the predicted readings `samples`: sorted oldest first, keeping only
    /// those not earlier than `current` and dropping an exact copy of `current`.
    #[must_use]
    pub fn with_predicted(self, mut samples: Vec<PointerSample>) -> Self {
        let current = self.current;
        samples.retain(|sample| sample.time >= current.time && *sample != current);
        samples.sort_by_key(|sample| sample.time);
        Self {
            predicted: samples,
            ..self
        }
    }
}

/// An event that only says a pointer did something at a moment: entering or leaving the
/// window, or touching a trackpad to stop the platform's scroll momentum.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct PointerSignal {
    /// Who.
    pub pointer: PointerInfo,
    /// When the platform observed it.
    pub time: EventTime,
    /// Where the pointer was, when the platform says.
    pub position: Option<PointerPosition>,
}

impl PointerSignal {
    /// A signal from `pointer` at `time` with no position.
    #[must_use]
    pub const fn new(pointer: PointerInfo, time: EventTime) -> Self {
        Self {
            pointer,
            time,
            position: None,
        }
    }

    /// This signal at `position`.
    #[must_use]
    pub const fn with_position(self, position: PointerPosition) -> Self {
        Self {
            position: Some(position),
            ..self
        }
    }
}

/// Why a pointer's sequence ended without its `Up`.
///
/// Whatever the reason, the consumer ends the sequence the same way: no tap, no fling, no
/// drop. The reason is for diagnostics and for policy that differs by cause.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CancelReason {
    /// The platform cancelled the contact: a system gesture took it over, palm rejection
    /// discarded it, a touch-cancel arrived.
    Platform,
    /// The window lost the pointer capture it held for the sequence, so the release will not
    /// be delivered to it.
    CaptureLost,
    /// The window lost focus or was deactivated mid-sequence.
    FocusLost,
    /// The device was disconnected.
    DeviceRemoved,
    /// The platform's release could not be used (its position was not finite), so where the
    /// sequence ended is unknown.
    InvalidInput,
}

/// A pointer's sequence ended without its `Up`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct PointerCancel {
    /// Whose sequence ended.
    pub pointer: PointerInfo,
    /// When the platform observed the cancellation.
    pub time: EventTime,
    /// Why it ended.
    pub reason: CancelReason,
}

impl PointerCancel {
    /// `pointer`'s sequence cancelled at `time` for `reason`.
    #[must_use]
    pub const fn new(pointer: PointerInfo, time: EventTime, reason: CancelReason) -> Self {
        Self {
            pointer,
            time,
            reason,
        }
    }
}

/// A pointer device was connected or disconnected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct PointerDeviceChange {
    /// The device.
    pub device: DeviceId,
    /// What kind of device it is.
    pub kind: PointerKind,
    /// When the platform observed the change.
    pub time: EventTime,
}

impl PointerDeviceChange {
    /// `device` of `kind` changing at `time`.
    #[must_use]
    pub const fn new(device: DeviceId, kind: PointerKind, time: EventTime) -> Self {
        Self { device, kind, time }
    }
}

/// One pointer input event, as a backend delivers it to a window.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum PointerEvent {
    /// A contact began: the first button went down, a finger or a pen tip touched.
    Down(PointerPress),
    /// Another button went down or came up while the contact continues (a right-click during
    /// a left drag). The sequence stays the same one.
    ButtonChange(ButtonChange),
    /// The contact ended: the last button came up, the finger or pen lifted.
    Up(PointerRelease),
    /// The pointer moved, in contact or hovering.
    Move(PointerMove),
    /// The pointer's sequence ended without an `Up`.
    Cancel(PointerCancel),
    /// The pointer entered the window, or a pen came into range over it.
    Enter(PointerSignal),
    /// The pointer left the window, or a pen went out of range. A contact still down is
    /// cancelled first, not left behind.
    Leave(PointerSignal),
    /// A wheel or trackpad scroll.
    Scroll(ScrollEvent),
    /// A finger touched the trackpad while the platform was scrolling with momentum: a
    /// framework fling in progress stops too.
    ScrollInertiaCancel(PointerSignal),
    /// A trackpad pan/zoom gesture event.
    PanZoom(PanZoomEvent),
    /// A pointer device was connected.
    DeviceAdded(PointerDeviceChange),
    /// A pointer device was disconnected. A backend cancels each of its active contacts with
    /// [`CancelReason::DeviceRemoved`] first; a consumer that still holds a sequence for the
    /// device ends it here.
    DeviceRemoved(PointerDeviceChange),
}
