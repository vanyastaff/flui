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

mod completion;
mod owner;
mod registrar;
mod window_ref;

pub use completion::{Abandoned, Completion, CompletionSource, Resolver};
pub use owner::{OwnerJob, OwnerPoster, OwnerRetired, OwnerScope, PendingCompletions};
pub use registrar::{CapabilityRegistrar, Plugin, RegisteredProvider};
pub use window_ref::WindowRef;

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
    /// sharing it refuse to run.
    const NAME: &'static str;
}

/// Makes the handle of capability `C` for one acquisition.
///
/// Called on the owner thread, once per acquisition: the registry keeps no
/// cache, so a handle that holds an operating-system subscription ends it
/// when its last clone drops. A provider may share state internally.
pub trait CapabilityProvider<C: Capability>: 'static {
    /// The handle for the widget acquiring it in `cx`.
    ///
    /// # Errors
    ///
    /// [`Unsupported`] when this platform, device or context cannot provide
    /// `C`; a capability that needs a window answers
    /// [`NoWindow`](crate::UnsupportedReason::NoWindow) itself.
    fn provide(&self, cx: &ProviderContext<'_>) -> Result<C::Handle, Unsupported>;
}

impl<C, F> CapabilityProvider<C> for F
where
    C: Capability,
    F: Fn(&ProviderContext<'_>) -> Result<C::Handle, Unsupported> + 'static,
{
    fn provide(&self, cx: &ProviderContext<'_>) -> Result<C::Handle, Unsupported> {
        self(cx)
    }
}

/// What a provider is handed for one acquisition.
///
/// Grows by methods, never by public fields (ADR-0152 §2, item 6).
#[non_exhaustive]
pub struct ProviderContext<'a> {
    owner: &'a OwnerPoster,
    completions: &'a CompletionSource,
    window: Option<WindowRef>,
}

impl<'a> ProviderContext<'a> {
    /// The context a UI runtime hands its providers.
    #[must_use]
    pub fn new(
        owner: &'a OwnerPoster,
        completions: &'a CompletionSource,
        window: Option<WindowRef>,
    ) -> Self {
        Self {
            owner,
            completions,
            window,
        }
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
