//! The lock state machine a widget's text store embeds: when a grant the
//! platform asked for runs (ADR-0151 §6; the grant, its outcome and the commit
//! gate are `flui_platform::text_store`'s contract).
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
//! hands it ([`TextStore::set_commit_gate`](flui_platform::text_store::TextStore::set_commit_gate))
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

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::marker::PhantomData;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};

use flui_platform::text_store::{
    CommitGate, DEFERRED_LOCK_CAPACITY, LockGrant, LockOutcome, LockTiming, OwnerCalls,
    TextStoreError,
};

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
            return self.defer(grant, timing);
        }
        // Earlier requests first, so this one cannot overtake them. When one
        // of them fails, this request is still accepted work: an
        // asynchronous one waits behind the failed grant's tail, and a
        // synchronous one, which cannot wait, is destroyed unrun under
        // containment behind the earlier failure (ADR-0281 §3).
        let mut earlier = OwnerCalls::new();
        earlier.run(|| self.drain(open, settle));
        if let Some(payload) = earlier.into_failure() {
            let refused = match timing {
                LockTiming::Async => self.enqueue(grant).err(),
                LockTiming::Sync => Some(grant),
            };
            if let Some(grant) = refused {
                crate::containment::drop_contained(grant);
            }
            resume_unwind(payload);
        }
        // An earlier grant may have closed the presentation's transaction gate.
        if !self.may_commit() {
            return self.defer(grant, timing);
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
    ///
    /// A store uses this while publishing an application change: writes
    /// wait in this same bounded queue until the publication completes.
    ///
    /// # Errors
    ///
    /// [`TextStoreError::SyncLockUnavailable`] for a synchronous request,
    /// or [`TextStoreError::DeferredQueueFull`] when the async queue is full.
    pub fn defer(
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

    /// Read a frozen publication snapshot without running queued writes.
    ///
    /// This is the narrow exception to normal request ordering: an observer
    /// must be able to read the old document before its publication, while
    /// writes wait until that publication finishes. The store drains and
    /// settles the normal queue after leaving the publication. Nested reads
    /// and reads behind a closed commit gate are refused as usual.
    ///
    /// # Errors
    ///
    /// [`TextStoreError::SyncLockUnavailable`] while another grant runs or
    /// commits are not allowed.
    pub fn read_snapshot<R>(&self, read: impl FnOnce() -> R) -> Result<R, TextStoreError> {
        if self.locked.get() || !self.may_commit() {
            drop(read);
            return Err(TextStoreError::SyncLockUnavailable);
        }
        self.locked.set(true);
        let _held = Held(&self.locked);
        Ok(read())
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

    /// Attempt every eligible queued grant inside the caller's failure scope.
    ///
    /// A publication uses this before releasing its observer: a failing
    /// grant or settle must not strand a later accepted write. Each grant
    /// is removed before calling other code, so a failure is never retried.
    /// Eligibility is checked between attempts; a closed gate or nested
    /// lock leaves the remaining grants for their next owner anchor.
    /// Ordinary [`Self::run_deferred`] keeps its existing unwind behavior.
    /// Before admitting each grant, `before_grant` runs unlocked so an
    /// application edit made by earlier work is published before a later
    /// platform write can absorb it. It runs only with queued work; lock,
    /// gate and queue eligibility are checked again afterwards.
    ///
    /// Newly parked settle failures join `calls` in order through
    /// [`OwnerCalls::run_parking`]. Returns the number attempted, including
    /// failed bodies or settles, saturating at [`usize::MAX`].
    pub fn run_deferred_contained(
        &self,
        calls: &mut OwnerCalls,
        before_grant: &mut dyn FnMut(&mut OwnerCalls),
        open: &mut dyn FnMut(LockGrant),
        settle: &mut dyn FnMut(&mut OwnerCalls),
    ) -> usize {
        let mut attempted = 0_usize;
        while !self.locked.get() && self.may_commit() && self.pending() > 0 {
            #[expect(
                clippy::disallowed_methods,
                reason = "ADR-0287: a before-grant callback runs contained so pending grants proceed"
            )]
            let prepared = catch_unwind(AssertUnwindSafe(|| before_grant(calls)));
            if let Err(payload) = prepared {
                calls.keep(payload);
            }
            if self.locked.get() || !self.may_commit() {
                break;
            }
            let next = self.queue.borrow_mut().pop_front();
            let Some(grant) = next else {
                break;
            };
            if let Some(admitting) = self.owner_gate() {
                calls.run_parking(&admitting, || self.run_one(grant, open, settle));
                calls.run(|| drop(admitting));
            } else {
                calls.run(|| self.run_one(grant, open, settle));
            }
            attempted = attempted.saturating_add(1);
        }
        attempted
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
    /// after it are still destroyed, each contained (ADR-0281).
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
        #[expect(
            clippy::disallowed_methods,
            reason = "ADR-0287: the store owner's settle runs contained after the grant"
        )]
        let settled = catch_unwind(AssertUnwindSafe(|| settle(&mut settling)));
        if let Err(payload) = settled {
            settling.keep(payload);
        }
        settling.resume();
    }
}

impl Drop for LockArbiter {
    /// Grants still queued when the store goes are its requesters' code:
    /// destroyed each under containment; during an unwind their first failure
    /// is discarded, not resumed (ADR-0281).
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

    use super::*;
    use flui_platform::text_store::{
        Composition, PointMode, RangeRect, Selection, TextStoreRead, Utf16Offset, Utf16Range,
    };

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
}
