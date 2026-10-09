use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

fn a_panicking_continuation_does_not_starve_its_siblings_and_the_first_payload_is_reraised() {
    let (completer, future) = AnimationRunFuture::pending();
    let middle_ran = Arc::new(AtomicBool::new(false));
    let middle_ran2 = Arc::clone(&middle_ran);

    future.when_complete_or_cancel(|_outcome| panic!("first continuation panics"));
    future.when_complete_or_cancel(move |_outcome| {
        middle_ran2.store(true, Ordering::SeqCst);
    });
    future.when_complete_or_cancel(|_outcome| panic!("second continuation panics"));

    let delivery = completer.complete();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| delivery.deliver()));

    assert!(
        middle_ran.load(Ordering::SeqCst),
        "a panicking continuation must not prevent its siblings from running"
    );
    let payload =
        result.expect_err("the first caught payload must be re-raised once delivery has finished");
    assert_eq!(
        flui_foundation::panic::payload_text(&*payload),
        Some("first continuation panics"),
        "the FIRST caught payload must be the one re-raised, not the last"
    );
}

/// **Discriminating**, and the one test in this file with a process-survival
/// stake: reverting `deliver_now`'s `std::thread::panicking()` gate — always
/// `resume_unwind`ing the first caught payload — turns this into a
/// panic-during-panic, which the Rust runtime aborts rather than unwinds. A
/// `Drop for RunCompleter` that runs mid-unwind is exactly the shape a
/// caller's own panicking `Drop` produces in production.
fn a_completer_dropped_mid_unwind_with_a_panicking_continuation_does_not_abort() {
    let (completer, future) = AnimationRunFuture::pending();
    future.when_complete_or_cancel(|_outcome| panic!("continuation panics"));

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _completer = completer;
        panic!("outer panic drops the completer while already unwinding");
    }));

    assert!(
        result.is_err(),
        "the outer panic must still propagate to catch_unwind"
    );
    assert!(
        future.is_canceled(),
        "Drop for RunCompleter must still resolve the future, even mid-unwind"
    );
    let payload = result.expect_err("outer failure");
    assert_eq!(
        flui_foundation::panic::payload_text(&*payload),
        Some("outer panic drops the completer while already unwinding")
    );
}

#[test]
fn run_future_unwind_matrix() {
    crate::test_cases::run_cases(&[
        (
            "continuation tail",
            a_panicking_continuation_does_not_starve_its_siblings_and_the_first_payload_is_reraised,
        ),
        (
            "incoming unwind",
            a_completer_dropped_mid_unwind_with_a_panicking_continuation_does_not_abort,
        ),
    ]);
}
