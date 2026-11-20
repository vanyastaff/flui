//! What a driver call fails with: one variant per code of ADR-0080 and ADR-0238 §3 with the fields
//! that code carries, and the failures only the driver sees (its transport, a lane, the user
//! taking the desktop back, an expectation that never held).

use std::io;

use flui_protocol::act::{ActionabilityCheck, Effect, EffectKind};
use flui_protocol::error::{ErrorCode, ErrorObject, HandleKind, Retry};
use flui_protocol::version::ProtocolVersion;

/// A driver call's result.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Why a driver call failed.
///
/// The variants up to [`Error::Platform`] are the app's answers, one per wire code, each with the
/// fields its code carries and the `retry` the app said; [`Error::from_wire`] reads them. The driver
/// raises the same codes for what it refuses itself, with the code's own retry. The rest are the
/// driver's own failures.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// A parameter is malformed or contradicts another, or the target refused a rule the call
    /// broke (a scenario's window size, among others).
    #[error("invalid argument: {message}")]
    InvalidArgument {
        /// What was wrong, for people.
        message: String,
        /// Whether the same call can succeed later.
        retry: Retry,
    },
    /// The target cannot do this at all; `capability` names what it lacks.
    #[error("not supported{}: {message}", capability.as_deref().map_or_default(|c| format!(" ({c})")))]
    NotSupported {
        /// The capability the target lacks.
        capability: Option<String>,
        /// What was asked, for people.
        message: String,
        /// Whether the same call can succeed later.
        retry: Retry,
    },
    /// The target is busy; the same call may succeed soon.
    #[error("busy: {message}")]
    Busy {
        /// Why, for people.
        message: String,
        /// Whether the same call can succeed later.
        retry: Retry,
    },
    /// Nothing matched; `candidates` lists the nearest elements.
    #[error("not found: {message}")]
    NotFound {
        /// What was looked for, for people.
        message: String,
        /// The nearest elements by role, name and identifier, as outline lines.
        candidates: Vec<String>,
        /// Whether the same call can succeed later.
        retry: Retry,
    },
    /// The handle was never issued to this session.
    #[error("unknown handle {}: {message}", handle.as_deref().unwrap_or("?"))]
    UnknownHandle {
        /// What kind of handle.
        kind: Option<HandleKind>,
        /// The handle as it was sent.
        handle: Option<String>,
        /// For people.
        message: String,
        /// Whether the same call can succeed later.
        retry: Retry,
    },
    /// What the handle named is gone: an element, a window, or the app's process.
    #[error("gone{}: {message}", exit.map_or_default(|code| format!(" (exit code {code})")))]
    Gone {
        /// What kind of handle; [`HandleKind::Process`] when the app itself went.
        kind: Option<HandleKind>,
        /// The handle as it was sent.
        handle: Option<String>,
        /// The process's exit code, when the app exited and the OS reported one.
        exit: Option<i32>,
        /// How much of an acting call reached what went, when that is known: an element that went
        /// away while it was acted on may have run the act first.
        effect: Option<Effect>,
        /// For people.
        message: String,
        /// Whether the same call can succeed later.
        retry: Retry,
    },
    /// The element refuses interaction.
    #[error("disabled: {message}")]
    Disabled {
        /// For people.
        message: String,
        /// Whether the same call can succeed later.
        retry: Retry,
    },
    /// The element does not offer this action.
    #[error("action unsupported: {message}")]
    ActionUnsupported {
        /// For people.
        message: String,
        /// Whether the same call can succeed later.
        retry: Retry,
    },
    /// The target window is not in the foreground.
    #[error("not in the foreground: {message}")]
    NotForeground {
        /// For people.
        message: String,
        /// Whether the same call can succeed later.
        retry: Retry,
    },
    /// Keyboard focus is outside the target.
    #[error("focus is elsewhere: {message}")]
    FocusElsewhere {
        /// For people.
        message: String,
        /// Whether the same call can succeed later.
        retry: Retry,
    },
    /// The point is outside the target: never clamped into it.
    #[error("outside the target: {message}")]
    OutsideTarget {
        /// For people.
        message: String,
        /// Whether the same call can succeed later.
        retry: Retry,
    },
    /// No answer came in time. An acting call carries `may_have_run` and is never resent: read
    /// the tree for its effect.
    #[error("timeout: {message}")]
    Timeout {
        /// How much of an acting call may have reached the app.
        effect: Option<Effect>,
        /// For people.
        message: String,
        /// Whether the same call can succeed later.
        retry: Retry,
    },
    /// Input is held by a previous call.
    #[error("input held: {message}")]
    InputHeld {
        /// For people.
        message: String,
        /// Whether the same call can succeed later.
        retry: Retry,
    },
    /// The app or the agent is shutting down.
    #[error("shutting down: {message}")]
    ShuttingDown {
        /// For people.
        message: String,
        /// Whether the same call can succeed later.
        retry: Retry,
    },
    /// A strict locator matched more than one element.
    #[error("ambiguous: {message}")]
    Ambiguous {
        /// For people.
        message: String,
        /// The first ten matches, as outline lines.
        candidates: Vec<String>,
        /// Whether the same call can succeed later.
        retry: Retry,
    },
    /// An actionability check never held within the timeout.
    #[error("not actionable{}: {message}", failed.map_or_default(|check| format!(" ({})", check.name())))]
    NotActionable {
        /// The check that never held.
        failed: Option<ActionabilityCheck>,
        /// What the hit-test reached instead, as its outline line.
        covered_by: Option<String>,
        /// For people.
        message: String,
        /// Whether the same call can succeed later.
        retry: Retry,
    },
    /// A handle from an earlier generation that is still unknown after re-reading.
    #[error("stale handle {}: {message}", handle.as_deref().unwrap_or("?"))]
    Stale {
        /// The handle as it was sent.
        handle: Option<String>,
        /// For people.
        message: String,
        /// Whether the same call can succeed later.
        retry: Retry,
    },
    /// The driver and the app share no protocol version.
    #[error("no shared protocol: {message}")]
    Version {
        /// The schema the app speaks.
        app: Option<ProtocolVersion>,
        /// The schema the driver asked for.
        client: Option<ProtocolVersion>,
        /// For people.
        message: String,
        /// Whether the same call can succeed later.
        retry: Retry,
    },
    /// The call needs the app's driver: another client drives it, or this connection observes.
    #[error("not the app's driver: {message}")]
    NotDriver {
        /// For people.
        message: String,
        /// Whether the same call can succeed later.
        retry: Retry,
    },
    /// The app failed in a way no other code names: a panic in its code during the call among
    /// others, contained by the app, which keeps serving.
    #[error("platform: {message}")]
    Platform {
        /// How much of an acting call reached the app, when the app said.
        effect: Option<Effect>,
        /// For people.
        message: String,
        /// Whether the same call can succeed later.
        retry: Retry,
    },
    /// The person at the machine used it during an OS-input run: the lane stopped before its
    /// next event and released everything it held. The run cannot be verified.
    #[error("interrupted by user input on the {lane} lane: {event}")]
    InterruptedByUser {
        /// The lane that saw it.
        lane: String,
        /// What it saw.
        event: String,
    },
    /// A lane failed, or cannot run on this host (no device, a locked desktop, a refused portal).
    #[error("the {lane} lane failed: {message}")]
    Lane {
        /// The lane.
        lane: String,
        /// Whether the host cannot run the lane at all, which makes a check cannot-verify.
        unavailable: bool,
        /// For people.
        message: String,
    },
    /// The lane could not release input it pressed itself: the keys, mouse buttons and touch
    /// contacts in `held` are still down, and a pen named there is still down or in range. Not
    /// [`Error::InputHeld`], which is someone else's input in the way and worth waiting out; this
    /// is the lane's own failure, which a retry of the call does not cure. A lane gathers it from
    /// its parts with [`lane::release_outcome`](crate::lane::release_outcome).
    #[error("the {lane} lane left {} held: {message}", held.join(", "))]
    ReleaseFailed {
        /// The lane.
        lane: String,
        /// What it still holds, by name.
        held: Vec<String>,
        /// The first release each part could not make, in the order the parts ran, for people.
        message: String,
        /// The failure that stopped the input before the release ran, when one did
        /// (`lane::send_guarded`): what it says of the call's effect and of the check's verdict
        /// stays true of this error ([`Error::effect`], [`Error::is_cannot_verify`]).
        first: Option<Box<Error>>,
    },
    /// The conversation with the app broke its protocol: a line that is not one, an answer to
    /// nothing asked.
    #[error("protocol: {message}")]
    Protocol {
        /// For people.
        message: String,
    },
    /// An expectation did not hold within its timeout: expected against what was last seen.
    #[error("expected {expected}, last saw {actual}")]
    Expectation {
        /// What was expected.
        expected: String,
        /// What was last seen.
        actual: String,
    },
    /// The driver's own input or output failed: starting the app, its endpoint, a file.
    #[error(transparent)]
    Io(#[from] io::Error),
}

impl Error {
    /// The error the app answered with, read into its variant, its `retry` kept.
    #[must_use]
    pub fn from_wire(error: ErrorObject) -> Self {
        let ErrorObject {
            code,
            message,
            retry,
            effect,
            kind,
            handle,
            exit,
            candidates,
            failed,
            covered_by,
            capability,
            app,
            client,
            ..
        } = error;
        match code {
            ErrorCode::InvalidArgument => Self::InvalidArgument { message, retry },
            ErrorCode::NotSupported => Self::NotSupported {
                capability,
                message,
                retry,
            },
            ErrorCode::Busy => Self::Busy { message, retry },
            ErrorCode::NotFound => Self::NotFound {
                message,
                candidates,
                retry,
            },
            ErrorCode::UnknownHandle => Self::UnknownHandle {
                kind,
                handle,
                message,
                retry,
            },
            ErrorCode::Gone => Self::Gone {
                kind,
                handle,
                exit,
                effect,
                message,
                retry,
            },
            ErrorCode::Disabled => Self::Disabled { message, retry },
            ErrorCode::ActionUnsupported => Self::ActionUnsupported { message, retry },
            ErrorCode::NotForeground => Self::NotForeground { message, retry },
            ErrorCode::FocusElsewhere => Self::FocusElsewhere { message, retry },
            ErrorCode::OutsideTarget => Self::OutsideTarget { message, retry },
            ErrorCode::Timeout => Self::Timeout {
                effect,
                message,
                retry,
            },
            ErrorCode::InputHeld => Self::InputHeld { message, retry },
            ErrorCode::ShuttingDown => Self::ShuttingDown { message, retry },
            ErrorCode::Ambiguous => Self::Ambiguous {
                message,
                candidates,
                retry,
            },
            ErrorCode::NotActionable => Self::NotActionable {
                failed,
                covered_by,
                message,
                retry,
            },
            ErrorCode::Stale => Self::Stale {
                handle,
                message,
                retry,
            },
            ErrorCode::Version => Self::Version {
                app,
                client,
                message,
                retry,
            },
            ErrorCode::NotDriver => Self::NotDriver { message, retry },
            // `Platform`, and every code a newer schema adds, which an older reader reads as it.
            _ => Self::Platform {
                effect,
                message,
                retry,
            },
        }
    }

    /// The driver refuses a parameter or a rule the call broke.
    pub(crate) fn invalid_argument(message: impl Into<String>) -> Self {
        Self::InvalidArgument {
            message: message.into(),
            retry: ErrorCode::InvalidArgument.default_retry(),
        }
    }

    /// The target lacks `capability`.
    pub(crate) fn not_supported(capability: impl Into<String>, message: impl Into<String>) -> Self {
        Self::NotSupported {
            capability: Some(capability.into()),
            message: message.into(),
            retry: ErrorCode::NotSupported.default_retry(),
        }
    }

    /// Nothing matched.
    pub(crate) fn not_found(message: impl Into<String>) -> Self {
        Self::NotFound {
            message: message.into(),
            candidates: Vec::new(),
            retry: ErrorCode::NotFound.default_retry(),
        }
    }

    /// The target is busy.
    pub(crate) fn busy(message: impl Into<String>) -> Self {
        Self::Busy {
            message: message.into(),
            retry: ErrorCode::Busy.default_retry(),
        }
    }

    /// A point outside the target.
    pub(crate) fn outside_target(message: impl Into<String>) -> Self {
        Self::OutsideTarget {
            message: message.into(),
            retry: ErrorCode::OutsideTarget.default_retry(),
        }
    }

    /// The handle `handle` of `kind` is gone; `exit` is a process's exit code.
    pub(crate) fn gone(
        kind: HandleKind,
        handle: Option<String>,
        exit: Option<i32>,
        message: impl Into<String>,
    ) -> Self {
        Self::Gone {
            kind: Some(kind),
            handle,
            exit,
            effect: None,
            message: message.into(),
            retry: ErrorCode::Gone.default_retry(),
        }
    }

    /// The app's process went: `exit` is its exit code when the OS reported one.
    pub(crate) fn process_gone(message: impl Into<String>, exit: Option<i32>) -> Self {
        Self::gone(HandleKind::Process, None, exit, message)
    }

    /// The wire code this error answers to; `None` for the driver's own failures.
    #[must_use]
    pub fn code(&self) -> Option<ErrorCode> {
        Some(match self {
            Self::InvalidArgument { .. } => ErrorCode::InvalidArgument,
            Self::NotSupported { .. } => ErrorCode::NotSupported,
            Self::Busy { .. } => ErrorCode::Busy,
            Self::NotFound { .. } => ErrorCode::NotFound,
            Self::UnknownHandle { .. } => ErrorCode::UnknownHandle,
            Self::Gone { .. } => ErrorCode::Gone,
            Self::Disabled { .. } => ErrorCode::Disabled,
            Self::ActionUnsupported { .. } => ErrorCode::ActionUnsupported,
            Self::NotForeground { .. } => ErrorCode::NotForeground,
            Self::FocusElsewhere { .. } => ErrorCode::FocusElsewhere,
            Self::OutsideTarget { .. } => ErrorCode::OutsideTarget,
            Self::Timeout { .. } => ErrorCode::Timeout,
            Self::InputHeld { .. } => ErrorCode::InputHeld,
            Self::ShuttingDown { .. } => ErrorCode::ShuttingDown,
            Self::Ambiguous { .. } => ErrorCode::Ambiguous,
            Self::NotActionable { .. } => ErrorCode::NotActionable,
            Self::Stale { .. } => ErrorCode::Stale,
            Self::Version { .. } => ErrorCode::Version,
            Self::NotDriver { .. } => ErrorCode::NotDriver,
            Self::Platform { .. } => ErrorCode::Platform,
            Self::InterruptedByUser { .. }
            | Self::Lane { .. }
            | Self::ReleaseFailed { .. }
            | Self::Protocol { .. }
            | Self::Expectation { .. }
            | Self::Io(_) => return None,
        })
    }

    /// Whether the same call can succeed later: what the app said for its answers, and never for
    /// the driver's own failures.
    #[must_use]
    pub fn retry(&self) -> Retry {
        match self {
            Self::InvalidArgument { retry, .. }
            | Self::NotSupported { retry, .. }
            | Self::Busy { retry, .. }
            | Self::NotFound { retry, .. }
            | Self::UnknownHandle { retry, .. }
            | Self::Gone { retry, .. }
            | Self::Disabled { retry, .. }
            | Self::ActionUnsupported { retry, .. }
            | Self::NotForeground { retry, .. }
            | Self::FocusElsewhere { retry, .. }
            | Self::OutsideTarget { retry, .. }
            | Self::Timeout { retry, .. }
            | Self::InputHeld { retry, .. }
            | Self::ShuttingDown { retry, .. }
            | Self::Ambiguous { retry, .. }
            | Self::NotActionable { retry, .. }
            | Self::Stale { retry, .. }
            | Self::Version { retry, .. }
            | Self::NotDriver { retry, .. }
            | Self::Platform { retry, .. } => *retry,
            Self::InterruptedByUser { .. }
            | Self::Lane { .. }
            | Self::ReleaseFailed { .. }
            | Self::Protocol { .. }
            | Self::Expectation { .. }
            | Self::Io(_) => Retry::Never,
        }
    }

    /// How much of an acting call reached the app, when the failure says.
    #[must_use]
    pub fn effect(&self) -> Option<&Effect> {
        match self {
            Self::Timeout { effect, .. }
            | Self::Platform { effect, .. }
            | Self::Gone { effect, .. } => effect.as_ref(),
            Self::ReleaseFailed {
                first: Some(first), ..
            } => first.effect(),
            _ => None,
        }
    }

    /// Whether the call may have run: its effect is anything but nothing.
    #[must_use]
    pub fn may_have_run(&self) -> bool {
        self.effect()
            .is_some_and(|effect| effect.kind != EffectKind::Partial || effect.sent != Some(0))
    }

    /// Whether this failure means the check could not be made rather than that it failed: a
    /// capability the target lacks, a lane the host cannot run, or the user taking the desktop
    /// back during an OS-input run. A report turns such a check into `cannot_verify`, never a
    /// failure and never a pass.
    #[must_use]
    pub fn is_cannot_verify(&self) -> bool {
        match self {
            Self::NotSupported { .. }
            | Self::InterruptedByUser { .. }
            | Self::Lane {
                unavailable: true, ..
            } => true,
            Self::ReleaseFailed {
                first: Some(first), ..
            } => first.is_cannot_verify(),
            _ => false,
        }
    }

    /// The capability a cannot-verify failure names, for a report's list.
    #[must_use]
    pub fn missing_capability(&self) -> Option<String> {
        match self {
            Self::NotSupported { capability, .. } => {
                Some(capability.clone().unwrap_or_else(|| "unnamed".to_owned()))
            }
            Self::InterruptedByUser { .. } => Some("uninterrupted_os_input".to_owned()),
            Self::Lane {
                lane,
                unavailable: true,
                ..
            } => Some(format!("{lane}_lane")),
            Self::ReleaseFailed {
                first: Some(first), ..
            } => first.missing_capability(),
            _ => None,
        }
    }
}
