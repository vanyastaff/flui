//! The text-store contract a field implements and the observer a platform
//! registers on it.

use std::rc::Rc;

use super::lock::{LockGrant, LockOutcome, LockTiming, TextStoreError};
use super::session::{TextChange, TextStoreStatus};

/// A text field's document as the platform's input method sees it
/// (ADR-0090 §1).
///
/// The platform never touches the document directly: it asks for a lock
/// with [`Self::request_lock`], and reads or edits through the session the
/// grant receives. Owner-thread only: stores are shared as
/// `Rc<dyn TextStore>` and are not `Send`.
///
/// An implementation embeds a [`LockArbiter`](super::LockArbiter) so every
/// store follows the same lock rules, and passes the conformance kit in
/// `flui_testing::text_store_kit`.
pub trait TextStore {
    /// The store's standing properties.
    fn status(&self) -> TextStoreStatus;

    /// Run `grant` now, queue it, or refuse it — see
    /// [`LockArbiter`](super::LockArbiter) for the rules.
    ///
    /// # Errors
    ///
    /// [`TextStoreError::SyncLockUnavailable`],
    /// [`TextStoreError::DeferredQueueFull`], or
    /// [`TextStoreError::Detached`] once the field is gone.
    fn request_lock(
        &self,
        grant: LockGrant,
        timing: LockTiming,
    ) -> Result<LockOutcome, TextStoreError>;

    /// Run the grants queued while commits were closed, if commits are
    /// allowed now. The store's owner calls this at each commit anchor;
    /// returns how many ran.
    fn run_deferred_grants(&self) -> usize;

    /// Register the platform's observer, replacing any earlier one, or
    /// remove it with `None`.
    fn set_observer(&self, observer: Option<Rc<dyn TextStoreObserver>>);
}

/// Notifications from a store to the platform, TSF's `ITextStoreACPSink`.
///
/// A store calls these only for changes the platform did not make itself
/// (an edit by the application, a relayout), and never while a lock is
/// held, so an observer may request a synchronous lock from inside one.
pub trait TextStoreObserver {
    /// The document changed outside a platform session.
    fn text_changed(&self, change: TextChange);

    /// The selection changed outside a platform session.
    fn selection_changed(&self);

    /// The document's geometry changed (`OnLayoutChange`).
    fn layout_changed(&self);

    /// [`TextStore::status`] changed.
    fn status_changed(&self);
}
