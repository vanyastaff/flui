//! The platform side of a pull-model window's text input (ADR-0135).

use std::rc::Rc;

use super::store::TextStore;

/// A pull-model platform's side of one window's text input: the object a
/// window's input-method integration (Win32 text services) hands the
/// presentation, through which the presentation tells it which field's
/// [`TextStore`] it reads and edits (ADR-0090 §3, ADR-0135).
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

    /// End the platform's composition in the focused store, keeping its
    /// text.
    ///
    /// # Errors
    ///
    /// [`TextStoreHostError::Unavailable`] when the window's text services
    /// are gone; the caller then treats the composition as abandoned.
    fn complete_composition(&self) -> Result<CompositionEnd, TextStoreHostError>;
}

/// How [`TextStoreHost::complete_composition`] ended the composition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum CompositionEnd {
    /// The platform committed its composition into the store.
    Committed,
    /// The platform could not end its composition (its lock was refused);
    /// it discarded its own. The caller clears the store's composing range,
    /// keeping the text.
    Abandoned,
    /// The request arrived inside a platform call into a store. The host
    /// ends the composition when that call returns and, if the platform
    /// then cannot, clears the store's composing range itself.
    Deferred,
}

/// Why a [`TextStoreHost`] could not act.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum TextStoreHostError {
    /// The window's text services were shut down (the window is closing, or
    /// they failed and the window fell back to character messages).
    #[error("the window's text services are unavailable")]
    Unavailable,
}
