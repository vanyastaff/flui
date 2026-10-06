//! [`OwnerCalls`]: the one containment around code a text store's owner or
//! platform supplies (ADR-0142 item 8).
//!
//! A store, its [`LockArbiter`](super::LockArbiter) and the presentation that
//! attaches it run code they do not control at these points, and every one
//! of them goes through an [`OwnerCalls`]:
//!
//! | Point | Where |
//! |-------|-------|
//! | a grant's session body; the admitting gate, read before it | `LockArbiter::run_one` (`lock.rs`) |
//! | a store's settle, in a scope that parks each failure in the admitting gate as it is caught | `LockArbiter::run_one` (`lock.rs`) |
//! | a grant refused (synchronous, queue full) | `LockArbiter::defer_or_refuse` (`lock.rs`) |
//! | the caller's grant when an earlier queued grant fails: queued if asynchronous, retained if synchronous | `LockArbiter::request` (`lock.rs`) |
//! | queued grants dropped unrun, by a detach or with the store | `LockArbiter::clear`, `Drop for LockArbiter` (`lock.rs`) |
//! | the gate a store follows, replaced outside the borrow | `LockArbiter::set_gate` (`lock.rs`) |
//! | a parked failure no owner took when the gate's last clone goes | `CommitGate`'s failure cell (`lock.rs`), which retains it as a scope does |
//! | the in-memory owner listener, and its snapshot | `InMemoryTextStore::settle` (`in_memory.rs`) |
//! | a replaced in-memory owner listener or observer, and both when the store goes | `InMemoryTextStore::set_owner_listener`, `set_observer`, `Drop` (`in_memory.rs`) |
//! | observer notifications and the observer snapshot, in the caller's scope (a settle's included) | `InMemoryTextStore::flush_notifications` (`in_memory.rs`), `EditableTextStore::flush_notifications`, `notify` (`flui-widgets` `text/text_store.rs`) |
//! | the flush before a request: a failure there refuses it and retains the grant | `request_lock` of `InMemoryTextStore` (`in_memory.rs`) and `EditableTextStore` (`flui-widgets` `text/text_store.rs`) |
//! | the flush after a request's grants, behind what the last one's settle parked in the gate that admitted it ([`OwnerCalls::run_behind_parked`], [`OwnerCalls::parking_gate`]) | `request_lock`, `run_deferred_grants` of both stores |
//! | a grant a detached field refuses | `EditableTextStore::request_lock` (`flui-widgets` `text/text_store.rs`) |
//! | a grant's body reading or editing the in-memory store: no borrow is held across it, and an application edit wins | `InMemoryTextStore::open` (`in_memory.rs`) |
//! | `on_changed`, and its snapshot, taken when the edit is accepted (a session written back, a key or semantic edit before it runs), so a rebuild the edit causes cannot drop or redirect it | `EditObserver::accept`, `EditObserver::deliver` (`flui-widgets` `text/editable_text.rs`), from `EditableTextStore::settle` and from a key edit's `EditObserver::around`, which contains the edit's listener notification so the owner still hears of the change |
//! | the controller's listeners, and the controller snapshot | `EditableTextStore::settle` (`flui-widgets` `text/text_store.rs`) |
//! | a replaced or detached `EditableText` observer; the observer, `on_changed` and the controller when the store outlives its field | `EditableTextStore::set_observer`, `detach`, `Drop` (`flui-widgets` `text/text_store.rs`), `EditObserver::retire` |
//! | a key edit, and a semantic text edit: the queued grants, the edit with `on_changed`, the platform's notification, each run though an earlier one failed | the key handler and `FieldSemanticsActions::set_text` (`flui-widgets` `text/editable_text.rs`) |
//! | an update's store notifications, the replaced controller and focus node, the focus node replacement (the focus listeners it notifies) and the focus transition, `set_can_request_focus` last | `EditableTextState::did_update_view` (`flui-widgets` `text/editable_text.rs`) |
//! | the cursor-area loop: the store's notifications, the platform's cursor area, the loop rescheduled before a failure is resumed | `CursorAreaLoop::fire` (`flui-widgets` `text/editable_text.rs`) |
//! | a blur's detach, with the token taken before it | the field's focus listener (`flui-widgets` `text/editable_text.rs`) |
//! | dispose: detaching the client and the store, the attachment, the controller listener, each run though an earlier one failed | `EditableTextState::dispose` (`flui-widgets` `text/editable_text.rs`) |
//! | a store installing the presentation's gate, behind what its grants of other stores parked there ([`OwnerCalls::run_parking`]) | `TextInputOwner::attach` (`flui-interaction` `text_input.rs`) |
//! | a client a closed owner rejects, store then callback | `retire_rejected`, from `TextInputOwner::attach` and `TextInputHandle::attach` (`flui-interaction` `text_input.rs`) |
//! | the platform's `set_ime_allowed` | `TextInputOwner::attach`, `detach` (`flui-interaction` `text_input.rs`) |
//! | the platform's `set_ime_cursor_area` | `TextInputOwner::set_cursor_area` (`flui-interaction` `text_input.rs`) |
//! | `on_session_start`, the projection, and the dispatched client snapshot | `TextInputOwner::dispatch` (`flui-interaction` `text_input.rs`) |
//! | clients replaced or detached, and what their destruction parks; once the client is active, attach parks its failures and returns the token | `TextInputOwner::attach`, `detach` (`flui-interaction` `text_input.rs`) |
//! | stores retired at an anchor | `TextInputOwner::run_deferred_grants` (`flui-interaction` `text_input.rs`) |
//! | diagnostics (`tracing` runs a user-installed subscriber) | `TextInputOwner::attach`, `detach`, `dispatch`; `EditableTextState::dispose`, the blur detach and the cursor-area loop |
//!
//! Presentation close (`TextInputOwner::close_with_mode` and its `Drop`)
//! keeps its close-mode containment (ADR-0123), which retires the same
//! values under the same retention rule; it takes a failure parked for its
//! next turn too, ahead of its own, raised by an ordinary close and retained
//! by a preserving one or by `Drop`. The remaining diagnostics (the focus
//! listener's attach warnings, a deferred projection's warning inside a
//! grant body) run inside the containment of the code that calls them: the
//! focus notifier, the grant.
//!
//! An update and the cursor-area loop run inside a frame, whose shut gate
//! defers the notifications of a store behind it to after it; they are
//! contained for a store behind another gate, for the focus listeners an
//! update's node replacement notifies, and for platforms whose cursor-area
//! call is other code.
//!
//! The rules it keeps, in order of the calls a scope makes:
//!
//! - **What owner code is owed is read before it runs.** A caller takes its
//!   obligations, their values and the gate a failure belongs to before the
//!   first call, never after: owner code may reenter, settle a nested session
//!   or move the store to another presentation.
//! - **Every call is contained, and the first failure is authoritative.**
//!   Owner code after a failed call still runs (an observer still hears of
//!   an edit the owner made before it panicked); a later failure is retained
//!   (ADR-0127), never dropped and never reported in its place.
//! - **A failure parked in a gate before a call's own panic came first**, so
//!   [`OwnerCalls::run_parking`] and [`OwnerCalls::retire_parking`] take
//!   what the call parked before keeping that panic; a failure the gate
//!   already held stays for its owner's turn. A failure parked while the
//!   thread was unwinding (a guard's `Drop` in the call's cleanup requested
//!   a grant whose settle failed) came from that unwind, after the panic
//!   that started it, and is kept behind it. When a panic began cannot be
//!   observed from outside it, so a failure parked during an unwind the call
//!   caught itself and then outlived is ordered behind the call's panic too:
//!   the conservative order, which keeps both. The gate records whether it
//!   was parked while unwinding ([`CommitGate::defer_failure`]). A scope
//!   that is the owner's turn takes what was parked before it ran anything
//!   ([`OwnerCalls::take_parked`]).
//! - **A settle's failure belongs to its gate at once**
//!   ([`OwnerCalls::parking_in`]): it is parked the moment it is caught, so a
//!   session the owner's later code opens, whose own settle parks there too,
//!   cannot overtake it.
//! - **A snapshot retires inside the scope** ([`OwnerCalls::retire`]): dropped,
//!   contained, while the scope is healthy; retained once it has failed or
//!   while the thread unwinds (ADR-0127), so a capture whose `Drop` panics
//!   can neither replace the first failure nor panic during an unwind.

use std::any::Any;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::rc::Rc;
use std::sync::Arc;

use super::lock::{CommitGate, LockGrant, Parked};

/// A value whose destruction may run owner code, which a failed
/// [`OwnerCalls`] retains instead of destroying (ADR-0127).
pub trait RetainOnFailure {
    /// Forget whatever dropping `self` would destroy; release the rest.
    fn retain(self);
}

impl<T: ?Sized> RetainOnFailure for Rc<T> {
    /// A clone that is not the last owner runs no owner code when dropped,
    /// so only the last one is forgotten.
    fn retain(self) {
        if Rc::strong_count(&self) == 1 {
            std::mem::forget(self);
        }
    }
}

impl<T: ?Sized> RetainOnFailure for Arc<T> {
    /// Always forgotten: another thread may release its clone after any
    /// count check, which would make this drop the last one.
    fn retain(self) {
        std::mem::forget(self);
    }
}

impl<T: ?Sized> RetainOnFailure for Box<T> {
    fn retain(self) {
        std::mem::forget(self);
    }
}

impl<T: RetainOnFailure> RetainOnFailure for Option<T> {
    fn retain(self) {
        if let Some(value) = self {
            value.retain();
        }
    }
}

impl<T: RetainOnFailure> RetainOnFailure for Vec<T> {
    fn retain(self) {
        for value in self {
            value.retain();
        }
    }
}

impl RetainOnFailure for LockGrant {
    fn retain(self) {
        std::mem::forget(self);
    }
}

/// One operation's containment of owner code: runs each call, retires each
/// snapshot, and keeps the first failure for the caller to resume or park.
///
/// See the module doc for the points that use it and the rules it keeps.
/// Dropping a scope that still holds a failure retains the failure's payload
/// (ADR-0119): a caller resumes it with [`Self::resume`] or hands it on with
/// [`Self::into_failure`].
#[derive(Default)]
#[must_use = "a scope holds the first failure until it is resumed or handed on"]
pub struct OwnerCalls {
    first: Option<Box<dyn Any + Send>>,
    failed: bool,
    /// Where a failure goes the moment it is caught, for a scope whose
    /// failures belong to a gate's owner (a store's settle).
    parks_in: Option<CommitGate>,
}

impl std::fmt::Debug for OwnerCalls {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OwnerCalls")
            .field("failed", &self.failed())
            .field("parks", &self.parks_in.is_some())
            .finish_non_exhaustive()
    }
}

impl OwnerCalls {
    /// A scope that has caught nothing.
    pub const fn new() -> Self {
        Self {
            first: None,
            failed: false,
            parks_in: None,
        }
    }

    /// A scope that parks each failure in `gate` the moment it is caught, so
    /// it is in the gate before any later owner code (a session that code
    /// opens, whose settle parks there too) can be; without a gate, an
    /// ordinary scope.
    pub fn parking_in(gate: Option<CommitGate>) -> Self {
        Self {
            first: None,
            failed: false,
            parks_in: gate,
        }
    }

    /// The gate this scope parks its failures in ([`Self::parking_in`]): for
    /// a settle scope, the gate that admitted its grant. A store records it
    /// from each settle, so the flush after its grants runs behind the gate
    /// the last of them settled under, though an earlier grant moved the
    /// store ([`Self::run_behind_parked`]).
    #[must_use]
    pub fn parking_gate(&self) -> Option<&CommitGate> {
        self.parks_in.as_ref()
    }

    /// Whether a failure has been caught or taken.
    #[must_use]
    pub fn failed(&self) -> bool {
        self.failed
    }

    /// Keep `payload`, a failure caught elsewhere, behind any earlier one.
    pub fn keep(&mut self, payload: Box<dyn Any + Send>) {
        self.failed = true;
        if let Some(gate) = &self.parks_in {
            gate.defer_failure(payload);
        } else if self.first.is_none() {
            self.first = Some(payload);
        } else {
            flui_foundation::panic::retain_opaque_payload(payload);
        }
    }

    /// Take the failure parked in `gate` (an owner failure a store caught
    /// while settling a grant), behind any failure this scope already holds.
    pub fn take_parked(&mut self, gate: &CommitGate) {
        if let Some(payload) = gate.take_failure() {
            self.keep(payload);
        }
    }

    /// Run `call`, owner code, contained; `None` when it panicked.
    pub fn run<R>(&mut self, call: impl FnOnce() -> R) -> Option<R> {
        match catch_unwind(AssertUnwindSafe(call)) {
            Ok(value) => Some(value),
            Err(payload) => {
                self.keep(payload);
                None
            }
        }
    }

    /// [`Self::run`], for owner code that may request grants of stores
    /// behind `gate`: what their settles parked there during the call is
    /// taken after it, ahead of the call's own panic unless it was parked
    /// during that panic's unwind (see the module doc). A failure the gate
    /// already held stays for its owner's turn.
    pub fn run_parking<R>(&mut self, gate: &CommitGate, call: impl FnOnce() -> R) -> Option<R> {
        let held = gate.holds_failure();
        let unwinding = std::thread::panicking();
        match catch_unwind(AssertUnwindSafe(call)) {
            Ok(value) => {
                if !held {
                    self.take_parked(gate);
                }
                Some(value)
            }
            Err(payload) => {
                let parked = if held { None } else { gate.take_parked() };
                self.keep_in_order(payload, parked, unwinding);
                None
            }
        }
    }

    /// Keep `payload`, the panic of a call that began while the thread was
    /// `unwinding` (or not), and `parked`, what that call left in a gate, in
    /// the order they happened. A failure parked before the call's panic
    /// came first. One parked while the thread was unwinding, by a call that
    /// began outside an unwind, is taken to come from the cleanup of the
    /// call's own panic, and so after it: when a panic began is not
    /// observable from outside it (the panic hook is the process's and the
    /// application's), and the call's panic, which started that cleanup, is
    /// the conservative first. A failure parked during an unwind the call
    /// caught itself and then outlived is ordered the same way. Within an
    /// outer unwind the mark tells nothing, and the parked failure is first.
    fn keep_in_order(
        &mut self,
        payload: Box<dyn Any + Send>,
        parked: Option<Parked>,
        unwinding: bool,
    ) {
        match parked {
            Some(parked) if parked.while_unwinding && !unwinding => {
                self.keep(payload);
                self.keep(parked.payload);
            }
            Some(parked) => {
                self.keep(parked.payload);
                self.keep(payload);
            }
            None => self.keep(payload),
        }
    }

    /// [`Self::run`], for owner code a store runs after grants whose
    /// settle may have parked a failure in `gate` (the gate that admitted
    /// the last of them, which an earlier one may have moved the store to):
    /// when the call panics, that parked failure came first, so it is
    /// taken ahead of the call's own; one the call's unwind parked is kept
    /// behind it (see the module doc). When the call succeeds the parked
    /// failure stays for the gate's owner to report at its turn.
    pub fn run_behind_parked<R>(
        &mut self,
        gate: Option<&CommitGate>,
        call: impl FnOnce() -> R,
    ) -> Option<R> {
        let unwinding = std::thread::panicking();
        match catch_unwind(AssertUnwindSafe(call)) {
            Ok(value) => Some(value),
            Err(payload) => {
                let parked = gate.and_then(CommitGate::take_parked);
                self.keep_in_order(payload, parked, unwinding);
                None
            }
        }
    }

    /// Retire `value`, a snapshot or a withdrawn value whose destruction may
    /// run owner code: dropped, contained, while this scope is healthy;
    /// retained once it has failed or while the thread unwinds.
    pub fn retire<T: RetainOnFailure>(&mut self, value: T) {
        if self.failed() || std::thread::panicking() {
            value.retain();
        } else if let Err(payload) = catch_unwind(AssertUnwindSafe(move || drop(value))) {
            self.keep(payload);
        }
    }

    /// [`Self::retire`], for a value whose destruction may request grants of
    /// stores behind `gate`: what they park there during it is taken, as
    /// for [`Self::run_parking`].
    pub fn retire_parking<T: RetainOnFailure>(&mut self, gate: &CommitGate, value: T) {
        if self.failed() || std::thread::panicking() {
            value.retain();
            return;
        }
        let held = gate.holds_failure();
        match catch_unwind(AssertUnwindSafe(move || drop(value))) {
            Ok(()) => {
                if !held {
                    self.take_parked(gate);
                }
            }
            Err(payload) => {
                let parked = if held { None } else { gate.take_parked() };
                // Retiring begins outside an unwind (checked above).
                self.keep_in_order(payload, parked, false);
            }
        }
    }

    /// The first failure, for a caller that parks or wraps it.
    #[must_use]
    pub fn into_failure(mut self) -> Option<Box<dyn Any + Send>> {
        self.first.take()
    }

    /// Resume the first failure, if any.
    pub fn resume(self) {
        if let Some(payload) = self.into_failure() {
            resume_unwind(payload);
        }
    }
}

impl Drop for OwnerCalls {
    fn drop(&mut self) {
        if let Some(payload) = self.first.take() {
            flui_foundation::panic::retain_opaque_payload(payload);
        }
    }
}
