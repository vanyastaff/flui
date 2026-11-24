//! The threads the shell's work runs on, each holding one of the runtime's
//! off-owner places ([`crate::off_owner`]) and inside the apartment its calls
//! ask for (`ShellExecuteExW`, `IFileOperation` and `ICustomDestinationList`
//! a single-threaded one, the WinRT storage calls a multithreaded one).

use std::cell::RefCell;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use flui_platform::capability::Resolver;
use windows::Win32::System::Com::{
    COINIT, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, COINIT_MULTITHREADED, CoInitializeEx,
    CoUninitialize,
};

use crate::off_owner::{self, OffOwner};

/// Which COM apartment a thread enters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ApartmentKind {
    /// A single-threaded apartment: the shell's objects and dialogs.
    SingleThreaded,
    /// The multithreaded apartment: a thread that only waits on WinRT
    /// asynchronous calls and pumps no messages.
    MultiThreaded,
}

/// This thread's COM apartment, entered for as long as it is held.
pub(crate) struct Apartment {
    entered: bool,
}

impl Apartment {
    /// Enter a single-threaded apartment on this thread.
    pub(crate) fn enter() -> Self {
        Self::enter_kind(ApartmentKind::SingleThreaded)
    }

    /// Enter an apartment of `kind` on this thread. A thread the apartment
    /// could not be entered on still runs its work: each COM call then
    /// fails with `CO_E_NOTINITIALIZED`, which the work answers.
    pub(crate) fn enter_kind(kind: ApartmentKind) -> Self {
        let model: COINIT = match kind {
            ApartmentKind::SingleThreaded => COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE,
            ApartmentKind::MultiThreaded => COINIT_MULTITHREADED,
        };
        // SAFETY: the call takes no pointer of ours; it is balanced by
        // `CoUninitialize` in `drop` on the same thread, only when it
        // succeeded (`S_OK` or `S_FALSE`).
        let entered = unsafe { CoInitializeEx(None, model) }.is_ok();
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

/// The work was refused: the runtime's off-owner places are all taken, or no
/// thread could be started.
#[derive(Debug)]
pub(crate) struct Refused;

/// The runtime's off-owner places ([`crate::off_owner`]) as its shell
/// threads take them: what any thread that starts shell work takes a place
/// in, the owner's requests and the share sheet's handler alike.
#[derive(Debug, Default)]
pub(crate) struct ShellPool {
    places: OffOwner,
}

impl ShellPool {
    /// Run `work` on a new thread named `name`, inside an apartment of
    /// `kind`, holding one of the runtime's off-owner places until it ends,
    /// unwinding included, or until the owner reclaims it.
    ///
    /// # Errors
    ///
    /// [`Refused`] when every place is taken, or the thread could not be
    /// started; `work` is dropped unrun, on this thread.
    pub(crate) fn spawn(
        &self,
        name: &str,
        kind: ApartmentKind,
        work: impl FnOnce() + Send + 'static,
    ) -> Result<Arc<Place>, Refused> {
        let taken = self.places.place().ok_or(Refused)?;
        let place = Arc::new(Place(Mutex::new(Some(taken))));
        let held = PlaceHold(Arc::clone(&place));
        std::thread::Builder::new()
            .name(name.to_owned())
            .spawn(move || {
                // Released last, after the apartment.
                let _held = held;
                let _apartment = Apartment::enter_kind(kind);
                work();
            })
            .map_err(|_| Refused)?;
        Ok(place)
    }
}

/// One running thread's off-owner place: given back once, by the thread as
/// it ends or by the owner when it abandons the thread.
#[derive(Debug)]
pub(crate) struct Place(Mutex<Option<off_owner::Place>>);

impl Place {
    fn release(&self) {
        // The slot holds a place or none; nothing runs under its lock.
        drop(self.0.lock().unwrap_or_else(PoisonError::into_inner).take());
    }

    fn is_released(&self) -> bool {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_none()
    }
}

/// Gives the place back when the thread that holds it ends.
struct PlaceHold(Arc<Place>);

impl Drop for PlaceHold {
    fn drop(&mut self) {
        self.0.release();
    }
}

/// A request the owner started and has not seen end.
struct Running {
    started: Instant,
    place: Arc<Place>,
    /// Answers the request's caller when the owner abandons the thread.
    abandon: Option<Box<dyn FnOnce()>>,
}

/// The shell threads one UI runtime runs for its requests.
///
/// A request runs on a thread of its own, so one that stalls (a slow disk, a
/// handler that does not return) delays nobody else; at most
/// [`off_owner::MAX_IN_FLIGHT`] run at once, and a request past that is refused for
/// now. A thread still running [`ABANDON_AFTER`](Self::ABANDON_AFTER) after
/// it started is abandoned when the owner next needs a place: its place is
/// given back, its caller answered with the request's refusal, and whatever
/// it answers later is discarded, since the caller's resolver was taken
/// (the request's ticket). Owner-local.
pub(crate) struct ShellThreads {
    pool: Arc<ShellPool>,
    running: RefCell<Vec<Running>>,
    abandon_after: Duration,
}

impl Default for ShellThreads {
    fn default() -> Self {
        Self::abandoning_after(Self::ABANDON_AFTER)
    }
}

impl ShellThreads {
    /// How long a shell request may run before its place is reclaimed.
    pub(crate) const ABANDON_AFTER: Duration = Duration::from_secs(120);

    /// Threads whose requests are abandoned after `abandon_after`.
    pub(crate) fn abandoning_after(abandon_after: Duration) -> Self {
        Self {
            pool: Arc::default(),
            running: RefCell::default(),
            abandon_after,
        }
    }

    /// The count the runtime's shell threads share, for code that starts
    /// one off the owner thread.
    pub(crate) fn pool(&self) -> Arc<ShellPool> {
        Arc::clone(&self.pool)
    }

    /// The runtime's off-owner places, which the shell threads take: every
    /// other piece of work off the owner thread (a save's write) takes its
    /// place in the same count.
    pub(crate) fn off_owner(&self) -> &OffOwner {
        &self.pool.places
    }

    /// Answer `answer` with what `work` returns on a single-threaded
    /// apartment thread of its own, or with `refused` when the thread was
    /// refused or abandoned: every request is answered on each exit, and a
    /// thread that unwinds drops the resolver, which ends its completion
    /// `ProviderDropped`.
    pub(crate) fn answer<T: Send + 'static>(
        &self,
        name: &str,
        answer: Resolver<T>,
        refused: impl FnOnce() -> T + 'static,
        work: impl FnOnce() -> T + Send + 'static,
    ) {
        self.reclaim();
        let (worker, here) = OnceAnswer::pair(answer);
        let spawned = self
            .pool
            .spawn(name, ApartmentKind::SingleThreaded, move || {
                worker.resolve(work);
            });
        match spawned {
            Ok(place) => self.running.borrow_mut().push(Running {
                started: Instant::now(),
                place,
                abandon: Some(Box::new(move || here.resolve(refused))),
            }),
            Err(Refused) => here.resolve(refused),
        }
    }

    /// Forget the requests that ended, and abandon those that ran past
    /// [`ABANDON_AFTER`](Self::ABANDON_AFTER); each abandoned caller hears
    /// its refusal after the table's borrow ends.
    fn reclaim(&self) {
        let abandoned = {
            let mut running = self.running.borrow_mut();
            running.retain(|request| !request.place.is_released());
            let (late, kept): (Vec<Running>, Vec<Running>) = std::mem::take(&mut *running)
                .into_iter()
                .partition(|request| request.started.elapsed() >= self.abandon_after);
            *running = kept;
            late
        };
        for mut request in abandoned {
            request.place.release();
            tracing::warn!(
                after = ?self.abandon_after,
                "a shell request did not end in time: its thread is abandoned and its place reclaimed"
            );
            if let Some(abandon) = request.abandon.take() {
                abandon();
            }
        }
    }

    /// How many places are taken now.
    #[cfg(test)]
    pub(crate) fn running(&self) -> usize {
        self.pool.places.held()
    }
}

/// A resolver two places may answer, whichever takes it first: the thread
/// that does the work, or the owner when that thread never started or was
/// abandoned. Taking it is the request's ticket: the later taker answers
/// nothing.
pub(crate) struct OnceAnswer<T>(Arc<Mutex<Option<Resolver<T>>>>);

impl<T: 'static> OnceAnswer<T> {
    /// `answer`'s two takers.
    pub(crate) fn pair(answer: Resolver<T>) -> (Self, Self) {
        let shared = Arc::new(Mutex::new(Some(answer)));
        (Self(Arc::clone(&shared)), Self(shared))
    }

    /// Answer with what `value` makes, unless the other taker answered.
    /// `value` runs only when this one answers, with no lock held.
    pub(crate) fn resolve(self, value: impl FnOnce() -> T) {
        // A slot holds a resolver or none; a panic while it was held leaves
        // one of the two.
        let taken = self.0.lock().unwrap_or_else(PoisonError::into_inner).take();
        if let Some(answer) = taken {
            let _ = answer.resolve(value());
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::sync::{Arc, Mutex, mpsc};
    use std::time::Duration;

    use flui_platform::capability::Abandoned;

    use super::{ApartmentKind, ShellThreads};
    use crate::off_owner::MAX_IN_FLIGHT;
    use crate::testing::win32::TestOwner;

    /// Where a request's answer lands.
    type Landed = Rc<RefCell<Option<Result<&'static str, Abandoned>>>>;

    fn wait_until_free(threads: &ShellThreads) {
        while threads.running() > 0 {
            std::thread::yield_now();
        }
    }

    #[test]
    fn a_runtime_runs_a_bounded_number_of_shell_requests_and_frees_each_place() {
        let threads = ShellThreads::default();
        let pool = threads.pool();
        let (release, released) = mpsc::channel::<()>();
        let released = Arc::new(Mutex::new(released));
        let (ended, ends) = mpsc::channel::<()>();
        for _ in 0..MAX_IN_FLIGHT {
            let released = Arc::clone(&released);
            let ended = ended.clone();
            pool.spawn(
                "flui-shell-test",
                ApartmentKind::SingleThreaded,
                move || {
                    let _ = released.lock().expect("BUG: the test's lock").recv();
                    let _ = ended.send(());
                },
            )
            .expect("a place is free");
        }
        assert!(
            pool.spawn("flui-shell-test", ApartmentKind::MultiThreaded, || {})
                .is_err(),
            "a request past the bound is refused, whoever starts it"
        );

        // Each thread gives its place back as it ends.
        drop(release);
        for _ in 0..MAX_IN_FLIGHT {
            ends.recv().expect("every thread ends");
        }
        wait_until_free(&threads);

        // A thread that panics gives its place back as it unwinds.
        let (done, finished) = mpsc::channel::<()>();
        pool.spawn(
            "flui-shell-test",
            ApartmentKind::SingleThreaded,
            move || {
                let _ = done.send(());
                panic!("the work fails");
            },
        )
        .expect("every place is free again");
        finished.recv().expect("the work ran");
        wait_until_free(&threads);
        pool.spawn("flui-shell-test", ApartmentKind::SingleThreaded, || {})
            .expect("a panicking request gave its place back");
    }

    #[test]
    fn a_request_past_its_deadline_is_abandoned_its_caller_refused_and_its_place_reclaimed() {
        let owner = TestOwner::new();
        let threads = ShellThreads::abandoning_after(Duration::ZERO);
        let (release, released) = mpsc::channel::<()>();
        let released = Arc::new(Mutex::new(released));
        let answers: Vec<Landed> = (0..MAX_IN_FLIGHT)
            .map(|_| {
                let (completion, answer) = owner.ctx(None).completions().pending();
                let released = Arc::clone(&released);
                threads.answer(
                    "flui-shell-test",
                    answer,
                    || "abandoned",
                    move || {
                        let _ = released.lock().expect("BUG: the test's lock").recv();
                        "answered late"
                    },
                );
                let slot = Rc::new(RefCell::new(None));
                let into = Rc::clone(&slot);
                completion.on_complete(move |outcome| *into.borrow_mut() = Some(outcome));
                slot
            })
            .collect();

        // Every place is taken by a hung request; the next one reclaims the
        // first late one's place and runs.
        let (completion, answer) = owner.ctx(None).completions().pending();
        threads.answer("flui-shell-test", answer, || "refused", || "ran");
        let next = Rc::new(RefCell::new(None));
        let into = Rc::clone(&next);
        completion.on_complete(move |outcome| *into.borrow_mut() = Some(outcome));
        owner.turn_until(|| next.borrow().is_some());
        assert_eq!(*next.borrow(), Some(Ok("ran")));
        assert!(
            answers
                .iter()
                .all(|slot| *slot.borrow() == Some(Ok("abandoned"))),
            "each abandoned caller hears its refusal"
        );

        // The hung threads end later; their answers reach nobody.
        drop(release);
        wait_until_free(&threads);
        owner.turn();
        assert!(
            answers
                .iter()
                .all(|slot| *slot.borrow() == Some(Ok("abandoned")))
        );
    }
}
