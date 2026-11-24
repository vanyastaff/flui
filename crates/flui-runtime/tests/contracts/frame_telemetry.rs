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
        .attach_root_widget(&flui_widgets::SizedBox::shrink())
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
    // The ids are the packed numbers a reader rebuilds the typed ids from (ADR-0242 §7), not
    // their display text.
    assert_eq!(
        first_event.1.as_deref(),
        Some(first.presentation_id().as_u64().to_string().as_str())
    );
    assert_eq!(
        second_event.1.as_deref(),
        Some(second.presentation_id().as_u64().to_string().as_str())
    );
    assert_eq!(first_event.2.as_deref(), Some("1"), "a frame is its number");
}

/// A sink that presents each frame and, like a GPU slot, hands back its first frame's
/// GPU time once a later frame was submitted, plus one time for a frame this
/// runtime never made. After its second frame it also answers, ahead of time,
/// for the epoch the next frame will be stamped with: the order a queued frame
/// meets when its time is read back before its settlement is drained.
#[derive(Default)]
struct TimedSink {
    submitted: Vec<flui_foundation::FrameEpoch>,
    answered: usize,
}

/// The time [`TimedSink`] answers for a frame before it is recorded.
const AHEAD_US: u64 = 77;

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
        let finished = self.submitted.len().saturating_sub(1).min(1);
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
        if self.submitted.len() == 2 {
            let next = self.submitted[1].get() + 1;
            into.push(flui_foundation::GpuFrameTime::new(
                flui_foundation::FrameEpoch::from_raw(next),
                std::time::Duration::from_micros(AHEAD_US),
            ));
        }
        self.answered = self.answered.max(finished);
    }
}

/// A frame's GPU time reaches the runtime after the frame's own telemetry was
/// sent, so it is traced as an event of its own that names the frame; a time
/// for a frame the runtime never made traces nothing, and a time that comes
/// before its frame is recorded is traced with that frame (ADR-0388).
///
/// Red-check: skip `record_gpu_times` after a submit and no event is traced;
/// skip the GPU event in `record_submit_telemetry` and the early time is lost.
#[test]
fn a_frames_gpu_time_is_traced_once_it_arrives() {
    let mut clock = ManualClock::new();
    let fonts = flui_painting::FontCollection::new();
    let mut runtime = runtime(&clock, &fonts);
    let mut sink = TimedSink::default();

    let ((), log) = capture(|| {
        for _ in 0..3 {
            assert!(runtime.pump(&mut clock, &mut sink).presented());
            runtime.mark_primary_needs_full_repaint();
            clock.advance(std::time::Duration::from_millis(16));
        }
    });
    assert_eq!(
        sink.submitted[2].get(),
        sink.submitted[1].get() + 1,
        "the sink's guess of the next epoch held"
    );
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
        2,
        "the first frame's time and the third's early one; the stranger's traces nothing: \
         {timed:?}"
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
    assert_eq!(
        timed[0].field("presentation"),
        Some(runtime.presentation_id().as_u64().to_string().as_str()),
        "the presentation is its packed number"
    );
    assert_eq!(
        timed[1].field("gpu_us"),
        Some(AHEAD_US.to_string().as_str())
    );
    assert_eq!(
        timed[1].field("frame_id"),
        frames[2].field("frame_id"),
        "the early time waited for its own frame"
    );
}

/// A sink that refuses its first frame for a retry and panics the first time
/// the runtime asks for its GPU times.
#[derive(Default)]
struct PanickingTimesSink {
    submitted: usize,
    asked: usize,
}

impl FrameSink for PanickingTimesSink {
    fn surface_size(&mut self) -> (u32, u32) {
        (400, 400)
    }

    fn submit(
        &mut self,
        _: flui_foundation::FrameEpoch,
        _: flui_layer::ScenePublication,
    ) -> SubmitVerdict {
        self.submitted += 1;
        if self.submitted == 1 {
            SubmitVerdict::Retry
        } else {
            SubmitVerdict::Presented
        }
    }

    fn take_gpu_times(&mut self, _: &mut Vec<flui_foundation::GpuFrameTime>) {
        self.asked += 1;
        assert!(self.asked > 1, "the sink's GPU times failed");
    }
}

/// A sink that panics handing out its GPU times leaves its frame's tail to
/// run: the retry its refusal asked for still comes, and the panic resumes
/// after the tail (ADR-0388).
///
/// Red-check: call `take_gpu_times` outside `contain` and the retry never
/// comes.
#[test]
fn a_sink_that_panics_handing_out_gpu_times_still_gets_its_retry() {
    let mut runtime = crate::owner_publication::runtime_with_wake(std::sync::Arc::new(|| {}));
    runtime
        .attach_root_widget(&flui_widgets::Text::new("timed"))
        .expect("mount text");
    let mut sink = PanickingTimesSink::default();
    let mut now = web_time::Instant::now();

    now += std::time::Duration::from_millis(16);
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        runtime.pump(&mut flui_runtime::pump::SampledClock(now), &mut sink)
    }));
    let payload = failure.expect_err("the sink's panic resumes after the frame");
    assert_eq!(
        flui_foundation::panic::payload_text(payload.as_ref()),
        Some("the sink's GPU times failed")
    );
    assert_eq!(sink.submitted, 1);

    now += std::time::Duration::from_millis(16);
    let _ = runtime.pump(&mut flui_runtime::pump::SampledClock(now), &mut sink);
    assert_eq!(sink.submitted, 2, "the refused frame was retried");
}
