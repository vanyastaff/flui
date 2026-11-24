//! A presentation's frame snapshots in the exported timeline.
#![cfg(feature = "timeline")]

use flui_devtools::Timeline;
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

/// A frame snapshot's event carries the runtime and presentation of its address, so the Chrome
/// export puts each frame in the process of the runtime that produced it, not in the process of
/// events that name no runtime.
///
/// Fails when the snapshot's event names no runtime or presentation.
#[test]
fn a_snapshot_frame_lands_in_its_runtimes_process() {
    let clock = FrameClock::new();
    let start = Instant::now();
    let times = FrameTimes {
        clock_timestamp: start,
        segment_start: start,
        segment_end: start,
        submit_at: start,
        phases: FramePhaseDurations::default(),
    };
    let addresses = [
        PresentationAddress {
            ui_runtime_id: UiRuntimeId::new(1),
            presentation_id: PresentationId::new(1),
        },
        PresentationAddress {
            ui_runtime_id: UiRuntimeId::new(2),
            presentation_id: PresentationId::new(3),
        },
    ];
    for (epoch, address) in (1..).zip(addresses) {
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

    let timeline = Timeline::new();
    timeline.record_instant("names no runtime");
    timeline.record_frame_snapshots(&clock.frames_since(None));
    let frames: Vec<_> = timeline
        .get_events()
        .into_iter()
        .filter(|event| event.args["frame_id"].is_string())
        .map(|event| (event.runtime, event.presentation))
        .collect();
    assert_eq!(
        frames,
        addresses.map(|address| (Some(address.ui_runtime_id), Some(address.presentation_id)))
    );

    let trace: serde_json::Value =
        serde_json::from_str(&timeline.export_chrome_trace()).expect("the export is JSON");
    let events = trace["traceEvents"].as_array().expect("traceEvents");
    let process_name = |pid: &serde_json::Value| {
        events
            .iter()
            .find(|event| event["name"] == "process_name" && event["pid"] == *pid)
            .map(|event| event["args"]["name"].clone())
            .expect("every process is named")
    };
    let processes: Vec<_> = events
        .iter()
        .filter(|event| event["args"]["frame_id"].is_string())
        .map(|event| {
            (
                process_name(&event["pid"]),
                event["args"]["presentation"].clone(),
            )
        })
        .collect();
    assert_eq!(
        processes,
        addresses.map(|address| (
            serde_json::json!(format!(
                "FLUI runtime {:#x}",
                address.ui_runtime_id.as_u64()
            )),
            serde_json::json!(address.presentation_id.as_u64()),
        ))
    );
}
