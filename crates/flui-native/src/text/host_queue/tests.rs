//! The host queue, run on every host.

use std::rc::Rc;

use flui_platform::text_store::TextStore;
use flui_testing::text_store_kit::InMemoryTextStore;

use super::{HostQueue, MirrorHostOp};

#[test]
fn a_request_outside_a_store_call_runs_now() {
    let queue = HostQueue::<u32>::new();
    assert!(!queue.is_entered());
    assert_eq!(
        queue.defer(1),
        Some(1),
        "nothing is on the stack to wait for"
    );
    assert_eq!(queue.next(), None);
}

#[test]
fn requests_inside_a_store_call_wait_for_the_outermost_to_return() {
    let queue = HostQueue::<u32>::new();
    let outer = queue.enter();
    assert_eq!(queue.defer(1), None);
    let inner = queue.enter();
    assert_eq!(queue.defer(2), None);
    drop(inner);
    assert_eq!(queue.next(), None, "the outer call is still running");
    drop(outer);
    // A drained request that calls into a store queues behind the rest.
    let first = queue.next();
    let during = queue.enter();
    assert_eq!(queue.defer(3), None);
    assert_eq!(queue.next(), None, "not while the drained request runs");
    drop(during);
    assert_eq!(
        [first, queue.next(), queue.next(), queue.next()],
        [Some(1), Some(2), Some(3), None]
    );
}

#[test]
fn a_store_call_that_unwinds_still_ends() {
    let queue = HostQueue::<u32>::new();
    let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _entered = queue.enter();
        assert_eq!(queue.defer(1), None);
        panic!("a store callback panics");
    }));
    assert!(unwound.is_err());
    assert!(!queue.is_entered());
    assert_eq!(queue.next(), Some(1), "the next host operation drains it");
}

#[test]
fn the_newest_queued_focus_is_the_one_asked_about() {
    let queue = HostQueue::<MirrorHostOp>::new();
    let first: Rc<dyn TextStore> = InMemoryTextStore::new("first");
    let second: Rc<dyn TextStore> = InMemoryTextStore::new("second");
    let current = || Some(Rc::clone(&first));
    let is = |focus: Option<Rc<dyn TextStore>>, store: &Rc<dyn TextStore>| {
        focus.is_some_and(|focus| Rc::ptr_eq(&focus, store))
    };
    assert!(
        is(queue.focus_after_queue(current), &first),
        "nothing queued: the current focus"
    );

    let entered = queue.enter();
    assert!(
        queue
            .defer(MirrorHostOp::Focus(Some(Rc::clone(&second))))
            .is_none()
    );
    assert!(
        queue
            .defer(MirrorHostOp::Complete(Rc::clone(&first)))
            .is_none()
    );
    assert!(
        is(queue.focus_after_queue(current), &second),
        "a completion queued after it gives no focus"
    );

    assert!(queue.defer(MirrorHostOp::Focus(None)).is_none());
    assert!(
        queue.focus_after_queue(current).is_none(),
        "an unfocus is a focus change too"
    );

    drop(entered);
    assert!(
        matches!(queue.next(), Some(MirrorHostOp::Focus(Some(store))) if Rc::ptr_eq(&store, &second))
    );
    assert!(
        matches!(queue.next(), Some(MirrorHostOp::Complete(store)) if Rc::ptr_eq(&store, &first)),
        "a completion keeps the store it was asked for"
    );
    assert!(
        queue.focus_after_queue(current).is_none(),
        "the unfocus is still queued"
    );

    queue.clear();
    assert!(is(queue.focus_after_queue(current), &first));
}
