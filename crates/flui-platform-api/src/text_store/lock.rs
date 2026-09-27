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
//! | inside a frame transaction | [`TextStoreError::SyncLockUnavailable`] | queued, [`LockOutcome::Deferred`] |
//!
//! A grant queued while a lock was held runs as soon as that lock is
//! released; one queued while commits were closed runs at the next
//! [`LockArbiter::run_deferred`] with commits allowed (ADR-0027 §3's commit
//! anchor). Queued grants run in request order, each under its own lock,
//! and a grant that can run now first lets every queued one run ahead of
//! it, so a later request never overtakes an earlier one.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::marker::PhantomData;

use super::session::{TextStoreEdit, TextStoreRead};
use super::utf16::OffsetError;

/// A read lock's body.
pub type ReadBody = Box<dyn FnOnce(&dyn TextStoreRead)>;

/// A read-write lock's body.
pub type EditBody = Box<dyn FnOnce(&mut dyn TextStoreEdit)>;

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
    /// The store accepts no edits.
    #[error("the text store is read-only")]
    ReadOnly,
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

/// The lock state machine every text store embeds.
///
/// It holds the "locked" flag and the deferred queue; the store supplies
/// whether commits are allowed at each call and an `open` function that runs
/// one grant with the right session. Owner-thread only, and not `Send`.
///
/// A panic inside a grant releases the lock: the flag is reset by a guard's
/// `Drop`, which runs no further grants. Queued grants stay queued for the
/// next release or [`Self::run_deferred`].
#[derive(Default)]
pub struct LockArbiter {
    locked: Cell<bool>,
    queue: RefCell<VecDeque<LockGrant>>,
    _owner_thread: PhantomData<*const ()>,
}

impl std::fmt::Debug for LockArbiter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LockArbiter")
            .field("locked", &self.locked.get())
            .field("pending", &self.pending())
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
    /// An unlocked arbiter with nothing queued.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Decide `grant`'s fate: run it through `open` now, queue it, or refuse.
    ///
    /// `may_commit` is whether the store's owner is outside a frame
    /// transaction. See the module doc for the table this follows.
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
        may_commit: bool,
        open: &mut dyn FnMut(LockGrant),
    ) -> Result<LockOutcome, TextStoreError> {
        if self.locked.get() || !may_commit {
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
    /// and commits are allowed. Returns how many ran.
    ///
    /// A grant queued by one of these grants runs in the same call.
    pub fn run_deferred(&self, may_commit: bool, open: &mut dyn FnMut(LockGrant)) -> usize {
        if self.locked.get() || !may_commit {
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

    use flui_types::geometry::{Bounds, Pixels, Point};

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
        fn document_bounds(&self) -> Result<Bounds<Pixels>, TextStoreError> {
            Err(TextStoreError::NoLayout)
        }
        fn index_at_point(
            &self,
            _: Point<Pixels>,
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

    #[test]
    fn a_free_store_grants_at_once() {
        let arbiter = LockArbiter::new();
        let (log, grants) = labelled();
        let outcome = arbiter.request(grants.grant("now"), LockTiming::Sync, true, &mut open);
        assert_eq!(outcome, Ok(LockOutcome::Granted));
        assert_eq!(*log.borrow(), ["now"]);
        assert!(!arbiter.is_locked());
    }

    #[test]
    fn sync_inside_a_session_is_refused() {
        let arbiter = Rc::new(LockArbiter::new());
        let seen = Rc::new(Cell::new(None));
        let (inner_arbiter, inner_seen) = (Rc::clone(&arbiter), Rc::clone(&seen));
        let outer = LockGrant::read(move |_| {
            let (_, grants) = labelled();
            inner_seen.set(Some(inner_arbiter.request(
                grants.grant("nested"),
                LockTiming::Sync,
                true,
                &mut open,
            )));
        });
        assert_eq!(
            arbiter.request(outer, LockTiming::Sync, true, &mut open),
            Ok(LockOutcome::Granted)
        );
        assert_eq!(seen.get(), Some(Err(TextStoreError::SyncLockUnavailable)));
    }

    #[test]
    fn async_inside_a_session_runs_on_release() {
        let arbiter = Rc::new(LockArbiter::new());
        let (log, grants) = labelled();
        let grants = Rc::new(grants);
        let (inner_arbiter, inner_log, inner_grants) =
            (Rc::clone(&arbiter), Rc::clone(&log), Rc::clone(&grants));
        let outer = LockGrant::read(move |_| {
            inner_log.borrow_mut().push("outer");
            let outcome = inner_arbiter.request(
                inner_grants.grant("nested"),
                LockTiming::Async,
                true,
                &mut open,
            );
            assert_eq!(outcome, Ok(LockOutcome::Deferred));
            inner_log.borrow_mut().push("outer ends");
        });
        assert_eq!(
            arbiter.request(outer, LockTiming::Async, true, &mut open),
            Ok(LockOutcome::Granted)
        );
        assert_eq!(*log.borrow(), ["outer", "outer ends", "nested"]);
        assert_eq!(arbiter.pending(), 0);
    }

    #[test]
    fn async_while_commits_closed_waits_for_run_deferred() {
        let arbiter = LockArbiter::new();
        let (log, grants) = labelled();
        assert_eq!(
            arbiter.request(grants.grant("later"), LockTiming::Async, false, &mut open),
            Ok(LockOutcome::Deferred)
        );
        assert_eq!(
            arbiter.request(grants.grant("sync"), LockTiming::Sync, false, &mut open),
            Err(TextStoreError::SyncLockUnavailable)
        );
        assert_eq!(
            arbiter.run_deferred(false, &mut open),
            0,
            "commits still closed"
        );
        assert!(log.borrow().is_empty());
        assert_eq!(arbiter.run_deferred(true, &mut open), 1);
        assert_eq!(*log.borrow(), ["later"]);
    }

    #[test]
    fn deferred_run_in_fifo_order() {
        let arbiter = LockArbiter::new();
        let (log, grants) = labelled();
        for label in ["first", "second", "third"] {
            let _ = arbiter.request(grants.grant(label), LockTiming::Async, false, &mut open);
        }
        // A request made once commits reopen, before the anchor, still runs
        // after the three queued ahead of it.
        assert_eq!(
            arbiter.request(grants.grant("fourth"), LockTiming::Async, true, &mut open),
            Ok(LockOutcome::Granted)
        );
        assert_eq!(*log.borrow(), ["first", "second", "third", "fourth"]);
    }

    #[test]
    fn a_full_queue_refuses_with_deferred_queue_full() {
        let arbiter = LockArbiter::new();
        let (_, grants) = labelled();
        for _ in 0..DEFERRED_LOCK_CAPACITY {
            assert_eq!(
                arbiter.request(grants.grant("q"), LockTiming::Async, false, &mut open),
                Ok(LockOutcome::Deferred)
            );
        }
        assert_eq!(
            arbiter.request(grants.grant("over"), LockTiming::Async, false, &mut open),
            Err(TextStoreError::DeferredQueueFull)
        );
        assert_eq!(arbiter.pending(), DEFERRED_LOCK_CAPACITY);
    }

    #[test]
    fn a_panicking_grant_releases_the_lock() {
        let arbiter = LockArbiter::new();
        let panicking = LockGrant::read(|_| panic!("a grant that fails"));
        let unwound = catch_unwind(AssertUnwindSafe(|| {
            let _ = arbiter.request(panicking, LockTiming::Sync, true, &mut open);
        }));
        assert!(unwound.is_err());
        assert!(!arbiter.is_locked());
        let (log, grants) = labelled();
        assert_eq!(
            arbiter.request(grants.grant("after"), LockTiming::Sync, true, &mut open),
            Ok(LockOutcome::Granted)
        );
        assert_eq!(*log.borrow(), ["after"]);
    }

    #[test]
    fn clear_drops_pending_grants_unrun() {
        let arbiter = LockArbiter::new();
        let (log, grants) = labelled();
        let _ = arbiter.request(grants.grant("a"), LockTiming::Async, false, &mut open);
        let _ = arbiter.request(grants.grant("b"), LockTiming::Async, false, &mut open);
        assert_eq!(arbiter.clear(), 2);
        assert_eq!(arbiter.run_deferred(true, &mut open), 0);
        assert!(log.borrow().is_empty());
    }

    #[test]
    fn a_grant_reports_its_kind() {
        assert_eq!(LockGrant::read(|_| {}).kind(), LockKind::Read);
        assert_eq!(LockGrant::read_write(|_| {}).kind(), LockKind::ReadWrite);
    }

    static_assertions::assert_not_impl_any!(LockArbiter: Send, Sync);
}
