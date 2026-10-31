//! `MemoryStorage`'s holds against the code a caller's waker runs.

use std::future::Future;
use std::pin::pin;
use std::sync::{Arc, mpsc};
use std::task::{Context, RawWaker, RawWakerVTable, Waker};
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
