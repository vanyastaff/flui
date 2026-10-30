//! [`TimerHandle`] and [`Sleep`]: deadlines on the UI runtime's clock.
//!
//! A UI runtime's deadlines live beside its owner-local tasks, in the store
//! its [`OwnerFrame`](crate::OwnerFrame) owns, and read the clock its
//! scheduler reads: the platform clock in production, the testing kit's
//! virtual clock in a test. A widget reaches them through its
//! [`AsyncDriver`](crate::AsyncDriver) ([`AsyncDriver::timer`](crate::AsyncDriver::timer)).
//!
//! # Firing
//!
//! A [`Sleep`] registers its deadline and waker when it is polled before the
//! deadline. Every owner turn that polls tasks first fires the deadlines
//! already due, one at a time: the entry is removed and the borrow released
//! before its waker is woken, so a waker that panics loses no later entry.
//! The earliest armed deadline is the UI runtime's
//! [`next_deadline`](crate::OwnerFrame::next_deadline), which the host's
//! wait honours.
//!
//! # Teardown
//!
//! The store goes with the owner frame. Its wakers are dropped, never woken:
//! the tasks they would wake go with the same owner. A [`Sleep`] polled after
//! that stays pending and keeps no waker; dropping it does nothing.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::rc::{Rc, Weak};
use std::task::{Context, Poll, Waker};

use flui_foundation::MonotonicClock as _;
use web_time::{Duration, Instant};

use crate::frame_clock::ClockSource;

/// One armed deadline: its instant, then the [`Sleep`] it belongs to.
type DeadlineKey = (Instant, u64);

/// The deadlines of one UI runtime.
#[derive(Default)]
pub(crate) struct Deadlines {
    /// The clock the deadlines are read against: the scheduler's.
    clock: RefCell<ClockSource>,
    /// Each polled, pending [`Sleep`]'s waker, earliest first.
    armed: RefCell<BTreeMap<DeadlineKey, Waker>>,
    /// Set when the owner retires: nothing is armed after it.
    closed: Cell<bool>,
    /// The id the next [`Sleep`] gets.
    next_id: Cell<u64>,
}

impl Deadlines {
    /// Read time from `clock` from now on.
    pub(crate) fn set_clock(&self, clock: ClockSource) {
        *self.clock.borrow_mut() = clock;
    }

    /// The earliest armed deadline, if any.
    pub(crate) fn next_deadline(&self) -> Option<Instant> {
        self.armed
            .borrow()
            .keys()
            .next()
            .map(|(deadline, _)| *deadline)
    }

    /// Wake every armed deadline due by the clock's now, earliest first,
    /// removing each before it is woken. Returns how many were woken.
    pub(crate) fn fire_due(&self) -> usize {
        let now = self.clock.borrow().now();
        let mut fired = 0;
        loop {
            let due = {
                let mut armed = self.armed.borrow_mut();
                match armed.first_key_value() {
                    Some(((deadline, _), _)) if *deadline <= now => armed.pop_first(),
                    _ => None,
                }
            };
            let Some((_, waker)) = due else {
                return fired;
            };
            fired += 1;
            waker.wake();
        }
    }

    /// Close the deadlines as the owner retires: hand back every armed
    /// waker, unwoken, for the retirement to drop, and arm nothing again.
    pub(crate) fn close(&self) -> Vec<Waker> {
        self.closed.set(true);
        std::mem::take(&mut *self.armed.borrow_mut())
            .into_values()
            .collect()
    }

    fn mint_id(&self) -> u64 {
        let id = self.next_id.get();
        self.next_id.set(
            id.checked_add(1)
                .expect("BUG: a UI runtime made 2^64 sleeps"),
        );
        id
    }
}

/// A widget's handle to its UI runtime's deadlines: [`Sleep`] futures that
/// resolve on the UI runtime's clock.
///
/// Owner-local (`!Send`) and weak: it keeps nothing alive past the UI
/// runtime. Cheap to clone.
#[derive(Clone)]
pub struct TimerHandle {
    deadlines: Weak<Deadlines>,
    clock: ClockSource,
}

impl fmt::Debug for TimerHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TimerHandle").finish_non_exhaustive()
    }
}

impl TimerHandle {
    pub(crate) fn new(deadlines: &Rc<Deadlines>) -> Self {
        let clock = deadlines.clock.borrow().clone();
        Self {
            deadlines: Rc::downgrade(deadlines),
            clock,
        }
    }

    /// The UI runtime's clock reading now.
    #[must_use]
    pub fn now(&self) -> Instant {
        self.clock.now()
    }

    /// A future that resolves once `duration` has passed on the UI runtime's
    /// clock, counted from now.
    pub fn sleep(&self, duration: Duration) -> Sleep {
        self.sleep_until_or_never(self.now().checked_add(duration))
    }

    /// A future that resolves at `deadline` on the UI runtime's clock; at
    /// once when `deadline` has passed.
    pub fn sleep_until(&self, deadline: Instant) -> Sleep {
        self.sleep_until_or_never(Some(deadline))
    }

    /// `None` is a deadline past what the clock can represent.
    fn sleep_until_or_never(&self, deadline: Option<Instant>) -> Sleep {
        let id = self.deadlines.upgrade().map_or(0, |store| store.mint_id());
        Sleep {
            deadlines: Weak::clone(&self.deadlines),
            clock: self.clock.clone(),
            key: deadline.map(|deadline| (deadline, id)),
        }
    }
}

/// Resolves at a deadline on the UI runtime's clock; see
/// [`TimerHandle::sleep`].
///
/// Polled only on the owner thread. Dropping it disarms its deadline. Once
/// the UI runtime is gone it never resolves and keeps no waker.
#[must_use = "a future does nothing unless polled"]
pub struct Sleep {
    deadlines: Weak<Deadlines>,
    clock: ClockSource,
    /// `None` for a deadline the clock never reaches.
    key: Option<DeadlineKey>,
}

impl fmt::Debug for Sleep {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Sleep")
            .field("deadline", &self.key.map(|(deadline, _)| deadline))
            .finish_non_exhaustive()
    }
}

impl Future for Sleep {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let Some(store) = self.deadlines.upgrade() else {
            return Poll::Pending;
        };
        let (Some(key), false) = (self.key, store.closed.get()) else {
            return Poll::Pending;
        };
        let due = self.clock.now() >= key.0;
        // A replaced or removed waker's drop is foreign code: it runs after
        // the borrow is released.
        let released = {
            let mut armed = store.armed.borrow_mut();
            if due {
                armed.remove(&key)
            } else {
                match armed.get_mut(&key) {
                    Some(known) if known.will_wake(cx.waker()) => None,
                    Some(known) => Some(std::mem::replace(known, cx.waker().clone())),
                    None => armed.insert(key, cx.waker().clone()),
                }
            }
        };
        drop(released);
        if due { Poll::Ready(()) } else { Poll::Pending }
    }
}

impl Drop for Sleep {
    fn drop(&mut self) {
        if let (Some(store), Some(key)) = (self.deadlines.upgrade(), self.key) {
            let removed = store.armed.borrow_mut().remove(&key);
            // A waker's drop is foreign code: no borrow is held.
            drop(removed);
        }
    }
}
