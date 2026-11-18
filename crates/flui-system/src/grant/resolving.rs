//! A resolve in flight: the sandbox it waits for, the backend's hold it
//! lasts by, and the [`Resolving`] its caller holds.

use std::any::Any;
use std::cell::Cell;
use std::fmt;
use std::future::Future;
use std::rc::Rc;

use flui_platform::capability::{Abandoned, Completion};

use super::GrantResult;
use crate::sandbox::Sandbox;

/// What a backend gives the handle for each resolve: the work goes on while
/// the handle holds it, and its drop stops the work nobody waits for (an
/// Android read's cancellation signal and its read loop). `()` holds
/// nothing, for a backend whose work cannot be stopped.
#[must_use = "dropping the hold stops the resolve it holds"]
pub struct ResolveHold(#[expect(dead_code, reason = "held for its drop")] Box<dyn Any>);

impl ResolveHold {
    /// A hold whose drop drops `keep`: the value whose `Drop` stops the work.
    pub fn new(keep: impl Any) -> Self {
        Self(Box::new(keep))
    }
}

impl fmt::Debug for ResolveHold {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ResolveHold").finish_non_exhaustive()
    }
}

/// What a resolve that waits for the sandbox runs once it is read: `None`
/// when the backend answered none.
type WhenRead = Box<dyn FnOnce(Option<Rc<Sandbox>>)>;

/// The sandbox a handle's resolves are held to: read once, off the owner
/// thread where it reads the file system, and the resolves asked for
/// meanwhile wait for it.
pub(super) struct SandboxSlot {
    /// One `Cell` over the state, so each step takes it out and puts the
    /// next in: no borrow is held while a waiting resolve starts.
    state: Cell<SlotState>,
}

#[derive(Default)]
enum SlotState {
    /// The backend has not answered; these resolves wait.
    Waiting(Vec<WhenRead>),
    /// The backend answered this sandbox, or none.
    Read(Option<Rc<Sandbox>>),
    /// Taken for the length of a step.
    #[default]
    Taken,
}

impl SandboxSlot {
    /// A slot the backend's answer `read` fills.
    pub(super) fn waiting_for(read: Completion<Sandbox>) -> Rc<Self> {
        let slot = Rc::new(Self {
            state: Cell::new(SlotState::Waiting(Vec::new())),
        });
        let filled = Rc::downgrade(&slot);
        read.on_complete(move |outcome| {
            let Some(slot) = filled.upgrade() else {
                return;
            };
            let sandbox = outcome.ok().map(Rc::new);
            let waiting = match slot.state.replace(SlotState::Read(sandbox.clone())) {
                SlotState::Waiting(waiting) => waiting,
                SlotState::Read(_) | SlotState::Taken => Vec::new(),
            };
            // The slot is committed before any resolve starts: one that asks
            // the handle again finds the sandbox read.
            for start in waiting {
                start(sandbox.clone());
            }
        });
        slot
    }

    /// Run `start` with the sandbox: now when it is read, else once it is.
    pub(super) fn when_read(&self, start: impl FnOnce(Option<Rc<Sandbox>>) + 'static) {
        let state = &self.state;
        match state.take() {
            SlotState::Read(sandbox) => {
                state.set(SlotState::Read(sandbox.clone()));
                start(sandbox);
            }
            SlotState::Waiting(mut waiting) => {
                waiting.push(Box::new(start));
                state.set(SlotState::Waiting(waiting));
            }
            SlotState::Taken => {
                unreachable!("BUG: the slot's state is put back before any code it runs")
            }
        }
    }
}

/// What a resolve holds while it runs: the backend's hold, once the
/// sandbox was read and the backend asked.
#[derive(Default)]
pub(super) struct ResolveState {
    hold: Cell<Option<ResolveHold>>,
}

impl ResolveState {
    /// Keep `hold` for as long as the caller holds the resolve.
    pub(super) fn hold(&self, hold: ResolveHold) {
        // The earlier value is none: a resolve asks its backend once.
        drop(self.hold.replace(Some(hold)));
    }
}

/// A resolve in flight, and the answer it will give.
///
/// Owner-thread (`!Send`). The resolve lasts while this value, or the
/// closure or future it was turned into, is held: dropping it stops the
/// backend's work and nothing is delivered. The answer settles at an owner
/// turn, never inside the call that asked.
#[must_use = "dropping a resolve stops it"]
pub struct Resolving {
    completion: Completion<GrantResult>,
    state: Rc<ResolveState>,
}

impl Resolving {
    pub(super) fn new(completion: Completion<GrantResult>, state: Rc<ResolveState>) -> Self {
        Self { completion, state }
    }

    /// Runs `f` once, with the outcome, at an owner turn boundary; the
    /// resolve goes on until then.
    pub fn on_complete(self, mut f: impl FnMut(Result<GrantResult, Abandoned>) + 'static) {
        let Self { completion, state } = self;
        completion.on_complete(move |outcome| {
            // Held until the outcome arrives, so the resolve goes on.
            let _state = &state;
            f(outcome);
        });
    }

    /// Takes the outcome if it has settled: `None` while the resolve runs,
    /// and after the outcome was taken.
    pub fn poll(&mut self) -> Option<Result<GrantResult, Abandoned>> {
        self.completion.poll()
    }

    /// The outcome as a future, for code outside the frame path; dropping
    /// the future stops the resolve.
    pub fn into_future(self) -> impl Future<Output = Result<GrantResult, Abandoned>> {
        let Self { completion, state } = self;
        let outcome = completion.into_future();
        async move {
            let _state = state;
            outcome.await
        }
    }
}

impl fmt::Debug for Resolving {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Resolving")
            .field("completion", &self.completion)
            .finish_non_exhaustive()
    }
}
