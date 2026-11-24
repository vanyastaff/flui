//! The bound on a UI runtime's work off its owner thread (ADR-0360 §2).
//!
//! Whatever waits on the file system, a daemon or another application runs
//! off the owner thread, and a stalled volume or a provider that never
//! answers can hold that work for good. One count per UI runtime bounds it on
//! every platform: Apple's dispatch work items, Android's and Windows'
//! threads and a save's write each take a place in [`OffOwner`] before they
//! start, at most [`MAX_IN_FLIGHT`] at once, and give it back on every exit,
//! a panic included. A request past the bound is refused at once with its
//! caller's refusal, never queued.
#![cfg_attr(
    target_arch = "wasm32",
    expect(
        dead_code,
        reason = "a page runs no work off its owner thread: its saves are the browser's"
    )
)]

use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use flui_platform::capability::{ProviderContext, Resolver};

/// The most pieces of work one UI runtime runs off its owner thread at once.
pub(crate) const MAX_IN_FLIGHT: usize = 8;

/// One UI runtime's places for work off its owner thread.
///
/// Clones share the count, so a backend can hand it to a thread that starts
/// work of its own (Windows' share handler).
#[derive(Clone, Debug, Default)]
pub(crate) struct OffOwner {
    /// How many pieces of work hold a place; given back on their threads.
    in_flight: Arc<AtomicUsize>,
}

/// One piece of work's place, given back when it drops.
#[derive(Debug)]
#[must_use = "the place is given back when it drops"]
pub(crate) struct Place(Arc<AtomicUsize>);

impl Drop for Place {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Why no thread took a piece of work.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NotRun {
    /// [`MAX_IN_FLIGHT`] pieces of work hold a place already.
    Busy,
    /// The system started no thread, with its code when it gave one.
    NoThread(Option<i32>),
}

impl OffOwner {
    /// The places of the UI runtime acquiring in `ctx`.
    ///
    /// On Windows every shell request runs on the runtime's shell threads,
    /// so the count is theirs; elsewhere it is the runtime's own.
    pub(crate) fn of(ctx: &ProviderContext<'_>) -> Rc<Self> {
        #[cfg(windows)]
        {
            Rc::new(
                ctx.runtime_local::<crate::win32::ShellThreads>()
                    .off_owner()
                    .clone(),
            )
        }
        #[cfg(not(windows))]
        {
            ctx.runtime_local::<Self>()
        }
    }

    /// A place for one more piece of work, or `None` while
    /// [`MAX_IN_FLIGHT`] hold one.
    pub(crate) fn place(&self) -> Option<Place> {
        self.in_flight
            .try_update(Ordering::AcqRel, Ordering::Acquire, |held| {
                (held < MAX_IN_FLIGHT).then_some(held + 1)
            })
            .ok()
            .map(|_| Place(Arc::clone(&self.in_flight)))
    }

    /// How many places are taken now.
    #[cfg(all(test, windows))]
    pub(crate) fn held(&self) -> usize {
        self.in_flight.load(Ordering::Acquire)
    }

    /// Run `work` on a thread named `name`, holding a place until it ends,
    /// unwinding included.
    ///
    /// # Errors
    ///
    /// Why no thread took it; `work` is dropped unrun, on this thread.
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
    /// thread takes it.
    ///
    /// Work that panics drops the resolver unanswered, so its caller's
    /// completion ends as `ProviderDropped`, and gives its place back.
    pub(crate) fn run<T: Send + 'static>(
        &self,
        name: &str,
        answer: Resolver<T>,
        refused: fn(NotRun) -> T,
        work: impl FnOnce() -> T + Send + 'static,
    ) {
        // The thread answers through the slot; when none takes the work the
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

    use super::{MAX_IN_FLIGHT, NotRun, OffOwner};
    use crate::testing::owner::TestOwner;

    #[test]
    fn a_runtime_holds_a_bounded_number_of_places_and_each_comes_back_when_dropped() {
        let places = OffOwner::default();
        let shared = places.clone();
        let mut held = (0..MAX_IN_FLIGHT)
            .map(|_| shared.place().expect("a place below the bound"))
            .collect::<Vec<super::Place>>();
        assert!(
            places.place().is_none(),
            "no place past the bound, whichever clone asks"
        );
        drop(held.pop());
        assert!(places.place().is_some(), "a dropped place is free again");
    }

    /// A runtime runs a bounded number of threads: one more is refused at
    /// once with what its caller makes of the reason, and a thread that ends
    /// gives its place to the next.
    #[test]
    fn a_runtime_runs_a_bounded_number_of_threads_and_refuses_the_next() {
        let owner = TestOwner::new();
        let places = OffOwner::default();
        let (release, held) = mpsc::channel::<()>();
        let held = std::sync::Arc::new(std::sync::Mutex::new(held));
        let mut answers = Vec::new();
        for _ in 0..MAX_IN_FLIGHT {
            let (completion, answer) = owner.completions().pending::<Result<u8, NotRun>>();
            let held = std::sync::Arc::clone(&held);
            places.run("flui-system-test", answer, Err, move || {
                held.lock().expect("no holder panics").recv().ok();
                Ok(1)
            });
            answers.push(completion);
        }
        let (refused, answer) = owner.completions().pending::<Result<u8, NotRun>>();
        places.run("flui-system-test", answer, Err, || Ok(2));
        assert_eq!(owner.settle(refused), Ok(Err(NotRun::Busy)));

        for _ in 0..MAX_IN_FLIGHT {
            release.send(()).expect("a thread waits");
        }
        for completion in answers {
            assert_eq!(owner.settle(completion), Ok(Ok(1)));
        }
        // A thread answers before its place comes back, so wait for one.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while places.place().is_none() {
            assert!(std::time::Instant::now() < deadline, "a place came back");
            std::thread::yield_now();
        }
        let (next, answer) = owner.completions().pending::<Result<u8, NotRun>>();
        places.run("flui-system-test", answer, Err, || Ok(3));
        assert_eq!(owner.settle(next), Ok(Ok(3)));
    }
}
