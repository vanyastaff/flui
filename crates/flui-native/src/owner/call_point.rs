//! The one point where the platform calls a handler: the application's
//! handler (`app_delivery`), a window's (`window_delivery`), or a callback a
//! host keeps (ADR-0151 §10).
//!
//! A handler waits between calls in a [`HandlerSlot`], wherever its host keeps
//! it: a field of a state mutex, a `RefCell` the owner thread borrows, a
//! window's own `Mutex<Option<_>>`. [`call`] takes it out under the slot's
//! lock or borrow, releases them, runs it inside a panic boundary, and then
//! puts it back, unless a handler was installed while it ran: the newer one
//! stays and the one that ran is retired. Every caller holds no lock or borrow
//! of its own across the call, so a handler is never called, and a handler's
//! captures never dropped, while the platform holds one: a handler may install
//! another, quit, or open a window.
//! [`install`] replaces a handler and retires the one it displaces the same
//! way, and [`retire`] drops any other user-owned value outside the locks.
//!
//! A handler's panic is the operation's first failure: the slot is settled
//! without running user code a second time (a handler displaced during a call
//! that failed is retained, not dropped, ADR-0127 §2), and the payload comes
//! back as a [`Failure`] for the caller to resume or contain. Nothing here
//! reads `std::thread::panicking()` to learn about that failure: it is false
//! once the panic is caught.

use std::any::Any;
use std::cell::RefCell;
use std::mem::ManuallyDrop;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};

use parking_lot::Mutex;

use super::panic_boundary::contain_owner_callback;

/// Where a handler waits between calls.
///
/// Every method takes and releases the slot's own lock or borrow before it
/// returns and runs no user code: what a method displaces or refuses it hands
/// back, for the caller to retire outside the lock.
pub(crate) trait HandlerSlot {
    /// The stored handler.
    type Handler;

    /// Take the handler out for one call; `None` when the slot is empty.
    fn take(&self) -> Option<Self::Handler>;

    /// Put a handler back after its call. Hands it back when a handler was
    /// installed while it was out, or when the slot takes no handler any
    /// more.
    fn restore(&self, handler: Self::Handler) -> Option<Self::Handler>;

    /// Install `handler`, handing back the one it displaces, or `handler`
    /// itself when the slot refuses it.
    fn replace(&self, handler: Self::Handler) -> Option<Self::Handler>;
}

/// How one [`call`] ended.
#[must_use = "a handler's panic is the operation's first failure: resume or contain it"]
pub(crate) enum Called<R> {
    /// The slot held no handler; nothing ran.
    Vacant,
    /// The handler returned.
    Returned(R),
    /// The handler panicked. Its slot is settled.
    Panicked(Failure),
}

impl<R> Called<R> {
    /// What the handler returned, or `vacant` when none ran. A panic resumes
    /// unwinding here, with the handler's own payload.
    pub(crate) fn or_resume(self, vacant: R) -> R {
        match self {
            Self::Vacant => vacant,
            Self::Returned(value) => value,
            Self::Panicked(failure) => failure.resume(),
        }
    }

    /// What the handler returned, or `fallback` when none ran or it panicked,
    /// for a caller with no frame to unwind into (an OS callback): the panic
    /// is logged under `handler` and its payload retained.
    #[cfg(any(
        target_os = "windows",
        target_os = "macos",
        target_os = "ios",
        target_arch = "wasm32",
        all(target_os = "linux", feature = "x11"),
        all(test, not(target_arch = "wasm32"))
    ))]
    pub(crate) fn or_contain(self, fallback: R, handler: &'static str) -> R {
        match self {
            Self::Vacant => fallback,
            Self::Returned(value) => value,
            Self::Panicked(failure) => {
                failure.contain(handler);
                fallback
            }
        }
    }
}

/// A handler's panic, caught by [`call`].
///
/// Dropping it retains the payload rather than destroying it: an opaque
/// payload's destructor is user code too (ADR-0119).
pub(crate) struct Failure(ManuallyDrop<Box<dyn Any + Send>>);

impl Failure {
    /// Resume the handler's unwind with its own payload.
    pub(crate) fn resume(self) -> ! {
        let Self(payload) = self;
        resume_unwind(ManuallyDrop::into_inner(payload))
    }

    /// Log the panic under `handler` and retain its payload.
    pub(crate) fn contain(self, handler: &'static str) {
        let Self(payload) = self;
        let payload = ManuallyDrop::into_inner(payload);
        contain_owner_callback(|| {
            tracing::error!(
                handler,
                panic = flui_foundation::panic::payload_text(&*payload)
                    .unwrap_or("non-string panic payload"),
                "a handler panicked; the platform carries on without its answer"
            );
        });
        flui_foundation::panic::discard_payload(payload);
    }
}

impl std::fmt::Debug for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Failure")
            .field(&flui_foundation::panic::payload_text(&**self.0))
            .finish()
    }
}

/// Call the handler waiting in `slot` with `body`, holding no lock or borrow
/// of the slot while it runs.
///
/// Afterwards the handler goes back to its slot, unless one was installed
/// while it ran (by the handler itself or by anything it called): the newer
/// one stays, and the one that ran is dropped here, its destructor contained.
/// When the handler panicked, that displaced handler is retained instead, and
/// the payload is returned as the call's [`Failure`]. Called while the thread
/// is already unwinding, nothing is dropped.
pub(crate) fn call<S, R>(slot: &S, body: impl FnOnce(&mut S::Handler) -> R) -> Called<R>
where
    S: HandlerSlot + ?Sized,
{
    let Some(mut handler) = slot.take() else {
        return Called::Vacant;
    };
    let outcome = catch_unwind(AssertUnwindSafe(|| body(&mut handler)));
    let displaced = slot.restore(handler);
    match outcome {
        Ok(value) => {
            retire(displaced);
            Called::Returned(value)
        }
        Err(payload) => {
            // The operation already failed: the displaced handler's captures
            // stay out of it (ADR-0127 §2).
            std::mem::forget(displaced);
            Called::Panicked(Failure(ManuallyDrop::new(payload)))
        }
    }
}

/// Install `handler` in `slot`; the handler it displaces, or `handler`
/// itself when the slot refuses it, is retired outside the slot's lock.
pub(crate) fn install<S>(slot: &S, handler: S::Handler)
where
    S: HandlerSlot + ?Sized,
{
    retire(slot.replace(handler));
}

/// Drop `value`, whose destructor is user code, outside every lock: its
/// panic is contained, and while the thread is already unwinding the value is
/// retained instead (ADR-0127 §2). The caller holds no lock or borrow.
pub(crate) fn retire<T>(value: T) {
    if std::thread::panicking() {
        std::mem::forget(value);
    } else {
        contain_owner_callback(|| drop(value));
    }
}

/// State behind a lock or a `RefCell`, reached for the length of one closure:
/// the guard or borrow ends before [`Guarded::with`] returns.
pub(crate) trait Guarded {
    /// What the lock or cell holds.
    type Target: ?Sized;

    /// Run `body` on the state with the lock held or the cell borrowed.
    fn with<R>(&self, body: impl FnOnce(&mut Self::Target) -> R) -> R;
}

impl<T: ?Sized> Guarded for Mutex<T> {
    type Target = T;

    fn with<R>(&self, body: impl FnOnce(&mut T) -> R) -> R {
        body(&mut self.lock())
    }
}

impl<T: ?Sized> Guarded for RefCell<T> {
    type Target = T;

    fn with<R>(&self, body: impl FnOnce(&mut T) -> R) -> R {
        body(&mut self.borrow_mut())
    }
}

impl<S: HandlerSlot + ?Sized> HandlerSlot for &S {
    type Handler = S::Handler;

    fn take(&self) -> Option<S::Handler> {
        (**self).take()
    }

    fn restore(&self, handler: S::Handler) -> Option<S::Handler> {
        (**self).restore(handler)
    }

    fn replace(&self, handler: S::Handler) -> Option<S::Handler> {
        (**self).replace(handler)
    }
}

/// A handler slot of its own, as a window keeps each of its callbacks.
impl<H> HandlerSlot for Mutex<Option<H>> {
    type Handler = H;

    fn take(&self) -> Option<H> {
        self.lock().take()
    }

    fn restore(&self, handler: H) -> Option<H> {
        let mut slot = self.lock();
        if slot.is_none() {
            *slot = Some(handler);
            None
        } else {
            Some(handler)
        }
    }

    fn replace(&self, handler: H) -> Option<H> {
        self.lock().replace(handler)
    }
}

/// A handler slot that is one field of state behind a lock or a `RefCell`.
///
/// A handler goes back only while `accepts` answers `true` (asked under the
/// lock), so a host that stopped can refuse the handler a call returns.
pub(crate) struct Field<'a, G: ?Sized + Guarded, H, A = fn() -> bool> {
    state: &'a G,
    project: fn(&mut G::Target) -> &mut Option<H>,
    accepts: A,
}

impl<'a, G: ?Sized + Guarded, H> Field<'a, G, H> {
    /// The field of `state` that `project` names.
    pub(crate) fn new(state: &'a G, project: fn(&mut G::Target) -> &mut Option<H>) -> Self {
        Self {
            state,
            project,
            accepts: || true,
        }
    }

    /// The same field, taking a handler back only while `accepts` answers
    /// `true`.
    #[cfg(any(target_os = "ios", all(test, not(target_arch = "wasm32"))))]
    pub(crate) fn restoring_while<A: Fn() -> bool>(self, accepts: A) -> Field<'a, G, H, A> {
        Field {
            state: self.state,
            project: self.project,
            accepts,
        }
    }
}

impl<G: ?Sized + Guarded, H, A: Fn() -> bool> HandlerSlot for Field<'_, G, H, A> {
    type Handler = H;

    fn take(&self) -> Option<H> {
        self.state.with(|state| (self.project)(state).take())
    }

    fn restore(&self, handler: H) -> Option<H> {
        self.state.with(|state| {
            let slot = (self.project)(state);
            if slot.is_none() && (self.accepts)() {
                *slot = Some(handler);
                None
            } else {
                Some(handler)
            }
        })
    }

    fn replace(&self, handler: H) -> Option<H> {
        self.state
            .with(|state| (self.project)(state).replace(handler))
    }
}

// A panic on wasm32 aborts instead of unwinding, so the failure cases have
// nothing to catch there.
#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;
