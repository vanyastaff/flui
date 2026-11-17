//! Starting the application when the user logs in.
//!
//! A widget acquires [`LaunchAtLogin`] through
//! `ctx.capability::<LaunchAtLoginCapability>()`. The entry names the
//! running executable; the user's own choice in the system's settings is
//! reported, never overridden.

use std::fmt;
use std::rc::Rc;

use flui_platform::capability::{
    Capability, Completion, CompletionSource, ProviderContext, Resolver,
};
use flui_platform::{SystemFailure, Unsupported};

/// The launch-at-login capability:
/// `ctx.capability::<LaunchAtLoginCapability>()` hands out a
/// [`LaunchAtLogin`].
#[derive(Debug)]
#[expect(
    clippy::exhaustive_enums,
    reason = "a capability marker has no values; it is only ever named"
)]
pub enum LaunchAtLoginCapability {}

impl Capability for LaunchAtLoginCapability {
    type Handle = LaunchAtLogin;
    const NAME: &'static str = "launch-at-login";
}

/// Whether the application starts when the user logs in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum LoginItemState {
    /// It starts at login.
    Enabled,
    /// It does not: there is no entry, or the entry names another
    /// executable.
    Disabled,
    /// The application asked to start and the user turned it off in the
    /// system's settings (Windows' Startup apps, macOS' Login Items). The
    /// entry stays; only the user turns it back on.
    DisabledByUser,
}

/// What a [`LaunchAtLogin`] request asks the system.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum LoginItemRequest {
    /// Read the state.
    Read,
    /// Start the application at login, then read the state.
    Enable,
    /// Stop starting it at login, then read the state.
    Disable,
}

/// Why the system did not answer a [`LaunchAtLogin`] request.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum LoginItemError {
    /// The system or a policy refused.
    #[error("the system refused")]
    Denied,
    /// This system, or this kind of installation (a packaged application
    /// whose manifest declares its start-up task), has no entry this
    /// handle can write; the [`Unsupported`] says why.
    #[error("launch at login cannot do this: {0}")]
    Unsupported(#[from] Unsupported),
    /// The system failed in a way the handle does not name.
    #[error("the system failed: {0}")]
    Failed(#[from] SystemFailure),
}

/// What a request answers: the state after it.
pub type LoginItemResult = Result<LoginItemState, LoginItemError>;

/// The system's login items as [`LaunchAtLogin`] reaches them.
///
/// Owner-local. It answers each request through its [`Resolver`]; whatever
/// reads or writes the system's settings runs off the owner thread.
pub trait LaunchAtLoginBackend: 'static {
    /// Do what `request` asks and answer the state after it.
    fn ask(&self, request: LoginItemRequest, answer: Resolver<LoginItemResult>);
}

/// Widget-facing launch at login.
///
/// Owner-thread (`!Send`), cheap to clone. Every request answers with a
/// [`Completion`] of the state after it, which settles at an owner turn.
#[derive(Clone)]
pub struct LaunchAtLogin {
    backend: Rc<dyn LaunchAtLoginBackend>,
    completions: CompletionSource,
}

impl LaunchAtLogin {
    /// A handle over `backend` for the acquisition `ctx`.
    #[must_use]
    pub fn new(backend: impl LaunchAtLoginBackend, ctx: &ProviderContext<'_>) -> Self {
        Self {
            backend: Rc::new(backend),
            completions: ctx.completions().clone(),
        }
    }

    /// Whether the application starts at login.
    pub fn state(&self) -> Completion<LoginItemResult> {
        self.ask(LoginItemRequest::Read)
    }

    /// Start the application at login. Answers
    /// [`LoginItemState::DisabledByUser`] when the user turned it off: the
    /// request is kept, the user's choice stands.
    pub fn enable(&self) -> Completion<LoginItemResult> {
        self.ask(LoginItemRequest::Enable)
    }

    /// Stop starting the application at login.
    pub fn disable(&self) -> Completion<LoginItemResult> {
        self.ask(LoginItemRequest::Disable)
    }

    fn ask(&self, request: LoginItemRequest) -> Completion<LoginItemResult> {
        let (completion, answer) = self.completions.pending();
        self.backend.ask(request, answer);
        completion
    }
}

impl fmt::Debug for LaunchAtLogin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LaunchAtLogin").finish_non_exhaustive()
    }
}
