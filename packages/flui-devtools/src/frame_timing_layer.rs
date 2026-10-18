//! [`FrameTimingLayer`] — feeds the [`Profiler`] from
//! the framework's own tracing spans.
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
//! # What it listens to
//!
//! | span | opened by | phase |
//! |------|-----------|-------|
//! | `frame` | `UpdateScheduler::drive_frame` (`flui-scheduler`) | the frame itself; its `period_us` field is the display period the jank line follows |
//! | `build` | `BuildOwner::build_scope` (`flui-view`) | [`Build`](crate::profiler::FramePhase::Build) |
//! | `layout` | `PipelineOwner::run_layout` (`flui-rendering`) | [`Layout`](crate::profiler::FramePhase::Layout) |
//! | `paint` | `PipelineOwner::run_paint` (`flui-rendering`) | [`Paint`](crate::profiler::FramePhase::Paint) |
//! | `compositing` | the compositing pass (`flui-rendering`) | `Custom("Compositing")` |
//! | `animation_tick` | the public Vsync registry walk (`flui-animation`) | `Custom("Animation tick")` |
//! | `animation_sampling` | time, simulation or repeat computation (`flui-animation`) | `Custom("Animation sampling")` |
//! | `animation_listeners` | value/status listeners and run delivery (`flui-animation`) | `Custom("Animation listeners")` |
//!
//! Animation spans require the `flui_animation` target (ADR-0202).
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
//! # One frame at a time, by span identity
//!
//! The layer owns at most one frame at a time, keyed by the frame span's `Id`.
//! Phase spans count only when their nearest frame is that owning span, and
//! only its outermost exit ends the frame. Scheduler frames are recognized by
//! target as well as name, so an application's `frame` span cannot hide them.
//! So when two `UiRuntime`s render
//! concurrently under one shared subscriber, the second UI runtime's frame — and
//! every phase inside it — is **dropped whole** rather than mixed into the
//! first UI runtime's measurements: a missing frame is honest, a blended one is a
//! fabrication. To profile several UI runtimes at once, install one
//! `FrameTimingLayer` + [`Profiler`] pair per UI runtime.
//!
//! # Timing comes from the span, not the callback
//!
//! Duration is measured by a [phase guard](crate::profiler::PhaseGuard) parked
//! in the span's own extensions. Phases record their outermost
//! entered producer visit at exit, including reentry depth. Retaining a span clone
//! cannot postpone its measurement into another frame. Scheduler frames also
//! end at their outermost exit: a retained child keeps its parent's identity
//! alive, but cannot extend the frame's measurement.
//! A completed phase span keeps a closed marker until final destruction;
//! entering its clone again cannot restart sampling inside listener work.

use std::sync::Arc;

use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::{Metadata, Subscriber};
use tracing_subscriber::filter::{FilterFn, Filtered, filter_fn};
use tracing_subscriber::layer::{Context, Layer};
use tracing_subscriber::registry::LookupSpan;

use crate::profiler::{FramePhase, PhaseGuard, Profiler};

/// The span `UpdateScheduler::drive_frame` opens around every frame.
const FRAME_SPAN: &str = "frame";

/// The scheduler's frame boundary, not an application's same-named span.
fn is_scheduler_frame(metadata: &Metadata<'_>) -> bool {
    metadata.is_span()
        && metadata.name() == FRAME_SPAN
        && metadata.target() == "flui_scheduler::scheduler::execution"
}

fn is_animation_phase(metadata: &Metadata<'_>) -> bool {
    metadata.target() == "flui_animation"
        && matches!(
            metadata.name(),
            "animation_tick" | "animation_sampling" | "animation_listeners"
        )
}

/// Maps a span name to the phase it measures, or `None` if it is not a phase.
fn phase_for(name: &str) -> Option<FramePhase> {
    match name {
        "build" => Some(FramePhase::Build),
        "layout" => Some(FramePhase::Layout),
        "paint" => Some(FramePhase::Paint),
        "compositing" => Some(FramePhase::Custom("Compositing")),
        "animation_tick" => Some(FramePhase::Custom("Animation tick")),
        "animation_sampling" => Some(FramePhase::Custom("Animation sampling")),
        "animation_listeners" => Some(FramePhase::Custom("Animation listeners")),
        _ => None,
    }
}

/// Frame/phase spans and controller identities; no events or unrelated spans.
fn is_frame_or_phase(metadata: &Metadata<'_>) -> bool {
    if matches!(
        metadata.name(),
        "animation_tick" | "animation_sampling" | "animation_listeners"
    ) {
        return metadata.is_span() && is_animation_phase(metadata);
    }
    metadata.is_span()
        && (is_scheduler_frame(metadata)
            || phase_for(metadata.name()).is_some()
            || (metadata.target() == "flui_animation::diagnostics"
                && metadata.name() == "animation_controller"))
}

/// The filter [`FrameTimingLayer::new`] wraps the layer in.
pub type FrameSpanFilter = FilterFn<fn(&Metadata<'_>) -> bool>;

/// Parked in a phase span's extensions for the span's lifetime.
///
/// A newtype rather than a bare `PhaseGuard` so this layer's entry cannot
/// collide with another layer storing the same type in the same extensions map.
// The field is never read on purpose: the guard records the phase duration when
// it is *dropped*, which is the whole mechanism.
struct ActivePhase {
    _guard: PhaseGuard,
    entries: usize,
}

/// A producer owns one entered visit; retained clones cannot restart it.
struct ClosedPhase;

struct ActiveFrame {
    id: Id,
    entries: usize,
}

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
#[derive(Clone)]
pub struct FrameTimingLayer {
    profiler: Arc<Profiler>,
    /// The frame span currently being measured, if any.
    ///
    /// Shared across clones (`Arc`) so a cloned layer still sees the same
    /// single-frame ownership. `Some` from the owning span's creation until
    /// its outermost exit; a frame span created while this is `Some` belongs to a
    /// concurrently rendering UI runtime and is ignored entirely.
    active_frame: Arc<parking_lot::Mutex<Option<ActiveFrame>>>,
}

impl FrameTimingLayer {
    /// Wraps a profiler this layer will feed, behind the layer's own filter
    /// (the module docs say why the filter is not optional).
    #[must_use]
    pub fn new<S>(profiler: Arc<Profiler>) -> Filtered<Self, FrameSpanFilter, S>
    where
        S: Subscriber + for<'a> LookupSpan<'a>,
    {
        let layer = Self {
            profiler,
            active_frame: Arc::new(parking_lot::Mutex::new(None)),
        };
        layer.with_filter(filter_fn(is_frame_or_phase as fn(&Metadata<'_>) -> bool))
    }
}

impl FrameTimingLayer {
    /// Forward the `period_us` field of the owning frame span, if `record`
    /// carries one, to the profiler: the period of the display the frame is
    /// produced for, which the jank line follows.
    fn report_period(&self, record: impl FnOnce(&mut dyn Visit)) {
        struct PeriodVisitor(Option<u64>);
        impl Visit for PeriodVisitor {
            fn record_u64(&mut self, field: &Field, value: u64) {
                if field.name() == "period_us" {
                    self.0 = Some(value);
                }
            }
            fn record_debug(&mut self, _: &Field, _: &dyn std::fmt::Debug) {}
        }
        let mut visitor = PeriodVisitor(None);
        record(&mut visitor);
        if let Some(micros) = visitor.0 {
            self.profiler
                .set_frame_period(std::time::Duration::from_micros(micros));
        }
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
    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, _ctx: Context<'_, S>) {
        if !is_scheduler_frame(attrs.metadata()) {
            return;
        }
        // Frame boundaries open here rather than on enter: a frame span is
        // entered once, and opening on creation keeps `begin_frame` ahead of
        // any phase span the same frame may create.
        let mut active_frame = self.active_frame.lock();
        if active_frame.is_some() {
            // A second ui_runtime's frame under the same subscriber. Dropping it
            // whole is honest; letting its `begin_frame` reset the running
            // frame would blend two ui_runtimes' timings into one measurement.
            return;
        }
        *active_frame = Some(ActiveFrame {
            id: id.clone(),
            entries: 0,
        });
        self.profiler.begin_frame();
        self.report_period(|visitor| attrs.record(visitor));
    }

    fn on_record(&self, id: &Id, values: &Record<'_>, _ctx: Context<'_, S>) {
        let owns_frame = self
            .active_frame
            .lock()
            .as_ref()
            .is_some_and(|active| active.id == *id);
        if owns_frame {
            self.report_period(|visitor| values.record(visitor));
        }
    }

    fn on_enter(&self, id: &Id, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(id) else { return };
        if is_scheduler_frame(span.metadata()) {
            let mut active_frame = self.active_frame.lock();
            if let Some(active) = active_frame.as_mut()
                && active.id == *id
            {
                active.entries += 1;
            }
            return;
        }
        let Some(phase) = phase_for(span.name()) else {
            return;
        };
        if span.extensions().get::<ClosedPhase>().is_some() {
            return;
        }

        // Only phases inside the owning frame span count. A phase from a
        // concurrently rendering ui_runtime — or outside any frame — must not be
        // attributed to the frame being measured.
        let owning_frame = self
            .active_frame
            .lock()
            .as_ref()
            .map(|active| active.id.clone());
        let Some(frame_id) = owning_frame else { return };
        if !span
            .scope()
            .find(|ancestor| is_scheduler_frame(ancestor.metadata()))
            .is_some_and(|owner| owner.id() == frame_id)
        {
            return;
        }

        // A reentrant walk/sample is already covered by the same phase's
        // enclosing extent. Adding both guards would double-count its work.
        if span.scope().skip(1).any(|ancestor| {
            phase_for(ancestor.name()) == Some(phase)
                && ancestor.extensions().get::<ActivePhase>().is_some()
        }) {
            return;
        }

        let mut extensions = span.extensions_mut();
        if let Some(active) = extensions.get_mut::<ActivePhase>() {
            active.entries += 1;
            // Re-entered without closing: the guard already running measures
            // the full extent, so a second one would double-count.
            return;
        }
        extensions.insert(ActivePhase {
            _guard: self.profiler.profile_phase(phase),
            entries: 1,
        });
    }

    fn on_exit(&self, id: &Id, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(id) else { return };
        if is_scheduler_frame(span.metadata()) {
            let mut active_frame = self.active_frame.lock();
            if let Some(active) = active_frame.as_mut()
                && active.id == *id
            {
                active.entries -= 1;
                if active.entries == 0 {
                    self.profiler.end_frame();
                    *active_frame = None;
                }
            }
            return;
        }
        if phase_for(span.name()).is_none() {
            return;
        }
        // User sources can retain or reenter Span::current(). The outermost
        // exit ends this visit; external clones must not extend its CPU time.
        let finished = {
            let mut extensions = span.extensions_mut();
            let Some(active) = extensions.get_mut::<ActivePhase>() else {
                // Covered or unowned visits must not become new work later.
                let _ = extensions.replace(ClosedPhase);
                return;
            };
            active.entries -= 1;
            if active.entries == 0 {
                let finished = extensions.remove::<ActivePhase>();
                let _ = extensions.replace(ClosedPhase);
                finished
            } else {
                None
            }
        };
        drop(finished);
    }

    fn on_close(&self, id: Id, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(&id) else { return };
        let name = span.name();

        if phase_for(name).is_some() {
            // Dropping the guard is what records the duration.
            drop(span.extensions_mut().remove::<ActivePhase>());
            return;
        }

        if is_scheduler_frame(span.metadata()) {
            // Fallback for a frame closed without entry. Normal frames end
            // at exit even if a retained child keeps this span alive.
            let mut active_frame = self.active_frame.lock();
            if active_frame.as_ref().is_some_and(|active| active.id == id) {
                self.profiler.end_frame();
                *active_frame = None;
            }
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
                    target: "flui_scheduler::scheduler::execution",
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

    /// This side of the wiring contract: the names this layer maps to phases.
    /// It can only prove the layer's half, because the spans are emitted here
    /// by the test itself; `tests/frame_profile_end_to_end.rs` proves the
    /// framework still emits them.
    #[test]
    fn the_frameworks_phase_spans_become_profiler_phases() {
        let profiler = profile(|| {
            let _frame =
                tracing::debug_span!(target: "flui_scheduler::scheduler::execution", "frame")
                    .entered();
            {
                let _build = tracing::debug_span!("build", dirty_elements = 3).entered();
            }
            {
                let _layout = tracing::debug_span!("layout", dirty_nodes = 1).entered();
            }
            {
                let _paint = tracing::debug_span!("paint").entered();
            }
            {
                let _compositing = tracing::debug_span!("compositing").entered();
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
    }
}
