//! The foundation test kit's wakers count, act and retire as documented.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use flui_foundation::panic::{FirstPanic, payload_text};
use flui_foundation::testing::{CountingWaker, panicking_waker};

struct Retired(Arc<AtomicUsize>);

impl Drop for Retired {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn a_counting_waker_counts_each_wake_and_runs_its_action() {
    let actions = Arc::new(AtomicUsize::new(0));
    let wakes = {
        let actions = Arc::clone(&actions);
        CountingWaker::calling(move || {
            actions.fetch_add(1, Ordering::SeqCst);
        })
        .shared()
    };
    assert!(!wakes.was_woken());
    let waker = wakes.waker();
    waker.wake_by_ref();
    wakes.waker().wake();
    assert_eq!((wakes.count(), actions.load(Ordering::SeqCst)), (2, 2));
}

#[test]
fn a_retiring_waker_drops_its_captures_with_its_last_clone_and_the_count_stays_readable() {
    let actions = Arc::new(AtomicUsize::new(0));
    let retired = Arc::new(AtomicUsize::new(0));
    let wakes = CountingWaker::new().shared();
    let waker = {
        let actions = Arc::clone(&actions);
        wakes.retiring_waker(
            move || {
                actions.fetch_add(1, Ordering::SeqCst);
            },
            Retired(Arc::clone(&retired)),
        )
    };
    let clone = waker.clone();
    waker.wake();
    assert_eq!(retired.load(Ordering::SeqCst), 0, "a clone is still out");
    clone.wake_by_ref();
    drop(clone);
    assert_eq!(retired.load(Ordering::SeqCst), 1);
    assert_eq!((wakes.count(), actions.load(Ordering::SeqCst)), (2, 2));
}

#[test]
fn a_panicking_waker_panics_with_its_message_on_every_wake() {
    let waker = panicking_waker("woken");
    for _ in 0..2 {
        let mut first = FirstPanic::new();
        first.contain(|| waker.wake_by_ref());
        let payload = first.held().expect("the wake panics");
        assert_eq!(payload.downcast_ref::<&str>(), Some(&"woken"));
        assert_eq!(payload_text(payload), Some("woken"));
    }
}
