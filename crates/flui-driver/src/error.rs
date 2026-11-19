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
/// fields its code carries; [`Error::from_wire`] reads them. The rest are the driver's own.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// A parameter is malformed or contradicts another, or the target refused a rule the call
    /// broke (a scenario's window size, among others).
    #[error("invalid argument: {message}")]
    InvalidArgument {
        /// What was wrong, for people.
        message: String,
    },
    /// The target cannot do this at all; `capability` names what it lacks.
    #[error("not supported{}: {message}", capability.as_deref().map_or_default(|c| format!(" ({c})")))]
    NotSupported {
        /// The capability the target lacks.
        capability: Option<String>,
        /// What was asked, for people.
        message: String,
    },
    /// The target is busy; the same call may succeed soon.
    #[error("busy: {message}")]
    Busy {
        /// Why, for people.
        message: String,
    },
    /// Nothing matched; `candidates` lists the nearest elements.
    #[error("not found: {message}")]
    NotFound {
        /// What was looked for, for people.
        message: String,
        /// The nearest elements by role, name and identifier, as outline lines.
        candidates: Vec<String>,
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
        /// For people.
        message: String,
    },
    /// The element refuses interaction.
    #[error("disabled: {message}")]
    Disabled {
        /// For people.
        message: String,
    },
    /// The element does not offer this action.
    #[error("action unsupported: {message}")]
    ActionUnsupported {
        /// For people.
        message: String,
    },
    /// The target window is not in the foreground.
    #[error("not in the foreground: {message}")]
    NotForeground {
        /// For people.
        message: String,
    },
    /// Keyboard focus is outside the target.
    #[error("focus is elsewhere: {message}")]
    FocusElsewhere {
        /// For people.
        message: String,
    },
    /// The point is outside the target: never clamped into it.
    #[error("outside the target: {message}")]
    OutsideTarget {
        /// For people.
        message: String,
    },
    /// No answer came in time. An acting call carries `may_have_run` and is never resent: read
    /// the tree for its effect.
    #[error("timeout: {message}")]
    Timeout {
        /// How much of an acting call may have reached the app.
        effect: Option<Effect>,
        /// For people.
        message: String,
    },
    /// Input is held by a previous call.
    #[error("input held: {message}")]
    InputHeld {
        /// For people.
        message: String,
    },
    /// The app or the agent is shutting down.
    #[error("shutting down: {message}")]
    ShuttingDown {
        /// For people.
        message: String,
    },
    /// A strict locator matched more than one element.
    #[error("ambiguous: {message}")]
    Ambiguous {
        /// For people.
        message: String,
        /// The first ten matches, as outline lines.
        candidates: Vec<String>,
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
    },
    /// A handle from an earlier generation that is still unknown after re-reading.
    #[error("stale handle {}: {message}", handle.as_deref().unwrap_or("?"))]
    Stale {
        /// The handle as it was sent.
        handle: Option<String>,
        /// For people.
        message: String,
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
    },
    /// The call needs the app's driver: another client drives it, or this connection observes.
    #[error("not the app's driver: {message}")]
    NotDriver {
        /// For people.
        message: String,
    },
    /// The app failed in a way no other code names: a panic in its code during the call among
    /// others, contained by the app, which keeps serving.
    #[error("platform: {message}")]
    Platform {
        /// How much of an acting call reached the app, when the app said.
        effect: Option<Effect>,
        /// For people.
        message: String,
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
    /// The error the app answered with, read into its variant.
    #[must_use]
    pub fn from_wire(error: ErrorObject) -> Self {
        let ErrorObject {
            code,
            message,
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
            ErrorCode::InvalidArgument => Self::InvalidArgument { message },
            ErrorCode::NotSupported => Self::NotSupported {
                capability,
                message,
            },
            ErrorCode::Busy => Self::Busy { message },
            ErrorCode::NotFound => Self::NotFound {
                message,
                candidates,
            },
            ErrorCode::UnknownHandle => Self::UnknownHandle {
                kind,
                handle,
                message,
            },
            ErrorCode::Gone => Self::Gone {
                kind,
                handle,
                exit,
                message,
            },
            ErrorCode::Disabled => Self::Disabled { message },
            ErrorCode::ActionUnsupported => Self::ActionUnsupported { message },
            ErrorCode::NotForeground => Self::NotForeground { message },
            ErrorCode::FocusElsewhere => Self::FocusElsewhere { message },
            ErrorCode::OutsideTarget => Self::OutsideTarget { message },
            ErrorCode::Timeout => Self::Timeout { effect, message },
            ErrorCode::InputHeld => Self::InputHeld { message },
            ErrorCode::ShuttingDown => Self::ShuttingDown { message },
            ErrorCode::Ambiguous => Self::Ambiguous {
                message,
                candidates,
            },
            ErrorCode::NotActionable => Self::NotActionable {
                failed,
                covered_by,
                message,
            },
            ErrorCode::Stale => Self::Stale { handle, message },
            ErrorCode::Version => Self::Version {
                app,
                client,
                message,
            },
            ErrorCode::NotDriver => Self::NotDriver { message },
            // `Platform`, and every code a newer schema adds, which an older reader reads as it.
            _ => Self::Platform { effect, message },
        }
    }

    /// The app's process went: `exit` is its exit code when the OS reported one.
    pub(crate) fn process_gone(message: impl Into<String>, exit: Option<i32>) -> Self {
        Self::Gone {
            kind: Some(HandleKind::Process),
            handle: None,
            exit,
            message: message.into(),
        }
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
            | Self::Protocol { .. }
            | Self::Expectation { .. }
            | Self::Io(_) => return None,
        })
    }

    /// Whether the same call can succeed later. A timeout of a call that may have acted is never
    /// to be retried as it stands: read the tree first.
    #[must_use]
    pub fn retry(&self) -> Retry {
        match self {
            Self::Timeout { effect: None, .. } | Self::NotDriver { .. } => Retry::Soon,
            Self::Timeout {
                effect: Some(_), ..
            } => Retry::Never,
            _ => self.code().map_or(Retry::Never, ErrorCode::default_retry),
        }
    }

    /// How much of an acting call reached the app, when the failure says.
    #[must_use]
    pub fn effect(&self) -> Option<&Effect> {
        match self {
            Self::Timeout { effect, .. } | Self::Platform { effect, .. } => effect.as_ref(),
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
        matches!(
            self,
            Self::NotSupported { .. }
                | Self::InterruptedByUser { .. }
                | Self::Lane {
                    unavailable: true,
                    ..
                }
        )
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
            _ => None,
        }
    }
}
