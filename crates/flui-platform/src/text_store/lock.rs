//! Document locks: the grant a platform hands over, and the one state
//! machine every store uses to decide when it runs.
//!
//! A lock is a closure. The platform asks for one with
//! [`TextStore::request_lock`](super::TextStore::request_lock), and the
//! store runs it with the session the grant's kind allows: a
//! [`LockGrant::Read`] closure receives `&dyn TextStoreRead`, so an edit
//! inside a read lock does not compile.
//!
//! ```compile_fail
//! use flui_platform::text_store::{LockGrant, Utf16Offset, Utf16Range};
//!
//! let _ = LockGrant::read(|session| {
//!     let _ = session.replace(Utf16Range::collapsed(Utf16Offset::ZERO), "x");
//! });
//! ```
//!
//! The same edit under a read-write grant compiles:
//!
//! ```
//! use flui_platform::text_store::{LockGrant, Utf16Offset, Utf16Range};
//!
//! let _ = LockGrant::read_write(|session| {
//!     let _ = session.replace(Utf16Range::collapsed(Utf16Offset::ZERO), "x");
//! });
//! ```
//!
//! When a grant runs follows TSF's `ITextStoreACP::RequestLock`: a
//! synchronous request inside a lock or behind a shut [`CommitGate`] is
//! refused ([`TextStoreError::SyncLockUnavailable`]), an asynchronous one is
//! queued ([`LockOutcome::Deferred`]) and runs in request order once the lock
//! is released or the gate opens. A widget's store embeds
//! `flui_interaction::text_input::LockArbiter`, the state machine that keeps
//! that order (ADR-0151 §6).

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use super::session::{TextStoreEdit, TextStoreRead};
use super::utf16::OffsetError;

/// A read lock's body. Private: callers build one with [`LockGrant::read`].
type ReadBody = Box<dyn FnOnce(&dyn TextStoreRead)>;

/// A read-write lock's body. Private: callers build one with
/// [`LockGrant::read_write`].
type EditBody = Box<dyn FnOnce(&mut dyn TextStoreEdit)>;

/// A lock's body, which the store runs with the session its kind allows.
#[allow(
    clippy::exhaustive_enums,
    reason = "a lock body for each of TSF's two lock kinds"
)]
pub enum LockGrant {
    /// Reads only.
    Read(ReadBody),
    /// Reads and edits.
    ReadWrite(EditBody),
}

impl LockGrant {
    /// A read lock running `f`.
    pub fn read(f: impl FnOnce(&dyn TextStoreRead) + 'static) -> Self {
        Self::Read(Box::new(f))
    }

    /// A read-write lock running `f`.
    pub fn read_write(f: impl FnOnce(&mut dyn TextStoreEdit) + 'static) -> Self {
        Self::ReadWrite(Box::new(f))
    }

    /// Which kind of lock this is.
    #[must_use]
    pub fn kind(&self) -> LockKind {
        match self {
            Self::Read(_) => LockKind::Read,
            Self::ReadWrite(_) => LockKind::ReadWrite,
        }
    }
}

impl std::fmt::Debug for LockGrant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("LockGrant").field(&self.kind()).finish()
    }
}

/// A lock's kind, TSF's `TS_LF_READ` or `TS_LF_READWRITE`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(
    clippy::exhaustive_enums,
    reason = "TSF's two lock kinds, `TS_LF_READ` and `TS_LF_READWRITE`"
)]
pub enum LockKind {
    /// Reads only.
    Read,
    /// Reads and edits.
    ReadWrite,
}

/// Whether the caller needs the grant now (`TS_LF_SYNC`) or accepts it later.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(clippy::exhaustive_enums, reason = "TSF's `TS_LF_SYNC` is set or not")]
pub enum LockTiming {
    /// Now or never: refused with [`TextStoreError::SyncLockUnavailable`]
    /// when the store cannot grant it at once.
    Sync,
    /// Now if possible, otherwise queued.
    Async,
}

/// What happened to a lock request that was not refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[must_use]
#[allow(
    clippy::exhaustive_enums,
    reason = "a lock runs before its request returns or is queued (TSF's `TS_S_ASYNC`)"
)]
pub enum LockOutcome {
    /// The grant ran before the request returned.
    Granted,
    /// The grant is queued and runs at a later anchor (`TS_S_ASYNC`).
    Deferred,
}

/// How many grants a store queues before refusing with
/// [`TextStoreError::DeferredQueueFull`].
pub const DEFERRED_LOCK_CAPACITY: usize = 16;

/// Why a store refused a request or an operation inside a session.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum TextStoreError {
    /// A synchronous lock was asked for while the store could not grant one
    /// (`TS_E_SYNCHRONOUS`).
    #[error("a synchronous lock is unavailable now")]
    SyncLockUnavailable,
    /// [`DEFERRED_LOCK_CAPACITY`] grants are already queued.
    #[error("the deferred-lock queue is full")]
    DeferredQueueFull,
    /// An offset or range does not name a position in the document.
    #[error(transparent)]
    Offset(#[from] OffsetError),
    /// The store's text must not be read out.
    #[error("the text store is protected")]
    Protected,
    /// The field has no layout for its current text.
    #[error("the text store has no layout")]
    NoLayout,
    /// The point misses the text.
    #[error("the point is outside the text")]
    PointOutside,
    /// The field this store belonged to is gone.
    #[error("the text store is detached from its field")]
    Detached,
}

/// Whether the stores behind it may commit: shut for the length of their
/// presentation's frame transaction (ADR-0027 §3), open otherwise.
///
/// One gate per presentation. Its `TextInputOwner` shuts and opens it and
/// installs it into every store it attaches
/// ([`TextStore::set_commit_gate`](super::TextStore::set_commit_gate)); the
/// store's lock arbiter reads it on every request. Clones share one state.
/// Owner-thread only, and not `Send`.
///
/// The gate is also the stores' way back to their owner when owner code
/// fails outside any frame: a panic caught while a store settled a grant
/// waits here ([`Self::defer_failure`]) until the owner takes it
/// ([`Self::take_failure`]) and reports it.
///
/// While its UI runtime is checked out of its owner's slot
/// ([`Self::set_checked_out`]), the gate is open only inside its owner's own
/// calls ([`Self::owner_entry`]): a platform entry that reaches the store then
/// (a nested message loop, a text service called back from inside a handler)
/// is refused a synchronous lock and queues an asynchronous one, and no owner
/// code runs (ADR-0142 item 6). A text service the owner itself called (its
/// composition ended for a press) runs inside the owner's call and is served.
#[derive(Clone, Default)]
pub struct CommitGate {
    shut: Rc<Cell<bool>>,
    checked_out: Rc<Cell<bool>>,
    owner_depth: Rc<Cell<u32>>,
    failure: Rc<ParkedFailure>,
}

/// Leaves an owner call when dropped, unwinding included.
struct OwnerEntry<'a>(&'a Cell<u32>);

impl Drop for OwnerEntry<'_> {
    fn drop(&mut self) {
        self.0.set(self.0.get() - 1);
    }
}

impl std::fmt::Debug for CommitGate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CommitGate")
            .field("open", &self.is_open())
            .field("failed", &self.failure.0.borrow().is_some())
            .finish_non_exhaustive()
    }
}

impl CommitGate {
    /// An open gate.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether commits are allowed now: the gate is not shut, and its UI
    /// runtime is in its slot or the caller is inside an owner call.
    #[must_use]
    pub fn is_open(&self) -> bool {
        !self.shut.get() && (!self.checked_out.get() || self.owner_depth.get() > 0)
    }

    /// Open or shut the gate for every store holding a clone of it.
    pub fn set_open(&self, open: bool) {
        self.shut.set(!open);
    }

    /// Whether the gate is shut ([`Self::set_open`]), whatever the
    /// checkout: a frame transaction is open.
    #[must_use]
    pub fn is_shut(&self) -> bool {
        self.shut.get()
    }

    /// Record whether the gate's UI runtime is checked out of its owner's
    /// slot: while it is, only [`Self::owner_entry`] opens the gate.
    pub fn set_checked_out(&self, checked_out: bool) {
        self.checked_out.set(checked_out);
    }

    /// Run `f` as the owner's own call: the gate admits what it reaches while
    /// the UI runtime is checked out, a text service the owner called
    /// included. Nested calls nest.
    pub fn owner_entry<R>(&self, f: impl FnOnce() -> R) -> R {
        let depth = self
            .owner_depth
            .get()
            .checked_add(1)
            .expect("BUG: owner calls nest fewer than u32::MAX deep");
        self.owner_depth.set(depth);
        let _entry = OwnerEntry(&self.owner_depth);
        f()
    }

    /// Hold `payload`, a panic caught after a grant had already run, for
    /// the owner to report. The first failure is kept until it is taken;
    /// a later one is retained unreported (ADR-0127): it is never dropped
    /// here, since dropping an opaque payload can run user code.
    ///
    /// The gate records whether the thread was unwinding when the failure
    /// was parked: one parked by the cleanup of a panic is ordered behind
    /// that panic ([`OwnerCalls`](super::OwnerCalls)).
    pub fn defer_failure(&self, payload: Box<dyn Any + Send>) {
        let later = {
            let mut held = self.failure.0.borrow_mut();
            if held.is_some() {
                Some(payload)
            } else {
                *held = Some(Parked {
                    payload,
                    while_unwinding: std::thread::panicking(),
                });
                None
            }
        };
        if let Some(later) = later {
            drop(later);
        }
    }

    /// Whether a failure waits here for its owner's next turn.
    #[must_use]
    pub fn holds_failure(&self) -> bool {
        self.failure.0.borrow().is_some()
    }

    /// The failure held since the last call, if any; the owner reports it
    /// (resumes it inside its own containment).
    #[must_use]
    pub fn take_failure(&self) -> Option<Box<dyn Any + Send>> {
        self.take_parked().map(|parked| parked.payload)
    }

    /// [`Self::take_failure`], with whether the thread was unwinding when it
    /// was parked.
    pub(crate) fn take_parked(&self) -> Option<Parked> {
        self.failure.0.borrow_mut().take()
    }
}

/// A failure a gate holds, and whether the thread was unwinding when it was
/// parked (by the cleanup of a panic, a guard's `Drop` that requested a
/// grant whose settle failed).
pub(crate) struct Parked {
    pub(crate) payload: Box<dyn Any + Send>,
    pub(crate) while_unwinding: bool,
}

/// The failure a gate holds for its owner. A payload no owner took before
/// the last clone of the gate went (the presentation closed first) is
/// retained, not dropped (ADR-0119, ADR-0127): its destructor is user code.
#[derive(Default)]
struct ParkedFailure(RefCell<Option<Parked>>);

impl Drop for ParkedFailure {
    fn drop(&mut self) {
        if let Some(parked) = self.0.get_mut().take() {
            drop(parked.payload);
        }
    }
}

#[cfg(test)]
static_assertions::assert_not_impl_any!(CommitGate: Send, Sync);
