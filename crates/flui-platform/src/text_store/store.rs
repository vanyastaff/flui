//! The text-store contract a field implements and the observer a platform
//! registers on it.

use std::rc::Rc;

use super::lock::{CommitGate, LockGrant, LockOutcome, LockTiming, TextStoreError};
use super::session::{TextChange, TextStoreStatus};

/// A text field's document as the platform's input method sees it
/// (ADR-0090 §1).
///
/// The platform never touches the document directly: it asks for a lock
/// with [`Self::request_lock`], and reads or edits through the session the
/// grant receives. Owner-thread only: stores are shared as
/// `Rc<dyn TextStore>` and are not `Send`.
///
/// An implementation embeds a `flui_interaction::text_input::LockArbiter` so every
/// store follows the same lock rules, hands it the gate
/// [`Self::set_commit_gate`] receives, and passes the conformance kit in
/// `flui_testing::text_store_kit`.
pub trait TextStore {
    /// The store's standing properties.
    fn status(&self) -> TextStoreStatus;

    /// Run `grant` now, queue it, or refuse it — see
    /// `flui_interaction::text_input::LockArbiter` for the rules.
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

    /// Follow `gate` from now on: while it is shut, a sync lock is refused
    /// and an async one waits for [`Self::run_deferred_grants`].
    ///
    /// The owner that attaches the store installs its presentation's gate
    /// here, so a store has no transaction flag of its own; an
    /// implementation passes `gate` to its arbiter's
    /// `LockArbiter::set_gate`. One that
    /// ignores it commits mid-frame, and fails the conformance kit's
    /// transaction cases.
    fn set_commit_gate(&self, gate: CommitGate);

    /// Register the platform's observer, replacing any earlier one, or
    /// remove it with `None`.
    fn set_observer(&self, observer: Option<Rc<dyn TextStoreObserver>>);

    /// The field's text moved on the screen without a relayout: its window
    /// moved or changed scale. The platform calls this; the store reports
    /// [`TextStoreObserver::layout_changed`] under the same rule as its own
    /// layout changes, at once or at the next point it may, so an input
    /// method's candidate window follows the text.
    ///
    /// The default reports nothing: a store without screen geometry of its
    /// own has no layout to report.
    fn screen_moved(&self) {}
}

/// Notifications from a store to the platform, TSF's `ITextStoreACPSink`.
///
/// A store calls these only for changes the platform did not make itself
/// (an edit by the application, a relayout), and never while a lock is held
/// or its [`CommitGate`] is shut: a change made then is reported once both
/// allow it, at the latest by the next
/// [`TextStore::run_deferred_grants`]. So an observer may request a
/// synchronous lock from inside a notification and be granted it.
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
