//! System notices the host delivers to the application (ADR-0237): the user's
//! session ending, the machine sleeping and waking, the session locking, and
//! memory pressure.
//!
//! A notice is a one-off event, not state: it is not replayed to a late
//! subscriber and it is not part of `AppLifecycleState`. The platform reports a
//! session end to the host as a [`SessionEndPhase`] and takes a
//! [`SessionEndAnswer`] back; the host turns the phases into [`HostNotice`]s for
//! the application and answers [`SessionEndAnswer::Block`] while any
//! subscriber holds a [`SessionEndBlock`].

use std::cell::{Cell, RefCell};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;

/// A phase of the user's session ending, as the platform reports it to
/// `Platform::on_session_end`.
///
/// On Windows a session end is a query to every top-level window
/// (`WM_QUERYENDSESSION`), then its outcome (`WM_ENDSESSION`): either the
/// session ends, and the process is terminated soon after the callback
/// returns, or it was cancelled, by the user or by another application.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SessionEndPhase {
    /// The session is about to end; the answer says whether the application
    /// asks the user to wait.
    Query,
    /// The session end was cancelled; the session goes on.
    Cancelled,
    /// The session is ending now; the process will not outlive it for long.
    Ending,
}

/// The application's answer to a [`SessionEndPhase`]. Only an answer to
/// [`SessionEndPhase::Query`] is read; the platform ignores the others.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SessionEndAnswer {
    /// Let the session end.
    Proceed,
    /// Ask the user to wait: work that cannot be written without a decision
    /// would be lost. The operating system shows the application as blocking
    /// and lets the user end the session anyway.
    Block,
}

/// How much memory the system wants back.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum MemoryPressureLevel {
    /// Free what is easy to restore.
    Moderate,
    /// Free everything the current frame does not need.
    Critical,
}

/// A one-off system event for the application.
///
/// Delivered on the owner thread through `LifecycleHandle::subscribe_notices`.
/// A notice holds an owner-local handle ([`SessionEndRequest`]) in one variant,
/// so it is neither `Send` nor `Sync`.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum HostNotice {
    /// The user's session is about to end. A subscriber that needs time calls
    /// [`SessionEndRequest::block`] before it returns.
    SessionEnding(SessionEndRequest),
    /// The session end was cancelled; the session goes on.
    SessionEndCancelled,
    /// The session is ending now. Save what must survive.
    SessionEnded,
    /// The machine is about to sleep.
    Sleep,
    /// The machine woke from sleep.
    Wake,
    /// The user's session was locked.
    SessionLocked,
    /// The user's session was unlocked.
    SessionUnlocked,
    /// The system wants memory back.
    MemoryPressure(MemoryPressureLevel),
}

/// What a session end is waiting for: the blocks its subscribers hold.
///
/// Every presentation of one host receives clones of one request, so the blocks
/// they hold add up: the host answers [`SessionEndAnswer::Block`] while
/// [`is_blocked`](Self::is_blocked).
#[derive(Clone)]
pub struct SessionEndRequest {
    ledger: Rc<Ledger>,
}

struct Ledger {
    held: RefCell<Vec<(u64, String)>>,
    next: Cell<u64>,
    on_released: RefCell<Option<Box<dyn FnOnce()>>>,
}

impl std::fmt::Debug for SessionEndRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionEndRequest")
            .field("reasons", &self.reasons())
            .finish()
    }
}

impl Default for SessionEndRequest {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionEndRequest {
    /// A request nobody holds a block on.
    #[must_use]
    pub fn new() -> Self {
        Self {
            ledger: Rc::new(Ledger {
                held: RefCell::new(Vec::new()),
                next: Cell::new(0),
                on_released: RefCell::new(None),
            }),
        }
    }

    /// Run `release` once, when the last held block is released, so the host
    /// can tell the operating system it may go on. A panic in `release` is
    /// contained and dropped: releasing a block never panics.
    #[must_use]
    pub fn releasing(self, release: impl FnOnce() + 'static) -> Self {
        *self.ledger.on_released.borrow_mut() = Some(Box::new(release));
        self
    }

    /// Ask the session end to wait, for `reason` (shown to the user where the
    /// operating system shows one). The wait lasts until the returned block
    /// and every other block on this request are dropped.
    pub fn block(&self, reason: impl Into<String>) -> SessionEndBlock {
        let id = self.ledger.next.get();
        self.ledger.next.set(id.wrapping_add(1));
        self.ledger.held.borrow_mut().push((id, reason.into()));
        SessionEndBlock {
            ledger: Rc::clone(&self.ledger),
            id,
        }
    }

    /// Whether any block is held.
    #[must_use]
    pub fn is_blocked(&self) -> bool {
        !self.ledger.held.borrow().is_empty()
    }

    /// The reasons of the blocks held now, oldest first.
    #[must_use]
    pub fn reasons(&self) -> Vec<String> {
        self.ledger
            .held
            .borrow()
            .iter()
            .map(|(_, reason)| reason.clone())
            .collect()
    }
}

/// One hold on a session end. Dropping it releases it; when it was the last,
/// the request's release hook runs.
#[must_use = "dropping a SessionEndBlock releases it at once"]
pub struct SessionEndBlock {
    ledger: Rc<Ledger>,
    id: u64,
}

impl std::fmt::Debug for SessionEndBlock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionEndBlock")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

impl SessionEndBlock {
    /// Release this block now. The same as dropping it.
    pub fn release(self) {}
}

impl Drop for SessionEndBlock {
    fn drop(&mut self) {
        let emptied = {
            let mut held = self.ledger.held.borrow_mut();
            held.retain(|(id, _)| *id != self.id);
            held.is_empty()
        };
        if !emptied {
            return;
        }
        let release = self.ledger.on_released.borrow_mut().take();
        if let Some(release) = release {
            // A hook that panics must not turn a release (possibly during
            // unwinding) into an abort; its payload is left alone rather
            // than dropped, which could panic in turn.
            if let Err(payload) = catch_unwind(AssertUnwindSafe(release)) {
                std::mem::forget(payload);
            }
        }
    }
}
