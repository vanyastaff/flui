//! `MemoryStorage`'s holds against the code a caller's waker runs.

use std::future::Future;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::pin::pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::task::{Context, RawWaker, RawWakerVTable, Wake, Waker};
use std::thread;
use std::time::Duration;

use flui_platform::{Storage, StorageName};
use flui_testing::storage::MemoryStorage;

const NOTES: StorageName = StorageName::from_static("notes");

/// What a [`cloning_waker`] runs each time it is cloned.
struct CloneExecutor(Box<dyn Fn() + Send + Sync>);

/// A waker that runs `action` on every clone, as an executor's may: the one
/// behavior the safe `Wake` trait cannot customize. Each raw waker owns one
/// strong reference to its executor.
#[expect(
    unsafe_code,
    reason = "exercise an executor's clone through the public Future API"
)]
fn cloning_waker(action: impl Fn() + Send + Sync + 'static) -> Waker {
    unsafe fn clone(data: *const ()) -> RawWaker {
        // SAFETY: the raw waker this clones owns a live strong reference for
        // the whole call. The new reference is taken only after the action
        // returns, so a panicking action leaves no unowned reference.
        let executor = unsafe { &*data.cast::<CloneExecutor>() };
        (executor.0)();
        unsafe { Arc::increment_strong_count(data.cast::<CloneExecutor>()) };
        RawWaker::new(data, &VTABLE)
    }
    unsafe fn release(data: *const ()) {
        // SAFETY: a consuming wake or a drop receives the one reference this
        // raw waker owns, exactly once.
        drop(unsafe { Arc::from_raw(data.cast::<CloneExecutor>()) });
    }
    unsafe fn wake_by_ref(_: *const ()) {}
    const VTABLE: RawWakerVTable = RawWakerVTable::new(clone, release, wake_by_ref, release);
    let data = Arc::into_raw(Arc::new(CloneExecutor(Box::new(action)))).cast();
    // SAFETY: `data` is an `Arc` allocation whose one strong reference passes
    // to the waker, and the vtable keeps that ownership as above.
    unsafe { Waker::from_raw(RawWaker::new(data, &VTABLE)) }
}

#[test]
fn a_held_copy_polled_with_a_waker_that_reads_the_storage_answers() {
    let storage = MemoryStorage::new();
    storage.put(&NOTES, "old");
    let (answered, answer) = mpsc::channel();
    // A waker that re-entered the storage under its lock would deadlock this
    // thread, so it runs apart and the test waits a bounded time.
    thread::spawn(move || {
        let reader = storage.clone();
        let waker = cloning_waker(move || {
            let _ = reader.contents(&NOTES);
        });
        let mut cx = Context::from_waker(&waker);
        let held = storage.hold_copies();
        let mut copy = pin!(storage.copy_aside(&NOTES));
        let first = copy.as_mut().poll(&mut cx).is_pending();
        drop(held);
        let second = copy.as_mut().poll(&mut cx).is_ready();
        let _ = answered.send((first, second, storage.kept_aside(&NOTES)));
    });
    assert_eq!(
        answer.recv_timeout(Duration::from_secs(30)),
        Ok((true, true, vec![b"old".to_vec()])),
        "the held copy waits, then answers, without deadlocking"
    );
}

/// A waker that panics when woken.
struct PanicsOnWake;

impl Wake for PanicsOnWake {
    fn wake(self: Arc<Self>) {
        panic!("the first waiting copy's waker panics");
    }
}

/// A waker that records being woken.
#[derive(Default)]
struct Woken(AtomicBool);

impl Wake for Woken {
    fn wake(self: Arc<Self>) {
        self.0.store(true, Ordering::SeqCst);
    }
}

#[test]
fn a_panicking_waker_leaves_the_other_held_copies_woken() {
    let storage = MemoryStorage::new();
    storage.put(&NOTES, "old");
    let held = storage.hold_copies();
    let panics = Waker::from(Arc::new(PanicsOnWake));
    let woken = Arc::new(Woken::default());
    let told = Waker::from(Arc::clone(&woken));
    let mut first = pin!(storage.copy_aside(&NOTES));
    let mut second = pin!(storage.copy_aside(&NOTES));
    assert!(
        first
            .as_mut()
            .poll(&mut Context::from_waker(&panics))
            .is_pending()
    );
    assert!(
        second
            .as_mut()
            .poll(&mut Context::from_waker(&told))
            .is_pending()
    );

    let released = catch_unwind(AssertUnwindSafe(|| drop(held)));
    assert!(released.is_err(), "the waker's panic reaches the release");
    assert!(
        woken.0.load(Ordering::SeqCst),
        "the second copy is woken all the same"
    );
    assert!(
        second
            .as_mut()
            .poll(&mut Context::from_waker(&told))
            .is_ready()
    );
}

/// A waker that panics with a payload of its own when woken.
struct PanicsSecond;

impl Wake for PanicsSecond {
    fn wake(self: Arc<Self>) {
        panic!("the second waiting copy's waker panics");
    }
}

#[test]
fn competing_waker_panics_keep_the_first_and_wake_the_rest() {
    let storage = MemoryStorage::new();
    storage.put(&NOTES, "old");
    let held = storage.hold_copies();
    let first_waker = Waker::from(Arc::new(PanicsOnWake));
    let second_waker = Waker::from(Arc::new(PanicsSecond));
    let woken = Arc::new(Woken::default());
    let third_waker = Waker::from(Arc::clone(&woken));
    let mut first = pin!(storage.copy_aside(&NOTES));
    let mut second = pin!(storage.copy_aside(&NOTES));
    let mut third = pin!(storage.copy_aside(&NOTES));
    for (copy, waker) in [
        (first.as_mut(), &first_waker),
        (second.as_mut(), &second_waker),
        (third.as_mut(), &third_waker),
    ] {
        assert!(copy.poll(&mut Context::from_waker(waker)).is_pending());
    }

    let released = catch_unwind(AssertUnwindSafe(|| drop(held)));
    let payload = released.expect_err("the first waker's panic reaches the release");
    assert_eq!(
        payload.downcast_ref::<&str>(),
        Some(&"the first waiting copy's waker panics"),
        "the first failure is the one resumed"
    );
    assert!(
        woken.0.load(Ordering::SeqCst),
        "the third copy is woken all the same"
    );
}
