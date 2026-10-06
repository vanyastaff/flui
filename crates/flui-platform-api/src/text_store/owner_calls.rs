//! [`OwnerCalls`]: the one containment around code a text store's owner or
//! platform supplies (ADR-0090 amendment, "Owner code").
//!
//! A store, its [`LockArbiter`](super::LockArbiter) and the presentation that
//! attaches it run code they do not control at these points, and every one
//! of them goes through an [`OwnerCalls`]:
//!
//! | Point | Where |
//! |-------|-------|
//! | a grant's session body | `LockArbiter::run_one` (`lock.rs`) |
//! | a store's settle | `LockArbiter::run_one` (`lock.rs`) |
//! | queued grants dropped unrun | `LockArbiter::clear` (`lock.rs`) |
//! | the in-memory owner listener, and its snapshot | `InMemoryTextStore::settle` (`in_memory.rs`) |
//! | a replaced in-memory owner listener or observer | `InMemoryTextStore::set_owner_listener`, `set_observer` (`in_memory.rs`) |
//! | observer notifications, and the observer snapshot | `InMemoryTextStore::flush_notifications` (`in_memory.rs`), `EditableTextStore::notify` (`flui-widgets` `text/text_store.rs`) |
//! | `on_changed`, and its snapshot | `EditObserver::deliver` (`flui-widgets` `text/editable_text.rs`), from `EditableTextStore::settle` and a key edit |
//! | the controller's listeners, and the controller snapshot | `EditableTextStore::settle` (`flui-widgets` `text/text_store.rs`) |
//! | a replaced or detached `EditableText` observer | `EditableTextStore::set_observer`, `detach` (`flui-widgets` `text/text_store.rs`) |
//! | `on_session_start`, the projection, and the dispatched client snapshot | `TextInputOwner::dispatch` (`flui-interaction` `text_input.rs`) |
//! | clients replaced or detached, stores retired at an anchor | `TextInputOwner::attach`, `detach`, `run_deferred_grants` (`flui-interaction` `text_input.rs`) |
//! | a pull host's `focus_store` and `complete_composition` from the owner's queue, the completed store, and the host clone | `TextInputOwner::apply_host_ops` (`flui-interaction` `text_input.rs`), drained by `attach`, `detach`, `complete_composition` and `run_deferred_grants` |
//! | a push completion committed in place, and its store | `TextInputOwner::complete_composition` (`flui-interaction` `text_input.rs`) |
//!
//! Presentation close (`TextInputOwner::close_with_mode` and its `Drop`)
//! keeps its close-mode containment (ADR-0123), which retires the same
//! values under the same retention rule; that includes the host calls a
//! close makes.
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
//! - **A failure parked in a gate during a call came before the call's own
//!   unwind**, which happened after it, so [`Self::run_parking`] and
//!   [`Self::retire_parking`] take the gate before keeping that unwind, and
//!   a scope that reports to a gate takes what was parked there before it
//!   ran anything ([`Self::take_parked`]).
//! - **A snapshot retires inside the scope** ([`Self::retire`]): dropped,
//!   contained, while the scope is healthy; retained once it has failed or
//!   while the thread unwinds (ADR-0127), so a capture whose `Drop` panics
//!   can neither replace the first failure nor panic during an unwind.

use std::any::Any;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::rc::Rc;
use std::sync::Arc;

use super::lock::{CommitGate, LockGrant};

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
}

impl std::fmt::Debug for OwnerCalls {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OwnerCalls")
            .field("failed", &self.failed())
            .finish()
    }
}

impl OwnerCalls {
    /// A scope that has caught nothing.
    pub const fn new() -> Self {
        Self { first: None }
    }

    /// Whether a failure has been caught or taken.
    #[must_use]
    pub fn failed(&self) -> bool {
        self.first.is_some()
    }

    /// Keep `payload`, a failure caught elsewhere, behind any earlier one.
    pub fn keep(&mut self, payload: Box<dyn Any + Send>) {
        if self.first.is_none() {
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
    /// behind `gate`: what their settles parked there is taken after the
    /// call, ahead of the call's own panic.
    pub fn run_parking<R>(&mut self, gate: &CommitGate, call: impl FnOnce() -> R) -> Option<R> {
        let outcome = catch_unwind(AssertUnwindSafe(call));
        self.take_parked(gate);
        match outcome {
            Ok(value) => Some(value),
            Err(payload) => {
                self.keep(payload);
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
    /// stores behind `gate`.
    pub fn retire_parking<T: RetainOnFailure>(&mut self, gate: &CommitGate, value: T) {
        if self.failed() || std::thread::panicking() {
            value.retain();
            return;
        }
        let outcome = catch_unwind(AssertUnwindSafe(move || drop(value)));
        self.take_parked(gate);
        if let Err(payload) = outcome {
            self.keep(payload);
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
