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
fn a_counting_waker_counts_each_wake_runs_its_action_and_retires_its_capture() {
    let actions = Arc::new(AtomicUsize::new(0));
    let retired = Arc::new(AtomicUsize::new(0));
    let wakes = {
        let actions = Arc::clone(&actions);
        CountingWaker::calling(move || {
            actions.fetch_add(1, Ordering::SeqCst);
        })
        .holding(Retired(Arc::clone(&retired)))
        .shared()
    };
    assert!(!wakes.was_woken());
    let waker = wakes.waker();
    waker.wake_by_ref();
    wakes.waker().wake();
    assert_eq!((wakes.count(), actions.load(Ordering::SeqCst)), (2, 2));
    drop(waker);
    assert_eq!(
        retired.load(Ordering::SeqCst),
        0,
        "the test still shares it"
    );
    drop(wakes);
    assert_eq!(retired.load(Ordering::SeqCst), 1);
}

#[test]
fn a_panicking_waker_panics_on_every_wake() {
    let waker = panicking_waker("woken");
    for _ in 0..2 {
        let mut first = FirstPanic::new();
        first.contain(|| waker.wake_by_ref());
        assert_eq!(first.held().and_then(payload_text), Some("woken"));
    }
}
