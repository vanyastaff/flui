//! The way back to the owner thread.

use std::any::Any;
use std::{cell::RefCell, fmt, rc::Rc, sync::Arc};

use flui_foundation::panic::FirstPanic;

/// The owner has retired: it runs nothing more, and what was posted to it is
/// dropped unrun.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, thiserror::Error)]
#[error("the owner has retired")]
#[allow(
    clippy::exhaustive_structs,
    reason = "retirement is the whole fact: there is nothing more to say about it"
)]
pub struct OwnerRetired;

/// One piece of work posted to the owner thread.
///
/// The owner [runs](Self::run) it at its next turn with its [`OwnerScope`].
/// It is an `FnMut` called at most once, so a job that panics keeps its
/// captures until [`Self::run`] has caught the panic; they are then destroyed
/// under containment instead of during the unwind (ADR-0281 §3).
pub struct OwnerJob(Box<dyn FnMut(&OwnerScope<'_>) + Send>);

impl OwnerJob {
    /// Work that needs the owner's scope. The seam's own jobs capture only
    /// its own values, whose drops run no user code, so they may be `FnOnce`.
    pub(crate) fn scoped(job: impl FnOnce(&OwnerScope<'_>) + Send + 'static) -> Self {
        let mut job = Some(job);
        Self(Box::new(move |scope| {
            if let Some(job) = job.take() {
                job(scope);
            }
        }))
    }

    /// Run the job on the owner thread.
    ///
    /// A panic in the job resumes here once its captures are dropped, under
    /// containment: a panic from their destructor is discarded behind the
    /// job's (ADR-0281).
    pub fn run(self, scope: &OwnerScope<'_>) {
        let mut job = self.0;
        let mut first = FirstPanic::new();
        first.contain(|| job(scope));
        first.contain(|| drop(job));
        first.resume();
    }
}

impl fmt::Debug for OwnerJob {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OwnerJob").finish_non_exhaustive()
    }
}

/// Runs work on the owner thread at its next turn; callable from any thread.
///
/// It is how an operating-system callback that arrives on another thread gets
/// back to the owner (ADR-0152 §2, item 6): the callback's only act is to
/// post, and everything else runs on the owner.
#[derive(Clone)]
pub struct OwnerPoster {
    sink: Arc<dyn Fn(OwnerJob) -> Result<(), OwnerRetired> + Send + Sync>,
}

impl OwnerPoster {
    /// A poster over `sink`, which queues a job for the owner's next turn and
    /// wakes the owner, or answers [`OwnerRetired`] and drops the job.
    ///
    /// The UI runtime makes its poster over its own inbox.
    #[must_use]
    pub fn new(
        sink: impl Fn(OwnerJob) -> Result<(), OwnerRetired> + Send + Sync + 'static,
    ) -> Self {
        Self {
            sink: Arc::new(sink),
        }
    }

    /// Runs `f` once, at the owner's next turn.
    ///
    /// `f` is an `FnMut` called at most once, so its captures outlive its
    /// call and are dropped contained after it, a panic included (see
    /// [`OwnerJob`]).
    ///
    /// # Errors
    ///
    /// [`OwnerRetired`] once the owner has retired; `f` is dropped unrun, so
    /// an operating-system subscription that posts here learns to end itself.
    pub fn post(&self, mut f: impl FnMut() + Send + 'static) -> Result<(), OwnerRetired> {
        self.post_job(OwnerJob(Box::new(move |_| f())))
    }

    pub(crate) fn post_job(&self, job: OwnerJob) -> Result<(), OwnerRetired> {
        (self.sink)(job)
    }
}

impl fmt::Debug for OwnerPoster {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OwnerPoster").finish_non_exhaustive()
    }
}

/// What a posted job reaches on the owner thread.
#[derive(Debug)]
pub struct OwnerScope<'a> {
    completions: &'a PendingCompletions,
}

impl<'a> OwnerScope<'a> {
    /// The scope of the owner that keeps `completions`.
    #[must_use]
    pub fn new(completions: &'a PendingCompletions) -> Self {
        Self { completions }
    }

    pub(crate) fn completions(&self) -> &PendingCompletions {
        self.completions
    }
}

/// An owner's [`Completion`](super::Completion)s that have not settled yet.
///
/// Owner-local. The UI runtime keeps one and lends it to every job it runs;
/// the closures waiting on a completion live here, because a job posted from
/// another thread cannot carry them. [`retire`](Self::retire) ends them all.
#[derive(Clone, Default)]
pub struct PendingCompletions {
    pub(super) inner: Rc<RefCell<PendingState>>,
}

#[derive(Default)]
pub(super) struct PendingState {
    pub(super) slots: Vec<PendingSlot>,
    pub(super) free: Vec<u32>,
    pub(super) retired: bool,
}

pub(super) struct PendingSlot {
    pub(super) generation: u32,
    pub(super) settle: Option<Rc<dyn super::completion::Settle>>,
}

impl PendingCompletions {
    /// An owner with nothing pending.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The owner retires: every pending completion ends as
    /// [`Abandoned::OwnerRetired`](super::Abandoned::OwnerRetired), and the
    /// closures waiting on them are dropped unrun. A completion that settled
    /// before keeps its value.
    ///
    /// Each completion is retired under its own catch, so a capture whose
    /// `Drop` panics, or a waker that panics, leaves the others retired and
    /// woken. The first such panic is returned for the owner to resume with
    /// its own first failure; a later one is discarded. A destructor that
    /// panics while the thread is already unwinding aborts, as in std
    /// (ADR-0281).
    #[must_use = "the first panic of a retirement belongs to the owner's teardown"]
    pub fn retire(&self) -> Option<Box<dyn Any + Send>> {
        let pending: Vec<_> = {
            let mut state = self.inner.borrow_mut();
            state.retired = true;
            state.free.clear();
            state
                .slots
                .drain(..)
                .filter_map(|slot| slot.settle)
                .collect()
        };
        // Run with no borrow held: a waiting closure's captures and a waker
        // may run user code that re-enters this table.
        let mut first = FirstPanic::new();
        for settle in pending {
            first.contain(|| settle.abandon_on_retire());
            // The last reference may hold an answer that never drained; it is
            // dropped after a failure too, and a later panic is discarded.
            first.contain(|| drop(settle));
        }
        first.into_payload()
    }

    /// Whether the owner has retired.
    #[must_use]
    pub fn is_retired(&self) -> bool {
        self.inner.borrow().retired
    }

    /// How many completions are still pending.
    #[must_use]
    pub fn len(&self) -> usize {
        self.inner
            .borrow()
            .slots
            .iter()
            .filter(|slot| slot.settle.is_some())
            .count()
    }

    /// Whether nothing is pending.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl fmt::Debug for PendingCompletions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PendingCompletions")
            .field("pending", &self.len())
            .field("retired", &self.is_retired())
            .finish()
    }
}
