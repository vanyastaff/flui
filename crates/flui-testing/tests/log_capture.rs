//! Capture survives the race that defeats a thread-local subscriber.
//!
//! `tracing` computes a callsite's `Interest` once, on whichever thread first
//! reaches it, and caches it process-globally
//! (`tracing_core::callsite::Rebuilder::JustOne` → `dispatcher::get_default`).
//! A capture built on [`tracing::subscriber::with_default`] therefore loses
//! every event from a callsite another thread reached first with no subscriber
//! installed: that callsite is cached as `Interest::never()` and stays silent.
//!
//! The tests below force that interleaving with a barrier instead of hoping for
//! it, so they are a regression pin rather than a flake detector.

use std::sync::Barrier;
use std::time::Duration;

use flui_testing::log_capture::capture;

/// One callsite, reachable from two threads.
///
/// It must be a function: two `warn!` invocations at different source
/// locations are different callsites, and the race is about a *single* one.
fn emit_probe(tag: &str) {
    tracing::warn!(tag, "log-capture probe");
}

#[test]
fn a_callsite_another_thread_reaches_mid_capture_is_still_recorded() {
    // Two rendezvous points put the other thread's emission strictly INSIDE
    // the capture: after the subscriber is installed (so a `with_default`
    // capture's own dispatcher-registration rebuild has already happened and
    // cannot save it), and before this thread emits.
    let barrier = Barrier::new(2);

    std::thread::scope(|scope| {
        scope.spawn(|| {
            barrier.wait();
            // No subscriber on this thread. Under a `with_default` capture this
            // is the poison: the callsite's interest is computed against
            // `NoSubscriber` and cached as `never` for the whole process.
            emit_probe("other-thread");
            barrier.wait();
        });

        let ((), log) = capture(|| {
            barrier.wait();
            barrier.wait();
            emit_probe("capturing-thread");
        });

        assert_eq!(
            log.count_containing("capturing-thread"),
            1,
            "the capturing thread's own event must survive a callsite another \
             thread reached first with no subscriber; captured:\n{log}",
        );
        assert_eq!(
            log.count_containing("other-thread"),
            0,
            "capture is per-thread: another thread's events must not leak into \
             this buffer; captured:\n{log}",
        );
    });
}

#[test]
fn two_threads_capture_concurrently_without_seeing_each_other() {
    // The predecessor helpers serialised their capturing tests behind a mutex
    // because a process-global subscriber would have mixed their output. A
    // thread-local sink needs no such lock.
    let barrier = Barrier::new(2);

    std::thread::scope(|scope| {
        let left = scope.spawn(|| {
            capture(|| {
                barrier.wait();
                tracing::warn!("left side");
                std::thread::sleep(Duration::from_millis(1));
            })
            .1
        });
        let right = scope.spawn(|| {
            capture(|| {
                barrier.wait();
                tracing::warn!("right side");
                std::thread::sleep(Duration::from_millis(1));
            })
            .1
        });

        let left = left.join().expect("left capture thread");
        let right = right.join().expect("right capture thread");

        assert!(
            left.contains("left side") && !left.contains("right side"),
            "{left}"
        );
        assert!(
            right.contains("right side") && !right.contains("left side"),
            "{right}"
        );
    });
}

#[test]
fn a_thread_with_no_active_capture_records_nothing() {
    // The global subscriber is installed for the whole process once any test
    // captures, so this asserts the per-event gate actually gates.
    let ((), first) = capture(|| tracing::warn!("inside"));
    assert!(first.contains("inside"));

    tracing::warn!("outside");

    let ((), second) = capture(|| {});
    assert!(
        second.is_empty(),
        "an event emitted outside any capture must not be buffered for the \
         next one; captured:\n{second}",
    );
}

#[test]
fn fields_and_levels_survive_the_round_trip() {
    let ((), log) = capture(|| {
        tracing::error!(count = 3, name = "widget", "structured probe");
    });

    let record = log
        .at_level(tracing::Level::ERROR)
        .find(|record| record.message == "structured probe")
        .expect("the error event must be captured");
    assert_eq!(record.field("count"), Some("3"));
    assert_eq!(record.field("name"), Some("widget"));
    assert!(
        record.target.ends_with("log_capture"),
        "the emitting module path is preserved: {}",
        record.target,
    );
}

#[test]
fn a_panicking_body_still_tears_the_sink_down() {
    let panicked = std::panic::catch_unwind(|| {
        capture(|| panic!("probe panic"));
    });
    assert!(panicked.is_err(), "the body's panic must propagate");

    // If the sink had leaked, this would panic on the nesting assertion.
    let ((), log) = capture(|| tracing::warn!("after the panic"));
    assert!(log.contains("after the panic"), "{log}");
}
