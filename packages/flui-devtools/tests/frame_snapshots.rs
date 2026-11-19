//! A presentation's frame snapshots in the exported timeline.
#![cfg(feature = "timeline")]

use flui_devtools::timeline::Timeline;
use flui_foundation::{FrameEpoch, PresentationAddress, PresentationId, UiRuntimeId};
use flui_scheduler::{
    DemandKind, FrameClock, FramePhaseDurations, FrameTimes, FramesInFlight, Instant,
    PresentOutcome,
};

/// A frame whose GPU time arrived carries it in its trace event's args as
/// `gpu_us`; a frame without one carries none (ADR-0388).
///
/// Red-check: leave `gpu_us` out of the args and the timed frame has none.
#[test]
fn a_timed_frame_carries_its_gpu_time_into_the_trace() {
    let clock = FrameClock::new();
    let start = Instant::now();
    let times = FrameTimes {
        clock_timestamp: start,
        segment_start: start,
        segment_end: start,
        submit_at: start,
        phases: FramePhaseDurations::default(),
    };
    let address = PresentationAddress {
        ui_runtime_id: UiRuntimeId::new(1),
        presentation_id: PresentationId::new(1),
    };
    for epoch in 1..=2 {
        clock.mark_demand(DemandKind::Dirty);
        assert!(clock.poll(start, FramesInFlight::NONE).should_run_segment());
        let _ = clock.record_frame(
            clock.telemetry_cut(),
            address,
            FrameEpoch::from_raw(epoch),
            times,
            PresentOutcome::Presented,
        );
    }
    let _ = clock
        .record_gpu_time(
            FrameEpoch::from_raw(1),
            std::time::Duration::from_micros(420),
        )
        .expect("the ring holds the first frame");

    let timeline = Timeline::new();
    timeline.record_frame_snapshots(&clock.frames_since(None));
    let trace: serde_json::Value =
        serde_json::from_str(&timeline.export_chrome_trace()).expect("the export is JSON");
    let gpu = trace["traceEvents"]
        .as_array()
        .expect("traceEvents")
        .iter()
        .filter(|event| event["args"]["frame_id"].is_string())
        .map(|event| event["args"]["gpu_us"].as_u64())
        .collect::<Vec<_>>();
    assert_eq!(gpu, [Some(420), None]);
}
