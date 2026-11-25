//! Frame performance over a span (ADR-0238 §10): what `perf.stop` reports and the budget
//! `perf.expect` judges it against.
//!
//! Times are milliseconds. Dropped and janky frames need the presentation's refresh period;
//! when it is unknown they are `None`, and a budget that names them cannot judge.

use crate::tree::WindowId;
use crate::verdict::Verdict;

/// `perf.start`: begin keeping frame samples for a window.
///
/// A span is its caller's, one per connection on a window: another connection's span never
/// refuses this one's `perf.start` and never ends with this one's `perf.stop`. A `perf.start`
/// while the caller's own span runs is refused `invalid_argument`, and so is a `perf.stop` with
/// none running; a window that keeps as many spans as it admits answers `busy`. A span whose
/// connection has gone is dropped.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct PerfStartParams {
    /// The window.
    pub window: WindowId,
    /// A dropped frame whose work exceeds this many refresh periods is janky too; 2 when
    /// absent. Finite and at least 1: any other value is refused `invalid_argument`.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub jank_multiplier: Option<f64>,
    /// How many of the worst frames to report; 5 when absent, at most 64: a larger number
    /// reports 64.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub worst: Option<u32>,
}

impl PerfStartParams {
    /// Samples for `window` with the default jank threshold.
    #[must_use]
    pub const fn new(window: WindowId) -> Self {
        Self {
            window,
            jank_multiplier: None,
            worst: None,
        }
    }
}

/// Percentiles of a set of times, in milliseconds.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct Distribution {
    /// The median.
    pub p50: f64,
    /// The 90th percentile.
    pub p90: f64,
    /// The 95th percentile.
    pub p95: f64,
    /// The 99th percentile.
    pub p99: f64,
    /// The largest.
    pub max: f64,
}

impl Distribution {
    /// The percentiles `p50`, `p90`, `p95`, `p99` and the largest.
    #[must_use]
    pub const fn new(p50: f64, p90: f64, p95: f64, p99: f64, max: f64) -> Self {
        Self {
            p50,
            p90,
            p95,
            p99,
            max,
        }
    }
}

/// Input-to-present latency in INP's three phases.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct Latency {
    /// Journal timestamp to present.
    pub total: Distribution,
    /// Journal timestamp to the frame that dispatched the input.
    pub input_delay: Distribution,
    /// That frame's build, layout and paint.
    pub processing: Distribution,
    /// Submit to present.
    pub presentation_delay: Distribution,
}

vocabulary! {
    /// A phase of a frame.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[cfg_attr(
        feature = "serde",
        derive(serde::Serialize, serde::Deserialize),
        serde(rename_all = "snake_case")
    )]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
    pub enum FramePhase {
        /// Input dispatch.
        Input => "input",
        /// Animation ticks and build.
        Build => "build",
        /// Layout.
        Layout => "layout",
        /// Paint.
        Paint => "paint",
        /// Semantics publish.
        Semantics => "semantics",
        /// Layer commit.
        Commit => "commit",
        /// Raster and submit.
        Submit => "submit",
    }
}

/// How long one phase of a frame took.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct PhaseTime {
    /// The phase.
    pub phase: FramePhase,
    /// How long, in milliseconds.
    pub ms: f64,
}

impl PhaseTime {
    /// `phase` took `ms`.
    #[must_use]
    pub const fn new(phase: FramePhase, ms: f64) -> Self {
        Self { phase, ms }
    }
}

/// One frame among the worst of a span.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct FrameSample {
    /// The frame's number in the window.
    pub frame: u64,
    /// Its frame time, in milliseconds, as [`PerfReport::frame_time`] measures it.
    pub ms: f64,
    /// Its phases. The `submit` phase runs from the end of paint to the submit's return, so
    /// it includes the wait for the next vsync where the backend's present blocks, and is not
    /// part of the frame time.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Vec::is_empty")
    )]
    pub phases: Vec<PhaseTime>,
    /// The `perf.tag` states active during it.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Vec::is_empty")
    )]
    pub tags: Vec<String>,
}

impl FrameSample {
    /// Frame `frame`, which took `ms`, with no phase and no tag.
    #[must_use]
    pub const fn new(frame: u64, ms: f64) -> Self {
        Self {
            frame,
            ms,
            phases: Vec::new(),
            tags: Vec::new(),
        }
    }

    /// With how long each of its phases took.
    #[must_use]
    pub fn phases(mut self, phases: Vec<PhaseTime>) -> Self {
        self.phases = phases;
        self
    }
}

/// What `perf.stop` answers for the span since `perf.start`.
#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct PerfReport {
    /// Frames presented.
    pub frames: u64,
    /// Frame times: the work of each frame on the thread that builds, lays out and paints it,
    /// from the start of build to the end of paint. The submit is not part of it: on a backend
    /// whose present blocks, the submit's span holds the wait for the next vsync, and raster
    /// work after the hand-off to a raster thread is not timed here at all.
    pub frame_time: Distribution,
    /// The refresh period every frame of the span was judged against, in milliseconds; for a
    /// span with no frame, the period at `perf.stop`. `None` when a frame had no known period,
    /// or when frames were judged against different ones (the window moved to a display with
    /// another period).
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub refresh_period_ms: Option<f64>,
    /// Frames that missed their present deadline: whose frame time exceeded the refresh
    /// period the frame was judged against, each against the period of the display the window
    /// was on when it presented. A frame that missed the deadline only in its submit is not
    /// counted, since the frame time does not hold the submit. `None` when a frame of the span
    /// had no known period.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub dropped: Option<u32>,
    /// The dropped frames whose frame time also exceeded `jank_multiplier` refresh periods:
    /// the hitches among them, each counted in `dropped` too. `None` when `dropped` is.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub janky: Option<u32>,
    /// Input-to-present latency; `None` on virtual time or with no input in the span.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub input_to_present: Option<Latency>,
    /// The worst frames, worst first.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Vec::is_empty")
    )]
    pub worst: Vec<FrameSample>,
}

/// A time limit, absolute or in refresh periods.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(rename_all = "snake_case")
)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub enum TimeLimit {
    /// Milliseconds.
    Ms(f64),
    /// Refresh periods; cannot judge without a period.
    Periods(f64),
}

/// The limits a target's runs must stay within; a limit left out is not judged.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct PerfBudget {
    /// Dropped frames at most. A janky frame is a dropped frame, so the limit holds the worst
    /// hitch of the span too.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub max_dropped: Option<u32>,
    /// The 95th-percentile frame time at most.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub p95_frame_time: Option<TimeLimit>,
    /// The 95th-percentile input-to-present latency at most.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub p95_input_to_present: Option<TimeLimit>,
}

/// One target's budget, keyed like `android:SM-G991B`.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct TargetBudget {
    /// The target.
    pub target: String,
    /// Its limits.
    pub budget: PerfBudget,
}

/// `perf.expect`: judge a target's runs against its budget. The caller ran the span
/// `reports.len()` times; the median of the per-run percentiles is judged.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct PerfExpectParams {
    /// The target the runs were on.
    pub target: String,
    /// The budgets by target; a target with no entry cannot be verified, never passes.
    pub budgets: Vec<TargetBudget>,
    /// One report per run.
    pub reports: Vec<PerfReport>,
}

/// What `perf.expect` answers.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct PerfVerdict {
    /// Pass, fail, or cannot verify.
    pub verdict: Verdict,
    /// Each limit exceeded, or each reason a limit could not be judged, for people.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Vec::is_empty")
    )]
    pub reasons: Vec<String>,
    /// The median of the runs, as judged.
    pub median: PerfReport,
}
