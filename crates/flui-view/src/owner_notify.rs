//! The one channel through which close guards and the flush registry tell
//! their owner that something changed.
//!
//! A transition — a close recorded or withdrawn, the last hold released, a
//! write finished — never runs waiters or application code where it happens.
//! It only signals the owner through [`OwnerNotify`]; the owner then settles
//! the guard ([`CloseGuardSource::settle`](crate::__runtime::CloseGuardSource::settle))
//! or the registry ([`FlushHost::settle`](crate::__runtime::FlushHost::settle))
//! on its own turn, inside its single containment boundary, and that is where
//! waiters wake. A signal that cannot be delivered loses nothing: the
//! transition stays recorded, and the next settle finds it.

use std::fmt;
use std::sync::Arc;

/// What changed, as an [`OwnerNotify`] reports it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum LifecycleEvent {
    /// A presentation's recorded closes changed: one was recorded,
    /// withdrawn, discarded or resolved to stay open.
    CloseChanged,
    /// The last hold over a recorded close was released: the close is due.
    CloseDue,
    /// A write of the flush registry finished, or failed.
    FlushChanged,
}

/// The owner can no longer be reached: its loop has ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("the owner can no longer be reached")]
pub struct Undeliverable;

/// The host's channel for telling its owner thread that a guard or the
/// registry has something to settle.
///
/// `Send + Sync`: a write finishes on an IO thread. The callback must only
/// signal (wake the owner's loop, queue a turn) and never run application
/// code itself.
#[derive(Clone)]
pub struct OwnerNotify(Arc<dyn Fn(LifecycleEvent) -> Result<(), Undeliverable> + Send + Sync>);

impl fmt::Debug for OwnerNotify {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("OwnerNotify").finish_non_exhaustive()
    }
}

impl OwnerNotify {
    /// A channel that signals the owner through `notify`.
    pub fn new(
        notify: impl Fn(LifecycleEvent) -> Result<(), Undeliverable> + Send + Sync + 'static,
    ) -> Self {
        Self(Arc::new(notify))
    }

    /// Signal the owner that `event` happened.
    ///
    /// # Errors
    ///
    /// [`Undeliverable`] when the owner can no longer be reached; the
    /// transition that signalled stays recorded.
    pub fn notify(&self, event: LifecycleEvent) -> Result<(), Undeliverable> {
        (self.0)(event)
    }
}
