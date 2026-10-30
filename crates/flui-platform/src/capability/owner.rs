//! The way back to the owner thread.

use std::{cell::RefCell, fmt, rc::Rc, sync::Arc};

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
pub struct OwnerJob(Box<dyn FnOnce(&OwnerScope<'_>) + Send>);

impl OwnerJob {
    /// Work that needs the owner's scope.
    pub(crate) fn scoped(job: impl FnOnce(&OwnerScope<'_>) + Send + 'static) -> Self {
        Self(Box::new(job))
    }

    /// Run the job on the owner thread.
    pub fn run(self, scope: &OwnerScope<'_>) {
        (self.0)(scope);
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

    /// Runs `f` at the owner's next turn.
    ///
    /// # Errors
    ///
    /// [`OwnerRetired`] once the owner has retired; `f` is dropped unrun, so
    /// an operating-system subscription that posts here learns to end itself.
    pub fn post(&self, f: impl FnOnce() + Send + 'static) -> Result<(), OwnerRetired> {
        self.post_job(OwnerJob::scoped(move |_| f()))
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
    pub fn retire(&self) {
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
        // Dropped with no borrow held: a waiting closure's captures may run
        // user code in their `Drop`.
        for settle in pending {
            settle.abandon_on_retire();
        }
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
