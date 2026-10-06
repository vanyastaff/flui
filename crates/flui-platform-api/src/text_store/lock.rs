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
//!
//! # Settling
//!
//! A grant runs the platform's code, never the field owner's: a store that
//! changed its committed text inside a grant owes its owner a notification
//! (a widget's `on_changed`), and owner code may ask for a lock of its own.
//! So every [`LockArbiter::request`] and [`LockArbiter::run_deferred`] takes
//! a second function, `settle`, which the arbiter calls after each grant has
//! released the lock and before the next queued grant runs. The store
//! delivers what it owes there: the lock is free, so a synchronous request
//! from the owner is granted (after any grant queued ahead of it), and the
//! platform's next grant sees whatever the owner changed.
//!
//! A panic in `settle` does not undo the grant before it, which already
//! ran. The arbiter catches it and hands the payload to the store's
//! [`CommitGate`] ([`CommitGate::defer_failure`]), whose owner reports it at
//! its next turn, and the queue keeps running. A store whose owner never
//! installed a gate has no one to report to, so there the panic resumes
//! out of the request once the lock is released.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::marker::PhantomData;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::rc::Rc;

use super::owner_calls::{OwnerCalls, RetainOnFailure};
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
///
/// The gate is also the stores' way back to their owner when owner code
/// fails outside any frame: a panic caught while a store settled a grant
/// waits here ([`Self::defer_failure`]) until the owner takes it
/// ([`Self::take_failure`]) and reports it.
#[derive(Clone, Default)]
pub struct CommitGate {
    shut: Rc<Cell<bool>>,
    failure: Rc<ParkedFailure>,
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

    /// Whether commits are allowed now.
    #[must_use]
    pub fn is_open(&self) -> bool {
        !self.shut.get()
    }

    /// Open or shut the gate for every store holding a clone of it.
    pub fn set_open(&self, open: bool) {
        self.shut.set(!open);
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
            flui_foundation::panic::retain_opaque_payload(later);
        }
    }

    /// Whether a failure waits here.
    pub(crate) fn holds_failure(&self) -> bool {
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
            flui_foundation::panic::retain_opaque_payload(parked.payload);
        }
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
    /// Whether an owner installed the gate ([`Self::set_gate`]), so a
    /// failure caught while settling has someone to be reported to.
    owned_gate: Cell<bool>,
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
        // The replaced gate goes once the borrow is released; a failure still
        // parked in it is retained with it (`ParkedFailure`).
        let replaced = std::mem::replace(&mut *self.gate.borrow_mut(), gate);
        self.owned_gate.set(true);
        drop(replaced);
    }

    /// The gate an owner installed ([`Self::set_gate`]), where a failure
    /// caught while settling waits; `None` with the arbiter's own gate. A
    /// store reads it before owner code runs, which may install another.
    #[must_use]
    pub fn owner_gate(&self) -> Option<CommitGate> {
        self.owned_gate.get().then(|| self.gate.borrow().clone())
    }

    /// Whether the installed gate is open.
    #[must_use]
    pub fn may_commit(&self) -> bool {
        self.gate.borrow().is_open()
    }

    /// Decide `grant`'s fate: run it through `open` now, queue it, or refuse.
    /// See the module doc for the table this follows. `settle` runs after
    /// each grant this call runs, once its lock is released and before the
    /// next one (module doc, "Settling").
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
        settle: &mut dyn FnMut(&mut OwnerCalls),
    ) -> Result<LockOutcome, TextStoreError> {
        if self.locked.get() || !self.may_commit() {
            return self.defer_or_refuse(grant, timing);
        }
        // Earlier requests first, so this one cannot overtake them. When one
        // of them fails, this request is still accepted work: an
        // asynchronous one waits behind the failed grant's tail, and a
        // synchronous one, which cannot wait, is retained unrun (ADR-0127);
        // neither is destroyed during the unwind.
        let mut earlier = OwnerCalls::new();
        earlier.run(|| self.drain(open, settle));
        if let Some(payload) = earlier.into_failure() {
            let refused = match timing {
                LockTiming::Async => self.enqueue(grant).err(),
                LockTiming::Sync => Some(grant),
            };
            if let Some(grant) = refused {
                RetainOnFailure::retain(grant);
            }
            resume_unwind(payload);
        }
        // An earlier grant may have closed the presentation's transaction gate.
        if !self.may_commit() {
            return self.defer_or_refuse(grant, timing);
        }
        self.run_one(grant, open, settle);
        // Whatever this grant queued runs now that its lock is released.
        self.drain(open, settle);
        Ok(LockOutcome::Granted)
    }

    /// Queue `grant`, or hand it back when the queue is full.
    fn enqueue(&self, grant: LockGrant) -> Result<(), LockGrant> {
        let mut queue = self.queue.borrow_mut();
        if queue.len() >= DEFERRED_LOCK_CAPACITY {
            return Err(grant);
        }
        queue.push_back(grant);
        Ok(())
    }

    /// Queue `grant` or refuse it. A refused grant is retired inside a
    /// scope, after the queue's borrow is released: its captures are the
    /// requester's code.
    fn defer_or_refuse(
        &self,
        grant: LockGrant,
        timing: LockTiming,
    ) -> Result<LockOutcome, TextStoreError> {
        let (refused, outcome) = match timing {
            LockTiming::Sync => (Some(grant), Err(TextStoreError::SyncLockUnavailable)),
            LockTiming::Async => match self.enqueue(grant) {
                Ok(()) => (None, Ok(LockOutcome::Deferred)),
                Err(grant) => (Some(grant), Err(TextStoreError::DeferredQueueFull)),
            },
        };
        let mut calls = OwnerCalls::new();
        calls.retire(refused);
        calls.resume();
        outcome
    }

    /// Run every queued grant, in request order, when the store is unlocked
    /// and its gate is open. Returns how many ran.
    ///
    /// A grant queued by one of these grants runs in the same call, and
    /// `settle` runs after each, as for [`Self::request`].
    pub fn run_deferred(
        &self,
        open: &mut dyn FnMut(LockGrant),
        settle: &mut dyn FnMut(&mut OwnerCalls),
    ) -> usize {
        if self.locked.get() || !self.may_commit() {
            return 0;
        }
        self.drain(open, settle)
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
    ///
    /// # Panics
    ///
    /// Resumes the first panic a dropped grant's captures raised; the grants
    /// after it are retained (ADR-0127).
    pub fn clear(&self) -> usize {
        let dropped = std::mem::take(&mut *self.queue.borrow_mut());
        let count = dropped.len();
        let mut calls = OwnerCalls::new();
        for grant in dropped {
            calls.retire(grant);
        }
        calls.resume();
        count
    }

    fn drain(
        &self,
        open: &mut dyn FnMut(LockGrant),
        settle: &mut dyn FnMut(&mut OwnerCalls),
    ) -> usize {
        let mut ran = 0;
        loop {
            if !self.may_commit() {
                return ran;
            }
            let next = self.queue.borrow_mut().pop_front();
            let Some(grant) = next else {
                return ran;
            };
            self.run_one(grant, open, settle);
            ran += 1;
        }
    }

    fn run_one(
        &self,
        grant: LockGrant,
        open: &mut dyn FnMut(LockGrant),
        settle: &mut dyn FnMut(&mut OwnerCalls),
    ) {
        // A failure in this grant's settle belongs to the presentation that
        // admitted the grant: read before any owner code, the grant's own
        // included, can move the store to another presentation.
        let admitting = self.owner_gate();
        let mut calls = OwnerCalls::new();
        {
            self.locked.set(true);
            let _held = Held(&self.locked);
            calls.run(|| open(grant));
        }
        if calls.failed() {
            // A grant's own panic reaches whoever requested it.
            calls.resume();
            return;
        }
        // The grant ran and its lock is released: the store's owner runs now,
        // before the next grant. A failure there cannot undo the grant; it
        // goes to the admitting gate the moment it is caught, ahead of any
        // session the owner's code opens after it, or, with no owner gate,
        // resumes once the settle is done.
        let mut settling = OwnerCalls::parking_in(admitting);
        let settled = catch_unwind(AssertUnwindSafe(|| settle(&mut settling)));
        if let Err(payload) = settled {
            settling.keep(payload);
        }
        settling.resume();
    }
}

impl Drop for LockArbiter {
    /// Grants still queued when the store goes are its requesters' code:
    /// retired inside a scope, retained during an unwind (ADR-0127).
    fn drop(&mut self) {
        let queued = std::mem::take(self.queue.get_mut());
        let mut calls = OwnerCalls::new();
        for grant in queued {
            calls.retire(grant);
        }
        if !std::thread::panicking() {
            calls.resume();
        }
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
                &mut |_: &mut OwnerCalls| {},
            )));
        });
        assert_eq!(
            arbiter.request(
                outer,
                LockTiming::Sync,
                &mut open,
                &mut |_: &mut OwnerCalls| {}
            ),
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
            let _ = arbiter.request(
                grants.grant(label),
                LockTiming::Async,
                &mut open,
                &mut |_: &mut OwnerCalls| {},
            );
        }
        // A request made once the gate reopens, before the anchor, still
        // runs after the three queued ahead of it.
        gate.set_open(true);
        assert_eq!(
            arbiter.request(
                grants.grant("fourth"),
                LockTiming::Async,
                &mut open,
                &mut |_: &mut OwnerCalls| {}
            ),
            Ok(LockOutcome::Granted)
        );
        assert_eq!(*log.borrow(), ["first", "second", "third", "fourth"]);
    }

    fn a_full_queue_refuses_with_deferred_queue_full() {
        let (arbiter, _gate) = behind_a_shut_gate();
        let (_, grants) = labelled();
        for _ in 0..DEFERRED_LOCK_CAPACITY {
            assert_eq!(
                arbiter.request(
                    grants.grant("q"),
                    LockTiming::Async,
                    &mut open,
                    &mut |_: &mut OwnerCalls| {}
                ),
                Ok(LockOutcome::Deferred)
            );
        }
        assert_eq!(
            arbiter.request(
                grants.grant("over"),
                LockTiming::Async,
                &mut open,
                &mut |_: &mut OwnerCalls| {}
            ),
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
            let _ = arbiter.request(
                panicking,
                LockTiming::Sync,
                &mut open,
                &mut |_: &mut OwnerCalls| {},
            );
        }));
        assert!(unwound.is_err());
        assert!(!arbiter.is_locked());
        let (log, grants) = labelled();
        assert_eq!(
            arbiter.request(
                grants.grant("after"),
                LockTiming::Sync,
                &mut open,
                &mut |_: &mut OwnerCalls| {}
            ),
            Ok(LockOutcome::Granted)
        );
        assert_eq!(*log.borrow(), ["after"]);
    }

    static_assertions::assert_not_impl_any!(LockArbiter: Send, Sync);
    static_assertions::assert_not_impl_any!(CommitGate: Send, Sync);
}
