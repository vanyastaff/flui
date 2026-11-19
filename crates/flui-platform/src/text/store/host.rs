//! The platform side of a pull-model window's text input (ADR-0135).

use std::rc::Rc;

use super::contract::TextStore;
use super::lock::{LockOutcome, TextStoreError};
use super::projection::edit_async;
use crate::Unsupported;

/// A pull-model platform's side of one window's text input: the object a
/// window's input-method integration (Win32 text services) hands the
/// presentation, through which the presentation tells it which field's
/// [`TextStore`] it reads and edits (ADR-0090 §3, ADR-0142 item 7, ADR-0135).
///
/// Owner thread only: shared as `Rc<dyn TextStoreHost>` and not `Send`.
/// A backend hands one out only to a caller holding owner-thread proof
/// (`flui_native::OwnerPlatform::text_store_host`).
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
    /// - [`TextStoreHostError::Unsupported`] when the window's text services
    ///   are gone.
    ///
    /// Either way the caller commits the composition in place
    /// ([`commit_composition_in_place`]).
    fn complete_composition(
        &self,
        store: &Rc<dyn TextStore>,
    ) -> Result<CompositionEnd, TextStoreHostError>;

    /// The user tapped `store`'s field, which has the input: show the
    /// platform's soft keyboard over it if the user put it away, as a tap
    /// on the platform's own text field does. A host whose platform shows
    /// it by itself, or has none, does nothing; so does one for a `store`
    /// it is not serving (compared by identity, as
    /// [`Self::complete_composition`] compares). Called with no borrow
    /// held, after the window's gesture arena gave the tap to the field.
    fn show_keyboard(&self, store: &Rc<dyn TextStore>);

    /// Who draws the selection UI of the field this host serves and owns the
    /// gestures that make a selection in it. One answer for the host's life:
    /// iOS's host puts a `UITextInteraction` over every field it serves.
    ///
    /// The default is [`SelectionUi::Field`]: the platform draws nothing over
    /// the field.
    fn selection_ui(&self) -> SelectionUi {
        SelectionUi::Field
    }
}

/// Who draws a focused field's caret, selection highlight, selection handles,
/// magnifier and edit menu, and recognizes the taps, long presses and drags
/// that place its caret and make its selection
/// ([`TextStoreHost::selection_ui`]).
///
/// The field always draws its text and its composition's underline.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
#[expect(
    clippy::exhaustive_enums,
    reason = "the selection UI is the field's or the platform's: there is no third party to draw it"
)]
pub enum SelectionUi {
    /// The field, as on every platform whose input method draws nothing
    /// over it.
    #[default]
    Field,
    /// The platform: the field draws none of it and recognizes none of those
    /// gestures while it holds the input, and the platform's edit menu
    /// reaches it through [`TextStore::perform_edit_command`].
    Platform,
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
    /// The window's text services are not there: they were shut down as the
    /// window closed ([`NoWindow`](crate::UnsupportedReason::NoWindow)), or
    /// they failed and the window fell back to character messages.
    #[error("the window's text services cannot act: {0}")]
    Unsupported(#[from] Unsupported),
}

impl TextStoreHostError {
    /// What is not here, when that is why the host could not act.
    #[must_use]
    pub const fn unsupported(&self) -> Option<Unsupported> {
        match self {
            Self::Unsupported(unsupported) => Some(*unsupported),
            Self::NotFocused => None,
        }
    }
}

/// Commit `store`'s composition where it stands: clear its composing range
/// and keep the text. What the caller of
/// [`TextStoreHost::complete_composition`] does when the platform could not
/// end the composition, what a host does for a request it queued and then
/// could not complete, and what a push-model or storeless presentation does
/// instead of asking a host.
///
/// The lock is asynchronous, so inside a frame transaction the edit waits for
/// the commit anchor like any other grant, and so does one requested while a
/// grant on the store is running (from inside that grant's code).
///
/// Returns what the store did with the request. [`LockOutcome::Deferred`]
/// means the commit is queued in the store: whoever accepted the completion
/// owes it a later [`TextStore::run_deferred_grants`], whether or not its
/// frame transaction was open when it asked, since the grant ahead of it may
/// fail and leave it queued. A store that queued the commit and then refuses
/// it hands that refusal to `late_refusal`, which the caller supplies to
/// report it (the contract does not log).
///
/// # Errors
///
/// The store's refusal of the lock ([`TextStore::request_lock`]), or of the
/// edit that ends the composition when it ran before this returned; the
/// composition is then left as it was.
pub fn commit_composition_in_place(
    store: &dyn TextStore,
    late_refusal: impl FnOnce(TextStoreError) + 'static,
) -> Result<LockOutcome, TextStoreError> {
    edit_async(
        store,
        |session| {
            if session.composition().is_some() {
                session.set_composition(None)
            } else {
                Ok(())
            }
        },
        late_refusal,
    )
}
