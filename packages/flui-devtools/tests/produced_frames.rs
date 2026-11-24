//! The frames a presentation produced, read from its runtime's frame telemetry through a capture,
//! in the timeline over it and its Chrome export. Real frames: each runtime is a headless host
//! pumped under the capture's subscriber.
#![cfg(all(feature = "profiling", feature = "timeline"))]

use std::time::Duration;

use flui_devtools::{Capture, FrameOutcome, ProducedFrame, Timeline, TimelineEventKind};
use flui_sdk::foundation::{PresentationId, UiRuntimeId};
use flui_sdk::widgets::SizedBox;
use flui_testing::host::{HeadlessHost, HeadlessWindow};

/// A host whose window shows one box, pumped through `frames` frames.
fn pumped_host(frames: u32) -> HeadlessHost {
    let mut host = HeadlessHost::new(HeadlessWindow::new(64, 64));
    host.attach(&SizedBox::new(32.0, 32.0))
        .expect("the box attaches");
    for _ in 0..frames {
        host.request_frame();
        let _outcome = host.pump(Duration::from_millis(16));
    }
    host
}

/// The runtime and presentation of every presentation segment the capture saw.
fn segment_partitions(timeline: &Timeline) -> Vec<(Option<UiRuntimeId>, Option<PresentationId>)> {
    let mut partitions: Vec<_> = timeline
        .events_of(TimelineEventKind::Segment)
        .into_iter()
        .map(|event| (event.runtime, event.presentation))
        .collect();
    partitions.sort_by_key(|(runtime, presentation)| {
        (
            runtime.map(UiRuntimeId::as_u64),
            presentation.map(PresentationId::as_u64),
        )
    });
    partitions.dedup();
    partitions
}

/// Each frame a presentation produced is an event of its own, in the process of the runtime
/// that produced it, named by its whole address and carrying the ids as packed numbers. The ids
/// are the runtime's own: the same ones its frame and segment spans name.
///
/// Fails when the capture does not admit the `frame_telemetry` event (or the `enabled!` gate the
/// runtime asks before tracing it), and when the runtime writes its ids as text.
#[test]
fn a_produced_frame_lands_in_its_runtimes_process() {
    let capture = Capture::new();
    let timeline = Timeline::over(&capture);
    crate::profiler::capturing(&capture, || {
        let _first = pumped_host(1);
        let _second = pumped_host(1);
    });
    timeline.record_instant("names no runtime");

    let produced = timeline.events_of(TimelineEventKind::ProducedFrame);
    let mut partitions: Vec<_> = produced
        .iter()
        .map(|event| (event.runtime, event.presentation))
        .collect();
    partitions.sort_by_key(|(runtime, presentation)| {
        (
            runtime.map(UiRuntimeId::as_u64),
            presentation.map(PresentationId::as_u64),
        )
    });
    partitions.dedup();
    assert_eq!(
        partitions,
        segment_partitions(&timeline),
        "every presentation that ran a segment produced frames, under its own ids: {produced:?}"
    );
    assert_eq!(partitions.len(), 2, "two runtimes, one presentation each");
    for event in &produced {
        let (runtime, presentation) = (
            event.runtime.expect("a produced frame names its runtime"),
            event
                .presentation
                .expect("a produced frame names its presentation"),
        );
        let number = event.args["frame_id"]
            .as_u64()
            .expect("a frame is a number");
        assert_eq!(
            event.name,
            format!("Frame {number} [{runtime} {presentation}]"),
            "the name holds the frame's whole address"
        );
        assert_eq!(event.args["ui_runtime"], runtime.as_u64());
        assert_eq!(event.args["present_outcome"], "presented");
        assert!(event.args["input_count"].is_u64(), "{event:?}");
    }

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
    let mut exported: Vec<_> = events
        .iter()
        .filter(|event| event["ph"] == "X" && event["cat"] == "ProducedFrame")
        .map(|event| {
            (
                process_name(&event["pid"]),
                event["args"]["presentation"].clone(),
            )
        })
        .collect();
    exported.dedup();
    let expected: Vec<_> = partitions
        .iter()
        .map(|(runtime, presentation)| {
            (
                serde_json::json!(format!(
                    "FLUI runtime {:#x}",
                    runtime.expect("named").as_u64()
                )),
                serde_json::json!(presentation.expect("named").as_u64()),
            )
        })
        .collect();
    exported.sort_by_key(|(process, _)| process.to_string());
    let mut expected = expected;
    expected.sort_by_key(|(process, _)| process.to_string());
    assert_eq!(exported, expected);
}

/// A produced frame carries the timings, outcome and inputs of its telemetry, and a GPU time
/// joins the frame its runtime, presentation and number name: that frame's trace event carries
/// it as `gpu_us`, and every other frame carries none (ADR-0388). The headless sink measures no
/// GPU time, so the time is traced here in the runtime's `frame_gpu_time` shape, naming a frame
/// the runtime really produced.
///
/// Fails when the capture does not admit the `frame_gpu_time` event, or joins it by frame number
/// alone.
#[test]
fn a_timed_frame_carries_its_gpu_time_into_the_trace() {
    let capture = Capture::new();
    let timeline = Timeline::over(&capture);
    crate::profiler::capturing(&capture, || {
        let _first = pumped_host(2);
        let _second = pumped_host(1);
    });
    let produced = capture.produced_frames();
    let timed: ProducedFrame = produced
        .first()
        .cloned()
        .expect("the first host produced frames");
    assert!(
        produced
            .iter()
            .any(|frame| frame.runtime() != timed.runtime() && frame.number() == timed.number()),
        "another runtime's frame has the same number: {produced:?}"
    );
    for frame in &produced {
        assert_eq!(frame.outcome(), FrameOutcome::Presented);
        assert!(frame.segment() >= frame.build(), "{frame:?}");
        assert_eq!(frame.gpu_time(), None, "no time has arrived yet");
    }
    crate::profiler::capturing(&capture, || {
        tracing::trace!(
            target: "flui.frame",
            event = "frame_gpu_time",
            ui_runtime_id = timed.runtime().as_u64(),
            presentation = timed.presentation().as_u64(),
            frame_id = timed.number().get(),
            epoch = 1_u64,
            gpu_us = 420_u64,
            "frame GPU time"
        );
    });

    let trace: serde_json::Value =
        serde_json::from_str(&timeline.export_chrome_trace()).expect("the export is JSON");
    let gpu: Vec<_> = trace["traceEvents"]
        .as_array()
        .expect("traceEvents")
        .iter()
        .filter(|event| event["cat"] == "ProducedFrame")
        .map(|event| {
            (
                event["pid"].clone(),
                event["args"]["frame_id"].as_u64(),
                event["args"]["gpu_us"].as_u64(),
            )
        })
        .filter(|(_, _, gpu)| gpu.is_some())
        .collect();
    assert_eq!(gpu.len(), 1, "one frame was timed: {gpu:?}");
    assert_eq!(gpu[0].1, Some(timed.number().get()));
    assert_eq!(gpu[0].2, Some(420));
    assert_eq!(
        capture
            .produced_frames_of(timed.runtime())
            .first()
            .and_then(ProducedFrame::gpu_time),
        Some(Duration::from_micros(420))
    );
}
