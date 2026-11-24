//! The owner half of `perf.start` and `perf.stop` (ADR-0238 §10): a window's agent keeps the
//! frames the window presents between the two and answers a [`PerfReport`] over them.
//!
//! The frames are the window's own telemetry: the [`FrameSnapshot`]s its frame clock records
//! when a sink settles a frame. After each frame the runtime's drive reaches the agent through
//! [`AgentSide::after_frame`](super::AgentSide::after_frame), and a running span takes the
//! snapshots recorded since its cursor; `perf.stop` takes the last ones before it answers. A
//! window with no span running pays one borrow and one check per frame and allocates nothing.
//!
//! # What a span counts
//!
//! A frame is one the window presented. Its frame time is its work: from its segment's start
//! (build, layout, paint) to the return of its submit. Against the refresh period of the
//! display the window is on, a frame whose work exceeds the period missed its present deadline
//! and is dropped; one whose work exceeds `jank_multiplier` periods is janky instead, so the two
//! counts never hold the same frame. While the period is unknown for any frame of the span,
//! neither count can be judged and both are `None`. Input-to-present latency is not joined
//! here: `input_to_present` is `None`.
//!
//! A span keeps the frame times of its last [`MAX_SAMPLES`] frames, the slowest
//! [`MAX_WORST`] frames at most, and counts every frame: past the cap the oldest time gives way,
//! so the percentiles describe the last [`MAX_SAMPLES`] frames while `frames`, the largest
//! time, `dropped`, `janky` and `worst` cover the whole span. A span with no frame reports zero
//! for every percentile.
//!
//! # Edges
//!
//! - `perf.start` while a span runs on the window is refused `invalid_argument`: the running
//!   span keeps its frames, and `perf.stop` ends it.
//! - `perf.stop` with no span running is refused `invalid_argument`.
//! - A window that closes between the two drops its span with it; `perf.stop` answers `gone`,
//!   as every call on a closed window does.
//! - A span nobody stops ends when the window handles that started it are all dropped: the
//!   next frame, or the next `perf` call, finds it orphaned and lets it go, so a vend after
//!   that starts from no span.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::Weak;

use flui_protocol::method;
use flui_protocol::perf::{
    Distribution, FramePhase, FrameSample, PerfReport, PerfStartParams, PhaseTime,
};
use flui_protocol::session::{Empty, WindowParams};
use flui_scheduler::{FrameId, FrameSnapshot, PresentOutcome};
use web_time::Duration;

use super::UiRuntime;
use super::agent::AgentError;
use super::agent_serve::{Answer, Serve, ServeContext};
use crate::presentation::PresentationState;
use crate::semantics_host::SemanticsHandle;

/// How many frame times a span keeps for its percentiles: a minute at 60 Hz.
pub(super) const MAX_SAMPLES: usize = 3600;

/// The most slowest frames a span reports, whatever `worst` asks for.
pub(super) const MAX_WORST: usize = 64;

/// How many slowest frames a span reports when `worst` is absent.
const DEFAULT_WORST: usize = 5;

/// The refresh periods of work that make a frame janky when `jank_multiplier` is absent.
const DEFAULT_JANK_MULTIPLIER: f64 = 2.0;

/// A window agent's frame sampling: the span `perf.start` began, if one runs. Owner thread only.
#[derive(Debug, Default)]
pub(crate) struct PerfSampling {
    span: RefCell<Option<PerfSpan>>,
}

/// One slow frame, as the report names it.
#[derive(Debug, Clone, Copy)]
pub(super) struct SlowFrame {
    pub(super) frame: u64,
    pub(super) ms: f64,
    pub(super) build_ms: f64,
    pub(super) layout_ms: f64,
    pub(super) paint_ms: f64,
    pub(super) submit_ms: f64,
}

/// The frames of one span between `perf.start` and `perf.stop`.
#[derive(Debug)]
pub(super) struct PerfSpan {
    /// The window handles that started it: once none is alive nobody can stop it.
    handles: Weak<SemanticsHandle>,
    /// The last snapshot taken; the next pull starts after it.
    cursor: Option<FrameId>,
    jank_multiplier: f64,
    worst_kept: usize,
    frames: u64,
    /// The frame times of the last [`MAX_SAMPLES`] frames, oldest first, in milliseconds.
    times: VecDeque<f64>,
    largest_ms: f64,
    /// The slowest frames so far, slowest first, at most `worst_kept`.
    slowest: Vec<SlowFrame>,
    dropped: u32,
    janky: u32,
    /// A frame of the span was taken while the display's period was unknown.
    unjudged: bool,
}

impl PerfSpan {
    /// A span keeping `worst_kept` slowest frames, judging jank at `jank_multiplier` periods.
    pub(super) fn new(
        handles: Weak<SemanticsHandle>,
        cursor: Option<FrameId>,
        jank_multiplier: f64,
        worst_kept: usize,
    ) -> Self {
        Self {
            handles,
            cursor,
            jank_multiplier,
            worst_kept,
            frames: 0,
            times: VecDeque::with_capacity(MAX_SAMPLES),
            largest_ms: 0.0,
            slowest: Vec::with_capacity(worst_kept),
            dropped: 0,
            janky: 0,
            unjudged: false,
        }
    }

    /// Whether a window handle that started the span is still alive.
    fn is_held(&self) -> bool {
        self.handles.strong_count() > 0
    }

    /// Takes the frames `presentation` recorded since the cursor, judged against its display's
    /// period now.
    fn pull(&mut self, presentation: &PresentationState) {
        let snapshots = presentation.clock().frames_since(self.cursor);
        let Some(last) = snapshots.last() else {
            return;
        };
        self.cursor = Some(last.frame_id);
        let period = presentation
            .window_display
            .get()
            .and_then(|display| display.refresh_period)
            .map(flui_platform::RefreshPeriod::get);
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
        match period.map(|period| period.as_secs_f64() * 1000.0) {
            Some(period) if frame.ms > period * self.jank_multiplier => {
                self.janky = self.janky.saturating_add(1);
            }
            Some(period) if frame.ms > period => self.dropped = self.dropped.saturating_add(1),
            Some(_) => {}
            None => self.unjudged = true,
        }
        if self.worst_kept > 0 {
            let at = self.slowest.partition_point(|kept| kept.ms >= frame.ms);
            if at < self.worst_kept {
                if self.slowest.len() == self.worst_kept {
                    self.slowest.pop();
                }
                self.slowest.insert(at, frame);
            }
        }
    }

    /// The report over the span, with `period` the display's period now.
    pub(super) fn report(&self, period: Option<Duration>) -> PerfReport {
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
        report.refresh_period_ms = period.map(|period| period.as_secs_f64() * 1000.0);
        let judged = !self.unjudged && period.is_some();
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
    let ms = |duration: Duration| duration.as_secs_f64() * 1000.0;
    SlowFrame {
        frame: snapshot.frame_id.get(),
        ms: ms(snapshot
            .submit_at
            .saturating_duration_since(snapshot.segment_start)),
        build_ms: ms(snapshot.phase_duration(flui_scheduler::FramePhase::Build)),
        layout_ms: ms(snapshot.phase_duration(flui_scheduler::FramePhase::Layout)),
        paint_ms: ms(snapshot.phase_duration(flui_scheduler::FramePhase::Paint)),
        submit_ms: ms(snapshot.phase_duration(flui_scheduler::FramePhase::Composite)),
    }
}

/// `frame` on the wire. The protocol gives a [`FrameSample`] no constructor outside its crate,
/// so it is built from its wire form, which names every field it has.
fn frame_sample(frame: &SlowFrame) -> FrameSample {
    let phases = [
        PhaseTime::new(FramePhase::Build, frame.build_ms),
        PhaseTime::new(FramePhase::Layout, frame.layout_ms),
        PhaseTime::new(FramePhase::Paint, frame.paint_ms),
        PhaseTime::new(FramePhase::Submit, frame.submit_ms),
    ];
    let wire = serde_json::json!({
        "frame": frame.frame,
        "ms": frame.ms,
        "phases": phases,
    });
    serde_json::from_value::<FrameSample>(wire)
        .expect("BUG: a frame sample's wire form names its fields with finite times")
}

impl PerfSampling {
    /// Whether a span is kept, orphaned or not.
    #[cfg(test)]
    pub(super) fn runs(&self) -> bool {
        self.span.borrow().is_some()
    }

    /// Takes the frames `presentation` presented since the last pull into the running span, or
    /// lets an orphaned span go.
    pub(super) fn after_frame(&self, presentation: &PresentationState) {
        let mut span = self.span.borrow_mut();
        match span.as_mut() {
            None => {}
            Some(running) if running.is_held() => running.pull(presentation),
            Some(_) => *span = None,
        }
    }

    /// Starts a span for the handles `handles`, from what `presentation` recorded so far.
    fn start(
        &self,
        presentation: &PresentationState,
        handles: Weak<SemanticsHandle>,
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
        let mut span = self.span.borrow_mut();
        if span.as_ref().is_some_and(PerfSpan::is_held) {
            return Err(AgentError::InvalidArgument {
                reason: "a perf span already runs on this window; perf.stop ends it",
            });
        }
        let cursor = presentation
            .clock()
            .frames_since(None)
            .last()
            .map(|snapshot| snapshot.frame_id);
        *span = Some(PerfSpan::new(handles, cursor, jank_multiplier, worst_kept));
        Ok(())
    }

    /// Ends the running span and reports it, its last frames taken first.
    fn stop(&self, presentation: &PresentationState) -> Result<PerfReport, AgentError> {
        let span = self.span.borrow_mut().take();
        let Some(mut span) = span.filter(PerfSpan::is_held) else {
            return Err(AgentError::InvalidArgument {
                reason: "no perf span runs on this window; perf.start begins one",
            });
        };
        span.pull(presentation);
        let period = presentation
            .window_display
            .get()
            .and_then(|display| display.refresh_period)
            .map(flui_platform::RefreshPeriod::get);
        Ok(span.report(period))
    }
}

impl UiRuntime {
    /// A frame has passed: every window agent's running perf span takes the frames presented
    /// since its last pull.
    pub(super) fn sample_agent_perf(&self) {
        for presentation in self.presentations.iter() {
            if let Some(slot) = &*presentation.dev_agent.borrow() {
                slot.perf.after_frame(presentation);
            }
        }
    }
}

impl Serve for method::PerfStart {
    /// Begins keeping the window's presented frames (module docs, "Edges").
    fn serve(ctx: &mut ServeContext<'_, Empty>, params: PerfStartParams) -> Answer<Empty> {
        let slot = ctx.state.dev_agent.borrow();
        let Some(slot) = slot.as_ref() else {
            return Answer::Now(Err(AgentError::PresentationGone));
        };
        Answer::Now(
            slot.perf
                .start(ctx.state, slot.window_handles(), &params)
                .map(|()| Empty::default()),
        )
    }
}

impl Serve for method::PerfStop {
    /// Ends the window's span and reports its frames (module docs).
    fn serve(ctx: &mut ServeContext<'_, PerfReport>, _: WindowParams) -> Answer<PerfReport> {
        let slot = ctx.state.dev_agent.borrow();
        let Some(slot) = slot.as_ref() else {
            return Answer::Now(Err(AgentError::PresentationGone));
        };
        Answer::Now(slot.perf.stop(ctx.state))
    }
}
