//! The platform side of a pull-model window's text input (ADR-0135).

use std::rc::Rc;

use super::lock::{LockGrant, LockTiming};
use super::store::TextStore;

/// A pull-model platform's side of one window's text input: the object a
/// window's input-method integration (Win32 text services) hands the
/// presentation, through which the presentation tells it which field's
/// [`TextStore`] it reads and edits (ADR-0090 §3, ADR-0142 item 7, ADR-0135).
///
/// Owner thread only: shared as `Rc<dyn TextStoreHost>` and not `Send`.
/// A backend hands one out only to a caller holding owner-thread proof
/// (`flui_platform::OwnerPlatform::text_store_host`).
///
/// # Reentry
///
/// A call on the host can reach application code: the platform ends a
/// composition by editing the store, and the store's owner runs after the
/// edit. That code may call the host again. The presentation's owner never
/// nests its own calls (it queues the inner one until the outer returns),
/// but a platform call into a store that started outside FLUI (a text
/// service asking for a lock) can reach it too. Inside such a call the host
/// queues the request and applies it when that call returns, in the order it
/// was made.
pub trait TextStoreHost {
    /// `store` now receives this window's text input; `None` when no field
    /// does. The same store again is a no-op. Called with no borrow held.
    fn focus_store(&self, store: Option<Rc<dyn TextStore>>);

    /// End the platform's composition in `store`, keeping its text.
    ///
    /// `store` is compared by identity (`Rc::ptr_eq`) with the store this
    /// host was last told to focus, queued focus changes included. A request
    /// the host queues inside a platform call keeps `store`, so it ends that
    /// store's composition even when a later focus change is queued behind
    /// it.
    ///
    /// # Errors
    ///
    /// - [`TextStoreHostError::NotFocused`] when `store` is not the focused
    ///   store: the platform composes in no other, so it has nothing to end.
    /// - [`TextStoreHostError::Unavailable`] when the window's text services
    ///   are gone.
    ///
    /// Either way the caller commits the composition in place
    /// ([`commit_composition_in_place`]).
    fn complete_composition(
        &self,
        store: &Rc<dyn TextStore>,
    ) -> Result<CompositionEnd, TextStoreHostError>;
}

/// How [`TextStoreHost::complete_composition`] ended the composition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum CompositionEnd {
    /// The platform committed its composition into the store.
    Committed,
    /// The platform could not end its composition (its lock was refused);
    /// it discarded its own. The caller commits the store's composition in
    /// place ([`commit_composition_in_place`]).
    Abandoned,
    /// The request arrived inside a platform call into a store. The host
    /// ends the composition when that call returns and, if the platform
    /// then cannot, commits it in place itself.
    Deferred,
}

/// Why a [`TextStoreHost`] could not act.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum TextStoreHostError {
    /// The store asked about is not the one the host was last told to focus.
    #[error("the store is not the window's focused text store")]
    NotFocused,
    /// The window's text services were shut down (the window is closing, or
    /// they failed and the window fell back to character messages).
    #[error("the window's text services are unavailable")]
    Unavailable,
}

/// Commit `store`'s composition where it stands: clear its composing range
/// and keep the text. What the caller of
/// [`TextStoreHost::complete_composition`] does when the platform could not
/// end the composition, what a host does for a request it queued and then
/// could not complete, and what a push-model or storeless presentation does
/// instead of asking a host.
///
/// The lock is asynchronous, so inside a frame transaction the edit waits for
/// the commit anchor like any other grant. A refusal is logged with
/// `tracing::warn!`: no caller is left to act on it.
pub fn commit_composition_in_place(store: &dyn TextStore) {
    let grant = LockGrant::read_write(|session| {
        if session.composition().is_some()
            && let Err(error) = session.set_composition(None)
        {
            tracing::warn!(?error, "a composition could not be committed in place");
        }
    });
    if let Err(error) = store.request_lock(grant, LockTiming::Async) {
        tracing::warn!(?error, "a composition could not be committed in place");
    }
}
