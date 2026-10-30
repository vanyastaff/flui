//! `Completion<T>`: the one owner-thread result type for what the operating
//! system finishes later (ADR-0152 §2, item 5).

use std::{
    cell::{Cell, RefCell},
    fmt,
    future::Future,
    panic::{AssertUnwindSafe, catch_unwind, resume_unwind},
    pin::Pin,
    rc::Rc,
    sync::Arc,
    task::{Context, Poll, Waker},
};

use parking_lot::Mutex;

use super::owner::{OwnerJob, OwnerPoster, OwnerRetired, PendingCompletions, PendingSlot};

/// Why a [`Completion`] ended without a value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, thiserror::Error)]
#[non_exhaustive]
pub enum Abandoned {
    /// The provider dropped its [`Resolver`] without resolving it.
    #[error("the provider dropped the completion without answering")]
    ProviderDropped,
    /// The owner retired before the answer arrived.
    #[error("the owner retired before the answer arrived")]
    OwnerRetired,
}

type Outcome<T> = Result<T, Abandoned>;
type Shared<T> = Arc<Mutex<Option<Outcome<T>>>>;
// `FnMut`, called at most once: a panic in it leaves its captures in the box,
// which is then kept rather than destroyed mid-unwind, where a capture whose
// `Drop` panics too would abort the process.
type Waiting<T> = Box<dyn FnMut(Outcome<T>)>;

/// The owner side of one completion.
struct Local<T> {
    /// Where the resolving side leaves the outcome; read only by the settle
    /// job, on the owner thread.
    shared: Shared<T>,
    /// The outcome once it settled at an owner turn, until it is taken.
    settled: RefCell<Option<Outcome<T>>>,
    /// Whether the outcome was handed out; a completion settles once.
    taken: Cell<bool>,
    on_complete: RefCell<Option<Waiting<T>>>,
    waker: RefCell<Option<Waker>>,
}

/// The owner-local half a settle job reaches through the owner's
/// [`PendingCompletions`].
pub(super) trait Settle {
    /// The outcome arrived: run the waiting closure, or keep the outcome for
    /// `poll`. Runs at an owner turn with no borrow of the table held.
    fn settle(self: Rc<Self>);
    /// The owner retired first: drop the waiting closure unrun.
    fn abandon_on_retire(self: Rc<Self>);
}

impl<T: 'static> Settle for Local<T> {
    fn settle(self: Rc<Self>) {
        let outcome = self
            .settled
            .borrow_mut()
            .take()
            .or_else(|| self.shared.lock().take());
        let Some(outcome) = outcome else { return };
        let on_complete = self.on_complete.borrow_mut().take();
        if let Some(mut on_complete) = on_complete {
            self.taken.set(true);
            if let Err(payload) = catch_unwind(AssertUnwindSafe(|| on_complete(outcome))) {
                // Kept, not destroyed during this unwind (see `Waiting`).
                std::mem::forget(on_complete);
                resume_unwind(payload);
            }
            drop(on_complete);
        } else {
            *self.settled.borrow_mut() = Some(outcome);
            let waker = self.waker.borrow_mut().take();
            if let Some(waker) = waker {
                waker.wake();
            }
        }
    }

    fn abandon_on_retire(self: Rc<Self>) {
        let on_complete = self.on_complete.borrow_mut().take();
        drop(on_complete);
        let waker = self.waker.borrow_mut().take();
        if let Some(waker) = waker {
            waker.wake();
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SlotId {
    index: u32,
    generation: u32,
}

impl PendingCompletions {
    fn insert(&self, settle: Rc<dyn Settle>) -> Option<SlotId> {
        let mut state = self.inner.borrow_mut();
        if state.retired {
            return None;
        }
        if let Some(index) = state.free.pop() {
            let slot = &mut state.slots[index as usize];
            slot.generation = slot.generation.wrapping_add(1);
            slot.settle = Some(settle);
            return Some(SlotId {
                index,
                generation: slot.generation,
            });
        }
        let index = u32::try_from(state.slots.len())
            .expect("BUG: fewer than u32::MAX completions are pending at once");
        state.slots.push(PendingSlot {
            generation: 0,
            settle: Some(settle),
        });
        Some(SlotId {
            index,
            generation: 0,
        })
    }

    fn take(&self, id: SlotId) -> Option<Rc<dyn Settle>> {
        let mut state = self.inner.borrow_mut();
        let slot = state.slots.get_mut(id.index as usize)?;
        if slot.generation != id.generation {
            return None;
        }
        let settle = slot.settle.take()?;
        state.free.push(id.index);
        Some(settle)
    }
}

/// Where an owner's [`Completion`]s are made: its poster and its
/// [`PendingCompletions`]. Owner-local; a handle keeps one to answer each
/// call it finishes later.
#[derive(Clone, Debug)]
pub struct CompletionSource {
    poster: OwnerPoster,
    pending: PendingCompletions,
}

impl CompletionSource {
    /// Completions settled through `poster` and kept in `pending` until then.
    #[must_use]
    pub fn new(poster: OwnerPoster, pending: PendingCompletions) -> Self {
        Self { poster, pending }
    }

    /// The owner's poster.
    #[must_use]
    pub fn poster(&self) -> &OwnerPoster {
        &self.poster
    }

    /// A pending completion and the resolver that answers it.
    ///
    /// The resolver may move to any thread; the completion settles at the
    /// owner's next turn after [`Resolver::resolve`], never inline. Once the
    /// owner has retired, the completion is born
    /// [`OwnerRetired`](Abandoned::OwnerRetired).
    pub fn pending<T: Send + 'static>(&self) -> (Completion<T>, Resolver<T>) {
        let shared: Shared<T> = Arc::new(Mutex::new(None));
        let local = Rc::new(Local {
            shared: Arc::clone(&shared),
            settled: RefCell::new(None),
            taken: Cell::new(false),
            on_complete: RefCell::new(None),
            waker: RefCell::new(None),
        });
        let id = self.pending.insert(Rc::clone(&local) as Rc<dyn Settle>);
        let completion = Completion {
            inner: Some(Inner {
                local,
                id,
                source: self.clone(),
            }),
        };
        let resolver = Resolver {
            shared,
            id,
            poster: self.poster.clone(),
            done: false,
        };
        (completion, resolver)
    }
}

struct Inner<T> {
    local: Rc<Local<T>>,
    /// `None` when the owner had already retired at creation.
    id: Option<SlotId>,
    source: CompletionSource,
}

impl<T: 'static> Inner<T> {
    fn retired(&self) -> bool {
        self.id.is_none() || self.source.pending.is_retired()
    }

    /// The outcome, once: the settled value, or `OwnerRetired` when the owner
    /// retired before it settled.
    fn take(&self) -> Option<Outcome<T>> {
        if self.local.taken.get() {
            return None;
        }
        let outcome = self
            .local
            .settled
            .borrow_mut()
            .take()
            .or_else(|| self.retired().then_some(Err(Abandoned::OwnerRetired)));
        if outcome.is_some() {
            self.local.taken.set(true);
        }
        outcome
    }
}

/// The result of an operation the operating system finishes later.
///
/// Owner-thread only (`!Send`). It settles at an owner turn boundary: never
/// inline in the call that created it, never on the thread the operating
/// system called back on, and never with a borrow of the owner's state held.
/// Consume it with [`on_complete`](Self::on_complete) or
/// [`poll`](Self::poll); [`into_future`](Self::into_future) adapts it for
/// code outside the frame path.
///
/// It always ends: with the value, with
/// [`ProviderDropped`](Abandoned::ProviderDropped) when the provider drops its
/// resolver unanswered, or with [`OwnerRetired`](Abandoned::OwnerRetired)
/// when the owner retires first. At retirement a closure still waiting is
/// dropped unrun, since there is no owner left to run it; `poll` and the
/// future answer `OwnerRetired`.
#[must_use = "a completion delivers its result only to on_complete, poll or its future"]
pub struct Completion<T: 'static> {
    inner: Option<Inner<T>>,
}

impl<T: 'static> Completion<T> {
    /// Runs `f` once, with the outcome, at an owner turn boundary.
    ///
    /// `f` is an `FnMut` called at most once: if it panics, its captures are
    /// kept rather than dropped during the unwind, so a capture whose `Drop`
    /// also panics cannot abort the process.
    ///
    /// When the outcome has already settled, `f` runs at the owner's next
    /// turn, not inside this call.
    pub fn on_complete(mut self, f: impl FnMut(Result<T, Abandoned>) + 'static) {
        let Some(inner) = self.inner.take() else {
            return;
        };
        if inner.local.taken.get() {
            return;
        }
        if inner.retired() {
            let ready = inner.local.settled.borrow().is_some();
            if !ready {
                // No owner is left to run `f`.
                return;
            }
        }
        *inner.local.on_complete.borrow_mut() = Some(Box::new(f));
        if inner.local.settled.borrow().is_some() {
            // Settled before anyone waited: deliver at the next turn.
            if let Some(id) = inner
                .source
                .pending
                .insert(Rc::clone(&inner.local) as Rc<dyn Settle>)
            {
                let posted = inner.source.poster.post_job(settle_job(id));
                if posted.is_err() {
                    drop(inner.source.pending.take(id));
                    drop(inner.local.on_complete.borrow_mut().take());
                }
            }
        }
    }

    /// Takes the outcome if it has settled: `None` while it is pending, and
    /// after it was taken.
    pub fn poll(&mut self) -> Option<Result<T, Abandoned>> {
        self.inner.as_ref().and_then(Inner::take)
    }

    /// The outcome as a future, for code outside the frame path. Polled on
    /// the owner thread; it is woken when the outcome settles.
    pub fn into_future(mut self) -> impl Future<Output = Result<T, Abandoned>> {
        CompletionFuture {
            inner: self.inner.take(),
        }
    }
}

impl<T: 'static> Drop for Completion<T> {
    fn drop(&mut self) {
        // Nobody waits: free the slot, so a later settle finds nothing.
        if let Some(Inner {
            id: Some(id),
            source,
            ..
        }) = self.inner.take()
        {
            drop(source.pending.take(id));
        }
    }
}

impl<T: 'static> fmt::Debug for Completion<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let state = match &self.inner {
            None => "consumed",
            Some(inner) if inner.local.taken.get() => "taken",
            Some(inner) if inner.local.settled.borrow().is_some() => "settled",
            Some(inner) if inner.retired() => "owner retired",
            Some(_) => "pending",
        };
        f.debug_struct("Completion").field("state", &state).finish()
    }
}

struct CompletionFuture<T: 'static> {
    inner: Option<Inner<T>>,
}

impl<T: 'static> Future for CompletionFuture<T> {
    type Output = Result<T, Abandoned>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let Some(inner) = self.inner.as_ref() else {
            return Poll::Pending;
        };
        if let Some(outcome) = inner.take() {
            return Poll::Ready(outcome);
        }
        // The replaced waker drops after the borrow ends: its `Drop` may re-enter
        // the owner and settle this completion.
        let replaced = inner.local.waker.replace(Some(cx.waker().clone()));
        drop(replaced);
        Poll::Pending
    }
}

impl<T: 'static> Drop for CompletionFuture<T> {
    fn drop(&mut self) {
        if let Some(Inner {
            id: Some(id),
            source,
            ..
        }) = self.inner.take()
        {
            drop(source.pending.take(id));
        }
    }
}

fn settle_job(id: SlotId) -> OwnerJob {
    OwnerJob::scoped(move |scope| {
        let settle = scope.completions().take(id);
        // The table's borrow ended above: the waiting closure may create
        // completions of its own.
        if let Some(settle) = settle {
            settle.settle();
        }
    })
}

/// The provider's half of a [`Completion`]: it answers it once, from any
/// thread when `T: Send`.
///
/// Dropping it unanswered ends the completion as
/// [`ProviderDropped`](Abandoned::ProviderDropped), so a completion never
/// waits on a provider that has gone.
#[must_use = "a dropped resolver ends its completion as ProviderDropped"]
pub struct Resolver<T> {
    shared: Shared<T>,
    id: Option<SlotId>,
    poster: OwnerPoster,
    done: bool,
}

impl<T> Resolver<T> {
    fn deliver(&mut self, outcome: Outcome<T>) -> Result<(), OwnerRetired> {
        self.done = true;
        *self.shared.lock() = Some(outcome);
        match self.id {
            Some(id) => self.poster.post_job(settle_job(id)),
            None => Err(OwnerRetired),
        }
    }
}

impl<T: Send + 'static> Resolver<T> {
    /// Answers the completion with `value`; it settles at the owner's next
    /// turn.
    ///
    /// # Errors
    ///
    /// [`OwnerRetired`] when the owner has retired: nothing will receive the
    /// value, and an operating-system subscription that answers here should
    /// end.
    pub fn resolve(mut self, value: T) -> Result<(), OwnerRetired> {
        self.deliver(Ok(value))
    }
}

impl<T> Drop for Resolver<T> {
    fn drop(&mut self) {
        if !self.done {
            // The owner may have retired too; then nothing is waiting.
            let _ = self.deliver(Err(Abandoned::ProviderDropped));
        }
    }
}

impl<T> fmt::Debug for Resolver<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Resolver")
            .field("answered", &self.done)
            .finish_non_exhaustive()
    }
}
