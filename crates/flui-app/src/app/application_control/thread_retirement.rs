//! A bounded non-UI join helper for each admitted owner. Rust's is_finished()
//! can become true before TLS destructors finish, so admission must never join
//! an owner even after that observation. Membership stays until the actual join.
use super::OwnerRoute;
use parking_lot::{Condvar, Mutex};
use std::sync::Arc;
use std::thread::JoinHandle;

enum Handoff {
    Waiting,
    Spawned(JoinHandle<()>),
    Cancelled,
}

struct Slot {
    handoff: Mutex<Handoff>,
    ready: Condvar,
}

/// Once prepared, dropping this ticket without a thread cancels the handoff.
/// The caller settles/quiesces its launch before dropping this ticket.
pub(crate) struct ThreadRetirement(Arc<Slot>);
impl ThreadRetirement {
    pub(crate) fn prepare(route: OwnerRoute) -> std::io::Result<Self> {
        let slot = Arc::new(Slot {
            handoff: Mutex::new(Handoff::Waiting),
            ready: Condvar::new(),
        });
        let worker_slot = Arc::clone(&slot);
        let ingress = Arc::clone(&route.ingress);
        let joiner = std::thread::Builder::new()
            .name("flui-owner-join".to_owned())
            .spawn(move || {
                let handoff = {
                    let mut state = worker_slot.handoff.lock();
                    while matches!(*state, Handoff::Waiting) {
                        worker_slot.ready.wait(&mut state);
                    }
                    std::mem::replace(&mut *state, Handoff::Cancelled)
                };
                let result = match handoff {
                    Handoff::Spawned(thread) => thread.join(),
                    Handoff::Cancelled => Ok(()),
                    Handoff::Waiting => unreachable!("BUG: handoff wait ended without a decision"),
                };
                route.thread_finished(result);
            })?;
        ingress.keep_thread(joiner);
        Ok(Self(slot))
    }

    pub(crate) fn handoff(self, thread: JoinHandle<()>) {
        *self.0.handoff.lock() = Handoff::Spawned(thread);
        self.0.ready.notify_one();
    }
}

impl Drop for ThreadRetirement {
    fn drop(&mut self) {
        let mut state = self.0.handoff.lock();
        if matches!(*state, Handoff::Waiting) {
            *state = Handoff::Cancelled;
        }
        drop(state);
        self.0.ready.notify_one();
    }
}
