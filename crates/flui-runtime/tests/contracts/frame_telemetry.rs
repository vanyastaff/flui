//! What a frame's telemetry names (ADR-0266 §3): the UI runtime as well as the
//! presentation, so the frames of two UI runtimes on one owner thread stay
//! apart in one trace even though each numbers its frames from 1.

use std::sync::{Arc, atomic::AtomicBool};

use flui_foundation::ManualClock;
use flui_runtime::sink::{FrameSink, SubmitVerdict};
use flui_runtime::ui_runtime::{RuntimeHostServices, UiRuntime};
use flui_testing::log_capture::{CapturedRecord, capture};

struct Sink;

impl FrameSink for Sink {
    fn surface_size(&mut self) -> (u32, u32) {
        (800, 600)
    }

    fn submit(
        &mut self,
        _epoch: flui_foundation::FrameEpoch,
        _: flui_layer::ScenePublication,
    ) -> SubmitVerdict {
        SubmitVerdict::Presented
    }
}

fn runtime(clock: &ManualClock, fonts: &flui_painting::FontCollection) -> UiRuntime {
    let runtime = UiRuntime::try_new(
        crate::owner_publication::window(),
        1.0,
        RuntimeHostServices::new(
            Arc::new(|| {}),
            Arc::new(AtomicBool::new(false)),
            fonts,
            flui_scheduler::ClockSource::Manual(clock.clone()),
        ),
    )
    .expect("telemetry runtime");
    runtime
        .attach_root_widget_with_size(&flui_widgets::SizedBox::shrink(), 800.0, 600.0)
        .expect("telemetry root");
    runtime
}

fn frame_events(records: &[CapturedRecord]) -> Vec<&CapturedRecord> {
    records
        .iter()
        .filter(|record| {
            record.target == "flui.frame" && record.field("event") == Some("frame_telemetry")
        })
        .collect()
}

#[test]
fn frames_of_two_ui_runtimes_name_their_runtime() {
    let mut clock = ManualClock::new();
    let fonts = flui_painting::FontCollection::new();
    let mut first = runtime(&clock, &fonts);
    let mut second = runtime(&clock, &fonts);

    let ((), log) = capture(|| {
        assert!(first.pump(&mut clock, &mut Sink).presented());
        assert!(second.pump(&mut clock, &mut Sink).presented());
    });
    let events = frame_events(log.records());
    assert_eq!(
        events.len(),
        2,
        "one telemetry event per presented frame: {events:?}"
    );

    let named = |event: &CapturedRecord| {
        (
            event.field("ui_runtime_id").map(str::to_owned),
            event.field("presentation").map(str::to_owned),
            event.field("frame_id").map(str::to_owned),
        )
    };
    let (first_event, second_event) = (named(events[0]), named(events[1]));
    assert_eq!(
        first_event.2, second_event.2,
        "each UI runtime numbers its own frames, so the frame id alone cannot tell them apart"
    );
    assert_eq!(
        first_event.0.as_deref(),
        Some(first.id().as_u64().to_string().as_str()),
        "the first runtime's frame names the first runtime"
    );
    assert_eq!(
        second_event.0.as_deref(),
        Some(second.id().as_u64().to_string().as_str()),
        "the second runtime's frame names the second runtime"
    );
    assert_eq!(
        first_event.1.as_deref(),
        Some(first.presentation_id().to_string().as_str())
    );
    assert_eq!(
        second_event.1.as_deref(),
        Some(second.presentation_id().to_string().as_str())
    );
}

/// A sink that presents each frame and, like a GPU slot, hands back a frame's
/// GPU time once a later frame was submitted, plus one time for a frame this
/// runtime never made.
#[derive(Default)]
struct TimedSink {
    submitted: Vec<flui_foundation::FrameEpoch>,
    answered: usize,
}

impl FrameSink for TimedSink {
    fn surface_size(&mut self) -> (u32, u32) {
        (800, 600)
    }

    fn submit(
        &mut self,
        epoch: flui_foundation::FrameEpoch,
        _: flui_layer::ScenePublication,
    ) -> SubmitVerdict {
        self.submitted.push(epoch);
        SubmitVerdict::Presented
    }

    fn take_gpu_times(&mut self, into: &mut Vec<flui_foundation::GpuFrameTime>) {
        let finished = self.submitted.len().saturating_sub(1);
        for &epoch in &self.submitted[self.answered..finished] {
            into.push(flui_foundation::GpuFrameTime::new(
                epoch,
                std::time::Duration::from_micros(250),
            ));
        }
        if finished > self.answered {
            into.push(flui_foundation::GpuFrameTime::new(
                flui_foundation::FrameEpoch::from_raw(9_999),
                std::time::Duration::from_micros(1),
            ));
        }
        self.answered = self.answered.max(finished);
    }
}

/// A frame's GPU time reaches the runtime after the frame's own telemetry was
/// sent, so it is traced as an event of its own that names the frame; a time
/// for a frame the runtime never made is dropped (ADR-0388).
///
/// Red-check: skip `record_gpu_times` after a submit and no event is traced.
#[test]
fn a_frames_gpu_time_is_traced_once_it_arrives() {
    let mut clock = ManualClock::new();
    let fonts = flui_painting::FontCollection::new();
    let mut runtime = runtime(&clock, &fonts);
    let mut sink = TimedSink::default();

    let ((), log) = capture(|| {
        assert!(runtime.pump(&mut clock, &mut sink).presented());
        runtime.mark_primary_needs_full_repaint();
        clock.advance(std::time::Duration::from_millis(16));
        assert!(runtime.pump(&mut clock, &mut sink).presented());
    });
    let frames = frame_events(log.records());
    let timed: Vec<_> = log
        .records()
        .iter()
        .filter(|record| {
            record.target == "flui.frame" && record.field("event") == Some("frame_gpu_time")
        })
        .collect();
    assert_eq!(
        timed.len(),
        1,
        "one time, for the first frame; the stranger's is dropped: {timed:?}"
    );
    assert_eq!(timed[0].field("gpu_us"), Some("250"));
    assert_eq!(
        timed[0].field("frame_id"),
        frames[0].field("frame_id"),
        "the time names the frame it was measured for"
    );
    assert_eq!(
        timed[0].field("epoch"),
        Some(sink.submitted[0].get().to_string().as_str())
    );
    assert_eq!(
        timed[0].field("ui_runtime_id"),
        Some(runtime.id().as_u64().to_string().as_str())
    );
}
