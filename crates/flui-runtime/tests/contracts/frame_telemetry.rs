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

    fn submit(&mut self, _: flui_layer::Scene) -> SubmitVerdict {
        SubmitVerdict::Presented
    }
}

fn runtime(clock: &ManualClock, fonts: &flui_painting::FontCollection) -> UiRuntime {
    let runtime = UiRuntime::new(
        crate::owner_publication::window(),
        1.0,
        RuntimeHostServices::new(
            Arc::new(|| {}),
            Arc::new(AtomicBool::new(false)),
            Arc::new(flui_platform::InMemoryClipboard::new()),
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
