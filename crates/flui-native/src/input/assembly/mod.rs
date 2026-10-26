//! Pointer assembly shared by every native backend (ADR-0217 §3).
//!
//! A backend copies each native event into a plain [`NativeReading`] — no
//! `windows::*`, `objc2`, JNI or DOM type — and [`decode`] turns it into owned
//! [`PlatformInput`] against a [`ContactRegistry`]. The registry owns contact
//! identity, device presence ([`DeviceRegistry`]), held buttons
//! ([`HeldButtons`]), click counting ([`ClickCounter`]), trackpad pinch and
//! rotate accumulation ([`PanZoomAccumulator`]) and per-sequence clock choice;
//! [`ScrollPhases`] gives every backend's scrolling one phase grammar, a
//! two-finger pan included;
//! [`MoveBatch`] attaches a native batch's coalesced and predicted readings to
//! the move it ends. The rules are written once, and Linux CI runs them over
//! recorded readings of every backend.
//!
//! A reading is decoded completely before anything is committed. One that
//! fails ([`DecodeError`]) cancels its open contact with
//! [`CancelReason::InvalidInput`]; one that belongs to no open contact is an
//! orphan and emits nothing. Both log a diagnostic. Decoding a reading
//! allocates nothing once the registry holds its contacts.
//!
//! In debug builds with the `native-capture` feature, `capture` journals
//! every registry call on a thread as a native recording (ADR-0215 §4).

#![forbid(unsafe_code)]

/// Report a registry call to the native capture, in builds that have one.
macro_rules! journal {
    ($journal:expr, $call:expr) => {
        #[cfg(all(debug_assertions, feature = "native-capture"))]
        $journal.record($call);
    };
}

mod batch;
mod buttons;
#[cfg(all(debug_assertions, feature = "native-capture"))]
pub mod capture;
mod clock;
mod pan_zoom;
mod registry;
mod scroll;
mod touchpad;
mod wheel;

use std::num::NonZeroU8;

use flui_platform::keyboard::Modifiers;
use flui_platform::pointer::{
    CancelReason, InputValueError, PanZoomEvent, PointerButton, PointerButtons, PointerCancel,
    PointerEvent, PointerKind,
};
use smallvec::SmallVec;

pub use batch::MoveBatch;
pub use buttons::{ButtonTransition, ClickCounter, ClickSettings, HeldButtons};
pub use clock::{ClockChoice, CounterAnchor, NativeClock, NativeTime, QpcTicks, ReadingTime};
pub use pan_zoom::{GestureComponent, GestureDelta, GesturePhase, PanZoomAccumulator};
pub(crate) use registry::decode_derived;
pub use registry::{ContactRegistry, DeviceRegistry, decode};
pub use scroll::{ContactScroll, ScrollPhases, ScrollReading};
pub use touchpad::{ContentTransform, TouchpadFrame, ViewportReading, ViewportStatus};
pub use wheel::{WHEEL_DETENT, WheelAxis, WheelReading};

use flui_platform::{PlatformInput, PointerFrame};

/// The events one reading assembles into; two fit without allocating.
pub type Assembled = SmallVec<[PlatformInput; 2]>;

/// The inputs of the contacts a platform reported as one frame, as the frame
/// they are: when every input is a pointer event and there are at least two,
/// one [`PlatformInput::PointerFrame`]; otherwise the inputs in order.
#[must_use]
pub fn framed(contacts: impl IntoIterator<Item = Assembled>) -> Assembled {
    let inputs: Assembled = contacts.into_iter().flatten().collect();
    if inputs.len() < 2
        || !inputs
            .iter()
            .all(|input| matches!(input, PlatformInput::Pointer(_)))
    {
        return inputs;
    }
    let events = inputs
        .into_iter()
        .filter_map(|input| match input {
            PlatformInput::Pointer(event) => Some(event),
            _ => None,
        })
        .collect();
    PointerFrame::new(events)
        .map(|frame| smallvec::smallvec![PlatformInput::PointerFrame(frame)])
        .unwrap_or_default()
}

/// The input surface a contact belongs to: a backend's window, or `0` for a
/// backend whose registry serves one window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct NativeSurface(pub u64);

/// The backend's label for an input device. Labels are minted into
/// [`DeviceId`](flui_platform::pointer::DeviceId)s by the registry, so a
/// label the OS reuses after removal never revives a retired identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NativeDevice(pub u64);

/// The backend's label for one contact of a device: a Win32 pointer id, an
/// Android pointer id, a `UITouch` address, a DOM `pointerId`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NativeContact(pub u64);

/// A position as the OS reported it, in device pixels of the surface.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RawPoint {
    /// Horizontal device pixels.
    pub x: f64,
    /// Vertical device pixels.
    pub y: f64,
}

/// The angles of a pen, in the form the OS reports them.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum PenAngles {
    /// Tilt from vertical along x and y, in degrees within `[-90, 90]` (Win32).
    TiltDegrees {
        /// Tilt toward positive x.
        x: f64,
        /// Tilt toward positive y.
        y: f64,
    },
    /// Altitude above the surface and azimuth, in radians; either may be absent.
    Spherical {
        /// Altitude in `[0, π/2]`.
        altitude: Option<f64>,
        /// Azimuth, any finite angle.
        azimuth: Option<f64>,
    },
}

/// Sensor readings as plain numbers. Absent means the device does not report
/// that sensor; a present value must be valid, or the reading fails.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct RawSensors {
    /// Normalized pressure in `[0, 1]`.
    pub pressure: Option<f64>,
    /// Normalized barrel pressure in `[-1, 1]`.
    pub tangential_pressure: Option<f64>,
    /// Pen angles.
    pub angles: Option<PenAngles>,
    /// Clockwise twist in degrees within `[0, 360)`.
    pub twist_degrees: Option<f64>,
    /// Contact width and height in device pixels.
    pub contact_size: Option<(f64, f64)>,
}

/// One reading of a trackpad gesture component.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GestureReading {
    /// Which component.
    pub component: GestureComponent,
    /// Its phase.
    pub phase: GesturePhase,
    /// The change it makes.
    pub delta: GestureDelta,
}

/// What a reading says the contact did.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum ReadingAction {
    /// Came into range or entered the surface.
    Enter,
    /// Left range or the surface.
    Leave,
    /// Moved, in contact or hovering.
    Move,
    /// A button went down; a touch or pen tip touching is the primary button.
    Press(PointerButton),
    /// A button came up; a touch or pen tip lifting is the primary button.
    Release(PointerButton),
    /// The platform ended the sequence without a release.
    Cancel(NativeCancel),
    /// A trackpad gesture component.
    Gesture(GestureReading),
}

/// Why the platform ended a contact's sequence: every [`CancelReason`] but
/// [`CancelReason::Claimed`], which only the gesture arena emits (ADR-0208), so
/// a backend cannot report it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum NativeCancel {
    /// A system gesture took the contact, palm rejection discarded it, a
    /// touch-cancel arrived.
    Platform,
    /// The window lost the pointer capture it held for the sequence.
    CaptureLost,
    /// The window lost focus or was deactivated mid-sequence.
    FocusLost,
    /// The device was disconnected.
    DeviceRemoved,
    /// The platform's release could not be used.
    InvalidInput,
}

impl From<NativeCancel> for CancelReason {
    fn from(reason: NativeCancel) -> Self {
        match reason {
            NativeCancel::Platform => Self::Platform,
            NativeCancel::CaptureLost => Self::CaptureLost,
            NativeCancel::FocusLost => Self::FocusLost,
            NativeCancel::DeviceRemoved => Self::DeviceRemoved,
            NativeCancel::InvalidInput => Self::InvalidInput,
        }
    }
}

/// One native pointer reading as plain data: what a backend produces and a
/// native recording holds (ADR-0215 §4).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NativeReading {
    /// The surface it was delivered to.
    pub surface: NativeSurface,
    /// The device, when the platform names one.
    pub device: Option<NativeDevice>,
    /// The contact.
    pub contact: NativeContact,
    /// The kind of device.
    pub kind: PointerKind,
    /// The platform's own primary flag; `None` lets the registry decide (the
    /// first open contact of its kind on the surface is primary).
    pub primary: Option<bool>,
    /// What happened.
    pub action: ReadingAction,
    /// The buttons held after the reading, when the platform reports the set.
    pub buttons: Option<PointerButtons>,
    /// The modifiers held.
    pub modifiers: Modifiers,
    /// The platform's own click count for a press.
    pub click_count: Option<NonZeroU8>,
    /// When it was measured.
    pub time: ReadingTime,
    /// Where, in device pixels; `None` when the event carries no position and
    /// the contact's last one applies.
    pub position: Option<RawPoint>,
    /// Device pixels per logical pixel.
    pub scale: f64,
    /// Sensor values.
    pub sensors: RawSensors,
}

impl NativeReading {
    /// A reading of `contact`, a `kind` device, doing `action` at `time`, on
    /// surface `0` with no device, position, buttons, modifiers or sensors and
    /// a scale of one.
    #[must_use]
    pub const fn new(
        contact: NativeContact,
        kind: PointerKind,
        action: ReadingAction,
        time: ReadingTime,
    ) -> Self {
        Self {
            surface: NativeSurface(0),
            device: None,
            contact,
            kind,
            primary: None,
            action,
            buttons: None,
            modifiers: Modifiers::NONE,
            click_count: None,
            time,
            position: None,
            scale: 1.0,
            sensors: RawSensors {
                pressure: None,
                tangential_pressure: None,
                angles: None,
                twist_degrees: None,
                contact_size: None,
            },
        }
    }

    /// This reading at device-pixel `(x, y)` with `scale` device pixels per
    /// logical pixel.
    #[must_use]
    pub const fn at(self, x: f64, y: f64, scale: f64) -> Self {
        Self {
            position: Some(RawPoint { x, y }),
            scale,
            ..self
        }
    }

    /// This reading on `surface`.
    #[must_use]
    pub const fn on(self, surface: NativeSurface) -> Self {
        Self { surface, ..self }
    }

    /// This reading from `device`.
    #[must_use]
    pub const fn from_device(self, device: NativeDevice) -> Self {
        Self {
            device: Some(device),
            ..self
        }
    }

    /// This reading with the platform's held set.
    #[must_use]
    pub const fn with_buttons(self, buttons: PointerButtons) -> Self {
        Self {
            buttons: Some(buttons),
            ..self
        }
    }

    /// This reading with the platform's primary flag.
    #[must_use]
    pub const fn with_primary(self, primary: bool) -> Self {
        Self {
            primary: Some(primary),
            ..self
        }
    }

    /// This reading with `modifiers` held.
    #[must_use]
    pub const fn with_modifiers(self, modifiers: Modifiers) -> Self {
        Self { modifiers, ..self }
    }

    /// This reading with the platform's click count.
    #[must_use]
    pub const fn with_click_count(self, count: NonZeroU8) -> Self {
        Self {
            click_count: Some(count),
            ..self
        }
    }

    /// This reading with `sensors`.
    #[must_use]
    pub const fn with_sensors(self, sensors: RawSensors) -> Self {
        Self { sensors, ..self }
    }
}

/// Why a reading could not be assembled.
#[derive(Debug, Clone, Copy, PartialEq, thiserror::Error)]
#[non_exhaustive]
pub enum DecodeFault {
    /// The scale is not a positive finite number.
    #[error("device scale {0} is not positive and finite")]
    Scale(f64),
    /// A position or sensor value is outside its range.
    #[error("reading value refused: {0}")]
    Value(#[from] InputValueError),
    /// The pen angles are outside their range.
    #[error("pen angles are outside their range")]
    Angles,
    /// The time stamp is not a finite time.
    #[error("reading time is not finite")]
    Time,
    /// A coalesced reading has no position.
    #[error("a coalesced reading has no position")]
    MissingPosition,
    /// The contact's label came back with a different kind.
    #[error("the native contact changed kind mid-sequence")]
    IdentityChanged,
    /// The terminal pointer or device identity was already issued.
    #[error("pointer or device identities are exhausted")]
    Exhausted,
    /// No open contact matches the reading.
    #[error("the reading belongs to no open contact")]
    Orphan,
}

/// A reading that could not be assembled, with the cancel that ends its open
/// contact, if it had one.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[error("{fault}")]
pub struct DecodeError {
    fault: DecodeFault,
    cancel: Option<Ending>,
}

/// The event that ends a contact's sequence early. The rare pan-zoom ending
/// is boxed so a decode result stays small.
#[derive(Debug, Clone, PartialEq)]
enum Ending {
    Pointer(PointerCancel),
    PanZoom(Box<PanZoomEvent>),
}

impl Ending {
    fn into_input(self) -> PlatformInput {
        PlatformInput::Pointer(match self {
            Self::Pointer(cancel) => PointerEvent::Cancel(cancel),
            Self::PanZoom(event) => PointerEvent::PanZoom(*event),
        })
    }
}

impl DecodeError {
    /// Why it failed.
    #[must_use]
    pub const fn fault(&self) -> DecodeFault {
        self.fault
    }

    /// The event that ended the reading's open contact (a pointer cancel, or a
    /// cancelled pan-zoom for a trackpad gesture), if it had one.
    #[must_use]
    pub fn cancel(&self) -> Option<PlatformInput> {
        self.cancel.clone().map(Ending::into_input)
    }

    /// The events a backend delivers for this failure: the cancel, if any.
    #[must_use]
    pub fn into_inputs(self) -> Assembled {
        self.cancel.map(Ending::into_input).into_iter().collect()
    }
}

#[cfg(test)]
mod tests;
