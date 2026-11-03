//! [`FrameTimingLayer`] — feeds the [`Profiler`] from the one [`Capture`] of the framework's
//! frame spans.
//!
//! # Why a subscriber and not a call
//!
//! `flui-devtools` is layer 9 of the workspace DAG and nothing in the framework
//! may depend on it, so frame timings cannot be pushed here through an API. The
//! framework *emits* — a `frame` span from the scheduler, and `build`,
//! `layout`, `paint`, `compositing` spans from the pipeline — and this adapter
//! subscribes. That is the only seam the layering permits, and it is why the
//! framework needed no knowledge of the profiler at all.
//!
//! # One recorder
//!
//! The layer is a [`CaptureLayer`] with the profiler as one reader of what it captures. It
//! recognizes nothing itself: [`Phase::of`] says which spans are phases, and [`crate::capture`]
//! says how a span is measured, attributed to its frame, and partitioned by the runtime and
//! presentation it ran for. [`FrameTimingLayer::over`] shares one [`Capture`] with a
//! [`Timeline`](crate::timeline::Timeline) and the trace export, so every tool shows the same
//! measurement.
//!
//! # What it listens to
//!
//! | span | opened by | phase |
//! |------|-----------|-------|
//! | `frame` | `UpdateScheduler::drive_frame` (`flui-scheduler`) | the frame itself; its `period_us` field is the display period the jank line follows, its `runtime` field the runtime that drove it |
//! | `build` | `BuildOwner::build_scope` (`flui-view`) | [`Build`](crate::profiler::FramePhase::Build) |
//! | `layout` | `PipelineOwner::run_layout` (`flui-rendering`) | [`Layout`](crate::profiler::FramePhase::Layout) |
//! | `paint` | `PipelineOwner::run_paint` (`flui-rendering`) | [`Paint`](crate::profiler::FramePhase::Paint) |
//! | `compositing`, `semantics`, `commit`, `submit` | the pipeline and the runtime | `Custom("Compositing")`, … |
//! | `animation_tick`, `animation_sampling`, `animation_listeners` | `flui-animation` | `Custom("Animation tick")`, … |
//!
//! Animation spans require the `flui.animation` target (ADR-0202).
//! `profiling` enables their optional producer through the SDK. Their durations
//! are inclusive: tick contains sampling and listener work. Do not sum them.
//! A reentrant phase inside the same phase is covered by its outer extent.
//!
//! A frame is delimited by the `frame` span `UpdateScheduler::drive_frame`
//! opens — the one driver every runner and `HeadlessBinding::pump_frame`
//! share — so a profiled frame's extent is exactly the scheduler's own
//! "in a frame" state. Phase spans outside any frame are ignored rather than
//! folded into a neighbouring frame: a headless bootstrap or a one-off layout
//! pass is not a frame, and attributing its cost to one would be a fabricated
//! measurement.
//!
//! The names are strings on both sides, and strings drift. The unit tests in
//! this module pin this side only; `tests/frame_profile_end_to_end.rs` drives
//! a real tree through `drive_frame` and reads the profile back, which is the
//! only evidence that the framework still emits what this layer expects. The
//! first version of this layer waited for a `render_frame_entered` span that
//! nothing ever opened, and its unit tests — which emitted that span
//! themselves — stayed green while every real profile stayed empty.
//!
//! # The layer carries its own filter
//!
//! [`FrameTimingLayer::new`] returns the layer behind a per-layer filter that
//! admits the frame/phase spans above and animation controller identities,
//! without events. Two things follow.
//! A log layer with an `INFO` filter beside it (FLUI's default) does not
//! starve the profiler: the registry records a `DEBUG` frame span for this
//! layer while the log layer never sees it. And attaching the profiler does
//! not switch on `DEBUG`/`TRACE` for the whole process: an unfiltered layer
//! declares interest in every callsite, so every event in the app would pay
//! the dispatch cost the moment profiling starts. Measuring must not change
//! what is measured.
//!
//! # One runtime at a time
//!
//! A [`Profiler`] has one stream of frames, so it follows the first runtime that reports a
//! frame and ignores frames of any other, and ignores a frame opened inside another. When two
//! `UiRuntime`s render concurrently under one shared subscriber the second runtime's frames
//! are not mixed into the first's statistics; a missing frame is honest, a blended one is a
//! fabrication. The [`Capture`] keeps both, partitioned. To profile several runtimes at once,
//! install one pair per runtime with [`FrameTimingLayer::over_runtime`], each over its own
//! capture.
//!
//! # Timing comes from the span, not the callback
//!
//! Duration is measured from a span's outermost entered visit, including reentry depth.
//! Retaining a span clone cannot postpone its measurement into another frame, and scheduler
//! frames end at their outermost exit: a retained child keeps its parent's identity alive but
//! cannot extend the frame's measurement. A completed span keeps a closed marker until final
//! destruction; entering its clone again cannot restart sampling inside listener work.

use std::sync::Arc;

use flui_sdk::foundation::UiRuntimeId;
use parking_lot::Mutex;
use tracing::Subscriber;
use tracing::span::{Attributes, Id, Record};
use tracing_subscriber::filter::Filtered;
use tracing_subscriber::layer::{Context, Layer};
use tracing_subscriber::registry::LookupSpan;

pub use crate::capture::FrameSpanFilter;
use crate::capture::{Capture, CaptureLayer};
use crate::profiler::{FramePhase, Profiler};

/// Subscriber layer that turns the framework's frame spans into
/// [frame statistics](crate::profiler::FrameStats).
///
/// Install it on any `tracing_subscriber` registry:
///
/// ```no_run
/// use std::sync::Arc;
///
/// use flui_devtools::{FrameTimingLayer, Profiler};
/// use tracing_subscriber::layer::SubscriberExt;
///
/// let profiler = Arc::new(Profiler::new());
/// let subscriber =
///     tracing_subscriber::registry().with(FrameTimingLayer::new(Arc::clone(&profiler)));
/// tracing::subscriber::set_global_default(subscriber).expect("no subscriber installed yet");
/// ```
///
/// In a FLUI app the registry is the one `flui_log::LogConfig::subscriber`
/// builds; the crate README shows that composition.
pub struct FrameTimingLayer {
    inner: CaptureLayer,
}

impl FrameTimingLayer {
    /// Wraps a profiler this layer will feed, behind the layer's own filter
    /// (the module docs say why the filter is not optional), over a capture of its own.
    #[must_use]
    pub fn new<S>(profiler: Arc<Profiler>) -> Filtered<Self, crate::capture::FrameSpanFilter, S>
    where
        S: Subscriber + for<'a> LookupSpan<'a>,
    {
        Self::over(&Capture::new(), profiler)
    }

    /// Like [`Self::new`], over a capture other readers share: the same measurement then feeds
    /// the profiler, a [`Timeline`](crate::timeline::Timeline) and the trace export.
    #[must_use]
    pub fn over<S>(
        capture: &Capture,
        profiler: Arc<Profiler>,
    ) -> Filtered<Self, crate::capture::FrameSpanFilter, S>
    where
        S: Subscriber + for<'a> LookupSpan<'a>,
    {
        Self::following(capture, profiler, None)
    }

    /// Like [`Self::over`], profiling exactly `runtime`: the frames of any other runtime, and
    /// frames that did not say which runtime drove them, are left to the capture. One pair per
    /// runtime profiles several runtimes at once.
    #[must_use]
    pub fn over_runtime<S>(
        capture: &Capture,
        profiler: Arc<Profiler>,
        runtime: UiRuntimeId,
    ) -> Filtered<Self, crate::capture::FrameSpanFilter, S>
    where
        S: Subscriber + for<'a> LookupSpan<'a>,
    {
        Self::following(capture, profiler, Some(runtime))
    }

    /// `pinned` is the runtime the profiler follows; without one, the first runtime that names
    /// itself.
    fn following<S>(
        capture: &Capture,
        profiler: Arc<Profiler>,
        pinned: Option<UiRuntimeId>,
    ) -> Filtered<Self, crate::capture::FrameSpanFilter, S>
    where
        S: Subscriber + for<'a> LookupSpan<'a>,
    {
        let followed: Mutex<Option<UiRuntimeId>> = Mutex::new(pinned);
        let accepts = move |runtime: Option<UiRuntimeId>| match runtime {
            Some(runtime) => *followed.lock().get_or_insert(runtime) == runtime,
            // A frame that did not say which runtime drove it cannot be told apart, so a
            // profiler that follows the first runtime takes it; one pinned to a runtime does not.
            None => pinned.is_none(),
        };
        let accepts = Arc::new(accepts);
        // The profiler's frame begins when the followed frame opens, so a phase the application
        // measures by hand while the frame runs belongs to that frame, and one measured between
        // frames is dropped by the next frame's start.
        let opened: Arc<Mutex<Option<u64>>> = Arc::new(Mutex::new(None));
        {
            let (accepts, opened, profiler) = (
                Arc::clone(&accepts),
                Arc::clone(&opened),
                Arc::clone(&profiler),
            );
            capture.on_frame_open(move |_, opening| {
                // One frame at a time: the profiler is reserved for the frame it began until that
                // frame closes, so an overlapping one cannot reset or ingest its measurements.
                let mut opened = opened.lock();
                if opened.is_none() && opening.is_outermost() && accepts(opening.runtime()) {
                    profiler.begin_frame();
                    *opened = Some(opening.id());
                }
            });
        }
        capture.on_frame(move |_, frame| {
            if !frame.is_outermost() {
                return;
            }
            // Held through ingestion: another frame opening meanwhile must not begin over this
            // frame's measurements.
            let mut opened = opened.lock();
            if *opened != Some(frame.id()) {
                // Not the frame the profiler was reserved for.
                return;
            }
            let _release = Release(&mut opened);
            if accepts(frame.span().partition().runtime()) {
                profiler.ingest(frame);
            } else {
                profiler.abandon_frame();
            }
        });
        let layer = Self {
            inner: CaptureLayer::new(capture.clone()),
        };
        layer.with_filter(crate::capture::filter())
    }
}

/// Releases the profiler's reservation when the admitted frame is done with it, however that goes.
struct Release<'a>(&'a mut Option<u64>);

impl Drop for Release<'_> {
    fn drop(&mut self) {
        *self.0 = None;
    }
}

impl std::fmt::Debug for FrameTimingLayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FrameTimingLayer").finish_non_exhaustive()
    }
}

impl<S> Layer<S> for FrameTimingLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        self.inner.on_new_span(attrs, id, ctx);
    }

    fn on_record(&self, id: &Id, values: &Record<'_>, ctx: Context<'_, S>) {
        self.inner.on_record(id, values, ctx);
    }

    fn on_enter(&self, id: &Id, ctx: Context<'_, S>) {
        self.inner.on_enter(id, ctx);
    }

    fn on_exit(&self, id: &Id, ctx: Context<'_, S>) {
        self.inner.on_exit(id, ctx);
    }
}

impl From<crate::phase::Phase> for FramePhase {
    fn from(phase: crate::phase::Phase) -> Self {
        match phase {
            crate::phase::Phase::Build => Self::Build,
            crate::phase::Phase::Layout => Self::Layout,
            crate::phase::Phase::Paint => Self::Paint,
            other => Self::Custom(other.label()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tracing::Dispatch;
    use tracing_subscriber::layer::SubscriberExt;

    /// Runs `body` with the layer installed against a fresh profiler.
    fn profile(body: impl FnOnce()) -> Arc<Profiler> {
        let profiler = Arc::new(Profiler::new());
        let subscriber =
            tracing_subscriber::registry().with(FrameTimingLayer::new(Arc::clone(&profiler)));
        // Disarm `tracing`'s process-global callsite-interest cache first: it is
        // computed on whichever thread reaches a callsite FIRST, so without this a
        // sibling test can have it cached as `never` and silently empty this capture.
        // See `flui_testing::log_capture`.
        flui_testing::log_capture::disarm_interest_cache();
        tracing::dispatcher::with_default(&Dispatch::new(subscriber), body);
        profiler
    }

    /// The jank line follows the display period the `frame` span reports in
    /// `period_us`: the same 12 ms frame is jank on a 120 Hz display and smooth
    /// against a one-second period, and a frame that reports none is judged at
    /// 60 Hz.
    ///
    /// The frame is slept rather than faked because the profiler measures its
    /// own wall clock; a sleep can only run long, which never turns a frame that
    /// must be jank into a smooth one, and the smooth case has a line far above.
    #[test]
    fn profiler_jank_line_follows_the_refresh_period() {
        let frame_that_takes = |period_us: Option<u64>, takes: std::time::Duration| {
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
        let twelve = std::time::Duration::from_millis(12);
        assert!(
            frame_that_takes(Some(8_333), twelve),
            "12 ms on a 120 Hz display is jank"
        );
        assert!(
            !frame_that_takes(Some(1_000_000), twelve),
            "12 ms against a one-second period is not jank"
        );
        assert!(
            frame_that_takes(None, std::time::Duration::from_millis(40)),
            "a frame that reports no period is judged at 60 Hz"
        );
    }

    #[test]
    fn a_phase_measured_by_hand_during_a_frame_stays_in_its_stats() {
        let profiler = Arc::new(Profiler::new());
        let subscriber =
            tracing_subscriber::registry().with(FrameTimingLayer::new(Arc::clone(&profiler)));
        flui_testing::log_capture::disarm_interest_cache();
        tracing::dispatcher::with_default(&Dispatch::new(subscriber), || {
            let _frame = tracing::debug_span!(target: "flui.frame", "frame").entered();
            drop(profiler.profile_phase(crate::profiler::FramePhase::Custom("app work")));
            let _paint = tracing::debug_span!(target: "flui.frame", "paint").entered();
        });

        let stats = profiler.frame_stats().expect("a frame was recorded");
        assert!(
            stats
                .phase(crate::profiler::FramePhase::Custom("app work"))
                .is_some(),
            "the hand-measured phase is kept: {stats:?}"
        );
        assert!(stats.phase(crate::profiler::FramePhase::Paint).is_some());
    }

    /// This side of the wiring contract: the names this layer maps to phases.
    /// It can only prove the layer's half, because the spans are emitted here
    /// by the test itself; `tests/frame_profile_end_to_end.rs` proves the
    /// framework still emits them.
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

        let phases: Vec<_> = stats.phases.iter().map(|info| info.phase).collect();
        assert!(
            phases.contains(&FramePhase::Build),
            "build span must land as a Build phase; saw {phases:?}",
        );
        assert!(
            phases.contains(&FramePhase::Layout),
            "layout span must land as a Layout phase; saw {phases:?}",
        );
        assert!(
            phases.contains(&FramePhase::Paint),
            "paint span must land as a Paint phase; saw {phases:?}",
        );
        assert!(
            phases.contains(&FramePhase::Custom("Compositing")),
            "compositing span must land as a Compositing phase; saw {phases:?}",
        );
        for name in ["Semantics", "Commit", "Submit"] {
            assert!(
                phases.contains(&FramePhase::Custom(name)),
                "the {name} span must land as a phase; saw {phases:?}",
            );
        }
    }

    /// A phase is a `(target, name)` pair: an application span that merely shares a name with
    /// A profiler has one stream of frames: it follows the first runtime that names itself and
    /// leaves another's frames to the capture, which keeps both.
    ///
    /// Fails when the profiler takes every frame, which blends two runtimes' statistics.
    #[test]
    fn a_profiler_follows_one_runtime_and_the_capture_keeps_both() {
        use std::num::NonZeroU32;

        let frame_of = |runtime: u64| {
            let _frame =
                tracing::debug_span!(target: "flui.frame", "frame", runtime = runtime).entered();
            let _paint = tracing::debug_span!(target: "flui.frame", "paint").entered();
        };
        let first = UiRuntimeId::new_gen(0, NonZeroU32::MIN).as_u64();
        let second = UiRuntimeId::new_gen(1, NonZeroU32::MIN).as_u64();
        let capture = Capture::new();
        let profiler = Arc::new(Profiler::new());
        let subscriber = tracing_subscriber::registry()
            .with(FrameTimingLayer::over(&capture, Arc::clone(&profiler)));
        flui_testing::log_capture::disarm_interest_cache();
        tracing::dispatcher::with_default(&Dispatch::new(subscriber), || {
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

    /// Two pairs on one registry each profile the runtime they were given, not the first one seen.
    #[test]
    fn a_profiler_over_a_named_runtime_ignores_the_others() {
        use std::num::NonZeroU32;

        let frame_of = |runtime: u64| {
            let _frame =
                tracing::debug_span!(target: "flui.frame", "frame", runtime = runtime).entered();
            let _paint = tracing::debug_span!(target: "flui.frame", "paint").entered();
        };
        let first = UiRuntimeId::new_gen(0, NonZeroU32::MIN);
        let second = UiRuntimeId::new_gen(1, NonZeroU32::MIN);
        let (profile_first, profile_second) =
            (Arc::new(Profiler::new()), Arc::new(Profiler::new()));
        let subscriber = tracing_subscriber::registry()
            .with(FrameTimingLayer::over_runtime(
                &Capture::new(),
                Arc::clone(&profile_first),
                first,
            ))
            .with(FrameTimingLayer::over_runtime(
                &Capture::new(),
                Arc::clone(&profile_second),
                second,
            ));
        flui_testing::log_capture::disarm_interest_cache();
        tracing::dispatcher::with_default(&Dispatch::new(subscriber), || {
            frame_of(first.as_u64());
            frame_of(second.as_u64());
            frame_of(second.as_u64());
        });

        assert_eq!(profile_first.frame_history().len(), 1);
        assert_eq!(profile_second.frame_history().len(), 2);
    }

    /// A phase measured between frames is no frame's work.
    #[test]
    fn a_hand_measured_phase_outside_the_frame_is_not_charged_to_it() {
        let profiler = Arc::new(Profiler::new());
        let subscriber =
            tracing_subscriber::registry().with(FrameTimingLayer::new(Arc::clone(&profiler)));
        flui_testing::log_capture::disarm_interest_cache();
        tracing::dispatcher::with_default(&Dispatch::new(subscriber), || {
            drop(profiler.profile_phase(crate::profiler::FramePhase::Custom("between frames")));
            let _frame = tracing::debug_span!(target: "flui.frame", "frame").entered();
            let _paint = tracing::debug_span!(target: "flui.frame", "paint").entered();
        });

        let stats = profiler.frame_stats().expect("a frame was recorded");
        assert!(
            stats
                .phase(crate::profiler::FramePhase::Custom("between frames"))
                .is_none(),
            "work done before the frame began is not the frame's: {stats:?}"
        );
        assert!(stats.phase(crate::profiler::FramePhase::Paint).is_some());
    }

    /// A measurement taken between frames must not displace or merge with the ones taken inside
    /// the frame: the frame's statistics carry every one of its own.
    #[test]
    fn many_hand_measured_phases_in_a_frame_all_reach_its_statistics() {
        let profiler = Arc::new(Profiler::new());
        let subscriber =
            tracing_subscriber::registry().with(FrameTimingLayer::new(Arc::clone(&profiler)));
        flui_testing::log_capture::disarm_interest_cache();
        tracing::dispatcher::with_default(&Dispatch::new(subscriber), || {
            drop(profiler.profile_phase(crate::profiler::FramePhase::Custom("between frames")));
            let _frame = tracing::debug_span!(target: "flui.frame", "frame").entered();
            for _ in 0..1000 {
                drop(profiler.profile_phase(crate::profiler::FramePhase::Custom("inside")));
            }
        });

        let stats = profiler.frame_stats().expect("a frame was recorded");
        assert!(
            stats
                .phase(crate::profiler::FramePhase::Custom("between frames"))
                .is_none()
        );
        assert!(
            stats
                .phase(crate::profiler::FramePhase::Custom("inside"))
                .is_some(),
            "{stats:?}"
        );
    }

    /// An excluded runtime's frame that finishes while the followed runtime's is still running
    /// must not abandon the followed frame's measurements.
    #[test]
    fn another_runtimes_frame_closing_does_not_abandon_the_followed_frame() {
        use std::num::NonZeroU32;
        let profiler = Arc::new(Profiler::new());
        let capture = Capture::new();
        let followed = UiRuntimeId::new_gen(0, NonZeroU32::MIN);
        let subscriber = tracing_subscriber::registry().with(FrameTimingLayer::over_runtime(
            &capture,
            Arc::clone(&profiler),
            followed,
        ));
        flui_testing::log_capture::disarm_interest_cache();
        let dispatch = Dispatch::new(subscriber);
        tracing::dispatcher::with_default(&dispatch, || {
            let _frame = tracing::debug_span!(
                target: "flui.frame",
                "frame",
                runtime = followed.as_u64()
            )
            .entered();
            drop(profiler.profile_phase(crate::profiler::FramePhase::Custom("followed work")));
            std::thread::scope(|scope| {
                scope.spawn(|| {
                    tracing::dispatcher::with_default(&dispatch, || {
                        drop(
                            tracing::debug_span!(
                                target: "flui.frame",
                                "frame",
                                runtime = UiRuntimeId::new_gen(1, NonZeroU32::MIN).as_u64()
                            )
                            .entered(),
                        );
                    });
                });
            });
        });

        let stats = profiler
            .frame_stats()
            .expect("the followed frame was recorded");
        assert!(
            stats
                .phase(crate::profiler::FramePhase::Custom("followed work"))
                .is_some(),
            "{stats:?}"
        );
    }

    /// Two frames that name no runtime and overlap on different threads: the second must not reset
    /// the first's measurements, nor take the profiler when it closes first.
    #[test]
    fn an_overlapping_unnamed_frame_does_not_disturb_the_admitted_one() {
        let profiler = Arc::new(Profiler::new());
        let subscriber =
            tracing_subscriber::registry().with(FrameTimingLayer::new(Arc::clone(&profiler)));
        flui_testing::log_capture::disarm_interest_cache();
        let dispatch = Dispatch::new(subscriber);
        tracing::dispatcher::with_default(&dispatch, || {
            let _frame = tracing::debug_span!(target: "flui.frame", "frame").entered();
            drop(profiler.profile_phase(crate::profiler::FramePhase::Custom("admitted work")));
            std::thread::scope(|scope| {
                scope.spawn(|| {
                    tracing::dispatcher::with_default(&dispatch, || {
                        drop(tracing::debug_span!(target: "flui.frame", "frame").entered());
                    });
                });
            });
        });

        let stats = profiler
            .frame_stats()
            .expect("the admitted frame was recorded");
        assert!(
            stats
                .phase(crate::profiler::FramePhase::Custom("admitted work"))
                .is_some(),
            "{stats:?}"
        );
    }

    /// A phase measured by hand part-way through a frame keeps its place in it.
    #[test]
    fn a_hand_measured_phase_keeps_its_offset_in_the_frame() {
        let profiler = Arc::new(Profiler::new());
        let subscriber =
            tracing_subscriber::registry().with(FrameTimingLayer::new(Arc::clone(&profiler)));
        flui_testing::log_capture::disarm_interest_cache();
        tracing::dispatcher::with_default(&Dispatch::new(subscriber), || {
            let _frame = tracing::debug_span!(target: "flui.frame", "frame").entered();
            std::thread::sleep(std::time::Duration::from_millis(30));
            drop(profiler.profile_phase(crate::profiler::FramePhase::Custom("late work")));
        });

        let stats = profiler.frame_stats().expect("a frame was recorded");
        let late = stats
            .phase(crate::profiler::FramePhase::Custom("late work"))
            .expect("the hand-measured phase is kept");
        assert!(
            late.start_offset >= std::time::Duration::from_millis(25),
            "placed after the 30 ms wait, not at the frame start: {late:?}"
        );
    }

    /// An application span that merely shares a phase's name
    /// is not counted.
    ///
    /// Fails when `phase_for` goes back to matching the name alone.
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
        let phases: Vec<_> = stats.phases.iter().map(|info| info.phase).collect();
        assert_eq!(
            phases,
            [FramePhase::Build],
            "only the framework's span counts"
        );
    }
}
