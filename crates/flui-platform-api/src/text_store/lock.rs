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
//! use flui_platform_api::text_store::{LockGrant, Utf16Offset, Utf16Range};
//!
//! let _ = LockGrant::read(|session| {
//!     let _ = session.replace(Utf16Range::collapsed(Utf16Offset::ZERO), "x");
//! });
//! ```
//!
//! The same edit under a read-write grant compiles:
//!
//! ```
//! use flui_platform_api::text_store::{LockGrant, Utf16Offset, Utf16Range};
//!
//! let _ = LockGrant::read_write(|session| {
//!     let _ = session.replace(Utf16Range::collapsed(Utf16Offset::ZERO), "x");
//! });
//! ```
//!
//! [`LockArbiter`] decides when a grant runs, following TSF's
//! `ITextStoreACP::RequestLock`:
//!
//! | The store is… | Sync request | Async request |
//! |---|---|---|
//! | free, commits allowed | runs now | runs now |
//! | inside a lock | [`TextStoreError::SyncLockUnavailable`] (`TS_E_SYNCHRONOUS`) | queued, [`LockOutcome::Deferred`] (`TS_S_ASYNC`) |
//! | inside a frame transaction (its [`CommitGate`] shut) | [`TextStoreError::SyncLockUnavailable`] | queued, [`LockOutcome::Deferred`] |
//!
//! A grant queued while a lock was held runs as soon as that lock is
//! released; one queued while the gate was shut runs at the next
//! [`LockArbiter::run_deferred`] once it is open (ADR-0027 §3's commit
//! anchor). Queued grants run in request order, each under its own lock,
//! and a grant that can run now first lets every queued one run ahead of
//! it, so a later request never overtakes an earlier one.
//!
//! The arbiter reads the gate itself: a store installs the gate its owner
//! hands it ([`TextStore::set_commit_gate`](super::TextStore::set_commit_gate))
//! with [`LockArbiter::set_gate`], and has no flag of its own to forget.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::marker::PhantomData;
use std::rc::Rc;

use super::session::{TextStoreEdit, TextStoreRead};
use super::utf16::OffsetError;

/// A read lock's body. Private: callers build one with [`LockGrant::read`].
type ReadBody = Box<dyn FnOnce(&dyn TextStoreRead)>;

/// A read-write lock's body. Private: callers build one with
/// [`LockGrant::read_write`].
type EditBody = Box<dyn FnOnce(&mut dyn TextStoreEdit)>;

/// A lock's body, which the store runs with the session its kind allows.
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
pub enum LockKind {
    /// Reads only.
    Read,
    /// Reads and edits.
    ReadWrite,
}

/// Whether the caller needs the grant now (`TS_LF_SYNC`) or accepts it later.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
/// store's [`LockArbiter`] reads it on every request. Clones share one state.
/// Owner-thread only, and not `Send`.
#[derive(Clone, Debug, Default)]
pub struct CommitGate {
    shut: Rc<Cell<bool>>,
}

impl CommitGate {
    /// An open gate.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether commits are allowed now.
    #[must_use]
    pub fn is_open(&self) -> bool {
        !self.shut.get()
    }

    /// Open or shut the gate for every store holding a clone of it.
    pub fn set_open(&self, open: bool) {
        self.shut.set(!open);
    }
}

/// The lock state machine every text store embeds.
///
/// It holds the "locked" flag, the deferred queue and the store's
/// [`CommitGate`]; the store supplies an `open` function that runs one grant
/// with the right session. Owner-thread only, and not `Send`.
///
/// A panic inside a grant releases the lock: the flag is reset by a guard's
/// `Drop`, which runs no further grants. Queued grants stay queued for the
/// next release or [`Self::run_deferred`].
#[derive(Default)]
pub struct LockArbiter {
    locked: Cell<bool>,
    queue: RefCell<VecDeque<LockGrant>>,
    gate: RefCell<CommitGate>,
    _owner_thread: PhantomData<*const ()>,
}

impl std::fmt::Debug for LockArbiter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LockArbiter")
            .field("locked", &self.locked.get())
            .field("pending", &self.pending())
            .field("may_commit", &self.may_commit())
            .finish_non_exhaustive()
    }
}

/// Resets the locked flag on every exit from a grant, unwinding included.
struct Held<'a>(&'a Cell<bool>);

impl Drop for Held<'_> {
    fn drop(&mut self) {
        self.0.set(false);
    }
}

impl LockArbiter {
    /// An unlocked arbiter with nothing queued, behind a gate of its own that
    /// stays open until [`Self::set_gate`] replaces it.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Follow `gate` from now on, replacing the one installed before.
    pub fn set_gate(&self, gate: CommitGate) {
        *self.gate.borrow_mut() = gate;
    }

    /// Whether the installed gate is open.
    #[must_use]
    pub fn may_commit(&self) -> bool {
        self.gate.borrow().is_open()
    }

    /// Decide `grant`'s fate: run it through `open` now, queue it, or refuse.
    /// See the module doc for the table this follows.
    ///
    /// # Errors
    ///
    /// [`TextStoreError::SyncLockUnavailable`] for a sync request the store
    /// cannot grant now, and [`TextStoreError::DeferredQueueFull`] for an
    /// async one the queue has no room for.
    pub fn request(
        &self,
        grant: LockGrant,
        timing: LockTiming,
        open: &mut dyn FnMut(LockGrant),
    ) -> Result<LockOutcome, TextStoreError> {
        if self.locked.get() || !self.may_commit() {
            return match timing {
                LockTiming::Sync => Err(TextStoreError::SyncLockUnavailable),
                LockTiming::Async => {
                    let mut queue = self.queue.borrow_mut();
                    if queue.len() >= DEFERRED_LOCK_CAPACITY {
                        return Err(TextStoreError::DeferredQueueFull);
                    }
                    queue.push_back(grant);
                    Ok(LockOutcome::Deferred)
                }
            };
        }
        // Earlier requests first, so this one cannot overtake them.
        self.drain(open);
        self.run_one(grant, open);
        // Whatever this grant queued runs now that its lock is released.
        self.drain(open);
        Ok(LockOutcome::Granted)
    }

    /// Run every queued grant, in request order, when the store is unlocked
    /// and its gate is open. Returns how many ran.
    ///
    /// A grant queued by one of these grants runs in the same call.
    pub fn run_deferred(&self, open: &mut dyn FnMut(LockGrant)) -> usize {
        if self.locked.get() || !self.may_commit() {
            return 0;
        }
        self.drain(open)
    }

    /// Whether a grant is running.
    #[must_use]
    pub fn is_locked(&self) -> bool {
        self.locked.get()
    }

    /// How many grants are queued.
    #[must_use]
    pub fn pending(&self) -> usize {
        self.queue.borrow().len()
    }

    /// Drop every queued grant without running it; returns how many there
    /// were. A detached field calls this so a queued edit never reaches a
    /// buffer that no longer belongs to it.
    pub fn clear(&self) -> usize {
        let dropped = std::mem::take(&mut *self.queue.borrow_mut());
        dropped.len()
    }

    fn drain(&self, open: &mut dyn FnMut(LockGrant)) -> usize {
        let mut ran = 0;
        loop {
            let next = self.queue.borrow_mut().pop_front();
            let Some(grant) = next else {
                return ran;
            };
            self.run_one(grant, open);
            ran += 1;
        }
    }

    fn run_one(&self, grant: LockGrant, open: &mut dyn FnMut(LockGrant)) {
        self.locked.set(true);
        let _held = Held(&self.locked);
        open(grant);
    }
}

#[cfg(test)]
mod tests {
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::rc::Rc;

    use flui_foundation::geometry::{Bounds, Point};

    use super::super::{Composition, PointMode, RangeRect, Selection, Utf16Offset, Utf16Range};
    use super::*;

    /// Makes grants whose only effect is recording a label when they run,
    /// so the log shows the order they ran in.
    struct Labelled {
        log: Rc<RefCell<Vec<&'static str>>>,
    }

    impl Labelled {
        fn grant(&self, label: &'static str) -> LockGrant {
            let log = Rc::clone(&self.log);
            LockGrant::read(move |_| log.borrow_mut().push(label))
        }
    }

    struct Empty;

    impl TextStoreRead for Empty {
        fn document_len(&self) -> Utf16Offset {
            Utf16Offset::ZERO
        }
        fn text(&self, _: Utf16Range) -> Result<String, TextStoreError> {
            Ok(String::new())
        }
        fn selection(&self) -> Selection {
            Selection::collapsed(Utf16Offset::ZERO)
        }
        fn composition(&self) -> Option<Composition> {
            None
        }
        fn rect_for_range(&self, _: Utf16Range) -> Result<RangeRect, TextStoreError> {
            Err(TextStoreError::NoLayout)
        }
        fn document_bounds(&self) -> Result<Bounds<f64>, TextStoreError> {
            Err(TextStoreError::NoLayout)
        }
        fn index_at_point(
            &self,
            _: Point<f64>,
            _: PointMode,
        ) -> Result<Utf16Offset, TextStoreError> {
            Err(TextStoreError::NoLayout)
        }
    }

    fn open(grant: LockGrant) {
        match grant {
            LockGrant::Read(f) => f(&Empty),
            LockGrant::ReadWrite(_) => unreachable!("these tests only take read locks"),
        }
    }

    fn labelled() -> (Rc<RefCell<Vec<&'static str>>>, Labelled) {
        let log = Rc::new(RefCell::new(Vec::new()));
        (
            Rc::clone(&log),
            Labelled {
                log: Rc::clone(&log),
            },
        )
    }

    fn sync_inside_a_session_is_refused() {
        let arbiter = Rc::new(LockArbiter::new());
        let seen = Rc::new(Cell::new(None));
        let (inner_arbiter, inner_seen) = (Rc::clone(&arbiter), Rc::clone(&seen));
        let outer = LockGrant::read(move |_| {
            let (_, grants) = labelled();
            inner_seen.set(Some(inner_arbiter.request(
                grants.grant("nested"),
                LockTiming::Sync,
                &mut open,
            )));
        });
        assert_eq!(
            arbiter.request(outer, LockTiming::Sync, &mut open),
            Ok(LockOutcome::Granted)
        );
        assert_eq!(seen.get(), Some(Err(TextStoreError::SyncLockUnavailable)));
    }

    /// An arbiter following a gate the test holds, shut.
    fn behind_a_shut_gate() -> (LockArbiter, CommitGate) {
        let arbiter = LockArbiter::new();
        let gate = CommitGate::new();
        gate.set_open(false);
        arbiter.set_gate(gate.clone());
        (arbiter, gate)
    }

    fn deferred_run_in_fifo_order() {
        let (arbiter, gate) = behind_a_shut_gate();
        let (log, grants) = labelled();
        for label in ["first", "second", "third"] {
            let _ = arbiter.request(grants.grant(label), LockTiming::Async, &mut open);
        }
        // A request made once the gate reopens, before the anchor, still
        // runs after the three queued ahead of it.
        gate.set_open(true);
        assert_eq!(
            arbiter.request(grants.grant("fourth"), LockTiming::Async, &mut open),
            Ok(LockOutcome::Granted)
        );
        assert_eq!(*log.borrow(), ["first", "second", "third", "fourth"]);
    }

    fn a_full_queue_refuses_with_deferred_queue_full() {
        let (arbiter, _gate) = behind_a_shut_gate();
        let (_, grants) = labelled();
        for _ in 0..DEFERRED_LOCK_CAPACITY {
            assert_eq!(
                arbiter.request(grants.grant("q"), LockTiming::Async, &mut open),
                Ok(LockOutcome::Deferred)
            );
        }
        assert_eq!(
            arbiter.request(grants.grant("over"), LockTiming::Async, &mut open),
            Err(TextStoreError::DeferredQueueFull)
        );
        assert_eq!(arbiter.pending(), DEFERRED_LOCK_CAPACITY);
    }

    /// The arbiter's refusals and ordering: a sync request inside a session,
    /// deferred grants running FIFO, and a full deferred queue.
    #[test]
    fn the_arbiter_refuses_and_orders_requests_by_the_lock_protocol() {
        sync_inside_a_session_is_refused();
        deferred_run_in_fifo_order();
        a_full_queue_refuses_with_deferred_queue_full();
    }

    #[test]
    fn a_panicking_grant_releases_the_lock() {
        let arbiter = LockArbiter::new();
        let panicking = LockGrant::read(|_| panic!("a grant that fails"));
        let unwound = catch_unwind(AssertUnwindSafe(|| {
            let _ = arbiter.request(panicking, LockTiming::Sync, &mut open);
        }));
        assert!(unwound.is_err());
        assert!(!arbiter.is_locked());
        let (log, grants) = labelled();
        assert_eq!(
            arbiter.request(grants.grant("after"), LockTiming::Sync, &mut open),
            Ok(LockOutcome::Granted)
        );
        assert_eq!(*log.borrow(), ["after"]);
    }

    static_assertions::assert_not_impl_any!(LockArbiter: Send, Sync);
    static_assertions::assert_not_impl_any!(CommitGate: Send, Sync);
}
