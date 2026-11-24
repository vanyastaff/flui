//! The owner half of `perf.start` and `perf.stop` (ADR-0238 §10): a window's agent keeps the
//! frames the window presents between the two and answers a [`PerfReport`] over them.
//!
//! The frames are the window's own telemetry: the [`FrameSnapshot`]s its frame clock records
//! when a sink settles a frame. After each frame the runtime's drive reaches the agent through
//! [`AgentSide::after_frame`](super::AgentSide::after_frame), and a running span takes the
//! snapshots recorded since its cursor; `perf.stop` takes the last ones before it answers. A
//! window with no span running pays one borrow and one check per frame.
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
//! # Who owns a span
//!
//! A span is its starter's, a [`SpanOwner`]: the input lease the call was asked through, which
//! is one conversation's (a connection holds one lease per window, ADR-0400 §2), or, for a call
//! asked through no lease, the window handles every such caller shares. Only its owner stops
//! it. A lease's span goes when the lease is released, at the drain that releases what the
//! lease holds ([`DevAgentSlot::take_released`](super::agent::DevAgentSlot::take_released)), so
//! a conversation that ends without `perf.stop` leaves nothing that blocks the next one. A span
//! asked through no lease ends when the window handles are all dropped: the next frame, or the
//! next `perf` call, finds it orphaned and lets it go.
//!
//! # Edges
//!
//! - `perf.start` while a span runs on the window is refused `invalid_argument`, whoever owns
//!   it: the running span keeps its frames, and its owner's `perf.stop` ends it.
//! - `perf.stop` with no span running, or one another owner started, is refused
//!   `invalid_argument`, and the span runs on.
//! - A window that closes between the two drops its span with it; `perf.stop` answers `gone`,
//!   as every call on a closed window does.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::Weak;

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

/// Who started a span, and so who may stop it (module docs, "Who owns a span").
#[derive(Debug, Clone)]
pub(super) enum SpanOwner {
    /// The input lease the call was asked through: one conversation's. Released, it takes the
    /// span with it.
    Lease(LeaseId),
    /// The window handles a call asked through no lease shares: once none is alive nobody can
    /// stop it.
    Handles(Weak<SemanticsHandle>),
}

impl SpanOwner {
    /// The owner of a call asked through `lease`, or through none from a window whose handles
    /// share `handles`.
    fn of(lease: Option<LeaseId>, handles: Weak<SemanticsHandle>) -> Self {
        match lease {
            Some(lease) => Self::Lease(lease),
            None => Self::Handles(handles),
        }
    }

    /// Whether the owner can still stop the span: a lease until its release takes the span,
    /// the handles while one is alive.
    fn is_held(&self) -> bool {
        match self {
            Self::Lease(_) => true,
            Self::Handles(handles) => handles.strong_count() > 0,
        }
    }

    /// Whether `self` and `other` are the same caller.
    fn is(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Lease(own), Self::Lease(other)) => own == other,
            (Self::Handles(own), Self::Handles(other)) => Weak::ptr_eq(own, other),
            (Self::Lease(_), Self::Handles(_)) | (Self::Handles(_), Self::Lease(_)) => false,
        }
    }
}

/// The frames of one span between `perf.start` and `perf.stop`.
#[derive(Debug)]
pub(super) struct PerfSpan {
    /// Who started it, and may stop it.
    owner: SpanOwner,
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
        owner: SpanOwner,
        cursor: Option<FrameId>,
        jank_multiplier: f64,
        worst_kept: usize,
    ) -> Self {
        Self {
            owner,
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

    /// Whether its owner can still stop it.
    fn is_held(&self) -> bool {
        self.owner.is_held()
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

    /// Lets go the span one of `released` owns: its conversation has ended.
    pub(super) fn forget(&self, released: &[LeaseId]) {
        let mut span = self.span.borrow_mut();
        if span.as_ref().is_some_and(
            |span| matches!(span.owner, SpanOwner::Lease(lease) if released.contains(&lease)),
        ) {
            *span = None;
        }
    }

    /// Starts a span for `owner`, from what `presentation` recorded so far.
    fn start(
        &self,
        presentation: &PresentationState,
        owner: SpanOwner,
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
        *span = Some(PerfSpan::new(owner, cursor, jank_multiplier, worst_kept));
        Ok(())
    }

    /// Ends the span `owner` started and reports it, its last frames taken first.
    fn stop(
        &self,
        presentation: &PresentationState,
        owner: &SpanOwner,
    ) -> Result<PerfReport, AgentError> {
        let mut kept = self.span.borrow_mut();
        let mut span = match kept.take().filter(PerfSpan::is_held) {
            Some(span) if span.owner.is(owner) => span,
            Some(other) => {
                *kept = Some(other);
                return Err(AgentError::InvalidArgument {
                    reason: "the perf span on this window is another caller's; its perf.stop ends it",
                });
            }
            None => {
                return Err(AgentError::InvalidArgument {
                    reason: "no perf span runs on this window; perf.start begins one",
                });
            }
        };
        drop(kept);
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
        let owner = SpanOwner::of(ctx.lease, slot.window_handles());
        Answer::Now(
            slot.perf
                .start(ctx.state, owner, &params)
                .map(|()| Empty::default()),
        )
    }
}

impl Serve for method::PerfStop {
    /// Ends the span the caller started and reports its frames (module docs).
    fn serve(ctx: &mut ServeContext<'_, PerfReport>, _: WindowParams) -> Answer<PerfReport> {
        let slot = ctx.state.dev_agent.borrow();
        let Some(slot) = slot.as_ref() else {
            return Answer::Now(Err(AgentError::PresentationGone));
        };
        let owner = SpanOwner::of(ctx.lease, slot.window_handles());
        Answer::Now(slot.perf.stop(ctx.state, &owner))
    }
}
