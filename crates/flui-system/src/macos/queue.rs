//! The requests that leave the main thread: whatever touches the file
//! system, waits on a daemon or asks Launch Services runs as a work item on
//! a global Grand Central Dispatch queue (ADR-0360 §2), at most
//! [`WorkQueue::MAX_IN_FLIGHT`] at once per UI runtime, and answers its
//! caller's resolver from there.
//!
//! The bound keeps a stalled volume from tying up the dispatch pool: a
//! request past it is refused at once with the refusal its caller gave,
//! and a work item gives its place back on every exit, a panic included.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

#[cfg(target_os = "macos")]
use flui_platform::capability::Resolver;

/// One UI runtime's requests off the main thread.
#[derive(Debug, Default)]
pub(crate) struct WorkQueue {
    /// How many work items hold a place; decremented on the queue's threads.
    in_flight: Arc<AtomicUsize>,
}

/// A work item's place, given back when it drops.
#[derive(Debug)]
pub(crate) struct Place(Arc<AtomicUsize>);

impl Drop for Place {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

impl WorkQueue {
    /// The most requests of one UI runtime off the main thread at once.
    pub(crate) const MAX_IN_FLIGHT: usize = 8;

    /// A place for one more work item, or `None` while
    /// [`MAX_IN_FLIGHT`](Self::MAX_IN_FLIGHT) hold one.
    pub(crate) fn place(&self) -> Option<Place> {
        self.in_flight
            .try_update(Ordering::AcqRel, Ordering::Acquire, |held| {
                (held < Self::MAX_IN_FLIGHT).then_some(held + 1)
            })
            .ok()
            .map(|_| Place(Arc::clone(&self.in_flight)))
    }

    /// Run `work` on a global queue and answer `answer` with what it
    /// returns; answer `busy` at once when no place is free.
    ///
    /// The work item moves its captures into the boundary, so the place,
    /// the resolver and `work` drop inside it: a panic in `work` is logged
    /// and the resolver, dropped unanswered, ends the caller's completion
    /// as `ProviderDropped`.
    #[cfg(target_os = "macos")]
    pub(crate) fn run<T: Send + 'static>(
        &self,
        answer: Resolver<T>,
        busy: T,
        work: impl FnOnce() -> T + Send + 'static,
    ) {
        use dispatch2::{DispatchQoS, DispatchQueue, GlobalQueueIdentifier};

        use crate::owner::panic_boundary::{AfterPanic, os_entry};

        let Some(place) = self.place() else {
            // The owner retiring first leaves nothing waiting for the answer.
            let _ = answer.resolve(busy);
            return;
        };
        let queue = DispatchQueue::global_queue(GlobalQueueIdentifier::QualityOfService(
            DispatchQoS::UserInitiated,
        ));
        queue.exec_async(move || {
            os_entry(
                "flui-system work item",
                (),
                AfterPanic::Answer(|()| ()),
                move || {
                    let _place = place;
                    let value = work();
                    // The owner retiring first leaves nothing waiting.
                    let _ = answer.resolve(value);
                },
            );
        });
    }
}

#[cfg(test)]
mod tests {
    use super::WorkQueue;

    #[test]
    fn a_runtime_holds_a_bounded_number_of_places_and_each_comes_back_when_dropped() {
        let queue = WorkQueue::default();
        let mut places = (0..WorkQueue::MAX_IN_FLIGHT)
            .map(|_| queue.place().expect("a place below the bound"))
            .collect::<Vec<super::Place>>();
        assert!(queue.place().is_none(), "no place past the bound");
        drop(places.pop());
        assert!(queue.place().is_some(), "a dropped place is free again");
    }
}
