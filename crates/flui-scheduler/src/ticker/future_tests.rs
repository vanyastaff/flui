//! Async-half coverage for [`TickerFuture`], [`TickerCompleter`], and
//! [`TickerDelivery`].
//!
//! Every test here carries an explicit strength label, because a green suite
//! over this family has previously been read as deeper coverage than it was:
//!
//! - **discriminating** — fails on the code this change replaces;
//! - **regression pin** — passes before and after; it exists to redden if a
//!   specific line this change authors (or inherits) is ever reverted, and the
//!   line is named in the test's own doc;
//! - **compile-time fence** — pins a trait bound; no production line reverts
//!   it.
//!
//! A label is only worth having if it is kept honest in both directions: one
//! test below was first labelled "pins nothing" and turned out to pin real
//! production behaviour, which corrupts the inventory just as badly as an
//! over-claim would.
//!
//! The wakers here are `std::task::Wake` implementors, not hand-rolled
//! `RawWakerVTable`s: `Arc::strong_count` on the implementor is the oracle for
//! "is this waker still parked somewhere", the workspace lints `unsafe_code`,
//! and the sibling reproducer in `tests/end_of_frame_lifecycle.rs` already uses
//! this shape.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use super::*;

// ---------------------------------------------------------------------------
// probes
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// ordering: register before the decisive read
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// publication order and resolution shape
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// continuations
// ---------------------------------------------------------------------------

/// **Discriminating.** Two panicking continuations, with distinct messages,
/// bracket a third that must still run — a panicking continuation must not
/// stop its siblings — and the payload re-raised once delivery finishes must
/// be the FIRST one caught, not the last. Removing the per-continuation
/// `catch_unwind` in `deliver_now` reddens the "siblings still run" half
/// (the middle continuation would never fire); replacing
/// `first_payload.get_or_insert(payload)` with an unconditional overwrite
/// (`Option::insert`) reddens the "first, not last" half — the re-raised
/// text would read "second continuation panics" instead.
fn a_panicking_continuation_does_not_starve_its_siblings_and_the_first_payload_is_reraised() {
    let (completer, future) = TickerFuture::pending();
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
/// `Drop for TickerCompleter` that runs mid-unwind is exactly the shape a
/// caller's own panicking `Drop` produces in production.
fn a_completer_dropped_mid_unwind_with_a_panicking_continuation_does_not_abort() {
    let (completer, future) = TickerFuture::pending();
    future.when_complete_or_cancel(|_outcome| panic!("continuation panics"));

    let (result, log) = flui_testing::log_capture::capture(|| {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _completer = completer;
            panic!("outer panic drops the completer while already unwinding");
        }))
    });

    assert!(
        result.is_err(),
        "the outer panic must still propagate to catch_unwind"
    );
    assert!(
        future.is_canceled(),
        "Drop for TickerCompleter must still resolve the future, even mid-unwind"
    );
    assert!(
        log.count_containing("already unwinding") >= 1,
        "the continuation's panic must be logged rather than silently lost: {log}"
    );
}

#[test]
fn ticker_future_unwind_matrix() {
    crate::table_test::run_table(
        "ticker_future_unwind_matrix",
        &[
            ("a_panicking_continuation_does_not_starve_its_siblings_and_the_first_payload_is_reraised", a_panicking_continuation_does_not_starve_its_siblings_and_the_first_payload_is_reraised as fn()),
            ("a_completer_dropped_mid_unwind_with_a_panicking_continuation_does_not_abort", a_completer_dropped_mid_unwind_with_a_panicking_continuation_does_not_abort as fn()),
        ],
    );
}

// ---------------------------------------------------------------------------
// mute -> start must still be refused
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// contract fences
// ---------------------------------------------------------------------------
