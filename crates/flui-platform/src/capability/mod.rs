//! The capability seam (ADR-0152): how a platform service reaches a widget.
//!
//! A capability is a marker type, [`Capability`], owned by the crate that owns
//! its handle. Providers ([`CapabilityProvider`]) live per UI runtime; a
//! widget acquires a handle in a lifecycle hook, and the registry calls the
//! provider on every acquisition, with no cache. A provider is handed a
//! [`ProviderContext`], not a window: its [`owner`](ProviderContext::owner)
//! poster is how an operating-system callback gets back to the owner thread,
//! and its [`window`](ProviderContext::window) is the acquiring widget's
//! window as a [`WindowRef`], if it has one.
//!
//! What the operating system finishes later is a [`Completion`]: one
//! owner-thread result type, resolved at an owner turn boundary through its
//! [`Resolver`], never inline and never on the thread the operating system
//! called back on.
//!
//! A package or a capability crate registers its providers with a [`Plugin`]
//! through a [`CapabilityRegistrar`]; the registry itself, its precedence and
//! its conflicts live in the UI runtime.
//!
//! Under the `scripted` feature, [`scripted`] is the scenario primitive a
//! capability's simulated provider is built from: typed, ordered answers,
//! values and event streams on a virtual clock, every answer settling at an
//! owner turn through a `Completion` (ADR-0154 §10).

mod activity_result;
mod completion;
mod owner;
mod registrar;
mod runtime_local;
#[cfg(feature = "scripted")]
pub mod scripted;
mod window_lifetime;
mod window_ref;

pub use activity_result::{
    Activity, ActivityBridge, ActivityResult, CapabilityId, DataRefused, HostBridge, IntentFrame,
    InvalidResultKey, JniFrame, MAX_OPEN_REQUESTS, MAX_OPEN_REQUESTS_PER_CAPABILITY,
    MAX_RESULT_KEY_LEN, RequestKey, ResultCode, ResultData, ResultDataHold, ResultKey, ResultMint,
    ResultOutcome, ResultRefused, ResultRequest, ResultTicket, ResultTicketHold, ResultToken,
    ResultTokenMint,
};
pub use completion::{Abandoned, Completion, CompletionSource, Resolver};
pub use owner::{OwnerJob, OwnerPoster, OwnerRetired, OwnerScope, PendingCompletions};
pub use registrar::{CapabilityRegistrar, Plugin, RegisteredProvider};
pub use runtime_local::RuntimeLocals;
pub use window_lifetime::{WindowCloseWatch, WindowCloser, WindowLifetime};
pub use window_ref::WindowRef;

use std::rc::Rc;

use crate::Unsupported;

/// A platform capability: a marker type whose [`Handle`](Self::Handle) a
/// widget acquires through the seam.
///
/// The marker lives in the crate that owns the handle (ADR-0152 §2, item 1).
/// The registry is keyed by the marker, not by an implementation, so an
/// application substitutes a provider without replacing anything else.
pub trait Capability: 'static {
    /// What a widget holds. Owner-local handles stay `!Send`.
    type Handle: Clone + 'static;
    /// The capability's name, unique within one application: two capabilities
    /// sharing it refuse to run. It is a [`ResultKey`] (1 to
    /// [`MAX_RESULT_KEY_LEN`] bytes of printable ASCII), since the Android
    /// host keys a provider's requests for results under it: a registry
    /// refuses any other name when it is built.
    const NAME: &'static str;
}

/// Makes the handle of capability `C` for one acquisition.
///
/// Called on the owner thread, once per acquisition: the registry keeps no
/// cache, so a handle that holds an operating-system subscription ends it
/// when its last clone drops. A provider may share state internally.
pub trait CapabilityProvider<C: Capability>: 'static {
    /// The handle for the widget acquiring it in `ctx`.
    ///
    /// # Errors
    ///
    /// [`Unsupported`] when this platform, device or context cannot provide
    /// `C`; a capability that needs a window answers
    /// [`NoWindow`](crate::UnsupportedReason::NoWindow) itself.
    fn provide(&self, ctx: &ProviderContext<'_>) -> Result<C::Handle, Unsupported>;
}

impl<C, F> CapabilityProvider<C> for F
where
    C: Capability,
    F: Fn(&ProviderContext<'_>) -> Result<C::Handle, Unsupported> + 'static,
{
    fn provide(&self, ctx: &ProviderContext<'_>) -> Result<C::Handle, Unsupported> {
        self(ctx)
    }
}

/// What a provider is handed for one acquisition.
///
/// Grows by methods, never by public fields (ADR-0152 §2, item 6).
#[non_exhaustive]
pub struct ProviderContext<'a> {
    owner: &'a OwnerPoster,
    completions: &'a CompletionSource,
    locals: &'a RuntimeLocals,
    host: &'a HostBridge,
    /// The capability being provided: set by its [`RegisteredProvider`] as
    /// it calls the provider, never by the context's maker.
    capability: Option<&'a CapabilityId>,
    window: Option<WindowRef>,
}

impl<'a> ProviderContext<'a> {
    /// The context a UI runtime hands a provider: `host` is what the running
    /// host offers beyond the window. The capability it is scoped to is the
    /// one whose [`RegisteredProvider`] the context is handed to.
    #[must_use]
    pub fn new(
        owner: &'a OwnerPoster,
        completions: &'a CompletionSource,
        locals: &'a RuntimeLocals,
        host: &'a HostBridge,
        window: Option<WindowRef>,
    ) -> Self {
        Self {
            owner,
            completions,
            locals,
            host,
            capability: None,
            window,
        }
    }

    /// This context, scoped to `capability`.
    pub(crate) fn scoped<'s>(&'s self, capability: &'s CapabilityId) -> ProviderContext<'s> {
        ProviderContext {
            owner: self.owner,
            completions: self.completions,
            locals: self.locals,
            host: self.host,
            capability: Some(capability),
            window: self.window.clone(),
        }
    }

    /// The running Android Activity, which starts activities for results
    /// ([`Activity::launch_for_result`]) keyed under the acquiring
    /// capability; `None` on every other platform, between two Activities,
    /// for a capability whose name is not a [`ResultKey`], and for a context
    /// not handed through a [`RegisteredProvider`].
    #[must_use]
    pub fn activity(&self) -> Option<Activity> {
        self.host.activity(self.capability?)
    }

    /// A poster that runs a closure on the owner thread; it may be cloned and
    /// called from any thread.
    #[must_use]
    pub fn owner(&self) -> &OwnerPoster {
        self.owner
    }

    /// Where a handle makes the [`Completion`]s of the calls it answers later.
    #[must_use]
    pub fn completions(&self) -> &CompletionSource {
        self.completions
    }

    /// The acquiring UI runtime's `T`, shared by every handle a provider
    /// hands out in that runtime and ended with it: the one place for state a
    /// provider keeps per runtime ([`RuntimeLocals`]). Made with
    /// `T::default()` the first time.
    #[must_use]
    pub fn runtime_local<T: Default + 'static>(&self) -> Rc<T> {
        self.locals.get_or_default()
    }

    /// The window the acquiring widget belongs to, if any.
    #[must_use]
    pub fn window(&self) -> Option<WindowRef> {
        self.window.clone()
    }
}

impl std::fmt::Debug for ProviderContext<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProviderContext")
            .field("window", &self.window)
            .finish_non_exhaustive()
    }
}
