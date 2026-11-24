//! [`Profiler`]: per-frame statistics of the framework's own frames, read from a [`Capture`].
//!
//! The profiler records no spans: [`Capture::layer`] measures them, and [`Profiler::over`]
//! turns each frame the capture closes into [`FrameStats`]. [`Profiler`]'s own docs say which
//! frames it follows.

use std::collections::VecDeque;
use std::fmt;
use std::sync::{Arc, Weak};

use flui_sdk::foundation::UiRuntimeId;
use parking_lot::Mutex;
use web_time::Duration;

use crate::capture::{Capture, CaptureSubscription, CapturedFrame};
use crate::phase::Phase;

/// The period a frame that reports none is judged against: one 60 Hz frame.
const DEFAULT_FRAME_PERIOD: Duration = Duration::from_nanos(16_666_667);

/// Where the profiler draws the line between a smooth frame and jank.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum JankLine {
    /// A frame longer than this is jank, on every display.
    Fixed(Duration),
    /// A frame longer than the period of the display it was produced for is
    /// jank: 8.3 ms on a 120 Hz display, 16.7 ms on a 60 Hz one. The period
    /// arrives with the frame (the scheduler's `frame` span carries it as
    /// `period_us`); a frame that reports none is judged against 60 Hz. When one
    /// frame serves several displays the period is the shortest among them.
    FramePeriod,
}

/// What the profiler treats as jank, which runtime it follows, and how much history it keeps.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProfilerConfig {
    /// Where a frame turns from smooth into jank.
    pub jank_line: JankLine,
    /// Frames kept in [`Profiler::frame_history`]; older ones are dropped. At zero the profiler
    /// keeps no frames but still counts them and their jank.
    pub max_frame_history: usize,
    /// The UI runtime whose frames are profiled. `Some` profiles exactly that runtime, and a
    /// frame that does not say which runtime drove it is not taken. `None` follows the first
    /// runtime whose frame names itself and takes the frames that name no runtime.
    pub runtime: Option<UiRuntimeId>,
}

impl Default for ProfilerConfig {
    /// The display's own frame period as the jank line, five seconds of history
    /// at 60 Hz, following the first runtime seen.
    fn default() -> Self {
        Self {
            jank_line: JankLine::FramePeriod,
            max_frame_history: 300,
            runtime: None,
        }
    }
}

/// The time one [`Phase`] took in one frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct PhaseInfo {
    phase: Phase,
    duration: Duration,
    start_offset: Duration,
}

impl PhaseInfo {
    /// The phase measured.
    #[must_use]
    pub const fn phase(&self) -> Phase {
        self.phase
    }

    /// How long the phase ran in the frame, summed over every segment it ran in.
    #[must_use]
    pub const fn duration(&self) -> Duration {
        self.duration
    }

    /// When the phase first began, from the start of its frame.
    #[must_use]
    pub const fn start_offset(&self) -> Duration {
        self.start_offset
    }

    /// [`Self::duration`] in milliseconds.
    #[must_use]
    pub fn duration_ms(&self) -> f64 {
        self.duration.as_secs_f64() * 1000.0
    }

    /// [`Self::start_offset`] in milliseconds.
    #[must_use]
    pub fn start_offset_ms(&self) -> f64 {
        self.start_offset.as_secs_f64() * 1000.0
    }
}

/// One profiled frame.
///
/// Its [`Display`](fmt::Display) is a short report: `frame #N: 12.34 ms (jank)`, then one
/// indented line per phase with its label and milliseconds.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct FrameStats {
    number: u64,
    start: Duration,
    duration: Duration,
    phases: Vec<PhaseInfo>,
    is_jank: bool,
}

impl FrameStats {
    /// The frame's place among the frames this profiler recorded, counted from 0.
    #[must_use]
    pub const fn number(&self) -> u64 {
        self.number
    }

    /// When the frame began, from the capture's [epoch](Capture::epoch).
    #[must_use]
    pub const fn start(&self) -> Duration {
        self.start
    }

    /// How long the frame took, from its outermost entry to its outermost exit.
    #[must_use]
    pub const fn duration(&self) -> Duration {
        self.duration
    }

    /// [`Self::duration`] in milliseconds.
    #[must_use]
    pub fn duration_ms(&self) -> f64 {
        self.duration.as_secs_f64() * 1000.0
    }

    /// The phases the frame ran, in the order they first began.
    ///
    /// Each phase appears at most once per frame: a phase that ran in several
    /// segments (an initial build plus layout-builder rebuilds, say) is
    /// recorded as one entry holding the summed duration and the first
    /// segment's start offset.
    #[must_use]
    pub fn phases(&self) -> &[PhaseInfo] {
        &self.phases
    }

    /// The time `phase` took in this frame, if it ran.
    #[must_use]
    pub fn phase(&self, phase: Phase) -> Option<&PhaseInfo> {
        self.phases.iter().find(|info| info.phase == phase)
    }

    /// The time `phase` took in this frame in milliseconds, if it ran.
    #[must_use]
    pub fn phase_duration_ms(&self, phase: Phase) -> Option<f64> {
        self.phase(phase).map(PhaseInfo::duration_ms)
    }

    /// Whether the frame outlasted the profiler's [`JankLine`].
    #[must_use]
    pub const fn is_jank(&self) -> bool {
        self.is_jank
    }
}

impl fmt::Display for FrameStats {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "frame #{}: {:.2} ms", self.number, self.duration_ms())?;
        if self.is_jank {
            f.write_str(" (jank)")?;
        }
        for info in &self.phases {
            write!(
                f,
                "\n  {}: {:.2} ms",
                info.phase.label(),
                info.duration_ms()
            )?;
        }
        Ok(())
    }
}

/// What a profiler has recorded, behind its handle's lock.
struct ProfilerInner {
    config: ProfilerConfig,
    /// The runtime an unpinned profiler follows, once one of its frames named it.
    followed: Option<UiRuntimeId>,
    history: VecDeque<FrameStats>,
    /// Every frame recorded, retained or not.
    total_frames: u64,
    /// Every recorded frame that was jank.
    jank_frames: u64,
}

impl ProfilerInner {
    fn new(config: ProfilerConfig) -> Self {
        Self {
            followed: config.runtime,
            history: VecDeque::with_capacity(config.max_frame_history),
            config,
            total_frames: 0,
            jank_frames: 0,
        }
    }

    /// Whether a frame driven by `runtime` is one this profiler records.
    fn follows(&mut self, runtime: Option<UiRuntimeId>) -> bool {
        match (self.config.runtime, runtime) {
            (Some(pinned), runtime) => runtime == Some(pinned),
            // A frame that did not say which runtime drove it cannot be told apart, so a
            // profiler that follows the first runtime takes it.
            (None, None) => true,
            (None, Some(runtime)) => *self.followed.get_or_insert(runtime) == runtime,
        }
    }

    /// Record `frame` if it is an outermost frame of the runtime this profiler follows.
    fn ingest(&mut self, frame: &CapturedFrame) {
        if !frame.is_outermost() || !self.follows(frame.span().partition().runtime()) {
            return;
        }
        let frame_start = frame.span().start();
        let mut spans: Vec<_> = frame
            .spans()
            .iter()
            .filter_map(|span| span.phase().map(|phase| (phase, span)))
            .collect();
        spans.sort_by_key(|(_, span)| span.start());
        let mut phases: Vec<PhaseInfo> = Vec::new();
        for (phase, span) in spans {
            // A phase can run more than once per frame (a layout-builder rebuild is still
            // Build work); its segments merge so `FrameStats::phase` sees all of them.
            if let Some(recorded) = phases.iter_mut().find(|info| info.phase == phase) {
                recorded.duration = recorded.duration.saturating_add(span.duration());
            } else {
                phases.push(PhaseInfo {
                    phase,
                    duration: span.duration(),
                    start_offset: span.start().saturating_sub(frame_start),
                });
            }
        }
        let duration = frame.span().duration();
        let line = match self.config.jank_line {
            JankLine::Fixed(line) => line,
            JankLine::FramePeriod => frame.period().unwrap_or(DEFAULT_FRAME_PERIOD),
        };
        let is_jank = duration > line;
        let stats = FrameStats {
            number: self.total_frames,
            start: frame_start,
            duration,
            phases,
            is_jank,
        };
        self.total_frames = self.total_frames.saturating_add(1);
        if is_jank {
            self.jank_frames = self.jank_frames.saturating_add(1);
        }
        if self.config.max_frame_history > 0 {
            if self.history.len() >= self.config.max_frame_history {
                self.history.pop_front();
            }
            self.history.push_back(stats);
        }
    }

    fn frame_rate(&self) -> Option<f64> {
        if self.history.len() < 2 {
            return None;
        }
        let starts = self.history.iter().map(FrameStats::start);
        let first = starts.clone().min()?;
        let last = starts.max()?;
        let span = last.checked_sub(first).filter(|span| !span.is_zero())?;
        Some((self.history.len() - 1) as f64 / span.as_secs_f64())
    }

    fn jank_percentage(&self) -> f64 {
        if self.total_frames == 0 {
            return 0.0;
        }
        (self.jank_frames as f64 / self.total_frames as f64) * 100.0
    }
}

/// A profiler's statistics and the registration of the sink that records them.
struct ProfilerShared {
    state: Mutex<ProfilerInner>,
    /// Removes the sink from the capture when the last [`Profiler`] handle drops. The sink holds
    /// this state weakly, so the two make no cycle.
    _sink: CaptureSubscription,
}

/// Frame statistics of one UI runtime, read from a [`Capture`].
///
/// The profiler records no spans itself. [`Capture::layer`] measures the frame span the
/// scheduler opens and the [`Phase`] spans inside it, and a profiler made with
/// [`Profiler::over`] turns every frame that capture closes into [`FrameStats`]: the frame's
/// start and duration, the time each phase took, and whether the frame was jank. The same
/// capture can feed a [`Timeline`](crate::Timeline) and the trace export, so every tool shows
/// one measurement.
///
/// The profiler measures framework phases only. Work an application wants to see beside them
/// is recorded on a timeline with [`Timeline::record_event`](crate::Timeline::record_event),
/// where it shares the capture's time base.
///
/// A handle is cheap to clone, and every clone reads the same statistics. The capture holds
/// the profiler only weakly, and the profiler holds its sink's [`CaptureSubscription`]: once
/// the last handle is dropped, its sink leaves the capture and its frames are no longer
/// recorded.
///
/// # One runtime per profiler
///
/// A profiler has one stream of frames, so it follows one UI runtime: the one
/// [`ProfilerConfig::runtime`] names, or else the first runtime whose frame names itself. The
/// frames of any other runtime are left to the capture, which keeps them in their own
/// [`Partition`](crate::Partition); a missing frame is honest, a blended one is a fabrication.
/// A frame opened inside another frame (a nested runtime pumped from the outer one's callback)
/// is not followed either: its work is already inside the outer frame's extent. To profile
/// several runtimes at once, make one profiler per runtime over the same capture.
///
/// # Example
///
/// The framework opens the frame and phase spans; this example opens two itself.
///
/// ```rust
/// use flui_devtools::{Capture, Phase, Profiler, ProfilerConfig};
/// use tracing_subscriber::layer::SubscriberExt;
///
/// let capture = Capture::new();
/// let profiler = Profiler::over(&capture, ProfilerConfig::default());
/// let subscriber = tracing_subscriber::registry().with(capture.layer());
/// tracing::subscriber::with_default(subscriber, || {
///     let _frame = tracing::debug_span!(target: "flui.frame", "frame").entered();
///     let _layout = tracing::debug_span!(target: "flui.frame", "layout").entered();
/// });
///
/// let frame = profiler.frame_stats().expect("the frame closed");
/// assert!(frame.phase(Phase::Layout).is_some());
/// println!("{frame}");
/// ```
#[derive(Clone)]
pub struct Profiler {
    shared: Arc<ProfilerShared>,
}

impl Profiler {
    /// A profiler of the frames `capture` closes, from now on, until its last handle drops.
    ///
    /// It records the outermost frames of the runtime [`ProfilerConfig::runtime`] selects, as
    /// [`Profiler`] describes.
    #[must_use]
    pub fn over(capture: &Capture, config: ProfilerConfig) -> Self {
        let shared = Arc::new_cyclic(|reader: &Weak<ProfilerShared>| {
            let reader = Weak::clone(reader);
            let sink = capture.on_frame(move |_, frame| {
                if let Some(shared) = reader.upgrade() {
                    shared.state.lock().ingest(frame);
                }
            });
            ProfilerShared {
                state: Mutex::new(ProfilerInner::new(config)),
                _sink: sink,
            }
        });
        Self { shared }
    }

    /// The most recent frame, unless no frame is retained.
    #[must_use]
    pub fn frame_stats(&self) -> Option<FrameStats> {
        self.shared.state.lock().history.back().cloned()
    }

    /// The retained frames, oldest first: at most [`ProfilerConfig::max_frame_history`].
    #[must_use]
    pub fn frame_history(&self) -> Vec<FrameStats> {
        self.shared.state.lock().history.iter().cloned().collect()
    }

    /// Frames per second over the retained history: one less than the number of retained
    /// frames, over the time from the first one's start to the last one's.
    ///
    /// It counts the frames the scheduler produced, not how fast the frame work ran: two frames
    /// of 2 ms each, one display period apart, are 60 frames per second. An idle application
    /// produces no frames, so its rate is not zero; it stays whatever the cadence of its last
    /// frames was. `None` while fewer than two frames are retained, or when they all started at
    /// once.
    #[must_use]
    pub fn frame_rate(&self) -> Option<f64> {
        self.shared.state.lock().frame_rate()
    }

    /// The percentage of recorded frames that were jank, retained or not.
    #[must_use]
    pub fn jank_percentage(&self) -> f64 {
        self.shared.state.lock().jank_percentage()
    }
}

impl fmt::Debug for Profiler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Profiler")
            .field("frame_count", &self.shared.state.lock().total_frames)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use flui_sdk::foundation::generation::Generation;
    use tracing::Dispatch;
    use tracing_subscriber::layer::SubscriberExt;

    use super::*;

    fn runtime(index: u32) -> UiRuntimeId {
        UiRuntimeId::new_gen(index, Generation::FIRST)
    }

    /// Runs `body` with `capture`'s layer installed on this thread.
    fn capturing(capture: &Capture, body: impl FnOnce()) {
        let subscriber = tracing_subscriber::registry().with(capture.layer());
        // Disarm `tracing`'s process-global callsite-interest cache first: it is
        // computed on whichever thread reaches a callsite FIRST, so without this a
        // sibling test can have it cached as `never` and silently empty this capture.
        // See `flui_testing::log_capture`.
        flui_testing::log_capture::disarm_interest_cache();
        tracing::dispatcher::with_default(&Dispatch::new(subscriber), body);
    }

    /// Runs `body` under a fresh capture and returns the default profiler over it.
    fn profile(body: impl FnOnce()) -> Profiler {
        let capture = Capture::new();
        let profiler = Profiler::over(&capture, ProfilerConfig::default());
        capturing(&capture, body);
        profiler
    }

    fn frame_of(runtime: u64) {
        let _frame =
            tracing::debug_span!(target: "flui.frame", "frame", runtime = runtime).entered();
        let _paint = tracing::debug_span!(target: "flui.frame", "paint").entered();
    }

    /// The jank line follows the display period the `frame` span reports in
    /// `period_us`: the same 12 ms frame is jank on a 120 Hz display and smooth
    /// against a one-second period, and a frame that reports none is judged at
    /// 60 Hz.
    ///
    /// The frame is slept because the capture measures the wall clock; a sleep
    /// can only run long, which never turns a frame that must be jank into a
    /// smooth one, and the smooth case has a line far above.
    #[test]
    fn profiler_jank_line_follows_the_refresh_period() {
        let frame_that_takes = |period_us: Option<u64>, takes: Duration| {
            let profiler = profile(|| {
                let frame = tracing::debug_span!(
                    target: "flui.frame",
                    "frame",
                    period_us = tracing::field::Empty
                );
                let _entered = frame.enter();
                if let Some(period_us) = period_us {
                    frame.record("period_us", period_us);
                }
                std::thread::sleep(takes);
            });
            profiler
                .frame_stats()
                .expect("the frame span closed, so a frame was recorded")
                .is_jank()
        };
        let twelve = Duration::from_millis(12);
        assert!(
            frame_that_takes(Some(8_333), twelve),
            "12 ms on a 120 Hz display is jank"
        );
        assert!(
            !frame_that_takes(Some(1_000_000), twelve),
            "12 ms against a one-second period is not jank"
        );
        assert!(
            frame_that_takes(None, Duration::from_millis(40)),
            "a frame that reports no period is judged at 60 Hz"
        );
    }

    /// This side of the wiring contract: every span of the phase table becomes the profiler
    /// phase it names. It can only prove the profiler's half, because the spans are emitted here
    /// by the test itself; `tests/frame_profile_end_to_end.rs` proves the framework still emits
    /// them.
    #[test]
    fn the_frameworks_phase_spans_become_profiler_phases() {
        let profiler = profile(|| {
            let _frame = tracing::debug_span!(target: "flui.frame", "frame").entered();
            {
                let _build =
                    tracing::debug_span!(target: "flui.frame", "build", dirty_elements = 3)
                        .entered();
            }
            {
                let _layout =
                    tracing::debug_span!(target: "flui.frame", "layout", dirty_nodes = 1).entered();
            }
            {
                let _paint = tracing::debug_span!(target: "flui.frame", "paint").entered();
            }
            {
                let _compositing =
                    tracing::debug_span!(target: "flui.frame", "compositing").entered();
            }
            {
                let _semantics = tracing::debug_span!(target: "flui.frame", "semantics").entered();
            }
            {
                let _commit = tracing::debug_span!(target: "flui.frame", "commit").entered();
            }
            {
                let _submit = tracing::debug_span!(target: "flui.frame", "submit").entered();
            }
        });

        let stats = profiler
            .frame_stats()
            .expect("the frame span closed, so a frame was recorded");
        let phases: Vec<Phase> = stats.phases().iter().map(PhaseInfo::phase).collect();
        assert_eq!(
            phases,
            [
                Phase::Build,
                Phase::Layout,
                Phase::Paint,
                Phase::Compositing,
                Phase::Semantics,
                Phase::Commit,
                Phase::Submit,
            ],
            "each span lands as its own phase, in the order they began"
        );
    }

    /// A phase that ran in several segments of one frame is one entry: the durations summed,
    /// the first segment's offset kept.
    #[test]
    fn a_phase_that_runs_twice_in_a_frame_is_one_entry() {
        let profiler = profile(|| {
            let _frame = tracing::debug_span!(target: "flui.frame", "frame").entered();
            for _ in 0..2 {
                let _build = tracing::debug_span!(target: "flui.frame", "build").entered();
                std::thread::sleep(Duration::from_millis(5));
            }
        });

        let stats = profiler.frame_stats().expect("a frame was recorded");
        assert_eq!(stats.phases().len(), 1, "{stats:?}");
        let build = stats.phase(Phase::Build).expect("the build is recorded");
        assert!(
            build.duration() >= Duration::from_millis(10),
            "both segments are summed: {build:?}"
        );
        assert!(
            build.start_offset() < Duration::from_millis(5),
            "the first segment's offset is kept: {build:?}"
        );
    }

    /// A profiler has one stream of frames: it follows the first runtime that names itself and
    /// leaves another's frames to the capture, which keeps both.
    ///
    /// Fails when the profiler takes every frame, which blends two runtimes' statistics.
    #[test]
    fn a_profiler_follows_one_runtime_and_the_capture_keeps_both() {
        let (first, second) = (runtime(0).as_u64(), runtime(1).as_u64());
        let capture = Capture::new();
        let profiler = Profiler::over(&capture, ProfilerConfig::default());
        capturing(&capture, || {
            frame_of(first);
            frame_of(second);
            frame_of(first);
        });

        assert_eq!(
            profiler.frame_history().len(),
            2,
            "the second runtime's frame is left out"
        );
        assert_eq!(
            capture.frames().len(),
            3,
            "the capture keeps every runtime's frames"
        );
    }

    /// Two profilers over one capture each profile the runtime their config names, not the
    /// first one seen.
    #[test]
    fn a_profiler_over_a_named_runtime_ignores_the_others() {
        let (first, second) = (runtime(0), runtime(1));
        let capture = Capture::new();
        let pinned = |runtime| ProfilerConfig {
            runtime: Some(runtime),
            ..ProfilerConfig::default()
        };
        let profile_first = Profiler::over(&capture, pinned(first));
        let profile_second = Profiler::over(&capture, pinned(second));
        capturing(&capture, || {
            {
                // Names no runtime: a pinned profiler cannot tell whose it is.
                let _frame = tracing::debug_span!(target: "flui.frame", "frame").entered();
            }
            frame_of(first.as_u64());
            frame_of(second.as_u64());
            frame_of(second.as_u64());
        });

        assert_eq!(profile_first.frame_history().len(), 1);
        assert_eq!(profile_second.frame_history().len(), 2);
    }

    /// Another runtime's frame that closes while the followed runtime's is still running
    /// disturbs neither: the followed frame keeps its own phases, and the other is not taken.
    #[test]
    fn another_runtimes_frame_closing_does_not_disturb_the_followed_frame() {
        let capture = Capture::new();
        let followed = runtime(0);
        let profiler = Profiler::over(
            &capture,
            ProfilerConfig {
                runtime: Some(followed),
                ..ProfilerConfig::default()
            },
        );
        let dispatch = Dispatch::new(tracing_subscriber::registry().with(capture.layer()));
        flui_testing::log_capture::disarm_interest_cache();
        tracing::dispatcher::with_default(&dispatch, || {
            let _frame = tracing::debug_span!(
                target: "flui.frame",
                "frame",
                runtime = followed.as_u64()
            )
            .entered();
            drop(tracing::debug_span!(target: "flui.frame", "paint").entered());
            std::thread::scope(|scope| {
                scope.spawn(|| {
                    tracing::dispatcher::with_default(&dispatch, || {
                        frame_of(runtime(1).as_u64());
                    });
                });
            });
            drop(tracing::debug_span!(target: "flui.frame", "layout").entered());
        });

        let history = profiler.frame_history();
        assert_eq!(history.len(), 1, "only the followed frame: {history:?}");
        let phases: Vec<Phase> = history[0].phases().iter().map(PhaseInfo::phase).collect();
        assert_eq!(phases, [Phase::Paint, Phase::Layout]);
    }

    /// Two frames that name no runtime and overlap on different threads are two frames, and
    /// neither's phases are charged to the other.
    #[test]
    fn overlapping_unnamed_frames_keep_their_own_phases() {
        let capture = Capture::new();
        let profiler = Profiler::over(&capture, ProfilerConfig::default());
        let dispatch = Dispatch::new(tracing_subscriber::registry().with(capture.layer()));
        flui_testing::log_capture::disarm_interest_cache();
        tracing::dispatcher::with_default(&dispatch, || {
            let _frame = tracing::debug_span!(target: "flui.frame", "frame").entered();
            drop(tracing::debug_span!(target: "flui.frame", "paint").entered());
            std::thread::scope(|scope| {
                scope.spawn(|| {
                    tracing::dispatcher::with_default(&dispatch, || {
                        drop(tracing::debug_span!(target: "flui.frame", "frame").entered());
                    });
                });
            });
        });

        let history = profiler.frame_history();
        assert_eq!(history.len(), 2, "{history:?}");
        let phases: Vec<Vec<Phase>> = history
            .iter()
            .map(|frame| frame.phases().iter().map(PhaseInfo::phase).collect())
            .collect();
        assert_eq!(
            phases,
            [vec![], vec![Phase::Paint]],
            "the frame that closed first ran no phase"
        );
    }

    /// A phase that began part-way through a frame keeps its place in it.
    #[test]
    fn a_phase_keeps_its_offset_in_the_frame() {
        let profiler = profile(|| {
            let _frame = tracing::debug_span!(target: "flui.frame", "frame").entered();
            std::thread::sleep(Duration::from_millis(30));
            drop(tracing::debug_span!(target: "flui.frame", "paint").entered());
        });

        let stats = profiler.frame_stats().expect("a frame was recorded");
        let paint = stats.phase(Phase::Paint).expect("the paint is recorded");
        assert!(
            paint.start_offset() >= Duration::from_millis(25),
            "placed after the 30 ms wait, not at the frame start: {paint:?}"
        );
    }

    /// An application span that merely shares a phase's name is not counted.
    ///
    /// Fails when a phase is recognized by its name alone.
    #[test]
    fn an_application_span_named_like_a_phase_is_not_a_phase() {
        let profiler = profile(|| {
            let _frame = tracing::debug_span!(target: "flui.frame", "frame").entered();
            {
                let _app = tracing::debug_span!(target: "my_app", "paint").entered();
            }
            {
                let _app = tracing::debug_span!(target: "my_app::render", "layout").entered();
            }
            {
                let _own = tracing::debug_span!(target: "flui.frame", "build").entered();
            }
        });

        let stats = profiler
            .frame_stats()
            .expect("the frame span closed, so a frame was recorded");
        let phases: Vec<Phase> = stats.phases().iter().map(PhaseInfo::phase).collect();
        assert_eq!(phases, [Phase::Build], "only the framework's span counts");
    }

    /// A frame opened inside another frame is captured and not profiled.
    #[test]
    fn a_nested_frame_is_not_profiled() {
        let capture = Capture::new();
        let profiler = Profiler::over(&capture, ProfilerConfig::default());
        capturing(&capture, || {
            let _outer = tracing::debug_span!(target: "flui.frame", "frame").entered();
            frame_of(runtime(0).as_u64());
        });

        assert_eq!(capture.frames().len(), 2);
        let history = profiler.frame_history();
        assert_eq!(history.len(), 1, "{history:?}");
        assert!(
            history[0].phases().is_empty(),
            "the nested frame's paint is its own: {history:?}"
        );
    }

    /// A profiler's sink leaves the capture with the profiler's last handle, so making and
    /// dropping profilers over one capture leaves no sink behind.
    ///
    /// Fails when the sink stays registered, a dead closure that runs on every frame.
    #[test]
    fn dropping_the_last_profiler_removes_its_sink() {
        let capture = Capture::new();
        let profiler = Profiler::over(&capture, ProfilerConfig::default());
        let clone = profiler.clone();
        assert_eq!(capture.sink_count(), 1);

        drop(profiler);
        assert_eq!(capture.sink_count(), 1, "a clone still records");
        drop(clone);
        assert_eq!(capture.sink_count(), 0, "the last handle took its sink");

        for _ in 0..3 {
            drop(Profiler::over(&capture, ProfilerConfig::default()));
        }
        assert_eq!(capture.sink_count(), 0);
    }

    /// The summary is one line for the frame and one per phase.
    #[test]
    fn a_frames_display_is_its_number_duration_jank_and_phases() {
        let frame = FrameStats {
            number: 7,
            start: Duration::ZERO,
            duration: Duration::from_micros(12_340),
            phases: vec![PhaseInfo {
                phase: Phase::AnimationTick,
                duration: Duration::from_micros(1_500),
                start_offset: Duration::ZERO,
            }],
            is_jank: true,
        };

        assert_eq!(
            frame.to_string(),
            "frame #7: 12.34 ms (jank)\n  Animation tick: 1.50 ms"
        );
    }
}
