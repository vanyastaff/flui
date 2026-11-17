//! The threads the shell's work runs on: each inside a single-threaded
//! apartment, as `ShellExecuteExW`, `IFileOperation`, `ICustomDestinationList`
//! and the common item dialogs ask, and bounded per UI runtime.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use flui_platform::capability::Resolver;

use windows::Win32::System::Com::{
    COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx, CoUninitialize,
};

/// This thread's single-threaded apartment, entered for as long as it is
/// held.
pub(super) struct Apartment {
    entered: bool,
}

impl Apartment {
    /// Enter a single-threaded apartment on this thread. A thread the
    /// apartment could not be entered on still runs its work: each COM call
    /// then fails with `CO_E_NOTINITIALIZED`, which the work answers.
    pub(super) fn enter() -> Self {
        // SAFETY: the call takes no pointer of ours; it is balanced by
        // `CoUninitialize` in `drop` on the same thread, only when it
        // succeeded (`S_OK` or `S_FALSE`).
        let entered =
            unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) }
                .is_ok();
        Self { entered }
    }
}

impl Drop for Apartment {
    fn drop(&mut self) {
        if self.entered {
            // SAFETY: balances the successful `CoInitializeEx` of `enter`,
            // on the thread that made it, after every COM object of the
            // work was released.
            unsafe { CoUninitialize() };
        }
    }
}

/// The apartment threads one UI runtime has running for the shell's work.
///
/// A request runs on a thread of its own, so one that stalls (a slow disk, a
/// handler that does not return) delays nobody else; at most
/// [`MAX_IN_FLIGHT`](Self::MAX_IN_FLIGHT) run at once, and a request past
/// that is refused for now. Owner-local; the count is shared with the
/// threads, which release their place as they end, unwinding included.
#[derive(Default)]
pub(crate) struct ShellThreads {
    running: Arc<AtomicUsize>,
}

/// The work was refused: the runtime has [`ShellThreads::MAX_IN_FLIGHT`]
/// threads running, or no thread could be started.
#[derive(Debug)]
pub(crate) struct Refused;

impl ShellThreads {
    /// The most shell requests one UI runtime runs at once.
    pub(crate) const MAX_IN_FLIGHT: usize = 8;

    /// Run `work` on a new thread named `name`, inside its apartment.
    ///
    /// # Errors
    ///
    /// [`Refused`] when [`MAX_IN_FLIGHT`](Self::MAX_IN_FLIGHT) requests run,
    /// or the thread could not be started; `work` is dropped unrun, on this
    /// thread.
    pub(crate) fn spawn(
        &self,
        name: &str,
        work: impl FnOnce() + Send + 'static,
    ) -> Result<(), Refused> {
        let taken = self
            .running
            .try_update(Ordering::AcqRel, Ordering::Acquire, |running| {
                (running < Self::MAX_IN_FLIGHT).then_some(running + 1)
            });
        if taken.is_err() {
            return Err(Refused);
        }
        let place = Place(Arc::clone(&self.running));
        std::thread::Builder::new()
            .name(name.to_owned())
            .spawn(move || {
                // Released last, after the apartment: an unwinding thread
                // releases it too.
                let _place = place;
                let _apartment = Apartment::enter();
                work();
            })
            .map(drop)
            .map_err(|_| Refused)
    }

    /// Answer `answer` with what `work` returns on a thread of its own, or
    /// with `refused` when the thread was refused: every request is answered
    /// on both exits, and a thread that unwinds drops the resolver, which
    /// ends its completion `ProviderDropped`.
    pub(crate) fn answer<T: Send + 'static>(
        &self,
        name: &str,
        answer: Resolver<T>,
        refused: impl FnOnce() -> T,
        work: impl FnOnce() -> T + Send + 'static,
    ) {
        let (worker, here) = OnceAnswer::pair(answer);
        if self.spawn(name, move || worker.resolve(work)).is_err() {
            here.resolve(refused);
        }
    }

    /// How many requests run now.
    #[cfg(test)]
    pub(crate) fn running(&self) -> usize {
        self.running.load(Ordering::Acquire)
    }
}

/// A resolver two places may answer, whichever takes it first: the thread
/// that does the work, or the caller when that thread never started.
pub(super) struct OnceAnswer<T>(Arc<Mutex<Option<Resolver<T>>>>);

impl<T: 'static> OnceAnswer<T> {
    /// `answer`'s two takers.
    pub(super) fn pair(answer: Resolver<T>) -> (Self, Self) {
        let shared = Arc::new(Mutex::new(Some(answer)));
        (Self(Arc::clone(&shared)), Self(shared))
    }

    /// Answer with what `value` makes, unless the other taker answered.
    /// `value` runs only when this one answers, with no lock held.
    pub(super) fn resolve(self, value: impl FnOnce() -> T) {
        // A slot holds a resolver or none; a panic while it was held leaves
        // one of the two.
        let taken = self.0.lock().unwrap_or_else(PoisonError::into_inner).take();
        if let Some(answer) = taken {
            let _ = answer.resolve(value());
        }
    }
}

/// One running request's place in the count.
struct Place(Arc<AtomicUsize>);

impl Drop for Place {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use super::ShellThreads;

    #[test]
    fn a_runtime_runs_a_bounded_number_of_shell_requests_and_frees_each_place() {
        let threads = ShellThreads::default();
        let (release, released) = mpsc::channel::<()>();
        let released = std::sync::Arc::new(std::sync::Mutex::new(released));
        let (ended, ends) = mpsc::channel::<()>();
        for _ in 0..ShellThreads::MAX_IN_FLIGHT {
            let released = std::sync::Arc::clone(&released);
            let ended = ended.clone();
            threads
                .spawn("flui-shell-test", move || {
                    let _ = released.lock().expect("BUG: the test's lock").recv();
                    let _ = ended.send(());
                })
                .expect("a place is free");
        }
        assert!(
            threads.spawn("flui-shell-test", || {}).is_err(),
            "a request past the bound is refused"
        );

        // One that panics releases its place as it unwinds.
        release.send(()).expect("a thread waits");
        ends.recv().expect("the released thread ends");
        drop(release);
        for _ in 1..ShellThreads::MAX_IN_FLIGHT {
            ends.recv().expect("every thread ends");
        }
        while threads.running() > 0 {
            std::thread::yield_now();
        }
        let (done, finished) = mpsc::channel::<()>();
        threads
            .spawn("flui-shell-test", move || {
                let _ = done.send(());
                panic!("the work fails");
            })
            .expect("every place is free again");
        finished.recv().expect("the work ran");
        while threads.running() > 0 {
            std::thread::yield_now();
        }
        threads
            .spawn("flui-shell-test", || {})
            .expect("a panicking request gave its place back");
    }
}
