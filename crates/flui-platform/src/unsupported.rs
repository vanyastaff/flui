//! One vocabulary for "not here" (ADR-0151 §4).
//!
//! [`Unsupported`] is the one value that says a platform, a device, the
//! application's declarations or the moment does not have what a caller asked
//! for, and [`UnsupportedReason`] says which. Every error and outcome that can
//! mean absence embeds it instead of a unit variant of its own:
//! [`AdmissionError`](crate::AdmissionError),
//! [`SubmitError`](crate::SubmitError),
//! [`CursorError`](crate::CursorError), [`DragError`](crate::drag::DragError),
//! [`HapticOutcome`](crate::haptics::HapticOutcome),
//! [`StorageError`](crate::StorageError),
//! [`PreferenceQueryError`](crate::PreferenceQueryError) and
//! [`TextStoreHostError`](crate::text_store::TextStoreHostError). Each converts
//! from it and answers [`unsupported`](crate::AdmissionError::unsupported)
//! with it, so a caller asks any of them the same question.
//!
//! No reason names an operating-system version: a version gate that refused
//! answers [`UnsupportedReason::NotOnThisDevice`], which says the device lacks
//! the thing and invites no branch on which version has it (ADR-0244). What
//! the operating system has and the host does not implement yet answers
//! [`UnsupportedReason::NotYetOnThisHost`], so "never on this platform" and
//! "not in FLUI yet" stay apart.
//!
//! [`Availability`] says how a capability is delivered where it is: by the
//! operating system, made up by the host from other input, or not at all.

use std::fmt;

/// Why what a caller asked for is not here.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum UnsupportedReason {
    /// This platform has no such thing, on any device.
    NotOnThisPlatform,
    /// This device lacks it: its operating system build or its hardware. The
    /// reason names no version.
    NotOnThisDevice,
    /// The operating system has it, but this FLUI host does not implement it
    /// yet.
    ///
    /// Of the three lasting absences it is the one a later FLUI may remove:
    /// [`NotOnThisPlatform`](Self::NotOnThisPlatform) says no device of this
    /// platform has the thing, [`NotOnThisDevice`](Self::NotOnThisDevice)
    /// that this device or its operating system build lacks it, and this
    /// that the platform has it and the host has no path to it. It is not
    /// [retryable](Unsupported::is_retryable): the same build of the host
    /// answers the same way every time.
    NotYetOnThisHost,
    /// The application did not declare what the platform requires for it.
    NotDeclared {
        /// The declaration that is missing: a manifest permission, a property
        /// list key.
        what: &'static str,
    },
    /// Not now: the thing exists, and trying again later may succeed.
    Unavailable,
    /// Nothing is registered to provide it.
    NotRegistered,
    /// It belongs to a window, and that window is not there: never opened,
    /// or already closed.
    NoWindow,
}

/// Where a capability that is not here is missing: the platform, this device
/// or this host. These are the reasons a capability that lasts can be absent
/// for, which a host records against the capability and every refusal of it
/// answers ([`WindowCapabilities::unavailable`](crate::WindowCapabilities::unavailable)).
///
/// None of them is [retryable](Unsupported::is_retryable): a capability is
/// published as `Unavailable` until the host publishes it again, so a reason
/// that asks to be tried again ([`UnsupportedReason::Unavailable`]) is not one
/// a capability can carry, nor are the reasons about a window or a
/// registration that has gone ([`NoWindow`](UnsupportedReason::NoWindow),
/// [`NotRegistered`](UnsupportedReason::NotRegistered)).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CapabilityAbsence {
    /// No device of this platform has it
    /// ([`UnsupportedReason::NotOnThisPlatform`]).
    #[default]
    NotOnThisPlatform,
    /// The platform has it and this device or its operating system build
    /// lacks it ([`UnsupportedReason::NotOnThisDevice`]).
    NotOnThisDevice,
    /// The operating system has it and this host does not implement it yet
    /// ([`UnsupportedReason::NotYetOnThisHost`]).
    NotYetOnThisHost,
}

impl From<CapabilityAbsence> for UnsupportedReason {
    fn from(absence: CapabilityAbsence) -> Self {
        absence.reason()
    }
}

impl CapabilityAbsence {
    /// The reason a refusal of the capability answers.
    #[must_use]
    pub const fn reason(self) -> UnsupportedReason {
        match self {
            Self::NotOnThisPlatform => UnsupportedReason::NotOnThisPlatform,
            Self::NotOnThisDevice => UnsupportedReason::NotOnThisDevice,
            Self::NotYetOnThisHost => UnsupportedReason::NotYetOnThisHost,
        }
    }
}

impl From<CapabilityAbsence> for Unsupported {
    fn from(absence: CapabilityAbsence) -> Self {
        Self::new(absence.reason())
    }
}

impl fmt::Display for UnsupportedReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotOnThisPlatform => f.write_str("not on this platform"),
            Self::NotOnThisDevice => f.write_str("not on this device"),
            Self::NotYetOnThisHost => f.write_str("not yet implemented by this host"),
            Self::NotDeclared { what } => write!(f, "the application does not declare {what}"),
            Self::Unavailable => f.write_str("unavailable for now"),
            Self::NotRegistered => f.write_str("nothing is registered to provide it"),
            Self::NoWindow => f.write_str("its window is not there"),
        }
    }
}

/// What a caller asked for is not here, and [why](Self::reason).
///
/// Built with [`Unsupported::new`]. It is not written as a literal outside
/// this crate, so it can gain a field (the name of the capability asked
/// for, ADR-0152) without breaking anyone:
///
/// ```compile_fail,E0639
/// use flui_platform::{Unsupported, UnsupportedReason};
///
/// let forged = Unsupported {
///     reason: UnsupportedReason::NotOnThisPlatform,
/// };
/// ```
///
/// ```
/// use flui_platform::{Unsupported, UnsupportedReason};
///
/// let absent = Unsupported::new(UnsupportedReason::NotOnThisDevice);
/// assert_eq!(absent.reason, UnsupportedReason::NotOnThisDevice);
/// assert!(!absent.is_retryable());
/// assert_eq!(absent.to_string(), "unsupported: not on this device");
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, thiserror::Error)]
#[error("unsupported: {reason}")]
#[non_exhaustive]
pub struct Unsupported {
    /// Why it is not here.
    pub reason: UnsupportedReason,
}

impl Unsupported {
    /// Not here, for `reason`.
    #[must_use]
    pub const fn new(reason: UnsupportedReason) -> Self {
        Self { reason }
    }

    /// Whether asking again later may succeed: the reason is
    /// [`UnsupportedReason::Unavailable`]. Every other reason holds until the
    /// device, the application, its windows or the host itself change.
    #[must_use]
    pub const fn is_retryable(self) -> bool {
        match self.reason {
            UnsupportedReason::Unavailable => true,
            UnsupportedReason::NotOnThisPlatform
            | UnsupportedReason::NotOnThisDevice
            | UnsupportedReason::NotYetOnThisHost
            | UnsupportedReason::NotDeclared { .. }
            | UnsupportedReason::NotRegistered
            | UnsupportedReason::NoWindow => false,
        }
    }
}

/// How a backend delivers a capability, where it reports one.
///
/// Delivered is not present: [`Native`](Self::Native) with no matching device
/// attached is a normal state, and
/// [`InputCapabilities::present`](crate::input_capabilities::InputCapabilities::present)
/// says which devices are attached.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[allow(
    clippy::exhaustive_enums,
    reason = "a capability is delivered by the operating system, made up by the host, or not at all; there is no fourth way"
)]
pub enum Availability {
    /// Delivered by the operating system.
    Native,
    /// Built by the backend from what the operating system does provide, and
    /// only as good as that: the web's ctrl+wheel decoded as a pinch (possibly
    /// ambiguous with that input), a Win32 fullscreen made of a borderless
    /// window sized to its monitor.
    Emulated,
    /// This backend in this session has no path for it.
    Unavailable,
}

impl Availability {
    /// Whether the capability is delivered at all, natively or emulated.
    #[must_use]
    pub const fn is_available(self) -> bool {
        !matches!(self, Self::Unavailable)
    }
}
