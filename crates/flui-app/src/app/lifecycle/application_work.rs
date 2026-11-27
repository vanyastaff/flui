//! The application's share of the loop's execution services: one
//! cancellation node that every task and worker the application spawns hangs
//! under, and the count of the jobs still holding it.
//!
//! The loop's pools outlive an application in a development restart
//! (ADR-0393 §4), so the pools' root signal cannot end one application's
//! work. The service registry, which the application's scope owns, holds
//! this node instead: retiring the application closes it, which refuses every
//! later spawn of the earlier application's [`TaskSpawner`] clones, cancels
//! each of its tasks and workers whoever holds their handles, and waits by a
//! deadline for the jobs already running.
//!
//! [`TaskSpawner`]: super::TaskSpawner

use std::sync::Arc;

use flui_runtime::execution::SpawnError;
use tokio_util::sync::CancellationToken;
use web_time::Instant;

use super::CancellationSignal;

/// The node the registry owns. Dropping it does not cancel: the registry
/// closes it on purpose, and the loop's root reaches it at the loop's exit.
#[derive(Debug)]
pub(super) struct ApplicationWork {
    shared: Arc<Shared>,
}

/// A spawner's reference to the node it spawns under.
#[derive(Clone, Debug)]
pub(super) struct WorkRef {
    shared: Arc<Shared>,
}

/// One job's hold on the node, moved into the job; the node counts it
/// running until the job is dropped, run or unrun.
#[derive(Debug)]
pub(super) struct Running {
    shared: Arc<Shared>,
}

#[derive(Debug)]
struct Shared {
    /// A child of the loop's root, so the loop's exit cancels it too.
    token: CancellationToken,
    state: parking_lot::Mutex<State>,
    idle: parking_lot::Condvar,
}

#[derive(Debug, Default)]
struct State {
    /// Set before the token is cancelled, under the lock, so a job that
    /// enters after the close cannot be missed by the join.
    closed: bool,
    running: usize,
}

impl ApplicationWork {
    /// A node under the loop's `root`.
    pub(super) fn child_of(root: &CancellationToken) -> Self {
        Self {
            shared: Arc::new(Shared {
                token: root.child_token(),
                state: parking_lot::Mutex::new(State::default()),
                idle: parking_lot::Condvar::new(),
            }),
        }
    }

    pub(super) fn work_ref(&self) -> WorkRef {
        WorkRef {
            shared: Arc::clone(&self.shared),
        }
    }

    /// Whether [`Self::close_and_join`] ran, or the loop's root was
    /// cancelled.
    pub(super) fn is_closed(&self) -> bool {
        self.shared.token.is_cancelled()
    }

    /// Refuse every later spawn, cancel every task and worker spawned under
    /// this node, and wait until `by` for the jobs still running. Returns
    /// how many were still running at `by`: a compute job that does not
    /// check its cancellation runs on past it.
    pub(super) fn close_and_join(&self, by: Instant) -> usize {
        self.shared.state.lock().closed = true;
        // Outside the lock: cancelling wakes the IO lane's futures, and an
        // executor that polls a woken future in place drops its `Running`
        // here, which takes the lock.
        self.shared.token.cancel();
        let mut state = self.shared.state.lock();
        while state.running > 0 {
            if self.shared.idle.wait_until(&mut state, by).timed_out() {
                break;
            }
        }
        state.running
    }
}

impl WorkRef {
    /// A job's hold on the node, refused once the node is closed.
    pub(super) fn enter(&self) -> Result<Running, SpawnError> {
        let mut state = self.shared.state.lock();
        if state.closed || self.shared.token.is_cancelled() {
            return Err(SpawnError::ShuttingDown);
        }
        state.running += 1;
        Ok(Running {
            shared: Arc::clone(&self.shared),
        })
    }

    /// A new signal under the node: a task's, a worker's.
    pub(super) fn signal(&self) -> CancellationSignal {
        CancellationSignal::child_of(&self.shared.token)
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        let mut state = self.shared.state.lock();
        state.running -= 1;
        if state.running == 0 {
            self.shared.idle.notify_all();
        }
    }
}
