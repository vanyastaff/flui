//! `TimerHandle` and `Sleep`: deadlines on the UI runtime's clock, fired by
//! the owner turn that polls tasks.

use std::cell::{Cell, RefCell};
use std::future::Future;
use std::pin::pin;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll, RawWaker, RawWakerVTable, Wake, Waker};
use std::time::Duration;

use flui_foundation::ManualClock;
use flui_scheduler::{ClockSource, OwnerFrame, SchedulerBuilder, UpdateScheduler};

/// A scheduler on a virtual clock, and its owner frame.
fn on_virtual_clock() -> (ManualClock, UpdateScheduler, OwnerFrame) {
    let clock = ManualClock::new();
    let scheduler = SchedulerBuilder::new()
        .clock(ClockSource::Manual(clock.clone()))
        .build();
    let owner = OwnerFrame::try_new(&scheduler).expect("fresh owner");
    (clock, scheduler, owner)
}

/// A task that sleeps `after` and then counts its completion.
fn sleeper(owner: &OwnerFrame, after: Duration) -> (flui_scheduler::TaskToken, Rc<Cell<bool>>) {
    let done = Rc::new(Cell::new(false));
    let finished = Rc::clone(&done);
    let sleep = owner.async_driver().timer().sleep(after);
    let token = owner.async_driver().spawn_local(Box::pin(async move {
        sleep.await;
        finished.set(true);
    }));
    (token, done)
}

/// A sleep resolves at its deadline on the runtime's clock, not before: the
/// turn that finds it due fires it and polls its task in the same batch, and
/// the host is told the deadline to wake at meanwhile.
#[test]
fn a_sleep_resolves_at_its_deadline_on_the_runtime_clock() {
    let (clock, _scheduler, owner) = on_virtual_clock();
    let start = owner.async_driver().timer().now();
    let (_token, done) = sleeper(&owner, Duration::from_millis(50));
    assert_eq!(
        owner.pump_background(|| {}),
        Ok(1),
        "the first poll arms it"
    );
    assert_eq!(
        owner.next_deadline(),
        Some(start + Duration::from_millis(50)),
        "the host wakes at the deadline"
    );

    clock.advance(Duration::from_millis(49));
    assert_eq!(owner.pump_background(|| {}), Ok(0), "49 ms: nothing is due");
    assert!(!done.get());

    clock.advance(Duration::from_millis(1));
    assert_eq!(
        owner.pump_background(|| {}),
        Ok(1),
        "50 ms: fired and polled"
    );
    assert!(done.get());
    assert_eq!(owner.next_deadline(), None);
}

/// The earliest armed deadline is the one reported; a dropped sleep disarms
/// its own and leaves the others.
#[test]
fn a_dropped_sleep_disarms_only_its_deadline() {
    let (_clock, _scheduler, owner) = on_virtual_clock();
    let start = owner.async_driver().timer().now();
    let (near, _) = sleeper(&owner, Duration::from_millis(10));
    let (_far, _) = sleeper(&owner, Duration::from_millis(30));
    assert_eq!(owner.pump_background(|| {}), Ok(2));
    assert_eq!(
        owner.next_deadline(),
        Some(start + Duration::from_millis(10))
    );
    drop(near);
    assert_eq!(
        owner.next_deadline(),
        Some(start + Duration::from_millis(30))
    );
}

/// A waker that counts its wakes; its `Arc` counts its live clones.
#[derive(Default)]
struct Counted(AtomicUsize);

impl Wake for Counted {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

/// A sleep outliving its owner never resolves and keeps no waker: the
/// owner's teardown drops the wakers it held without waking them.
#[test]
fn a_sleep_outliving_its_owner_stays_pending_without_a_waker() {
    let (_clock, scheduler, owner) = on_virtual_clock();
    let timer = owner.async_driver().timer();
    let counted = Arc::new(Counted::default());
    let waker = Waker::from(Arc::clone(&counted));
    let mut cx = Context::from_waker(&waker);
    let mut sleep = pin!(timer.sleep(Duration::from_millis(5)));
    assert_eq!(sleep.as_mut().poll(&mut cx), Poll::Pending);
    assert_eq!(Arc::strong_count(&counted), 3, "the deadline holds a waker");

    assert!(owner.retire().is_none(), "retirement runs no failing code");
    assert_eq!(Arc::strong_count(&counted), 2, "retirement dropped it");
    drop((owner, scheduler));
    assert_eq!(
        Arc::strong_count(&counted),
        2,
        "dropping the owner wakes nothing"
    );
    assert_eq!(counted.0.load(Ordering::SeqCst), 0, "without waking it");
    assert_eq!(sleep.as_mut().poll(&mut cx), Poll::Pending);
    assert_eq!(
        Arc::strong_count(&counted),
        2,
        "a poll after teardown keeps no waker"
    );
    assert_eq!(
        pin!(timer.sleep(Duration::ZERO)).poll(&mut cx),
        Poll::Pending,
        "a sleep made after teardown never resolves"
    );
}

thread_local! {
    /// What the next clone of a [`cloning_waker`] runs.
    static ON_WAKER_CLONE: RefCell<Option<Box<dyn FnOnce()>>> = const { RefCell::new(None) };
}

#[expect(
    unsafe_code,
    reason = "exercise an executor's clone through the public Future API"
)]
fn cloning_waker() -> Waker {
    unsafe fn clone(data: *const ()) -> RawWaker {
        if let Some(hook) = ON_WAKER_CLONE.with(|hook| hook.borrow_mut().take()) {
            hook();
        }
        RawWaker::new(data, &VTABLE)
    }
    unsafe fn noop(_: *const ()) {}
    // The data pointer is a null constant that nothing dereferences, so
    // clones and drops have nothing to count.
    const VTABLE: RawWakerVTable = RawWakerVTable::new(clone, noop, noop, noop);
    // SAFETY: no vtable function dereferences `data`, and each is safe to call
    // from any thread any number of times.
    unsafe { Waker::from_raw(RawWaker::new(std::ptr::null(), &VTABLE)) }
}

/// An executor's waker clone is foreign code: one that reads the deadlines
/// and drops another sleep while the poll registers its own finds the store
/// free, and the registration still lands.
#[test]
fn a_waker_clone_that_reenters_the_deadlines_still_arms_the_sleep() {
    let (_clock, _scheduler, owner) = on_virtual_clock();
    let owner = Rc::new(owner);
    let timer = owner.async_driver().timer();
    let start = timer.now();
    let mut other = Box::pin(timer.sleep(Duration::from_millis(5)));
    let noop = Waker::noop();
    assert_eq!(
        other.as_mut().poll(&mut Context::from_waker(noop)),
        Poll::Pending
    );

    let seen = Rc::new(Cell::new(None));
    let (reader, seen_by_clone) = (Rc::clone(&owner), Rc::clone(&seen));
    let mut other = Some(other);
    ON_WAKER_CLONE.with(|hook| {
        *hook.borrow_mut() = Some(Box::new(move || {
            seen_by_clone.set(reader.next_deadline());
            drop(other.take());
        }));
    });
    let waker = cloning_waker();
    let mut sleep = pin!(timer.sleep(Duration::from_millis(20)));
    assert_eq!(
        sleep.as_mut().poll(&mut Context::from_waker(&waker)),
        Poll::Pending
    );
    assert_eq!(
        seen.get(),
        Some(start + Duration::from_millis(5)),
        "the clone read the deadlines"
    );
    assert_eq!(
        owner.next_deadline(),
        Some(start + Duration::from_millis(20)),
        "the other sleep is disarmed and this one is armed"
    );
}
