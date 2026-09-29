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

use flui_testing::log_capture::capture;

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

fn a_panicking_body_still_tears_the_sink_down() {
    let panicked = std::panic::catch_unwind(|| {
        capture(|| panic!("probe panic"));
    });
    assert!(panicked.is_err(), "the body's panic must propagate");

    // If the sink had leaked, this would panic on the nesting assertion.
    let ((), log) = capture(|| tracing::warn!("after the panic"));
    assert!(log.contains("after the panic"), "{log}");
}

#[test]
fn log_capture_matrix() {
    crate::run_table(
        "log_capture_matrix",
        &[
            (
                "fields_and_levels_survive_the_round_trip",
                fields_and_levels_survive_the_round_trip as fn(),
            ),
            (
                "a_panicking_body_still_tears_the_sink_down",
                a_panicking_body_still_tears_the_sink_down as fn(),
            ),
        ],
    );
}
