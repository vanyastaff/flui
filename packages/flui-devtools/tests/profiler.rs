//! The profiler over a capture fed frame spans the test opens itself, so it knows when each
//! frame starts and how long it runs.
#![cfg(feature = "profiling")]

use std::time::Duration;

use flui_devtools::{Capture, JankLine, Profiler, ProfilerConfig};
use tracing::Dispatch;
use tracing_subscriber::layer::SubscriberExt;

/// Runs `body` with `capture`'s layer installed on this thread.
fn capturing(capture: &Capture, body: impl FnOnce()) {
    flui_testing::log_capture::disarm_interest_cache();
    let subscriber = tracing_subscriber::registry().with(capture.layer());
    tracing::dispatcher::with_default(&Dispatch::new(subscriber), body);
}

/// One frame span as the scheduler opens it, reporting `period_us` and running for `takes`.
fn frame(period_us: Option<u64>, takes: Duration) {
    let frame = tracing::debug_span!(
        target: "flui.frame",
        "frame",
        period_us = tracing::field::Empty
    );
    let _entered = frame.enter();
    if let Some(period_us) = period_us {
        frame.record("period_us", period_us);
    }
    if !takes.is_zero() {
        std::thread::sleep(takes);
    }
}

/// The frame rate counts the frames produced per second, from their starts: three frames that
/// each take microseconds, started at least 50 ms apart, are at most 20 frames per second, not
/// the thousands that dividing a second by each frame's work would report.
///
/// Fails when the rate is taken from frame durations, or counts the frames instead of the
/// intervals between them.
#[test]
fn frame_rate_counts_produced_frames_per_second_from_frame_starts() {
    let capture = Capture::new();
    let profiler = Profiler::over(&capture, ProfilerConfig::default());
    capturing(&capture, || frame(None, Duration::ZERO));
    assert_eq!(profiler.frame_rate(), None, "one frame has no rate");

    capturing(&capture, || {
        for _ in 0..2 {
            std::thread::sleep(Duration::from_millis(50));
            frame(None, Duration::ZERO);
        }
    });

    let history = profiler.frame_history();
    assert_eq!(history.len(), 3);
    let rate = profiler
        .frame_rate()
        .expect("three frames started apart have a rate");
    assert!(
        rate > 1.0 && rate <= 20.0,
        "two intervals of at least 50 ms: {rate} frames per second"
    );
    let first = history.first().expect("three frames").start();
    let last = history.last().expect("three frames").start();
    let span = last
        .checked_sub(first)
        .expect("frames are recorded in the order they start");
    let expected = 2.0 / span.as_secs_f64();
    assert!(
        (rate - expected).abs() < 1e-9,
        "two intervals over the span of the starts: {rate} against {expected}"
    );
}

/// A fixed jank line judges a frame by its duration alone, whatever display period the frame
/// reports: a frame shorter than the line is smooth, a longer one is jank.
///
/// Sleeps only lengthen a frame, so the smooth rows have a line a second above their work.
#[test]
fn a_fixed_jank_line_judges_each_frame_by_its_duration() {
    let ms = Duration::from_millis;
    let rows = [
        (ms(1000), None, Duration::ZERO, false),
        (ms(1000), Some(1_000), Duration::ZERO, false),
        (ms(5), None, ms(20), true),
        (ms(5), Some(1_000_000), ms(20), true),
    ];
    for (line, period_us, takes, jank) in rows {
        let capture = Capture::new();
        let profiler = Profiler::over(
            &capture,
            ProfilerConfig {
                jank_line: JankLine::Fixed(line),
                ..ProfilerConfig::default()
            },
        );
        capturing(&capture, || frame(period_us, takes));

        let stats = profiler.frame_stats().expect("the frame was recorded");
        assert_eq!(
            stats.is_jank(),
            jank,
            "a {takes:?} frame against a fixed {line:?} line, reporting {period_us:?} µs: {stats}"
        );
        assert_eq!(profiler.jank_percentage(), if jank { 100.0 } else { 0.0 });
    }
}
