//! The requests that leave the owner thread: whatever reads another
//! application's content provider or the file system runs on a thread of its
//! own (ADR-0360 §2), at most [`Workers::MAX_IN_FLIGHT`] at once per UI
//! runtime, and answers its caller's resolver from there.
//!
//! A content provider answers when it will (a cloud provider once it has the
//! document) and may ignore a cancellation, so a worker can stay inside a
//! call for good. The bound keeps such providers from piling threads up: a
//! request past it is refused at once, and a worker gives its place back on
//! every exit, a panic included.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use flui_platform::capability::Resolver;

/// One UI runtime's requests off the owner thread.
#[derive(Debug, Default)]
pub(crate) struct Workers {
    /// How many workers hold a place; decremented on the workers' threads.
    in_flight: Arc<AtomicUsize>,
}

/// A worker's place, given back when it drops.
#[derive(Debug)]
struct Place(Arc<AtomicUsize>);

impl Drop for Place {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Why no worker took a request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NotRun {
    /// [`Workers::MAX_IN_FLIGHT`] workers run already.
    Busy,
    /// The system started no thread, with its code when it gave one.
    NoThread(Option<i32>),
}

impl Workers {
    /// The most requests of one UI runtime off the owner thread at once.
    pub(crate) const MAX_IN_FLIGHT: usize = 4;

    /// A place for one more worker, or `None` while
    /// [`MAX_IN_FLIGHT`](Self::MAX_IN_FLIGHT) hold one.
    fn place(&self) -> Option<Place> {
        self.in_flight
            .try_update(Ordering::AcqRel, Ordering::Acquire, |held| {
                (held < Self::MAX_IN_FLIGHT).then_some(held + 1)
            })
            .ok()
            .map(|_| Place(Arc::clone(&self.in_flight)))
    }

    /// Run `work` on a thread named `name`.
    ///
    /// # Errors
    ///
    /// Why no worker took it; `work` is dropped unrun.
    pub(crate) fn spawn(
        &self,
        name: &str,
        work: impl FnOnce() + Send + 'static,
    ) -> Result<(), NotRun> {
        let place = self.place().ok_or(NotRun::Busy)?;
        std::thread::Builder::new()
            .name(name.to_owned())
            .spawn(move || {
                let _place = place;
                work();
            })
            .map(drop)
            .map_err(|error| NotRun::NoThread(error.raw_os_error()))
    }

    /// Run `work` on a thread named `name` and answer `answer` with what it
    /// returns; answer what `refused` makes of the reason at once when no
    /// worker takes it.
    ///
    /// A worker that panics drops the resolver unanswered, so its caller's
    /// completion ends as `ProviderDropped`, and gives its place back.
    pub(crate) fn run<T: Send + 'static>(
        &self,
        name: &str,
        answer: Resolver<T>,
        refused: fn(NotRun) -> T,
        work: impl FnOnce() -> T + Send + 'static,
    ) {
        // The worker answers through the slot; when none takes the work the
        // resolver is still there, and the refusal is answered here.
        let slot = Arc::new(Mutex::new(Some(answer)));
        let worker = Arc::clone(&slot);
        let spawned = self.spawn(name, move || {
            let value = work();
            let answer = worker.lock().unwrap_or_else(PoisonError::into_inner).take();
            if let Some(answer) = answer {
                // The owner retiring first leaves nothing waiting.
                let _ = answer.resolve(value);
            }
        });
        if let Err(reason) = spawned {
            let answer = slot.lock().unwrap_or_else(PoisonError::into_inner).take();
            if let Some(answer) = answer {
                let _ = answer.resolve(refused(reason));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use super::{NotRun, Workers};
    use crate::testing::owner::TestOwner;

    /// A runtime runs a bounded number of workers: one more is refused at
    /// once with what its caller makes of the reason, and a worker that ends
    /// gives its place to the next.
    #[test]
    fn a_runtime_runs_a_bounded_number_of_workers_and_refuses_the_next() {
        let owner = TestOwner::new();
        let workers = Workers::default();
        let (release, held) = mpsc::channel::<()>();
        let held = std::sync::Arc::new(std::sync::Mutex::new(held));
        let mut answers = Vec::new();
        for _ in 0..Workers::MAX_IN_FLIGHT {
            let (completion, answer) = owner.completions().pending::<Result<u8, NotRun>>();
            let held = std::sync::Arc::clone(&held);
            workers.run("flui-system-test", answer, Err, move || {
                held.lock().expect("no holder panics").recv().ok();
                Ok(1)
            });
            answers.push(completion);
        }
        let (refused, answer) = owner.completions().pending::<Result<u8, NotRun>>();
        workers.run("flui-system-test", answer, Err, || Ok(2));
        assert_eq!(owner.settle(refused), Ok(Err(NotRun::Busy)));

        for _ in 0..Workers::MAX_IN_FLIGHT {
            release.send(()).expect("a worker waits");
        }
        for completion in answers {
            assert_eq!(owner.settle(completion), Ok(Ok(1)));
        }
        // A worker answers before its place comes back, so wait for one.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while workers.place().is_none() {
            assert!(std::time::Instant::now() < deadline, "a place came back");
            std::thread::yield_now();
        }
        let (next, answer) = owner.completions().pending::<Result<u8, NotRun>>();
        workers.run("flui-system-test", answer, Err, || Ok(3));
        assert_eq!(owner.settle(next), Ok(Ok(3)));
    }
}
