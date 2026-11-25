//! `perf.start` and `perf.stop` as the agent serves them (ADR-0238 §10): the frames a window
//! presents between the two, counted and judged against its display's refresh period, each
//! caller's span its own, and the answer to each call out of order.

use std::num::NonZeroU64;
use std::time::Duration;

use flui_foundation::{FrameEpoch, PresentationAddress};
use flui_platform::{DisplayKey, DisplayRegistry, RefreshPeriod, WindowDisplay};
use flui_protocol::error::{ErrorCode, ErrorObject, Retry};
use flui_protocol::method;
use flui_protocol::perf::{FramePhase, PerfReport, PerfStartParams};
use flui_protocol::session::WindowParams;
use flui_scheduler::{FramePhaseDurations, FrameTimes, PresentOutcome};
use flui_view::__runtime::LeaseId;
use flui_view::dev_agent::{AgentWindow, InputLease};

use super::agent_semantics::{answer, counter, frame};
use super::*;
use crate::ui_runtime::agent_perf::{
    MAX_SAMPLES, MAX_SPANS, MAX_WORST, PerfSpan, SlowFrame, percentile,
};

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

/// Records on the primary window's frame clock a presented frame whose segment took `segment`
/// and whose submit returned `submit` after it, as a sink's settlement does, without running a
/// frame: no span takes it until the next frame or `perf.stop`.
fn record_presented(ui_runtime: &UiRuntime, segment: Duration, submit: Duration) {
    let state = ui_runtime
        .presentations
        .get(ui_runtime.presentation_id())
        .expect("the primary presentation is hosted");
    let clock = state.clock();
    let start = flui_scheduler::Instant::now();
    let times = FrameTimes {
        clock_timestamp: start,
        segment_start: start,
        segment_end: start + segment,
        submit_at: start + segment + submit,
        phases: FramePhaseDurations::new(segment / 4, segment / 4, segment / 2),
    };
    let address = PresentationAddress {
        ui_runtime_id: ui_runtime.id(),
        presentation_id: ui_runtime.presentation_id(),
    };
    let _snapshot = clock.record_frame(
        clock.telemetry_cut(),
        address,
        FrameEpoch::ZERO,
        times,
        PresentOutcome::Presented,
    );
}

#[expect(
    clippy::result_large_err,
    reason = "the protocol's error object is the answer under test"
)]
fn start(ui_runtime: &UiRuntime, lease: &InputLease) -> Result<(), ErrorObject> {
    start_with(ui_runtime, lease, PerfStartParams::new(lease.window()))
}

/// `perf.start` asked with `params` through `lease`.
#[expect(
    clippy::result_large_err,
    reason = "the protocol's error object is the answer under test"
)]
fn start_with(
    ui_runtime: &UiRuntime,
    lease: &InputLease,
    params: PerfStartParams,
) -> Result<(), ErrorObject> {
    let reply = lease
        .call::<method::PerfStart>(params)
        .expect("the inbox has room");
    answer(ui_runtime, reply).map(|_| ())
}

#[expect(
    clippy::result_large_err,
    reason = "the protocol's error object is the answer under test"
)]
fn stop(ui_runtime: &UiRuntime, lease: &InputLease) -> Result<PerfReport, ErrorObject> {
    let reply = lease
        .call::<method::PerfStop>(WindowParams::new(lease.window()))
        .expect("the inbox has room");
    answer(ui_runtime, reply)
}

/// How many spans the primary window keeps.
fn running(ui_runtime: &UiRuntime) -> usize {
    ui_runtime
        .presentations
        .get(ui_runtime.presentation_id())
        .expect("the primary presentation is hosted")
        .dev_agent
        .borrow()
        .as_ref()
        .expect("the agent's slot outlives its handles")
        .perf
        .running()
}

/// Asserts `refused` is `code` and its reason names `names`, so the caller knows what to do.
fn assert_refused(refused: &ErrorObject, code: ErrorCode, names: &str) {
    assert_eq!(refused.code, code, "{refused:?}");
    assert!(
        refused.message.contains(names),
        "the reason names {names}: {refused:?}"
    );
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
        let lease = window.input_lease().expect("a lease");
        if let Some(period) = period {
            on_display(&ui_runtime, period);
        }
        presented_frame(&ui_runtime, &mut sink);
        start(&ui_runtime, &lease).expect("the span starts");
        for _ in 0..7 {
            presented_frame(&ui_runtime, &mut sink);
        }
        // A frame whose submit fails is recorded, and is not a presented frame.
        let mut failing = ScriptedSink::new(|_, _| crate::sink::SubmitVerdict::Retry);
        presented_frame(&ui_runtime, &mut failing);
        let report = stop(&ui_runtime, &lease).expect("the span reports");
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

/// A frame's time is its segment's work, without the submit: a 4 ms frame whose submit returns
/// 30 ms later, waiting for the next vsync as a blocking present does, reads 4 ms and makes its
/// 60 Hz deadline, while its `submit` phase shows the wait. `perf.stop` takes the frames
/// recorded since the last frame before it answers, and a pull that takes several frames
/// resumes after the last of them.
#[test]
fn a_frame_time_is_its_work_without_the_wait_for_the_next_vsync() {
    let (ui_runtime, window, mut sink) = shown();
    let lease = window.input_lease().expect("a lease");
    on_display(&ui_runtime, Duration::from_micros(16_667));
    start(&ui_runtime, &lease).expect("the span starts");
    record_presented(
        &ui_runtime,
        Duration::from_millis(4),
        Duration::from_millis(30),
    );
    let report = stop(&ui_runtime, &lease).expect("the span reports");
    assert_eq!(report.frames, 1, "stop took the frame: {report:#?}");
    assert_eq!(report.frame_time.max, 4.0, "{report:#?}");
    assert_eq!(
        (report.dropped, report.janky),
        (Some(0), Some(0)),
        "{report:#?}"
    );
    let submit = report.worst[0]
        .phases
        .iter()
        .find(|phase| phase.phase == FramePhase::Submit)
        .expect("the submit phase is reported");
    assert_eq!(submit.ms, 30.0, "{report:#?}");

    // Two frames recorded between pulls, then a frame: the pull after it takes all three, and
    // the stop finds nothing new. A frame first, so the span starts after a frame the clock
    // counted (a recorded frame shares the id of the last one the clock produced).
    presented_frame(&ui_runtime, &mut sink);
    start(&ui_runtime, &lease).expect("a new span starts");
    for _ in 0..2 {
        record_presented(&ui_runtime, Duration::from_millis(1), Duration::ZERO);
    }
    presented_frame(&ui_runtime, &mut sink);
    let report = stop(&ui_runtime, &lease).expect("the span reports");
    assert_eq!(report.frames, 3, "each frame once: {report:#?}");
}

/// Each count against a 10 ms period, as one frame of a span: on time at the period, dropped
/// past it, and janky too past `jank_multiplier` periods; the worst hitch, a 100 ms frame at
/// 60 Hz, is dropped and janky, so a budget of no dropped frame holds it.
#[test]
fn each_frame_is_dropped_past_one_period_and_janky_past_the_multiplier() {
    let ten = Duration::from_millis(10);
    for (name, period, ms, multiplier, counts) in [
        ("at the period", ten, 10.0, 2.0, (0, 0)),
        ("past the period", ten, 10.5, 2.0, (1, 0)),
        ("at the multiplier", ten, 20.0, 2.0, (1, 0)),
        ("past the multiplier", ten, 20.5, 2.0, (1, 1)),
        ("a multiplier of one", ten, 10.5, 1.0, (1, 1)),
        (
            "the worst hitch at 60 Hz",
            Duration::from_micros(16_667),
            100.0,
            2.0,
            (1, 1),
        ),
    ] {
        let mut span = PerfSpan::new(caller(1), None, multiplier, 5);
        span.sample(slow(1, ms), Some(period));
        let report = span.report(Some(period));
        assert_eq!(
            (report.dropped, report.janky),
            (Some(counts.0), Some(counts.1)),
            "{name}: {report:#?}"
        );
    }
}

/// `refresh_period_ms` is the period every frame was judged against: a frame judged at 10 ms
/// keeps 10 ms when the display changes before the stop; frames judged against two periods
/// report none and each count by its own; a frame with no period leaves both counts unjudged;
/// a span with no frame reports the period at the stop.
#[test]
fn a_report_names_the_period_its_frames_were_judged_against() {
    let ten = Some(Duration::from_millis(10));
    let twenty = Some(Duration::from_millis(20));
    for (name, frames, at_stop, expected) in [
        ("one period", vec![ten, ten], twenty, (Some(10.0), Some(2))),
        ("two periods", vec![ten, twenty], ten, (None, Some(1))),
        ("an unknown period", vec![ten, None], ten, (None, None)),
        ("unknown, then known", vec![None, ten], ten, (None, None)),
        ("no frame", vec![], twenty, (Some(20.0), Some(0))),
        ("no frame and no period", vec![], None, (None, None)),
    ] {
        let mut span = PerfSpan::new(caller(1), None, 2.0, 5);
        for (frame, period) in (1..).zip(frames) {
            // 15 ms: dropped at 10 ms, on time at 20 ms.
            span.sample(slow(frame, 15.0), period);
        }
        let report = span.report(at_stop);
        assert_eq!(
            (report.refresh_period_ms, report.dropped),
            expected,
            "{name}: {report:#?}"
        );
        assert_eq!(
            report.janky,
            report.dropped.map(|_| 0),
            "{name}: {report:#?}"
        );
    }

    // Through the window: a frame taken on a 10 ms display, the display moved to a one-second
    // one before the stop. The frame that makes the span take it is not presented, so only
    // the recorded frame is counted.
    let (ui_runtime, window, _sink) = shown();
    let lease = window.input_lease().expect("a lease");
    on_display(&ui_runtime, Duration::from_millis(10));
    start(&ui_runtime, &lease).expect("the span starts");
    record_presented(&ui_runtime, Duration::from_millis(15), Duration::ZERO);
    let mut failing = ScriptedSink::new(|_, _| crate::sink::SubmitVerdict::Retry);
    presented_frame(&ui_runtime, &mut failing);
    on_display(&ui_runtime, Duration::from_secs(1));
    let report = stop(&ui_runtime, &lease).expect("the span reports");
    assert_eq!(report.refresh_period_ms, Some(10.0), "{report:#?}");
    assert_eq!(report.dropped, Some(1), "the 15 ms frame: {report:#?}");
}

/// Each call out of order has its answer: a call through no lease is refused and starts
/// nothing, a stop with no span and a second start by the same caller are refused
/// `invalid_argument`, and a stop after the window closed answers `gone`, whether it was asked
/// before the close or after.
#[test]
fn each_perf_call_out_of_order_has_a_defined_answer() {
    let (mut ui_runtime, window, mut sink) = shown();
    let lease = window.input_lease().expect("a lease");
    let reply = window
        .call::<method::PerfStart>(PerfStartParams::new(window.id()))
        .expect("the inbox has room");
    let refused = answer(&ui_runtime, reply).expect_err("no lease names the caller");
    assert_refused(&refused, ErrorCode::InvalidArgument, "lease");
    assert_eq!(running(&ui_runtime), 0, "the refusal started nothing");
    let reply = window
        .call::<method::PerfStop>(WindowParams::new(window.id()))
        .expect("the inbox has room");
    let refused = answer(&ui_runtime, reply).expect_err("no lease names the caller");
    assert_refused(&refused, ErrorCode::InvalidArgument, "lease");

    let refused = stop(&ui_runtime, &lease).expect_err("no span runs");
    assert_refused(&refused, ErrorCode::InvalidArgument, "perf.start");
    start(&ui_runtime, &lease).expect("the span starts");
    let again = start(&ui_runtime, &lease).expect_err("the caller's span already runs");
    assert_refused(&again, ErrorCode::InvalidArgument, "perf.stop");
    presented_frame(&ui_runtime, &mut sink);
    let report = stop(&ui_runtime, &lease).expect("the first span still runs");
    assert_eq!(
        report.frames, 1,
        "the refused start left the span as it was"
    );

    // A window that closes between the two.
    let second = ui_runtime.install_second_presentation_for_test();
    let other = ui_runtime
        .dev_agent_window(second)
        .expect("the ui_runtime hosts the second presentation");
    let other_lease = other.input_lease().expect("a lease");
    frame(&ui_runtime, &mut sink);
    start(&ui_runtime, &other_lease).expect("the span starts");
    // Asked before the close, answered after it.
    let queued = other_lease
        .call::<method::PerfStop>(WindowParams::new(other.id()))
        .expect("the inbox has room");
    assert!(
        ui_runtime.close_presentation_entered(second),
        "the window closes"
    );
    let gone = answer(&ui_runtime, queued).expect_err("the window closed");
    assert_eq!(gone.code, ErrorCode::Gone, "{gone:?}");
    // Asked after it: the closed window refuses the call itself, or its owner answers it gone.
    let gone = match other_lease.call::<method::PerfStop>(WindowParams::new(other.id())) {
        Err(refused) => refused,
        Ok(reply) => answer(&ui_runtime, reply).expect_err("the window closed"),
    };
    assert_eq!(gone.code, ErrorCode::Gone, "{gone:?}");
}

/// The lease `value`.
fn caller(value: u64) -> LeaseId {
    LeaseId::new(NonZeroU64::new(value).expect("a lease is never zero"))
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
/// janky past two and dropped too; the percentiles by nearest rank; the worst frames slowest
/// first and capped, the earliest kept among equal times.
#[test]
fn a_span_judges_each_frame_against_the_period_and_keeps_its_slowest() {
    let period = Some(Duration::from_millis(10));
    let mut span = PerfSpan::new(caller(1), None, 2.0, 3);
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
        Some(3),
        "15, 20 and 25 ms miss one period: {report:#?}"
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

    // Three frames of one time, two kept: the earliest two, in order.
    let mut span = PerfSpan::new(caller(1), None, 2.0, 2);
    for frame in 1..=3 {
        span.sample(slow(frame, 10.0), period);
    }
    let worst: Vec<u64> = span
        .report(period)
        .worst
        .iter()
        .map(|sample| sample.frame)
        .collect();
    assert_eq!(worst, vec![1, 2], "the earliest of equal times");
}

/// Past the cap the oldest times give way: the percentiles describe the last frames, the count
/// and the largest time the whole span. An empty span reports zeros.
#[test]
fn a_span_keeps_a_bounded_window_of_times() {
    let mut span = PerfSpan::new(caller(1), None, 2.0, MAX_WORST);
    let slow_frames = 50;
    for frame in 1..=slow_frames {
        span.sample(slow(frame, 500.0), None);
    }
    let samples = u64::try_from(MAX_SAMPLES).expect("fits");
    for frame in slow_frames + 1..=slow_frames + samples {
        span.sample(slow(frame, 1.0), None);
    }
    let report = span.report(None);
    assert_eq!(report.frames, slow_frames + samples);
    assert_eq!(
        report.frame_time.p99, 1.0,
        "the 500 ms frames left the window"
    );
    assert_eq!(
        report.frame_time.max, 500.0,
        "but the largest covers the span"
    );
    assert_eq!(report.worst[0].frame, 1);

    let empty = PerfSpan::new(caller(1), None, 2.0, 5).report(None);
    assert_eq!(empty.frames, 0);
    assert_eq!(empty.frame_time.p50, 0.0);
    assert_eq!(percentile(&[], 0.5), 0.0);
    assert_eq!(percentile(&[1.0, 2.0, 3.0, 4.0], 0.5), 2.0);
    assert_eq!(percentile(&[1.0, 2.0, 3.0, 4.0], 0.99), 4.0);
    assert_eq!(
        percentile(&[1.0, 2.0, 3.0, 4.0], 0.0),
        1.0,
        "rank 0 is the first"
    );
}

/// `perf.start`'s parameters are checked before a span starts: a jank multiplier that is not
/// finite and at least 1 is refused `invalid_argument` and starts nothing; `worst` reports at
/// most 64 frames whatever it asks, and as many as it asks below that.
#[test]
fn perf_start_checks_its_parameters() {
    let (ui_runtime, window, mut sink) = shown();
    let lease = window.input_lease().expect("a lease");
    for refused in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 0.5, 0.999] {
        let mut params = PerfStartParams::new(window.id());
        params.jank_multiplier = Some(refused);
        let error = start_with(&ui_runtime, &lease, params).expect_err("refused");
        assert_refused(&error, ErrorCode::InvalidArgument, "jank_multiplier");
        let stopped = stop(&ui_runtime, &lease).expect_err("the refusal started no span");
        assert_eq!(
            stopped.code,
            ErrorCode::InvalidArgument,
            "{refused}: {stopped:?}"
        );
    }
    let mut at_one = PerfStartParams::new(window.id());
    at_one.jank_multiplier = Some(1.0);
    start_with(&ui_runtime, &lease, at_one).expect("one period is a threshold");
    stop(&ui_runtime, &lease).expect("the span reports");

    for (name, worst, kept) in [
        ("past the cap", Some(1000), MAX_WORST),
        ("the largest u32", Some(u32::MAX), MAX_WORST),
        ("one asked", Some(1), 1),
        ("none asked", Some(0), 0),
    ] {
        let mut params = PerfStartParams::new(window.id());
        params.worst = worst;
        start_with(&ui_runtime, &lease, params).expect("the span starts");
        for _ in 0..MAX_WORST + 6 {
            presented_frame(&ui_runtime, &mut sink);
        }
        let report = stop(&ui_runtime, &lease).expect("the span reports");
        assert_eq!(report.worst.len(), kept, "{name}: {report:#?}");
    }
}

/// The percentiles are by nearest rank over the span's frames, each in its own slot: over the
/// times 1 to 100 ms the 50th, 90th, 95th and 99th percentiles are 50, 90, 95 and 99 ms.
#[test]
fn a_report_names_each_percentile_by_nearest_rank() {
    let mut span = PerfSpan::new(caller(1), None, 2.0, 5);
    // Out of order: the report sorts them.
    for ms in (1..=100).rev() {
        span.sample(slow(ms, f64::from(u32::try_from(ms).expect("fits"))), None);
    }
    let report = span.report(None);
    let time = report.frame_time;
    assert_eq!(
        (time.p50, time.p90, time.p95, time.p99, time.max),
        (50.0, 90.0, 95.0, 99.0, 100.0),
        "{report:#?}"
    );
}

/// Each caller keeps its own span: a second lease starts beside the first, and either stops
/// only its own, counted from its own start; a lease released without `perf.stop` takes only
/// its own span; a window keeps at most `MAX_SPANS` and refuses one more `busy` until one ends.
#[test]
fn each_caller_keeps_its_own_span_until_it_stops_or_goes() {
    let (ui_runtime, window, mut sink) = shown();
    let first = window.input_lease().expect("a lease");
    let second = window.input_lease().expect("a lease");

    start(&ui_runtime, &first).expect("the first caller's span starts");
    presented_frame(&ui_runtime, &mut sink);
    start(&ui_runtime, &second).expect("the second caller's span starts beside it");
    presented_frame(&ui_runtime, &mut sink);
    let report = stop(&ui_runtime, &second).expect("the second caller stops its own");
    assert_eq!(report.frames, 1, "from its own start: {report:#?}");
    let refused = stop(&ui_runtime, &second).expect_err("its span has ended");
    assert_eq!(refused.code, ErrorCode::InvalidArgument, "{refused:?}");
    presented_frame(&ui_runtime, &mut sink);
    let report = stop(&ui_runtime, &first).expect("the first span ran on");
    assert_eq!(report.frames, 3, "the other's stop left it: {report:#?}");

    // The second caller goes without stopping: the drain that serves the next call releases
    // its lease, and its span alone.
    start(&ui_runtime, &first).expect("a new span starts");
    start(&ui_runtime, &second).expect("a new span starts");
    presented_frame(&ui_runtime, &mut sink);
    drop(second);
    let _report = ui_runtime.drain_commands();
    assert_eq!(running(&ui_runtime), 1, "the released caller's span went");
    presented_frame(&ui_runtime, &mut sink);
    let report = stop(&ui_runtime, &first).expect("the first span ran on");
    assert_eq!(report.frames, 2, "{report:#?}");

    // At most MAX_SPANS: one more is refused until one ends.
    let callers: Vec<InputLease> = (0..MAX_SPANS)
        .map(|_| window.input_lease().expect("a lease"))
        .collect();
    for lease in &callers {
        start(&ui_runtime, lease).expect("a span starts");
    }
    let refused = start(&ui_runtime, &first).expect_err("the window keeps as many as it admits");
    assert_eq!(
        (refused.code, refused.retry),
        (ErrorCode::Busy, Retry::Soon),
        "{refused:?}"
    );
    drop(callers);
    let _report = ui_runtime.drain_commands();
    start(&ui_runtime, &first).expect("the released spans made room");
}
