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
//! # One frame at a time, by span identity
//!
//! The layer owns at most one frame at a time, keyed by the frame span's `Id`.
//! Phase spans count only when they are *descendants* of that owning span, and
//! only its own close ends the frame. So when two `UiRealm`s render
//! concurrently under one shared subscriber, the second realm's frame — and
//! every phase inside it — is **dropped whole** rather than mixed into the
//! first realm's measurements: a missing frame is honest, a blended one is a
//! fabrication. To profile several realms at once, install one
//! `FrameTimingLayer` + [`Profiler`] pair per realm.
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
    /// The frame span currently being measured, if any.
    ///
    /// Shared across clones (`Arc`) so a cloned layer still sees the same
    /// single-frame ownership. `Some` from the owning span's creation until
    /// its close; a frame span created while this is `Some` belongs to a
    /// concurrently rendering realm and is ignored entirely.
    active_frame: Arc<parking_lot::Mutex<Option<Id>>>,
}

impl FrameTimingLayer {
    /// Wraps a profiler this layer will feed.
    #[must_use]
    pub fn new(profiler: Arc<Profiler>) -> Self {
        Self {
            profiler,
            active_frame: Arc::new(parking_lot::Mutex::new(None)),
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
        if attrs.metadata().name() != FRAME_SPAN {
            return;
        }
        // Frame boundaries open here rather than on enter: a frame span is
        // entered once, and opening on creation keeps `begin_frame` ahead of
        // any phase span the same frame may create.
        let mut active_frame = self.active_frame.lock();
        if active_frame.is_some() {
            // A second realm's frame under the same subscriber. Dropping it
            // whole is honest; letting its `begin_frame` reset the running
            // frame would blend two realms' timings into one measurement.
            return;
        }
        *active_frame = Some(id.clone());
        self.profiler.begin_frame();
    }

    fn on_enter(&self, id: &Id, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(id) else { return };
        let Some(phase) = phase_for(span.name()) else {
            return;
        };

        // Only phases inside the owning frame span count. A phase from a
        // concurrently rendering realm — or outside any frame — must not be
        // attributed to the frame being measured.
        let owning_frame = self.active_frame.lock().clone();
        let Some(frame_id) = owning_frame else { return };
        if !span.scope().any(|ancestor| ancestor.id() == frame_id) {
            return;
        }

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
            // Only the owning span's close ends the frame; the lock is held
            // across `end_frame` so a frame beginning on another thread
            // cannot slot in between the release and the recording.
            let mut active_frame = self.active_frame.lock();
            if active_frame.as_ref() == Some(&id) {
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

    /// A second realm's frame under the same subscriber is dropped whole —
    /// its `begin_frame` must not reset the running frame, its phases must
    /// not be attributed to it, and its close must not end it. `parent: None`
    /// makes the second frame a structural sibling, exactly what a
    /// concurrently rendering realm's span looks like to a shared layer.
    #[test]
    fn a_concurrent_frame_is_dropped_rather_than_blended() {
        let profiler = profile(|| {
            let own_frame = tracing::debug_span!("render_frame_entered");
            let _own = own_frame.enter();
            {
                // The other realm's whole frame, opened mid-way through ours.
                let other_frame = tracing::debug_span!(parent: None, "render_frame_entered");
                let _other = other_frame.enter();
                let _other_layout = tracing::debug_span!("layout").entered();
            } // ...and closed while ours is still running.
            let _own_build = tracing::debug_span!("build").entered();
        });

        assert_eq!(
            profiler.frame_history().len(),
            1,
            "only the owning frame records; the concurrent one is dropped",
        );
        let stats = profiler.frame_stats().expect("the owning frame closed");
        let phases: Vec<_> = stats.phases.iter().map(|info| info.phase).collect();
        assert_eq!(
            phases,
            vec![FramePhase::Build],
            "the other realm's layout must not leak into this frame",
        );
    }

    /// Build work split across several spans in one frame (the global build
    /// plus a layout-builder rebuild) reports as ONE Build entry, because
    /// `FrameStats::phase` returns the first match and separate entries
    /// would hide everything after it.
    #[test]
    fn repeated_build_spans_report_one_build_phase() {
        let profiler = profile(|| {
            let _frame = tracing::debug_span!("render_frame_entered").entered();
            {
                let _build = tracing::debug_span!("build", dirty_elements = 2).entered();
            }
            {
                let _layout = tracing::debug_span!("layout").entered();
                let _rebuild = tracing::debug_span!("build", during_layout = true).entered();
            }
        });

        let stats = profiler.frame_stats().expect("a frame was recorded");
        let build_entries = stats
            .phases
            .iter()
            .filter(|info| info.phase == FramePhase::Build)
            .count();
        assert_eq!(
            build_entries, 1,
            "both build spans merge into one Build total; saw {:?}",
            stats.phases,
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
