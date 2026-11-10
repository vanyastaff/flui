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
//! [`LaunchError`](crate::launch::LaunchError),
//! [`StorageError`](crate::StorageError),
//! [`PreferenceQueryError`](crate::PreferenceQueryError),
//! [`TextStoreHostError`](crate::text_store::TextStoreHostError) and
//! `flui_system::dialog::DialogError` (its own test, in that crate). Each converts
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

/// Why a capability that lasts is not here: an [`UnsupportedReason`] that no
/// later attempt changes. A host records it against the capability and every
/// refusal of it answers it
/// ([`WindowCapabilities::unavailable`](crate::WindowCapabilities::unavailable)).
///
/// It holds one of three reasons:
/// [`NotOnThisPlatform`](UnsupportedReason::NotOnThisPlatform),
/// [`NotOnThisDevice`](UnsupportedReason::NotOnThisDevice) or
/// [`NotYetOnThisHost`](UnsupportedReason::NotYetOnThisHost), named by the
/// constants of the same names and read back with [`reason`](Self::reason).
/// The vocabulary is [`UnsupportedReason`]'s; this type only narrows it. A
/// capability is published as `Unavailable` until the host publishes it
/// again, so a reason that asks to be tried again
/// ([`Unavailable`](UnsupportedReason::Unavailable)) is not one it can carry,
/// nor are the reasons about a declaration, a window or a registration
/// ([`NotDeclared`](UnsupportedReason::NotDeclared),
/// [`NoWindow`](UnsupportedReason::NoWindow),
/// [`NotRegistered`](UnsupportedReason::NotRegistered)). Converting from a
/// reason refuses those, so none of them reaches a capability:
///
/// ```
/// use flui_platform::{CapabilityAbsence, Unsupported, UnsupportedReason};
///
/// let device = CapabilityAbsence::try_from(UnsupportedReason::NotOnThisDevice);
/// assert_eq!(device, Ok(CapabilityAbsence::NOT_ON_THIS_DEVICE));
/// assert_eq!(
///     CapabilityAbsence::try_from(UnsupportedReason::Unavailable),
///     Err(UnsupportedReason::Unavailable)
/// );
/// assert!(!Unsupported::from(CapabilityAbsence::NOT_ON_THIS_DEVICE).is_retryable());
/// ```
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct CapabilityAbsence(Lasting);

/// The storage of a [`CapabilityAbsence`]: one byte, so a capability table
/// that records one per capability stays small. Each value maps to one
/// [`UnsupportedReason`] in [`CapabilityAbsence::reason`],
/// and [`CapabilityAbsence::try_from`] matches every reason, so a reason
/// added to [`UnsupportedReason`] does not compile until it is classed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
enum Lasting {
    #[default]
    Platform,
    Device,
    Host,
}

const _: () = assert!(
    size_of::<CapabilityAbsence>() == 1,
    "a capability table records one absence per capability"
);

impl CapabilityAbsence {
    /// No device of this platform has it
    /// ([`UnsupportedReason::NotOnThisPlatform`]). What a capability set to
    /// `Unavailable` by hand answers, and the default.
    pub const NOT_ON_THIS_PLATFORM: Self = Self(Lasting::Platform);
    /// The platform has it and this device or its operating system build
    /// lacks it ([`UnsupportedReason::NotOnThisDevice`]).
    pub const NOT_ON_THIS_DEVICE: Self = Self(Lasting::Device);
    /// The operating system has it and this host does not implement it yet
    /// ([`UnsupportedReason::NotYetOnThisHost`]).
    pub const NOT_YET_ON_THIS_HOST: Self = Self(Lasting::Host);

    /// The reason a refusal of the capability answers.
    #[must_use]
    pub const fn reason(self) -> UnsupportedReason {
        match self.0 {
            Lasting::Platform => UnsupportedReason::NotOnThisPlatform,
            Lasting::Device => UnsupportedReason::NotOnThisDevice,
            Lasting::Host => UnsupportedReason::NotYetOnThisHost,
        }
    }
}

/// The reason, as a capability records it; the reasons a capability cannot
/// carry come back as the error.
impl TryFrom<UnsupportedReason> for CapabilityAbsence {
    type Error = UnsupportedReason;

    fn try_from(reason: UnsupportedReason) -> Result<Self, Self::Error> {
        Ok(Self(match reason {
            UnsupportedReason::NotOnThisPlatform => Lasting::Platform,
            UnsupportedReason::NotOnThisDevice => Lasting::Device,
            UnsupportedReason::NotYetOnThisHost => Lasting::Host,
            UnsupportedReason::NotDeclared { .. }
            | UnsupportedReason::Unavailable
            | UnsupportedReason::NotRegistered
            | UnsupportedReason::NoWindow => return Err(reason),
        }))
    }
}

/// Prints the reason it holds: `CapabilityAbsence(NotOnThisDevice)`.
impl fmt::Debug for CapabilityAbsence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("CapabilityAbsence")
            .field(&self.reason())
            .finish()
    }
}

impl From<CapabilityAbsence> for UnsupportedReason {
    fn from(absence: CapabilityAbsence) -> Self {
        absence.reason()
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
#[non_exhaustive]
pub struct Unsupported {
    /// Why it is not here.
    pub reason: UnsupportedReason,
    /// The [`NAME`](crate::capability::Capability::NAME) of the capability
    /// asked for, when the refusal names one.
    capability: Option<&'static str>,
}

impl fmt::Display for Unsupported {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.capability {
            Some(name) => write!(f, "unsupported: {name}: {}", self.reason),
            None => write!(f, "unsupported: {}", self.reason),
        }
    }
}

impl Unsupported {
    /// Not here, for `reason`.
    #[must_use]
    pub const fn new(reason: UnsupportedReason) -> Self {
        Self {
            reason,
            capability: None,
        }
    }

    /// The capability `C` is not here, for `reason`; the refusal names it.
    #[must_use]
    pub const fn of<C: crate::capability::Capability>(reason: UnsupportedReason) -> Self {
        Self {
            reason,
            capability: Some(C::NAME),
        }
    }

    /// The capability named `name` is not here, for `reason`: the erased form
    /// of [`Self::of`], for a registry that holds the name and not the type.
    #[must_use]
    pub const fn named(name: &'static str, reason: UnsupportedReason) -> Self {
        Self {
            reason,
            capability: Some(name),
        }
    }

    /// The [`NAME`](crate::capability::Capability::NAME) of the capability
    /// this refusal is about, when it names one.
    #[must_use]
    pub const fn capability(self) -> Option<&'static str> {
        self.capability
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
