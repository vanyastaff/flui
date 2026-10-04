//! The framework's half of the profiler's wiring contract.
//!
//! `frame_timing_layer`'s unit tests emit the frame and phase spans
//! themselves, so they can only prove that the layer maps the right names.
//! This test drives a real tree through `HeadlessBinding::pump_frame`, which
//! runs the same `UpdateScheduler::drive_frame` the desktop, Android and web
//! runners run, and reads the profile back. If the scheduler stops opening
//! the `frame` span, or the pipeline renames a phase span, this is the test
//! that fails — the unit tests cannot.
#![cfg(feature = "profiling")]

use std::sync::Arc;
use std::time::Duration;

use flui_devtools::profiler::{FramePhase, FrameStats};
use flui_devtools::{FrameTimingLayer, Profiler};
use flui_objects::RenderSizedBox;
use flui_sdk::view::{RenderView, View};
use flui_testing::{HeadlessBinding, MountOptions, MountOwners};
use tracing::Dispatch;
use tracing_subscriber::layer::SubscriberExt;

/// The smallest tree that builds, lays out and paints: one render leaf.
#[derive(Clone)]
struct Leaf;

impl RenderView for Leaf {
    type Protocol = flui_sdk::rendering::BoxProtocol;
    type RenderObject = RenderSizedBox;

    fn create_render_object(
        &self,
        _ctx: &flui_sdk::view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        RenderSizedBox::shrink()
    }

    fn update_render_object(
        &self,
        _ctx: &flui_sdk::view::RenderObjectContext<'_>,
        _render_object: &mut Self::RenderObject,
    ) -> flui_sdk::rendering::RenderUpdateImpact {
        flui_sdk::rendering::RenderUpdateImpact::NONE
    }
}

impl View for Leaf {
    fn create_element(&self) -> flui_sdk::view::element::ElementKind {
        flui_sdk::view::element::ElementKind::render_variable(self)
    }
}

fn phases(stats: &FrameStats) -> Vec<FramePhase> {
    stats.phases.iter().map(|info| info.phase).collect()
}

#[test]
fn a_real_frame_reaches_the_profiler_with_its_phases() {
    // `tracing` caches callsite interest process-wide on first use; a sibling
    // test binary cannot poison this one, but the sentinel is cheap and it
    // is the documented way to capture under a thread-local subscriber.
    flui_testing::log_capture::disarm_interest_cache();

    let profiler = Arc::new(Profiler::new());
    let subscriber =
        tracing_subscriber::registry().with(FrameTimingLayer::new(Arc::clone(&profiler)));

    tracing::dispatcher::with_default(&Dispatch::new(subscriber), || {
        let mut binding = HeadlessBinding::new();
        let mounted =
            binding.mount_root(&Leaf, MountOwners::fresh(), MountOptions::tight(64.0, 64.0));
        assert!(mounted.painted, "the bootstrap frame must commit a paint");

        // The bootstrap runs the pipeline directly, outside `drive_frame`, so
        // it is not a frame: nothing may be attributed to a frame that never
        // opened. An empty history here is the honest reading.
        assert!(
            profiler.frame_history().is_empty(),
            "the bootstrap is not a frame; got {:?}",
            profiler.frame_history()
        );

        // An idle frame: build and layout run (their spans are unconditional),
        // there is nothing to paint.
        binding.pump_frame(Duration::from_millis(16));

        // Dirty the root and pump again: this frame paints.
        binding
            .pipeline_owner()
            .expect("mount_root leaves the binding tree-bound")
            .with_mut(|owner| owner.mark_needs_paint(mounted.render_root));
        binding.pump_frame(Duration::from_millis(16));
        assert!(
            binding.did_paint_last_frame(),
            "marking the root dirty must make the second pump paint"
        );
    });

    let history = profiler.frame_history();
    assert_eq!(
        history.len(),
        2,
        "one profiled frame per `drive_frame`, and only those; got {history:?}"
    );
    for (index, frame) in history.iter().enumerate() {
        let seen = phases(frame);
        assert!(
            seen.contains(&FramePhase::Build),
            "frame {index}: the pipeline's `build` span must land as Build; saw {seen:?}"
        );
        assert!(
            seen.contains(&FramePhase::Layout),
            "frame {index}: the pipeline's `layout` span must land as Layout; saw {seen:?}"
        );
        assert!(
            frame.total_time_ms() > 0.0,
            "frame {index}: a driven frame takes time; got {frame:?}"
        );
    }
    let painted = phases(&history[1]);
    assert!(
        painted.contains(&FramePhase::Paint),
        "the dirtied frame's `paint` span must land as Paint; saw {painted:?}"
    );
}

fn zero_history_capacity_keeps_counters_without_frames() {
    let profiler = Profiler::with_config(flui_devtools::ProfilerConfig {
        max_frame_history: 0,
        jank_threshold_ms: -1.0,
    });
    for _ in 0..2 {
        profiler.begin_frame();
        let phase = profiler.profile_phase(FramePhase::Build);
        drop(phase);
        profiler.end_frame();
        assert!(profiler.frame_history().is_empty());
        assert!(profiler.frame_stats().is_none());
    }
    assert_eq!(profiler.jank_percentage(), 100.0);
}

#[cfg(feature = "timeline")]
fn a_guard_before_clear_cannot_update_a_new_event() {
    use flui_devtools::timeline::{EventCategory, Timeline};
    let timeline = Timeline::new();
    let old = timeline.record_event("old", EventCategory::Custom);
    timeline.clear();
    let duration = Duration::from_secs(123_456_789);
    timeline.record_completed_event(
        "new",
        EventCategory::Custom,
        web_time::Instant::now(),
        duration,
        serde_json::Value::Null,
    );
    drop(old);
    let events = timeline.get_events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].name, "new");
    assert_eq!(events[0].duration_micros, duration.as_micros());
}

#[cfg(feature = "timeline")]
fn a_guard_after_clear_updates_its_own_event() {
    use flui_devtools::timeline::{EventCategory, Timeline};
    let timeline = Timeline::new();
    timeline.record_instant("old", EventCategory::Custom);
    timeline.clear();
    let current = timeline.record_event("current", EventCategory::Custom);
    std::thread::sleep(Duration::from_millis(1));
    drop(current);
    let events = timeline.get_events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].name, "current");
    assert!(events[0].duration_micros > 0);
}

#[cfg(feature = "timeline")]
fn completed_event_duration_preserves_large_values() {
    use flui_devtools::timeline::{EventCategory, Timeline};
    let timeline = Timeline::new();
    let duration = Duration::new(18_446_744_073_710, 123_456_000);
    timeline.record_completed_event(
        "large",
        EventCategory::Custom,
        web_time::Instant::now(),
        duration,
        serde_json::Value::Null,
    );
    assert_eq!(timeline.get_events()[0].duration(), duration);
    let mut event = timeline.get_events().remove(0);
    event.duration_micros = u128::MAX;
    assert_eq!(event.duration(), Duration::MAX);
}

#[cfg(feature = "timeline")]
fn run_reentrant_name_case(kind: &str) {
    use flui_devtools::timeline::{EventCategory, Timeline};
    struct Name(Timeline);
    impl From<Name> for String {
        fn from(name: Name) -> Self {
            name.0.clear();
            "reentrant".to_owned()
        }
    }
    let timeline = Timeline::new();
    timeline.record_instant("removed", EventCategory::Custom);
    let name = Name(timeline.clone());
    match kind {
        "guard" => drop(timeline.record_event(name, EventCategory::Custom)),
        "instant" => timeline.record_instant(name, EventCategory::Custom),
        "completed" => timeline.record_completed_event(
            name,
            EventCategory::Custom,
            web_time::Instant::now(),
            Duration::from_secs(7),
            serde_json::Value::Null,
        ),
        _ => panic!("unknown name case"),
    }
    let events = timeline.get_events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].name, "reentrant");
    timeline.record_instant("next", EventCategory::Custom);
    assert_eq!(timeline.event_count(), 2);
}

#[cfg(feature = "timeline")]
fn isolated_reentrant_name(kind: &str) {
    use std::process::{Command, Stdio};
    let mut child = Command::new(std::env::current_exe().expect("test executable"))
        .args([
            "--exact",
            "developer_history_respects_capacity_and_clear",
            "--nocapture",
        ])
        .env("FLUI_TIMELINE_NAME_CASE", kind)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn timeline consumer child");
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        if child.try_wait().expect("child status").is_some() {
            break;
        }
        if std::time::Instant::now() >= deadline {
            child.kill().expect("kill blocked child");
            let output = child.wait_with_output().expect("reap blocked child");
            panic!("name conversion {kind} blocked: {output:?}");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let output = child.wait_with_output().expect("consumer output");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success() && stdout.contains("1 passed"),
        "name conversion {kind} failed: {output:?}"
    );
}

#[cfg(feature = "timeline")]
fn guard_name_conversion_can_reenter_the_timeline() {
    isolated_reentrant_name("guard");
}
#[cfg(feature = "timeline")]
fn instant_name_conversion_can_reenter_the_timeline() {
    isolated_reentrant_name("instant");
}
#[cfg(feature = "timeline")]
fn completed_name_conversion_can_reenter_the_timeline() {
    isolated_reentrant_name("completed");
}

#[test]
fn developer_history_respects_capacity_and_clear() {
    #[cfg(feature = "timeline")]
    if let Ok(kind) = std::env::var("FLUI_TIMELINE_NAME_CASE") {
        run_reentrant_name_case(&kind);
        return;
    }
    let cases: &[(&str, fn())] = &[
        (
            "zero_capacity",
            zero_history_capacity_keeps_counters_without_frames,
        ),
        #[cfg(feature = "timeline")]
        (
            "stale_guard",
            a_guard_before_clear_cannot_update_a_new_event,
        ),
        #[cfg(feature = "timeline")]
        ("fresh_guard", a_guard_after_clear_updates_its_own_event),
        #[cfg(feature = "timeline")]
        (
            "large_duration",
            completed_event_duration_preserves_large_values,
        ),
        #[cfg(feature = "timeline")]
        (
            "reentrant_guard_name",
            guard_name_conversion_can_reenter_the_timeline,
        ),
        #[cfg(feature = "timeline")]
        (
            "reentrant_instant_name",
            instant_name_conversion_can_reenter_the_timeline,
        ),
        #[cfg(feature = "timeline")]
        (
            "reentrant_completed_name",
            completed_name_conversion_can_reenter_the_timeline,
        ),
    ];
    let mut failures = Vec::new();
    for &(name, case) in cases {
        if std::panic::catch_unwind(case).is_err() {
            failures.push(name);
        }
    }
    assert!(failures.is_empty(), "failed cases: {failures:?}");
}
