//! The text-store contract a field implements and the observer a platform
//! registers on it.

use std::rc::Rc;

use super::OwnerCalls;
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

/// An application change about to become visible to the input method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum TextStoreChange {
    /// Text changed, with a corresponding selection notification.
    Text(TextChange),
    /// Only the selection changed.
    Selection,
    /// The composition or the committed text it stands for changed, with
    /// a corresponding selection-display refresh.
    Composition,
}

/// Notifications from a store to the platform, TSF's `ITextStoreACPSink`.
///
/// A store calls these only for changes the platform did not make itself
/// (an edit by the application, a relayout). A new publication starts only
/// outside a lock with its [`CommitGate`] open; changes made while either
/// prevents it wait until the next eligible owner anchor, at the latest by
/// [`TextStore::run_deferred_grants`]. An already admitted publication still
/// finishes its matched notifications if a callback closes the gate. New
/// grants honor the current gate, so synchronous reads from such a later
/// notification may be refused rather than run inside the closed gate.
pub trait TextStoreObserver {
    /// Publish an application's change between the input method's before
    /// and after notifications. A synchronous read before `publish` sees
    /// the previous published document; one inside the resulting
    /// [`Self::text_changed`] or [`Self::selection_changed`] sees the new
    /// document. Platform edits are never echoed through this method.
    ///
    /// The store captures one observer for the entire transaction. Calling
    /// `publish` delivers its matching after notifications synchronously.
    /// The store publishes exactly once even if this method omits, repeats
    /// or panics before that call. Read-write requests during this method
    /// are deferred, or synchronously refused, through the normal bounded
    /// lock queue. A reentrant application edit is a later publication.
    ///
    /// All callbacks and cleanup use `calls`, preserving the first failure
    /// across before, publication and after notifications. The store owns
    /// resuming that failure after finishing the transaction and its queue.
    fn publish_change(
        &self,
        _change: TextStoreChange,
        publish: &mut dyn FnMut(&mut OwnerCalls),
        calls: &mut OwnerCalls,
    ) {
        publish(calls);
    }

    /// The document changed outside a platform session.
    fn text_changed(&self, change: TextChange);

    /// Refresh the selection display after an application change. This also
    /// follows a text or composition change whose numeric selection stayed
    /// the same, since its display may differ.
    fn selection_changed(&self);

    /// The composition changed without a text or selection change outside
    /// a platform session. Delivered after [`Self::publish_change`]
    /// publishes it; its preceding read sees the previous composition.
    fn composition_changed(&self) {}

    /// The document's geometry changed (`OnLayoutChange`).
    fn layout_changed(&self);

    /// [`TextStore::status`] changed.
    fn status_changed(&self);
}
