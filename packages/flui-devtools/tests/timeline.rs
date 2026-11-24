//! What a timeline keeps, counts and returns.
#![cfg(feature = "timeline")]

use std::time::Duration;

use flui_devtools::{Timeline, TimelineEventKind};
use flui_scheduler::Instant;

fn names(timeline: &Timeline) -> Vec<String> {
    timeline
        .events()
        .into_iter()
        .map(|event| event.name)
        .collect()
}

fn record(timeline: &Timeline, name: &str, start: Instant, duration: Duration) {
    timeline.record_completed_event(name, start, duration, serde_json::Value::Null);
}

/// A timeline over a capture counts what `events` returns: each frame, its spans and the
/// application's own events.
///
/// Fails when the count leaves out a frame's own span or its child spans, or the application's
/// events.
#[cfg(feature = "profiling")]
#[test]
fn the_event_count_is_the_number_of_events_returned() {
    use flui_devtools::Capture;

    let capture = Capture::new();
    let timeline = Timeline::over(&capture);
    crate::profiler::capturing(&capture, || {
        for _ in 0..2 {
            let _frame = tracing::debug_span!(target: "flui.frame", "frame").entered();
            let _layout = tracing::debug_span!(target: "flui.frame", "layout").entered();
        }
    });
    timeline.record_instant("application work");

    let events = timeline.events();
    assert_eq!(events.len(), 5, "two frames, two layouts and one event");
    assert_eq!(timeline.event_count(), events.len());
}

/// A bounded timeline keeps the newest events, oldest first, and an event trimmed while its
/// guard is held is not closed by another event's duration.
///
/// Fails when the oldest event is not the one dropped, or when the trim does not move the
/// handles of the events that survive it.
#[test]
fn a_bounded_timeline_keeps_the_newest_events_and_a_trimmed_guard_closes_nothing() {
    let timeline = Timeline::bounded(2);
    let guard = timeline.record_event("held");
    let now = Instant::now();
    record(&timeline, "second", now, Duration::from_millis(5));
    record(&timeline, "third", now, Duration::from_millis(7));
    assert_eq!(names(&timeline), ["second", "third"]);

    drop(guard);

    let durations: Vec<_> = timeline
        .events()
        .iter()
        .map(flui_devtools::TimelineEvent::duration)
        .collect();
    assert_eq!(
        durations,
        [Duration::from_millis(5), Duration::from_millis(7)]
    );
    assert_eq!(timeline.event_count(), 2);

    for index in 0..5 {
        timeline.record_instant(format!("instant {index}"));
    }
    assert_eq!(names(&timeline), ["instant 3", "instant 4"]);
}

/// `events_between` keeps the events wholly inside the range, measured from the timeline's
/// start, and leaves one that starts before it and one that ends after it.
///
/// Fails when either end of the range is not compared, or the end compares only the start.
#[test]
fn events_between_keeps_only_the_events_wholly_inside() {
    let timeline = Timeline::new();
    let base = Instant::now();
    record(
        &timeline,
        "starts before",
        base + Duration::from_secs(1),
        Duration::from_millis(100),
    );
    record(
        &timeline,
        "inside",
        base + Duration::from_secs(2),
        Duration::from_millis(500),
    );
    record(
        &timeline,
        "ends after",
        base + Duration::from_millis(2800),
        Duration::from_millis(500),
    );

    let inside = timeline.events_between(Duration::from_millis(1500), Duration::from_secs(3));

    let kept: Vec<_> = inside.iter().map(|event| event.name.as_str()).collect();
    assert_eq!(kept, ["inside"]);
    assert_eq!(timeline.events_of(TimelineEventKind::Custom).len(), 3);
    assert!(timeline.events_of(TimelineEventKind::Frame).is_empty());
}
