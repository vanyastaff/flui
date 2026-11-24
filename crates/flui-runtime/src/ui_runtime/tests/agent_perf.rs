//! `perf.start` and `perf.stop` as the agent serves them (ADR-0238 §10): the frames a window
//! presents between the two, counted and judged against its display's refresh period, and the
//! answer to each call out of order.

use std::time::Duration;

use flui_platform::{DisplayKey, DisplayRegistry, RefreshPeriod, WindowDisplay};
use flui_protocol::error::{ErrorCode, ErrorObject};
use flui_protocol::method;
use flui_protocol::perf::{PerfReport, PerfStartParams};
use flui_protocol::session::WindowParams;
use flui_view::dev_agent::AgentWindow;

use super::agent_semantics::{answer, counter, frame};
use super::*;
use crate::ui_runtime::agent_perf::{MAX_SAMPLES, MAX_WORST, PerfSpan, SlowFrame, percentile};

/// A runtime showing a counter, one frame in, and its window.
fn shown() -> (UiRuntime, AgentWindow, ScriptedSink) {
    let ui_runtime = UiRuntime::for_test();
    let _presses = counter(&ui_runtime, None);
    let window = ui_runtime
        .dev_agent_window(ui_runtime.presentation_id())
        .expect("the ui_runtime hosts its primary presentation");
    let mut sink = ScriptedSink::always_presents();
    frame(&ui_runtime, &mut sink);
    (ui_runtime, window, sink)
}

/// Puts the primary window on a display refreshing every `period`, as the host delivers it.
fn on_display(ui_runtime: &UiRuntime, period: Duration) {
    let display = WindowDisplay::new(
        DisplayRegistry::new()
            .mint(&DisplayKey::Injected(1))
            .expect("id"),
        Some(RefreshPeriod::try_from(period).expect("period")),
    );
    ui_runtime.apply_window_display(ui_runtime.presentation_id(), Some(display));
}

/// One frame that repaints the whole tree, so the window presents it.
fn presented_frame(ui_runtime: &UiRuntime, sink: &mut ScriptedSink) {
    ui_runtime.pipeline_for_test().with_mut(|pipeline| {
        if let Some(root) = pipeline.root_id() {
            pipeline.mark_needs_paint(root);
        }
    });
    ui_runtime.request_redraw();
    frame(ui_runtime, sink);
}

#[expect(
    clippy::result_large_err,
    reason = "the protocol's error object is the answer under test"
)]
fn start(ui_runtime: &UiRuntime, window: &AgentWindow) -> Result<(), ErrorObject> {
    let reply = window
        .call::<method::PerfStart>(PerfStartParams::new(window.id()))
        .expect("the inbox has room");
    answer(ui_runtime, reply).map(|_| ())
}

#[expect(
    clippy::result_large_err,
    reason = "the protocol's error object is the answer under test"
)]
fn stop(ui_runtime: &UiRuntime, window: &AgentWindow) -> Result<PerfReport, ErrorObject> {
    let reply = window
        .call::<method::PerfStop>(WindowParams::new(window.id()))
        .expect("the inbox has room");
    answer(ui_runtime, reply)
}

/// A span over a known number of presented frames counts exactly them, its worst frames are
/// the slowest first, and dropped and janky frames are judged against a known refresh period
/// and left unjudged without one.
#[test]
fn a_perf_span_counts_the_frames_presented_between_start_and_stop() {
    for (name, period) in [
        ("a known period", Some(Duration::from_secs(1))),
        ("an unknown period", None),
    ] {
        let (ui_runtime, window, mut sink) = shown();
        if let Some(period) = period {
            on_display(&ui_runtime, period);
        }
        presented_frame(&ui_runtime, &mut sink);
        start(&ui_runtime, &window).expect("the span starts");
        for _ in 0..7 {
            presented_frame(&ui_runtime, &mut sink);
        }
        // A frame whose submit fails is recorded, and is not a presented frame.
        let mut failing = ScriptedSink::new(|_, _| crate::sink::SubmitVerdict::Retry);
        presented_frame(&ui_runtime, &mut failing);
        let report = stop(&ui_runtime, &window).expect("the span reports");
        assert_eq!(report.frames, 7, "{name}: {report:#?}");
        assert_eq!(
            report.worst.len(),
            5,
            "{name}: the default five: {report:#?}"
        );
        assert!(
            report.worst.windows(2).all(|pair| pair[0].ms >= pair[1].ms),
            "{name}: slowest first: {report:#?}"
        );
        assert!(
            report.frame_time.max >= report.frame_time.p50 && report.frame_time.p50 > 0.0,
            "{name}: {report:#?}"
        );
        // A one-second period: no frame of the span comes near it.
        let judged = period.map_or((None, None, None), |_| (Some(1000.0), Some(0), Some(0)));
        assert_eq!(
            (report.refresh_period_ms, report.dropped, report.janky),
            judged,
            "{name}: {report:#?}"
        );
        assert!(report.input_to_present.is_none(), "{name}");
    }
}

/// Each call out of order has its answer: a stop with no span and a second start are refused
/// `invalid_argument`, a stop after the window closed answers `gone`, and a span whose window
/// handles all went is let go, so a later window starts from none.
#[test]
fn each_perf_call_out_of_order_has_a_defined_answer() {
    let (mut ui_runtime, window, mut sink) = shown();
    let refused = stop(&ui_runtime, &window).expect_err("no span runs");
    assert_eq!(refused.code, ErrorCode::InvalidArgument, "{refused:?}");

    start(&ui_runtime, &window).expect("the span starts");
    let again = start(&ui_runtime, &window).expect_err("a span already runs");
    assert_eq!(again.code, ErrorCode::InvalidArgument, "{again:?}");
    presented_frame(&ui_runtime, &mut sink);
    let report = stop(&ui_runtime, &window).expect("the first span still runs");
    assert_eq!(
        report.frames, 1,
        "the refused start left the span as it was"
    );

    // A span nobody stops ends with the handles that started it.
    start(&ui_runtime, &window).expect("a new span starts");
    drop(window);
    presented_frame(&ui_runtime, &mut sink);
    let state = ui_runtime
        .presentations
        .get(ui_runtime.presentation_id())
        .expect("the primary presentation is hosted");
    assert!(
        !state
            .dev_agent
            .borrow()
            .as_ref()
            .expect("the agent's slot outlives its handles")
            .perf
            .runs(),
        "the orphaned span is let go at the next frame"
    );
    let window = ui_runtime
        .dev_agent_window(ui_runtime.presentation_id())
        .expect("the ui_runtime hosts its primary presentation");
    let refused = stop(&ui_runtime, &window).expect_err("the new window has no span");
    assert_eq!(refused.code, ErrorCode::InvalidArgument, "{refused:?}");

    // A window that closes between the two.
    let second = ui_runtime.install_second_presentation_for_test();
    let other = ui_runtime
        .dev_agent_window(second)
        .expect("the ui_runtime hosts the second presentation");
    frame(&ui_runtime, &mut sink);
    start(&ui_runtime, &other).expect("the span starts");
    assert!(
        ui_runtime.close_presentation_entered(second),
        "the window closes"
    );
    // The closed window refuses the call itself, or its owner answers it gone.
    let gone = match other.call::<method::PerfStop>(WindowParams::new(other.id())) {
        Err(refused) => refused,
        Ok(reply) => answer(&ui_runtime, reply).expect_err("the window closed"),
    };
    assert_eq!(gone.code, ErrorCode::Gone, "{gone:?}");
}

/// A presented frame `ms` long.
fn slow(frame: u64, ms: f64) -> SlowFrame {
    SlowFrame {
        frame,
        ms,
        build_ms: ms / 4.0,
        layout_ms: ms / 4.0,
        paint_ms: ms / 4.0,
        submit_ms: ms / 4.0,
    }
}

/// Frames against a 10 ms period at a 2× jank threshold: on time, dropped past one period,
/// janky past two and not also dropped; the percentiles by nearest rank; the worst frames
/// slowest first and capped.
#[test]
fn a_span_judges_each_frame_against_the_period_and_keeps_its_slowest() {
    let period = Some(Duration::from_millis(10));
    let mut span = PerfSpan::new(std::sync::Weak::new(), None, 2.0, 3);
    for (frame, ms) in [
        (1, 5.0),
        (2, 10.0),
        (3, 15.0),
        (4, 20.0),
        (5, 25.0),
        (6, 1.0),
    ] {
        span.sample(slow(frame, ms), period);
    }
    let report = span.report(period);
    assert_eq!(report.frames, 6);
    assert_eq!(
        report.dropped,
        Some(2),
        "15 and 20 ms miss one period: {report:#?}"
    );
    assert_eq!(
        report.janky,
        Some(1),
        "25 ms is past two periods: {report:#?}"
    );
    let worst: Vec<u64> = report.worst.iter().map(|sample| sample.frame).collect();
    assert_eq!(
        worst,
        vec![5, 4, 3],
        "slowest first, three kept: {report:#?}"
    );
    assert_eq!(report.worst[0].phases.len(), 4, "{report:#?}");
    assert_eq!(report.frame_time.p50, 10.0, "{report:#?}");
    assert_eq!(report.frame_time.p90, 25.0, "{report:#?}");
    assert_eq!(report.frame_time.max, 25.0, "{report:#?}");

    // A frame taken without a period leaves both counts unjudged.
    span.sample(slow(7, 2.0), None);
    let report = span.report(period);
    assert_eq!((report.dropped, report.janky), (None, None), "{report:#?}");
}

/// Past the cap the oldest time gives way: the percentiles describe the last frames, the count
/// and the largest time the whole span. An empty span reports zeros.
#[test]
fn a_span_keeps_a_bounded_window_of_times() {
    let mut span = PerfSpan::new(std::sync::Weak::new(), None, 2.0, MAX_WORST);
    span.sample(slow(1, 500.0), None);
    for frame in 2..=u64::try_from(MAX_SAMPLES).expect("fits") + 1 {
        span.sample(slow(frame, 1.0), None);
    }
    let report = span.report(None);
    assert_eq!(report.frames, u64::try_from(MAX_SAMPLES).expect("fits") + 1);
    assert_eq!(
        report.frame_time.p99, 1.0,
        "the 500 ms frame left the window"
    );
    assert_eq!(
        report.frame_time.max, 500.0,
        "but the largest covers the span"
    );
    assert_eq!(report.worst[0].frame, 1);

    let empty = PerfSpan::new(std::sync::Weak::new(), None, 2.0, 5).report(None);
    assert_eq!(empty.frames, 0);
    assert_eq!(empty.frame_time.p50, 0.0);
    assert_eq!(percentile(&[], 0.5), 0.0);
    assert_eq!(percentile(&[1.0, 2.0, 3.0, 4.0], 0.5), 2.0);
    assert_eq!(percentile(&[1.0, 2.0, 3.0, 4.0], 0.99), 4.0);
}
