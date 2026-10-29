//! System notices the host delivers to the application (ADR-0237): the user's
//! session ending, the machine sleeping and waking, the session locking, and
//! memory pressure.
//!
//! A notice is a one-off event, not state: it is not replayed to a late
//! subscriber and it is not part of `AppLifecycleState`. The platform reports a
//! session end to the host as a [`SessionEndPhase`] and takes a
//! [`SessionEndAnswer`] back, and every other event as a [`SystemNotice`]; the
//! host turns both into [`HostNotice`]s for the application and answers
//! [`SessionEndAnswer::Block`] while any subscriber holds a [`SessionEndBlock`].

use std::cell::{Cell, RefCell};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;

pub use flui_foundation::memory::MemoryPressureLevel;

/// A phase of the user's session ending, as the platform reports it to the
/// application handler (`AppHandler::session_end` in the core host).
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

/// A system event other than a session end, as the platform reports it to the
/// application handler (`AppHandler::host_notice` in the core host).
///
/// It carries no owner-local handle, so it is `Copy` and crosses threads; the
/// host turns it into the [`HostNotice`] the application hears.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SystemNotice {
    /// The machine is about to sleep.
    Sleep,
    /// The machine woke from sleep.
    ///
    /// Displays and windows can have changed while the machine slept without an
    /// event reaching the process. The platform marks its display snapshot and
    /// every window's state stale before it hands this notice to the host, so
    /// the first read after it asks the operating system again.
    Wake,
    /// The user's session was locked.
    SessionLocked,
    /// The user's session was unlocked.
    SessionUnlocked,
    /// The system wants memory back.
    MemoryPressure(MemoryPressureLevel),
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
    ///
    /// Where the platform cannot tell a real end from the process merely being
    /// at risk (Android's `onStop` and `onSaveInstanceState`, once per trip to the background),
    /// this means "the process may be killed": it can repeat, and the
    /// application can run again afterwards. Save what must survive; do not
    /// close what the application still needs.
    SessionEnded,
    /// The machine is about to sleep.
    Sleep,
    /// The machine woke from sleep. See [`SystemNotice::Wake`].
    Wake,
    /// The user's session was locked.
    SessionLocked,
    /// The user's session was unlocked.
    SessionUnlocked,
    /// The system wants memory back.
    MemoryPressure(MemoryPressureLevel),
}

impl From<SystemNotice> for HostNotice {
    fn from(notice: SystemNotice) -> Self {
        match notice {
            SystemNotice::Sleep => Self::Sleep,
            SystemNotice::Wake => Self::Wake,
            SystemNotice::SessionLocked => Self::SessionLocked,
            SystemNotice::SessionUnlocked => Self::SessionUnlocked,
            SystemNotice::MemoryPressure(level) => Self::MemoryPressure(level),
        }
    }
}

/// Why a session end took no more blocks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum SessionEndBlockError {
    /// The host has already answered the platform's query. A block taken now
    /// could not change the answer, so none is taken: save before the session
    /// ends instead.
    #[error("the session end was already answered")]
    Answered,
    /// Every block identity of this request is used up. It refuses rather than
    /// reissue one that a live block still holds.
    #[error("the session end request has issued every block it can")]
    Exhausted,
}

/// What a session end is waiting for: the blocks its subscribers hold.
///
/// Every presentation of one host receives clones of one request, so the blocks
/// they hold add up: the host answers [`SessionEndAnswer::Block`] while
/// [`is_blocked`](Self::is_blocked), and closes the request's book by
/// [`answer`](Self::answer)ing: after it, no new block is taken.
#[derive(Clone)]
pub struct SessionEndRequest {
    holds: Rc<Holds>,
}

struct Holds {
    held: RefCell<Vec<(u64, String)>>,
    next: Cell<Option<u64>>,
    answered: Cell<bool>,
    on_release: RefCell<Option<Box<dyn FnOnce()>>>,
}

impl std::fmt::Debug for SessionEndRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionEndRequest")
            .field("reasons", &self.reasons())
            .field("answered", &self.holds.answered.get())
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
            holds: Rc::new(Holds {
                held: RefCell::new(Vec::new()),
                next: Cell::new(Some(0)),
                answered: Cell::new(false),
                on_release: RefCell::new(None),
            }),
        }
    }

    /// Run `release` once, when the last held block is released, so the host
    /// can tell the platform the application is done and the session may go
    /// on. A panic in `release` is contained and dropped: releasing a block
    /// never panics.
    #[must_use]
    pub fn on_release(self, release: impl FnOnce() + 'static) -> Self {
        *self.holds.on_release.borrow_mut() = Some(Box::new(release));
        self
    }

    /// Ask the session end to wait, for `reason` (shown to the user where the
    /// operating system shows one). The wait lasts until the returned block
    /// and every other block on this request are dropped.
    ///
    /// # Errors
    /// [`SessionEndBlockError::Answered`] once the host has answered the query
    /// ([`answer`](Self::answer)); [`SessionEndBlockError::Exhausted`] when the
    /// request has issued every block identity it can.
    pub fn block(
        &self,
        reason: impl Into<String>,
    ) -> Result<SessionEndBlock, SessionEndBlockError> {
        if self.holds.answered.get() {
            return Err(SessionEndBlockError::Answered);
        }
        let id = self
            .holds
            .next
            .get()
            .ok_or(SessionEndBlockError::Exhausted)?;
        self.holds.next.set(id.checked_add(1));
        self.holds.held.borrow_mut().push((id, reason.into()));
        Ok(SessionEndBlock {
            holds: Rc::clone(&self.holds),
            id,
        })
    }

    /// Close the request's book and say how the host answers the platform:
    /// [`SessionEndAnswer::Block`] while any block is held, else
    /// [`SessionEndAnswer::Proceed`]. Later [`block`](Self::block) calls are
    /// refused, so a block can never be taken after the answer it would have
    /// changed.
    pub fn answer(&self) -> SessionEndAnswer {
        self.holds.answered.set(true);
        if self.is_blocked() {
            SessionEndAnswer::Block
        } else {
            SessionEndAnswer::Proceed
        }
    }

    /// Whether any block is held.
    #[must_use]
    pub fn is_blocked(&self) -> bool {
        !self.holds.held.borrow().is_empty()
    }

    /// The reasons of the blocks held now, oldest first.
    #[must_use]
    pub fn reasons(&self) -> Vec<String> {
        self.holds
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
    holds: Rc<Holds>,
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
            let mut held = self.holds.held.borrow_mut();
            held.retain(|(id, _)| *id != self.id);
            held.is_empty()
        };
        // Before the answer, a released block only changes what the answer
        // will be; the platform has been told nothing to take back.
        if !emptied || !self.holds.answered.get() {
            return;
        }
        let release = self.holds.on_release.borrow_mut().take();
        if let Some(release) = release {
            // A hook that panics must not turn a release (possibly during
            // unwinding) into an abort; its payload is left alone rather
            // than dropped, which could panic in turn.
            if let Err(payload) = catch_unwind(AssertUnwindSafe(release)) {
                flui_foundation::panic::retain_opaque_payload(payload);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{SessionEndBlockError, SessionEndRequest};

    /// A request that has issued every block identity it can refuses the next
    /// block instead of reissuing an identity a live block still holds.
    #[test]
    fn block_identities_are_refused_on_exhaustion_and_never_wrap() {
        let request = SessionEndRequest::new();
        request.holds.next.set(Some(u64::MAX));
        let last = request.block("last").expect("the last identity is issued");
        assert_eq!(
            request.block("one too many").map(|_| ()),
            Err(SessionEndBlockError::Exhausted)
        );
        assert_eq!(
            request.reasons(),
            ["last"],
            "the live block is not displaced"
        );
        drop(last);
    }
}
