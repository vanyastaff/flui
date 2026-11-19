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

    /// What a press at `point` (window-root logical pixels) would land on, by
    /// the window's own hit test and what each target there would do with a
    /// touch going down: the field, another target that would contend for
    /// the press, or nothing that does ([`PressTarget`]). A platform that
    /// recognizes gestures over the field itself (iOS's `UITextInteraction`)
    /// begins one only on the field, or where nothing else contends for the
    /// press, so every control keeps its tap.
    ///
    /// Asked of the store, outside any lock, on the owner thread with no
    /// borrow held: answering runs code the store does not own (render
    /// objects' hit tests, recognizers' admission predicates), which must not
    /// run inside a read lock. The caller contains a panic.
    ///
    /// No default: a store that wraps another forwards it, and one that left
    /// it out would hand a control's tap to the platform.
    ///
    /// # Errors
    ///
    /// [`TextStoreError::NoLayout`] when the field is not laid out or its tree
    /// cannot be hit-tested now, and [`TextStoreError::WindowUnavailable`]
    /// when the window cannot be asked now (off its owner thread, closing): a
    /// platform begins no gesture on either.
    fn press_target_at(
        &self,
        point: flui_foundation::geometry::Point<f64>,
    ) -> Result<super::PressTarget, TextStoreError>;

    /// Whether the field would perform `command` now, as its own menu would
    /// offer it: a platform shows the command in its edit menu only when it
    /// would. Called on the owner thread with no borrow held, outside a lock.
    ///
    /// The default performs none: a store that is not a field's has no
    /// clipboard or selection behaviour of its own.
    fn edit_command_enabled(&self, command: EditCommand) -> bool {
        let _ = command;
        false
    }

    /// Perform `command` as the field's own menu or shortcut performs it,
    /// with the same rules (a protected field copies nothing, a cut deletes
    /// only what the clipboard took, a paste ends the composition first), and
    /// answer whether the field took it. What the edit changes reaches the
    /// observer as an application edit, which the platform did not make
    /// through a session. Called on the owner thread with no borrow held,
    /// outside a lock and outside a frame transaction, as a key press is.
    ///
    /// The default performs none.
    fn perform_edit_command(&self, command: EditCommand) -> bool {
        let _ = command;
        false
    }
}

/// An edit the platform's own text UI asks the focused field for: an item of
/// iOS's edit menu, which `UITextInteraction` shows over the field
/// (`cut:`, `copy:`, `paste:`, `select:`, `selectAll:`). The field performs it with its
/// own clipboard and selection rules ([`TextStore::perform_edit_command`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum EditCommand {
    /// Copy the selection to the clipboard and delete it.
    Cut,
    /// Copy the selection to the clipboard.
    Copy,
    /// Replace the selection with the clipboard's text.
    Paste,
    /// Select the word at the caret (`select:`, the item a collapsed caret's
    /// menu offers).
    SelectWord,
    /// Select the whole text.
    SelectAll,
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

    /// The document's geometry changed (`OnLayoutChange`): its caret or
    /// composition moved, or its text was laid out again. Reported on every
    /// caret move, typing included, so an input method's candidate window
    /// follows it.
    fn layout_changed(&self);

    /// The field itself moved in the window: its visible box moved or changed
    /// size (a scroll, a transform, the keyboard lifting the view, an
    /// ancestor's clip changing), whatever its caret did. Not reported for a
    /// caret move inside an unmoved field, so a platform that redraws its own
    /// selection UI over the field asks for its geometry again only on this.
    /// Delivered under the same rule as [`Self::layout_changed`]. The default
    /// ignores it: an input method that follows the caret alone has the
    /// layout change.
    fn field_moved(&self) {}

    /// [`TextStore::status`] changed, in some part: a field tells of any
    /// difference, including one a platform has no word for.
    ///
    /// A host compares what it passes on of the new status with what it last
    /// told the platform and acts only on a difference; a difference that
    /// would restart the input method waits until no composition is in
    /// progress, and none shows a keyboard (the field's focus and its own tap
    /// do).
    fn status_changed(&self);
}
