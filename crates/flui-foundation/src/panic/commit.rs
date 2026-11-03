//! Commit an owner's state first, then run the user code the change owes
//! (ADR-0287).
//!
//! [`commit`] hands a mutation the state and an [`Outbox`], never the cell:
//! user code the change owes (a listener, a waker, the destructor of a
//! replaced value) is queued while the state is borrowed and runs once the
//! borrow is released, each item contained, the first failure resumed after
//! every item ran. [`Obligation`] keeps an owed delivery owed until it
//! completes.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::task::Waker;

use smallvec::SmallVec;

use super::{FirstPanic, PanicRecovery};

/// One piece of user code a mutation queued.
enum Deferred {
    Call(Box<dyn FnMut()>),
    Wake(Waker),
    Release(Box<dyn Any>),
}

/// The user code a mutation owes, queued while its state is borrowed.
///
/// Only [`commit`] and [`commit_into`] make one, and they run it after the
/// borrow is released: in queue order, each item contained. A call that
/// commits again runs its own queue before the rest of this one.
pub struct Outbox {
    queue: SmallVec<[Deferred; 4]>,
}

impl std::fmt::Debug for Outbox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Outbox")
            .field("queued", &self.queue.len())
            .finish()
    }
}

impl Outbox {
    /// Run `call` once the state is released. It does not run when the
    /// mutation that queued it panics: it is destroyed instead.
    ///
    /// `call` is an `FnMut` called at most once, so its captures outlive the
    /// call: they are destroyed after it, contained, a panic included, never
    /// during its unwind (ADR-0281 §3).
    pub fn call(&mut self, call: impl FnMut() + 'static) {
        self.queue.push(Deferred::Call(Box::new(call)));
    }

    /// Wake `waker` by value once the state is released, after a mutation
    /// that panicked too: a spurious wake costs a poll, a lost one hangs a
    /// task.
    pub fn wake(&mut self, waker: Waker) {
        self.queue.push(Deferred::Wake(waker));
    }

    /// Destroy `value` once the state is released: its destructor is user
    /// code (a replaced listener, a withdrawn capture).
    pub fn release<T: 'static>(&mut self, value: T) {
        self.queue.push(Deferred::Release(Box::new(value)));
    }

    /// Run the queue in order, each item contained by `sink`. After a failed
    /// mutation only the wakes run; the rest is destroyed.
    fn deliver(self, committed: bool, sink: &mut impl Contain) {
        for item in self.queue {
            match item {
                Deferred::Call(mut call) => {
                    if committed {
                        sink.contain_unit(&mut call);
                    }
                    sink.contain_unit(move || drop(call));
                }
                Deferred::Wake(waker) => sink.contain_unit(move || waker.wake()),
                Deferred::Release(value) => sink.contain_unit(move || drop(value)),
            }
        }
    }
}

/// Where a commit's failures go.
trait Contain {
    fn contain_unit(&mut self, run: impl FnOnce());
}

impl Contain for FirstPanic {
    fn contain_unit(&mut self, run: impl FnOnce()) {
        let _: Option<()> = self.contain(run);
    }
}

impl Contain for PanicRecovery {
    fn contain_unit(&mut self, run: impl FnOnce()) {
        self.run(run);
    }
}

/// Borrow `cell` for `mutate`, then deliver what it queued. `None` when
/// `mutate` panicked; its panic is in `sink`.
fn commit_with<S, R>(
    cell: &RefCell<S>,
    sink: &mut impl Contain,
    mutate: impl FnOnce(&mut S, &mut Outbox) -> R,
) -> Option<R> {
    let mut outbox = Outbox {
        queue: SmallVec::new(),
    };
    let mut committed = None;
    sink.contain_unit(|| {
        let mut state = cell
            .try_borrow_mut()
            .expect("BUG: a commit reached a cell that is already borrowed");
        committed = Some(mutate(&mut state, &mut outbox));
    });
    outbox.deliver(committed.is_some(), sink);
    committed
}

/// Change the state in `cell`, then run the user code the change queued.
///
/// `mutate` gets the state and an [`Outbox`]; the borrow ends when it
/// returns, and the queue runs after that, in order, each item contained.
/// The first panic, from `mutate` or from a queued item, is resumed once
/// every item ran (ADR-0287 §1, §2).
///
/// # Panics
///
/// With `BUG:` when `cell` is already borrowed: a commit inside a mutation
/// of the same cell. Resumes the first panic of `mutate` or of a queued item.
/// While the thread is already unwinding, a panic of a queued item is
/// discarded behind that unwind; one of `mutate` aborts, as a panic out of
/// any destructor does then (ADR-0281 §1).
pub fn commit<S, R>(cell: &RefCell<S>, mutate: impl FnOnce(&mut S, &mut Outbox) -> R) -> R {
    let mut first = FirstPanic::new();
    match commit_with(cell, &mut first, mutate) {
        Some(value) => {
            first.resume();
            value
        }
        None => std::panic::resume_unwind(
            first
                .into_payload()
                .expect("BUG: a mutation that did not return holds its panic"),
        ),
    }
}

/// [`commit`] inside an operation that already owns a failure context: each
/// failure goes into `recovery`, behind any it already holds. `None` when
/// `mutate` panicked (ADR-0287 §3).
pub fn commit_into<S, R>(
    cell: &RefCell<S>,
    recovery: &mut PanicRecovery,
    mutate: impl FnOnce(&mut S, &mut Outbox) -> R,
) -> Option<R> {
    commit_with(cell, recovery, mutate)
}

/// A claimed, owed delivery: the flag it was taken from stays cleared only
/// if the delivery completes (ADR-0287 §4).
///
/// Dropped without [`discharge`](Self::discharge), after a panic or an early
/// return, it sets the flag again, so the next turn delivers again. The flag
/// is cleared while the delivery runs, so an obligation raised again during
/// it stays raised after `discharge`.
#[must_use = "an obligation dropped without `discharge` is owed again"]
#[derive(Debug)]
pub struct Obligation<'a> {
    flag: &'a Cell<bool>,
    discharged: bool,
}

impl<'a> Obligation<'a> {
    /// Claim the obligation `flag` records: `None` when nothing is owed.
    #[must_use = "an obligation dropped at once is owed again"]
    pub fn take(flag: &'a Cell<bool>) -> Option<Self> {
        flag.replace(false).then_some(Self {
            flag,
            discharged: false,
        })
    }

    /// The delivery completed: the obligation ends, and the flag keeps
    /// whatever was raised since it was claimed.
    pub fn discharge(mut self) {
        self.discharged = true;
    }
}

impl Drop for Obligation<'_> {
    fn drop(&mut self) {
        if !self.discharged {
            self.flag.set(true);
        }
    }
}
