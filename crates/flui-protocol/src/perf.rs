//! Frame performance over a span (ADR-0238 §10): what `perf.stop` reports and the budget
//! `perf.expect` judges it against.
//!
//! Times are milliseconds. Dropped and janky frames need the presentation's refresh period;
//! when it is unknown they are `None`, and a budget that names them cannot judge.

use crate::tree::WindowId;
use crate::verdict::Verdict;

/// `perf.start`: begin keeping frame samples for a window.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct PerfStartParams {
    /// The window.
    pub window: WindowId,
    /// Work over this many refresh periods makes a frame janky; 2 when absent.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub jank_multiplier: Option<f64>,
    /// How many of the worst frames to report; 5 when absent.
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
    /// Its frame time, in milliseconds.
    pub ms: f64,
    /// Its phases.
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

/// What `perf.stop` answers for the span since `perf.start`.
#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct PerfReport {
    /// Frames presented.
    pub frames: u64,
    /// Frame times.
    pub frame_time: Distribution,
    /// The presentation's refresh period, in milliseconds, when known.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub refresh_period_ms: Option<f64>,
    /// Frames that missed their present deadline; `None` without a refresh period.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub dropped: Option<u32>,
    /// Frames whose work exceeded the jank threshold, counted apart from dropped ones; `None`
    /// without a refresh period.
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
    /// Dropped frames at most.
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
