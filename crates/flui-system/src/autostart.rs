//! Starting the application when the user logs in.
//!
//! A widget acquires [`Autostart`] through
//! `ctx.capability::<AutostartCapability>()` and asks with an
//! [`AutostartRequest`]. The system may ask the user, attached to the
//! acquiring widget's window: the answer says what the system granted, which
//! may be less than was asked.

use std::{fmt, rc::Rc};

use flui_platform::capability::{
    Capability, Completion, CompletionSource, ProviderContext, Resolver,
};
use flui_platform::{SystemFailure, Unsupported};

/// The autostart capability: `ctx.capability::<AutostartCapability>()` hands
/// out [`Autostart`].
#[derive(Debug)]
#[expect(
    clippy::exhaustive_enums,
    reason = "a capability marker has no values; it is only ever named"
)]
pub enum AutostartCapability {}

impl Capability for AutostartCapability {
    type Handle = Autostart;
    const NAME: &'static str = "autostart";
}

/// Whether the application starts when the user logs in, as the system
/// answered a request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum AutostartState {
    /// The system starts the application at login.
    Enabled,
    /// The system does not start it.
    Disabled,
}

/// Why an autostart request gave no answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum AutostartError {
    /// The request is malformed, as the [`MalformedAutostart`] says; the
    /// system was not asked.
    #[error("the autostart request is malformed: {0}")]
    InvalidRequest(#[from] MalformedAutostart),
    /// The user, or the system's policy, refused to let the application run
    /// in the background.
    #[error("the user or the system refused")]
    Denied,
    /// The service is not here (no background portal, or one too old, or a
    /// sandbox it cannot start), as the [`Unsupported`] says.
    #[error("autostart is not available: {0}")]
    Unsupported(#[from] Unsupported),
    /// The system failed in a way the handle does not name.
    #[error("the system failed: {0}")]
    Failed(#[from] SystemFailure),
}

/// What an autostart request answers.
pub type AutostartResult = Result<AutostartState, AutostartError>;

/// What is wrong with an [`AutostartRequest`], which the handle refuses
/// before the system is asked.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum MalformedAutostart {
    /// The reason is blank, longer than [`AutostartRequest::MAX_REASON_LEN`],
    /// or holds a control character.
    #[error("the reason is not one line of text")]
    Reason,
}

/// One autostart request: start the application at login, or stop, and the
/// reason the system may show the user.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct AutostartRequest {
    enabled: bool,
    /// The reason shown to the user, if set: one line.
    pub reason: Option<String>,
}

impl AutostartRequest {
    /// The longest reason, in bytes: a dialog shows one line.
    pub const MAX_REASON_LEN: usize = 256;

    /// Start the application when the user logs in.
    #[must_use]
    pub fn enable() -> Self {
        Self {
            enabled: true,
            reason: None,
        }
    }

    /// Stop starting it at login.
    #[must_use]
    pub fn disable() -> Self {
        Self {
            enabled: false,
            reason: None,
        }
    }

    /// The reason the system may show the user.
    #[must_use]
    pub fn reason(mut self, reason: impl Into<String>) -> Self {
        self.reason = Some(reason.into());
        self
    }

    /// Whether it asks to start the application at login.
    #[must_use]
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    fn check(&self) -> Result<(), MalformedAutostart> {
        let one_line = |reason: &String| {
            reason.len() <= Self::MAX_REASON_LEN
                && !reason.trim().is_empty()
                && !reason.chars().any(char::is_control)
        };
        if self.reason.as_ref().is_none_or(one_line) {
            Ok(())
        } else {
            Err(MalformedAutostart::Reason)
        }
    }
}

/// The system's autostart as [`Autostart`] reaches it.
///
/// Owner-local. A request it receives is well formed; it answers through the
/// [`Resolver`] when the system does, never calling back into the handle.
pub trait AutostartBackend: 'static {
    /// Ask the system to start the application at login, or to stop.
    fn request(&self, request: AutostartRequest, answer: Resolver<AutostartResult>);
}

/// Widget-facing autostart.
///
/// Owner-thread (`!Send`), cheap to clone. An answer settles at an owner
/// turn after the system answers, never inside the call that asked.
#[derive(Clone)]
pub struct Autostart {
    backend: Rc<dyn AutostartBackend>,
    completions: CompletionSource,
}

impl Autostart {
    /// A handle over `backend` for the acquisition `ctx`.
    pub fn new(backend: impl AutostartBackend, ctx: &ProviderContext<'_>) -> Self {
        Self {
            backend: Rc::new(backend),
            completions: ctx.completions().clone(),
        }
    }

    /// Ask the system for `request`; a malformed one answers
    /// [`AutostartError::InvalidRequest`] without asking. The answer says
    /// what the system granted: [`AutostartState::Disabled`] to a request
    /// that enables means the system left it off.
    pub fn request(&self, request: AutostartRequest) -> Completion<AutostartResult> {
        let (completion, answer) = self.completions.pending();
        match request.check() {
            Ok(()) => self.backend.request(request, answer),
            Err(malformed) => {
                // The owner retiring first leaves nothing waiting.
                let _ = answer.resolve(Err(malformed.into()));
            }
        }
        completion
    }
}

impl fmt::Debug for Autostart {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Autostart").finish_non_exhaustive()
    }
}
