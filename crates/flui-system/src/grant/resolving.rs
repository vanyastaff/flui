//! A resolve in flight: the sandbox it waits for, the backend's hold it
//! lasts by, and the [`Resolving`] its caller holds.

use std::any::Any;
use std::cell::Cell;
use std::fmt;
use std::future::Future;
use std::rc::Rc;

use flui_platform::capability::{Abandoned, Completion, FirstPanic};

use super::GrantResult;
use crate::sandbox::Sandbox;

/// What a backend gives the handle for each resolve: the work goes on while
/// the handle holds it, and its drop stops what of the work nobody waits for
/// can be stopped (an Android read's loop and its cancellation signal, an
/// Apple scope not yet started). `()` holds nothing, for a backend whose work
/// cannot be stopped.
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

/// The sandbox a handle's resolves are held to: read off the owner thread
/// where it reads the file system, one read at a time, the resolves asked
/// for meanwhile waiting for it. A read the backend did not answer is not
/// kept: the next resolve asks again.
pub(super) struct SandboxSlot {
    /// One `Cell` over the state, so each step takes it out and puts the
    /// next in: no borrow is held while a waiting resolve starts.
    state: Cell<SlotState>,
    /// The ticket the next waiting resolve gets.
    next_ticket: Cell<u64>,
}

#[derive(Default)]
enum SlotState {
    /// No read is in flight, and none answered: the next resolve asks.
    #[default]
    Unread,
    /// A read is in flight; these resolves wait for it, by ticket.
    Reading(Vec<(u64, WhenRead)>),
    /// The backend answered this sandbox.
    Read(Rc<Sandbox>),
}

/// Too many resolves already wait for the sandbox.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct TooManyWaiting;

impl SandboxSlot {
    /// The most resolves of one handle that wait for its sandbox at once.
    pub(super) const MAX_WAITING: usize = 64;

    /// A slot whose sandbox the first resolve asks for: every read starts in
    /// a resolve, so a backend's panic always reaches the one that asked.
    pub(super) fn unread() -> Rc<Self> {
        Rc::new(Self {
            state: Cell::new(SlotState::Unread),
            next_ticket: Cell::new(0),
        })
    }

    /// Fill `slot` with `read`'s answer, and start the resolves that wait:
    /// with the sandbox, or with none when the backend dropped the read,
    /// which leaves the slot unread for the next resolve to ask again.
    fn fill_on(slot: &Rc<Self>, read: Completion<Sandbox>) {
        let filled = Rc::downgrade(slot);
        read.on_complete(move |outcome| {
            let Some(slot) = filled.upgrade() else {
                return;
            };
            let sandbox = outcome.ok().map(Rc::new);
            let settled = sandbox.clone().map_or(SlotState::Unread, SlotState::Read);
            let waiting = match slot.state.replace(settled) {
                SlotState::Reading(waiting) => waiting,
                SlotState::Unread | SlotState::Read(_) => Vec::new(),
            };
            // The slot is committed before any resolve starts: one that asks
            // the handle again finds the sandbox read, or asks again.
            let mut failure = FirstPanic::new();
            start_each(waiting, sandbox.as_ref(), &mut failure);
            failure.resume();
        });
    }

    /// Ask `read` for the sandbox; the slot already holds the resolves that
    /// wait for it.
    ///
    /// A backend that panics asking leaves no read in flight: the slot goes
    /// back to unread, every resolve waiting is started with no sandbox (a
    /// typed refusal), and the panic resumes once that is done (ADR-0287),
    /// so the next resolve asks again.
    fn ask(self: &Rc<Self>, read: impl FnOnce() -> Completion<Sandbox>) {
        let mut failure = FirstPanic::new();
        if let Some(read) = failure.contain(read) {
            Self::fill_on(self, read);
            return;
        }
        let waiting = match self.state.take() {
            SlotState::Reading(waiting) => waiting,
            // A read the backend answered before it panicked stays.
            other => {
                self.state.set(other);
                Vec::new()
            }
        };
        start_each(waiting, None, &mut failure);
        failure.resume();
    }

    /// Run `start` with the sandbox now when it is read; else queue it under
    /// a ticket, asking `read` for the sandbox when no read is in flight.
    ///
    /// # Errors
    ///
    /// [`TooManyWaiting`] when [`MAX_WAITING`](Self::MAX_WAITING) resolves
    /// wait already; `start` is dropped unrun.
    pub(super) fn when_read(
        self: &Rc<Self>,
        start: impl FnOnce(Option<Rc<Sandbox>>) + 'static,
        read: impl FnOnce() -> Completion<Sandbox>,
    ) -> Result<Option<u64>, TooManyWaiting> {
        match self.state.take() {
            SlotState::Read(sandbox) => {
                self.state.set(SlotState::Read(Rc::clone(&sandbox)));
                start(Some(sandbox));
                Ok(None)
            }
            SlotState::Reading(mut waiting) => {
                if waiting.len() >= Self::MAX_WAITING {
                    self.state.set(SlotState::Reading(waiting));
                    return Err(TooManyWaiting);
                }
                let ticket = self.ticket();
                waiting.push((ticket, Box::new(start)));
                self.state.set(SlotState::Reading(waiting));
                Ok(Some(ticket))
            }
            SlotState::Unread => {
                let ticket = self.ticket();
                self.state
                    .set(SlotState::Reading(vec![(ticket, Box::new(start))]));
                // The state is committed before the backend is asked.
                self.ask(read);
                Ok(Some(ticket))
            }
        }
    }

    /// Take the resolve waiting under `ticket` off the queue: its caller let
    /// go of it.
    pub(super) fn withdraw(&self, ticket: u64) {
        let withdrawn = match self.state.take() {
            SlotState::Reading(mut waiting) => {
                let index = waiting.iter().position(|(held, _)| *held == ticket);
                let withdrawn = index.map(|index| waiting.remove(index));
                self.state.set(SlotState::Reading(waiting));
                withdrawn
            }
            other => {
                self.state.set(other);
                None
            }
        };
        // Dropped once the slot is whole again: it holds the caller's
        // resolver, whose drop is the caller's to hear.
        drop(withdrawn);
    }

    fn ticket(&self) -> u64 {
        let ticket = self.next_ticket.get();
        self.next_ticket.set(
            ticket
                .checked_add(1)
                .expect("BUG: a handle asks fewer than u64::MAX resolves"),
        );
        ticket
    }
}

/// Start each of `waiting` with `sandbox`, every one even when an earlier
/// one panics; `failure` keeps the first panic for its caller to resume.
fn start_each(
    waiting: Vec<(u64, WhenRead)>,
    sandbox: Option<&Rc<Sandbox>>,
    failure: &mut FirstPanic,
) {
    for (_, start) in waiting {
        let sandbox = sandbox.cloned();
        failure.contain(move || start(sandbox));
    }
}

/// What a resolve holds while it runs: its place in the queue for the
/// sandbox, then the backend's hold once the backend was asked.
#[derive(Default)]
pub(super) struct ResolveState {
    hold: Cell<Option<ResolveHold>>,
    waiting: Cell<Option<(std::rc::Weak<SandboxSlot>, u64)>>,
}

impl ResolveState {
    /// Keep `hold` for as long as the caller holds the resolve.
    pub(super) fn hold(&self, hold: ResolveHold) {
        // The earlier value is none: a resolve asks its backend once.
        drop(self.hold.replace(Some(hold)));
    }

    /// The resolve waits in `slot`'s queue under `ticket`.
    pub(super) fn waits_in(&self, slot: &Rc<SandboxSlot>, ticket: u64) {
        self.waiting.set(Some((Rc::downgrade(slot), ticket)));
    }

    /// The resolve left the queue: its sandbox was read.
    pub(super) fn started(&self) {
        self.waiting.set(None);
    }
}

impl Drop for ResolveState {
    fn drop(&mut self) {
        if let Some((slot, ticket)) = self.waiting.take()
            && let Some(slot) = slot.upgrade()
        {
            slot.withdraw(ticket);
        }
    }
}

/// A resolve in flight, and the answer it will give.
///
/// Owner-thread (`!Send`). The resolve lasts while this value, or the
/// closure or future it was turned into, is held; dropping it delivers
/// nothing. What else the drop stops depends on where the resolve is: one
/// still waiting for the sandbox leaves the queue and never reaches the
/// backend; on Android the read stops before its next chunk and the
/// provider's calls are cancelled; on macOS and iOS a bookmark already
/// resolving finishes, but its security scope is not started; on Linux and
/// Windows the one `metadata` call finishes and its answer is dropped. The
/// answer settles at an owner turn, never inside the call that asked.
#[must_use = "dropping a resolve delivers nothing"]
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
