//! Holding a presentation open while work that must not be lost finishes.
//!
//! A [`CloseGuard`] belongs to one presentation and is acquired with
//! [`LifecycleContext::close_guard`](crate::LifecycleContext::close_guard) in
//! a lifecycle hook. While a widget keeps a [`CloseHold`] from it, a close the
//! platform lets the application refuse (see [`CloseGuard::can_veto`]) is not
//! carried out; it is recorded as a [`PendingClose`] instead, one per
//! [`CloseReason`], and nothing is torn down. When the last hold is released
//! the recorded close is carried out once, without asking the application's
//! close-request handler again, unless it was resolved with
//! [`PendingClose::stay_open`].
//!
//! Two kinds of hold differ in what they refuse:
//!
//! - [`CloseGuard::hold`] covers work that will finish by itself, such as a
//!   write in flight. It refuses [`CloseReason::User`] and
//!   [`CloseReason::Program`], but lets [`CloseReason::SessionEnd`] through:
//!   the framework writes the latest published bytes while the session ends.
//! - [`CloseGuard::require_decision`] covers a state only the user can
//!   resolve, such as a failed write with unsaved edits. It refuses every
//!   reason the platform lets the application refuse. The user resolves it
//!   with [`PendingClose::discard_and_close`] or [`PendingClose::stay_open`],
//!   or by fixing the cause: a retry that succeeds releases the hold, and the
//!   recorded close is then carried out.
//!
//! The guard is the presentation's own owner-thread state, not a copy: every
//! clone, hold, pending close and [`CloseChanged`] future of one presentation
//! shares it, and none of them leaves the owner thread. A change never wakes
//! anything where it happens; it signals the owner, and waiters wake on the
//! owner's turn.
//!
//! Not yet wired: no host hands a guard to its presentations' widgets yet.

use std::cell::RefCell;
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll, Waker};

use crate::owner_notify::{LifecycleEvent, OwnerNotify};

/// Why a presentation is being closed.
///
/// `#[non_exhaustive]`: a platform with another kind of close adds a variant.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CloseReason {
    /// The user asked: the window's close button, a keyboard shortcut, the
    /// window manager.
    User,
    /// The application asked, by quitting through its own code.
    Program,
    /// The operating system is ending the user's session: log off, restart,
    /// shut down.
    SessionEnd,
}

/// Which of the two kinds of hold a [`CloseHold`] is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HoldKind {
    /// From [`CloseGuard::hold`]: work that finishes by itself.
    Work,
    /// From [`CloseGuard::require_decision`]: a state only the user resolves.
    Decision,
}

/// One recorded close.
#[derive(Clone, Copy, Debug)]
struct Record {
    reason: CloseReason,
    /// Which recording of `reason` this is; never reused.
    epoch: u64,
}

/// One presentation's close state, shared by its guard, holds, pending
/// closes and change futures.
///
/// Every transition commits its change here and releases the `RefCell`
/// borrow before it calls [`OwnerNotify`]: a host may settle the guard
/// synchronously from inside the signal, and must find the state committed
/// and unborrowed.
struct GuardState {
    /// The reasons the presentation's platform lets the application refuse.
    vetoable: Vec<CloseReason>,
    /// How a transition signals the owner.
    notify: OwnerNotify,
    /// Live holds from [`CloseGuard::hold`].
    work_holds: usize,
    /// Live holds from [`CloseGuard::require_decision`].
    decision_holds: usize,
    /// The recorded closes, earliest first, at most one per reason.
    records: Vec<Record>,
    /// The epoch the next record gets.
    next_epoch: u64,
    /// The close due to be carried out, until the owner settles it.
    due: Option<CloseReason>,
    /// How many changes to the recorded closes were committed.
    changes: u64,
    /// How many of them the owner has settled: waiters taken before this
    /// count are resolved.
    settled: u64,
    /// The wakers of pending [`CloseChanged`] futures, each with the change
    /// count it was taken at.
    waiters: Vec<(u64, Waker)>,
}

impl GuardState {
    /// Whether a live hold refuses a close for `reason`.
    fn held_against(&self, reason: CloseReason) -> bool {
        match reason {
            CloseReason::User | CloseReason::Program => {
                self.work_holds > 0 || self.decision_holds > 0
            }
            CloseReason::SessionEnd => self.decision_holds > 0,
        }
    }

    fn position(&self, reason: CloseReason, epoch: u64) -> Option<usize> {
        self.records
            .iter()
            .position(|record| record.reason == reason && record.epoch == epoch)
    }

    fn changed(&mut self) {
        self.changes = self
            .changes
            .checked_add(1)
            .expect("BUG: a close guard counted 2^64 changes");
    }

    /// Make the presentation's close due as `reason`: every record is
    /// consumed, since one close carries them all out. A close already due
    /// and not yet settled stays the one carried out.
    fn make_due(&mut self, reason: CloseReason) {
        self.records.clear();
        self.due.get_or_insert(reason);
        self.changed();
    }
}

/// Commit a transition to `state`, then signal the owner with the event it
/// returns, after the borrow is released. A signal the owner cannot receive
/// loses nothing: the transition stays committed for the next settle.
fn transition(
    state: &RefCell<GuardState>,
    commit: impl FnOnce(&mut GuardState) -> Option<LifecycleEvent>,
) {
    let signal = {
        let mut state = state.borrow_mut();
        commit(&mut state).map(|event| (event, state.notify.clone()))
    };
    if let Some((event, notify)) = signal {
        let _undelivered = notify.notify(event);
    }
}

/// A presentation's capability to hold its close while work finishes.
///
/// Cheap to clone; every clone is the same guard. Owner-thread only
/// (`!Send`): a hold is taken and released where the work's completion is
/// observed, on the owner. Acquire it in `ViewState::init_state` or
/// `did_change_dependencies` through
/// [`LifecycleContext::close_guard`](crate::LifecycleContext::close_guard).
#[derive(Clone)]
pub struct CloseGuard {
    state: Rc<RefCell<GuardState>>,
}

impl fmt::Debug for CloseGuard {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CloseGuard").finish_non_exhaustive()
    }
}

impl CloseGuard {
    /// Hold the close while work that will finish by itself runs: a close
    /// for [`CloseReason::User`] or [`CloseReason::Program`] is recorded as a
    /// [`PendingClose`] and carried out when the last hold is released.
    /// [`CloseReason::SessionEnd`] is not held.
    pub fn hold(&self) -> CloseHold {
        self.take(HoldKind::Work)
    }

    /// Hold the close until the user decides: every reason the platform lets
    /// the application refuse is recorded as a [`PendingClose`]. Releasing
    /// this hold, as a retry that succeeded does, carries the recorded close
    /// out unless it was resolved with [`PendingClose::stay_open`].
    pub fn require_decision(&self) -> CloseHold {
        self.take(HoldKind::Decision)
    }

    fn take(&self, kind: HoldKind) -> CloseHold {
        {
            let mut state = self.state.borrow_mut();
            let count = match kind {
                HoldKind::Work => &mut state.work_holds,
                HoldKind::Decision => &mut state.decision_holds,
            };
            *count = count
                .checked_add(1)
                .expect("BUG: more close holds than addressable memory");
        }
        CloseHold {
            state: Rc::clone(&self.state),
            kind,
        }
    }

    /// The earliest close recorded and not yet carried out, withdrawn or
    /// resolved, if any. At most one is recorded per [`CloseReason`]; a
    /// repeated request for the same reason records nothing more.
    #[must_use]
    pub fn pending(&self) -> Option<PendingClose> {
        let record = *self.state.borrow().records.first()?;
        Some(PendingClose {
            reason: record.reason,
            epoch: record.epoch,
            state: Rc::clone(&self.state),
        })
    }

    /// A future that resolves at the first change to what
    /// [`pending`](Self::pending) answers after it was taken: a close
    /// recorded, withdrawn, resolved or carried out. It wakes on the owner's
    /// turn, never inside the change.
    ///
    /// Take the future first, then read [`pending`](Self::pending): a change
    /// between the two then still resolves it. Read in the other order, a
    /// change after the read and before the future was taken is missed.
    pub fn changed(&self) -> CloseChanged {
        let taken_at = self.state.borrow().changes;
        CloseChanged {
            state: Rc::clone(&self.state),
            taken_at,
        }
    }

    /// Whether the platform lets the application refuse a close for
    /// `reason`. Where it does not (a session that is already ending, the
    /// web, Android, iOS), holds are not consulted and only the framework's
    /// flush writes what was published.
    #[must_use]
    pub fn can_veto(&self, reason: CloseReason) -> bool {
        self.state.borrow().vetoable.contains(&reason)
    }
}

/// A hold on a presentation's close, from [`CloseGuard::hold`] or
/// [`CloseGuard::require_decision`]. Dropping it releases the hold; if it was
/// the last, a recorded close becomes due. Owner-thread only.
///
/// Releasing it commits the release to the guard's state and drops the
/// state's borrow before the owner is signalled, so a host that settles
/// synchronously from inside the signal finds the release already
/// recorded.
#[must_use = "the hold is released as soon as it is dropped"]
pub struct CloseHold {
    state: Rc<RefCell<GuardState>>,
    kind: HoldKind,
}

impl fmt::Debug for CloseHold {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CloseHold")
            .field("kind", &self.kind)
            .finish_non_exhaustive()
    }
}

impl Drop for CloseHold {
    fn drop(&mut self) {
        let kind = self.kind;
        transition(&self.state, |state| {
            // Taking a hold counted it, so the count is at least one.
            match kind {
                HoldKind::Work => state.work_holds -= 1,
                HoldKind::Decision => state.decision_holds -= 1,
            }
            let held = state.work_holds > 0 || state.decision_holds > 0;
            let first = state.records.first().map(|record| record.reason);
            match (held, first) {
                (false, Some(reason)) => {
                    state.make_due(reason);
                    Some(LifecycleEvent::CloseDue)
                }
                _ => None,
            }
        });
    }
}

/// A close a hold refused and that is waiting, from [`CloseGuard::pending`].
///
/// It names one recording of the close: once that recording is carried out,
/// withdrawn or resolved, this value is stale, even if the same reason is
/// recorded again later.
pub struct PendingClose {
    reason: CloseReason,
    /// Which recording of `reason` this is.
    epoch: u64,
    state: Rc<RefCell<GuardState>>,
}

impl fmt::Debug for PendingClose {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PendingClose")
            .field("reason", &self.reason)
            .finish_non_exhaustive()
    }
}

/// The recorded close a [`PendingClose`] named is gone: carried out,
/// withdrawn or already resolved.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("the recorded close was already carried out, withdrawn or resolved")]
pub struct StaleClose;

impl PendingClose {
    /// Why the close was requested.
    #[must_use]
    pub fn reason(&self) -> CloseReason {
        self.reason
    }

    /// Close the presentation now, past every hold, discarding what the
    /// holds protected. The decision of the presentation's owner, for
    /// example after the user chose "Close without saving".
    ///
    /// On success the close becomes due: the owner is signalled
    /// ([`LifecycleEvent::CloseDue`]),
    /// its next [`CloseGuardSource::settle`] returns this reason, and the
    /// owner carries the close out. Closes recorded for other reasons are
    /// carried out with it.
    ///
    /// # Errors
    ///
    /// [`StaleClose`] when this recording is gone; nothing is closed.
    pub fn discard_and_close(self) -> Result<(), StaleClose> {
        self.resolve(|state, _| {
            state.make_due(self.reason);
            LifecycleEvent::CloseDue
        })
    }

    /// Withdraw this recorded close and keep the presentation open; releasing
    /// the holds later carries nothing out for it. Closes recorded for other
    /// reasons stay recorded.
    ///
    /// # Errors
    ///
    /// [`StaleClose`] when this recording is gone.
    pub fn stay_open(self) -> Result<(), StaleClose> {
        self.resolve(|state, index| {
            state.records.remove(index);
            state.changed();
            LifecycleEvent::CloseChanged
        })
    }

    /// Apply `resolve` to this recording, if it is still recorded. The epoch
    /// decides, not the reason: a resolution of an older recording never
    /// touches a newer one of the same reason.
    fn resolve(
        &self,
        resolve: impl FnOnce(&mut GuardState, usize) -> LifecycleEvent,
    ) -> Result<(), StaleClose> {
        let mut outcome = Err(StaleClose);
        transition(&self.state, |state| {
            let index = state.position(self.reason, self.epoch)?;
            outcome = Ok(());
            Some(resolve(state, index))
        });
        outcome
    }
}

/// Resolves at the next change to a presentation's recorded closes; see
/// [`CloseGuard::changed`]. Owner-thread only.
#[must_use = "a future does nothing unless polled"]
pub struct CloseChanged {
    state: Rc<RefCell<GuardState>>,
    /// The guard's change count when the future was taken.
    taken_at: u64,
}

impl fmt::Debug for CloseChanged {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CloseChanged").finish_non_exhaustive()
    }
}

impl Future for CloseChanged {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let mut state = self.state.borrow_mut();
        if state.settled > self.taken_at {
            return Poll::Ready(());
        }
        let waker = cx.waker();
        if !state
            .waiters
            .iter()
            .any(|(taken_at, known)| *taken_at == self.taken_at && known.will_wake(waker))
        {
            state.waiters.push((self.taken_at, waker.clone()));
        }
        Poll::Pending
    }
}

/// The presentation's side of a [`CloseGuard`]: the host asks it whether a
/// close may proceed, withdraws a close the platform cancelled, settles it
/// on the owner's turn, and hands the guard to the presentation's widgets.
pub struct CloseGuardSource {
    state: Rc<RefCell<GuardState>>,
}

impl fmt::Debug for CloseGuardSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CloseGuardSource")
            .field("vetoable", &self.state.borrow().vetoable)
            .finish_non_exhaustive()
    }
}

impl CloseGuardSource {
    /// A guard for one presentation whose platform lets the application
    /// refuse a close for each reason in `vetoable`. Every transition
    /// signals the owner through `notify`; the owner then calls
    /// [`settle`](Self::settle) on its turn.
    #[must_use]
    pub fn new(vetoable: &[CloseReason], notify: OwnerNotify) -> Self {
        Self {
            state: Rc::new(RefCell::new(GuardState {
                vetoable: vetoable.to_vec(),
                notify,
                work_holds: 0,
                decision_holds: 0,
                records: Vec::new(),
                next_epoch: 0,
                due: None,
                changes: 0,
                settled: 0,
                waiters: Vec::new(),
            })),
        }
    }

    /// The guard the presentation's widgets acquire.
    #[must_use]
    pub fn guard(&self) -> CloseGuard {
        CloseGuard {
            state: Rc::clone(&self.state),
        }
    }

    /// Whether a hold refuses a close for `reason`. When one does, the close
    /// is recorded as a [`PendingClose`] for `reason` (once; a repeated
    /// request records nothing more) and nothing is torn down; otherwise the
    /// caller carries the close out. A reason the platform does not let the
    /// application refuse is never refused.
    #[must_use]
    pub fn refuses(&self, reason: CloseReason) -> bool {
        let mut refused = false;
        transition(&self.state, |state| {
            if !state.vetoable.contains(&reason) || !state.held_against(reason) {
                return None;
            }
            refused = true;
            if state.records.iter().any(|record| record.reason == reason) {
                return None;
            }
            let epoch = state.next_epoch;
            state.next_epoch = epoch
                .checked_add(1)
                .expect("BUG: a close guard recorded 2^64 closes");
            state.records.push(Record { reason, epoch });
            state.changed();
            Some(LifecycleEvent::CloseChanged)
        });
        refused
    }

    /// Withdraw the close recorded for `reason`, if one is: the platform
    /// cancelled it, as when a log-off is cancelled. Closes recorded for
    /// other reasons stay recorded.
    pub fn withdraw(&self, reason: CloseReason) {
        transition(&self.state, |state| {
            let index = state
                .records
                .iter()
                .position(|record| record.reason == reason)?;
            state.records.remove(index);
            state.changed();
            Some(LifecycleEvent::CloseChanged)
        });
    }

    /// Settle the guard on the owner's turn: take the recorded close that is
    /// due to be carried out now, once, and the wakers of the
    /// [`CloseChanged`] futures a change resolved. A due close whose signal
    /// could not be delivered is still returned here.
    ///
    /// Nothing is woken or run here: the guard's state is committed and its
    /// borrow released before this returns, and the owner wakes the wakers
    /// itself, inside its single containment boundary, one by one. A waker
    /// that panics therefore loses neither the due close, which the owner
    /// already holds, nor the wakers after it.
    pub fn settle(&self) -> SettledClose {
        let mut state = self.state.borrow_mut();
        let close = state.due.take();
        state.settled = state.changes;
        let settled = state.settled;
        let (wake, waiting): (Vec<_>, Vec<_>) = std::mem::take(&mut state.waiters)
            .into_iter()
            .partition(|(taken_at, _)| *taken_at < settled);
        state.waiters = waiting;
        SettledClose {
            close,
            wake: wake.into_iter().map(|(_, waker)| waker).collect(),
        }
    }
}

/// What [`CloseGuardSource::settle`] hands its owner.
#[derive(Debug)]
#[must_use = "a due close is carried out, and the wakers woken, by the owner"]
#[non_exhaustive]
pub struct SettledClose {
    /// The recorded close due to be carried out now, if one is.
    pub close: Option<CloseReason>,
    /// The wakers of the [`CloseChanged`] futures a change resolved, for the
    /// owner to wake inside its containment boundary.
    pub wake: Vec<Waker>,
}
