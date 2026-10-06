//! Holding a presentation open while work that must not be lost finishes.
//!
//! A [`CloseGuard`] belongs to one presentation and is acquired with
//! [`LifecycleContext::close_guard`](crate::LifecycleContext::close_guard) in
//! a lifecycle hook. While a widget keeps a [`CloseHold`] from it, a close the
//! platform lets the application refuse (see [`CloseGuard::can_veto`]) is not
//! carried out; it is recorded as a [`PendingClose`] instead, one per
//! [`CloseReason`], and nothing is torn down. Releasing the last hold carries
//! the recorded close out once, without asking the application's
//! close-request handler again.
//!
//! Two kinds of hold differ in what they refuse:
//!
//! - [`CloseGuard::hold`] covers work that will finish by itself, such as a
//!   write in flight. It refuses [`CloseReason::User`] and
//!   [`CloseReason::Program`], but lets [`CloseReason::SessionEnd`] through:
//!   the framework writes the latest published bytes while the session ends.
//! - [`CloseGuard::require_decision`] covers a state only the user can
//!   resolve, such as a failed write with unsaved edits. It refuses every
//!   reason the platform lets the application refuse, and a recorded close
//!   then waits for [`PendingClose::discard_and_close`] or
//!   [`PendingClose::stay_open`].
//!
//! The guard is the presentation's own state, not a copy: every clone, hold,
//! pending close and [`CloseChanged`] future of one presentation shares it.
//!
//! Not yet wired: no presentation hands a guard to its widgets yet, holds do
//! not refuse a close, nothing is ever recorded as pending, and no platform
//! reports a reason it lets the application refuse.

use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

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

/// One presentation's close state, shared by its guard, holds, pending
/// closes and change futures.
#[derive(Default)]
struct GuardState {}

/// A presentation's capability to hold its close while work finishes.
///
/// `Send + Sync` and cheap to clone; every clone is the same guard. Acquire
/// it in `ViewState::init_state` or `did_change_dependencies` through
/// [`LifecycleContext::close_guard`](crate::LifecycleContext::close_guard).
#[derive(Clone)]
pub struct CloseGuard {
    state: Arc<GuardState>,
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
        CloseHold {
            _state: Arc::clone(&self.state),
        }
    }

    /// Hold the close until the user decides: every reason the platform lets
    /// the application refuse is recorded as a [`PendingClose`], which stays
    /// until [`PendingClose::discard_and_close`] or
    /// [`PendingClose::stay_open`] resolves it, even after this hold is
    /// released.
    pub fn require_decision(&self) -> CloseHold {
        CloseHold {
            _state: Arc::clone(&self.state),
        }
    }

    /// The earliest close recorded and not yet carried out or withdrawn, if
    /// any. At most one is recorded per [`CloseReason`]; a repeated request
    /// for the same reason does not add another.
    #[must_use]
    pub fn pending(&self) -> Option<PendingClose> {
        None
    }

    /// A future that resolves at the next change to what
    /// [`pending`](Self::pending) answers: a close recorded, withdrawn, or
    /// carried out. Take a new one after it resolves.
    pub fn changed(&self) -> CloseChanged {
        CloseChanged {
            _state: Arc::clone(&self.state),
        }
    }

    /// Whether the platform lets the application refuse a close for
    /// `reason`. Where it does not (a session that is already ending, the
    /// web, Android, iOS), holds are not consulted and only the framework's
    /// flush writes what was published.
    #[must_use]
    pub fn can_veto(&self, reason: CloseReason) -> bool {
        let _ = reason;
        false
    }
}

/// A hold on a presentation's close, from [`CloseGuard::hold`] or
/// [`CloseGuard::require_decision`]. Dropping it releases the hold.
#[must_use = "the hold is released as soon as it is dropped"]
pub struct CloseHold {
    _state: Arc<GuardState>,
}

impl fmt::Debug for CloseHold {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CloseHold").finish_non_exhaustive()
    }
}

/// A close that a hold refused and that is waiting, from
/// [`CloseGuard::pending`].
pub struct PendingClose {
    reason: CloseReason,
    _state: Arc<GuardState>,
}

impl fmt::Debug for PendingClose {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PendingClose")
            .field("reason", &self.reason)
            .finish_non_exhaustive()
    }
}

impl PendingClose {
    /// Why the close was requested.
    #[must_use]
    pub fn reason(&self) -> CloseReason {
        self.reason
    }

    /// Close the presentation now, past every hold, discarding what the
    /// holds protected. The decision of the presentation's owner, for
    /// example after the user chose "Close without saving".
    pub fn discard_and_close(self) {}

    /// Withdraw this recorded close and keep the presentation open. Closes
    /// recorded for other reasons stay recorded.
    pub fn stay_open(self) {}
}

/// Resolves at the next change to a presentation's pending closes; see
/// [`CloseGuard::changed`].
#[must_use = "a future does nothing unless polled"]
pub struct CloseChanged {
    _state: Arc<GuardState>,
}

impl fmt::Debug for CloseChanged {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CloseChanged").finish_non_exhaustive()
    }
}

impl Future for CloseChanged {
    type Output = ();

    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<()> {
        // Nothing records or withdraws a close yet, so no change can come.
        Poll::Pending
    }
}

/// The presentation's side of a [`CloseGuard`]: the host asks it whether a
/// close may proceed, withdraws a close the platform cancelled, and hands
/// the guard to the presentation's widgets.
///
/// Not yet wired: no close is refused, so nothing is recorded or queued.
pub struct CloseGuardSource {
    state: Arc<GuardState>,
    vetoable: Vec<CloseReason>,
    #[expect(
        dead_code,
        reason = "called once a released hold carries out a recorded close"
    )]
    queue_close: Box<dyn Fn() + Send + Sync>,
}

impl fmt::Debug for CloseGuardSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CloseGuardSource")
            .field("vetoable", &self.vetoable)
            .finish_non_exhaustive()
    }
}

impl CloseGuardSource {
    /// A guard for one presentation whose platform lets the application
    /// refuse a close for each reason in `vetoable`. `queue_close` queues one
    /// close of the presentation on its owner; the guard calls it once when
    /// the last hold over a recorded close is released, never from inside a
    /// borrow of its own state.
    #[must_use]
    pub fn new(vetoable: &[CloseReason], queue_close: impl Fn() + Send + Sync + 'static) -> Self {
        Self {
            state: Arc::new(GuardState::default()),
            vetoable: vetoable.to_vec(),
            queue_close: Box::new(queue_close),
        }
    }

    /// The guard the presentation's widgets acquire.
    #[must_use]
    pub fn guard(&self) -> CloseGuard {
        CloseGuard {
            state: Arc::clone(&self.state),
        }
    }

    /// Whether a hold refuses a close for `reason`. When one does, the close
    /// is recorded as a [`PendingClose`] for `reason` and nothing is torn
    /// down; otherwise the caller carries the close out.
    #[must_use]
    pub fn refuses(&self, reason: CloseReason) -> bool {
        let _ = reason;
        false
    }

    /// Withdraw the close recorded for `reason`, if one is: the platform
    /// cancelled it, as when a log-off is cancelled. Closes recorded for
    /// other reasons stay recorded.
    pub fn withdraw(&self, reason: CloseReason) {
        let _ = reason;
    }
}
