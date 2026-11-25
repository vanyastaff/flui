//! The owner half of `perf.start` and `perf.stop` (ADR-0238 §10): a window's agent keeps the
//! frames the window presents between the two and answers a [`PerfReport`] over them.
//!
//! The frames are the window's own telemetry: the [`FrameSnapshot`]s its frame clock records
//! when a sink settles a frame. After each frame the runtime's drive reaches the agent through
//! [`AgentSide::after_frame`](super::AgentSide::after_frame), and each running span takes the
//! snapshots recorded since its cursor; `perf.stop` takes the last ones before it answers. A
//! window with no span running pays one borrow per frame.
//!
//! # What a span counts
//!
//! A frame is one the window presented. Its frame time is the owner thread's work on it: its
//! segment, from the start of build to the end of paint (compositing and freezing the scene
//! included). The submit is not part of it: the instant the telemetry takes after the submit
//! includes the wait for the next vsync on a backend whose present blocks (wgpu's Fifo on
//! Vulkan and Wayland), and no instant separates that wait from the encoding and raster before
//! it. So a frame time is comparable across backends and holds no vsync wait, and raster work,
//! on the owner thread inside the submit or on a raster thread after the hand-off, is not
//! counted; the `submit` phase of a worst frame reports the submit's span, vsync wait included.
//!
//! Each frame is judged against the refresh period of the display the window is on when the
//! frame is taken, right after it is presented. A frame whose work exceeds one period missed
//! its present deadline and is dropped; a dropped frame whose work also exceeds
//! `jank_multiplier` periods is janky too, so `janky` counts the hitches among the dropped
//! frames and a budget on `dropped` holds them. A frame taken while the period was unknown
//! leaves both counts unjudged (`None`) for the span. The report's `refresh_period_ms` is the
//! period every frame was judged against, or `None` once frames were judged against different
//! ones (the window moved to another display) or one had none; a span with no frame reports
//! the period at its stop. Input-to-present latency is not joined here: `input_to_present` is
//! `None`.
//!
//! A span keeps the frame times of its last [`MAX_SAMPLES`] frames, the slowest
//! [`MAX_WORST`] frames at most, and counts every frame: past the cap the oldest time gives way,
//! so the percentiles describe the last [`MAX_SAMPLES`] frames while `frames`, the largest
//! time, `dropped`, `janky` and `worst` cover the whole span. Among frames of equal time the
//! earliest is kept and reported first. A span with no frame reports zero for every percentile.
//!
//! # Whose a span is
//!
//! A span is its caller's: the input lease the call was asked through ([`LeaseId`], minted
//! for one holder and never reused by the window), which over the wire is one connection's (a
//! connection holds one lease per window, ADR-0400 §2). `perf.start` and `perf.stop` asked
//! through no lease are refused `invalid_argument`, so no two callers share a span. A window
//! keeps one span per caller, at most [`MAX_SPANS`]: measuring only reads, so one caller's span
//! never refuses another's start, and a development tool that measures beside the driver
//! leaves the driver's span alone. A caller's span goes when its lease is released, at the
//! drain that releases what the lease holds
//! ([`DevAgentSlot::take_released`](super::agent::DevAgentSlot::take_released)), so a
//! connection that ends without `perf.stop` leaves nothing behind.
//!
//! # Edges
//!
//! - `perf.start` while the caller's own span runs is refused `invalid_argument`, and the
//!   span keeps its frames; with [`MAX_SPANS`] spans running it is refused `busy`.
//! - `perf.stop` from a caller with no span running is refused `invalid_argument`; the other
//!   callers' spans run on.
//! - A window that closes between the two drops its spans with it; `perf.stop` answers `gone`,
//!   as every call on a closed window does.

use std::cell::RefCell;
use std::collections::VecDeque;

use flui_protocol::method;
use flui_protocol::perf::{
    Distribution, FramePhase, FrameSample, PerfReport, PerfStartParams, PhaseTime,
};
use flui_protocol::session::{Empty, WindowParams};
use flui_scheduler::{FrameId, FrameSnapshot, PresentOutcome};
use flui_view::__runtime::LeaseId;
use web_time::Duration;

use super::UiRuntime;
use super::agent::AgentError;
use super::agent_serve::{Answer, Serve, ServeContext};
use crate::presentation::PresentationState;

/// How many frame times a span keeps for its percentiles: a minute at 60 Hz.
pub(super) const MAX_SAMPLES: usize = 3600;

/// The most slowest frames a span reports, whatever `worst` asks for.
pub(super) const MAX_WORST: usize = 64;

/// How many spans a window keeps at once, one per caller: twice the connections the agent's
/// service admits by default, with room for callers in the process. Each holds about 30 KiB.
pub(super) const MAX_SPANS: usize = 8;

/// How many slowest frames a span reports when `worst` is absent.
const DEFAULT_WORST: usize = 5;

/// The refresh periods of work that make a dropped frame janky when `jank_multiplier` is
/// absent: JankStats' default.
const DEFAULT_JANK_MULTIPLIER: f64 = 2.0;

/// A window agent's frame sampling: the spans `perf.start` began, one per caller. Owner thread
/// only.
#[derive(Debug, Default)]
pub(crate) struct PerfSampling {
    spans: RefCell<Vec<PerfSpan>>,
}

/// One slow frame, as the report names it.
#[derive(Debug, Clone, Copy)]
pub(super) struct SlowFrame {
    pub(super) frame: u64,
    /// The frame time: the segment's span.
    pub(super) ms: f64,
    pub(super) build_ms: f64,
    pub(super) layout_ms: f64,
    pub(super) paint_ms: f64,
    /// The submit's span, the vsync wait included where the backend's present blocks.
    pub(super) submit_ms: f64,
}

/// The refresh periods a span's frames were judged against so far.
#[derive(Debug, Clone, Copy, PartialEq)]
enum PeriodsSeen {
    /// No frame was taken yet.
    NoFrame,
    /// Every frame was judged against this period.
    Steady(Duration),
    /// Frames were judged against different periods.
    Changed,
    /// A frame was taken while the period was unknown: neither count can be judged.
    Unknown,
}

impl PeriodsSeen {
    /// What is seen once a frame judged against `period` is taken too.
    fn with(self, period: Option<Duration>) -> Self {
        match (self, period) {
            (Self::Unknown, _) | (_, None) => Self::Unknown,
            (Self::NoFrame, Some(period)) => Self::Steady(period),
            (Self::Steady(seen), Some(period)) if seen == period => self,
            (Self::Steady(_) | Self::Changed, Some(_)) => Self::Changed,
        }
    }
}

/// The frames of one caller's span between `perf.start` and `perf.stop`.
#[derive(Debug)]
pub(super) struct PerfSpan {
    /// Whose it is: only this caller stops it, and its release ends it.
    caller: LeaseId,
    /// The last snapshot taken; the next pull starts after it.
    cursor: Option<FrameId>,
    jank_multiplier: f64,
    worst_kept: usize,
    frames: u64,
    /// The frame times of the last [`MAX_SAMPLES`] frames, oldest first, in milliseconds.
    times: VecDeque<f64>,
    largest_ms: f64,
    /// The slowest frames so far, slowest first and the earliest first among equal times, at
    /// most `worst_kept`.
    slowest: Vec<SlowFrame>,
    dropped: u32,
    janky: u32,
    periods: PeriodsSeen,
}

/// `duration` in milliseconds.
fn ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

/// The refresh period of the display `presentation` is on, when known.
fn refresh_period(presentation: &PresentationState) -> Option<Duration> {
    presentation
        .window_display
        .get()
        .and_then(|display| display.refresh_period)
        .map(flui_platform::RefreshPeriod::get)
}

impl PerfSpan {
    /// `caller`'s span keeping `worst_kept` slowest frames, judging jank at `jank_multiplier`
    /// periods, taking the frames recorded after `cursor`.
    pub(super) fn new(
        caller: LeaseId,
        cursor: Option<FrameId>,
        jank_multiplier: f64,
        worst_kept: usize,
    ) -> Self {
        Self {
            caller,
            cursor,
            jank_multiplier,
            worst_kept,
            frames: 0,
            times: VecDeque::with_capacity(MAX_SAMPLES),
            largest_ms: 0.0,
            slowest: Vec::with_capacity(worst_kept),
            dropped: 0,
            janky: 0,
            periods: PeriodsSeen::NoFrame,
        }
    }

    /// Takes the frames `presentation` presented since the cursor, judged against its
    /// display's period now.
    pub(super) fn pull(&mut self, presentation: &PresentationState) {
        let snapshots = presentation.clock().frames_since(self.cursor);
        let Some(last) = snapshots.last() else {
            return;
        };
        self.cursor = Some(last.frame_id);
        let period = refresh_period(presentation);
        for snapshot in snapshots
            .iter()
            .filter(|snapshot| snapshot.present_outcome == PresentOutcome::Presented)
        {
            self.sample(slow_frame(snapshot), period);
        }
    }

    /// Counts one presented frame, judged against `period` when it is known.
    pub(super) fn sample(&mut self, frame: SlowFrame, period: Option<Duration>) {
        self.frames = self.frames.saturating_add(1);
        if self.times.len() == MAX_SAMPLES {
            self.times.pop_front();
        }
        self.times.push_back(frame.ms);
        self.largest_ms = self.largest_ms.max(frame.ms);
        self.periods = self.periods.with(period);
        if let Some(period) = period.map(ms)
            && frame.ms > period
        {
            self.dropped = self.dropped.saturating_add(1);
            if frame.ms > period * self.jank_multiplier {
                self.janky = self.janky.saturating_add(1);
            }
        }
        // After every kept frame at least as slow, so the earliest of equal times stays.
        let at = self.slowest.partition_point(|kept| kept.ms >= frame.ms);
        if at < self.worst_kept {
            if self.slowest.len() == self.worst_kept {
                self.slowest.pop();
            }
            self.slowest.insert(at, frame);
        }
    }

    /// The report over the span, with `period_now` the display's period at the stop.
    pub(super) fn report(&self, period_now: Option<Duration>) -> PerfReport {
        let mut sorted: Vec<f64> = self.times.iter().copied().collect();
        sorted.sort_by(f64::total_cmp);
        let mut report = PerfReport::default();
        report.frames = self.frames;
        report.frame_time = Distribution::new(
            percentile(&sorted, 0.50),
            percentile(&sorted, 0.90),
            percentile(&sorted, 0.95),
            percentile(&sorted, 0.99),
            self.largest_ms,
        );
        let (period, judged) = match self.periods {
            PeriodsSeen::NoFrame => (period_now, period_now.is_some()),
            PeriodsSeen::Steady(period) => (Some(period), true),
            PeriodsSeen::Changed => (None, true),
            PeriodsSeen::Unknown => (None, false),
        };
        report.refresh_period_ms = period.map(ms);
        report.dropped = judged.then_some(self.dropped);
        report.janky = judged.then_some(self.janky);
        report.worst = self
            .slowest
            .iter()
            .map(frame_sample)
            .collect::<Vec<FrameSample>>();
        report
    }
}

/// The `quantile` of `sorted` times by nearest rank: the smallest time at least that share of
/// them do not exceed. Zero for no times.
pub(super) fn percentile(sorted: &[f64], quantile: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    #[expect(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a rank among at most MAX_SAMPLES times, from a quantile in 0..=1"
    )]
    let rank = (quantile * sorted.len() as f64).ceil() as usize;
    sorted[rank.clamp(1, sorted.len()) - 1]
}

/// A presented frame's work and phases, in milliseconds.
fn slow_frame(snapshot: &FrameSnapshot) -> SlowFrame {
    SlowFrame {
        frame: snapshot.frame_id.get(),
        ms: ms(snapshot.segment_span()),
        build_ms: ms(snapshot.phase_duration(flui_scheduler::FramePhase::Build)),
        layout_ms: ms(snapshot.phase_duration(flui_scheduler::FramePhase::Layout)),
        paint_ms: ms(snapshot.phase_duration(flui_scheduler::FramePhase::Paint)),
        submit_ms: ms(snapshot.phase_duration(flui_scheduler::FramePhase::Composite)),
    }
}

/// `frame` on the wire, with the four phases the frame clock times.
fn frame_sample(frame: &SlowFrame) -> FrameSample {
    FrameSample::new(frame.frame, frame.ms).phases(vec![
        PhaseTime::new(FramePhase::Build, frame.build_ms),
        PhaseTime::new(FramePhase::Layout, frame.layout_ms),
        PhaseTime::new(FramePhase::Paint, frame.paint_ms),
        PhaseTime::new(FramePhase::Submit, frame.submit_ms),
    ])
}

impl PerfSampling {
    /// How many spans run.
    #[cfg(test)]
    pub(super) fn running(&self) -> usize {
        self.spans.borrow().len()
    }

    /// Takes the frames `presentation` presented since each running span's last pull.
    pub(super) fn after_frame(&self, presentation: &PresentationState) {
        for span in self.spans.borrow_mut().iter_mut() {
            span.pull(presentation);
        }
    }

    /// Lets go the spans `released` started: their callers have gone.
    pub(super) fn forget(&self, released: &[LeaseId]) {
        self.spans
            .borrow_mut()
            .retain(|span| !released.contains(&span.caller));
    }

    /// Starts `caller`'s span, from what `presentation` recorded so far.
    fn start(
        &self,
        presentation: &PresentationState,
        caller: LeaseId,
        params: &PerfStartParams,
    ) -> Result<(), AgentError> {
        let jank_multiplier = params.jank_multiplier.unwrap_or(DEFAULT_JANK_MULTIPLIER);
        if !(jank_multiplier.is_finite() && jank_multiplier >= 1.0) {
            return Err(AgentError::InvalidArgument {
                reason: "jank_multiplier is a finite number of refresh periods, at least 1",
            });
        }
        let worst_kept = params
            .worst
            .map_or(DEFAULT_WORST, |worst| {
                usize::try_from(worst).unwrap_or(MAX_WORST)
            })
            .min(MAX_WORST);
        let mut spans = self.spans.borrow_mut();
        if spans.iter().any(|span| span.caller == caller) {
            return Err(AgentError::InvalidArgument {
                reason: "this caller's perf span already runs on this window; its perf.stop ends it",
            });
        }
        if spans.len() == MAX_SPANS {
            return Err(AgentError::TooManyPerfSpans);
        }
        let cursor = presentation
            .clock()
            .frames_since(None)
            .last()
            .map(|snapshot| snapshot.frame_id);
        spans.push(PerfSpan::new(caller, cursor, jank_multiplier, worst_kept));
        Ok(())
    }

    /// Ends `caller`'s span and reports it, its last frames taken first.
    fn stop(
        &self,
        presentation: &PresentationState,
        caller: LeaseId,
    ) -> Result<PerfReport, AgentError> {
        let mut spans = self.spans.borrow_mut();
        let Some(at) = spans.iter().position(|span| span.caller == caller) else {
            return Err(AgentError::InvalidArgument {
                reason: "this caller has no perf span on this window; perf.start begins one",
            });
        };
        let mut span = spans.swap_remove(at);
        span.pull(presentation);
        Ok(span.report(refresh_period(presentation)))
    }
}

/// What `perf.start` or `perf.stop` asked through no lease answers: a span is its caller's,
/// and the lease is the caller.
fn unleased() -> AgentError {
    AgentError::InvalidArgument {
        reason: "perf.start and perf.stop are asked through an input lease, whose span it is",
    }
}

impl UiRuntime {
    /// A frame has passed: every window agent's running perf spans take the frames presented
    /// since their last pull.
    pub(super) fn sample_agent_perf(&self) {
        for presentation in self.presentations.iter() {
            if let Some(slot) = &*presentation.dev_agent.borrow() {
                slot.perf.after_frame(presentation);
            }
        }
    }
}

impl Serve for method::PerfStart {
    /// Begins keeping the window's presented frames for the caller (module docs, "Edges").
    fn serve(ctx: &mut ServeContext<'_, Empty>, params: PerfStartParams) -> Answer<Empty> {
        let slot = ctx.state.dev_agent.borrow();
        let Some(slot) = slot.as_ref() else {
            return Answer::Now(Err(AgentError::PresentationGone));
        };
        let Some(caller) = ctx.lease else {
            return Answer::Now(Err(unleased()));
        };
        Answer::Now(
            slot.perf
                .start(ctx.state, caller, &params)
                .map(|()| Empty::default()),
        )
    }
}

impl Serve for method::PerfStop {
    /// Ends the caller's span and reports its frames (module docs).
    fn serve(ctx: &mut ServeContext<'_, PerfReport>, _: WindowParams) -> Answer<PerfReport> {
        let slot = ctx.state.dev_agent.borrow();
        let Some(slot) = slot.as_ref() else {
            return Answer::Now(Err(AgentError::PresentationGone));
        };
        let Some(caller) = ctx.lease else {
            return Answer::Now(Err(unleased()));
        };
        Answer::Now(slot.perf.stop(ctx.state, caller))
    }
}
