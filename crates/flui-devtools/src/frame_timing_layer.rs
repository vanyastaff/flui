//! [`FrameTimingLayer`] — feeds the [`Profiler`](crate::profiler::Profiler) from
//! the framework's own tracing spans.
//!
//! # Why a subscriber and not a call
//!
//! `flui-devtools` is layer 9 of the workspace DAG and nothing in the framework
//! may depend on it, so frame timings cannot be pushed here through an API. The
//! framework *emits* — `build`, `layout`, `paint` and `compositing` spans from
//! the pipeline — and this adapter subscribes. That is the only seam the
//! layering permits, and it is why the framework needed no knowledge of the
//! profiler at all.
//!
//! # What it listens to
//!
//! | span | phase |
//! |------|-------|
//! | `build` | [`FramePhase::Build`] |
//! | `layout` | [`FramePhase::Layout`] |
//! | `paint` | [`FramePhase::Paint`] |
//! | `compositing` | `FramePhase::Custom("Compositing")` |
//!
//! A frame is delimited by the `render_frame_entered` span that `UiRealm` opens
//! per frame. Phase spans outside any frame are ignored rather than folded into
//! a neighbouring frame — a headless test or a one-off layout pass is not a
//! frame, and attributing its cost to one would be a fabricated measurement.
//!
//! # Timing comes from the span, not the callback
//!
//! Duration is measured by a [`PhaseGuard`](crate::profiler::PhaseGuard) parked
//! in the span's own extensions: created when the span is *entered*, dropped
//! when it closes. That measures the span's real extent even when a subscriber
//! callback runs late, and it means a span entered and exited repeatedly (as
//! `layout` is, across a fixpoint) still reports one contiguous phase rather
//! than a sum of visits.

use std::sync::Arc;

use tracing::Subscriber;
use tracing::span::{Attributes, Id};
use tracing_subscriber::layer::{Context, Layer};
use tracing_subscriber::registry::LookupSpan;

use crate::profiler::{FramePhase, PhaseGuard, Profiler};

/// The span `UiRealm` opens once per frame.
const FRAME_SPAN: &str = "render_frame_entered";

/// Maps a span name to the phase it measures, or `None` if it is not a phase.
fn phase_for(name: &str) -> Option<FramePhase> {
    match name {
        "build" => Some(FramePhase::Build),
        "layout" => Some(FramePhase::Layout),
        "paint" => Some(FramePhase::Paint),
        "compositing" => Some(FramePhase::Custom("Compositing")),
        _ => None,
    }
}

/// Parked in a phase span's extensions for the span's lifetime.
///
/// A newtype rather than a bare `PhaseGuard` so this layer's entry cannot
/// collide with another layer storing the same type in the same extensions map.
// The field is never read on purpose: the guard records the phase duration when
// it is *dropped*, which is the whole mechanism.
#[allow(dead_code)]
struct ActivePhase(PhaseGuard);

/// Subscriber layer that turns the framework's frame spans into
/// [`FrameStats`](crate::profiler::FrameStats).
///
/// Install it on any `tracing_subscriber` registry:
///
/// ```no_run
/// # use std::sync::Arc;
/// # use flui_devtools::profiler::Profiler;
/// # use flui_devtools::frame_timing_layer::FrameTimingLayer;
/// use tracing_subscriber::layer::SubscriberExt;
///
/// let profiler = Arc::new(Profiler::new());
/// let subscriber =
///     tracing_subscriber::registry().with(FrameTimingLayer::new(Arc::clone(&profiler)));
/// tracing::subscriber::set_global_default(subscriber).expect("no subscriber installed yet");
/// ```
///
/// The framework emits its frame spans at `DEBUG`, so a filter above that level
/// silently starves this layer. That is a filter misconfiguration rather than a
/// bug here, but it looks identical to “profiling is broken”, so it is worth
/// checking first.
#[derive(Clone)]
pub struct FrameTimingLayer {
    profiler: Arc<Profiler>,
}

impl FrameTimingLayer {
    /// Wraps a profiler this layer will feed.
    #[must_use]
    pub fn new(profiler: Arc<Profiler>) -> Self {
        Self { profiler }
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
    fn on_new_span(&self, attrs: &Attributes<'_>, _id: &Id, _ctx: Context<'_, S>) {
        if attrs.metadata().name() != FRAME_SPAN {
            return;
        }
        // Frame boundaries open here rather than on enter: a frame span is
        // entered once, and opening on creation keeps `begin_frame` ahead of
        // any phase span the same frame may create.
        self.profiler.begin_frame();
    }

    fn on_enter(&self, id: &Id, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(id) else { return };
        let Some(phase) = phase_for(span.name()) else {
            return;
        };

        let mut extensions = span.extensions_mut();
        if extensions.get_mut::<ActivePhase>().is_some() {
            // Re-entered without closing: the guard already running measures
            // the full extent, so a second one would double-count.
            return;
        }
        extensions.insert(ActivePhase(self.profiler.profile_phase(phase)));
    }

    fn on_close(&self, id: Id, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(&id) else { return };
        let name = span.name();

        if phase_for(name).is_some() {
            // Dropping the guard is what records the duration.
            drop(span.extensions_mut().remove::<ActivePhase>());
            return;
        }

        if name == FRAME_SPAN {
            self.profiler.end_frame();
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
        tracing::dispatcher::with_default(&Dispatch::new(subscriber), body);
        profiler
    }

    /// **The wiring contract.** The framework's own span names must reach the
    /// profiler as phases. If a span is renamed on either side this fails,
    /// which is the point — the two halves are coupled only by these strings.
    #[test]
    fn the_frameworks_phase_spans_become_profiler_phases() {
        let profiler = profile(|| {
            let _frame = tracing::debug_span!("render_frame_entered").entered();
            {
                let _build = tracing::debug_span!("build", dirty_elements = 3).entered();
            }
            {
                let _layout = tracing::debug_span!("layout", dirty_nodes = 1).entered();
            }
            {
                let _paint = tracing::debug_span!("paint").entered();
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
    }

    /// A span the framework does not emit as a phase must not become one.
    /// Without this, any unrelated span in the process would be timed as frame
    /// work and the profile would be quietly wrong rather than empty.
    #[test]
    fn unrelated_spans_are_not_phases() {
        let profiler = profile(|| {
            let _frame = tracing::debug_span!("render_frame_entered").entered();
            let _other = tracing::debug_span!("some_unrelated_work").entered();
        });

        let stats = profiler.frame_stats().expect("a frame was recorded");
        assert!(
            stats.phases.is_empty(),
            "only the framework's four phase spans count; saw {:?}",
            stats.phases,
        );
    }

    /// Phase work outside any frame is dropped rather than folded into a
    /// neighbouring frame. A headless layout pass is not a frame, and
    /// attributing its cost to one would be a fabricated measurement.
    #[test]
    fn a_phase_outside_a_frame_records_no_frame() {
        let profiler = profile(|| {
            let _layout = tracing::debug_span!("layout").entered();
        });

        assert!(
            profiler.frame_stats().is_none(),
            "no frame span opened, so there is no frame to attribute work to",
        );
    }

    /// Two frames stay separate. A layer that failed to close a frame would
    /// report one enormous frame and every jank threshold would misfire.
    #[test]
    fn consecutive_frames_are_recorded_separately() {
        let profiler = profile(|| {
            for _ in 0..2 {
                let _frame = tracing::debug_span!("render_frame_entered").entered();
                let _build = tracing::debug_span!("build").entered();
            }
        });

        assert_eq!(
            profiler.frame_history().len(),
            2,
            "each frame span must close its own frame",
        );
    }
}
